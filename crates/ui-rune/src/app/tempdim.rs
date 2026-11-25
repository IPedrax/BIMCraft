//! The temporary dimensions in the app: the element selected alone has its dimensions (`tempdim::dims`), each with the text being typed
//! into its number, rebuilt with the Properties rows (`App::refresh`); a number committed goes to the core as the command `tempdim::apply`
//! makes of it. A child of `app`, so it reaches the private handlers (`run`); `app.rs` only declares it and holds the list.

use rune::TextEdit;

use super::App;
use crate::tempdim::{self, Dim};

/// A dimension and the number being typed into it.
pub struct TempField {
    pub dim: Dim,
    pub edit: TextEdit,
}

impl App {
    /// Makes the dimensions of the element selected alone, each with its length as the text to edit; none for any other selection.
    pub(super) fn refresh_dims(&mut self) {
        let m = &self.session.model;
        self.temp = match self.selection.as_slice() {
            &[id] => tempdim::dims(m, id).into_iter().map(|dim| TempField { edit: TextEdit::new(m.field(dim.length)), dim }).collect(),
            _ => Vec::new(),
        };
    }

    /// The dimensions to show now: while no tool is in hand and no end is being dragged.
    pub fn dims_shown(&self) -> &[TempField] {
        if self.tool.is_none() && !self.gripping() { self.temp.as_slice() } else { &[] }
    }

    /// The number typed into dimension `i` is committed: the element moves or changes to measure it. A number that is no length, or that
    /// the core refuses, is said in the status bar and changes nothing. False when nothing was done.
    pub fn commit_dim(&mut self, i: usize) -> bool {
        let Some(f) = self.temp.get(i) else { return false };
        match tempdim::apply(&self.session.model, &f.dim, &f.edit.text) {
            Ok(Some((name, params))) => self.run(name, &params),
            Ok(None) => false,
            Err(why) => {
                self.message = Some(why);
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "tempdim_tests.rs"]
mod tests;
