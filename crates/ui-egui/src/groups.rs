//! Groups on the desktop ([groups](../../../docs/areas/groups.md)): Create Group (GP) of a selection all of one kind, Place
//! Model Group and Place Detail Group with the type on the Options Bar, Repeating Detail, Ungroup (UG), Edit Group with its
//! Add, Remove, Finish and Cancel, Exclude and Restore All Excluded, and what the window asks when a change would break a
//! group: Revit's Ungroup, Create New Type or Cancel. The core makes and keeps them (`bimcraft_core::groups`).

use bimcraft_core::geom::P2;
use bimcraft_core::groups::{self as core_groups, Kind, Placement, RepeatingDetail, Scope};
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_i18n::msg;
use egui::{ComboBox, Context, Id as UiId, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::{BimApp, Status, big_button, panel};

/// What Flexible means, said where it is set: on Place Model Group's Options Bar and in a model group's Properties.
pub(crate) const FLEXIBLE_TIP: &str = msg!(
    "Flexible: the group may stand where the storeys are of other heights. A member reaching up to a level (a wall to the level \
     above) stretches or shrinks to reach it there; what it stands on, its place in plan and its offsets stay as the group's type \
     has them. Off, the group stands only where the storeys are as high as its type's, and moving a level it reaches asks first."
);

/// Edit Group's Add or Remove as a pick: a click (or a box) takes elements into the group, or out of it, at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupPick {
    Add,
    Remove,
}

/// What a pick of `ids` (nearest first) takes: in Edit Group (`scope`), only what is the group's, or with Add (`mode`) only what
/// may join it; else a member's group first ([`core_groups::group_picks`]), or for a box ([`core_groups::as_picked`]) in its
/// place.
pub(crate) fn picks(m: &Model, ids: &[Id], scope: Option<&Scope>, mode: Option<GroupPick>, boxed: bool) -> Vec<Id> {
    match (mode, scope) {
        (Some(GroupPick::Add), Some(sc)) => sc.addable(m, ids),
        _ if boxed => core_groups::as_picked(m, ids, scope),
        _ => core_groups::group_picks(m, ids, scope),
    }
}

/// What the element under the pointer (`raw`, as the picture picks it) lights: what a click would take ([`picks`]), and if that is a
/// group, its members, since a group has no lines or faces of its own. In Edit Group, nothing outside it (but for Add, what may
/// join).
pub(crate) fn hover_lit(m: &Model, raw: Option<Id>, scope: Option<&Scope>, mode: Option<GroupPick>) -> Vec<Id> {
    let Some(raw) = raw else { return Vec::new() };
    let first: Vec<Id> = picks(m, &[raw], scope, mode, false).into_iter().take(1).collect();
    core_groups::with_members(m, &first)
}

/// In Edit Group, what a picture greys: every element but the group's own and `lit` (what is selected or under the pointer).
pub(crate) fn greyed(m: &Model, scope: Option<&Scope>, lit: &[Id]) -> Vec<Id> {
    let Some(scope) = scope else { return Vec::new() };
    let lit: std::collections::HashSet<Id> = lit.iter().copied().collect();
    m.elements.iter().map(|(id, _)| *id).filter(|id| !scope.editable(*id) && !lit.contains(id)).collect()
}

