//! Place on Face (T16): a work plane-based family's component hosted on a face of a wall, a floor, a ceiling or a roof,
//! named by its role (`exterior`, `interior`, `top`, `bottom`) with its place in that face's own terms; following the face
//! when the host moves, turns, changes its slope, thickness or type; Always Vertical on every face; carried and copied
//! with its host, deleted with it, kept with a warning when its face no longer reaches it; and files of 17.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, FORMAT, FaceRole, HostRef, Id, Model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Two angles the same turn, to what a turn through the commands' micron keeps.
fn same_angle(a: f64, b: f64) -> bool {
    ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs() < 1e-4
}

/// The first element a command made that answers with `ids`.
fn first(v: &Value) -> Id {
    v["ids"][0].as_u64().unwrap()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// A light: a box 400 by 400, 100 tall over its insertion point, work plane-based (Always Vertical as given).
fn light(s: &mut Session, always_vertical: bool) {
    run(
        s,
        "family",
        json!({"name": "Light", "category": "generic", "types": {"A": {}},
               "options": {"work_plane_based": true, "always_vertical": always_vertical},
               "forms": [{"profile": [[-200, -200], [200, -200], [200, 200], [-200, 200]], "to": 100}]}),
    );
}

fn component(s: &Session, id: Id) -> &crate::model::Component {
    match s.model.elements.get(id) {
        Some(Element::Component(c)) => c,
        other => panic!("{id} is no component: {other:?}"),
    }
}

/// Its insertion point in space.
fn point(s: &Session, id: Id) -> [f64; 3] {
    let c = component(s, id);
    [c.at.0, c.at.1, s.model.elevation(c.level) + c.offset]
}

/// Its family's z in the model.
fn z_of(s: &Session, id: Id) -> [f64; 3] {
    s.model.component_placement(component(s, id)).axes[2]
}

fn on(s: &mut Session, element: Id, face: &str, at: Value) -> Id {
    id_of(&run(s, "component", json!({"family": "Light", "at": at, "host": {"face": {"element": element, "face": face}}})))
}

fn json_of(s: &mut Session, id: Id) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == json!(id)).unwrap().clone()
}

