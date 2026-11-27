//! A curtain wall type's Edit Pattern (egui's `curtain.rs`): its grid made by adding segments (a column or a row of a size each, how the wall
//! fits and justifies them), the panel type of each cell and the mullion of each grid line picked, the mullions round the wall and at its
//! corner, and the smallest panel left. egui draws the module to scale and a click picks a piece; here the pieces are lists of check boxes
//! (a panel by its column and row, a mullion by the line it runs along), and what is picked is given a type below. OK or Apply puts the pattern on the type
//! as one `type` command. The type is the selected curtain wall's.

use bimcraft_core::curtain::{Continuous, CurtainLayout, Fit, Join, Justify, MullionPick};
use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::units::Units;
use rune::TextEdit;
use serde_json::json;

use super::{Cell, Choice, Discipline, Foot, Host, Outcome, Section, edits, typed};
use crate::lang::{t, tf};

/// The most segments a way has: the pieces are named by a column letter and a row number.
const MOST: usize = 64;

/// The places of the edits: the columns' sizes, the rows' sizes and the smallest panel.
const COLUMNS: usize = 0;
const ROWS: usize = 1000;
const SMALLEST: usize = 2000;

/// The lists, by place.
const COLUMN_FIT: usize = 0;
const COLUMN_JUSTIFY: usize = 1;
const ROW_FIT: usize = 2;
const ROW_JUSTIFY: usize = 3;
/// The four borders (left, right, bottom, top) and the corner mullion.
const BORDER: usize = 4;
const CORNER: usize = 8;
const CONTINUOUS: usize = 9;
const PANEL: usize = 10;
const MULLION: usize = 11;
const JOIN: usize = 12;

const FITS: [(Fit, &str); 3] = [(Fit::Flexible, "Last Grid Flexible"), (Fit::Fixed, "Fixed Grid"), (Fit::Evenly, "Fit Evenly")];
const JUSTIFIES: [(Justify, &str); 4] =
    [(Justify::Start, "Start"), (Justify::Centre, "Centre"), (Justify::CentreLine, "Centre Line"), (Justify::End, "End")];
const CONTINUOUSES: [(Continuous, &str); 2] =
    [(Continuous::Vertical, "Vertical Mullions"), (Continuous::Horizontal, "Horizontal Mullions")];
const JOINS: [(Join, &str); 2] = [(Join::Butt, "Butt"), (Join::Mitre, "Mitre")];

/// A place on the pattern that holds a type or nothing: `row` and `column` of its segments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    Cell(usize, usize),
    Vertical(usize, usize),
    Horizontal(usize, usize),
}

impl Piece {
    /// The place a check box gives when it is pressed, and the piece that is.
    fn code(self) -> usize {
        match self {
            Piece::Cell(r, c) => r * 100 + c,
            Piece::Vertical(r, c) => 10_000 + r * 100 + c,
            Piece::Horizontal(r, c) => 20_000 + r * 100 + c,
        }
    }

    fn of(code: usize) -> Piece {
        let (r, c) = ((code % 10_000) / 100, code % 100);
        match code / 10_000 {
            0 => Piece::Cell(r, c),
            1 => Piece::Vertical(r, c),
            _ => Piece::Horizontal(r, c),
        }
    }
}

/// A column's name: A, B... Z, AA.
fn letter(i: usize) -> String {
    let c = |k: usize| char::from(b'A' + (k % 26) as u8);
    if i < 26 { c(i).to_string() } else { format!("{}{}", c(i / 26 - 1), c(i)) }
}

/// Edit Pattern: one curtain wall type's pattern as it's being made.
pub struct Pattern {
    name: String,
    l: CurtainLayout,
    units: Units,
    /// The typed sizes of the columns, then the rows, then the smallest panel.
    columns: Vec<TextEdit>,
    rows: Vec<TextEdit>,
    smallest: TextEdit,
    selected: Vec<Piece>,
    /// The types the lists offer.
    panels: Vec<String>,
    mullions: Vec<String>,
    corners: Vec<String>,
    /// What the pattern makes: its module and thickness, as of the last change.
    note: String,
    error: Option<String>,
}

