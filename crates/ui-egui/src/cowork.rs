//! Co-Work in the window (COWORK-PLAN.md, [Co-Work](../../../docs/areas/cowork.md)): Collaborate > Co-Work (CW) hosts this
//! window's project or joins another's, and Settings > Co-Work keeps the name, the address and port, and whether the host is
//! visible on the network. The session itself is `bimcraft_cowork::live`: the window pumps it once a pass ([`BimApp::pump`]),
//! and a guest's commands go through it from [`BimApp::exec`]. A guest's project is the host's, kept read-only in this
//! window ([`GUEST`]), and stays so when the session ends, to save as a copy.
//!
//! Each window sends where it is ([`BimApp::my_presence`]) and draws the others in their colours ([`colour`]): their pointers on
//! the plan it shows and what they have selected or just changed ([`paint_plan`]), their cameras and selections in 3D
//! ([`paint_3d`]), a badge for each on the view control bar that follows them, and in Properties who else has the selection.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use bimcraft_core::cowork::{Look, MAX_SELECTION, On, Presence};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{Drawing, Prim, View};
use bimcraft_core::view3d::{Camera, P3, Scene};
use bimcraft_cowork::live::{Happened, Hosting, Joined, Other, VERSION};
use bimcraft_cowork::{Code, DISCOVERY_PORT, Found, HostConfig, PORT, Timing, discover, lan_address};
use egui::{Color32, Context, FontId, Id as UiId, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, vec2};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;

mod chat;
use crate::style::pal;
use crate::{BimApp, Sheet};
pub use chat::{Ai, Lending};

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
    /// Whether the host's AI key answers guests who have none.
    pub lend_key: bool,
    /// Whether, as a guest, this window's key answers others who have none.
    pub lending: Lending,
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
            lend_key: true,
            lending: Lending::Ask,
        }
    }
}

impl Prefs {
    pub fn to_json(&self) -> Value {
        let (lend_key, lending) = (self.lend_key, self.lending.key());
        json!({"name": self.name, "addr": self.addr, "port": self.port, "visible": self.visible, "edit": self.edit, "lend_key": lend_key, "lending": lending})
    }

