//! A reusable four-point panel, as one is made in Revit: four reference points on the family's axes (one of them raised, so the
//! panel is warped), made adaptive and oriented to the instance, a closed profile through them extruded, and a point hosted
//! halfway along a reference line.
use super::*;
pub fn four_point_panel() -> Definition {
    let mut nodes = Vec::new();
    for (index, origin) in [(1, [0., 0., 0.]), (2, [1200., 0., 120.]), (3, [1200., 900., 240.]), (4, [0., 900., 0.])] {
        nodes.push(Node {
            id: format!("p{index}"),
            name: format!("Placement {index}"),
            operation: Operation::Point {
                host: Host::Adaptive { number: index, frame: Frame::at(origin), orients: OrientsTo::Instance },
                transform: Transform::default(),
            },
            appearance: Appearance::default(),
        });
    }
    let mut push = |id: &str, operation| nodes.push(Node { id: id.into(), name: id.into(), operation, appearance: Appearance::default() });
    push("outline", Operation::Polyline { points: (1..=4).map(|i| format!("p{i}")).collect(), closed: true });
    push("panel", Operation::Extrude { chain: "outline".into(), distance: Expr::Number(60.), direction: None });
    push("reference", Operation::Line { a: "p1".into(), b: "p2".into() });
    push(
        "midpoint",
        Operation::Point {
            host: Host::Curve {
                curve: "reference".into(),
                location: CurveLocation { measurement: Measurement::NormalizedSegmentLength, value: Expr::Number(0.5), from_end: false },
                orientation: Orientation::ProjectInterpolatedNormals,
            },
            transform: Transform::default(),
        },
    );
    Definition { nodes, ..Definition::default() }
}
