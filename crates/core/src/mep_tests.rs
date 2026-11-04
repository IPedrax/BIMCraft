//! The MEP tabs' elements (plumbing, electrical, lightning protection, data) through the commands every front end drives. The
//! run categories share one table, so a tab adds a row; what only one tab has (its fixtures, its devices) has tests of its own.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::geom::P2;
use crate::model::Element;
use crate::render::{self, Drawing, POCHE, Prim, View};

#[test]
fn stage5_line_insertion_and_deletion_recalculate_marks_without_replacing_devices() {
    let mut s = Session::new();
    let panel = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[0,0]}));
    let first = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[1000,0]}));
    let last = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[3000,0]}));
    let last_guid = s.model.guid(last);
    let line = made(&mut s, "electrical_line", json!({"source":panel,"devices":[first,last]}));
    let marks = |s: &mut Session| run(s, "electrical_line_inspect", json!({"id":line}))["marks"].clone();
    assert_eq!(marks(&mut s), json!(["1", "2", "3"]));

    let middle = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[2000,0]}));
    run(&mut s, "electrical_line_edit", json!({"id":line,"devices":[first,middle,last]}));
    assert_eq!(marks(&mut s), json!(["1", "2", "3", "4"]));
    assert_eq!(s.model.guid(last), last_guid);
    run(&mut s, "delete", json!({"ids":[middle]}));
    assert_eq!(marks(&mut s), json!(["1", "2", "3"]));
    assert_eq!(s.model.guid(last), last_guid);
    run(&mut s, "undo", json!({}));
    assert_eq!(marks(&mut s), json!(["1", "2", "3", "4"]));
    run(&mut s, "redo", json!({}));
    assert_eq!(marks(&mut s), json!(["1", "2", "3"]));
    run(&mut s, "electrical_line_mark", json!({"id":line,"device":last,"mark":"3"}));
    let file = s.model.to_json().unwrap();
    let (opened, omitted) = crate::model::Model::read(&file).unwrap();
    assert!(omitted.is_empty());
    s.reset(opened, None);
    assert_eq!(s.model.guid(last), last_guid);
    let middle = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[2000,0]}));
    run(&mut s, "electrical_line_edit", json!({"id":line,"devices":[first,middle,last]}));
    assert_eq!(marks(&mut s), json!(["1", "2", "4", "3"]));
    assert_eq!(s.model.guid(last), last_guid);
}

#[test]
fn stage5_locked_marks_survive_line_edits_and_file_roundtrip() {
    let mut s = Session::new();
    let panel = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[0,0]}));
    let first = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[1000,0]}));
    let last = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[3000,0]}));
    run(&mut s, "electrical_mark_template", json!({"system":"Fire Alarm","prefix":"FA-"}));
    let line = made(&mut s, "electrical_line", json!({"source":panel,"devices":[last],"system":"Fire Alarm"}));
    run(&mut s, "electrical_line_mark", json!({"id":line,"device":last,"mark":"FA-2"}));
    run(&mut s, "electrical_line_edit", json!({"id":line,"devices":[first,last]}));
    let saved = s.model.to_json().unwrap();
    let (opened, omitted) = crate::model::Model::read(&saved).unwrap();
    assert!(omitted.is_empty());
    assert_eq!(opened.guid(line), s.model.guid(line));
    s.reset(opened, None);
    let marks = |s: &mut Session| run(s, "electrical_line_inspect", json!({"id":line}))["marks"].clone();
    assert_eq!(marks(&mut s), json!(["FA-1", "FA-3", "FA-2"]));
    let renumbered = run(&mut s, "electrical_line_renumber", json!({"id":line,"mark_prefix":"AL-"}));
    assert_eq!(renumbered["marks"], json!(["AL-1", "AL-2", "FA-2"]));
    run(&mut s, "electrical_line_mark", json!({"id":line,"device":last,"mark":null}));
    assert_eq!(marks(&mut s), json!(["AL-1", "AL-2", "AL-3"]));
}

#[test]
fn stage5_logical_terminal_can_be_renamed_without_moving_its_wire() {
    let mut s = Session::new();
    let panel = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[0,0]}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[3000,500]}));
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]]}));
    let terminal = made(
        &mut s,
        "electrical_terminal",
        json!({"device":device,"name":"A","purpose":"Input","address":"007","load_ma":25,"limit_ma":20}),
    );
    let feeders = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"end":1,"point":[0,0,0],"terminal":terminal})));
    assert!(!feeders.is_empty());
    let line = made(&mut s, "electrical_line", json!({"source":panel,"devices":[device],"segments":[wire],"terminals":[terminal]}));
    let before = s.model.elements.get(wire).cloned();
    let guid = s.model.guid(terminal);
    run(&mut s, "electrical_terminal", json!({"id":terminal,"name":"B","purpose":"Output"}));
    assert_eq!(s.model.guid(terminal), guid);
    let inspected = run(&mut s, "electrical_line_inspect", json!({"id":line}));
    assert_eq!(inspected["terminal_checks"][0]["address"], json!("007"));
    assert_eq!(inspected["terminal_checks"][0]["overload"], json!(true));
    assert_eq!(element_json(&s.model, terminal)["address"], json!("007"));
    assert_eq!(s.model.elements.get(wire), before.as_ref());
    assert_eq!(run(&mut s, "electrical_line_inspect", json!({"id":line}))["terminals"], json!([guid]));
    let (opened, omitted) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(omitted.is_empty());
    assert_eq!(opened.elements.get(wire).unwrap().run().unwrap().connections[0].terminal, Some(guid));
    s.reset(opened, None);
    run(&mut s, "delete", json!({"ids":[terminal]}));
    assert_eq!(run(&mut s, "electrical_line_inspect", json!({"id":line}))["terminals"], json!([]));
    let mut expected = before.unwrap();
    expected.run_mut().unwrap().connections[0].terminal = None;
    assert_eq!(s.model.elements.get(wire), Some(&expected));
    assert!(feeders.iter().all(|id| s.model.elements.get(*id).is_some()));
}

#[test]
fn stage5_cable_journal_totals_physical_segments_with_reserve_once() {
    let mut s = Session::new();
    let panel = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[0,0]}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[3000,0]}));
    let a = made(&mut s, "wire", json!({"points":[[0,0],[1000,0]],"reserve":10}));
    let b = made(&mut s, "wire", json!({"points":[[1000,0],[3000,0]]}));
    let line = made(&mut s, "electrical_line", json!({"source":panel,"devices":[device],"segments":[a,b]}));
    let detailed = run(&mut s, "cable_journal", json!({"id":line,"mode":"segments"}));
    assert_eq!(detailed["rows"][0]["length"], json!(1100.0));
    assert_eq!(detailed["rows"][1]["length"], json!(2000.0));
    let total = run(&mut s, "cable_journal", json!({"id":line,"mode":"line"}));
    assert_eq!(total["rows"][0]["length"], json!(3100.0));
    run(&mut s, "split", json!({"id":b,"at":[2000,0]}));
    let detailed = run(&mut s, "cable_journal", json!({"id":line,"mode":"segments"}));
    assert_eq!(detailed["rows"].as_array().unwrap().len(), 3);
    assert_eq!(run(&mut s, "cable_journal", json!({"id":line,"mode":"line"}))["rows"][0]["length"], json!(3100.0));
    let feeds = ids(&run(&mut s, "wire_connect", json!({"wire":a,"device":panel,"point":[0,500,0]})));
    assert!(!feeds.is_empty());
    let feed_length: f64 = feeds.into_iter().map(|id| s.model.run_geom(s.model.elements.get(id).unwrap()).unwrap().length()).sum();
    let expected = ((3100.0 + feed_length * 1.1) * 100.0).round() / 100.0;
    assert_eq!(run(&mut s, "cable_journal", json!({"id":line,"mode":"line"}))["rows"][0]["length"], json!(expected));
}

#[test]
fn stage5_numbering_visits_short_branch_before_long_branch_and_continuation() {
    let mut s = Session::new();
    let mut device = |at| made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":at}));
    let panel = device([0, 0]);
    let junction = device([1000, 0]);
    let continuation = device([2000, 0]);
    let short = device([1000, -500]);
    let equal = device([1000, 500]);
    let long = device([1000, 1500]);
    let line = made(
        &mut s,
        "electrical_line",
        json!({
            "source":panel,"devices":[junction,continuation],
        "branches":[{"from":junction,"devices":[long]},{"from":junction,"devices":[equal]},{"from":junction,"devices":[short]}]
        }),
    );
    let inspected = run(&mut s, "electrical_line_inspect", json!({"id":line}));
    assert_eq!(inspected["ordered_devices"], json!([panel, junction, short, equal, long, continuation]));
    assert_eq!(inspected["marks"], json!(["1", "2", "3", "4", "5", "6"]));
    let saved = s.model.to_json().unwrap();
    assert!(s.execute("electrical_line_edit", &json!({"id":line,"branches":[{"from":short,"devices":[short]}]})).is_err());
    assert_eq!(s.model.to_json().unwrap(), saved);

    let line_guid = s.model.guid(line);
    run(&mut s, "delete", json!({"ids":[junction]}));
    let inspected = run(&mut s, "electrical_line_inspect", json!({"id":line}));
    assert_eq!(inspected["ordered_devices"], json!([panel, short, equal, long, continuation]));
    assert_eq!(s.model.guid(line), line_guid);
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), saved);
}

#[test]
fn stage4_render_is_bound_to_requested_revision() {
    let mut s = Session::new();
    let revision = run(&mut s, "inspect", json!({}))["revision"].clone();
    assert!(revision.is_string());
    let capture = json!({"view":"{3D}","width":64,"height":64,"revision":revision});
    let result = run(&mut s, "render", capture.clone());
    assert_eq!(result["revision"], revision);
    assert!(result["pngBase64"].as_str().is_some_and(|png| png.starts_with("iVBORw0KGgo")));
    made(&mut s, "wall", json!({"points":[[0,0],[4000,0]]}));
    assert!(s.execute("render", &capture).is_err());
}

#[test]
fn stage4_free_device_exit_slides_on_allowed_side_and_touches_wall() {
    let mut s = Session::new();
    let wall = made(&mut s, "wall", json!({"points":[[0,0],[6000,0]],"height":6000}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[1000,-100]}));
    run(&mut s, "modify", json!({"id":device,"connection_sides":["top"]}));
    let carrier =
        made(&mut s, "cable_carrier", json!({"points":[[1040,-300],[1040,-300]],"offset":3000,"end_offset":5000,"type":"Tray 100 x 50"}));
    run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
    let wire = made(&mut s, "wire", json!({"points":[[1040,-102],[1040,-102]],"offset":3000,"end_offset":5000}));
    let result =
        run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"carrier":carrier,"surface":{"host":wall,"face":"interior"}}));
    let feeder = ids(&result);
    assert_eq!(feeder.len(), 1, "free exit must not create a normal micro-leg towards the carrier centre");
    let guid = s.model.guid(feeder[0]);
    let geometry = s.model.run_geom(s.model.elements.get(feeder[0]).unwrap()).unwrap();
    assert_eq!(geometry.a, [1040.0, -102.0, 1900.0]);
    assert_eq!(geometry.b, [1040.0, -102.0, 3000.0]);
    let before = s.model.to_json().unwrap();
    run(&mut s, "move", json!({"ids":[carrier],"by":[60,0]}));
    let geometry = s.model.run_geom(s.model.elements.get(feeder[0]).unwrap()).unwrap();
    assert_eq!(geometry.a, [1100.0, -102.0, 1900.0]);
    assert_eq!(geometry.b, [1100.0, -102.0, 3000.0]);
    assert_eq!(s.model.guid(feeder[0]), guid);
    let after = s.model.to_json().unwrap();
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "redo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), after);
    let (opened, left_out) = crate::model::Model::read(&after).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(opened.to_json().unwrap(), after);
}

fn free_exit_fixture() -> (Session, u64, u64, u64, u64) {
    let mut s = Session::new();
    let wall = made(&mut s, "wall", json!({"points":[[0,0],[6000,0]],"height":6000}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[1000,-100]}));
    let carrier =
        made(&mut s, "cable_carrier", json!({"points":[[1040,-300],[1040,-300]],"offset":3000,"end_offset":5000,"type":"Tray 100 x 50"}));
    run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
    let wire = made(&mut s, "wire", json!({"points":[[1040,-102],[1040,-102]],"offset":3000,"end_offset":5000}));
    (s, wall, device, carrier, wire)
}

#[test]
fn stage4_free_device_four_ends_keep_independent_routes_and_atomic_history() {
    let mut s = Session::new();
    let wall = made(&mut s, "wall", json!({"points":[[0,0],[6000,0]],"height":6000}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[1000,-100]}));
    run(&mut s, "modify", json!({"id":device,"connection_sides":["top","right"]}));
    let wire = made(&mut s, "wire", json!({"points":[[1040,-102],[1040,-102]],"offset":3000,"end_offset":5000}));
    let other = made(&mut s, "wire", json!({"points":[[960,-102],[960,-102]],"offset":3100,"end_offset":5000}));
    let initial_feeders = [wire, other].map(|parent| {
        [0, 1].map(|end| {
            let result =
                run(&mut s, "wire_connect", json!({"wire":parent,"device":device,"end":end,"surface":{"host":wall,"face":"interior"}}));
            let legs = ids(&result);
            assert_eq!(legs.len(), 1);
            s.model.guid(legs[0])
        })
    });
    let positions = [1040.0, 960.0, 1100.0, 900.0];
    let carriers = positions.map(|x| {
        let carrier =
            made(&mut s, "cable_carrier", json!({"points":[[x,-300],[x,-300]],"offset":3000,"end_offset":5000,"type":"Tray 100 x 50"}));
        run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
        carrier
    });
    let mut feeders = Vec::new();
    for (ring, parent) in [wire, other].into_iter().enumerate() {
        for (end, initial_guid) in initial_feeders[ring].into_iter().enumerate() {
            let route = ring * 2 + end;
            let carrier = carriers[route];
            let result = run(
                &mut s,
                "wire_connect",
                json!({"wire":parent,"device":device,"end":end,"carrier":carrier,
                "in_carrier":true,"surface":{"host":wall,"face":"interior"}}),
            );
            let legs = ids(&result);
            assert_eq!(legs.len(), 1);
            assert_eq!(s.model.guid(legs[0]), initial_guid);
            let branch = result["carriers"][0].as_u64().unwrap();
            let g = s.model.run_geom(s.model.elements.get(branch).unwrap()).unwrap();
            assert!((g.a[1] + 125.0).abs() < 1e-8, "branch back must touch wall");
            feeders.push((legs[0], s.model.guid(legs[0]), route));
        }
    }
    assert_eq!(feeders.iter().map(|v| v.0).collect::<std::collections::BTreeSet<_>>().len(), 4);
    let before = s.model.to_json().unwrap();
    run(&mut s, "move", json!({"ids":[carriers[1]],"by":[-40,0]}));
    for (id, guid, route) in feeders {
        let g = s.model.run_geom(s.model.elements.get(id).unwrap()).unwrap();
        assert_eq!(g.a, [positions[route] - if route == 1 { 40.0 } else { 0.0 }, -102.0, 1900.0]);
        assert_eq!(s.model.guid(id), guid);
    }
    let after = s.model.to_json().unwrap();
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "redo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), after);
    let (opened, left_out) = crate::model::Model::read(&after).unwrap();
    assert!(left_out.is_empty());
    assert!(opened == *s.model, "save/open must preserve both rings, their feeder GUIDs and their separate routes");
}

