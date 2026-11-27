//! The discipline dialogs (Reinforce, Quantities, Electrical Settings, a stair type's page, a curtain wall's pattern): egui has each as a
//! window of its own; here each is a [`Discipline`], the dialog described as data, and one widget (`ui/discipline.rs`) draws every one. A
//! dialog says what is on it (sections of rows of cells: words, boxes to type in, lists to choose from, buttons), keeps what is typed, and
//! does what a button asks through a [`Host`], which is the `Session` (a refusal comes back as words, said inside the dialog). So a dialog
//! is tested with no window: press, read the commands it made, read what it says.
//!
//! The pure parts that egui keeps inside its window crate (which fields a kind of host has, how a field is read) are copied into each
//! dialog's module; `ui-egui` is left as it is. Commands that already do the job through the generic form (`forms.rs`) stay there: a
//! dialog here is only for what egui has a dialog of its own for.

use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use rune::TextEdit;
use serde_json::Value;

pub mod quantities;
pub mod reinforce;

/// A cell of a row.
pub enum Cell {
    /// Words: a label, a number read from a reply, a note. `strong` for a heading.
    Text { text: String, strong: bool, width: f32 },
    /// A box typed in: the place of its text in the dialog's edits, its name for a screen reader and what it shows while empty.
    Field { edit: usize, name: String, hint: String, width: f32 },
    /// A list to choose from: the place of its choices in `Discipline::choice`.
    Pick { pick: usize, width: f32 },
    /// A button: what it asks of `Discipline::press` (an action and which row it is of), its words and whether it can be pressed now.
    Button { action: &'static str, arg: usize, label: String, on: bool },
    /// A check box: pressing it asks `Discipline::press` for `action` (which turns it), its words and whether it can be changed now.
    Check { action: &'static str, label: String, on: bool, enabled: bool },
}

impl Cell {
    pub fn text(text: impl Into<String>, width: f32) -> Cell {
        Cell::Text { text: text.into(), strong: false, width }
    }

    pub fn head(text: impl Into<String>, width: f32) -> Cell {
        Cell::Text { text: text.into(), strong: true, width }
    }

    pub fn field(edit: usize, name: impl Into<String>, hint: impl Into<String>, width: f32) -> Cell {
        Cell::Field { edit, name: name.into(), hint: hint.into(), width }
    }

    pub fn button(action: &'static str, arg: usize, label: impl Into<String>, on: bool) -> Cell {
        Cell::Button { action, arg, label: label.into(), on }
    }
}

/// A titled group of rows, each a row of cells side by side.
pub struct Section {
    pub title: String,
    pub rows: Vec<Vec<Cell>>,
}

impl Section {
    pub fn new(title: impl Into<String>, rows: Vec<Vec<Cell>>) -> Section {
        Section { title: title.into(), rows }
    }
}

/// What a list offers: its name for a screen reader, the choices and the one in force.
pub struct Choice {
    pub name: String,
    pub options: Vec<String>,
    pub current: Option<usize>,
}

/// A button of the dialog's foot: the action it asks, its words, and whether it is the one that goes ahead.
pub struct Foot {
    pub action: &'static str,
    pub label: String,
    pub primary: bool,
    pub on: bool,
    /// A file the button asks the system's dialog for before it does anything (the name it suggests); `Discipline::saved` is told where.
    pub file: Option<&'static str>,
}

impl Foot {
    pub fn new(action: &'static str, label: impl Into<String>, primary: bool) -> Foot {
        Foot { action, label: label.into(), primary, on: true, file: None }
    }
}

/// What a press does to the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Keep,
    Close,
}

/// What a discipline dialog needs of the window: the project, what is selected, and the engine. A refusal is the core's words.
pub trait Host {
    fn model(&self) -> &Model;
    fn selection(&self) -> &[Id];
    /// Says something in the status bar.
    fn say(&mut self, text: String);
    /// One command.
    fn execute(&mut self, name: &str, params: &Value) -> Result<Value, String>;
    /// Several commands as one undo step, all or nothing; what each made.
    fn batch(&mut self, label: &str, commands: &[(&'static str, Value)]) -> Result<Vec<Value>, String>;
}

/// A dialog described as data.
pub trait Discipline {
    fn title(&self) -> String;
    /// A line under the title.
    fn about(&self) -> String {
        String::new()
    }
    fn width(&self) -> f32 {
        720.0
    }
    /// The pages, when the dialog has them, and the one shown. A page is chosen by `press("tab", page)`.
    fn tabs(&self) -> Vec<String> {
        Vec::new()
    }
    fn tab(&self) -> usize {
        0
    }
    /// The dialog has just been opened: what it shows at once is asked of the project here.
    fn opened(&mut self, _host: &mut dyn Host) {}
    fn sections(&self) -> Vec<Section>;
    fn edit(&self, i: usize) -> Option<&TextEdit>;
    fn edit_mut(&mut self, i: usize) -> Option<&mut TextEdit>;
    fn choice(&self, _pick: usize) -> Option<Choice> {
        None
    }
    /// Choice `option` of list `pick` was taken.
    fn choose(&mut self, _pick: usize, _option: usize, _host: &mut dyn Host) {}
    /// The system's dialog answered a button that asked for a file (`Foot::file`): where to write.
    fn saved(&mut self, _action: &str, _path: std::path::PathBuf, _host: &mut dyn Host) -> Outcome {
        Outcome::Keep
    }
    /// A button was pressed (a cell's or the foot's). What it asks of the project goes through `host`.
    fn press(&mut self, action: &str, arg: usize, host: &mut dyn Host) -> Outcome;
    fn foot(&self) -> Vec<Foot>;
    /// What a command said besides what it did: the check's notes, a summary.
    fn notes(&self) -> Vec<String> {
        Vec::new()
    }
    /// What stopped the last press: the dialog's own complaint or the core's refusal.
    fn error(&self) -> Option<&str> {
        None
    }
}

/// The dialogs there are, and how Find and the menus name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Reinforce,
    Materials,
    SteelList,
    CutPlan,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Reinforce, Kind::Materials, Kind::SteelList, Kind::CutPlan];

    /// Its name in Find, and Revit's shortcut where egui has one.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Reinforce => "Reinforce",
            Kind::Materials => "Materials",
            Kind::SteelList => "Steel List",
            Kind::CutPlan => "Cut Plan",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Kind::Reinforce => "RF",
            Kind::Materials => "QT",
            Kind::SteelList => "BL",
            Kind::CutPlan => "CU",
        }
    }

