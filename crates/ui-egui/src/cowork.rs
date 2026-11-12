//! Co-Work in the window (COWORK-PLAN.md, [Co-Work](../../../docs/areas/cowork.md)): Collaborate > Co-Work (CW) hosts this
//! window's project or joins another's, and Settings > Co-Work keeps the name, the address and port, and whether the host is
//! visible on the network. The session itself is `bimcraft_cowork::live`: the window pumps it once a pass ([`BimApp::pump`]),
//! and a guest's commands go through it from [`BimApp::exec`]. A guest's project is the host's, kept read-only in this
//! window ([`GUEST`]), and stays so when the session ends, to save as a copy.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use bimcraft_core::model::Model;
use bimcraft_cowork::live::{Happened, Hosting, Joined, VERSION};
use bimcraft_cowork::{Code, DISCOVERY_PORT, Found, HostConfig, PORT, Timing, discover, lan_address};
use egui::{Context, Id as UiId, RichText, Ui};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// What a guest's copy says when something would change it in this window: the host makes every change.
pub(crate) const GUEST: &str = "it is the host's project in a Co-Work session: the host makes every change, and a guest's go to the host";
/// How often the Join list asks the network who hosts, and how long it listens.
const LOOK_EVERY: Duration = Duration::from_secs(3);
const LISTEN: Duration = Duration::from_millis(800);

/// What the user keeps for Co-Work (`settings.json`).
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// The name the others see.
    pub name: String,
    /// Where a host listens: 0.0.0.0, every address of this machine's, unless the user says otherwise.
    pub addr: String,
    pub port: u16,
    /// Whether a host answers "who hosts?" on the LAN.
    pub visible: bool,
    /// Whether guests may edit, or only look.
    pub edit: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        let name = ["USER", "USERNAME"].iter().find_map(|v| std::env::var(v).ok()).unwrap_or_default();
        Prefs {
            name: name.chars().take(bimcraft_cowork::MAX_NAME).collect(),
            addr: "0.0.0.0".into(),
            port: PORT,
            visible: true,
            edit: true,
        }
    }
}

impl Prefs {
    pub fn to_json(&self) -> Value {
        json!({"name": self.name, "addr": self.addr, "port": self.port, "visible": self.visible, "edit": self.edit})
    }

    pub fn from_json(v: &Value, base: Prefs) -> Prefs {
        Prefs {
            name: v["name"].as_str().map_or(base.name, |n| n.chars().take(bimcraft_cowork::MAX_NAME).collect()),
            addr: v["addr"].as_str().filter(|a| a.parse::<IpAddr>().is_ok()).map_or(base.addr, str::to_string),
            port: v["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or(base.port),
            visible: v["visible"].as_bool().unwrap_or(base.visible),
            edit: v["edit"].as_bool().unwrap_or(base.edit),
        }
    }
}

/// The window's session, when it is in one.
pub enum Live {
    Host(Hosting),
    Guest(Joined),
}

/// The Co-Work dialog, while it is open.
#[derive(Default)]
pub struct Dialog {
    pub(crate) code: String,
    pub(crate) address: String,
    /// The hosts that answered last, and the asking under way.
    found: Vec<Found>,
    looking: Option<Receiver<Vec<Found>>>,
    looked: Option<Instant>,
    joining: Option<Receiver<Result<(Joined, Model), String>>>,
    pub(crate) error: Option<String>,
}

/// Where to join, as far as the window can tell at once: an address typed (a port after it, or the session port), or on the LAN
/// the host whose tag is the code's. None for a name, which the join thread looks up: a name server may take its time.
fn address(typed: &str, code: &Code, found: &[Found]) -> Result<Option<SocketAddr>, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return found
            .iter()
            .find(|f| f.tag == code.tag())
            .map(|f| Some(f.addr))
            .ok_or_else(|| t("No host on this network has that code: type the host's address too").to_string());
    }
    if let Ok(a) = typed.parse::<SocketAddr>() {
        return Ok(Some(a));
    }
    Ok(typed.parse::<IpAddr>().ok().map(|ip| SocketAddr::new(ip, PORT)))
}

/// `name` looked up, with a port after a colon or the session port.
fn look_up(name: &str) -> Option<SocketAddr> {
    name.to_socket_addrs().or_else(|_| (name, PORT).to_socket_addrs()).ok().and_then(|mut a| a.next())
}

impl BimApp {
    /// Collaborate > Co-Work (CW).
    pub(crate) fn open_cowork(&mut self) {
        if self.cowork_dialog.is_none() {
            self.cowork_dialog = Some(Dialog::default());
        }
    }

