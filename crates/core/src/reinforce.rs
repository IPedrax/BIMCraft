//! Bars by rules: the bars, stirrups and meshes an engineer tells for a host, laid out in its own frame inside the cover. The
//! rules here are geometry only: that what is asked fits the section, with room between the bars. How many bars, how thick and
//! how far apart is the engineer's, not this module's; no standard's table is in it.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::geom::P2;
use crate::model::{Element, MAX_COORD, MAX_TEXT, Model, TOL, heading};
use crate::rebar::{DIAMETERS, GRADE, MAX_BARS};
use crate::view3d::{P3, add, mul, sub, unit};
use crate::walls::Plane;

/// A bar to make: on `layer` of its host's, `diameter` mm thick, of steel `grade`, along `path` (mm, over the host's level).
pub struct Bar {
    pub layer: &'static str,
    pub diameter: f64,
    pub grade: String,
    pub path: Vec<P3>,
}

/// A host's own axes: `a` along it, `b` across and `c` up in its section, from the point `o`. Lengths of a bar are measured
/// along them.
struct Frame {
    o: P3,
    a: P3,
    b: P3,
    c: P3,
}

impl Frame {
    fn at(&self, a: f64, b: f64, c: f64) -> P3 {
        add(add(add(self.o, mul(self.a, a)), mul(self.b, b)), mul(self.c, c))
    }
}

/// How far the concrete reaches past a bar's outer face: at the top and bottom faces of a beam, a footing or a floor (the ends
/// of a column), and all round the rest.
struct Cover {
    top: f64,
    bottom: f64,
    sides: f64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CoverArg {
    All(f64),
    Faces(Faces),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Faces {
    top: Option<f64>,
    bottom: Option<f64>,
    sides: Option<f64>,
}

/// `count` bars of `diameter` mm in a row, with a hook at each end if told.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    count: usize,
    diameter: f64,
    hook: Option<Value>,
}

/// Bars `diameter` mm thick no further apart than `spacing` mm.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pitch {
    diameter: f64,
    spacing: f64,
}

/// Closer stirrups for `length` mm at each end.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ends {
    length: f64,
    spacing: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stirrup {
    diameter: f64,
    spacing: f64,
    ends: Option<Ends>,
    /// The straight tail of each end, mm; none unless told.
    hook: Option<f64>,
}

/// How far a beam's bars run past its ends, mm; flush with a face when 0, a cover short when not told.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeamAnchor {
    start: Option<f64>,
    end: Option<f64>,
}

/// A column's bars: `diameter` mm, `x` of them along each face across its width and `y` along each face across its depth
/// (corners counted), two each unless told.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnBars {
    diameter: f64,
    x: Option<usize>,
    y: Option<usize>,
    /// For a round column: how many, evenly round its circle.
    count: Option<usize>,
    hook: Option<Value>,
}

/// How far a column's bars run past its base and its top, mm: flush when 0, a cover short when not told.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnAnchor {
    below: Option<f64>,
    above: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ties {
    diameter: f64,
    /// The straight tail, mm, each end turns into the section.
    hook: Option<f64>,
}

/// A footing's mesh: bars along its width (`x`, spread across its depth) and along its depth (`y`), the lower layer first, with
/// their ends turned up `hook` mm.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mesh {
    x: Option<Pitch>,
    y: Option<Pitch>,
    hook: Option<f64>,
}

/// A floor's mesh: bars along east (`x`) and along north (`y`), their ends turned `hook` mm up (a bottom mesh) or down (a top one).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Grid {
    x: Option<Pitch>,
    y: Option<Pitch>,
    hook: Option<f64>,
}

/// What every host takes, read before its own: the cover and the steel, and the room between bars.
struct Common {
    cover: Cover,
    grade: String,
    grade_stirrup: String,
    clearance: Option<f64>,
}

/// The parameters of a command, taken by key; what is left over is not a parameter of the host.
struct Args {
    map: Map<String, Value>,
}

impl Args {
    fn new(p: &Value) -> Result<Args, String> {
        let mut map = p.as_object().cloned().ok_or("reinforce: the parameters must be an object")?;
        map.remove("host");
        map.remove("hosts");
        Ok(Args { map })
    }

    fn take<T: DeserializeOwned>(&mut self, key: &str) -> Result<Option<T>, String> {
        match self.map.remove(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => serde_json::from_value(v).map(Some).map_err(|e| format!("reinforce: `{key}`: {e}")),
        }
    }

    fn done(self, host: &str) -> Result<(), String> {
        match self.map.keys().next() {
            Some(k) => Err(format!("reinforce: `{k}` isn't a parameter of {host}")),
            None => Ok(()),
        }
    }
}

fn too_many() -> String {
    format!("reinforce: that is more than the {MAX_BARS} bars a command makes: widen a spacing or use fewer bars")
}

/// `x` mm, over 0 and no more than the model reaches.
fn length(key: &str, x: f64) -> Result<f64, String> {
    if x > 0.0 && x <= MAX_COORD { Ok(x) } else { Err(format!("reinforce: `{key}` must be over 0 and up to {MAX_COORD} mm, got {x}")) }
}

fn diameter(key: &str, d: f64) -> Result<f64, String> {
    let (thin, thick) = DIAMETERS;
    if (thin..=thick).contains(&d) { Ok(d) } else { Err(format!("reinforce: `{key}` diameter {d} mm isn't between {thin} and {thick} mm")) }
}

