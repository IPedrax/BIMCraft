//! Manage > Fill Patterns, as Revit's dialogs: the project's filled region types (a foreground and a background pattern,
//! their colours, masking or not) and its fill patterns (families of lines, drafting or model), on two tabs of one draft,
//! applied as one undo step (OK and Apply), or dropped (Cancel). A pattern of one family, or two square to each other, is
//! edited as Revit's simple pattern: its angle, its spacing, crosshatched or not; the rest show as they are.

use bimcraft_core::engine::Session;
use bimcraft_core::fills::{self, FillPattern, FilledRegionType, HatchLine, MAX_SPACING, MIN_SPACING, Target};
use bimcraft_core::geom::P2;
use bimcraft_core::model::Model;
use egui::{
    Align, Button, Checkbox, Color32, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, Sense, Stroke,
    TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::{BimApp, Status};

/// The dialog's tabs.
const TABS: [&str; 2] = ["Region Types", "Patterns"];

/// A pattern as edited: the name it had (none, new), the name it has, what it is.
struct PatternRow {
    was: Option<String>,
    name: String,
    pattern: FillPattern,
}

/// A type as edited: the name it had, the name it has, its patterns as their rows (none, none), its colours, masking.
struct TypeRow {
    was: Option<String>,
    name: String,
    foreground: Option<usize>,
    foreground_color: [u8; 3],
    background: Option<usize>,
    background_color: [u8; 3],
    masking: bool,
}

/// The Fill Patterns dialog: its tab, the types and patterns as edited.
pub(crate) struct FillPatternsDialog {
    tab: usize,
    types: Vec<TypeRow>,
    patterns: Vec<PatternRow>,
    /// Patterns deleted: their rows stay, so the types' indices do, and are skipped.
    gone: Vec<bool>,
}

impl FillPatternsDialog {
    /// Open on the project's region types and fill patterns.
    pub(crate) fn new(m: &Model) -> FillPatternsDialog {
        let patterns: Vec<PatternRow> =
            m.fill_patterns.iter().map(|(n, p)| PatternRow { was: Some(n.clone()), name: n.clone(), pattern: p.clone() }).collect();
        let row = |n: &Option<String>| n.as_ref().and_then(|n| patterns.iter().position(|r| r.name == *n));
        let types = (m.region_types.iter())
            .map(|(n, x)| TypeRow {
                was: Some(n.clone()),
                name: n.clone(),
                foreground: row(&x.foreground),
                foreground_color: x.foreground_color,
                background: row(&x.background),
                background_color: x.background_color,
                masking: x.masking,
            })
            .collect();
        let gone = vec![false; patterns.len()];
        FillPatternsDialog { tab: 0, types, patterns, gone }
    }

    /// The patterns not deleted, with their rows.
    fn live(&self) -> impl Iterator<Item = (usize, &PatternRow)> {
        self.patterns.iter().enumerate().filter(|(i, _)| !self.gone[*i])
    }

    /// A type as the project would hold it, its patterns by their names in the draft.
    fn region_type(&self, r: &TypeRow) -> FilledRegionType {
        let name = |i: Option<usize>| i.and_then(|i| self.patterns.get(i)).map(|p| p.name.clone());
        FilledRegionType {
            foreground: name(r.foreground),
            foreground_color: r.foreground_color,
            background: name(r.background),
            background_color: r.background_color,
            masking: r.masking,
        }
    }
}

/// `base` and a number, the first that `taken` hasn't: `Pattern 1`, `Pattern 2`…
fn fresh(base: &str, taken: &[&str]) -> String {
    (1..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(&n.as_str())).unwrap_or_default()
}

/// A text field named `name` for screen readers.
fn name_field(ui: &mut Ui, text: &mut String, name: String) {
    let r = ui.add_sized(vec2(150.0, 20.0), TextEdit::singleline(text));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, name.clone()));
}

/// A small button named `name` for screen readers, showing `text`.
fn named_button(ui: &mut Ui, text: &str, name: String) -> bool {
    let r = ui.add(Button::new(text).small());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    r.clicked()
}

