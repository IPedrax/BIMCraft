//! Windows and room contents: where a room type's windows go in its exterior walls, and where its components stand. A pure step on
//! the rooms' measured boundaries, the walls, the doors and the separation lines: it draws no random number, so a plan's layout
//! never depends on it, and what does not fit is left out (a plan is never failed for its contents).
//!
//! Every distance here is chosen so the checks of the spec hold with room to spare: a window is 300 mm clear of a corner, a door
//! and another window; a component lies wholly inside its room, clear of the others, of every door's swing and of 600 mm in
//! front of every door and opening.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::emit::{DoorAt, WallAt};
use super::family::{Against, Content};

type P = [f64; 2];

/// The exterior wall's thickness is not known here; a room's exterior face lies this far from the wall's centre line. Walls are
/// told apart by their `exterior` flag, whose thickness is 250.
const EXTERIOR_HALF: f64 = 125.0;
/// Where a face is taken to be the face of a wall, mm.
const FACE_TOL: f64 = 3.0;
/// A window's clearance from a room's corner, a door and another window (the spec's 300), and what is added to be sure.
const CLEAR: f64 = 300.0;
const CLEAR_PLUS: f64 = 5.0;
/// Which windows, widest first, and the average length of exterior wall a window serves.
const WINDOWS: [(&str, i64); 2] = [("1200 x 1500", 1200), ("600 x 900", 600)];
pub const WINDOW_FAMILY: &str = "Fixed Window";
const PER_WINDOW: f64 = 4000.0;
/// How far inside a wall a component stands, mm: it is on the boundary to the checks' tolerance and inside it.
const INSET: f64 = 2.0;
/// The gap kept between two components, mm.
const GAP: f64 = 50.0;
/// The band in front of a door or an opening, mm, and what is added to it.
const FRONT: f64 = 600.0;
const FRONT_PLUS: f64 = 20.0;
/// What a window keeps clear of components: its span and this much each side, and this far into the room. A window's sill and
/// frame are drawn over what stands under them, which would cut the piece's outline short of the wall.
const WINDOW_SIDE: f64 = 100.0;
const WINDOW_DEEP: f64 = 200.0;
/// How far a component slides along a wall, and the grid the centre of a room is looked for on, mm.
const SLIDE: f64 = 50.0;
const GRID: f64 = 100.0;
/// The centre candidates tried, best first.
const CENTRE_TRIES: usize = 3000;

#[derive(Clone, Debug, PartialEq)]
pub struct WindowAt {
    pub at: [i64; 2],
    pub ty: &'static str,
    pub width: i64,
    pub room: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentAt {
    pub family: String,
    pub ty: String,
    pub at: [i64; 2],
    pub angle: f64,
    pub room: usize,
}

/// A component an entry of the room type lists that found no place.
#[derive(Clone, Debug, PartialEq)]
pub struct LeftOut {
    pub room: usize,
    pub family: String,
}

#[derive(Clone, Debug, Default)]
pub struct Placed {
    pub windows: Vec<WindowAt>,
    pub components: Vec<ComponentAt>,
    pub left_out: Vec<LeftOut>,
}

/// What the placement reads of a room: its measured boundary, whether it has windows, and what it holds.
pub struct RoomIn<'a> {
    pub poly: Vec<P>,
    pub window: bool,
    pub contents: &'a [Content],
}