/// A spacing of bars `d` mm thick: more than they are thick.
fn spacing(key: &str, s: f64, d: f64) -> Result<f64, String> {
    if s > d && s <= MAX_COORD {
        Ok(s)
    } else {
        Err(format!("reinforce: `{key}` spacing {s} mm must be more than the {d} mm diameter, and up to {MAX_COORD}"))
    }
}

fn count(key: &str, n: usize) -> Result<usize, String> {
    if (1..=MAX_BARS).contains(&n) { Ok(n) } else { Err(format!("reinforce: `{key}` count {n} isn't 1 to {MAX_BARS}")) }
}

/// How far a bar runs past a face, mm: 0 or more.
fn reach(key: &str, x: Option<f64>) -> Result<Option<f64>, String> {
    match x {
        Some(v) if !(0.0..=MAX_COORD).contains(&v) => Err(format!("reinforce: `{key}` {v} mm must be 0 or more, up to {MAX_COORD}")),
        other => Ok(other),
    }
}

/// The legs of a bar's two hooks, mm, from `hook`: a length for both, or an object with a length for each end (`ends` names
/// them); none where there is none.
fn legs(key: &str, hook: Option<&Value>, ends: [&str; 2]) -> Result<(f64, f64), String> {
    let bad = || format!("reinforce: `{key}` must be a length in mm, or {{{}, {}}}", ends[0], ends[1]);
    let leg = |v: Option<&Value>| match v {
        None | Some(Value::Null) => Ok(0.0),
        Some(v) => v.as_f64().ok_or_else(bad).and_then(|x| reach(key, Some(x)).map(|x| x.unwrap_or(0.0))),
    };
    match hook {
        None | Some(Value::Null) => Ok((0.0, 0.0)),
        Some(Value::Object(o)) => match o.keys().find(|k| !ends.contains(&k.as_str())) {
            Some(_) => Err(bad()),
            None => Ok((leg(o.get(ends[0]))?, leg(o.get(ends[1]))?)),
        },
        Some(v) => {
            let x = leg(Some(v))?;
            Ok((x, x))
        }
    }
}

/// A bar's points from `a` to `b`, with a leg at either end, `first` and `last` (where it has one).
fn with_legs(first: Option<P3>, a: P3, b: P3, last: Option<P3>) -> Vec<P3> {
    first.into_iter().chain([a, b]).chain(last).collect()
}

fn cover(arg: Option<CoverArg>, own: Option<f64>) -> Result<Cover, String> {
    let (top, bottom, sides) = match arg {
        Some(CoverArg::All(x)) => (Some(x), Some(x), Some(x)),
        Some(CoverArg::Faces(f)) => (f.top, f.bottom, f.sides),
        None => (None, None, None),
    };
    let face = |name: &str, x: Option<f64>| {
        let x = x.or(own).ok_or_else(|| {
            format!("reinforce: no cover for the {name}: give `cover` in mm (a number, or top, bottom and sides), or give the host a Cobrimento parameter")
        })?;
        length("cover", x)
    };
    Ok(Cover { top: face("top", top)?, bottom: face("bottom", bottom)?, sides: face("sides", sides)? })
}

/// `n` places from `lo` to `hi`, evenly; the middle when there is one.
fn spread_n(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if n <= 1 {
        return vec![(lo + hi) / 2.0];
    }
    (0..n).map(|i| lo + (hi - lo) * i as f64 / (n - 1) as f64).collect()
}

/// Places from `lo` to `hi`, the first at one and the last at the other, evenly and no further apart than `step`.
fn spread(lo: f64, hi: f64, step: f64) -> Result<Vec<f64>, String> {
    let span = hi - lo;
    if span <= TOL {
        return Ok(vec![(lo + hi) / 2.0]);
    }
    let gaps = (span / step).ceil();
    if gaps > MAX_BARS as f64 {
        return Err(too_many());
    }
    let gaps = gaps.max(1.0) as usize;
    Ok((0..=gaps).map(|i| lo + span * i as f64 / gaps as f64).collect())
}

/// Where stirrups go from `lo` to `hi`: no further apart than `spacing`, and than `ends.spacing` in the first and last
/// `ends.length` mm when told.
fn stations(lo: f64, hi: f64, spacing: f64, ends: Option<&Ends>) -> Result<Vec<f64>, String> {
    let Some(e) = ends.filter(|e| 2.0 * e.length < hi - lo - TOL) else {
        // Ends that fill the whole span are the whole span.
        let step = ends.map_or(spacing, |e| spacing.min(e.spacing));
        return spread(lo, hi, step);
    };
    let (first, last) = (spread(lo, lo + e.length, e.spacing)?, spread(hi - e.length, hi, e.spacing)?);
    let middle = spread(lo + e.length, hi - e.length, spacing)?;
    let inner = middle.len().saturating_sub(2);
    Ok(first.into_iter().chain(middle.into_iter().skip(1).take(inner)).chain(last).collect())
}

/// A stirrup round `corners` (in order round the section, three or more): a closed loop, or with a `hook` a loop that goes on
/// past its first corner to a tail turned into the section at each end, as the two ends of a stirrup are.
fn stirrup(f: &Frame, t: f64, corners: &[(f64, f64)], hook: f64) -> Vec<P3> {
    let mut pts: Vec<P3> = corners.iter().map(|&(b, c)| f.at(t, b, c)).collect();
    let (Some(&p0), Some(&p1), Some(&last)) = (pts.first(), pts.get(1), pts.last()) else { return pts };
    pts.push(p0);
    if hook <= TOL {
        return pts;
    }
    // Toward the inside from the first corner: halfway between its two sides.
    let tail = add(p0, mul(unit(add(unit(sub(p1, p0)), unit(sub(last, p0)))), hook));
    std::iter::once(tail).chain(pts).chain([tail]).collect()
}

