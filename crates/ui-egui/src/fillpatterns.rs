//! Manage > Annotation Settings > Fill Patterns, as Revit's dialog: the project's fill patterns (families of lines, drafting
//! or model), a draft applied as one undo step (OK and Apply), or dropped (Cancel). A pattern of whole lines (Simple Lines) is
//! edited line by line with Edit Lines: each one's angle, origin and spacing. A custom one is made of lines drawn in a plan, as pyRevit's Make Pattern makes one: New Pattern > Custom hides the dialog,
//! the detail lines are picked (Enter, or the right-click menu's Finish), then the tile's two corners, and the dialog comes
//! back with the pattern previewed, to keep (OK), pick again, or drop. Patterns also come in from a PAT file and go out to
//! one. The filled region types are edited from a region, with Edit Type: the same draft, on the type's own page.

use crate::popup::Modal;

use bimcraft_i18n::msg;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bimcraft_core::engine::Session;
use bimcraft_core::fills::make::{self, Made};
use bimcraft_core::fills::{self, FillPattern, FilledRegionType, HatchLine, MAX_SPACING, MIN_SPACING, Target};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::render::{PAPER, Rgb};
use egui::{
    Align, Button, Color32, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, RichText, ScrollArea, Sense, Shape, Stroke, TextEdit,
    Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::renames::{self, Row};
use crate::style::{ERROR_HEIGHT, boxed_text, canvas_pal, dialog_room, dialog_row, pal};
use crate::typeprops::{self, Head, Naming};
use crate::{BimApp, Status, Tool};

/// A pattern as edited: the name it had (none, new), the name it has, what it is.
struct PatternRow {
    was: Option<String>,
    name: String,
    pattern: FillPattern,
}

/// What New Pattern > Custom is picking in the plan while the dialog is away: the lines, then (they picked) the tile's corners.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Picking {
    Lines,
    Corners(Vec<Id>),
}

/// A type as edited: the name it had, the name it has, its patterns as their rows (none, none), its colours, masking.
#[derive(Clone)]
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
    /// Edit Type's pages: the type (0), or the patterns (1), which are all Manage's dialog shows.
    tab: usize,
    types: Vec<TypeRow>,
    patterns: Vec<PatternRow>,
    /// Patterns deleted: their rows stay, so the types' indices do, and are skipped.
    gone: Vec<bool>,
    /// The pattern whose families Edit Lines shows, and each family's dashes as typed.
    editing: Option<usize>,
    /// Edit Type's current row and the instances to assign it to; None opens Manage's table.
    subject: Option<(usize, Vec<Id>)>,
    /// Edit Type's new name being typed, for a duplicate or a rename.
    naming: Naming,
    error: Option<String>,
    /// What the last import or export said, and whether something went wrong or was left out.
    note: Option<(String, bool)>,
    /// Whether the patterns of a file replace those of the same name in the draft; else they are left out.
    replace: bool,
    /// The patterns and types the project held when this opened: a draft is not applied over a project that has changed since.
    held: (BTreeMap<String, FillPattern>, BTreeMap<String, FilledRegionType>),
    /// New Pattern > Custom, picking in the plan: the dialog is hidden meanwhile.
    pub(crate) picking: Option<Picking>,
    /// The pattern the lines and corners picked make, previewed: OK, Pick Again or Cancel; and those lines, picked again by
    /// Pick Again.
    made: Option<Made>,
    picked: Vec<Id>,
    /// Redefine: the row whose pattern the lines picked make anew; none, New Pattern > Custom makes a new one.
    redefine: Option<usize>,
}

/// What the Patterns tab's buttons ask of the app: a file to import from, or to export to; or the lines of a custom pattern,
/// picked in the plan.
#[derive(Clone, Copy, PartialEq)]
enum FileAsk {
    Import,
    Export,
    Custom,
}