fn floor(s: &mut Session) -> Id {
    id_of(&run(s, "floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "offset": 500})))
}

#[test]
fn a_component_on_a_floors_top_or_bottom_follows_its_offset_slope_and_type() {
    let mut s = Session::new();
    light(&mut s, true);
    let f = floor(&mut s);
    let top = on(&mut s, f, "top", json!([2000, 1000]));
    let under = on(&mut s, f, "bottom", json!([2000, 1000]));
    assert!(near(point(&s, top), [2000.0, 1000.0, 500.0]), "{:?}", point(&s, top));
    assert!(near(point(&s, under), [2000.0, 1000.0, 300.0]), "under the floor's 200: {:?}", point(&s, under));
    let v = json_of(&mut s, top);
    assert_eq!(v["work_plane"], "Floor : Generic - 200mm : Top");
    assert_eq!(v["offset_from_host"], json!(0.0));
    assert_eq!(component(&s, top).host.map(|h| h.host), Some(HostRef::Face { element: s.model.guid(f), face: FaceRole::Top }));
    // The floor raised: both go with it.
    run(&mut s, "modify", json!({"id": f, "offset": 800}));
    assert!(close(point(&s, top)[2], 800.0) && close(point(&s, under)[2], 600.0));
    // Its type thicker: the underside goes down, the top stays.
    run(&mut s, "type", json!({"category": "floor", "name": "Generic - 200mm", "thickness": 300}));
    assert!(close(point(&s, top)[2], 800.0) && close(point(&s, under)[2], 500.0), "{:?}", point(&s, under));
    // Sloped 10 degrees rising east from its west edge: the top climbs 2000 tan 10 under it.
    run(&mut s, "modify", json!({"id": f, "slope": 10, "arrow": [[0, 2000], [6000, 2000]]}));
    let rise = 2000.0 * 10f64.to_radians().tan();
    assert!(close(point(&s, top)[2], 800.0 + rise), "{:?}", point(&s, top));
    assert!(close(point(&s, top)[0], 2000.0) && close(point(&s, top)[1], 1000.0), "it keeps its place in plan");
    // Offset from Host: along the face's normal, up from the top, down from the underside.
    run(&mut s, "modify", json!({"id": under, "offset_from_host": 100}));
    let n = s.model.host_at(&component(&s, under).host.unwrap()).unwrap().normal;
    assert!(n[2] < -0.98 && n[0] > 0.0, "the underside faces down, leaning with the slope: {n:?}");
}

#[test]
fn always_vertical_keeps_z_up_on_every_face_and_off_it_follows_the_face() {
    for always_vertical in [true, false] {
        let mut s = Session::new();
        light(&mut s, always_vertical);
        let f = floor(&mut s);
        run(&mut s, "modify", json!({"id": f, "slope": 20, "arrow": [[0, 2000], [6000, 2000]]}));
        let top = on(&mut s, f, "top", json!([3000, 2000]));
        let ceiling = id_of(&run(&mut s, "ceiling", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})));
        let hung = on(&mut s, ceiling, "bottom", json!([3000, 2000]));
        let (sin, cos) = 20f64.to_radians().sin_cos();
        if always_vertical {
            assert!(near(z_of(&s, top), [0.0, 0.0, 1.0]) && near(z_of(&s, hung), [0.0, 0.0, 1.0]), "plumb, under a ceiling too");
        } else {
            assert!(near(z_of(&s, top), [-sin, 0.0, cos]), "tilted with the slope: {:?}", z_of(&s, top));
            assert!(near(z_of(&s, hung), [0.0, 0.0, -1.0]), "hanging from the underside: {:?}", z_of(&s, hung));
            // What it draws hangs below the ceiling's underside, 2600 up.
            let low = s.model.component_pieces(component(&s, hung)).iter().map(|p| p.bottom).fold(f64::INFINITY, f64::min);
            assert!(close(low, 2500.0), "{low}");
        }
    }
}

#[test]
fn a_component_on_a_walls_side_follows_the_wall_and_turns_with_it() {
    let mut s = Session::new();
    light(&mut s, false);
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 3000})));
    // [x, y] isn't enough on a wall's side: it needs its height.
    let e = err(&mut s, "component", json!({"family": "Light", "at": [2000, 0], "host": {"face": {"element": w, "face": "exterior"}}}));
    assert!(e.contains("[x, y, z]"), "{e}");
    // The exterior is to the left of start to end, 100 out of a wall 200 thick; the point is taken onto it.
    let ext = on(&mut s, w, "exterior", json!([2000, 900, 1200]));
    let int = on(&mut s, w, "interior", json!([4000, -900, 1200]));
    assert!(near(point(&s, ext), [2000.0, 100.0, 1200.0]), "{:?}", point(&s, ext));
    assert!(near(point(&s, int), [4000.0, -100.0, 1200.0]), "{:?}", point(&s, int));
    let p = s.model.component_placement(component(&s, ext));
    // Following the face: z out of it, y up it, x level along it.
    assert!(near(p.axes[2], [0.0, 1.0, 0.0]) && near(p.axes[1], [0.0, 0.0, 1.0]) && near(p.axes[0], [-1.0, 0.0, 0.0]), "{:?}", p.axes);
    assert_eq!(json_of(&mut s, ext)["work_plane"], "Wall : Generic - 200mm : Exterior");
    // The wall moved, turned, thicker: it stays on its face.
    run(&mut s, "move", json!({"ids": [w], "by": [0, 1000]}));
    assert!(near(point(&s, ext), [2000.0, 1100.0, 1200.0]), "{:?}", point(&s, ext));
    run(&mut s, "rotate", json!({"ids": [w], "about": [0, 0], "angle": 90}));
    assert!(near(point(&s, ext), [-1100.0, 2000.0, 1200.0]), "{:?}", point(&s, ext));
    assert!(near(z_of(&s, ext), [-1.0, 0.0, 0.0]), "it turned with the wall: {:?}", z_of(&s, ext));
    run(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "thickness": 300}));
    assert!(near(point(&s, ext), [-1150.0, 2000.0, 1200.0]), "{:?}", point(&s, ext));
    // Flipped, the wall runs the other way: it stays on the same face, now the interior.
    run(&mut s, "flip", json!({"ids": [w]}));
    assert!(near(point(&s, ext), [-1150.0, 2000.0, 1200.0]), "{:?}", point(&s, ext));
    assert_eq!(component(&s, ext).host.map(|h| h.host), Some(HostRef::Face { element: s.model.guid(w), face: FaceRole::Interior }));
    // Moved itself, it slides on the face.
    run(&mut s, "move", json!({"ids": [ext], "by": [500, 300]}));
    assert!(near(point(&s, ext), [-1150.0, 2300.0, 1200.0]), "{:?}", point(&s, ext));
}

