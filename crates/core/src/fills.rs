//! Fill patterns, filled region types and filled regions, as Revit's Manage > Additional Settings > Fill Patterns and
//! Annotate > Region.
//!
//! A [`FillPattern`] is families of parallel lines, as a PAT file has them: each family at an angle through an origin, its
//! lines `spacing` apart and each `shift` along from the one before, dashed or not; a pattern with no lines is solid. A
//! drafting pattern's lengths are mm on paper, so it reads the same at any scale; a model pattern's are mm in the building
//! (a brick's course, a tile). A [`FilledRegionType`] is a foreground and a background pattern, each in its colour, over
//! the paper when it masks; a [`FilledRegion`] is a sketch's outlines (`crate::sketch`) in one view, filled as its type
//! says, its boundary in a line style or unseen. Patterns and types are entries of the project's library; a type names its
//! patterns, a region its type and its line style.

use bimcraft_i18n::msg;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geom::{self, P2};
use crate::ids::Guid;
use crate::language::Language;
use crate::sketch::SketchLine;

pub mod make;

/// What a pattern's lengths are measured in: mm on paper (drafting), or mm in the building (model).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    #[default]
    Drafting,
    Model,
}

/// A family of parallel lines in a pattern: at `angle` degrees through `origin`, each line `spacing` from the last across
/// it and `shift` along it; dashed by `dashes` (a dash's length, a space's as a negative one, 0 a dot), or whole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HatchLine {
    pub angle: f64,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub origin: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: f64,
    pub spacing: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashes: Vec<f64>,
}

fn is_origin(p: &P2) -> bool {
    *p == P2(0.0, 0.0)
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl HatchLine {
    /// Whole lines at `angle`, `spacing` apart.
    pub fn simple(angle: f64, spacing: f64) -> HatchLine {
        HatchLine { angle, origin: P2(0.0, 0.0), shift: 0.0, spacing, dashes: Vec::new() }
    }
}

/// A fill pattern: its lines, in drafting or model lengths; none, it fills solid. One made of lines drawn in a view
/// ([`make`]) remembers that view's scale, `drawn`: drafting, it is the drawing on paper at that scale, model, the drawing
/// itself, so turning it from one to the other rescales it; and the desktop offers to redefine it from lines again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FillPattern {
    #[serde(default)]
    pub target: Target,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<HatchLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn: Option<f64>,
}

/// The most line families a pattern has, and dashes a family has.
pub const MAX_FAMILIES: usize = 256;
pub const MAX_DASHES: usize = 16;
/// The closest a family's lines may be, and the furthest apart, in the pattern's lengths; and the longest dash.
pub const MIN_SPACING: f64 = 0.05;
pub const MAX_SPACING: f64 = 100_000.0;

impl FillPattern {
    pub fn is_solid(&self) -> bool {
        self.lines.is_empty()
    }

    /// Whether it is lines alone, whole (no dashes) and not shifted along: what the Fill Patterns dialog's Simple Lines
    /// makes and its Edit Lines edits, an angle, an origin and a spacing for each family. A shift only staggers dashes.
    pub fn whole_lines(&self) -> bool {
        !self.lines.is_empty() && self.lines.iter().all(|l| l.dashes.is_empty() && l.shift == 0.0)
    }

    /// The pattern as a `target` one: one made of lines drawn is rescaled (drafting, the drawing on paper at the scale it was
    /// drawn at; model, the drawing), any other only told so.
    pub fn retargeted(&self, target: Target) -> FillPattern {
        let k = match (self.drawn, self.target, target) {
            (Some(k), Target::Model, Target::Drafting) => 1.0 / k,
            (Some(k), Target::Drafting, Target::Model) => k,
            _ => 1.0,
        };
        FillPattern { target, ..self.scaled(k) }
    }

    /// The same pattern `k` times as big: its origins, shifts, spacings and dashes.
    pub fn scaled(&self, k: f64) -> FillPattern {
        let lines = (self.lines.iter())
            .map(|l| HatchLine {
                angle: l.angle,
                origin: l.origin * k,
                shift: l.shift * k,
                spacing: l.spacing * k,
                dashes: l.dashes.iter().map(|d| d * k).collect(),
            })
            .collect();
        FillPattern { target: self.target, lines, drawn: self.drawn }
    }

    /// Whether it's one family or two square to each other, all whole and through the origin: what the Fill Patterns
    /// dialog's simple pattern edits (an angle, a spacing, crosshatched or not).
    pub fn simple(&self) -> Option<(f64, f64, bool)> {
        let plain = |l: &HatchLine| l.dashes.is_empty() && l.shift == 0.0 && l.origin == P2(0.0, 0.0);
        match self.lines.as_slice() {
            [a] if plain(a) => Some((a.angle, a.spacing, false)),
            [a, b] if plain(a) && plain(b) && a.spacing == b.spacing && ((b.angle - a.angle).rem_euclid(180.0) - 90.0).abs() < 1e-9 => {
                Some((a.angle, a.spacing, true))
            }
            _ => None,
        }
    }

    /// An error saying what to fix, unless its families are few enough and each is a family of lines.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("fill pattern `{name}`");
        if self.drawn.is_some_and(|k| !(k.is_finite() && k > 0.0 && k <= MAX_SPACING)) {
            return Err(format!("{who}: the scale it was drawn at is a number above 0, at most {MAX_SPACING}"));
        }
        if self.lines.len() > MAX_FAMILIES {
            return Err(format!("{who}: {} line families, more than {MAX_FAMILIES}", self.lines.len()));
        }
        for (i, l) in self.lines.iter().enumerate() {
            let fam = format!("{who} line {}", i + 1);
            if !(l.angle.is_finite() && l.origin.is_finite() && l.shift.is_finite() && l.shift.abs() <= MAX_SPACING) {
                return Err(format!("{fam}: its angle, origin and shift are numbers, the shift within {MAX_SPACING}"));
            }
            if l.origin.0.abs() > MAX_SPACING || l.origin.1.abs() > MAX_SPACING {
                return Err(format!("{fam}: its origin is within {MAX_SPACING} of the pattern's"));
            }
            if !(l.spacing.is_finite() && (MIN_SPACING..=MAX_SPACING).contains(&l.spacing)) {
                return Err(format!("{fam}: its lines are {MIN_SPACING} to {MAX_SPACING} apart, not {}", l.spacing));
            }
            if l.dashes.len() > MAX_DASHES {
                return Err(format!("{fam}: {} dashes and spaces, more than {MAX_DASHES}", l.dashes.len()));
            }
            if l.dashes.iter().any(|x| !x.is_finite() || x.abs() > MAX_SPACING) {
                return Err(format!("{fam}: a dash or a space is a length within {MAX_SPACING}"));
            }
            // Something drawn (a dash, or a dot: 0), a space between, and a pattern that repeats no closer than lines may lie.
            if !l.dashes.is_empty()
                && (l.dashes.iter().map(|x| x.abs()).sum::<f64>() < MIN_SPACING
                    || !l.dashes.iter().any(|x| *x < 0.0)
                    || !l.dashes.iter().any(|x| *x >= 0.0))
            {
                return Err(format!(
                    "{fam}: dashes need a dash (0 is a dot) and a space (a negative length) among them, and {MIN_SPACING} or more in all"
                ));
            }
        }
        Ok(())
    }
}

/// How short a dot draws, in the pattern's lengths.
const DOT: f64 = 0.15;