/// A colour button named `name`.
fn colour_field(ui: &mut Ui, rgb: &mut [u8; 3], name: String) {
    let mut c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let r = ui.color_edit_button_srgba(&mut c);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, name.clone()));
    *rgb = [c.r(), c.g(), c.b()];
}

/// A swatch of `p`, `width` x 22 px, in `colour`: a drafting pattern at 5 px to its mm, a model one at a tenth of a px.
pub(crate) fn swatch(ui: &mut Ui, p: Option<&FillPattern>, width: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
    let Some(p) = p else { return };
    if p.is_solid() {
        painter.rect_filled(rect.shrink(1.0), 2.0, colour);
        return;
    }
    let px = if p.target == Target::Model { 0.1 } else { 5.0 };
    let (w, h) = (f64::from(rect.width()) / px, f64::from(rect.height()) / px);
    let ring = vec![P2(0.0, 0.0), P2(w, 0.0), P2(w, h), P2(0.0, h)];
    let strokes = fills::strokes(p, std::slice::from_ref(&ring), 1.0, (P2(0.0, 0.0), P2(w, h)), 4000).unwrap_or_default();
    let at = |q: P2| pos2(rect.left() + (q.0 * px) as f32, rect.bottom() - (q.1 * px) as f32);
    for [a, b] in strokes {
        if a.dist(b) * px < 0.5 {
            painter.circle_filled(at(a), 0.7, colour);
        } else {
            painter.line_segment([at(a), at(b)], Stroke::new(1.0, colour));
        }
    }
}

