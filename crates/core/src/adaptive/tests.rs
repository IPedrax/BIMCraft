use super::frame::norm;
use super::*;
use crate::view3d::sub;
fn scalar(n: f64) -> Expr {
    Expr::Number(n)
}
fn node(id: &str, operation: Operation) -> Node {
    Node { id: id.into(), name: id.into(), operation, appearance: Appearance::default() }
}
fn adaptive(id: &str, number: u32, origin: P3) -> Node {
    node(
        id,
        Operation::Point {
            host: Host::Adaptive { number, frame: Frame::at(origin), orients: OrientsTo::Host },
            transform: Transform::default(),
        },
    )
}
fn xyz(report: &Evaluation, id: &str) -> P3 {
    point(report, id).unwrap().origin
}
fn near(a: P3, b: P3) {
    assert!(norm(sub(a, b)) < 1e-6, "{a:?} != {b:?}");
}
fn panel() -> Definition {
    let mut nodes = vec![
        adaptive("p1", 1, [0., 0., 0.]),
        adaptive("p2", 2, [1000., 0., 0.]),
        adaptive("p3", 3, [1000., 1000., 0.]),
        adaptive("p4", 4, [0., 1000., 0.]),
    ];
    nodes.push(node("outline", Operation::Polyline { points: (1..=4).map(|i| format!("p{i}")).collect(), closed: true }));
    nodes.push(node("panel", Operation::Extrude { chain: "outline".into(), distance: scalar(100.), direction: None }));
    Definition { nodes, ..Definition::default() }
}
#[test]
fn full_frame_offsets_need_no_cascade() {
    let parent = Frame::at([10., 20., 30.]).transformed([0.; 3], [0., 0., 90.]).unwrap();
    let f = parent.transformed([1., 2., 3.], [20., 30., 40.]).unwrap();
    near(f.origin, [8., 21., 33.]);
    f.checked().unwrap();
}
#[test]
fn definition_and_instance_frames_are_independent() {
    let mut d = panel();
    d.nodes.push(node(
        "child",
        Operation::Point {
            host: Host::Frame { point: "p1".into() },
            transform: Transform { offset: [scalar(100.), scalar(0.), scalar(0.)], ..Transform::default() },
        },
    ));
    let placement = Placement {
        handles: BTreeMap::new(),
        bindings: BTreeMap::new(),
        frames: [(1, Frame::at([100., 100., 100.]).transformed([0.; 3], [0., 0., 90.]).unwrap())].into(),
        context: "cylinder sample".into(),
    };
    let result = d.evaluate(&BTreeMap::new(), &placement);
    near(xyz(&result, "child"), [100., 200., 100.]);
    near(xyz(&d.evaluate(&BTreeMap::new(), &d.preview), "child"), [100., 0., 0.]);
}
#[test]
fn child_offsets_resolve_shared_family_parameters() {
    let mut d = panel();
    d.nodes.push(node(
        "child",
        Operation::Point {
            host: Host::Frame { point: "p1".into() },
            transform: Transform { offset: [Expr::Formula("Width / 2".into()), scalar(0.), scalar(0.)], ..Transform::default() },
        },
    ));
    near(xyz(&d.evaluate(&[("Width".into(), Val::Number(400.))].into(), &Placement::default()), "child"), [200., 0., 0.]);
}
#[test]
fn nonparallel_placement_frames_keep_orthonormal_curve_hosts() {
    let mut d = panel();
    d.nodes.push(node("line", Operation::Line { a: "p1".into(), b: "p2".into() }));
    d.nodes.push(node(
        "mid",
        Operation::Point {
            host: Host::Curve {
                curve: "line".into(),
                location: CurveLocation { measurement: Measurement::NormalizedCurveParameter, value: scalar(0.5), from_end: false },
                orientation: Orientation::ProjectInterpolatedNormals,
            },
            transform: Transform::default(),
        },
    ));
    let p = Placement {
        handles: BTreeMap::new(),
        bindings: BTreeMap::new(),
        frames: [
            (1, Frame::at([0., 0., 0.]).transformed([0.; 3], [20., 30., 0.]).unwrap()),
            (2, Frame::at([1000., 0., 0.]).transformed([0.; 3], [-30., 10., 90.]).unwrap()),
        ]
        .into(),
        context: String::new(),
    };
    let report = d.evaluate(&BTreeMap::new(), &p);
    let f = point(&report, "mid").unwrap();
    f.checked().unwrap();
    near(f.origin, [500., 0., 0.]);
    assert!(dot(f.z, f.x).abs() < 1e-8);
}
#[test]
fn antiparallel_normals_do_not_make_zero_frames() {
    let (normal, used) = frame::interpolate_normal([0., 0., 1.], [0., 0., -1.], 0.5).unwrap();
    assert!(used);
    assert!((norm(normal) - 1.).abs() < 1e-8);
}
#[test]
fn tangent_normal_degeneracy_is_deterministic_and_reported() {
    let mut d = Definition {
        nodes: vec![
            adaptive("a", 1, [0., 0., 0.]),
            adaptive("b", 2, [0., 0., 100.]),
            node("l", Operation::Line { a: "a".into(), b: "b".into() }),
        ],
        ..Definition::default()
    };
    d.nodes.push(node(
        "p",
        Operation::Point {
            host: Host::Curve {
                curve: "l".into(),
                location: CurveLocation { measurement: Measurement::NormalizedCurveParameter, value: scalar(0.5), from_end: false },
                orientation: Orientation::ProjectWorldZ,
            },
            transform: Transform::default(),
        },
    ));
    let r = d.evaluate(&BTreeMap::new(), &Placement::default());
    point(&r, "p").unwrap().checked().unwrap();
    assert!(r.diagnostics.iter().any(|d| d.warning));
    assert_eq!(r, d.evaluate(&BTreeMap::new(), &Placement::default()));
}
#[test]
fn distance_and_normalized_parameter_are_not_confused() {
    let c = Curve {
        controls: vec![Frame::at([0., 0., 0.]), Frame::at([10., 0., 0.]), Frame::at([10., 90., 0.])],
        closed: false,
        bezier: false,
    };
    let values = BTreeMap::new();
    let mut location = CurveLocation { measurement: Measurement::NormalizedCurveParameter, value: scalar(0.5), from_end: false };
    near(c.point(c.parameter(&location, &values).unwrap()), [10., 0., 0.]);
    location.measurement = Measurement::NormalizedSegmentLength;
    near(c.point(c.parameter(&location, &values).unwrap()), [10., 40., 0.]);
    location.measurement = Measurement::SegmentLength;
    location.value = scalar(20.);
    location.from_end = true;
    near(c.point(c.parameter(&location, &values).unwrap()), [10., 70., 0.]);
}
#[test]
fn nonplanar_closed_profile_extrudes_without_flattening() {
    let mut d = panel();
    if let Operation::Point { host: Host::Adaptive { frame, .. }, .. } = &mut d.nodes[2].operation {
        frame.origin[2] = 200.;
    }
    let report = d.evaluate(&BTreeMap::new(), &Placement::default());
    assert!(!report.failed(), "{:?}", report.diagnostics);
    let Evaluated::Form { mesh, .. } = &report.values["panel"] else { panic!() };
    assert!(mesh.closed);
    assert!(mesh.signed_volume() > 0.);
    near(mesh.vertices[2], [1000., 1000., 200.]);
}
#[test]
fn loft_accepts_three_ordered_profiles() {
    let base = panel();
    let mut nodes = vec![];
    let mut profiles = vec![];
    for section in 0..3 {
        let mut ids = vec![];
        for i in 0..4 {
            let id = format!("p{section}_{i}");
            let Operation::Point { host: Host::Adaptive { frame, .. }, .. } = &base.nodes[i].operation else { panic!() };
            let mut p = frame.origin;
            p[2] = section as f64 * 1000.;
            nodes.push(adaptive(&id, (section * 4 + i + 1) as u32, p));
            ids.push(id);
        }
        let id = format!("c{section}");
        nodes.push(node(&id, Operation::Polyline { points: ids, closed: true }));
        profiles.push(id);
    }
    nodes.push(node("loft", Operation::Loft { profiles }));
    let report = Definition { nodes, ..Definition::default() }.evaluate(&BTreeMap::new(), &Placement::default());
    assert!(!report.failed(), "{:?}", report.diagnostics);
    let Evaluated::Form { mesh, .. } = &report.values["loft"] else { panic!() };
    assert!((mesh.signed_volume() - 2e9).abs() < 1.);
}
#[test]
fn errors_keep_valid_upstream_geometry_and_replay_exact_placement() {
    let d = panel();
    let placement = Placement {
        handles: BTreeMap::new(),
        bindings: BTreeMap::new(),
        frames: [(2, Frame::at([0., 0., 0.]))].into(),
        context: "failed panel 7".into(),
    };
    let report = d.evaluate(&BTreeMap::new(), &placement);
    assert!(report.failed());
    assert!(report.values.contains_key("p1"));
    assert!(!report.values.contains_key("panel"));
    let saved = serde_json::to_string(&(d.clone(), placement.clone())).unwrap();
    let (definition, placement): (Definition, Placement) = serde_json::from_str(&saved).unwrap();
    assert_eq!(report, definition.evaluate(&BTreeMap::new(), &placement));
}
#[test]
fn cycles_and_duplicate_numbers_are_rejected() {
    let mut d = panel();
    d.nodes.push(node("cycle", Operation::Point { host: Host::Frame { point: "cycle".into() }, transform: Transform::default() }));
    assert!(d.check().unwrap_err().contains("cycle"));
    d.nodes.pop();
    d.nodes.push(adaptive("another", 1, [0.; 3]));
    assert!(d.check().is_err());
}
#[test]
fn subelement_hosts_track_geometry_but_reject_topology_changes() {
    let mut d = panel();
    let r = d.evaluate(&BTreeMap::new(), &Placement::default());
    let Evaluated::Form { mesh, .. } = &r.values["panel"] else { panic!() };
    let topology = mesh.topology();
    d.nodes.push(node(
        "vertex",
        Operation::Point {
            host: Host::Vertex { form: "panel".into(), topology: topology.clone(), vertex: 2 },
            transform: Transform::default(),
        },
    ));
    near(xyz(&d.evaluate(&BTreeMap::new(), &Placement::default()), "vertex"), [1000., 1000., 0.]);
    let p = Placement {
        handles: BTreeMap::new(),
        bindings: BTreeMap::new(),
        frames: [(3, Frame::at([1200., 1000., 0.]))].into(),
        context: String::new(),
    };
    near(xyz(&d.evaluate(&BTreeMap::new(), &p), "vertex"), [1200., 1000., 0.]);
    if let Operation::Point { host: Host::Vertex { topology, .. }, .. } = &mut d.nodes.last_mut().unwrap().operation {
        *topology = "old-topology".into();
    }
    assert!(d.evaluate(&BTreeMap::new(), &p).diagnostics.iter().any(|d| d.message.contains("topology changed")));
}
#[test]
fn intersecting_host_directrices_use_full_adaptive_frames() {
    let mut d = Definition { nodes: vec![adaptive("a", 1, [-10., 0., 0.]), adaptive("b", 2, [0., -10., 0.])], ..Definition::default() };
    d.nodes.push(node(
        "intersection",
        Operation::Intersection { a: "a".into(), axis_a: Axis::X, b: "b".into(), axis_b: Axis::Y, forward_only: true },
    ));
    near(xyz(&d.evaluate(&BTreeMap::new(), &Placement::default()), "intersection"), [0.; 3]);
}

