//! What an element made from a mass remembers of it, under reserved names in its `params` (as phases and paint are held):
//! a floor `mass_floors` made holds `{mass, level}` under [`FLOOR`], so a second run updates it instead of making another;
//! a wall, floor or roof made by face holds `{mass, face}` under [`FACE`] (an edge number for a wall, `"top"` for a floor or
//! a roof), so `update_by_face` can rebuild it from the mass as it is now.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::geom::{self, P2};
use crate::model::{Element, Guid, Id, Mass, Model};
use crate::view3d::P3;

pub const FLOOR: &str = "Mass Floor";
pub const FACE: &str = "Mass Face";

/// The face of a mass whose footprint edge lies nearest `p` in plan, within `reach` mm: the mass, and the edge's number
/// from its footprint's first point, as `wall_by_face` takes it. What Pick Faces picks.
pub fn face_at(m: &Model, p: P2, reach: f64) -> Option<(Id, usize)> {
    let mut best: Option<(f64, Id, usize)> = None;
    for (id, e) in m.elements.iter() {
        let Element::Mass(ms) = e else { continue };
        let ring = ms.footprint();
        for (i, a) in ring.iter().enumerate() {
            let b = ring[(i + 1) % ring.len()];
            let d = geom::dist_to_segment(p, *a, b);
            if d <= reach && best.is_none_or(|(was, ..)| d < was) {
                best = Some((d, *id, i));
            }
        }
    }
    best.map(|(_, id, i)| (id, i))
}

/// Whether `name` is one of the parameters a mass's elements hold their mark in: no project or family parameter may take it.
pub fn reserved(name: &str) -> bool {
    matches!(name, FLOOR | FACE)
}

/// The GUID a mark holds under `key` (`mass` or `level`): written as one, or, given by a command, an id.
pub fn mark(o: &serde_json::Map<String, Value>, key: &str) -> Option<Guid> {
    serde_json::from_value(o.get(key)?.clone()).ok()
}

/// The masses and levels the marks in `params` name, each given to `f`, which may put another in its place.
pub fn refs_mut(params: &mut BTreeMap<String, Value>, f: &mut dyn FnMut(&mut Guid)) {
    for key in [FLOOR, FACE] {
        let Some(o) = params.get_mut(key).and_then(Value::as_object_mut) else { continue };
        for k in ["mass", "level"] {
            if let Some(mut g) = mark(o, k) {
                f(&mut g);
                o.insert(k.into(), serde_json::json!(g));
            }
        }
    }
}

/// The masses and levels the marks in `params` name.
pub fn refs(params: &BTreeMap<String, Value>) -> Vec<Guid> {
    [FLOOR, FACE]
        .iter()
        .filter_map(|key| params.get(*key)?.as_object())
        .flat_map(|o| ["mass", "level"].into_iter().filter_map(|k| mark(o, k)))
        .collect()
}

/// What an element's mark says, if `key` is where one is held: a mass of the project, and the level or the face of it.
pub(crate) fn check_value(m: &Model, who: &str, key: &str, v: &Value, e: &Element) -> Option<Result<(), String>> {
    reserved(key).then(|| {
        let shape = if key == FLOOR { "{mass, level}" } else { "{mass, face}" };
        let wrong = || format!("{who}: `{key}` is {shape}, not {v}");
        let o = v.as_object().filter(|o| o.len() == 2).ok_or_else(wrong)?;
        let mass = mark(o, "mass").ok_or_else(wrong)?;
        if !matches!(m.elements.get(mass), Some(Element::Mass(_))) {
            return Err(format!("{who}: `{key}` names {mass}, which isn't a mass"));
        }
        if key == FLOOR {
            if !matches!(e, Element::Floor(_)) {
                return Err(format!("{who}: only a floor holds `{key}`"));
            }
            let level = mark(o, "level").ok_or_else(wrong)?;
            m.level(level).map(|_| ()).ok_or_else(|| format!("{who}: `{key}` names {level}, which isn't a level"))
        } else {
            match (e, o.get("face")) {
                (Element::Wall(_), Some(f)) if f.is_u64() => Ok(()),
                (Element::Floor(_) | Element::Roof(_), Some(f)) if f == "top" => Ok(()),
                _ => Err(format!("{who}: `{key}` is {shape}: a wall's face is an edge number, a floor's or a roof's is \"top\", not {v}")),
            }
        }
    })
}