/// The place of `value` in a list of values and their labels.
fn place<T: PartialEq + Copy>(options: &[(T, &str)], value: T) -> Option<usize> {
    options.iter().position(|(v, _)| *v == value)
}

impl Pattern {
    /// Opens on the curtain wall type of the wall selected. Why not, in words.
    pub fn open(m: &Model, selection: &[Id]) -> Result<Pattern, String> {
        let name = match selection {
            [id] => crate::typeprops::of_element(m, *id).filter(|k| k.category == "wall").map(|k| k.name),
            _ => None,
        };
        let layout = name.as_ref().and_then(|n| m.types.wall.get(n)).and_then(|w| w.curtain.clone());
        let (Some(name), Some(l)) = (name, layout) else {
            return Err(t("Select a curtain wall to edit its pattern first.").to_string());
        };
        let sizes = |d: &bimcraft_core::curtain::Direction| edits(d.segments.iter().map(|s| m.field(*s)));
        let mut d = Pattern {
            name,
            units: m.units,
            columns: sizes(&l.columns),
            rows: sizes(&l.rows),
            smallest: TextEdit::new(m.field(l.min_panel)),
            l,
            selected: Vec::new(),
            panels: m.type_names("curtainpanel").into_iter().map(String::from).collect(),
            mullions: m.type_names("mullion").into_iter().map(String::from).collect(),
            corners: m.type_names("cornermullion").into_iter().map(String::from).collect(),
            note: String::new(),
            error: None,
        };
        d.refresh(m);
        Ok(d)
    }

    fn add_column(&mut self) {
        if self.l.columns.segments.len() >= MOST {
            return;
        }
        let size = self.l.columns.segments.last().copied().unwrap_or(1000.0);
        self.l.columns.segments.push(size);
        self.columns.push(TextEdit::new(self.columns.last().map(typed).unwrap_or_default()));
        for row in &mut self.l.panels {
            row.push(row.last().cloned().flatten());
        }
        for row in self.l.vertical.iter_mut().chain(self.l.horizontal.iter_mut()) {
            row.push(row.last().cloned().flatten());
        }
    }

    fn add_row(&mut self) {
        if self.l.rows.segments.len() >= MOST {
            return;
        }
        let size = self.l.rows.segments.last().copied().unwrap_or(1000.0);
        self.l.rows.segments.push(size);
        self.rows.push(TextEdit::new(self.rows.last().map(typed).unwrap_or_default()));
        self.l.panels.push(self.l.panels.last().cloned().unwrap_or_default());
        self.l.vertical.push(self.l.vertical.last().cloned().unwrap_or_default());
        self.l.horizontal.push(self.l.horizontal.last().cloned().unwrap_or_default());
    }

    fn remove_column(&mut self) {
        if self.l.columns.segments.len() > 1 {
            self.l.columns.segments.pop();
            self.columns.pop();
            for row in &mut self.l.panels {
                row.pop();
            }
            for row in self.l.vertical.iter_mut().chain(self.l.horizontal.iter_mut()) {
                row.pop();
            }
            self.selected.clear();
        }
    }

    fn remove_row(&mut self) {
        if self.l.rows.segments.len() > 1 {
            self.l.rows.segments.pop();
            self.rows.pop();
            self.l.panels.pop();
            self.l.vertical.pop();
            self.l.horizontal.pop();
            self.selected.clear();
        }
    }

