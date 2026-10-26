//! Manage > Purge Unused: what nothing in the project uses, listed by what it is, to take out together; the commands are
//! `purge` and `delete_type`. A checked kind's unused entries are listed as a dry run finds them, and Purge runs the same.

use bimcraft_i18n::msg;

use bimcraft_core::library::PURGE_KINDS;
use egui::{Context, Id as UiId, Modal, RichText, ScrollArea};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::style::pal;

/// What the window names each kind it can purge.
const LABELS: [&str; 6] = ["Types", "Families", "Materials", "Pictures", msg!("View Filters"), "View Templates"];

/// The Purge Unused window: which kinds are checked, and what that would take out (kind and name).
pub struct PurgeDialog {
    pub(crate) kinds: [bool; 6],
    pub(crate) listed: Vec<(String, String)>,
}

impl BimApp {
    /// Opens Purge Unused, as Revit's does: types and families checked.
    pub(crate) fn open_purge(&mut self) {
        let mut d = PurgeDialog { kinds: [true, true, false, false, false, false], listed: Vec::new() };
        self.relist_purge(&mut d);
        self.purge_dialog = Some(d);
    }

    /// What the checked kinds would take out: a dry run of the command.
    fn relist_purge(&mut self, d: &mut PurgeDialog) {
        let kinds: Vec<&str> = PURGE_KINDS.iter().zip(d.kinds).filter(|(_, on)| *on).map(|(k, _)| *k).collect();
        let reply = self.session.execute("purge", &json!({"kinds": kinds, "dry_run": true})).unwrap_or_default();
        d.listed = reply["removed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| (e["kind"].as_str().unwrap_or_default().to_string(), e["name"].as_str().unwrap_or_default().to_string()))
            .collect();
    }

    pub(crate) fn purge_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.purge_dialog.take() else { return };
        let (mut close, mut purge, mut changed) = (false, false, false);
        let r = Modal::new(UiId::new("purge-unused")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(t("Purge Unused")).heading());
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
                for (kind, name) in &d.listed {
                    ui.label(format!("{}: {name}", t(kind)));
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let count = d.listed.len();
                if ui.add_enabled(count > 0, egui::Button::new(tf("Purge {}", &[&count.to_string()]))).clicked() {
                    purge = true;
                }
                close = ui.button(t("Cancel")).clicked();
            });
        });
        if changed {
            self.relist_purge(&mut d);
        }
        if purge {
            let kinds: Vec<&str> = PURGE_KINDS.iter().zip(d.kinds).filter(|(_, on)| *on).map(|(k, _)| *k).collect();
            let done: Option<Value> = self.run("purge", json!({"kinds": kinds}));
            let count = done.map_or(0, |r| r["count"].as_u64().unwrap_or(0));
            self.status = crate::Status { text: tf("Purged {} unused.", &[&count.to_string()]), error: false };
            close = true;
        }
        if !(close || r.should_close()) {
            self.purge_dialog = Some(d);
        }
    }
}
