//! A separate, reversible draft inside the Line Styles draft.
use crate::popup::Modal;

use super::*;
use bimcraft_core::{
    lines::{MAX_SEGMENT, MAX_SEGMENTS, MIN_SEGMENT},
    render::{Prim, Rgb},
};
use egui::{DragValue, Rect, Stroke, pos2};

pub(in crate::linestyles) struct PatternEditor {
    row: Option<usize>,
    name: String,
    pub(in crate::linestyles) segments: Vec<Segment>,
    curved: bool,
    zoom: f32,
    error: Option<String>,
}
impl PatternEditor {
    pub(super) fn new(row: Option<usize>, name: String, segments: Vec<Segment>) -> Self {
        Self { row, name, segments, curved: false, zoom: 8.0, error: None }
    }
}

/// Paint the core's geometry, including the same glyph outlines used in SVG, PNG and PDF.
pub(super) fn paint(ui: &Ui, segments: &[Segment], rect: Rect, k: f32, curved: bool) {
    let ink = ui.visuals().text_color();
    paint_with(ui, segments, rect, k, curved, ink);
}

/// A straight sample of `segments` in `ink`, a solid line where there are none: a row of a Line Style list.
pub(super) fn paint_in(ui: &Ui, segments: &[Segment], rect: Rect, k: f32, ink: egui::Color32) {
    if segments.is_empty() {
        ui.painter().line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.5, ink));
    } else {
        paint_with(ui, segments, rect, k, false, ink);
    }
}

fn paint_with(ui: &Ui, segments: &[Segment], rect: Rect, k: f32, curved: bool, ink: egui::Color32) {
    let painter = ui.painter().with_clip_rect(rect);
    let pts: Vec<_> = (0..=120)
        .map(|i| {
            let f = i as f32 / 120.0;
            (
                rect.left() + f * rect.width(),
                rect.center().y + if curved { (f * std::f32::consts::TAU).sin() * rect.height() * 0.22 } else { 0.0 },
            )
        })
        .collect();
    if rect.height() > 40.0 {
        painter.add(egui::Shape::line(
            pts.iter().map(|&(x, y)| pos2(x, y)).collect(),
            Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.35)),
        ));
    }
    for prim in LinePattern(segments.to_vec()).draw(&pts, k, 1.8, Rgb(ink.r(), ink.g(), ink.b())) {
        match prim {
            Prim::Line { pts, width, .. } => {
                painter.add(egui::Shape::line(pts.into_iter().map(|(x, y)| pos2(x, y)).collect(), Stroke::new(width, ink)));
            }
            Prim::Fill { rings, .. } => {
                let mut mesh = egui::Mesh::default();
                for triangle in bimcraft_core::text::triangles(&rings) {
                    let start = mesh.vertices.len() as u32;
                    for (x, y) in triangle {
                        mesh.colored_vertex(pos2(x, y), ink);
                    }
                    mesh.add_triangle(start, start + 1, start + 2);
                }
                painter.add(egui::Shape::mesh(mesh));
            }
            _ => {}
        }
    }
}

fn kind(segment: &Segment) -> usize {
    match segment {
        Segment::Space(_) => 0,
        Segment::Dash(_) => 1,
        Segment::Dot | Segment::DotLength(_) => 2,
        Segment::Offset { .. } => 3,
        Segment::Wave { .. } => 4,
        Segment::Zigzag { .. } => 5,
        Segment::Text { .. } => 6,
    }
}
const KINDS: [&str; 7] = [msg!("Gap"), msg!("Straight"), "Dot", msg!("Offset"), msg!("Wave"), msg!("Zig Zag"), "Text"];
fn replacement(index: usize, length: f64) -> Segment {
    match index {
        0 => Segment::Space(length),
        1 => Segment::Dash(length),
        2 => Segment::DotLength(length),
        3 => Segment::Offset { length, count: 2, distance: 0.5 },
        4 => Segment::Wave { length, frequency: 2.0, amplitude: 1.0 },
        5 => Segment::Zigzag { length, frequency: 2.0, amplitude: 1.0 },
        _ => Segment::Text { length: length.max(12.0), size: 2.5, text: "GAS".into() },
    }
}
fn number(ui: &mut Ui, value: &mut f64, label: String, min: f64, max: f64, suffix: &str) {
    let r = ui.add(DragValue::new(value).range(min..=max).speed(0.05).suffix(suffix));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, label.clone()));
}