    /// What's typed put on the pattern, or what isn't a length.
    pub fn read(&mut self) -> Result<(), String> {
        let units = self.units;
        let read = |edits: &[TextEdit], sizes: &mut [f64]| -> Result<(), String> {
            for (edit, size) in edits.iter().zip(sizes.iter_mut()) {
                let text = typed(edit);
                *size = units.parse(&text).filter(|x| x.is_finite()).ok_or_else(|| tf("`{}` is not a length", &[text.trim()]))?;
            }
            Ok(())
        };
        read(&self.columns, &mut self.l.columns.segments)?;
        read(&self.rows, &mut self.l.rows.segments)?;
        let text = typed(&self.smallest);
        self.l.min_panel = units.parse(&text).filter(|x| x.is_finite()).ok_or_else(|| tf("`{}` is not a length", &[text.trim()]))?;
        Ok(())
    }

    /// What the pattern makes: the module and the thickness, from what is typed so far (a size that is none waits, and OK says so).
    fn refresh(&mut self, m: &Model) {
        let _ = self.read();
        let module = |w: &bimcraft_core::curtain::Direction| m.length(w.segments.iter().sum());
        let thick = bimcraft_core::curtain::envelope(m, &self.l).map_or(0.0, |(a, b)| a + b);
        self.note = tf("Module {} x {}, thickness {}", &[&module(&self.l.columns), &module(&self.l.rows), &m.length(thick)]);
    }

    /// What the panels picked hold, if they all hold the same; `None` in the result is their being mixed.
    fn panel_now(&self) -> Option<Option<String>> {
        let mut held = self.selected.iter().filter_map(|p| match p {
            Piece::Cell(r, c) => self.l.panels.get(*r).and_then(|x| x.get(*c)).cloned(),
            _ => None,
        });
        let first = held.next()?;
        held.all(|h| h == first).then_some(first)
    }

    fn edge(&self, p: Piece) -> Option<&Option<MullionPick>> {
        match p {
            Piece::Vertical(r, c) => self.l.vertical.get(r).and_then(|x| x.get(c)),
            Piece::Horizontal(r, c) => self.l.horizontal.get(r).and_then(|x| x.get(c)),
            Piece::Cell(..) => None,
        }
    }

    fn edge_mut(&mut self, p: Piece) -> Option<&mut Option<MullionPick>> {
        match p {
            Piece::Vertical(r, c) => self.l.vertical.get_mut(r).and_then(|x| x.get_mut(c)),
            Piece::Horizontal(r, c) => self.l.horizontal.get_mut(r).and_then(|x| x.get_mut(c)),
            Piece::Cell(..) => None,
        }
    }

    fn edges(&self) -> Vec<Piece> {
        self.selected.iter().copied().filter(|p| !matches!(p, Piece::Cell(..))).collect()
    }

    /// The border `i` of the four (left, right, bottom, top).
    fn border(&mut self, i: usize) -> &mut Option<MullionPick> {
        let b = &mut self.l.borders;
        match i {
            0 => &mut b.left,
            1 => &mut b.right,
            2 => &mut b.bottom,
            _ => &mut b.top,
        }
    }

    /// A way's segments, with its fit and justification, as a section.
    fn way(&self, columns: bool) -> Section {
        let (title, one, fit, justify, edits_at, texts) = if columns {
            (t("Vertical Grid"), t("Column"), t("Column Fit"), t("Column Justification"), COLUMNS, &self.columns)
        } else {
            (t("Horizontal Grid"), t("Row"), t("Row Fit"), t("Row Justification"), ROWS, &self.rows)
        };
        let mut rows = Vec::new();
        for (i, _) in texts.iter().enumerate() {
            let id = if columns { letter(i) } else { (i + 1).to_string() };
            rows.push(vec![
                Cell::text(format!("{one} {id}"), 110.0),
                Cell::field(edits_at + i, format!("{one} {id}"), self.units.name(), 100.0),
                Cell::text(self.units.name(), 40.0),
            ]);
        }
        let (add, remove) = if columns { (t("Add Column"), t("Remove Column")) } else { (t("Add Row"), t("Remove Row")) };
        let (add_action, remove_action) = if columns { ("add_column", "remove_column") } else { ("add_row", "remove_row") };
        rows.push(vec![Cell::button(add_action, 0, add, texts.len() < MOST), Cell::button(remove_action, 0, remove, texts.len() > 1)]);
        let (fit_at, justify_at) = if columns { (COLUMN_FIT, COLUMN_JUSTIFY) } else { (ROW_FIT, ROW_JUSTIFY) };
        rows.push(vec![Cell::text(fit, 160.0), Cell::Pick { pick: fit_at, width: 220.0 }]);
        rows.push(vec![Cell::text(justify, 160.0), Cell::Pick { pick: justify_at, width: 220.0 }]);
        Section::new(title, rows)
    }