#[test]
fn stage4_free_device_host_and_route_deletion_preserve_straight_lead() {
    let (mut s, wall, device, carrier, wire) = free_exit_fixture();
    let feeder = ids(&run(
        &mut s,
        "wire_connect",
        json!({"wire":wire,"device":device,"carrier":carrier,
        "surface":{"host":wall,"face":"interior"}}),
    ))[0];
    run(&mut s, "move", json!({"ids":[wall],"by":[100,200]}));
    let g = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
    assert_eq!(g.a, [1140.0, 98.0, 1900.0]);
    assert_eq!(g.b, [1140.0, 98.0, 3000.0]);
    for deleted in [carrier, wall] {
        run(&mut s, "delete", json!({"ids":[deleted]}));
        let actual = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
        assert_eq!(actual.a, g.a);
        assert_eq!(actual.b, g.b);
        let saved = s.model.to_json().unwrap();
        let (opened, left_out) = crate::model::Model::read(&saved).unwrap();
        assert!(left_out.is_empty());
        assert_eq!(opened.to_json().unwrap(), saved);
    }
    run(&mut s, "move", json!({"ids":[device],"by":[20,0]}));
    let actual = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
    assert_eq!(actual.a, [1140.0, 98.0, 1900.0], "free side slides while last route landing stays fixed");
    let snapshot = s.model.to_json().unwrap();
    assert!(
        s.execute("move", &json!({"ids":[device],"by":[0,100]})).is_err(),
        "a detached lead cannot silently move its frozen route landing onto a different plane"
    );
    assert_eq!(s.model.to_json().unwrap(), snapshot);
}

#[test]
fn stage4_free_device_contact_is_independent_of_diameter_and_bad_surface_rolls_back() {
    let (mut s, wall, device, carrier, wire) = free_exit_fixture();
    let feeder = ids(&run(
        &mut s,
        "wire_connect",
        json!({"wire":wire,"device":device,"carrier":carrier,
        "surface":{"host":wall,"face":"interior"}}),
    ))[0];
    let contact = s.model.elements.get(wire).unwrap().run().unwrap().connections[0].point;
    let type_name = s.model.elements.get(wire).unwrap().run().unwrap().type_name.clone();
    run(&mut s, "type", json!({"category":"wire","name":type_name,"width":12}));
    let connection = &s.model.elements.get(wire).unwrap().run().unwrap().connections[0];
    assert_eq!(connection.point, contact);
    let g = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
    assert_eq!(g.a, [1040.0, -106.0, 1900.0]);
    for params in [
        json!({"wire":wire,"device":device,"carrier":carrier,"surface":{"host":wall,"face":"top"}}),
        json!({"wire":wire,"device":device,"point":[0,0,0],"surface":{"host":wall,"face":"interior"}}),
    ] {
        let snapshot = s.model.to_json().unwrap();
        assert!(s.execute("wire_connect", &params).is_err());
        assert_eq!(s.model.to_json().unwrap(), snapshot);
    }
    let snapshot = s.model.to_json().unwrap();
    assert!(s.execute("modify", &json!({"id":device,"connection_sides":["bottom"]})).is_err());
    assert_eq!(s.model.to_json().unwrap(), snapshot);
}

#[test]
fn stage4_free_device_copy_rotation_and_mirror_keep_contacts_on_transformed_wall() {
    for mirror in [false, true] {
        let (mut s, wall, device, carrier, wire) = free_exit_fixture();
        let feeder = ids(&run(
            &mut s,
            "wire_connect",
            json!({"wire":wire,"device":device,"carrier":carrier,
            "surface":{"host":wall,"face":"interior"}}),
        ))[0];
        let original = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
        if mirror {
            run(&mut s, "mirror", json!({"ids":[wall,device,carrier,wire],"axis":[[0,0],[0,1000]]}));
        } else {
            run(&mut s, "rotate", json!({"ids":[wall,device,carrier,wire],"about":[0,0],"angle":37}));
        }
        let g = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
        let (sin, cos) = 37.0_f64.to_radians().sin_cos();
        for (p, q) in [original.a, original.b].into_iter().zip([g.a, g.b]) {
            let expected = if mirror { [-p[0], p[1], p[2]] } else { [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos, p[2]] };
            assert!(expected.into_iter().zip(q).all(|(a, b)| (a - b).abs() < 0.002), "{expected:?} != {q:?}");
        }
        let copied = ids(&run(&mut s, "copy", json!({"ids":[wall,device,carrier,wire],"by":[8000,0]})));
        let main = copied.iter().find_map(|id| s.model.elements.get(*id).unwrap().run().filter(|r| !r.connections.is_empty())).unwrap();
        assert_ne!(main.connections[0].device, s.model.guid(device));
        assert_ne!(main.connections[0].surface.as_ref().unwrap().host, Some(s.model.guid(wall)));
        assert_ne!(main.connections[0].carrier, Some(s.model.guid(carrier)));
        let saved = s.model.to_json().unwrap();
        let (opened, left_out) = crate::model::Model::read(&saved).unwrap();
        assert!(left_out.is_empty());
        for id in [feeder, wire].into_iter().chain(copied.iter().copied()) {
            let before = s.model.elements.get(id).unwrap();
            let after = opened.elements.get(id).unwrap();
            assert_eq!(opened.guid(id), s.model.guid(id));
            if let (Some(a), Some(b)) = (s.model.run_geom(before), opened.run_geom(after)) {
                assert!(a.a.into_iter().chain(a.b).zip(b.a.into_iter().chain(b.b)).all(|(x, y)| (x - y).abs() < 0.002));
            }
            if let (Some(a), Some(b)) = (before.run(), after.run()) {
                assert_eq!(a.connections.len(), b.connections.len());
                for (a, b) in a.connections.iter().zip(&b.connections) {
                    assert_eq!((a.device, a.carrier, a.end), (b.device, b.carrier, b.end));
                    assert_eq!(a.surface.as_ref().map(|p| p.host), b.surface.as_ref().map(|p| p.host));
                    assert!(a.point.into_iter().zip(b.point).all(|(x, y)| (x - y).abs() < 0.002));
                }
            }
        }
    }
}

#[test]
fn stage4_free_device_all_local_sides_and_continuous_wall_support() {
    for (side, points, offset, end_offset, expected) in [
        ("top", [[1040, -300], [1040, -300]], 3000, 5000, [1040.0, -102.0, 1900.0]),
        ("bottom", [[1040, -300], [1040, -300]], 500, 1000, [1040.0, -102.0, 1500.0]),
        ("right", [[2000, -300], [4000, -300]], 1700, 1700, [1150.0, -102.0, 1700.0]),
        ("left", [[100, -300], [700, -300]], 1700, 1700, [850.0, -102.0, 1700.0]),
    ] {
        let (mut s, wall, device, _, wire) = free_exit_fixture();
        run(&mut s, "modify", json!({"id":device,"connection_sides":[side]}));
        let carrier =
            made(&mut s, "cable_carrier", json!({"points":points,"offset":offset,"end_offset":end_offset,"type":"Tray 100 x 50"}));
        run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
        let feeder = ids(&run(
            &mut s,
            "wire_connect",
            json!({"wire":wire,"device":device,"carrier":carrier,
            "surface":{"host":wall,"face":"interior"}}),
        ))[0];
        let g = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
        assert_eq!(g.a, expected, "{side}");
    }
    let (mut s, wall, device, carrier, wire) = free_exit_fixture();
    made(&mut s, "window", json!({"at":[1040,0]}));
    let before = s.model.to_json().unwrap();
    assert!(
        s.execute(
            "wire_connect",
            &json!({"wire":wire,"device":device,"carrier":carrier,
        "surface":{"host":wall,"face":"interior"}})
        )
        .is_err(),
        "a free cable cannot bridge an opening"
    );
    assert_eq!(s.model.to_json().unwrap(), before);
}

#[test]
fn stage4_unmounted_rectangular_sections_follow_rotation_and_mirror() {
    for vertical in [true, false] {
        for roll in [0.0, 31.0, 90.0] {
            for mirror in [false, true] {
                let mut s = Session::new();
                let id = made(
                    &mut s,
                    "cable_carrier",
                    json!({"points":[[1000,1000],if vertical {[1000,1000]} else {[3000,1700]}],
                        "offset":500,"end_offset":2500,"type":"Tray 100 x 50"}),
                );
                run(&mut s, "modify", json!({"id":id,"section_roll":roll}));
                let before = s.model.to_json().unwrap();
                let original = s.model.run_geom(s.model.elements.get(id).unwrap()).unwrap();
                let angle = 37.0_f64.to_radians();
                let (sin, cos) = angle.sin_cos();
                let map = |p: [f64; 3]| {
                    if mirror { [-p[0], p[1], p[2]] } else { [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos, p[2]] }
                };
                if mirror {
                    run(&mut s, "mirror", json!({"ids":[id],"axis":[[0,0],[0,1000]]}));
                } else {
                    run(&mut s, "rotate", json!({"ids":[id],"about":[0,0],"angle":37}));
                }
                let actual = s.model.run_geom(s.model.elements.get(id).unwrap()).unwrap();
                // Legacy sloped prisms have plumb-cut ends; compare their actual section
                // only where the physical box representation already applies.
                if vertical || roll != 0.0 {
                    let points: Vec<_> = actual.box_triangles().into_iter().flatten().collect();
                    for expected in original.box_triangles().into_iter().flatten().map(map) {
                        assert!(
                            points.iter().any(|p| p.iter().zip(expected).all(|(a, b)| (a - b).abs() < 0.002)),
                            "vertical={vertical}, roll={roll}, mirror={mirror}: missing {expected:?}, actual {actual:?}"
                        );
                    }
                } else {
                    assert!(!actual.square_ends && actual.roll == 0.0, "legacy plumb-cut ends must retain their representation");
                }
                let after = s.model.to_json().unwrap();
                let (opened, left_out) = crate::model::Model::read(&after).unwrap();
                assert!(left_out.is_empty());
                assert_eq!(opened.to_json().unwrap(), after);
                run(&mut s, "undo", json!({}));
                assert_eq!(s.model.to_json().unwrap(), before);
                run(&mut s, "redo", json!({}));
                assert_eq!(s.model.to_json().unwrap(), after);
            }
        }
    }
}

#[test]
fn stage4_steep_floor_mount_uses_actual_normal_and_follows_slope() {
    for (face, x) in [("top", 1000), ("bottom", 1000), ("top", 5000), ("bottom", 5000)] {
        let mut s = Session::new();
        let host = made(&mut s, "floor", json!({"boundary":[[0,0],[6000,0],[6000,4000],[0,4000]],"slope":70,"arrow":[[0,0],[6000,0]]}));
        let id = made(&mut s, "cable_carrier", json!({"points":[[x,1000],[x,3000]],"offset":3000,"type":"Tray 100 x 50"}));
        run(&mut s, "run_mount", json!({"id":id,"host":host,"face":face}));
        for slope in [70.0_f64, 35.0] {
            if slope != 70.0 {
                run(&mut s, "modify", json!({"id":host,"slope":slope}));
            }
            let element = s.model.elements.get(id).unwrap();
            let r = element.run().unwrap();
            let mount = r.mount.as_ref().unwrap();
            let (origin, _, normal) = s.model.run_mount_frame(mount).unwrap();
            let g = s.model.run_geom(element).unwrap();
            let depth = g.frame().2;
            assert!(crate::view3d::dot(depth, normal) > 1.0 - 1e-9);
            for point in [g.a, g.b] {
                assert!((crate::view3d::dot(crate::view3d::sub(point, origin), normal) - 25.0).abs() < 1e-6);
            }
            assert!((normal[2].abs() - slope.to_radians().cos()).abs() < 1e-9);
        }
    }
}

#[test]
fn stage4_slab_mount_contacts_both_faces_and_follows_host() {
    for category in ["floor", "ceiling"] {
        for face in ["top", "bottom"] {
            let mut s = Session::new();
            let host = made(&mut s, category, json!({"boundary":[[0,0],[6000,0],[6000,4000],[0,4000]],"offset":1200}));
            let hit = run(
                &mut s,
                "surface_query",
                json!({"host":host,"origin":[2000,1000,if face == "top" {5000} else {-5000}],"direction":[0,0,if face == "top" {-1} else {1}],"reach":10000}),
            );
            let z = hit["hit"]["point"][2].as_f64().unwrap();
            let sign = if face == "top" { 1.0 } else { -1.0 };
            for (command, params, half) in [
                ("wire", json!({"points":[[1000,1000],[3000,1000]],"offset":2000}), 2.0),
                ("cable_carrier", json!({"points":[[1000,2000],[3000,2000]],"offset":2000,"type":"Tray 100 x 50"}), 25.0),
            ] {
                let id = made(&mut s, command, params);
                run(&mut s, "run_mount", json!({"id":id,"host":host,"face":face}));
                let g = s.model.run_geom(s.model.elements.get(id).unwrap()).unwrap();
                assert!((g.a[2] - z - sign * half).abs() < 1e-6, "{category}/{face}: {g:?}");
            }
            let before = s.model.to_json().unwrap();
            let axes: Vec<_> = s.model.elements.iter().filter_map(|(id, e)| s.model.run_geom(e).map(|g| (*id, g))).collect();
            run(&mut s, "move", json!({"ids":[host],"by":[100,200]}));
            for (id, old) in axes {
                let new = s.model.run_geom(s.model.elements.get(id).unwrap()).unwrap();
                assert!((new.a[0] - old.a[0] - 100.0).abs() < 1e-6);
                assert!((new.a[1] - old.a[1] - 200.0).abs() < 1e-6);
            }
            run(&mut s, "undo", json!({}));
            assert_eq!(s.model.to_json().unwrap(), before);
            run(&mut s, "modify", json!({"id":host,"offset":1500}));
            for (_, e) in s.model.elements.iter().filter(|(_, e)| e.run().is_some()) {
                let r = e.run().unwrap();
                assert!((r.offset - z - 300.0).abs() < 1e-6);
            }
            run(&mut s, "undo", json!({}));
            assert_eq!(s.model.to_json().unwrap(), before);
            let (loaded, omitted) = crate::model::Model::read(&before).unwrap();
            assert!(omitted.is_empty());
            assert_eq!(loaded.to_json().unwrap(), before);
        }
    }
}

