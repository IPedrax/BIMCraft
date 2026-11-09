//! Manage > Title Blocks: the project's own title blocks, as Revit's title block families with labels. A list of them (New,
//! Duplicate, Delete, Import DXF…), and the one picked: a preview, filled in as the first sheet that names it fills it in,
//! its fields (each the project's or each sheet's, with its label, default and, for the project's, its value), its texts,
//! its lines and its size, all typed. OK and Apply make it with the `title_block` command and give the project's values
//! with `title_values`; a sheet picks one, and fills in its own fields, in Properties.

use std::collections::BTreeMap;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Model};
use bimcraft_core::pen::{MAX_PEN, Pen};
use bimcraft_core::titleblock::{self, Align as TextAlign, Field, Scope, Stroke, Text, TitleBlock};
use egui::{Align, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, RichText, ScrollArea, Sense, TextEdit, Ui, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::canvas::{Place, paint};
use crate::lang::{t, tf};
use crate::popup::{self, Modal};
use bimcraft_i18n::msg;

const TABS: [&str; 4] = [msg!("Fields"), msg!("Texts"), msg!("Lines"), msg!("Size")];

/// The Title Blocks dialog: the one picked, as typed, and the project's values of its fields, as typed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TitleBlocksDialog {
    /// The one shown, by name; empty when the project has none.
    name: String,
    tb: TitleBlock,
    /// What it's to be called (Rename), as typed.
    rename: String,
    values: BTreeMap<String, String>,
    tab: usize,
    /// A new one's name, as typed.
    new_name: String,
    error: Option<String>,
    /// The text picked on the preview, by its place in the list.
    selected: Option<usize>,
    /// The key and scope Make Field gives the text picked, as typed.
    field_key: String,
    field_scope: Scope,
    /// How much the preview is enlarged past fitting: 1 fits it.
    zoom: f32,
}

impl TitleBlocksDialog {
    /// The dialog on `name` (else the project's first title block).
    pub(crate) fn new(m: &Model, name: Option<&str>) -> TitleBlocksDialog {
        let name = name.filter(|n| m.title_blocks.contains_key(*n)).map(str::to_string).or_else(|| m.title_blocks.keys().next().cloned());
        let name = name.unwrap_or_default();
        let tb = m.title_blocks.get(&name).cloned().unwrap_or_default();
        TitleBlocksDialog {
            rename: name.clone(),
            name,
            tb,
            values: m.title_values.clone(),
            tab: 0,
            new_name: String::new(),
            error: None,
            selected: None,
            field_key: String::new(),
            field_scope: Scope::Sheet,
            zoom: 1.0,
        }
    }

    /// Text `i` picked on the preview (none, no text): Make Field offers a key made of what it says.
    pub(crate) fn select(&mut self, i: Option<usize>) {
        if i != self.selected {
            self.selected = i;
            self.field_key = i.and_then(|i| self.tb.texts.get(i)).map_or_else(String::new, |x| self.tb.free_key(&x.text));
        }
    }

    /// What stopped OK, Apply, New, Duplicate or Delete, if anything did.
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The project's value of `key`, as typed.
    #[cfg(test)]
    pub(crate) fn set_value(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }

    /// The `title_block` command for it as typed.
    fn params(&self) -> Value {
        let mut p = serde_json::to_value(&self.tb).unwrap_or_else(|_| json!({}));
        if let Some(o) = p.as_object_mut() {
            o.insert("name".into(), json!(self.name));
            for key in ["lines", "fills", "texts", "fields"] {
                o.entry(key).or_insert(json!([]));
            }
            if !self.rename.trim().is_empty() && self.rename.trim() != self.name {
                o.insert("rename".into(), json!(self.rename.trim()));
            }
        }
        p
    }