/// As [`strokes`], the pattern started at `origin` and turned `angle` degrees counter-clockwise.
pub fn strokes_placed(
    p: &FillPattern,
    rings: &[Vec<P2>],
    k: f64,
    window: (P2, P2),
    max: usize,
    (origin, angle): (P2, f64),
) -> Option<Vec<[P2; 2]>> {
    if at_origin(&origin) && angle == 0.0 {
        return strokes(p, rings, k, window, max);
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    let into = |q: P2| {
        let d = q - origin;
        P2(d.0 * cos + d.1 * sin, d.1 * cos - d.0 * sin)
    };
    let back = |q: P2| origin + P2(q.0 * cos - q.1 * sin, q.0 * sin + q.1 * cos);
    let rings: Vec<Vec<P2>> = rings.iter().map(|r| r.iter().map(|q| into(*q)).collect()).collect();
    let (lo, hi) = window;
    let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)].map(into);
    let fold = |f: fn(f64, f64) -> f64, start: f64, of: fn(&P2) -> f64| corners.iter().map(of).fold(start, f);
    let w = (
        P2(fold(f64::min, f64::INFINITY, |q| q.0), fold(f64::min, f64::INFINITY, |q| q.1)),
        P2(fold(f64::max, f64::NEG_INFINITY, |q| q.0), fold(f64::max, f64::NEG_INFINITY, |q| q.1)),
    );
    Some(strokes(p, &rings, k, w, max)?.into_iter().map(|[a, b]| [back(a), back(b)]).collect())
}

/// The pattern's strokes over the area inside `rings` (outlines and holes, either way round: inside is inside an odd number
/// of them), each as a straight piece in model mm, `k` model mm to each of the pattern's (the view's scale for a drafting
/// pattern, 1 for a model one); within `window` (its lowest and its highest corner), and no more than `max` of them. None
/// when it would take more, too dense to draw.
pub fn strokes(p: &FillPattern, rings: &[Vec<P2>], k: f64, window: (P2, P2), max: usize) -> Option<Vec<[P2; 2]>> {
    let mut out = Vec::new();
    let pts = rings.iter().flatten();
    let (mut lo, mut hi) = (P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for q in pts {
        (lo, hi) = (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)));
    }
    let (lo, hi) = (P2(lo.0.max(window.0.0), lo.1.max(window.0.1)), P2(hi.0.min(window.1.0), hi.1.min(window.1.1)));
    if !(lo.0 < hi.0 && lo.1 < hi.1 && k.is_finite() && k > 0.0) {
        return Some(out);
    }
    let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)];
    let edges = rings.iter().map(Vec::len).sum::<usize>().max(1);
    let mut work = 0usize;
    for l in &p.lines {
        let (sin, cos) = l.angle.to_radians().sin_cos();
        let (u, n) = (P2(cos, sin), P2(-sin, cos));
        let (origin, step, shift) = (l.origin * k, l.spacing * k, l.shift * k);
        // The lines that cross the box: their numbers across it.
        let across = corners.iter().map(|c| (*c - origin).dot(n));
        let (c0, c1) = across.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
        let (first, last) = ((c0 / step).ceil(), (c1 / step).floor());
        if !(first.is_finite() && last.is_finite()) || last - first > max as f64 {
            return None;
        }
        // Each line is tried against every edge, and each dash stepped over: too much to ask of a drawing, when it comes to it.
        work = work.saturating_add(((last - first + 1.0).max(0.0) as usize).saturating_mul(edges));
        if work > MAX_WORK {
            return None;
        }
        let dashes: Vec<f64> = l.dashes.iter().map(|x| x * k).collect();
        let repeat: f64 = dashes.iter().map(|x| x.abs()).sum();
        if !dashes.is_empty() && !(repeat.is_finite() && repeat > 0.0) {
            continue;
        }
        let mut j = first;
        while j <= last {
            let base = origin + n * (j * step) + u * (j * shift);
            // Only the stretch of the line over the box is wanted: what lies past it is never drawn.
            let Some((w0, w1)) = reach(base, u, lo, hi) else {
                j += 1.0;
                continue;
            };
            for (t0, t1) in spans(rings, base, u, n).into_iter().map(|(a, b)| (a.max(w0), b.min(w1))).filter(|(a, b)| b > a) {
                if dashes.is_empty() {
                    out.push([base + u * t0, base + u * t1]);
                } else {
                    // Each repeat of the dashes from the base along the line, where it's inside.
                    let mut r = (t0 / repeat).floor();
                    while r * repeat <= t1 {
                        let mut s = r * repeat;
                        for d in &dashes {
                            let len = d.abs();
                            if *d >= 0.0 {
                                let (a, b) = (s.max(t0), (s + if *d == 0.0 { DOT * k } else { len }).min(t1));
                                if b > a {
                                    out.push([base + u * a, base + u * b]);
                                }
                            }
                            s += len;
                        }
                        r += 1.0;
                        #[cfg(test)]
                        done(dashes.len());
                        work = work.saturating_add(dashes.len());
                        if out.len() > max || work > MAX_WORK {
                            return None;
                        }
                    }
                }
                if out.len() > max {
                    return None;
                }
            }
            j += 1.0;
        }
    }
    Some(out)
}

/// The most work (an edge tried against a line, a dash stepped over) one pattern may ask of a drawing, however dense it is.
pub const MAX_WORK: usize = 5_000_000;

#[cfg(test)]
thread_local! {
    /// What [`strokes`] has done on this thread: the edges tried against a line, the dashes stepped over. The tests assert on it,
    /// not on time, which varies with the load of the machine.
    static DONE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn done(n: usize) {
    DONE.with(|d| d.set(d.get().saturating_add(n)));
}

/// The stretch of the line through `base` along unit `u` that lies in the box from `lo` to `hi`, as distances from `base`.
fn reach(base: P2, u: P2, lo: P2, hi: P2) -> Option<(f64, f64)> {
    let mut t = (f64::NEG_INFINITY, f64::INFINITY);
    for (b, d, a, z) in [(base.0, u.0, lo.0, hi.0), (base.1, u.1, lo.1, hi.1)] {
        if d.abs() < 1e-12 {
            if b < a || b > z {
                return None;
            }
        } else {
            let (x, y) = ((a - b) / d, (z - b) / d);
            t = (t.0.max(x.min(y)), t.1.min(x.max(y)));
        }
    }
    (t.0 < t.1).then_some(t)
}

/// Where the line through `base` along unit `u` (`n` square to it) is inside `rings`, as runs along it from `base`: it
/// crosses a ring's edge where the edge's ends lie on either side, and between each two crossings it's in or out by turns.
fn spans(rings: &[Vec<P2>], base: P2, u: P2, n: P2) -> Vec<(f64, f64)> {
    #[cfg(test)]
    done(rings.iter().map(Vec::len).sum());
    let mut ts: Vec<f64> = Vec::new();
    for (a, b) in rings.iter().flat_map(|r| geom::ring_edges(r)) {
        let (sa, sb) = ((a - base).dot(n), (b - base).dot(n));
        if (sa > 0.0) != (sb > 0.0) {
            let x = a + (b - a) * (sa / (sa - sb));
            ts.push((x - base).dot(u));
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).filter(|(a, b)| b > a).collect()
}

/// A filled region type: its foreground and background patterns (none, nothing) and their colours, and whether it masks
/// what's under it, the paper showing through around its patterns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilledRegionType {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default)]
    pub foreground_color: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default)]
    pub background_color: [u8; 3],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub masking: bool,
}

impl FilledRegionType {
    /// An error saying what to fix, unless the patterns it names are `patterns`'.
    pub fn check(&self, name: &str, patterns: &BTreeMap<String, FillPattern>) -> Result<(), String> {
        for p in [&self.foreground, &self.background].into_iter().flatten() {
            if !patterns.contains_key(p) {
                let there: Vec<&str> = patterns.keys().map(String::as_str).collect();
                return Err(format!("filled region type `{name}`: there's no fill pattern `{p}` (there are: {})", there.join(", ")));
            }
        }
        Ok(())
    }
}

/// A filled region: the outlines its sketch closes into (holes and islands as a sketch nests them), in one view (a plan,
/// a section, an elevation or a sheet, in its own terms), filled as type `type_name` says, its boundary drawn in line style
/// `line_style` or, none, unseen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilledRegion {
    pub view: Guid,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_style: Option<String>,
    pub sketch: Vec<SketchLine>,
    /// Where its patterns start, and how far they're turned, degrees: Revit's aligned and rotated model pattern.
    #[serde(default, skip_serializing_if = "at_origin")]
    pub pattern_origin: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pattern_angle: f64,
}