#[test]
fn on_a_curved_walls_side_it_faces_out_from_the_centre() {
    let mut s = Session::new();
    light(&mut s, false);
    // A half circle from (0, 0) to (4000, 0) bulging to its right (south), its centre (2000, 0), radius 2000.
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "arc": 180, "height": 3000})));
    let c = on(&mut s, w, "interior", json!([2000, -2500, 1000]));
    let z = z_of(&s, c);
    let p = point(&s, c);
    assert!((p[0] - 2000.0).abs() < 1e-6 && (p[1] + 2100.0).abs() < 1e-6, "on the interior face, 100 out: {p:?}");
    assert!(near(z, [0.0, -1.0, 0.0]), "out from the centre: {z:?}");
}

#[test]
fn on_a_roofs_slope_it_rises_with_the_slope() {
    let mut s = Session::new();
    light(&mut s, true);
    let r = id_of(&run(&mut s, "roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "slope": 30, "overhang": 0})));
    let base = s.model.elevation(component_level(&s, r)) + 0.0;
    let c = on(&mut s, r, "top", json!([4000, 1000]));
    let rise = |deg: f64| 1000.0 * deg.to_radians().tan();
    let z = point(&s, c)[2];
    assert!(z > base + rise(30.0) - 1.0, "on the south slope, 1000 in from its eaves: {z} over {base}");
    run(&mut s, "modify", json!({"id": r, "slope": 40}));
    let higher = point(&s, c)[2];
    // Its top over the eaves' line by the slope, and the roof's 300 plumb across it.
    let top = |deg: f64| rise(deg) + 300.0 / deg.to_radians().cos();
    assert!(close(higher - z, top(40.0) - top(30.0)), "{higher} {z}");
}

fn component_level(s: &Session, roof: Id) -> Id {
    match s.model.elements.get(roof) {
        Some(Element::Roof(r)) => s.model.elements.id(r.level).unwrap(),
        _ => panic!("a roof"),
    }
}

#[test]
fn a_floors_or_a_walls_components_go_with_it_when_it_is_moved_turned_or_copied() {
    let mut s = Session::new();
    light(&mut s, true);
    let f = floor(&mut s);
    let c = on(&mut s, f, "top", json!([2000, 1000]));
    run(&mut s, "move", json!({"ids": [f], "by": [1000, 0]}));
    assert!(near(point(&s, c), [3000.0, 1000.0, 500.0]), "{:?}", point(&s, c));
    run(&mut s, "rotate", json!({"ids": [f], "about": [0, 0], "angle": 90}));
    assert!(
        near(point(&s, c), [-1000.0, 3000.0, 500.0]) && same_angle(component(&s, c).angle, 90.0),
        "{:?} {}",
        point(&s, c),
        component(&s, c).angle
    );
    // A copy of the floor brings a copy of what is on it, on the copy.
    let made = run(&mut s, "copy", json!({"ids": [f], "by": [0, 10000]}));
    let ids: Vec<Id> = made["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let (floor2, light2) = (ids[0], ids[1]);
    assert!(near(point(&s, light2), [-1000.0, 13000.0, 500.0]), "{:?}", point(&s, light2));
    assert_eq!(component(&s, light2).host.map(|h| h.host.guid()), Some(s.model.guid(floor2)));
    // A wall's copy brings its components on the same place of its sides.
    let w = first(&run(&mut s, "wall", json!({"points": [[0, -5000], [6000, -5000]], "height": 3000})));
    let on_wall = on(&mut s, w, "exterior", json!([1000, -4900, 1500]));
    let made = run(&mut s, "copy", json!({"ids": [w], "by": [0, -3000]}));
    let ids: Vec<Id> = made["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 2, "{made}");
    assert!(near(point(&s, ids[1]), [1000.0, -7900.0, 1500.0]), "{:?}", point(&s, ids[1]));
    assert!(near(point(&s, on_wall), [1000.0, -4900.0, 1500.0]));
}

#[test]
fn deleting_the_host_deletes_its_components_and_one_its_face_no_longer_reaches_is_kept_with_a_warning() {
    let mut s = Session::new();
    light(&mut s, true);
    let f = floor(&mut s);
    let c = on(&mut s, f, "top", json!([5000, 3000]));
    assert!(s.model.warnings().iter().all(|w| !w.contains(&c.to_string())));
    // The floor made smaller: the component keeps to the floor's plane where it was, and is warned of.
    run(&mut s, "modify", json!({"id": f, "boundary": [[0, 0], [3000, 0], [3000, 2000], [0, 2000]]}));
    assert!(near(point(&s, c), [5000.0, 3000.0, 500.0]));
    let warned: Vec<String> = s.model.warnings().into_iter().filter(|w| w.contains(&format!("component {c}"))).collect();
    assert!(warned.len() == 1 && warned[0].contains("off its host's face"), "{warned:?}");
    run(&mut s, "undo", json!({}));
    assert!(s.model.warnings().iter().all(|w| !w.contains(&format!("component {c}"))));
    let r = run(&mut s, "delete", json!({"ids": [f]}));
    assert!(r.to_string().contains(&c.to_string()), "{r}");
    assert!(s.model.elements.get(c).is_none());
}

#[test]
fn only_a_face_its_element_has_hosts_and_only_a_work_plane_based_familys_component() {
    let mut s = Session::new();
    light(&mut s, true);
    let f = floor(&mut s);
    let e = err(&mut s, "component", json!({"family": "Light", "at": [0, 0], "host": {"face": {"element": f, "face": "exterior"}}}));
    assert!(e.contains("exterior") && e.contains("top and bottom"), "{e}");
    let e = err(&mut s, "component", json!({"family": "Light", "at": [0, 0], "host": {"face": {"element": f, "face": "side"}}}));
    assert!(e.contains("face"), "{e}");
    let level = s.model.levels()[0].0;
    let e = err(&mut s, "component", json!({"family": "Light", "at": [0, 0], "host": {"face": {"element": level, "face": "top"}}}));
    assert!(e.contains("wall, a floor, a ceiling or a roof"), "{e}");
    run(
        &mut s,
        "family",
        json!({"name": "Box", "types": {"A": {}}, "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": 100}]}),
    );
    let e = err(&mut s, "component", json!({"family": "Box", "at": [100, 100], "host": {"face": {"element": f, "face": "top"}}}));
    assert!(e.contains("work_plane_based"), "{e}");
    // Changed by hand to a face that isn't there, it is refused too.
    let c = on(&mut s, f, "top", json!([100, 100]));
    let e = err(&mut s, "modify", json!({"id": c, "host": {"host": {"face": {"element": f, "face": "interior"}}}}));
    assert!(e.contains("interior"), "{e}");
}

#[test]
fn edit_work_plane_puts_components_on_a_face_and_back_on_a_level() {
    let mut s = Session::new();
    light(&mut s, true);
    let f = floor(&mut s);
    let c = id_of(&run(&mut s, "component", json!({"family": "Light", "at": [1500, 1500]})));
    run(&mut s, "edit_work_plane", json!({"ids": [c], "host": {"face": {"element": f, "face": "bottom"}}}));
    assert!(near(point(&s, c), [1500.0, 1500.0, 300.0]), "{:?}", point(&s, c));
    assert_eq!(json_of(&mut s, c)["work_plane"], "Floor : Generic - 200mm : Bottom");
    run(&mut s, "edit_work_plane", json!({"ids": [c], "host": {"level": "Level 1"}}));
    assert!(component(&s, c).host.is_none() && near(point(&s, c), [1500.0, 1500.0, 0.0]));
}

#[test]
fn a_file_keeps_its_face_hosts_and_a_file_of_22_reads_as_it_did() {
    let mut s = Session::new();
    light(&mut s, false);
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 3000})));
    let c = on(&mut s, w, "exterior", json!([2000, 100, 1500]));
    assert_eq!(FORMAT, 18);
    let text = s.model.to_json().unwrap();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let Some(Element::Component(read)) = m.elements.get(c) else { panic!() };
    assert_eq!(read.host, component(&s, c).host);
    // IFC: its placement's z is the wall's exterior normal.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCDIRECTION((0.,1.,0.))"), "no axis out of the wall");
    // Schedules: its Host, the wall's id.
    let v = json_of(&mut s, c);
    assert_eq!(v["host_element"], json!(s.model.guid(w)));
    let mut old: Value = serde_json::from_str(&text).unwrap();
    old["format"] = json!(17);
    for e in old["elements"].as_object_mut().unwrap().values_mut() {
        if let Some(o) = e.as_object_mut() {
            o.remove("host");
        }
    }
    let (m, left_out) = Model::read(&old.to_string()).unwrap();
    assert!(left_out.is_empty() && m.format == FORMAT, "{left_out:?}");
}

