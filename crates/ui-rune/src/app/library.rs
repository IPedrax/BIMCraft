//! The library managers' state and what their handlers do, a slice of `App` (the dialog is `ui/library.rs`, the entries and what a
//! change sends are `library.rs`): an entry of Materials, Line Styles, Phases and the rest chosen from a list, its fields typed in
//! boxes, Apply, Rename and Delete, each one command of the core through `Session::execute`. A refusal is said in the dialog.

use rune::TextEdit;
use serde_json::{Value, json};

use super::App;
use crate::forms;
use crate::lang::t;
use crate::library::{self, Info, Source};
use crate::props::{self, Kind, Row};

/// The most characters of a refusal the dialog shows.
const MAX_REFUSAL: usize = 2_000;

/// One field of the entry that can be typed, with the text being typed in it.
pub struct LibField {
    pub name: String,
    pub kind: Kind,
    pub edit: TextEdit,
}

/// A manager, open on one entry of its source.
pub struct LibDialog {
    pub info: &'static Info,
    /// The names of the source's entries.
    pub names: Vec<String>,
    pub selected: Option<String>,
    /// The entry as the project holds it: what Apply compares the typed fields with.
    pub json: Value,
    pub fields: Vec<LibField>,
    /// Every field of the entry, as the Properties panel shows an element's.
    pub rows: Vec<Row>,
    /// A new name for the entry, when its command renames.
    pub rename: TextEdit,
    /// The list of entries is open.
    pub open: bool,
    pub error: Option<String>,
    /// The last Apply or Delete went through (or had nothing to change).
    pub done: bool,
}

impl App {
    /// Opens the manager of `source` on its first entry.
    pub fn open_library(&mut self, source: Source) {
        self.show_library(source, None, false);
    }

    /// The manager of `source` as the project holds it now, on the entry `pick` when it is there, else the first.
    fn show_library(&mut self, source: Source, pick: Option<String>, done: bool) {
        let info = source.info();
        let entries = library::entries(&self.session.model, source);
        let names: Vec<String> = entries.iter().map(|(name, _)| name.clone()).collect();
        let selected = pick.filter(|p| names.contains(p)).or_else(|| names.first().cloned());
        let json =
            selected.as_ref().and_then(|s| entries.iter().find(|(name, _)| name == s)).map(|(_, json)| json.clone()).unwrap_or(Value::Null);
        let fields = library::editable(info, &json)
            .into_iter()
            .map(|(name, kind, text)| LibField { name, kind, edit: TextEdit::new(text) })
            .collect();
        let rows = props::rows(&json);
        self.dialogs.clear();
        self.dialogs.library =
            Some(LibDialog { info, names, selected, json, fields, rows, rename: TextEdit::new(""), open: false, error: None, done });
    }

    /// Shows another entry of the same source.
    pub fn library_select(&mut self, name: &str) {
        if let Some(source) = self.dialogs.library.as_ref().map(|d| d.info.source) {
            self.show_library(source, Some(name.to_string()), false);
        }
    }

    pub fn toggle_library_list(&mut self) {
        if let Some(d) = self.dialogs.library.as_mut() {
            d.open = !d.open;
        }
    }

    pub fn close_library_list(&mut self) {
        if let Some(d) = self.dialogs.library.as_mut() {
            d.open = false;
        }
    }

    /// New: the command's own form, to name and make an entry.
    pub fn library_new(&mut self) {
        if let Some(command) = self.dialogs.library.as_ref().map(|d| d.info.command) {
            self.open_command(command);
        }
    }

    /// Apply: the fields that differ (and a new name) go to the core in one command; the dialog then shows the entry as the project holds
    /// it. True when it went through, or there was nothing to change.
    pub fn library_apply(&mut self) -> bool {
        let Some(d) = self.dialogs.library.as_mut() else { return false };
        let Some(name) = d.selected.clone() else {
            d.error = Some(t("There is no entry to change: use New").to_string());
            return false;
        };
        let typed: Vec<(String, Kind, String)> = d.fields.iter().map(|f| (f.name.clone(), f.kind, f.edit.text.clone())).collect();
        let renamed = d.rename.text.trim().to_string();
        let params = match library::changes(d.info, &name, &d.json, &typed, &renamed) {
            Ok(Some(params)) => params,
            Ok(None) => {
                (d.error, d.done) = (None, true);
                return true;
            }
            Err(why) => {
                (d.error, d.done) = (Some(why), false);
                return false;
            }
        };
        let (command, source) = (d.info.command, d.info.source);
        match self.execute(command, &params) {
            Ok(_) => {
                self.show_library(source, Some(if renamed.is_empty() { name } else { renamed }), true);
                true
            }
            Err(why) => self.library_refused(why),
        }
    }

    /// Delete: the entry goes, when its command can delete one.
    pub fn library_delete(&mut self) -> bool {
        let Some(d) = self.dialogs.library.as_mut() else { return false };
        let Some(params) = d.selected.as_ref().and_then(|name| library::deletion(d.info, name)) else {
            d.error = Some(t("This kind of entry is not deleted here").to_string());
            return false;
        };
        let (command, source) = (d.info.command, d.info.source);
        match self.execute(command, &params) {
            Ok(_) => {
                self.show_library(source, None, true);
                true
            }
            Err(why) => self.library_refused(why),
        }
    }

    /// Make active: the entry shown becomes the project's active one, when its command can make one so (a location).
    pub fn library_activate(&mut self) -> bool {
        let Some(d) = self.dialogs.library.as_ref() else { return false };
        let Some(name) = d.selected.clone().filter(|_| library::takes(d.info, "activate")) else { return false };
        let (command, source) = (d.info.command, d.info.source);
        match self.execute(command, &json!({"name": name, "activate": true})) {
            Ok(_) => {
                self.show_library(source, Some(name), true);
                true
            }
            Err(why) => self.library_refused(why),
        }
    }

    /// The core's refusal, said in the dialog.
    fn library_refused(&mut self, why: String) -> bool {
        if let Some(d) = self.dialogs.library.as_mut() {
            (d.error, d.done) = (Some(forms::cut(&why, MAX_REFUSAL)), false);
        }
        false
    }
}

#[cfg(test)]
mod tests;