#[test]
fn clockwise_and_counterclockwise_warped_profiles_have_consistent_winding() {
    for reversed in [false, true] {
        let mut d = panel();
        if let Operation::Point { host: Host::Adaptive { frame, .. }, .. } = &mut d.nodes[2].operation {
            frame.origin[2] = 200.;
        }
        if reversed && let Operation::Polyline { points, .. } = &mut d.nodes[4].operation {
            points.reverse();
        }
        let r = d.evaluate(&BTreeMap::new(), &Placement::default());
        assert!(!r.failed(), "{:?}", r.diagnostics);
        let Evaluated::Form { mesh, .. } = &r.values["panel"] else { panic!() };
        assert!(mesh.signed_volume() > 0.);
        let mut edges = BTreeMap::new();
        for [a, b, c] in &mesh.faces {
            for (a, b) in [(*a, *b), (*b, *c), (*c, *a)] {
                let key = (a.min(b), a.max(b));
                *edges.entry(key).or_insert(0) += if a < b { 1 } else { -1 };
            }
        }
        assert!(edges.values().all(|v| *v == 0));
    }
}
#[test]
fn structural_failures_retain_unrelated_geometry_and_name_the_dependency() {
    let mut d = panel();
    d.nodes.push(node("missing", Operation::Line { a: "absent".into(), b: "p1".into() }));
    d.nodes.push(node("cycle", Operation::Point { host: Host::Frame { point: "cycle".into() }, transform: Transform::default() }));
    let r = d.evaluate(&BTreeMap::new(), &Placement::default());
    assert!(r.failed());
    assert!(r.values.contains_key("panel"));
    assert!(r.diagnostics.iter().any(|d| d.node == "absent"));
    assert!(r.diagnostics.iter().any(|d| d.message.contains("cycle")));
}
#[test]
fn hidden_forms_remain_hostable_and_invalid_voids_report_composition_failure() {
    let mut d = panel();
    d.nodes[5].appearance.visible = Some(scalar(0.));
    let r = d.evaluate(&BTreeMap::new(), &Placement::default());
    assert!(r.pieces().is_empty());
    let Evaluated::Form { mesh, .. } = &r.values["panel"] else { panic!() };
    assert_eq!(mesh.vertices.len(), 8);
    d.nodes.push(node("void_patch", Operation::Patch { chain: "outline".into() }));
    d.nodes.last_mut().unwrap().appearance.void = true;
    let r = d.evaluate(&BTreeMap::new(), &Placement::default());
    assert!(r.diagnostics.iter().any(|d| d.node == "void_patch" && d.code == "composition"));
}
#[test]
fn orientation_endpoints_and_rotation_order_are_explicit() {
    let a = [0., 0., 1.];
    let b = frame::unit([0.00001, 0., -1.]).unwrap();
    near(frame::interpolate_normal(a, b, 0.).unwrap().0, a);
    near(frame::interpolate_normal(a, b, 1.).unwrap().0, b);
    let f = Frame::default().transformed([1., 2., 3.], [90., 90., 0.]).unwrap();
    near(f.origin, [1., 2., 3.]);
    near(f.x, [0., 1., 0.]);
    near(f.y, [0., 0., 1.]);
    near(f.z, [1., 0., 0.]);
}
#[test]
fn adaptive_commands_capture_place_replay_modify_undo_and_persist() {
    use crate::engine::Session;
    use crate::model::{Element, Model};
    use serde_json::json;
    let mut s = Session::new();
    s.execute("family", &json!({"name":"Adaptive panel","types":{"A":{}},"adaptive":four_point_panel()})).unwrap();
    let created = s.execute("component", &json!({"family":"Adaptive panel","at":[100,200],"angle":30})).unwrap();
    let id = created["id"].clone();
    let capture=s.execute("adaptive_capture",&json!({"sources":[{"id":id,"host":{"type":"frame","point":"p1"}},{"id":id,"host":{"type":"frame","point":"p2"}},{"id":id,"host":{"type":"frame","point":"p3"}},{"id":id,"host":{"type":"frame","point":"p4"}}]})).unwrap();
    let captured: Placement = serde_json::from_value(capture.clone()).unwrap();
    assert_eq!(captured.frames.len(), 4);
    assert_eq!(captured.bindings.len(), 4);
    near(captured.frames[&1].origin, [100., 200., 0.]);
    assert_eq!(captured.frames[&1].z, captured.frames[&2].z, "points oriented to the instance share its axes");
    let batch = s
        .execute(
            "adaptive_place",
            &json!({"family":"Adaptive panel","placements":[{"at":[0,0],"adaptive":capture},{"at":[3000,0],"adaptive":captured}]}),
        )
        .unwrap();
    assert_eq!(batch["ids"].as_array().unwrap().len(), 2);
    let placed = batch["ids"][0].clone();
    let replay = s.execute("adaptive_evaluate", &json!({"id":placed})).unwrap();
    assert_eq!(replay["placement"], capture);
    let source = s.execute("adaptive_evaluate", &json!({"id":id})).unwrap();
    let mut failed = captured.clone();
    failed.frames.insert(2, failed.frames[&1]);
    s.execute("modify", &json!({"id":placed,"adaptive":failed})).unwrap();
    let failure = s.execute("adaptive_evaluate", &json!({"id":placed})).unwrap();
    assert!(failure["evaluation"]["diagnostics"].as_array().unwrap().iter().any(|d| d["warning"] == false));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.execute("adaptive_evaluate", &json!({"id":placed})).unwrap()["placement"], capture);
    assert_eq!(s.execute("adaptive_evaluate", &json!({"id":id})).unwrap(), source);
    let saved = s.model.to_json().unwrap();
    let (loaded, left_out) = Model::read(&saved).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    loaded.check().unwrap();
    assert!(saved.contains("\"adaptive\""), "the placements are in the file");
    let components =
        loaded.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c) } else { None }).collect::<Vec<_>>();
    assert_eq!(components.len(), 3);
    let mut cache = crate::component_geometry::Components::new(&loaded);
    for c in components {
        assert_eq!(cache.pieces(c), loaded.component_pieces(c));
    }
    let before = s.model.clone();
    assert!(
        s.execute("adaptive_place", &json!({"family":"Adaptive panel","placements":[{"at":[0,0],"adaptive":capture},{"at":[0,0]}]}))
            .is_err()
    );
    assert_eq!(serde_json::to_string(&*s.model).unwrap(), serde_json::to_string(&*before).unwrap());
}

