//! A family's options (Revit's Family Category and Parameters): the flags kept for later, the room calculation point that
//! decides which room a component is in, the override of its category's rule for sections, and its OmniClass number, through
//! the `family` command, files, sections, `elements` and IFC.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::Family;
use crate::geom::P2;
use crate::model::{Id, Model};
use crate::render::{self, POCHE, Prim};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} should be refused, gave {v}"),
        Err(e) => e,
    }
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap()
}

/// A cabinet: length parameters Width and Depth, a number Count, one type, one box.
fn cabinet(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Cabinet", "category": "furniture", "types": {"A": {}}, "parameters": [
            {"name": "Width", "value": 1000}, {"name": "Depth", "value": 600}, {"name": "Count", "kind": "number", "value": 2}
        ], "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "to": 800}]}),
    );
}

#[test]
fn a_family_without_options_writes_none_and_always_vertical_is_on() {
    let f = Family::default();
    assert!(f.always_vertical && !f.work_plane_based && !f.cut_with_voids_when_loaded && !f.shared);
    let v = serde_json::to_value(&f).unwrap();
    for k in crate::family::OPTION_KEYS {
        assert!(v.get(k).is_none(), "{k} is written at its default: an older build would have to leave it out");
    }
    // Read from a file that has none of them, each is its default.
    let back: Family = serde_json::from_value(v).unwrap();
    assert_eq!(back, f);
}

#[test]
fn options_merge_round_trip_and_null_takes_one_back_to_its_default() {
    let mut s = Session::new();
    cabinet(&mut s);
    let all = json!({
        "work_plane_based": true, "always_vertical": false, "cut_with_voids_when_loaded": true, "shared": true,
        "room_calculation_point": ["Width / 2", 900.0, 0.0], "cut_in_views": true,
        "omniclass": {"number": "23.40.20.00", "title": "General Furniture"}
    });
    let r = run(&mut s, "family", json!({"name": "Cabinet", "options": all}));
    assert_eq!(r["options"], all);
    // Always vertical off without work plane based is allowed (a wall-hosted family has it either way).
    run(&mut s, "family", json!({"name": "Cabinet", "options": {"work_plane_based": false}}));
    let f = &s.model.families["Cabinet"];
    assert!(!f.work_plane_based && !f.always_vertical && f.shared, "the others kept");
    // Read, the options come back; a dry run says which changed.
    let read = run(&mut s, "family", json!({"name": "Cabinet"}));
    assert_eq!(read["options"]["omniclass"]["number"], "23.40.20.00");
    let dry = run(&mut s, "family", json!({"name": "Cabinet", "dry_run": true, "options": {"shared": false, "cut_in_views": null}}));
    assert_eq!(dry["changes"]["options"], json!(["shared", "cut_in_views"]));
    // A project file keeps them, and nothing is left out.
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Cabinet"], s.model.families["Cabinet"]);
    // So does a family file another project loads.
    let path = std::env::temp_dir().join(format!("bimcraft-family-options-{}.json", std::process::id()));
    run(&mut s, "family", json!({"name": "Cabinet", "save": path}));
    let mut other = Session::new();
    run(&mut other, "family", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(other.model.families["Cabinet"], s.model.families["Cabinet"]);
    // `null` takes each back to its default.
    let nulls: serde_json::Map<String, Value> = crate::family::OPTION_KEYS.iter().map(|k| ((*k).to_string(), Value::Null)).collect();
    run(&mut s, "family", json!({"name": "Cabinet", "options": nulls}));
    assert_eq!(s.model.families["Cabinet"].options_json(), Family::default().options_json());
    // Undo puts them back.
    run(&mut s, "undo", json!({}));
    assert!(s.model.families["Cabinet"].shared);
}

#[test]
fn an_older_build_reads_a_file_with_options_by_leaving_them_out() {
    // A key a family doesn't know is left out of a file and named, as an older build does with each option.
    let mut s = Session::new();
    cabinet(&mut s);
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let families = v["families"].as_object_mut().unwrap();
    let (_, cabinet) = families.iter_mut().next().unwrap();
    cabinet["a_newer_option"] = json!({"number": "1"});
    let (back, left_out) = Model::read(&v.to_string()).unwrap();
    assert_eq!(left_out.len(), 1, "{left_out:?}");
    assert!(left_out[0].ends_with("/a_newer_option"), "{left_out:?}");
    assert_eq!(back.families["Cabinet"], s.model.families["Cabinet"]);
}

#[test]
fn options_are_checked_with_errors_that_say_what_to_fix() {
    let mut s = Session::new();
    cabinet(&mut s);
    let bad = |s: &mut Session, options: Value, says: &str| {
        let e = err(s, "family", json!({"name": "Cabinet", "options": options}));
        assert!(e.contains(says), "{options}: {e}");
    };
    bad(&mut s, json!({"room_calculation_point": [1e300, 0, 0]}), "room_calculation_point");
    bad(&mut s, json!({"room_calculation_point": [0, 0]}), "[x, y, z]");
    bad(&mut s, json!({"room_calculation_point": ["Count", 0, 0]}), "length parameters");
    bad(&mut s, json!({"room_calculation_point": ["Nowhere * 2", 0, 0]}), "length parameters");
    bad(&mut s, json!({"room_calculation_point": ["Width * 1e300", 0, 0]}), "type `A`");
    bad(&mut s, json!({"room_calculation_point": ["Width +", 0, 0]}), "room_calculation_point");
    for number in ["", "23-30", "a.b", "23..30", ".23", "23.", &"1".repeat(65)] {
        bad(&mut s, json!({"omniclass": {"number": number}}), "omniclass");
    }
    bad(&mut s, json!({"omniclass": {"number": "23", "title": "x".repeat(crate::model::MAX_TEXT + 1)}}), "title");
    bad(&mut s, json!({"omniclass": {"number": "23", "title": "two\nlines"}}), "title");
    bad(&mut s, json!({"omniclass": {"number": "23", "code": "x"}}), "omniclass");
    bad(&mut s, json!({"cut_in_views": "yes"}), "cut_in_views");
    bad(&mut s, json!({"shared": 1}), "shared");
    bad(&mut s, json!({"room_point": [0, 0, 0]}), "room_point");
    bad(&mut s, json!([1, 2]), "options");
    assert_eq!(s.model.families["Cabinet"].options_json(), Family::default().options_json(), "nothing was taken");
    // What is sound is taken; a parameter it reads can't go, and follows a rename.
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "options": {"omniclass": {"number": "23.40.20"}, "room_calculation_point": ["Width / 2", "-Depth", 0]}}),
    );
    let e = err(&mut s, "family", json!({"name": "Cabinet", "delete_parameter": ["Depth"]}));
    assert!(e.contains("Depth"), "{e}");
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Width", "rename": "Breadth"}}));
    let point = s.model.families["Cabinet"].room_calculation_point.clone().unwrap();
    assert_eq!(serde_json::to_value(point).unwrap(), json!(["Breadth / 2", "-Depth", 0.0]));
}

