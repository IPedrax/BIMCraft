//! Linked models: Manage > Link Model asks for a project or an IFC, then how to position it, and links it; Reload Links reads
//! the files again.

use std::path::{Path, PathBuf};

use egui::{Align, Context, Id as UiId, Layout, RichText, TextEdit};
use serde_json::json;

use crate::BimApp;
use crate::lang::t;
use crate::popup::Modal;
use crate::style::pal;

/// The ways a link can be positioned, as Revit's Link dialog names them, and what each does.
const POSITIONS: [(&str, &str, &str); 3] = [
    (
        "manual",
        bimcraft_i18n::msg!("Auto - Origin to Origin"),
        bimcraft_i18n::msg!("Its internal origin on this project's: where it was modelled, as it was modelled"),
    ),
    (
        "base",
        bimcraft_i18n::msg!("Auto - Project Base Point to Project Base Point"),
        bimcraft_i18n::msg!("Its Project Base Point on this project's, turned as the two base points are"),
    ),
    (
        "shared",
        bimcraft_i18n::msg!("Auto - By Shared Coordinates"),
        bimcraft_i18n::msg!("Where its shared coordinates put it in the world: the two projects agree on the site"),
    ),
];

/// Link Model's box: the file picked, how it is positioned, and which of its locations shared coordinates use.
pub(crate) struct LinkBox {
    path: PathBuf,
    by: &'static str,
    location: String,
    error: Option<String>,
}

impl LinkBox {
    #[cfg(test)]
    pub(crate) fn for_test(path: PathBuf) -> LinkBox {
        LinkBox { path, by: "manual", location: String::new(), error: None }
    }
}

/// What Link Model offers: a project of any of its names, or an IFC file.
pub(crate) fn link_extensions() -> Vec<&'static str> {
    let mut extensions = bimcraft_core::files::PROJECTS.to_vec();
    extensions.push("ifc");
    extensions
}

impl BimApp {
    /// Link Model (LK): the file the user picks, then its box.
    pub(crate) fn link_model_dialog(&mut self, ctx: &Context) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft project or IFC"), &link_extensions());
        self.ask_file(ctx, dialog, false, |app, path| {
            app.link_box = Some(LinkBox { path, by: "manual", location: String::new(), error: None })
        });
    }

    /// Links the project at `path` origin to origin (what tests hand a file to, as the system's dialog would), kept relative to this project's folder where it lies under it, so the two move together.
    #[cfg(test)]
    pub(crate) fn link_path(&mut self, path: &Path) {
        self.link_path_by(path, "manual", "");
    }

    /// The project at `path` linked, positioned `by` (manual, base or shared), with the link's `location` for shared
    /// coordinates (its active one when blank). Whether it was linked.
    pub(crate) fn link_path_by(&mut self, path: &Path, by: &str, location: &str) -> bool {
        let folder = self.session.path.as_deref().and_then(Path::parent);
        let kept = folder.and_then(|dir| path.strip_prefix(dir).ok()).unwrap_or(path);
        let mut p = json!({"path": kept.to_string_lossy()});
        if by != "manual" {
            p["by"] = json!(by);
        }
        if by == "shared" && !location.trim().is_empty() {
            p["location"] = json!(location.trim());
        }
        self.run("link", p).is_some()
    }

    /// Link Model's box, while it's open: the file, its positioning, OK or Cancel.
    pub(crate) fn link_window(&mut self, ctx: &Context) {
        let Some(mut b) = self.link_box.take() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("link-model"), t("Link Model")).width(460.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(b.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).strong());
                ui.add_space(8.0);
                ui.label(t("Positioning"));
                for (by, label, says) in POSITIONS {
                    ui.radio_value(&mut b.by, by, t(label)).on_hover_text(t(says));
                }
                ui.add_space(4.0);
                ui.add_enabled_ui(b.by == "shared", |ui| {
                    ui.horizontal(|ui| {
                        let l = ui.label(t("Its location"));
                        ui.add(TextEdit::singleline(&mut b.location).hint_text(t("its active one")).desired_width(200.0)).labelled_by(l.id);
                    });
                });
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if ok {
            if self.link_path_by(&b.path, b.by, &b.location) {
                return;
            }
            b.error = Some(self.status.text.clone());
        } else if close || r.should_close() {
            return;
        }
        self.link_box = Some(b);
    }
}
