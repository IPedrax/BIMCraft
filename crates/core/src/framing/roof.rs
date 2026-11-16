//! A framed roof's rafters, ridge boards, rims, trimmers, headers and blocking, worked out from the roof, its openings and the
//! shafts through it, the walls and beams under it and its framing type whenever they are asked for, and never stored, as a
//! framed floor's joists are ([`super::floor`], whose region framer each of its slopes is framed by).
//!
//! The members stand in the roof type's structure layer (the thickest, where it has several), as the roof's own solid lays
//! its layers: each layer's share of the roof's depth plumb (`Model::roof_rise`, its type's thickness square to the slope), so
//! a rafter is as deep square to the slope as its layer is thick (a 2x10 roof is one with a 235 mm structure layer) and fills
//! that layer from its underside to its top; the layers over it (sheathing, roofing) stay drawn as layers.
//!
//! Each slope (`Model::roof_faces`, numbered as it gives them; a gable's face, which has no area, frames nothing) is framed in
//! a frame of its own: `s` runs up the slope in plan from the first corner of its outline, `across` to its left along the
//! eave; a flat roof's one slope has its rafters square to its outline's longest edge, as a floor's joists are. Every member
//! carries that frame ([`Member::frame`]) and its bottom and top follow the slope along `s`: its ends are plumb, and its cut
//! length is along the slope. In that frame the floor's rules hold, with these:
//! - Rafters run from the eave (the footprint's edge, so the overhang is part of them) to whatever bounds the slope: a ridge
//!   board, a hip or a valley, an upright gable edge at the top of a shed. An edge of the slope along the rafters (or within a
//!   degree of them) on the roof's edge (a rake) has an edge rafter flush with it; one on a hip, a valley or a ridge has none.
//!   Slopes in one plane that share an edge (a point along a straight eave) are one slope ([`seams`]).
//! - With `rim`, a rim (a sub-fascia) along each edge of the slope on the roof's edge square to the rafters (its eaves, and a
//!   shed's high edge), the rafters stopping at its inner face. Along an edge skewed to them neither a rim nor an edge rafter
//!   is put, with a warning, rims or none. A flat roof's courtyard is framed round as an opening is.
//! - With `ridge`, where two slopes meet along a level line at their tops, a ridge board one thickness thick centred on the
//!   line, a level box wholly in the layer: from the rafters' underside on the line (the higher of the two) to the lower of
//!   their tops at its faces, so each rafter's plumb cut is flush with its top, and its bottom half the board's thickness
//!   times the slope below the board's. The rafters of both slopes stop at its faces. Without `ridge`, they run to the line.
//!   An eave board or a ridge board longer than the longest `stock` length is spliced over the rafters that end on it, as a
//!   floor's rim is over its joists ([`super::floor::spliced_rim`]).
//! - No hip or valley rafter is made: the jack rafters end at the line, square where their nearer corner meets it, and each
//!   hip, valley or other line where slopes meet but a ridge (where a joined roof meets the one it joins) gives one warning.
//! - A wall under the roof bears the rafters where its top, at either face of it, is within [`BEARING`] of their underside,
//!   or at its centre line within that and half its width along the rafters times the slope (a level beam's top under a
//!   sloping rafter); a beam the same. A line of rafters the longest `stock` length holds is one piece, eave to top,
//!   continuous over what bears it. A longer one is spliced over what bears it into the fewest pieces that length holds,
//!   each measured along the slope with its laps, and of those splits the one whose longest piece is shortest (ties: the
//!   splices nearest the eave), the pieces butting on a support's centre line (`lap` 0) or each running `lap` past it, every
//!   other one a thickness over beside the one before. Where no split keeps every piece within it, the split whose longest
//!   piece is shortest, with a warning of each piece too long. `span` is the longest clear span between what bears a
//!   rafter, in plan.
//! - Openings and the shafts through a slope are framed as a floor's are: trimmer rafters each side, headers across each end,
//!   tail rafters between; rows of blocking (`bridging`) between neighbouring rafters, square to them.
//!
//! Keys are each slope's: `face/<i>/rafter/<station>/<piece>`, `face/<i>/edge/<edge>/<piece>`, `face/<i>/rim/<edge>`,
//! `face/<i>/opening/<index or shaft GUID>/…`, `face/<i>/blocking/…`, a ridge board `face/<i>/ridge/<edge>` (of the lower
//! numbered of its two slopes, its edge counted round that slope's loops); a spliced rim's or ridge board's pieces add `/<n>`. An edge is counted round its slope's outline as
//! `roof_faces` gives it, so an edit to the footprint that changes how the slopes meet (a point added, a gable) may number
//! the slopes and their edges anew; stations are counted from the slope's least `across` and stay while it does.