/// The fill rings of element `id` in section `sec`, as a fitted view draws them, and whether a point (along, up) is inside.
fn filled(s: &Session, sec: Id, id: Id) -> impl Fn(f64, f64) -> bool + use<> {
    let view = render::section_fit(&s.model, sec, 1000, 800);
    let d = render::section(&s.model, sec, view);
    let rings: Vec<Vec<P2>> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(id))
        .flat_map(|(p, _)| match p {
            Prim::Fill { rings, .. } => rings.iter().map(|r| r.iter().map(|q| view.mm(*q)).collect()).collect(),
            _ => Vec::new(),
        })
        .collect();
    move |along, up| rings.iter().any(|r| crate::geom::contains(r, P2(along, up)))
}

/// A box 1000 square and 500 tall at (0, 0) of `category`, with `cut_in_views` as given, and a section across its middle.
fn boxed(category: &str, cut_in_views: Value) -> (Session, Id, Id) {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Box", "category": category, "types": {"A": {}}, "options": {"cut_in_views": cut_in_views},
               "forms": [{"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "to": 500}]}),
    );
    let id = id_of(&run(&mut s, "component", json!({"family": "Box", "at": [0, 0]})));
    let sec = id_of(&run(&mut s, "section", json!({"start": [-500, 500], "end": [1500, 500]})));
    (s, id, sec)
}

#[test]
fn cut_in_views_overrides_the_categorys_rule_in_sections() {
    // Furniture isn't cut by its category; told to be, it is, poché.
    let (s, id, sec) = boxed("furniture", json!(null));
    assert!(!filled(&s, sec, id)(1000.0, 250.0), "furniture, by its category");
    let (s, id, sec) = boxed("furniture", json!(true));
    assert!(filled(&s, sec, id)(1000.0, 250.0), "furniture told to be cut");
    let view = render::section_fit(&s.model, sec, 1000, 800);
    let png = render::section_png(&s.model, sec, view).unwrap();
    let pm = tiny_skia::Pixmap::decode_png(&png).unwrap();
    let (x, y) = view.px(P2(1000.0, 250.0));
    let c = pm.pixel(x as u32, y as u32).unwrap();
    assert_eq!((c.red(), c.green(), c.blue()), (POCHE.0, POCHE.1, POCHE.2), "poché");
    // A generic model is cut by its category; told not to be, it isn't.
    let (s, id, sec) = boxed("generic", json!(null));
    assert!(filled(&s, sec, id)(1000.0, 250.0), "a generic model, by its category");
    let (s, id, sec) = boxed("generic", json!(false));
    assert!(!filled(&s, sec, id)(1000.0, 250.0), "a generic model told not to be cut");
    // The 3D scene's solids say the same.
    let cut = |s: &Session| {
        crate::view3d::solids(&s.model).iter().any(|x| x.owner() == id && matches!(x, crate::view3d::Solid::Mesh { cut: true, .. }))
    };
    assert!(!cut(&s));
    let (s, _, _) = boxed("furniture", json!(true));
    assert!(cut(&s));
}

/// Two rooms, west (0..4000) and east (4000..8000), split by a wall along x = 4000, and a cabinet family placed at
/// (3500, 3000) in the west one, turned `angle` and mirrored or not.
fn two_rooms(point: Option<Value>, angle: f64, mirrored: bool) -> (Session, Id, [Id; 2]) {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]]}));
    let west = id_of(&run(&mut s, "room", json!({"point": [2000, 3000], "name": "West"})));
    let east = id_of(&run(&mut s, "room", json!({"point": [6000, 3000], "name": "East"})));
    cabinet(&mut s);
    if let Some(p) = point {
        run(&mut s, "family", json!({"name": "Cabinet", "options": {"room_calculation_point": p}}));
    }
    let c = id_of(&run(&mut s, "component", json!({"family": "Cabinet", "at": [3500, 3000], "angle": angle})));
    if mirrored {
        let mut m = (*s.model).clone();
        if let Some(crate::model::Element::Component(cp)) = m.elements.get_mut(c) {
            cp.mirrored = true;
        }
        s.model = m.into();
    }
    (s, c, [west, east])
}