/// A tie from `a` to `b`, with a tail of `hook` mm at each end, turned toward `side` at the first and away from it at the last.
fn tie(a: P3, b: P3, side: P3, hook: f64) -> Vec<P3> {
    if hook <= TOL {
        return vec![a, b];
    }
    let (along, back) = (unit(sub(b, a)), unit(sub(a, b)));
    vec![add(a, mul(unit(add(along, side)), hook)), a, b, add(b, mul(unit(add(back, mul(side, -1.0))), hook))]
}

/// The room a row of `n` bars of `d` mm leaves, at best: they go in `half` mm either side of the middle, and `gap` is the least
/// a bar may be from the next.
fn row(half: f64, n: usize, d: f64, gap: f64, what: &str) -> Result<Vec<f64>, String> {
    if half < 0.0 {
        return Err(format!("reinforce: the section is too narrow for the {what} bars of {d} mm with that cover and stirrup"));
    }
    let places = spread_n(-half, half, n);
    if n > 1 {
        let clear = 2.0 * half / (n - 1) as f64 - d;
        if clear < gap - TOL {
            return Err(format!(
                "reinforce: {n} {what} bars of {d} mm leave {clear:.1} mm between them, less than the {gap} mm clearance: use fewer or thinner bars"
            ));
        }
    }
    Ok(places)
}

/// Bars along a host and the layers they are on, as they are made.
struct Out {
    bars: Vec<Bar>,
}

impl Out {
    fn push(&mut self, layer: &'static str, diameter: f64, grade: &str, path: Vec<P3>) -> Result<(), String> {
        if self.bars.len() >= MAX_BARS {
            return Err(too_many());
        }
        self.bars.push(Bar { layer, diameter, grade: grade.to_string(), path });
        Ok(())
    }
}

/// What a command makes for `host`: the bars, in the host's level.
pub fn bars(m: &Model, host: &Element, p: &Value) -> Result<Vec<Bar>, String> {
    let mut a = Args::new(p)?;
    let own = host.params().and_then(|v| ["Cobrimento", "ConcreteCover"].iter().find_map(|k| v.get(*k))).and_then(Value::as_f64);
    let grade = a.take::<String>("grade")?.unwrap_or_else(|| GRADE.into());
    let grade_stirrup = a.take::<String>("grade_stirrup")?.unwrap_or_else(|| grade.clone());
    if grade.trim().is_empty() || grade_stirrup.trim().is_empty() {
        return Err("reinforce: a `grade` can't be blank".into());
    }
    let clearance = match a.take::<f64>("clearance")? {
        Some(x) => Some(length("clearance", x)?),
        None => None,
    };
    let cv = cover(a.take("cover")?, own)?;
    let x = Common { cover: cv, grade, grade_stirrup, clearance };
    let bars = match host {
        Element::Beam(_) => {
            let g = m.run_geom(host).ok_or("reinforce: the beam has no type")?;
            if g.round {
                return Err("reinforce: a round beam isn't reinforced yet; a rectangular one is".into());
            }
            let along = unit(sub(g.b, g.a));
            let across = unit([-along[1], along[0], 0.0]);
            let up = crate::view3d::cross(along, across);
            let level = m.level_of(host).map_or(0.0, |l| m.elevation(l));
            let f = Frame { o: [g.a[0], g.a[1], g.a[2] - level], a: along, b: across, c: up };
            beam(&f, g.length(), (g.width, g.depth), &x, a)?
        }
        Element::Column(col) | Element::StructuralColumn(col) => {
            let t = m.column_type(host).ok_or("reinforce: the column has no type")?;
            let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
            let u = heading(col.angle);
            let f = Frame {
                o: [col.at.0, col.at.1, bottom - m.elevation(col.level)],
                a: [0.0, 0.0, 1.0],
                b: [u.0, u.1, 0.0],
                c: [-u.1, u.0, 0.0],
            };
            column(&f, top - bottom, (t.width, t.depth), t.round, &x, a)?
        }
        Element::Footing(col) => {
            let t = m.column_type(host).ok_or("reinforce: the footing has no type")?;
            let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
            let u = heading(col.angle);
            let f = Frame {
                o: [col.at.0, col.at.1, bottom - m.elevation(col.level)],
                a: [u.0, u.1, 0.0],
                b: [-u.1, u.0, 0.0],
                c: [0.0, 0.0, 1.0],
            };
            footing(&f, top - bottom, (t.width, t.depth), t.round, &x, a)?
        }
        Element::Floor(fl) => {
            // Its thickness is square to its top, which may slope; the cover and the bars are measured from it.
            let thick = m.types.floor.get(&fl.type_name).map_or(0.0, |t| t.thickness);
            if thick <= TOL {
                return Err("reinforce: the floor's type has no thickness".into());
            }
            slab(fl, &m.floor_top(fl), m.elevation(fl.level), thick, &x, a)?
        }
        other => return Err(format!("reinforce: a {} can't be armed: a beam, a column, a footing or a floor can", other.category())),
    };
    Ok(bars)
}

