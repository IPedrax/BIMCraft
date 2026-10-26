//! Snapping: where a point picked near the cursor lands. It snaps to what the plan has there (the
//! ends, middles and centres of what's drawn, where two lines cross, the foot of the perpendicular
//! from the last point picked), to a guide through the last point picked at an angle (from east,
//! and from the line before it), to a wall's centre line, or to the nearest 10 mm. The ruler's
//! numbers and graduations are here too, so every front end reads them alike.

use bimcraft_i18n::ui_msg;

use crate::geom::{self, P2};
use crate::graphics::Look;
use crate::model::{Element, Id, Model, TOL};
use crate::walls::Axis;

/// How far off a guide, degrees, a direction may be and still snap to it (less where guides lie
/// closer together than that).
const TOLERANCE: f64 = 4.0;
/// Points with nothing to snap to round to this, mm.
const STEP: f64 = 10.0;
/// The most lines near the cursor that are tried against each other.
const MAX_NEAR: usize = 64;
/// How far past its centre line a wall's face may lie and still be taken for the cursor's, mm: a
/// thicker wall isn't snapped to from farther than the reach plus this.
const MAX_HALF_THICKNESS: f64 = 2500.0;
/// How far over or under a plan's cut plane a scan's point may lie and still be snapped to, mm: where the plan cuts what
/// was scanned.
pub const CUT_BAND: f64 = 100.0;

/// What a point snapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Endpoint,
    Midpoint,
    Intersection,
    Centre,
    Perpendicular,
    /// The nearest point of a wall's centre line.
    Nearest,
    /// A point of a scan where the plan's cut plane cuts it.
    Point,
    /// A point on an angle guide.
    Angle,
    /// Nothing to snap to: the nearest 10 mm.
    Free,
}

impl Kind {
    /// What it's called, in English.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Endpoint => ui_msg!("Endpoint"),
            Kind::Midpoint => ui_msg!("Midpoint"),
            Kind::Intersection => ui_msg!("Intersection"),
            Kind::Centre => "Centre",
            Kind::Perpendicular => ui_msg!("Perpendicular"),
            Kind::Nearest => ui_msg!("Nearest"),
            Kind::Point => "Point Cloud",
            Kind::Angle => ui_msg!("Angle"),
            Kind::Free => "",
        }
    }
}

/// The snaps in force.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snaps {
    pub endpoint: bool,
    pub midpoint: bool,
    pub intersection: bool,
    pub centre: bool,
    pub perpendicular: bool,
    pub nearest: bool,
    /// The points of a scan where the plan's cut plane cuts it.
    pub cloud: bool,
    /// Degrees between the angles a direction snaps to, from east and from the line before it,
    /// from 1 to 180; anything else for none.
    pub angle: f64,
}

impl Snaps {
    /// No snap at all, and no angle steps.
    pub fn none() -> Snaps {
        Snaps {
            endpoint: false,
            midpoint: false,
            intersection: false,
            centre: false,
            perpendicular: false,
            nearest: false,
            cloud: false,
            angle: 0.0,
        }
    }

    /// Just `kind` snapped to (a kind with no switch of its own is none), and no angle steps: what
    /// Revit's snap shortcuts give the next point.
    pub fn only(kind: Kind) -> Snaps {
        let none = Snaps::none();
        match kind {
            Kind::Endpoint => Snaps { endpoint: true, ..none },
            Kind::Midpoint => Snaps { midpoint: true, ..none },
            Kind::Intersection => Snaps { intersection: true, ..none },
            Kind::Centre => Snaps { centre: true, ..none },
            Kind::Perpendicular => Snaps { perpendicular: true, ..none },
            Kind::Nearest => Snaps { nearest: true, ..none },
            Kind::Point => Snaps { cloud: true, ..none },
            Kind::Angle | Kind::Free => none,
        }
    }
}

impl Default for Snaps {
    fn default() -> Snaps {
        Snaps {
            endpoint: true,
            midpoint: true,
            intersection: true,
            centre: true,
            perpendicular: true,
            nearest: true,
            cloud: true,
            angle: 15.0,
        }
    }
}

/// How far from the cursor a snap reaches, mm: to a point, and to a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub point: f64,
    pub line: f64,
}

/// A line through a point, at an angle, that a pick snapped to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub from: P2,
    /// Degrees from east, counter-clockwise, in [0, 360).
    pub angle: f64,
    /// With a line before the last point, the angle between that line and this one, degrees.
    pub corner: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub at: P2,
    pub kind: Kind,
    pub guide: Option<Guide>,
}