// ---- plane geometry

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: P, k: f64) -> P {
    [a[0] * k, a[1] * k]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn dist(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
/// The unit vector from `a` to `b` and the length.
fn unit(a: P, b: P) -> (P, f64) {
    let d = sub(b, a);
    let l = d[0].hypot(d[1]);
    if l > 0.0 { ([d[0] / l, d[1] / l], l) } else { ([1.0, 0.0], 0.0) }
}
fn normal(u: P) -> P {
    [-u[1], u[0]]
}
fn seg_dist(p: P, a: P, b: P) -> f64 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    if l2 <= 0.0 {
        return dist(p, a);
    }
    let t = (dot(sub(p, a), d) / l2).clamp(0.0, 1.0);
    dist(p, add(a, mul(d, t)))
}
fn seg_seg(s: (P, P), t: (P, P)) -> f64 {
    seg_dist(s.0, t.0, t.1).min(seg_dist(s.1, t.0, t.1)).min(seg_dist(t.0, s.0, s.1)).min(seg_dist(t.1, s.0, s.1))
}

/// The polygon without repeated points and without points on the straight line between their neighbours.
fn simplify(poly: &[P]) -> Vec<P> {
    let mut pts: Vec<P> = Vec::new();
    for (i, p) in poly.iter().enumerate() {
        let prev = poly[(i + poly.len() - 1) % poly.len()];
        if dist(*p, prev) > 0.5 {
            pts.push(*p);
        }
    }
    loop {
        if pts.len() <= 3 {
            break;
        }
        let n = pts.len();
        match (0..n).find(|&i| seg_dist(pts[i], pts[(i + n - 1) % n], pts[(i + 1) % n]) < 0.5) {
            Some(i) => {
                pts.remove(i);
            }
            None => break,
        }
    }
    pts
}

fn edges(poly: &[P]) -> impl Iterator<Item = (P, P)> + '_ {
    poly.iter().enumerate().map(|(i, p)| (*p, poly[(i + 1) % poly.len()]))
}

fn area2(poly: &[P]) -> f64 {
    edges(poly).map(|(a, b)| cross(a, b)).sum()
}

