//! A 3D view's Background (`view3d::background`): the row in its Properties with a button that opens the Background dialog
//! (none, a solid colour, two colours, a gradient or a picture), which runs `background` on OK or Apply; and the background
//! painted under the card's picture, which leaves clear where nothing is drawn. The processor's picture has the background
//! painted in by the core, as `render`'s PNG has.

use std::path::PathBuf;

use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::view3d::background::{Background, Fill, Fit, GROUND, HAZE, MAX_STOPS, SKY, Stop, gradient_at};
use egui::{Align, Color32, Context, Id as UiId, Layout, Painter, Pos2, Rect, RichText, Sense, TextureHandle, Ui, pos2, vec2};
use serde_json::json;

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// The kinds, as the dialog names them, in the command's order.
const KINDS: [(&str, &str); 5] = [
    ("none", bimcraft_i18n::msg!("None")),
    ("solid", bimcraft_i18n::msg!("Solid Color")),
    ("two_color", bimcraft_i18n::msg!("Two Colors")),
    ("gradient", bimcraft_i18n::msg!("Gradient")),
    ("image", bimcraft_i18n::msg!("Image")),
];

/// How a picture lies, as the dialog names it.
fn fit_name(f: Fit) -> &'static str {
    match f {
        Fit::Fill => bimcraft_i18n::msg!("Fill the view"),
        Fit::Fit => bimcraft_i18n::msg!("Fit inside the view"),
        Fit::Stretch => bimcraft_i18n::msg!("Stretch over the view"),
    }
}

/// A kind's name in the dialog.
pub(crate) fn kind_name(kind: &str) -> &'static str {
    KINDS.iter().find(|(k, _)| *k == kind).map_or("None", |(_, n)| n)
}

/// The Background dialog, until OK, Apply or Cancel: the kind picked, each colour, a gradient's colours with where each is,
/// the picture (the one the view has, or a file picked to take in) and how it lies, the horizon and its ground's colour; and why
/// the last Apply was refused.
pub(crate) struct BackgroundDialog {
    pub(crate) kind: &'static str,
    pub(crate) color: [u8; 3],
    pub(crate) top: [u8; 3],
    pub(crate) bottom: [u8; 3],
    pub(crate) stops: Vec<Stop>,
    pub(crate) picture: Option<String>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) fit: Fit,
    pub(crate) horizon: bool,
    pub(crate) ground: [u8; 3],
    error: Option<String>,
}

impl BackgroundDialog {
    fn of(b: &Background) -> BackgroundDialog {
        let mut d = BackgroundDialog {
            kind: KINDS.iter().map(|(k, _)| *k).find(|k| *k == b.fill.kind()).unwrap_or("none"),
            color: [210, 220, 230],
            top: SKY,
            bottom: GROUND,
            stops: vec![Stop { at: 0.0, color: SKY }, Stop { at: 50.0, color: HAZE }, Stop { at: 100.0, color: GROUND }],
            picture: None,
            path: None,
            fit: Fit::Fill,
            horizon: b.ground.is_some(),
            ground: b.ground.unwrap_or(GROUND),
            error: None,
        };
        match &b.fill {
            Fill::None => {}
            Fill::Solid { color } => d.color = *color,
            Fill::TwoColor { top, bottom } => (d.top, d.bottom) = (*top, *bottom),
            Fill::Gradient { stops } => d.stops.clone_from(stops),
            Fill::Image { picture, fit } => (d.picture, d.fit) = (Some(picture.clone()), *fit),
        }
        d
    }

    /// What `background` is asked, for `view`.
    fn params(&self, view: serde_json::Value) -> serde_json::Value {
        let ground = if self.horizon { json!(self.ground) } else { json!(false) };
        let mut p = json!({"view": view, "kind": self.kind, "ground": ground});
        match self.kind {
            "solid" => p["color"] = json!(self.color),
            "two_color" => (p["top"], p["bottom"]) = (json!(self.top), json!(self.bottom)),
            "gradient" => p["stops"] = json!(self.stops.iter().map(|s| json!([s.at, s.color])).collect::<Vec<_>>()),
            "image" => {
                p["fit"] = json!(self.fit.name());
                match (&self.path, &self.picture) {
                    (Some(path), _) => p["path"] = json!(path),
                    (None, Some(picture)) => p["picture"] = json!(picture),
                    (None, None) => {}
                }
            }
            _ => {}
        }
        p
    }

    /// The fill it stands for, as far as it can say without the picture taken in: for its preview.
    fn preview(&self) -> Fill {
        match self.kind {
            "solid" => Fill::Solid { color: self.color },
            "two_color" => Fill::TwoColor { top: self.top, bottom: self.bottom },
            "gradient" => Fill::Gradient { stops: self.stops.clone() },
            _ => Fill::None,
        }
    }
}

/// The background of 3D view `view` of `model` (none: the default `{3D}`).
pub(crate) fn of(model: &Model, view: Option<Id>) -> Background {
    let id = view.filter(|v| matches!(model.elements.get(*v), Some(Element::View3D(_)))).or_else(|| model.default_3d());
    match id.and_then(|v| model.elements.get(v)) {
        Some(Element::View3D(v)) => v.background.clone(),
        _ => Background::default(),
    }
}