/// `a` degrees turned into [0, 360).
fn norm(a: f64) -> f64 {
    let a = a.rem_euclid(360.0);
    if a >= 360.0 { 0.0 } else { a }
}

/// The direction of `d`, degrees from east, counter-clockwise, in [0, 360).
fn bearing(d: P2) -> f64 {
    if d.0 == 0.0 && d.1 == 0.0 { 0.0 } else { norm(d.1.atan2(d.0).to_degrees()) }
}

fn round(p: P2) -> P2 {
    P2((p.0 / STEP).round() * STEP, (p.1 / STEP).round() * STEP)
}

/// `p` to the micron: what a point meant to be exact is, without float noise.
fn micron(p: P2) -> P2 {
    P2((p.0 * 1e3).round() / 1e3, (p.1 * 1e3).round() / 1e3)
}

/// What the plan of a level has to snap to.
struct Scene {
    points: Vec<(P2, Kind)>,
    /// Straight lines: walls' centre lines, and grids.
    lines: Vec<(P2, P2)>,
    /// The walls, with their thickness.
    walls: Vec<(Axis, f64)>,
}

impl Scene {
    /// What `level`'s plan has within `window` mm of `p`, but for what `look` hides: a cheap look at
    /// each element's extent first, so a plan of thousands of walls costs a few microseconds a wall.
    fn of(m: &Model, level: Id, p: P2, window: f64, look: &Look) -> Scene {
        let mut s = Scene { points: Vec::new(), lines: Vec::new(), walls: Vec::new() };
        let level = m.guid(level);
        let above = m.level_above(level);
        let cut = m.plan_range(level).cut;
        // Whether the box round `pts` lies within `window` of `p`.
        let near = |pts: &[P2]| {
            let (lo, hi) = pts.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
                (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
            });
            p.0 >= lo.0 - window && p.0 <= hi.0 + window && p.1 >= lo.1 - window && p.1 <= hi.1 + window
        };
        let ring = |pts: &[P2]| if near(pts) { pts.iter().map(|q| (*q, Kind::Endpoint)).collect::<Vec<_>>() } else { Vec::new() };
        for e in m.elements.iter().filter(|(id, _)| look.shows(**id)).map(|(_, e)| e) {
            match e {
                Element::Wall(w) => {
                    let a = Axis::of(w);
                    // An arc may bulge past its ends: take the box round its circle.
                    let reach = match (a.centre(), a.radius()) {
                        (Some(c), Some(r)) => [c + P2(-r, -r), c + P2(r, r)],
                        _ => [a.start, a.end],
                    };
                    if !near(&reach) || !m.cut_by(w, level) {
                        continue;
                    }
                    s.points.extend([(a.start, Kind::Endpoint), (a.end, Kind::Endpoint), (a.at(a.length() / 2.0, 0.0), Kind::Midpoint)]);
                    match a.centre() {
                        Some(c) => s.points.push((c, Kind::Centre)),
                        None => s.lines.push((a.start, a.end)),
                    }
                    s.walls.push((a, m.thickness(w)));
                }
                Element::Grid(g) if near(&[g.start, g.end]) => {
                    s.points.extend([(g.start, Kind::Endpoint), (g.end, Kind::Endpoint)]);
                    s.lines.push((g.start, g.end));
                }
                Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) if c.level == level && near(&[c.at]) => {
                    s.points.push((c.at, Kind::Centre))
                }
                crate::model::run_arm!(r) if r.level == level && near(&[r.start, r.end]) => {
                    s.points.extend([
                        (r.start, Kind::Endpoint),
                        (r.end, Kind::Endpoint),
                        (r.start + (r.end - r.start) * 0.5, Kind::Midpoint),
                    ]);
                    s.lines.push((r.start, r.end));
                }
                Element::Component(c) if c.level == level && near(&[c.at]) => s.points.push((c.at, Kind::Centre)),
                Element::Floor(f) if f.level == level => {
                    s.points.extend(ring(&f.boundary));
                    f.openings.iter().for_each(|o| s.points.extend(ring(o)));
                }
                Element::Ceiling(c) if c.level == level => {
                    s.points.extend(ring(&c.boundary));
                    c.openings.iter().for_each(|o| s.points.extend(ring(o)));
                }
                Element::Roof(r) if r.level == level || Some(r.level) == above => {
                    s.points.extend(ring(&r.boundary));
                    r.courtyards.iter().chain(&r.openings).for_each(|o| s.points.extend(ring(o)));
                }
                Element::Shaft(sh) if sh.level == level => s.points.extend(ring(&sh.boundary)),
                Element::Mass(ms) if m.mass_cut_by(ms, cut) => s.points.extend(ring(&m.mass_outline(ms, cut))),
                Element::Stair(st) if st.level == level && near(&[st.start]) => s.points.push((st.start, Kind::Endpoint)),
                Element::Ramp(r) if r.level == level && near(&[r.start]) => s.points.push((r.start, Kind::Endpoint)),
                Element::Railing(r) if r.level == level => s.points.extend(ring(&r.path)),
                _ => {}
            }
        }
        s
    }
}