/// Whether `p` is inside the polygon (ray casting).
fn inside(poly: &[P], p: P) -> bool {
    let mut c = false;
    for (a, b) in edges(poly) {
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

fn clearance(poly: &[P], p: P) -> f64 {
    edges(poly).map(|(a, b)| seg_dist(p, a, b)).fold(f64::INFINITY, f64::min)
}

/// A rotation by `deg` degrees, exact for the quarter turns.
fn rotation(deg: f64) -> (f64, f64) {
    let q = deg.rem_euclid(360.0);
    for (k, cs) in [(0.0, (1.0, 0.0)), (90.0, (0.0, 1.0)), (180.0, (-1.0, 0.0)), (270.0, (0.0, -1.0))] {
        if (q - k).abs() < 0.01 || (q - 360.0 - k).abs() < 0.01 {
            return cs;
        }
    }
    let r = q.to_radians();
    (r.cos(), r.sin())
}

/// An angle in degrees from 0 up to 360, snapped to a quarter turn when it is near one.
fn angle(deg: f64) -> f64 {
    let q = deg.rem_euclid(360.0);
    for k in [0.0, 90.0, 180.0, 270.0, 360.0] {
        if (q - k).abs() < 0.01 {
            return if k == 360.0 { 0.0 } else { k };
        }
    }
    (q * 1000.0).round() / 1000.0
}

fn turn(v: P, (c, s): (f64, f64)) -> P {
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// The four corners of a footprint `[x0, y0, x1, y1]` turned by `deg` and put at `at`.
fn corners(fp: [f64; 4], at: P, deg: f64) -> [P; 4] {
    let cs = rotation(deg);
    [[fp[0], fp[1]], [fp[2], fp[1]], [fp[2], fp[3]], [fp[0], fp[3]]].map(|c| add(at, turn(c, cs)))
}

/// A rectangle `half` long each way along `u` from `c`, and `deep` each side of it.
fn rect(c: P, u: P, half: f64, deep: f64) -> [P; 4] {
    let n = normal(u);
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(a, b)| add(c, add(mul(u, a * half), mul(n, b * deep))))
}

/// Whether two convex polygons are closer than `margin` (by separating axes): `margin` 0 is overlapping.
fn near(p: &[P], q: &[P], margin: f64) -> bool {
    for poly in [p, q] {
        for (a, b) in edges(poly) {
            let (u, l) = unit(a, b);
            if l <= 1e-9 {
                continue;
            }
            let ax = normal(u);
            let pp: Vec<f64> = p.iter().map(|v| dot(*v, ax)).collect();
            let qq: Vec<f64> = q.iter().map(|v| dot(*v, ax)).collect();
            let (pmax, pmin) = (pp.iter().copied().fold(f64::NEG_INFINITY, f64::max), pp.iter().copied().fold(f64::INFINITY, f64::min));
            let (qmax, qmin) = (qq.iter().copied().fold(f64::NEG_INFINITY, f64::max), qq.iter().copied().fold(f64::INFINITY, f64::min));
            if pmax + margin <= qmin || qmax + margin <= pmin {
                return false;
            }
        }
    }
    true
}

/// Whether two segments cross through each other, more than a millimetre from where they end.
fn crosses(p: (P, P), q: (P, P)) -> bool {
    let side = |a: P, b: P, x: P| {
        let (u, l) = unit(a, b);
        if l > 0.0 { cross(u, sub(x, a)) } else { 0.0 }
    };
    let (d1, d2) = (side(q.0, q.1, p.0), side(q.0, q.1, p.1));
    let (e1, e2) = (side(p.0, p.1, q.0), side(p.0, p.1, q.1));
    ((d1 > 1.0 && d2 < -1.0) || (d1 < -1.0 && d2 > 1.0)) && ((e1 > 1.0 && e2 < -1.0) || (e1 < -1.0 && e2 > 1.0))
}

/// Whether the box lies wholly inside the polygon: its corners do, no corner of the polygon is inside it and no wall crosses it.
fn holds(poly: &[P], b: &[P; 4]) -> bool {
    if !b.iter().all(|c| inside(poly, *c)) {
        return false;
    }
    let shrunk = {
        let m = [(b[0][0] + b[1][0] + b[2][0] + b[3][0]) / 4.0, (b[0][1] + b[1][1] + b[2][1] + b[3][1]) / 4.0];
        b.map(|c| {
            let (u, l) = unit(m, c);
            sub(c, mul(u, 1.5_f64.min(l)))
        })
    };
    if poly.iter().any(|v| inside(&shrunk, *v)) {
        return false;
    }
    !edges(poly).any(|e| (0..4).any(|i| crosses(e, (b[i], b[(i + 1) % 4]))))
}

// ---- the library's footprints

fn library() -> &'static BTreeMap<String, crate::family::Family> {
    static LIB: OnceLock<BTreeMap<String, crate::family::Family>> = OnceLock::new();
    LIB.get_or_init(|| crate::family::library(crate::language::Language::English))
}

/// The plan footprint `[x0, y0, x1, y1]` of a built-in family's type, in its own axes: the back of a wall-hugging piece is at y 0.
pub fn footprint(family: &str, ty: &str) -> Option<[f64; 4]> {
    let f = library().get(family)?;
    f.plan_footprint(&f.values(ty, &BTreeMap::new()).ok()?)
}

// ---- walls and doors

struct Seg {
    a: P,
    u: P,
    len: f64,
}

fn seg_of(a: [i64; 2], b: [i64; 2]) -> Seg {
    let (a, b) = ([a[0] as f64, a[1] as f64], [b[0] as f64, b[1] as f64]);
    let (u, len) = unit(a, b);
    Seg { a, u, len }
}

/// Where a point is on a segment's line (offset across, distance along).
fn on(s: &Seg, p: P) -> (f64, f64) {
    let d = sub(p, s.a);
    (cross(s.u, d), dot(s.u, d))
}

/// The span of a door along the wall it stands in.
struct DoorBox {
    span: (P, P),
    zone: [P; 4],
}

fn door_boxes(walls: &[WallAt], doors: &[DoorAt]) -> Vec<DoorBox> {
    let segs: Vec<(Seg, f64)> = walls.iter().map(|w| (seg_of(w.a, w.b), if w.exterior { 250.0 } else { 130.0 })).collect();
    doors
        .iter()
        .filter_map(|d| {
            let at = [d.at[0] as f64, d.at[1] as f64];
            let (s, t) = segs.iter().find(|(s, _)| {
                let (off, along) = on(s, at);
                s.len > 0.0 && off.abs() <= 1.0 && (0.0..=s.len).contains(&along)
            })?;
            let w = d.width as f64;
            Some(DoorBox {
                span: (sub(at, mul(s.u, w / 2.0)), add(at, mul(s.u, w / 2.0))),
                zone: rect(at, s.u, w / 2.0 + FRONT_PLUS, t / 2.0 + FRONT.max(w) + FRONT_PLUS),
            })
        })
        .collect()
}

// ---- windows

struct Face {
    a: P,
    u: P,
    len: f64,
    /// The exterior segments the face lies on.
    on: Vec<usize>,
}

/// The faces of a room's exterior walls: each polygon edge parallel to an exterior wall and half its thickness from its centre
/// line, and the length of exterior wall they hold together.
fn faces(poly: &[P], ext: &[Seg]) -> (Vec<Face>, f64) {
    let mut out = Vec::new();
    let mut total = 0.0;
    for (a, b) in edges(poly) {
        let (u, len) = unit(a, b);
        if len < 1.0 {
            continue;
        }
        let mut on_segs = Vec::new();
        for (k, s) in ext.iter().enumerate() {
            if cross(u, s.u).abs() >= 1e-3 || (on(s, a).0.abs() - EXTERIOR_HALF).abs() > FACE_TOL {
                continue;
            }
            let (t0, t1) = {
                let (x, y) = (on(s, a).1, on(s, b).1);
                (x.min(y), x.max(y))
            };
            let ov = t1.min(s.len) - t0.max(0.0);
            if ov > 1.0 {
                total += ov;
                on_segs.push(k);
            }
        }
        if !on_segs.is_empty() {
            out.push(Face { a, u, len, on: on_segs });
        }
    }
    (out, total)
}

fn windows(rooms: &[RoomIn], walls: &[WallAt], boxes: &[DoorBox]) -> (Vec<WindowAt>, Vec<[P; 4]>) {
    let ext: Vec<Seg> = walls.iter().filter(|w| w.exterior).map(|w| seg_of(w.a, w.b)).collect();
    let mut out: Vec<WindowAt> = Vec::new();
    let mut spans: Vec<(P, P)> = Vec::new();
    let mut zones: Vec<[P; 4]> = Vec::new();
    for (room, r) in rooms.iter().enumerate() {
        if !r.window {
            continue;
        }
        let (faces, total) = faces(&r.poly, &ext);
        let want = (total / PER_WINDOW).ceil() as usize;
        // Centres put on each face so far, along it.
        let mut held: Vec<Vec<f64>> = vec![Vec::new(); faces.len()];
        for _ in 0..want {
            let mut order: Vec<usize> = (0..faces.len()).collect();
            order.sort_by(|x, y| {
                let (kx, ky) = (faces[*x].len / (held[*x].len() + 1) as f64, faces[*y].len / (held[*y].len() + 1) as f64);
                ky.total_cmp(&kx).then(x.cmp(y))
            });
            let mut done = false;
            'faces: for f in order {
                let face = &faces[f];
                for (ty, w) in WINDOWS {
                    let w = w as f64;
                    let (lo, hi) = (CLEAR + w / 2.0, face.len - CLEAR - w / 2.0);
                    if hi < lo {
                        continue;
                    }
                    let mut best: Option<(f64, f64, P, (P, P))> = None;
                    let steps = ((hi - lo) / SLIDE).floor() as i64;
                    for k in 0..=steps {
                        let t = lo + k as f64 * SLIDE;
                        let Some((c, span)) = centre(face, &ext, t, w) else { continue };
                        let far = CLEAR + CLEAR_PLUS;
                        if boxes.iter().any(|d| seg_seg(span, d.span) < far) || spans.iter().any(|s| seg_seg(span, *s) < far) {
                            continue;
                        }
                        let score = held[f].iter().map(|h| (t - h).abs()).fold(2.0 * t.min(face.len - t), f64::min);
                        if best.is_none_or(|b| score > b.0) {
                            best = Some((score, t, c, span));
                        }
                    }
                    if let Some((_, t, c, span)) = best {
                        held[f].push(t);
                        spans.push(span);
                        zones.push(rect(c, face.u, w / 2.0 + WINDOW_SIDE, EXTERIOR_HALF + WINDOW_DEEP));
                        out.push(WindowAt { at: [c[0].round() as i64, c[1].round() as i64], ty, width: w as i64, room });
                        done = true;
                        break 'faces;
                    }
                }
            }
            if !done {
                break;
            }
        }
    }
    (out, zones)
}

