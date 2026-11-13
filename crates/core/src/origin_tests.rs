//! A family's origin (O1): placed by where the planes that define its origin cross, every way of putting its axes in the
//! project agreeing.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| v["ids"][0].as_u64().unwrap())
}

/// Planes square to x, y and z through `o`, each defining the origin in its direction (none where `o` is 0 there), as
/// `ref_planes` and a family file take them.
fn origin_planes(o: [f64; 3]) -> Vec<Value> {
    let mut out = Vec::new();
    if o[0] != 0.0 {
        out.push(json!({"name": "Origin X", "start": [o[0], 0, 0], "end": [o[0], 1000, 0], "up": [0, 0, 1000], "defines_origin": true}));
    }
    if o[1] != 0.0 {
        out.push(json!({"name": "Origin Y", "start": [0, o[1], 0], "end": [1000, o[1], 0], "up": [0, 0, 1000], "defines_origin": true}));
    }
    if o[2] != 0.0 {
        out.push(json!({"name": "Origin Z", "start": [0, 0, o[2]], "end": [1000, 0, o[2]], "up": [0, 1000, 0], "defines_origin": true}));
    }
    out
}

/// A box 300 by 200, 150 tall, off its datums so a mirror shows, with a plan symbol, a room calculation point and a pipe
/// connector, work plane-based and not Always Vertical, so it tilts with a plane it is placed on.
fn boxes(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Box", "category": "generic", "types": {"A": {}},
               "options": {"work_plane_based": true, "always_vertical": false, "room_calculation_point": [150, 100, 10]},
               "symbols": [{"line": [[0, 0], [300, 200]]}],
               "connectors": [{"kind": "pipe", "domain": "cold", "size": 20, "at": [300, 100, 75], "direction": [1, 0, 0]}],
               "forms": [{"profile": [[0, 0], [300, 0], [300, 200], [0, 200]], "to": 150}]}),
    );
}

fn rounded(v: f64) -> i64 {
    (v * 1e4).round() as i64
}

fn r3(p: [f64; 3]) -> [i64; 3] {
    p.map(rounded)
}

fn component(m: &crate::model::Model, id: Id) -> &crate::model::Component {
    match m.elements.get(id) {
        Some(Element::Component(c)) => c,
        other => panic!("{id} is no component: {other:?}"),
    }
}

#[test]
fn a_family_is_placed_by_where_the_planes_that_define_its_origin_cross() {
    let mut s = Session::new();
    boxes(&mut s);
    assert_eq!(s.model.families["Box"].origin(), [0.0; 3], "the datums, until a plane of its own takes the origin");
    let o = [120.0, 40.0, 30.0];
    run(&mut s, "family", json!({"name": "Box", "ref_planes": origin_planes(o)}));
    assert_eq!(s.model.families["Box"].origin(), o);
    // Placed at a point, turned 90° and mirrored: the family's point o is there, and its own zero o back from it, turned and
    // mirrored with it.
    let id = id_of(&run(&mut s, "component", json!({"family": "Box", "at": [1000, 2000], "angle": 90, "mirrored": true, "offset": 500})));
    let c = component(&s.model, id);
    let p = s.model.component_placement(c);
    assert_eq!(r3(p.insertion), r3([1000.0, 2000.0, 500.0]));
    // Turned 90°, the family's x is the project's y; mirrored, its y is the project's x.
    assert_eq!(r3(p.origin), r3([1000.0 - 40.0, 2000.0 - 120.0, 500.0 - 30.0]));
    assert_eq!(r3(p.place([120.0, -40.0, 30.0])), r3(p.insertion), "the origin, mirrored as the pieces are, is the insertion point");
    // A component's pieces sit about the insertion point as the family's sit about its origin.
    let lo = s
        .model
        .component_pieces(c)
        .iter()
        .flat_map(|pc| pc.tris.iter().flatten())
        .fold([f64::INFINITY; 3], |a, q| std::array::from_fn(|k| a[k].min(q[k])));
    assert_eq!(r3(lo), r3([1000.0 - 40.0, 2000.0 - 120.0, 470.0]));
    // Its room calculation point is the family's; a family without one is found by its insertion point.
    assert_eq!(r3(s.model.component_room_point(c)), r3(p.place([150.0, -100.0, 10.0])));
}

#[test]
fn an_adaptive_family_has_no_insertion_point_to_move() {
    let mut f = crate::family::Family { adaptive: Some(crate::adaptive::four_point_panel()), ..Default::default() };
    f.planes.extend(origin_planes([100.0, 0.0, 0.0]).into_iter().map(|p| serde_json::from_value::<crate::family::RefPlane>(p).unwrap()));
    f.settle_planes();
    assert_eq!(f.origin(), [0.0; 3]);
}

#[test]
fn an_imported_3d_models_family_keeps_its_origin_at_its_base() {
    let dir = std::env::temp_dir().join(format!("bimcraft-origin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A box 2 by 1 by 1 m, its corner at (3, 0, -1) in the file (y up).
    let corners: Vec<String> = (0..8)
        .map(|k| format!("v {} {} {}", 3.0 + 2.0 * f64::from(k & 1), f64::from((k >> 1) & 1), -1.0 + f64::from((k >> 2) & 1)))
        .collect();
    let faces = ["f 1 3 4 2", "f 5 6 8 7", "f 1 2 6 5", "f 3 7 8 4", "f 1 5 7 3", "f 2 4 8 6"];
    std::fs::write(dir.join("block.obj"), format!("{}\n{}\n", corners.join("\n"), faces.join("\n"))).unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "import_mesh", json!({"path": dir.join("block.obj"), "units": "m"}));
    let id = r["components"][0].as_u64().unwrap();
    let name = r["families"][0].as_str().unwrap();
    assert_eq!(s.model.families[name].origin(), [0.0; 3], "its box's centre on its base");
    let c = component(&s.model, id);
    let p = s.model.component_placement(c);
    assert_eq!(p.origin, p.insertion);
    let tris: Vec<[f64; 3]> = s.model.component_pieces(c).iter().flat_map(|pc| pc.tris.iter().flatten().copied()).collect();
    let lo = tris.iter().fold([f64::INFINITY; 3], |a, q| std::array::from_fn(|k| a[k].min(q[k])));
    let hi = tris.iter().fold([f64::NEG_INFINITY; 3], |a, q| std::array::from_fn(|k| a[k].max(q[k])));
    // Where the file has it: x 3 to 5 m, the file's -z north, up 0 to 1 m.
    assert_eq!((r3(lo), r3(hi)), (r3([3000.0, 0.0, 0.0]), r3([5000.0, 1000.0, 1000.0])));
    let _ = std::fs::remove_dir_all(&dir);
}
