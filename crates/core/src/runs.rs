//! Runs: the geometry of a straight element between two points, whatever it carries (a beam, a strip footing, a pipe, a
//! conduit, a cable tray, a wire, a conductor, a cable): the ends of its centre line, its footprint in plan, the prism or the
//! tube it makes in 3D. It knows no category but for the table that gives its IFC class and the defaults it takes (may it stand,
//! where its heights are); the plan, the section, the 3D view, the snaps and IFC ask it.

use crate::geom::{self, P2};
use crate::model::{Element, Id, Justify, MAX_SLOPE, Model, Run, RunType, TOL, Wall, heading};
use crate::profiles::Piece;
use crate::view3d::{P3, add, cross, mul, sub, unit};
use crate::walls::{self, Axis, Plane};

/// Sides a round run's tube is drawn with.
pub const SIDES: usize = 24;

/// Whether a run of `category` may stand upright (a riser, a down conductor) rather than run along: not a beam or a
/// strip footing, which are columns when they stand.
pub fn may_stand(category: &str) -> bool {
    !matches!(category, "beam" | "stripfooting")
}

/// How each run category is written in IFC: its class and predefined type (all of them take a product's nine attributes). Import
/// reads the same table back, a class and a type to the first category that has them.
pub const IFC: &[(&str, &str, &str)] = &[
    ("beam", "IFCBEAM", ".BEAM."),
    ("stripfooting", "IFCFOOTING", ".STRIP_FOOTING."),
    ("pipe", "IFCPIPESEGMENT", ".RIGIDSEGMENT."),
    ("duct", "IFCDUCTSEGMENT", ".RIGIDSEGMENT."),
    ("conduit", "IFCCABLECARRIERSEGMENT", ".CONDUITSEGMENT."),
    ("cabletray", "IFCCABLECARRIERSEGMENT", ".CABLETRAYSEGMENT."),
    // A ladder is a tray to BIMCraft, written as a tray: only a file of another tool's says it.
    ("cabletray", "IFCCABLECARRIERSEGMENT", ".CABLELADDERSEGMENT."),
    ("wire", "IFCCABLESEGMENT", ".CABLESEGMENT."),
    // IFC has no lightning protection: a down conductor is a conductor segment, as is an earthing conductor (BIMCraft's own
    // `Category` tells them apart).
    ("downconductor", "IFCCABLESEGMENT", ".CONDUCTORSEGMENT."),
    ("groundconductor", "IFCCABLESEGMENT", ".CONDUCTORSEGMENT."),
    // A data cable is a cable segment, as a wire is (BIMCraft's own `Category` tells them apart).
    ("cable", "IFCCABLESEGMENT", ".CABLESEGMENT."),
];

/// The IFC class and predefined type a run of `category` is written as.
pub fn ifc(category: &str) -> Option<(&'static str, &'static str)> {
    IFC.iter().find(|(c, ..)| *c == category).map(|(_, class, kind)| (*class, *kind))
}

/// The category of a run an IFC product is, from its `class`, its `kind` (the predefined type) and, when a file of BIMCraft's says
/// so, the `declared` category: that one if the class has it, else the one whose type it is, else the class's first.
pub fn from_ifc(class: &str, kind: Option<&str>, declared: Option<&str>) -> Option<&'static str> {
    let class = if class == "IFCBEAMSTANDARDCASE" { "IFCBEAM" } else { class };
    let rows = || IFC.iter().filter(move |(_, c, _)| *c == class);
    let by_kind = |k: &str| rows().find(|(_, _, t)| t.trim_matches('.') == k);
    let by_name = |d: &str| rows().find(|(c, ..)| *c == d);
    declared.and_then(by_name).or_else(|| kind.and_then(by_kind)).or_else(|| rows().next()).map(|(c, ..)| *c)
}

/// The line of its section a run of `category` has its heights at unless told: the top of a beam or a footing, as engineers
/// think of them (top of beam at the top of slab), the middle of anything that carries something (a pipe, a cable).
pub fn default_justify(category: &str) -> Justify {
    if may_stand(category) { Justify::Centre } else { Justify::Top }
}

/// Why `g`, a run of `category`, isn't one the model takes, if it isn't.
pub fn check(who: &str, category: &str, g: &Geom) -> Result<(), String> {
    if g.length() <= TOL {
        return Err(format!("{who}: too short ({} mm)", g.length()));
    }
    if !may_stand(category) {
        if g.vertical() {
            return Err(format!("{who}: its ends are one over the other; a {category} runs along, and what stands is a column"));
        }
        let tilt = (g.b[2] - g.a[2]).abs().atan2(g.plan_length()).to_degrees();
        if tilt > MAX_SLOPE {
            return Err(format!("{who}: it slopes {tilt:.1}°, more than the {MAX_SLOPE}° a {category} may; what stands is a column"));
        }
    }
    Ok(())
}

/// A run worked out: its centre line from `a` to `b` (mm: x east, y north, z up), and its section, `width` across by `depth`
/// deep, square to that line; round, `width` is the diameter and `depth` is as much.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geom {
    pub a: P3,
    pub b: P3,
    pub width: f64,
    pub depth: f64,
    pub round: bool,
}

impl Geom {
    /// `r` of type `t`: its heights are those of its `justify` line, so its centre line is half the section's depth (square
    /// to the line) under it for a top, over it for a bottom.
    pub fn of(m: &Model, r: &Run, t: &RunType) -> Geom {
        let base = m.elevation(r.level);
        let (z0, z1) = (base + r.offset, base + r.end_offset.unwrap_or(r.offset));
        let depth = if t.round { t.width } else { t.depth };
        let g = Geom { a: [r.start.0, r.start.1, z0], b: [r.end.0, r.end.1, z1], width: t.width, depth, round: t.round };
        // Upright, it has no top or bottom to be the line.
        let shift = match r.justify {
            Justify::Top => -g.half(),
            Justify::Centre => 0.0,
            Justify::Bottom => g.half(),
        };
        Geom { a: [g.a[0], g.a[1], z0 + shift], b: [g.b[0], g.b[1], z1 + shift], ..g }
    }