    /// The project's values to give with it: each project field's as typed, cleared where it's blank; the rest as they are.
    fn value_params(&self, m: &Model) -> Value {
        let mut out = serde_json::Map::new();
        for f in self.tb.fields.iter().filter(|f| f.scope == Scope::Project) {
            let key = f.key.trim().to_ascii_uppercase();
            if !titleblock::valid_key(&key) {
                continue;
            }
            let typed = self.values.get(&key).filter(|v| !v.is_empty());
            if typed != m.title_values.get(&key).filter(|v| !v.is_empty()) {
                out.insert(key, typed.map_or(Value::Null, |v| json!(v)));
            }
        }
        json!({"values": out})
    }
}

/// A length in mm on paper, from `lo` to `hi`.
fn mm(x: &mut f64, lo: f64, hi: f64) -> DragValue<'_> {
    DragValue::new(x).range(lo..=hi).speed(0.5).max_decimals(2).suffix(" mm")
}

fn pen(ui: &mut Ui, p: &mut Pen) {
    let mut n = p.0;
    ui.add(DragValue::new(&mut n).range(0..=MAX_PEN).prefix(format!("{} ", t("Pen"))));
    *p = Pen(n);
}

/// The fields: key, label, whose, default, and a project field's value.
fn fields_tab(ui: &mut Ui, d: &mut TitleBlocksDialog) {
    ui.label(t(
        "A text names a field as {KEY}. A project field has one value for every sheet; a sheet field, each sheet's own, filled in under Properties. The automatic fields need no definition:",
    ));
    let auto: Vec<String> = titleblock::AUTO.iter().map(|(k, _)| format!("{{{k}}}")).collect();
    ui.label(RichText::new(auto.join("  ")).monospace().small());
    ui.add_space(4.0);
    let mut gone = None;
    Grid::new("title-block-fields").striped(true).num_columns(6).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        for h in [msg!("Key"), msg!("Label"), msg!("Scope"), msg!("Default"), msg!("Value"), ""] {
            ui.label(RichText::new(t(h)).strong());
        }
        ui.end_row();
        for (i, f) in d.tb.fields.iter_mut().enumerate() {
            ui.add_sized([110.0, 20.0], TextEdit::singleline(&mut f.key));
            ui.add_sized([130.0, 20.0], TextEdit::singleline(&mut f.label));
            ComboBox::from_id_salt(("title-field-scope", i))
                .selected_text(t(if f.scope == Scope::Project { msg!("Project") } else { msg!("Sheet") }))
                .width(80.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.scope, Scope::Sheet, t("Sheet"));
                    ui.selectable_value(&mut f.scope, Scope::Project, t("Project"));
                });
            ui.add_sized([110.0, 20.0], TextEdit::singleline(&mut f.default));
            if f.scope == Scope::Project {
                let key = f.key.trim().to_ascii_uppercase();
                let v = d.values.entry(key).or_default();
                ui.add_sized([150.0, 20.0], TextEdit::singleline(v));
            } else {
                ui.label(RichText::new(t("each sheet's")).weak());
            }
            if ui.small_button(t("Remove")).on_hover_text(t("Remove the field")).clicked() {
                gone = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = gone {
        d.tb.fields.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button(t("Add Field")).clicked() {
            let key = (1..).map(|n| format!("FIELD_{n}")).find(|k| d.tb.field(k).is_none()).unwrap_or_default();
            d.tb.fields.push(Field { key, label: String::new(), scope: Scope::Sheet, default: String::new() });
        }
        let undefined = d.tb.undefined();
        if !undefined.is_empty() {
            let names: Vec<&str> = undefined.iter().map(String::as_str).collect();
            let tip = tf("The texts name {} without defining them", &[&names.join(", ")]);
            if ui.button(t("Add the Fields the Texts Name")).on_hover_text(tip).clicked() {
                for key in undefined {
                    d.tb.fields.push(Field { key, label: String::new(), scope: Scope::Sheet, default: String::new() });
                }
            }
        }
    });
}