pub(super) fn window(ctx: &Context, d: &mut LineStylesDialog) {
    let Some(mut editor) = d.creator.take() else { return };
    let (mut save, mut cancel) = (false, false);
    let response = Modal::new(UiId::new("line-pattern-creator"), t("Line Pattern Creator")).width(680.0).show(ctx, |ui| {
        crate::popup::body(ui, |ui| {
            ui.horizontal(|ui| {
                let label = ui.label(t("Pattern Name"));
                let r = ui.add_sized([320.0, 24.0], TextEdit::singleline(&mut editor.name));
                r.labelled_by(label.id);
            });
            ui.add_space(8.0);
            ui.label(RichText::new(t("Preview")).strong());
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 110.0), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
            paint(ui, &editor.segments, rect.shrink(10.0), editor.zoom, editor.curved);
            ui.horizontal(|ui| {
                ui.checkbox(&mut editor.curved, t("Curve preview"));
                ui.label(t("Zoom"));
                ui.add(egui::Slider::new(&mut editor.zoom, 2.0..=20.0).show_value(false));
            });
            ui.label(t("Lengths and sizes are mm on paper. The centerline defines geometry and dimensions."));
            let mut remove = None;
            let mut swap = None;
            let room = dialog_room(ui, 110.0 + if editor.error.is_some() { ERROR_HEIGHT + 6.0 } else { 0.0 }).clamp(80.0, 300.0);
            ScrollArea::vertical().id_salt("creator-segments").stick_to_bottom(true).max_height(room).show(ui, |ui| {
                let last = editor.segments.len();
                for (i, s) in editor.segments.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(format!("{}.", i + 1));
                            let old = kind(s);
                            let mut next = old;
                            let c = ComboBox::from_id_salt("kind").selected_text(t(KINDS[old])).width(110.0).show_ui(ui, |ui| {
                                for (k, n) in KINDS.iter().enumerate() {
                                    ui.selectable_value(&mut next, k, t(n));
                                }
                            });
                            c.response
                                .widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, tf("Segment {}", &[&(i + 1).to_string()])));
                            if next != old {
                                *s = replacement(next, s.length().max(MIN_SEGMENT));
                            }
                            ui.label(t("Length"));
                            // A legacy instantaneous dot gains a reserved length only when its field is changed.
                            let mut length = s.length().max(MIN_SEGMENT);
                            let before = length;
                            number(ui, &mut length, tf("Segment {} Length", &[&(i + 1).to_string()]), MIN_SEGMENT, MAX_SEGMENT, " mm");
                            if length != before {
                                *s = match s {
                                    Segment::Dot => Segment::DotLength(length),
                                    Segment::Dash(_) => Segment::Dash(length),
                                    Segment::Space(_) => Segment::Space(length),
                                    Segment::DotLength(_) => Segment::DotLength(length),
                                    Segment::Offset { count, distance, .. } => {
                                        Segment::Offset { length, count: *count, distance: *distance }
                                    }
                                    Segment::Wave { frequency, amplitude, .. } => {
                                        Segment::Wave { length, frequency: *frequency, amplitude: *amplitude }
                                    }
                                    Segment::Zigzag { frequency, amplitude, .. } => {
                                        Segment::Zigzag { length, frequency: *frequency, amplitude: *amplitude }
                                    }
                                    Segment::Text { size, text, .. } => Segment::Text { length, size: *size, text: text.clone() },
                                };
                            }
                            let n = (i + 1).to_string();
                            ui.add_enabled_ui(i > 0, |ui| {
                                if named_button(ui, t("Up"), tf("Segment {}: Up", &[&n])) {
                                    swap = Some((i, i - 1));
                                }
                            });
                            ui.add_enabled_ui(i + 1 < last, |ui| {
                                if named_button(ui, t("Down"), tf("Segment {}: Down", &[&n])) {
                                    swap = Some((i, i + 1));
                                }
                            });
                            if named_button(ui, t("Delete"), tf("Segment {}: Delete", &[&n])) {
                                remove = Some(i);
                            }
                        });
                        ui.horizontal(|ui| {
                            let n = (i + 1).to_string();
                            match s {
                                Segment::Offset { count, distance, .. } => {
                                    ui.label(t("Count"));
                                    let r = ui.add(DragValue::new(count).range(2..=9));
                                    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, tf("Segment {} Count", &[&n])));
                                    ui.label(t("Spacing"));
                                    number(ui, distance, tf("Segment {} Spacing", &[&n]), MIN_SEGMENT, MAX_SEGMENT, " mm");
                                }
                                Segment::Wave { frequency, amplitude, .. } | Segment::Zigzag { frequency, amplitude, .. } => {
                                    ui.label(t("Frequency (cycles per segment)"));
                                    number(ui, frequency, tf("Segment {} Frequency", &[&n]), 0.1, 100.0, "");
                                    ui.label(t("Amplitude"));
                                    number(ui, amplitude, tf("Segment {} Amplitude", &[&n]), MIN_SEGMENT, MAX_SEGMENT, " mm");
                                }
                                Segment::Text { size, text, .. } => {
                                    ui.label(t("Size"));
                                    number(ui, size, tf("Segment {} Size", &[&n]), MIN_SEGMENT, MAX_SEGMENT, " mm");
                                    let r = ui.add_sized([240.0, 24.0], TextEdit::singleline(text).char_limit(256));
                                    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, tf("Segment {} Text", &[&n])));
                                }
                                _ => {}
                            }
                        });
                    });
                }
            });
            if let Some(i) = remove {
                editor.segments.remove(i);
            }
            if let Some((a, b)) = swap {
                editor.segments.swap(a, b);
            }
            if ui.add_enabled(editor.segments.len() < MAX_SEGMENTS, Button::new(t("Add Segment"))).clicked() {
                editor.segments.push(Segment::Dash(3.0));
            }
            ui.label(t("Text: IBM Plex Sans (bundled). Text scales down to fit its segment."));
            if let Some(error) = &editor.error {
                boxed_text(ui, "creator-error", pal().error, error, ERROR_HEIGHT);
            }
        });
        crate::popup::footer(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                cancel = ui.button(t("Cancel Pattern")).clicked();
                save = ui.button(t("Save Pattern")).clicked();
            });
        });
    });
    if cancel || response.should_close() {
        return;
    }
    if save {
        editor.name = editor.name.trim().to_string();
        let error = if editor.name.is_empty() {
            Some(t("Enter a name for every style and pattern.").to_string())
        } else if editor.name == SOLID || d.live().any(|(i, p)| Some(i) != editor.row && p.name.trim() == editor.name) {
            Some(tf("The name `{}` is already in use.", &[&editor.name]))
        } else {
            LinePattern(editor.segments.clone()).check(&editor.name).err()
        };
        if let Some(error) = error {
            editor.error = Some(error);
        } else {
            if let Some(i) = editor.row {
                d.patterns[i].name = editor.name;
                d.patterns[i].segments = editor.segments;
            } else {
                d.patterns.push(PatternRow { was: None, name: editor.name, segments: editor.segments });
                d.gone.push(false);
            }
            return;
        }
    }
    d.creator = Some(editor);
}
