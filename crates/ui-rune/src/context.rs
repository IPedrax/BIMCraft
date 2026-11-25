//! The right-click menu of the plan, as data: which items the moment offers (`items`) and what each is (`Action`); `App::act` does it.
//! After the egui window's `context.rs` (`context_items` and `Action`), which is the reference and stays as it is: this is a copy of
//! the part that is about the plan and the selection, to fold into the core when egui adopts it. Not here yet from that menu: Edit
//! Family and Type, Select All Instances, the groups' items, Hide and Override Graphics (a view's), Reload CAD, and the clipboard's.

use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model};

/// What an item of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Enter: the tool in hand is finished with the points it has.
    Finish,
    /// Esc: the tool's points, else the tool, else the selection.
    Cancel,
    Delete,
    /// The selected elements pinned where they are, or unpinned.
    Pin,
    Unpin,
    /// The selected walls' and runs' joins refused (Revit's Disallow Join), or taken up again.
    DisallowJoin,
    AllowJoin,
    ZoomToFit,
    ZoomIn,
    ZoomOut,
}

impl Action {
    /// Its label, in English: the menu translates it.
    pub fn label(self) -> &'static str {
        match self {
            Action::Finish => "Finish Drawing",
            Action::Cancel => "Cancel",
            Action::Delete => "Delete",
            Action::Pin => "Pin",
            Action::Unpin => "Unpin",
            Action::DisallowJoin => "Disallow Join",
            Action::AllowJoin => "Allow Join",
            Action::ZoomToFit => "Zoom to Fit",
            Action::ZoomIn => "Zoom In",
            Action::ZoomOut => "Zoom Out",
        }
    }

    /// The key that does the same, shown beside it: only the plain keys, the Revit shortcuts wait for the shared registry.
    pub fn key(self) -> Option<&'static str> {
        match self {
            Action::Finish => Some("Enter"),
            Action::Cancel => Some("Esc"),
            Action::Delete => Some("Del"),
            _ => None,
        }
    }
}

/// An item of the menu: what it does, and whether it can now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub action: Action,
    pub enabled: bool,
}

impl Item {
    fn new(action: Action, enabled: bool) -> Item {
        Item { action, enabled }
    }
}

/// What the menu offers now, in groups: finishing and cancelling; what the selection can do (delete, pin, join); zooming. `tool` is a
/// tool in hand and `finishable` that it has points Enter would finish.
pub fn items(m: &Model, selection: &[Id], tool: bool, finishable: bool) -> Vec<Vec<Item>> {
    let some = !selection.is_empty();
    let mut first = vec![Item::new(Action::Cancel, some || tool)];
    if finishable {
        first.insert(0, Item::new(Action::Finish, true));
    }
    let mut groups = vec![first];
    if some {
        groups.push(vec![Item::new(Action::Delete, true)]);
        // What is pinned stays where it is until it is unpinned.
        let pinned: Vec<bool> = selection.iter().map(|id| m.is_pinned(*id)).collect();
        groups.push(vec![Item::new(Action::Pin, pinned.iter().any(|p| !*p)), Item::new(Action::Unpin, pinned.iter().any(|p| *p))]);
        // Walls and runs mitre against what they meet; each item is offered when some selected element would change by taking it.
        let joins: Vec<bool> = selection
            .iter()
            .filter_map(|id| match m.elements.get(*id) {
                Some(Element::Wall(w)) => Some(w.join),
                Some(e) => e.run().map(|r| r.join),
                None => None,
            })
            .collect();
        if !joins.is_empty() {
            groups.push(vec![
                Item::new(Action::DisallowJoin, joins.iter().any(|j| *j)),
                Item::new(Action::AllowJoin, joins.iter().any(|j| !*j)),
            ]);
        }
    }
    groups.push(vec![Item::new(Action::ZoomToFit, true), Item::new(Action::ZoomIn, true), Item::new(Action::ZoomOut, true)]);
    groups
}

#[cfg(test)]
mod tests;
