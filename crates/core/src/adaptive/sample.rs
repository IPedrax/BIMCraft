//! A reusable panel with deliberately nonparallel test frames.
use super::*;
pub fn four_point_panel() -> Definition {
    let mut nodes = Vec::new();
    for (index, origin, rotation) in [
        (1, [0., 0., 0.], [0., -20., 0.]),
        (2, [1200., 0., 120.], [0., 20., 0.]),
        (3, [1200., 900., 240.], [15., 20., 0.]),
        (4, [0., 900., 0.], [15., -20., 0.]),
    ] {
        nodes.push(Node {
            id: format!("p{index}"),
            name: format!("Placement {index}"),
            operation: Operation::Point {
                host: Host::Adaptive {
                    number: index,
                    frame: Frame::at(origin).transformed([0.; 3], rotation).unwrap_or_default(),
                    orients: OrientsTo::Host,
                },
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
    push(
        "child",
        Operation::Point {
            host: Host::Frame { point: "p1".into() },
            transform: Transform {
                offset: [Expr::Number(100.), Expr::Number(50.), Expr::Number(100.)],
                rotation: [Expr::Number(10.), Expr::Number(20.), Expr::Number(30.)],
            },
        },
    );
    Definition { nodes, ..Definition::default() }
}