    /// Along the centre line: its true length.
    pub fn length(&self) -> f64 {
        self.plan_length().hypot(self.b[2] - self.a[2])
    }

    /// Its length in plan.
    pub fn plan_length(&self) -> f64 {
        P2(self.b[0] - self.a[0], self.b[1] - self.a[1]).dist(P2(0.0, 0.0))
    }

    /// Whether it stands upright: its ends one over the other in plan.
    pub fn vertical(&self) -> bool {
        self.plan_length() <= TOL
    }

    /// Half the section's depth, taken straight up: more than half of it, square to the line, where the line slopes.
    pub fn half(&self) -> f64 {
        let plan = self.plan_length();
        if plan > TOL { self.depth / 2.0 * self.length() / plan } else { 0.0 }
    }

    /// Along it in plan: east when upright.
    fn along(&self) -> P2 {
        if self.vertical() { P2(1.0, 0.0) } else { P2(self.b[0] - self.a[0], self.b[1] - self.a[1]).unit() }
    }

    /// Its footprint in plan, counter-clockwise: a strip `width` wide from end to end, or, upright, its section, `width`
    /// along east and `depth` along north, or round.
    pub fn footprint(&self) -> Vec<P2> {
        let (a, b) = (P2(self.a[0], self.a[1]), P2(self.b[0], self.b[1]));
        if !self.vertical() {
            let n = self.along().perp() * (self.width / 2.0);
            return vec![a - n, b - n, b + n, a + n];
        }
        if self.round {
            return (0..SIDES).map(|i| a + heading(360.0 * i as f64 / SIDES as f64) * (self.width / 2.0)).collect();
        }
        let (w, d) = (self.width / 2.0, self.depth / 2.0);
        [(-w, -d), (w, -d), (w, d), (-w, d)].into_iter().map(|(x, y)| a + P2(x, y)).collect()
    }

    /// The lowest and the highest it reaches.
    pub fn z_span(&self) -> (f64, f64) {
        let h = self.half();
        (self.a[2].min(self.b[2]) - h, self.a[2].max(self.b[2]) + h)
    }

    /// As a prism: its footprint between a bottom plane and a top one, a section a rectangle (round, the same square to it).
    pub fn prism(&self) -> (Vec<P2>, Plane, Plane) {
        let ring = self.footprint();
        if self.vertical() {
            let (lo, hi) = self.z_span();
            return (ring, Plane::flat(lo), Plane::flat(hi));
        }
        let a = P2(self.a[0], self.a[1]);
        let g = self.along() * ((self.b[2] - self.a[2]) / self.plan_length());
        let z = self.a[2] - g.dot(a);
        (ring, Plane { z: z - self.half(), g }, Plane { z: z + self.half(), g })
    }