/// A drop-down of the patterns (or none), named `label`, `row` the one picked.
fn pattern_pick(ui: &mut Ui, row: &mut Option<usize>, names: &[(usize, String)], label: String) {
    let shown = row.and_then(|p| names.iter().find(|(i, _)| *i == p)).map_or_else(|| t("<None>").to_string(), |(_, n)| n.clone());
    let c = ComboBox::from_id_salt(&label).selected_text(shown).width(150.0).show_ui(ui, |ui| {
        ui.selectable_value(row, None, t("<None>"));
        for (p, n) in names {
            ui.selectable_value(row, Some(*p), n);
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
}

/// The types: a row each, its name, its foreground and background (pattern, colour, swatch), masking, Delete; New Type.
fn types_tab(ui: &mut Ui, d: &mut FillPatternsDialog) {
    ui.label(t("What filled regions are filled with: a foreground pattern over a background one, over the paper when it masks."));
    ui.add_space(4.0);
    let names: Vec<(usize, String)> = d.live().map(|(i, p)| (i, p.name.clone())).collect();
    let patterns: Vec<FillPattern> = d.patterns.iter().map(|p| p.pattern.clone()).collect();
    let mut delete = None;
    ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        Grid::new("region-types").striped(true).num_columns(9).show(ui, |ui| {
            for h in ["Name", "Foreground", "", "", "Background", "", "", "Masking", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, r) in d.types.iter_mut().enumerate() {
                let who = r.name.clone();
                name_field(ui, &mut r.name, tf("{}: Name", &[&who]));
                for (row, rgb, side) in
                    [(&mut r.foreground, &mut r.foreground_color, "Foreground"), (&mut r.background, &mut r.background_color, "Background")]
                {
                    pattern_pick(ui, row, &names, tf("{}: {}", &[&who, t(side)]));
                    colour_field(ui, rgb, tf("{}: {} Colour", &[&who, t(side)]));
                    // As the plan draws it: black ink light on a dark ground.
                    let ink = crate::canvas::color(bimcraft_core::render::Rgb(rgb[0], rgb[1], rgb[2]));
                    swatch(ui, row.and_then(|p| patterns.get(p)), 60.0, ink);
                }
                let c = ui.add(Checkbox::without_text(&mut r.masking));
                c.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, r.masking, tf("{}: Masking", &[&who])));
                if named_button(ui, t("Delete"), tf("{}: Delete", &[&who])) {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = delete {
        d.types.remove(i);
    }
    if ui.button(t("New Type")).clicked() {
        let taken: Vec<&str> = d.types.iter().map(|s| s.name.as_str()).collect();
        let name = fresh(t("Region Type"), &taken);
        let solid = names.iter().find(|(_, n)| n.as_str() == t(fills::SOLID_FILL) || n == fills::SOLID_FILL).map(|(i, _)| *i);
        d.types.push(TypeRow {
            was: None,
            name,
            foreground: solid,
            foreground_color: [0, 0, 0],
            background: None,
            background_color: [255, 255, 255],
            masking: false,
        });
    }
}

/// The patterns: a row each, its name, drafting or model, a swatch, and (a simple one) its angle, spacing and crosshatch,
/// else how many line families it has; Delete; New Pattern.
fn patterns_tab(ui: &mut Ui, d: &mut FillPatternsDialog) {
    ui.label(t("Families of parallel lines: drafting ones mm on paper, the same at any scale; model ones mm in the building."));
    ui.add_space(4.0);
    let ink = ui.visuals().text_color();
    let mut delete = None;
    ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        Grid::new("fill-patterns").striped(true).num_columns(7).show(ui, |ui| {
            for h in ["Name", "Type", "Sample", "Angle", "Spacing", "Crosshatch", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, r) in d.patterns.iter_mut().enumerate().filter(|(i, _)| !d.gone[*i]) {
                let who = r.name.clone();
                name_field(ui, &mut r.name, tf("{}: Name", &[&who]));
                let label = tf("{}: Type", &[&who]);
                let shown = t(if r.pattern.target == Target::Model { "Model" } else { "Drafting" });
                let c = ComboBox::from_id_salt(("fill-target", i)).selected_text(shown).width(90.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut r.pattern.target, Target::Drafting, t("Drafting"));
                    ui.selectable_value(&mut r.pattern.target, Target::Model, t("Model"));
                });
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
                swatch(ui, Some(&r.pattern), 80.0, ink);
                match r.pattern.simple() {
                    Some((mut angle, mut spacing, mut cross)) => {
                        let a = ui.add(DragValue::new(&mut angle).range(-360.0..=360.0).speed(1.0).suffix("°"));
                        a.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, tf("{}: Angle", &[&who])));
                        let s = ui.add(DragValue::new(&mut spacing).range(MIN_SPACING..=MAX_SPACING).speed(0.05).suffix(" mm"));
                        s.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, tf("{}: Spacing", &[&who])));
                        let c = ui.add(Checkbox::without_text(&mut cross));
                        c.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, cross, tf("{}: Crosshatch", &[&who])));
                        let mut lines = vec![HatchLine::simple(angle, spacing)];
                        if cross {
                            lines.push(HatchLine::simple(angle + 90.0, spacing));
                        }
                        r.pattern.lines = lines;
                    }
                    None if r.pattern.is_solid() => {
                        ui.label(t("Solid"));
                        ui.label("");
                        ui.label("");
                    }
                    None => {
                        let families = r.pattern.lines.len().to_string();
                        ui.label(t("Custom")).on_hover_text(tf("{} line families", &[&families]));
                        ui.label("");
                        ui.label("");
                    }
                }
                if named_button(ui, t("Delete"), tf("{}: Delete", &[&who])) {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = delete {
        d.gone[i] = true;
        for r in &mut d.types {
            for row in [&mut r.foreground, &mut r.background] {
                if *row == Some(i) {
                    *row = None;
                }
            }
        }
    }
    if ui.button(t("New Pattern")).clicked() {
        let taken: Vec<&str> = d.live().map(|(_, p)| p.name.as_str()).collect();
        let name = fresh(t("Pattern"), &taken);
        let pattern = FillPattern { target: Target::Drafting, lines: vec![HatchLine::simple(45.0, 2.0)] };
        d.patterns.push(PatternRow { was: None, name, pattern });
        d.gone.push(false);
    }
}

impl FillPatternsDialog {
    /// What the draft asks of the project, in an order each can be done in: patterns made and changed (renamed, which
    /// renames them in the types), types made and changed, then what's deleted, types before patterns.
    fn commands(&self, m: &Model) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        for (_, r) in self.live() {
            let body = json!({"target": r.pattern.target, "lines": r.pattern.lines});
            match &r.was {
                Some(was) if *was == r.name && m.fill_patterns.get(was) == Some(&r.pattern) => {}
                Some(was) => out.push(("fill_pattern", with(body, json!({"name": was, "rename": r.name})))),
                None => out.push(("fill_pattern", with(body, json!({"name": r.name})))),
            }
        }
        // A pattern renamed renames it in the types already: compare what the type holds read through the renames.
        let renamed = |n: &String| self.patterns.iter().find(|r| r.was.as_ref() == Some(n)).map_or_else(|| n.clone(), |r| r.name.clone());
        for r in &self.types {
            let mine = self.region_type(r);
            let body = json!({
                "foreground": mine.foreground, "foreground_color": mine.foreground_color,
                "background": mine.background, "background_color": mine.background_color, "masking": mine.masking,
            });
            let same = |x: &FilledRegionType| {
                FilledRegionType {
                    foreground: x.foreground.as_ref().map(renamed),
                    background: x.background.as_ref().map(renamed),
                    ..x.clone()
                } == mine
            };
            match &r.was {
                Some(was) if *was == r.name && m.region_types.get(was).is_some_and(same) => {}
                Some(was) => out.push(("filled_region_type", with(body, json!({"name": was, "rename": r.name})))),
                None => out.push(("filled_region_type", with(body, json!({"name": r.name})))),
            }
        }
        for was in m.region_types.keys().filter(|n| !self.types.iter().any(|r| r.was.as_ref() == Some(n))) {
            out.push(("filled_region_type", json!({"name": was, "delete": true})));
        }
        for r in self.patterns.iter().zip(&self.gone).filter(|(_, g)| **g).map(|(r, _)| r) {
            if let Some(was) = &r.was {
                out.push(("fill_pattern", json!({"name": was, "delete": true})));
            }
        }
        out
    }
}

