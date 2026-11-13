//! Who comes into a session and who hosts it: someone without the code asks to join (Ask to Join), and the host lets them in once
//! the two have compared the digits both windows show (`bimcraft_cowork::knock`); and a host hands the session to a guest (Make
//! Host, or Hand Over and Close), whose window hosts it from its copy while the others move there.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::mpsc::channel;

use bimcraft_cowork::live::{Joined, VERSION};
use bimcraft_cowork::{PORT, Timing, knock};
use egui::{Context, Id as UiId, RichText};

use super::{Dialog, GUEST, Live, look_up};
use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

impl BimApp {
    /// Ask to Join: knocks at the address typed (or the session picked in the list) on a thread, shows the digits once the host has
    /// them too, and joins with the code if the host lets this window in.
    pub(super) fn start_knocking(&mut self, d: &mut Dialog, ctx: &Context) {
        if self.dirty() {
            d.error = Some(t("Save this project first: joining puts the host's project in this window").to_string());
            return;
        }
        let typed = d.address.trim().to_string();
        if typed.is_empty() {
            d.error = Some(t("Pick a session in the list, or type the host's address, to ask to join").to_string());
            return;
        }
        let at = typed.parse::<SocketAddr>().ok().or_else(|| typed.parse::<IpAddr>().ok().map(|ip| SocketAddr::new(ip, PORT)));
        // Made here: the interface's language is this thread's.
        let unknown =
            tf("Couldn't find {}: type an IP address or a name, with a port after a colon if it isn't {}", &[&typed, &PORT.to_string()]);
        let (name, wake) = (self.cowork_prefs.name.trim().to_string(), ctx.clone());
        let ((tx, rx), (digits_tx, digits_rx)) = (channel(), channel());
        let started = std::thread::Builder::new().name("cowork-knock".into()).spawn(move || {
            let (woken, shown) = (wake.clone(), wake.clone());
            let joined = at.or_else(|| look_up(&typed)).ok_or(unknown).and_then(|addr| {
                let code = knock(addr, &name, VERSION, Timing::default(), move |d| {
                    let _ = digits_tx.send(d.to_string());
                    shown.request_repaint();
                })?;
                Joined::join(addr, &code, &name, Arc::new(move || woken.request_repaint()))
            });
            let _ = tx.send(joined);
            wake.request_repaint();
        });
        match started {
            Ok(_) => (d.joining, d.digits, d.shown, d.error) = (Some(rx), Some(digits_rx), None, None),
            Err(e) => d.error = Some(e.to_string()),
        }
    }

    /// The host's window: the first knock waiting, to let in or turn away once the person has read out the same digits.
    pub(crate) fn knock_window(&mut self, ctx: &Context) {
        let Some((knock, name, digits)) = self.cowork_seen.knocks.first().cloned() else { return };
        if !matches!(self.cowork, Some(Live::Host(_))) {
            self.cowork_seen.knocks.clear();
            return;
        }
        let (mut yes, mut no) = (false, false);
        let r = Modal::new(UiId::new(("cowork-knock", knock)), t("Someone asks to join")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(tf("{} asks to join your Co-Work session without the code.", &[&name]));
                ui.add_space(6.0);
                ui.label(RichText::new(&digits).monospace().size(22.0).strong());
                ui.add_space(6.0);
                let said = t("Let them in only if their screen shows these same digits: ask them to read theirs out. Others would see different ones.");
                ui.label(RichText::new(said).color(pal().muted));
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    yes = ui.button(t("Let In")).clicked();
                    no = ui.button(t("Turn Away")).clicked();
                });
            });
        });
        if yes || no || r.should_close() {
            if let Some(Live::Host(h)) = &self.cowork {
                h.answer_knock(knock, yes);
            }
            self.cowork_seen.knocks.retain(|(k, ..)| *k != knock);
        }
    }

    /// Hands the session to guest `id`: its window hosts it from its copy, and the others move there.
    pub(crate) fn make_host(&mut self, id: u32) {
        let Some(Live::Host(h)) = &mut self.cowork else { return };
        let name = h.others().get(&id).map(|o| o.name.clone()).unwrap_or_default();
        // The chat goes with the session, in the form a saved chat has: the new host's window goes on with it.
        let project = self.session.path.clone().unwrap_or_else(|| self.session.model.name.clone().into());
        let chat = self.assistant.chat.as_ref().map_or(serde_json::Value::Null, |c| c.to_saved(&project));
        self.status = match h.hand_over(&mut self.session, id, chat) {
            Ok(()) => crate::Status { text: tf("Handing the session to {}…", &[&name]), error: false },
            Err(e) => crate::Status { text: e, error: true },
        };
    }

    /// The host's window: the guest it handed the session to hosts it now, and the others are on their way: its own session ends
    /// (its project stays its own), and it closes if that is why it handed over.
    pub(super) fn handed_over(&mut self, ctx: &Context, name: &str) {
        if let Some(Live::Host(h)) = self.cowork.take() {
            h.end(&mut self.session, &tf("{} hosts the session now", &[name]));
        }
        self.status = crate::Status { text: tf("{} hosts the session now: the others have moved there", &[name]), error: false };
        if std::mem::take(&mut self.cowork_seen.close_after) {
            self.cowork_quit = Some(true);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// A guest's window the session is handed to: it hosts it from its copy, as Co-Work's settings say, goes on with the old host's
    /// AI chat (`chat`, read as a saved chat is: whole or not at all), and tells the old host where; or, when it can't, says why and
    /// stays a guest.
    pub(super) fn take_over(&mut self, ctx: &Context, chat: &serde_json::Value) {
        let Some(Live::Guest(old)) = self.cowork.take() else { return };
        self.session.read_only = None;
        match self.start_hosting(ctx) {
            Ok(()) => {
                if let Some(Live::Host(h)) = &self.cowork {
                    old.hosting(Ok((h.host().port(), h.host().code().clone())));
                }
                let provider = self.assistant.provider;
                if let Some(c) = self.assistant.chat.as_mut().filter(|_| !chat.is_null()) {
                    c.restore(chat, provider);
                }
                let said = t("You host the Co-Work session now. The project is your copy of it: save it as a file of your own");
                self.status = crate::Status { text: said.to_string(), error: false };
            }
            Err(e) => {
                old.hosting(Err(e.clone()));
                self.session.read_only = Some(GUEST);
                self.cowork = Some(Live::Guest(old));
                self.status = crate::Status { text: tf("Couldn't host the session: {}", &[&e]), error: true };
            }
        }
    }

    /// A guest's window whose session moved: it joins the new host, its copy replaced by the new host's.
    pub(super) fn move_to_new_host(&mut self, ctx: &Context) {
        let Some((host, addr, code)) = self.cowork_seen.moving.take() else { return };
        let mut d = Dialog { code: code.to_string(), address: addr.to_string(), ..Dialog::default() };
        self.join_now(&mut d, ctx);
        self.cowork_dialog = Some(d);
        self.status = crate::Status { text: tf("The session moved to {}: joining there", &[&host]), error: false };
    }
}