fn beam(f: &Frame, len: f64, (w, h): (f64, f64), x: &Common, mut a: Args) -> Result<Vec<Bar>, String> {
    let bottom = a.take::<Row>("bottom")?;
    let top = a.take::<Row>("top")?;
    let stir = a.take::<Stirrup>("stirrup")?;
    let skin = a.take::<Pitch>("skin")?;
    let anchor = a.take::<BeamAnchor>("anchor")?;
    a.done("a beam; it takes cover, grade, grade_stirrup, clearance, bottom, top, stirrup, skin and anchor")?;
    if bottom.is_none() && top.is_none() && stir.is_none() {
        return Err("reinforce: nothing to reinforce with: give `bottom` or `top` bars, or a `stirrup`".into());
    }
    let cv = &x.cover;
    let dst = stir.as_ref().map(|s| diameter("stirrup", s.diameter)).transpose()?.unwrap_or(0.0);
    // Inside the stirrups, where the bars go: half the width, and from the lowest centre to the highest.
    let (half_w, c_lo, c_hi) = (w / 2.0 - cv.sides - dst, -h / 2.0 + cv.bottom + dst, h / 2.0 - cv.top - dst);
    if half_w <= 0.0 || c_hi - c_lo <= 0.0 {
        return Err("reinforce: the section is too narrow or too shallow for that cover and stirrup".into());
    }
    let anchor = anchor.unwrap_or(BeamAnchor { start: None, end: None });
    let (start, end) = (reach("anchor start", anchor.start)?, reach("anchor end", anchor.end)?);
    let (s0, s1) = (-start.unwrap_or(-cv.sides), len + end.unwrap_or(-cv.sides));
    if s1 - s0 <= TOL {
        return Err("reinforce: the beam is too short for that cover".into());
    }
    let gap = |d: f64, other: f64| x.clearance.unwrap_or_else(|| d.max(other).max(20.0));
    let mut out = Out { bars: Vec::new() };
    let mut levels = Vec::new();
    let (mut c_bottom, mut c_top) = (c_lo, c_hi);
    for (layer, rows, at) in [("bottom", &bottom, c_lo), ("top", &top, c_hi)] {
        let Some(r) = rows else { continue };
        let (d, n) = (diameter(layer, r.diameter)?, count(layer, r.count)?);
        if d > c_hi - c_lo {
            return Err(format!("reinforce: a {d} mm {layer} bar doesn't fit inside the stirrup, {:.1} mm deep", c_hi - c_lo));
        }
        let c = if layer == "bottom" { at + d / 2.0 } else { at - d / 2.0 };
        levels.push((c, d));
        if layer == "bottom" {
            c_bottom = c;
        } else {
            c_top = c;
        }
        // Hooks turn up from the bottom bars and down from the top ones, and stay inside the stirrup.
        let (h0, h1) = legs(&format!("{layer} hook"), r.hook.as_ref(), ["start", "end"])?;
        let turn = if layer == "bottom" { 1.0 } else { -1.0 };
        for leg in [h0, h1] {
            let tip = c + turn * leg;
            if leg > TOL && !(c_lo - TOL..=c_hi + TOL).contains(&tip) {
                return Err(format!(
                    "reinforce: a {layer} hook of {leg} mm doesn't fit inside the stirrup, which is {:.1} mm deep from the bars",
                    c_hi - c_lo
                ));
            }
        }
        for b in row(half_w - d / 2.0, n, d, gap(d, 0.0), layer)? {
            let leg = |at: f64, leg: f64| (leg > TOL).then(|| f.at(at, b, c + turn * leg));
            out.push(layer, d, &x.grade, with_legs(leg(s0, h0), f.at(s0, b, c), f.at(s1, b, c), leg(s1, h1)))?;
        }
    }
    if let [(low, dl), (high, dh)] = *levels.as_slice() {
        let between = (high - dh / 2.0) - (low + dl / 2.0);
        if between < gap(dl, dh) - TOL {
            return Err(format!(
                "reinforce: the bars at the top and the bottom meet: {between:.1} mm between them, less than the {} mm clearance",
                gap(dl, dh)
            ));
        }
    }
    if let Some(sk) = skin {
        let d = diameter("skin", sk.diameter)?;
        let sp = spacing("skin", sk.spacing, d)?;
        let (lo, hi) = (c_bottom, c_top);
        let side = half_w - d / 2.0;
        if side <= 0.0 {
            return Err("reinforce: the section is too narrow for skin bars with that cover and stirrup".into());
        }
        let rows = ((hi - lo) / sp).ceil();
        if rows > MAX_BARS as f64 {
            return Err(too_many());
        }
        for k in 1..(rows.max(1.0) as usize) {
            let c = lo + (hi - lo) * k as f64 / rows;
            for b in [-side, side] {
                out.push("skin", d, &x.grade, vec![f.at(s0, b, c), f.at(s1, b, c)])?;
            }
        }
    }
    if let Some(st) = stir {
        let sp = spacing("stirrup", st.spacing, dst)?;
        let hook = reach("hook", st.hook)?.unwrap_or(0.0);
        if let Some(e) = &st.ends {
            length("ends length", e.length).map_err(|_| format!("reinforce: `ends` length {} mm must be over 0", e.length))?;
            spacing("ends", e.spacing, dst)?;
        }
        let (bs, lo, hi) = (w / 2.0 - cv.sides - dst / 2.0, c_lo - dst / 2.0, c_hi + dst / 2.0);
        let t0 = cv.sides + dst / 2.0;
        if len - 2.0 * t0 <= TOL {
            return Err("reinforce: the beam is too short for a stirrup inside that cover".into());
        }
        for t in stations(t0, len - t0, sp, st.ends.as_ref())? {
            out.push("stirrup", dst, &x.grade_stirrup, stirrup(f, t, &[(-bs, hi), (bs, hi), (bs, lo), (-bs, lo)], hook))?;
        }
    }
    Ok(out.bars)
}