/// `a` with `b`'s keys added.
fn with(mut a: Value, b: Value) -> Value {
    if let (Some(a), Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
    a
}

impl BimApp {
    /// The Fill Patterns dialog, while it's open.
    pub(crate) fn fill_patterns_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.fill_patterns_dialog.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(UiId::new("fill-patterns")).show(ctx, |ui| {
            ui.set_width(860.0);
            ui.label(RichText::new(t("Fill Patterns")).heading());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, tab) in TABS.iter().enumerate() {
                    if ui.selectable_label(d.tab == i, t(tab)).clicked() {
                        d.tab = i;
                    }
                }
            });
            ui.separator();
            if d.tab == 0 {
                types_tab(ui, &mut d);
            } else {
                patterns_tab(ui, &mut d);
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                apply = ui.button(t("Apply")).clicked();
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
        // What changed, as one undo step, all or nothing: a refusal keeps the dialog open to fix it, saying why.
        let mut failed = false;
        if ok || apply {
            let commands = d.commands(&self.session.model);
            if !commands.is_empty() {
                let done = self
                    .session
                    .transaction("Fill Patterns", |s: &mut Session| commands.iter().try_for_each(|(c, p)| s.execute(c, p).map(|_| ())));
                match done {
                    Ok(()) => self.status = Status::default(),
                    Err(e) => {
                        self.status = Status { text: e, error: true };
                        failed = true;
                    }
                }
            }
            if apply && !failed {
                let tab = d.tab;
                d = FillPatternsDialog::new(&self.session.model);
                d.tab = tab;
            }
        }
        if failed || !(ok || cancel || r.should_close()) {
            self.fill_patterns_dialog = Some(d);
        }
    }
}