/// The texts: what each says (a field's, what it says until it's given a value), whether it's a field, where, how big, which way
/// and at what angle. Make Field makes one a field of each sheet's, keyed by what it says; Make Text makes it plain again.
fn texts_tab(ui: &mut Ui, d: &mut TitleBlocksDialog) {
    let (w, h) = (d.tb.width, d.tb.height);
    let mut gone = None;
    let mut make: Option<(usize, bool)> = None;
    let TitleBlock { texts, fields, .. } = &mut d.tb;
    Grid::new("title-block-texts").striped(true).num_columns(8).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        for head in [msg!("Text"), msg!("Field"), "X", "Y", msg!("Size"), msg!("Angle (°)"), msg!("Justification"), ""] {
            ui.label(RichText::new(t(head)).strong());
        }
        ui.end_row();
        for (i, x) in texts.iter_mut().enumerate() {
            let field = titleblock::whole_key(&x.text).and_then(|k| fields.iter_mut().find(|f| f.key.eq_ignore_ascii_case(&k)));
            match field {
                Some(f) => {
                    let tip = tf("What field {} says until a sheet or the project gives it a value", &[&f.key]);
                    ui.add_sized([200.0, 20.0], TextEdit::singleline(&mut f.default)).on_hover_text(tip);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{{{}}}", f.key)).monospace().small());
                        let tip = t("It says this again, as plain text, and the field goes when no other text names it");
                        if ui.small_button(t("Make Text")).on_hover_text(tip).clicked() {
                            make = Some((i, false));
                        }
                    });
                }
                None => {
                    ui.add_sized([200.0, 20.0], TextEdit::singleline(&mut x.text));
                    let tip = t("The text becomes {KEY}, and what it said becomes the field's default");
                    if let Some(k) = titleblock::whole_key(&x.text) {
                        let what = if titleblock::is_auto(&k) { msg!("automatic") } else { msg!("not defined") };
                        ui.label(RichText::new(t(what)).weak());
                    } else if ui.small_button(t("Make Field")).on_hover_text(tip).clicked() {
                        make = Some((i, true));
                    }
                }
            }
            ui.add(mm(&mut x.at.0, -w, 2.0 * w));
            ui.add(mm(&mut x.at.1, -h, 2.0 * h));
            ui.add(mm(&mut x.size, titleblock::MIN_LETTERING, titleblock::MAX_LETTERING));
            ui.add(DragValue::new(&mut x.angle).range(-360.0..=360.0).speed(1.0));
            let shown = match x.align {
                TextAlign::Left => msg!("Left"),
                TextAlign::Center => msg!("Center"),
                TextAlign::Right => msg!("Right"),
            };
            ComboBox::from_id_salt(("title-text-align", i)).selected_text(t(shown)).width(70.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut x.align, TextAlign::Left, t("Left"));
                ui.selectable_value(&mut x.align, TextAlign::Center, t("Center"));
                ui.selectable_value(&mut x.align, TextAlign::Right, t("Right"));
            });
            if ui.small_button(t("Remove")).on_hover_text(t("Remove the text")).clicked() {
                gone = Some(i);
            }
            ui.end_row();
        }
    });
    match make {
        Some((i, true)) => {
            let key = d.tb.texts.get(i).map(|x| d.tb.free_key(&x.text)).unwrap_or_default();
            d.error = d.tb.make_field(i, &key, Scope::Sheet).err();
        }
        Some((i, false)) => d.error = d.tb.make_text(i).err(),
        None => {}
    }
    if let Some(i) = gone {
        d.tb.texts.remove(i);
        d.select(None);
    }
    if ui.button(t("Add Text")).clicked() {
        d.tb.texts.push(Text {
            at: P2(4.0, 4.0),
            text: t("Text").to_string(),
            size: 2.5,
            angle: 0.0,
            align: TextAlign::Left,
            color: None,
            width_factor: 1.0,
        });
    }
}