pub fn at_origin(p: &P2) -> bool {
    *p == P2(0.0, 0.0)
}

/// The fill pattern every project has, which fills solid: in English.
pub const SOLID_FILL: &str = msg!("Solid fill");
/// The filled region type a region takes unless told another, and the one Masking Region takes: in English.
pub const SOLID_BLACK: &str = msg!("Solid Black");
pub const MASKING: &str = msg!("Masking");

/// The fill patterns a project starts with, named in `language`: our own, drafting ones mm on paper and model ones mm in the
/// building.
pub fn patterns(language: Language) -> BTreeMap<String, FillPattern> {
    let drafting = |lines: Vec<HatchLine>| FillPattern { target: Target::Drafting, lines, drawn: None };
    let model = |lines: Vec<HatchLine>| FillPattern { target: Target::Model, lines, drawn: None };
    let s = HatchLine::simple;
    [
        (SOLID_FILL, drafting(Vec::new())),
        (msg!("Diagonal Up"), drafting(vec![s(45.0, 1.5)])),
        (msg!("Diagonal Down"), drafting(vec![s(135.0, 1.5)])),
        (msg!("Diagonal Crosshatch"), drafting(vec![s(45.0, 2.0), s(135.0, 2.0)])),
        (msg!("Crosshatch"), drafting(vec![s(0.0, 2.0), s(90.0, 2.0)])),
        (msg!("Horizontal"), drafting(vec![s(0.0, 1.5)])),
        (msg!("Vertical"), drafting(vec![s(90.0, 1.5)])),
        // A dot every mm, each row half a step along from the last.
        (msg!("Sand"), drafting(vec![HatchLine { shift: 0.5, dashes: vec![0.0, -1.0], ..s(0.0, 1.0) }])),
        // Courses 75 mm high; joints every 225 mm, half a brick along from one course to the next.
        (
            msg!("Brick 225 x 75"),
            model(vec![
                s(0.0, 75.0),
                HatchLine { angle: 90.0, origin: P2(0.0, 0.0), shift: 75.0, spacing: 112.5, dashes: vec![75.0, -75.0] },
            ]),
        ),
        (msg!("Tile 300"), model(vec![s(0.0, 300.0), s(90.0, 300.0)])),
    ]
    .into_iter()
    .map(|(n, p)| (language.word(n).to_string(), p))
    .collect()
}

pub fn default_patterns() -> BTreeMap<String, FillPattern> {
    patterns(Language::English)
}

/// The filled region types a project starts with, named in `language`.
pub fn region_types(language: Language) -> BTreeMap<String, FilledRegionType> {
    let fg = |p: &str| FilledRegionType { foreground: Some(language.word(p).to_string()), ..Default::default() };
    [
        (SOLID_BLACK, fg(SOLID_FILL)),
        ("Diagonal Up", fg("Diagonal Up")),
        ("Crosshatch", fg("Crosshatch")),
        ("Sand", fg("Sand")),
        ("Brick", fg("Brick 225 x 75")),
        (MASKING, FilledRegionType { masking: true, ..Default::default() }),
    ]
    .into_iter()
    .map(|(n, t)| (language.word(n).to_string(), t))
    .collect()
}

pub fn default_region_types() -> BTreeMap<String, FilledRegionType> {
    region_types(Language::English)
}

