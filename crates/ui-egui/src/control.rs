//! The control port (issue 41): a window started with `--control PORT`, or whose user opened it in File > Settings > Agents, listens
//! on its own machine's address, and an agent that `bimcraft-cli mcp --connect` works on the project the window has open, as the
//! user does, rather than on a headless copy of its own. The protocol, what an agent may ask and may not, and the file that has the
//! port and the token are `bimcraft_core::bridge`; this is the threads that listen, the frame that answers, what Settings keeps of
//! it ([`Agents`]: whether the window opens it, on which port, and whether agents may edit or only read), and the state the view
//! control bar shows (off, listening, how many agents, read-only).
//!
//! The requests come from outside, so they are hostile: only this machine's address is bound, a connection that does not say the
//! token in its first line is dropped, a line is read to a limit, only a few connections are held, and a request that
//! is no request is an error reply. A request runs on the window's own thread, in the frame, as a command from a button does
//! (`BimApp::answer_agents`), so what the agent makes is the user's to see, to undo and to save; while the user lets agents only
//! read, the engine runs it read-only (`bridge::Access::Read`). A port dropped is closed: it stops listening and hangs up on its
//! agents, so it may be opened again.

use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bimcraft_core::bridge::{self, Access, Entry, Error, MAX_LINE, Request};
use bimcraft_core::ids::Guid;
use bimcraft_i18n::msg;
use egui::{Context, RichText, Ui, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::style::pal;

/// The most agents connected at once.
const MAX_CONNECTIONS: usize = 8;
/// How long a connection has, from the start, to say its first line whole, with the token in it.
const FIRST_LINE: Duration = Duration::from_secs(10);
/// How long a request waits for the window to answer: a render or an import of a big model takes a while.
const ANSWER: Duration = Duration::from_secs(600);
/// How many requests a frame answers.
const PER_FRAME: usize = 16;

/// File > Settings > Agents: the control port as the user keeps it (in `settings.json`), and whether agents may edit.
#[derive(Clone, Debug)]
pub(crate) struct Agents {
    /// The window opens the port when it starts, and opened it when the box was ticked.
    pub(crate) listen: bool,
    /// The port it opens on: 0 for any that is free.
    pub(crate) port: u16,
    /// Agents may change the project, as the user does; else they only read it, and what they ask that would change it is refused.
    pub(crate) edit: bool,
    /// Where the window writes the bridge file when it opens the port itself: beside the favorites (`main` says), none in a test.
    file: Option<PathBuf>,
    /// Why the port could not be opened, the last time it was asked to: said in Settings.
    pub(crate) failed: Option<String>,
}

impl Default for Agents {
    fn default() -> Self {
        Agents { listen: false, port: 0, edit: true, file: None, failed: None }
    }
}

/// A request on its way to the window, and where its reply goes. An agent that has given up waiting says so, and the window does not
/// do a request nobody is waiting for any more.
pub(crate) struct Asked {
    pub(crate) request: Request,
    pub(crate) reply: Sender<String>,
    pub(crate) abandoned: Arc<AtomicBool>,
}

/// What the threads of a port share with it: whether it is closing, and the agents connected.
#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    /// The connections that have said a request with the token, by a number of their own: hung up on when the port closes.
    agents: Mutex<Vec<(u64, TcpStream)>>,
    next: AtomicU64,
}

impl Shared {
    /// An agent that has said its first request, and the number to forget it by.
    fn arrived(&self, stream: &TcpStream) -> io::Result<u64> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let held = stream.try_clone()?;
        if let Ok(mut agents) = self.agents.lock() {
            agents.push((id, held));
        }
        Ok(id)
    }

    fn left(&self, id: u64) {
        if let Ok(mut agents) = self.agents.lock() {
            agents.retain(|(n, _)| *n != id);
        }
    }

    fn connected(&self) -> usize {
        self.agents.lock().map_or(0, |a| a.len())
    }
}

/// The listening port and what it has taken.
pub struct Control {
    /// Where agents find it: the port, and the token that lets them in.
    pub entry: Entry,
    asked: Receiver<Asked>,
    /// The window's context, once the first frame has set it, so that a request wakes the window.
    wake: Arc<OnceLock<Context>>,
    /// The bridge file written for it, taken away when the window goes.
    file: Option<PathBuf>,
    shared: Arc<Shared>,
    /// Whether a thread listens: a test's detached control has none.
    listening: bool,
}