fn column(f: &Frame, len: f64, (w, d): (f64, f64), round: bool, x: &Common, mut a: Args) -> Result<Vec<Bar>, String> {
    let bars = a.take::<ColumnBars>("bars")?;
    let stir = a.take::<Stirrup>("stirrup")?;
    let ties = a.take::<Ties>("ties")?;
    let anchor = a.take::<ColumnAnchor>("anchor")?;
    a.done("a column; it takes cover, grade, grade_stirrup, clearance, bars, stirrup, ties and anchor")?;
    if bars.is_none() && stir.is_none() {
        return Err("reinforce: nothing to reinforce with: give `bars` or a `stirrup`".into());
    }
    if round && ties.is_some() {
        return Err("reinforce: a round column has no ties: they go across a rectangular section".into());
    }
    let cv = &x.cover;
    let dst = stir.as_ref().map(|s| diameter("stirrup", s.diameter)).transpose()?.unwrap_or(0.0);
    // Inside the stirrups: half the width and half the depth (round, the radius, in the width).
    let (half_b, half_c) = (w / 2.0 - cv.sides - dst, d / 2.0 - cv.sides - dst);
    if half_b <= 0.0 || half_c <= 0.0 {
        return Err("reinforce: the section is too narrow for that cover and stirrup".into());
    }
    let anchor = anchor.unwrap_or(ColumnAnchor { below: None, above: None });
    let (below, above) = (reach("anchor below", anchor.below)?, reach("anchor above", anchor.above)?);
    let (a0, a1) = (-below.unwrap_or(-cv.bottom), len + above.unwrap_or(-cv.top));
    if a1 - a0 <= TOL {
        return Err("reinforce: the column is too short for that cover".into());
    }
    let mut out = Out { bars: Vec::new() };
    // Where the bars are: along the width at each face across the depth, along the depth at each face across the width.
    let mut across: (Vec<f64>, Vec<f64>, f64, f64) = (Vec::new(), Vec::new(), 0.0, 0.0);
    if let Some(r) = bars {
        let dia = diameter("bars", r.diameter)?;
        let gap = x.clearance.unwrap_or_else(|| dia.max(20.0));
        let (h0, h1) = legs("bars hook", r.hook.as_ref(), ["below", "above"])?;
        if round {
            if r.x.is_some() || r.y.is_some() {
                return Err("reinforce: a round column takes `count` bars round its circle, not `x` and `y`".into());
            }
            let n = r.count.ok_or("reinforce: a round column needs `bars` count: how many round its circle")?;
            if !(3..=MAX_BARS).contains(&n) {
                return Err(format!("reinforce: `bars` count {n} isn't 3 to {MAX_BARS}"));
            }
            let rb = half_b - dia / 2.0;
            if rb < 0.0 {
                return Err(format!("reinforce: the section is too narrow for the bars of {dia} mm with that cover and stirrup"));
            }
            let clear = 2.0 * rb * (std::f64::consts::PI / n as f64).sin() - dia;
            if clear < gap - TOL {
                return Err(format!(
                    "reinforce: {n} bars of {dia} mm leave {clear:.1} mm between them round the circle, less than the {gap} mm clearance: use fewer or thinner bars"
                ));
            }
            for leg in [h0, h1] {
                if leg > 2.0 * rb + TOL {
                    return Err(format!(
                        "reinforce: a bars hook of {leg} mm doesn't fit in a section {:.1} mm across between the bars",
                        2.0 * rb
                    ));
                }
            }
            for i in 0..n {
                let angle = std::f64::consts::TAU * i as f64 / n as f64;
                let (b, c) = (rb * angle.cos(), rb * angle.sin());
                // A leg runs straight toward the middle.
                let leg = |at: f64, leg: f64| (leg > TOL).then(|| f.at(at, b * (rb - leg) / rb, c * (rb - leg) / rb));
                out.push("bars", dia, &x.grade, with_legs(leg(a0, h0), f.at(a0, b, c), f.at(a1, b, c), leg(a1, h1)))?;
            }
        } else {
            if r.count.is_some() {
                return Err("reinforce: a rectangular column takes `x` and `y` bars along its faces, not `count`".into());
            }
            let (nx, ny) = (r.x.unwrap_or(2), r.y.unwrap_or(2));
            for (key, n) in [("bars x", nx), ("bars y", ny)] {
                if !(2..=MAX_BARS).contains(&n) {
                    return Err(format!("reinforce: `{key}` is {n} bars along a face; it is 2 (the corners) to {MAX_BARS}"));
                }
            }
            let (bb, cc) = (half_b - dia / 2.0, half_c - dia / 2.0);
            let (xs, ys) = (row(bb, nx, dia, gap, "x face")?, row(cc, ny, dia, gap, "y face")?);
            // Hooks at the foot and the top turn into the section, along the way across the face the bar is on, and stop short of
            // the middle.
            for (leg, room) in [(h0, 2.0 * cc.min(bb)), (h1, 2.0 * cc.min(bb))] {
                if leg > room + TOL {
                    return Err(format!(
                        "reinforce: a bars hook of {leg} mm doesn't fit in a section {room:.1} mm across between the bars"
                    ));
                }
            }
            for c in [-cc, cc] {
                for &b in &xs {
                    let leg = |at: f64, leg: f64| (leg > TOL).then(|| f.at(at, b, c - c.signum() * leg));
                    out.push("bars", dia, &x.grade, with_legs(leg(a0, h0), f.at(a0, b, c), f.at(a1, b, c), leg(a1, h1)))?;
                }
            }
            for b in [-bb, bb] {
                for &c in ys.iter().skip(1).take(ny.saturating_sub(2)) {
                    let leg = |at: f64, leg: f64| (leg > TOL).then(|| f.at(at, b - b.signum() * leg, c));
                    out.push("bars", dia, &x.grade, with_legs(leg(a0, h0), f.at(a0, b, c), f.at(a1, b, c), leg(a1, h1)))?;
                }
            }
            across = (
                xs.into_iter().skip(1).take(nx.saturating_sub(2)).collect(),
                ys.into_iter().skip(1).take(ny.saturating_sub(2)).collect(),
                bb + dia / 2.0,
                cc + dia / 2.0,
            );
        }
    }
    if let Some(st) = stir {
        let sp = spacing("stirrup", st.spacing, dst)?;
        let hook = reach("hook", st.hook)?.unwrap_or(0.0);
        if let Some(e) = &st.ends {
            length("ends length", e.length).map_err(|_| format!("reinforce: `ends` length {} mm must be over 0", e.length))?;
            spacing("ends", e.spacing, dst)?;
        }
        let (bs, cs) = (w / 2.0 - cv.sides - dst / 2.0, d / 2.0 - cv.sides - dst / 2.0);
        // A hoop of a round column is a polygon of 24 sides on the circle.
        let corners: Vec<(f64, f64)> = if round {
            (0..24).map(|k| std::f64::consts::TAU * f64::from(k) / 24.0).map(|angle| (bs * angle.cos(), bs * angle.sin())).collect()
        } else {
            vec![(-bs, cs), (bs, cs), (bs, -cs), (-bs, -cs)]
        };
        let (t0, t1) = (cv.bottom + dst / 2.0, len - cv.top - dst / 2.0);
        if t1 - t0 <= TOL {
            return Err("reinforce: the column is too short for a stirrup inside that cover".into());
        }
        let tied = match ties {
            Some(t) => Some((diameter("ties", t.diameter)?, reach("ties hook", t.hook)?.unwrap_or(0.0))),
            None => None,
        };
        if tied.is_some() && (across.0.is_empty() && across.1.is_empty()) {
            return Err("reinforce: no tie to make: ties go across the section between bars that are not at a corner, so `bars` needs x or y over 2".into());
        }
        let (extra_b, extra_c) = (across.2, across.3);
        for t in stations(t0, t1, sp, st.ends.as_ref())? {
            out.push("stirrup", dst, &x.grade_stirrup, stirrup(f, t, &corners, hook))?;
            if let Some((td, tail)) = tied {
                for &b in &across.0 {
                    out.push("tie", td, &x.grade_stirrup, tie(f.at(t, b, -extra_c), f.at(t, b, extra_c), f.b, tail))?;
                }
                for &c in &across.1 {
                    out.push("tie", td, &x.grade_stirrup, tie(f.at(t, -extra_b, c), f.at(t, extra_b, c), f.c, tail))?;
                }
            }
        }
    } else if ties.is_some() {
        return Err("reinforce: ties go with the stirrups: give a `stirrup` too".into());
    }
    Ok(out.bars)
}

