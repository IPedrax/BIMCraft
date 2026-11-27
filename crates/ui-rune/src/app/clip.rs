//! The clipboard in the window: Copy to Clipboard and Cut keep the selection as the core's text (`copy_to_clipboard`, `cut_to_clipboard`),
//! on the system clipboard so another BIMCraft window pastes it, and in this window too for a system clipboard that has lost it; Paste
//! places it (`paste`): by hand in a plan (a click puts the middle of the copy down), or Aligned, to the same place, to the view on
//! screen, to the levels or the views a dialog lists. The system clipboard is the window's to reach (`ui::clipboard_io`, which has the
//! `Tx`): what is to go on it waits in `clip_out`, and a paste asks for it by `paste_asked`. After egui's `clipboard.rs`; the logic with
//! no window is `crate::clip`. A child of `app` so it reaches the private handlers.

use std::collections::BTreeSet;

use bimcraft_core::clipboard as core_clip;
use bimcraft_core::ids::Id;
use serde_json::{Value, json};

use super::App;
use crate::clip::{self, Choose, Mode};
use crate::lang::{t, tf};

impl App {
    /// Copy, or with `cut` cut, the selection: the text is kept here and queued for the system clipboard. False when nothing is selected
    /// or the core refuses (what it says is the status bar's).
    pub fn copy_selection(&mut self, cut: bool) -> bool {
        if self.selection.is_empty() {
            self.message = Some(t("select what to copy first").to_string());
            return false;
        }
        let mut p = json!({"ids": self.selection});
        if let Some(view) = clip::view_id(self.view) {
            p["view"] = json!(view);
        }
        let reply = match self.session.execute(if cut { "cut_to_clipboard" } else { "copy_to_clipboard" }, &p) {
            Ok(reply) => reply,
            Err(why) => {
                self.message = Some(why);
                return false;
            }
        };
        let Some(text) = reply["clipboard"].as_str() else { return false };
        (self.clip_text, self.clip_out) = (Some(text.to_string()), Some(text.to_string()));
        if cut {
            self.selection.clear();
        }
        self.settle();
        let count = reply["count"].as_u64().unwrap_or(0).to_string();
        let left = reply["left_out"].as_array().map_or(0, Vec::len);
        self.message = Some(match (cut, left) {
            (false, 0) => tf("Copied {} elements to the clipboard.", &[&count]),
            (true, 0) => tf("Cut {} elements to the clipboard.", &[&count]),
            (_, n) => tf("{} elements to the clipboard; {} left out (levels, views and sheets aren't copied).", &[&count, &n.to_string()]),
        });
        true
    }

    /// The text a copy left for the system clipboard, once.
    pub fn take_clip_out(&mut self) -> Option<String> {
        self.clip_out.take()
    }

    /// A paste in `mode` is asked for: the window reads the system clipboard and hands its text to [`App::pasted_text`].
    pub fn paste_request(&mut self, mode: Mode) {
        self.paste_asked = Some(mode);
    }

    /// The paste asked for, once.
    pub fn take_paste_asked(&mut self) -> Option<Mode> {
        self.paste_asked.take()
    }

    /// A paste of `text` (the system clipboard's; none, or not BIMCraft's: this window's own copy), placed as `mode` says.
    pub fn pasted_text(&mut self, text: Option<String>, mode: Mode) {
        let text = match text {
            Some(text) if core_clip::is_clipboard(&text) => Some(text),
            _ => self.clip_text.clone(),
        };
        let Some(text) = text else {
            self.message = Some(t("the clipboard holds nothing to paste: copy elements first (Ctrl+C)").to_string());
            return;
        };
        let read = match core_clip::read(&text) {
            Ok(read) => read,
            Err(why) => {
                self.message = Some(why);
                return;
            }
        };
        let kind = self.view;
        match mode {
            Mode::Manual if clip::by_hand(kind) => {
                self.pasting = Some(text);
                self.message = Some(t("Click where the pasted elements go. Esc cancels.").to_string());
            }
            // By hand needs a plan to follow the pointer in: elsewhere it is what Ctrl+V does there.
            Mode::Manual => self.pasted_text(Some(text), clip::ctrl_v(kind)),
            Mode::SamePlace => {
                self.paste_with(&text, json!({"mode": "same_place"}));
            }
            Mode::CurrentView if clip::frame(&self.session.model, kind).is_some() => {
                self.paste_with(&text, json!({"mode": "current_view"}));
            }
            Mode::CurrentView => self.message = Some(t("Aligned to Current View needs a view to paste into.").to_string()),
            Mode::SelectedLevels => {
                let offered = clip::levels_of(&self.session.model);
                self.dialogs.choose = Some(Choose { text, views: false, offered, picked: BTreeSet::new() });
            }
            Mode::SelectedViews => {
                let offered = clip::views_like(&self.session.model, &read);
                self.dialogs.choose = Some(Choose { text, views: true, offered, picked: BTreeSet::new() });
            }
        }
    }

    /// A click with a paste waiting to be put down by hand, at window point `to`: the middle of what is pasted goes there, in the
    /// view's terms. False when no paste waits.
    pub fn paste_at(&mut self, to: (f32, f32)) -> bool {
        let Some(text) = self.pasting.take() else { return false };
        let Some(view) = clip::view_id(self.view) else { return true };
        let p = self.camera.view(self.size.0, self.size.1).mm(to);
        if p.0.is_finite() && p.1.is_finite() {
            self.paste_with(&text, json!({"mode": "manual", "view": view, "at": [p.0, p.1]}));
        }
        true
    }

    /// The row of `id` in the list of levels or views a paste goes onto: ticked, or the tick taken off.
    pub fn choose_toggle(&mut self, id: Id) {
        if let Some(c) = self.dialogs.choose.as_mut() {
            c.toggle(id);
        }
    }

    /// Paste onto what is ticked. The list stays open when nothing is.
    pub fn choose_paste(&mut self) -> bool {
        let Some(c) = self.dialogs.choose.take() else { return false };
        if c.picked.is_empty() {
            self.message = Some(t("Tick at least one.").to_string());
            self.dialogs.choose = Some(c);
            return false;
        }
        let params = c.params();
        self.paste_with(&c.text, params)
    }

    /// `paste` with `p`'s mode and places, into the view on screen unless `p` says one; the pasted elements are the selection, and what
    /// the core left out is said.
    fn paste_with(&mut self, text: &str, mut p: Value) -> bool {
        p["clipboard"] = json!(text);
        if p.get("view").is_none()
            && let Some(view) = clip::view_value(self.view)
        {
            p["view"] = view;
        }
        let reply = match self.session.execute("paste", &p) {
            Ok(reply) => reply,
            Err(why) => {
                self.message = Some(why);
                return false;
            }
        };
        let ids: Vec<Id> = reply["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        let left: Vec<String> =
            reply["left_out"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        self.settle();
        self.select(ids.clone());
        self.message = Some(match left.first() {
            None => tf("Pasted {} elements.", &[&ids.len().to_string()]),
            Some(why) => tf("Pasted {} elements; {} left out: {}", &[&ids.len().to_string(), &left.len().to_string(), why]),
        });
        true
    }
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