    /// A steel section's pieces (`profiles::Profile::pieces`) swept along it, its x across it (to the left of the way it
    /// runs) and its y up, square to its line. Its ends are upright, as a prism's are, but for an end `caps` gives (its
    /// start's and its end's [`joined_cap`], where it joins other runs): there each piece runs on as far as the cap reaches
    /// and is cut by the planes of its segments, as [`end_cuts`] gives them, so its plates meet the other run's on the
    /// outline the plan fills, as a rectangular run's prism does. A rectangle is a prism between two planes, which a section cuts as it cuts
    /// any; a convex piece (a fillet's triangle, a pipe's segment) a closed mesh, which a section cuts where its edges cross
    /// the plane. None upright.
    pub fn swept(&self, pieces: &[Piece], caps: [Option<&[P2]>; 2]) -> Vec<Swept> {
        if self.vertical() {
            return Vec::new();
        }
        let way = self.along();
        let n = way.perp();
        let (a, b) = (P2(self.a[0], self.a[1]), P2(self.b[0], self.b[1]));
        // Each cap's planes, the other end inside them.
        let cuts: Vec<(P2, P2, P2)> =
            [(caps[0], b), (caps[1], a)].into_iter().filter_map(|(cap, inside)| Some(cap_cuts(cap?, inside))).flatten().collect();
        // How far past an end its cap reaches: as far as its pieces run on before the planes cut them.
        let past = |cap: Option<&[P2]>, end: P2, out: P2| cap.unwrap_or_default().iter().map(|p| (*p - end).dot(out)).fold(0.0, f64::max);
        let (a, b) = (a - way * past(caps[0], a, way * -1.0), b + way * past(caps[1], b, way));
        // Up the plane at its start, a section's y is y / cos θ: as far again as the line is longer than its plan.
        let k = self.length() / self.plan_length();
        let g = way * ((self.b[2] - self.a[2]) / self.plan_length());
        let z = self.a[2] - g.dot(P2(self.a[0], self.a[1]));
        let run = [b.0 - a.0, b.1 - a.1, g.dot(b - a)];
        pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Rect(lo, hi) => {
                    let ring = vec![a + n * lo.0, b + n * lo.0, b + n * hi.0, a + n * hi.0];
                    let ring = cuts.iter().fold(ring, |ring, cut| cut_ring(&ring, *cut));
                    (ring.len() >= 3).then_some(Swept::Prism(ring, Plane { z: z + lo.1 * k, g }, Plane { z: z + hi.1 * k, g }))
                }
                Piece::Convex(pts) => {
                    let start: Vec<P3> = pts.iter().map(|q| [a.0 + n.0 * q.0, a.1 + n.1 * q.0, z + g.dot(a) + q.1 * k]).collect();
                    let end: Vec<P3> = start.iter().map(|p| add(*p, run)).collect();
                    let at = |ring: &[P3], i: usize| ring.get(i).copied().unwrap_or_default();
                    let mut tris = Vec::with_capacity(4 * pts.len());
                    // Its ends first, the start turned back along it, then its sides.
                    for i in 1..pts.len().saturating_sub(1) {
                        tris.push([at(&start, 0), at(&start, i + 1), at(&start, i)]);
                        tris.push([at(&end, 0), at(&end, i), at(&end, i + 1)]);
                    }
                    let caps = tris.len();
                    for i in 0..pts.len() {
                        let j = (i + 1) % pts.len();
                        tris.push([at(&start, i), at(&start, j), at(&end, j)]);
                        tris.push([at(&start, i), at(&end, j), at(&end, i)]);
                    }
                    // A piece is convex, so the planes cut it as they cut a round run's tube.
                    let (tris, caps) = if cuts.is_empty() { (tris, caps) } else { clip_tube(tris, caps, &cuts) };
                    (!tris.is_empty()).then_some(Swept::Mesh(tris, caps))
                }
            })
            .collect()
    }

    /// A round run as a tube of triangles turned out: its two caps' first, and how many they are. It is [`unit_tube`]
    /// placed by [`Geom::tube_place`], to the last bit, so the 3D view's card can draw every round run from one tube.
    pub fn tube(&self) -> (Vec<[P3; 3]>, usize) {
        let (tris, caps) = unit_tube();
        let place = self.tube_place(crate::model::Id::default(), 0);
        (tris.iter().map(|t| t.map(|q| place.place(q))).collect(), caps)
    }

    /// Where [`unit_tube`] goes to be this run's, as `owner`'s copy of the `shape`th shape: its line along the tube's x, the
    /// radius along y and z, round its line from east (from the north, upright).
    pub(crate) fn tube_place(&self, owner: crate::model::Id, shape: usize) -> crate::component_geometry::ComponentInstance {
        let (d, r) = (unit(sub(self.b, self.a)), self.width / 2.0);
        let e1 = if self.vertical() { [1.0, 0.0, 0.0] } else { unit([-d[1], d[0], 0.0]) };
        let e2 = cross(d, e1);
        crate::component_geometry::ComponentInstance { owner, shape, at: self.a, axes: [sub(self.b, self.a), mul(e1, r), mul(e2, r)] }
    }
}