/// Of the points of the scans `look` shows that lie within [`CUT_BAND`] of `level`'s cut plane, the nearest to `p` seen from
/// above, within `reach`.
fn cloud_point(m: &Model, level: Id, p: P2, reach: f64, look: &Look) -> Option<P2> {
    if m.clouds.is_empty() {
        return None;
    }
    let cut = m.plan_range(level).cut;
    m.elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::PointCloud(pc) if look.shows(id) => Some(pc),
            _ => None,
        })
        .filter_map(|pc| {
            let cloud = m.clouds.get(&pc.cloud)?;
            // The cursor in the scan's own terms: from its middle, turned back.
            let base = m.elevation(pc.level) + pc.offset;
            let (sin, cos) = pc.angle.to_radians().sin_cos();
            let d = p - pc.at;
            let local = [d.0 * cos + d.1 * sin, -d.0 * sin + d.1 * cos];
            let q = cloud.nearest(local, reach, (cut - CUT_BAND - base, cut + CUT_BAND - base))?;
            let [x, y, _] = m.cloud_place(pc)(q);
            Some(P2(x, y))
        })
        .min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))
}

/// Where the segments `lines` cross each other.
fn crossings(lines: &[(P2, P2)]) -> Vec<P2> {
    let on = |q: P2, (a, b): (P2, P2)| geom::dist_to_segment(q, a, b) <= TOL;
    let mut out = Vec::new();
    for (i, &(a, b)) in lines.iter().enumerate() {
        for &(c, d) in lines.iter().skip(i + 1) {
            if let Some(q) = geom::line_intersection(a, b, c, d)
                && on(q, (a, b))
                && on(q, (c, d))
            {
                out.push(q);
            }
        }
    }
    out
}

/// Where the ray from `l` along the unit vector `u` meets the segment `a`→`b`.
fn ray_hit(l: P2, u: P2, a: P2, b: P2) -> Option<P2> {
    let e = b - a;
    let (len, den) = (e.length(), u.cross(e));
    if den.abs() <= 1e-12 * len {
        return None;
    }
    let (s, t) = ((a - l).cross(e) / den, (a - l).cross(u) / den);
    (s > 0.0 && t * len >= -TOL && t * len <= len + TOL).then(|| l + u * s)
}

/// The angles, degrees, a direction from the last of `from` snaps to, every `inc`: from east, and
/// from the line before the last point.
fn rays(from: &[P2], inc: f64) -> Vec<f64> {
    // As many as it takes to come round: the last short of 360° where the step doesn't divide it.
    let n = (360.0 / inc - 1e-9).ceil().min(360.0) as usize;
    let mut out: Vec<f64> = (0..n).map(|k| k as f64 * inc).collect();
    if let [.., prev, last] = from
        && prev.dist(*last) > TOL
    {
        let base = bearing(*last - *prev);
        out.extend((0..n).map(|k| norm(base + k as f64 * inc)));
    }
    out
}

/// The angle at `last` between the line back to `prev` and the line on to `next`, degrees: a
/// straight run on is 180°, a right-angled turn 90°; none while either line has no length.
pub fn corner(prev: P2, last: P2, next: P2) -> Option<f64> {
    if prev.dist(last) <= TOL || last.dist(next) <= TOL {
        return None;
    }
    let turn = norm(bearing(next - last) - bearing(last - prev));
    Some(180.0 - if turn > 180.0 { 360.0 - turn } else { turn })
}

/// With a line before the last of `from`, the corner it makes with one going on in direction `d`.
fn corner_on(from: &[P2], d: P2) -> Option<f64> {
    let [.., prev, last] = from else { return None };
    // A step long enough to count as a line, whatever length `d` is.
    corner(*prev, *last, *last + d.unit() * (2.0 * TOL))
}

/// How points rank where two lie as near: an end before a crossing before a middle before a centre.
fn rank(k: Kind) -> u8 {
    match k {
        Kind::Endpoint => 0,
        Kind::Intersection => 1,
        Kind::Midpoint => 2,
        _ => 3,
    }
}

