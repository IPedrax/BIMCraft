//! Manage > Line Styles, as Revit's dialogs: the project's line styles (a pen, a colour and a pattern each) and its line
//! patterns (dashes, dots and spaces, mm on paper), on two tabs of one draft, applied as one undo step (OK and Apply), or
//! dropped (Cancel).

use bimcraft_core::engine::Session;
use bimcraft_core::lines::{LinePattern, LineStyle, MAX_SEGMENT, MAX_SEGMENTS, MIN_SEGMENT, SOLID, Segment};
use bimcraft_core::model::Model;
use bimcraft_core::pen::{MAX_PEN, Pen};
use egui::{
    Align, Button, Color32, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, Sense, Stroke, TextEdit,
    Ui, WidgetInfo, WidgetType, vec2,
};
use serde_json::{Value, json};

use crate::graphics::pen_name;
use crate::lang::{t, tf};
use crate::{BimApp, Status};

/// The dialog's tabs.
const TABS: [&str; 2] = ["Styles", "Patterns"];

/// A pattern as edited: the name it had (none, new), the name it has, its segments.
struct PatternRow {
    was: Option<String>,
    name: String,
    segments: Vec<Segment>,
}

/// A style as edited: the name it had (none, new), the name it has, and its pattern, as its row in the patterns (none,
/// solid), so it follows the pattern's new name.
struct StyleRow {
    was: Option<String>,
    name: String,
    pen: Pen,
    color: [u8; 3],
    pattern: Option<usize>,
}

/// The Line Styles dialog: its tab, the styles and patterns as edited, and the pattern whose segments show.
pub(crate) struct LineStylesDialog {
    tab: usize,
    styles: Vec<StyleRow>,
    patterns: Vec<PatternRow>,
    /// Patterns deleted: their rows stay, so the styles' indices do, and are skipped.
    gone: Vec<bool>,
    open: Option<usize>,
}

impl LineStylesDialog {
    /// Open on the project's line styles and patterns.
    pub(crate) fn new(m: &Model) -> LineStylesDialog {
        let patterns: Vec<PatternRow> =
            m.line_patterns.iter().map(|(n, p)| PatternRow { was: Some(n.clone()), name: n.clone(), segments: p.0.clone() }).collect();
        let styles = (m.line_styles.iter())
            .map(|(n, s)| StyleRow {
                was: Some(n.clone()),
                name: n.clone(),
                pen: s.pen,
                color: s.color,
                pattern: s.pattern.as_ref().and_then(|p| patterns.iter().position(|r| r.name == *p)),
            })
            .collect();
        let gone = vec![false; patterns.len()];
        LineStylesDialog { tab: 0, styles, patterns, gone, open: None }
    }

    /// The patterns not deleted, with their rows.
    fn live(&self) -> impl Iterator<Item = (usize, &PatternRow)> {
        self.patterns.iter().enumerate().filter(|(i, _)| !self.gone[*i])
    }
}

/// `base` and a number, the first that `taken` hasn't: `Style 1`, `Style 2`…
fn fresh(base: &str, taken: &[&str]) -> String {
    (1..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(&n.as_str())).unwrap_or_default()
}

/// A drop-down of the pens, named `name` for screen readers.
fn pen_field(ui: &mut Ui, pen: &mut Pen, name: String) {
    let c = ComboBox::from_id_salt(&name).selected_text(pen_name(*pen)).width(120.0).show_ui(ui, |ui| {
        for each in (0..=MAX_PEN).map(Pen) {
            ui.selectable_value(pen, each, pen_name(each));
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
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

/// A sample of `segments` drawn across `width` px, a mm of paper `k` px: what a line in the pattern looks like.
fn sample(ui: &mut Ui, segments: &[Segment], width: f32, k: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), Sense::hover());
    let ink = ui.visuals().text_color();
    let y = rect.center().y;
    let dash = LinePattern(segments.to_vec()).dash(k);
    let Some(dash) = dash.filter(|d| d.len() >= 2) else {
        ui.painter().line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.5, ink));
        return;
    };
    let mut x = rect.left();
    for (i, len) in dash.iter().cycle().enumerate() {
        if x >= rect.right() || i > 4096 {
            break;
        }
        // A dot as a dot: a stroke a pixel long fades away between pixels.
        match (i % 2 == 0, *len < 1.0) {
            (true, true) => {
                ui.painter().circle_filled(egui::pos2(x, y), 1.0, ink);
            }
            (true, false) => {
                ui.painter().line_segment([egui::pos2(x, y), egui::pos2((x + len).min(rect.right()), y)], Stroke::new(1.5, ink));
            }
            _ => {}
        }
        x += len;
    }
}