/// What the preview's buttons say: keep the pattern, pick its lines and corners again, or drop it.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    Keep,
    Again,
    Drop,
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
        let held = (m.fill_patterns.clone(), m.region_types.clone());
        FillPatternsDialog {
            tab: 1,
            types,
            patterns,
            gone,
            editing: None,
            subject: None,
            naming: Naming::None,
            error: None,
            note: None,
            replace: false,
            held,
            picking: None,
            made: None,
            picked: Vec::new(),
            redefine: None,
        }
    }

    /// The patterns a PAT file held, added to the draft (one of a name it has already takes its place).
    fn import(&mut self, patterns: Vec<(String, FillPattern)>) {
        for (name, pattern) in patterns {
            match self.patterns.iter().enumerate().position(|(i, r)| !self.gone[i] && r.name == name) {
                Some(i) => self.patterns[i].pattern = pattern,
                None => {
                    self.patterns.push(PatternRow { was: None, name, pattern });
                    self.gone.push(false);
                }
            }
        }
    }

    /// The PAT file at `path`, read by the `import_fill_patterns` command, so that one code path reads, caps and keeps or
    /// replaces; it runs on a scratch project that holds the names of the draft's patterns, whose own keep what it brings.
    /// What came into the draft and what was left out, said.
    fn read(&mut self, path: &Path) -> Result<(String, bool), String> {
        let mut scratch = Session::new();
        let m = scratch.model_mut();
        m.fill_patterns =
            self.live().map(|(_, r)| r.name.trim()).filter(|n| !n.is_empty()).map(|n| (n.to_string(), FillPattern::default())).collect();
        m.region_types.clear();
        for x in m.materials.values_mut() {
            (x.cut_pattern, x.surface_pattern) = (None, None);
        }
        let reply = scratch.execute("import_fill_patterns", &json!({"path": path.to_string_lossy(), "replace": self.replace}))?;
        let names: Vec<&str> = reply["imported"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let brought = names.iter().filter_map(|n| scratch.model.fill_patterns.get(*n).map(|p| (n.to_string(), p.clone()))).collect();
        self.import(brought);
        let out: Vec<String> = (reply["left_out"].as_array().into_iter().flatten())
            .map(|l| {
                let name = l["name"].as_str().unwrap_or_default();
                format!("{} ({})", short(name, 30), short(reason(name, l["why"].as_str().unwrap_or_default()), 70))
            })
            .collect();
        let n = names.len().to_string();
        let one = names.len() == 1;
        // The reply names the first few it left out and counts them all.
        let left = reply["left_out_count"].as_u64().map_or(0, |c| usize::try_from(c).unwrap_or(usize::MAX)).max(out.len());
        Ok(match left {
            0 => (tf(if one { msg!("{} pattern imported.") } else { msg!("{} patterns imported.") }, &[&n]), false),
            k => {
                let list = out.iter().take(LEFT_OUT_SHOWN).cloned().collect::<Vec<_>>().join("; ");
                let list = if k > LEFT_OUT_SHOWN { tf("{}; and {} more", &[&list, &(k - LEFT_OUT_SHOWN).to_string()]) } else { list };
                (
                    tf(
                        if one { msg!("{} pattern imported; left out: {}") } else { msg!("{} patterns imported; left out: {}") },
                        &[&n, &list],
                    ),
                    true,
                )
            }
        })
    }

    /// The patterns as they are in the draft, for a PAT file.
    fn drafted(&self) -> Vec<(String, FillPattern)> {
        self.live().map(|(_, r)| (r.name.clone(), r.pattern.clone())).collect()
    }

    /// Open Edit Type for a region being placed or selected.
    pub(crate) fn edit(m: &Model, name: &str, elements: Vec<Id>) -> Self {
        let mut d = Self::new(m);
        let row = d.types.iter().position(|r| r.name == name).unwrap_or(0);
        (d.subject, d.tab) = (Some((row, elements)), 0);
        d
    }

    /// What stops the draft being applied: a project that changed since it was opened (an agent's command, say), whose
    /// patterns and types the draft would undo or delete unseen; or a name left blank or used twice.
    fn check(&self, m: &Model) -> Result<(), String> {
        if self.held.0 != m.fill_patterns || self.held.1 != m.region_types {
            return Err(t("The project changed while this window was open: Cancel, and open it again.").to_string());
        }
        self.validate_names()
    }

    /// Refuse name collisions before commands can accidentally update an existing definition.
    fn validate_names(&self) -> Result<(), String> {
        for names in [self.types.iter().map(|r| r.name.trim()).collect::<Vec<_>>(), self.live().map(|(_, r)| r.name.trim()).collect()] {
            let mut seen = std::collections::BTreeSet::new();
            for n in names {
                if n.is_empty() {
                    return Err(t("Enter a name for every type and pattern.").to_string());
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

    /// The pattern lines picked made: the row being redefined's, as drafting or model as it is; else a new row, a model
    /// pattern named anew.
    fn keep(&mut self, made: &Made) {
        if let Some(r) = self.redefine.take().and_then(|i| self.patterns.get_mut(i)) {
            r.pattern = made.as_target(r.pattern.target);
            return;
        }
        let taken: Vec<&str> = self.live().map(|(_, p)| p.name.as_str()).collect();
        let name = fresh(t("Pattern"), &taken);
        self.patterns.push(PatternRow { was: None, name, pattern: made.as_target(Target::Model) });
        self.gone.push(false);
    }
}

/// How many of the patterns a file left out the dialog names; the rest it counts.
const LEFT_OUT_SHOWN: usize = 5;

/// `text` cut to at most `most` characters at a word's end, as a name or a reason is told in a message: a word cut in two is a
/// reason lost (a text of one word is cut where it must be).
fn short(text: &str, most: usize) -> String {
    let Some((end, _)) = text.char_indices().nth(most) else { return text.to_string() };
    let cut = &text[..end];
    let cut =
        if text[end..].starts_with(char::is_whitespace) { cut } else { cut.rsplit_once(char::is_whitespace).map_or(cut, |(head, _)| head) };
    format!("{}…", cut.trim_end())
}

/// Why the core left pattern `name` out, without the pattern's name it begins with (the message names it, and the dialog does).
fn reason<'a>(name: &str, why: &'a str) -> &'a str {
    why.strip_prefix(&format!("fill pattern `{name}`")).map_or(why, |rest| rest.trim_start_matches([':', ' ']))
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

/// A button of a row of the list, `width` wide, named `name` for screen readers, showing `text`, saying `tip`.
fn row_button(ui: &mut Ui, text: &str, name: String, tip: &str, width: f32) -> bool {
    let r = ui.add(Button::new(text).small().min_size(vec2(width, 0.0)));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    r.on_hover_text(tip).clicked()
}

/// A small button named `name` for screen readers, showing `text`.
fn named_button(ui: &mut Ui, text: &str, name: String) -> bool {
    let r = ui.add(Button::new(text).small());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    r.clicked()
}

/// A colour button named `name`: red, green and blue only, since the alpha slider of an RGBA button premultiplies them.
fn colour_field(ui: &mut Ui, rgb: &mut [u8; 3], name: String) {
    let r = ui.color_edit_button_srgb(rgb);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, name.clone()));
}

/// A swatch of `p`, `width` x 22 px, in `colour`: a drafting pattern at 5 px to its mm, a model one at a tenth of a px. A
/// `colour` that is the canvas's (ink as the plan draws it) is shown on the canvas's `paper`: in a dark interface with a light
/// canvas, black ink would be lost on the interface's own ground.
pub(crate) fn swatch(ui: &mut Ui, p: Option<&FillPattern>, width: f32, colour: Color32, paper: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    if let Some(paper) = paper {
        ui.painter().rect_filled(rect, 2.0, paper);
    }
    paint_pattern(ui, rect, p, colour);
    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
}

pub(crate) fn paint_pattern(ui: &Ui, rect: egui::Rect, p: Option<&FillPattern>, colour: Color32) {
    // Off the screen (a scroll area's rows past its edge) there is nothing to paint, and a long list is not worked out each frame.
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let Some(p) = p else { return };
    if p.is_solid() {
        painter.rect_filled(rect, 0.0, colour);
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
fn pattern_pick(ui: &mut Ui, row: &mut Option<usize>, names: &[(usize, String)], label: String, width: f32) {
    let shown = row.and_then(|p| names.iter().find(|(i, _)| *i == p)).map_or_else(|| t("<None>").to_string(), |(_, n)| n.clone());
    let c = ComboBox::from_id_salt(&label).selected_text(shown).width(width).show_ui(ui, |ui| {
        ui.selectable_value(row, None, t("<None>"));
        for (p, n) in names {
            ui.selectable_value(row, Some(*p), n);
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
}

/// The lines of pattern `d.editing`, one a row: its angle, origin and spacing; Delete; Add Line and Done, and a gap under them.
/// Whole lines only: a dashed pattern is drawn and made with New Pattern > Custom. The rows scroll within `room`.
fn families(ui: &mut Ui, d: &mut FillPatternsDialog, room: f32) {
    let Some(i) = d.editing else { return };
    let Some(r) = d.patterns.get_mut(i) else { return };
    let who = r.name.clone();
    ui.add_space(6.0);
    ui.label(RichText::new(tf("Lines of {}", &[&who])).strong());
    let mut delete = None;
    // The heading above, the row of buttons and the gap below are not the rows' room.
    let rows = (room - 2.0 * dialog_row(ui) - 6.0 - EDITOR_GAP).max(60.0);
    ScrollArea::vertical().id_salt("pattern-families-rows").max_height(rows).show(ui, |ui| {
        Grid::new("pattern-families").striped(true).num_columns(6).show(ui, |ui| {
            for h in ["", "Angle", msg!("X"), msg!("Y"), msg!("Spacing"), ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (k, l) in r.pattern.lines.iter_mut().enumerate() {
                let n = (k + 1).to_string();
                ui.label(&n);
                let name = |what: &str| tf("{} line {}: {}", &[&who, &n, t(what)]);
                for (what, v, range) in [
                    ("Angle", &mut l.angle, -360.0..=360.0),
                    ("X", &mut l.origin.0, -MAX_SPACING..=MAX_SPACING),
                    ("Y", &mut l.origin.1, -MAX_SPACING..=MAX_SPACING),
                    ("Spacing", &mut l.spacing, MIN_SPACING..=MAX_SPACING),
                ] {
                    let label = name(what);
                    let r = ui.add(DragValue::new(v).range(range).speed(0.1).max_decimals(3));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, label.clone()));
                }
                if named_button(ui, t("Delete"), name("Delete")) {
                    delete = Some(k);
                }
                ui.end_row();
            }
        });
    });
    if let Some(k) = delete {
        r.pattern.lines.remove(k);
    }
    let mut done = false;
    ui.horizontal(|ui| {
        if ui.button(t("Add Line")).clicked() && r.pattern.lines.len() < fills::MAX_FAMILIES {
            r.pattern.lines.push(HatchLine::simple(0.0, if r.pattern.target == Target::Model { 100.0 } else { 2.0 }));
        }
        done = ui.button(t("Done")).clicked();
    });
    // Apart from New Pattern and the files' buttons under it.
    ui.add_space(EDITOR_GAP);
    if done {
        d.editing = None;
    }
}

/// The room under the line editor, between it and the row of buttons.
const EDITOR_GAP: f32 = 20.0;

/// Edit one region type, as in Revit's Type Properties: Duplicate and Rename change the draft, which OK and Apply put in the
/// project with the rest. A pattern's "…" opens the Patterns tab, to edit them.
fn edit_type(ui: &mut Ui, d: &mut FillPatternsDialog, below: f32) {
    let Some(mut picked) = d.subject.as_ref().map(|(row, _)| *row) else { return };
    let names: Vec<String> = d.types.iter().map(|r| r.name.clone()).collect();
    let current = names.get(picked).cloned().unwrap_or_default();
    let family = tf("System Family: {}", &[t("Filled region")]);
    match typeprops::head(ui, "region-type", &family, &names, &current, &mut d.naming, true) {
        Some(Head::Switch(n)) => picked = names.iter().position(|x| *x == n).unwrap_or(picked),
        // A name another type has is refused at once, the name left to change.
        Some(Head::Duplicate(n) | Head::Rename(n)) if names.iter().enumerate().any(|(i, x)| *x == n && i != picked) => {
            d.error = Some(tf("The name `{}` is already in use.", &[&n]));
        }
        Some(Head::Duplicate(n)) => {
            if let Some(r) = d.types.get(picked) {
                let mut copy = r.clone();
                copy.was = None;
                copy.name = n;
                picked = d.types.len();
                d.types.push(copy);
            }
            (d.naming, d.error) = (Naming::None, None);
        }
        Some(Head::Rename(n)) => {
            if let Some(r) = d.types.get_mut(picked) {
                r.name = n;
            }
            (d.naming, d.error) = (Naming::None, None);
        }
        None => {}
    }
    if let Some((row, _)) = &mut d.subject {
        *row = picked;
    }
    let pattern_names: Vec<(usize, String)> = d.live().map(|(i, p)| (i, p.name.clone())).collect();
    let mut open_patterns = false;
    let patterns = &d.patterns;
    let Some(r) = d.types.get_mut(picked) else { return };
    typeprops::table(ui, "region-type", below + 22.0, |tb| {
        tb.group("Graphics");
        for (row, rgb, label, colour) in [
            (&mut r.foreground, &mut r.foreground_color, msg!("Foreground Pattern"), msg!("Foreground Color")),
            (&mut r.background, &mut r.background_color, msg!("Background Pattern"), msg!("Background Color")),
        ] {
            tb.row(t(label), |ui, _| {
                let width = ui.available_width() - 30.0;
                ui.scope(|ui| {
                    ui.set_width(width);
                    pattern_pick(ui, row, &pattern_names, t(label).to_string(), width);
                });
                open_patterns |= named_button(ui, "…", tf("{}: Edit Patterns", &[t(label)]));
            });
            tb.row(t(colour), |ui, _| colour_field(ui, rgb, t(colour).to_string()));
        }
        tb.check(t("Masking"), &mut r.masking);
        tb.group("Preview");
        tb.full(|ui| preview(ui, r, patterns));
    });
    ui.add_space(4.0);
    ui.label(RichText::new(t("Changes to a type affect every filled region of that type.")).small().color(pal().muted));
    if open_patterns {
        d.tab = 1;
    }
}

/// A type's sample as the plan draws a region of it, in the canvas's colours: a model line across the paper, which the
/// region's fills lie under (ink is kept above a region that doesn't mask) unless it masks, when the paper hides the line.
fn preview(ui: &mut Ui, r: &TypeRow, patterns: &[PatternRow]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 100.0), Sense::hover());
    let (paper, ink) = (crate::canvas::color(PAPER), crate::canvas::color(Rgb(0, 0, 0)));
    let inner = rect.shrink(12.0);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, paper);
    let line = || painter.line_segment([rect.left_center(), rect.right_center()], Stroke::new(2.0, ink));
    if r.masking {
        line();
        painter.rect_filled(inner, 0.0, paper);
    }
    for (row, rgb) in [(r.background, r.background_color), (r.foreground, r.foreground_color)] {
        let p = row.and_then(|i| patterns.get(i));
        paint_pattern(ui, inner, p.map(|p| &p.pattern), crate::canvas::color(Rgb(rgb[0], rgb[1], rgb[2])));
    }
    if !r.masking {
        line();
    }
    painter.rect_stroke(inner, 0.0, Stroke::new(1.0, canvas_pal().border), egui::StrokeKind::Inside);
}

/// The patterns: a row each, its name, drafting or model, a swatch (how many line families it has in its tip), Edit Lines for
/// one of whole lines or Redefine for one made of lines drawn; Delete; New Pattern (Simple Lines or Custom), Import and Export (what's
/// asked of the app). A pattern made of lines turned Drafting is the drawing on paper at its view's scale. The list
/// scrolls in what the window has left over `below` (the dialog's notes and buttons) and the row of buttons under it; when a
/// pattern's families are open, the list and they share it.
fn patterns_tab(ui: &mut Ui, d: &mut FillPatternsDialog, below: f32) -> Option<FileAsk> {
    ui.add(
        egui::Label::new(t("Families of parallel lines: drafting ones mm on paper, the same at any scale; model ones mm in the building."))
            .wrap(),
    );
    ui.add_space(4.0);
    let ink = ui.visuals().text_color();
    let (mut delete, mut redefine) = (None, None);
    // Edit Lines and Redefine share a column: one width, the wider name's, in any language.
    let row_width = [t("Edit Lines"), t("Redefine")]
        .map(|x| ui.painter().layout_no_wrap(x.to_string(), egui::TextStyle::Button.resolve(ui.style()), ink).size().x)
        .into_iter()
        .fold(0.0, f32::max)
        + 2.0 * ui.spacing().button_padding.x;
    let room = dialog_room(ui, below + dialog_row(ui));
    let editor = if d.editing.is_some() { room / 2.0 } else { 0.0 };
    ScrollArea::vertical().max_height(room - editor).show(ui, |ui| {
        Grid::new("fill-patterns").striped(true).num_columns(8).show(ui, |ui| {
            for h in ["Name", "Type", msg!("Sample"), "", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, r) in d.patterns.iter_mut().enumerate().filter(|(i, _)| !d.gone[*i]) {
                let who = r.name.clone();
                name_field(ui, &mut r.name, tf("{}: Name", &[&who]));
                let label = tf("{}: Type", &[&who]);
                let shown = t(if r.pattern.target == Target::Model { "Model" } else { "Drafting" });
                let target = r.pattern.target;
                let c = ComboBox::from_id_salt(("fill-target", i)).selected_text(shown).width(90.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut r.pattern.target, Target::Drafting, t("Drafting"));
                    ui.selectable_value(&mut r.pattern.target, Target::Model, t("Model"));
                });
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
                // One made of lines is rescaled: drafting, it is the drawing on paper at its view's scale.
                if r.pattern.target != target {
                    let to = r.pattern.target;
                    r.pattern.target = target;
                    r.pattern = r.pattern.retargeted(to);
                }
                let tip = if r.pattern.is_solid() {
                    t("Solid Fill").to_string()
                } else {
                    tf("{} line families", &[&r.pattern.lines.len().to_string()])
                };
                // The swatch says what it is to screen readers, and in its tip.
                let sample = ui.scope(|ui| swatch(ui, Some(&r.pattern), 110.0, ink, None)).response;
                sample.widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, tip.clone()));
                sample.on_hover_text(tip);
                // Whole lines are edited line by line; one made of lines drawn is made again from lines picked; another (dashed,
                // imported or a starter's) is kept as it came.
                if r.pattern.drawn.is_some() {
                    let tip = t("Pick the lines and the tile's corners again: the pattern keeps its name");
                    if row_button(ui, t("Redefine"), tf("{}: Redefine", &[&who]), tip, row_width) {
                        redefine = Some(i);
                    }
                } else if r.pattern.whole_lines() {
                    if row_button(ui, t("Edit Lines"), tf("{}: Edit Lines", &[&who]), t("Each line's angle, origin and spacing"), row_width)
                    {
                        d.editing = Some(i);
                    }
                } else {
                    ui.label("");
                }
                if named_button(ui, t("Delete"), tf("{}: Delete", &[&who])) {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    families(ui, d, editor);
    if let Some(i) = delete {
        d.gone[i] = true;
        if d.editing == Some(i) {
            d.editing = None;
        }
        for r in &mut d.types {
            for row in [&mut r.foreground, &mut r.background] {
                if *row == Some(i) {
                    *row = None;
                }
            }
        }
    }
    let mut ask = None;
    if redefine.is_some() {
        (d.redefine, ask) = (redefine, Some(FileAsk::Custom));
    }
    ui.horizontal(|ui| {
        let mut simple = false;
        ui.menu_button(t("New Pattern"), |ui| {
            simple =
                ui.button(t("Simple Lines")).on_hover_text(t("Parallel lines at an angle and a spacing, crosshatched or not")).clicked();
            let tip = t("Made of detail lines picked in the plan, repeating the rectangle between two corners");
            if ui.button(t("Custom")).on_hover_text(tip).clicked() {
                (d.redefine, ask) = (None, Some(FileAsk::Custom));
            }
        });
        if simple {
            let taken: Vec<&str> = d.live().map(|(_, p)| p.name.as_str()).collect();
            let name = fresh(t("Pattern"), &taken);
            let pattern = FillPattern { target: Target::Drafting, lines: vec![HatchLine::simple(45.0, 2.0)], drawn: None };
            d.patterns.push(PatternRow { was: None, name, pattern });
            d.gone.push(false);
        }
        // A PAT file's, on the right: what an import does with a pattern of a name the list has already is the check box's.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(t("Export...")).on_hover_text(t("These patterns to a PAT file, but the solid ones")).clicked() {
                ask = Some(FileAsk::Export);
            }
            if ui.button(t("Import...")).on_hover_text(t("Patterns from a PAT file, AutoCAD's and Revit's")).clicked() {
                ask = Some(FileAsk::Import);
            }
            ui.checkbox(&mut d.replace, t("Replace imported by same name"))
                .on_hover_text(t("Import: a pattern of the file replaces the one of its name in this list; unchecked, it is left out"));
        });
    });
    ask
}

impl FillPatternsDialog {
    /// What the draft asks of the project, in an order each can be done in: patterns renamed (so a chain or a swap goes
    /// through, [`renames`]), patterns made and changed, types renamed, made and changed, then what's deleted, types before
    /// patterns. A pattern or a type deleted and made again under its name is changed in place.
    fn commands(&self, m: &Model) -> Vec<(&'static str, Value)> {
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        let rows: Vec<Row> =
            (self.patterns.iter().zip(&self.gone)).map(|(r, g)| Row { was: r.was.as_deref(), name: &r.name, gone: *g }).collect();
        let patterns = renames::plan(&m.fill_patterns.keys().cloned().collect(), &rows);
        out.extend(patterns.renames.iter().map(|(from, to)| ("fill_pattern", json!({"name": from, "rename": to}))));
        for (_, r) in self.live() {
            if r.was.as_ref().and_then(|w| m.fill_patterns.get(w)) != Some(&r.pattern) {
                let p = &r.pattern;
                out.push(("fill_pattern", json!({"name": r.name, "target": p.target, "lines": p.lines, "drawn": p.drawn})));
            }
        }
        // A pattern renamed renames it in the types already: compare what the type holds read through the renames.
        let renamed = |n: &String| self.patterns.iter().find(|r| r.was.as_ref() == Some(n)).map_or_else(|| n.clone(), |r| r.name.clone());
        let kept: BTreeSet<&str> = self.types.iter().filter_map(|r| r.was.as_deref()).collect();
        let mut rows: Vec<Row> = self.types.iter().map(|r| Row { was: r.was.as_deref(), name: &r.name, gone: false }).collect();
        rows.extend(m.region_types.keys().filter(|n| !kept.contains(n.as_str())).map(|n| Row { was: Some(n), name: n, gone: true }));
        let types = renames::plan(&m.region_types.keys().cloned().collect(), &rows);
        out.extend(types.renames.iter().map(|(from, to)| ("filled_region_type", json!({"name": from, "rename": to}))));
        for r in &self.types {
            let mine = self.region_type(r);
            let same = |x: &FilledRegionType| {
                FilledRegionType {
                    foreground: x.foreground.as_ref().map(renamed),
                    background: x.background.as_ref().map(renamed),
                    ..x.clone()
                } == mine
            };
            if !r.was.as_ref().and_then(|w| m.region_types.get(w)).is_some_and(same) {
                out.push((
                    "filled_region_type",
                    json!({
                        "name": r.name, "foreground": mine.foreground, "foreground_color": mine.foreground_color,
                        "background": mine.background, "background_color": mine.background_color, "masking": mine.masking,
                    }),
                ));
            }
        }
        out.extend(types.deletes.iter().map(|n| ("filled_region_type", json!({"name": n, "delete": true}))));
        out.extend(patterns.deletes.iter().map(|n| ("fill_pattern", json!({"name": n, "delete": true}))));
        out
    }
}

impl BimApp {
    /// The Fill Patterns dialog, while it's open; away while a custom pattern's lines and corners are picked in the plan (Esc,
    /// another tool or another view brings it back).
    pub(crate) fn fill_patterns_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.fill_patterns_dialog.take() else { return };
        if let Some(p) = &d.picking {
            let on = match p {
                Picking::Lines => self.tool == Tool::Modify,
                Picking::Corners(_) => matches!(self.tool, Tool::PatternCorner(_)),
            };
            if on && self.plan_level().is_some() {
                self.fill_patterns_dialog = Some(d);
                return;
            }
            (d.picking, d.redefine) = (None, None);
        }
        let editing = d.subject.is_some();
        let (mut ok, mut apply, mut cancel, mut ask, mut step) = (false, false, false, None, None);
        let width = if editing && d.tab == 0 && d.made.is_none() { typeprops::WIDTH } else { WIDTH };
        let units = self.session.model.units;
        let r = Modal::new(UiId::new("fill-patterns"), t(if editing { "Type Properties" } else { msg!("Fill Patterns") }))
            .width(width)
            .show(ctx, |ui| {
                crate::popup::body(ui, |ui| {
                    if let Some(made) = &d.made {
                        let of = d.redefine.and_then(|i| d.patterns.get(i)).map(|r| r.name.as_str());
                        made_preview(ui, made, of, units);
                        return;
                    }
                    if editing && d.tab == 0 {
                        let below = typeprops::foot_height(ui, d.error.is_some());
                        edit_type(ui, &mut d, below);
                        typeprops::error(ui, d.error.as_deref());
                        return;
                    }
                    if editing {
                        ui.add_space(6.0);
                        if ui.button(t("‹ Back")).on_hover_text(t("Back to the type")).clicked() {
                            d.tab = 0;
                        }
                        ui.separator();
                    }
                    // Under the list: its notes, then OK, Cancel and Apply. What the window has left over them is the list's, so
                    // that on a small one the buttons stay in reach and the rows scroll.
                    let below = messages_height(&d) + 8.0 + dialog_row(ui);
                    ask = patterns_tab(ui, &mut d, below);
                    messages(ui, &d);
                });
                crate::popup::footer(ui, |ui| {
                    if d.made.is_some() {
                        step = made_foot(ui);
                    } else {
                        (ok, cancel, apply) = typeprops::foot(ui);
                    }
                });
            });
        match step {
            Some(Step::Keep) => {
                if let Some(made) = d.made.take() {
                    d.keep(&made);
                }
            }
            Some(Step::Again) => {
                d.made = None;
                self.selection = std::mem::take(&mut d.picked);
                ask = Some(FileAsk::Custom);
            }
            Some(Step::Drop) => (d.made, d.redefine) = (None, None),
            None => {}
        }
        let failed = (ok || apply) && !self.apply_fill_patterns(&mut d, apply);
        // The draft is read only for the file it is exported to.
        let drafted = matches!(ask, Some(FileAsk::Export)).then(|| d.drafted());
        if failed || !(ok || cancel || r.should_close()) {
            self.fill_patterns_dialog = Some(d);
        }
        match (ask, drafted) {
            (Some(FileAsk::Import), _) => {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("Fill patterns"), &["pat", "PAT"]);
                self.ask_file(ctx, dialog, false, |app, path| app.import_patterns(&path));
            }
            (Some(FileAsk::Export), Some(drafted)) => {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("Fill patterns"), &["pat"]).set_file_name("patterns.pat");
                self.ask_file(ctx, dialog, true, move |app, path| app.export_patterns(&path, &drafted));
            }
            (Some(FileAsk::Custom), _) => self.pick_pattern_lines(),
            _ => {}
        }
    }

    /// New Pattern > Custom and Pick Again: the dialog steps aside for the plan, where the pattern's detail lines are picked
    /// as anything is (those picked already stay picked); refused, saying why, outside a plan or while a sketch is open.
    fn pick_pattern_lines(&mut self) {
        let why = if self.sketch.is_some() {
            Some(t("Finish or cancel the sketch first: a pattern's lines are picked in the plan."))
        } else if self.plan_level().is_none() {
            Some(t("Open a plan to pick the pattern's lines in: detail lines are drawn in plans."))
        } else {
            None
        };
        let Some(d) = &mut self.fill_patterns_dialog else { return };
        d.error = why.map(str::to_string);
        if why.is_none() {
            d.picking = Some(Picking::Lines);
            self.status = Status::default();
            if self.tool != Tool::Modify {
                self.set_tool(Tool::Modify);
            }
        }
    }

    /// Whether a custom pattern's lines are being picked: Enter and the menu's Finish take them.
    pub(crate) fn picking_pattern_lines(&self) -> bool {
        self.fill_patterns_dialog.as_ref().is_some_and(|d| d.picking == Some(Picking::Lines))
    }

    /// Enter, or the menu's Finish, while a custom pattern's lines are picked: the detail lines selected are the pattern's,
    /// and its tile's corners are asked next; with none selected, that is said.
    pub(crate) fn finish_pattern_lines(&mut self) {
        let m = &self.session.model;
        let lines: Vec<Id> =
            self.selection.iter().copied().filter(|id| matches!(m.elements.get(*id), Some(Element::DetailLine(_)))).collect();
        let Some(d) = self.fill_patterns_dialog.as_mut().filter(|d| d.picking == Some(Picking::Lines)) else { return };
        if lines.is_empty() {
            let text = t("Select the detail lines the pattern is made of, then press Enter.").to_string();
            self.status = Status { text, error: true };
            return;
        }
        d.picking = Some(Picking::Corners(lines));
        self.status = Status::default();
        self.set_tool(Tool::PatternCorner(None));
    }

    /// Esc while a custom pattern is being picked: back to the dialog. Whether there was one.
    pub(crate) fn cancel_pattern_pick(&mut self) -> bool {
        let Some(d) = self.fill_patterns_dialog.as_mut().filter(|d| d.picking.is_some()) else { return false };
        d.picking = None;
        self.status = Status::default();
        if matches!(self.tool, Tool::PatternCorner(_)) {
            self.tool = Tool::Modify;
        }
        true
    }

    /// The tile's second corner clicked, `b`, the first `a`: the pattern the lines picked make of it, previewed in the dialog;
    /// or what's wrong said and the corners asked again.
    pub(crate) fn pattern_corners(&mut self, a: P2, b: P2) {
        let Some(d) = self.fill_patterns_dialog.as_mut() else {
            self.tool = Tool::Modify;
            return;
        };
        let Some(Picking::Corners(lines)) = &d.picking else {
            self.tool = Tool::Modify;
            return;
        };
        match make::from_lines(&self.session.model, lines, a, b) {
            Ok(made) => {
                d.picked = lines.clone();
                (d.made, d.picking, d.error) = (Some(made), None, None);
                self.status = Status::default();
                self.tool = Tool::Modify;
            }
            Err(e) => {
                self.status = Status { text: e, error: true };
                self.tool = Tool::PatternCorner(None);
            }
        }
    }

    /// OK and Apply: the draft done as one undo step, all or nothing, or refused (a project that changed since it opened, a
    /// name blank or used twice, the core's reasons) with the dialog saying why. Whether it was done.
    fn apply_fill_patterns(&mut self, d: &mut FillPatternsDialog, apply: bool) -> bool {
        let editing = d.subject.is_some();
        d.error = None;
        // Normalize names before validation and assignment, as the commands do.
        d.types.iter_mut().for_each(|r| r.name = r.name.trim().to_string());
        d.patterns.iter_mut().for_each(|r| r.name = r.name.trim().to_string());
        let selected = d.subject.as_ref().and_then(|(i, ids)| d.types.get(*i).map(|r| (r.name.clone(), ids.clone())));
        let done = d.check(&self.session.model).and_then(|()| {
            let mut commands = d.commands(&self.session.model);
            if let Some((name, ids)) = &selected
                && !ids.is_empty()
            {
                commands.push(("modify", json!({"ids": ids, "type": name})));
            }
            if commands.is_empty() {
                return Ok(());
            }
            self.session.transaction(if editing { "Type Properties" } else { "Fill Patterns" }, |s: &mut Session| {
                commands.iter().try_for_each(|(c, p)| s.execute(c, p).map(|_| ()))
            })
        });
        if let Err(e) = done {
            d.error = Some(e.clone());
            self.status = Status { text: e, error: true };
            return false;
        }
        self.status = Status::default();
        // The sketch is a draft too: keep its type reference in step with applied renames.
        if let Some(sk) = &mut self.sketch {
            if let Some(row) = d.types.iter().find(|r| r.was.as_ref() == Some(&sk.region.0)) {
                sk.region.0 = row.name.clone();
            }
            if let Some((name, ids)) = &selected
                && ids.is_empty()
            {
                sk.region.0 = name.clone();
            }
        }
        if let Some((name, _)) = &selected {
            self.types.insert("region", name.clone());
        }
        if apply {
            let tab = d.tab;
            *d = match selected {
                Some((name, ids)) => FillPatternsDialog::edit(&self.session.model, &name, ids),
                None => FillPatternsDialog::new(&self.session.model),
            };
            d.tab = tab;
        }
        true
    }

    /// The patterns of the PAT file at `path`, into the Fill Patterns dialog's draft; what came and what didn't, said in it.
    pub(crate) fn import_patterns(&mut self, path: &Path) {
        let Some(d) = &mut self.fill_patterns_dialog else { return };
        d.note = Some(d.read(path).unwrap_or_else(|e| (e, true)));
    }

    /// `patterns` written to the PAT file at `path`, ending in `.pat`: a file by that name that the system's Save didn't ask
    /// about is not written over.
    pub(crate) fn export_patterns(&mut self, path: &Path, patterns: &[(String, FillPattern)]) {
        let named = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pat"));
        let target = if named { path.to_path_buf() } else { path.with_extension("pat") };
        let (text, error) = if !named && target.exists() {
            (tf("{} already exists: save under a name ending in .pat to replace it.", &[&target.display().to_string()]), true)
        } else {
            let text = bimcraft_core::pat::write(patterns.iter().map(|(n, p)| (n, p)));
            match bimcraft_core::autosave::atomic_write(&target, &text) {
                Ok(()) => (tf("Fill patterns exported to {}", &[&target.display().to_string()]), false),
                Err(e) => (e, true),
            }
        };
        match &mut self.fill_patterns_dialog {
            Some(d) => d.note = Some((text, error)),
            None => self.status = Status { text, error },
        }
    }
}

