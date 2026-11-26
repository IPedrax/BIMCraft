//! Framing members edited one by one (`framing/edit.rs`), the framing types a project starts with, the structure layer a type
//! marks for its members, and the framing kept with the model: driven through commands, as a front end drives them.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn first(v: &Value) -> u64 {
    v["ids"][0].as_u64().or(v["id"].as_u64()).unwrap_or_else(|| panic!("no id in {v}"))
}

fn members(s: &mut Session, id: u64) -> Vec<Value> {
    run(s, "framing", json!({"id": id}))["members"].as_array().cloned().unwrap_or_default()
}

fn keyed<'a>(ms: &'a [Value], key: &str) -> Option<&'a Value> {
    ms.iter().find(|m| m["key"] == key)
}

fn f(v: &Value, i: usize) -> f64 {
    v[i].as_f64().unwrap_or(f64::NAN)
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 0.02, "{what}: {a} isn't {b}");
}

/// A 6 m wall of the starter type `wall_type`, 2700 tall, framed with `framing`, a 1000 x 1200 window at its middle.
fn framed(s: &mut Session, wall_type: &str, framing: &str) -> (u64, u64) {
    let w = first(&run(s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": wall_type, "height": 2700})));
    run(s, "type", json!({"category": "window", "name": "1000 x 1200", "width": 1000, "height": 1200, "sill": 900}));
    let win = first(&run(s, "window", json!({"wall": w, "offset": 3000, "type": "1000 x 1200"})));
    run(s, "frame", json!({"id": w, "type": framing}));
    (w, win)
}

#[test]
fn a_new_project_has_north_american_and_european_framing_types_of_pine_and_timber_frame_walls_for_them() {
    let s = Session::new();
    let types = &s.model.types.framing;
    let (us, eu) = (&types[crate::framing::DEFAULT_TYPE], &types[crate::framing::EUROPEAN_TYPE]);
    assert!(s.model.materials.contains_key("Pine"));
    assert_eq!(us.material.as_deref(), Some("Pine"));
    assert_eq!(eu.material.as_deref(), Some("Pine"));
    close(us.spacing, 406.4, "2x4s 16 inches on centre");
    close(us.thickness, 38.0, "a 2x is 1 1/2 inches");
    close(eu.spacing, 625.0, "KVH studs at 62.5 cm");
    close(eu.thickness, 60.0, "KVH 60 mm");
    assert_eq!((eu.top_plates, eu.stock.clone()), (1, vec![5000.0, 13000.0]));
    for (name, depth) in [("Timber Frame - 114mm", 89.0), ("Timber Frame - 185mm", 160.0)] {
        let layers = s.model.type_layers("wall", name);
        let k = crate::framing::framed_layer(layers).unwrap_or_else(|| panic!("{name}: no framed layer"));
        assert!(layers[k].framed && layers[k].material == "Pine");
        close(layers[k].thickness, depth, name);
    }
}

#[test]
fn a_european_wall_is_framed_in_kvh_60x160_at_625_of_pine() {
    let mut s = Session::new();
    let (w, _) = framed(&mut s, "Timber Frame - 185mm", crate::framing::EUROPEAN_TYPE);
    let reply = run(&mut s, "framing", json!({"id": w}));
    assert_eq!(reply["material"], "Pine");
    let ms = reply["members"].as_array().cloned().unwrap_or_default();
    let grid: Vec<&Value> = ms.iter().filter(|m| m["role"] == "stud" && m["key"].as_str().is_some_and(|k| k.contains("stud/"))).collect();
    assert!(grid.len() >= 6, "{} studs", grid.len());
    for m in &grid {
        assert_eq!(m["section"], json!([60.0, 160.0]), "{m}");
    }
    // One top plate: the type's single Rähm.
    assert_eq!(ms.iter().filter(|m| m["role"] == "top_plate").count(), 1);
    // The grid at 625 from its start.
    let s1 = grid.iter().map(|m| (f(&m["s"], 0) + f(&m["s"], 1)) / 2.0).filter(|c| *c > 100.0).fold(f64::INFINITY, f64::min);
    close(s1, 625.0, "the first grid stud");
}

#[test]
fn a_type_with_two_structure_layers_frames_the_one_it_marks_and_only_one_structure_layer_can_be_marked() {
    let mut s = Session::new();
    let layers = |framed: [bool; 3]| {
        json!([
            {"function": "structure", "material": "Concrete", "thickness": 150, "framed": framed[0]},
            {"function": "thermal", "material": "Insulation", "thickness": 50, "framed": framed[1]},
            {"function": "structure", "material": "Pine", "thickness": 89, "framed": framed[2]}
        ])
    };
    run(&mut s, "type", json!({"category": "wall", "name": "Two cores", "layers": layers([false, false, true])}));
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Two cores", "height": 2700})));
    run(&mut s, "frame", json!({"id": w}));
    // The interior layer, 89 deep, not the thicker concrete: across the wall from the interior face (-144.5) 89 mm in.
    let ms = members(&mut s, w);
    let stud = ms.iter().find(|m| m["role"] == "stud").unwrap_or_else(|| panic!("no stud"));
    close(f(&stud["across"], 1) - f(&stud["across"], 0), 89.0, "the stud is as deep as the marked layer");
    close(f(&stud["across"], 0), -144.5, "it lies at the interior face");
    // Two marked, or a thermal layer marked: refused, saying what to do.
    for (bad, what) in [([true, false, true], "mark one structure layer at most"), ([false, true, false], "only a structure layer")] {
        let e = s.execute("type", &json!({"category": "wall", "name": "Two cores", "layers": layers(bad)})).err().unwrap_or_default();
        assert!(e.contains(what), "{e}");
    }
}

#[test]
fn a_member_left_out_is_gone_from_every_output_and_taken_back_comes_back() {
    let mut s = Session::new();
    let (w, _) = framed(&mut s, "Timber Frame - 114mm", crate::framing::DEFAULT_TYPE);
    let before = members(&mut s, w);
    let key = before
        .iter()
        .find(|m| m["role"] == "stud" && m["key"].as_str().is_some_and(|k| k.starts_with("stud/") && k != "stud/start"))
        .and_then(|m| m["key"].as_str())
        .unwrap_or_else(|| panic!("no grid stud"))
        .to_string();
    let solids = |s: &Session| crate::view3d::solids(&s.model).into_iter().filter(|x| x.owner() == w).count();
    let ifc = |s: &Session| crate::ifc::export(&s.model).matches("IFCMEMBER(").count();
    let (n3d, nifc) = (solids(&s), ifc(&s));
    let reply = run(&mut s, "framing_member", json!({"id": w, "key": key, "exclude": true}));
    assert_eq!(reply["edits"][0]["key"], key.as_str());
    assert_eq!(reply["edits"][0]["excluded"], true);
    let after = members(&mut s, w);
    assert_eq!(after.len(), before.len() - 1);
    assert!(keyed(&after, &key).is_none());
    assert_eq!(solids(&s), n3d - 1, "one solid fewer in 3D");
    assert_eq!(ifc(&s), nifc - 1, "one IfcMember fewer");
    let edits = run(&mut s, "framing", json!({"id": w}))["edits"].clone();
    assert_eq!(edits[0]["key"], key.as_str());
    // Its edit has an id and a GUID of its own.
    let e = &s.model.wall(w).map(|x| x.framing_edits.clone()).unwrap_or_default()[0];
    assert!(e.id != 0 && !e.guid.is_nil(), "{e:?}");
    // Taken back: there again, and the edit gone, as it changes nothing.
    run(&mut s, "framing_member", json!({"id": w, "key": key, "exclude": false}));
    assert_eq!(members(&mut s, w).len(), before.len());
    assert!(s.model.wall(w).is_some_and(|x| x.framing_edits.is_empty()));
    // Undone and redone as one step.
    run(&mut s, "framing_member", json!({"id": w, "key": key, "exclude": true}));
    run(&mut s, "undo", json!({}));
    assert_eq!(members(&mut s, w).len(), before.len());
}

#[test]
fn a_member_resized_keeps_its_place_and_is_warned_of_where_it_no_longer_fits() {
    let mut s = Session::new();
    let (w, win) = framed(&mut s, "Timber Frame - 114mm", crate::framing::DEFAULT_TYPE);
    let g = s.model.guid(win);
    let before = members(&mut s, w);
    // A grid stud doubled: two plies side by side, centred where it stood.
    let stud = before
        .iter()
        .find(|m| m["key"].as_str().is_some_and(|k| k.starts_with("stud/") && k != "stud/start" && k != "stud/end"))
        .cloned()
        .unwrap_or_else(|| panic!("no grid stud"));
    let key = stud["key"].as_str().unwrap_or_default().to_string();
    let reply = run(&mut s, "framing_member", json!({"id": w, "key": key, "plies": 2}));
    let doubled = &reply["members"][0];
    close(f(&doubled["s"], 1) - f(&doubled["s"], 0), 76.0, "two 38 mm plies");
    close((f(&doubled["s"], 0) + f(&doubled["s"], 1)) / 2.0, (f(&stud["s"], 0) + f(&stud["s"], 1)) / 2.0, "centred where it stood");
    // Given a deeper section than the 89 mm layer: still centred across it, and warned of.
    let reply = run(&mut s, "framing_member", json!({"id": w, "key": key, "section": [38, 140]}));
    let deep = &reply["members"][0];
    close(f(&deep["across"], 1) - f(&deep["across"], 0), 140.0, "140 deep");
    close((f(&deep["across"], 0) + f(&deep["across"], 1)) / 2.0, (f(&stud["across"], 0) + f(&stud["across"], 1)) / 2.0, "centred across");
    let warned = reply["warnings"]
        .as_array()
        .is_some_and(|ws| ws.iter().any(|x| x.as_str().is_some_and(|x| x.contains(&key) && x.contains("deeper"))));
    assert!(warned, "{}", reply["warnings"]);
    // The header made deeper: on its jacks still, its top higher, running into the cripples over it.
    let header = format!("{g}/header");
    let old = keyed(&before, &header).cloned().unwrap_or_else(|| panic!("no header"));
    let reply = run(&mut s, "framing_member", json!({"id": w, "key": header, "section": [38, 286]}));
    let new = &reply["members"][0];
    close(f(&new["z"], 0), f(&old["z"], 0), "it stays on its jacks");
    close(f(&new["z"], 2) - f(&new["z"], 0), 286.0, "286 deep");
    assert!(reply["warnings"].as_array().is_some_and(|ws| ws.iter().any(|x| x.as_str().is_some_and(|x| x.contains("runs into")))));
    // The cut list takes the new sizes.
    let cuts = run(&mut s, "framing", json!({"id": w}))["cut_list"].clone();
    assert!(cuts.as_array().is_some_and(|c| c.iter().any(|r| r["section"] == json!([38.0, 286.0]))), "{cuts}");
    // Reset: as the type lays it out again.
    run(&mut s, "framing_member", json!({"id": w, "keys": [key, header], "reset": true}));
    assert_eq!(members(&mut s, w), before);
}

#[test]
fn an_edit_of_a_member_that_goes_sleeps_and_wakes_when_it_comes_back_and_a_wrong_key_is_refused() {
    let mut s = Session::new();
    let (w, _) = framed(&mut s, "Timber Frame - 114mm", crate::framing::DEFAULT_TYPE);
    let e = s.execute("framing_member", &json!({"id": w, "key": "stud/nowhere", "exclude": true})).err().unwrap_or_default();
    assert!(e.contains("no framing member `stud/nowhere`"), "{e}");
    let e = s.execute("framing_member", &json!({"id": w, "key": "stud/start", "section": [0, 89]})).err().unwrap_or_default();
    assert!(e.contains("`section`"), "{e}");
    // The end stud left out; the wall made longer, its end stud is another station's, `stud/end` still: left out again.
    run(&mut s, "framing_member", json!({"id": w, "key": "stud/end", "exclude": true}));
    run(&mut s, "modify", json!({"id": w, "end": [7000, 0]}));
    assert!(keyed(&members(&mut s, w), "stud/end").is_none());
    // A grid stud left out, then the grid laid out from the wall's end: its station's key is gone, and the edit sleeps.
    let key = members(&mut s, w)
        .iter()
        .filter_map(|m| m["key"].as_str().filter(|k| k.starts_with("stud/") && *k != "stud/start").map(String::from))
        .next()
        .unwrap_or_else(|| panic!("no grid stud"));
    run(&mut s, "framing_member", json!({"id": w, "key": key, "exclude": true}));
    run(&mut s, "type", json!({"category": "framing", "name": crate::framing::DEFAULT_TYPE, "layout": "end"}));
    assert_eq!(s.model.wall(w).map(|x| x.framing_edits.len()), Some(2), "both edits kept");
    run(&mut s, "type", json!({"category": "framing", "name": crate::framing::DEFAULT_TYPE, "layout": "start"}));
    assert!(keyed(&members(&mut s, w), &key).is_none(), "awake again, it leaves the stud out");
}

#[test]
fn edits_and_the_marked_layer_last_through_a_file() {
    let mut s = Session::new();
    let (w, _) = framed(&mut s, "Timber Frame - 114mm", crate::framing::DEFAULT_TYPE);
    run(&mut s, "framing_member", json!({"id": w, "key": "stud/start", "plies": 2}));
    let text = s.model.to_json().unwrap_or_else(|e| panic!("{e}"));
    let back = crate::model::Model::read(&text).unwrap_or_else(|e| panic!("{e}")).0;
    let e = back.wall(w).map(|x| x.framing_edits.clone()).unwrap_or_default();
    assert_eq!((e.len(), e[0].key.as_str(), e[0].plies), (1, "stud/start", Some(2)));
    assert_eq!(e[0].guid, s.model.wall(w).map(|x| x.framing_edits[0].guid).unwrap_or_default(), "its GUID kept");
    assert!(back.type_layers("wall", "Timber Frame - 114mm").iter().any(|l| l.framed));
}

#[test]
fn a_walls_framing_is_kept_until_the_model_changes() {
    let mut s = Session::new();
    let (w, win) = framed(&mut s, "Timber Frame - 114mm", crate::framing::DEFAULT_TYPE);
    let once = crate::framing::wall(&s.model, w).ok().flatten().unwrap_or_else(|| panic!("not framed"));
    let again = crate::framing::wall(&s.model, w).ok().flatten().unwrap_or_else(|| panic!("not framed"));
    assert!(Arc::ptr_eq(&once, &again), "worked out once");
    run(&mut s, "modify", json!({"id": win, "offset": 2000}));
    let moved = crate::framing::wall(&s.model, w).ok().flatten().unwrap_or_else(|| panic!("not framed"));
    assert!(!Arc::ptr_eq(&once, &moved) && moved.members != once.members, "the window moved: framed again");
    // A type changed outside a command's elements: framed again too.
    run(&mut s, "type", json!({"category": "framing", "name": crate::framing::DEFAULT_TYPE, "spacing": 609.6}));
    let wider = crate::framing::wall(&s.model, w).ok().flatten().unwrap_or_else(|| panic!("not framed"));
    assert!(wider.members.len() < moved.members.len());
}

#[test]
fn tab_finds_the_members_under_a_ray_nearest_first_and_in_a_plan_round_a_point() {
    let mut s = Session::new();
    let (w, _) = framed(&mut s, "Timber Frame - 114mm", crate::framing::DEFAULT_TYPE);
    let f = crate::framing::wall(&s.model, w).ok().flatten().unwrap_or_else(|| panic!("not framed"));
    // The wall runs east from the origin, its exterior north: looking south through it at 19 mm along, 1200 up, the start stud.
    let hits = crate::framing::edit::on_ray(&f, [19.0, 2000.0, 1200.0], [19.0, -2000.0, 1200.0], &|_| true);
    assert_eq!(hits.first().map(|h| h.0.as_str()), Some("stud/start"), "{hits:?}");
    // Looking down at the start: the top plates first, then the stud under them.
    let down = crate::framing::edit::on_ray(&f, [19.0, 0.0, 5000.0], [19.0, 0.0, 0.0], &|_| true);
    let keys: Vec<&str> = down.iter().map(|h| h.0.as_str()).collect();
    assert!(keys.first().is_some_and(|k| k.starts_with("top_plate")) && keys.contains(&"stud/start"), "{keys:?}");
    // A plan cut at 1200: what lies within 10 mm of a point on the start stud.
    let near = crate::framing::edit::in_plan(&f, crate::geom::P2(19.0, 0.0), 1200.0, 10.0);
    assert_eq!(near.first().map(String::as_str), Some("stud/start"), "{near:?}");
}