/// Places for a layer of bars `p.diameter` thick from `lo` to `hi`, no further apart than `p.spacing`, with room between.
fn layer_at(lo: f64, hi: f64, p: &Pitch, gap: Option<f64>, what: &str) -> Result<(f64, Vec<f64>), String> {
    let d = diameter(what, p.diameter)?;
    let sp = spacing(what, p.spacing, d)?;
    if hi < lo - TOL {
        return Err(format!("reinforce: the {what} bars don't fit inside that cover"));
    }
    let places = spread(lo, hi, sp)?;
    let need = gap.unwrap_or_else(|| d.max(20.0));
    if let [a, b, ..] = places[..] {
        let clear = b - a - d;
        if clear < need - TOL {
            return Err(format!(
                "reinforce: {what} bars of {d} mm each {:.1} mm leave {clear:.1} mm between them, less than the {need} mm clearance: widen the spacing",
                b - a
            ));
        }
    }
    Ok((d, places))
}

fn footing(f: &Frame, thick: f64, (w, d): (f64, f64), round: bool, x: &Common, mut a: Args) -> Result<Vec<Bar>, String> {
    let mesh = a.take::<Mesh>("mesh")?;
    a.done("a footing; it takes cover, grade, clearance and mesh")?;
    let Some(mesh) = mesh.filter(|m| m.x.is_some() || m.y.is_some()) else {
        return Err("reinforce: nothing to reinforce with: give a `mesh` with `x` or `y` bars".into());
    };
    let hook = reach("hook", mesh.hook)?.unwrap_or(0.0);
    let cv = &x.cover;
    let mut out = Out { bars: Vec::new() };
    // The layers one on the other from the bottom cover up.
    let mut level = cv.bottom;
    for (layer, p, along, across) in [("mesh x", mesh.x, w, d), ("mesh y", mesh.y, d, w)] {
        let Some(p) = p else { continue };
        // Round, a bar stops at the circle, 450 from the middle of a 1000 footing with 50 of cover.
        let reach_ = along / 2.0 - cv.sides;
        let (lo, hi) = (-across / 2.0 + cv.sides + p.diameter / 2.0, across / 2.0 - cv.sides - p.diameter / 2.0);
        let (dia, places) = layer_at(lo, hi, &p, x.clearance, layer)?;
        if reach_ <= TOL {
            return Err("reinforce: the footing is too small for that cover".into());
        }
        let c = level + dia / 2.0;
        level += dia;
        if hook > 0.0 && c + hook + dia / 2.0 > thick - cv.top {
            return Err(format!(
                "reinforce: a hook of {hook} mm turned up from the {layer} bars reaches past the top cover of a footing {thick} mm thick"
            ));
        }
        for q in places {
            let half = if round { (reach_ * reach_ - q * q).max(0.0).sqrt() } else { reach_ };
            if half <= TOL {
                continue;
            }
            // x bars run along the footing's width at q across it; y bars along its depth.
            let end = |s: f64, c: f64| if layer == "mesh x" { f.at(s, q, c) } else { f.at(q, s, c) };
            let mut path = vec![end(-half, c), end(half, c)];
            if hook > 0.0 {
                path.insert(0, end(-half, c + hook));
                path.push(end(half, c + hook));
            }
            out.push(layer, dia, &x.grade, path)?;
        }
    }
    if level + cv.top > thick + TOL {
        return Err(format!("reinforce: the mesh doesn't fit the footing's {thick} mm thickness between the covers"));
    }
    Ok(out.bars)
}