use super::floor::{BEARING, Hole, MAX_BLOCKS, Region, Splice, Support, Top, Words, ccw, gather, meets, region, spliced_rim, strip};
use super::{EPS, Framing, Grain, MAX_STATIONS, Member, Role};
use crate::geom::{self, P2};
use crate::model::{Element, Function, Id, Model, Roof, TOL};
use crate::walls::{self, Plane};

const RAFTERS: Words = Words { role: Role::Rafter, one: "rafter", many: "rafters", host: "slope" };

/// One of a roof's slopes as it is framed: its number in `roof_faces`, its loops in plan (its outline counter-clockwise,
/// then its holes), the plane of the roof's underside there, and its frame: `origin`, and `x` the way `s` runs, up the slope.
pub struct Face {
    pub index: usize,
    pub loops: Vec<Vec<P2>>,
    pub plane: Plane,
    pub origin: P2,
    pub x: P2,
}

impl Face {
    /// How steep it is, degrees.
    pub fn slope(&self) -> f64 {
        self.plane.g.length().atan().to_degrees()
    }

    /// Plan point `q` in its frame: `s` along `x`, `across` to its left.
    fn local(&self, q: P2) -> P2 {
        let d = q - self.origin;
        P2(d.dot(self.x), d.dot(self.x.perp()))
    }

    /// The plan point `p` in its frame stands for.
    fn plan(&self, p: P2) -> P2 {
        self.origin + self.x * p.0 + self.x.perp() * p.1
    }
}

/// How far up from the roof's underside its type's thickest structure layer (the first of the thickest) lies, as shares of
/// the roof's plumb depth: its index, and its underside's and top's shares. None without one.
pub(crate) fn structure(m: &Model, r: &Roof) -> Option<(usize, [f64; 2])> {
    let layers = m.type_layers("roof", &r.type_name);
    let (k, _) = layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.function == Function::Structure)
        .max_by(|(i, a), (j, b)| a.thickness.total_cmp(&b.thickness).then(j.cmp(i)))?;
    let shares = shares(m, r);
    Some((k, [*shares.get(k + 1)?, *shares.get(k)?]))
}

/// Where each of roof `r`'s layers starts, from its top down, as a share of its plumb depth over its underside (1 its top),
/// and its underside last (0): as its 3D solid lays them. None without layers.
pub(crate) fn shares(m: &Model, r: &Roof) -> Vec<f64> {
    let layers = m.type_layers("roof", &r.type_name);
    let total: f64 = layers.iter().map(|l| l.thickness).sum();
    if !total.is_finite() || total <= 0.0 {
        return Vec::new();
    }
    let mut above = 0.0;
    std::iter::once(1.0)
        .chain(layers.iter().map(|l| {
            above += l.thickness;
            1.0 - above / total
        }))
        .collect()
}

/// Roof `r`'s structure layer, where it is framed and can be: its members stand in it, and its own solid there is drawn only
/// where a view hides Roofs > Framing.
pub(crate) fn framed(m: &Model, r: &Roof) -> Option<(usize, [f64; 2])> {
    r.framing.as_ref()?;
    check(m, r).ok()?;
    structure(m, r)
}

/// The material framed roof `r`'s framing type names for its members, if it names one: else they're its structure layer's.
pub(crate) fn material<'a>(m: &'a Model, r: &Roof) -> Option<&'a str> {
    m.types.framing.get(r.framing.as_ref()?)?.material.as_deref()
}

/// The piece of framed roof `r`'s slab over plan point `p` whose underside slopes as `g`: that underside, and how far the
/// roof's top lies over it, plumb. What a piece of its solid cut at its structure layer lies on, so its layers can be found.
pub(crate) fn under_at(m: &Model, r: &Roof, p: P2, g: P2) -> Option<(Plane, f64)> {
    m.roof_prisms(r)
        .into_iter()
        .filter(|(ring, bottom, _)| (bottom.g - g).length() < 1e-9 && geom::contains(ring, p))
        .map(|(_, bottom, top)| (bottom, top.at(p) - bottom.at(p)))
        .next()
}

