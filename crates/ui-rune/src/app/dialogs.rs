//! The dialogs' state and what their handlers do, a slice of `App` (the elements are `ui/dialog.rs` and `ui/find.rs`): Find, and the
//! generic form of a command (`forms.rs`). A form is run through `Session::execute`; a refusal, or the form's own complaint about what
//! was typed, is kept here and said inside the dialog (a status line can sit under a modal's buttons), and what a command replied is
//! shown in the dialog too.

use bimcraft_core::engine;
use rune::TextEdit;
use serde_json::Value;

use super::App;
use crate::finder::{self, Found, Hit};
use crate::forms::{self, Form};

/// The most characters of a command's reply the dialog shows.
const MAX_REPLY: usize = 4_000;
/// The most characters of a refusal the dialog shows.
const MAX_REFUSAL: usize = 2_000;

/// What is typed in one field of a form: text for a field that is typed, and the choice for one that is chosen.
pub struct Entry {
    pub edit: TextEdit,
    pub choice: String,
}

/// A command's form open in a dialog.
pub struct FormState {
    pub form: Form,
    /// One for each of `form.fields`.
    pub entries: Vec<Entry>,
    /// The box for more parameters, as JSON (shown when `form.rest`).
    pub rest: TextEdit,
    /// The field whose choices are listed.
    pub open: Option<usize>,
    /// What stopped the last Run: the form's complaint or the core's refusal.
    pub error: Option<String>,
    /// What the last Run replied, as JSON.
    pub reply: Option<String>,
}

/// Find open: what is typed and the row the arrow keys are on.
pub struct FindState {
    pub edit: TextEdit,
    pub at: usize,
}

#[derive(Default)]
pub struct Dialogs {
    pub form: Option<FormState>,
    pub find: Option<FindState>,
    /// The Settings dialog (`app/settings.rs`).
    pub settings: Option<super::settings::SettingsDialog>,
    /// Type Properties (`app/types.rs`).
    pub types: Option<super::types::TypeDialog>,
    /// The levels or views a paste goes onto, being ticked (`app/clip.rs`).
    pub choose: Option<crate::clip::Choose>,
    /// Where an edit goes when its field is gone (a stale handler): never read.
    pub scratch: TextEdit,
}

impl Dialogs {
    /// A dialog is open: the keys and pointer are its, not the plan's.
    pub fn any(&self) -> bool {
        self.form.is_some() || self.find.is_some() || self.settings.is_some() || self.types.is_some() || self.choose.is_some()
    }

    /// Closes every dialog: they are open one at a time.
    pub fn clear(&mut self) {
        (self.form, self.find, self.settings, self.types, self.choose) = (None, None, None, None, None);
    }

    /// The box of field `i` of the open Type Properties.
    pub fn type_edit(&mut self, i: usize) -> &mut TextEdit {
        match self.types.as_mut().and_then(|d| d.fields.get_mut(i)) {
            Some(f) => &mut f.edit,
            None => &mut self.scratch,
        }
    }

    /// The box Find types into.
    pub fn find_edit(&mut self) -> &mut TextEdit {
        match self.find.as_mut() {
            Some(f) => &mut f.edit,
            None => &mut self.scratch,
        }
    }

    /// The box of field `i` of the open form (`None` for the rest box).
    pub fn field_edit(&mut self, i: Option<usize>) -> &mut TextEdit {
        match (self.form.as_mut(), i) {
            (Some(f), None) => &mut f.rest,
            (Some(f), Some(i)) => f.entries.get_mut(i).map_or(&mut self.scratch, |e| &mut e.edit),
            (None, _) => &mut self.scratch,
        }
    }
}

impl App {
    /// Ctrl+K: opens Find, or closes it when it is open.
    pub fn toggle_find(&mut self) {
        if self.dialogs.find.take().is_some() {
            return;
        }
        // The field starts focused, so what is typed goes to it.
        let mut edit = TextEdit::new("");
        edit.focused = true;
        self.close_menu();
        self.dialogs.clear();
        self.dialogs.find = Some(FindState { edit, at: 0 });
    }

    /// What Find lists now.
    pub fn found(&self) -> Vec<Hit> {
        self.dialogs.find.as_ref().map(|f| finder::find(&f.edit.text, &self.levels)).unwrap_or_default()
    }

    /// The arrow keys: the row Find is on moves by `by`, kept inside the list.
    pub fn find_step(&mut self, by: isize) {
        let n = self.found().len();
        if let Some(f) = self.dialogs.find.as_mut() {
            f.at = if n == 0 { 0 } else { f.at.saturating_add_signed(by).min(n - 1) };
        }
    }