/// For the hostile sweep (`tests::hostile_params_never_panic_or_break_the_model`): a floor and a wall with a component on
/// each, made in `base`; and payloads for the commands that place, move and host them.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    light(base, false);
    let f = base.execute("floor", &json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})).map_or(0, |v| id_of(&v));
    let w = base.execute("wall", &json!({"points": [[0, -1000], [6000, -1000]], "height": 3000})).map_or(0, |v| first(&v));
    let face = |element: Id, face: &str| json!({"face": {"element": element, "face": face}});
    let c = base.execute("component", &json!({"family": "Light", "at": [1000, 1000], "host": face(f, "top")})).map_or(0, |v| id_of(&v));
    let k = base
        .execute("component", &json!({"family": "Light", "at": [1000, -1000, 1000], "host": face(w, "exterior")}))
        .map_or(0, |v| id_of(&v));
    let p = |cmd: &'static str, v: Value| (cmd, v);
    vec![
        p("component", json!({"family": "Light", "at": [1e300, 0], "host": face(f, "top"), "offset": 1e300})),
        p("component", json!({"family": "Light", "at": [0, 0, -1e300], "host": face(w, "interior")})),
        p("component", json!({"family": "Light", "at": [0, 0], "host": {"face": {"element": f}}})),
        p("component", json!({"family": "Light", "at": [0, 0], "host": {"face": {"element": "nope", "face": "top"}}})),
        p("component", json!({"family": "Light", "at": [0, 0], "host": {"face": 5}})),
        p("edit_work_plane", json!({"ids": [c, k], "host": face(w, "exterior")})),
        p("edit_work_plane", json!({"ids": [k], "host": face(f, "bottom")})),
        p("modify", json!({"id": c, "host": {"uv": [1e300, -1e300]}})),
        p("modify", json!({"id": k, "host": {"host": {"face": {"element": c, "face": "top"}}}})),
        p("modify", json!({"id": f, "slope": 89.9, "arrow": [[0, 0], [1, 0]]})),
        p("modify", json!({"id": w, "start": [6000, -1000]})),
        p("move", json!({"ids": [f], "by": [9.99e8, 0]})),
        p("rotate", json!({"ids": [f, w, c], "about": [0, 0], "angle": 1e300})),
        p("mirror", json!({"ids": [f, w], "axis": [[0, 0], [1, 1]], "copy": true})),
        p("copy", json!({"ids": [w, k], "by": [0, 1000]})),
        p("flip", json!({"ids": [w]})),
        p("delete", json!({"ids": [w]})),
        p("type", json!({"category": "floor", "name": "Generic - 200mm", "thickness": 1e300})),
    ]
}
