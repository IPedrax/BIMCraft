//! Manage > Line Styles, as Revit's dialogs: the project's line styles (a pen, a colour and a pattern each) and its line
//! patterns (graphic segments, mm on paper), on two tabs of one draft, applied as one undo step (OK and Apply), or
//! dropped (Cancel).

use crate::popup::Modal;

use bimcraft_i18n::msg;

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::engine::Session;
use bimcraft_core::lines::{LinePattern, LineStyle, SOLID, Segment};
use bimcraft_core::model::Model;
use bimcraft_core::pen::{MAX_PEN, Pen};
use egui::{
    Align, Button, ComboBox, Context, Grid, Id as UiId, Layout, RichText, ScrollArea, Sense, TextEdit, Ui, WidgetInfo, WidgetType, vec2,
};
use serde_json::{Value, json};

use crate::graphics::pen_name;
use crate::lang::{t, tf};
use crate::renames::{self, Row};
use crate::style::{ERROR_HEIGHT, boxed_text, dialog_room, dialog_row, pal};
use crate::{BimApp, Status};

/// The dialog's tabs.
mod creator;

const TABS: [&str; 2] = [msg!("Styles"), "Patterns"];

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
    creator: Option<creator::PatternEditor>,
    /// Why Apply was refused, said in the dialog.
    error: Option<String>,
    /// The patterns and styles the project held when this opened: a draft is not applied over a project that has changed since.
    held: (BTreeMap<String, LinePattern>, BTreeMap<String, LineStyle>),
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
        let held = (m.line_patterns.clone(), m.line_styles.clone());
        LineStylesDialog { tab: 0, styles, patterns, gone, creator: None, error: None, held }
    }

    /// Open on the Patterns tab, as Manage > Annotation Settings > Line Patterns does.
    pub(crate) fn patterns(m: &Model) -> LineStylesDialog {
        LineStylesDialog { tab: 1, ..Self::new(m) }
    }

    /// What stops the draft being applied: a project that changed since it was opened (an agent's command, say), or a name
    /// left blank or used twice, which the commands would take for a listing or for an edit of the entry of that name.
    fn check(&self, m: &Model) -> Result<(), String> {
        if self.held.0 != m.line_patterns || self.held.1 != m.line_styles {
            return Err(t("The project changed while this window was open: Cancel, and open it again.").to_string());
        }
        for names in [self.styles.iter().map(|s| s.name.trim()).collect::<Vec<_>>(), self.live().map(|(_, p)| p.name.trim()).collect()] {
            let mut seen = BTreeSet::new();
            for n in names {
                if n.is_empty() {
                    return Err(t("Enter a name for every style and pattern.").to_string());
                }
                if !seen.insert(n) {
                    return Err(tf("The name `{}` is already in use.", &[n]));
                }
            }
        }
        Ok(())
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
    let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    creator::paint(ui, segments, rect, k, false);
}