    /// Does what row `i` of Find says: puts a tool in hand, opens a command's form or shows a level. Find closes.
    pub fn pick_found(&mut self, i: usize) {
        let Some(hit) = self.found().into_iter().nth(i) else { return };
        self.dialogs.find = None;
        match hit.found {
            Found::Tool(id) => {
                if self.tool != Some(id) {
                    self.set_tool(id);
                }
            }
            Found::Command(id) => {
                self.open_command(id);
            }
            Found::Level(id) => self.set_level(id),
            Found::Settings => self.toggle_settings(),
            Found::TypeProperties => {
                self.open_type_of_selection();
            }
        }
    }

    /// Enter in Find: the row the arrow keys are on.
    pub fn pick_current(&mut self) {
        let at = self.dialogs.find.as_ref().map_or(0, |f| f.at);
        self.pick_found(at.min(self.found().len().saturating_sub(1)));
    }

    /// What is typed in Find changed: the arrow keys start at the first row again.
    pub fn find_reset(&mut self) {
        if let Some(f) = self.dialogs.find.as_mut() {
            f.at = 0;
        }
    }

    /// Opens the form of the command with this id or shortcut; false for one the core does not have.
    pub fn open_command(&mut self, name: &str) -> bool {
        let Some(form) = Form::named(name) else { return false };
        let mut entries: Vec<Entry> =
            form.fields.iter().map(|f| Entry { edit: TextEdit::new(f.default.clone()), choice: String::new() }).collect();
        // The first field that is typed takes the keys.
        if let Some(i) = form.fields.iter().position(|f| !f.kind.chosen()) {
            entries[i].edit.focused = true;
        }
        self.close_menu();
        self.dialogs.clear();
        self.dialogs.form = Some(FormState { form, entries, rest: TextEdit::new(""), open: None, error: None, reply: None });
        true
    }

    /// Closes whatever dialog is open.
    pub fn close_dialogs(&mut self) {
        self.dialogs.clear();
    }

    /// Opens the choices of field `i`, or closes them when they are open.
    pub fn toggle_choices(&mut self, i: usize) {
        if let Some(f) = self.dialogs.form.as_mut() {
            f.open = if f.open == Some(i) { None } else { Some(i) };
        }
    }

    /// Field `i` takes this choice (empty: none), and its choices close.
    pub fn choose(&mut self, i: usize, choice: String) {
        if let Some(f) = self.dialogs.form.as_mut() {
            if let Some(e) = f.entries.get_mut(i) {
                e.choice = choice;
            }
            f.open = None;
        }
    }

    pub fn close_choices(&mut self) {
        if let Some(f) = self.dialogs.form.as_mut() {
            f.open = None;
        }
    }

    /// Run: what is typed read as the command's parameters, and the command run through the engine. The dialog stays open with what
    /// the command replied, or with the reason it did not run. True when it ran.
    pub fn run_form(&mut self) -> bool {
        let Some(f) = self.dialogs.form.as_mut() else { return false };
        let texts: Vec<String> = f
            .form
            .fields
            .iter()
            .zip(&f.entries)
            .map(|(field, e)| if field.kind.chosen() { e.choice.clone() } else { e.edit.text.clone() })
            .collect();
        let id = f.form.id;
        let params = match f.form.params(&texts, &f.rest.text) {
            Ok(params) => params,
            Err(why) => {
                (f.error, f.reply) = (Some(why), None);
                return false;
            }
        };
        let outcome = self.execute(id, &params);
        let Some(f) = self.dialogs.form.as_mut() else { return false };
        match outcome {
            Ok(reply) => {
                let shown = serde_json::to_string_pretty(&reply).unwrap_or_default();
                (f.error, f.reply) = (None, Some(forms::cut(&shown, MAX_REPLY)));
                true
            }
            Err(refusal) => {
                (f.error, f.reply) = (Some(forms::cut(&refusal, MAX_REFUSAL)), None);
                false
            }
        }
    }

    /// Runs a command for a dialog: like `run`, but a refusal is the dialog's to say, not the status bar's, and the command's reply
    /// comes back. A command that replaces the project starts the view again, as Open does.
    pub(super) fn execute(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let reply = self.session.execute(id, params)?;
        self.message = None;
        if engine::REPLACES.contains(&id) {
            (self.fitted, self.moved) = (false, false);
            self.history.clear();
            self.picked.clear();
            self.selection.clear();
        }
        self.settle();
        Ok(reply)
    }
}

#[cfg(test)]
mod tests;