/// Paint background `b` over `rect`, under the picture: its fill (colours in bands, or the picture `texture` holds, `size` px,
/// placed as it says, paper round it; `paper` where it shows nothing), and with a horizon, the ground from `horizon` (its height
/// on screen) down.
pub(crate) fn paint(
    p: &Painter,
    rect: Rect,
    b: &Background,
    picture: Option<(&TextureHandle, [usize; 2])>,
    paper: Color32,
    horizon: Option<f32>,
) {
    paint_fill(p, rect, &b.fill, picture, paper);
    if let (Some(g), Some(y)) = (b.ground, horizon.filter(|y| !y.is_nan())) {
        let top = y.clamp(rect.top(), rect.bottom());
        if top < rect.bottom() {
            p.rect_filled(Rect::from_min_max(pos2(rect.left(), top), rect.max), 0.0, Color32::from_rgb(g[0], g[1], g[2]));
        }
    }
}

/// A fill over `rect`.
fn paint_fill(p: &Painter, rect: Rect, fill: &Fill, picture: Option<(&TextureHandle, [usize; 2])>, paper: Color32) {
    let c = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    match fill {
        Fill::None => {
            p.rect_filled(rect, 0.0, paper);
        }
        Fill::Solid { color } => {
            p.rect_filled(rect, 0.0, c(*color));
        }
        Fill::TwoColor { top, bottom } => {
            let mid = rect.center().y;
            p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, mid)), 0.0, c(*top));
            p.rect_filled(Rect::from_min_max(pos2(rect.min.x, mid), rect.max), 0.0, c(*bottom));
        }
        Fill::Gradient { stops } => {
            // A band between each two places a colour changes, blended from its top row's colour to its bottom row's; the ends
            // as far as the view's edges.
            let mut at: Vec<f64> = stops.iter().map(|s| s.at.clamp(0.0, 100.0)).chain([0.0, 100.0]).collect();
            at.sort_by(f64::total_cmp);
            at.dedup();
            let mut mesh = egui::Mesh::default();
            for (i, pct) in at.iter().enumerate() {
                let y = rect.top() + rect.height() * (*pct / 100.0) as f32;
                let colour = c(gradient_at(stops, *pct).unwrap_or([255, 255, 255]));
                mesh.colored_vertex(pos2(rect.min.x, y), colour);
                mesh.colored_vertex(pos2(rect.max.x, y), colour);
                if i > 0 {
                    let k = (i as u32 - 1) * 2;
                    mesh.add_triangle(k, k + 1, k + 2);
                    mesh.add_triangle(k + 1, k + 3, k + 2);
                }
            }
            p.add(egui::Shape::mesh(mesh));
        }
        Fill::Image { fit, .. } => {
            p.rect_filled(rect, 0.0, paper);
            let Some((texture, [w, h])) = picture else { return };
            let (on, uv) = fit.place((w as f64, h as f64), (f64::from(rect.width()), f64::from(rect.height())));
            let at = |x: f64, y: f64| rect.min + vec2(rect.width() * x as f32, rect.height() * y as f32);
            let shown = Rect::from_min_max(at(on[0], on[1]), at(on[2], on[3]));
            let uv = Rect::from_min_max(Pos2::new(uv[0] as f32, uv[1] as f32), Pos2::new(uv[2] as f32, uv[3] as f32));
            p.image(texture.id(), shown, uv, Color32::WHITE);
        }
    }
}

impl BimApp {
    /// The background of the 3D view the 3D tab shows.
    pub(crate) fn background_now(&self) -> Background {
        of(&self.session.model, self.active_3d())
    }

    /// The Background row of a 3D view's Properties: what it is, and the button that opens the dialog.
    pub(crate) fn background_properties(&mut self, ui: &mut Ui) {
        let now = self.background_now();
        let mut open = false;
        crate::properties::grid(ui, "background-properties", |ui| {
            let label = ui.label(t("Background"));
            let r = ui
                .button(tf("{}…", &[t(kind_name(now.fill.kind()))]))
                .labelled_by(label.id)
                .on_hover_text(t("What the 3D view shows where the model doesn't: a colour, two colours, a gradient or a picture"));
            open = r.clicked();
            ui.end_row();
        });
        if open {
            self.open_background();
        }
    }

    /// The Background dialog, for the 3D view; in another view, says it is the 3D view's.
    pub(crate) fn open_background(&mut self) {
        if self.sheet == Sheet::View3D {
            self.background_dialog = Some(BackgroundDialog::of(&self.background_now()));
        } else {
            self.status = Status { text: t("a background is a 3D view's: open one from the Project Browser").into(), error: true };
        }
    }