/// Roof `r`'s slopes as they are framed, those with no area (a gable's) left out, and those in one plane that share an edge
/// (a seam, where its footprint has a point along a straight eave) made one, numbered as the first ([`seams`]).
pub fn faces(m: &Model, r: &Roof) -> Result<Vec<Face>, String> {
    let mut faces: Vec<(usize, Vec<Vec<P2>>, Plane)> = m
        .roof_faces(r)?
        .into_iter()
        .enumerate()
        .filter_map(|(index, (loops, plane))| {
            let outline = loops.first()?;
            if outline.len() < 3 || geom::area(outline).abs() < 1.0 {
                return None;
            }
            let loops =
                loops.into_iter().map(ccw).enumerate().map(|(i, l)| if i == 0 { l } else { l.into_iter().rev().collect() }).collect();
            Some((index, loops, plane))
        })
        .collect();
    seams(&mut faces);
    Ok(faces
        .into_iter()
        .filter_map(|(index, loops, plane)| {
            let outline = loops.first()?;
            let x = if plane.g.length() > 1e-9 {
                plane.g.unit()
            } else {
                // Level: square to its outline's longest edge (the first of the longest), as a floor's joists.
                let mut longest = (0.0, P2(1.0, 0.0));
                for (a, b) in geom::ring_edges(outline) {
                    if a.dist(b) > longest.0 {
                        longest = (a.dist(b), b - a);
                    }
                }
                let t = (longest.1.1.atan2(longest.1.0).to_degrees() + 90.0).rem_euclid(180.0).to_radians();
                P2(t.cos(), t.sin())
            };
            let origin = outline.first().copied().unwrap_or_default();
            Some(Face { index, loops, plane, origin, x })
        })
        .collect())
}

/// Sloped slopes (each one loop, counter-clockwise) in one plane that share an edge, merged into the first, which is then
/// one slope framed as one: no line is between them. Then each corner a slope has in a straight line, where every slope that
/// has that corner has it so (the ends of a seam, a footprint's point along a straight eave), taken out, so the slopes meet
/// edge to edge at a ridge as they would without it.
fn seams(faces: &mut Vec<(usize, Vec<Vec<P2>>, Plane)>) {
    let sloped = |f: &(usize, Vec<Vec<P2>>, Plane)| f.1.len() == 1 && f.2.g.length() > 1e-9;
    let boxed = |r: &[P2]| {
        r.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |[x0, y0, x1, y1], q| {
            [x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1)]
        })
    };
    let mut merged = false;
    let mut a = 0;
    while a < faces.len() {
        let joined = faces.get(a).filter(|f| sloped(f)).and_then(|fa| {
            let ra = fa.1.first()?;
            let ba = boxed(ra);
            faces.iter().enumerate().skip(a + 1).find_map(|(b, fb)| {
                let rb = fb.1.first()?;
                let bb = boxed(rb);
                let near = ba[0] <= bb[2] + TOL && bb[0] <= ba[2] + TOL && ba[1] <= bb[3] + TOL && bb[1] <= ba[3] + TOL;
                let probe = ra.first().copied().unwrap_or_default();
                let flat = (fa.2.g - fb.2.g).length() < 1e-9 && (fa.2.at(probe) - fb.2.at(probe)).abs() < 0.01;
                if !(near && sloped(fb) && flat) {
                    return None;
                }
                let (na, nb) = (ra.len(), rb.len());
                (0..na).find_map(|i| {
                    let (p, q) = (ra[i], ra[(i + 1) % na]);
                    let j = (0..nb).find(|j| rb[*j].dist(q) <= TOL && rb[(j + 1) % nb].dist(p) <= TOL)?;
                    // Round the first from the edge's end to its start, then round the second from past its start to its end.
                    let ring: Vec<P2> = (1..=na).map(|k| ra[(i + k) % na]).chain((2..nb).map(|k| rb[(j + k) % nb])).collect();
                    Some((b, ring))
                })
            })
        });
        match joined {
            Some((b, ring)) => {
                if let Some(f) = faces.get_mut(a) {
                    f.1 = vec![ring];
                }
                faces.remove(b);
                merged = true;
            }
            None => a += 1,
        }
    }
    if !merged {
        return;
    }
    // Every corner, by slope: whether it lies in a straight line between its neighbours (or folds back on itself).
    let straight = |r: &[P2], i: usize| {
        let n = r.len();
        let (p, v, q) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
        p.dist(q) <= TOL || geom::dist_to_segment(v, p, q) <= 0.01
    };
    let corners: Vec<(P2, bool)> = faces
        .iter()
        .filter(|f| sloped(f))
        .flat_map(|f| f.1.first().map(|r| (0..r.len()).map(|i| (r[i], straight(r, i))).collect::<Vec<_>>()).unwrap_or_default())
        .collect();
    for f in faces.iter_mut().filter(|f| sloped(f)) {
        let Some(r) = f.1.first_mut() else { continue };
        loop {
            let n = r.len();
            // A corner twice over (where a seam was more than one edge), or one in a straight line on every slope with it.
            let gone = (0..n).find(|i| {
                n > 3
                    && (r[*i].dist(r[(i + 1) % n]) <= EPS
                        || straight(r, *i) && corners.iter().filter(|(c, _)| c.dist(r[*i]) <= TOL).all(|(_, s)| *s))
            });
            let Some(i) = gone else { break };
            r.remove(i);
        }
    }
}