/// The mass and the face a by-face element was made on, if it holds a mark: a `Some` face is an edge number, `None` the top.
pub fn face_of(e: &Element) -> Option<(Guid, Option<usize>)> {
    let o = e.params()?.get(FACE)?.as_object()?;
    Some((mark(o, "mass")?, o.get("face")?.as_u64().and_then(|f| usize::try_from(f).ok())))
}

/// The floors `mass_floors` made from `mass` that still lie on the level their mark names, with that level.
pub fn floors_of(m: &Model, mass: Guid) -> Vec<(Id, Guid)> {
    m.elements
        .iter()
        .filter_map(|(id, e)| {
            let Element::Floor(f) = e else { return None };
            let o = f.params.get(FLOOR)?.as_object()?;
            (mark(o, "mass")? == mass && mark(o, "level")? == f.level).then_some((*id, f.level))
        })
        .collect()
}

/// `params` without a mass's marks: a copy or a split piece is an ordinary element.
pub fn detach(params: &mut BTreeMap<String, Value>) {
    params.remove(FLOOR);
    params.remove(FACE);
}

/// The marks that name a mass in `gone` taken off, since it's deleted: what was made from it is ordinary from then on.
pub fn forget(m: &mut Model, gone: &BTreeSet<Guid>) {
    for params in m.elements.values_mut().filter_map(Element::params_mut) {
        for key in [FLOOR, FACE] {
            if params.get(key).and_then(Value::as_object).and_then(|o| mark(o, "mass")).is_some_and(|mass| gone.contains(&mass)) {
                params.remove(key);
            }
        }
    }
}

/// Element `e`, made on `face` of `ms` (an edge number for a wall, none for the top), rebuilt from the mass as it is now:
/// a wall stands on the edge as tall as the mass, a floor or a roof lies on its top face. What was done to it since
/// (its type, a roof's slope) stays. The reason, when the face is gone.
pub fn follow(e: &mut Element, ms: &Mass, mass: Id, face: Option<usize>) -> Result<(), String> {
    let ring = ms.footprint();
    match (e, face) {
        (Element::Wall(w), Some(f)) => {
            let n = ring.len();
            let (Some(&a), Some(&b)) = (ring.get(f), ring.get((f + 1) % n)) else {
                return Err(format!("was made on face {f} of mass {mass}, which has {n} faces now (0 to {})", n.saturating_sub(1)));
            };
            if !plumb(&ring, &ms.top_ring(), f) {
                return Err(format!("was made on face {f} of mass {mass}, which leans now: a wall stands plumb"));
            }
            // Counter-clockwise, the mass is to the left of its edge; a wall faces out to its left, so it runs the other way.
            (w.start, w.end, w.level, w.base_offset) = (b, a, ms.level, ms.base_offset);
            w.top_level = ms.top_level;
            if ms.top_level.is_none() {
                w.height = ms.height;
            }
            Ok(())
        }
        (Element::Floor(fl), None) => {
            (fl.boundary, (fl.level, fl.offset)) = (ms.top_ring(), ms.top_face());
            Ok(())
        }
        (Element::Roof(r), None) => {
            (r.boundary, (r.level, r.offset)) = (ms.top_ring(), ms.top_face());
            Ok(())
        }
        (e, _) => Err(format!("is a {}, which holds no such face of mass {mass}", e.category())),
    }
}

