//! View > Schedules > Material Takeoff, and the Quantities dialog's own button: the dialog that makes a
//! material takeoff view (`takeoff`) — which categories it counts, how it groups them, the waste it adds, the
//! level and the name — and opens it as the table a schedule is. It is the window's way to the command;
//! nothing is counted here, the core's `takeoff` is.

use bimcraft_core::model::{Id, Model};
use bimcraft_core::takeoff::{By, CATEGORIES, MAX_WASTE};
use egui::{Align, Button, ComboBox, Context, DragValue, Id as UiId, Layout, TextEdit};
use serde_json::json;

use crate::lang::{self, t};
use crate::popup::{self, Modal};
use crate::{BimApp, Sheet};

/// What the dialog holds before it makes a takeoff: the categories checked, the grouping, the waste, the
/// level (none: the whole project) and the name typed.
#[derive(Clone, Debug)]
pub(crate) struct NewTakeoff {
    categories: [bool; 4],
    by: By,
    waste: f64,
    level: Option<Id>,
    name: String,
}

impl NewTakeoff {
    /// A takeoff of every category, by material, with no waste, over the whole project, named as the project's
    /// language names it and as no view is yet.
    pub(crate) fn new(m: &Model) -> NewTakeoff {
        NewTakeoff { categories: [true; 4], by: By::Material, waste: 0.0, level: None, name: bimcraft_core::takeoff::free_name(m) }
    }
}

/// The English name of a grouping, as the dialog's buttons say it.
fn by_label(by: By) -> &'static str {
    match by {
        By::Material => "Material",
        By::Layer => "Layer",
        By::Type => "Type",
    }
}

impl BimApp {
    /// The Material Takeoff dialog, while it is open. OK makes the view and opens it; a refusal stays in it,
    /// its reason in the status bar, to be named again.
    pub(crate) fn new_takeoff_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.new_takeoff.take() else { return };
        let levels: Vec<(Id, String)> = self.session.model.levels().into_iter().map(|(id, l)| (id, l.name.clone())).collect();
        let (mut ok, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("new-takeoff"), t("New Material Takeoff")).width(420.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t("Categories:"));
                for (i, cat) in CATEGORIES.iter().enumerate() {
                    ui.checkbox(&mut d.categories[i], lang::category(cat, true));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t("Group by:"));
                    for by in [By::Material, By::Layer, By::Type] {
                        if ui.selectable_label(d.by == by, t(by_label(by))).clicked() {
                            d.by = by;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let l = ui.label(t("Waste:"));
                    ui.add(DragValue::new(&mut d.waste).range(0.0..=MAX_WASTE).speed(0.5).suffix(" %")).labelled_by(l.id);
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let l = ui.label(t("Level:"));
                    let picked = d
                        .level
                        .and_then(|id| levels.iter().find(|(level, _)| *level == id))
                        .map_or_else(|| t("Whole project"), |(_, name)| name.as_str());
                    let c = ComboBox::from_id_salt("takeoff-level").selected_text(picked).width(220.0).show_ui(ui, |ui| {
                        if ui.selectable_label(d.level.is_none(), t("Whole project")).clicked() {
                            d.level = None;
                        }
                        for (id, name) in &levels {
                            if ui.selectable_label(d.level == Some(*id), name).clicked() {
                                d.level = Some(*id);
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                });
                ui.add_space(6.0);
                let l = ui.label(t("Name:"));
                ui.add(TextEdit::singleline(&mut d.name).desired_width(340.0)).labelled_by(l.id);
            });
            popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(d.ready(), Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok {
            let categories: Vec<&str> = CATEGORIES.iter().zip(&d.categories).filter(|(_, on)| **on).map(|(c, _)| *c).collect();
            let mut p = json!({"name": d.name.trim(), "categories": categories, "by": d.by.name(), "waste": d.waste});
            if let Some(level) = d.level {
                p["level"] = json!(level);
            }
            if let Some(made) = self.run("takeoff", p) {
                if let Some(id) = made["id"].as_u64() {
                    self.open_sheet(Sheet::Table(id));
                }
                return;
            }
        }
        // Refused, it stays open, the status saying why, to be named again.
        if !(cancel || r.should_close()) {
            self.new_takeoff = Some(d);
        }
    }
}

impl NewTakeoff {
    /// Whether it can be made: a category, a name and a waste a takeoff takes.
    fn ready(&self) -> bool {
        self.categories.iter().any(|on| *on)
            && !self.name.trim().is_empty()
            && self.waste.is_finite()
            && (0.0..=MAX_WASTE).contains(&self.waste)
    }
}