/// The styles: a row each, its name, pen, colour and pattern, and its Delete; then New Style.
fn styles_tab(ui: &mut Ui, d: &mut LineStylesDialog) {
    ui.label(t("What detail lines draw in: a pen, a colour and a line pattern."));
    ui.add_space(4.0);
    let names: Vec<(usize, String)> = d.live().map(|(i, p)| (i, p.name.clone())).collect();
    let mut delete = None;
    ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        Grid::new("line-styles").striped(true).num_columns(5).show(ui, |ui| {
            for h in ["Name", "Line Weight", "Colour", "Pattern", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, s) in d.styles.iter_mut().enumerate() {
                let who = s.name.clone();
                name_field(ui, &mut s.name, tf("{}: Name", &[&who]));
                pen_field(ui, &mut s.pen, tf("{}: Line Weight", &[&who]));
                let mut c = Color32::from_rgb(s.color[0], s.color[1], s.color[2]);
                let r = ui.color_edit_button_srgba(&mut c);
                r.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, tf("{}: Colour", &[&who])));
                s.color = [c.r(), c.g(), c.b()];
                let shown =
                    s.pattern.and_then(|p| names.iter().find(|(i, _)| *i == p)).map_or_else(|| t(SOLID).to_string(), |(_, n)| n.clone());
                let label = tf("{}: Pattern", &[&who]);
                let c = ComboBox::from_id_salt(("line-style-pattern", i)).selected_text(shown).width(140.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.pattern, None, t(SOLID));
                    for (p, n) in &names {
                        ui.selectable_value(&mut s.pattern, Some(*p), n);
                    }
                });
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
                if named_button(ui, t("Delete"), tf("{}: Delete", &[&who])) {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = delete {
        d.styles.remove(i);
    }
    if ui.button(t("New Style")).clicked() {
        let taken: Vec<&str> = d.styles.iter().map(|s| s.name.as_str()).collect();
        let name = fresh(t("Style"), &taken);
        d.styles.push(StyleRow { was: None, name, pen: Pen::FINE, color: [0, 0, 0], pattern: None });
    }
}