#[test]
fn stage4_slab_mount_rejects_holes_and_unsupported_back() {
    let mut s = Session::new();
    let host = made(
        &mut s,
        "floor",
        json!({"boundary":[[0,0],[6000,0],[6000,4000],[0,4000]],"openings":[[[2000,1500],[3000,1500],[3000,2500],[2000,2500]]]}),
    );
    for (command, y) in [("wire", 2000), ("cable_carrier", 1475)] {
        let id = made(
            &mut s,
            command,
            if command == "cable_carrier" {
                json!({"points":[[1000,y],[4000,y]],"offset":500,"type":"Tray 100 x 50"})
            } else {
                json!({"points":[[1000,y],[4000,y]],"offset":500})
            },
        );
        let before = s.model.to_json().unwrap();
        assert!(s.execute("run_mount", &json!({"id":id,"host":host,"face":"top"})).is_err());
        assert_eq!(s.model.to_json().unwrap(), before);
    }
}

#[test]
fn stage4_mounted_carrier_moves_linked_wire_with_wall_and_section_depth() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].as_u64().unwrap();
    let carrier = made(&mut s, "cable_carrier", json!({"points":[[3000,-500],[5000,-500]],"offset":1500,"type":"Tray 100 x 50"}));
    run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
    let wire = made(&mut s, "wire", json!({"points":[[3500,-125],[4500,-125]],"offset":1535}));
    run(&mut s, "modify", json!({"id":wire,"carrier":carrier}));
    let guid = s.model.guid(wire);
    run(&mut s, "move", json!({"ids":[wall],"by":[0,200]}));
    let geometry = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
    assert!((geometry.a[1] - 75.0).abs() < 1e-6);
    assert!((geometry.a[2] - 1535.0).abs() < 1e-6);
    let before = s.model.to_json().unwrap();
    run(&mut s, "type", json!({"category":"cablecarrier","name":"Tray 100 x 50","width":100,"depth":80,"round":false}));
    let geometry = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
    assert!((geometry.a[1] - 60.0).abs() < 1e-6);
    assert_eq!(s.model.guid(wire), guid);
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
}

#[test]
fn stage4_rectangular_mount_uses_back_and_rotated_section_for_containment() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].as_u64().unwrap();
    let carrier = made(&mut s, "cable_carrier", json!({"points":[[3000,-500],[5000,-500]],"offset":1500,"type":"Tray 100 x 50"}));
    run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
    let g = s.model.run_geom(s.model.elements.get(carrier).unwrap()).unwrap();
    assert_eq!(g.a, [3000.0, -125.0, 1500.0]);
    assert_eq!(g.z_span(), (1450.0, 1550.0));
    let footprint = g.footprint();
    assert!(footprint.iter().all(|p| p.1 >= -150.000001 && p.1 <= -99.999999));
    let wire = made(&mut s, "wire", json!({"points":[[3500,-125],[4500,-125]],"offset":1535}));
    assert!(s.model.carrier_wire_candidates(carrier).unwrap().contains(&wire));
    let file = ifc(&mut s);
    let profile = entities(&file, "IFCRECTANGLEPROFILEDEF").into_iter().find(|line| line.ends_with(",100.,50.);")).unwrap();
    let profile_id = profile.split('=').next().unwrap();
    let sweep = entities(&file, "IFCEXTRUDEDAREASOLID").into_iter().find(|line| line.contains(&format!("({profile_id},"))).unwrap();
    let placement = sweep.split(',').nth(1).unwrap();
    let placement_line = file.lines().find(|line| line.starts_with(&format!("{placement}="))).unwrap();
    let across = placement_line.rsplit(',').next().unwrap().trim_end_matches(");");
    assert!(file.lines().any(|line| line == format!("{across}=IFCDIRECTION((0.,0.,1.));")), "{placement_line}");
    let before = s.model.to_json().unwrap();
    let (loaded, left_out) = crate::model::Model::read(&before).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(loaded.to_json().unwrap(), before);
    run(&mut s, "run_mount", json!({"id":carrier,"host":null}));
    assert_eq!(s.model.run_geom(s.model.elements.get(carrier).unwrap()).unwrap(), g);
    run(&mut s, "modify", json!({"id":carrier,"justify":"top"}));
    let top = s.model.run_geom(s.model.elements.get(carrier).unwrap()).unwrap();
    assert!((top.z_span().1 - 1500.0).abs() < 1e-6, "top alignment must use the rotated section: {top:?}");
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
}

#[test]
fn stage4_rectangular_back_rejects_opening_away_from_supported_centreline() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "door", json!({"at":[2000,0]}));
    let carrier = made(&mut s, "cable_carrier", json!({"points":[[1500,-500],[2500,-500]],"offset":2125,"type":"Tray 100 x 50"}));
    let query = run(&mut s, "surface_query", json!({"host":wall,"origin":[2000,-1000,2125],"direction":[0,1,0],"reach":2000}));
    assert!(!query["hit"].is_null(), "the contact centre itself is above the door");
    let before = s.model.to_json().unwrap();
    let error = s.execute("run_mount", &json!({"id":carrier,"host":wall,"face":"interior"})).unwrap_err();
    assert!(error.contains("mounting back"), "{error}");
    assert_eq!(s.model.to_json().unwrap(), before);
}

#[test]
fn stage4_round_mount_does_not_leak_to_generated_feeders() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].as_u64().unwrap();
    let wire = made(&mut s, "wire", json!({"points":[[3000,-500],[5000,-500]],"offset":1500}));
    run(&mut s, "run_mount", json!({"id":wire,"host":wall,"face":"interior"}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[3500,-1000],"offset":1000}));
    let legs = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"point":[0,0,0]})));
    assert_eq!(legs.len(), 2);
    for id in legs {
        assert!(s.model.elements.get(id).unwrap().run().unwrap().mount.is_none());
    }
    assert!(s.model.elements.get(wire).unwrap().run().unwrap().mount.is_some());
}

#[test]
fn stage4_round_mount_follows_level_elevation_once() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].as_u64().unwrap();
    let wire = made(&mut s, "wire", json!({"points":[[3000,-500],[5000,-500]],"offset":1500}));
    run(&mut s, "run_mount", json!({"id":wire,"host":wall,"face":"interior"}));
    let level = s.model.elements.get(wire).unwrap().run().unwrap().level;
    let before = s.model.to_json().unwrap();
    run(&mut s, "modify", json!({"id":level,"elevation":200}));
    let geometry = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
    assert_eq!(geometry.a[2], 1700.0);
    assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().offset, 1500.0);
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
}

#[test]
fn stage4_round_mount_checks_openings_copy_and_transfer_to_carrier() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "door", json!({"at":[2000,0]}));
    let blocked = made(&mut s, "wire", json!({"points":[[1500,-500],[2500,-500]],"offset":1000}));
    let before = s.model.to_json().unwrap();
    assert!(s.execute("run_mount", &json!({"id":blocked,"host":wall,"face":"interior"})).is_err());
    assert_eq!(s.model.to_json().unwrap(), before);
    let wire = made(&mut s, "wire", json!({"points":[[3500,-500],[5000,-500]],"offset":1500}));
    run(&mut s, "run_mount", json!({"id":wire,"host":wall,"face":"interior"}));
    let original = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
    let copied = run(&mut s, "copy", json!({"ids":[wall,wire],"by":[100,300]}));
    let copied_ids = copied["ids"].as_array().unwrap();
    let copy_wire = copied_ids
        .iter()
        .filter_map(Value::as_u64)
        .find(|id| matches!(s.model.elements.get(*id), Some(crate::model::Element::Wire(_))))
        .unwrap();
    let element = s.model.elements.get(copy_wire).unwrap();
    assert_ne!(element.run().unwrap().mount.as_ref().unwrap().host, s.model.guid(wall));
    let g = s.model.run_geom(element).unwrap();
    assert!((g.a[0] - original.a[0] - 100.0).abs() < 1e-8 && (g.a[1] - original.a[1] - 300.0).abs() < 1e-8);
    let carrier = made(&mut s, "cable_carrier", json!({"from_wire":wire,"type":"Tray 100 x 50"}));
    assert!(s.model.elements.get(carrier).unwrap().run().unwrap().mount.is_none());
    assert!(s.model.elements.get(wire).unwrap().run().unwrap().mount.is_none());
    assert_eq!(s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap(), original);
}

#[test]
fn stage4_round_mount_preserves_contact_through_radius_host_motion_and_delete() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "type", json!({"category":"wire","name":"Round20","width":20,"depth":20,"round":true}));
    run(&mut s, "type", json!({"category":"wire","name":"Round40","width":40,"depth":40,"round":true}));
    let wire = made(&mut s, "wire", json!({"points":[[3000,-500],[5000,-500]],"offset":1500,"type":"Round20"}));
    run(&mut s, "run_mount", json!({"id":wire,"host":wall,"face":"interior"}));
    let axis = |s: &Session| s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
    assert_eq!(axis(&s).a, [3000.0, -110.0, 1500.0]);
    assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().start, P2(3000.0, -100.0));
    run(&mut s, "modify", json!({"id":wire,"type":"Round40"}));
    assert_eq!(axis(&s).a, [3000.0, -120.0, 1500.0]);
    run(&mut s, "move", json!({"ids":[wall],"by":[0,200]}));
    assert_eq!(axis(&s).a, [3000.0, 80.0, 1500.0]);
    let before = s.model.to_json().unwrap();
    let (loaded, left_out) = crate::model::Model::read(&before).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(loaded.to_json().unwrap(), before);
    assert!(s.execute("run_mount", &json!({"id":wire})).is_err());
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "delete", json!({"ids":[wall]}));
    assert_eq!(axis(&s).a, [3000.0, 80.0, 1500.0]);
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "redo", json!({}));
    assert_eq!(axis(&s).a, [3000.0, 80.0, 1500.0]);
}

#[test]
fn stage4_surface_query_checks_the_whole_mounting_back() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].clone();
    run(&mut s, "door", json!({"at":[2000,0]}));
    let mut query = json!({"host":wall,"origin":[5000,-1000,1500],"direction":[0,1,0],"reach":2000,
        "footprint":[[1500,-100,500],[2500,-100,500],[2500,-100,2500],[1500,-100,2500]]});
    let before = s.model.to_json().unwrap();
    let history = s.history();
    assert_eq!(run(&mut s, "surface_query", query.clone())["supported"], false);
    query["footprint"] = json!([[4000, -100, 500], [5000, -100, 500], [5000, -100, 2500], [4000, -100, 2500]]);
    assert_eq!(run(&mut s, "surface_query", query.clone())["supported"], true);
    query["footprint"][0][1] = json!(-99);
    assert!(s.execute("surface_query", &query).is_err());
    query["origin"] = json!([9000, -1000, 1500]);
    query["footprint"] = json!([[0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0]]);
    assert!(s.execute("surface_query", &query).is_err());
    assert_eq!(s.model.to_json().unwrap(), before);
    assert_eq!(s.history(), history);
}

#[test]
fn stage4_surface_query_is_readonly_strict_and_rejects_stale_snapshots() {
    let mut s = Session::new();
    let wall = run(&mut s, "wall", json!({"points":[[0,0],[8000,0]]}))["ids"][0].clone();
    let query = json!({"host":wall,"origin":[5000,-1000,1500],"direction":[0,7,0],"reach":2000});
    let before = s.model.to_json().unwrap();
    let history = s.history();
    let hit = run(&mut s, "surface_query", query.clone());
    assert_eq!(hit["hit"]["point"], json!([5000.0, -100.0, 1500.0]));
    assert_eq!(hit["hit"]["face"], "interior");
    assert_eq!(s.model.to_json().unwrap(), before);
    assert_eq!(s.history(), history);
    let mut guarded = query.clone();
    guarded["revision"] = hit["revision"].clone();
    assert_eq!(run(&mut s, "surface_query", guarded.clone()), hit);
    let mut invalid = query.clone();
    invalid["typo"] = json!(true);
    assert!(s.execute("surface_query", &invalid).is_err());
    assert_eq!(run(&mut s, "surface_query", guarded.clone()), hit);
    run(&mut s, "move", json!({"ids":[wall],"by":[0,100]}));
    assert!(s.execute("surface_query", &guarded).unwrap_err().contains("stale"));
    let moved = run(&mut s, "surface_query", query.clone());
    assert_ne!(moved["revision"], hit["revision"]);
    run(&mut s, "undo", json!({}));
    let undone = run(&mut s, "surface_query", query.clone());
    assert_eq!(undone["hit"], hit["hit"]);
    assert_ne!(undone["revision"], hit["revision"]);
    run(&mut s, "redo", json!({}));
    guarded["revision"] = undone["revision"].clone();
    assert!(s.execute("surface_query", &guarded).is_err());
    let latest = run(&mut s, "surface_query", query.clone());
    guarded["revision"] = latest["revision"].clone();
    // Even when no query observes the intermediate edit, Undo is a new commit.
    run(&mut s, "move", json!({"ids":[wall],"by":[0,100]}));
    run(&mut s, "undo", json!({}));
    assert!(s.execute("surface_query", &guarded).unwrap_err().contains("stale"));
}

/// A run category and what is true of it.
struct Case {
    category: &'static str,
    /// The command that makes it, and its Revit shortcut (or the letters BIMCraft gives it).
    command: &'static str,
    alias: &'static str,
    /// The type a new one gets, and whether its section is round.
    default_type: &'static str,
    round: bool,
    /// The schedule's columns, from the system on.
    columns: &'static [&'static str],
    /// Whether its class and predefined type say which category it is when another tool wrote them, with nothing of
    /// BIMCraft's beside them.
    alone: bool,
    /// The IFC class and predefined type it is written as.
    class: &'static str,
    predefined: &'static str,
}