/// The lines: a straight one's two ends, a polyline's points counted; each one's pen.
fn lines_tab(ui: &mut Ui, d: &mut TitleBlocksDialog) {
    let (w, h) = (d.tb.width, d.tb.height);
    let mut gone = None;
    Grid::new("title-block-lines").striped(true).num_columns(6).spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        for head in [msg!("Start"), "", msg!("End"), "", msg!("Pen"), ""] {
            ui.label(RichText::new(t(head)).strong());
        }
        ui.end_row();
        for (i, l) in d.tb.lines.iter_mut().enumerate() {
            match l.points.as_mut_slice() {
                [a, b] if !l.closed => {
                    ui.add(mm(&mut a.0, -w, 2.0 * w));
                    ui.add(mm(&mut a.1, -h, 2.0 * h));
                    ui.add(mm(&mut b.0, -w, 2.0 * w));
                    ui.add(mm(&mut b.1, -h, 2.0 * h));
                }
                pts => {
                    let what = if l.closed { msg!("Closed polyline, {} points") } else { msg!("Polyline, {} points") };
                    ui.label(tf(what, &[&pts.len().to_string()]));
                    ui.label("");
                    ui.label("");
                    ui.label("");
                }
            }
            pen(ui, &mut l.pen);
            if ui.small_button(t("Remove")).on_hover_text(t("Remove the line")).clicked() {
                gone = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = gone {
        d.tb.lines.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button(t("Add Line")).clicked() {
            d.tb.lines.push(Stroke { points: vec![P2(0.0, h / 2.0), P2(w, h / 2.0)], closed: false, pen: Pen::FINE, color: None });
        }
        if ui.button(t("Add Rectangle")).clicked() {
            let pts = vec![P2(0.0, 0.0), P2(w, 0.0), P2(w, h), P2(0.0, h)];
            d.tb.lines.push(Stroke { points: pts, closed: true, pen: Pen::BORDER, color: None });
        }
    });
}

/// Its box, and its fills.
fn size_tab(ui: &mut Ui, d: &mut TitleBlocksDialog) {
    ui.label(t("The title block stands in the sheet's bottom right corner, inside the border; views go above it."));
    ui.add_space(4.0);
    Grid::new("title-block-size").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        ui.label(t("Width"));
        ui.add(mm(&mut d.tb.width, 1.0, titleblock::MAX_SIZE));
        ui.end_row();
        ui.label(t("Height"));
        ui.add(mm(&mut d.tb.height, 1.0, titleblock::MAX_SIZE));
        ui.end_row();
        ui.label(t("Font"));
        let fonts = bimcraft_core::text::families();
        ComboBox::from_id_salt("title-block-font").selected_text(d.tb.font.clone()).width(220.0).show_ui(ui, |ui| {
            ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for f in &fonts {
                    ui.selectable_value(&mut d.tb.font, f.clone(), f);
                }
            });
        });
        ui.end_row();
        ui.label(t("Fills"));
        ui.horizontal(|ui| {
            ui.label(d.tb.fills.len().to_string());
            if !d.tb.fills.is_empty() && ui.small_button(t("Remove the fills")).clicked() {
                d.tb.fills.clear();
            }
        });
        ui.end_row();
    });
}

/// The title block as it prints, to pick a text on and drag it: a click picks the text under it (or none), a drag moves the text
/// picked, and the arrow keys nudge it half a millimetre while no field has the keyboard.
fn canvas(ui: &mut Ui, model: &Model, d: &mut TitleBlocksDialog, values: &BTreeMap<String, String>) {
    let drawing = titleblock::preview(model, &d.tb, values);
    let (dw, dh) = (drawing.view.width as f32, drawing.view.height as f32);
    let (room_w, room_h) = (860.0, 480.0);
    ui.horizontal(|ui| {
        ui.label(t("Zoom"));
        ui.add(egui::Slider::new(&mut d.zoom, 1.0..=6.0).show_value(false));
        if ui.small_button(t("Fit")).clicked() {
            d.zoom = 1.0;
        }
    });
    let k = (room_w / dw).min(room_h / dh).min(3.0) * d.zoom;
    ScrollArea::both()
        .id_salt("title-block-canvas")
        .max_width(room_w)
        .max_height(room_h)
        .show(ui, |ui| canvas_at(ui, d, values, &drawing, k));
}