impl Control {
    /// Listens on `port` of this machine's own address (0: any that is free) and writes the bridge `file` with the port and a token
    /// of 128 random bits, for the user alone.
    pub fn start(port: u16, file: Option<PathBuf>) -> io::Result<Control> {
        Control::start_within(port, file, FIRST_LINE)
    }

    /// [`Control::start`] with `first_line` as the time a connection has to say its first.
    pub(crate) fn start_within(port: u16, file: Option<PathBuf>, first_line: Duration) -> io::Result<Control> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let entry = Entry { port: listener.local_addr()?.port(), token: Guid::new().to_string() };
        if let Some(file) = &file {
            bridge::write_entry(file, &entry)?;
        }
        let (tx, asked) = channel();
        let wake = Arc::new(OnceLock::new());
        let shared = Arc::new(Shared::default());
        let (token, woken, live, held) = (entry.token.clone(), Arc::clone(&wake), Arc::new(AtomicUsize::new(0)), Arc::clone(&shared));
        std::thread::Builder::new().name("control-accept".into()).spawn(move || {
            for stream in listener.incoming() {
                // The port is closing: the listener goes with this thread.
                if held.stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else {
                    // A failing accept (no file descriptors left, say) is waited out, not spun on.
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                };
                // A ninth agent is turned away, not queued: it was dropped, which closes it.
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (token, tx, woken, live, held) = (token.clone(), tx.clone(), Arc::clone(&woken), Arc::clone(&live), Arc::clone(&held));
                let spawned = std::thread::Builder::new().name("control-agent".into()).spawn(move || {
                    let mut id = None;
                    let _ = serve(stream, &token, &tx, &woken, first_line, &held, &mut id);
                    if let Some(id) = id {
                        held.left(id);
                        // The bar counts the agents.
                        if let Some(ctx) = woken.get() {
                            ctx.request_repaint();
                        }
                    }
                    live.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    break;
                }
            }
        })?;
        Ok(Control { entry, asked, wake, file, shared, listening: true })
    }

    /// How many agents are connected: those that have said a request with the token and not hung up.
    pub fn agents(&self) -> usize {
        self.shared.connected()
    }
}

#[cfg(test)]
impl Control {
    /// A control with no port: the requests are put in by the test.
    pub(crate) fn detached() -> (Control, Sender<Asked>) {
        let (tx, asked) = channel();
        let entry = Entry { port: 0, token: String::new() };
        let shared = Arc::new(Shared::default());
        (Control { entry, asked, wake: Arc::new(OnceLock::new()), file: None, shared, listening: false }, tx)
    }
}

impl Drop for Control {
    /// Closes the port: the agents are hung up on, and the thread that listens is woken by a connection of its own to see that it
    /// is to stop, which lets the port go.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Ok(agents) = self.shared.agents.lock() {
            for (_, stream) in agents.iter() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        if self.listening {
            let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, self.entry.port)), Duration::from_millis(500));
        }
        if let Some(file) = &self.file {
            bridge::remove_entry(file, &self.entry);
        }
    }
}

/// One line of at most [`MAX_LINE`] bytes, or none at the end of what the agent sends; a longer one is an error.
pub(crate) fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = String::new();
    let read = reader.by_ref().take(MAX_LINE as u64 + 1).read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if read > MAX_LINE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "a request line is too long"));
    }
    Ok(Some(line))
}

/// The first line of a connection: whole within `within` of the start, not a byte now and then, and no longer than [`MAX_LINE`].
/// None where the connection ends before it.
fn read_first_line(reader: &mut io::BufReader<TcpStream>, within: Duration) -> io::Result<Option<String>> {
    let start = Instant::now();
    let mut line = Vec::new();
    loop {
        let left = within
            .checked_sub(start.elapsed())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "the first line did not come in time"))?;
        reader.get_ref().set_read_timeout(Some(left))?;
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(None);
        }
        let end = buffer.iter().position(|&b| b == b'\n').map_or(buffer.len(), |at| at + 1);
        line.extend_from_slice(&buffer[..end]);
        let whole = buffer[end - 1] == b'\n';
        reader.consume(end);
        if line.len() > MAX_LINE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a request line is too long"));
        }
        if whole {
            return String::from_utf8(line).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
}

