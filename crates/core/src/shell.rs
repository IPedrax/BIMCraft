//! The roof a building's tags ask for, as the roofs BIMCraft makes: flat, hipped (a pyramid is a hip on a square) and gabled. Pure:
//! `building_shell` turns the plan this makes into a roof. A footprint is a ring of points in mm; nothing here touches a model.

use crate::geom::{self, P2};

/// The slope, degrees, of a pitched roof that is told neither its angle nor its height.
pub const DEFAULT_SLOPE: f64 = 30.0;

/// A roof as BIMCraft makes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Flat,
    /// Every edge slopes up to a ridge or a point.
    Hipped,
    /// The edges at the two ends stay upright; the other two slope up to a ridge between them.
    Gabled,
}

/// What a roof is made of: its shape, the edges (numbered from the footprint's first point) that stay upright, and what was not as
/// asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub shape: Shape,
    pub gables: Vec<usize>,
    pub notes: Vec<String>,
}

/// The smallest rectangle round a footprint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rectangle {
    pub centre: P2,
    /// The unit direction of its longer side.
    pub along: P2,
    pub long: f64,
    pub short: f64,
    /// How much of it the footprint fills, 0 to 1: a rectangle fills it.
    pub fill: f64,
}

/// The smallest rectangle round `ring`, none for a ring with no area.
pub fn rectangle(ring: &[P2]) -> Option<Rectangle> {
    let hull = geom::hull(ring.to_vec());
    let n = hull.len();
    if n < 3 {
        return None;
    }
    let mut best: Option<(f64, Rectangle)> = None;
    for i in 0..n {
        let u = (hull[(i + 1) % n] - hull[i]).unit();
        let v = u.perp();
        if !(u.0.is_finite() && u.1.is_finite()) {
            continue;
        }
        let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in &hull {
            let (a, b) = (p.dot(u), p.dot(v));
            (u0, u1, v0, v1) = (u0.min(a), u1.max(a), v0.min(b), v1.max(b));
        }
        let (du, dv) = (u1 - u0, v1 - v0);
        let area = du * dv;
        if area > 0.0 && best.is_none_or(|(a, _)| area < a) {
            let centre = u * ((u0 + u1) / 2.0) + v * ((v0 + v1) / 2.0);
            let (along, long, short) = if du >= dv { (u, du, dv) } else { (v, dv, du) };
            best = Some((area, Rectangle { centre, along, long, short, fill: 0.0 }));
        }
    }
    let (area, mut r) = best?;
    r.fill = (geom::area(ring).abs() / area).clamp(0.0, 1.0);
    Some(r)
}

/// The roof asked for by `roof:shape` (`tag`) and `roof:orientation` (`orientation`: `along`, the ridge parallel to the footprint's
/// longer side, which is what none means, or `across`), over `ring`. A shape BIMCraft does not make is a flat roof, and a gabled roof over a
/// footprint that is not near a rectangle is a hipped one; each is said in the notes.
pub fn plan(ring: &[P2], tag: &str, orientation: &str) -> Plan {
    let tag = tag.trim().to_lowercase();
    let made = |shape, gables: Vec<usize>, notes: Vec<String>| Plan { shape, gables, notes };
    match tag.as_str() {
        "" | "flat" => made(Shape::Flat, Vec::new(), Vec::new()),
        "hipped" | "pyramidal" => made(Shape::Hipped, Vec::new(), Vec::new()),
        "gabled" => gabled(ring, orientation),
        other => {
            let shown: String = other.chars().take(40).collect();
            made(
                Shape::Flat,
                Vec::new(),
                vec![format!("roof:shape `{shown}` is not made (flat, hipped, pyramidal and gabled are): a flat roof")],
            )
        }
    }
}

