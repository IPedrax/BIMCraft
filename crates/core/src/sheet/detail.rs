//! A rebar detail on a sheet: a beam's or a column's elevation, or a footing's or a floor's plan, with every set of bars drawn and
//! tagged and its lengths dimensioned, a section at each station at twice the scale (a footing's, one along its first axis), and the
//! steel list with each bar's shape sketched, as one drawing from its top left corner. Everything is laid out as a display list ([`Op`]) first, in pixels, so the frame it needs and what it draws are one
//! computation and a test can read it; [`draw`] puts that list on a sheet's drawing.
//!
//! The picture is the member's and its bars' as they are now. It is not [`crate::render::section_in`]: a section of the model hides a
//! bar beyond its cut under the concrete, and a detail doesn't.

use std::collections::HashSet;

use super::{CELL_TEXT, INK, ROW};
use crate::font;
use crate::model::{Element, Id, Model, Rebar};
use crate::pen::Pen;
use crate::rebar_detail::{self, MAX_STATIONS, RebarDetail, Set, in_frame, sketch, square, tag_text};
use crate::reinforce::{Frame, Kind, Located};
use crate::render::{Drawing, Prim};
use crate::steel::{self, Bars, Scope};
use crate::view3d::P3;

/// The most lines a detail draws; past it the drawing says so and leaves the rest out.
pub const MAX_DRAWN: usize = 50_000;
/// A tag's lettering and a small one's, px: 2 mm and 1.6 mm on paper.
const TAG: f32 = 10.0;
const SMALL: f32 = 8.0;
/// Room between the picture's blocks, between a dimension's rows, and a tag's row, px.
const GAP: f32 = 60.0;
const DIM: f32 = 26.0;
const TAG_ROW: f32 = 18.0;
/// Room round the whole drawing, px.
const MARGIN: f32 = 30.0;
/// A steel list row that holds a bar's sketch, px, and the width of that sketch's cell.
const SHAPE_ROW: f32 = 64.0;
const SHAPE_COLUMN: f32 = 190.0;
/// A bar's dot in a section is never smaller than this radius, px (0.4 mm on paper), so that it reads.
const MIN_DOT: f64 = 2.0;

/// One thing a detail draws, in pixels from its own origin.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    Line {
        pts: Vec<(f32, f32)>,
        pen: Pen,
        dashed: bool,
        closed: bool,
    },
    /// A filled shape: a bar cut square.
    Dot {
        ring: Vec<(f32, f32)>,
    },
    /// Lettering centred on `at`.
    Text {
        text: String,
        at: (f32, f32),
        height: f32,
    },
}

/// A laid out detail: what it draws, and the room it takes, px.
pub(crate) struct Layout {
    pub(crate) ops: Vec<Op>,
    pub(crate) width: f32,
    pub(crate) height: f32,
    /// What it could not draw as asked, in English.
    pub(crate) notes: Vec<String>,
}

#[derive(Default)]
struct Ink {
    ops: Vec<Op>,
    notes: Vec<String>,
    drawn: usize,
}

impl Ink {
    fn push(&mut self, op: Op) {
        self.drawn += 1;
        if self.drawn <= MAX_DRAWN {
            self.ops.push(op);
        } else if self.drawn == MAX_DRAWN + 1 {
            self.notes.push(format!("A rebar detail draws no more than {MAX_DRAWN} lines; the rest are left out"));
        }
    }

    fn full(&self) -> bool {
        self.drawn >= MAX_DRAWN
    }

    fn line(&mut self, pts: Vec<(f32, f32)>, pen: Pen, closed: bool) {
        self.push(Op::Line { pts, pen, dashed: false, closed });
    }

    fn centred(&mut self, text: &str, at: (f32, f32), height: f32) {
        if !text.is_empty() {
            self.push(Op::Text { text: text.to_string(), at, height });
        }
    }

    /// `text` with its left edge at `left`.
    fn word(&mut self, text: &str, left: f32, y: f32, height: f32) {
        self.centred(text, (left + font::width(text, height) / 2.0, y), height);
    }

