//! Manage > Purge Unused: what nothing in the project uses, listed by what it is, to take out together; the commands are
//! `purge` and `delete_type`. A checked kind's unused entries are listed as a dry run finds them, each checked, and Purge runs
//! the same, keeping what is unchecked (`purge {keep}`), as Revit's tree of what to purge does.

use crate::popup::Modal;

use bimcraft_i18n::msg;

use bimcraft_core::library::PURGE_KINDS;
use egui::{Context, Id as UiId, RichText, ScrollArea};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::style::pal;

/// What the window names each kind it can purge.
const LABELS: [&str; 7] = ["Types", "Families", "Materials", "Pictures", msg!("View Filters"), "View Templates", msg!("Title Blocks")];

/// The Purge Unused window: which kinds are checked, what that would take out (kind and name), and which of those are
/// unchecked, to keep.
pub struct PurgeDialog {
    pub(crate) kinds: [bool; 7],
    pub(crate) listed: Vec<(String, String)>,
    pub(crate) keep: Vec<(String, String)>,
}

impl BimApp {
    /// Opens Purge Unused, as Revit's does: types and families checked.
    pub(crate) fn open_purge(&mut self) {
        let mut d = PurgeDialog { kinds: [true, true, false, false, false, false, false], listed: Vec::new(), keep: Vec::new() };
        self.relist_purge(&mut d);
        self.purge_dialog = Some(d);
    }

    /// What the checked kinds would take out: a dry run of the command.
    fn relist_purge(&mut self, d: &mut PurgeDialog) {
        let kinds: Vec<&str> = PURGE_KINDS.iter().zip(d.kinds).filter(|(_, on)| *on).map(|(k, _)| *k).collect();
        let reply = self.exec("purge", &json!({"kinds": kinds, "dry_run": true})).unwrap_or_default();
        d.listed = reply["removed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| (e["kind"].as_str().unwrap_or_default().to_string(), e["name"].as_str().unwrap_or_default().to_string()))
            .collect();
        let listed = &d.listed;
        d.keep.retain(|e| listed.contains(e));
    }

    pub(crate) fn purge_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.purge_dialog.take() else { return };
        let (mut close, mut purge, mut changed) = (false, false, false);
        let r = Modal::new(UiId::new("purge-unused"), t("Purge Unused")).width(520.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t("Takes out what nothing in the project uses. Each category keeps a type; Undo brings it back."))
                        .color(pal().muted),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for (on, label) in d.kinds.iter_mut().zip(LABELS) {
                        changed |= ui.checkbox(on, t(label)).changed();
                    }
                });
                ui.add_space(6.0);
                ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                    if d.listed.is_empty() {
                        ui.label(RichText::new(t("Nothing is unused.")).color(pal().muted));
                    }
                    for entry in &d.listed {
                        let mut on = !d.keep.contains(entry);
                        if ui.checkbox(&mut on, format!("{}: {}", t(&entry.0), entry.1)).changed() {
                            if on {
                                d.keep.retain(|e| e != entry);
                            } else {
                                d.keep.push(entry.clone());
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(t("Check All")).clicked() {
                        d.keep.clear();
                        ui.ctx().request_repaint();
                    }
                    if ui.button(t("Check None")).clicked() {
                        d.keep = d.listed.clone();
                        ui.ctx().request_repaint();
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    let count = d.listed.len().saturating_sub(d.keep.len());
                    if ui.add_enabled(count > 0, egui::Button::new(tf("Purge {}", &[&count.to_string()]))).clicked() {
                        purge = true;
                    }
                    close = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if changed {
            self.relist_purge(&mut d);
        }
        if purge {
            let kinds: Vec<&str> = PURGE_KINDS.iter().zip(d.kinds).filter(|(_, on)| *on).map(|(k, _)| *k).collect();
            let keep: Vec<Value> = d.keep.iter().map(|(kind, name)| json!({"kind": kind, "name": name})).collect();
            let done: Option<Value> = self.run("purge", json!({"kinds": kinds, "keep": keep}));
            let count = done.map_or(0, |r| r["count"].as_u64().unwrap_or(0));
            self.status = crate::Status { text: tf("Purged {} unused.", &[&count.to_string()]), error: false };
            close = true;
        }
        if !(close || r.should_close()) {
            self.purge_dialog = Some(d);
        }
    }
}
