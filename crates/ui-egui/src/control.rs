//! The control port (issue 41): a window started with `--control PORT` listens on its own machine's address, and an agent that
//! `bimcraft-cli mcp --connect` works on the project the window has open, as the user does, rather than on a headless
//! copy of its own. The protocol, what an agent may ask and may not, and the file that has the port and the token are
//! `bimcraft_core::bridge`; this is the threads that listen and the frame that answers.
//!
//! The requests come from outside, so they are hostile: only this machine's address is bound, a connection that does not say the
//! token in its first line is dropped, a line is read to a limit, only a few connections are held, and a request that
//! is no request is an error reply. A request runs on the window's own thread, in the frame, as a command from a button does
//! (`BimApp::answer_agents`), so what the agent makes is the user's to see, to undo and to save.

use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bimcraft_core::bridge::{self, Entry, Error, MAX_LINE, Request};
use bimcraft_core::ids::Guid;
use egui::Context;

use crate::BimApp;

/// The most agents connected at once.
const MAX_CONNECTIONS: usize = 8;
/// How long a connection has, from the start, to say its first line whole, with the token in it.
const FIRST_LINE: Duration = Duration::from_secs(10);
/// How long a request waits for the window to answer: a render or an import of a big model takes a while.
const ANSWER: Duration = Duration::from_secs(600);
/// How many requests a frame answers.
const PER_FRAME: usize = 16;

/// A request on its way to the window, and where its reply goes. An agent that has given up waiting says so, and the window does not
/// do a request nobody is waiting for any more.
pub(crate) struct Asked {
    pub(crate) request: Request,
    pub(crate) reply: Sender<String>,
    pub(crate) abandoned: Arc<AtomicBool>,
}

pub(crate) struct Capture {
    revision: String,
    reply: Sender<String>,
    tag: Option<egui::UserData>,
    deadline: Instant,
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
        let (token, woken, live) = (entry.token.clone(), Arc::clone(&wake), Arc::new(AtomicUsize::new(0)));
        std::thread::Builder::new().name("control-accept".into()).spawn(move || {
            for stream in listener.incoming() {
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
                let (token, tx, woken, live) = (token.clone(), tx.clone(), Arc::clone(&woken), Arc::clone(&live));
                let spawned = std::thread::Builder::new().name("control-agent".into()).spawn(move || {
                    let _ = serve(stream, &token, &tx, &woken, first_line);
                    live.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    break;
                }
            }
        })?;
        Ok(Control { entry, asked, wake, file })
    }
}

#[cfg(test)]
impl Control {
    /// A control with no port: the requests are put in by the test.
    pub(crate) fn detached() -> (Control, Sender<Asked>) {
        let (tx, asked) = channel();
        let entry = Entry { port: 0, token: String::new() };
        (Control { entry, asked, wake: Arc::new(OnceLock::new()), file: None }, tx)
    }
}

impl Drop for Control {
    fn drop(&mut self) {
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
/// the connection (a web page's HTTP, say), after its reply.
fn serve(stream: TcpStream, token: &str, tx: &Sender<Asked>, wake: &OnceLock<Context>, first_line: Duration) -> io::Result<()> {
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
            if let Request::Capture { revision } = request {
                if self.capture.is_some() {
                    let _ = reply.send(bridge::reply(Err("another capture is pending".into())));
                } else {
                    self.capture = Some(Capture { revision, reply, tag: None, deadline: Instant::now() + Duration::from_secs(10) });
                }
                continue;
            }
            self.made_in();
            let result = bridge::run(&mut self.session, &request);
            // The core renames and drops the option with its set.
            self.active_option = self.session.option.clone();
            let _ = reply.send(bridge::reply(result));
            ctx.request_repaint();
        }
        if more {
            ctx.request_repaint();
        }
    }
}

impl BimApp {
    pub(crate) fn capture_frame(&mut self, ctx: &Context) {
        let Some(mut capture) = self.capture.take() else { return };
        let result = if self.session.revision().to_string() != capture.revision {
            Err("stale revision; inspect the current project before capturing".into())
        } else if Instant::now() >= capture.deadline {
            Err("the desktop did not return a frame in time".into())
        } else {
            if let Some(tag) = &capture.tag {
                let image = ctx.input(|input| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::Screenshot { viewport_id, user_data, image }
                            if *viewport_id == egui::ViewportId::ROOT && user_data == tag =>
                        {
                            Some(image.clone())
                        }
                        _ => None,
                    })
                });
                if let Some(image) = image {
                    let _ = capture.reply.send(bridge::reply(capture_png(&capture.revision, &image)));
                    return;
                }
            } else {
                let tag = egui::UserData::new(());
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Screenshot(tag.clone()));
                capture.tag = Some(tag);
            }
            ctx.request_repaint_after(capture.deadline.saturating_duration_since(Instant::now()));
            self.capture = Some(capture);
            return;
        };
        let _ = capture.reply.send(bridge::reply(result));
    }
}

fn capture_png(revision: &str, image: &egui::ColorImage) -> Result<serde_json::Value, String> {
    use image::ImageEncoder as _;
    let [width, height] = image.size;
    if width == 0 || height == 0 || width.checked_mul(height) != Some(image.pixels.len()) || image.pixels.len() > 16_777_216 {
        return Err("screenshot size is out of range".into());
    }
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|color| color.to_srgba_unmultiplied()).collect();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, width as u32, height as u32, image::ExtendedColorType::Rgba8)
        .map_err(|error| error.to_string())?;
    if png.len() > (MAX_LINE - 1024) * 3 / 4 {
        return Err("screenshot PNG exceeds the bridge reply limit".into());
    }
    let digest = bimcraft_update::sha256_of(&mut png.as_slice()).map_err(|error| error.to_string())?;
    let sha256: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(serde_json::json!({
        "revision": revision, "width": width, "height": height, "bytes": png.len(),
        "sha256": sha256, "pngBase64": bimcraft_core::base64(&png)
    }))
}

#[cfg(test)]
#[test]
fn capture_png_reports_the_encoded_frame() {
    let frame = egui::ColorImage::filled([2, 3], egui::Color32::RED);
    let result = capture_png("current", &frame).unwrap();
    assert_eq!(result["revision"], "current");
    assert_eq!(result["width"], 2);
    assert_eq!(result["height"], 3);
    assert!(result["bytes"].as_u64().is_some_and(|bytes| bytes > 8));
    assert_eq!(result["sha256"].as_str().unwrap().len(), 64);
    assert!(result["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"));
}