/// The type a filled region takes unless told another, of `types` in `language`: Solid Black by the name the language gives it,
/// else by its English name (the types of a file from before they were named in a language), else the first.
pub fn default_region_type(types: &BTreeMap<String, FilledRegionType>, language: Language) -> Option<&String> {
    [language.word(SOLID_BLACK), SOLID_BLACK]
        .into_iter()
        .find_map(|n| types.get_key_value(n).map(|(k, _)| k))
        .or_else(|| types.keys().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Vec<P2> {
        vec![P2(x, y), P2(x + s, y), P2(x + s, y + s), P2(x, y + s)]
    }

    const ALL: (P2, P2) = (P2(-1e6, -1e6), P2(1e6, 1e6));

    #[test]
    fn a_family_of_lines_fills_inside_the_outline_and_not_its_hole() {
        let p = FillPattern { target: Target::Model, lines: vec![HatchLine::simple(0.0, 100.0)], drawn: None };
        let rings = vec![square(0.0, 0.0, 1000.0), square(400.0, 400.0, 200.0)];
        let s = strokes(&p, &rings, 1.0, ALL, 10_000).unwrap();
        // Lines at y = 0 … 900 (an edge's lower side is inside, its upper side out, so 1000 is left out); those through the
        // hole broken round it.
        let at = |y: f64| s.iter().filter(|l| (l[0].1 - y).abs() < 1e-9).count();
        assert_eq!((at(0.0), at(100.0), at(500.0), at(1000.0)), (1, 1, 2, 0));
        let length: f64 = s.iter().map(|l| l[0].dist(l[1])).sum();
        assert!((length - (10.0 * 1000.0 - 2.0 * 200.0)).abs() < 1e-6, "{length}");
        assert!(s.iter().all(|l| !(l[0].0 > 400.0 && l[1].0 < 600.0 && l[0].1 > 400.0 && l[0].1 < 600.0)), "none inside the hole");
    }

    #[test]
    fn a_drafting_patterns_lengths_grow_with_the_scale_and_dashes_and_dots_repeat_along_it() {
        let p = FillPattern {
            target: Target::Drafting,
            lines: vec![HatchLine { dashes: vec![2.0, -1.0, 0.0, -1.0], ..HatchLine::simple(0.0, 1.0) }],
            drawn: None,
        };
        let ring = vec![square(0.0, 0.0, 400.0)];
        // At 1 : 100 a mm of paper is 100 mm: lines 100 apart, a 200 dash, a dot, every 400.
        let s = strokes(&p, &ring, 100.0, ALL, 10_000).unwrap();
        let row: Vec<&[P2; 2]> = s.iter().filter(|l| (l[0].1 - 100.0).abs() < 1e-9).collect();
        assert_eq!(row.len(), 2, "{row:?}");
        assert!((row[0][1].0 - row[0][0].0 - 200.0).abs() < 1e-9 && (row[1][1].0 - row[1][0].0 - 15.0).abs() < 1e-9, "{row:?}");
        assert_eq!(s.iter().map(|l| l[0].1).fold(0.0, f64::max), 300.0);
    }

    #[test]
    fn running_bond_staggers_its_joints_half_a_brick_course_by_course() {
        let brick = &default_patterns()["Brick 225 x 75"];
        let s = strokes(brick, &[square(1.0, 1.0, 1000.0)], 1.0, ALL, 100_000).unwrap();
        let joints = |y: f64| {
            let mut x: Vec<f64> =
                s.iter().filter(|l| l[0].0 == l[1].0 && l[0].1.min(l[1].1) < y && l[0].1.max(l[1].1) > y).map(|l| l[0].0).collect();
            x.sort_by(f64::total_cmp);
            x
        };
        let (first, second) = (joints(40.0), joints(115.0));
        assert!(first.iter().all(|x| (x / 225.0).fract().abs() < 1e-9 || (x / 225.0).fract() > 1.0 - 1e-9), "{first:?}");
        assert!(second.iter().all(|x| ((x - 112.5) / 225.0).fract().abs() < 1e-9), "{second:?}");
    }

    #[test]
    fn too_dense_a_pattern_draws_nothing_rather_than_too_much() {
        let p = &default_patterns()["Diagonal Up"];
        assert_eq!(strokes(p, &[square(0.0, 0.0, 1e6)], 0.001, ALL, 10_000), None);
        assert!(strokes(&default_patterns()[SOLID_FILL], &[square(0.0, 0.0, 10.0)], 1.0, ALL, 10).is_some_and(|s| s.is_empty()));
    }

    #[test]
    fn patterns_are_checked_and_a_simple_one_is_read_back_as_its_angle_spacing_and_crosshatch() {
        assert_eq!(default_patterns()["Crosshatch"].simple(), Some((0.0, 2.0, true)));
        assert_eq!(default_patterns()["Diagonal Down"].simple(), Some((135.0, 1.5, false)));
        assert_eq!(default_patterns()["Brick 225 x 75"].simple(), None);
        for (n, p) in default_patterns() {
            assert_eq!(p.check(&n), Ok(()));
        }
        let bad = FillPattern { target: Target::Drafting, lines: vec![HatchLine::simple(0.0, 0.0)], drawn: None };
        assert!(bad.check("x").unwrap_err().contains("apart"));
        let gapless =
            FillPattern { lines: vec![HatchLine { dashes: vec![1.0, 2.0], ..HatchLine::simple(0.0, 1.0) }], ..Default::default() };
        assert!(gapless.check("x").unwrap_err().contains("space"));
        for (n, t) in default_region_types() {
            assert_eq!(t.check(&n, &default_patterns()), Ok(()));
        }
    }

    use crate::engine::Session;
    use crate::model::{Element, Id};
    use crate::render::{Drawing, Prim, View};
    use serde_json::{Value, json};

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// A square's lines, drawn round from `[x, y]`.
    fn lines(x: f64, y: f64, size: f64) -> Vec<Value> {
        let c = [[x, y], [x + size, y], [x + size, y + size], [x, y + size]];
        (0..4).map(|i| json!([c[i], c[(i + 1) % 4]])).collect()
    }

    fn holed() -> Value {
        let mut ls = lines(0.0, 0.0, 4000.0);
        ls.extend(lines(1500.0, 1500.0, 1000.0));
        Value::Array(ls)
    }

    fn plan(s: &Session) -> Drawing {
        let level = s.model.levels()[0].0;
        crate::render::draw(&s.model, level, View { scale: 0.1, cx: 2000.0, cy: 2000.0, width: 600, height: 600 })
    }

    /// The prims `id` draws, with where each is in the drawing's order.
    fn of(d: &Drawing, id: Id) -> Vec<(usize, &Prim)> {
        d.prims.iter().zip(&d.owners).enumerate().filter(|(_, (_, o))| **o == Some(id)).map(|(i, (p, _))| (i, p)).collect()
    }

    #[test]
    fn fill_patterns_come_in_from_a_pat_file_and_go_out_to_one_undone_in_one_step() {
        let mut s = Session::new();
        let text = "*Herringbone, a test\n;%TYPE=MODEL\n45, 0,0, 0,100, 200,-200\n*Crosshatch\n0, 0,0, 0,3\n*Bad\n0, 0,0, 0,0\n";
        let r = run(&mut s, "import_fill_patterns", json!({"text": text}));
        assert_eq!(r["imported"], json!(["Herringbone"]));
        let why: Vec<&str> = r["left_out"].as_array().into_iter().flatten().filter_map(|v| v["name"].as_str()).collect();
        assert_eq!(why, ["Crosshatch", "Bad"], "the project's own Crosshatch stays, Bad can't be one");
        assert_eq!(s.model.fill_patterns["Herringbone"].target, Target::Model);
        let r = run(&mut s, "import_fill_patterns", json!({"text": text, "replace": true}));
        assert_eq!(r["imported"], json!(["Herringbone", "Crosshatch"]));
        assert_eq!(s.model.fill_patterns["Crosshatch"].lines, vec![HatchLine::simple(0.0, 3.0)]);
        // Out again: every pattern but Solid fill, read back the same.
        let out = run(&mut s, "export_fill_patterns", json!({}));
        let back = crate::pat::read(out["text"].as_str().unwrap_or_default()).unwrap_or_default();
        assert_eq!(back.patterns.len(), s.model.fill_patterns.len() - 1);
        assert!(back.patterns.iter().all(|(n, p)| s.model.fill_patterns.get(n) == Some(p)));
        let one = run(&mut s, "export_fill_patterns", json!({"names": ["Herringbone"]}));
        assert!(one["text"].as_str().is_some_and(|t| t.contains("*Herringbone\n;%TYPE=MODEL\n45,0,0,0,100,200,-200")), "{one}");
        assert!(s.execute("export_fill_patterns", &json!({"names": ["Nope"]})).is_err());
        run(&mut s, "undo", json!({}));
        assert!(!s.model.fill_patterns["Crosshatch"].simple().is_some_and(|x| x.1 == 3.0), "the replace undone");
        assert!(s.execute("import_fill_patterns", &json!({})).is_err());
    }

    #[test]
    fn fill_patterns_and_region_types_are_the_projects_named_in_its_language_and_kept_while_used() {
        let mut s = Session::new();
        let r = run(&mut s, "fill_pattern", json!({}));
        assert_eq!(r["patterns"]["Solid fill"], json!({"target": "drafting"}));
        assert_eq!(r["patterns"]["Brick 225 x 75"]["target"], "model");
        assert_eq!(
            run(&mut s, "filled_region_type", json!({}))["types"]["Masking"],
            json!({"foreground_color": [0, 0, 0], "background_color": [0, 0, 0], "masking": true})
        );
        let pt = crate::model::Model::new_in("p", Language::Portuguese);
        assert!(pt.fill_patterns.contains_key("Areia") && pt.region_types.contains_key("Preto sólido"));
        assert!(s.history().is_empty(), "asking changes nothing");
        // Made, renamed (the type filling with it following), and refused deleted while a type fills with it.
        run(&mut s, "fill_pattern", json!({"name": "Hatch 30", "lines": [{"angle": 30, "spacing": 2}]}));
        run(&mut s, "filled_region_type", json!({"name": "Earth", "foreground": "Hatch 30", "foreground_color": "#805020"}));
        run(&mut s, "fill_pattern", json!({"name": "Hatch 30", "rename": "Earth Hatch"}));
        assert_eq!(s.model.region_types["Earth"].foreground.as_deref(), Some("Earth Hatch"));
        let e = s.execute("fill_pattern", &json!({"name": "Earth Hatch", "delete": true})).unwrap_err();
        assert!(e.contains("`Earth` fills with `Earth Hatch`"), "{e}");
        let e = s.execute("filled_region_type", &json!({"name": "Bad", "foreground": "Nowhere"})).unwrap_err();
        assert!(e.contains("no fill pattern `Nowhere`"), "{e}");
        let e = s.execute("fill_pattern", &json!({"name": "Tight", "lines": [{"angle": 0, "spacing": 0}]})).unwrap_err();
        assert!(e.contains("apart"), "{e}");
        // Each an entry of the library, with an id and a GUID.
        assert!(s.model.library.id(&crate::library::Kind::FillPattern, "Earth Hatch").is_some());
        assert!(s.model.library.id(&crate::library::Kind::FilledRegionType, "Earth").is_some());
        // A type is refused deleted while a region is of it.
        run(&mut s, "filled_region", json!({"sketch": lines(0.0, 0.0, 1000.0), "type": "Earth"}));
        let e = s.execute("filled_region_type", &json!({"name": "Earth", "delete": true})).unwrap_err();
        assert!(e.contains("is of `Earth`"), "{e}");
    }

    #[test]
    fn a_region_with_no_type_takes_solid_black_in_a_file_from_before_the_types_were_named_in_its_language() {
        for (language, solid) in [(Language::Portuguese, "Preto sólido"), (Language::Spanish, "Negro sólido")] {
            // A new project of the language names its types in it; a file written before the types were (format 5, with no
            // line or fill styles) reads with the English ones, whatever its language.
            let fresh = crate::model::Model::new_in("p", language);
            assert!(fresh.region_types.contains_key(solid), "{solid}");
            assert_eq!(default_region_type(&fresh.region_types, language).map(String::as_str), Some(solid));
            let file = fresh.to_json().unwrap();
            let mut v: Value = serde_json::from_str(&file).unwrap();
            let keys = ["line_styles", "line_patterns", "fill_patterns", "region_types"];
            for key in keys {
                assert!(v.as_object_mut().and_then(|o| o.remove(key)).is_some(), "{key}");
            }
            v["format"] = json!(5);
            let (m, left_out) = crate::model::Model::read(&v.to_string()).unwrap();
            assert!(left_out.is_empty(), "{left_out:?}");
            assert!(m.region_types.contains_key(SOLID_BLACK) && !m.region_types.contains_key(solid));
            let mut s = Session::new();
            s.reset(m, None);
            let id = run(&mut s, "filled_region", json!({"sketch": lines(0.0, 0.0, 1000.0)}))["id"].as_u64().unwrap();
            let Some(Element::FilledRegion(r)) = s.model.elements.get(id) else { panic!("no region") };
            assert_eq!(r.type_name, SOLID_BLACK, "{language:?}");
            assert_eq!(s.model.default_type("region"), Some(SOLID_BLACK));
        }
        // Without Solid Black by either name, the first.
        let mut types = default_region_types();
        types.remove(SOLID_BLACK);
        assert_eq!(default_region_type(&types, Language::English).map(String::as_str), types.keys().next().map(String::as_str));
    }

    #[test]
    fn a_filled_region_fills_its_outline_round_its_holes_in_its_type_and_draws_its_boundary() {
        let mut s = Session::new();
        let r = run(&mut s, "filled_region", json!({"sketch": holed()}));
        assert_eq!(r["area_m2"], 15.0);
        let id = r["id"].as_u64().unwrap();
        let d = plan(&s);
        let drawn = of(&d, id);
        let fills: Vec<&Vec<Vec<(f32, f32)>>> = drawn
            .iter()
            .filter_map(|(_, p)| match p {
                Prim::Fill { rings, color } if *color == crate::render::Rgb(0, 0, 0) => Some(rings),
                _ => None,
            })
            .collect();
        assert!(!fills.is_empty(), "Solid Black fills it");
        let hole = d.view.px(P2(2000.0, 2000.0));
        let inside = |r: &[(f32, f32)]| {
            geom::contains(
                &r.iter().map(|q| P2(f64::from(q.0), f64::from(q.1))).collect::<Vec<_>>(),
                P2(f64::from(hole.0), f64::from(hole.1)),
            )
        };
        assert!(fills.iter().flat_map(|rs| rs.iter()).all(|r| !inside(r)), "not over its hole");
        let boundary = drawn.iter().filter(|(_, p)| matches!(p, Prim::Line { .. })).count();
        assert_eq!(boundary, 8, "its eight sketch lines, in Thin Lines");
        // A hatch: lines, none through the hole; no boundary with invisible lines.
        let h = run(&mut s, "filled_region", json!({"sketch": holed(), "type": "Diagonal Up", "line_style": null}))["id"].as_u64().unwrap();
        let d = plan(&s);
        let strokes: Vec<&Prim> = of(&d, h).into_iter().map(|(_, p)| p).collect();
        assert!(strokes.len() > 10, "{}", strokes.len());
        assert!(strokes.iter().all(|p| matches!(p, Prim::Line { width, .. } if *width <= 1.0)), "pattern lines, thin; no boundary");
        // Refused: a sketch that doesn't close.
        let mut open = lines(0.0, 0.0, 1000.0);
        open.pop();
        let e = s.execute("filled_region", &json!({"sketch": open})).unwrap_err();
        assert!(e.contains("open"), "{e}");
    }

    #[test]
    fn foreground_and_background_colors_render_separately_and_leave_holes_open() {
        let mut s = Session::new();
        run(
            &mut s,
            "filled_region_type",
            json!({"name": "Two layers", "foreground": "Crosshatch", "foreground_color": [180, 20, 30],
            "background": SOLID_FILL, "background_color": [220, 230, 240], "masking": true}),
        );
        run(&mut s, "filled_region", json!({"sketch": holed(), "type": "Two layers", "line_style": null}));
        let drawing = plan(&s);
        let pixels = tiny_skia::Pixmap::decode_png(&crate::render::png(&drawing).unwrap()).unwrap();
        let has = |rgb: [u8; 3]| pixels.pixels().iter().any(|p| [p.red(), p.green(), p.blue()] == rgb);
        // A thin hatch can straddle pixels: its ink blends with the background through antialiasing.
        assert!(
            pixels.pixels().iter().any(|p| p.red() > p.green().saturating_add(5) && p.red() > p.blue().saturating_add(5)),
            "red foreground lines remain distinguishable over the blue-gray background"
        );
        assert!(has([220, 230, 240]), "background remains visible between the foreground lines");
        let (x, y) = drawing.view.px(P2(2000.0, 2000.0));
        let hole = pixels.pixel(x as u32, y as u32).unwrap();
        assert_eq!([hole.red(), hole.green(), hole.blue()], [255, 255, 255]);
        let saved = crate::model::Model::from_json(&s.model.to_json().unwrap()).unwrap();
        assert_eq!(saved.region_types["Two layers"], s.model.region_types["Two layers"]);
    }

    #[test]
    fn non_masking_regions_replace_model_backgrounds_but_keep_visible_cut_and_projection_ink() {
        let mut s = Session::new();
        run(&mut s, "material", json!({"name": "Hatched", "cut": "diagonal"}));
        run(&mut s, "type", json!({"category": "wall", "name": "Hatched Wall", "layers": [{"material": "Hatched", "thickness": 500}]}));
        run(&mut s, "wall", json!({"points": [[0,0], [4000,4000]], "type": "Hatched Wall"}));
        let low = run(&mut s, "wall", json!({"points": [[0,1500], [4000,1500]], "height": 500, "type": "Hatched Wall"}))["ids"][0].clone();
        run(&mut s, "graphics", json!({"elements": {low.to_string(): {"projection": {"color": [255,0,0]}}}}));
        run(
            &mut s,
            "filled_region_type",
            json!({"name": "Blue", "foreground": SOLID_FILL, "foreground_color": [0,0,255], "masking": false}),
        );
        run(&mut s, "filled_region", json!({"sketch": lines(1000.0, 1000.0, 2000.0), "type": "Blue", "line_style": null}));
        let render = |s: &Session| {
            let d = plan(s);
            let pm = tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap();
            (d.view, pm)
        };
        let count = |v: View, pm: &tiny_skia::Pixmap, p: P2, predicate: fn([u8; 3]) -> bool| {
            let (x, y) = v.px(p);
            (-5..=5)
                .flat_map(|dx| (-5..=5).map(move |dy| (dx, dy)))
                .filter_map(|(dx, dy)| pm.pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32))
                .filter(|c| predicate([c.red(), c.green(), c.blue()]))
                .count()
        };
        // A one-pixel hatch centered on a pixel boundary blends its ink with the blue behind it.
        let dark = |c: [u8; 3]| c[0] < 80 && c[1] < 80 && c[2] < 200;
        let red = |c: [u8; 3]| c[0] > 150 && c[1] < 80 && c[2] < 80;
        let (v, pm) = render(&s);
        assert!(count(v, &pm, P2(2500.0, 2500.0), dark) > 0, "cut hatch above the region");
        assert!(count(v, &pm, P2(2500.0, 1750.0), red) > 0, "projected outline above the region");
        assert_eq!(count(v, &pm, P2(1750.0, 1750.0), red), 0, "a cut wall still occludes the low wall behind it");
        assert!(pm.pixels().iter().any(|c| [c.red(), c.green(), c.blue()] == [0, 0, 255]), "region replaces the cut background");
        run(&mut s, "material", json!({"name": "Hatched", "cut": "solid"}));
        let (v, solid) = render(&s);
        let (x, y) = v.px(P2(2500.0, 2500.0));
        let c = solid.pixel(x as u32, y as u32).unwrap();
        assert_eq!([c.red(), c.green(), c.blue()], [25, 25, 25], "solid model foreground also stays above a non-masking fill");
        run(&mut s, "material", json!({"name": "Hatched", "cut": "diagonal"}));
        run(&mut s, "filled_region_type", json!({"name": "Blue", "masking": true}));
        let (v, pm) = render(&s);
        assert_eq!(count(v, &pm, P2(2500.0, 2500.0), dark), 0, "masking covers cut patterns");
        assert_eq!(count(v, &pm, P2(2500.0, 1750.0), red), 0, "masking covers projected lines");
    }

    #[test]
    fn non_masking_regions_in_sections_retain_cut_edges_and_projected_model_lines() {
        let mut s = Session::new();
        run(&mut s, "material", json!({"name": "Plain", "cut": "none"}));
        run(&mut s, "type", json!({"category": "wall", "name": "Plain Wall", "layers": [{"material": "Plain", "thickness": 200}]}));
        run(&mut s, "wall", json!({"points": [[3000,0],[3000,6000]], "type": "Plain Wall"}));
        run(&mut s, "column", json!({"at": [6000,4500]}));
        let section = run(&mut s, "section", json!({"start": [-1000,3000], "end": [9000,3000]}))["id"].as_u64().unwrap();
        run(&mut s, "filled_region_type", json!({"name": "Blue", "foreground": SOLID_FILL, "foreground_color": [0,0,255]}));
        run(
            &mut s,
            "filled_region",
            json!({"view": section, "type": "Blue", "line_style": null,
            "sketch": [[[3000,1000],[8000,1000]],[[8000,1000],[8000,2000]],[[8000,2000],[3000,2000]],[[3000,2000],[3000,1000]]]}),
        );
        let v = View { scale: 0.08, cx: 5000.0, cy: 1500.0, width: 800, height: 400 };
        let sample = |pm: &tiny_skia::Pixmap, p: P2| {
            let (x, y) = v.px(p);
            (-3..=3)
                .flat_map(|dx| (-3..=3).map(move |dy| (dx, dy)))
                .filter_map(|(dx, dy)| pm.pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32))
                .filter(|c| c.red() < 100 && c.green() < 100 && c.blue() < 100)
                .count()
        };
        let pm = tiny_skia::Pixmap::decode_png(&crate::render::section_png(&s.model, section, v).unwrap()).unwrap();
        assert!(sample(&pm, P2(3900.0, 1500.0)) > 0, "the cut wall edge stays in front");
        assert!(sample(&pm, P2(6800.0, 1500.0)) > 0, "the projected column stays visible");
        let (x, y) = v.px(P2(4000.0, 1500.0));
        let c = pm.pixel(x as u32, y as u32).unwrap();
        assert_eq!([c.red(), c.green(), c.blue()], [0, 0, 255], "cut background replaced");
        run(&mut s, "filled_region_type", json!({"name": "Blue", "masking": true}));
        let pm = tiny_skia::Pixmap::decode_png(&crate::render::section_png(&s.model, section, v).unwrap()).unwrap();
        assert_eq!(sample(&pm, P2(3900.0, 1500.0)), 0);
        assert_eq!(sample(&pm, P2(6800.0, 1500.0)), 0);
    }

    #[test]
    fn non_masking_regions_keep_their_holes_and_do_not_reveal_model_ink_through_a_mask() {
        let mut s = Session::new();
        run(&mut s, "material", json!({"name": "Plain", "cut": "none"}));
        run(&mut s, "type", json!({"category": "wall", "name": "Plain Wall", "layers": [{"material": "Plain", "thickness": 400}]}));
        run(&mut s, "wall", json!({"points": [[0,2000],[4000,2000]], "type": "Plain Wall"}));
        run(&mut s, "filled_region_type", json!({"name": "Blue", "foreground": SOLID_FILL, "foreground_color": [0,0,255]}));
        let mut sketch = lines(1000.0, 1000.0, 2000.0);
        sketch.extend(lines(1750.0, 1750.0, 500.0));
        run(&mut s, "filled_region", json!({"sketch": sketch, "type": "Blue", "line_style": null}));
        let d = plan(&s);
        let pm = tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap();
        let at = |pm: &tiny_skia::Pixmap, p: P2| {
            let (x, y) = d.view.px(p);
            let c = pm.pixel(x as u32, y as u32).unwrap();
            [c.red(), c.green(), c.blue()]
        };
        assert_eq!(at(&pm, P2(1500.0, 2000.0)), [0, 0, 255]);
        assert_eq!(at(&pm, P2(2000.0, 2000.0)), [168, 168, 168], "the hole leaves the model background alone");
        run(&mut s, "filled_region", json!({"sketch": lines(1250.0,1800.0,500.0), "type": "Masking", "line_style": null}));
        run(&mut s, "filled_region", json!({"sketch": lines(1000.0,1000.0,2000.0), "type": "Blue", "line_style": null}));
        let pm = tiny_skia::Pixmap::decode_png(&crate::render::png(&plan(&s)).unwrap()).unwrap();
        assert_eq!(at(&pm, P2(1500.0, 2200.0)), [0, 0, 255], "a later non-masking region cannot restore model ink through a mask");
        let c = at(&pm, P2(2500.0, 2200.0));
        assert!(c[2] < 150, "the model edge outside the mask is still visible: {c:?}");
    }

    #[test]
    fn a_masking_region_covers_the_model_and_the_annotation_stays_over_it() {
        let mut s = Session::new();
        let w = run(&mut s, "wall", json!({"points": [[0, 2000], [4000, 2000]]}))["ids"][0].as_u64().unwrap();
        let m = run(&mut s, "filled_region", json!({"sketch": lines(500.0, 1000.0, 2000.0), "type": "Masking", "line_style": null}))["id"]
            .as_u64()
            .unwrap();
        let t = run(&mut s, "text", json!({"at": [1000, 2000], "text": "OVER"}))["id"].as_u64().unwrap();
        let d = plan(&s);
        let last = |id| of(&d, id).last().map(|(i, _)| *i).unwrap();
        let first = |id| of(&d, id).first().map(|(i, _)| *i).unwrap();
        assert!(matches!(of(&d, m)[0].1, Prim::Fill { color, .. } if *color == crate::render::PAPER), "paper");
        assert!(first(m) > last(w), "over the wall");
        assert!(first(t) > last(m), "under the note");
    }

    #[test]
    fn a_regions_sketch_is_edited_as_any_other_its_lines_keeping_their_ids_and_any_number_of_loops() {
        let mut s = Session::new();
        let id = run(&mut s, "filled_region", json!({"sketch": lines(0.0, 0.0, 1000.0)}))["id"].clone();
        let Some(Element::FilledRegion(r)) = s.model.elements.get(id.as_u64().unwrap()).cloned() else { panic!() };
        assert!(r.sketch.iter().all(|l| l.id != 0), "its lines are parts");
        let mut again: Vec<Value> = r.sketch.iter().map(|l| json!({"id": l.id, "start": l.start(), "end": l.end()})).collect();
        again.extend(lines(3000.0, 0.0, 500.0));
        let r2 = run(&mut s, "edit_boundary", json!({"id": id, "sketch": again}));
        assert_eq!(r2["area_m2"], 1.25, "two loops");
        let Some(Element::FilledRegion(after)) = s.model.elements.get(id.as_u64().unwrap()) else { panic!() };
        assert_eq!(after.sketch[..4].iter().map(|l| l.id).collect::<Vec<_>>(), r.sketch.iter().map(|l| l.id).collect::<Vec<_>>());
    }

    #[test]
    fn a_materials_patterns_are_the_projects_fill_patterns_and_a_regions_pattern_moves_and_turns() {
        let mut s = Session::new();
        // A material's cut and surface may be fill patterns: kept while used, renamed with it.
        run(&mut s, "material", json!({"name": "Stone", "cut": "Diagonal Up", "surface": "Tile 300"}));
        assert_eq!(s.model.materials["Stone"].cut_pattern.as_deref(), Some("Diagonal Up"));
        assert!(s.execute("fill_pattern", &json!({"name": "Tile 300", "delete": true})).is_err(), "Stone's surface");
        run(&mut s, "fill_pattern", json!({"name": "Tile 300", "rename": "Tile"}));
        assert_eq!(s.model.materials["Stone"].surface_pattern.as_deref(), Some("Tile"));
        assert!(s.execute("material", &json!({"name": "Stone", "cut": "No such"})).is_err());
        run(&mut s, "material", json!({"name": "Stone", "cut": "brick"}));
        assert_eq!(
            (s.model.materials["Stone"].cut, s.model.materials["Stone"].cut_pattern.as_deref()),
            (crate::model::Pattern::Brick, None)
        );
        // A region of a model pattern: its lines through the pattern's origin, square, until moved and turned.
        run(&mut s, "filled_region_type", json!({"name": "Tiles", "foreground": "Tile"}));
        let id = run(&mut s, "filled_region", json!({"sketch": lines(0.0, 0.0, 3000.0), "type": "Tiles", "line_style": null}))["id"]
            .as_u64()
            .unwrap();
        let angles = |s: &Session| -> Vec<i64> {
            let d = plan(s);
            let mut a: Vec<i64> = of(&d, id)
                .into_iter()
                .filter_map(|(_, p)| match p {
                    Prim::Line { pts, .. } if pts.len() == 2 => {
                        let (dx, dy) = (pts[1].0 - pts[0].0, pts[0].1 - pts[1].1);
                        Some((f64::from(dy).atan2(f64::from(dx)).to_degrees().rem_euclid(180.0).round() as i64) % 180)
                    }
                    _ => None,
                })
                .collect();
            a.sort();
            a.dedup();
            a
        };
        assert_eq!(angles(&s), [0, 90]);
        let column = |s: &Session, x: f64| {
            let d = plan(s);
            let px = d.view.px(P2(x, 1500.0)).0;
            of(&d, id).into_iter().any(|(_, p)| matches!(p, Prim::Line { pts, .. } if pts.iter().all(|q| (q.0 - px).abs() < 0.5)))
        };
        assert!(column(&s, 600.0) && !column(&s, 700.0));
        run(&mut s, "modify", json!({"id": id, "pattern_origin": [100, 0]}));
        assert!(column(&s, 700.0) && !column(&s, 600.0), "the origin moved 100 mm east");
        run(&mut s, "modify", json!({"id": id, "pattern_angle": 30}));
        assert_eq!(angles(&s), [30, 120]);
        // Rotating the region turns its pattern with it.
        run(&mut s, "rotate", json!({"ids": [id], "about": [1500, 1500], "angle": 15}));
        assert_eq!(angles(&s), [45, 135]);
        assert!(s.execute("modify", &json!({"id": id, "pattern_angle": f64::MAX})).is_err());
    }

    /// What `f` did, in [`super::done`]'s steps.
    fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
        let before = super::DONE.with(std::cell::Cell::get);
        (f(), super::DONE.with(std::cell::Cell::get) - before)
    }

    #[test]
    fn dashes_must_draw_something_and_leave_a_space_and_add_up_to_a_line() {
        let with = |dashes: Vec<f64>| {
            FillPattern { lines: vec![HatchLine { dashes, ..HatchLine::simple(0.0, 1.0) }], ..Default::default() }.check("x")
        };
        for bad in [vec![-1.0], vec![-0.05], vec![-1.0, -2.0], vec![1.0, 2.0], vec![0.0, 0.0], vec![0.0, -0.01]] {
            assert!(with(bad.clone()).is_err(), "{bad:?}");
        }
        for good in [vec![0.0, -1.0], vec![2.0, -1.0, 0.0, -1.0], vec![0.0, -0.05], vec![-1.0, 1.0]] {
            assert_eq!(with(good.clone()), Ok(()), "{good:?}");
        }
        // Through the command, so a file or an agent can't make one either.
        let mut s = Session::new();
        let e = s.execute("fill_pattern", &json!({"name": "Gap", "lines": [{"angle": 0, "spacing": 1, "dashes": [-1]}]})).unwrap_err();
        assert!(e.contains("dash"), "{e}");
    }

    #[test]
    fn a_pattern_over_more_than_the_view_shows_draws_only_what_the_view_shows() {
        // Sand at 1 : 100 over 200 m square, seen through a window 20 by 15 m: 200 rows of 200 dots, not 2000 dots in each of them.
        let sand = &default_patterns()["Sand"];
        let window = (P2(90_000.0, 90_000.0), P2(110_000.0, 105_000.0));
        let s = strokes(sand, &[square(0.0, 0.0, 200_000.0)], 100.0, window, 50_000).expect("what the window holds is under the cap");
        assert!((30_000..=50_000).contains(&s.len()), "{}", s.len());
        assert!(s.iter().all(|l| l.iter().all(|q| q.0 >= 90_000.0 - 1e-6 && q.0 <= 110_000.0 + 1e-6)), "none past the window");
    }

    #[test]
    fn a_pattern_too_dense_for_its_view_says_so_and_a_sparser_view_draws_it() {
        let mut s = Session::new();
        let lines: Vec<Value> = (0..32).map(|i| json!({"angle": f64::from(i) * 5.6, "spacing": 100})).collect();
        run(&mut s, "fill_pattern", json!({"name": "Mesh", "target": "model", "lines": lines}));
        run(&mut s, "filled_region_type", json!({"name": "Meshed", "foreground": "Mesh"}));
        run(&mut s, "filled_region", json!({"sketch": lines_of_square(-100_000.0, 200_000.0), "type": "Meshed", "line_style": null}));
        let level = s.model.levels()[0].0;
        let near = crate::render::draw(&s.model, level, View { scale: 0.05, cx: 0.0, cy: 0.0, width: 8192, height: 8192 });
        assert_eq!(near.notes.len(), 1, "{:?}", near.notes);
        assert!(near.notes[0].contains("more lines"), "{}", near.notes[0]);
        let far = crate::render::draw(&s.model, level, View { scale: 0.05, cx: 0.0, cy: 0.0, width: 800, height: 800 });
        assert!(far.notes.is_empty() && far.prims.len() > 1000, "{} prims, {:?}", far.prims.len(), far.notes);
        // The command that draws it says it too.
        let path = std::env::temp_dir().join(format!("bimcraft-dense-{}.svg", std::process::id()));
        let said = run(&mut s, "render", json!({"view": "Level 1", "width": 4096, "height": 4096, "path": path}));
        let _ = std::fs::remove_file(&path);
        assert!(said["notes"][0].as_str().is_some_and(|n| n.contains("more lines")), "{said}");
    }

    fn lines_of_square(x: f64, size: f64) -> Vec<Value> {
        lines(x, x, size)
    }

    #[test]
    fn what_a_drawing_could_not_draw_is_in_the_notes_of_every_reply_that_draws_it() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 3000}));
        let elevation = run(&mut s, "elevation", json!({"facing": "south"}))["id"].as_u64().unwrap();
        let Some(Element::Elevation(e)) = s.model.elements.get(elevation) else { panic!("no elevation") };
        let start = e.start;
        // 32 families of lines 4 mm apart: the view is 0.29 px to the mm, so they are 1.2 px apart, and far more than a view holds.
        let families: Vec<Value> = (0..32).map(|i| json!({"angle": f64::from(i) * 5.6, "spacing": 4})).collect();
        run(&mut s, "fill_pattern", json!({"name": "Mesh", "target": "model", "lines": families}));
        run(&mut s, "filled_region_type", json!({"name": "Meshed", "foreground": "Mesh"}));
        let sketch = lines(-200.0 - start.0, -600.0, 6600.0);
        run(&mut s, "filled_region", json!({"view": elevation, "type": "Meshed", "line_style": null, "sketch": sketch}));
        let said = |r: &Value| r["notes"][0].as_str().is_some_and(|n| n.contains("more lines"));
        // The elevation's reply says it, as the plan's does.
        let r = run(&mut s, "render", json!({"view": elevation, "width": 4096, "height": 2048}));
        assert!(said(&r), "{r}");
        // And the sheet's, as a PNG and as a PDF.
        let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
        run(&mut s, "place", json!({"sheet": "A101", "view": elevation, "at": [200, 150], "scale": 1}));
        let r = run(&mut s, "render", json!({"view": sheet}));
        assert!(said(&r), "the sheet as a PNG: {}", r["notes"]);
        let path = std::env::temp_dir().join(format!("bimcraft-notes-{}.pdf", std::process::id()));
        let r = run(&mut s, "render", json!({"view": sheet, "path": path}));
        let _ = std::fs::remove_file(&path);
        assert!(said(&r), "the sheet as a PDF: {}", r["notes"]);
    }

    #[test]
    fn a_dense_pattern_over_an_outline_of_thousands_of_corners_is_refused_in_time() {
        let p = FillPattern {
            target: Target::Model,
            lines: (0..32).map(|i| HatchLine::simple(f64::from(i) * 5.0, 20.0)).collect(),
            drawn: None,
        };
        let ring: Vec<P2> = (0..10_000)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 10_000.0;
                P2(1e5 * a.cos(), 1e5 * a.sin())
            })
            .collect();
        let (r, steps) = counted(|| strokes(&p, &[ring], 1.0, (P2(-1e5, -1e5), P2(1e5, 1e5)), 50_000));
        assert_eq!(r, None);
        // Refused from the count of lines times the count of edges, before any line was tried against one.
        assert_eq!(steps, 0, "{steps} steps");
    }

    #[test]
    fn dots_a_twentieth_of_a_millimetre_apart_cost_a_bounded_amount() {
        let p = FillPattern {
            target: Target::Model,
            lines: vec![HatchLine { dashes: vec![0.0, -0.05], ..HatchLine::simple(0.0, 10_000.0) }],
            drawn: None,
        };
        assert_eq!(p.check("dots"), Ok(()));
        let (r, steps) = counted(|| strokes(&p, &[square(0.0, 0.0, 100_000.0)], 1.0, (P2(-1.0, -1.0), P2(101_000.0, 101_000.0)), 50_000));
        assert_eq!(r, None);
        assert!(steps <= super::MAX_WORK, "{steps} steps");
    }

    #[test]
    fn a_hatch_keeps_its_spacing_on_the_paper_at_every_scale_and_a_model_pattern_its_own_in_the_building() {
        let mut s = Session::new();
        run(&mut s, "filled_region", json!({"sketch": lines(0.0, 0.0, 4000.0), "type": "Diagonal Up", "line_style": null}));
        run(&mut s, "filled_region", json!({"sketch": lines(6000.0, 0.0, 4000.0), "type": "Brick", "line_style": null}));
        let level = s.model.levels()[0].0;
        let at = |n: f64| {
            let view = View { scale: crate::sheet::DENSITY / n, cx: 5000.0, cy: 2000.0, width: 2000, height: 600 };
            crate::render::draw_at(&s.model, level, view, 1.0)
        };
        // Perpendicular distances between neighbouring lines of the 45 degree hatch, in px, and how many lines each has.
        let hatch = |d: &Drawing| -> Vec<f64> {
            let mut offsets: Vec<f64> = d
                .prims
                .iter()
                .filter_map(|p| match p {
                    Prim::Line { pts, .. } if pts.len() == 2 && (pts[0].0 - pts[1].0).abs() > 1.0 && (pts[0].1 - pts[1].1).abs() > 1.0 => {
                        Some(f64::from(pts[0].0 + pts[0].1) / std::f64::consts::SQRT_2)
                    }
                    _ => None,
                })
                .collect();
            offsets.sort_by(f64::total_cmp);
            offsets.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            offsets
        };
        let (d100, d50) = (hatch(&at(100.0)), hatch(&at(50.0)));
        let gap = |o: &[f64]| o.windows(2).map(|w| w[1] - w[0]).sum::<f64>() / (o.len() - 1) as f64;
        // 1.5 mm of paper is 7.5 px at either scale (and the lines stay 1.5 mm apart on it): so 1 : 50 has twice the lines.
        assert!((gap(&d100) - 7.5).abs() < 0.05 && (gap(&d50) - 7.5).abs() < 0.05, "{} and {}", gap(&d100), gap(&d50));
        assert!((d50.len() as f64 / d100.len() as f64 - 2.0).abs() < 0.1, "{} lines at 1 : 50, {} at 1 : 100", d50.len(), d100.len());
        // Brick's courses are 75 mm of the building, so the same number of them at each scale.
        let courses = |d: &Drawing| -> usize {
            d.prims
                .iter()
                .filter(|p| matches!(p, Prim::Line { pts, .. } if pts.len() == 2 && (pts[0].1 - pts[1].1).abs() < 1e-3 && (pts[0].0 - pts[1].0).abs() > 100.0))
                .count()
        };
        assert_eq!(courses(&at(100.0)), courses(&at(50.0)));
    }

    #[test]
    fn a_region_whose_sketch_no_longer_closes_is_refused_by_the_check() {
        let mut s = Session::new();
        let id = run(&mut s, "filled_region", json!({"sketch": lines(0.0, 0.0, 1000.0)}))["id"].as_u64().unwrap();
        let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
        let sketch = file["elements"][id.to_string()]["sketch"].as_array_mut().unwrap();
        sketch.pop();
        let m = crate::model::Model::from_json(&file.to_string()).unwrap();
        let e = m.check().unwrap_err();
        assert!(e.contains("doesn't close") && e.contains("open"), "{e}");
    }

    #[test]
    fn a_detailing_duplicate_of_a_plan_takes_its_detail_lines_and_regions_into_its_own_view() {
        let mut s = Session::new();
        let line = run(&mut s, "detail_line", json!({"points": [[0, 0], [2000, 0]]}));
        let region =
            run(&mut s, "filled_region", json!({"sketch": lines(0.0, 1000.0, 1000.0), "type": "Diagonal Up"}))["id"].as_u64().unwrap();
        let line = line["id"].as_u64().unwrap();
        let copy = run(&mut s, "duplicate_view", json!({"view": "Level 1", "name": "Detail", "mode": "detailing"}));
        assert_eq!(copy["annotations"], 2);
        let view = copy["id"].as_u64().unwrap();
        let (source, copied) = (s.model.guid(100000), s.model.guid(view));
        let views = |kind: fn(&Element) -> bool, of: crate::ids::Guid| {
            s.model.elements.values().filter(|e| kind(e) && s.model.annotation_plan(e) == Some(of)).count()
        };
        for kind in [|e: &Element| matches!(e, Element::DetailLine(_)), |e: &Element| matches!(e, Element::FilledRegion(_))] {
            assert_eq!((views(kind, source), views(kind, copied)), (1, 1), "one in each view, not two in the source");
        }
        // Drawn once in each: the source's own are the only ones there, and the copy's are its own.
        let owners = |d: &Drawing| -> std::collections::BTreeSet<Id> { d.owners.iter().flatten().copied().collect() };
        let a = owners(&crate::render::draw(&s.model, 100000, View { scale: 0.1, cx: 1000.0, cy: 1000.0, width: 400, height: 300 }));
        let b = owners(&crate::render::draw(&s.model, view, View { scale: 0.1, cx: 1000.0, cy: 1000.0, width: 400, height: 300 }));
        assert!(a.contains(&line) && a.contains(&region), "the source keeps its own");
        assert!(
            !b.contains(&line) && !b.contains(&region) && b.iter().filter(|id| **id > region).count() >= 2,
            "the copy has its own: {b:?}"
        );
    }
}