    /// The pieces, as check boxes: each named by its column and row.
    fn pieces(&self) -> Vec<Section> {
        let (rows, columns) = (self.l.rows.segments.len(), self.l.columns.segments.len());
        let mut out = Vec::new();
        for (title, make) in [
            (t("Panels"), Piece::Cell as fn(usize, usize) -> Piece),
            (t("Vertical mullions"), Piece::Vertical as fn(usize, usize) -> Piece),
            (t("Horizontal mullions"), Piece::Horizontal as fn(usize, usize) -> Piece),
        ] {
            let table: Vec<Vec<Cell>> = (0..rows)
                .map(|r| {
                    (0..columns)
                        .map(|c| {
                            let p = make(r, c);
                            Cell::Check {
                                action: "piece",
                                arg: p.code(),
                                label: format!("{}{}", letter(c), r + 1),
                                on: self.selected.contains(&p),
                                enabled: true,
                            }
                        })
                        .collect()
                })
                .collect();
            out.push(Section::new(title, table));
        }
        out
    }

    /// The pattern on the type through the `type` command.
    fn apply(&mut self, host: &mut dyn Host) -> bool {
        if let Err(why) = self.read() {
            self.error = Some(why);
            return false;
        }
        match host.execute("type", &json!({"category": "wall", "name": self.name, "curtain": self.l})) {
            Ok(_) => {
                self.error = None;
                self.refresh(host.model());
                true
            }
            Err(why) => {
                self.error = Some(why);
                false
            }
        }
    }
}

/// A list of `names` with none first, and the place of `now` in it.
fn with_none(names: &[String], now: Option<&str>) -> (Vec<String>, Option<usize>) {
    let options: Vec<String> = std::iter::once(t("None").to_string()).chain(names.iter().cloned()).collect();
    let current = match now {
        None => Some(0),
        Some(n) => names.iter().position(|x| x == n).map(|i| i + 1),
    };
    (options, current)
}

impl Discipline for Pattern {
    fn title(&self) -> String {
        t("Edit Pattern").to_string()
    }

    fn about(&self) -> String {
        tf("The pattern of the curtain wall type {}.", &[&self.name])
    }

    fn width(&self) -> f32 {
        760.0
    }

