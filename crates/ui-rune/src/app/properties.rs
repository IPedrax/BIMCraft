//! The Properties palette's slice of `App`: the panel for what is selected (`props::panel`), the text being typed into each field, what
//! is folded and which list is open, and the handlers a row's editor calls. A change goes to the core as the command `props::apply`
//! makes of it, through `App::run` like every other; a refusal is shown under the title of the palette and in the status bar.

use std::collections::BTreeSet;

use bimcraft_core::model::Id;
use rune::TextEdit;

use super::App;
use crate::lang::{t, tf};
use crate::props::apply::{self, Command, Edit};
use crate::props::panel::{Editor, Panel};
use crate::typeprops;
use crate::views::ViewKind;

/// A field of the palette with the text being typed into it.
pub struct Field {
    pub key: &'static str,
    pub edit: TextEdit,
}

pub struct Palette {
    /// What the palette shows: made again whenever the selection or the model changes.
    pub panel: Panel,
    /// The text rows, by their field's key.
    pub fields: Vec<Field>,
    /// The groups folded, by their English name: kept across selections, as egui's are.
    pub folded: BTreeSet<&'static str>,
    /// The row whose list is open, by its field's key.
    pub open: Option<&'static str>,
    /// The category the palette is narrowed to, and the selection it was picked for: another selection starts it over.
    pub filter: Option<(&'static str, Vec<Id>)>,
    /// Why the last edit was refused, as the palette says it under its title.
    pub refusal: Option<String>,
    /// Where an edit goes that has no field (a stale one): never read.
    pub scratch: TextEdit,
}

impl Palette {
    pub fn new() -> Palette {
        Palette {
            panel: Panel::empty(),
            fields: Vec::new(),
            folded: BTreeSet::new(),
            open: None,
            filter: None,
            refusal: None,
            scratch: TextEdit::new(""),
        }
    }
}

impl Default for Palette {
    fn default() -> Palette {
        Palette::new()
    }
}

impl App {
    /// The palette for the selection as it is now, with a field for each of its text rows. Called whenever the selection or the model
    /// changes.
    pub(super) fn refresh_panel(&mut self) {
        let narrowed = self.palette.filter.as_ref().filter(|(_, picked)| *picked == self.selection).map(|(category, _)| *category);
        if narrowed.is_none() {
            self.palette.filter = None;
        }
        // The plan's own properties are a floor plan's: another view on screen (the level stays the last plan's) has none to show.
        let plan = match self.view {
            ViewKind::Plan(id) if self.session.model.level(id).is_some() => Some(id),
            _ => None,
        };
        let panel = Panel::of(&self.session.model, &self.selection, narrowed, plan);
        self.palette.fields = panel
            .all_rows()
            .filter(|r| matches!(r.editor, Editor::Text { .. }))
            .map(|r| Field { key: r.key, edit: TextEdit::new(r.shown.clone()) })
            .collect();
        self.palette.panel = panel;
        self.palette.open = None;
        self.palette.refusal = None;
    }

    /// Narrows the palette to one category of a mixed selection, or back to all of it.
    pub fn set_filter(&mut self, category: Option<&'static str>) {
        self.palette.filter = category.map(|c| (c, self.selection.clone()));
        self.refresh_panel();
    }

    /// Folds a group, or opens it again.
    pub fn fold(&mut self, group: &'static str) {
        if !self.palette.folded.remove(group) {
            self.palette.folded.insert(group);
        }
    }

    /// Opens the list of row `key`, or closes it when it is the one open.
    pub fn open_list(&mut self, key: &'static str) {
        self.palette.open = if self.palette.open == Some(key) { None } else { Some(key) };
    }

    /// Runs the edit of row `key`; a value that is none, or a refusal, is said and changes nothing. True when the core took it.
    fn edit_row(&mut self, key: &'static str, edit: Edit) -> bool {
        let made = apply::command(&self.palette.panel, self.session.model.units, key, edit);
        self.carry_out(made)
    }

    /// Runs what an edit came to, if it came to a command; else says why it did not. True when the core took it.
    fn carry_out(&mut self, made: Result<Option<Command>, String>) -> bool {
        let refused = match made {
            Ok(Some(command)) => {
                if self.run(command.name, &command.params) {
                    return true;
                }
                self.message.clone()
            }
            Ok(None) => return false,
            Err(why) => Some(why),
        };
        self.palette.refusal = refused.clone();
        self.message = refused;
        false
    }

    /// The text typed into field `i` goes to the core; a number that is no number, or a refusal, is said and changes nothing.
    pub fn commit_field(&mut self, i: usize) -> bool {
        let Some(field) = self.palette.fields.get(i) else { return false };
        let (key, typed) = (field.key, field.edit.text.clone());
        self.edit_row(key, Edit::Text(&typed))
    }

    /// A check box of row `key` was set to `on`.
    pub fn set_check(&mut self, key: &'static str, on: bool) -> bool {
        self.edit_row(key, Edit::Check(on))
    }

    /// Option `i` of the list of row `key` was picked: its list closes.
    pub fn choose_row(&mut self, key: &'static str, i: usize) -> bool {
        self.palette.open = None;
        self.edit_row(key, Edit::Choice(i))
    }

    /// Type `i` of the Type Selector's list was picked: its list closes, and the shown elements are of that type.
    pub fn pick_type(&mut self, i: usize) -> bool {
        self.palette.open = None;
        let made = apply::set_type(&self.palette.panel, i);
        self.carry_out(made)
    }

    /// Edit Type: opens Type Properties (`app/types.rs`) on the type of element `id`, the one the palette shows or the first of the
    /// category it shows; says in the status bar when that element has no type.
    pub fn edit_type(&mut self, id: Id) {
        match typeprops::of_element(&self.session.model, id) {
            Some(key) => {
                self.open_type(key);
            }
            None => self.message = Some(t("Select one element that has a type first").to_string()),
        }
    }

    /// A link of the palette was clicked: the element it points at is all that is selected.
    pub fn select_link(&mut self, id: Id) {
        if self.session.model.elements.get(id).is_some() {
            self.select(vec![id]);
        }
    }

    /// What a dialog does that this window does not have yet (the Type Properties dialog, View Range): said in the palette, until the
    /// dialogs session's window opens it.
    pub fn seam(&mut self, what: &str) {
        self.palette.refusal = Some(tf("{} is not in this window yet", &[what]));
    }
}
