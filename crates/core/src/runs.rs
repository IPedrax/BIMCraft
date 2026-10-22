//! Runs: the geometry of a straight element between two points, whatever it carries (a beam, a strip footing, a pipe, a
//! conduit, a cable tray, a wire, a conductor, a cable): the ends of its centre line, its footprint in plan, the prism or the
//! tube it makes in 3D. It knows no category but for the table that gives its IFC class and the defaults it takes (may it stand,
//! where its heights are); the plan, the section, the 3D view, the snaps and IFC ask it.

use crate::geom::P2;
use crate::model::{Justify, MAX_SLOPE, Model, Run, RunType, TOL, heading};
use crate::view3d::{P3, add, cross, mul, sub, unit};
use crate::walls::Plane;

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

    /// A round run as a tube of triangles turned out: its two caps' first, and how many they are.
    pub fn tube(&self) -> (Vec<[P3; 3]>, usize) {
        let (d, r) = (unit(sub(self.b, self.a)), self.width / 2.0);
        let e1 = if self.vertical() { [1.0, 0.0, 0.0] } else { unit([-d[1], d[0], 0.0]) };
        let e2 = cross(d, e1);
        let ring = |c: P3| -> Vec<P3> {
            (0..SIDES)
                .map(|k| {
                    let t = std::f64::consts::TAU * k as f64 / SIDES as f64;
                    add(c, add(mul(e1, r * t.cos()), mul(e2, r * t.sin())))
                })
                .collect()
        };
        let (ra, rb) = (ring(self.a), ring(self.b));
        let next = |k: usize| (k + 1) % SIDES;
        let mut tris = Vec::with_capacity(4 * SIDES);
        for k in 0..SIDES {
            tris.push([self.a, ra[next(k)], ra[k]]);
            tris.push([self.b, rb[k], rb[next(k)]]);
        }
        for k in 0..SIDES {
            tris.push([ra[k], ra[next(k)], rb[k]]);
            tris.push([ra[next(k)], rb[next(k)], rb[k]]);
        }
        (tris, 2 * SIDES)
    }
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
            system: String::new(),
            params: Default::default(),
        }
    }

    fn rect(width: f64, depth: f64) -> RunType {
        RunType { width, depth, round: false, material: String::new() }
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
        let round = RunType { width: 200.0, depth: 200.0, round: true, material: String::new() };
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