    fn sections(&self) -> Vec<Section> {
        let mut out = vec![self.way(true), self.way(false)];
        let borders = ["Border Left", "Border Right", "Border Bottom", "Border Top"];
        let mut rows: Vec<Vec<Cell>> =
            borders.iter().enumerate().map(|(i, b)| vec![Cell::text(t(b), 160.0), Cell::Pick { pick: BORDER + i, width: 240.0 }]).collect();
        rows.push(vec![Cell::text(t("Corner Mullion"), 160.0), Cell::Pick { pick: CORNER, width: 240.0 }]);
        out.push(Section::new(t("Borders"), rows));
        out.push(Section::new(
            "",
            vec![
                vec![Cell::text(t("Of One Size, Runs Through"), 200.0), Cell::Pick { pick: CONTINUOUS, width: 220.0 }],
                vec![Cell::Check {
                    action: "anchor",
                    arg: 0,
                    label: t("Restart pattern at levels crossing").to_string(),
                    on: self.l.anchor_rows,
                    enabled: true,
                }],
                vec![
                    Cell::text(t("Smallest Panel"), 200.0),
                    Cell::field(SMALLEST, t("Smallest Panel"), self.units.name(), 100.0),
                    Cell::text(self.units.name(), 40.0),
                ],
            ],
        ));
        out.push(Section::new(self.note.clone(), Vec::new()));
        out.extend(self.pieces());
        let mut picked = Vec::new();
        if self.selected.is_empty() {
            picked.push(vec![Cell::text(t("Click a panel or a mullion to set its type: tick the pieces above."), 520.0)]);
        }
        if self.selected.iter().any(|p| matches!(p, Piece::Cell(..))) {
            picked.push(vec![Cell::text(t("Panel Type"), 160.0), Cell::Pick { pick: PANEL, width: 240.0 }]);
        }
        if !self.edges().is_empty() {
            picked.push(vec![Cell::text(t("Mullion Type"), 160.0), Cell::Pick { pick: MULLION, width: 240.0 }]);
            picked.push(vec![Cell::text(t("Join"), 160.0), Cell::Pick { pick: JOIN, width: 240.0 }]);
        }
        out.push(Section::new(t("What is ticked"), picked));
        out
    }

    fn edit(&self, i: usize) -> Option<&TextEdit> {
        match i {
            SMALLEST => Some(&self.smallest),
            i if i >= ROWS => self.rows.get(i - ROWS),
            i => self.columns.get(i - COLUMNS),
        }
    }

    fn edit_mut(&mut self, i: usize) -> Option<&mut TextEdit> {
        match i {
            SMALLEST => Some(&mut self.smallest),
            i if i >= ROWS => self.rows.get_mut(i - ROWS),
            i => self.columns.get_mut(i - COLUMNS),
        }
    }

    fn choice(&self, pick: usize) -> Option<Choice> {
        let one = |name: &str, options: Vec<String>, current: Option<usize>| Some(Choice { name: t(name).to_string(), options, current });
        let l = &self.l;
        match pick {
            COLUMN_FIT => one("Column Fit", FITS.iter().map(|(_, s)| t(s).to_string()).collect(), place(&FITS, l.columns.fit)),
            COLUMN_JUSTIFY => {
                one("Column Justification", JUSTIFIES.iter().map(|(_, s)| t(s).to_string()).collect(), place(&JUSTIFIES, l.columns.justify))
            }
            ROW_FIT => one("Row Fit", FITS.iter().map(|(_, s)| t(s).to_string()).collect(), place(&FITS, l.rows.fit)),
            ROW_JUSTIFY => {
                one("Row Justification", JUSTIFIES.iter().map(|(_, s)| t(s).to_string()).collect(), place(&JUSTIFIES, l.rows.justify))
            }
            CONTINUOUS => one(
                "Of One Size, Runs Through",
                CONTINUOUSES.iter().map(|(_, s)| t(s).to_string()).collect(),
                place(&CONTINUOUSES, l.continuous),
            ),
            p if (BORDER..BORDER + 4).contains(&p) => {
                let b = &l.borders;
                let now = [&b.left, &b.right, &b.bottom, &b.top][p - BORDER].as_ref().map(|q| q.type_name.as_str());
                let (options, current) = with_none(&self.mullions, now);
                let name = ["Border Left", "Border Right", "Border Bottom", "Border Top"][p - BORDER];
                one(name, options, current)
            }
            CORNER => {
                let (options, current) = with_none(&self.corners, l.corner.as_deref());
                one("Corner Mullion", options, current)
            }
            PANEL => {
                // Panels that hold different types have none picked.
                let (options, current) = match self.panel_now() {
                    Some(now) => with_none(&self.panels, now.as_deref()),
                    None => (with_none(&self.panels, None).0, None),
                };
                one("Panel Type", options, current)
            }
            MULLION => {
                let held: Vec<Option<MullionPick>> = self.edges().iter().filter_map(|p| self.edge(*p).cloned()).collect();
                let same = held.first().filter(|f| held.iter().all(|h| h == *f));
                let (options, current) = with_none(&self.mullions, same.and_then(|h| h.as_ref().map(|q| q.type_name.as_str())));
                one("Mullion Type", options, same.and(current))
            }
            JOIN => {
                let join = self.edges().iter().find_map(|p| self.edge(*p).cloned().flatten()).map_or(Join::Butt, |q| q.join);
                one("Join", JOINS.iter().map(|(_, s)| t(s).to_string()).collect(), place(&JOINS, join))
            }
            _ => None,
        }
    }