/// [`canvas`], drawn `k` screen points a drawing pixel.
fn canvas_at(ui: &mut Ui, d: &mut TitleBlocksDialog, values: &BTreeMap<String, String>, drawing: &bimcraft_core::render::Drawing, k: f32) {
    let (dw, dh) = (drawing.view.width as f32, drawing.view.height as f32);
    let (rect, r) = ui.allocate_exact_size(vec2(dw * k, dh * k), Sense::click_and_drag());
    ui.painter().rect_filled(rect, 0.0, egui::Color32::WHITE);
    let at = Place { origin: rect.min, k, paper: true, hide: None };
    paint(&ui.painter_at(rect), at, drawing, &[], None, false, &[]);
    let view = drawing.view;
    let to_mm = |q: egui::Pos2| view.mm(((q.x - rect.min.x) / k, (q.y - rect.min.y) / k));
    let to_screen = |p: P2| {
        let (x, y) = view.px(p);
        rect.min + vec2(x * k, y * k)
    };
    let outlines: Vec<[P2; 4]> = d.tb.texts.iter().map(|x| x.outline(&d.tb.font, &titleblock::fill(&x.text, values))).collect();
    // The last drawn wins, as it shows on top.
    let under = |q: egui::Pos2| outlines.iter().rposition(|o| bimcraft_core::geom::contains(o, to_mm(q)));
    let hovered = r.hover_pos().and_then(under);
    if r.clicked() {
        d.select(r.interact_pointer_pos().and_then(under));
    }
    if r.drag_started() {
        let hit = r.interact_pointer_pos().and_then(under);
        if hit.is_some() {
            d.select(hit);
        }
    }
    let per_mm = f64::from(k) * view.scale;
    if r.dragged()
        && let Some(x) = d.selected.and_then(|i| d.tb.texts.get_mut(i))
    {
        let delta = r.drag_delta();
        x.at = x.at + P2(f64::from(delta.x) / per_mm, -f64::from(delta.y) / per_mm);
    }
    if r.drag_stopped()
        && let Some(x) = d.selected.and_then(|i| d.tb.texts.get_mut(i))
    {
        // To the tenth of a millimetre, as a drawing is typed.
        x.at = P2((x.at.0 * 10.0).round() / 10.0, (x.at.1 * 10.0).round() / 10.0);
    }
    let typing = ui.memory(|m| m.focused().is_some());
    if !typing
        && r.hovered()
        && let Some(x) = d.selected.and_then(|i| d.tb.texts.get_mut(i))
    {
        let step = 0.5;
        ui.input(|i| {
            for (key, by) in [
                (egui::Key::ArrowLeft, P2(-step, 0.0)),
                (egui::Key::ArrowRight, P2(step, 0.0)),
                (egui::Key::ArrowUp, P2(0.0, step)),
                (egui::Key::ArrowDown, P2(0.0, -step)),
            ] {
                if i.key_pressed(key) {
                    x.at = x.at + by;
                }
            }
        });
    }
    let accent = ui.visuals().selection.bg_fill;
    let ring = |o: &[P2; 4]| o.iter().map(|p| to_screen(*p)).collect::<Vec<_>>();
    if let Some(o) = hovered.filter(|h| Some(*h) != d.selected).and_then(|i| outlines.get(i)) {
        ui.painter().add(egui::Shape::closed_line(ring(o), egui::Stroke::new(1.0, accent.gamma_multiply(0.6))));
    }
    if let Some(o) = d.selected.and_then(|i| outlines.get(i)) {
        ui.painter().add(egui::Shape::closed_line(ring(o), egui::Stroke::new(2.0, accent)));
    }
    let tip = if hovered.is_some() { t("Click to pick the text, drag to move it") } else { t("Click a text to pick it") };
    r.on_hover_text(tip);
}