/// A mass mirrored in place runs its outline round the other way, so its edge `f` of `n` is edge `n - 2 - f` (mod `n`) now:
/// the faces its walls were made on, numbered again, so they follow the same edges.
pub fn mirrored(m: &mut Model, mass: Id) {
    let (Some(Element::Mass(ms)), Some(guid)) = (m.elements.get(mass), m.elements.guid(mass)) else { return };
    let n = ms.boundary.len();
    for params in m.elements.values_mut().filter_map(Element::params_mut) {
        if let Some(o) = params.get_mut(FACE).and_then(Value::as_object_mut)
            && mark(o, "mass") == Some(guid)
            && let Some(f) = o.get("face").and_then(Value::as_u64).and_then(|f| usize::try_from(f).ok()).filter(|f| *f < n)
        {
            o.insert("face".into(), Value::from((2 * n - 2 - f) % n));
        }
    }
}

/// The outline of a lofted mass a fraction `t` of the way up, from `bottom` (0) to `top` (1): each corner that far along its
/// straight way to its twin.
pub fn ring_at(bottom: &[P2], top: &[P2], t: f64) -> Vec<P2> {
    bottom.iter().zip(top).map(|(a, b)| *a + (*b - *a) * t).collect()
}

/// Whether face `f` of a mass stands plumb: its top edge is right over its bottom edge, so a wall of its height fits it.
pub fn plumb(bottom: &[P2], top: &[P2], f: usize) -> bool {
    let n = bottom.len();
    [f, (f + 1) % n].iter().all(|i| matches!((bottom.get(*i), top.get(*i)), (Some(a), Some(b)) if a.dist(*b) <= 1e-6))
}

/// How many strips up its height a side that isn't flat is cut into. A side is flat when its twin edges are parallel (a sheared
/// or a tapered mass), and two triangles are it; one that twists is a curved patch, and each strip of two triangles lies a little
/// inside it. shortcut: the error shrinks as 1/strips (under 2% of the volume for a 45° turn); raise it if a loft needs finer.
const STRIPS: usize = 16;

/// The skin of a lofted mass as triangles turned out: `bottom` at height `z.0`, `top` at `z.1` (both counter-clockwise, twin
/// for twin), a flat side as two triangles, a twisted one as [`STRIPS`] strips of them (fewer when the outline has very many
/// points), and the two caps.
pub fn tris(bottom: &[P2], top: &[P2], z: (f64, f64)) -> Vec<[P3; 3]> {
    let at = |p: P2, z: f64| [p.0, p.1, z];
    let pt = |ring: &[P2], i: usize, z: f64| at(ring.get(i % ring.len().max(1)).copied().unwrap_or_default(), z);
    let mut out = Vec::new();
    for [a, b, c] in geom::triangulate(top) {
        out.push([pt(top, a, z.1), pt(top, b, z.1), pt(top, c, z.1)]);
    }
    for [a, b, c] in geom::triangulate(bottom) {
        out.push([pt(bottom, a, z.0), pt(bottom, c, z.0), pt(bottom, b, z.0)]);
    }
    let n = bottom.len().min(top.len());
    let strips = (20_000 / n.max(1)).clamp(1, STRIPS);
    for i in 0..n {
        let ((b0, b1), (t0, t1)) = ((bottom[i], bottom[(i + 1) % n]), (top[i], top[(i + 1) % n]));
        let flat = (b1 - b0).cross(t1 - t0).abs() <= 1e-9 * (b1 - b0).length() * (t1 - t0).length() + 1e-9;
        let k = if flat { 1 } else { strips };
        // The side's corners a fraction `f` of the way up.
        let at = |f: f64| ((b0 + (t0 - b0) * f, b1 + (t1 - b1) * f), z.0 + (z.1 - z.0) * f);
        for j in 0..k {
            let (((l0, l1), zl), ((h0, h1), zh)) = (at(j as f64 / k as f64), at((j + 1) as f64 / k as f64));
            out.push([[l0.0, l0.1, zl], [l1.0, l1.1, zl], [h1.0, h1.1, zh]]);
            out.push([[l0.0, l0.1, zl], [h1.0, h1.1, zh], [h0.0, h0.1, zh]]);
        }
    }
    out
}

