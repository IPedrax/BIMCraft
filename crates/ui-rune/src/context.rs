//! The right-click menu of the plan, as data: which items the moment offers (`items`) and what each is (`Action`); `App::act` does it.
//! After the egui window's `context.rs` (`context_items` and `Action`), which is the reference and stays as it is: this is a copy of
//! the part that is about the plan and the selection, to fold into the core when egui adopts it. Not here yet from that menu: Edit
//! Family and Type, Select All Instances, Hide and Override Graphics (a view's) and Reload CAD.

use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model};

use crate::views::{Target, ViewKind};

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
    /// The selection to the clipboard, taken away or kept; and the clipboard pasted, as Ctrl+V does in the view shown, or Aligned
    /// (`app/clip.rs`).
    Cut,
    CopyToClipboard,
    Paste,
    PasteSamePlace,
    PasteCurrentView,
    PasteSelectedLevels,
    PasteSelectedViews,
    /// Groups (`app/groups.rs`): the selection made one, a group's members plain again, one edited and what Edit Group does, a member
    /// left out of its group and put back.
    CreateGroup,
    Ungroup,
    EditGroup,
    AddToGroup,
    RemoveFromGroup,
    FinishGroup,
    CancelGroup,
    ExcludeMember,
    RestoreExcluded,
    // The Project Browser's menu (`browser_items`): what a right click over a row, a group, a type or a linked model offers.
    /// A copy of a plan view, a copy with the annotation of the plan, a view that follows it (`duplicate_view`).
    Duplicate,
    DuplicateDetailing,
    DuplicateDependent,
    /// Takes the view (a plan view, a 3D view, a callout, a schedule) out of the project.
    DeleteView,
    /// A print set of the views picked together, or of the one the click was over.
    CreatePrintSet,
    ExpandAll,
    CollapseAll,
    /// Type Properties of the type the row is.
    EditType,
    /// Takes the type out of the project, which refuses while an element uses it.
    DeleteType,
    ReloadLink,
    RemoveLink,
    /// The plan or section the row is, shown under the plan on screen (`underlay`); and the plan on screen's underlay taken off.
    ShowAsUnderlay,
    RemoveUnderlay,
}

impl Action {
    /// Whether it is an item of the Project Browser's menu: what it does is `App::act_browser`'s.
    pub fn is_browser(self) -> bool {
        matches!(
            self,
            Action::Duplicate
                | Action::DuplicateDetailing
                | Action::DuplicateDependent
                | Action::DeleteView
                | Action::CreatePrintSet
                | Action::ExpandAll
                | Action::CollapseAll
                | Action::EditType
                | Action::DeleteType
                | Action::ReloadLink
                | Action::RemoveLink
                | Action::ShowAsUnderlay
                | Action::RemoveUnderlay
        )
    }

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
            Action::Cut => "Cut to Clipboard",
            Action::CopyToClipboard => "Copy to Clipboard",
            Action::Paste => "Paste",
            Action::PasteSamePlace => "Paste Aligned to Same Place",
            Action::PasteCurrentView => "Paste Aligned to Current View",
            Action::PasteSelectedLevels => "Paste Aligned to Selected Levels\u{2026}",
            Action::PasteSelectedViews => "Paste Aligned to Selected Views\u{2026}",
            Action::CreateGroup => "Create Group",
            Action::Ungroup => "Ungroup",
            Action::EditGroup => "Edit Group",
            Action::AddToGroup => "Add to Group",
            Action::RemoveFromGroup => "Remove from Group",
            Action::FinishGroup => "Finish Edit Group",
            Action::CancelGroup => "Cancel Edit Group",
            Action::ExcludeMember => "Exclude",
            Action::RestoreExcluded => "Restore All Excluded",
            Action::Duplicate => "Duplicate",
            Action::DuplicateDetailing => "Duplicate with Detailing",
            Action::DuplicateDependent => "Duplicate as Dependent",
            Action::DeleteView | Action::DeleteType => "Delete",
            Action::CreatePrintSet => "Create Print Set\u{2026}",
            Action::ExpandAll => "Expand All",
            Action::CollapseAll => "Collapse All",
            Action::EditType => "Edit Type",
            Action::ReloadLink => "Reload",
            Action::RemoveLink => "Remove",
            Action::ShowAsUnderlay => "Show as Underlay",
            Action::RemoveUnderlay => "Remove Underlay",
        }
    }

    /// The key that does the same, shown beside it: only the plain keys, the Revit shortcuts wait for the shared registry.
    pub fn key(self) -> Option<&'static str> {
        match self {
            Action::Finish => Some("Enter"),
            Action::Cancel => Some("Esc"),
            Action::Delete => Some("Del"),
            Action::Cut => Some("Ctrl+X"),
            Action::CopyToClipboard => Some("Ctrl+C"),
            Action::Paste => Some("Ctrl+V"),
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
        groups.push(vec![Item::new(Action::Cut, true), Item::new(Action::CopyToClipboard, true)]);
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
    // The clipboard pasted: as Ctrl+V does in the view shown, or Aligned.
    groups.push(vec![
        Item::new(Action::Paste, true),
        Item::new(Action::PasteSamePlace, true),
        Item::new(Action::PasteCurrentView, true),
        Item::new(Action::PasteSelectedLevels, true),
        Item::new(Action::PasteSelectedViews, true),
    ]);
    groups.push(vec![Item::new(Action::ZoomToFit, true), Item::new(Action::ZoomIn, true), Item::new(Action::ZoomOut, true)]);
    groups
}

/// What the Project Browser's menu offers over `target`, in groups, as egui's browser has them: a plan (a level's or a plan view's) can be
/// duplicated three ways, a plan view, a 3D view other than `{3D}`, a callout, a schedule of the project's and a reinforcement detail
/// can be deleted, a view can go in a print set, a type can be edited or deleted, a linked model reloaded or removed; folding is
/// always there.
pub fn browser_items(m: &Model, target: &Target) -> Vec<Vec<Item>> {
    let on = |a| Item::new(a, true);
    let mut groups: Vec<Vec<Item>> = Vec::new();
    match target {
        Target::View(kind) => {
            let plan = match *kind {
                ViewKind::Plan(id) | ViewKind::Ceiling(id) => matches!(m.elements.get(id), Some(Element::Level(_) | Element::PlanView(_))),
                _ => false,
            };
            if plan {
                groups.push(vec![on(Action::Duplicate), on(Action::DuplicateDetailing), on(Action::DuplicateDependent)]);
            }
            let deletable = match *kind {
                ViewKind::Plan(id) | ViewKind::Section(id) | ViewKind::Ceiling(id) => {
                    matches!(m.elements.get(id), Some(Element::PlanView(_) | Element::DetailView(_)))
                }
                ViewKind::View3D(Some(id)) => matches!(m.elements.get(id), Some(Element::View3D(v)) if v.name != "{3D}"),
                ViewKind::Table(id) => matches!(m.elements.get(id), Some(Element::Schedule(_) | Element::RebarDetail(_))),
                _ => false,
            };
            if deletable {
                groups.push(vec![on(Action::DeleteView)]);
            }
            groups.push(vec![Item::new(Action::CreatePrintSet, crate::views::element_id(m, *kind).is_some())]);
        }
        Target::Type(..) => groups.push(vec![on(Action::EditType), on(Action::DeleteType)]),
        Target::Link(_) => groups.push(vec![on(Action::ReloadLink), on(Action::RemoveLink)]),
        Target::Group(_) => {}
    }
    groups.push(vec![on(Action::ExpandAll), on(Action::CollapseAll)]);
    groups
}

#[cfg(test)]
mod tests;