    fn rect(&mut self, (x1, y1): (f32, f32), (x2, y2): (f32, f32), pen: Pen) {
        self.line(vec![(x1, y1), (x2, y1), (x2, y2), (x1, y2)], pen, true);
    }

    /// A dimension line from `x1` to `x2` at `y`, a tick at each end and its text over it.
    fn dimension(&mut self, x1: f32, x2: f32, y: f32, text: &str) {
        self.line(vec![(x1, y), (x2, y)], Pen::PROJECTION, false);
        for x in [x1, x2] {
            self.line(vec![(x, y - 5.0), (x, y + 5.0)], Pen::HEAVY, false);
        }
        self.centred(text, ((x1 + x2) / 2.0, y - 9.0), TAG);
    }

    /// A dimension line from `y1` to `y2` at `x`, a tick at each end and its text to the left of it.
    fn dimension_v(&mut self, y1: f32, y2: f32, x: f32, text: &str) {
        self.line(vec![(x, y1), (x, y2)], Pen::PROJECTION, false);
        for y in [y1, y2] {
            self.line(vec![(x - 5.0, y), (x + 5.0, y)], Pen::HEAVY, false);
        }
        self.centred(text, (x - 9.0 - font::width(text, TAG) / 2.0, (y1 + y2) / 2.0), TAG);
    }

    /// A filled circle of radius `r` at `c`.
    fn dot(&mut self, c: (f32, f32), r: f32) {
        self.push(Op::Dot { ring: circle(c, r, 12) });
    }

    /// A bar's line, or its dot where it is seen end on: all its points in one place, `d` px across.
    fn rod(&mut self, pts: &[(f32, f32)], pen: Pen, d: f64) {
        match pts.first() {
            Some(p) if pts.iter().all(|q| (q.0 - p.0).abs() < 0.5 && (q.1 - p.1).abs() < 0.5) => {
                self.dot(*p, (d / 2.0).max(MIN_DOT) as f32)
            }
            _ => self.line(pts.to_vec(), pen, false),
        }
    }

    /// `tags`, each an anchor on the drawing and what it says, in a column at `x` from `top` down, with a leader from each to its
    /// anchor. Ordered by where the anchors are, top to bottom, so no two tags share a row and none can overlap.
    fn column(&mut self, mut tags: Vec<((f32, f32), String)>, x: f32, top: f32) {
        tags.sort_by(|a, b| a.0.1.total_cmp(&b.0.1).then(a.0.0.total_cmp(&b.0.0)));
        for (i, (anchor, text)) in tags.into_iter().enumerate() {
            let y = top + TAG_ROW * i as f32;
            self.line(vec![anchor, (x - 4.0, y)], Pen::FINE, false);
            self.word(&text, x, y, TAG);
        }
    }
}

