//! A family's origin (O1): placed by where the planes that define its origin cross, every way of putting its axes in the
//! project agreeing, and its instances staying where they stand when the origin moves, whether by an edit, a reload, a
//! nested or shared family's reload or a file of a build that placed by the datums alone.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id, Model};

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

/// Family `name` as the project holds it, as a family file would, its origin moved to `o`: what a reload brings.
fn moved_definition(m: &Model, name: &str, o: [f64; 3]) -> Value {
    let mut d = serde_json::to_value(&m.families[name]).unwrap();
    let planes = d["planes"].as_array_mut().unwrap();
    planes.retain(|p| !p["name"].as_str().is_some_and(|n| n.starts_with("Origin ")));
    planes.extend(origin_planes(o));
    d
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

/// A room of walls 8 by 6 m, and Boxes in it: turned, turned and mirrored, raised, and on an upright reference plane, tilted
/// with it.
fn project() -> (Session, Vec<Id>) {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    run(&mut s, "room", json!({"point": [4000, 3000]}));
    boxes(&mut s);
    let mut ids = vec![
        id_of(&run(&mut s, "component", json!({"family": "Box", "at": [1000, 1000]}))),
        id_of(&run(&mut s, "component", json!({"family": "Box", "at": [3000, 2000], "angle": 30, "mirrored": true}))),
        id_of(&run(&mut s, "component", json!({"family": "Box", "at": [5000, 4000], "angle": -120, "offset": 300}))),
    ];
    run(&mut s, "reference_plane", json!({"start": [6000, 1000], "end": [7000, 2000], "name": "Slant"}));
    ids.push(id_of(&run(
        &mut s,
        "component",
        json!({"family": "Box", "at": [6500, 1500, 400], "angle": 15, "mirrored": true, "host": {"plane": "Slant"}}),
    )));
    (s, ids)
}

fn rounded(v: f64) -> i64 {
    (v * 1e4).round() as i64
}

fn r3(p: [f64; 3]) -> [i64; 3] {
    p.map(rounded)
}

/// Every number in `text` to a tenth of a micron, and its header's time left out: two exports of one model compare equal.
fn numbers_rounded(text: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        match token.parse::<f64>() {
            Ok(v) if token.contains('.') => out.push_str(&format!("{}", rounded(v))),
            _ => out.push_str(token),
        }
        token.clear();
    };
    for line in text.lines().filter(|l| !l.starts_with("FILE_NAME")) {
        for ch in line.chars() {
            if ch.is_ascii_digit() || ch == '.' || (ch == '-' && token.is_empty()) || ((ch == 'E' || ch == 'e') && !token.is_empty()) {
                token.push(ch);
            } else {
                flush(&mut token, &mut out);
                out.push(ch);
            }
        }
        flush(&mut token, &mut out);
        out.push('\n');
    }
    out
}

/// What each path from the family's axes into the project makes of component `id`: its triangles, plan outline and marks, its
/// connectors' places and ways, its room calculation point and room, and its box.
fn placed(m: &Model, id: Id) -> Value {
    let Some(Element::Component(c)) = m.elements.get(id) else { panic!("{id} is a component") };
    let tris: Vec<[[i64; 3]; 3]> = m.component_pieces(c).iter().flat_map(|p| p.tris.iter().map(|t| t.map(r3))).collect();
    let outline: Vec<[i64; 2]> =
        m.component_pieces(c).iter().flat_map(|p| p.outline.iter().map(|q| [rounded(q.0), rounded(q.1)])).collect();
    let marks: Vec<Vec<[i64; 2]>> = m
        .component_marks(c)
        .iter()
        .map(|k| match k {
            crate::family::Mark::Line(points) => points.iter().map(|q| [rounded(q.0), rounded(q.1)]).collect(),
            other => panic!("a line: {other:?}"),
        })
        .collect();
    let connectors: Vec<Value> =
        m.component_connectors(c).iter().map(|(_, at, dir)| json!([at.map(r3), dir.map(|d| r3(d.map(|x| x * 1000.0)))])).collect();
    let f = &m.families[&c.family];
    let bounds = match f.conservative_bounds(&f.values(&c.type_name, &c.params).unwrap()) {
        Some(b) => match crate::bounds::place_component(m, c, b) {
            crate::bounds::Bounds::Box([lo, hi]) => json!([r3(lo), r3(hi)]),
            other => json!(format!("{other:?}")),
        },
        None => json!(null),
    };
    let room = crate::rooms::Rooms::new(m).component(c);
    // A family without a room calculation point is found by its insertion point, which is its origin: that moves with it.
    let room_point = f.room_calculation_point.as_ref().map(|_| r3(m.component_room_point(c)));
    json!({"tris": tris, "outline": outline, "marks": marks, "connectors": connectors, "room_point": room_point, "room": room,
           "bounds": bounds})
}

