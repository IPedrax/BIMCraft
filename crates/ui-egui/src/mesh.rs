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
                self.pdf_page_box = Some(PdfPageBox::new(path, link, pages));
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
        self.say_missing_fonts(&v);
    }

    /// Says which fonts a PDF just placed or reloaded names without holding them and draws in a stand-in, each once.
    pub(crate) fn say_missing_fonts(&mut self, v: &serde_json::Value) {
        let names: Vec<&str> = v["missing_fonts"].as_array().into_iter().flatten().filter_map(|n| n.as_str()).collect();
        if !names.is_empty() {
            let text = crate::lang::tf("Fonts the PDF doesn't hold, drawn in a stand-in: {}", &[&names.join(", ")]);
            self.status = crate::Status { text, error: true };
        }
    }
}

/// Insert > PDF's question, for a PDF of more than one page: which page to place, each shown small as it looks.
pub(crate) struct PdfPageBox {
    pub(crate) path: PathBuf,
    link: bool,
    /// Each page's number and its size on paper, mm.
    pub(crate) pages: Vec<(u64, [f64; 2])>,
    pub(crate) page: u64,
    /// Each page's picture, by its number, as the thread drawing them hands them over.
    pub(crate) thumbs: std::collections::HashMap<u64, egui::TextureHandle>,
    drawn: Option<std::sync::mpsc::Receiver<(u64, egui::ColorImage)>>,
    /// Set when the box closes, so the thread stops drawing.
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// How big a page's picture is in the page picker, px on its longer side.
const THUMB: u32 = 150;

impl PdfPageBox {
    /// The picker for `pages` of the PDF at `path`, its pages drawn small on a thread of their own, so a long PDF doesn't hold the
    /// window up.
    pub(crate) fn new(path: &std::path::Path, link: bool, pages: Vec<(u64, [f64; 2])>) -> PdfPageBox {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (file, halt) = (path.to_path_buf(), stop.clone());
        let _ = std::thread::Builder::new().name("pdf-thumbnails".into()).spawn(move || {
            // A hostile PDF that panics hayro ends this thread, nothing more.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let small = std::fs::metadata(&file).is_ok_and(|m| m.len() <= bimcraft_core::pdf_in::MAX_BYTES);
                let Some(bytes) = small.then(|| std::fs::read(&file).ok()).flatten() else { return };
                let _ = bimcraft_core::pdf_in::thumbnails(bytes, THUMB, |n, w, h, rgba| {
                    let image = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &rgba);
                    !halt.load(std::sync::atomic::Ordering::Relaxed) && tx.send((n as u64, image)).is_ok()
                });
            }));
        });
        PdfPageBox { path: path.to_path_buf(), link, pages, page: 1, thumbs: Default::default(), drawn: Some(rx), stop }
    }

    /// The pages drawn since the last frame, made textures; whether more are coming.
    fn take_drawn(&mut self, ctx: &Context) -> bool {
        let Some(rx) = &self.drawn else { return false };
        loop {
            match rx.try_recv() {
                Ok((n, image)) => {
                    let texture = ctx.load_texture(format!("pdf-page-{n}"), image, egui::TextureOptions::LINEAR);
                    self.thumbs.insert(n, texture);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.drawn = None;
                    return false;
                }
            }
        }
    }
}

impl Drop for PdfPageBox {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl BimApp {
    /// The page picker, while a PDF waits for its page.
    pub(crate) fn pdf_page_window(&mut self, ctx: &Context) {
        let Some(mut b) = self.pdf_page_box.take() else { return };
        if b.take_drawn(ctx) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let (mut ok, mut close) = (false, false);
        let name = b.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let r = crate::popup::Modal::new(egui::Id::new("pdf-page"), t("Import PDF")).width(560.0).min_height(480.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&name).strong()).truncate());
                ui.label(tf("It has {} pages. Pick the one to place:", &[&b.pages.len().to_string()]));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (n, size) in &b.pages {
                            let r = page_card(ui, *n, *size, b.thumbs.get(n), b.page == *n);
                            if r.clicked() {
                                b.page = *n;
                            }
                            if r.double_clicked() {
                                b.page = *n;
                                ok = true;
                            }
                        }
                    });
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

/// A page of the picker: its picture (or, until it's drawn, a blank of its shape), its number and its size, outlined when
/// `picked`; clicked to pick it, double-clicked to place it.
fn page_card(ui: &mut egui::Ui, n: u64, [w, h]: [f64; 2], thumb: Option<&egui::TextureHandle>, picked: bool) -> egui::Response {
    let side = THUMB as f32;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(side + 16.0, side + 52.0), egui::Sense::click());
    let text = tf("Page {}  ({} × {} mm)", &[&n.to_string(), &w.to_string(), &h.to_string()]);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, picked, &text));
    let p = ui.painter();
    let pal = crate::style::pal();
    if picked || r.hovered() {
        let fill = if picked { pal.select_fill } else { pal.hover_fill };
        p.rect_filled(rect, 6.0, fill);
    }
    if picked {
        p.rect_stroke(rect, 6.0, egui::Stroke::new(1.5, pal.accent), egui::StrokeKind::Inside);
    }
    // The page as its paper is shaped, as big as fits the square above its name.
    let aspect = if w > 0.0 && h > 0.0 { (w / h) as f32 } else { 1.0 / 2f32.sqrt() };
    let size = if aspect >= 1.0 { egui::vec2(side, side / aspect) } else { egui::vec2(side * aspect, side) };
    let square = egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(side, side));
    let page = egui::Rect::from_center_size(square.center(), size);
    p.rect_filled(page, 0.0, egui::Color32::WHITE);
    if let Some(t) = thumb {
        p.image(t.id(), page, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
    p.rect_stroke(page, 0.0, egui::Stroke::new(1.0, pal.border), egui::StrokeKind::Outside);
    let below = egui::pos2(rect.center().x, square.bottom() + 6.0);
    p.text(below, egui::Align2::CENTER_TOP, tf("Page {}", &[&n.to_string()]), egui::FontId::proportional(12.5), pal.text);
    let mm = format!("{w} × {h} mm");
    p.text(below + egui::vec2(0.0, 16.0), egui::Align2::CENTER_TOP, mm, egui::FontId::proportional(11.0), pal.muted);
    r
}
