//! The right-click menu every view has (plans, ceiling plans, sections, elevations, the 3D view,
//! sheets and schedules), as Revit's: what the open view and the selection can do. A menu is groups
//! of [`Item`]s, drawn with separators between them; [`BimApp::context_items`] says which items the
//! moment offers, and [`BimApp::act`] does what one says. A new item is an [`Action`] variant, its
//! label (a row in `lang::WORDS`), the group it joins in `context_items`, and its arm in `act`.

use bimcraft_i18n::msg;

use egui::{Button, Response};

use crate::lang::t;
use crate::{BimApp, Sheet, Tool};

/// What an item of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Esc: the tool's points, else the tool, else the selection.
    Cancel,
    /// Enter: a spline or a polyline made of the points clicked so far.
    Finish,
    ZoomToFit,
    ZoomIn,
    ZoomOut,
    /// The 3D view's default orientation.
    HomeView,
    EditType,
    SelectAllInstances,
    HideElement,
    HideCategory,
    OverrideGraphics,
    CreateGroup,
    Ungroup,
    Delete,
    /// The linked CAD drawings selected, read again from their files.
    ReloadCad,
    /// The 3D view's Work Plane dialog.
    WorkPlane,
}

impl Action {
    /// Its label, in English: the menu translates it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Action::Cancel => "Cancel",
            Action::Finish => msg!("Finish Drawing"),
            Action::ZoomToFit => "Zoom to Fit",
            Action::ZoomIn => "Zoom In",
            Action::ZoomOut => "Zoom Out",
            Action::HomeView => "Home View",
            Action::EditType => "Edit Type",
            Action::SelectAllInstances => msg!("Select All Instances"),
            Action::HideElement => "Hide Element",
            Action::HideCategory => "Hide Category",
            Action::OverrideGraphics => "Override Graphics",
            Action::CreateGroup => msg!("Create Group"),
            Action::Ungroup => msg!("Ungroup"),
            Action::Delete => "Delete",
            Action::ReloadCad => msg!("Reload CAD"),
            Action::WorkPlane => msg!("Set Work Plane…"),
        }
    }

    /// The keys that do the same, shown beside it.
    pub(crate) fn shortcut(self) -> Option<&'static str> {
        match self {
            Action::Cancel => Some("Esc"),
            Action::Finish => Some("Enter"),
            Action::ZoomToFit => Some("ZF"),
            Action::SelectAllInstances => Some("SA"),
            Action::HideElement => Some("EH"),
            Action::HideCategory => Some("VH"),
            Action::CreateGroup => Some("GP"),
            Action::Ungroup => Some("UG"),
            Action::Delete => Some("Del"),
            _ => None,
        }
    }
}

/// An item of the menu: what it does, and whether it can now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub action: Action,
    pub enabled: bool,
}

impl Item {
    fn new(action: Action, enabled: bool) -> Item {
        Item { action, enabled }
    }
}

/// The menu of `groups` on `response`'s right click; the item clicked goes to `chosen`. Free of the
/// app, so a view drawing in a closure that holds it can still offer the menu.
pub(crate) fn menu(response: &Response, groups: &[Vec<Item>], chosen: &mut Option<Action>) {
    response.context_menu(|ui| {
        for (i, group) in groups.iter().filter(|g| !g.is_empty()).enumerate() {
            if i > 0 {
                ui.separator();
            }
            for item in group {
                let mut button = Button::new(t(item.action.label()));
                if let Some(keys) = item.action.shortcut() {
                    button = button.shortcut_text(keys);
                }
                if ui.add_enabled(item.enabled, button).clicked() {
                    *chosen = Some(item.action);
                    ui.close();
                }
            }
        }
    });
}

