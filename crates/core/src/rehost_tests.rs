use crate::engine::Session;
use crate::model::Element;
use serde_json::{Value, json};

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

#[test]
fn pick_new_host_keeps_opening_identity_and_instance_values() {
    for category in ["door", "window"] {
        let mut s = Session::new();
        let a = run(&mut s, "wall", json!({"points": [[0,0],[6000,0]]}))["ids"][0].as_u64().unwrap();
        let b = run(&mut s, "wall", json!({"points": [[6000,3000],[0,3000]]}))["ids"][0].as_u64().unwrap();
        let id = run(&mut s, category, json!({"wall": a, "offset": 2000, "mark": "KEEP", "flip_hand": true, "flip_facing": true}))["id"]
            .as_u64()
            .unwrap();
        let guid = s.model.guid(id);
        let before = s.model.elements.get(id).unwrap().clone();
        let target = s.model.guid(b);
        run(&mut s, "pick_new_host", json!({"id": guid, "wall": target, "offset": 2500}));
        assert_eq!(s.model.guid(id), guid);
        let mut expected = before.clone();
        let (Element::Door(o) | Element::Window(o)) = &mut expected else { panic!("expected the original opening category") };
        o.host = s.model.guid(b);
        o.offset = 2500.0;
        assert_eq!(s.model.elements.get(id), Some(&expected));
        run(&mut s, "undo", json!({}));
        assert_eq!(s.model.elements.get(id), Some(&before));
        run(&mut s, "redo", json!({}));
        assert_eq!(s.model.elements.get(id), Some(&expected));
    }
}

fn fixture() -> (Session, u64, u64, u64) {
    let mut s = Session::new();
    let a = run(&mut s, "wall", json!({"points": [[0,0],[6000,0]], "height": 3000}))["ids"][0].as_u64().unwrap();
    let b = run(&mut s, "wall", json!({"points": [[0,3000],[6000,3000]], "height": 3000}))["ids"][0].as_u64().unwrap();
    let id = run(&mut s, "window", json!({"wall": a, "offset": 2000, "sill": 800, "mark": "R-1"}))["id"].as_u64().unwrap();
    (s, a, b, id)
}

fn refused(s: &mut Session, params: Value, message: &str) {
    let before = std::sync::Arc::clone(&s.model);
    let history: Vec<_> = s.history().into_iter().map(str::to_owned).collect();
    let e = s.execute("pick_new_host", &params).unwrap_err();
    assert!(e.contains(message), "{params}: {e}; wanted {message}");
    assert!(std::sync::Arc::ptr_eq(&before, &s.model), "refusal changes no model");
    assert_eq!(s.history(), history);
}

#[test]
fn pick_new_host_rejects_invalid_targets_atomically() {
    let (mut s, a, b, id) = fixture();
    for (params, message) in [
        (json!({"id": id, "wall": a, "offset": 3000}), "another wall"),
        (json!({"id": id, "wall": id, "offset": 3000}), "wall"),
        (json!({"id": a, "wall": b, "offset": 3000}), "door or window"),
        (json!({"id": 999, "wall": b, "offset": 3000}), "no element"),
        (json!({"id": id, "wall": b, "offset": 0}), "fit"),
        (json!({"id": id, "wall": b, "offset": 1e100}), "offset"),
        (json!({"id": id, "wall": b, "offset": null}), "offset"),
        (json!({"id": id, "wall": b, "offset": "NaN"}), "offset"),
        (json!({"id": id, "wall": b, "offset": 2000, "typo": true}), "typo"),
    ] {
        refused(&mut s, params, message);
    }
    run(&mut s, "window", json!({"wall": b, "offset": 2000}));
    refused(&mut s, json!({"id": id, "wall": b, "offset": 2000}), "overlap");
    run(&mut s, "pin", json!({"ids": [id]}));
    refused(&mut s, json!({"id": id, "wall": b, "offset": 4000}), "pinned");
}

