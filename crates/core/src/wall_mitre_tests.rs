//! Face crossings at the thickness limit are corners, not shallow-angle wedges.

use serde_json::json;

use crate::engine::Session;
use crate::geom::P2;

fn corner(origin: P2, end: P2) -> (Session, u64) {
    let mut s = Session::new();
    let made = s
        .execute(
            "wall",
            &json!({"points": [origin + P2(-2400.0, -3200.0), origin, origin + end],
                "type": "Exterior - 300mm", "location": "exterior-finish"}),
        )
        .unwrap();
    (s, made["ids"][0].as_u64().unwrap())
}

fn assert_cap(s: &Session, wall: u64, expected: [P2; 3]) {
    let cap = crate::walls::cap(&s.model, wall, true);
    assert_eq!(cap.len(), expected.len());
    for (actual, expected) in cap.iter().zip(expected) {
        assert!(actual.dist(expected) < 1e-4, "expected {expected:?}, got {cap:?}");
    }
}

#[test]
fn mitre_boundary_keeps_the_exact_thickness_crossing() {
    let (s, wall) = corner(P2(0.0, 0.0), P2(-3200.0, 2400.0));
    // A 3:4:5 right-angle L: inward normals are (4/5, -3/5) and (3/5, 4/5).
    // Their 300 mm faces cross at their sum, (420, 60), exactly 300 mm along each wall.
    assert_cap(&s, wall, [P2(0.0, 0.0), P2(0.0, 0.0), P2(420.0, 60.0)]);
}

#[test]
fn mitre_boundary_is_unchanged_at_large_coordinates() {
    let origin = P2(10_000_000.125, -20_000_000.25);
    let (s, wall) = corner(origin, P2(-3200.0, 2400.0));
    assert_cap(&s, wall, [origin, origin, origin + P2(420.0, 60.0)]);
}

#[test]
fn mitre_boundary_does_not_admit_a_shallow_angle_wedge() {
    let (s, wall) = corner(P2(0.0, 0.0), P2(-2500.0, -3000.0));
    // The interior faces cross far beyond 300 mm: retain the square end on its own face.
    assert_cap(&s, wall, [P2(0.0, 0.0), P2(0.0, 0.0), P2(240.0, -180.0)]);
}