/// The styles: a row each, its name, pen, colour and pattern, and its Delete; then New Style. The rows scroll in what the window
/// has left over `below`, the dialog's own notes and buttons.
fn styles_tab(ui: &mut Ui, d: &mut LineStylesDialog, below: f32) {
    ui.label(t("What detail lines draw in: a pen, a colour and a line pattern."));
    ui.add_space(4.0);
    let names: Vec<(usize, String)> = d.live().map(|(i, p)| (i, p.name.clone())).collect();
    let mut delete = None;
    ScrollArea::vertical().max_height(dialog_room(ui, below + dialog_row(ui))).show(ui, |ui| {
        Grid::new("line-styles").striped(true).num_columns(5).show(ui, |ui| {
            for h in ["Name", msg!("Line Weight"), msg!("Colour"), "Pattern", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, s) in d.styles.iter_mut().enumerate() {
                let who = s.name.clone();
                name_field(ui, &mut s.name, tf("{}: Name", &[&who]));
                pen_field(ui, &mut s.pen, tf("{}: Line Weight", &[&who]));
                // Red, green and blue only: the alpha slider of an RGBA button premultiplies them and darkens the colour.
                let r = ui.color_edit_button_srgb(&mut s.color);
                r.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, tf("{}: Colour", &[&who])));
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

/// The pattern library. New and Edit open an independent creator draft.
fn patterns_tab(ui: &mut Ui, d: &mut LineStylesDialog, below: f32) {
    ui.label(t("Graphic segments in mm on paper, repeating along the centerline."));
    ui.add_space(4.0);
    let mut delete = None;
    ScrollArea::vertical().id_salt("line-patterns-rows").max_height(dialog_room(ui, below + dialog_row(ui))).show(ui, |ui| {
        Grid::new("line-patterns").striped(true).num_columns(4).show(ui, |ui| {
            for h in ["Name", "Sample", "", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, p) in d.patterns.iter_mut().enumerate().filter(|(i, _)| !d.gone[*i]) {
                let who = p.name.clone();
                name_field(ui, &mut p.name, tf("{}: Name", &[&who]));
                sample(ui, &p.segments, 160.0, 6.0);
                if named_button(ui, t("Edit"), tf("{}: Edit", &[&who])) {
                    d.creator = Some(creator::PatternEditor::new(Some(i), p.name.clone(), p.segments.clone()));
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
        for s in d.styles.iter_mut().filter(|s| s.pattern == Some(i)) {
            s.pattern = None;
        }
    }
    if ui.button(t("New Pattern")).clicked() {
        let taken: Vec<&str> = d.live().map(|(_, p)| p.name.as_str()).collect();
        let name = fresh(t("Pattern"), &taken);
        d.creator = Some(creator::PatternEditor::new(None, name, vec![Segment::Dash(3.0), Segment::Space(1.5)]));
    }
}

impl LineStylesDialog {
    /// What the draft asks of the project, as commands in an order each can be done in: patterns renamed (so a chain or a
    /// swap goes through, [`renames`]), made and changed, styles renamed, made and changed, then what's deleted, styles before
    /// patterns. An entry deleted and made again under its name is changed in place.
    fn commands(&self, m: &Model) -> Vec<(&'static str, Value)> {
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        let rows: Vec<Row> =
            (self.patterns.iter().zip(&self.gone)).map(|(r, g)| Row { was: r.was.as_deref(), name: &r.name, gone: *g }).collect();
        let patterns = renames::plan(&m.line_patterns.keys().cloned().collect(), &rows);
        out.extend(patterns.renames.iter().map(|(from, to)| ("line_pattern", json!({"name": from, "rename": to}))));
        for (_, r) in self.live() {
            if r.was.as_ref().and_then(|w| m.line_patterns.get(w)) != Some(&LinePattern(r.segments.clone())) {
                out.push(("line_pattern", json!({"name": r.name, "segments": r.segments})));
            }
        }
        let kept: BTreeSet<&str> = self.styles.iter().filter_map(|s| s.was.as_deref()).collect();
        let mut rows: Vec<Row> = self.styles.iter().map(|s| Row { was: s.was.as_deref(), name: &s.name, gone: false }).collect();
        rows.extend(m.line_styles.keys().filter(|n| !kept.contains(n.as_str())).map(|n| Row { was: Some(n), name: n, gone: true }));
        let styles = renames::plan(&m.line_styles.keys().cloned().collect(), &rows);
        out.extend(styles.renames.iter().map(|(from, to)| ("line_style", json!({"name": from, "rename": to}))));
        for s in &self.styles {
            let pattern = s.pattern.and_then(|i| self.patterns.get(i)).map(|p| p.name.clone());
            let mine = LineStyle { pen: s.pen, color: s.color, pattern: pattern.clone() };
            // A pattern renamed renames it in the style already: what the style holds then is what to compare.
            if !s.was.as_ref().and_then(|w| m.line_styles.get(w)).is_some_and(|x| styles_match(x, &mine, self)) {
                let pattern = pattern.map_or_else(|| json!(SOLID), |p| json!(p));
                out.push(("line_style", json!({"name": s.name, "pen": s.pen, "color": s.color, "pattern": pattern})));
            }
        }
        out.extend(styles.deletes.iter().map(|n| ("line_style", json!({"name": n, "delete": true}))));
        out.extend(patterns.deletes.iter().map(|n| ("line_pattern", json!({"name": n, "delete": true}))));
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
        let editing = d.creator.is_some();
        let r = Modal::new(UiId::new("line-styles"), t("Line Styles")).width(640.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                if editing {
                    ui.disable();
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (i, tab) in TABS.iter().enumerate() {
                        if ui.selectable_label(d.tab == i, t(tab)).clicked() {
                            d.tab = i;
                        }
                    }
                });
                ui.separator();
                // Under the tab: the error, then OK, Cancel and Apply. What the window has left over them is the tab's, so that on a
                // small one the buttons stay in reach and the rows scroll.
                let below = d.error.as_ref().map_or(0.0, |_| ERROR_HEIGHT + 6.0) + 8.0 + dialog_row(ui);
                if d.tab == 0 {
                    styles_tab(ui, &mut d, below);
                } else {
                    patterns_tab(ui, &mut d, below);
                }
                if let Some(error) = &d.error {
                    boxed_text(ui, "line-styles-error", pal().error, error, ERROR_HEIGHT);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        // Settle the child modal's measured size before the next pointer event uses its widget positions.
        if !editing && d.creator.is_some() {
            ctx.request_repaint();
        }
        creator::window(ctx, &mut d);
        let failed = (ok || apply) && !self.apply_line_styles(&mut d, apply);
        if failed || !(ok || cancel || (!editing && r.should_close())) {
            self.line_styles_dialog = Some(d);
        }
    }

    /// OK and Apply: what changed, as one undo step, all or nothing; a refusal (a project that changed since it opened, a name
    /// blank or used twice, the core's reasons) keeps the dialog open to fix it, saying why in itself. Whether it was done.
    fn apply_line_styles(&mut self, d: &mut LineStylesDialog, apply: bool) -> bool {
        d.error = None;
        d.styles.iter_mut().for_each(|s| s.name = s.name.trim().to_string());
        d.patterns.iter_mut().for_each(|p| p.name = p.name.trim().to_string());
        let done = d.check(&self.session.model).and_then(|()| {
            let commands = d.commands(&self.session.model);
            if commands.is_empty() {
                return Ok(());
            }
            self.session.transaction("Line Styles", |s: &mut Session| commands.iter().try_for_each(|(c, p)| s.execute(c, p).map(|_| ())))
        });
        if let Err(e) = done {
            d.error = Some(e.clone());
            self.status = Status { text: e, error: true };
            return false;
        }
        self.status = Status::default();
        if apply {
            let tab = d.tab;
            *d = LineStylesDialog::new(&self.session.model);
            d.tab = tab;
        }
        true
    }
}

/// The Linework tool's choices besides the line styles: hide the edge, or put it back as its category draws it.
pub(crate) const INVISIBLE: &str = "<Invisible lines>";
pub(crate) const BY_CATEGORY: &str = msg!("<By Category>");

impl BimApp {
    /// The Linework tool's Options Bar: the style the edge clicked takes, hidden, or back by category.
    pub(crate) fn linework_options(&mut self, ui: &mut Ui) {
        let m = &self.session.model;
        let options: Vec<String> = [INVISIBLE, BY_CATEGORY].into_iter().map(str::to_string).chain(m.line_styles.keys().cloned()).collect();
        if !options.contains(&self.linework_style) {
            let wide = m.language.word("Wide Lines").to_string();
            self.linework_style = if m.line_styles.contains_key(&wide) { wide } else { INVISIBLE.to_string() };
        }
        let l = ui.label(t("Line Style:"));
        let c = ComboBox::from_id_salt("linework-style").selected_text(t(&self.linework_style)).width(160.0).show_ui(ui, |ui| {
            for n in &options {
                ui.selectable_value(&mut self.linework_style, n.clone(), t(n));
            }
        });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Line Style")));
        c.response.labelled_by(l.id);
    }

    /// The Detail Line tool's Options Bar: the line style it draws in.
    pub(crate) fn line_style_options(&mut self, ui: &mut Ui) {
        let m = &self.session.model;
        if !m.line_styles.contains_key(&self.line_style) {
            self.line_style = bimcraft_core::lines::default_style(&m.line_styles, m.language).cloned().unwrap_or_default();
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