/// The room `elements` says component `c` is in, by GUID.
fn room_of(s: &mut Session, c: Id) -> Value {
    let all = run(s, "elements", json!({"category": "component"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == json!(c)).unwrap()["room"].clone()
}

#[test]
fn the_room_calculation_point_decides_which_room_a_component_is_in() {
    // Without a point, its insertion point's room.
    let (mut s, c, [west, east]) = two_rooms(None, 0.0, false);
    let guid = |s: &Session, id: Id| json!(s.model.guid(id));
    assert_eq!(room_of(&mut s, c), guid(&s, west));
    // Its point 1000 along its x, across the wall: the east room.
    let (mut s, c, [west, east2]) = two_rooms(Some(json!(["Width", 300, 0])), 0.0, false);
    assert_eq!(room_of(&mut s, c), guid(&s, east2));
    let _ = east;
    // Turned half round, the point turns with it: further west, still the west room.
    let (mut s, c, [west2, _]) = two_rooms(Some(json!(["Width", 300, 0])), 180.0, false);
    assert_eq!(room_of(&mut s, c), guid(&s, west2));
    let _ = west;
    // Turned a quarter, its y is west; mirrored, its y turns about its x and the point goes east.
    let (mut s, c, [_, e]) = two_rooms(Some(json!([0, 1000, 0])), 90.0, true);
    assert_eq!(room_of(&mut s, c), guid(&s, e));
    let (mut s, c, [w, _]) = two_rooms(Some(json!([0, 1000, 0])), 90.0, false);
    assert_eq!(room_of(&mut s, c), guid(&s, w));
    // A point in no room (outside the building): none.
    let (mut s, c, _) = two_rooms(Some(json!([-5000, 0, 0])), 0.0, false);
    assert_eq!(room_of(&mut s, c), Value::Null);
}

/// The STEP entity number of the line holding `what`, e.g. `=IFCSPACE(` with `'East'`.
fn entity(f: &str, what: &[&str]) -> String {
    f.lines().find(|l| what.iter().all(|w| l.contains(w))).and_then(|l| l.split('=').next()).unwrap().to_string()
}

#[test]
fn ifc_contains_a_component_in_the_space_its_point_is_in() {
    let (s, _, _) = two_rooms(Some(json!(["Width", 300, 0])), 0.0, false);
    let f = crate::ifc::export(&s.model);
    let (east, cabinet) = (entity(&f, &["=IFCSPACE(", "'East'"]), entity(&f, &["=IFCFURNITURE("]));
    let rel = f.lines().find(|l| l.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE(") && l.contains(&format!("({cabinet})"))).unwrap();
    assert!(rel.trim_end_matches(';').ends_with(&format!(",{east})")), "{rel}");
    // Without the point, the west space; outside every room, its storey.
    let (s, _, _) = two_rooms(None, 0.0, false);
    let f = crate::ifc::export(&s.model);
    let (west, cabinet) = (entity(&f, &["=IFCSPACE(", "'West'"]), entity(&f, &["=IFCFURNITURE("]));
    let rel = f.lines().find(|l| l.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE(") && l.contains(&format!("({cabinet})"))).unwrap();
    assert!(rel.trim_end_matches(';').ends_with(&format!(",{west})")), "{rel}");
    let (s, _, _) = two_rooms(Some(json!([-5000, 0, 0])), 0.0, false);
    let f = crate::ifc::export(&s.model);
    let storey = entity(&f, &["=IFCBUILDINGSTOREY("]);
    let cabinet = entity(&f, &["=IFCFURNITURE("]);
    let rel = f
        .lines()
        .find(|l| {
            l.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE(") && (l.contains(&format!("{cabinet},")) || l.contains(&format!("{cabinet})")))
        })
        .unwrap();
    assert!(rel.trim_end_matches(';').ends_with(&format!(",{storey})")), "{rel}");
    // Read back, it is on its level still.
    let (s, _, _) = two_rooms(Some(json!(["Width", 300, 0])), 0.0, false);
    let back = crate::import::ifc(crate::ifc::export(&s.model).as_bytes(), "x").unwrap();
    assert_eq!(back.model.elements.values().filter(|e| matches!(e, crate::model::Element::Component(_))).count(), 1);
}

#[test]
fn omniclass_shows_in_elements_and_classifies_the_type_in_ifc() {
    let mut s = Session::new();
    cabinet(&mut s);
    run(&mut s, "family", json!({"name": "Cabinet", "options": {"omniclass": {"number": "23.40.20.14", "title": "Casework"}}}));
    let c = id_of(&run(&mut s, "component", json!({"family": "Cabinet", "at": [0, 0]})));
    assert_eq!(crate::element_json(&s.model, c)["omniclass"], json!({"number": "23.40.20.14", "title": "Casework"}));
    let f = crate::ifc::export(&s.model);
    let system = entity(&f, &["=IFCCLASSIFICATION(", "'OmniClass'"]);
    let reference = entity(&f, &["=IFCCLASSIFICATIONREFERENCE(", "'23.40.20.14'", "'Casework'", &format!("{system},")]);
    let kind = entity(&f, &["=IFCFURNITURETYPE("]);
    let rel = f.lines().find(|l| l.contains("=IFCRELASSOCIATESCLASSIFICATION(")).unwrap();
    assert!(rel.contains(&format!("({kind})")) && rel.trim_end_matches(';').ends_with(&format!(",{reference})")), "{rel}");
    // A family with none writes no classification.
    let mut plain = Session::new();
    cabinet(&mut plain);
    run(&mut plain, "component", json!({"family": "Cabinet", "at": [0, 0]}));
    assert!(!crate::ifc::export(&plain.model).contains("IFCCLASSIFICATION"));
}