const CASES: &[Case] = &[
    Case {
        category: "pipe",
        command: "pipe",
        alias: "PI",
        default_type: "PVC 50",
        round: true,
        columns: &["System", "Length", "Diameter"],
        alone: true,
        class: "IFCPIPESEGMENT",
        predefined: ".RIGIDSEGMENT.",
    },
    Case {
        category: "cablecarrier",
        command: "cable_carrier",
        alias: "CC",
        default_type: "PVC 25",
        round: true,
        columns: &["System", "Length", "Width", "Depth"],
        alone: true,
        class: "IFCCABLECARRIERSEGMENT",
        predefined: ".CONDUITSEGMENT.",
    },
    Case {
        category: "wire",
        command: "wire",
        alias: "EW",
        default_type: "Wire 2.5",
        round: true,
        columns: &["System", "Length", "Diameter"],
        alone: true,
        class: "IFCCABLESEGMENT",
        predefined: ".CABLESEGMENT.",
    },
    Case {
        category: "downconductor",
        command: "down_conductor",
        alias: "DC",
        default_type: "Steel 8",
        round: true,
        columns: &["System", "Length", "Width", "Depth"],
        alone: true,
        class: "IFCCABLESEGMENT",
        predefined: ".CONDUCTORSEGMENT.",
    },
    Case {
        category: "groundconductor",
        command: "ground_conductor",
        alias: "GC",
        default_type: "Copper 9",
        round: true,
        columns: &["System", "Length", "Width", "Depth"],
        alone: false,
        class: "IFCCABLESEGMENT",
        predefined: ".CONDUCTORSEGMENT.",
    },
    Case {
        category: "cable",
        command: "cabling",
        alias: "CB",
        default_type: "Cat6",
        round: true,
        columns: &["System", "Length", "Diameter"],
        alone: false,
        class: "IFCCABLESEGMENT",
        predefined: ".CABLESEGMENT.",
    },
];

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn ids(r: &Value) -> Vec<u64> {
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

#[test]
fn stage4_carrier_from_wire_keeps_slope_and_guid_link() {
    let mut s = Session::default();
    let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 2100, "end_offset": 2700}));
    let before = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
    let carrier = made(&mut s, "cable_carrier", json!({"from_wire": wire, "type": "Tray 100 x 50"}));
    let actual = s.model.run_geom(s.model.elements.get(carrier).unwrap()).unwrap();
    assert_eq!(actual.a, before.a);
    assert_eq!(actual.b, before.b);
    let w = s.model.elements.get(wire).unwrap().run().unwrap();
    assert_eq!(w.carrier, Some(s.model.guid(carrier)));
}

#[test]
fn stage4_carrier_from_vertical_wire_is_one_undoable_action() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points": [[50,50],[50,50]], "offset": 500, "end_offset": 3000}));
    let before = s.model.clone();
    let carrier = made(&mut s, "cable_carrier", json!({"from_wire": wire, "type": "Tray 100 x 50"}));
    let after = s.model.clone();
    let geometry = s.model.run_geom(s.model.elements.get(carrier).unwrap()).unwrap();
    assert!(geometry.vertical());
    assert_eq!(geometry.length(), 2500.0);
    run(&mut s, "undo", json!({}));
    assert_eq!(*s.model, *before);
    run(&mut s, "redo", json!({}));
    assert_eq!(*s.model, *after);
    fails(&mut s, "cable_carrier", json!({"from_wire": wire, "type": "Tray 100 x 50"}));
    fails(&mut s, "cable_carrier", json!({"from_wire": carrier}));
}

#[test]
fn stage4_carrier_from_wire_rejects_mixed_geometry_and_bad_type_atomically() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points": [[0,0],[1000,0]]}));
    fails(&mut s, "cable_carrier", json!({"from_wire": wire, "points": [[0,0],[2000,0]]}));
    fails(&mut s, "cable_carrier", json!({"from_wire": wire, "offset": 400}));
    fails(&mut s, "cable_carrier", json!({"from_wire": wire, "type": "missing carrier"}));
}

#[test]
fn stage4_ring_ends_at_one_device_keep_independent_feeders_and_undo() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[1000,1000],"offset":1000}));
    let first = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"end":0,"point":[-100,0,0]})));
    let second = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"end":1,"point":[100,0,0]})));
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert!(first.iter().all(|id| !second.contains(id)));
    let before = s.model.to_json().unwrap();
    let updated = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"end":1,"point":[150,0,0]})));
    assert_eq!(updated, second);
    let run_model = s.model.elements.get(wire).unwrap().run().unwrap();
    assert_eq!(run_model.connections.len(), 2);
    assert_eq!(run_model.connections[0].point, [-100.0, 0.0, 0.0]);
    let (opened, left_out) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(opened.elements.get(wire).unwrap().run().unwrap().connections, run_model.connections);
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "redo", json!({}));
    assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().connections.len(), 2);
    let snapshot = s.model.to_json().unwrap();
    assert!(s.execute("wire_connect", &json!({"wire":wire,"device":device,"end":2,"point":[0,0,0]})).is_err());
    assert_eq!(s.model.to_json().unwrap(), snapshot);
}

#[test]
fn stage4_ring_ends_follow_separate_carriers_and_keep_geometry_when_one_is_deleted() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let first = made(&mut s, "cable_carrier", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let second = made(&mut s, "cable_carrier", json!({"points":[[0,300],[3000,300]],"offset":3200}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[1000,1000],"offset":1000}));
    let ends: Vec<_> = [first, second]
        .into_iter()
        .enumerate()
        .map(|(end, carrier)| {
            ids(&run(
                &mut s,
                "wire_connect",
                json!({"wire":wire,"device":device,"end":end,"carrier":carrier,
            "point":[0,0,0],"in_carrier":true}),
            ))
        })
        .collect();
    let target = |s: &Session, ids: &[u64]| s.model.run_geom(s.model.elements.get(*ids.last().unwrap()).unwrap()).unwrap().b;
    assert_eq!(target(&s, &ends[0]), [1000.0, 0.0, 3000.0]);
    assert_eq!(target(&s, &ends[1]), [1000.0, 300.0, 3200.0]);
    let guids: Vec<_> = ends.iter().flatten().map(|id| s.model.guid(*id)).collect();
    let other_wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let other_ends: Vec<_> = [first, second]
        .into_iter()
        .enumerate()
        .map(|(end, carrier)| {
            ids(&run(
                &mut s,
                "wire_connect",
                json!({"wire":other_wire,"device":device,"end":end,"carrier":carrier,
            "point":[0,0,0],"in_carrier":true}),
            ))
        })
        .collect();
    assert!(other_ends.iter().flatten().all(|id| !ends.iter().flatten().any(|existing| existing == id)));
    run(&mut s, "move", json!({"ids":[second],"by":[0,100]}));
    assert_eq!(target(&s, &ends[0]), [1000.0, 0.0, 3000.0]);
    assert_eq!(target(&s, &ends[1]), [1000.0, 400.0, 3200.0]);
    assert_eq!(target(&s, &other_ends[1]), [1000.0, 400.0, 3200.0]);
    assert_eq!(ends.iter().flatten().map(|id| s.model.guid(*id)).collect::<Vec<_>>(), guids);
    let before = s.model.to_json().unwrap();
    run(&mut s, "delete", json!({"ids":[second]}));
    assert_eq!(target(&s, &ends[1]), [1000.0, 400.0, 3200.0]);
    assert_eq!(target(&s, &ends[0]), [1000.0, 0.0, 3000.0]);
    assert_eq!(target(&s, &other_ends[1]), [1000.0, 400.0, 3200.0]);
    assert!(ends[1].iter().all(|id| s.model.elements.get(*id).unwrap().run().unwrap().carrier.is_none()));
    let (opened, left_out) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(opened.to_json().unwrap(), s.model.to_json().unwrap());
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "redo", json!({}));
    let snapshot = s.model.to_json().unwrap();
    assert!(s.execute("wire_connect", &json!({"wire":wire,"device":device,"carrier":wire,"point":[0,0,0]})).is_err());
    assert_eq!(s.model.to_json().unwrap(), snapshot);
    let copied = ids(&run(&mut s, "copy", json!({"ids":[wire,device,first],"by":[500,0]})));
    let copy = copied.iter().find_map(|id| s.model.elements.get(*id).unwrap().run().filter(|r| !r.connections.is_empty())).unwrap();
    assert_ne!(copy.connections[0].carrier, Some(s.model.guid(first)));
    assert_eq!(copy.connections[1].landing, Some([1500.0, 400.0, 3200.0]));
    assert_ne!(copy.connections[0].device, s.model.guid(device));
}

#[test]
fn stage4_format14_connections_and_owners_default_to_first_end() {
    let device = crate::model::Guid::new();
    let wire = crate::model::Guid::new();
    let connection: crate::model::WireConnection =
        serde_json::from_value(json!({"device":device,"point":[0,0,0],"in_carrier":false})).unwrap();
    let feeder: crate::model::Feeder = serde_json::from_value(json!({"wire":wire,"device":device,"leg":0})).unwrap();
    assert_eq!(connection.end, 0);
    assert_eq!(connection.carrier, None);
    assert_eq!(connection.landing, None);
    assert_eq!(feeder.end, 0);
}

#[test]
fn stage4_connection_tracks_explicit_device_point_without_duplicating_vertical_wire() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 3000}));
    let device = made(&mut s, "component", json!({"family": "Fire Alarm Panel", "type": "300 x 400", "at": [0,0], "offset": 1000}));
    let result = run(&mut s, "wire_connect", json!({"wire": wire, "device": device, "point": [0,0,0]}));
    let feeder = ids(&result)[0];
    let guid = s.model.guid(feeder);
    let geometry = s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap();
    assert!(geometry.vertical());
    assert_eq!(geometry.length(), 2000.0);
    run(&mut s, "modify", json!({"id": device, "offset": 1500}));
    assert_eq!(s.model.guid(feeder), guid);
    assert_eq!(s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap().length(), 1500.0);
    assert_eq!(s.model.elements.values().filter(|e| e.category() == "wire").count(), 2);
}

#[test]
fn stage4_connection_follows_translated_and_rotated_local_point_in_place() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[0,1000],"offset":1000,"angle":90}));
    let legs = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"point":[100,200,50]})));
    assert_eq!(legs.len(), 2);
    let guids: Vec<_> = legs.iter().map(|id| s.model.guid(*id)).collect();
    run(&mut s, "move", json!({"ids":[device],"by":[500,500]}));
    run(&mut s, "modify", json!({"id":device,"angle":180}));
    run(&mut s, "move", json!({"ids":[wire],"by":[0,1000]}));
    assert_eq!(legs.iter().map(|id| s.model.guid(*id)).collect::<Vec<_>>(), guids);
    let vertical = s.model.run_geom(s.model.elements.get(legs[0]).unwrap()).unwrap();
    let horizontal = s.model.run_geom(s.model.elements.get(legs[1]).unwrap()).unwrap();
    for (actual, expected) in vertical.a.into_iter().zip([400.0, 1300.0, 1050.0]) {
        assert!((actual - expected).abs() < 1e-6);
    }
    assert!((horizontal.b[0] - 400.0).abs() < 1e-6);
    assert_eq!(horizontal.b[1], 1000.0);
    assert_eq!(horizontal.b[2], 3000.0);
    assert_eq!(s.model.elements.values().filter(|element| element.category() == "wire").count(), 3);
}

#[test]
fn stage4_connections_cover_above_equal_sloped_and_horizontal_routes() {
    for (at, height, end_height, expected) in [
        ([1500, 0], 4000.0, 3000.0, 1000.0),
        ([1500, 0], 3000.0, 3000.0, 0.0),
        ([1500, 0], 1000.0, 4000.0, 2500.0),
        ([1500, 2000], 1000.0, 3000.0, 4000.0),
    ] {
        let mut s = Session::new();
        let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 3000, "end_offset": end_height}));
        let device = made(&mut s, "component", json!({"family": "Fire Alarm Panel", "at": at, "offset": height}));
        let params = json!({"wire": wire, "device": device, "point": [0,0,0]});
        let feeders = ids(&run(&mut s, "wire_connect", params.clone()));
        let total: f64 = feeders.iter().map(|id| s.model.run_geom(s.model.elements.get(*id).unwrap()).unwrap().length()).sum();
        assert!((total - expected).abs() < 1e-6, "at={at:?}, height={height}, end={end_height}: {total}");
        assert_eq!(ids(&run(&mut s, "wire_connect", params)), feeders, "reconnecting preserves generated elements");
    }
}

#[test]
fn stage4_connection_creates_carrier_and_counts_physical_reserve_once() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 3000, "reserve": 10}));
    made(&mut s, "cable_carrier", json!({"from_wire": wire, "type": "Tray 100 x 50"}));
    let device = made(&mut s, "component", json!({"family": "Fire Alarm Panel", "at": [0,0], "offset": 1000}));
    let result = run(&mut s, "wire_connect", json!({"wire": wire, "device": device, "point": [0,0,0], "in_carrier": true}));
    let feeder = ids(&result)[0];
    let carrier = result["carriers"][0].as_u64().unwrap();
    assert_eq!(s.model.elements.get(feeder).unwrap().run().unwrap().carrier, Some(s.model.guid(carrier)));
    let length = element_json(&s.model, wire)["length"].as_f64().unwrap() + element_json(&s.model, feeder)["length"].as_f64().unwrap();
    assert_eq!(length, 5500.0, "5000 physical mm plus 10 percent once");
    let before = s.model.clone();
    fails(&mut s, "wire_connect", json!({"wire": wire, "device": device, "point": [0,0,1e300]}));
    assert_eq!(s.model, before);
}

#[test]
fn stage4_connection_keeps_guids_through_save_open_and_undo_redo() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 3000}));
    let device = made(&mut s, "component", json!({"family": "Fire Alarm Panel", "at": [0,0], "offset": 1000}));
    let feeder = ids(&run(&mut s, "wire_connect", json!({"wire": wire, "device": device, "point": [0,0,0]})))[0];
    let guid = s.model.guid(feeder);
    let connected = s.model.clone();
    run(&mut s, "undo", json!({}));
    assert!(s.model.elements.get(feeder).is_none());
    run(&mut s, "redo", json!({}));
    assert_eq!(s.model, connected);
    assert_eq!(s.model.guid(feeder), guid);
    let (opened, left_out) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(opened.elements.guid(feeder), Some(guid));
    let mut reopened = Session::new();
    *reopened.model_mut() = opened;
    run(&mut reopened, "modify", json!({"id": device, "offset": 1500}));
    assert_eq!(reopened.model.guid(feeder), guid);
    assert_eq!(reopened.model.run_geom(reopened.model.elements.get(feeder).unwrap()).unwrap().length(), 1500.0);
}