#[test]
fn pick_new_host_limits_levels_bases_curves_and_groups() {
    for patch in [json!({"base_offset": 100}), json!({"level": "Level 2"}), json!({"arc": 30})] {
        let (mut s, _, b, id) = fixture();
        let mut params = patch;
        params["id"] = json!(b);
        run(&mut s, "modify", params);
        refused(&mut s, json!({"id": id, "wall": b, "offset": 3000}), "Pick New Host");
    }
    for source in [false, true] {
        let (mut s, a, b, id) = fixture();
        let ids = if source { vec![a, id] } else { vec![b] };
        let group = run(&mut s, "create_group", json!({"ids": ids}))["id"].clone();
        refused(&mut s, json!({"id": id, "wall": b, "offset": 3000}), "group");
        run(&mut s, "edit_group", json!({"id": group}));
        refused(&mut s, json!({"id": id, "wall": b, "offset": 3000}), "group");
    }
}

#[test]
fn pick_new_host_keeps_raised_sill_and_existing_top_notch_policy() {
    let (mut s, a, b, id) = fixture();
    run(&mut s, "modify", json!({"ids": [a,b], "base_offset": 400}));
    // Existing policy permits the head above the wall, provided the sill is within it.
    run(&mut s, "modify", json!({"id": b, "height": 1000}));
    run(&mut s, "pick_new_host", json!({"id": id, "wall": b, "offset": 3000}));
    let Element::Window(o) = s.model.elements.get(id).unwrap() else { panic!("expected the original opening category") };
    assert_eq!(o.sill, 800.0);
    assert_eq!(s.model.opening_frame(o).unwrap().2, 1200.0);
    run(&mut s, "undo", json!({}));
    run(&mut s, "modify", json!({"id": b, "height": 700}));
    refused(&mut s, json!({"id": id, "wall": b, "offset": 3000}), "sill");
}

#[test]
fn pick_new_host_retains_tags_reopens_and_survives_source_wall_deletion() {
    let (mut s, a, b, id) = fixture();
    run(&mut s, "parameter", json!({"name": "Owner Note", "categories": ["window"]}));
    run(&mut s, "modify", json!({"id": id, "params": {"Owner Note": "retain me"}}));
    run(&mut s, "tag", json!({}));
    let dimension = run(&mut s, "dimension", json!({"refs": [{"id": id, "face": "start"}, {"id": id, "face": "end"}], "at": [2000,-700]}))
        ["id"]
        .as_u64()
        .unwrap();
    let dimension_before = s.model.elements.get(dimension).unwrap().clone();
    let tags: Vec<_> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Tag(_))).map(|(id, e)| (*id, e.clone())).collect();
    assert!(!tags.is_empty());
    let guid = s.model.guid(id);
    run(&mut s, "pick_new_host", json!({"id": id, "wall": b, "offset": 3000}));
    for (id, tag) in &tags {
        assert_eq!(s.model.elements.get(*id), Some(tag));
    }
    assert_eq!(s.model.elements.get(dimension), Some(&dimension_before));
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rehost.bimc");
    run(&mut s, "save", json!({"path": path}));
    let mut loaded = Session::new();
    run(&mut loaded, "open", json!({"path": path}));
    assert_eq!(loaded.model.guid(id), guid);
    assert_eq!(loaded.model.elements.get(id), s.model.elements.get(id));
    run(&mut loaded, "delete", json!({"ids": [a]}));
    assert!(loaded.model.elements.get(guid).is_some());
    for (id, tag) in tags {
        assert_eq!(loaded.model.elements.get(id), Some(&tag));
    }
}

#[test]
fn pick_new_host_updates_old_and_new_wall_geometry_and_read_only_refuses() {
    use crate::view3d::Scene;
    let (mut s, _, b, id) = fixture();
    let before = std::sync::Arc::clone(&s.model);
    let scene = Scene::new(&before);
    let params = json!({"id": id, "wall": b, "offset": 3000});
    assert!(s.reading("test", |s| s.execute("pick_new_host", &params)).is_err());
    assert!(std::sync::Arc::ptr_eq(&before, &s.model));
    run(&mut s, "pick_new_host", params);
    let fresh = Scene::new(&s.model);
    let updated = scene.update(&before, &s.model);
    let camera = fresh.fit(-60.0, 30.0, 400, 300);
    assert_eq!(updated.render(&camera, &[]).png(), fresh.render(&camera, &[]).png());
    assert_ne!(scene.render(&camera, &[]).png(), fresh.render(&camera, &[]).png());
}

