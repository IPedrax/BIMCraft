//! The discipline dialogs' slice of `App` (`discipline.rs` has the dialogs as data, `ui/discipline.rs` draws them): opening one on the
//! project, the boxes typed in, the list that is open, and a press, which a dialog answers through `App` as its `Host`. The dialog is taken
//! out of `Dialogs` while it works, so it may run commands (which need all of `App`) and is put back unless it asked to close.

use std::path::PathBuf;

use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use serde_json::Value;

use super::App;
use crate::discipline::{Discipline, Host, Kind, Outcome};

/// A discipline dialog open, and the list of it that is open.
pub struct Open {
    pub dialog: Box<dyn Discipline>,
    pub open: Option<usize>,
}

impl Host for App {
    fn model(&self) -> &Model {
        &self.session.model
    }

    fn selection(&self) -> &[Id] {
        &self.selection
    }

    fn say(&mut self, text: String) {
        self.message = Some(text);
    }

    fn execute(&mut self, name: &str, params: &Value) -> Result<Value, String> {
        App::execute(self, name, params)
    }

    fn batch(&mut self, label: &str, commands: &[(&'static str, Value)]) -> Result<Vec<Value>, String> {
        let made = self
            .session
            .transaction(label, |s| commands.iter().map(|(name, params)| s.execute(name, params)).collect::<Result<Vec<Value>, String>>());
        if made.is_ok() {
            self.message = None;
            self.settle();
        }
        made
    }
}

impl App {
    /// Opens a discipline dialog on the project, the selection saying which host. Says why in the status bar when it cannot be opened.
    pub fn open_discipline(&mut self, kind: Kind) -> bool {
        let mut dialog = match kind.open(&self.session.model, &self.selection) {
            Ok(dialog) => dialog,
            Err(why) => {
                self.message = Some(why);
                return false;
            }
        };
        self.close_menu();
        self.dialogs.clear();
        // What the dialog shows at once is asked of the project.
        dialog.opened(self);
        self.dialogs.discipline = Some(Open { dialog, open: None });
        true
    }

    /// Opens or closes list `pick` of the open dialog.
    pub fn discipline_toggle(&mut self, pick: usize) {
        if let Some(o) = self.dialogs.discipline.as_mut() {
            o.open = if o.open == Some(pick) { None } else { Some(pick) };
        }
    }

    pub fn discipline_close_list(&mut self) {
        if let Some(o) = self.dialogs.discipline.as_mut() {
            o.open = None;
        }
    }

    /// Choice `option` of list `pick` taken.
    pub fn discipline_choose(&mut self, pick: usize, option: usize) {
        let Some(mut o) = self.dialogs.discipline.take() else { return };
        o.open = None;
        o.dialog.choose(pick, option, self);
        self.dialogs.discipline = Some(o);
    }

    /// A button pressed: what the dialog does with it. True while the dialog is still open.
    pub fn discipline_press(&mut self, action: &str, arg: usize) -> bool {
        let Some(mut o) = self.dialogs.discipline.take() else { return false };
        o.open = None;
        let outcome = o.dialog.press(action, arg, self);
        if outcome == Outcome::Keep {
            self.dialogs.discipline = Some(o);
        }
        outcome == Outcome::Keep
    }

    /// The system's dialog answered a button that asked for a file.
    pub fn discipline_saved(&mut self, action: &str, path: PathBuf) -> bool {
        let Some(mut o) = self.dialogs.discipline.take() else { return false };
        let outcome = o.dialog.saved(action, path, self);
        if outcome == Outcome::Keep {
            self.dialogs.discipline = Some(o);
        }
        outcome == Outcome::Keep
    }
}

#[cfg(test)]
#[path = "discipline_tests.rs"]
mod tests;