#[test]
fn stage4_connection_follows_wire_and_carrier_without_changing_feeder_guid() {
    for move_carrier in [false, true] {
        let mut s = Session::new();
        let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 3000}));
        let carrier = made(&mut s, "cable_carrier", json!({"from_wire": wire, "type": "Tray 100 x 50"}));
        let device = made(&mut s, "component", json!({"family": "Fire Alarm Panel", "at": [0,0], "offset": 1000}));
        let result = run(&mut s, "wire_connect", json!({"wire": wire, "device": device, "point": [0,0,0], "in_carrier": true}));
        let feeder = ids(&result)[0];
        let feeder_guid = s.model.guid(feeder);
        let feeder_carrier = result["carriers"][0].as_u64().unwrap();
        let carrier_guid = s.model.guid(feeder_carrier);
        run(&mut s, "modify", json!({"id": if move_carrier { carrier } else { wire }, "offset": 4000, "end_offset": 4000}));
        assert_eq!(s.model.guid(feeder), feeder_guid);
        assert_eq!(s.model.guid(feeder_carrier), carrier_guid);
        assert_eq!(s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap().length(), 3000.0);
        assert_eq!(s.model.elements.values().filter(|e| e.category() == "wire").count(), 2);
        assert_eq!(s.model.elements.values().filter(|e| e.category() == "cablecarrier").count(), 2);
    }
}

#[test]
fn stage4_deleting_connected_device_keeps_main_wire_and_removes_owned_legs() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 3000}));
    let device = made(&mut s, "component", json!({"family": "Fire Alarm Panel", "at": [0,0], "offset": 1000}));
    let feeder = ids(&run(&mut s, "wire_connect", json!({"wire": wire, "device": device, "point": [0,0,0]})))[0];
    let guid = s.model.guid(wire);
    run(&mut s, "delete", json!({"ids": [device]}));
    assert_eq!(s.model.guid(wire), guid);
    assert!(s.model.elements.get(wire).unwrap().run().unwrap().connections.is_empty());
    assert!(s.model.elements.get(feeder).is_none());
    assert_eq!(s.model.elements.values().filter(|e| e.category() == "wire").count(), 1);
    run(&mut s, "undo", json!({}));
    assert!(s.model.elements.get(feeder).is_some());
    assert!(s.model.elements.get(device).is_some());
}

#[test]
fn stage4_removing_last_device_keeps_the_main_wires_physical_reserve() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000,"reserve":10}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[1500,0],"offset":3000}));
    run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"point":[0,0,0]}));
    let before = crate::element_json(&s.model, wire)["length"].clone();
    assert_eq!(before, json!(3300.0));
    run(&mut s, "delete", json!({"ids":[device]}));
    assert_eq!(crate::element_json(&s.model, wire)["length"], before);
}

#[test]
fn stage4_copying_a_feeder_alone_creates_an_independent_wire() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points": [[0,0],[3000,0]], "offset": 3000}));
    let device = made(&mut s, "component", json!({"family": "Fire Alarm Panel", "at": [0,0], "offset": 1000}));
    let feeder = ids(&run(&mut s, "wire_connect", json!({"wire": wire, "device": device, "point": [0,0,0]})))[0];
    let original_guid = s.model.guid(feeder);
    let copy = ids(&run(&mut s, "copy", json!({"ids": [feeder], "by": [1000,0]})))[0];
    let copied = s.model.elements.get(copy).unwrap().run().unwrap();
    assert!(copied.feeder.is_none());
    assert!(copied.connections.is_empty());
    assert_ne!(s.model.guid(copy), original_guid);
    run(&mut s, "modify", json!({"id": device,"offset": 1500}));
    assert_eq!(s.model.guid(feeder), original_guid);
    assert_eq!(s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap().length(), 1500.0);
    assert_eq!(s.model.run_geom(s.model.elements.get(copy).unwrap()).unwrap().length(), 2000.0);
}

#[test]
fn stage4_carrier_overlap_lists_candidates_without_silently_attaching() {
    for (end, end_height) in [([3000, 0], 3500.0), ([0, 0], 6000.0)] {
        let mut s = Session::new();
        let wire_params = json!({"points":[[0,0],end],"offset":3000,"end_offset":end_height});
        let first = made(&mut s, "wire", wire_params.clone());
        let second = made(&mut s, "wire", wire_params.clone());
        let carrier =
            made(&mut s, "cable_carrier", json!({"points":[[0,0],end],"offset":3000,"end_offset":end_height,"type":"Tray 100 x 50"}));
        let before = s.model.clone();
        let candidates = run(&mut s, "cable_carrier_candidates", json!({"carrier":carrier}));
        assert_eq!(candidates["wires"], json!([first, second]));
        assert_eq!(candidates["ambiguous"], true);
        assert_eq!(s.model, before, "candidate lookup never confirms a connection");
        assert!(s.model.elements.get(first).unwrap().run().unwrap().carrier.is_none());
        run(&mut s, "modify", json!({"id":second,"carrier":carrier}));
        assert!(s.model.elements.get(first).unwrap().run().unwrap().carrier.is_none());
        assert_eq!(s.model.elements.get(second).unwrap().run().unwrap().carrier, Some(s.model.guid(carrier)));
        let candidates = run(&mut s, "cable_carrier_candidates", json!({"carrier":carrier}));
        assert_eq!(candidates["wires"], json!([first]));
        assert_eq!(candidates["ambiguous"], false);
    }
}

#[test]
fn stage4_carrier_overlap_excludes_crossing_and_distant_wires() {
    let mut s = Session::new();
    let carrier = made(&mut s, "cable_carrier", json!({"points":[[0,0],[3000,0]],"offset":3000,"type":"Tray 100 x 50"}));
    for params in [
        json!({"points":[[1500,-1000],[1500,1000]],"offset":3000}),
        json!({"points":[[0,0],[3000,0]],"offset":3100}),
        json!({"points":[[4000,0],[7000,0]],"offset":3000}),
    ] {
        made(&mut s, "wire", params);
    }
    let candidates = run(&mut s, "cable_carrier_candidates", json!({"carrier":carrier}));
    assert_eq!(candidates["wires"], json!([]));
    fails(&mut s, "cable_carrier_candidates", json!({"carrier":carrier,"unexpected":true}));
}

#[test]
fn stage4_carrier_from_wire_respects_section_placement_without_moving_wire() {
    use crate::view3d::{cross, dot, sub, unit};
    for end_height in [3000.0, 4000.0] {
        let mut s = Session::new();
        let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000,"end_offset":end_height}));
        let original = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
        let carrier =
            made(&mut s, "cable_carrier", json!({"from_wire":wire,"type":"Tray 100 x 50","placement":"bottom","local_offset":10}));
        let geometry = s.model.run_geom(s.model.elements.get(carrier).unwrap()).unwrap();
        let after = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap();
        assert_eq!(after.a, original.a);
        assert_eq!(after.b, original.b);
        let axis = unit(sub(geometry.b, geometry.a));
        let x = unit([-axis[1], axis[0], 0.0]);
        let y = cross(axis, x);
        let delta = sub(original.a, geometry.a);
        assert!((dot(delta, x) - 10.0).abs() < 1e-6);
        assert!((dot(delta, y) + (geometry.depth - original.depth) / 2.0).abs() < 1e-6);
        assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().local_offset, 10.0);
    }
}

#[test]
fn stage4_carrier_from_owned_feeder_is_refused_without_leaving_an_orphan() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[0,0],"offset":1000}));
    let feeder = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"point":[0,0,0]})))[0];
    fails(&mut s, "cable_carrier", json!({"from_wire":feeder,"type":"Tray 100 x 50"}));
}

#[test]
fn stage4_carrier_placement_rejects_rectangular_cable_corners_outside_round_section() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category":"wire","name":"Square Cable","width":30,"depth":30,"round":false}));
    run(&mut s, "type", json!({"category":"cablecarrier","name":"Round 40","width":40,"depth":40,"round":true}));
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"type":"Square Cable","offset":3000}));
    fails(&mut s, "cable_carrier", json!({"from_wire":wire,"type":"Round 40","placement":"centre"}));
}

#[test]
fn stage4_deleting_main_carrier_detaches_connection_and_keeps_physical_wire_guids() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let carrier = made(&mut s, "cable_carrier", json!({"from_wire":wire,"type":"Tray 100 x 50"}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[0,0],"offset":1000}));
    let result = run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"point":[0,0,0],"in_carrier":true}));
    let feeder = ids(&result)[0];
    let guid = s.model.guid(feeder);
    run(&mut s, "delete", json!({"ids":[carrier]}));
    assert_eq!(s.model.guid(feeder), guid);
    assert!(s.model.elements.get(feeder).unwrap().run().unwrap().carrier.is_none());
    assert!(!s.model.elements.get(wire).unwrap().run().unwrap().connections[0].in_carrier);
    assert_eq!(s.model.elements.values().filter(|e| e.category() == "cablecarrier").count(), 0);
    assert_eq!(s.model.run_geom(s.model.elements.get(feeder).unwrap()).unwrap().length(), 2000.0);
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn made(s: &mut Session, id: &str, p: Value) -> u64 {
    let r = run(s, id, p);
    r["id"].as_u64().or_else(|| ids(&r).first().copied()).unwrap()
}

/// The category's plural, as a person types it.
fn plural(c: &Case) -> String {
    format!("{}s", c.category)
}

/// The columns a run schedule starts with.
fn columns(c: &Case) -> Value {
    let mut all = vec!["Id", "Type", "Level"];
    all.extend(c.columns);
    json!(all)
}

#[test]
fn a_run_of_the_mep_is_made_between_points_one_for_each_segment_centred_on_its_height() {
    for c in CASES {
        let mut s = Session::new();
        let r = run(&mut s, c.command, json!({"points": [[0, 0], [6000, 0], [6000, 4000]]}));
        let made = ids(&r);
        assert_eq!(made.len(), 2, "{}", c.category);
        let e = element_json(&s.model, made[0]);
        assert_eq!(
            (e["category"].as_str(), e["type"].as_str(), e["justify"].as_str(), e["round"].as_bool()),
            (Some(c.category), Some(c.default_type), Some("centre"), Some(c.round)),
            "{}",
            c.category
        );
        // Its centre line is at the level, where its `offset` says.
        assert_eq!(
            (e["length"].as_f64(), e["axis"].clone()),
            (Some(6000.0), json!([[0.0, 0.0, 0.0], [6000.0, 0.0, 0.0]])),
            "{}",
            c.category
        );
        assert_eq!(element_json(&s.model, made[1])["length"], 4000.0);
        // The shortcut; closed, a ring; the plural finds them.
        let ring = run(&mut s, c.alias, json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "closed": true}));
        assert_eq!(ids(&ring).len(), 3, "{}", c.alias);
        assert_eq!(run(&mut s, "elements", json!({"category": plural(c)}))["count"], 5, "{}", c.category);
    }
}

#[test]
fn a_run_of_the_mep_falls_rises_stands_and_says_which_system_it_is_in() {
    for c in CASES {
        let mut s = Session::new();
        // A drain falling 60 mm over 6 m, in a system; a riser from the floor to the ceiling above, upright.
        let r = run(
            &mut s,
            c.command,
            json!({"points": [[0, 0], [6000, 0]], "offset": 2400, "end_offset": 2340, "system": "Esgoto", "level": "Level 1"}),
        );
        let e = element_json(&s.model, ids(&r)[0]);
        assert_eq!(e["axis"], json!([[0.0, 0.0, 2400.0], [6000.0, 0.0, 2340.0]]), "{}", c.category);
        assert_eq!(e["system"], "Esgoto");
        let up = run(&mut s, c.command, json!({"points": [[500, 500], [500, 500]], "offset": 0, "end_offset": 3000}));
        let e = element_json(&s.model, ids(&up)[0]);
        assert_eq!(
            (e["length"].as_f64(), e["axis"].clone()),
            (Some(3000.0), json!([[500.0, 500.0, 0.0], [500.0, 500.0, 3000.0]])),
            "{}",
            c.category
        );
        // And its height changes with `modify`, its system too.
        run(&mut s, "modify", json!({"id": ids(&up)[0], "offset": 100, "system": "Água fria"}));
        let e = element_json(&s.model, ids(&up)[0]);
        assert_eq!((e["axis"][0][2].as_f64(), e["system"].as_str()), (Some(100.0), Some("Água fria")), "{}", c.category);
    }
}

#[test]
fn a_run_of_the_mep_that_cannot_be_one_is_refused_and_nothing_changes() {
    for c in CASES {
        let mut s = Session::new();
        let line = json!([[0, 0], [3000, 0]]);
        let cmd = c.command;
        assert!(fails(&mut s, cmd, json!({"points": [[0, 0], [0, 0]]})).contains("too short"));
        assert!(fails(&mut s, cmd, json!({"points": [[0, 0]]})).contains("at least 2"));
        assert!(fails(&mut s, cmd, json!({"points": line, "type": "Nope"})).contains("type `Nope` doesn't exist"));
        assert!(fails(&mut s, cmd, json!({"points": line, "justify": "sideways"})).contains("justify"));
        assert!(fails(&mut s, cmd, json!({"points": line, "offset": 1e300})).contains("offset"));
        assert!(fails(&mut s, cmd, json!({"points": line, "system": "x".repeat(300)})).contains("system"));
        assert!(fails(&mut s, cmd, json!({"points": [[0, 0], [1000, 0], [2000, 0]], "end_offset": 100})).contains("end_offset"));
    }
}

#[test]
fn types_of_a_run_are_made_and_changed_with_type_round_or_not_and_in_a_material() {
    for c in CASES {
        let mut s = Session::new();
        let made = run(
            &mut s,
            "type",
            json!({"category": c.category, "name": "Mine", "width": 75, "depth": 75, "round": c.round, "material": "Metal"}),
        );
        assert_eq!(made["type"], json!({"width": 75.0, "depth": 75.0, "round": c.round, "material": "Metal"}), "{}", c.category);
        let id = ids(&run(&mut s, c.command, json!({"points": [[0, 0], [3000, 0]], "type": "Mine"})))[0];
        run(&mut s, "type", json!({"category": c.category, "name": "Mine", "width": 90}));
        assert_eq!(element_json(&s.model, id)["width"], 90.0, "it changes where it is used");
        assert!(
            fails(
                &mut s,
                "type",
                json!({"category": c.category, "name": "Odd", "width": 50, "depth": 50, "round": c.round, "material": "Unobtainium"})
            )
            .contains("library")
        );
    }
}