/// The point on the wall's centre line for the window at `t` along the face, and its span along it, when it is on an exterior wall.
fn centre(face: &Face, ext: &[Seg], t: f64, w: f64) -> Option<(P, (P, P))> {
    let p = add(face.a, mul(face.u, t));
    let s = face.on.iter().map(|k| &ext[*k]).find(|s| {
        let (off, along) = on(s, p);
        off.abs() <= EXTERIOR_HALF + FACE_TOL && (0.0..=s.len).contains(&along)
    })?;
    let c = sub(p, mul(normal(s.u), on(s, p).0));
    Some((c, (sub(c, mul(face.u, w / 2.0)), add(c, mul(face.u, w / 2.0)))))
}

// ---- components

/// What a component placement may not touch: the other components (with their gap) and the doors' and openings' zones.
struct Keep {
    zones: Vec<[P; 4]>,
    placed: Vec<[P; 4]>,
}

impl Keep {
    fn free(&self, b: &[P; 4]) -> bool {
        !self.zones.iter().any(|z| near(b, z, 0.0)) && !self.placed.iter().any(|q| near(b, q, GAP))
    }
}

/// The turn that puts a piece's back (its +y) toward the wall whose inward normal is `n`.
fn back_to(n: P) -> f64 {
    angle(n[0].atan2(-n[1]).to_degrees())
}