    pub(crate) fn background_window(&mut self, ctx: &Context) {
        let Some(d) = self.background_dialog.as_mut() else { return };
        let (mut ok, mut apply, mut close, mut browse) = (false, false, false, false);
        let r = Modal::new(UiId::new("background-dialog"), t("Background")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (kind, name) in KINDS {
                        if ui.radio(d.kind == kind, t(name)).clicked() {
                            d.kind = kind;
                        }
                    }
                });
                ui.add_space(8.0);
                let colour = |ui: &mut Ui, label: &str, c: &mut [u8; 3]| {
                    ui.horizontal(|ui| {
                        let l = ui.label(t(label));
                        ui.add_space((110.0 - l.rect.width()).max(0.0));
                        egui::color_picker::color_edit_button_srgb(ui, c).labelled_by(l.id);
                    });
                };
                let stops_n = d.stops.len();
                match d.kind {
                    "solid" => colour(ui, bimcraft_i18n::msg!("Color"), &mut d.color),
                    "two_color" => {
                        colour(ui, bimcraft_i18n::msg!("Top (sky)"), &mut d.top);
                        colour(ui, bimcraft_i18n::msg!("Bottom (ground)"), &mut d.bottom);
                    }
                    "gradient" => {
                        // Each colour and where it is, percent down the view; added and taken away.
                        let mut gone = None;
                        for (i, stop) in d.stops.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                egui::color_picker::color_edit_button_srgb(ui, &mut stop.color);
                                ui.add(egui::DragValue::new(&mut stop.at).range(0.0..=100.0).speed(0.5).suffix(" %"));
                                if ui.add_enabled(stops_n > 1, egui::Button::new(t("Remove"))).clicked() {
                                    gone = Some(i);
                                }
                            });
                        }
                        if let Some(i) = gone {
                            d.stops.remove(i);
                        }
                        if ui.add_enabled(d.stops.len() < MAX_STOPS, egui::Button::new(t("Add Color"))).clicked() {
                            // Halfway between the last two, or at the bottom.
                            let at = match d.stops.as_slice() {
                                [.., a, b] => (a.at + b.at) / 2.0,
                                _ => 100.0,
                            };
                            let color = gradient_at(&d.stops, at).unwrap_or(HAZE);
                            d.stops.push(Stop { at, color });
                        }
                    }
                    "image" => {
                        ui.horizontal(|ui| {
                            let l = ui.label(t("Image"));
                            let shown = match (&d.path, &d.picture) {
                                (Some(p), _) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
                                (None, Some(id)) => id.clone(),
                                (None, None) => t("<none>").to_string(),
                            };
                            ui.label(RichText::new(shown).strong()).labelled_by(l.id);
                            browse = ui.button(t("Browse…")).clicked();
                        });
                        ui.horizontal(|ui| {
                            let l = ui.label(t("Fit"));
                            let c = egui::ComboBox::from_id_salt("background-fit").selected_text(t(fit_name(d.fit))).show_ui(ui, |ui| {
                                for f in Fit::ALL {
                                    ui.selectable_value(&mut d.fit, f, t(fit_name(f)));
                                }
                            });
                            c.response.labelled_by(l.id);
                        });
                    }
                    _ => {
                        ui.label(RichText::new(t("The canvas's own colour, as in every other view.")).color(pal().muted));
                    }
                }
                // The horizon: the ground's colour under elevation 0, over whatever fills the rest.
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut d.horizon, t("Horizon"))
                        .on_hover_text(t("At elevation 0 and below the view shows one colour, its edge following the camera as it turns"));
                    ui.add_enabled_ui(d.horizon, |ui| {
                        let l = ui.label(t("Ground"));
                        egui::color_picker::color_edit_button_srgb(ui, &mut d.ground).labelled_by(l.id);
                    });
                });
                // What it will look like, for colours: the horizon shown at the middle.
                let preview = Background { fill: d.preview(), ground: d.horizon.then_some(d.ground) };
                if !preview.is_none() {
                    ui.add_space(8.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(380.0), 60.0), Sense::hover());
                    paint(ui.painter(), rect, &preview, None, Color32::WHITE, Some(rect.center().y));
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
                }
                if let Some(e) = &d.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if browse {
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("Images"), &["png", "jpg", "jpeg"]);
            self.ask_file(ctx, dialog, false, |app, path| {
                if let Some(d) = app.background_dialog.as_mut() {
                    d.path = Some(path);
                    d.error = None;
                }
            });
        } else if ok || apply {
            if self.apply_background() && ok {
                self.background_dialog = None;
            }
        } else if close || r.should_close() {
            self.background_dialog = None;
        }
    }

    /// Runs `background` on the 3D view with what the dialog has: whether the view has it now. A refusal is said in the dialog.
    fn apply_background(&mut self) -> bool {
        let view = self.active_3d().map_or_else(|| json!("{3D}"), |id| json!(id));
        let Some(p) = self.background_dialog.as_ref().map(|d| d.params(view)) else { return false };
        match self.run("background", p) {
            Some(done) => {
                // The picture taken in is the project's now: asked for again by its id.
                if let Some(d) = self.background_dialog.as_mut() {
                    d.picture = done["background"]["fill"]["picture"].as_str().map(str::to_owned).or(d.picture.take());
                    (d.path, d.error) = (None, None);
                }
                true
            }
            None => {
                let e = std::mem::take(&mut self.status).text;
                if let Some(d) = self.background_dialog.as_mut() {
                    d.error = Some(e);
                }
                false
            }
        }
    }
}