/// What a framed roof must be, which `Model::check` asks of each: of a known framing type, by footprint and unshaped, of a type
/// with a structure layer, its slopes found, and none wider across its rafters than [`MAX_STATIONS`] spacings, their rows of
/// blocking no more than [`MAX_BLOCKS`] blocks. Nothing an opening, a support or a hip does is refused: what can't be framed
/// is warned of.
pub(crate) fn check(m: &Model, r: &Roof) -> Result<(), String> {
    let Some(name) = &r.framing else { return Ok(()) };
    let t = m.types.framing.get(name).ok_or_else(|| {
        let names: Vec<&str> = m.types.framing.keys().map(String::as_str).collect();
        format!("framing type `{name}` doesn't exist (there are: {}); make it with `type` {{category: framing, name}}", names.join(", "))
    })?;
    if r.extrusion.is_some() {
        return Err("only a roof by footprint is framed, and this one is by extrusion: draw it by its footprint (`roof` {boundary, \
                    slope, gables}) or `unframe` it"
            .into());
    }
    if !r.shape.is_empty() {
        return Err("only a roof by footprint is framed, and this one's shape is edited (points or split lines on its top): reset \
                    its shape or `unframe` it"
            .into());
    }
    structure(m, r).ok_or_else(|| {
        format!(
            "its type `{}` has no structure layer to frame: give the type a layer of function structure (`type` {{category: roof, \
             layers}}) or `unframe` it",
            r.type_name
        )
    })?;
    for f in faces(m, r).map_err(|e| format!("its slopes can't be worked out to frame: {e}"))? {
        let local: Vec<P2> = f.loops.iter().flatten().map(|q| f.local(*q)).collect();
        let extent = |k: fn(&P2) -> f64| {
            let (lo, hi) = local.iter().map(k).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
            hi - lo
        };
        let (long, wide) = (extent(|p| p.0), extent(|p| p.1));
        if wide / t.spacing > MAX_STATIONS {
            return Err(format!(
                "framing type `{name}` puts more than {MAX_STATIONS} rafters across its slope {}, {wide:.0} mm wide; frame a narrower \
                 roof or space them wider",
                f.index
            ));
        }
        if t.bridging > 0.0 && (wide / t.spacing + 1.0) * (long / t.bridging + 1.0) > MAX_BLOCKS {
            return Err(format!(
                "framing type `{name}`'s rows of blocking every {} mm make more than {MAX_BLOCKS} blocks in its slope {}; space them \
                 wider (`bridging`) or make it 0",
                t.bridging, f.index
            ));
        }
    }
    Ok(())
}

/// A line where two slopes meet, or where one ends inside the footprint: the slopes' numbers and where each has it as an edge
/// (its loop and its edge in it, counted round all its loops), and its ends in plan.
struct Meeting {
    faces: [(usize, usize); 2],
    ends: [P2; 2],
}

