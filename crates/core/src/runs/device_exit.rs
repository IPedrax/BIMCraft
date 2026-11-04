//! Free contacts on the boundary of an equipment envelope in its mounting plane.

use crate::model::MAX_COORD;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Exit {
    /// Bottom, top, right, left in the equipment's mounting-plane coordinates.
    pub side: usize,
    pub contact: [f64; 2],
    pub landing: [f64; 2],
}

/// Minimise the free lead length over allowed boundary regions and the finite route.
/// Coordinates describe contact in the mounting plane; cable radius belongs to the
/// later conversion to a physical axis, never to this choice of an anchor.
pub(super) fn solve(bounds: [[f64; 2]; 2], route: [[f64; 2]; 2], allowed: [bool; 4]) -> Result<Exit, String> {
    if !bounds.into_iter().chain(route).flatten().all(|v| v.is_finite() && v.abs() <= MAX_COORD)
        || bounds[1][0] - bounds[0][0] < 1e-6
        || bounds[1][1] - bounds[0][1] < 1e-6
        || !allowed.contains(&true)
    {
        return Err("free connection requires finite bounded geometry, a nonempty envelope and allowed sides".into());
    }
    let [[left, bottom], [right, top]] = bounds;
    let sides = [
        ([[left, bottom], [right, bottom]], [0.0, -1.0]),
        ([[left, top], [right, top]], [0.0, 1.0]),
        ([[right, bottom], [right, top]], [1.0, 0.0]),
        ([[left, bottom], [left, top]], [-1.0, 0.0]),
    ];
    let mut best: Option<(f64, Exit)> = None;
    for (side, (edge, normal)) in sides.into_iter().enumerate() {
        if !allowed[side] {
            continue;
        }
        let distance = route.map(|q| dot(sub(q, edge[0]), normal));
        if distance[0] < 0.0 && distance[1] < 0.0 {
            continue;
        }
        // The edge's normal coordinate is constant. Clipping the route therefore
        // enforces the outward constraint for every candidate on the whole edge.
        let mut clipped = route;
        if distance[0] < 0.0 || distance[1] < 0.0 {
            let t = distance[0] / (distance[0] - distance[1]);
            let cut = lerp(route, t);
            clipped[usize::from(distance[1] < 0.0)] = cut;
        }
        for (contact, landing) in closest_candidates(edge, clipped) {
            let delta = sub(landing, contact);
            let length2 = dot(delta, delta);
            if best.as_ref().is_none_or(|(old, _)| length2 < *old) {
                best = Some((length2, Exit { side, contact, landing }));
            }
        }
    }
    best.map(|(_, exit)| exit).ok_or_else(|| "the route cannot be reached from an allowed equipment side without crossing its body".into())
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn lerp(segment: [[f64; 2]; 2], t: f64) -> [f64; 2] {
    [segment[0][0] + (segment[1][0] - segment[0][0]) * t, segment[0][1] + (segment[1][1] - segment[0][1]) * t]
}

fn project(point: [f64; 2], segment: [[f64; 2]; 2]) -> [f64; 2] {
    let delta = sub(segment[1], segment[0]);
    let length2 = dot(delta, delta);
    if length2 == 0.0 { segment[0] } else { lerp(segment, (dot(sub(point, segment[0]), delta) / length2).clamp(0.0, 1.0)) }
}

fn closest_candidates(edge: [[f64; 2]; 2], route: [[f64; 2]; 2]) -> Vec<([f64; 2], [f64; 2])> {
    // The midpoint wins ties for parallel sides, avoiding an arbitrary corner anchor.
    let mut points = Vec::with_capacity(6);
    for contact in [lerp(edge, 0.5), edge[0], edge[1]] {
        points.push((contact, project(contact, route)));
    }
    for landing in route {
        points.push((project(landing, edge), landing));
    }
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let p = sub(edge[1], edge[0]);
    let q = sub(route[1], route[0]);
    let determinant = cross(p, q);
    if determinant != 0.0 {
        let offset = sub(route[0], edge[0]);
        let t = cross(offset, q) / determinant;
        let u = cross(offset, p) / determinant;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            points.push((lerp(edge, t), lerp(route, u)));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: [[f64; 2]; 2] = [[-150.0, 0.0], [150.0, 400.0]];

    fn near(actual: [f64; 2], expected: [f64; 2]) {
        assert!(actual.into_iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-9), "{actual:?} != {expected:?}");
    }

    #[test]
    fn stage4_top_contact_slides_under_each_independent_parallel_route() {
        for x in [-120.0, -40.0, 40.0, 120.0] {
            let exit = solve(PANEL, [[x, 1000.0], [x, 3000.0]], [false, true, false, false]).unwrap();
            assert_eq!(exit.side, 1);
            near(exit.contact, [x, 400.0]);
            near(exit.landing, [x, 1000.0]);
        }
    }

    #[test]
    fn stage4_allowed_sides_are_regions_and_cannot_reach_through_the_body() {
        let route = [[-1000.0, 100.0], [-1000.0, 300.0]];
        assert!(solve(PANEL, route, [false, false, true, false]).is_err());
        let exit = solve(PANEL, route, [false, false, true, true]).unwrap();
        assert_eq!(exit.side, 3);
        assert_eq!(exit.contact, [-150.0, 200.0]);
        assert_eq!(exit.landing, [-1000.0, 200.0]);
        let exit = solve(PANEL, [[1000.0, 230.0]; 2], [true; 4]).unwrap();
        assert_eq!(exit.side, 2);
        near(exit.contact, [150.0, 230.0]);
    }

    #[test]
    fn stage4_route_is_clipped_to_the_allowed_outward_half_plane() {
        let exit = solve(PANEL, [[-500.0, 0.0], [500.0, 1000.0]], [false, false, true, false]).unwrap();
        assert_eq!(exit.contact, [150.0, 400.0]);
        assert_eq!(exit.landing, [150.0, 650.0]);
    }

    #[test]
    fn stage4_free_contact_rejects_invalid_and_unbounded_geometry() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
            assert!(solve(PANEL, [[value, 0.0], [0.0, 0.0]], [true; 4]).is_err());
        }
        assert!(solve(PANEL, [[0.0, 1000.0]; 2], [false; 4]).is_err());
        assert!(solve([[0.0, 0.0]; 2], [[0.0, 1000.0]; 2], [true; 4]).is_err());
    }
}