/// Whether two runs' heights overlap the way joined runs' must, strictly, as [`crate::walls::stacked`] holds two
/// walls': a footing whose top sits exactly on a beam's bottom meets it nowhere to mitre, while the touch of a
/// footing's top with a wall's base is the joint a glued footing makes ([`arms`] allows it of a wall).
fn overlapped(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Whether a run lies flat along the plan — from the run itself, what [`joinable`] says of its worked-out
/// geometry: upright it would be a column's stance and `end_offset` would make it slope.
pub(crate) fn flat(r: &Run) -> bool {
    r.start.dist(r.end) > TOL && (r.end_offset.unwrap_or(r.offset) - r.offset).abs() <= TOL
}

/// Whether `g` lies flat along the plan: upright it is a column's section, and with an `end_offset` its heights
/// change along it, so neither can mitre against anything (they draw as they did). A round run joins as its plan
/// outline shows it — its body is cut by the same planes as the outline (see [`tube_at`]).
pub(crate) fn joinable(g: &Geom) -> bool {
    !g.vertical() && (g.b[2] - g.a[2]).abs() <= TOL
}

/// Run `g`'s arm leaving the node at its start (or with `at_end`, at its end): [`run_arm_at`] from that end.
fn run_arm(g: &Geom, at_end: bool, mine: bool) -> walls::Arm {
    run_arm_at(g, if at_end { g.plan_length() } else { 0.0 }, !at_end, mine)
}

/// Run `g`'s arm leaving the node `s` mm along its line (from its `a`), going on with `forward` (else back along
/// itself): its way, where its two faces are there — the one left of the way first, as a wall's [`walls::Arm`] has
/// them — and how wide it is across.
fn run_arm_at(g: &Geom, s: f64, forward: bool, mine: bool) -> walls::Arm {
    let (a, b) = (P2(g.a[0], g.a[1]), P2(g.b[0], g.b[1]));
    let t = (b - a).unit();
    let n = t.perp() * (g.width / 2.0);
    let p = a + t * s;
    let (way, ccw, cw) = if forward { (t, p + n, p - n) } else { (t * -1.0, p - n, p + n) };
    walls::Arm { mine, way, ccw, cw, centre: p, thick: g.width, ..Default::default() }
}

/// A wall's arm at `s` mm along its line, as [`walls::arm`] makes one at an end: left of its way first.
fn wall_arm_at(m: &Model, w: &Wall, s: f64, forward: bool, mine: bool) -> walls::Arm {
    let axis = Axis::of(w);
    let (ext, int) = walls::faces(m, w);
    let t = axis.tangent(s);
    let (way, ccw, cw) = if forward { (t, ext, int) } else { (t * -1.0, int, ext) };
    walls::Arm {
        mine,
        way,
        ccw: axis.at(s, ccw),
        cw: axis.at(s, cw),
        centre: axis.at(s, (ext + int) / 2.0),
        thick: ext - int,
        ..Default::default()
    }
}

/// How far along `g` the node lies where its body takes it: over [`TOL`] from either end and within its half
/// width across; none, the node is beside it or past its ends.
fn lying_on(g: &Geom, node: P2) -> Option<f64> {
    let (a, b) = (P2(g.a[0], g.a[1]), P2(g.b[0], g.b[1]));
    let t = (b - a).unit();
    let s = (node - a).dot(t);
    let across = (node - a).dot(t.perp());
    (s > TOL && s < g.plan_length() - TOL && across.abs() <= g.width / 2.0 + TOL).then_some(s)
}

/// Run `id`'s arms round the node at its start (or with `at_end`, at its end), its own first, then those of the
/// runs of its own category that end there or that it ends on (or that its node lies on), and — of a strip footing
/// — of the walls it ends on, round the node counter-clockwise; and whether it ends ON one, which fills the node
/// past its own end. Runs meet only where their heights overlap (a footing meets a wall by its top touching the
/// wall's base), only when both join (`join`), and only runs lying flat. Candidates come through `index` when there
/// is one, else from every element of the model — the neighbours a plan of thousands of runs would cost O(n²).
fn arms(m: &Model, id: Id, at_end: bool, g: &Geom, index: Option<&walls::Index>) -> (Vec<walls::Arm>, bool) {
    let Some(e) = m.elements.get(id) else { return (Vec::new(), false) };
    let mut arms = vec![run_arm(g, at_end, true)];
    let mut through = false;
    // An element that refuses to join, or doesn't lie flat, meets nothing: its own square end is its whole cap.
    if !e.run().is_some_and(|r| r.join) || !joinable(g) {
        return (arms, false);
    }
    let category = e.category();
    let node = if at_end { P2(g.b[0], g.b[1]) } else { P2(g.a[0], g.a[1]) };
    let (lo, hi) = g.z_span();
    // The runs near the node: through the tile index when there is one, else every run, kept only those whose
    // bodies reach within half a width of the node.
    let near: Vec<(Id, &Element)> = match index {
        Some(index) => index.runs_near(node, lo, hi).into_iter().filter_map(|oid| Some((oid, m.elements.get(oid)?))).collect(),
        None => m
            .elements
            .iter()
            .filter(|(oid, oe)| **oid != id && oe.run().is_some())
            .filter_map(|(oid, oe)| {
                let og = m.run_geom(oe)?;
                let (a, b) = (P2(og.a[0], og.a[1]), P2(og.b[0], og.b[1]));
                (geom::dist_to_segment(node, a, b) <= og.width / 2.0 + TOL).then_some((*oid, oe))
            })
            .collect(),
    };
    for &(other, o) in &near {
        if other == id {
            continue;
        }
        let (Some(oe), Some(og)) = (o.run(), m.run_geom(o)) else { continue };
        if !oe.join || o.category() != category || !joinable(&og) {
            continue;
        }
        if !overlapped((lo, hi), og.z_span()) {
            continue;
        }
        let (oa, ob) = (P2(og.a[0], og.a[1]), P2(og.b[0], og.b[1]));
        if oa.dist(node) <= TOL {
            arms.push(run_arm(&og, false, false));
        } else if ob.dist(node) <= TOL {
            arms.push(run_arm(&og, true, false));
        } else if let Some(s) = lying_on(&og, node) {
            // Ending on it: it runs on both ways from where the node meets it, and fills the node.
            arms.push(run_arm_at(&og, s, true, false));
            arms.push(run_arm_at(&og, s, false, false));
            through = true;
        }
    }
    // A strip footing ends on walls: they stop it at their faces and are never changed by it — the walls' own
    // outlines know nothing of runs. Its top touching the wall's base is enough: that is how a footing meets the
    // wall it carries.
    if category == "stripfooting" {
        // The query asks the bands a touch falls in: a footing's top exactly on the wall's base shares no
        // band with it, and the tile bands would leave the wall out before the exact check below could
        // allow it. That check is still the one that decides, with the touch allowed.
        let at = ([node.0, node.1, node.0, node.1], lo - TOL, hi + TOL);
        let thickest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
        let lookup = m.lookup();
        for oid in lookup.walls_near_at(&[at], thickest + 1.0) {
            if oid == id {
                continue;
            }
            let Some(Element::Wall(w)) = m.elements.get(oid) else { continue };
            if !w.join || !(lo - TOL <= m.top_z(w) && m.base_z(w) - TOL <= hi) {
                continue;
            }
            let axis = Axis::of(w);
            // A straight wall directly over a footing at this joint is already represented by that run. Counting
            // the wall as another arm made the adjoining footing see a through-joint too: the duplicate collinear
            // arm set `through` and put the node back into the cap, breaking the mitre.
            if w.arc == 0.0 {
                let (s, across) = axis.project(node);
                let tangent = axis.tangent(s);
                let lies_under_wall = |rg: &Geom| {
                    let (rlo, rhi) = rg.z_span();
                    if !(rlo - TOL <= m.top_z(w) && m.base_z(w) - TOL <= rhi) {
                        return false;
                    }
                    let a = P2(rg.a[0], rg.a[1]);
                    let b = P2(rg.b[0], rg.b[1]);
                    let (_, a_off) = axis.project(a);
                    let (_, b_off) = axis.project(b);
                    let parallel = tangent.cross(rg.along()).abs() <= TOL / axis.length().max(rg.plan_length()).max(TOL);
                    a_off.abs() <= TOL && b_off.abs() <= TOL && parallel && geom::dist_to_segment(node, a, b) <= rg.width / 2.0 + TOL
                };
                let footing_under_wall = lies_under_wall(g)
                    || near.iter().any(|(_, re)| {
                        if re.category() != category || !re.run().is_some_and(|r| r.join) {
                            return false;
                        }
                        m.run_geom(re).filter(joinable).is_some_and(|rg| lies_under_wall(&rg))
                    });
                if across.abs() <= TOL && footing_under_wall {
                    continue;
                }
            }
            let (ext, int) = walls::faces(m, w);
            if w.start.dist(node) <= TOL {
                arms.push(wall_arm_at(m, w, 0.0, true, false));
            } else if w.end.dist(node) <= TOL {
                arms.push(wall_arm_at(m, w, axis.length(), false, false));
            } else {
                let (s, across) = axis.project(node);
                if s > TOL && s < axis.length() - TOL && (int - TOL..=ext + TOL).contains(&across) && axis.distance(node, across) <= TOL {
                    arms.push(wall_arm_at(m, w, s, true, false));
                    arms.push(wall_arm_at(m, w, s, false, false));
                    through = true;
                }
            }
        }
    }
    arms.sort_by(|a, b| a.way.1.atan2(a.way.0).total_cmp(&b.way.1.atan2(b.way.0)));
    (arms, through)
}

/// Run `id`'s cap at its start (or with `at_end`, at its end): the corners closing it from its face on the
/// clockwise side of the way it leaves the node round to the other, exactly as a wall's [`walls::cap`] closes
/// its own — faces crossing near the node (no farther than the widest arm is wide: past that the end goes
/// square), the node itself between them where several meet there, and just the two faces where it ends on one.
fn cap(m: &Model, id: Id, at_end: bool, g: &Geom, index: Option<&walls::Index>) -> Vec<P2> {
    let (arms, through) = arms(m, id, at_end, g, index);
    let n = arms.len();
    let Some(i) = arms.iter().position(|a| a.mine) else { return Vec::new() };
    let (prev, me, next) = (&arms[(i + n - 1) % n], &arms[i], &arms[(i + 1) % n]);
    if n < 2 {
        return vec![me.cw, me.ccw];
    }
    let node = if at_end { P2(g.b[0], g.b[1]) } else { P2(g.a[0], g.a[1]) };
    let limit = arms.iter().map(|a| a.thick).fold(0.0, f64::max);
    let meet = |face: P2, other: P2, other_way: P2| {
        geom::line_intersection(face, face + me.way, other, other + other_way)
            .filter(|c| (-limit..=limit).contains(&(*c - node).dot(me.way)))
            .unwrap_or(face)
    };
    let (cw, ccw) = (meet(me.cw, prev.ccw, prev.way), meet(me.ccw, next.cw, next.way));
    if through { vec![cw, ccw] } else { vec![cw, node, ccw] }
}

/// Run `id`'s outline in plan: its caps and its faces, mitred against the runs and (of a strip footing) the walls
/// they meet, as [`crate::walls::outline`] is of walls; its plain rectangle where it joins nothing there, its
/// `join` is off, or it doesn't lie flat. The line from `start` to `end` never changes: the join only shapes what
/// the plan fills, 3D cuts and IFC sweeps.
pub fn outline(m: &Model, id: Id) -> Vec<P2> {
    outline_in(m, id, None)
}

/// [`outline`], its joins found through `index` when there is one.
pub(crate) fn outline_in(m: &Model, id: Id, index: Option<&walls::Index>) -> Vec<P2> {
    let Some(e) = m.elements.get(id) else { return Vec::new() };
    let Some(g) = m.run_geom(e) else { return Vec::new() };
    if !joinable(&g) {
        return g.footprint();
    }
    // Its faces are straight, so the caps alone carry the corners, as a straight wall's caps do: each cap's last
    // point lies on the face the next cap's first lies on, and the ring closes along the other face.
    let (start, end) = (cap(m, id, false, &g, index), cap(m, id, true, &g, index));
    // Nothing joins it at either end: the plain rectangle, exactly as it was drawn before it could join.
    if plain(&g, false, &start) && plain(&g, true, &end) {
        return g.footprint();
    }
    let mut ring = start;
    ring.extend(end);
    ring.dedup_by(|a, b| a.dist(*b) < 1e-6);
    // Counter-clockwise, as [`Geom::footprint`] gives it, for the fills and prisms built from it.
    if ring.len() >= 3 && geom::area(&ring) < 0.0 {
        ring.reverse();
    }
    ring
}

/// Whether `cap` is just this end's own two corners: nothing joined there, so the rectangle (or the tube)
/// already ends where the outline does and nothing needs cutting.
fn plain(g: &Geom, at_end: bool, cap: &[P2]) -> bool {
    let square = run_arm(g, at_end, true);
    cap.len() == 2 && cap[0].dist(square.cw) <= 1e-6 && cap[1].dist(square.ccw) <= 1e-6
}

/// One end's cap of run `id` where it joins something: the corners closing it, as [`cap`] gives them. None where the
/// run doesn't lie flat, or where the cap is its plain square end.
pub(crate) fn joined_cap(m: &Model, id: Id, at_end: bool, g: &Geom, index: Option<&walls::Index>) -> Option<Vec<P2>> {
    if !joinable(g) {
        return None;
    }
    let cap = cap(m, id, at_end, g, index);
    (cap.len() >= 2 && !plain(g, at_end, &cap)).then_some(cap)
}

/// One end's cap of run `id` as the vertical planes a round run's body is cut by: each segment of the cap,
/// and the run's point at the other end, which side of the plane is its inside. None where the run doesn't lie
/// flat, or where the cap is its plain square end — the tube already ends there, and stays the one shape a view
/// shares between round runs of a material. The same plane twice (a mitre of equal widths, whose cap is one
/// straight line through the node) is taken once.
pub(crate) fn end_cuts(m: &Model, id: Id, at_end: bool, g: &Geom, index: Option<&walls::Index>) -> Option<Vec<(P2, P2, P2)>> {
    let cap = joined_cap(m, id, at_end, g, index)?;
    let inside = if at_end { P2(g.a[0], g.a[1]) } else { P2(g.b[0], g.b[1]) };
    let cuts = cap_cuts(&cap, inside);
    (!cuts.is_empty()).then_some(cuts)
}

/// The planes of `cap`'s segments, as [`end_cuts`] gives them, each with `inside`, the run's point at its other end.
fn cap_cuts(cap: &[P2], inside: P2) -> Vec<(P2, P2, P2)> {
    let mut cuts: Vec<(P2, P2, P2)> = Vec::new();
    for seg in cap.windows(2) {
        let (p0, p1) = (seg[0], seg[1]);
        if p0.dist(p1) <= 1e-9 {
            continue;
        }
        // A segment of one straight cap line is the same plane again: one cut is enough.
        let same_line = cuts.iter().any(|(q0, q1, _)| {
            let (q0, q1) = (*q0, *q1);
            let (d, e) = (p1 - p0, q1 - q0);
            d.cross(e).abs() <= 1e-9 * d.length() * e.length() && (q0 - p0).cross(d).abs() <= 1e-6 * d.length()
        });
        if !same_line {
            cuts.push((p0, p1, inside));
        }
    }
    cuts
}

/// `ring`, a footprint in plan, cut by the upright plane of a cut of [`end_cuts`], (p0, p1, inside): the side `inside`
/// lies on kept, as [`clip_tube`] keeps it of a mesh.
fn cut_ring(ring: &[P2], (p0, p1, inside): (P2, P2, P2)) -> Vec<P2> {
    let n = (p1 - p0).perp();
    let n = if (inside - p0).dot(n) < 0.0 { n * -1.0 } else { n };
    let d = |q: P2| (q - p0).dot(n);
    let mut out = Vec::with_capacity(ring.len() + 1);
    for (&cur, &next) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        let (dc, dn) = (d(cur), d(next));
        if dc >= 0.0 {
            out.push(cur);
        }
        if (dc > 0.0 && dn < 0.0) || (dc < 0.0 && dn > 0.0) {
            out.push(cur + (next - cur) * (dc / (dc - dn)));
        }
    }
    out.dedup_by(|x, y| x.dist(*y) < 1e-9);
    if out.len() > 1 && out.first().zip(out.last()).is_some_and(|(f, l)| f.dist(*l) < 1e-9) {
        out.pop();
    }
    out
}

