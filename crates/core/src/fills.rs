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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geom::{self, P2};
use crate::ids::Guid;
use crate::language::Language;
use crate::sketch::SketchLine;

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

/// A fill pattern: its lines, in drafting or model lengths; none, it fills solid.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FillPattern {
    #[serde(default)]
    pub target: Target,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<HatchLine>,
}

/// The most line families a pattern has, and dashes a family has.
pub const MAX_FAMILIES: usize = 32;
pub const MAX_DASHES: usize = 16;
/// The closest a family's lines may be, and the furthest apart, in the pattern's lengths; and the longest dash.
pub const MIN_SPACING: f64 = 0.05;
pub const MAX_SPACING: f64 = 100_000.0;

impl FillPattern {
    pub fn is_solid(&self) -> bool {
        self.lines.is_empty()
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
            if !l.dashes.is_empty() && (l.dashes.iter().map(|x| x.abs()).sum::<f64>() < MIN_SPACING || !l.dashes.iter().any(|x| *x < 0.0)) {
                return Err(format!("{fam}: dashes need a space among them (a negative length) and some length in all"));
            }
        }
        Ok(())
    }
}

/// How short a dot draws, in the pattern's lengths.
const DOT: f64 = 0.15;

/// The pattern's strokes over the area inside `rings` (outlines and holes, either way round: inside is inside an odd number
/// of them), each as a straight piece in model mm, `k` model mm to each of the pattern's (the view's scale for a drafting
/// pattern, 1 for a model one); within `window` (its lowest and its highest corner), and no more than `max` of them. None
/// when it would take more, too dense to draw.
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
        let dashes: Vec<f64> = l.dashes.iter().map(|x| x * k).collect();
        let repeat: f64 = dashes.iter().map(|x| x.abs()).sum();
        let mut j = first;
        while j <= last {
            let base = origin + n * (j * step) + u * (j * shift);
            for (t0, t1) in spans(rings, base, u, n) {
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
                        if out.len() > max {
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

/// Where the line through `base` along unit `u` (`n` square to it) is inside `rings`, as runs along it from `base`: it
/// crosses a ring's edge where the edge's ends lie on either side, and between each two crossings it's in or out by turns.
fn spans(rings: &[Vec<P2>], base: P2, u: P2, n: P2) -> Vec<(f64, f64)> {
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
pub const SOLID_FILL: &str = "Solid fill";
/// The filled region type a region takes unless told another, and the one Masking Region takes: in English.
pub const SOLID_BLACK: &str = "Solid Black";
pub const MASKING: &str = "Masking";

/// The fill patterns a project starts with, named in `language`: our own, drafting ones mm on paper and model ones mm in the
/// building.
pub fn patterns(language: Language) -> BTreeMap<String, FillPattern> {
    let drafting = |lines: Vec<HatchLine>| FillPattern { target: Target::Drafting, lines };
    let model = |lines: Vec<HatchLine>| FillPattern { target: Target::Model, lines };
    let s = HatchLine::simple;
    [
        (SOLID_FILL, drafting(Vec::new())),
        ("Diagonal Up", drafting(vec![s(45.0, 1.5)])),
        ("Diagonal Down", drafting(vec![s(135.0, 1.5)])),
        ("Diagonal Crosshatch", drafting(vec![s(45.0, 2.0), s(135.0, 2.0)])),
        ("Crosshatch", drafting(vec![s(0.0, 2.0), s(90.0, 2.0)])),
        ("Horizontal", drafting(vec![s(0.0, 1.5)])),
        ("Vertical", drafting(vec![s(90.0, 1.5)])),
        // A dot every mm, each row half a step along from the last.
        ("Sand", drafting(vec![HatchLine { shift: 0.5, dashes: vec![0.0, -1.0], ..s(0.0, 1.0) }])),
        // Courses 75 mm high; joints every 225 mm, half a brick along from one course to the next.
        (
            "Brick 225 x 75",
            model(vec![
                s(0.0, 75.0),
                HatchLine { angle: 90.0, origin: P2(0.0, 0.0), shift: 75.0, spacing: 112.5, dashes: vec![75.0, -75.0] },
            ]),
        ),
        ("Tile 300", model(vec![s(0.0, 300.0), s(90.0, 300.0)])),
    ]
    .into_iter()
    .map(|(n, p)| (language.word(n).to_string(), p))
    .collect()
}

pub fn default_patterns() -> BTreeMap<String, FillPattern> {
    patterns(Language::English)
}

pub fn is_default_patterns(p: &BTreeMap<String, FillPattern>) -> bool {
    *p == default_patterns()
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

pub fn is_default_region_types(t: &BTreeMap<String, FilledRegionType>) -> bool {
    *t == default_region_types()
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
        let p = FillPattern { target: Target::Model, lines: vec![HatchLine::simple(0.0, 100.0)] };
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
        let bad = FillPattern { target: Target::Drafting, lines: vec![HatchLine::simple(0.0, 0.0)] };
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
}
