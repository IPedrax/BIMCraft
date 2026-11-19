//! Insert > Import 3D Model: an OBJ or glTF file's objects brought into the project as Generic Model families, each placed
//! where the file has it on the level of the plan on screen (`import_mesh`), and selected. An OBJ is taken in metres with y up,
//! as most programs write one; agents and scripts give `units` and `up` for others.

use std::path::PathBuf;

use egui::Context;
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};

impl BimApp {
    /// Insert > Import 3D Model: the file the user picks, brought in.
    pub(crate) fn import_mesh_dialog(&mut self, ctx: &Context) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("3D models (OBJ, glTF)"), &["obj", "gltf", "glb"]);
        self.ask_files(ctx, dialog, |app, paths| {
            if let Some(path) = paths.into_iter().next() {
                app.import_mesh_path(path);
            }
        });
    }

    /// Brings in the objects of the file at `path`, on the level of the plan on screen, and selects them; says on the status
    /// bar how many came in and what was left out.
    pub(crate) fn import_mesh_path(&mut self, path: PathBuf) {
        let mut p = json!({"path": path.to_string_lossy()});
        if let Some(level) = self.plan_level() {
            p["level"] = json!(level);
        }
        let Some(v) = self.run("import_mesh", p) else { return };
        self.selection = v["components"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        let left: Vec<String> =
            v["left_out"].as_object().into_iter().flatten().map(|(what, n)| format!("{} {what}", n.as_u64().unwrap_or(0))).collect();
        let mut text = tf("Brought in {} objects as Generic Model families.", &[&self.selection.len().to_string()]);
        if !left.is_empty() {
            text.push(' ');
            text.push_str(&tf("Left out: {}.", &[&left.join(", ")]));
        }
        self.status = crate::Status { text, error: false };
    }
}

impl BimApp {
    /// Insert > Image, or Link Image with `link`: the PNG or JPEG the user picks (with `pdf`, a PDF, its first page), placed on
    /// the plan on screen.
    pub(crate) fn image_dialog(&mut self, ctx: &Context, link: bool, pdf: bool) {
        let dialog = if pdf {
            rfd::AsyncFileDialog::new().add_filter(t("PDF files"), &["pdf"])
        } else {
            rfd::AsyncFileDialog::new().add_filter(t("Images (PNG, JPEG)"), &["png", "jpg", "jpeg"])
        };
        self.ask_file(ctx, dialog, false, move |app, path| {
            app.image_path(&path, link);
        });
    }

    /// The picture at `path` placed in the middle of the level of the plan on screen, and selected; with `link`, its file kept.
    /// A PDF of more than one page asks which first.
    pub(crate) fn image_path(&mut self, path: &std::path::Path, link: bool) {
        let pdf = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
        if pdf {
            let Some(v) = self.run("pdf_pages", json!({"path": path.to_string_lossy()})) else { return };
            let pages: Vec<(u64, [f64; 2])> = v["pages"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| (p["page"].as_u64().unwrap_or(1), [p["size"][0].as_f64().unwrap_or(0.0), p["size"][1].as_f64().unwrap_or(0.0)]))
                .collect();
            if pages.len() > 1 {
                self.pdf_page_box = Some(PdfPageBox { path: path.to_path_buf(), link, pages, page: 1 });
                return;
            }
        }
        self.place_image(path, link, None);
    }

    /// The picture at `path` (a PDF's page `page`) placed on the plan on screen, and selected.
    pub(crate) fn place_image(&mut self, path: &std::path::Path, link: bool, page: Option<u64>) {
        let mut p = json!({"path": path.to_string_lossy(), "link": link});
        if let Some(n) = page {
            p["page"] = json!(n);
        }
        if let Some(level) = self.plan_level() {
            p["level"] = json!(level);
        }
        let Some(v) = self.run("image", p) else { return };
        self.selection = v["id"].as_u64().into_iter().collect();
    }
}

/// Insert > PDF's question, for a PDF of more than one page: which page to place.
pub(crate) struct PdfPageBox {
    pub(crate) path: PathBuf,
    link: bool,
    /// Each page's number and its size on paper, mm.
    pub(crate) pages: Vec<(u64, [f64; 2])>,
    pub(crate) page: u64,
}

impl BimApp {
    /// The page picker, while a PDF waits for its page.
    pub(crate) fn pdf_page_window(&mut self, ctx: &Context) {
        let Some(mut b) = self.pdf_page_box.take() else { return };
        let (mut ok, mut close) = (false, false);
        let name = b.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let r = crate::popup::Modal::new(egui::Id::new("pdf-page"), t("Import PDF")).width(420.0).min_height(360.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&name).strong()).truncate());
                ui.label(tf("It has {} pages. Pick the one to place:", &[&b.pages.len().to_string()]));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(240.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (n, [w, h]) in &b.pages {
                        let text = tf("Page {}  ({} × {} mm)", &[&n.to_string(), &w.to_string(), &h.to_string()]);
                        let r = ui.selectable_label(b.page == *n, text);
                        if r.clicked() {
                            b.page = *n;
                        }
                        if r.double_clicked() {
                            ok = true;
                        }
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok |= ui.button(t("OK")).clicked();
                });
            });
        });
        if ok {
            let (path, link, page) = (b.path.clone(), b.link, b.page);
            self.place_image(&path, link, Some(page));
            return;
        }
        if !(close || r.should_close()) {
            self.pdf_page_box = Some(b);
        }
    }
}
