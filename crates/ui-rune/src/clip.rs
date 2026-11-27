//! Revit's Clipboard panel without a window: Copy to Clipboard (Ctrl+C), Cut (Ctrl+X) and Paste (Ctrl+V) with the Aligned options, after
//! egui's `clipboard.rs`. What a copy is, a paste's modes and where each fits are the core's (`bimcraft_core::clipboard`); this is
//! what the window asks of it: the view on screen as the commands take it, the mode a paste has, the views Aligned to Selected Views
//! offers, and the list a person ticks. [Copy and paste](../../../docs/areas/copy-paste.md).

use std::collections::BTreeSet;

use bimcraft_core::clipboard::{Clip, Frame};
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model};
use serde_json::{Value, json};

use crate::views::ViewKind;

/// Revit's ways of pasting that this window has. Aligned to Picked Level (a click on a level line of a section) is not here yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The copy follows the pointer in a plan and a click puts it down.
    Manual,
    SelectedLevels,
    SelectedViews,
    CurrentView,
    SamePlace,
}

impl Mode {
    /// The English name egui's Clipboard panel gives it.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Manual => "Paste from Clipboard",
            Mode::SelectedLevels => "Aligned to Selected Levels",
            Mode::SelectedViews => "Aligned to Selected Views",
            Mode::CurrentView => "Aligned to Current View",
            Mode::SamePlace => "Aligned to Same Place",
        }
    }
}

/// The view on screen as a copy is made in and a paste goes into it: a plan, a section or an elevation, a sheet, a 3D view that has
/// been made. None for a schedule, and for a project with nothing to show.
pub fn view_id(kind: ViewKind) -> Option<Id> {
    match kind {
        ViewKind::Plan(k) | ViewKind::Ceiling(k) | ViewKind::Section(k) | ViewKind::Sheet(k) => Some(k),
        ViewKind::View3D(id) => id,
        _ => None,
    }
}

/// The view on screen as `paste` takes it: the 3D view by its name where nothing has made it yet.
pub fn view_value(kind: ViewKind) -> Option<Value> {
    match (view_id(kind), kind) {
        (Some(id), _) => Some(json!(id)),
        (None, ViewKind::View3D(None)) => Some(json!("{3D}")),
        _ => None,
    }
}

/// The frame of the view on screen: what a paste measures from and into.
pub fn frame(m: &Model, kind: ViewKind) -> Option<Frame> {
    match kind {
        ViewKind::View3D(_) => Some(Frame::ThreeD),
        _ => view_id(kind).and_then(|v| Frame::of(m, v)),
    }
}

/// Whether the view on screen is one the copy can follow the pointer in: a plan, a ceiling plan.
pub fn by_hand(kind: ViewKind) -> bool {
    matches!(kind, ViewKind::Plan(_) | ViewKind::Ceiling(_))
}

/// What Ctrl+V does in the view on screen: by hand in a plan; in a section, an elevation or a sheet where the elements were in their
/// view (Aligned to Current View); everywhere else where they were (Aligned to Same Place), as egui's does where it cannot ask.
pub fn ctrl_v(kind: ViewKind) -> Mode {
    match kind {
        ViewKind::Plan(_) | ViewKind::Ceiling(_) => Mode::Manual,
        ViewKind::Section(_) | ViewKind::Sheet(_) => Mode::CurrentView,
        _ => Mode::SamePlace,
    }
}

/// The views Aligned to Selected Views offers: those like the ones the view-specific elements were copied from (plans for a plan,
/// sections and elevations looking the same way for a section or an elevation, sheets for a sheet), by name.
pub fn views_like(m: &Model, c: &Clip) -> Vec<(Id, String)> {
    let from: Vec<&Frame> = c.source.views.values().collect();
    let like = |f: &Frame| {
        from.iter().any(|g| match (g, f) {
            (Frame::Sheet, Frame::Sheet) => true,
            _ => g.same_orientation(f),
        })
    };
    let mut views: Vec<(Id, String)> = m
        .elements
        .iter()
        .filter(|(_, e)| {
            matches!(e, Element::Level(_) | Element::PlanView(_) | Element::Section(_) | Element::Elevation(_) | Element::Sheet(_))
        })
        .filter_map(|(id, e)| {
            let f = Frame::of(m, *id)?;
            let name = match e {
                Element::Sheet(s) => format!("{} - {}", s.number, s.name),
                _ => m.view_name(*id)?.to_string(),
            };
            like(&f).then_some((*id, name))
        })
        .collect();
    views.sort_by(|a, b| a.1.cmp(&b.1));
    views
}

/// The levels Aligned to Selected Levels offers, lowest first, by name.
pub fn levels_of(m: &Model) -> Vec<(Id, String)> {
    m.levels().into_iter().map(|(id, l)| (id, l.name.clone())).collect()
}

/// A list of levels or views a paste goes onto, being ticked: the dialog's state.
#[derive(Clone, Debug)]
pub struct Choose {
    /// The clipboard's text, which the paste reads again.
    pub text: String,
    /// Views (Aligned to Selected Views) rather than levels.
    pub views: bool,
    pub offered: Vec<(Id, String)>,
    pub picked: BTreeSet<Id>,
}

impl Choose {
    /// Ticks the row of `id` if it is one of those offered, or takes the tick off.
    pub fn toggle(&mut self, id: Id) {
        if self.offered.iter().any(|(o, _)| *o == id) && !self.picked.remove(&id) {
            self.picked.insert(id);
        }
    }

    /// The command's own mode and list for what is ticked, in the order offered.
    pub fn params(&self) -> Value {
        let ids: Vec<Id> = self.offered.iter().map(|(id, _)| *id).filter(|id| self.picked.contains(id)).collect();
        if self.views { json!({"mode": "views", "views": ids}) } else { json!({"mode": "levels", "levels": ids}) }
    }
}

#[cfg(test)]
mod tests;