    /// Once a pass, from `pump`: the session's news, and the end of it.
    pub(crate) fn pump_cowork(&mut self) {
        let said = match &mut self.cowork {
            Some(Live::Host(h)) => h.pump(&mut self.session),
            Some(Live::Guest(j)) => j.pump(&mut self.session),
            None => return,
        };
        for happened in said {
            let (text, error) = match happened {
                Happened::Joined(name) => (tf("{} joined the session", &[&name]), false),
                Happened::Left(name, why) => (tf("{} left the session: {}", &[&name, &why]), false),
                Happened::Failed(why) => (why, true),
                Happened::Over(why) => {
                    self.cowork = None;
                    (
                        tf(
                            "The Co-Work session is over: {}. The project stays read-only here: save it as a file of your own to go on",
                            &[&why],
                        ),
                        true,
                    )
                }
            };
            self.status = crate::Status { text, error };
        }
    }

    /// The session, as it goes on when the project in the window was replaced (`reopened`): the host's ends, as it no longer has
    /// the project its guests have, and a guest's goes on only when the project is the one it joined (the join put it there).
    pub(crate) fn cowork_kept(&mut self, mut live: Option<Live>) -> Option<Live> {
        let uid = self.session.model.uid;
        live = match live {
            Some(Live::Host(h)) if h.project() != uid => {
                h.end(&mut self.session, &tf("{} opened another project", &[&self.cowork_prefs.name]));
                None
            }
            Some(Live::Guest(j)) if j.project() != uid => None,
            kept => kept,
        };
        // A project the user opened is theirs to change.
        if !matches!(live, Some(Live::Guest(_))) && self.session.read_only == Some(GUEST) {
            self.session.read_only = None;
        }
        live
    }

    /// The view control bar: whose session this is and who is in it; a click opens Co-Work. Nothing outside a session.
    pub(crate) fn cowork_indicator(&mut self, ui: &mut Ui) {
        let text = match &self.cowork {
            Some(Live::Host(h)) => tf("Co-Work: hosting, {} in", &[&h.people().len().to_string()]),
            Some(Live::Guest(j)) => tf("Co-Work: {}'s session", &[j.host()]),
            None => return,
        };
        ui.separator();
        let button = egui::Button::new(RichText::new(text).size(12.0).color(pal().accent)).frame(false).min_size(egui::vec2(0.0, 25.0));
        if ui.add(button).on_hover_text(t("Co-Work: who is in this session (CW)")).clicked() {
            self.open_cowork();
        }
    }

    /// What the title says of the session.
    pub(crate) fn cowork_title(&self) -> String {
        match &self.cowork {
            Some(Live::Host(_)) => format!("{} · ", t("Co-Work")),
            Some(Live::Guest(j)) => format!("{} · ", tf("{}'s Co-Work", &[j.host()])),
            None => String::new(),
        }
    }

    pub(crate) fn guest(&self) -> bool {
        matches!(self.cowork, Some(Live::Guest(_)))
    }

    /// Hosts the window's project, as Settings > Co-Work says.
    fn start_hosting(&mut self, ctx: &Context) -> Result<(), String> {
        let p = &self.cowork_prefs;
        let ip: IpAddr = p.addr.parse().map_err(|_| tf("{} is no address to listen on", &[&p.addr]))?;
        let name = if p.name.trim().is_empty() { t("Host").to_string() } else { p.name.trim().to_string() };
        let wake = ctx.clone();
        let config = HostConfig {
            addr: SocketAddr::new(ip, p.port),
            version: VERSION.into(),
            name,
            project: self.session.model.name.clone(),
            discovery: p.visible.then_some(DISCOVERY_PORT),
            timing: Timing::default(),
            wake: Arc::new(move || wake.request_repaint()),
        };
        let mut hosting = Hosting::start(config, &mut self.session)
            .map_err(|e| tf("Couldn't listen on port {}: {}", &[&p.port.to_string(), &e.to_string()]))?;
        hosting.edit = p.edit;
        self.cowork = Some(Live::Host(hosting));
        Ok(())
    }