/// The project's plan at level 1, as an SVG, its numbers rounded.
fn plan(m: &Model) -> String {
    let level = m.levels()[0].0;
    let view = crate::render::View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    numbers_rounded(&crate::render::svg(&crate::render::draw(m, level, view)))
}

fn component(m: &Model, id: Id) -> &crate::model::Component {
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
fn moving_the_origin_keeps_every_instance_where_it_stands_turned_and_mirrored_with_it() {
    let (mut s, ids) = project();
    run(&mut s, "schedule", json!({"name": "Boxes", "category": "components"}));
    let before = s.model.clone();
    let o = [120.0, -40.0, 30.0];
    run(&mut s, "family", json!({"name": "Box", "ref_planes": origin_planes(o)}));
    assert_eq!(s.model.families["Box"].origin(), o);
    for &id in &ids {
        assert_eq!(placed(&s.model, id), placed(&before, id), "component {id}");
        // Its insertion point is where the family's origin now is: its zero as before, moved by the origin turned with it.
        let (was, now) = (before.component_placement(component(&before, id)), s.model.component_placement(component(&s.model, id)));
        assert_eq!(r3(now.origin), r3(was.origin));
        let c = component(&before, id);
        let mirrored = [o[0], if c.mirrored { -o[1] } else { o[1] }, o[2]];
        assert_eq!(r3(now.insertion), r3(was.place(mirrored)), "component {id}: its insertion point, at its origin");
    }
    // A turned, mirrored one moved by the origin turned by its angle and mirrored, not by the origin as it is in plan.
    let c = component(&s.model, ids[1]);
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let (x, y) = (o[0], -o[1]);
    assert_eq!([rounded(c.at.0), rounded(c.at.1)], [rounded(3000.0 + x * cos - y * sin), rounded(2000.0 + x * sin + y * cos)]);
    // On its plane it is moved along the plane and off it, tilted as it is.
    let (h, was) = (component(&s.model, ids[3]).host.unwrap(), component(&before, ids[3]).host.unwrap());
    assert_ne!((h.uv, h.offset), (was.uv, was.offset));
    // The plan, the IFC and the schedule are as they were: nothing in the project moved.
    assert_eq!(plan(&s.model), plan(&before));
    assert_eq!(numbers_rounded(&crate::ifc::export(&s.model)), numbers_rounded(&crate::ifc::export(&before)));
    let table = |m: &Model| {
        let Some(Element::Schedule(sc)) = m.elements.values().find(|e| matches!(e, Element::Schedule(_))) else { panic!("a schedule") };
        serde_json::to_string(&crate::schedule::table(m, sc).unwrap().rows).unwrap()
    };
    assert_eq!(table(&s.model), table(&before));
    // Picked in the plan where it was.
    let level = s.model.levels()[0].0;
    let view = crate::render::View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    for &id in &ids[..3] {
        let at = s.model.component_pieces(component(&s.model, id))[0].outline[0];
        let near = |m: &Model| crate::render::draw(m, level, view).pick(view.px(at), 3.0);
        assert_eq!(near(&s.model), near(&before), "picking at component {id}'s corner");
        assert_eq!(near(&s.model), Some(id));
    }
    // One undo takes the origin and the corrections back together.
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.families["Box"].origin(), [0.0; 3]);
    for &id in &ids {
        assert_eq!(component(&s.model, id), component(&before, id));
    }
}

#[test]
fn a_component_on_a_face_keeps_its_place_along_it_and_follows_the_origin_across_it() {
    let mut s = Session::new();
    let wall = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]})));
    boxes(&mut s);
    let id = id_of(&run(
        &mut s,
        "component",
        json!({"family": "Box", "at": [2000, 0, 500], "host": {"face": {"element": wall, "face": "exterior"}}}),
    ));
    let before = s.model.clone();
    let o = [120.0, -40.0, 30.0];
    run(&mut s, "family", json!({"name": "Box", "ref_planes": origin_planes(o)}));
    let (was, now) = (before.component_placement(component(&before, id)), s.model.component_placement(component(&s.model, id)));
    let n = s.model.host_at(&component(&s.model, id).host.unwrap()).unwrap().normal;
    // Its family's zero moved only across the face: by the origin's part along the face's normal, back.
    let shift: [f64; 3] = std::array::from_fn(|k| now.origin[k] - was.origin[k]);
    let along_normal = crate::view3d::dot(shift, n);
    assert_eq!(r3(shift), r3(n.map(|v| v * along_normal)), "moved across the face only");
    let [a, b, z] = was.axes;
    let across = crate::view3d::dot(std::array::from_fn(|k| o[0] * a[k] + o[1] * b[k] + o[2] * z[k]), n);
    assert_eq!(rounded(along_normal), rounded(-across));
    assert_eq!(component(&s.model, id).host.unwrap().offset, component(&before, id).host.unwrap().offset, "its Offset from Host stays");
}