/// The text picked on the preview: what it says, its size, angle and justification, Make Field and Remove.
fn selected_text(ui: &mut Ui, d: &mut TitleBlocksDialog) {
    let Some(i) = d.selected.filter(|i| *i < d.tb.texts.len()) else {
        ui.label(RichText::new(t("Click a text on the title block to move it, change it or make it a field.")).weak());
        return;
    };
    let (mut make, mut remove) = (false, false);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let Some(x) = d.tb.texts.get_mut(i) else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("Text")).strong());
            ui.add_sized([320.0, 20.0], TextEdit::singleline(&mut x.text));
            ui.label(t("Size"));
            ui.add(mm(&mut x.size, titleblock::MIN_LETTERING, titleblock::MAX_LETTERING));
            ui.label(t("Angle (°)"));
            ui.add(DragValue::new(&mut x.angle).range(-360.0..=360.0).speed(1.0));
            ui.label(t("Width Factor"));
            ui.add(DragValue::new(&mut x.width_factor).range(0.1..=10.0).speed(0.01).max_decimals(2));
            let shown = match x.align {
                TextAlign::Left => msg!("Left"),
                TextAlign::Center => msg!("Center"),
                TextAlign::Right => msg!("Right"),
            };
            ComboBox::from_id_salt("title-picked-align").selected_text(t(shown)).width(80.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut x.align, TextAlign::Left, t("Left"));
                ui.selectable_value(&mut x.align, TextAlign::Center, t("Center"));
                ui.selectable_value(&mut x.align, TextAlign::Right, t("Right"));
            });
            remove = ui.button(t("Remove")).on_hover_text(t("Remove the text")).clicked();
        });
        let trimmed = x.text.trim();
        let is_field = trimmed.starts_with('{') && trimmed.ends_with('}') && titleblock::keys_in(trimmed).len() == 1;
        if !is_field {
            ui.horizontal(|ui| {
                ui.label(t("Field key"));
                ui.add_sized([180.0, 20.0], TextEdit::singleline(&mut d.field_key));
                ComboBox::from_id_salt("title-picked-scope")
                    .selected_text(t(if d.field_scope == Scope::Project { msg!("Project") } else { msg!("Sheet") }))
                    .width(80.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.field_scope, Scope::Sheet, t("Sheet"));
                        ui.selectable_value(&mut d.field_scope, Scope::Project, t("Project"));
                    });
                let tip = t("The text becomes {KEY}, and what it said becomes the field's default");
                make = ui.button(t("Make Field")).on_hover_text(tip).clicked();
            });
        }
    });
    if make {
        match d.tb.make_field(i, &d.field_key, d.field_scope) {
            Ok(()) => d.error = None,
            Err(e) => d.error = Some(e),
        }
    }
    if remove {
        d.tb.texts.remove(i);
        d.select(None);
    }
}

impl BimApp {
    /// Manage > Title Blocks, on `name` or the project's first.
    pub(crate) fn open_title_blocks(&mut self, name: Option<&str>) {
        self.title_blocks_dialog = Some(TitleBlocksDialog::new(&self.session.model, name));
    }

