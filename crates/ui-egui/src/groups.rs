//! Groups on the desktop ([groups](../../../docs/areas/groups.md)): Create Group (GP) of a selection all of one kind, Place
//! Model Group and Place Detail Group with the type on the Options Bar, Repeating Detail, Ungroup (UG), Edit Group with its
//! Add, Remove, Finish and Cancel, Exclude and Restore All Excluded, and what the window asks when a change would break a
//! group: Revit's Ungroup, Create New Type or Cancel. The core makes and keeps them (`bimcraft_core::groups`).

use bimcraft_core::geom::P2;
use bimcraft_core::groups::{self as core_groups, Kind, Placement, RepeatingDetail};
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_i18n::msg;
use egui::{ComboBox, Context, Id as UiId, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::{BimApp, Status, big_button, panel};

/// A command the core refused because it would change a group from inside: run again as the user chooses.
#[derive(Clone, Debug)]
pub(crate) struct Conflict {
    pub command: String,
    pub params: Value,
    pub message: String,
}

impl BimApp {
    /// What kind of group the selection makes, if it is all of one kind and none of it is grouped yet.
    pub(crate) fn groupable(&self) -> Option<Kind> {
        let m = &self.session.model;
        if self.selection.is_empty() || self.selection.iter().any(|id| core_groups::group_of(m, *id).is_some()) {
            return None;
        }
        core_groups::kind_of_all(m, &self.selection).ok()
    }

    /// Create Group: the selection, all model elements or all of one view's annotation, made one group, selected.
    pub(crate) fn create_group(&mut self) {
        if self.selection.is_empty() {
            self.status = Status { text: t("select the elements to group first").into(), error: true };
            return;
        }
        if let Err(e) = core_groups::kind_of_all(&self.session.model, &self.selection) {
            self.status = Status { text: e, error: true };
            return;
        }
        if let Some(r) = self.run("create_group", json!({"ids": self.selection})) {
            let name = r["name"].as_str().unwrap_or_default().to_string();
            if r["kind"] == "model" {
                self.model_group_type = name;
            } else {
                self.group_type = name;
            }
            self.selection = r["id"].as_u64().into_iter().collect();
        }
    }

    /// What the selection lights: it, and the members of the groups in it.
    pub(crate) fn lit_selection(&self) -> Vec<Id> {
        core_groups::with_members(&self.session.model, &self.selection)
    }

    /// The group instances selected.
    pub(crate) fn groups_selected(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.selection.iter().copied().filter(|id| m.elements.get(*id).and_then(core_groups::instance).is_some()).collect()
    }

    /// Ungroup: each selected group's members, plain elements again, selected.
    pub(crate) fn ungroup(&mut self) {
        let groups = self.groups_selected();
        if groups.is_empty() {
            self.status = Status { text: t("select a group to ungroup").into(), error: true };
            return;
        }
        if let Some(r) = self.run("ungroup", json!({"ids": groups})) {
            self.selection = r["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        }
    }

    /// The selected elements that are members of a group (Tab picked them), for Exclude.
    pub(crate) fn members_selected(&self) -> Vec<Id> {
        let m = &self.session.model;
        let editing = self.session.editing_group();
        self.selection.iter().copied().filter(|id| core_groups::group_of(m, *id).is_some_and(|g| Some(m.guid(g)) != editing)).collect()
    }

    /// Exclude: the selected members out of their groups here.
    pub(crate) fn exclude_selected(&mut self) {
        let members = self.members_selected();
        if members.is_empty() {
            self.status = Status { text: t("Tab to a member of a group and select it to exclude it").into(), error: true };
            return;
        }
        if self.run("exclude_member", json!({"ids": members})).is_some() {
            self.selection.clear();
        }
    }

    /// The selected groups that have members excluded, for Restore All Excluded.
    pub(crate) fn groups_excluding(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.groups_selected()
            .into_iter()
            .filter(|id| m.elements.get(*id).and_then(core_groups::instance).is_some_and(|i| i.members.iter().any(|s| s.excluded)))
            .collect()
    }

    /// Restore All Excluded, of the selected groups.
    pub(crate) fn restore_excluded(&mut self) {
        let groups = self.groups_excluding();
        if !groups.is_empty() {
            self.run("restore_excluded", json!({"ids": groups}));
        }
    }

    /// Edit Group, of the one group selected.
    pub(crate) fn edit_group(&mut self) {
        let Some(&g) = self.groups_selected().first() else {
            self.status = Status { text: t("select a group to edit").into(), error: true };
            return;
        };
        if self.run("edit_group", json!({"id": g})).is_some() {
            self.selection.clear();
        }
    }

    /// The group in Edit Group, if one is.
    pub(crate) fn editing_group(&self) -> Option<Id> {
        let g = self.session.editing_group()?;
        self.session.model.elements.id(g)
    }

    /// Edit Group's panel: Add and Remove (the selection), Finish and Cancel. Shown on every tab while a group is edited.
    pub(crate) fn edit_group_panel(&mut self, ui: &mut Ui) {
        if self.editing_group().is_none() {
            return;
        }
        panel(ui, msg!("Edit Group"), |ui| {
            let some = !self.selection.is_empty();
            let tip = msg!("Add: the selected elements join the group at Finish");
            if ui.add_enabled_ui(some, |ui| big_button(ui, Icon::AddToGroup, msg!("Add"), false, tip)).inner.clicked() {
                let ids = self.selection.clone();
                self.run("add_to_group", json!({"ids": ids}));
            }
            let tip = msg!("Remove: the selected members leave the group at Finish, staying as they are");
            if ui.add_enabled_ui(some, |ui| big_button(ui, Icon::RemoveFromGroup, msg!("Remove"), false, tip)).inner.clicked() {
                let ids = self.selection.clone();
                self.run("remove_from_group", json!({"ids": ids}));
            }
            let tip = msg!("Finish: the group's type becomes what it now is, and every group of it changes with it");
            if crate::inked_button(ui, Icon::Check, msg!("Finish Edit Group"), false, tip, Some(crate::style::pal().context)).clicked()
                && self.run("finish_group", json!({})).is_some()
            {
                self.selection.clear();
            }
            let tip = msg!("Cancel: the group and everything else as it was when the edit began");
            if crate::inked_button(ui, Icon::Close, msg!("Cancel Edit Group"), false, tip, Some(crate::style::pal().error)).clicked()
                && self.run("cancel_group", json!({})).is_some()
            {
                self.selection.clear();
            }
        });
    }

    /// The Modify tab's Create panel and, with a group or a member selected, Revit's Group panel.
    pub(crate) fn group_panels(&mut self, ui: &mut Ui) {
        if self.editing_group().is_some() {
            return;
        }
        let kind = self.groupable();
        // Create Group, and Place Model Group once there is a model group to place.
        panel(ui, msg!("Create"), |ui| {
            let tip = match kind {
                Some(Kind::Model) => msg!("Create Group (GP): the selected model elements made one model group"),
                Some(Kind::Detail) => msg!("Create Group (GP): the selected lines, regions, notes and dimensions made one detail group"),
                None => msg!(
                    "Create Group (GP): select model elements, or a view's detail lines, regions, notes and dimensions, all of one kind"
                ),
            };
            if ui.add_enabled_ui(kind.is_some(), |ui| big_button(ui, Icon::CreateGroup, "Create Group", false, tip)).inner.clicked() {
                self.create_group();
            }
            if !self.session.model.model_groups.is_empty() {
                let tip = msg!("Place Model Group: the model group on the Options Bar where you click, on this level");
                let active = self.tool == crate::Tool::ModelGroup;
                if big_button(ui, Icon::ModelGroup, msg!("Model Group"), active, tip).clicked() {
                    self.set_tool(crate::Tool::ModelGroup);
                }
            }
        });
        let groups = self.groups_selected();
        let members = self.members_selected();
        if groups.is_empty() && members.is_empty() {
            return;
        }
        panel(ui, msg!("Group"), |ui| {
            if !groups.is_empty() {
                let one = groups.len() == 1;
                let tip = msg!("Edit Group: change its members; Finish changes every group of its type");
                if ui.add_enabled_ui(one, |ui| big_button(ui, Icon::EditGroup, msg!("Edit Group"), false, tip)).inner.clicked() {
                    self.edit_group();
                }
                let tip = msg!("Ungroup (UG): its members as plain elements, where they are");
                if big_button(ui, Icon::Ungroup, "Ungroup", false, tip).clicked() {
                    self.ungroup();
                }
                let excluding = !self.groups_excluding().is_empty();
                let tip = msg!("Restore All Excluded: every member excluded from the group back, as its type has it");
                let r = ui.add_enabled_ui(excluding, |ui| big_button(ui, Icon::RestoreExcluded, msg!("Restore All Excluded"), false, tip));
                if r.inner.clicked() {
                    self.restore_excluded();
                }
            }
            if !members.is_empty() {
                let tip = msg!("Exclude: the selected member out of this group only; Restore All Excluded brings it back");
                if big_button(ui, Icon::ExcludeMember, msg!("Exclude"), false, tip).clicked() {
                    self.exclude_selected();
                }
            }
        });
    }

    /// The detail group type Place Detail Group places: the one chosen, else the first.
    pub(crate) fn placing_group(&self) -> Option<String> {
        let m = &self.session.model;
        m.detail_groups.contains_key(&self.group_type).then(|| self.group_type.clone()).or_else(|| m.detail_groups.keys().next().cloned())
    }

    /// The model group type Place Model Group places: the one chosen, else the first.
    pub(crate) fn placing_model_group(&self) -> Option<String> {
        let m = &self.session.model;
        m.model_groups
            .contains_key(&self.model_group_type)
            .then(|| self.model_group_type.clone())
            .or_else(|| m.model_groups.keys().next().cloned())
    }

    /// The members Place Detail Group would put at `at` in view `view`.
    pub(crate) fn group_at(&self, m: &Model, view: Id, at: P2) -> Vec<Element> {
        let Some(type_name) = self.placing_group() else { return Vec::new() };
        core_groups::preview(m, Kind::Detail, &type_name, m.guid(view), Placement { at, angle: 0.0, mirrored: false })
    }

    /// The members Place Model Group would put at `at` on level `level`.
    pub(crate) fn model_group_at(&self, m: &Model, level: Id, at: P2) -> Vec<Element> {
        let Some(type_name) = self.placing_model_group() else { return Vec::new() };
        core_groups::preview(m, Kind::Model, &type_name, m.guid(level), Placement { at, angle: 0.0, mirrored: false })
    }

    /// Place Detail Group's Options Bar: the type it places.
    pub(crate) fn group_options(&mut self, ui: &mut Ui) {
        let m = &self.session.model;
        let now = self.placing_group().unwrap_or_default();
        let l = ui.label(t("Group:"));
        let mut chosen = now.clone();
        let c = ComboBox::from_id_salt("detail-group-type").selected_text(&now).width(160.0).show_ui(ui, |ui| {
            for n in m.detail_groups.keys() {
                ui.selectable_value(&mut chosen, n.clone(), n);
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Group")));
        c.response.labelled_by(l.id);
        self.group_type = chosen;
    }

    /// Place Model Group's Options Bar: the type it places, and whether the group placed is forgiving.
    pub(crate) fn model_group_options(&mut self, ui: &mut Ui) {
        let m = &self.session.model;
        let now = self.placing_model_group().unwrap_or_default();
        let l = ui.label(t("Group:"));
        let mut chosen = now.clone();
        let c = ComboBox::from_id_salt("model-group-type").selected_text(&now).width(160.0).show_ui(ui, |ui| {
            for n in m.model_groups.keys() {
                ui.selectable_value(&mut chosen, n.clone(), n);
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Model Group")));
        c.response.labelled_by(l.id);
        self.model_group_type = chosen;
        ui.checkbox(&mut self.group_forgiving, t("Forgiving")).on_hover_text(t(msg!(
            "A forgiving group may stand where the storeys are another height: its members' tops follow the levels there"
        )));
    }

    /// The Options Bar's spacing, in millimetres; none typed as one, a message why.
    fn spacing(&self) -> Result<f64, String> {
        let m = &self.session.model;
        m.units.parse(&self.repeat_spacing).filter(|d| *d > 0.0).ok_or_else(|| tf("`{}` is not a spacing", &[&self.repeat_spacing]))
    }

    /// The repeating detail from `a` to `b` the tool would make in view `view`.
    pub(crate) fn repeat_to(&self, m: &Model, view: Id, a: P2, b: P2) -> Option<Element> {
        let type_name = self.placing_group()?;
        let spacing = self.spacing().ok()?;
        let r = RepeatingDetail { view: m.guid(view), type_name, start: a, end: b, spacing, mirrored: false };
        r.places().map(|_| Element::RepeatingDetail(r))
    }

    /// Repeating Detail: the group along `a` to `b` in view `view`; whether it was made.
    pub(crate) fn repeat(&mut self, view: Id, a: P2, b: P2) -> bool {
        let Some(type_name) = self.placing_group() else {
            self.status = Status { text: t("there's no detail group yet: select detail lines and Create Group").into(), error: true };
            return false;
        };
        let spacing = match self.spacing() {
            Ok(d) => d,
            Err(text) => {
                self.status = Status { text, error: true };
                return false;
            }
        };
        let p = json!({"type": type_name, "start": [a.0, a.1], "end": [b.0, b.1], "spacing": spacing, "view": view});
        self.run("repeating_detail", p).is_some()
    }

    /// Place Detail Group: one where clicked, in view `view`.
    pub(crate) fn place_group(&mut self, view: Id, at: P2) {
        let Some(type_name) = self.placing_group() else {
            self.status = Status { text: t("there's no detail group yet: select detail lines and Create Group").into(), error: true };
            return;
        };
        self.run("detail_group", json!({"type": type_name, "at": [at.0, at.1], "view": view}));
    }

    /// Place Model Group: one where clicked, on level `level`.
    pub(crate) fn place_model_group(&mut self, level: Id, at: P2) {
        let Some(type_name) = self.placing_model_group() else {
            self.status = Status { text: t("there's no model group yet: select model elements and Create Group").into(), error: true };
            return;
        };
        let p = json!({"type": type_name, "at": [at.0, at.1], "level": level, "forgiving": self.group_forgiving});
        self.run("model_group", p);
    }

    /// A command refused because it would change a group from inside: kept, so the window asks what to do.
    pub(crate) fn conflict_refused(&mut self, command: &str, params: &Value, e: &str) {
        if core_groups::is_conflict(e) && command != "resolve_group" {
            self.group_conflict = Some(Conflict { command: command.into(), params: params.clone(), message: e.into() });
        }
    }

    /// Revit's question when a change would break a group: Ungroup, Create New Type, or Cancel.
    pub(crate) fn group_conflict_window(&mut self, ctx: &Context) {
        let Some(c) = self.group_conflict.clone() else { return };
        let mut fix = None;
        let mut cancel = false;
        let r = Modal::new(UiId::new("group-conflict"), t(msg!("Group Changed"))).width(520.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t(msg!(
                    "Changes to groups are made in Edit Group. This change would make a group unlike the others of its type."
                )));
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&c.message).small().weak());
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t(msg!("Ungroup"))).on_hover_text(t(msg!("Ungroup the changed groups and keep the change"))).clicked() {
                        fix = Some("ungroup");
                    }
                    let tip = msg!("Make each changed group a group type of its own, as it now stands");
                    if ui.button(t(msg!("Create New Type"))).on_hover_text(t(tip)).clicked() {
                        fix = Some("new_type");
                    }
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if fix.is_some() || cancel || r.should_close() {
            self.group_conflict = None;
        }
        if let Some(fix) = fix {
            self.run("resolve_group", json!({"fix": fix, "command": c.command, "params": c.params}));
        }
    }
}