/// `tris` (a round run's tube, its first `caps` its flat end faces) cut by the planes of `cuts` — (p0, p1,
/// inside) — the side `inside` lies on kept, each cut open filled back flat: the body ends where the outline
/// says, so plan and 3D never show different joints. The flat faces stay first, for `caps` to count them.
pub(crate) fn clip_tube(tris: Vec<[P3; 3]>, caps: usize, cuts: &[(P2, P2, P2)]) -> (Vec<[P3; 3]>, usize) {
    /// On the plane, or near enough for a point of it: coordinates run to 1e9 mm, where the rounding of a
    /// crossing is still far below anything the model holds.
    const EPS: f64 = 1e-6;
    type Vertex = (u64, u64);
    type Edge = (Vertex, Vertex);
    let plan = |q: P3| P2(q[0], q[1]);
    let key = |q: P3| -> Vertex {
        let n = std::array::from_fn::<_, 2, _>(|i| if q[i] == 0.0 { 0.0 } else { q[i] }.to_bits());
        (n[0], n[1])
    };
    let edge = |a: P3, b: P3| -> Edge {
        let (ka, kb) = (key(a), key(b));
        if ka <= kb { (ka, kb) } else { (kb, ka) }
    };
    // Where a plane cuts an edge: cached by the edge's own vertices, so the two triangles that share an edge
    // fill it in with the very same point and the mesh stays welded (each is clipped on its own).
    let mut cut_at: std::collections::HashMap<Edge, P3> = std::collections::HashMap::new();
    let mut flat: Vec<[P3; 3]> = tris[..caps].to_vec();
    let mut sides: Vec<[P3; 3]> = tris[caps..].to_vec();
    for &(p0, p1, inside) in cuts {
        let n = (p1 - p0).perp();
        let nd = if (inside - p0).dot(n) < 0.0 { n * -1.0 } else { n };
        let d = |q: P3| (plan(q) - p0).dot(nd);
        // The rim of the cut face: the points of the clipped polygons that lie in the plane and are
        // joined there to another of them. The face a convex body takes through a plane is convex, so
        // ordering the rim about its middle closes the loop whatever the triangles under it were — no
        // chain to guess at, and the same order every time a scene is built.
        let mut rim: std::collections::BTreeMap<(i64, i64, i64), P3> = std::collections::BTreeMap::new();
        let grid = |q: P3| -> (i64, i64, i64) {
            let k = std::array::from_fn::<_, 3, _>(|i| (q[i] * 1e6).round() as i64);
            (k[0], k[1], k[2])
        };
        // One triangle kept where it lies, cut into a fan where the plane crosses it.
        let mut clip = |t: [P3; 3]| -> Vec<[P3; 3]> {
            let mut poly = Vec::with_capacity(4);
            for i in 0..3 {
                let (cur, next) = (t[i], t[(i + 1) % 3]);
                let (dc, dn) = (d(cur), d(next));
                if dc >= -EPS {
                    poly.push(cur);
                }
                if (dc > EPS && dn < -EPS) || (dc < -EPS && dn > EPS) {
                    let at = *cut_at.entry(edge(cur, next)).or_insert_with(|| {
                        let f = dc / (dc - dn);
                        std::array::from_fn(|k| cur[k] + (next[k] - cur[k]) * f)
                    });
                    poly.push(at);
                }
            }
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if d(a).abs() <= EPS && d(b).abs() <= EPS {
                    rim.entry(grid(a)).or_insert(a);
                    rim.entry(grid(b)).or_insert(b);
                }
            }
            (1..poly.len().saturating_sub(1)).map(|i| [poly[0], poly[i], poly[i + 1]]).collect()
        };
        for mesh in [&mut flat, &mut sides] {
            let was = std::mem::take(mesh);
            for t in was {
                mesh.extend(clip(t));
            }
        }
        // The cut face shows outward: where the kept side of the plane ends.
        let wanted = [-nd.0, -nd.1, 0.0];
        if rim.len() < 3 {
            continue;
        }
        let middle = rim.values().fold([0.0; 3], |s, p| std::array::from_fn(|i| s[i] + p[i] / rim.len() as f64));
        let along = (p1 - p0).unit();
        let mut ring: Vec<P3> = rim.into_values().collect();
        ring.sort_by(|a, b| {
            let turn = |q: &P3| {
                let s = P2(q[0] - middle[0], q[1] - middle[1]).dot(along);
                (q[2] - middle[2]).atan2(s)
            };
            turn(a).total_cmp(&turn(b))
        });
        // Newell's normal of the loop, turned to the side the cut face shows.
        let n = ring.iter().zip(ring.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| {
            [n[0] + (a[1] - b[1]) * (a[2] + b[2]), n[1] + (a[2] - b[2]) * (a[0] + b[0]), n[2] + (a[0] - b[0]) * (a[1] + b[1])]
        });
        if n[0] * wanted[0] + n[1] * wanted[1] < 0.0 {
            ring.reverse();
        }
        for i in 1..ring.len() - 1 {
            let (b, c) = (ring[i], ring[i + 1]);
            let cross = [
                (b[1] - ring[0][1]) * (c[2] - ring[0][2]) - (b[2] - ring[0][2]) * (c[1] - ring[0][1]),
                (b[2] - ring[0][2]) * (c[0] - ring[0][0]) - (b[0] - ring[0][0]) * (c[2] - ring[0][2]),
                (b[0] - ring[0][0]) * (c[1] - ring[0][1]) - (b[1] - ring[0][1]) * (c[0] - ring[0][0]),
            ];
            if cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2] > 1e-18 {
                flat.push([ring[0], b, c]);
            }
        }
    }
    let caps = flat.len();
    flat.extend(sides);
    (flat, caps)
}