    /// Joins as the dialog says, on a thread: the handshake and the project take a moment.
    fn start_joining(&mut self, d: &mut Dialog, ctx: &Context) {
        if self.dirty() {
            d.error = Some(t("Save this project first: joining puts the host's project in this window").to_string());
            return;
        }
        let code = match Code::parse(&d.code) {
            Ok(c) => c,
            Err(e) => return d.error = Some(e),
        };
        let at = match address(&d.address, &code, &d.found) {
            Ok(a) => a,
            Err(e) => return d.error = Some(e),
        };
        let typed = d.address.trim().to_string();
        // Made here: the interface's language is this thread's.
        let unknown =
            tf("Couldn't find {}: type an IP address or a name, with a port after a colon if it isn't {}", &[&typed, &PORT.to_string()]);
        let (name, wake) = (self.cowork_prefs.name.trim().to_string(), ctx.clone());
        let (tx, rx) = channel();
        let started = std::thread::Builder::new().name("cowork-join".into()).spawn(move || {
            let woken = wake.clone();
            let joined = at
                .or_else(|| look_up(&typed))
                .ok_or(unknown)
                .and_then(|addr| Joined::join(addr, &code, &name, Arc::new(move || woken.request_repaint())));
            let _ = tx.send(joined);
            wake.request_repaint();
        });
        match started {
            Ok(_) => (d.joining, d.error) = (Some(rx), None),
            Err(e) => d.error = Some(e.to_string()),
        }
    }