#[test]
fn pick_new_host_keeps_face_relative_orientation_on_rotated_hosts() {
    for end in [[0, 9000], [6000, 9000], [-6000, 3000]] {
        for hand in [false, true] {
            for facing in [false, true] {
                let (mut s, _, b, id) = fixture();
                run(&mut s, "modify", json!({"id": b, "end": end}));
                run(&mut s, "modify", json!({"id": id, "flip_hand": hand, "flip_facing": facing}));
                run(&mut s, "pick_new_host", json!({"id": id, "wall": b, "offset": 2000}));
                let Element::Window(o) = s.model.elements.get(id).unwrap() else { panic!("expected the original opening category") };
                assert_eq!((o.flip_hand, o.flip_facing), (hand, facing));
            }
        }
    }
}

#[test]
fn pick_new_host_retains_family_values_and_rebuilds_for_wall_thickness() {
    let (mut s, a, b, old) = fixture();
    run(&mut s, "delete", json!({"ids": [old]}));
    run(
        &mut s,
        "family",
        json!({"name": "Test Trim", "category": "window", "types": {"A": {}},
        "parameters": [{"name": "Width", "value": 1200, "instance": true}, {"name": "Height", "value": 1400},
                       {"name": "Sill Height", "value": 700}],
        "forms": [{"profile": [["-Width/2", "-Wall Thickness/2"], ["Width/2", "-Wall Thickness/2"],
                                  ["Width/2", "Wall Thickness/2"], ["-Width/2", "Wall Thickness/2"]], "from": 0, "to": 50}]}),
    );
    run(&mut s, "type", json!({"category": "wall", "name": "Thick Target", "thickness": 500}));
    run(&mut s, "modify", json!({"id": b, "type": "Thick Target"}));
    let id =
        run(&mut s, "window", json!({"wall": a, "offset": 2000, "family": "Test Trim", "type": "A", "mark": "F1"}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": id, "params": {"Width": 1600}}));
    let before = s.model.elements.get(id).unwrap().clone();
    run(&mut s, "pick_new_host", json!({"id": id, "wall": b, "offset": 3000}));
    let e = s.model.elements.get(id).unwrap();
    let (Element::Window(o), Element::Window(was)) = (e, &before) else { panic!("expected the original opening category") };
    assert_eq!((&o.family, &o.type_name, &o.params, &o.mark, o.sill), (&was.family, &was.type_name, &was.params, &was.mark, was.sill));
    let pieces = s.model.opening_local(e).unwrap().0;
    let ys: Vec<_> = pieces.iter().flat_map(|p| p.tris.iter().flatten().map(|p| p[1])).collect();
    assert_eq!(ys.iter().copied().fold(f64::NEG_INFINITY, f64::max), 250.0);
    assert_eq!(ys.iter().copied().fold(f64::INFINITY, f64::min), -250.0);
    assert_eq!(s.model.opening_dims(e).unwrap().width, 1600.0);
}

#[test]
fn pick_new_host_refuses_curtain_hosts_and_preserves_redo_after_refusal() {
    let (mut s, _, b, id) = fixture();
    run(&mut s, "pick_new_host", json!({"id": id, "wall": b, "offset": 3000}));
    run(&mut s, "undo", json!({}));
    refused(&mut s, json!({"id": id, "wall": b, "offset": 0}), "fit");
    run(&mut s, "redo", json!({}));
    assert!(matches!(s.model.elements.get(id), Some(Element::Window(o)) if s.model.same(o.host, b)));
    run(&mut s, "undo", json!({}));
    let curtain = s.model.types.wall.iter().find(|(_, t)| t.curtain.is_some()).unwrap().0.clone();
    run(&mut s, "modify", json!({"id": b, "type": curtain, "location": "exterior-finish"}));
    refused(&mut s, json!({"id": id, "wall": b, "offset": 3000}), "curtain");
}

pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let a = run(base, "wall", json!({"points": [[20000,-20000],[26000,-20000]]}))["ids"][0].as_u64().unwrap();
    let b = run(base, "wall", json!({"points": [[20000,-24000],[26000,-24000]]}))["ids"][0].as_u64().unwrap();
    let id = run(base, "window", json!({"wall": a, "offset": 2000}))["id"].as_u64().unwrap();
    [json!(0), json!(-1), json!(1e300), json!(null), json!("NaN"), json!([]), json!({}), json!(2000)]
        .into_iter()
        .map(|offset| ("pick_new_host", json!({"id": id, "wall": b, "offset": offset})))
        .collect()
}