/// Where the upright plane through `p` along unit `u` cuts a skin of triangles, as lines of `(s, z)` points: `s` along the
/// plane from `p`, kept between 0 and `len`, and each line as long as its pieces join end to end.
pub fn section_lines(skin: &[[P3; 3]], p: P2, u: P2, len: f64) -> Vec<Vec<P2>> {
    let side = |q: &P3| u.cross(P2(q[0], q[1]) - p);
    let along = |q: P3| P2(u.dot(P2(q[0], q[1]) - p), q[2]);
    let mut pieces: Vec<(P2, P2)> = Vec::new();
    for t in skin {
        let d = t.each_ref().map(side);
        let mut hits = Vec::new();
        for k in 0..3 {
            let (a, b, da, db) = (t[k], t[(k + 1) % 3], d[k], d[(k + 1) % 3]);
            if (da < 0.0) != (db < 0.0) {
                // The same point whichever triangle reaches it, so pieces meet exactly.
                let ((a, da), (b, db)) = if a <= b { ((a, da), (b, db)) } else { ((b, db), (a, da)) };
                let f = da / (da - db);
                hits.push(along([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]));
            }
        }
        if let [x, y] = hits[..]
            && x.dist(y) > 1e-9
            && x.0.max(y.0) >= 0.0
            && x.0.min(y.0) <= len
        {
            // Only what lies between 0 and `len` along the plane.
            let cut = |from: P2, to: P2, edge: f64| from + (to - from) * ((edge - from.0) / (to.0 - from.0));
            let clip = |a: P2, b: P2| {
                let a = if a.0 < 0.0 {
                    cut(a, b, 0.0)
                } else if a.0 > len {
                    cut(a, b, len)
                } else {
                    a
                };
                let b = if b.0 < 0.0 {
                    cut(b, a, 0.0)
                } else if b.0 > len {
                    cut(b, a, len)
                } else {
                    b
                };
                (a, b)
            };
            pieces.push(clip(x, y));
        }
    }
    let mut lines: Vec<Vec<P2>> = Vec::new();
    while let Some((a, b)) = pieces.pop() {
        let mut line = vec![a, b];
        // From one end, then from the other.
        for _ in 0..2 {
            while let Some(end) = line.last().copied()
                && let Some(i) = pieces.iter().position(|(c, d)| c.dist(end) < 1e-6 || d.dist(end) < 1e-6)
            {
                let (c, d) = pieces.swap_remove(i);
                line.push(if c.dist(end) < 1e-6 { d } else { c });
            }
            line.reverse();
        }
        lines.push(line);
    }
    lines
}

/// What is wrong with lofting between `bottom` and `top`, if anything: they pair up one for one, run the same way round, and the
/// outline between them keeps its area all the way up. (The area is a quadratic of the height, so sixteen steps see it dip to
/// nothing.)
pub fn loft_problem(bottom: &[P2], top: &[P2]) -> Option<String> {
    if top.len() != bottom.len() {
        return Some(format!("its top outline has {} points and its footprint {}; they pair up one for one", top.len(), bottom.len()));
    }
    let base = geom::area(bottom);
    if geom::area(top) * base <= 0.0 {
        return Some("its top outline runs the other way round from its footprint".into());
    }
    (1..16).find_map(|k| {
        let ring = ring_at(bottom, top, f64::from(k) / 16.0);
        (geom::area(&ring) * base <= crate::model::TOL * crate::model::TOL)
            .then(|| format!("lofted between its outlines it collapses {k}/16 of the way up"))
    })
}