#[test]
fn the_runs_of_the_mep_move_copy_rotate_mirror_and_delete() {
    for c in CASES {
        let mut s = Session::new();
        let a = ids(&run(&mut s, c.command, json!({"points": [[0, 0], [4000, 0]], "offset": 2800, "end_offset": 3000})))[0];
        let axis = |s: &Session, id| element_json(&s.model, id)["axis"].clone();
        let (z0, z1) = (axis(&s, a)[0][2].clone(), axis(&s, a)[1][2].clone());
        run(&mut s, "move", json!({"ids": [a], "by": [1000, 500]}));
        assert_eq!(axis(&s, a)[0], json!([1000.0, 500.0, z0]), "heights stay");
        run(&mut s, "rotate", json!({"ids": [a], "about": [1000, 500], "angle": 90}));
        let end = axis(&s, a)[1].clone();
        assert!((end[0].as_f64().unwrap() - 1000.0).abs() < 1e-6 && (end[1].as_f64().unwrap() - 4500.0).abs() < 1e-6, "{end}");
        let m = run(&mut s, "mirror", json!({"ids": [a], "axis": [[0, 0], [0, 1000]], "copy": true}));
        let mirrored = axis(&s, ids(&m)[0]);
        assert_eq!((&mirrored[0][2], &mirrored[1][2]), (&z0, &z1), "each end keeps its height");
        run(&mut s, "copy", json!({"ids": [a], "by": [0, -2000]}));
        assert_eq!(run(&mut s, "elements", json!({"category": c.category}))["count"], 3);
        run(&mut s, "delete", json!({"ids": [a]}));
        run(&mut s, "undo", json!({}));
        assert_eq!(run(&mut s, "elements", json!({"category": c.category}))["count"], 3);
    }
}

#[test]
fn a_project_with_runs_of_the_mep_saves_and_opens_again() {
    let mut s = Session::new();
    for c in CASES {
        run(&mut s, c.command, json!({"points": [[0, 0], [4000, 0]], "system": "S"}));
    }
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-mep.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(*t.model, *s.model);
}

// ---------- drawn ----------

fn view() -> View {
    View { scale: 0.1, cx: 3000.0, cy: 0.0, width: 1000, height: 800 }
}

fn of(d: &Drawing, id: u64) -> Vec<&Prim> {
    d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)).map(|(p, _)| p).collect()
}

/// The box the closed lines and fills of `id` take in the drawing, in model mm.
fn extent(d: &Drawing, id: u64) -> (P2, P2) {
    let pts: Vec<P2> = of(d, id)
        .into_iter()
        .flat_map(|p| match p {
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
            Prim::Image { .. } => Vec::new(),
        })
        .map(|q| d.view.mm(q))
        .collect();
    let lo = pts.iter().fold(P2(f64::MAX, f64::MAX), |a, q| P2(a.0.min(q.0), a.1.min(q.1)));
    let hi = pts.iter().fold(P2(f64::MIN, f64::MIN), |a, q| P2(a.0.max(q.0), a.1.max(q.1)));
    let tenth = |x: f64| (x * 10.0).round() / 10.0;
    (P2(tenth(lo.0), tenth(lo.1)), P2(tenth(hi.0), tenth(hi.1)))
}

#[test]
fn a_plan_sees_a_run_through_its_cut_plane_overhead_and_under_the_level_and_the_sketch_draws_it_too() {
    for c in CASES {
        let mut s = Session::new();
        // 1200 up: the cut plane passes through it. 2800 up: overhead, under the level above. Hanging with its top at the level.
        let cut = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 1200}));
        let over = made(&mut s, c.command, json!({"points": [[0, 1000], [6000, 1000]], "offset": 2800}));
        let high = made(&mut s, c.command, json!({"points": [[0, 4000], [6000, 4000]], "offset": 6000}));
        let d = render::draw(&s.model, 100000, view());
        let half = element_json(&s.model, cut)["width"].as_f64().unwrap() / 2.0;
        assert_eq!(extent(&d, cut), (P2(0.0, -half), P2(6000.0, half)), "{}", c.category);
        assert!(of(&d, cut).iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "{} is cut", c.category);
        assert!(
            !of(&d, over).is_empty() && of(&d, over).iter().all(|p| matches!(p, Prim::Line { dash: Some(_), .. })),
            "{} overhead is dashed",
            c.category
        );
        assert!(of(&d, high).is_empty(), "{} over the view range's top is not seen", c.category);
        let sketch = render::sketch(&s.model, view(), 999, &s.model.elements[&cut]);
        assert_eq!(extent(&sketch, 999), (P2(0.0, -half), P2(6000.0, half)), "{}", c.category);
    }
}

#[test]
fn a_section_cuts_a_run_across_its_section_a_round_one_in_a_ring() {
    for c in CASES {
        let mut s = Session::new();
        let a = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 2400}));
        let section = made(&mut s, "section", json!({"start": [3000, -2000], "end": [3000, 2000]}));
        let d = render::section(&s.model, section, View { scale: 0.1, cx: 2000.0, cy: 1500.0, width: 1000, height: 800 });
        let w = element_json(&s.model, a)["width"].as_f64().unwrap();
        let (lo, hi) = extent(&d, a);
        // Across the line at 2000 along it, centred on 2400 up; a round one's ring is a polygon in its circle, to the tenth.
        let (dx, dz) = if c.round { (w / 2.0, w / 2.0) } else { (w / 2.0, element_json(&s.model, a)["depth"].as_f64().unwrap() / 2.0) };
        let tol = if c.round { 0.2 * w / 2.0 / 50.0 + 0.5 } else { 0.05 };
        assert!(
            (lo.0 - (2000.0 - dx)).abs() <= tol + 0.1
                && (hi.0 - (2000.0 + dx)).abs() <= tol + 0.1
                && (lo.1 - (2400.0 - dz)).abs() <= tol + 0.1
                && (hi.1 - (2400.0 + dz)).abs() <= tol + 0.1,
            "{}: {lo:?} {hi:?}",
            c.category
        );
        assert!(of(&d, a).iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "{} is cut in poché", c.category);
    }
}

#[test]
fn the_3d_view_has_a_solid_for_a_run() {
    for c in CASES {
        let mut s = Session::new();
        let a = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 2400}));
        let solids = crate::view3d::solids(&s.model);
        let mine: Vec<_> = solids.iter().filter(|x| x.owner() == a).collect();
        let w = element_json(&s.model, a)["width"].as_f64().unwrap();
        let depth = element_json(&s.model, a)["depth"].as_f64().unwrap();
        let (lo, hi) = mine[0].heights();
        assert!((lo - (2400.0 - depth / 2.0)).abs() < 1e-6 && (hi - (2400.0 + depth / 2.0)).abs() < 1e-6, "{}: {lo} {hi} {w}", c.category);
        assert_eq!(matches!(mine.as_slice(), [crate::view3d::Solid::Mesh { round: true, .. }]), c.round, "{}", c.category);
    }
}

#[test]
fn points_picked_snap_to_the_ends_and_middle_of_a_run() {
    use crate::snap::{Kind, Reach, Snaps, snap};
    for c in CASES {
        let mut s = Session::new();
        made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "offset": 2400}));
        let reach = Reach { point: 100.0, line: 60.0 };
        let at = |p: P2| {
            let f = snap(&s.model, 100000, p, reach, &[], false, &Snaps::default());
            (f.at, f.kind)
        };
        assert_eq!(at(P2(6040.0, 30.0)), (P2(6000.0, 0.0), Kind::Endpoint), "{}", c.category);
        assert_eq!(at(P2(3040.0, 30.0)), (P2(3000.0, 0.0), Kind::Midpoint), "{}", c.category);
    }
}

#[test]
fn a_run_of_the_mep_has_a_schedule_by_system_hides_by_category_and_holds_project_parameters() {
    for c in CASES {
        let mut s = Session::new();
        let a = made(&mut s, c.command, json!({"points": [[0, 0], [6000, 0]], "system": "Sistema A"}));
        made(&mut s, c.command, json!({"points": [[0, 3000], [3000, 3000]], "system": "Sistema B"}));
        let r = run(&mut s, "schedule", json!({"name": "S", "category": plural(c)}));
        assert_eq!(r["columns"], columns(c), "{}", c.category);
        assert_eq!(r["rows"].as_array().unwrap().len(), 2);
        let r = run(
            &mut s,
            "schedule",
            json!({"name": "S", "fields": ["System", "Count", {"name": "Length", "total": true}], "sort": ["System"], "grouped": true}),
        );
        assert_eq!(r["rows"], json!([["Sistema A", 1.0, 6000.0], ["Sistema B", 1.0, 3000.0]]), "{}", c.category);
        assert_eq!(r["totals"], json!([null, null, 9000.0]));
        // Hidden by category in the plan.
        let shown = |s: &Session| render::draw(&s.model, 100000, view()).owners.iter().filter(|o| **o == Some(a)).count();
        assert!(shown(&s) > 0, "{}", c.category);
        run(&mut s, "graphics", json!({"view": "Level 1", "hide": [c.category]}));
        assert_eq!(shown(&s), 0, "{}", c.category);
        // A project parameter for the category, and a value of it.
        run(&mut s, "parameter", json!({"name": "Circuito", "kind": "text", "categories": [c.category]}));
        run(&mut s, "modify", json!({"id": a, "params": {"Circuito": "C1"}}));
        assert_eq!(element_json(&s.model, a)["params"]["Circuito"], "C1");
    }
}

// ---------- IFC ----------

fn ifc(s: &mut Session) -> String {
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-{}.ifc", std::process::id(), s.model.uid));
    run(s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    text
}

fn entities<'a>(file: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("={name}(");
    file.lines().filter(|l| l.contains(&head)).collect()
}

#[test]
fn a_run_of_the_mep_is_written_as_its_ifc_class_swept_along_its_axis_in_a_system() {
    for c in CASES {
        let mut s = Session::new();
        made(&mut s, c.command, json!({"points": [[0, 0], [3000, 0]], "offset": 0, "end_offset": 4000, "system": "Cold water"}));
        made(&mut s, c.command, json!({"points": [[0, 1000], [3000, 1000]], "system": "Cold water"}));
        made(&mut s, c.command, json!({"points": [[0, 2000], [3000, 2000]], "system": "Esgoto"}));
        let file = ifc(&mut s);
        let class = entities(&file, c.class);
        assert_eq!(class.len(), 3, "{}: {file}", c.category);
        assert!(class[0].contains(&format!("'{}'", c.default_type)) && class[0].ends_with(&format!("{});", c.predefined)), "{}", class[0]);
        let profile = if c.round { "IFCCIRCLEPROFILEDEF" } else { "IFCRECTANGLEPROFILEDEF" };
        assert_eq!(entities(&file, profile).len(), 3, "{}", c.category);
        // The sloped one is aimed along (3, 0, 4) / 5 and is 5000 long.
        assert!(
            file.contains("IFCDIRECTION((0.6,0.,0.8))") && entities(&file, "IFCEXTRUDEDAREASOLID")[0].ends_with(",5000.);"),
            "{}",
            c.category
        );
        // A system is an IfcSystem the runs in it are assigned to: two systems, three runs.
        let systems = entities(&file, "IFCSYSTEM");
        assert_eq!(systems.len(), 2, "{}", c.category);
        assert!(systems.iter().any(|l| l.contains("'Cold water'")) && systems.iter().any(|l| l.contains("'Esgoto'")));
        let assigns = entities(&file, "IFCRELASSIGNSTOGROUP");
        assert_eq!(assigns.len(), 2, "{}", c.category);
        assert!(assigns.iter().any(|l| l.matches('#').count() == 4), "two runs and the system, in one relationship: {assigns:?}");
        // Nothing in a system when it names none.
        made(&mut s, c.command, json!({"points": [[0, 3000], [3000, 3000]]}));
        assert_eq!(entities(&ifc(&mut s), "IFCSYSTEM").len(), 2);
    }
}

#[test]
fn cable_carriers_write_their_section_kind_and_both_read_as_one_category() {
    let mut s = Session::new();
    let round = made(&mut s, "cable_carrier", json!({"points": [[0, 0], [3000, 0]], "type": "PVC 25"}));
    let tray = made(&mut s, "cable_carrier", json!({"points": [[0, 1000], [3000, 1000]], "type": "Tray 100 x 50"}));
    let file = ifc(&mut s);
    let carriers = entities(&file, "IFCCABLECARRIERSEGMENT");
    assert_eq!(carriers.len(), 2);
    assert!(carriers.iter().any(|e| e.ends_with(".CONDUITSEGMENT.);")));
    assert!(carriers.iter().any(|e| e.ends_with(".CABLETRAYSEGMENT.);")));
    let imported = crate::import::ifc(file.as_bytes(), "Carriers").unwrap();
    assert_eq!(imported.model.check(), Ok(()));
    for id in [round, tray] {
        let e = imported.model.elements.get(s.model.guid(id)).unwrap();
        assert_eq!(e.category(), "cablecarrier");
        assert_eq!(imported.model.run_geom(e).unwrap().round, id == round);
    }
}

// ---------- IFC, back in ----------

/// What a run is and where, to the tenth of a millimetre, whatever its id: its section, its system, where its justify line is
/// at each end, and which way it is drawn left to the file's say (a run comes back running east, or north, or up).
fn described(m: &crate::model::Model, category: &str) -> Vec<String> {
    let tenths = |v: &Value| format!("{:.1}", v.as_f64().unwrap_or(f64::NAN) + 0.0);
    let mut out: Vec<String> = m
        .elements
        .iter()
        .filter(|(_, e)| e.category() == category)
        .map(|(id, _)| {
            let e = element_json(m, *id);
            let end = |k: &str, h: &Value| format!("{},{},{}", tenths(&e[k][0]), tenths(&e[k][1]), tenths(h));
            let (z0, z1) = (e["offset"].clone(), if e["end_offset"].is_null() { e["offset"].clone() } else { e["end_offset"].clone() });
            let mut ends = [end("start", &z0), end("end", &z1)];
            ends.sort();
            format!(
                "{}x{} round={} length={} justify={} system={} ends={}->{}",
                tenths(&e["width"]),
                tenths(&e["depth"]),
                e["round"],
                tenths(&e["length"]),
                e["justify"],
                e["system"],
                ends[0],
                ends[1]
            )
        })
        .collect();
    out.sort();
    out
}

fn the_mep(s: &mut Session) {
    for c in CASES {
        let cmd = c.command;
        run(s, cmd, json!({"points": [[0, 0], [6000, 0]], "offset": 2400, "system": "Cold water"}));
        run(s, cmd, json!({"points": [[0, 3000], [4000, 3000]], "offset": 2400, "end_offset": 2340, "justify": "top", "system": "Sewer"}));
        run(s, cmd, json!({"points": [[500, 5000], [3500, 4000]], "offset": 1000, "end_offset": 1900, "justify": "bottom"}));
        // Drawn west, and running north-north-west, whose direction the body's points alone cannot sign.
        run(s, cmd, json!({"points": [[6000, 6500], [0, 6500]], "offset": 2000, "system": "Cold water"}));
        run(s, cmd, json!({"points": [[7500, 500], [6500, 4500]], "offset": 2000, "justify": "top"}));
        // A riser: upright, its heights from end to end; and one drawn down.
        run(s, cmd, json!({"points": [[8000, 1000], [8000, 1000]], "offset": 0, "end_offset": 3000, "system": "Sewer"}));
        run(s, cmd, json!({"points": [[9000, 1000], [9000, 1000]], "offset": 3000, "end_offset": 500}));
        run(s, "type", json!({"category": c.category, "name": "Mine 75", "width": 75, "depth": 75, "round": c.round, "material": "Metal"}));
        run(s, cmd, json!({"points": [[0, 8000], [3000, 8000]], "type": "Mine 75"}));
    }
}

