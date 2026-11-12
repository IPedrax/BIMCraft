//! Adaptive work planes as the family's reference planes: the three datums an adaptive family had of its own before it had
//! reference planes are the family's datum planes, a point drawn on one names it by GUID, a point that names a datum reads
//! as that plane, and nothing moves.
use super::*;
use crate::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, Face, Family, PlaneRef, Planes, REF_LEVEL};
use serde_json::{Value, json};

fn near(a: P3, b: P3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

/// The old datums as an adaptive point named them, the plane each is now, and the frame `Datum::frame()` gave it, as it was.
fn old_datums() -> [(&'static str, Guid, Frame); 3] {
    [
        ("level", REF_LEVEL, Frame::default()),
        ("front_back", CENTER_FRONT_BACK, Frame { origin: [0.; 3], x: [1., 0., 0.], y: [0., 0., 1.], z: [0., -1., 0.] }),
        ("left_right", CENTER_LEFT_RIGHT, Frame { origin: [0.; 3], x: [0., 1., 0.], y: [0., 0., 1.], z: [1., 0., 0.] }),
    ]
}

type Guid = crate::ids::Guid;

#[test]
fn each_old_datums_frame_is_its_planes_frame_exactly() {
    let family = Family::default();
    for (name, guid, old) in old_datums() {
        let plane = PlaneRef::Plane(guid);
        // Through the family's own planes, and through planes that list none (every family holds the datums).
        for planes in [family.plane_set(), Planes::default()] {
            let f = Frame::from(planes.frame(&plane).expect("a datum has a frame"));
            assert_eq!(f, old, "{name}: its plane's frame is the one the datum had");
        }
        let on = WorkPlane::Plane(plane).frame(&Evaluation::default(), family.plane_set());
        assert_eq!(on, Some(old), "{name}: as a work plane too");
    }
}

#[test]
fn the_default_work_plane_is_ref_level() {
    assert_eq!(WorkPlane::default(), WorkPlane::Plane(PlaneRef::Plane(REF_LEVEL)));
}

/// A point's node, as one from before reference planes was held: on the world, its frame given, and the datum it was drawn on (none, no key).
fn old_point(id: &str, frame: Frame, datum: Option<&str>) -> Value {
    let mut host = json!({"type": "world", "frame": frame});
    if let Some(d) = datum {
        host["datum"] = json!(d);
    }
    json!({"id": id, "name": "Reference point", "operation": {"kind": "point", "host": host}})
}

#[test]
fn old_datum_keys_read_as_the_datum_planes_and_no_datum_stays_none() {
    for (name, guid, frame) in old_datums() {
        let node: Node = serde_json::from_value(old_point("p", frame, Some(name))).expect("an old point reads");
        assert_eq!(
            node.operation,
            Operation::Point { host: Host::World { frame, plane: Some(PlaneRef::Plane(guid)) }, transform: Transform::default() }
        );
        // Written back, it names the plane; read again, the same.
        let again: Node = serde_json::from_value(serde_json::to_value(&node).unwrap()).unwrap();
        assert_eq!(again, node);
        assert!(serde_json::to_string(&node).unwrap().contains(&guid.to_string()));
    }
    let none: Node = serde_json::from_value(old_point("p", Frame::at([1., 2., 3.]), None)).unwrap();
    assert!(matches!(none.operation, Operation::Point { host: Host::World { plane: None, .. }, .. }));
    // A name that was never a datum is refused, as before.
    assert!(serde_json::from_value::<Node>(old_point("p", Frame::default(), Some("roof"))).is_err());
    // A line's face reads too.
    let line = crate::ids::Guid::from_u128(7);
    let v = json!({"id": "p", "operation": {"kind": "point", "host": {"type": "world", "plane": {"line": line, "face": "vertical"}}}});
    let node: Node = serde_json::from_value(v).unwrap();
    assert!(
        matches!(node.operation, Operation::Point { host: Host::World { plane: Some(PlaneRef::Line(g, Face::Vertical)), .. }, .. } if g == line)
    );
}

/// The points of an adaptive family from before reference planes: one on each datum, as the old Definition::add_point_on put them, one with no datum, and one
/// hosted on the Front/Back point's XZ plane at an offset.
fn old_nodes() -> Vec<Value> {
    let [(l, _, level), (fb, _, front), (lr, _, side)] = old_datums();
    let mut nodes = vec![
        old_point("on_level", Frame { origin: [100., 200., 0.], ..level }, Some(l)),
        old_point("on_front", Frame { origin: [500., 0., 700.], ..front }, Some(fb)),
        old_point("on_side", Frame { origin: [0., 600., 700.], ..side }, Some(lr)),
        old_point("free", Frame::at([10., 20., 30.]), None),
    ];
    nodes.push(json!({"id": "hosted", "operation": {"kind": "point", "host": {"type": "plane", "point": "on_front", "plane": "xz"},
        "transform": {"offset": [40, 50, 0]}}}));
    nodes
}

#[test]
fn an_adaptive_family_from_before_planes_evaluates_its_points_where_it_did() {
    // A project saved now, made a file of 17 so the one step runs: its family without planes, its points naming datums
    // (read whatever the file's format).
    let mut s = crate::engine::Session::new();
    s.execute("family", &json!({"name": "Old", "types": {"A": {}}, "adaptive": {"nodes": []}})).unwrap();
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(17);
    let families = file["families"].as_object_mut().expect("families by name");
    let family = families.values_mut().find(|f| f.get("adaptive").is_some()).expect("the adaptive family");
    family.as_object_mut().unwrap().remove("planes");
    family["adaptive"]["nodes"] = Value::Array(old_nodes());
    let (model, left_out) = crate::model::Model::read(&file.to_string()).expect("a file of 17 opens");
    assert!(left_out.is_empty(), "{left_out:?}");
    model.check().expect("and is sound");
    let f = &model.families["Old"];
    let d = f.adaptive.as_ref().unwrap();
    // Where the old evaluator put them: a point on the world at its frame, the hosted one on its host's XZ plane.
    let before: Definition = serde_json::from_value(json!({"nodes": old_nodes()})).unwrap();
    let (old, new) = (before.evaluate(&BTreeMap::new(), &before.preview), d.evaluate(&BTreeMap::new(), &d.preview));
    let expected: [(&str, P3); 5] = [
        ("on_level", [100., 200., 0.]),
        ("on_front", [500., 0., 700.]),
        ("on_side", [0., 600., 700.]),
        ("free", [10., 20., 30.]),
        ("hosted", [540., -50., 700.]),
    ];
    for (id, at) in expected {
        let (Some(Evaluated::Point { frame: a }), Some(Evaluated::Point { frame: b })) = (old.values.get(id), new.values.get(id)) else {
            panic!("{id} evaluates")
        };
        assert_eq!(a, b, "{id}: the same frame before and after");
        assert!(near(b.origin, at), "{id} at {:?}", b.origin);
    }
    // Each names its plane, and a point drawn on that plane now lands exactly where the old one is.
    let planes = f.plane_set();
    for (id, (_, guid, _)) in ["on_level", "on_front", "on_side"].into_iter().zip(old_datums()) {
        let on = WorkPlane::Plane(PlaneRef::Plane(guid));
        assert_eq!(d.work_plane(id), Some(on.clone()));
        let mut redrawn = d.clone();
        let at = new.origin(id).unwrap();
        let fresh = redrawn.add_point_on(&on, at, &new, planes).expect("drawn");
        assert_eq!(redrawn.nodes.iter().find(|n| n.id == fresh).unwrap().operation, d.nodes.iter().find(|n| n.id == id).unwrap().operation);
    }
    assert_eq!(d.work_plane("free"), None);
    // Saved and opened again, nothing moves.
    let (again, _) = crate::model::Model::read(&model.to_json().unwrap()).unwrap();
    assert_eq!(again.families["Old"].adaptive, f.adaptive);
}

/// A family with a named plane of its own 500 mm in front of Center (Front/Back), an unnamed one, and a reference line on
/// Ref. Level.
fn with_planes() -> Family {
    serde_json::from_value(json!({
        "types": {"A": {}},
        "planes": [
            {"guid": "1b2c3d4e-5f60-4a71-8b92-a3b4c5d6e7f8", "name": "Front", "start": [-1000, -500, 0], "end": [1000, -500, 0], "up": [0, 0, 1]},
            {"guid": "3d4e5f60-7182-4c93-8da4-c5d6e7f8091a", "start": [700, -1000, 0], "end": [700, 1000, 0], "up": [0, 0, 1]}
        ],
        "lines": [{"guid": "2c3d4e5f-6071-4b82-9ca3-b4c5d6e7f809", "start": [0, 0, 0], "end": [1000, 1000, 0]}],
        "adaptive": {"nodes": []}
    }))
    .expect("a family")
}

#[test]
fn placement_plane_lists_the_datums_then_the_named_planes_then_each_lines_faces() {
    let f = with_planes();
    let listed: Vec<String> = f.plane_set().listed().into_iter().map(|(_, label)| label).collect();
    assert_eq!(
        listed,
        [
            "Level : Ref. Level",
            "Reference Plane : Center (Front/Back)",
            "Reference Plane : Center (Left/Right)",
            "Reference Plane : Front",
            "Reference Line 1 : Horizontal",
            "Reference Line 1 : Vertical",
            "Reference Line 1 : Start",
            "Reference Line 1 : End",
        ]
    );
    // A point drawn on the named plane lies on it, with its axes, and keeps it as its work plane.
    let front = f.plane_set().listed()[3].0;
    let mut d = f.adaptive.clone().unwrap();
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    let id = d.add_point_on(&WorkPlane::Plane(front), [300., 0., 400.], &r, f.plane_set()).expect("drawn");
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    let Some(Evaluated::Point { frame }) = r.values.get(&id) else { panic!("it evaluates") };
    assert!(near(frame.origin, [300., -500., 400.]), "{frame:?}");
    assert!(near(frame.z, [0., -1., 0.]));
    assert_eq!(d.work_plane(&id), Some(WorkPlane::Plane(front)));
    // On the line's vertical face: through the line, standing up.
    let vertical = WorkPlane::Plane(PlaneRef::Line(f.lines[0].guid, Face::Vertical));
    let up = d.add_point_on(&vertical, [500., 500., 300.], &r, f.plane_set()).expect("drawn");
    assert!(near(d.evaluate(&BTreeMap::new(), &d.preview).origin(&up).unwrap(), [500., 500., 300.]));
}

#[test]
fn pick_finds_a_named_plane_of_the_familys_own() {
    let f = with_planes();
    let d = f.adaptive.clone().unwrap();
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    // From the front, looking along +y: the plane at y = -500 is nearer the eye than Center (Front/Back).
    let cam =
        crate::view3d::Camera { azimuth: -90., elevation: 10., lens: None, target: [0., 0., 0.], scale: 0.2, width: 800, height: 600 };
    let room = ([-1500., -1500., -300.], [1500., 1500., 1500.]);
    let (x, y, _) = cam.project([200., -500., 600.]);
    let picked = r.pick_plane(f.plane_set(), &|o, n| cam.on((x, y), o, n), &|p| cam.project(p).2, 70., room);
    assert_eq!(picked, Some(WorkPlane::Plane(f.plane_set().listed()[3].0)));
}

#[test]
fn a_plane_a_point_is_drawn_on_cannot_be_deleted_and_follows_a_new_guid() {
    let mut f = with_planes();
    let front = f.planes[3].guid;
    let mut d = f.adaptive.take().unwrap();
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    let id = d.add_point_on(&WorkPlane::Plane(PlaneRef::Plane(front)), [0., 0., 0.], &r, f.plane_set()).unwrap();
    f.adaptive = Some(d);
    assert_eq!(f.hosted_on(front), [format!("adaptive point `{id}`")]);
    let fresh = crate::ids::Guid::from_u128(99);
    f.rehost(&[(front, fresh)], &[]);
    assert_eq!(f.adaptive.as_ref().unwrap().work_plane(&id), Some(WorkPlane::Plane(PlaneRef::Plane(fresh))));
    // A work plane that isn't one of the family's is refused.
    assert!(f.check(&|_| true).is_err_and(|e| e.contains("work plane")), "{:?}", f.check(&|_| true));
}

#[test]
fn renaming_a_parameter_renames_it_where_the_adaptive_definition_names_it() {
    let mut f: Family = serde_json::from_value(json!({
        "parameters": [{"name": "Width", "value": 1000}, {"name": "Widths", "value": 2}, {"name": "Show", "kind": "yesno", "value": true},
                       {"name": "Finish", "kind": "material", "value": ""}],
        "types": {"A": {"Width": 1200}},
        "adaptive": {"nodes": [
            {"id": "a", "operation": {"kind": "point", "host": {"type": "world"}, "transform": {"offset": ["Width", "Width / 2 + Widths", 0], "rotation": [0, 0, "Width"]}},
             "appearance": {"visible": "Show"}},
            {"id": "b", "operation": {"kind": "point", "host": {"type": "world", "frame": {"origin": [0, 1000, 0]}}}},
            {"id": "ab", "operation": {"kind": "line", "a": "a", "b": "b"}},
            {"id": "on", "operation": {"kind": "point", "host": {"type": "curve", "curve": "ab", "location": {"measurement": "segment_length", "value": "Width / 4"}}}},
            {"id": "c", "operation": {"kind": "point", "host": {"type": "world", "frame": {"origin": [1000, 1000, 0]}}}},
            {"id": "profile", "operation": {"kind": "polyline", "points": ["a", "b", "c"], "closed": true}},
            {"id": "form", "operation": {"kind": "extrude", "chain": "profile", "distance": "Width"}, "appearance": {"material": "Finish", "visible": "Show"}}
        ]}
    }))
    .expect("a family");
    f.rename_parameter("Width", "Span");
    f.rename_parameter("Show", "Shown");
    f.rename_parameter("Finish", "Skin");
    let text = serde_json::to_string(f.adaptive.as_ref().unwrap()).unwrap();
    assert!(!text.contains("\"Width") && !text.contains("Show\"") && !text.contains("Finish"), "{text}");
    let d = f.adaptive.as_ref().unwrap();
    let node = |id: &str| d.nodes.iter().find(|n| n.id == id).unwrap();
    let Operation::Point { transform, .. } = &node("a").operation else { panic!() };
    assert_eq!(transform.offset[0], Expr::Formula("Span".into()));
    assert_eq!(transform.offset[1], Expr::Formula("Span / 2 + Widths".into()), "a name that only starts the same is kept");
    assert_eq!(transform.rotation[2], Expr::Formula("Span".into()));
    assert_eq!(node("a").appearance.visible, Some(Expr::Formula("Shown".into())));
    let Operation::Point { host: Host::Curve { location, .. }, .. } = &node("on").operation else { panic!() };
    assert_eq!(location.value, Expr::Formula("Span / 4".into()));
    let Operation::Extrude { distance, .. } = &node("form").operation else { panic!() };
    assert_eq!(*distance, Expr::Formula("Span".into()));
    assert_eq!(node("form").appearance.material, "Skin", "a form tied to a material parameter follows it");
    // It still works out, with the type's value under the new name.
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let r = d.evaluate(&values, &d.preview);
    assert!(!r.failed(), "{:?}", r.diagnostics);
    assert!(near(r.origin("a").unwrap(), [1200., 602., 0.]));
}