    pub fn from_json(v: &Value, base: Prefs) -> Prefs {
        Prefs {
            name: v["name"].as_str().map_or(base.name, |n| n.chars().take(bimcraft_cowork::MAX_NAME).collect()),
            addr: v["addr"].as_str().filter(|a| a.parse::<IpAddr>().is_ok()).map_or(base.addr, str::to_string),
            port: v["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or(base.port),
            visible: v["visible"].as_bool().unwrap_or(base.visible),
            edit: v["edit"].as_bool().unwrap_or(base.edit),
            lend_key: v["lend_key"].as_bool().unwrap_or(base.lend_key),
            lending: v["lending"].as_str().and_then(Lending::from_key).unwrap_or(base.lending),
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
    pub(crate) fn pump_cowork(&mut self, ctx: &Context) {
        let said = match &mut self.cowork {
            Some(Live::Host(h)) => h.pump(&mut self.session),
            Some(Live::Guest(j)) => j.pump(&mut self.session),
            None => {
                // The chat of a session over is gone with it.
                self.cowork_ai = Ai::default();
                return;
            }
        };
        for happened in said {
            let (text, error) = match happened {
                Happened::Joined(name) => (tf("{} joined the session", &[&name]), false),
                Happened::Left(name, why) => (tf("{} left the session: {}", &[&name, &why]), false),
                Happened::Failed(why) => (why, true),
                Happened::Edited(by, ids) => {
                    let colour = others_of(&self.cowork).and_then(|o| o.get(&by)).map_or(0, |o| o.colour);
                    self.cowork_seen.flashes.push((Instant::now(), colour, ids));
                    continue;
                }
                Happened::ChatSay { peer, name, text, files } => {
                    let taken = match bimcraft_assistant::wire::files_from(&files) {
                        Some(files) => self.chat_on_host(ctx, peer, &name, &text, files),
                        None => Err(t("The files with that message weren't ones the assistant takes").to_string()),
                    };
                    if let (Err(why), Some(Live::Host(h))) = (taken, &self.cowork) {
                        h.refuse(peer, &why);
                    }
                    continue;
                }
                Happened::ChatStop(name) => {
                    if let Some(chat) = self.assistant.chat.as_mut().filter(|c| c.busy()) {
                        chat.stop(&mut self.session);
                        self.status = crate::Status { text: tf("{} stopped the assistant", &[&name]), error: false };
                    }
                    continue;
                }
                Happened::Chat(state) => {
                    self.chat_happened(&state);
                    continue;
                }
                Happened::AiRequest { reference, own, provider, model, history } => {
                    self.lend_for(reference, own, &provider, model, history);
                    continue;
                }
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
        self.pump_chat(ctx);
        let seen = &mut self.cowork_seen;
        seen.flashes.retain(|(at, ..)| at.elapsed() < FLASH);
        if !seen.flashes.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.cowork.is_none() {
            (seen.following, seen.followed) = (None, None);
            return;
        }
        // Where this window is, as often as the others need it: more would only crowd what goes to the host.
        if seen.sent.is_some_and(|at| at.elapsed() < PRESENT_EVERY) {
            ctx.request_repaint_after(PRESENT_EVERY);
        } else {
            let p = self.my_presence();
            match &mut self.cowork {
                Some(Live::Host(h)) => h.present(p),
                Some(Live::Guest(j)) => j.present(p),
                None => {}
            }
            self.cowork_seen.sent = Some(Instant::now());
        }
        self.follow();
    }

    /// The view on screen, as a presence names it.
    fn on_screen(&self) -> Option<On> {
        let (kind, id) = match self.sheet {
            Sheet::Plan(id) => ("plan", Some(id)),
            Sheet::Ceiling(id) => ("ceiling", Some(id)),
            Sheet::View3D => ("3d", self.active_3d()),
            Sheet::Section(id) => ("section", Some(id)),
            Sheet::Paper(id) => ("sheet", Some(id)),
            Sheet::Table(id) => ("schedule", Some(id)),
            _ => return None,
        };
        Some(On { kind: kind.into(), of: id.and_then(|id| self.session.model.elements.guid(id)) })
    }

    /// The view a presence names, in this window's project.
    fn sheet_of(&self, on: &On) -> Option<Sheet> {
        let id = on.of.and_then(|g| self.session.model.elements.id(g));
        Some(match (on.kind.as_str(), id) {
            ("plan", Some(id)) => Sheet::Plan(id),
            ("ceiling", Some(id)) => Sheet::Ceiling(id),
            ("3d", _) => Sheet::View3D,
            ("section", Some(id)) => Sheet::Section(id),
            ("sheet", Some(id)) => Sheet::Paper(id),
            ("schedule", Some(id)) => Sheet::Table(id),
            _ => return None,
        })
    }

    /// Where this window is: its view, the pointer on its plan and the plan's zoom, its 3D camera, its selection.
    pub(crate) fn my_presence(&self) -> Presence {
        let m = &self.session.model;
        let plan = match self.sheet {
            Sheet::Plan(k) | Sheet::Ceiling(k) => Some(k),
            _ => None,
        };
        Presence {
            view: self.on_screen(),
            cursor: self.cowork_seen.pointer.filter(|(k, _)| Some(*k) == plan).map(|(_, p)| p),
            zoom: plan.and_then(|k| self.canvas.view(k)).map(|v| (v.cx, v.cy, v.scale)),
            camera: self.orbit.camera.filter(|_| self.sheet == Sheet::View3D).map(|c| Look::of(&c)),
            selection: self.selection.iter().take(MAX_SELECTION).filter_map(|id| m.elements.guid(*id)).collect(),
        }
    }

    /// What following shows now: the view, a plan's zoom, the 3D camera without its size (each window has its own).
    fn followed_now(&self) -> Shown {
        let zoom = match self.sheet {
            Sheet::Plan(k) | Sheet::Ceiling(k) => self.canvas.view(k).map(|v| (v.cx, v.cy, v.scale)),
            _ => None,
        };
        (self.sheet, zoom, self.orbit.camera.map(|c| Camera { width: 0, height: 0, ..c }))
    }

    /// Following someone: their view, their plan's zoom, their 3D camera, until the user moves the view themselves.
    fn follow(&mut self) {
        let Some(id) = self.cowork_seen.following else { return };
        let Some(o) = others_of(&self.cowork).and_then(|o| o.get(&id)).cloned() else {
            (self.cowork_seen.following, self.cowork_seen.followed) = (None, None);
            return;
        };
        if self.cowork_seen.followed.is_some_and(|f| f != self.followed_now()) {
            (self.cowork_seen.following, self.cowork_seen.followed) = (None, None);
            self.status = crate::Status { text: tf("You stopped following {}", &[&o.name]), error: false };
            return;
        }
        let Some(sheet) = o.presence.view.as_ref().and_then(|on| self.sheet_of(on)) else { return };
        if sheet != self.sheet {
            self.open_sheet(sheet);
        }
        match (sheet, o.presence.zoom, o.presence.camera) {
            (Sheet::Plan(k) | Sheet::Ceiling(k), Some((cx, cy, scale)), _) => self.canvas.set_view(k, cx, cy, scale),
            (Sheet::View3D, _, Some(look)) => {
                let (width, height) = self.orbit.camera.map_or((look.width, look.height), |c| (c.width, c.height));
                self.orbit.camera = Some(Camera { width, height, ..look.camera() });
            }
            _ => {}
        }
        self.cowork_seen.followed = Some(self.followed_now());
    }

    /// Following `id`, or no longer when it is the one followed.
    pub(crate) fn toggle_follow(&mut self, id: u32) {
        let seen = &mut self.cowork_seen;
        seen.following = if seen.following == Some(id) { None } else { Some(id) };
        seen.followed = None;
    }

    /// In Properties: who else has the selection selected, each in their colour.
    pub(crate) fn held_by_others(&self, ui: &mut Ui) {
        let Some(others) = others_of(&self.cowork) else { return };
        let m = &self.session.model;
        let mine: HashSet<_> = self.selection.iter().filter_map(|id| m.elements.guid(*id)).collect();
        for o in others.values().filter(|o| o.presence.selection.iter().any(|g| mine.contains(g))) {
            ui.label(RichText::new(tf("{} has this selected", &[&o.name])).color(colour(o.colour)));
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
        // A badge for each of the others: where they are, and a click follows them.
        let others: Vec<(u32, Other)> = others_of(&self.cowork).into_iter().flatten().map(|(id, o)| (*id, o.clone())).collect();
        for (id, o) in others {
            // An id of its own, by whose it is, so a click lands on it whatever stands before it on the bar.
            let (rect, _) = ui.allocate_exact_size(vec2(20.0, 25.0), Sense::hover());
            let r = ui.interact(rect, UiId::new(("cowork-badge", id)), Sense::click());
            let (c, at) = (colour(o.colour), rect.center());
            ui.painter().circle_filled(at, 8.0, c);
            if self.cowork_seen.following == Some(id) {
                ui.painter().circle_stroke(at, 10.0, Stroke::new(2.0, pal().text));
            }
            let initial = o.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            ui.painter().text(at, egui::Align2::CENTER_CENTER, initial, FontId::proportional(10.0), Color32::WHITE);
            let view = o.presence.view.as_ref().and_then(|on| self.sheet_of(on)).map_or_else(String::new, |s| self.view_title(s));
            let tip = tf("{}: {}, {} selected. Click to follow", &[&o.name, &view, &o.presence.selection.len().to_string()]);
            let r = r.on_hover_text(tip);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &o.name));
            if r.clicked() {
                self.toggle_follow(id);
            }
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
                        ui.checkbox(&mut self.cowork_prefs.lend_key, t("Guests without an AI key may use mine"))
                            .on_hover_text(t("Whoever has a key of their own always uses it. Off, one who has none uses a key a guest lends"));
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
        ui.add_space(6.0);
        ui.label(t("The shared AI chat: whoever has an AI key of their own uses it; one who has none is lent one."));
        ui.checkbox(&mut p.lend_key, t("Guests without an AI key may use mine"));
        ui.horizontal(|ui| {
            ui.label(t("As a guest, lend my AI key to those without one:"));
            for l in Lending::ALL {
                ui.radio_value(&mut p.lending, l, t(l.label()));
            }
        });
    }
}

/// [`address`] with no host found on the network, for the tests.
#[cfg(test)]
pub(crate) fn tests_address(typed: &str, code: &Code) -> Result<Option<SocketAddr>, String> {
    address(typed, code, &[])
}

/// The session's colours, by its people's colour numbers, the host's first: none of them the selection's blue or the hover's.
const COLOURS: [Color32; 9] = [
    Color32::from_rgb(230, 126, 34),
    Color32::from_rgb(39, 174, 96),
    Color32::from_rgb(142, 68, 173),
    Color32::from_rgb(214, 51, 132),
    Color32::from_rgb(22, 160, 133),
    Color32::from_rgb(192, 57, 43),
    Color32::from_rgb(160, 104, 57),
    Color32::from_rgb(128, 128, 0),
    Color32::from_rgb(52, 73, 94),
];

pub(crate) fn colour(n: usize) -> Color32 {
    COLOURS[n % COLOURS.len()]
}

/// How long another's edit stays lit, and how often a window sends where it is at most.
const FLASH: Duration = Duration::from_millis(1500);
const PRESENT_EVERY: Duration = Duration::from_millis(66);

/// What following shows: the view, a plan's middle and zoom, the 3D camera.
type Shown = (Sheet, Option<(f64, f64, f64)>, Option<Camera>);
/// Others' selections boxed in 3D: the scene and the selections (colour, element) they were found for, and the boxes by colour.
pub(crate) type Boxes = (usize, Vec<(usize, Id)>, Vec<(usize, (P3, P3))>);

/// What the window keeps of the others and of its own presence.
#[derive(Default)]
pub struct Seen {
    /// The pointer on a plan: the plan's key and the model point under it.
    pub(crate) pointer: Option<(Id, P2)>,
    sent: Option<Instant>,
    /// Others' edits still lit: since when, in whose colour, which elements.
    flashes: Vec<(Instant, usize, Vec<Id>)>,
    /// Whom the window follows, and what following last showed: a change of the user's own stops it.
    pub(crate) following: Option<u32>,
    followed: Option<Shown>,
    /// Others' selections boxed in 3D: for which scene and which selections, and the boxes in their colours.
    pub(crate) boxes: Option<Boxes>,
}

/// The others in a window's session.
pub(crate) fn others_of(live: &Option<Live>) -> Option<&BTreeMap<u32, Other>> {
    match live {
        Some(Live::Host(h)) => Some(h.others()),
        Some(Live::Guest(j)) => Some(j.others()),
        None => None,
    }
}

/// What the others show on a view: their pointers on it, and the elements they have selected or just changed, each by their
/// colour number.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Marks {
    pub(crate) pointers: Vec<(String, usize, P2)>,
    pub(crate) lit: HashMap<Id, usize>,
}

pub(crate) fn marks(others: &BTreeMap<u32, Other>, m: &Model, here: &On, seen: &Seen) -> Marks {
    let mut marks = Marks::default();
    for o in others.values() {
        marks.lit.extend(o.presence.selection.iter().filter_map(|g| m.elements.id(*g)).map(|id| (id, o.colour)));
        if let Some(q) = o.presence.cursor.filter(|_| o.presence.view.as_ref() == Some(here)) {
            marks.pointers.push((o.name.clone(), o.colour, q));
        }
    }
    for (_, c, ids) in &seen.flashes {
        marks.lit.extend(ids.iter().map(|id| (*id, *c)));
    }
    marks
}

/// The plan `key` (a ceiling plan with `up`) as the others mark it, over its drawing: what they selected or just changed outlined
/// in their colours, and their pointers with their names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_plan(
    p: &Painter,
    live: &Option<Live>,
    seen: &Seen,
    m: &Model,
    (key, up): (Id, bool),
    rect: Rect,
    view: &View,
    d: (&Drawing, crate::canvas::Place),
) {
    let Some(others) = others_of(live) else { return };
    let here = On { kind: (if up { "ceiling" } else { "plan" }).into(), of: m.elements.guid(key) };
    let marks = marks(others, m, &here, seen);
    let (drawing, at) = d;
    if !marks.lit.is_empty() {
        for (prim, owner) in drawing.prims.iter().zip(&drawing.owners) {
            let Some(c) = owner.and_then(|id| marks.lit.get(&id)).map(|c| colour(*c)) else { continue };
            match prim {
                Prim::Line { pts, .. } => {
                    p.add(Shape::line(pts.iter().map(|q| at.pos(*q)).collect(), Stroke::new(2.5, c)));
                }
                Prim::Fill { rings, .. } => {
                    for r in rings {
                        p.add(Shape::closed_line(r.iter().map(|q| at.pos(*q)).collect(), Stroke::new(1.5, c)));
                    }
                }
                Prim::Image { .. } => {}
            }
        }
    }
    for (name, c, q) in marks.pointers {
        let (x, y) = view.px(q);
        pointer(p, rect.min + vec2(x, y), colour(c), &name);
    }
}

/// Another's pointer: an arrow at `at`, and their name on a tag of their colour.
fn pointer(p: &Painter, at: Pos2, c: Color32, name: &str) {
    if !at.is_finite() {
        return;
    }
    let arrow = vec![at, at + vec2(0.0, 16.0), at + vec2(4.5, 12.0), at + vec2(11.0, 11.0)];
    p.add(Shape::convex_polygon(arrow, c, Stroke::new(1.0, Color32::WHITE)));
    tag(p, at + vec2(10.0, 14.0), c, name);
}

fn tag(p: &Painter, at: Pos2, c: Color32, name: &str) {
    let galley = p.layout_no_wrap(name.to_string(), FontId::proportional(11.0), Color32::WHITE);
    let r = Rect::from_min_size(at, galley.size() + vec2(8.0, 4.0));
    p.rect_filled(r, 3.0, c);
    p.galley(r.min + vec2(4.0, 2.0), galley, Color32::WHITE);
}

/// The 3D view as the others mark it: what they selected, boxed in their colours, and for each in a 3D view, where they look
/// from and at, with their name. `screen` is where a model point shows (not finite: off the picture, or behind the eye).
pub(crate) fn paint_3d(p: &Painter, live: &Option<Live>, seen: &mut Seen, m: &Model, scene: &Scene, screen: &dyn Fn(P3) -> Pos2) {
    let Some(others) = others_of(live) else { return };
    let wanted: Vec<(usize, Id)> =
        others.values().flat_map(|o| o.presence.selection.iter().filter_map(|g| m.elements.id(*g)).map(move |id| (o.colour, id))).collect();
    let at = std::ptr::from_ref(scene) as usize;
    if seen.boxes.as_ref().is_none_or(|(s, w, _)| *s != at || *w != wanted) {
        let found = scene.boxes(&wanted.iter().map(|(_, id)| *id).collect());
        let boxes = wanted.iter().filter_map(|(c, id)| found.get(id).map(|b| (*c, *b))).collect();
        seen.boxes = Some((at, wanted, boxes));
    }
    for (c, b) in seen.boxes.iter().flat_map(|(_, _, b)| b) {
        for [a, z] in bimcraft_core::view3d::box_edges(*b) {
            let (a, z) = (screen(a), screen(z));
            if a.is_finite() && z.is_finite() {
                p.line_segment([a, z], Stroke::new(2.0, colour(*c)));
            }
        }
    }
    for o in others.values().filter(|o| o.presence.view.as_ref().is_some_and(|v| v.kind == "3d")) {
        let Some(look) = o.presence.camera else { continue };
        let (eye, target, c) = (screen(look.eye()), screen(look.target), colour(o.colour));
        if !eye.is_finite() {
            continue;
        }
        if target.is_finite() {
            p.add(Shape::dashed_line(&[eye, target], Stroke::new(1.5, c), 6.0, 4.0));
        }
        p.circle_filled(eye, 6.0, c);
        p.circle_stroke(eye, 6.0, Stroke::new(1.5, Color32::WHITE));
        tag(p, eye + vec2(9.0, -22.0), c, &o.name);
    }
}