/// The box a footprint makes when turned by `deg`, with the position `at` that puts the walls' constraints on it: the smallest
/// extent of the turned footprint across each normal `ns` is `INSET` inside the line through `p` with it.
fn extents(fp: [f64; 4], deg: f64, dir: P) -> (f64, f64) {
    let cs = rotation(deg);
    let ps: Vec<f64> = [[fp[0], fp[1]], [fp[2], fp[1]], [fp[2], fp[3]], [fp[0], fp[3]]].iter().map(|c| dot(turn(*c, cs), dir)).collect();
    (ps.iter().copied().fold(f64::INFINITY, f64::min), ps.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

fn rounded(at: P) -> P {
    [at[0].round(), at[1].round()]
}

/// Where a footprint may stand against a wall of the polygon, best first: each edge, longest first, each way the piece may face
/// (its back to the wall first), sliding from the middle of the edge outward.
fn wall_candidates(fp: [f64; 4], poly: &[P], ccw: bool) -> Vec<(P, f64)> {
    let mut es: Vec<(usize, P, P)> = edges(poly).enumerate().map(|(i, (a, b))| (i, a, b)).collect();
    es.sort_by(|x, y| dist(y.1, y.2).total_cmp(&dist(x.1, x.2)).then(x.0.cmp(&y.0)));
    let mut out = Vec::new();
    for (_, a, b) in es {
        let (u, len) = unit(a, b);
        if len < 1.0 {
            continue;
        }
        let n = if ccw { normal(u) } else { mul(normal(u), -1.0) };
        let back = back_to(n);
        for deg in [back, back + 90.0, back + 270.0, back + 180.0] {
            let (nlo, _) = extents(fp, deg, n);
            let (ulo, uhi) = extents(fp, deg, u);
            let (centre, half) = ((ulo + uhi) / 2.0, (uhi - ulo) / 2.0);
            if len < 2.0 * half {
                continue;
            }
            let k = INSET - nlo;
            let (lo, hi) = (half, len - half);
            let mid = (lo + hi) / 2.0;
            let steps = ((hi - lo) / 2.0 / SLIDE).floor() as i64;
            for i in 0..=steps {
                for s in if i == 0 { vec![mid] } else { vec![mid - i as f64 * SLIDE, mid + i as f64 * SLIDE] } {
                    let at = add(add(a, mul(u, s - centre)), mul(n, k));
                    out.push((rounded(at), angle(deg)));
                }
            }
        }
    }
    out
}

/// Where a footprint may stand in a corner of the polygon where two walls meet at a right angle, in the polygon's order.
fn corner_candidates(fp: [f64; 4], poly: &[P], ccw: bool) -> Vec<(P, f64)> {
    let n = poly.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (p, v, q) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
        let ((u1, l1), (u2, l2)) = (unit(p, v), unit(v, q));
        if l1 < 1.0 || l2 < 1.0 || (cross(u1, u2) > 0.0) != ccw || cross(u1, u2).abs() < 0.999 {
            continue;
        }
        let (n1, n2) = if ccw { (normal(u1), normal(u2)) } else { (mul(normal(u1), -1.0), mul(normal(u2), -1.0)) };
        for deg in [back_to(n1), back_to(n2), back_to(n1) + 180.0, back_to(n2) + 180.0] {
            let (lo1, _) = extents(fp, deg, n1);
            let (lo2, _) = extents(fp, deg, n2);
            let x1 = dot(v, n1) + INSET - lo1;
            let x2 = dot(v, n2) + INSET - lo2;
            out.push((rounded(add(mul(n1, x1), mul(n2, x2))), angle(deg)));
        }
    }
    out
}

/// Where a footprint may stand away from the walls: the points of the room furthest from any wall first.
fn centre_candidates(fp: [f64; 4], poly: &[P]) -> Vec<(P, f64)> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in poly {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let mut pts: Vec<(f64, P)> = Vec::new();
    let mut x = (lo[0] / GRID).ceil() * GRID;
    while x <= hi[0] {
        let mut y = (lo[1] / GRID).ceil() * GRID;
        while y <= hi[1] {
            if inside(poly, [x, y]) {
                pts.push((clearance(poly, [x, y]), [x, y]));
            }
            y += GRID;
        }
        x += GRID;
    }
    pts.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1[1].total_cmp(&b.1[1])).then(a.1[0].total_cmp(&b.1[0])));
    let mid = [(fp[0] + fp[2]) / 2.0, (fp[1] + fp[3]) / 2.0];
    let mut out = Vec::new();
    for (_, p) in pts.into_iter().take(CENTRE_TRIES) {
        for deg in [0.0, 90.0] {
            // The footprint's middle on the point, not its origin.
            out.push((rounded(sub(p, turn(mid, rotation(deg)))), deg));
        }
    }
    out
}