/// The colour a picture greys what is outside the group in Edit Group: nearly the paper's, a shade darker (or, on a dark canvas,
/// lighter), so it is there and faint, its shading and edges still showing its shape.
pub(crate) fn grey() -> bimcraft_core::render::Rgb {
    let p = crate::canvas::color(bimcraft_core::render::PAPER);
    let toward = |c: u8| if c > 128 { c.saturating_sub(12) } else { c.saturating_add(14) };
    bimcraft_core::render::Rgb(toward(p.r()), toward(p.g()), toward(p.b()))
}

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

    /// Create Group: the selection, all model elements or all of one view's annotation, made one group, selected. A selection
    /// that can't make one group (both kinds, or with what no group holds) asks which kind to make of it, the rest left out.
    pub(crate) fn create_group(&mut self) {
        if self.selection.is_empty() {
            self.status = Status { text: t("select the elements to group first").into(), error: true };
            return;
        }
        if self.groupable().is_some() {
            self.make_group(None);
            return;
        }
        let m = &self.session.model;
        let split = core_groups::split(m, &self.selection);
        if split.model.is_empty() && split.detail.is_empty() {
            let why = match core_groups::kind_of_all(m, &self.selection) {
                Err(e) => e,
                Ok(_) => t("what is selected is in a group already: ungroup it, or exclude it from its group, first").into(),
            };
            self.status = Status { text: why, error: true };
            return;
        }
        self.group_split = Some(split);
    }

    /// The group made of the selection, of `only` that kind if told, selected.
    fn make_group(&mut self, only: Option<Kind>) {
        let mut p = json!({"ids": self.selection});
        if let Some(k) = only {
            p["only"] = json!(k.name());
        }
        if let Some(r) = self.run("create_group", p) {
            let name = r["name"].as_str().unwrap_or_default().to_string();
            if r["kind"] == "model" {
                self.model_group_type = name;
            } else {
                self.group_type = name;
            }
            self.selection = r["id"].as_u64().into_iter().collect();
        }
    }

    /// Create Group's question for a selection that can't make one group: model elements only, or detail elements only.
    pub(crate) fn group_split_window(&mut self, ctx: &Context) {
        let Some(split) = self.group_split.clone() else { return };
        let all = self.selection.len();
        let mut chosen = None;
        let mut cancel = false;
        let r = Modal::new(UiId::new("group-split"), t("Create Group")).width(460.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t(msg!(
                    "A group holds model elements, or one view's detail lines, regions, notes and dimensions: not both, and nothing \
                     that is in a group already or that no group holds (levels, grids, views, tags, links). Which do you want to group?"
                )));
                ui.add_space(8.0);
                for (kind, label, tip) in [
                    (
                        Kind::Model,
                        msg!("Model elements only ({})"),
                        msg!("A model group of the walls, floors, doors, components and the like selected; the rest is left out"),
                    ),
                    (
                        Kind::Detail,
                        msg!("Detail elements only ({})"),
                        msg!("A detail group of the lines, regions, notes and dimensions selected in one view; the rest is left out"),
                    ),
                ] {
                    let n = split.of(kind).len();
                    let text = tf(label, &[&n.to_string()]);
                    let b = ui.add_enabled(n > 0, egui::Button::new(text).min_size(egui::vec2(260.0, 26.0)));
                    if b.on_hover_text(t(tip)).clicked() {
                        chosen = Some(kind);
                    }
                    if n > 0 {
                        let out = all.saturating_sub(n);
                        ui.label(egui::RichText::new(tf(msg!("{} of the selection left out"), &[&out.to_string()])).small().weak());
                    }
                    ui.add_space(4.0);
                }
            });
            crate::popup::footer(ui, |ui| {
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
        if chosen.is_some() || cancel || r.should_close() {
            self.group_split = None;
        }
        if let Some(kind) = chosen {
            self.make_group(Some(kind));
        }
    }

    /// What the selection lights: it, and the members of the groups in it.
    pub(crate) fn lit_selection(&self) -> Vec<Id> {
        let mut lit = core_groups::with_members(&self.session.model, &self.selection);
        lit.extend(self.railing_hosts_lit());
        lit
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

    /// Add or Remove picking in Edit Group, if one is on.
    pub(crate) fn group_pick(&self) -> Option<GroupPick> {
        self.group_pick.filter(|_| self.session.editing_group().is_some())
    }

    /// What a pick of `ids` takes now ([`picks`]).
    pub(crate) fn group_picks(&self, m: &Model, ids: &[Id], boxed: bool) -> Vec<Id> {
        picks(m, ids, self.session.group_scope().as_ref(), self.group_pick(), boxed)
    }

    /// Add or Remove picking: `found` joins the group or leaves it, at once. Whether a pick was taken so.
    pub(crate) fn group_pick_up(&mut self, found: &[Id]) -> bool {
        let Some(mode) = self.group_pick() else { return false };
        if !found.is_empty() {
            let command = if mode == GroupPick::Add { "add_to_group" } else { "remove_from_group" };
            self.run(command, json!({"ids": found}));
        }
        true
    }

    /// In Edit Group, the selection keeps only what is the group's: whatever picked it (the browser, a schedule, Select All),
    /// nothing else may be changed there.
    pub(crate) fn keep_to_group(&mut self) {
        if self.session.editing_group().is_none() {
            self.group_pick = None;
            return;
        }
        if let Some(scope) = self.session.group_scope() {
            self.selection.retain(|id| scope.editable(*id));
        }
    }

    /// The name of the group in Edit Group, and its kind: what the canvas says it is editing.
    pub(crate) fn editing_name(&self) -> Option<(String, Kind)> {
        let m = &self.session.model;
        let i = m.elements.get(self.editing_group()?).and_then(core_groups::instance)?;
        Some((i.type_name.to_string(), i.kind))
    }

    /// Edit Group, said over every view while it is on: a frame in the edit's colour round the view, and a banner at its top
    /// naming the group, what may be changed, and how to finish.
    pub(crate) fn edit_group_frame(&self, ui: &Ui) {
        let Some((name, kind)) = self.editing_name() else { return };
        let rect = self.view_rect;
        let p = ui.painter_at(rect);
        let ink = crate::style::pal().context;
        p.rect_stroke(rect.shrink(2.0), 0.0, egui::Stroke::new(4.0, ink), egui::StrokeKind::Inside);
        let what = if kind == Kind::Model { t(msg!("Editing model group")) } else { t(msg!("Editing detail group")) };
        let said = match self.group_pick() {
            Some(GroupPick::Add) => t(msg!("Adding: click greyed elements to add them")),
            Some(GroupPick::Remove) => t(msg!("Removing: click members to take them out")),
            None => t(msg!("Only its members can be picked and changed; the rest is greyed")),
        };
        let title = p.layout_no_wrap(format!("{what}: {name}"), egui::FontId::proportional(14.0), crate::style::pal().on_context);
        let line = p.layout_no_wrap(said.to_string(), egui::FontId::proportional(12.0), crate::style::pal().on_context);
        let size = egui::vec2(title.size().x.max(line.size().x), title.size().y + line.size().y + 2.0);
        let at = egui::pos2(rect.center().x - size.x / 2.0, rect.top() + 10.0);
        let banner = egui::Rect::from_min_size(at, size).expand2(egui::vec2(14.0, 7.0));
        p.rect_filled(banner, 6.0, ink);
        let title_h = title.size().y;
        p.galley(at, title, crate::style::pal().on_context);
        p.galley(at + egui::vec2(0.0, title_h + 2.0), line, crate::style::pal().on_context);
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
            let mode = self.group_pick();
            let tip = msg!("Add: click elements (greyed now) to add them to the group; click Add again to stop. They join at Finish");
            if big_button(ui, Icon::AddToGroup, msg!("Add"), mode == Some(GroupPick::Add), tip).clicked() {
                self.selection.clear();
                self.group_pick = (mode != Some(GroupPick::Add)).then_some(GroupPick::Add);
            }
            let tip = msg!(
                "Remove: click members to take them out of the group, staying as they are; click Remove again to stop. Members \
                 selected leave at once"
            );
            if big_button(ui, Icon::RemoveFromGroup, msg!("Remove"), mode == Some(GroupPick::Remove), tip).clicked() {
                if self.selection.is_empty() {
                    self.group_pick = (mode != Some(GroupPick::Remove)).then_some(GroupPick::Remove);
                } else {
                    let ids = std::mem::take(&mut self.selection);
                    self.run("remove_from_group", json!({"ids": ids}));
                }
            }
            let tip = msg!("Finish: the group's type becomes what it now is, and every group of it changes with it");
            if crate::inked_button(ui, Icon::Check, msg!("Finish Edit Group"), false, tip, Some(crate::style::pal().context)).clicked()
                && self.run("finish_group", json!({})).is_some()
            {
                self.selection.clear();
                self.group_pick = None;
            }
            let tip = msg!("Cancel: the group and everything else as it was when the edit began");
            if crate::inked_button(ui, Icon::Close, msg!("Cancel Edit Group"), false, tip, Some(crate::style::pal().error)).clicked()
                && self.run("cancel_group", json!({})).is_some()
            {
                self.selection.clear();
                self.group_pick = None;
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
                    "Create Group (GP): select model elements, or a view's detail lines, regions, notes and dimensions; of a mixed \
                     selection, it asks which kind to group"
                ),
            };
            // Always offered: a selection that can't make one group is asked which kind to make of it.
            if big_button(ui, Icon::CreateGroup, "Create Group", false, tip).clicked() {
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

    /// Place Model Group's Options Bar: the type it places, and whether the group placed is flexible.
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
        ui.checkbox(&mut self.group_flexible, t("Flexible")).on_hover_text(t(FLEXIBLE_TIP));
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
        let p = json!({"type": type_name, "at": [at.0, at.1], "level": level, "flexible": self.group_flexible});
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
