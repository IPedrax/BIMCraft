//! Detail groups on the desktop: Create Group (GP) of the selection, Place Detail Group and Repeating Detail with the group
//! on the Options Bar, and Ungroup (UG). The core makes and draws them (`bimcraft_core::groups`).

use std::sync::Arc;

use bimcraft_core::geom::P2;
use bimcraft_core::groups::{DetailGroup, RepeatingDetail};
use bimcraft_core::model::{Element, Id, Model};
use egui::{ComboBox, Ui, WidgetInfo, WidgetType};
use serde_json::json;

use crate::lang::{t, tf};
use crate::{BimApp, Status};

impl BimApp {
    /// Create Group: the selected detail lines, filled regions and text notes made one detail group, selected.
    pub(crate) fn create_group(&mut self) {
        if self.selection.is_empty() {
            self.status = Status { text: t("select the detail lines, filled regions and text to group first").into(), error: true };
            return;
        }
        if let Some(r) = self.run("create_group", json!({"ids": self.selection})) {
            self.group_type = r["name"].as_str().unwrap_or_default().to_string();
            self.selection = r["id"].as_u64().into_iter().collect();
        }
    }

    /// Ungroup: each selected detail group's members, as elements again, selected.
    pub(crate) fn ungroup(&mut self) {
        let m = Arc::clone(&self.session.model);
        let groups: Vec<Id> =
            self.selection.iter().copied().filter(|id| matches!(m.elements.get(*id), Some(Element::DetailGroup(_)))).collect();
        if groups.is_empty() {
            self.status = Status { text: t("select a detail group to ungroup").into(), error: true };
            return;
        }
        let mut back = Vec::new();
        for id in groups {
            if let Some(r) = self.run("ungroup", json!({"id": id})) {
                back.extend(r["ids"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()));
            }
        }
        self.selection = back;
    }

    /// Whether the selection holds a detail group, for Ungroup.
    pub(crate) fn group_selected(&self) -> bool {
        self.selection.iter().any(|id| matches!(self.session.model.elements.get(*id), Some(Element::DetailGroup(_))))
    }

    /// The group type Place Detail Group places: the one chosen, else the first.
    pub(crate) fn placing_group(&self) -> Option<String> {
        let m = &self.session.model;
        m.detail_groups.contains_key(&self.group_type).then(|| self.group_type.clone()).or_else(|| m.detail_groups.keys().next().cloned())
    }

    /// The group Place Detail Group would put at `at` in view `view`.
    pub(crate) fn group_at(&self, m: &Model, view: Id, at: P2) -> Option<Element> {
        let type_name = self.placing_group()?;
        Some(Element::DetailGroup(DetailGroup { view: m.guid(view), type_name, at, angle: 0.0, mirrored: false }))
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
}