fn components(rooms: &[RoomIn], boxes: &[DoorBox], seps: &[Vec<[i64; 2]>], window_zones: Vec<[P; 4]>) -> (Vec<ComponentAt>, Vec<LeftOut>) {
    let mut zones: Vec<[P; 4]> = boxes.iter().map(|d| d.zone).collect();
    zones.extend(window_zones);
    for run in seps {
        for w in run.windows(2) {
            let (a, b) = ([w[0][0] as f64, w[0][1] as f64], [w[1][0] as f64, w[1][1] as f64]);
            let (u, l) = unit(a, b);
            if l > 0.5 {
                zones.push(rect(mul(add(a, b), 0.5), u, l / 2.0 + FRONT_PLUS, FRONT + FRONT_PLUS));
            }
        }
    }
    let mut keep = Keep { zones, placed: Vec::new() };
    let (mut out, mut left) = (Vec::new(), Vec::new());
    for (room, r) in rooms.iter().enumerate() {
        if r.contents.is_empty() || r.poly.len() < 3 {
            continue;
        }
        let ccw = area2(&r.poly) > 0.0;
        for entry in r.contents {
            let mut placed = 0;
            for _ in 0..entry.count.1 {
                let found = entry.types.iter().find_map(|ty| {
                    let fp = footprint(&entry.family, ty)?;
                    let cands = match entry.against {
                        Against::Wall => wall_candidates(fp, &r.poly, ccw),
                        Against::Corner => corner_candidates(fp, &r.poly, ccw),
                        Against::Centre => centre_candidates(fp, &r.poly),
                    };
                    cands.into_iter().find_map(|(at, deg)| {
                        let b = corners(fp, at, deg);
                        (holds(&r.poly, &b) && keep.free(&b)).then(|| (ty.clone(), at, deg, b))
                    })
                });
                let Some((ty, at, deg, b)) = found else { break };
                keep.placed.push(b);
                out.push(ComponentAt { family: entry.family.clone(), ty, at: [at[0] as i64, at[1] as i64], angle: deg, room });
                placed += 1;
            }
            for _ in placed..entry.count.0 {
                left.push(LeftOut { room, family: entry.family.clone() });
            }
        }
    }
    (out, left)
}

/// The windows and components of a plan: `rooms` hold their measured boundaries (in the plan's order), `walls`, `doors` and
/// `separators` as drawn.
pub fn place(rooms: &[RoomIn], walls: &[WallAt], doors: &[DoorAt], separators: &[Vec<[i64; 2]>]) -> Placed {
    let rooms: Vec<RoomIn> = rooms.iter().map(|r| RoomIn { poly: simplify(&r.poly), window: r.window, contents: r.contents }).collect();
    let boxes = door_boxes(walls, doors);
    let (windows, window_zones) = windows(&rooms, walls, &boxes);
    let (components, left_out) = components(&rooms, &boxes, separators, window_zones);
    Placed { windows, components, left_out }
}

#[cfg(test)]
#[path = "contents_tests.rs"]
mod tests;