#[test]
fn the_runs_of_the_mep_come_back_from_ifc_with_their_justification_and_their_system() {
    let mut s = Session::new();
    the_mep(&mut s);
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-mep-round.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]), "{r}");
    for c in CASES {
        let (before, after) = (described(&s.model, c.category), described(&t.model, c.category));
        assert_eq!(before.len(), 8, "{}", c.category);
        assert_eq!(after, before, "{}", c.category);
        // The type they were of came with them, in its material.
        let mine = &t.model.types.runs[c.category]["Mine 75"];
        assert_eq!((mine.width, mine.round, mine.material.as_str()), (75.0, c.round, "Metal"), "{}", c.category);
    }
    assert!(t.model.check().is_ok());
}

#[test]
fn a_system_an_ifc_file_names_comes_back_on_its_runs_and_a_class_alone_says_the_category() {
    for c in CASES.iter().filter(|c| c.alone) {
        let mut s = Session::new();
        made(&mut s, c.command, json!({"points": [[0, 0], [3000, 0]], "system": "Cold water"}));
        let path = std::env::temp_dir().join(format!("bimcraft-test-{}-mep-system-{}.ifc", std::process::id(), c.category));
        run(&mut s, "save", json!({"path": path}));
        let text = std::fs::read_to_string(&path).unwrap();
        // Another tool's file: the run and its system, with BIMCraft's own property set taken out.
        let other: String = text.lines().filter(|l| !l.contains("BIMCraft_Run")).collect::<Vec<_>>().join("\n");
        std::fs::write(&path, other).unwrap();
        let mut t = Session::new();
        run(&mut t, "open", json!({"path": path}));
        let _ = std::fs::remove_file(&path);
        let id = t.model.elements.iter().find(|(_, e)| e.category() == c.category).map(|(id, _)| *id);
        let e = element_json(&t.model, id.unwrap_or_else(|| panic!("a {} came back", c.category)));
        assert_eq!((e["system"].as_str(), e["justify"].as_str()), (Some("Cold water"), Some("centre")), "{}", c.category);
    }
}

// ---------- fixtures and devices ----------

use crate::family::Category;
use crate::language::Language;

/// A starter family of a tab (in English), its category, its IFC class and predefined type, and where its forms are over its base
/// (mm: the range its lowest point may be in, and its highest), so a fixture's drawing is checked against what it is.
struct Fixture {
    family: &'static str,
    category: Category,
    class: &'static str,
    predefined: &'static str,
    /// The object type a proxy carries, if the class has no home for it.
    object_type: Option<&'static str>,
    base: (f64, f64),
    top: (f64, f64),
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        family: "WC",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (700.0, 900.0),
    },
    Fixture {
        family: "Washbasin",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (800.0, 900.0),
    },
    Fixture {
        family: "Kitchen Sink",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (850.0, 1300.0),
    },
    Fixture {
        family: "Shower",
        category: Category::Plumbing,
        class: "IFCSANITARYTERMINAL",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (0.0, 0.0),
        top: (2000.0, 2200.0),
    },
    Fixture {
        family: "Floor Drain",
        category: Category::Drain,
        class: "IFCWASTETERMINAL",
        predefined: ".FLOORTRAP.",
        object_type: None,
        base: (-100.0, 0.0),
        top: (0.0, 0.0),
    },
    Fixture {
        family: "Elbow 90",
        category: Category::PipeFitting,
        class: "IFCPIPEFITTING",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (-100.0, -10.0),
        top: (10.0, 100.0),
    },
    Fixture {
        family: "Tee",
        category: Category::PipeFitting,
        class: "IFCPIPEFITTING",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (-100.0, -10.0),
        top: (10.0, 100.0),
    },
    Fixture {
        family: "Gate Valve",
        category: Category::PipeAccessory,
        class: "IFCVALVE",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (-100.0, -10.0),
        top: (40.0, 300.0),
    },
    Fixture {
        family: "Distribution Board",
        category: Category::Equipment,
        class: "IFCELECTRICDISTRIBUTIONBOARD",
        predefined: ".DISTRIBUTIONBOARD.",
        object_type: None,
        base: (1200.0, 1400.0),
        top: (1700.0, 2300.0),
    },
    Fixture {
        family: "Ceiling Luminaire",
        category: Category::Lighting,
        class: "IFCLIGHTFIXTURE",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (2900.0, 3000.0),
        top: (2950.0, 3050.0),
    },
    Fixture {
        family: "Recessed Luminaire",
        category: Category::Lighting,
        class: "IFCLIGHTFIXTURE",
        predefined: ".NOTDEFINED.",
        object_type: None,
        base: (2900.0, 3000.0),
        top: (2950.0, 3050.0),
    },
    Fixture {
        family: "Outlet",
        category: Category::Outlet,
        class: "IFCOUTLET",
        predefined: ".POWEROUTLET.",
        object_type: None,
        base: (200.0, 2100.0),
        top: (300.0, 2200.0),
    },
    Fixture {
        family: "Switch",
        category: Category::Switch,
        class: "IFCSWITCHINGDEVICE",
        predefined: ".TOGGLESWITCH.",
        object_type: None,
        base: (200.0, 1200.0),
        top: (300.0, 1300.0),
    },
    Fixture {
        family: "Air Terminal",
        category: Category::AirTerminal,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("LightningTerminal"),
        base: (-1.0, 1.0),
        top: (400.0, 2100.0),
    },
    Fixture {
        family: "Earthing Electrode",
        category: Category::Earthing,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("EarthingRod"),
        base: (-3100.0, -2300.0),
        top: (0.0, 0.0),
    },
    Fixture {
        family: "Inspection Box",
        category: Category::InspectionBox,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("InspectionBox"),
        base: (-350.0, -200.0),
        top: (0.0, 0.0),
    },
    Fixture {
        family: "Data Outlet",
        category: Category::DataOutlet,
        class: "IFCOUTLET",
        predefined: ".DATAOUTLET.",
        object_type: None,
        base: (200.0, 1200.0),
        top: (300.0, 1300.0),
    },
    Fixture {
        family: "Voice Outlet",
        category: Category::VoiceOutlet,
        class: "IFCOUTLET",
        predefined: ".TELEPHONEOUTLET.",
        object_type: None,
        base: (200.0, 1200.0),
        top: (300.0, 1300.0),
    },
    Fixture {
        family: "Patch Panel",
        category: Category::PatchPanel,
        class: "IFCCOMMUNICATIONSAPPLIANCE",
        predefined: ".NETWORKAPPLIANCE.",
        object_type: None,
        base: (1400.0, 1600.0),
        top: (1500.0, 1700.0),
    },
    Fixture {
        family: "Rack",
        category: Category::Rack,
        class: "IFCBUILDINGELEMENTPROXY",
        predefined: ".USERDEFINED.",
        object_type: Some("Rack"),
        base: (-1.0, 1.0),
        top: (1900.0, 2100.0),
    },
    Fixture {
        family: "Fire Alarm Panel",
        category: Category::AlarmPanel,
        class: "IFCUNITARYCONTROLELEMENT",
        predefined: ".ALARMPANEL.",
        object_type: None,
        base: (1499.0, 1501.0),
        top: (1900.0, 2100.0),
    },
    Fixture {
        family: "Smoke Detector",
        category: Category::SmokeDetector,
        class: "IFCSENSOR",
        predefined: ".SMOKESENSOR.",
        object_type: None,
        base: (2900.0, 3000.0),
        top: (3000.0, 3000.0),
    },
    Fixture {
        family: "Heat Detector",
        category: Category::HeatDetector,
        class: "IFCSENSOR",
        predefined: ".HEATSENSOR.",
        object_type: None,
        base: (2900.0, 3000.0),
        top: (3000.0, 3000.0),
    },
    Fixture {
        family: "Manual Call Point",
        category: Category::CallPoint,
        class: "IFCALARM",
        predefined: ".BREAKGLASSBUTTON.",
        object_type: None,
        base: (1400.0, 1500.0),
        top: (1500.0, 1600.0),
    },
    Fixture {
        family: "Sounder",
        category: Category::Sounder,
        class: "IFCALARM",
        predefined: ".SIREN.",
        object_type: None,
        base: (2200.0, 2300.0),
        top: (2300.0, 2400.0),
    },
    Fixture {
        family: "Exit Sign",
        category: Category::ExitSign,
        class: "IFCALARM",
        predefined: ".LIGHT.",
        object_type: None,
        base: (2200.0, 2300.0),
        top: (2300.0, 2400.0),
    },
    Fixture {
        family: "Ceiling Speaker",
        category: Category::Speaker,
        class: "IFCAUDIOVISUALAPPLIANCE",
        predefined: ".SPEAKER.",
        object_type: None,
        base: (2900.0, 3000.0),
        top: (3000.0, 3000.0),
    },
    Fixture {
        family: "Motion Detector",
        category: Category::MotionDetector,
        class: "IFCSENSOR",
        predefined: ".MOVEMENTSENSOR.",
        object_type: None,
        base: (2200.0, 2300.0),
        top: (2300.0, 2400.0),
    },
    Fixture {
        family: "Camera",
        category: Category::Camera,
        class: "IFCAUDIOVISUALAPPLIANCE",
        predefined: ".CAMERA.",
        object_type: None,
        base: (2550.0, 2700.0),
        top: (2700.0, 2800.0),
    },
];

#[test]
fn a_new_project_has_the_starter_fixtures_of_its_tabs_in_every_language_and_each_one_builds() {
    for l in Language::ALL {
        let m = crate::model::Model::new_in("x", l);
        assert_eq!(m.check(), Ok(()), "{l:?}: every family builds, every type of it");
        for f in FIXTURES {
            let name = l.word(f.family);
            let family = m.families.get(name).unwrap_or_else(|| panic!("{l:?}: no family `{name}`"));
            assert_eq!(family.category, f.category, "{l:?} {name}");
            assert!(!family.types.is_empty(), "{l:?} {name}");
        }
    }
}

#[test]
fn a_starter_fixture_stands_as_it_is_and_every_type_of_it_has_a_body() {
    let mut s = Session::new();
    for f in FIXTURES {
        let types: Vec<String> = s.model.families[f.family].types.keys().cloned().collect();
        for ty in types {
            let id = made(&mut s, "component", json!({"family": f.family, "type": ty, "at": [1000, 1000]}));
            let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("{} is a component", f.family) };
            let pieces = s.model.component_pieces(c);
            assert!(!pieces.is_empty(), "{} {ty}", f.family);
            let top = pieces.iter().map(|p| p.top).fold(f64::MIN, f64::max);
            let bottom = pieces.iter().map(|p| p.bottom).fold(f64::MAX, f64::min);
            assert!(top >= f.top.0 - 1e-6 && top <= f.top.1 + 1e-6, "{} {ty}: reaches {top}", f.family);
            assert!(bottom >= f.base.0 - 1e-6 && bottom <= f.base.1 + 1e-6, "{} {ty}: starts at {bottom}", f.family);
        }
    }
}

#[test]
fn a_category_of_family_is_written_as_its_ifc_class_and_read_back_as_the_same_category() {
    for f in FIXTURES {
        assert_eq!(f.category.ifc(), (f.class, f.predefined, f.object_type), "{}", f.family);
        assert_eq!(Category::from_ifc(f.class, Some(f.predefined.trim_matches('.')), f.object_type), Some(f.category), "{}", f.family);
    }
    // A class BIMCraft makes no family of, or a proxy that says nothing, is the generic one's, or none.
    assert_eq!(Category::from_ifc("IFCBEAM", None, None), None);
    assert_eq!(Category::from_ifc("IFCBUILDINGELEMENTPROXY", None, None), Some(Category::Generic));
    assert_eq!(Category::from_ifc("IFCFURNISHINGELEMENT", None, None), Some(Category::Furniture));
    // An outlet of no type, or of one BIMCraft has no category for, is the power outlet, the class's first; a type it has is its own.
    assert_eq!(Category::from_ifc("IFCOUTLET", None, None), Some(Category::Outlet));
    assert_eq!(Category::from_ifc("IFCOUTLET", Some("NOTDEFINED"), None), Some(Category::Outlet));
    assert_eq!(Category::from_ifc("IFCOUTLET", Some("TELEPHONEOUTLET"), None), Some(Category::VoiceOutlet));
    // A sensor of no type is a smoke detector, the class's first.
    assert_eq!(Category::from_ifc("IFCSENSOR", Some("NOTDEFINED"), None), Some(Category::SmokeDetector));
    // A proxy of an object type no category names is the generic one's.
    assert_eq!(Category::from_ifc("IFCBUILDINGELEMENTPROXY", Some("USERDEFINED"), Some("Sculpture")), Some(Category::Generic));
}

#[test]
fn fixtures_are_ifc_products_of_their_class_and_come_back_in_a_family_of_their_category() {
    let mut s = Session::new();
    for f in FIXTURES {
        let ty = s.model.families[f.family].types.keys().next().cloned().unwrap();
        made(&mut s, "component", json!({"family": f.family, "type": ty, "at": [1000, 1000]}));
    }
    let file = ifc(&mut s);
    for f in FIXTURES {
        let written: Vec<&str> = entities(&file, f.class).into_iter().filter(|l| l.contains(&format!("'{}:", f.family))).collect();
        assert_eq!(written.len(), 1, "{}: {file}", f.family);
        assert!(written[0].ends_with(&format!("{});", f.predefined)), "{}: {}", f.family, written[0]);
        if let Some(object) = f.object_type {
            assert!(written[0].contains(&format!(",'{object}',")), "{}: its object type says what it is: {}", f.family, written[0]);
        }
    }
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-fixtures.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]), "{r}");
    for f in FIXTURES {
        let family = t.model.families.get(f.family).unwrap_or_else(|| panic!("{} came back", f.family));
        assert_eq!(family.category, f.category, "{}", f.family);
    }
    assert_eq!(t.model.elements.values().filter(|e| e.category() == "component").count(), FIXTURES.len());
    // Each in the family it was of: the library's of the same name would stand in for one the import named wrongly.
    let named: std::collections::BTreeSet<String> = t
        .model
        .elements
        .iter()
        .filter(|(_, e)| e.category() == "component")
        .filter_map(|(id, _)| element_json(&t.model, *id)["family"].as_str().map(String::from))
        .collect();
    let wanted: std::collections::BTreeSet<String> = FIXTURES.iter().map(|f| f.family.to_string()).collect();
    assert_eq!(named, wanted);
}