    fn choose(&mut self, pick: usize, option: usize, host: &mut dyn Host) {
        let named = |names: &[String]| option.checked_sub(1).and_then(|i| names.get(i)).cloned();
        match pick {
            COLUMN_FIT => self.l.columns.fit = FITS.get(option).map_or(self.l.columns.fit, |(v, _)| *v),
            COLUMN_JUSTIFY => self.l.columns.justify = JUSTIFIES.get(option).map_or(self.l.columns.justify, |(v, _)| *v),
            ROW_FIT => self.l.rows.fit = FITS.get(option).map_or(self.l.rows.fit, |(v, _)| *v),
            ROW_JUSTIFY => self.l.rows.justify = JUSTIFIES.get(option).map_or(self.l.rows.justify, |(v, _)| *v),
            CONTINUOUS => self.l.continuous = CONTINUOUSES.get(option).map_or(self.l.continuous, |(v, _)| *v),
            p if (BORDER..BORDER + 4).contains(&p) => {
                let name = named(&self.mullions);
                let slot = self.border(p - BORDER);
                let join = slot.as_ref().map_or(Join::Butt, |q| q.join);
                *slot = name.map(|type_name| MullionPick { type_name, join });
            }
            CORNER => self.l.corner = named(&self.corners),
            PANEL => {
                let value = named(&self.panels);
                for p in self.selected.clone() {
                    if let Piece::Cell(r, c) = p
                        && let Some(cell) = self.l.panels.get_mut(r).and_then(|x| x.get_mut(c))
                    {
                        cell.clone_from(&value);
                    }
                }
            }
            MULLION => {
                let name = named(&self.mullions);
                for p in self.edges() {
                    if let Some(e) = self.edge_mut(p) {
                        let join = e.as_ref().map_or(Join::Butt, |q| q.join);
                        *e = name.clone().map(|type_name| MullionPick { type_name, join });
                    }
                }
            }
            JOIN => {
                let join = JOINS.get(option).map_or(Join::Butt, |(v, _)| *v);
                for p in self.edges() {
                    if let Some(Some(q)) = self.edge_mut(p) {
                        q.join = join;
                    }
                }
            }
            _ => {}
        }
        self.refresh(host.model());
    }

    fn press(&mut self, action: &str, arg: usize, host: &mut dyn Host) -> Outcome {
        match action {
            "add_column" => self.add_column(),
            "remove_column" => self.remove_column(),
            "add_row" => self.add_row(),
            "remove_row" => self.remove_row(),
            "anchor" => self.l.anchor_rows = !self.l.anchor_rows,
            "piece" => {
                let p = Piece::of(arg);
                match self.selected.iter().position(|q| *q == p) {
                    Some(i) => {
                        self.selected.remove(i);
                    }
                    None => self.selected.push(p),
                }
            }
            "apply" => {
                self.apply(host);
            }
            "ok" => {
                if self.apply(host) {
                    return Outcome::Close;
                }
            }
            "cancel" => return Outcome::Close,
            _ => {}
        }
        self.refresh(host.model());
        Outcome::Keep
    }

    fn foot(&self) -> Vec<Foot> {
        vec![Foot::new("cancel", t("Cancel"), false), Foot::new("apply", t("Apply"), false), Foot::new("ok", t("OK"), true)]
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests;