/// How wide the dialog is: its list's columns, and the row of buttons under them.
const WIDTH: f32 = 560.0;

/// How tall the preview of a pattern made of lines is.
const MADE_HEIGHT: f32 = 300.0;

/// The pattern lines picked made, on the canvas's paper: three tiles each way about the one picked, which is outlined.
fn made_preview(ui: &mut Ui, made: &Made, of: Option<&str>, units: bimcraft_core::units::Units) {
    ui.add_space(6.0);
    let head = of.map_or_else(|| t("New pattern from lines").to_string(), |name| tf("{} redefined from lines", &[name]));
    ui.label(RichText::new(head).strong());
    let families = made.pattern.lines.len().to_string();
    let (w, h) = (units.length(made.tile.0), units.length(made.tile.1));
    ui.label(tf("{} line families, repeating a tile {} × {} (outlined).", &[&families, &w, &h]));
    ui.add_space(4.0);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), MADE_HEIGHT), Sense::hover());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, t("Pattern preview")));
    let (paper, ink) = (crate::canvas::color(PAPER), crate::canvas::color(Rgb(0, 0, 0)));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, paper);
    let P2(tw, th) = made.tile;
    let px = (f64::from(rect.width()) / (3.0 * tw)).min(f64::from(rect.height()) / (3.0 * th));
    if !(px.is_finite() && px > 0.0) {
        return;
    }
    let centre = P2(tw / 2.0, th / 2.0);
    let half = P2(f64::from(rect.width()) / 2.0 / px, f64::from(rect.height()) / 2.0 / px);
    let (lo, hi) = (centre - half, centre + half);
    let at = |q: P2| pos2(rect.center().x + ((q.0 - centre.0) * px) as f32, rect.center().y - ((q.1 - centre.1) * px) as f32);
    let ring = vec![lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)];
    match fills::strokes(&made.pattern, std::slice::from_ref(&ring), 1.0, (lo, hi), 40_000) {
        Some(strokes) => {
            for [a, b] in strokes {
                painter.line_segment([at(a), at(b)], Stroke::new(1.0, ink));
            }
        }
        None => {
            let text = t("Too dense to preview at this size.");
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(13.0), ink);
        }
    }
    let tile = [P2(0.0, 0.0), P2(tw, 0.0), P2(tw, th), P2(0.0, th), P2(0.0, 0.0)].map(at);
    painter.extend(Shape::dashed_line(&tile, Stroke::new(1.5, pal().accent), 6.0, 4.0));
}