/// A round run of length 1 and radius 1, from the origin along x: its caps' triangles first, and how many they are.
pub fn unit_tube() -> (Vec<[P3; 3]>, usize) {
    let ring = |x: f64| -> Vec<P3> {
        (0..SIDES)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / SIDES as f64;
                [x, t.cos(), t.sin()]
            })
            .collect()
    };
    let (a, b) = ([0.0; 3], [1.0, 0.0, 0.0]);
    let (ra, rb) = (ring(0.0), ring(1.0));
    let next = |k: usize| (k + 1) % SIDES;
    let mut tris = Vec::with_capacity(4 * SIDES);
    for k in 0..SIDES {
        tris.push([a, ra[next(k)], ra[k]]);
        tris.push([b, rb[k], rb[next(k)]]);
    }
    for k in 0..SIDES {
        tris.push([ra[k], ra[next(k)], rb[k]]);
        tris.push([ra[next(k)], rb[next(k)], rb[k]]);
    }
    (tris, 2 * SIDES)
}

/// A piece of a section swept along a run: a prism, its footprint between a bottom plane and a top one; or a closed mesh of
/// triangles turned out, its ends' first, and how many they are.
#[derive(Clone, Debug, PartialEq)]
pub enum Swept {
    Prism(Vec<P2>, Plane, Plane),
    Mesh(Vec<[P3; 3]>, usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> (Model, crate::model::Guid) {
        let m = Model::new("t");
        let level = m.guid(m.levels()[0].0);
        (m, level)
    }

    fn run(level: crate::model::Guid, a: P2, b: P2, offset: f64, end_offset: Option<f64>, justify: Justify) -> Run {
        Run {
            type_name: "t".into(),
            level,
            start: a,
            end: b,
            offset,
            end_offset,
            justify,
            join: true,
            system: String::new(),
            params: Default::default(),
        }
    }

    fn rect(width: f64, depth: f64) -> RunType {
        RunType { width, depth, round: false, material: String::new(), profile: None }
    }

    fn volume(tris: &[[P3; 3]]) -> f64 {
        // The divergence theorem: the sum of the signed volumes of the tetrahedra to the origin; positive if turned out.
        tris.iter().map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c)) / 6.0).sum()
    }

    #[test]
    fn a_runs_heights_are_those_of_its_justify_line() {
        let (m, level) = model();
        let t = rect(200.0, 500.0);
        let at = |j| Geom::of(&m, &run(level, P2(0.0, 0.0), P2(6000.0, 0.0), 2800.0, None, j), &t);
        // The top of the section is at 2800 for a top, the bottom for a bottom, the middle for a centre.
        assert_eq!(at(Justify::Top).z_span(), (2300.0, 2800.0));
        assert_eq!(at(Justify::Bottom).z_span(), (2800.0, 3300.0));
        assert_eq!(at(Justify::Centre).z_span(), (2550.0, 3050.0));
        assert_eq!((at(Justify::Top).a, at(Justify::Top).b), ([0.0, 0.0, 2550.0], [6000.0, 0.0, 2550.0]));
        assert_eq!(at(Justify::Top).length(), 6000.0);
    }

    #[test]
    fn a_sloped_run_keeps_its_section_square_to_its_axis() {
        let (m, level) = model();
        // 3000 along, 4000 up the ends' difference ... a 3-4-5: 3000 in plan, 4000 up, 5000 along.
        let g = Geom::of(&m, &run(level, P2(0.0, 0.0), P2(3000.0, 0.0), 0.0, Some(4000.0), Justify::Centre), &rect(100.0, 200.0));
        assert_eq!(g.length(), 5000.0);
        // Vertically its section is 200 / cos θ = 200 / 0.6 deep, so the prism's thickness is that, on each side of the line.
        let (ring, bottom, top) = g.prism();
        let q = ring[0];
        assert!((top.at(q) - bottom.at(q) - 200.0 / 0.6).abs() < 1e-9, "{} {}", top.at(q), bottom.at(q));
        // Its volume is the section's area times its length, whatever its slope (its ends are cut upright).
        let plan = crate::geom::area(&ring);
        let v = plan * (top.at(P2(1500.0, 0.0)) - bottom.at(P2(1500.0, 0.0)));
        assert!((v - 100.0 * 200.0 * 5000.0).abs() < 1.0, "{v}");
        assert!(plan > 0.0, "counter-clockwise");
        // The planes are over the line wherever it starts: 500 up at its start, 4500 at its end, 200 / 0.6 / 2 each way.
        let g =
            Geom::of(&m, &run(level, P2(1000.0, 2000.0), P2(4000.0, 2000.0), 500.0, Some(4500.0), Justify::Centre), &rect(100.0, 200.0));
        let (_, bottom, top) = g.prism();
        let half = 100.0 / 0.6;
        assert!(
            (bottom.at(P2(1000.0, 2000.0)) - (500.0 - half)).abs() < 1e-9 && (top.at(P2(4000.0, 2000.0)) - (4500.0 + half)).abs() < 1e-9
        );
    }

    #[test]
    fn a_run_a_millimetre_long_in_plan_stands_and_a_shorter_one_is_too_short() {
        let (m, level) = model();
        let at = |to: P2, end_offset| Geom::of(&m, &run(level, P2(0.0, 0.0), to, 0.0, end_offset, Justify::Centre), &rect(100.0, 200.0));
        assert!(at(P2(TOL, 0.0), Some(500.0)).vertical() && !at(P2(TOL + 0.01, 0.0), Some(500.0)).vertical());
        assert!(check("a beam 1", "beam", &at(P2(TOL, 0.0), None)).unwrap_err().contains("too short"), "a millimetre is too short");
        assert!(check("a beam 2", "beam", &at(P2(0.5, 0.0), None)).unwrap_err().contains("too short"));
        assert!(check("a beam 3", "beam", &at(P2(2.0, 0.0), None)).is_ok());
    }

    #[test]
    fn a_footprint_is_a_strip_or_when_upright_its_section() {
        let (m, level) = model();
        let strip = Geom::of(&m, &run(level, P2(0.0, 0.0), P2(4000.0, 0.0), 0.0, None, Justify::Top), &rect(300.0, 500.0)).footprint();
        assert_eq!(strip, vec![P2(0.0, -150.0), P2(4000.0, -150.0), P2(4000.0, 150.0), P2(0.0, 150.0)]);
        // Upright: its section, width along east and depth along north, round a point; its heights from end to end.
        let up = Geom::of(&m, &run(level, P2(1000.0, 1000.0), P2(1000.0, 1000.0), 0.0, Some(3000.0), Justify::Centre), &rect(300.0, 500.0));
        assert!(up.vertical());
        assert_eq!(up.length(), 3000.0);
        assert_eq!(up.footprint(), vec![P2(850.0, 750.0), P2(1150.0, 750.0), P2(1150.0, 1250.0), P2(850.0, 1250.0)]);
        assert_eq!(up.z_span(), (0.0, 3000.0));
        let (ring, bottom, top) = up.prism();
        assert_eq!((ring.len(), bottom.at(ring[0]), top.at(ring[0])), (4, 0.0, 3000.0));
    }

    #[test]
    fn a_round_run_is_a_closed_tube_turned_out() {
        let (m, level) = model();
        let round = RunType { width: 200.0, depth: 200.0, round: true, material: String::new(), profile: None };
        // From a point off the origin, where a cap turned the wrong way doesn't cancel in the volume.
        let from = P2(1000.0, -2000.0);
        for (by, end_offset) in
            [(P2(5000.0, 0.0), None), (P2(3000.0, 4000.0), Some(1000.0)), (P2(0.0, 0.0), Some(2000.0)), (P2(-3000.0, 1000.0), Some(-700.0))]
        {
            let g = Geom::of(&m, &run(level, from, from + by, 500.0, end_offset, Justify::Centre), &round);
            let (tris, caps) = g.tube();
            assert_eq!((tris.len(), caps), (4 * SIDES, 2 * SIDES));
            // The area of a regular polygon of SIDES sides in a circle of radius 100.
            let area = SIDES as f64 / 2.0 * 100.0 * 100.0 * (std::f64::consts::TAU / SIDES as f64).sin();
            assert!((volume(&tris) - area * g.length()).abs() < 1e-3 * area * g.length(), "{} {}", volume(&tris), area * g.length());
        }
    }

    #[test]
    fn a_runs_reach_covers_its_section_square_to_its_axis() {
        let (m, level) = model();
        let g = Geom::of(&m, &run(level, P2(0.0, 0.0), P2(3000.0, 0.0), 0.0, Some(4000.0), Justify::Centre), &rect(100.0, 200.0));
        let (lo, hi) = g.z_span();
        assert!((lo - -100.0 / 0.6).abs() < 1e-9 && (hi - (4000.0 + 100.0 / 0.6)).abs() < 1e-9, "{lo} {hi}");
    }
}