/// A door family 900 wide, 2100 tall: a frame round its opening, off-centre through the wall so its flips show, and a void
/// through the wall.
fn door_family(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Door", "category": "door", "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 2100}],
               "types": {"D": {}},
               "forms": [{"host": {"plane": crate::family::CENTER_FRONT_BACK}, "profile": [[-450, 0], [-400, 0], [-400, 2100], [-450, 2100]], "from": -20, "to": 60}]}),
    );
}

#[test]
fn a_door_keeps_its_place_along_its_wall_and_follows_the_origin_across_it() {
    let mut s = Session::new();
    let wall = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]})));
    door_family(&mut s);
    let plain = id_of(&run(&mut s, "door", json!({"wall": wall, "offset": 2000, "family": "Door"})));
    let flipped =
        id_of(&run(&mut s, "door", json!({"wall": wall, "offset": 4000, "family": "Door", "flip_hand": true, "flip_facing": true})));
    let before = s.model.clone();
    let pieces = |m: &Model, id: Id| -> Vec<[[i64; 3]; 3]> {
        m.opening_pieces(&m.elements[id]).iter().flat_map(|p| p.tris.iter().map(|t| t.map(r3))).collect()
    };
    let hole = |m: &Model, id: Id| {
        let Some(Element::Door(o)) = m.elements.get(id) else { panic!("a door") };
        let h = crate::walls::hole(m, m.wall(o.host).unwrap(), &m.elements[id], o).unwrap();
        [h.s0, h.s1, h.bottom, h.top].map(rounded)
    };
    // Along the wall and up it, nothing moves: its pieces, its hole, the plan and the IFC.
    run(&mut s, "family", json!({"name": "Door", "ref_planes": origin_planes([-450.0, 0.0, 0.0])}));
    for id in [plain, flipped] {
        assert_eq!(pieces(&s.model, id), pieces(&before, id), "door {id}");
        assert_eq!(hole(&s.model, id), hole(&before, id), "door {id}");
    }
    let Some(Element::Door(o)) = s.model.elements.get(flipped) else { panic!("a door") };
    assert_eq!(rounded(o.offset), rounded(4000.0 + 450.0), "its insertion point at the jamb, its hinge on the other side");
    assert_eq!(plan(&s.model), plan(&before));
    assert_eq!(numbers_rounded(&crate::ifc::export(&s.model)), numbers_rounded(&crate::ifc::export(&before)));
    // Across, the wall's middle fixes it: the family's origin is there now, its pieces moved, its hole not.
    run(&mut s, "family", json!({"name": "Door", "ref_planes": origin_planes([-450.0, 60.0, 0.0])}));
    for (id, way) in [(plain, -1.0), (flipped, 1.0)] {
        let moved: Vec<[[i64; 3]; 3]> = pieces(&before, id).iter().map(|t| t.map(|p| [p[0], p[1] + rounded(way * 60.0), p[2]])).collect();
        assert_eq!(pieces(&s.model, id), moved, "door {id}: across the wall by the origin, flipped as it is");
        assert_eq!(hole(&s.model, id), hole(&before, id));
    }
}

