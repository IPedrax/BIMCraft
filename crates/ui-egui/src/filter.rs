//! Filter, as Revit's: the selection counted by category, each checked to keep or cleared to leave out. And more: Filter by
//! any parameter every selected element has (`selection::common`): its values, each with how many hold it, to keep or leave
//! out the same way. OK keeps what is checked selected.

use crate::popup::Modal;

use std::collections::BTreeSet;

use bimcraft_core::model::Id;
use bimcraft_core::selection::{self, CATEGORY, Group};
use egui::{Align, Button, ComboBox, Context, Grid, Id as UiId, Layout, RichText, ScrollArea, Ui};

use crate::BimApp;
use crate::lang::{category, t, tf};

/// The Filter dialog: the selection it opened on, the parameter it goes by, that parameter's values, and those cleared.
pub(crate) struct SelectionFilter {
    pub(crate) ids: Vec<Id>,
    pub(crate) by: String,
    pub(crate) fields: Vec<String>,
    pub(crate) groups: Vec<Group>,
    pub(crate) off: BTreeSet<String>,
}

/// The list a Filter dialog shows, the project's and a family document's (`editor/selection.rs`) alike: under `heading`
/// (translated) and Count, each of `rows` (what it is called, how many are of it) with its check box, `on`; Check All and
/// Check None; and how many are checked in all. `salt` names its grid.
pub(crate) fn checklist(ui: &mut Ui, salt: &str, heading: &str, rows: &[(String, usize)], on: &mut [bool]) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(heading).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(t("Count")).strong()));
    });
    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        Grid::new(salt).num_columns(2).striped(true).min_col_width(60.0).show(ui, |ui| {
            for ((label, count), on) in rows.iter().zip(on.iter_mut()) {
                ui.checkbox(on, label.as_str());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(count.to_string()));
                ui.end_row();
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button(t("Check All")).clicked() {
            on.fill(true);
        }
        if ui.button(t("Check None")).clicked() {
            on.fill(false);
        }
    });
    let kept: usize = rows.iter().zip(on.iter()).filter(|(_, on)| **on).map(|((_, n), _)| n).sum();
    ui.label(tf("Total selected items: {}", &[&kept.to_string()]));
}

/// A value as the list shows it: a category by its name, nothing as `(none)`.
fn shown(by: &str, value: &str) -> String {
    match (by, value) {
        (_, "") => t("(none)").to_string(),
        (CATEGORY, c) => category(c, true),
        (_, v) => v.to_string(),
    }
}

impl BimApp {
    /// Filter: the selection, by category.
    pub(crate) fn open_filter(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let m = &self.session.model;
        let ids = self.selection.clone();
        let fields = selection::common(m, &ids).into_iter().map(|(n, _)| n).collect();
        let groups = selection::groups(m, &ids, CATEGORY);
        self.selection_filter = Some(SelectionFilter { ids, by: CATEGORY.into(), fields, groups, off: BTreeSet::new() });
    }

    /// Filter by another parameter: its values, all checked.
    pub(crate) fn filter_by(&mut self, by: &str) {
        let Some(d) = self.selection_filter.as_mut() else { return };
        if d.by == by || !d.fields.iter().any(|f| f == by) {
            return;
        }
        d.groups = selection::groups(&self.session.model, &d.ids, by);
        d.by = by.to_string();
        d.off.clear();
    }

    pub(crate) fn selection_filter_window(&mut self, ctx: &Context) {
        let Some(d) = self.selection_filter.as_mut() else { return };
        let (mut ok, mut close, mut by) = (false, false, None);
        let r = Modal::new(UiId::new("selection-filter"), t("Filter")).width(380.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t("Filter by:"));
                    ComboBox::from_id_salt("selection-filter-by").selected_text(t(&d.by)).width(220.0).show_ui(ui, |ui| {
                        for f in &d.fields {
                            if ui.selectable_label(*f == d.by, t(f)).clicked() {
                                by = Some(f.clone());
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                let rows: Vec<(String, usize)> = d.groups.iter().map(|g| (shown(&d.by, &g.value), g.ids.len())).collect();
                let mut on: Vec<bool> = d.groups.iter().map(|g| !d.off.contains(&g.value)).collect();
                checklist(ui, "selection-filter-grid", t(if d.by == CATEGORY { "Category" } else { "Value" }), &rows, &mut on);
                d.off = d.groups.iter().zip(&on).filter(|(_, on)| !**on).map(|(g, _)| g.value.clone()).collect();
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add(Button::new(t("OK"))).clicked();
                });
            });
        });
        if let Some(by) = by {
            self.filter_by(&by);
        } else if ok && let Some(d) = self.selection_filter.take() {
            let keep: Vec<String> = d.groups.iter().map(|g| g.value.clone()).filter(|v| !d.off.contains(v)).collect();
            self.selection = selection::kept(&self.session.model, &d.ids, &d.by, &keep);
        } else if close || r.should_close() {
            self.selection_filter = None;
        }
    }
}