fn gabled(ring: &[P2], orientation: &str) -> Plan {
    let hipped =
        |why: &str| Plan { shape: Shape::Hipped, gables: Vec::new(), notes: vec![format!("{why}: a hipped roof instead of a gabled one")] };
    let Some(rect) = rectangle(ring).filter(|r| r.fill >= 0.8) else {
        return hipped("its footprint is not near a rectangle");
    };
    let across = orientation == "across";
    let ridge = if across { rect.along.perp() } else { rect.along };
    let extent = if across { rect.short } else { rect.long };
    let n = ring.len();
    let (mut gables, mut ends) = (Vec::new(), (false, false));
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let d = (b - a).unit();
        if !(d.0.is_finite() && d.1.is_finite()) {
            continue;
        }
        // An edge across the ridge, at one of its ends.
        let t = ((a + b) * 0.5 - rect.centre).dot(ridge);
        if d.dot(ridge).abs() < 0.35 && t.abs() >= 0.4 * extent {
            gables.push(i);
            if t > 0.0 {
                ends.0 = true;
            } else {
                ends.1 = true;
            }
        }
    }
    if !(ends.0 && ends.1) {
        return hipped("its footprint has no edge across the ridge at each end");
    }
    Plan { shape: Shape::Gabled, gables, notes: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Vec<P2> {
        vec![P2(0.0, 0.0), P2(w, 0.0), P2(w, h), P2(0.0, h)]
    }

    #[test]
    fn the_smallest_rectangle_round_a_footprint_is_found_whichever_way_it_is_turned() {
        let r = rectangle(&rect(10_000.0, 6_000.0)).unwrap();
        assert!((r.long - 10_000.0).abs() < 1e-6 && (r.short - 6_000.0).abs() < 1e-6 && (r.fill - 1.0).abs() < 1e-9);
        assert!(r.along.0.abs() > 0.999, "the long side runs east-west: {:?}", r.along);
        // The same turned 30 degrees.
        let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
        let turned: Vec<P2> = rect(10_000.0, 6_000.0).iter().map(|p| P2(p.0 * c - p.1 * s, p.0 * s + p.1 * c)).collect();
        let r = rectangle(&turned).unwrap();
        assert!((r.long - 10_000.0).abs() < 1e-6 && (r.short - 6_000.0).abs() < 1e-6, "{r:?}");
        assert!((r.along.dot(P2(c, s)).abs() - 1.0).abs() < 1e-9);
        // An L fills part of it; a sliver or a point has none.
        let l =
            vec![P2(0.0, 0.0), P2(10_000.0, 0.0), P2(10_000.0, 3_000.0), P2(3_000.0, 3_000.0), P2(3_000.0, 10_000.0), P2(0.0, 10_000.0)];
        assert!(rectangle(&l).unwrap().fill < 0.7);
        assert!(rectangle(&[P2(0.0, 0.0), P2(1.0, 1.0), P2(2.0, 2.0)]).is_none());
        assert!(rectangle(&[]).is_none());
        assert!(rectangle(&[P2(f64::NAN, 0.0), P2(1.0, 0.0), P2(1.0, 1.0)]).is_none_or(|r| r.long.is_finite()));
    }

    #[test]
    fn a_shape_the_tag_names_is_the_roof_made_and_the_rest_are_flat_and_said() {
        let ring = rect(10_000.0, 6_000.0);
        for (tag, shape) in
            [("", Shape::Flat), ("flat", Shape::Flat), (" Hipped ", Shape::Hipped), ("pyramidal", Shape::Hipped), ("gabled", Shape::Gabled)]
        {
            let p = plan(&ring, tag, "");
            assert_eq!((p.shape, p.notes.is_empty()), (shape, true), "{tag}");
        }
        for tag in ["skillion", "dome", "onion", "mansard", "gambrel", "round", "saltbox", "half-hipped", "nonsense"] {
            let p = plan(&ring, tag, "");
            assert_eq!(p.shape, Shape::Flat, "{tag}");
            assert!(p.notes[0].contains(tag), "{:?}", p.notes);
        }
        assert!(plan(&ring, &"x".repeat(1000), "").notes[0].len() < 200, "a long tag is cut in the note");
    }

    #[test]
    fn a_gabled_roof_stands_its_short_ends_upright_unless_told_across() {
        let ring = rect(10_000.0, 6_000.0);
        // Edges: 0 south (10 m), 1 east (6 m), 2 north (10 m), 3 west (6 m).
        let along = plan(&ring, "gabled", "along");
        assert_eq!((along.shape, along.gables.as_slice()), (Shape::Gabled, &[1, 3][..]));
        assert_eq!(plan(&ring, "gabled", "").gables, [1, 3], "along is the default");
        let across = plan(&ring, "gabled", "across");
        assert_eq!((across.shape, across.gables.as_slice()), (Shape::Gabled, &[0, 2][..]));
        // Turned, with the ring started elsewhere: the same edges by length.
        let (c, s) = (40f64.to_radians().cos(), 40f64.to_radians().sin());
        let turned: Vec<P2> =
            [rect(6_000.0, 10_000.0)[1], rect(6_000.0, 10_000.0)[2], rect(6_000.0, 10_000.0)[3], rect(6_000.0, 10_000.0)[0]]
                .iter()
                .map(|p| P2(p.0 * c - p.1 * s, p.0 * s + p.1 * c))
                .collect();
        let p = plan(&turned, "gabled", "along");
        assert_eq!(p.shape, Shape::Gabled);
        let len = |i: usize| turned[(i + 1) % 4].dist(turned[i]);
        assert!(p.gables.iter().all(|i| (len(*i) - 6_000.0).abs() < 1e-6), "{:?}", p.gables);
        assert_eq!(p.gables.len(), 2);
    }

    #[test]
    fn a_gabled_roof_over_a_footprint_that_is_not_a_rectangle_is_hipped_and_says_so() {
        let l =
            vec![P2(0.0, 0.0), P2(10_000.0, 0.0), P2(10_000.0, 3_000.0), P2(3_000.0, 3_000.0), P2(3_000.0, 10_000.0), P2(0.0, 10_000.0)];
        let p = plan(&l, "gabled", "");
        assert_eq!((p.shape, p.gables.is_empty()), (Shape::Hipped, true));
        assert!(p.notes[0].contains("not near a rectangle"), "{:?}", p.notes);
        // A triangle has no end across a ridge.
        let t = vec![P2(0.0, 0.0), P2(10_000.0, 0.0), P2(5_000.0, 8_000.0)];
        assert_eq!(plan(&t, "gabled", "").shape, Shape::Hipped);
        assert_eq!(plan(&[], "gabled", "").shape, Shape::Hipped);
        assert_eq!(plan(&[], "hipped", "").shape, Shape::Hipped);
    }
}