#[test]
fn a_door_whose_origin_moves_below_its_sill_reloads_and_stays_where_it_is() {
    let mut s = Session::new();
    let wall = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]})));
    door_family(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"wall": wall, "offset": 2000, "family": "Door"})));
    let before = s.model.clone();
    let pieces = |m: &Model| -> Vec<[[i64; 3]; 3]> {
        m.opening_pieces(&m.elements[door]).iter().flat_map(|p| p.tris.iter().map(|t| t.map(r3))).collect()
    };
    let hole = |m: &Model| {
        let Some(Element::Door(o)) = m.elements.get(door) else { panic!("a door") };
        let h = crate::walls::hole(m, m.wall(o.host).unwrap(), &m.elements[door], o).unwrap();
        [h.s0, h.s1, h.bottom, h.top].map(rounded)
    };
    // Its family reloaded with a Shelf 100 mm under Ref. Level defining the origin in z: the door at sill 0 stays put, its
    // insertion point now 100 under the wall's base.
    let mut definition = moved_definition(&s.model, "Door", [0.0; 3]);
    definition["planes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "Shelf", "start": [-1000, 0, -100], "end": [1000, 0, -100], "up": [0, 1000, 0], "defines_origin": true}));
    run(&mut s, "family", json!({"name": "Door", "definition": definition, "overwrite": "version"}));
    assert_eq!(s.model.families["Door"].origin(), [0.0, 0.0, -100.0]);
    assert_eq!(pieces(&s.model), pieces(&before));
    assert_eq!(hole(&s.model), hole(&before));
    let Some(Element::Door(o)) = s.model.elements.get(door) else { panic!("a door") };
    assert_eq!(rounded(o.sill), rounded(-100.0), "its insertion point under the wall's base, its opening on it");
    // Saved and opened, and edited, it stays a door in its wall.
    let (read, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(read.check(), Ok(()));
    run(&mut s, "modify", json!({"ids": [door], "mark": "D1"}));
    assert_eq!(hole(&s.model), hole(&before));
}

#[test]
fn a_reload_with_a_new_origin_keeps_a_fittings_connectors_and_pipes_joined() {
    let mut s = Session::new();
    // A tee, turned and mirrored, its three pipes drawn to its connectors.
    let tee = id_of(&run(
        &mut s,
        "component",
        json!({"family": "Tee", "type": "50", "at": [0, 0], "offset": 500, "angle": 30, "mirrored": true}),
    ));
    let ends = |m: &Model| -> Vec<([f64; 3], [f64; 3])> {
        m.component_connectors(component(m, tee)).iter().filter_map(|(_, at, dir)| Some(((*at)?, (*dir)?))).collect()
    };
    let before = ends(&s.model);
    assert_eq!(before.len(), 3);
    for (at, dir) in &before {
        let far = [at[0] + dir[0] * 1000.0, at[1] + dir[1] * 1000.0];
        run(&mut s, "pipe", json!({"points": [[at[0], at[1]], far], "offset": at[2], "system": "Água fria"}));
    }
    let net = |s: &mut Session| {
        let n = run(s, "network", json!({}));
        let through = n["edges"].as_array().unwrap().iter().filter(|e| e["through"] == tee).count();
        (n["groups"].as_array().unwrap().len(), through, n["open"].as_array().unwrap().len())
    };
    assert_eq!(net(&mut s), (1, 2, 3), "joined through the tee");
    let definition = moved_definition(&s.model, "Tee", [40.0, 25.0, 10.0]);
    run(&mut s, "family", json!({"name": "Tee", "definition": definition, "overwrite": "version"}));
    assert_eq!(s.model.families["Tee"].origin(), [40.0, 25.0, 10.0]);
    let after = ends(&s.model);
    for ((a, da), (b, db)) in before.iter().zip(&after) {
        assert_eq!(r3(*a), r3(*b), "a connector's place");
        assert_eq!(r3(da.map(|x| x * 1000.0)), r3(db.map(|x| x * 1000.0)), "a connector's way");
    }
    assert_eq!(net(&mut s), (1, 2, 3), "the pipes still joined through the tee");
}

#[test]
fn a_file_whose_family_already_had_a_plane_marked_opens_as_it_was_drawn() {
    let (s, ids) = project();
    // A build that placed by the datums alone saved its family with a plane marked, and no record of an origin.
    let mut old = (*s.model).clone();
    let f = old.families.get_mut("Box").unwrap();
    f.planes.extend(origin_planes([120.0, -40.0, 30.0]).into_iter().map(|p| serde_json::from_value(p).unwrap()));
    f.settle_planes();
    let text = old.to_json().unwrap();
    assert!(!text.contains("placed_origin"));
    let (read, left_out) = Model::read(&text).unwrap();
    assert_eq!(left_out, Vec::<String>::new());
    for &id in &ids {
        assert_eq!(placed(&read, id), placed(&s.model, id), "component {id}");
    }
    // Saved again, it holds the record, and opens the same.
    let again = read.to_json().unwrap();
    assert!(again.contains("placed_origin"));
    let (twice, _) = Model::read(&again).unwrap();
    for &id in &ids {
        assert_eq!(component(&twice, id), component(&read, id));
    }
}

/// A box `w` wide from its datums, shared if asked, as a family file holds it.
fn box_definition(w: f64, shared: bool) -> Value {
    json!({"category": "generic", "shared": shared, "parameters": [{"name": "W", "value": w}], "types": {"B": {}},
           "forms": [{"profile": [[0, 0], ["W", 0], ["W", 100], [0, 100]], "to": 100}]})
}

/// The triangles of family `name` for type `t`, its nested instances' among them.
fn family_tris(m: &Model, name: &str, t: &str) -> Vec<[[i64; 3]; 3]> {
    let f = &m.families[name];
    let v = f.values(t, &BTreeMap::new()).unwrap();
    f.pieces(&v).unwrap().iter().flat_map(|p| p.tris.iter().map(|t| t.map(r3))).collect()
}

#[test]
fn nested_and_shared_instances_stay_where_they_stand_when_their_familys_origin_moves() {
    let mut s = Session::new();
    run(&mut s, "family", json!({"name": "Box", "definition": box_definition(300.0, true)}));
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "category": "furniture", "parameters": [{"name": "Turn", "value": 20}], "types": {"T": {}},
               "forms": [{"profile": [[0, -400], [10, -400], [10, -300], [0, -300]], "to": 10}]}),
    );
    run(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": box_definition(300.0, true), "name": "Box"}}));
    for (at, angle) in [(json!([100, 50]), json!(35)), (json!([400, -200]), json!("Turn * 2"))] {
        run(&mut s, "family", json!({"name": "Cabinet", "nested_instance": {"family": "Box", "type": "B", "at": at, "angle": angle}}));
    }
    let cabinet = id_of(&run(&mut s, "component", json!({"family": "Cabinet", "at": [2000, 1000], "angle": 45})));
    let boxed = id_of(&run(&mut s, "component", json!({"family": "Box", "at": [5000, 1000], "angle": 10})));
    let (cabinet_tris, cabinet_pieces) = (family_tris(&s.model, "Cabinet", "T"), placed(&s.model, cabinet));
    let box_pieces = placed(&s.model, boxed);
    // The project's Box reloaded with its origin moved: the shared copy nested in the Cabinet is it, its instances in the
    // Cabinet corrected as the project's are.
    let definition = moved_definition(&s.model, "Box", [150.0, 50.0, 20.0]);
    run(&mut s, "family", json!({"name": "Box", "definition": definition, "overwrite": "version"}));
    let nested = &s.model.families["Cabinet"].nested[0].family;
    assert_eq!(nested.origin(), [150.0, 50.0, 20.0], "the shared copy is the project's");
    assert_eq!(family_tris(&s.model, "Cabinet", "T"), cabinet_tris);
    assert_eq!(placed(&s.model, cabinet), cabinet_pieces);
    assert_eq!(placed(&s.model, boxed), box_pieces);
    // A turn by a formula is still one, and its place too.
    let inst = &s.model.families["Cabinet"].nested_instances[1];
    assert!(matches!(inst.at[0], crate::family::Expr::Formula(_)), "{:?}", inst.at);
    // Loaded into the Cabinet again with its origin elsewhere (Load into Project from the nested family's document), no longer
    // shared, so the Cabinet takes it as it is.
    let mut definition = moved_definition(&s.model, "Box", [-60.0, 0.0, 0.0]);
    definition["shared"] = json!(false);
    run(&mut s, "family", json!({"name": "Cabinet", "nest": {"definition": definition, "name": "Box", "overwrite": "version"}}));
    assert_eq!(s.model.families["Cabinet"].nested[0].family.origin(), [-60.0, 0.0, 0.0]);
    assert_eq!(family_tris(&s.model, "Cabinet", "T"), cabinet_tris);
    // And a family file of a build that placed nested families by their datums: its nested instances where they were drawn.
    let mut old = s.model.families["Cabinet"].clone();
    old.nested[0].family.placed_origin = crate::family::PlacedOrigin::default();
    let shift = old.nested[0].family.origin();
    for inst in &mut old.nested_instances {
        // As the old build saved it: placed against the datums, so as far off as the origin is.
        let undo = |e: &crate::family::Expr, by: f64| match e {
            crate::family::Expr::Number(x) => crate::family::Expr::Number(x - by),
            crate::family::Expr::Formula(f) => crate::family::Expr::Formula(format!("({f}) - ({by})")),
        };
        let (sin, cos) = match &inst.angle {
            crate::family::Expr::Number(a) => a.to_radians().sin_cos(),
            crate::family::Expr::Formula(_) => 40f64.to_radians().sin_cos(),
        };
        inst.at = [undo(&inst.at[0], shift[0] * cos - shift[1] * sin), undo(&inst.at[1], shift[0] * sin + shift[1] * cos)];
    }
    let mut file = serde_json::to_value(&old).unwrap();
    file.as_object_mut().unwrap().remove("placed_origin");
    run(&mut s, "family", json!({"name": "Old Cabinet", "definition": file}));
    assert_eq!(family_tris(&s.model, "Old Cabinet", "T"), cabinet_tris);
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