impl BimApp {
    /// What the menu offers now, in groups: cancelling; what the selection can do; what the view
    /// can (zooming, but not in a schedule).
    pub(crate) fn context_items(&self) -> Vec<Vec<Item>> {
        let some = !self.selection.is_empty();
        let mut first = vec![Item::new(Action::Cancel, some || self.tool != Tool::Modify)];
        if self.finishable() {
            first.insert(0, Item::new(Action::Finish, true));
        }
        let mut groups = vec![first];
        if some {
            groups.push(vec![
                Item::new(Action::EditType, self.type_subject().is_some()),
                Item::new(Action::SelectAllInstances, true),
                Item::new(Action::Delete, true),
            ]);
            // Detail lines, regions and notes group; a group comes apart.
            let m = &self.session.model;
            let groupable = self.selection.iter().all(|id| m.elements.get(*id).is_some_and(bimcraft_core::groups::holds));
            if groupable || self.group_selected() {
                groups.push(vec![Item::new(Action::CreateGroup, groupable), Item::new(Action::Ungroup, self.group_selected())]);
            }
            if !self.linked_cads().is_empty() {
                groups.push(vec![Item::new(Action::ReloadCad, true)]);
            }
            // What the view on screen hides and paints.
            if self.graphic_view().is_some() {
                groups.push(vec![
                    Item::new(Action::HideElement, true),
                    Item::new(Action::HideCategory, true),
                    Item::new(Action::OverrideGraphics, true),
                ]);
            }
        }
        let zoom = vec![Item::new(Action::ZoomToFit, true), Item::new(Action::ZoomIn, true), Item::new(Action::ZoomOut, true)];
        match self.sheet {
            Sheet::Plan(_) | Sheet::Ceiling(_) | Sheet::Section(_) | Sheet::Paper(_) => groups.push(zoom),
            Sheet::View3D => {
                groups.push(vec![Item::new(Action::WorkPlane, self.sketch.is_none())]);
                groups.push([vec![Item::new(Action::HomeView, true)], zoom].concat());
            }
            Sheet::Schedule(_) | Sheet::Table(_) => {}
        }
        groups
    }

    /// Do what `action` says.
    pub(crate) fn act(&mut self, action: Action) {
        match action {
            Action::Cancel => self.escape(),
            Action::Finish => self.finish_open_shape(),
            Action::ZoomToFit => self.fit(),
            Action::ZoomIn => self.zoom_view(1.25),
            Action::ZoomOut => self.zoom_view(0.8),
            Action::HomeView => self.orbit.camera = None,
            Action::EditType => self.edit_type(),
            Action::SelectAllInstances => self.select_all_instances(),
            Action::HideElement => self.hide_selection(false),
            Action::HideCategory => self.hide_selection(true),
            Action::OverrideGraphics => self.override_selection(),
            Action::CreateGroup => self.create_group(),
            Action::Ungroup => self.ungroup(),
            Action::Delete => self.delete(),
            Action::WorkPlane => self.open_work_plane(),
            Action::ReloadCad => {
                for cad in self.linked_cads() {
                    self.run("reload_cad", serde_json::json!({"cad": cad}));
                }
            }
        }
    }

    /// The linked CAD drawings the selection places, when all it holds are imports of linked drawings; else none.
    fn linked_cads(&self) -> Vec<String> {
        let m = &self.session.model;
        let mut names = Vec::new();
        for id in &self.selection {
            match m.elements.get(*id) {
                Some(bimcraft_core::model::Element::CadImport(ci)) if m.cads.get(&ci.cad).is_some_and(|c| c.path().is_some()) => {
                    if !names.contains(&ci.cad) {
                        names.push(ci.cad.clone());
                    }
                }
                _ => return Vec::new(),
            }
        }
        names
    }

    /// The menu on a view's `response`, doing what's chosen from it.
    pub(crate) fn context_menu(&mut self, response: &Response) {
        let groups = self.context_items();
        let mut chosen = None;
        menu(response, &groups, &mut chosen);
        if let Some(action) = chosen {
            self.act(action);
        }
    }
}