/// Every corner of `pieces`, rounded to a hundredth of a millimetre and sorted, so two builds compare as sets of points.
fn corners(pieces: &[Piece]) -> Vec<[i64; 3]> {
    let mut out: Vec<[i64; 3]> =
        pieces.iter().flat_map(|p| p.tris.iter().flatten()).map(|q| q.map(|v| (v * 100.).round() as i64)).collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn placed_captured_frames_keep_their_world_points_wherever_the_instance_stands() {
    use crate::engine::Session;
    use crate::model::Element;
    use serde_json::json;
    let mut s = Session::new();
    s.execute("family", &json!({"name":"Adaptive panel","types":{"A":{}},"adaptive":four_point_panel()})).unwrap();
    let source = s.execute("component", &json!({"family":"Adaptive panel","at":[100,200],"angle":30,"offset":500})).unwrap()["id"].clone();
    let sources: Vec<_> = (1..=4).map(|n| json!({"id":source,"host":{"type":"frame","point":format!("p{n}")}})).collect();
    let capture = s.execute("adaptive_capture", &json!({ "sources": sources })).unwrap();
    let placed = s
        .execute(
            "adaptive_place",
            &json!({"family":"Adaptive panel","placements":[{"at":[3000,-500],"angle":-45,"offset":250,"adaptive":capture},{"adaptive":capture}]}),
        )
        .unwrap();
    let pieces = |id: &serde_json::Value| {
        let id = id.as_u64().unwrap();
        let Some(Element::Component(c)) = s.model.elements.get(id) else { panic!("{id} is a component") };
        corners(&s.model.component_pieces(c))
    };
    let want = pieces(&source);
    assert!(!want.is_empty());
    for id in placed["ids"].as_array().unwrap() {
        assert_eq!(pieces(id), want, "an instance placed on captured frames builds where they were captured");
    }
    // With no point given, an instance stands at its first frame's.
    let second = placed["ids"][1].as_u64().unwrap();
    let Some(Element::Component(c)) = s.model.elements.get(second) else { panic!() };
    let first: Placement = serde_json::from_value(capture).unwrap();
    assert!((c.at.0 - first.frames[&1].origin[0]).abs() < 1e-6 && (c.at.1 - first.frames[&1].origin[1]).abs() < 1e-6, "{:?}", c.at);
}

#[test]
fn replay_keeps_resolved_parameters_and_near_antipodal_normals_stay_continuous() {
    let mut d = panel();
    if let Operation::Extrude { distance, .. } = &mut d.nodes[5].operation {
        *distance = Expr::Formula("Thickness".into());
    }
    let r = d.evaluate(&[("Thickness".into(), Val::Number(75.))].into(), &Placement::default());
    assert!(!r.failed());
    let saved = serde_json::to_string(&r).unwrap();
    let loaded: Evaluation = serde_json::from_str(&saved).unwrap();
    assert_eq!(loaded.replay(&d), r);
    let a = [0., 0., 1.];
    let b = frame::unit([0.0001, 0., -1.]).unwrap();
    let (near_end, fallback) = frame::interpolate_normal(a, b, 1. - 1e-10).unwrap();
    assert!(!fallback);
    near(near_end, b);
}