    /// The dialog opened on the project: what is selected says which host. Why not, in words.
    pub fn open(self, m: &Model, selection: &[Id]) -> Result<Box<dyn Discipline>, String> {
        match self {
            Kind::Reinforce => reinforce::Reinforce::open(m, selection).map(|s| Box::new(s) as Box<dyn Discipline>),
            Kind::Materials => Ok(Box::new(quantities::Quantities::open(quantities::Page::Materials, m, selection))),
            Kind::SteelList => Ok(Box::new(quantities::Quantities::open(quantities::Page::List, m, selection))),
            Kind::CutPlan => Ok(Box::new(quantities::Quantities::open(quantities::Page::Plan, m, selection))),
        }
    }
}

/// The most characters a field takes: what is typed is hostile like any text.
pub const MAX_CHARS: usize = 200;

/// The text of a field of a discipline dialog, kept to [`MAX_CHARS`].
pub fn typed(edit: &TextEdit) -> String {
    edit.text.chars().take(MAX_CHARS).collect()
}

/// The edits of a discipline dialog from the texts they start with.
pub fn edits<I: IntoIterator<Item = String>>(texts: I) -> Vec<TextEdit> {
    texts.into_iter().map(TextEdit::new).collect()
}

#[cfg(test)]
mod tests;