/// An agent's connection: a line in, the window's answer back, until it hangs up or says a wrong token. Until a request has come
/// with the token nothing is allowed that costs anything: the first line comes whole and soon, and what is not a request closes
/// the connection (a web page's HTTP, say), after its reply. Once one has, the agent counts as connected (`id`, in `shared`).
fn serve(
    stream: TcpStream,
    token: &str,
    tx: &Sender<Asked>,
    wake: &OnceLock<Context>,
    first_line: Duration,
    shared: &Shared,
    id: &mut Option<u64>,
) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = io::BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut known = false;
    loop {
        let line = if known { read_line(&mut reader)? } else { read_first_line(&mut reader, first_line)? };
        let Some(line) = line else { return Ok(()) };
        let reply = match bridge::parse(&line, token) {
            Err(e @ Error::Token) => {
                writeln!(writer, "{}", bridge::reply(Err(e.to_string())))?;
                return Ok(());
            }
            Err(e) if !known => {
                writeln!(writer, "{}", bridge::reply(Err(e.to_string())))?;
                return Ok(());
            }
            Err(e) => bridge::reply(Err(e.to_string())),
            Ok(request) => {
                if !std::mem::replace(&mut known, true) {
                    // Once it is known, an agent may take as long as it likes between requests.
                    reader.get_ref().set_read_timeout(None)?;
                    *id = Some(shared.arrived(&writer)?);
                }
                let (reply, answer) = channel();
                let abandoned = Arc::new(AtomicBool::new(false));
                if tx.send(Asked { request, reply, abandoned: Arc::clone(&abandoned) }).is_err() {
                    // The window has gone.
                    return Ok(());
                }
                if let Some(ctx) = wake.get() {
                    ctx.request_repaint();
                }
                answer.recv_timeout(ANSWER).unwrap_or_else(|_| {
                    abandoned.store(true, Ordering::SeqCst);
                    bridge::reply(Err("the window did not answer".into()))
                })
            }
        };
        writeln!(writer, "{reply}")?;
        writer.flush()?;
    }
}

impl BimApp {
    /// Lets agents work on this window's project, over `control`, and says so.
    pub fn use_control(&mut self, control: Control) {
        let port = control.entry.port.to_string();
        self.status = crate::Status {
            text: crate::lang::tf("An agent can work on this project: bimcraft-cli mcp --connect {}", &[&port]),
            error: false,
        };
        self.control = Some(control);
        self.agents.failed = None;
    }

    /// Where the window writes the bridge file when it opens the port itself (Settings > Agents): beside the favorites.
    pub fn use_bridge_file(&mut self, file: Option<PathBuf>) {
        self.agents.file = file;
    }

    /// Opens the port Settings keeps, when they keep it open and none is: at the start, after `--control` had its say. The error
    /// says why it could not.
    pub fn listen_as_kept(&mut self) -> Result<(), String> {
        if !self.agents.listen || self.control.is_some() {
            return Ok(());
        }
        self.open_control();
        self.agents.failed.clone().map_or(Ok(()), Err)
    }

    /// Opens the port on the one Settings says, in place of one that is open; what stops it is kept in `agents.failed`.
    pub(crate) fn open_control(&mut self) {
        let port = self.agents.port;
        match Control::start(port, self.agents.file.clone()) {
            Ok(control) => {
                // Settings says where agents come in: the status would lie over its buttons.
                (self.control, self.agents.failed) = (Some(control), None);
            }
            Err(e) => self.agents.failed = Some(tf("Couldn't listen on port {} for agents: {}", &[&port.to_string(), &e.to_string()])),
        }
    }

    /// Closes the port: no agent reaches the project, and those connected are hung up on.
    pub(crate) fn close_control(&mut self) {
        (self.control, self.agents.failed) = (None, None);
    }

    /// After the port's number changed in Settings: an open port moves to it, unless it is 0 (any port, which the open one is).
    pub(crate) fn control_port_changed(&mut self) {
        if self.control.as_ref().is_some_and(|c| self.agents.port != 0 && c.entry.port != self.agents.port) {
            self.open_control();
        }
    }