/// Where a point picked at `p` lands on `level`'s plan: what the plan has there, the foot of the
/// perpendicular from the last of `from` (the points this pick follows), a guide through that
/// point at an angle, a wall's centre line, else the nearest 10 mm. `square` keeps the step from
/// the last of `from` horizontal or vertical.
pub fn snap(m: &Model, level: Id, p: P2, reach: Reach, from: &[P2], square: bool, o: &Snaps) -> Found {
    let last = from.last().copied();
    let mut p = p;
    // The coordinate a square step pinned to the last point's: y if it runs along x.
    let mut pinned = None;
    if square && let Some(f) = last {
        if (p.0 - f.0).abs() > (p.1 - f.1).abs() {
            (p.1, pinned) = (f.1, Some((true, f.1)));
        } else {
            (p.0, pinned) = (f.0, Some((false, f.0)));
        }
    }
    let look = Look::of(m, level);
    let scene = Scene::of(m, level, p, reach.point.max(reach.line) + MAX_HALF_THICKNESS, &look);
    let by_distance = |a: &P2, b: &P2| a.dist(p).total_cmp(&b.dist(p));
    // The lines that pass near the cursor: what it can cross, or stand square to.
    let mut near: Vec<(P2, P2)> = scene.lines.iter().copied().filter(|(a, b)| geom::dist_to_segment(p, *a, *b) <= reach.point).collect();
    near.sort_by(|x, y| geom::dist_to_segment(p, x.0, x.1).total_cmp(&geom::dist_to_segment(p, y.0, y.1)));
    near.truncate(MAX_NEAR);

    // What the plan has here: its ends, middles and centres, the points picked so far, crossings.
    let wanted = |k: Kind| match k {
        Kind::Endpoint => o.endpoint,
        Kind::Midpoint => o.midpoint,
        Kind::Centre => o.centre,
        _ => true,
    };
    let mut points: Vec<(P2, Kind)> = scene.points.iter().copied().filter(|(_, k)| wanted(*k)).collect();
    if o.endpoint {
        points.extend(from.iter().map(|q| (*q, Kind::Endpoint)));
    }
    if o.intersection {
        points.extend(crossings(&near).into_iter().map(|q| (q, Kind::Intersection)));
    }
    let hit =
        points.into_iter().filter(|(q, _)| q.dist(p) <= reach.point).min_by(|a, b| by_distance(&a.0, &b.0).then(rank(a.1).cmp(&rank(b.1))));
    if let Some((at, kind)) = hit {
        return Found { at, kind, guide: None };
    }
    // Else a scan's point where the cut plane cuts it.
    if o.cloud
        && let Some(at) = cloud_point(m, level, p, reach.point, &look)
    {
        return Found { at: micron(at), kind: Kind::Point, guide: None };
    }

    // The foot of the perpendicular from the last point.
    if o.perpendicular
        && let Some(l) = last
    {
        let foot = near
            .iter()
            .filter_map(|&(a, b)| {
                let (len, u) = ((b - a).length(), (b - a).unit());
                let t = (l - a).dot(u);
                (len > 0.0 && (0.0..=len).contains(&t)).then(|| a + u * t)
            })
            .filter(|f| f.dist(p) <= reach.point && f.dist(l) > STEP)
            .min_by(by_distance);
        if let Some(f) = foot {
            let guide = Guide { from: l, angle: bearing(f - l), corner: corner_on(from, f - l) };
            return Found { at: micron(f), kind: Kind::Perpendicular, guide: Some(guide) };
        }
    }

    // A guide through the last point at the angle the cursor is nearest.
    if (1.0..=180.0).contains(&o.angle)
        && let Some(l) = last
        && p.dist(l) > 2.0 * STEP
    {
        let d = p - l;
        let tolerance = TOLERANCE.min(0.4 * o.angle);
        let best = rays(from, o.angle)
            .into_iter()
            .map(|a| {
                let u = P2(a.to_radians().cos(), a.to_radians().sin());
                (a, u, u.dot(d), u.cross(d).abs())
            })
            .filter(|(_, _, along, perp)| *along > 0.0 && *perp <= reach.line && (perp / along).atan().to_degrees() <= tolerance)
            .min_by(|x, y| x.3.total_cmp(&y.3));
        if let Some((angle, u, along, _)) = best {
            let guide = Some(Guide { from: l, angle, corner: corner_on(from, u) });
            // Where it crosses a wall or a grid near the cursor.
            let cross = near.iter().filter_map(|&(a, b)| ray_hit(l, u, a, b)).filter(|q| q.dist(p) <= reach.point).min_by(by_distance);
            // Else the grid point that lies on it, if one does, else as long as the nearest 10 mm.
            let grid = round(p);
            let on_grid = u.cross(grid - l).abs() <= 1e-3 && u.dot(grid - l) > 0.0;
            return match cross {
                Some(q) if o.intersection => Found { at: micron(q), kind: Kind::Intersection, guide },
                _ if on_grid => Found { at: grid, kind: Kind::Angle, guide },
                _ => Found { at: micron(l + u * ((along / STEP).round().max(1.0) * STEP)), kind: Kind::Angle, guide },
            };
        }
    }

    // A wall's centre line, to the nearest 10 mm along it from its start, else the nearest 10 mm.
    if o.nearest {
        let on = scene
            .walls
            .iter()
            .map(|(a, t)| (a, a.distance(p, 0.0) - t / 2.0))
            .filter(|(_, d)| *d <= reach.line)
            .min_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((a, _)) = on {
            return Found { at: micron(a.at((a.project(p).0 / STEP).round() * STEP, 0.0)), kind: Kind::Nearest, guide: None };
        }
    }
    // Rounded, but not off the line a square step keeps to.
    let mut at = round(p);
    match pinned {
        Some((true, y)) => at.1 = y,
        Some((false, x)) => at.0 = x,
        None => {}
    }
    Found { at, kind: Kind::Free, guide: None }
}