/// The corners of a regular polygon of `n` sides round `c`, `r` from it.
fn circle(c: (f32, f32), r: f32, n: usize) -> Vec<(f32, f32)> {
    (0..n)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / n as f32;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// How wide the widest of `tags` is, px.
fn widest(tags: &[((f32, f32), String)]) -> f32 {
    tags.iter().map(|(_, t)| font::width(t, TAG)).fold(0.0, f32::max)
}

fn bar(m: &Model, id: Id) -> Option<&Rebar> {
    match m.elements.get(id) {
        Some(Element::Rebar(rb)) => Some(rb),
        _ => None,
    }
}

/// Where the bar along `path` crosses the plane `s` mm along the member: how far across and up it is there.
fn crossing(f: &Frame, path: &[P3], s: f64) -> Option<[f64; 2]> {
    path.windows(2).find_map(|w| match w {
        [a, b] => {
            let (pa, pb) = (in_frame(f, *a), in_frame(f, *b));
            let (da, db) = (pa[0] - s, pb[0] - s);
            if da * db > 0.0 || (pb[0] - pa[0]).abs() < 1e-9 {
                return None;
            }
            let t = -da / (db - da);
            Some([pa[1] + t * (pb[1] - pa[1]), pa[2] + t * (pb[2] - pa[2])])
        }
        _ => None,
    })
}

/// A line's anchor for a tag: a stirrup's top, otherwise the middle of its longest piece.
fn anchor_of(pts: &[(f32, f32)], stirrup: bool) -> (f32, f32) {
    if stirrup {
        return pts.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or_default();
    }
    let len = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0).hypot(b.1 - a.1);
    pts.windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .max_by(|(a, b), (c, d)| len(*a, *b).total_cmp(&len(*c, *d)))
        .map_or_else(|| pts.first().copied().unwrap_or_default(), |(a, b)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
}

/// The anchor of a tag on a stirrup seen across a column: its point furthest right, nearest the column of tags.
fn anchor_right(pts: &[(f32, f32)]) -> (f32, f32) {
    pts.iter().copied().max_by(|a, b| a.0.total_cmp(&b.0)).unwrap_or_default()
}

/// The letters of sections, A to Z.
fn letter(i: usize) -> char {
    char::from(b'A' + u8::try_from(i % 26).unwrap_or(0))
}

/// The steel list at `(0, top)`: a ruled table of the member's rows, each bar's shape sketched, and its total weight. Returns the
/// y it ends at.
fn list(ink: &mut Ink, m: &Model, bars: &Bars, host: Id, top: f32) -> f32 {
    let scope = Scope { level: None, only: Some([host].into_iter().collect()) };
    let items = steel::items(m, bars, &scope);
    let rows = match steel::rows(m, &items) {
        Ok(r) => r,
        Err(why) => {
            ink.notes.push(format!("The steel list is left out: {why}"));
            return top;
        }
    };
    if rows.is_empty() {
        return top;
    }
    let w = |name: &str| m.language.word(name).to_string();
    let heads = [w("Mark"), w("Shape"), w("Diameter"), w("Count"), w("Length"), w("Total Length"), w("Weight")];
    let cols = [70.0f32, SHAPE_COLUMN, 70.0, 70.0, 90.0, 100.0, 90.0];
    // Where each column starts, and the table's width at the end.
    let mut x: Vec<f32> = vec![0.0];
    for c in cols {
        x.push(x.last().copied().unwrap_or(0.0) + c);
    }
    let width = x.last().copied().unwrap_or(0.0);
    let height = ROW + SHAPE_ROW * rows.len() as f32 + ROW;
    ink.rect((0.0, top), (width, top + height), Pen::HEAVY);
    for (i, head) in heads.iter().enumerate() {
        let (l, r) = (x.get(i).copied().unwrap_or(0.0), x.get(i + 1).copied().unwrap_or(width));
        ink.centred(head, ((l + r) / 2.0, top + ROW / 2.0), CELL_TEXT);
        if i > 0 {
            ink.line(vec![(l, top), (l, top + height)], Pen::FINE, false);
        }
    }
    ink.line(vec![(0.0, top + ROW), (width, top + ROW)], Pen::MEDIUM, false);
    let mut total = 0.0;
    for (n, row) in rows.iter().enumerate() {
        let y = top + ROW + SHAPE_ROW * n as f32;
        ink.line(vec![(0.0, y + SHAPE_ROW), (width, y + SHAPE_ROW)], Pen::FINE, false);
        let mid = y + SHAPE_ROW / 2.0;
        let cells = [
            Some(row.mark.clone()),
            None,
            Some(row.diameter.to_string()),
            Some(row.count.to_string()),
            Some(m.length(row.length)),
            Some(m.length(row.total_length)),
            Some(format!("{:.2}", row.weight_kg)),
        ];
        for (i, cell) in cells.iter().enumerate() {
            let (l, r) = (x.get(i).copied().unwrap_or(0.0), x.get(i + 1).copied().unwrap_or(width));
            if let Some(text) = cell {
                ink.centred(text, ((l + r) / 2.0, mid), CELL_TEXT);
            }
        }
        if let Some(path) = row.bars.first().and_then(|b| bar(m, *b)).map(|rb| rb.path.as_slice()) {
            shape(ink, m, path, (x.get(1).copied().unwrap_or(0.0), y, SHAPE_COLUMN, SHAPE_ROW));
        }
        total += row.weight_kg;
    }
    let y = top + ROW + SHAPE_ROW * rows.len() as f32;
    ink.centred(&w("Total"), (35.0, y + ROW / 2.0), CELL_TEXT);
    let last = x.get(6).copied().unwrap_or(0.0);
    ink.centred(&format!("{total:.2}"), (last + 45.0, y + ROW / 2.0), CELL_TEXT);
    top + height
}

/// A bar's sketch in the cell `(left, top, width, height)`: its unfolded legs scaled to fit, each lettered with its length, and each
/// bend that is not a right angle with its angle.
fn shape(ink: &mut Ink, m: &Model, path: &[P3], (left, top, w, h): (f32, f32, f32, f32)) {
    let sk = sketch(path);
    let (Some(first), true) = (sk.points.first().copied(), sk.points.len() >= 2) else { return };
    let (mut lo, mut hi) = (first, first);
    for p in &sk.points {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let (room_w, room_h) = (f64::from(w) - 40.0, f64::from(h) - 28.0);
    let s = (room_w / (hi[0] - lo[0]).max(1.0)).min(room_h / (hi[1] - lo[1]).max(1.0));
    let at = |p: &[f64; 2]| -> (f32, f32) {
        let (cx, cy) = (f64::from(left) + f64::from(w) / 2.0, f64::from(top) + f64::from(h) / 2.0);
        ((cx + (p[0] - (lo[0] + hi[0]) / 2.0) * s) as f32, (cy - (p[1] - (lo[1] + hi[1]) / 2.0) * s) as f32)
    };
    let pts: Vec<(f32, f32)> = sk.points.iter().map(at).collect();
    ink.line(pts.clone(), Pen::MEDIUM, false);
    let turns = steel::shape(path).turns;
    for (i, pair) in sk.points.windows(2).enumerate().take(steel::MAX_LABEL_LEGS) {
        if let ([a, b], Some(pa), Some(pb)) = (pair, pts.get(i), pts.get(i + 1)) {
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            ink.centred(&m.length(len), ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0 - 7.0), SMALL);
        }
        if let (Some(turn), Some(p)) = (turns.get(i), pts.get(i + 1))
            && (turn - 90.0).abs() > 1.0
            && *turn > 1.0
        {
            ink.centred(&format!("{}°", turn.round()), (p.0 + 14.0, p.1 + 8.0), SMALL);
        }
    }
    if sk.spatial {
        ink.centred(m.language.word("spatial"), (left + w / 2.0, top + h - 6.0), SMALL);
    }
}

/// Where a detail's lines and lettering stand, with `k` px to a millimetre of the member.
pub(crate) fn layout(m: &Model, rd: &RebarDetail, k: f64) -> Option<Layout> {
    let host = m.elements.id(rd.host)?;
    let b = rebar_detail::host_frame(m, rd)?;
    let bars = Bars::new(m);
    let sets = rebar_detail::sets(m, &bars, host);
    if sets.is_empty() {
        return None;
    }
    let mut ink = Ink::default();
    let under = match b.kind {
        Kind::Beam | Kind::Column => member(&mut ink, m, rd, &b, &sets, k),
        Kind::Footing => footing(&mut ink, m, &b, &sets, k),
        Kind::Floor => plan(&mut ink, m, &b, &sets, k)[3] + 40.0,
    };
    // Its steel list.
    if rd.list {
        list(&mut ink, m, &bars, host, under + GAP);
    }
    Some(frame(ink))
}

/// A beam's or a column's elevation with its tags, stations and dimensions, and its sections at twice the scale side by side under
/// it. A beam lies along x with its depth up; a column stands, its length along y and its width across. Returns where it ends, y.
fn member(ink: &mut Ink, m: &Model, rd: &RebarDetail, b: &Located, sets: &[Set], k: f64) -> f32 {
    let f = &b.frame;
    let (length, depth) = (b.length, b.depth);
    let tall = b.kind == Kind::Column;
    // A point of the member, in its own axes, as the elevation draws it, and where `u` mm along it is.
    let elev =
        |q: P3| -> (f32, f32) { if tall { ((q[1] * k) as f32, (-q[0] * k) as f32) } else { ((q[0] * k) as f32, (-q[2] * k) as f32) } };
    let pos = |u: f64| -> f32 { if tall { (-u * k) as f32 } else { (u * k) as f32 } };

    // The elevation: the concrete's outline, then each set's bars.
    let (c0, c1) = (elev(b.lo), elev([b.lo[0] + length, b.lo[1] + b.width, b.lo[2] + depth]));
    let (ex0, ex1) = (c0.0.min(c1.0), c0.0.max(c1.0));
    let (ey0, ey1) = (c0.1.min(c1.1), c0.1.max(c1.1));
    ink.rect(c0, c1, Pen::PROJECTION);
    let mut seen: HashSet<Vec<(i64, i64)>> = HashSet::new();
    let mut tags: Vec<((f32, f32), String)> = Vec::new();
    for set in sets {
        let lines: Vec<Vec<(f32, f32)>> =
            set.bars.iter().filter_map(|id| bar(m, *id)).map(|rb| rb.path.iter().map(|p| elev(in_frame(f, *p))).collect()).collect();
        let stirrup = set.bars.iter().filter_map(|id| bar(m, *id)).all(|rb| square(f, &rb.path));
        // A zone of stirrups is its first and its last with a thin line between, not every one.
        let drawn: Vec<&Vec<(f32, f32)>> =
            if stirrup && lines.len() >= 3 { lines.first().into_iter().chain(lines.last()).collect() } else { lines.iter().collect() };
        for pts in &drawn {
            let key: Vec<(i64, i64)> = pts.iter().map(|p| (p.0.round() as i64, p.1.round() as i64)).collect();
            if !ink.full() && seen.insert(key) {
                ink.rod(pts, if stirrup { Pen::MEDIUM } else { Pen::HEAVY }, set.diameter * k);
            }
        }
        if stirrup
            && lines.len() >= 3
            && let (Some(a), Some(z)) = (lines.first(), lines.last())
        {
            if tall {
                let left = |l: &Vec<(f32, f32)>| l.iter().map(|p| p.0).fold(f32::MAX, f32::min);
                let x = (left(a) + left(z)) / 2.0 + (ex1 - ex0) / 4.0;
                let y = |l: &Vec<(f32, f32)>| l.first().map_or(0.0, |p| p.1);
                ink.line(vec![(x, y(a)), (x, y(z))], Pen::FINE, false);
            } else {
                let (ya, yz) = (anchor_of(a, true).1, anchor_of(z, true).1);
                let mid = (ya + yz) / 2.0 + (ey1 - ey0) / 4.0;
                let (xa, xz) = (a.first().map_or(0.0, |p| p.0), z.first().map_or(0.0, |p| p.0));
                ink.line(vec![(xa, mid), (xz, mid)], Pen::FINE, false);
            }
        }
        if let Some(first) = lines.first() {
            let anchor = if tall && stirrup { anchor_right(first) } else { anchor_of(first, stirrup) };
            tags.push((anchor, tag_text(m, set, rebar_detail::spreads(set, f))));
        }
    }
    ink.column(tags, ex1 + 40.0, ey0);

    // The stations of its sections, marked on the elevation.
    let stations: Vec<f64> = if rd.stations.is_empty() { rebar_detail::default_stations(m, sets, b) } else { rd.stations.clone() }
        .into_iter()
        .filter(|s| (0.0..=length).contains(s))
        .take(MAX_STATIONS)
        .collect();
    for (i, s) in stations.iter().enumerate() {
        let p = pos(*s);
        if tall {
            ink.line(vec![(ex0 - 12.0, p), (ex1 + 12.0, p)], Pen::MEDIUM, false);
            ink.centred(&letter(i).to_string(), (ex0 - 22.0, p), TAG);
        } else {
            ink.line(vec![(p, ey0 - 12.0), (p, ey1 + 12.0)], Pen::MEDIUM, false);
            ink.centred(&letter(i).to_string(), (p, ey0 - 22.0), TAG);
        }
    }

    // Its dimensions: the length, and each zone of stirrups. A beam's are in rows under it, a column's in columns to its left.
    let zones = sets.iter().filter(|s| s.layer == "stirrup" && s.count >= 2).filter_map(|set| {
        let us: Vec<f64> =
            set.bars.iter().filter_map(|id| bar(m, *id)).filter_map(|rb| rb.path.first()).map(|p| in_frame(f, *p)[0]).collect();
        Some((us.iter().copied().reduce(f64::min)?, us.iter().copied().reduce(f64::max)?))
    });
    let ends = if tall {
        ink.dimension_v(pos(length), pos(0.0), ex0 - 90.0, &m.length(length));
        for (lo, hi) in zones {
            ink.dimension_v(pos(hi), pos(lo), ex0 - 40.0, &m.length(hi - lo));
        }
        ey1 + 12.0
    } else {
        let row = ey1 + 30.0;
        ink.dimension(0.0, ex1, row, &m.length(length));
        for (lo, hi) in zones {
            ink.dimension(pos(lo), pos(hi), row + DIM, &m.length(hi - lo));
        }
        row + DIM
    };

    // Its sections, at twice the scale, side by side under it.
    let kk = 2.0 * k;
    let scale = crate::sheet::DENSITY / kk;
    let (sw, sh) = ((b.width * kk) as f32, (depth * kk) as f32);
    let y0 = ends + GAP;
    let (mut x0, mut under) = (0.0f32, y0 + sh);
    for (i, s) in stations.iter().enumerate() {
        let (cx, cy) = (x0 + sw / 2.0, y0 + sh / 2.0);
        let at = |w: f64, v: f64| -> (f32, f32) { (cx + (w * kk) as f32, cy - (v * kk) as f32) };
        if b.round {
            ink.line(circle((cx, cy), sw / 2.0, 48), Pen::PROJECTION, true);
        } else {
            ink.rect((cx - sw / 2.0, cy - sh / 2.0), (cx + sw / 2.0, cy + sh / 2.0), Pen::PROJECTION);
        }
        let mut tags: Vec<((f32, f32), String)> = Vec::new();
        for set in sets {
            let bars: Vec<&Rebar> = set.bars.iter().filter_map(|id| bar(m, *id)).collect();
            if bars.iter().all(|rb| square(f, &rb.path)) {
                // The stirrup of the zone this station is in: the nearest of the set, if the set reaches it.
                let along = |rb: &Rebar| rb.path.first().map_or(0.0, |p| in_frame(f, *p)[0]);
                let near = bars.iter().copied().min_by(|a, c| (along(a) - *s).abs().total_cmp(&(along(c) - *s).abs()));
                let (lo, hi) = bars.iter().map(|r| along(r)).fold((f64::MAX, f64::MIN), |(l, h), u| (l.min(u), h.max(u)));
                let reach = set.spacing.unwrap_or(100.0) / 2.0;
                if let Some(rb) = near.filter(|_| *s >= lo - reach && *s <= hi + reach) {
                    let pts: Vec<(f32, f32)> = rb.path.iter().map(|p| in_frame(f, *p)).map(|q| at(q[1], q[2])).collect();
                    ink.rod(&pts, Pen::MEDIUM, set.diameter * kk);
                    tags.push((anchor_of(&pts, true), tag_text(m, set, false)));
                }
            } else {
                let spots: Vec<[f64; 2]> = bars.iter().filter_map(|rb| crossing(f, &rb.path, *s)).collect();
                for q in &spots {
                    ink.dot(at(q[0], q[1]), (set.diameter * kk / 2.0).max(MIN_DOT) as f32);
                }
                if let Some(q) = spots.first() {
                    tags.push((at(q[0], q[1]), tag_text(m, set, false)));
                }
            }
        }
        let column_x = cx + sw / 2.0 + 30.0;
        let wide = widest(&tags);
        ink.column(tags, column_x, cy - sh / 2.0);
        let across = if b.round { format!("ø{}", m.length(b.width)) } else { m.length(b.width) };
        ink.dimension(cx - sw / 2.0, cx + sw / 2.0, cy + sh / 2.0 + 22.0, &across);
        ink.centred(&format!("{0}-{0}   {1}", letter(i), m.units.scale(scale)), (cx, cy + sh / 2.0 + 48.0), TAG);
        under = under.max(cy + sh / 2.0 + 56.0);
        x0 = column_x + wide + GAP;
    }
    under
}

/// The plan of a footing or a floor, its width along x and its depth up: the outline (a floor's, its openings too), every set of bars
/// with its tag (these bars spread across the plan, so a tag says its spacing) and the dimensions. Returns the outline's box, px:
/// left, top, right, bottom.
fn plan(ink: &mut Ink, m: &Model, b: &Located, sets: &[Set], k: f64) -> [f32; 4] {
    let f = &b.frame;
    let at = |q: P3| -> (f32, f32) { ((q[0] * k) as f32, (-q[1] * k) as f32) };
    let (c0, c1) = (at(b.lo), at([b.lo[0] + b.length, b.lo[1] + b.width, 0.0]));
    let (x0, x1) = (c0.0.min(c1.0), c0.0.max(c1.0));
    let (y0, y1) = (c0.1.min(c1.1), c0.1.max(c1.1));
    if b.kind == Kind::Floor {
        for ring in &b.rings {
            ink.line(ring.iter().map(|p| at([p.0, p.1, 0.0])).collect(), Pen::PROJECTION, true);
        }
    } else if b.round {
        ink.line(circle(((x0 + x1) / 2.0, (y0 + y1) / 2.0), (x1 - x0) / 2.0, 48), Pen::PROJECTION, true);
    } else {
        ink.rect(c0, c1, Pen::PROJECTION);
    }
    let mut seen: HashSet<Vec<(i64, i64)>> = HashSet::new();
    let mut tags: Vec<((f32, f32), String)> = Vec::new();
    for set in sets {
        let lines: Vec<Vec<(f32, f32)>> =
            set.bars.iter().filter_map(|id| bar(m, *id)).map(|rb| rb.path.iter().map(|p| at(in_frame(f, *p))).collect()).collect();
        let spread = rebar_detail::spreads_over(set, [f.a, f.b]);
        // A set that spreads over the plan is its first bar and its last with a thin line between, not every one.
        let many = spread && lines.len() >= 3;
        let drawn: Vec<&Vec<(f32, f32)>> =
            if many { lines.first().into_iter().chain(lines.last()).collect() } else { lines.iter().collect() };
        for pts in &drawn {
            let key: Vec<(i64, i64)> = pts.iter().map(|p| (p.0.round() as i64, p.1.round() as i64)).collect();
            if !ink.full() && seen.insert(key) {
                ink.rod(pts, Pen::HEAVY, set.diameter * k);
            }
        }
        if many && let (Some(a), Some(z)) = (lines.first(), lines.last()) {
            ink.line(vec![anchor_of(a, false), anchor_of(z, false)], Pen::FINE, false);
        }
        if let Some(first) = lines.first() {
            tags.push((anchor_of(first, false), tag_text(m, set, spread)));
        }
    }
    ink.column(tags, x1 + 60.0, y0);
    let across = if b.round { format!("ø{}", m.length(b.length)) } else { m.length(b.length) };
    ink.dimension(x0, x1, y1 + 30.0, &across);
    if !b.round {
        ink.dimension_v(y0, y1, x0 - 50.0, &m.length(b.width));
    }
    [x0, y0, x1, y1]
}

/// A footing's plan, and under it, lined up with it, its section along its first axis through the middle: the bars as they lie, the
/// layers of the mesh one over the other, and its thickness. Returns where it ends, y.
fn footing(ink: &mut Ink, m: &Model, b: &Located, sets: &[Set], k: f64) -> f32 {
    let f = &b.frame;
    let [x0, y0, x1, y1] = plan(ink, m, b, sets, k);
    let ymid = (y0 + y1) / 2.0;
    ink.line(vec![(x0 - 12.0, ymid), (x1 + 12.0, ymid)], Pen::MEDIUM, false);
    ink.centred("A", (x0 - 24.0, ymid), TAG);
    ink.centred("A", (x1 + 24.0, ymid), TAG);
    let t = b.depth;
    let base = y1 + 30.0 + DIM + GAP + (t * k) as f32;
    let sec = |q: P3| -> (f32, f32) { ((q[0] * k) as f32, base - (q[2] * k) as f32) };
    ink.rect(sec([b.lo[0], 0.0, 0.0]), sec([b.lo[0] + b.length, 0.0, t]), Pen::PROJECTION);
    let mut seen: HashSet<Vec<(i64, i64)>> = HashSet::new();
    for set in sets {
        for rb in set.bars.iter().filter_map(|id| bar(m, *id)) {
            let pts: Vec<(f32, f32)> = rb.path.iter().map(|p| sec(in_frame(f, *p))).collect();
            let key: Vec<(i64, i64)> = pts.iter().map(|p| (p.0.round() as i64, p.1.round() as i64)).collect();
            if !ink.full() && seen.insert(key) {
                ink.rod(&pts, Pen::HEAVY, set.diameter * k);
            }
        }
    }
    ink.dimension_v(base - (t * k) as f32, base, x0 - 50.0, &m.length(t));
    ink.centred(&format!("A-A   {}", m.units.scale(crate::sheet::DENSITY / k)), ((x0 + x1) / 2.0, base + 22.0), TAG);
    base + 30.0
}

/// `ink` moved so that what it draws starts `MARGIN` px from the top left corner, and the room that takes.
fn frame(ink: Ink) -> Layout {
    let Ink { ops, notes, .. } = ink;
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    let mut grow = |x: f32, y: f32| {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    };
    for op in &ops {
        match op {
            Op::Line { pts, .. } | Op::Dot { ring: pts } => pts.iter().for_each(|p| grow(p.0, p.1)),
            Op::Text { text, at, height } => {
                let half = font::width(text, *height) / 2.0;
                grow(at.0 - half, at.1 - height / 2.0);
                grow(at.0 + half, at.1 + height / 2.0);
            }
        }
    }
    if lo.0 > hi.0 {
        return Layout { ops, width: 2.0 * MARGIN, height: 2.0 * MARGIN, notes };
    }
    let (dx, dy) = (MARGIN - lo.0, MARGIN - lo.1);
    let by = |p: &(f32, f32)| (p.0 + dx, p.1 + dy);
    let ops = ops
        .into_iter()
        .map(|op| match op {
            Op::Line { pts, pen, dashed, closed } => Op::Line { pts: pts.iter().map(by).collect(), pen, dashed, closed },
            Op::Dot { ring } => Op::Dot { ring: ring.iter().map(by).collect() },
            Op::Text { text, at, height } => Op::Text { text, at: by(&at), height },
        })
        .collect();
    Layout { ops, width: hi.0 - lo.0 + 2.0 * MARGIN, height: hi.1 - lo.1 + 2.0 * MARGIN, notes }
}

/// The room detail `rd` takes at `k` px to a millimetre of the member, px.
pub fn size(m: &Model, rd: &RebarDetail, k: f64) -> Option<(u32, u32)> {
    let l = layout(m, rd, k)?;
    Some((l.width.ceil().clamp(1.0, 1e7) as u32, l.height.ceil().clamp(1.0, 1e7) as u32))
}

/// Detail `rd` from its top left corner at `(x, y)` px of `d`.
pub fn draw(d: &mut Drawing, m: &Model, rd: &RebarDetail, k: f64, (x, y): (f32, f32)) {
    let Some(l) = layout(m, rd, k) else { return };
    let by = |p: (f32, f32)| (p.0 + x, p.1 + y);
    for op in l.ops {
        match op {
            Op::Line { pts, pen, dashed, closed } => {
                d.stroke(pts.into_iter().map(by).collect(), pen, INK, dashed.then(|| vec![10.0, 5.0]), closed)
            }
            Op::Dot { ring } => d.push(Prim::Fill { rings: vec![ring.into_iter().map(by).collect()], color: INK }),
            Op::Text { text, at, height } => d.text(&text, by(at), height, INK),
        }
    }
    for note in l.notes {
        if d.notes.len() < 32 && !d.notes.contains(&note) {
            d.notes.push(note);
        }
    }
}