#[test]
fn an_upright_run_comes_back_running_up_whichever_end_it_was_drawn_from() {
    for c in CASES {
        let mut s = Session::new();
        // Drawn downward: from 3000 to 500 over its level.
        made(&mut s, c.command, json!({"points": [[2000, 2000], [2000, 2000]], "offset": 3000, "end_offset": 500}));
        let path = std::env::temp_dir().join(format!("bimcraft-test-{}-riser-{}.ifc", std::process::id(), c.category));
        run(&mut s, "save", json!({"path": path}));
        let mut t = Session::new();
        run(&mut t, "open", json!({"path": path}));
        let _ = std::fs::remove_file(&path);
        let id = t.model.elements.iter().find(|(_, e)| e.category() == c.category).map(|(id, _)| *id).unwrap();
        let e = element_json(&t.model, id);
        assert_eq!((e["offset"].as_f64(), e["end_offset"].as_f64()), (Some(500.0), Some(3000.0)), "{}: {e}", c.category);
    }
}

#[test]
fn stage4_bend_radius_rejects_short_carrier_entry_atomically() {
    let mut s = Session::new();
    let wall = made(&mut s, "wall", json!({"points":[[0,0],[6000,0]],"height":6000}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[1000,-100]}));
    run(&mut s, "modify", json!({"id":device,"connection_sides":["top"]}));
    let carrier = made(&mut s, "cable_carrier", json!({"points":[[500,-300],[1500,-300]],"offset":3000,"type":"Tray 100 x 50"}));
    run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
    let wire = made(&mut s, "wire", json!({"points":[[500,-102],[1500,-102]],"offset":3000}));
    let mut params = json!({"wire":wire,"device":device,"carrier":carrier,"surface":{"host":wall,"face":"interior"},"min_bend_radius":100});
    run(&mut s, "wire_connect", params.clone());
    let Some(Element::Wire(main)) = s.model.elements.get(wire) else { panic!("expected main wire") };
    let connection = &main.connections[0];
    let mut saved = serde_json::to_value(connection).unwrap();
    assert_eq!(saved["min_bend_radius"], json!(100.0));
    saved.as_object_mut().unwrap().remove("min_bend_radius");
    assert_eq!(serde_json::from_value::<crate::model::WireConnection>(saved).unwrap().min_bend_radius, None);
    let model = s.model.clone();
    let history = s.history();
    let moved = s.execute("modify", &json!({"id":carrier,"offset":1950,"end_offset":1950}));
    assert!(moved.unwrap_err().contains("room for a 100 mm cable bend"));
    assert_eq!(s.model, model);
    assert_eq!(s.history(), history);
    params["min_bend_radius"] = json!(2000);
    assert!(s.execute("wire_connect", &params).unwrap_err().contains("room for a 2000 mm cable bend"));
    assert_eq!(s.model, model);
    assert_eq!(s.history(), history);
}

#[test]
fn stage41_split_route_can_place_a_bend_in_one_edit() {
    for (command, category) in [("wire", "wire"), ("cable_carrier", "cablecarrier")] {
        let mut s = Session::new();
        let original = run(&mut s, command, json!({"points":[[0,0],[4000,0]],"offset":1000,"end_offset":2000,"justify":"centre"}))["ids"]
            [0]
        .as_u64()
        .unwrap();
        let guid = s.model.guid(original);
        let result = run(&mut s, "split", json!({"id":original,"at":[2000,0],"to":[2000,1000],"dz":250}));
        let second = result["ids"][1].as_u64().unwrap();
        assert_eq!(result["ids"][0], original);
        assert_eq!(s.model.guid(original), guid);
        assert_ne!(s.model.guid(second), guid);
        let geometry = |id| {
            let e = s.model.elements.get(id).unwrap();
            assert_eq!(e.category(), category);
            s.model.run_geom(e).unwrap()
        };
        assert_eq!(geometry(original).a, [0.0, 0.0, 1000.0]);
        assert_eq!(geometry(original).b, [2000.0, 1000.0, 1750.0]);
        assert_eq!(geometry(second).a, geometry(original).b);
        assert_eq!(geometry(second).b, [4000.0, 0.0, 2000.0]);
        let saved = s.model.to_json().unwrap();
        let (opened, left_out) = crate::model::Model::read(&saved).unwrap();
        assert!(left_out.is_empty());
        assert_eq!(opened.guid(original), guid);
        assert_eq!(opened.guid(second), s.model.guid(second));
        assert_eq!(opened.to_json().unwrap(), saved);
    }
}

#[test]
fn stage41_split_vertical_route_at_fraction() {
    let mut s = Session::new();
    let original = made(&mut s, "wire", json!({"points":[[0,0],[0,0]],"offset":1000,"end_offset":3000,"justify":"centre"}));
    let result = run(&mut s, "split", json!({"id":original,"fraction":0.25,"to":[500,0]}));
    let first = s.model.run_geom(s.model.elements.get(original).unwrap()).unwrap();
    let second = s.model.run_geom(s.model.elements.get(ids(&result)[1]).unwrap()).unwrap();
    assert_eq!(first.b, [500.0, 0.0, 1500.0]);
    assert_eq!(second.a, first.b);
    assert_eq!(second.b, [0.0, 0.0, 3000.0]);
}

#[test]
fn stage41_move_carrier_can_disconnect_equipment_atomically() {
    let mut s = Session::new();
    let wire = made(&mut s, "wire", json!({"points":[[0,0],[3000,0]],"offset":3000}));
    let carrier = made(&mut s, "cable_carrier", json!({"from_wire":wire,"type":"Tray 100 x 50"}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","at":[0,0],"offset":1000}));
    run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"point":[0,0,0],"in_carrier":true}));
    let before = s.model.clone();
    let old_height = s.model.run_geom(s.model.elements.get(wire).unwrap()).unwrap().a[2];
    run(&mut s, "move", json!({"ids":[carrier],"by":[0,100],"dz":200,"disconnect_equipment":true}));
    let e = s.model.elements.get(wire).unwrap();
    assert!(e.run().unwrap().connections.is_empty());
    assert_eq!(e.run().unwrap().carrier, Some(s.model.guid(carrier)));
    assert_eq!(s.model.run_geom(e).unwrap().a[1], 100.0);
    assert_eq!(s.model.run_geom(e).unwrap().a[2], old_height + 200.0);
    assert_eq!(s.model.elements.get(device), before.elements.get(device));
    let after = s.model.clone();
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model, before);
    run(&mut s, "redo", json!({}));
    assert_eq!(s.model, after);
}

#[test]
fn stage41_free_lead_has_a_vertical_transition() {
    for (side, height, contact_height) in [("top", 3000, 1900.0), ("bottom", 800, 1500.0)] {
        let (mut s, wall, device, _, _) = free_exit_fixture();
        run(&mut s, "modify", json!({"id":device,"connection_sides":[side]}));
        let wire = made(&mut s, "wire", json!({"points":[[2000,-102],[4000,-102]],"offset":height}));
        let result = run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"surface":{"host":wall,"face":"interior"}}));
        let feeders = ids(&result);
        assert_eq!(feeders.len(), 2);
        let vertical = s.model.run_geom(s.model.elements.get(feeders[0]).unwrap()).unwrap();
        let horizontal = s.model.run_geom(s.model.elements.get(feeders[1]).unwrap()).unwrap();
        assert_eq!(vertical.a, [1150.0, -102.0, contact_height]);
        assert_eq!(vertical.b, [1150.0, -102.0, f64::from(height)]);
        assert_eq!(horizontal.a, vertical.b);
        assert_eq!(horizontal.b, [2000.0, -102.0, f64::from(height)]);
        let before = s.model.to_json().unwrap();
        let error = s
            .execute("wire_connect", &json!({"wire":wire,"device":device,"surface":{"host":wall,"face":"interior"},"min_bend_radius":1000}))
            .unwrap_err();
        assert!(error.to_string().contains("vertical transition"));
        assert_eq!(s.model.to_json().unwrap(), before);
        let carrier = made(&mut s, "cable_carrier", json!({"points":[[2000,-300],[4000,-300]],"offset":height,"type":"Tray 100 x 50"}));
        run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
        let result = run(
            &mut s,
            "wire_connect",
            json!({"wire":wire,"device":device,"carrier":carrier,"in_carrier":true,
                "surface":{"host":wall,"face":"interior"},"min_bend_radius":100}),
        );
        let branches = result["carriers"].as_array().unwrap();
        assert_eq!(branches.len(), 2);
        let riser = s.model.run_geom(s.model.elements.get(branches[0].as_u64().unwrap()).unwrap()).unwrap();
        assert_eq!(riser.a[0..2], riser.b[0..2]);
        assert_eq!(riser.a[2], contact_height);
        assert_eq!(riser.b[2], f64::from(height));
        let saved = s.model.to_json().unwrap();
        let (opened, left_out) = crate::model::Model::read(&saved).unwrap();
        assert!(left_out.is_empty());
        assert_eq!(opened.to_json().unwrap(), saved);
    }
    let (mut s, wall, device, _, _) = free_exit_fixture();
    run(&mut s, "modify", json!({"id":device,"connection_sides":["right"]}));
    let wire = made(&mut s, "wire", json!({"points":[[2000,-102],[4000,-102]],"offset":1700}));
    let feeders = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"surface":{"host":wall,"face":"interior"}})));
    assert_eq!(feeders.len(), 1);
    let lead = s.model.run_geom(s.model.elements.get(feeders[0]).unwrap()).unwrap();
    assert_eq!(lead.a[2], lead.b[2]);
}

#[test]
fn stage41_split_wire_keeps_carrier_fill_and_links() {
    let mut s = Session::new();
    let carrier = made(&mut s, "cable_carrier", json!({"points":[[1000,1000],[4000,1000]],"type":"Tray 100 x 50"}));
    let axis = s.model.run_geom(s.model.elements.get(carrier).unwrap()).unwrap();
    let wire = made(&mut s, "wire", json!({"points":[axis.a[..2],axis.b[..2]],"offset":axis.a[2],"carrier":carrier}));
    let guid = s.model.guid(carrier);
    let before = s.model.to_json().unwrap();
    let fill = s.model.carrier_sections()[&guid];
    let parts = ids(&run(&mut s, "split", json!({"id":wire,"fraction":0.5})));
    for id in parts {
        assert_eq!(s.model.elements.get(id).unwrap().run().unwrap().carrier, Some(guid));
    }
    assert_eq!(s.model.carrier_sections()[&guid], fill);
    let after = s.model.to_json().unwrap();
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "redo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), after);
}

#[test]
fn stage41_split_carrier_reassigns_equipment_by_landing() {
    let (mut s, wall, device, _, wire) = free_exit_fixture();
    let carrier =
        made(&mut s, "cable_carrier", json!({"points":[[1040,-300],[1040,-300]],"offset":2000,"end_offset":5000,"type":"Tray 100 x 50"}));
    run(&mut s, "run_mount", json!({"id":carrier,"host":wall,"face":"interior"}));
    run(&mut s, "modify", json!({"id":device,"connection_sides":["bottom"]}));
    run(&mut s, "move", json!({"ids":[device],"by":[0,0],"dz":3800}));
    let feeder =
        ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"carrier":carrier,"surface":{"host":wall,"face":"interior"}})))
            [0];
    let before = s.model.to_json().unwrap();
    let second = ids(&run(&mut s, "split", json!({"id":carrier,"fraction":0.5})))[1];
    assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().connections[0].carrier, Some(s.model.guid(second)));
    assert!(s.model.elements.get(feeder).is_some());
    let after = s.model.to_json().unwrap();
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), before);
    run(&mut s, "redo", json!({}));
    assert_eq!(s.model.to_json().unwrap(), after);
}

#[test]
fn stage41_carrier_bend_carries_the_wire_and_recomputes_fill() {
    let mut s = Session::new();
    let carrier = made(&mut s, "cable_carrier", json!({"points":[[1000,1000],[5000,1000]],"type":"Tray 100 x 50","justify":"centre"}));
    let wire = made(&mut s, "wire", json!({"points":[[1500,1000],[4500,1000]],"carrier":carrier}));
    let device = made(&mut s, "component", json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[4300,2500]}));
    let feeders = ids(&run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"point":[0,0,0]})));
    let fill = s.model.carrier_sections()[&s.model.guid(carrier)];
    let parts = ids(&run(&mut s, "split", json!({"id":carrier,"fraction":0.5,"to":[3000,2000]})));
    let cables: Vec<_> =
        s.model.elements.iter().filter(|(_, element)| matches!(element, Element::Wire(run) if run.feeder.is_none())).collect();
    assert_eq!(cables.len(), 2);
    assert_eq!(*cables[0].0, wire);
    for ((_, cable), carrier) in cables.iter().zip(parts) {
        let guid = s.model.guid(carrier);
        assert_eq!(cable.run().unwrap().carrier, Some(guid));
        assert_eq!(s.model.carrier_sections()[&guid], fill);
    }
    let a = s.model.run_geom(cables[0].1).unwrap();
    let b = s.model.run_geom(cables[1].1).unwrap();
    assert_eq!(a.b, b.a);
    assert_eq!(a.b[..2], [3000.0, 2000.0]);
    let owner = s.model.guid(*cables[1].0);
    for id in feeders {
        assert_eq!(s.model.elements.get(id).unwrap().run().unwrap().feeder.as_ref().unwrap().wire, owner);
    }
}

#[test]
fn stage41_route_vertex_rejects_a_bend_without_room() {
    let (mut s, wall, device, _, _) = free_exit_fixture();
    run(&mut s, "modify", json!({"id":device,"connection_sides":["top"]}));
    let wire = made(&mut s, "wire", json!({"points":[[500,-102],[1500,-102]],"offset":3000}));
    run(&mut s, "wire_connect", json!({"wire":wire,"device":device,"surface":{"host":wall,"face":"interior"},"min_bend_radius":100}));
    let before = s.model.to_json().unwrap();
    let result = s.execute("split", &json!({"id":wire,"fraction":0.5,"to":[2000,-101]}));
    assert!(result.unwrap_err().contains("route vertex does not have room"));
    assert_eq!(s.model.to_json().unwrap(), before);
}