/// The stretches of the line across `c` (along x, else along y) inside `rings` taken together, so a ring inside another is a
/// hole, each shortened by `inset` at both ends.
fn chords(rings: &[&[P2]], c: f64, along_x: bool, inset: f64) -> Vec<(f64, f64)> {
    let key = |p: &P2| if along_x { (p.0, p.1) } else { (p.1, p.0) };
    let mut hits = Vec::new();
    for ring in rings {
        for (i, p) in ring.iter().enumerate() {
            let Some(q) = ring.get((i + 1) % ring.len()) else { continue };
            let ((u0, v0), (u1, v1)) = (key(p), key(q));
            if (v0 <= c) != (v1 <= c) {
                hits.push(u0 + (c - v0) * (u1 - u0) / (v1 - v0));
            }
        }
    }
    hits.sort_by(f64::total_cmp);
    hits.chunks(2).filter_map(|w| if let [a, b] = w { Some((a + inset, b - inset)) } else { None }).filter(|(a, b)| b - a > TOL).collect()
}

/// A floor's mesh. Its top may slope (`plane`, `base` the level's height): each layer is `off` mm below the top face, square to it,
/// which is `off` times the secant of the slope plumb; a leg of a hook runs square to it too.
fn slab(fl: &crate::model::Floor, plane: &Plane, base: f64, thick: f64, x: &Common, mut a: Args) -> Result<Vec<Bar>, String> {
    let bottom = a.take::<Grid>("bottom")?.filter(|g| g.x.is_some() || g.y.is_some());
    let upper = a.take::<Grid>("top")?.filter(|g| g.x.is_some() || g.y.is_some());
    a.done("a floor; it takes cover, grade, clearance, bottom and top")?;
    if bottom.is_none() && upper.is_none() {
        return Err("reinforce: nothing to reinforce with: give a `bottom` or a `top` mesh with `x` or `y` bars".into());
    }
    let cv = &x.cover;
    let (Some((x0, x1)), Some((y0, y1))) = (span(fl.boundary.iter().map(|p| p.0)), span(fl.boundary.iter().map(|p| p.1))) else {
        return Err("reinforce: the floor has no outline".into());
    };
    let mut rings: Vec<&[P2]> = vec![&fl.boundary];
    rings.extend(fl.openings.iter().map(Vec::as_slice));
    // Each layer's distance below the top face, from the bottom face up and from the top face down, x bars before y.
    struct Layer {
        name: &'static str,
        along_x: bool,
        pitch: Pitch,
        off: f64,
        down: bool,
        hook: f64,
    }
    let mut layers: Vec<Layer> = Vec::new();
    let (mut up, mut down) = (cv.bottom, cv.top);
    for (name, grid) in [("bottom", bottom.as_ref()), ("top", upper.as_ref())] {
        let Some(g) = grid else { continue };
        let hook = reach("hook", g.hook)?.unwrap_or(0.0);
        for (axis, p) in [("x", g.x.as_ref()), ("y", g.y.as_ref())] {
            let Some(p) = p else { continue };
            let d = diameter(name, p.diameter)?;
            let layer: &'static str = match (name, axis) {
                ("bottom", "x") => "bottom x",
                ("bottom", _) => "bottom y",
                (_, "x") => "top x",
                _ => "top y",
            };
            let off = if name == "bottom" { thick - up - d / 2.0 } else { down + d / 2.0 };
            if name == "bottom" {
                up += d;
            } else {
                down += d;
            }
            layers.push(Layer {
                name: layer,
                along_x: axis == "x",
                pitch: Pitch { diameter: p.diameter, spacing: p.spacing },
                off,
                down: name == "top",
                hook,
            });
        }
    }
    let widest = layers.iter().map(|l| l.pitch.diameter).fold(0.0, f64::max);
    let gap = x.clearance.unwrap_or_else(|| widest.max(20.0));
    if bottom.is_some() && upper.is_some() {
        let between = (thick - up) - down;
        if between < gap - TOL {
            return Err(format!(
                "reinforce: the meshes at the top and the bottom meet: {between:.1} mm between them, less than the {gap} mm clearance"
            ));
        }
    } else if (bottom.is_some() && up + cv.top > thick + TOL) || (upper.is_some() && down + cv.bottom > thick + TOL) {
        return Err(format!("reinforce: the mesh doesn't fit the floor's {thick} mm thickness between the covers"));
    }
    // The way square to the top, up: the slope leans it, and the secant stretches a distance from it plumb.
    let secant = (1.0 + plane.g.dot(plane.g)).sqrt();
    let normal = [-plane.g.0 / secant, -plane.g.1 / secant, 1.0 / secant];
    let mut out = Out { bars: Vec::new() };
    for l in layers {
        let (lo, hi) = if l.along_x {
            (y0 + cv.sides + l.pitch.diameter / 2.0, y1 - cv.sides - l.pitch.diameter / 2.0)
        } else {
            (x0 + cv.sides + l.pitch.diameter / 2.0, x1 - cv.sides - l.pitch.diameter / 2.0)
        };
        let (dia, places) = layer_at(lo, hi, &l.pitch, x.clearance, l.name)?;
        // A leg turns up from a bottom layer and down from a top one, and stays inside the other face's cover.
        let room = if l.down { thick - cv.bottom - dia / 2.0 - l.off } else { l.off - cv.top - dia / 2.0 };
        if l.hook > room + TOL {
            return Err(format!(
                "reinforce: a hook of {} mm doesn't fit the {} bars, which have {room:.1} mm to the cover on the other face",
                l.hook, l.name
            ));
        }
        let turn = if l.down { -1.0 } else { 1.0 };
        let grade = &x.grade;
        for q in places {
            for (s, e) in chords(&rings, q, l.along_x, cv.sides) {
                let at = |s: f64| {
                    let p = if l.along_x { P2(s, q) } else { P2(q, s) };
                    [p.0, p.1, plane.at(p) - base - l.off * secant]
                };
                let leg = |p: P3| (l.hook > TOL).then(|| add(p, mul(normal, turn * l.hook)));
                let (a, b) = (at(s), at(e));
                out.push(l.name, dia, grade, with_legs(leg(a), a, b, leg(b)))?;
            }
        }
    }
    Ok(out.bars)
}