/// The patterns: a row each, its name, a sample of it, Edit and Delete; under them, the segments of the one open.
fn patterns_tab(ui: &mut Ui, d: &mut LineStylesDialog) {
    ui.label(t("Dashes, dots and spaces, mm on paper, so a pattern reads the same at any scale."));
    ui.add_space(4.0);
    let mut delete = None;
    Grid::new("line-patterns").striped(true).num_columns(4).show(ui, |ui| {
        for h in ["Name", "Sample", "", ""] {
            ui.label(RichText::new(t(h)).strong());
        }
        ui.end_row();
        for (i, p) in d.patterns.iter_mut().enumerate().filter(|(i, _)| !d.gone[*i]) {
            let who = p.name.clone();
            name_field(ui, &mut p.name, tf("{}: Name", &[&who]));
            sample(ui, &p.segments, 160.0, 6.0);
            let r = ui.selectable_label(d.open == Some(i), t("Edit"));
            r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, d.open == Some(i), tf("{}: Edit", &[&who])));
            if r.clicked() {
                d.open = Some(i);
            }
            if named_button(ui, t("Delete"), tf("{}: Delete", &[&who])) {
                delete = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = delete {
        d.gone[i] = true;
        if d.open == Some(i) {
            d.open = None;
        }
        for s in d.styles.iter_mut().filter(|s| s.pattern == Some(i)) {
            s.pattern = None;
        }
    }
    if ui.button(t("New Pattern")).clicked() {
        let taken: Vec<&str> = d.live().map(|(_, p)| p.name.as_str()).collect();
        let name = fresh(t("Pattern"), &taken);
        d.patterns.push(PatternRow { was: None, name, segments: vec![Segment::Dash(3.0), Segment::Space(1.5)] });
        d.gone.push(false);
        d.open = Some(d.patterns.len() - 1);
    }
    let Some(p) = d.open.and_then(|i| d.patterns.get_mut(i)) else { return };
    ui.separator();
    ui.label(RichText::new(tf("Segments of {}", &[&p.name])).strong());
    let mut remove = None;
    Grid::new("line-pattern-segments").num_columns(3).show(ui, |ui| {
        let last = p.segments.len();
        for (i, s) in p.segments.iter_mut().enumerate() {
            let n = (i + 1).to_string();
            if i % 2 == 0 {
                let dot = matches!(s, Segment::Dot);
                let shown = t(if dot { "Dot" } else { "Dash" });
                let label = tf("Segment {}", &[&n]);
                let c = ComboBox::from_id_salt(("segment", i)).selected_text(shown).width(90.0).show_ui(ui, |ui| {
                    if ui.selectable_label(!dot, t("Dash")).clicked() && dot {
                        *s = Segment::Dash(3.0);
                    }
                    if ui.selectable_label(dot, t("Dot")).clicked() {
                        *s = Segment::Dot;
                    }
                });
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
            } else {
                ui.label(t("Space"));
            }
            match s {
                Segment::Dash(l) | Segment::Space(l) => {
                    let label = tf("Segment {} Length", &[&n]);
                    let r = ui.add(DragValue::new(l).range(MIN_SEGMENT..=MAX_SEGMENT).speed(0.05).suffix(" mm"));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, label.clone()));
                }
                Segment::Dot => {
                    ui.label("");
                }
            }
            // A pair goes together, so the pattern stays a mark then a space.
            if i % 2 == 1 && last > 2 && named_button(ui, t("Delete"), tf("Segment {}: Delete", &[&n])) {
                remove = Some(i - 1);
            } else {
                ui.label("");
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        p.segments.drain(i..i + 2);
    }
    let full = p.segments.len() >= MAX_SEGMENTS;
    if ui.add_enabled(!full, Button::new(t("Add Segment"))).clicked() {
        let next = if p.segments.len() % 2 == 0 { Segment::Dash(3.0) } else { Segment::Space(1.5) };
        p.segments.push(next);
    }
}

impl LineStylesDialog {
    /// What the draft asks of the project, as commands in an order each can be done in: patterns made and changed (and
    /// renamed, which renames them in the styles), styles made and changed, then what's deleted, styles before patterns.
    fn commands(&self, m: &Model) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        let segments = |r: &PatternRow| json!(r.segments);
        for (_, r) in self.live() {
            let now = LinePattern(r.segments.clone());
            match &r.was {
                Some(was) if *was == r.name && m.line_patterns.get(was) == Some(&now) => {}
                Some(was) => out.push(("line_pattern", json!({"name": was, "rename": r.name, "segments": segments(r)}))),
                None => out.push(("line_pattern", json!({"name": r.name, "segments": segments(r)}))),
            }
        }
        let pattern_of = |s: &StyleRow| s.pattern.and_then(|i| self.patterns.get(i)).map_or_else(|| json!(SOLID), |p| json!(p.name));
        for s in &self.styles {
            let mine =
                LineStyle { pen: s.pen, color: s.color, pattern: s.pattern.and_then(|i| self.patterns.get(i)).map(|p| p.name.clone()) };
            let mut p = json!({"pen": s.pen, "color": s.color, "pattern": pattern_of(s)});
            match &s.was {
                // A pattern renamed renames it in the style already: what the style holds then is what to compare.
                Some(was) if *was == s.name && m.line_styles.get(was).is_some_and(|x| styles_match(x, &mine, self)) => continue,
                Some(was) => {
                    p["name"] = json!(was);
                    p["rename"] = json!(s.name);
                }
                None => p["name"] = json!(s.name),
            }
            out.push(("line_style", p));
        }
        for was in m.line_styles.keys().filter(|n| !self.styles.iter().any(|s| s.was.as_ref() == Some(n))) {
            out.push(("line_style", json!({"name": was, "delete": true})));
        }
        for r in self.patterns.iter().zip(&self.gone).filter(|(_, g)| **g).map(|(r, _)| r) {
            if let Some(was) = &r.was {
                out.push(("line_pattern", json!({"name": was, "delete": true})));
            }
        }
        out
    }
}

/// Whether the project's style `x` is the draft's `mine`, its pattern read through any rename the draft makes.
fn styles_match(x: &LineStyle, mine: &LineStyle, d: &LineStylesDialog) -> bool {
    let renamed = |n: &String| d.patterns.iter().find(|r| r.was.as_ref() == Some(n)).map_or_else(|| n.clone(), |r| r.name.clone());
    x.pen == mine.pen && x.color == mine.color && x.pattern.as_ref().map(renamed) == mine.pattern
}

impl BimApp {
    /// The Line Styles dialog, while it's open.
    pub(crate) fn line_styles_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.line_styles_dialog.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(UiId::new("line-styles")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.label(RichText::new(t("Line Styles")).heading());
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
                styles_tab(ui, &mut d);
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
                    .transaction("Line Styles", |s: &mut Session| commands.iter().try_for_each(|(c, p)| s.execute(c, p).map(|_| ())));
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
                d = LineStylesDialog::new(&self.session.model);
                d.tab = tab;
            }
        }
        if failed || !(ok || cancel || r.should_close()) {
            self.line_styles_dialog = Some(d);
        }
    }
}

impl BimApp {
    /// The Detail Line tool's Options Bar: the line style it draws in.
    pub(crate) fn line_style_options(&mut self, ui: &mut Ui) {
        let m = &self.session.model;
        let thin = m.language.word(bimcraft_core::lines::THIN).to_string();
        if !m.line_styles.contains_key(&self.line_style) {
            self.line_style =
                if m.line_styles.contains_key(&thin) { thin } else { m.line_styles.keys().next().cloned().unwrap_or_default() };
        }
        let l = ui.label(t("Line Style:"));
        let c = ComboBox::from_id_salt("detail-line-style").selected_text(&self.line_style).width(160.0).show_ui(ui, |ui| {
            for n in m.line_styles.keys() {
                ui.selectable_value(&mut self.line_style, n.clone(), n);
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Line Style")));
        c.response.labelled_by(l.id);
        self.insulation_options(ui);
        if self.shape().chains() {
            ui.checkbox(&mut self.line_chain, t("Chain"));
        }
        self.shape_options(ui);
    }
}