/// The preview's buttons: OK keeps the pattern, Pick Again picks its lines and corners anew, Cancel drops it.
fn made_foot(ui: &mut Ui) -> Option<Step> {
    let mut step = None;
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        let size = vec2(76.0, 0.0);
        for (label, what) in [("Cancel", Step::Drop), (msg!("Pick Again"), Step::Again), ("OK", Step::Keep)] {
            if ui.add(Button::new(t(label)).min_size(size)).clicked() {
                step = Some(what);
            }
        }
    });
    step
}

/// The tallest the note is, in a box that scrolls.
const NOTE_HEIGHT: f32 = 60.0;

/// The note, when the Patterns tab shows it: what an import or export said belongs to the patterns, not to the types.
fn note(d: &FillPatternsDialog) -> Option<&(String, bool)> {
    d.note.as_ref().filter(|_| d.tab == 1)
}

/// How tall what `messages` draws can be, with the gaps between.
fn messages_height(d: &FillPatternsDialog) -> f32 {
    let gap = 6.0;
    note(d).map_or(0.0, |_| NOTE_HEIGHT + gap) + d.error.as_ref().map_or(0.0, |_| ERROR_HEIGHT + gap)
}

/// What the dialog says in itself, under its tab: the last import or export (a long list of what was left out scrolls), and
/// what stopped Apply.
fn messages(ui: &mut Ui, d: &FillPatternsDialog) {
    if let Some((text, bad)) = note(d) {
        boxed_text(ui, "fill-patterns-note", if *bad { pal().error } else { ui.visuals().text_color() }, text, NOTE_HEIGHT);
    }
    if let Some(error) = &d.error {
        boxed_text(ui, "fill-patterns-error", pal().error, error, ERROR_HEIGHT);
    }
}