/// A length, its parts and its angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ruler {
    pub length: f64,
    pub dx: f64,
    pub dy: f64,
    /// Degrees from east, counter-clockwise, in [0, 360).
    pub angle: f64,
}

/// What a ruler laid from `a` to `b` reads.
pub fn ruler(a: P2, b: P2) -> Ruler {
    let d = b - a;
    Ruler { length: d.length(), dx: d.0, dy: d.1, angle: bearing(d) }
}

/// How far apart a ruler's graduations lie, mm, at `px_per_mm`: 1, 2 or 5 times a power of ten,
/// at least 10 px apart.
pub fn graduation(px_per_mm: f64) -> f64 {
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return 100.0;
    }
    let least = 10.0 / px_per_mm;
    let decade = 10f64.powi(least.log10().floor() as i32);
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|k| k * decade).find(|g| *g >= least * (1.0 - 1e-9)).unwrap_or(10.0 * decade)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    const REACH: Reach = Reach { point: 100.0, line: 60.0 };

    fn model(script: &str) -> Model {
        let mut s = Session::new();
        s.script(script).unwrap();
        (*s.model).clone()
    }

    fn at(m: &Model, p: P2, from: &[P2], o: &Snaps) -> Found {
        snap(m, 100000, p, REACH, from, false, o)
    }

    fn degrees(from: P2, to: P2) -> f64 {
        (to.1 - from.1).atan2(to.0 - from.0).to_degrees()
    }

    #[test]
    fn points_snap_to_the_corners_of_a_mass_where_the_plan_cuts_it() {
        let m = model(r#"mass {"boundary": [[1000,1000],[5000,1000],[5000,4000],[1000,4000]], "height": 2000}"#);
        let o = Snaps::default();
        let f = at(&m, P2(5030.0, 4020.0), &[], &o);
        assert_eq!((f.at, f.kind), (P2(5000.0, 4000.0), Kind::Endpoint));
        // Level 2's plan cuts at 4200 mm, over its top at 2000: nothing there to snap to.
        let f = snap(&m, 100001, P2(5030.0, 4020.0), REACH, &[], false, &o);
        assert_eq!((f.at, f.kind), (P2(5030.0, 4020.0), Kind::Free));
    }

    #[test]
    fn points_snap_to_the_corners_of_a_lofted_mass_at_the_height_the_plan_cuts_it() {
        let m = model(
            r#"mass {"boundary": [[0,0],[10000,0],[10000,6000],[0,6000]], "height": 6000, "top": [[2000,2000],[8000,2000],[8000,4000],[2000,4000]]}"#,
        );
        // Level 1's plan cuts 1200 mm up, a fifth of the way to the top outline: the corner is at 400, 400.
        let f = at(&m, P2(430.0, 380.0), &[], &Snaps::default());
        assert_eq!((f.at.0.round(), f.at.1.round(), f.kind), (400.0, 400.0, Kind::Endpoint));
    }

    #[test]
    fn points_snap_to_the_ends_middles_and_centres_of_walls() {
        let m = model(
            r#"wall {"points": [[0,0],[6000,0]]}
               wall {"points": [[0,5000],[4000,5000]], "arc": 180}"#,
        );
        let o = Snaps::default();
        let f = at(&m, P2(30.0, -20.0), &[], &o);
        assert_eq!((f.at, f.kind), (P2(0.0, 0.0), Kind::Endpoint));
        let f = at(&m, P2(3040.0, 30.0), &[], &o);
        assert_eq!((f.at, f.kind), (P2(3000.0, 0.0), Kind::Midpoint));
        // An arc's centre, and the middle of its length.
        let f = at(&m, P2(2030.0, 4980.0), &[], &o);
        assert_eq!((f.at, f.kind), (P2(2000.0, 5000.0), Kind::Centre));
        let f = at(&m, P2(2030.0, 3020.0), &[], &o);
        assert_eq!((f.kind, (f.at.dist(P2(2000.0, 5000.0)) * 1e3).round() / 1e3), (Kind::Midpoint, 2000.0));
        // Beyond their reach, a wall's centre line, then the nearest 10 mm.
        let f = at(&m, P2(1234.0, 40.0), &[], &o);
        assert_eq!((f.at, f.kind), (P2(1230.0, 0.0), Kind::Nearest));
        let f = at(&m, P2(1234.0, 900.0), &[], &o);
        assert_eq!((f.at, f.kind), (P2(1230.0, 900.0), Kind::Free));
    }

    #[test]
    fn walls_snap_where_they_cross_and_the_points_picked_so_far_are_ends() {
        let m = model(
            r#"wall {"points": [[0,0],[6000,0]]}
               wall {"points": [[2000,-1000],[2000,3000]]}"#,
        );
        let f = at(&m, P2(2040.0, 30.0), &[], &Snaps::default());
        assert_eq!((f.at, f.kind), (P2(2000.0, 0.0), Kind::Intersection));
        // Closing a loop: the first point picked.
        let f = at(&m, P2(5030.0, 5020.0), &[P2(5000.0, 5000.0), P2(5000.0, 4000.0), P2(3000.0, 4000.0)], &Snaps::default());
        assert_eq!((f.at, f.kind), (P2(5000.0, 5000.0), Kind::Endpoint));
    }

    #[test]
    fn a_point_snaps_to_the_foot_of_the_perpendicular_from_the_last_one() {
        let m = model(r#"wall {"points": [[0,0],[6000,2000]]}"#);
        // 14e6 / 40e6 along the wall: (2100, 700), straight down no multiple of 15°.
        let f = at(&m, P2(2130.0, 680.0), &[P2(1000.0, 4000.0)], &Snaps::default());
        assert_eq!(f.kind, Kind::Perpendicular);
        assert!(f.at.dist(P2(2100.0, 700.0)) < 1e-6, "{:?}", f.at);
        let g = f.guide.unwrap();
        assert_eq!(g.from, P2(1000.0, 4000.0));
        assert!((g.angle - degrees(P2(1000.0, 4000.0), P2(2100.0, 700.0)).rem_euclid(360.0)).abs() < 1e-6, "{}", g.angle);
        // Not there once it's off.
        let f = at(&m, P2(2130.0, 680.0), &[P2(1000.0, 4000.0)], &Snaps { perpendicular: false, ..Snaps::default() });
        assert_ne!(f.kind, Kind::Perpendicular);
    }

    #[test]
    fn directions_snap_to_the_angle_from_east_and_from_the_line_before() {
        let m = model("");
        let o = Snaps::default();
        // 30.2° from the first point: onto 30°, its length to the nearest 10 mm.
        let f = at(&m, P2(3470.0, 2020.0), &[P2(0.0, 0.0)], &o);
        assert_eq!(f.kind, Kind::Angle);
        assert!((degrees(P2(0.0, 0.0), f.at) - 30.0).abs() < 1e-4, "{:?}", f.at);
        assert!((f.at.length() / 10.0 - (f.at.length() / 10.0).round()).abs() < 1e-3, "{}", f.at.length());
        let g = f.guide.unwrap();
        assert_eq!((g.from, g.angle.round(), g.corner), (P2(0.0, 0.0), 30.0, None));
        // 22.5° is between guides: free.
        let f = at(&m, P2(3000.0, 1243.0), &[P2(0.0, 0.0)], &o);
        assert_eq!((f.kind, f.guide), (Kind::Free, None));
        // Off: free.
        let f = at(&m, P2(3470.0, 2020.0), &[P2(0.0, 0.0)], &Snaps { angle: 0.0, ..o });
        assert_eq!((f.at, f.kind), (P2(3470.0, 2020.0), Kind::Free));
        // From the line before: after a wall at 20°, 90° to it is 110°, no multiple of 15°.
        let (prev, last) = (P2(0.0, 0.0), P2(20f64.to_radians().cos(), 20f64.to_radians().sin()) * 3000.0);
        let want = last + P2(110f64.to_radians().cos(), 110f64.to_radians().sin()) * 2000.0;
        let f = at(&m, want + P2(25.0, 10.0), &[prev, last], &o);
        assert_eq!(f.kind, Kind::Angle);
        assert!((degrees(last, f.at) - 110.0).abs() < 1e-3, "{:?}", f.at);
        assert!(f.guide.and_then(|g| g.corner).is_some_and(|c| (c - 90.0).abs() < 1e-9), "{:?}", f.guide);
    }

    #[test]
    fn a_point_on_the_grid_that_lies_on_the_guide_stays_where_it_is() {
        let m = model("");
        // 225° from (4000, 0) runs through (2000, -2000), a whole 2828.4 mm out: not rounded to 2830.
        let f = at(&m, P2(2000.0, -2000.0), &[P2(4000.0, 0.0)], &Snaps::default());
        assert_eq!((f.at, f.kind), (P2(2000.0, -2000.0), Kind::Angle));
        // Near it, the same; where no grid point lies on the guide, its length goes to the nearest 10 mm.
        let f = at(&m, P2(2003.0, -2003.0), &[P2(4000.0, 0.0)], &Snaps::default());
        assert_eq!((f.at, f.kind), (P2(2000.0, -2000.0), Kind::Angle));
        let f = at(&m, P2(3470.0, 2020.0), &[P2(0.0, 0.0)], &Snaps::default());
        assert!((f.at.length() % 10.0).min(10.0 - f.at.length() % 10.0) < 1e-3, "{:?}", f.at);
    }

    #[test]
    fn a_guide_ends_where_it_crosses_a_wall() {
        let m = model(r#"wall {"points": [[3000,-3000],[3000,3000]]}"#);
        // 30° from the origin meets x = 3000 at y = 1732.05.
        let f = at(&m, P2(3010.0, 1700.0), &[P2(0.0, 0.0)], &Snaps::default());
        assert_eq!(f.kind, Kind::Intersection);
        assert!(f.at.dist(P2(3000.0, 3000.0 * 30f64.to_radians().tan())) < 1e-2, "{:?}", f.at);
        assert!(f.guide.is_some());
    }

    #[test]
    fn each_snap_switches_off_on_its_own() {
        let m = model(r#"wall {"points": [[0,0],[6000,0]]}"#);
        let o = Snaps::default();
        assert_eq!(at(&m, P2(30.0, -20.0), &[], &Snaps { endpoint: false, ..o }).kind, Kind::Nearest);
        assert_eq!(at(&m, P2(3040.0, 30.0), &[], &Snaps { midpoint: false, ..o }).kind, Kind::Nearest);
        let none = Snaps { nearest: false, ..Snaps { endpoint: false, ..o } };
        assert_eq!(at(&m, P2(34.0, -20.0), &[], &none).at, P2(30.0, -20.0));
    }

    #[test]
    fn nothing_a_cursor_or_a_chain_holds_makes_it_panic() {
        let m = model(r#"wall {"points": [[0,0],[6000,0]]}"#);
        let (nan, inf) = (f64::NAN, f64::INFINITY);
        let chains: [&[P2]; 5] = [&[], &[P2(0.0, 0.0)], &[P2(nan, 1.0), P2(1.0, 1.0)], &[P2(5.0, 5.0), P2(5.0, 5.0)], &[P2(inf, -inf)]];
        for p in [P2(nan, 0.0), P2(inf, -inf), P2(1e300, 1e300), P2(0.0, 0.0), P2(-0.0, 1e-300)] {
            for from in chains {
                for o in [
                    Snaps::default(),
                    Snaps { angle: nan, ..Snaps::default() },
                    Snaps { angle: 1.0, ..Snaps::default() },
                    Snaps { angle: 1e9, ..Snaps::default() },
                ] {
                    for square in [true, false] {
                        let _ = snap(&m, 100000, p, REACH, from, square, &o);
                    }
                }
            }
        }
        for reach in [Reach { point: 0.0, line: 0.0 }, Reach { point: nan, line: -1.0 }, Reach { point: inf, line: inf }] {
            let _ = snap(&m, 100000, P2(10.0, 10.0), reach, &[P2(0.0, 0.0)], false, &Snaps::default());
        }
    }

    #[test]
    fn a_wall_is_found_far_from_both_its_ends_and_an_arc_where_it_bulges() {
        let m = model(
            r#"wall {"points": [[0,0],[60000,0]]}
               wall {"points": [[0,30000],[10000,30000]], "arc": 180}"#,
        );
        let f = at(&m, P2(31234.0, 40.0), &[], &Snaps::default());
        assert_eq!((f.at, f.kind), (P2(31230.0, 0.0), Kind::Nearest));
        // Out where the arc bulges, 5000 mm from its chord: past the reach and the thickest wall.
        let want = m
            .elements
            .values()
            .find_map(|e| match e {
                Element::Wall(w) if w.arc != 0.0 => {
                    let a = Axis::of(w);
                    Some(a.at(a.length() / 2.0, 0.0))
                }
                _ => None,
            })
            .unwrap_or_default();
        assert!((want.1 - 30000.0).abs() > 4999.0, "{want:?}");
        let f = at(&m, want + P2(30.0, 20.0), &[], &Snaps::default());
        assert_eq!(f.kind, Kind::Midpoint, "{f:?}");
        assert!(f.at.dist(want) < 1e-3, "{:?}", f.at);
    }

    #[test]
    fn a_guides_corner_is_read_whichever_way_it_runs() {
        let m = model("");
        // A right angle off a line along an axis: the step the corner is read from is 1 mm long.
        let f = at(&m, P2(1000.0, 2000.0), &[P2(0.0, 0.0), P2(1000.0, 0.0)], &Snaps::default());
        assert_eq!(f.kind, Kind::Angle);
        assert!(f.guide.and_then(|g| g.corner).is_some_and(|c| (c - 90.0).abs() < 1e-9), "{:?}", f.guide);
        // Every one of the 24 guides, off lines along each axis and at 20°.
        for line in [P2(1000.0, 0.0), P2(0.0, 1000.0), P2(-1000.0, 0.0), P2(0.0, -1000.0), P2(940.0, 342.0)] {
            for k in 0..24 {
                let (a, last) = (f64::from(k) * 15f64.to_radians(), line);
                let to = last + P2(a.cos(), a.sin()) * 2000.0;
                let f = at(&m, to, &[P2(0.0, 0.0), last], &Snaps::default());
                let corner = f.guide.and_then(|g| g.corner);
                assert!(corner.is_some(), "off {line:?} at {}°: {f:?}", k * 15);
            }
        }
    }

    #[test]
    fn shift_keeps_a_step_square_from_a_point_off_the_grid() {
        let m = model("");
        let from = [P2(2501.5, 1000.25)];
        let f = snap(&m, 100000, P2(5000.0, 1003.0), REACH, &from, true, &Snaps::none());
        assert_eq!(f.at, P2(5000.0, 1000.25));
        let f = snap(&m, 100000, P2(2503.0, 4000.0), REACH, &from, true, &Snaps::none());
        assert_eq!(f.at, P2(2501.5, 4000.0));
    }

    #[test]
    fn every_step_of_an_angle_gets_its_guides_round_to_the_last() {
        let from = [P2(0.0, 0.0)];
        // 7° doesn't go into 360°: the last guide is at 357°, and 170° has one at 340°.
        assert_eq!(rays(&from, 7.0).last(), Some(&357.0));
        assert_eq!(rays(&from, 170.0), [0.0, 170.0, 340.0]);
        assert_eq!(rays(&from, 15.0).len(), 24);
        assert_eq!(rays(&from, 90.0), [0.0, 90.0, 180.0, 270.0]);
    }

    #[test]
    fn at_the_same_distance_an_end_wins_over_a_middle() {
        // A wall 100 mm long: its middle is 50 mm from either end; the cursor in the middle of it.
        let m = model(r#"wall {"points": [[0,0],[100,0]]}"#);
        let f = at(&m, P2(25.0, 0.0), &[], &Snaps::default());
        assert_eq!((f.at, f.kind), (P2(0.0, 0.0), Kind::Endpoint));
        let f = at(&m, P2(50.0, 0.0), &[], &Snaps::default());
        assert_eq!((f.at, f.kind), (P2(50.0, 0.0), Kind::Midpoint));
    }

    #[test]
    fn the_ruler_reads_length_parts_and_angle_and_its_graduations_are_round() {
        let r = ruler(P2(0.0, 0.0), P2(3000.0, 4000.0));
        assert_eq!((r.length, r.dx, r.dy), (5000.0, 3000.0, 4000.0));
        assert!((r.angle - 53.130102354).abs() < 1e-6);
        assert_eq!(ruler(P2(0.0, 0.0), P2(-1000.0, 0.0)).angle, 180.0);
        assert_eq!(ruler(P2(0.0, 0.0), P2(0.0, -1000.0)).angle, 270.0);
        assert_eq!(ruler(P2(5.0, 5.0), P2(5.0, 5.0)).angle, 0.0);
        // 1, 2 or 5 times a power of ten, at least 10 px apart.
        assert_eq!(
            [graduation(1.0), graduation(0.5), graduation(0.1), graduation(0.02), graduation(0.004)],
            [10.0, 20.0, 100.0, 500.0, 5000.0]
        );
        assert_eq!(graduation(0.0), 100.0);
        assert_eq!(graduation(f64::NAN), 100.0);
    }
}