/// The least and the most of `xs`.
fn span(xs: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    xs.fold(None, |s: Option<(f64, f64)>, x| Some(s.map_or((x, x), |(lo, hi)| (lo.min(x), hi.max(x)))))
}

/// The kinds of host a preset is for.
pub const KINDS: [&str; 4] = ["beam", "column", "footing", "floor"];
/// The most presets a project holds, and the most bytes one's parameters take as JSON.
pub const MAX_PRESETS: usize = 256;
const MAX_PRESET_BYTES: usize = 16 * 1024;

/// A name for a set of the parameters `reinforce` takes for one kind of host (everything but the host itself), kept in the
/// project to arm others alike.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub kind: String,
    pub params: Map<String, Value>,
}

/// The kind of host `e` is, if it is one `reinforce` arms.
pub fn kind_of(e: &Element) -> Option<&'static str> {
    match e {
        Element::Beam(_) => Some("beam"),
        Element::Column(_) | Element::StructuralColumn(_) => Some("column"),
        Element::Footing(_) => Some("footing"),
        Element::Floor(_) => Some("floor"),
        _ => None,
    }
}

/// The parameters a host of `kind` takes besides the ones every host takes.
fn own(kind: &str) -> &'static [&'static str] {
    match kind {
        "beam" => &["bottom", "top", "stirrup", "skin", "anchor"],
        "column" => &["bars", "stirrup", "ties", "anchor"],
        "footing" => &["mesh"],
        _ => &["bottom", "top"],
    }
}

impl Preset {
    /// Whether the preset is one the project may hold, by `name`: a kind of host, parameters of that kind and no other, in a
    /// size the file may carry. It is checked against a host only when it arms one.
    pub fn check(&self, name: &str) -> Result<(), String> {
        if name.trim().is_empty() || name.chars().count() > MAX_TEXT {
            return Err(format!(
                "reinforcement preset `{}`: its name is 1 to {MAX_TEXT} characters",
                name.chars().take(40).collect::<String>()
            ));
        }
        if !KINDS.contains(&self.kind.as_str()) {
            return Err(format!("reinforcement preset `{name}`: `kind` is one of {}, not `{}`", KINDS.join(", "), self.kind));
        }
        let common = ["cover", "grade", "grade_stirrup", "clearance"];
        if let Some(k) = self.params.keys().find(|k| !common.contains(&k.as_str()) && !own(&self.kind).contains(&k.as_str())) {
            return Err(format!(
                "reinforcement preset `{name}`: `{k}` isn't a parameter of a {}: {}, {}",
                self.kind,
                common.join(", "),
                own(&self.kind).join(", ")
            ));
        }
        if serde_json::to_string(&self.params).map_or(usize::MAX, |s| s.len()) > MAX_PRESET_BYTES {
            return Err(format!("reinforcement preset `{name}`: its params take more than {MAX_PRESET_BYTES} bytes"));
        }
        Ok(())
    }
}