    /// The Co-Work dialog: host this project or join another's, and, in a session, who is in it.
    pub(crate) fn cowork_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.cowork_dialog.take() else { return };
        // What the network said since the last pass.
        if let Some(found) = d.looking.as_ref().and_then(|r| r.try_recv().ok()) {
            (d.found, d.looking) = (found, None);
        }
        if let Some(joined) = d.joining.as_ref().and_then(|r| r.try_recv().ok()) {
            d.joining = None;
            match joined {
                Ok((joined, model)) => {
                    let host = joined.host().to_string();
                    self.session.reset(model, None);
                    self.session.read_only = Some(GUEST);
                    self.cowork = Some(Live::Guest(joined));
                    self.status = crate::Status { text: tf("You joined {}'s session", &[&host]), error: false };
                    return;
                }
                Err(e) => d.error = Some(e),
            }
        }
        if self.cowork.is_none() && d.looking.is_none() && d.looked.is_none_or(|at| at.elapsed() > LOOK_EVERY) {
            let (tx, rx) = channel();
            let wake = ctx.clone();
            let targets =
                [SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT)), SocketAddr::from((Ipv4Addr::LOCALHOST, DISCOVERY_PORT))];
            let asked = std::thread::Builder::new().name("cowork-look".into()).spawn(move || {
                let _ = tx.send(discover(&targets, LISTEN));
                wake.request_repaint();
            });
            if asked.is_ok() {
                (d.looking, d.looked) = (Some(rx), Some(Instant::now()));
            }
            ctx.request_repaint_after(LOOK_EVERY);
        }
        let (mut close, mut start, mut join, mut end) = (false, false, false, false);
        let mut kick = None;
        let r = Modal::new(UiId::new("cowork"), t("Co-Work")).width(500.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                match &mut self.cowork {
                    Some(Live::Host(h)) => {
                        ui.label(t("You host this project. Others join with this code; on another network they need this machine's address and port too."));
                        ui.add_space(6.0);
                        let code = h.host().code().to_string();
                        ui.horizontal(|ui| {
                            ui.label(t("Code:"));
                            ui.label(RichText::new(&code).monospace().size(16.0).strong());
                            if ui.button(t("Copy")).clicked() {
                                ui.ctx().copy_text(code.clone());
                            }
                        });
                        // The address it listens on, or for all of them (0.0.0.0) the one the LAN reaches it by.
                        let listen = self.cowork_prefs.addr.parse::<IpAddr>().ok().filter(|ip| !ip.is_unspecified());
                        let at = listen.or_else(lan_address).map_or_else(|| t("this machine").to_string(), |ip| ip.to_string());
                        ui.label(tf("Address: {}, port {}", &[&at, &h.host().port().to_string()]));
                        if self.cowork_prefs.visible && h.host().discovery_port().is_none() {
                            ui.label(RichText::new(t("Not visible on this network: another session on this machine has the port")).color(pal().muted));
                        }
                        if ui.checkbox(&mut h.edit, t("Guests may edit")).changed() {
                            self.cowork_prefs.edit = h.edit;
                        }
                        ui.add_space(6.0);
                        let people = h.people();
                        if people.is_empty() {
                            ui.label(RichText::new(t("Nobody has joined yet.")).color(pal().muted));
                        }
                        for (id, name) in people {
                            ui.horizontal(|ui| {
                                ui.label(&name);
                                if ui.small_button(t("Kick")).clicked() {
                                    kick = Some(id);
                                }
                            });
                        }
                    }
                    Some(Live::Guest(j)) => {
                        ui.label(tf("You are in {}'s session. Your edits go to the host; the project stays read-only here when the session ends.", &[j.host()]));
                        ui.add_space(6.0);
                        ui.label(tf("In it: {} and {}", &[j.host(), &j.people().join(", ")]));
                    }
                    None => {
                        ui.heading(t("Host this project"));
                        ui.label(t("Others on this network find it; on another, they join with this machine's address and port."));
                        start = ui.button(t("Start Co-Work")).clicked();
                        ui.add_space(10.0);
                        ui.heading(t("Join a session"));
                        if d.found.is_empty() {
                            ui.label(RichText::new(t("Looking for sessions on this network…")).color(pal().muted));
                        }
                        for f in &d.found {
                            let row = format!("{} · {} · {}", f.name, f.project, f.addr);
                            if ui.selectable_label(d.address == f.addr.to_string(), row).clicked() {
                                d.address = f.addr.to_string();
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label(t("Code:"));
                            ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(&mut d.code).hint_text("XXXX-XXXX-XXXX-XXXX"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(t("Address:"));
                            let hint = t("on this network, none");
                            ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(&mut d.address).hint_text(hint));
                        });
                        if d.joining.is_some() {
                            ui.label(RichText::new(t("Joining…")).color(pal().muted));
                        } else {
                            join = ui.button(t("Join")).clicked();
                        }
                    }
                }
                if let Some(e) = &d.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    match &self.cowork {
                        Some(Live::Host(_)) => end = ui.button(t("End Session")).clicked(),
                        Some(Live::Guest(_)) => end = ui.button(t("Leave")).clicked(),
                        None => {}
                    }
                    close = ui.button(t("Close")).clicked();
                });
            });
        });
        if start {
            d.error = self.start_hosting(ctx).err();
        }
        if join {
            self.start_joining(&mut d, ctx);
        }
        if let (Some(id), Some(Live::Host(h))) = (kick, &self.cowork) {
            h.kick(id, &tf("{} asked you to leave", &[h.name()]));
        }
        if end {
            match self.cowork.take() {
                Some(Live::Host(h)) => {
                    let why = tf("{} ended the session", &[h.name()]);
                    h.end(&mut self.session, &why);
                    self.status = crate::Status { text: t("The Co-Work session is over").to_string(), error: false };
                }
                Some(Live::Guest(_)) => {
                    self.status = crate::Status {
                        text: t("You left the session. The project stays read-only here: save it as a file of your own to go on")
                            .to_string(),
                        error: false,
                    };
                }
                None => {}
            }
        }
        if !(close || r.should_close()) {
            self.cowork_dialog = Some(d);
        }
    }

    /// File > Settings > Co-Work: the name the others see, where a host listens, and whether it is visible on the network.
    pub(crate) fn cowork_page(&mut self, ui: &mut Ui) {
        let p = &mut self.cowork_prefs;
        ui.label(t("Co-Work lets others on BIMCraft work on this project with you, live (Collaborate > Co-Work, CW)."));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(t("Your name:"));
            ui.add_sized([200.0, 22.0], egui::TextEdit::singleline(&mut p.name).char_limit(bimcraft_cowork::MAX_NAME));
        });
        ui.horizontal(|ui| {
            ui.label(t("Listen on:"));
            ui.add_sized([140.0, 22.0], egui::TextEdit::singleline(&mut p.addr));
            ui.label(t("port"));
            ui.add(egui::DragValue::new(&mut p.port).range(1..=u16::MAX).update_while_editing(false));
        });
        if p.addr.parse::<IpAddr>().is_err() {
            ui.label(RichText::new(t("That is no IP address: 0.0.0.0 listens on all of this machine's")).color(pal().error));
        }
        ui.label(
            RichText::new(t("0.0.0.0 listens on every address of this machine's. From outside this network, guests need the port forwarded to it, which is for you to set up."))
                .color(pal().muted),
        );
        ui.checkbox(&mut p.visible, t("Visible on this network")).on_hover_text(t(
            "Others on this network see the session in their Join list: your name and the project's. Off, they need this machine's address",
        ));
        ui.checkbox(&mut p.edit, t("Guests may edit"))
            .on_hover_text(t("Off, guests only look: they see the project and the others, and change nothing"));
    }
}

/// [`address`] with no host found on the network, for the tests.
#[cfg(test)]
pub(crate) fn tests_address(typed: &str, code: &Code) -> Result<Option<SocketAddr>, String> {
    address(typed, code, &[])
}