    /// What the port is doing, as the view control bar and Settings say it: off, listening on its port, or how many agents are
    /// connected, and read-only when agents may not edit.
    pub(crate) fn agents_state(&self) -> String {
        let Some(control) = &self.control else { return t("Agents: off").to_string() };
        let (n, port) = (control.agents(), control.entry.port.to_string());
        match (n, self.agents.edit) {
            (0, true) => tf("Agents: port {}", &[&port]),
            (0, false) => tf("Agents: port {}, read-only", &[&port]),
            (n, true) => tf("Agents: {} connected", &[&n.to_string()]),
            (n, false) => tf("Agents: {} connected, read-only", &[&n.to_string()]),
        }
    }

    /// The port's state on the view control bar, in both profiles: a click opens Settings at Agents.
    pub(crate) fn agents_indicator(&mut self, ui: &mut Ui) {
        let state = self.agents_state();
        let ink = match &self.control {
            Some(c) if c.agents() > 0 => pal().accent,
            Some(_) => pal().text,
            None => pal().muted,
        };
        let tip = msg!(
            "The control port, through which an agent works on this project (bimcraft-cli mcp --connect): whether it is open, how many \
             agents are connected, and whether they may only read. Click for File > Settings > Agents"
        );
        let button = egui::Button::new(RichText::new(&state).size(12.0).color(ink)).frame(false).min_size(vec2(0.0, 25.0));
        if ui.add(button).on_hover_text(t(tip)).clicked() {
            self.settings_page = Some(crate::settings::Page::Agents);
        }
    }

    /// A fresh desktop snapshot, read on the UI thread rather than cached in the bridge or the model.
    fn desktop_context(&self) -> Result<Value, String> {
        let m = &self.session.model;
        if m.uid != self.project {
            return Err("desktop context is not ready: the window is switching projects; retry the request".into());
        }
        if self.home.shown {
            return Ok(json!({"project": m.name, "selection": [], "active_view": null}));
        }
        let selection: Vec<Value> = self
            .selection
            .iter()
            .filter_map(|id| m.elements.get(id).map(|e| json!({"id": id, "guid": m.guid(*id), "category": e.category()})))
            .collect();
        let (kind, id, exists) = match self.sheet {
            crate::Sheet::Plan(id) => ("plan", Some(id), m.plan_level(id).is_some()),
            crate::Sheet::Ceiling(id) => ("ceiling", Some(id), m.plan_level(id).is_some()),
            crate::Sheet::View3D => ("3d", self.active_3d(), true),
            crate::Sheet::Section(id) => (
                if matches!(m.elements.get(id), Some(bimcraft_core::model::Element::Elevation(_))) { "elevation" } else { "section" },
                Some(id),
                m.section(id).is_some(),
            ),
            crate::Sheet::Paper(id) => ("sheet", Some(id), m.sheet(id).is_some()),
            crate::Sheet::Schedule(category) => ("schedule", bimcraft_core::schedule::standard(m, category).0, true),
            crate::Sheet::Table(id) => {
                ("schedule", Some(id), matches!(m.elements.get(id), Some(bimcraft_core::model::Element::Schedule(_))))
            }
        };
        let active_view = if exists {
            json!({"kind": kind, "name": self.view_name(self.sheet), "id": id, "guid": id.map(|id| m.guid(id))})
        } else {
            Value::Null
        };
        Ok(json!({"project": m.name, "selection": selection, "active_view": active_view}))
    }

    /// Once a frame: what agents asked of the project since the last one is done, each as a command from a button is, and
    /// answered. While a file is being read or an update installed the project is about to be replaced: they wait.
    pub(crate) fn answer_agents(&mut self, ctx: &Context) {
        let Some(control) = &self.control else { return };
        let _ = control.wake.set(ctx.clone());
        if self.importing() || self.installing() {
            return;
        }
        let asked: Vec<Asked> = std::iter::from_fn(|| control.asked.try_recv().ok()).take(PER_FRAME).collect();
        let more = asked.len() == PER_FRAME;
        for Asked { request, reply, abandoned } in asked {
            if abandoned.load(Ordering::SeqCst) {
                continue;
            }
            let result = if matches!(request, Request::Context) {
                self.desktop_context()
            } else {
                self.made_in();
                let access = if self.agents.edit { Access::Edit } else { Access::Read };
                let result = bridge::run(&mut self.session, &request, access);
                // The core renames and drops the option with its set.
                self.active_option = self.session.option.clone();
                result
            };
            let _ = reply.send(bridge::reply(result));
            ctx.request_repaint();
        }
        if more {
            ctx.request_repaint();
        }
    }
}
