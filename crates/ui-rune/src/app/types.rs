//! Type Properties' state and what its handlers do, a slice of `App` (the dialog is `ui/types.rs`, the types as a form are
//! `typeprops.rs`): the selected element's type, its scalar fields typed in boxes, and Apply, which sends the `type` command only
//! the fields that differ. A refusal, or a field that is no number, is said in the dialog.

use rune::TextEdit;
use serde_json::Value;

use super::App;
use crate::forms;
use crate::lang::t;
use crate::props::{Kind, Row};
use crate::typeprops::{self, TypeKey};

/// The most characters of a refusal the dialog shows.
const MAX_REFUSAL: usize = 2_000;

/// One field of the type that can be typed, with the text being typed in it.
pub struct TypeField {
    pub name: String,
    pub kind: Kind,
    pub edit: TextEdit,
}

/// Type Properties, open on one type.
pub struct TypeDialog {
    pub key: TypeKey,
    /// The names of the category's types: what the dialog can show instead.
    pub names: Vec<String>,
    /// The type as the project holds it: what Apply compares the typed fields with.
    pub json: Value,
    pub fields: Vec<TypeField>,
    /// Every field of the type, as the Properties panel shows an element's.
    pub rows: Vec<Row>,
    /// The list of types is open.
    pub open: bool,
    pub error: Option<String>,
    /// The last Apply went through (or had nothing to change).
    pub done: bool,
}

impl App {
    /// Opens Type Properties on `key`; false when the project has no such type.
    pub fn open_type(&mut self, key: TypeKey) -> bool {
        let Some(f) = typeprops::fields(&self.session.model, &key) else { return false };
        let names = typeprops::names(&self.session.model, &key.category);
        let fields = f.editable.into_iter().map(|(name, kind, text)| TypeField { name, kind, edit: TextEdit::new(text) }).collect();
        self.close_menu();
        self.dialogs.clear();
        self.dialogs.types = Some(TypeDialog { key, names, json: f.json, fields, rows: f.rows, open: false, error: None, done: false });
        true
    }

    /// Edit Type: Type Properties on the type of the selected element. Says in the status bar when that is not one element with a type.
    pub fn open_type_of_selection(&mut self) -> bool {
        let key = match self.selection.as_slice() {
            [id] => typeprops::of_element(&self.session.model, *id),
            _ => None,
        };
        match key {
            Some(key) => self.open_type(key),
            None => {
                self.message = Some(t("Select one element that has a type first").to_string());
                false
            }
        }
    }

    /// Shows another type of the same category.
    pub fn switch_type(&mut self, name: &str) -> bool {
        let Some(d) = &self.dialogs.types else { return false };
        let key = TypeKey { category: d.key.category.clone(), name: name.to_string() };
        self.open_type(key)
    }

    pub fn toggle_type_list(&mut self) {
        if let Some(d) = self.dialogs.types.as_mut() {
            d.open = !d.open;
        }
    }

    pub fn close_type_list(&mut self) {
        if let Some(d) = self.dialogs.types.as_mut() {
            d.open = false;
        }
    }

    /// Apply: the fields that differ from the type go to the core in one `type` command, which changes the type everywhere it is used.
    /// The dialog then shows the type as the project now holds it. True when it went through, or there was nothing to change.
    pub fn apply_type(&mut self) -> bool {
        let Some(d) = self.dialogs.types.as_mut() else { return false };
        let typed: Vec<(String, Kind, String)> = d.fields.iter().map(|f| (f.name.clone(), f.kind, f.edit.text.clone())).collect();
        let params = match typeprops::changes(&d.key, &d.json, &typed) {
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
        let key = d.key.clone();
        match self.execute("type", &params) {
            Ok(_) => {
                self.open_type(key);
                if let Some(d) = self.dialogs.types.as_mut() {
                    d.done = true;
                }
                true
            }
            Err(why) => {
                if let Some(d) = self.dialogs.types.as_mut() {
                    (d.error, d.done) = (Some(forms::cut(&why, MAX_REFUSAL)), false);
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests;