/// Roof `id`'s framing: none when it isn't a framed roof; an error when it can't be framed, as `Model::check` says.
pub fn roof(m: &Model, id: Id) -> Result<Option<Framing>, String> {
    let Some(Element::Roof(r)) = m.elements.get(id) else { return Ok(None) };
    let Some(name) = &r.framing else { return Ok(None) };
    check(m, r)?;
    let (Some(t), Some((k, [f0, f1]))) = (m.types.framing.get(name), structure(m, r)) else { return Ok(None) };
    let layer = m.type_layers("roof", &r.type_name).get(k);
    let faces = faces(m, r)?;
    let (origin, x) = faces.first().map_or((r.boundary.first().copied().unwrap_or_default(), P2(1.0, 0.0)), |f| (f.origin, f.x));
    let mut out = Framing {
        host: m.guid(id),
        type_name: name.clone(),
        layer: layer.map(|l| l.guid).unwrap_or_default(),
        material: t.material.clone().or_else(|| layer.map(|l| l.material.clone())).unwrap_or_default(),
        origin,
        x_axis: x,
        stock: t.stock.clone(),
        members: Vec::new(),
        warnings: Vec::new(),
        supports: Vec::new(),
        span: Some(0.0),
    };
    let th = t.thickness;
    // The footprint's edges, which a slope's edge on the roof's edge lies along: its whole length, over one edge of the
    // footprint or several in line (where a point along a straight eave leaves none between two slopes made one).
    let footprint: Vec<(P2, P2)> = r.edges().collect();
    let on_footprint = |p: P2, q: P2| {
        let near = |x: P2| footprint.iter().any(|(a, b)| geom::dist_to_segment(x, *a, *b) <= TOL);
        let d = q - p;
        let len2 = d.dot(d);
        let mut ts = vec![0.0, 1.0];
        if len2 > 0.0 {
            ts.extend(footprint.iter().map(|(a, _)| (*a - p).dot(d) / len2).filter(|t| *t > 0.0 && *t < 1.0));
        }
        ts.sort_by(f64::total_cmp);
        near(p) && near(q) && ts.windows(2).all(|w| near(p + d * ((w[0] + w[1]) / 2.0)))
    };
    // Each slope's structure layer: its underside's plane, and how deep it is plumb.
    let bands: Vec<(Plane, f64)> = faces
        .iter()
        .map(|f| {
            let rise = m.roof_rise(r, &f.plane);
            (Plane { z: f.plane.z + rise * f0, g: f.plane.g }, rise * (f1 - f0))
        })
        .collect();

    // Where the slopes meet: each edge inside the footprint, paired with the same edge of another slope.
    let mut inner: Vec<(usize, usize, P2, P2)> = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
        let mut j = 0;
        for l in &f.loops {
            for (p, q) in geom::ring_edges(l) {
                if p.dist(q) > EPS && !on_footprint(p, q) {
                    inner.push((fi, j, p, q));
                }
                j += 1;
            }
        }
    }
    let mut meetings: Vec<Meeting> = Vec::new();
    let mut lone: Vec<(usize, usize, P2, P2)> = Vec::new();
    let mut paired = vec![false; inner.len()];
    for a in 0..inner.len() {
        if paired[a] {
            continue;
        }
        let (fa, ja, p, q) = inner[a];
        let twin = (a + 1..inner.len()).find(|b| {
            let (fb, _, c, d) = inner[*b];
            !paired[*b] && fb != fa && ((c.dist(q) <= TOL && d.dist(p) <= TOL) || (c.dist(p) <= TOL && d.dist(q) <= TOL))
        });
        match twin {
            Some(b) => {
                paired[a] = true;
                paired[b] = true;
                meetings.push(Meeting { faces: [(fa, ja), (inner[b].0, inner[b].1)], ends: [p, q] });
            }
            None => lone.push((fa, ja, p, q)),
        }
    }

    // The holes through it in plan: its openings, and the shafts through any of its slopes.
    let mut shafts: Vec<Id> = faces
        .iter()
        .flat_map(|f| {
            let outline = f.loops.first().cloned().unwrap_or_default();
            m.less_shafts(&f.plane.lift(&outline), m.roof_rise(r, &f.plane)).1
        })
        .collect();
    shafts.sort();
    shafts.dedup();
    let holes: Vec<Vec<P2>> = r
        .openings
        .iter()
        .cloned()
        .chain(shafts.iter().filter_map(|sid| match m.elements.get(*sid) {
            Some(Element::Shaft(sh)) => Some(sh.boundary.clone()),
            _ => None,
        }))
        .collect();

    // The ridges, where two slopes rise to a level line; a ridge board along each, where the type has them. The others warned
    // of: no hip or valley rafter is made.
    let mut ridges: Vec<Vec<P2>> = Vec::new();
    let point = |p: P2| format!("({:.0}, {:.0})", p.0, p.1);
    for mt in &meetings {
        let [(fa, ja), (fb, _)] = mt.faces;
        let (Some(a), Some(b)) = (faces.get(fa), faces.get(fb)) else { continue };
        let [p, q] = mt.ends;
        let mid = (p + q) * 0.5;
        let level = (a.plane.at(p) - a.plane.at(q)).abs() <= 0.01;
        // Each slope falling away from the line: a point just inside it lies under the line.
        let falls = |f: &Face| {
            let inside = f
                .loops
                .iter()
                .flat_map(|l| geom::ring_edges(l))
                .find(|(c, d)| (c.dist(p) <= TOL && d.dist(q) <= TOL) || (c.dist(q) <= TOL && d.dist(p) <= TOL));
            inside.is_some_and(|(c, d)| f.plane.at(mid + (d - c).unit().perp()) < f.plane.at(mid) - 1e-9)
        };
        if level && falls(a) && falls(b) {
            if !t.ridge {
                out.warnings.push(format!(
                    "the ridge from {} to {} between slopes {} and {} isn't framed: the framing type has no ridge board (`ridge` \
                     off), so the rafters of both end at it, each against the other's only where they line up; turn `ridge` on or \
                     frame it by hand",
                    point(p),
                    point(q),
                    a.index,
                    b.index
                ));
                continue;
            }
            let (Some(&(ba, da)), Some(&(bb, db))) = (bands.get(fa), bands.get(fb)) else { continue };
            let (pa, qa) = (a.local(p), a.local(q));
            let s = (pa.0 + qa.0) / 2.0;
            let span = [pa.1.min(qa.1), pa.1.max(qa.1)];
            // From the rafters' underside on the line (the higher of the two) up to the lower of their tops at its faces, half
            // its thickness down each slope: wholly in the layer, under the roof's top and over its underside (and so over the
            // top plate of a gable wall under it), each rafter's plumb cut flush with its top.
            let (fa, fb) = (a.plan(P2(s - th / 2.0, (span[0] + span[1]) / 2.0)), a.plan(P2(s + th / 2.0, (span[0] + span[1]) / 2.0)));
            let bottom = ba.at(mid).max(bb.at(mid));
            let top = (ba.at(fa) + da).min(bb.at(fb) + db);
            if top - bottom <= EPS {
                continue;
            }
            // Along the line, clear of the holes through the roof and of the ridge boards before it: in a frame of its own,
            // `u` along it from its start, `v` across it.
            let start = a.plan(P2(s, span[0]));
            let u = a.x.perp();
            let own = |q: &P2| P2((*q - start).dot(u), (*q - start).dot(u.perp()));
            let len = span[1] - span[0];
            let base = vec![P2(0.0, -th / 2.0 - 1.0), P2(len, -th / 2.0 - 1.0), P2(len, th / 2.0 + 1.0), P2(0.0, th / 2.0 + 1.0)];
            let cut: Vec<Vec<P2>> = holes.iter().chain(&ridges).map(|h| h.iter().map(own).collect()).collect();
            let pieces = strip(&base, &cut, [-th / 2.0, th / 2.0]);
            let one = pieces.len() == 1;
            for (j, [u0, u1]) in pieces.into_iter().enumerate() {
                let key = if one { format!("face/{}/ridge/{ja}", a.index) } else { format!("face/{}/ridge/{ja}/{j}", a.index) };
                if u1 - u0 < th - EPS {
                    out.warnings.push(format!(
                        "a piece of {key} {:.0} mm long, shorter than it is thick, is left out: move what cuts it there",
                        u1 - u0
                    ));
                    continue;
                }
                let along = [span[0] + u0, span[0] + u1];
                let mut board = Member::lying(
                    key,
                    (Role::Ridge, Grain::Across),
                    [th, top - bottom],
                    1,
                    [[s - th / 2.0, s + th / 2.0], along],
                    [[bottom; 2], [top; 2]],
                );
                board.frame = Some([a.origin, a.x]);
                ridges.push(vec![
                    a.plan(P2(s - th / 2.0, along[0])),
                    a.plan(P2(s + th / 2.0, along[0])),
                    a.plan(P2(s + th / 2.0, along[1])),
                    a.plan(P2(s - th / 2.0, along[1])),
                ]);
                out.members.push(board);
            }
            continue;
        }
        // A hip where the slopes meet over the roof's outside, a valley where they meet in a fold.
        let inside_a = a
            .loops
            .iter()
            .flat_map(|l| geom::ring_edges(l))
            .find(|(c, d)| c.dist(p) <= TOL && d.dist(q) <= TOL || c.dist(q) <= TOL && d.dist(p) <= TOL);
        let into = inside_a.map_or(mid, |(c, d)| mid + (d - c).unit().perp());
        let what = if (a.plane.g - b.plane.g).length() < 1e-9 && (a.plane.at(mid) - b.plane.at(mid)).abs() < 0.01 {
            "seam"
        } else if b.plane.at(into) > a.plane.at(into) {
            "hip"
        } else {
            "valley"
        };
        out.warnings.push(format!(
            "the {what} from {} to {} between slopes {} and {} isn't framed: no {what} rafter is made, so the rafters of both end at \
             it with nothing there to bear them; put a beam along it (as a {what} rafter) or frame it by hand",
            point(p),
            point(q),
            a.index,
            b.index
        ));
    }
    for (fi, _, p, q) in &lone {
        let Some(f) = faces.get(*fi) else { continue };
        let joins = r.join.map_or_else(String::new, |h| format!(" (where it meets roof {})", m.shown(h)));
        out.warnings.push(format!(
            "the line from {} to {} where slope {} ends inside the footprint{joins} isn't framed: its rafters end at it with nothing \
             there to bear them; put a beam along it or frame it by hand",
            point(*p),
            point(*q),
            f.index
        ));
    }

    // Each slope, framed as a region of rafters.
    let mut bearing: Vec<crate::model::Guid> = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
        let Some(&(under, dz)) = bands.get(fi) else { continue };
        let who = format!("slope {}: ", f.index);
        let sec = (1.0 + f.plane.g.dot(f.plane.g)).sqrt();
        let ring: Vec<P2> = f.loops.first().map(|l| l.iter().map(|q| f.local(*q)).collect()).unwrap_or_default();
        // Its holes: its own (a flat roof's courtyards, framed round as openings are), the roof's openings, then the shafts
        // through it, those that reach it.
        let rise = m.roof_rise(r, &f.plane);
        let outline = f.loops.first().cloned().unwrap_or_default();
        let shafts = m.less_shafts(&f.plane.lift(&outline), rise).1;
        let courtyards = f.loops.iter().skip(1).enumerate().map(|(i, c)| (format!("courtyard {i}"), format!("courtyard/{i}"), c.clone()));
        let openings = r.openings.iter().enumerate().map(|(i, o)| (format!("opening {i}"), format!("opening/{i}"), o.clone())).chain(
            shafts.iter().filter_map(|sid| match m.elements.get(*sid) {
                Some(Element::Shaft(sh)) => Some((format!("shaft {sid}"), format!("opening/{}", m.guid(*sid)), sh.boundary.clone())),
                _ => None,
            }),
        );
        let holes: Vec<Hole> = courtyards
            .chain(openings.filter(|(_, _, o)| meets(o, &outline)))
            .map(|(name, key, o)| Hole::new(name, key, o.iter().map(|q| f.local(*q)).collect()))
            .collect();
        // What else its rafters run round: the ridge boards.
        let cuts: Vec<Vec<P2>> = ridges.iter().map(|rg| rg.iter().map(|q| f.local(*q)).collect()).collect();
        // What bears its rafters.
        let (lo, hi) = outline.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(under.at(*q)), hi.max(under.at(*q))));
        let plan = outline.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |[x0, y0, x1, y1], q| {
            [x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1)]
        });
        let local = |q: P2| f.local(q);
        let local_ring: Vec<P2> = ccw(ring.clone());
        let (supports, curved) = gather(m, &local_ring, plan, [lo - 4000.0, hi + 1000.0], [lo - 1000.0, hi + 1000.0], &local);
        for (wid, w) in curved {
            let c = walls::centre(m, w);
            let mid = walls::Axis::of(w).at(walls::Axis::of(w).length() / 2.0, c);
            if (walls::top_over(m, w, mid) - under.at(mid)).abs() <= BEARING && geom::contains(&outline, mid) {
                out.warnings
                    .push(format!("{who}wall {wid} under it is curved: it isn't taken to bear the rafters; check what bears them there"));
            }
        }
        let k = f.plane.g.length();
        let top_at = |sup: &Support, t: f64, q: P2| match &sup.top {
            Top::Beam([z0, z1]) => z0 + (z1 - z0) * t,
            Top::Wall(wid, _) => m.wall(*wid).map_or(f64::NAN, |w| walls::top_over(m, w, q)),
        };
        // Whether support `sup` bears what lies over plan point `q`, `t` of the way along it.
        let bears_at = |sup: &Support, t: f64, q: P2| {
            let [a, b] = sup.plan;
            let n = (b - a).unit().perp() * sup.half;
            let d = (sup.ends[1] - sup.ends[0]).unit();
            let half_along = sup.half / d.1.abs().max(0.1);
            [q + n, q - n].iter().any(|e| (top_at(sup, t, *e) - under.at(*e)).abs() <= BEARING)
                || (top_at(sup, t, q) - under.at(q)).abs() <= BEARING + half_along * k
        };
        let bears = |sup: &Support, a: f64| {
            let (s, t) = sup.crossing(a)?;
            bears_at(sup, t, f.plan(P2(s, a))).then_some(s)
        };
        let along = |sup: &Support| bears_at(sup, 0.5, (sup.plan[0] + sup.plan[1]) * 0.5);
        let zs = |[s0, s1]: [f64; 2]| {
            let [b0, b1] = [s0, s1].map(|s| under.at(f.origin + f.x * s));
            [[b0, b1], [b0 + dz, b1 + dz]]
        };
        let framed = region(&Region {
            t,
            words: RAFTERS,
            prefix: format!("face/{}/", f.index),
            who: who.clone(),
            origin: f.origin,
            x: f.x,
            ring,
            outer_edge: &|p: P2, q: P2| on_footprint(f.plan(p), f.plan(q)),
            skewed_rims: k <= 1e-9,
            holes,
            cuts,
            section: [th, dz / sec],
            z: &zs,
            supports: &supports,
            bears: &bears,
            along: &along,
            splice: Splice::Stock { longest: t.stock.last().copied().unwrap_or(f64::INFINITY), sec },
            lap_edges: true,
            standing: Vec::new(),
        });
        out.members.extend(framed.members.into_iter().map(|mut member| {
            member.frame.get_or_insert([f.origin, f.x]);
            member
        }));
        out.warnings.extend(framed.warnings);
        if framed.bearing.is_empty() && framed.lines > 0 {
            out.warnings.push(format!(
                "{who}nothing bears its rafters: no wall or beam under it has its top at their underside within {BEARING} mm, so they \
                 span {:.0} mm in plan, eave to top: put walls or beams under them, or check the span",
                framed.longest
            ));
        }
        out.span = Some(out.span.unwrap_or(0.0).max(framed.longest));
        bearing.extend(framed.bearing);
    }
    // Each ridge board longer than the longest stock length, spliced over the rafters that end on it, as a rim is: where
    // their top ends' centres lie along it, in its slope's frame.
    let rafters: Vec<P2> = out
        .members
        .iter()
        .filter(|m| m.role == Role::Rafter)
        .flat_map(|m| {
            let [o, x] = out.frame_of(m);
            let c = (m.across[0] + m.across[1]) / 2.0;
            m.s.map(|s| o + x * s + x.perp() * c)
        })
        .collect();
    let members = std::mem::take(&mut out.members);
    for board in members {
        if board.role != Role::Ridge {
            out.members.push(board);
            continue;
        }
        let [o, x] = out.frame_of(&board);
        let near = |p: &P2| {
            let s = (*p - o).dot(x);
            s >= board.s[0] - th - EPS && s <= board.s[1] + th + EPS
        };
        let centres: Vec<f64> = rafters.iter().filter(|p| near(p)).map(|p| (*p - o).dot(x.perp())).collect();
        out.members.extend(spliced_rim(board, (0.0, 0.0), &centres, (th, t.stock.last().copied()), ("", "rafter"), &mut out.warnings));
    }
    bearing.sort();
    bearing.dedup();
    out.supports = bearing;
    Ok(Some(out))
}