    /// Import DXF…: the file the user picks, read into the title block shown (a new one, named for the file, when there's
    /// none), and the dialog open on it again.
    fn import_title_block(&mut self, ctx: &Context, name: String) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("CAD drawings (DWG, DXF)"), &["dxf", "dwg"]);
        self.ask_files(ctx, dialog, move |app, paths| {
            let Some(path) = paths.into_iter().next() else { return };
            let name = if name.is_empty() {
                path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Title Block".into())
            } else {
                name
            };
            if let Some(reply) = app.run("title_block", json!({"name": name, "path": path.to_string_lossy()})) {
                app.open_title_blocks(Some(&name));
                // What the reading says (a drawing of no unit, read in mm) and what it left out, on the status bar.
                let mut said: Vec<String> =
                    reply["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect();
                let left: Vec<String> =
                    reply["left_out"].as_object().into_iter().flatten().map(|(what, n)| format!("{n} × {what}")).collect();
                if !left.is_empty() {
                    said.push(tf("Left out: {}", &[&left.join(", ")]));
                }
                if !said.is_empty() {
                    app.status = crate::Status { text: said.join("; "), error: false };
                }
            }
        });
    }

    /// Manage > Title Blocks, while it's open.
    pub(crate) fn title_blocks_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.title_blocks_dialog.take() else { return };
        let model = std::sync::Arc::clone(&self.session.model);
        let names: Vec<String> = model.title_blocks.keys().cloned().collect();
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let (mut pick, mut make, mut duplicate, mut delete, mut import) = (None, None, false, false, false);
        let r = Modal::new(UiId::new("title-blocks-dialog"), t("Title Blocks")).width(1120.0).show(ctx, |ui| {
            popup::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // The project's title blocks, and what makes and takes them.
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        ScrollArea::vertical().id_salt("title-block-list").max_height(300.0).show(ui, |ui| {
                            for n in &names {
                                if ui.selectable_label(*n == d.name, n).clicked() && *n != d.name {
                                    pick = Some(n.clone());
                                }
                            }
                            if names.is_empty() {
                                ui.label(RichText::new(t("The project has no title blocks of its own; sheets draw the built-in one.")).weak());
                            }
                        });
                        ui.separator();
                        ui.add(TextEdit::singleline(&mut d.new_name).hint_text(t("New title block's name")).desired_width(180.0));
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(t("New")).clicked() {
                                make = Some(d.new_name.trim().to_string());
                            }
                            if !d.name.is_empty() && ui.button(t("Duplicate")).clicked() {
                                duplicate = true;
                            }
                            if !d.name.is_empty() && ui.button(t("Delete")).clicked() {
                                delete = true;
                            }
                        });
                        let tip = t("Read a title block drawn at full size (in mm on paper) in the model space of a DXF or DWG: its lines, solid hatches and texts; each {KEY} its texts name becomes a field");
                        if ui.button(t("Import DXF…")).on_hover_text(tip).clicked() {
                            import = true;
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        if d.name.is_empty() {
                            ui.label(t("Make one with New, or read one from a DXF with Import DXF…."));
                            return;
                        }
                        ui.horizontal(|ui| {
                            ui.label(t("Name"));
                            ui.add_sized([240.0, 20.0], TextEdit::singleline(&mut d.rename));
                        });
                        // What it looks like, filled in as the first sheet that names it would be, else with its defaults.
                        let sheet = model.elements.iter().find_map(|(id, e)| matches!(e, Element::Sheet(s) if s.title_block == d.name).then_some(*id));
                        // No sheet names it yet: the automatic fields as a sheet of one view at 1 : 100 would have them.
                        let mut values = sheet.map(|s| bimcraft_core::sheet::title_values(&model, s)).unwrap_or_else(|| {
                            [
                                ("PROJECT", model.name.clone()),
                                ("SHEET_NAME", t("Sheet Name").to_string()),
                                ("SHEET_NUMBER", "A101".to_string()),
                                ("SCALE", model.units.scale(100.0)),
                                ("DATE", titleblock::today(model.language)),
                                ("SHEET_INDEX", "01".to_string()),
                                ("SHEET_COUNT", "01".to_string()),
                                ("REVISION", String::new()),
                            ]
                            .into_iter()
                            .map(|(k, v)| (k.to_string(), v))
                            .collect()
                        });
                        for f in &d.tb.fields {
                            let key = f.key.trim().to_ascii_uppercase();
                            let given = if f.scope == Scope::Project { d.values.get(&key).filter(|v| !v.is_empty()) } else { None };
                            if let Some(v) = given {
                                values.insert(key, v.clone());
                            } else if !values.contains_key(&key) {
                                values.insert(key.clone(), if f.default.is_empty() { format!("{{{key}}}") } else { f.default.clone() });
                            }
                        }
                        canvas(ui, &model, &mut d, &values);
                        selected_text(ui, &mut d);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            for (i, label) in TABS.iter().enumerate() {
                                if ui.selectable_label(d.tab == i, t(label)).clicked() {
                                    d.tab = i;
                                }
                            }
                        });
                        ui.separator();
                        ScrollArea::both().id_salt("title-block-tab").max_height(220.0).show(ui, |ui| match d.tab {
                            0 => fields_tab(ui, &mut d),
                            1 => texts_tab(ui, &mut d),
                            2 => lines_tab(ui, &mut d),
                            _ => size_tab(ui, &mut d),
                        });
                    });
                });
                if let Some(e) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
            });
            popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        let dirty = !d.name.is_empty() && (model.title_blocks.get(&d.name) != Some(&d.tb) || d.rename.trim() != d.name);
        let commit = |app: &mut BimApp, d: &mut TitleBlocksDialog| -> bool {
            if d.name.is_empty() {
                return true;
            }
            if dirty {
                match app.session.execute("title_block", &d.params()) {
                    Ok(v) => {
                        if let Some(n) = v["name"].as_str() {
                            d.name = n.to_string();
                            d.rename = d.name.clone();
                        }
                    }
                    Err(e) => {
                        d.error = Some(e);
                        return false;
                    }
                }
            }
            let values = d.value_params(&app.session.model);
            if values["values"].as_object().is_some_and(|o| !o.is_empty())
                && let Err(e) = app.session.execute("title_values", &values)
            {
                d.error = Some(e);
                return false;
            }
            d.error = None;
            true
        };
        let mut keep = !(cancel || r.should_close());
        if ok || apply {
            let done = commit(self, &mut d);
            keep = !(ok && done);
        }
        if let Some(n) = pick {
            // Another one shown: what was typed in this one first, if it may be.
            if commit(self, &mut d) {
                d = TitleBlocksDialog { tab: d.tab, ..TitleBlocksDialog::new(&self.session.model, Some(&n)) };
            }
        }
        if let Some(name) = make {
            let name = if name.is_empty() {
                (1..)
                    .map(|k| tf("Title Block {}", &[&k.to_string()]))
                    .find(|n| !self.session.model.title_blocks.contains_key(n))
                    .unwrap_or_default()
            } else {
                name
            };
            match self.session.execute("title_block", &json!({"name": name})) {
                Ok(_) => d = TitleBlocksDialog::new(&self.session.model, Some(&name)),
                Err(e) => d.error = Some(e),
            }
        }
        if duplicate {
            let base = format!("{} 2", d.name);
            let name = (2..).map(|k| format!("{} {k}", d.name)).find(|n| !self.session.model.title_blocks.contains_key(n)).unwrap_or(base);
            let mut p = d.params();
            p["name"] = json!(name);
            if let Some(o) = p.as_object_mut() {
                o.remove("rename");
            }
            match self.session.execute("title_block", &p) {
                Ok(_) => d = TitleBlocksDialog::new(&self.session.model, Some(&name)),
                Err(e) => d.error = Some(e),
            }
        }
        if delete {
            match self.session.execute("title_block", &json!({"name": d.name, "delete": true})) {
                Ok(_) => d = TitleBlocksDialog::new(&self.session.model, None),
                Err(e) => d.error = Some(e),
            }
        }
        if import {
            // What was typed is kept first, so the drawing goes over what the user sees.
            if commit(self, &mut d) {
                self.import_title_block(ctx, d.name.clone());
            }
            keep = false;
        }
        if keep {
            self.title_blocks_dialog = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_gives_the_command_its_title_block_and_only_the_values_that_changed() {
        let mut m = Model::new("P");
        m.title_blocks.insert("T".into(), TitleBlock::default());
        m.title_values.insert("CLIENT".into(), "A".into());
        let mut d = TitleBlocksDialog::new(&m, None);
        assert_eq!(d.name, "T");
        d.tb.fields.push(Field { key: "client".into(), label: String::new(), scope: Scope::Project, default: String::new() });
        d.tb.fields.push(Field { key: "ART".into(), label: String::new(), scope: Scope::Project, default: String::new() });
        d.values.insert("ART".into(), "123".into());
        let p = d.params();
        assert_eq!(p["name"], "T");
        assert!(p.get("rename").is_none());
        assert_eq!(d.value_params(&m), json!({"values": {"ART": "123"}}));
        d.values.insert("CLIENT".into(), String::new());
        assert_eq!(d.value_params(&m), json!({"values": {"ART": "123", "CLIENT": null}}));
        d.rename = "U".into();
        assert_eq!(d.params()["rename"], "U");
    }
}
