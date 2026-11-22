//! F7a: a family's dimensions, labels, locks, EQ and Align's locks, solved as a type is flexed (`family/constraints.rs`).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::constraints::{Edit, NOT_SATISFIED};
use crate::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, Family, REF_LEVEL};
use crate::ids::Guid;
use crate::model::Model;

const LEFT: &str = "11111111-1111-4111-8111-000000000001";
const RIGHT: &str = "11111111-1111-4111-8111-000000000002";
const FRONT: &str = "11111111-1111-4111-8111-000000000003";
const BACK: &str = "11111111-1111-4111-8111-000000000004";
const TOP: &str = "11111111-1111-4111-8111-000000000005";
const BAR: &str = "22222222-2222-4222-8222-000000000001";
const POST: &str = "22222222-2222-4222-8222-000000000002";

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

/// A parametric box: Left and Right labelled Width with EQ about Center (Left/Right), Front and Back labelled Depth with EQ
/// about Center (Front/Back), Top labelled Height over Ref. Level. A bar along x on Center (Left/Right) whose start and end
/// are locked to Left and Right, and a post on Ref. Level whose end is locked to Top.
fn the_box() -> Value {
    let plane = |guid: &str, name: &str, start: [f64; 3], end: [f64; 3], up: [f64; 3]| json!({"guid": guid, "name": name, "start": start, "end": end, "up": up});
    json!({
        "name": "Box",
        "parameters": [
            {"name": "Width", "kind": "length", "value": 1000},
            {"name": "Depth", "kind": "length", "value": 600},
            {"name": "Height", "kind": "length", "value": 900}
        ],
        "types": {
            "Small": {"Width": 600, "Depth": 400, "Height": 500},
            "Large": {"Width": 1400, "Depth": 800, "Height": 1100}
        },
        "ref_planes": [
            plane(LEFT, "Left", [-500.0, -1000.0, 0.0], [-500.0, 1000.0, 0.0], [0.0, 0.0, 1.0]),
            plane(RIGHT, "Right", [500.0, -1000.0, 0.0], [500.0, 1000.0, 0.0], [0.0, 0.0, 1.0]),
            plane(FRONT, "Front", [-1000.0, -300.0, 0.0], [1000.0, -300.0, 0.0], [0.0, 0.0, 1.0]),
            plane(BACK, "Back", [-1000.0, 300.0, 0.0], [1000.0, 300.0, 0.0], [0.0, 0.0, 1.0]),
            plane(TOP, "Top", [-1000.0, 0.0, 900.0], [1000.0, 0.0, 900.0], [0.0, 1.0, 0.0])
        ],
        "forms": [
            {"guid": BAR, "host": {"plane": CENTER_LEFT_RIGHT}, "profile": [[-100, 0], [100, 0], [100, 50], [-100, 50]], "from": -500, "to": 500},
            {"guid": POST, "profile": [[-50, -50], [50, -50], [50, 50], [-50, 50]], "from": 0, "to": 900}
        ],
        "dimensions": [
            {"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, -800, 0], "label": "Width"},
            {"view": "plan", "refs": [{"plane": LEFT}, {"plane": CENTER_LEFT_RIGHT}, {"plane": RIGHT}], "at": [0, -1000, 0], "equal": true},
            {"view": "plan", "refs": [{"plane": FRONT}, {"plane": BACK}], "at": [800, 0, 0], "label": "Depth"},
            {"view": "plan", "refs": [{"plane": FRONT}, {"plane": CENTER_FRONT_BACK}, {"plane": BACK}], "at": [1000, 0, 0], "equal": true},
            {"view": "front", "refs": [{"plane": REF_LEVEL}, {"plane": TOP}], "at": [800, -1000, 0], "label": "Height"},
            {"view": "plan", "refs": [{"plane": LEFT}, {"form": BAR, "end": "start"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true},
            {"view": "plan", "refs": [{"plane": RIGHT}, {"form": BAR, "end": "end"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true},
            {"view": "front", "refs": [{"plane": TOP}, {"form": POST, "end": "end"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true}
        ]
    })
}

fn boxed() -> Session {
    let mut s = Session::new();
    run(&mut s, "family", the_box());
    s
}

/// The least and greatest corner of the triangles of type `t`'s pieces.
fn extent(f: &Family, t: &str) -> ([f64; 3], [f64; 3]) {
    let v = f.values(t, &BTreeMap::new()).unwrap();
    let pieces = f.pieces(&v).unwrap();
    let pts: Vec<[f64; 3]> = pieces.iter().flat_map(|p| p.tris.iter().flatten().copied()).collect();
    let lo = std::array::from_fn(|k| pts.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min));
    let hi = std::array::from_fn(|k| pts.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max));
    (lo, hi)
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn a_parametric_box_flexes_through_its_types_and_its_forms_follow_the_planes() {
    let s = boxed();
    let f = &s.model.families["Box"];
    for (t, w, h) in [("Small", 600.0, 500.0), ("Large", 1400.0, 1100.0)] {
        let (lo, hi) = extent(f, t);
        assert!(close(lo, [-w / 2.0, -100.0, 0.0]), "{t}: {lo:?}");
        assert!(close(hi, [w / 2.0, 100.0, h]), "{t}: {hi:?}");
        // The same as forms drawn there by hand.
        let mut by_hand = f.clone();
        by_hand.dimensions.clear();
        by_hand.forms[0].from = crate::family::Expr::Number(-w / 2.0);
        by_hand.forms[0].to = crate::family::Expr::Number(w / 2.0);
        by_hand.forms[1].to = crate::family::Expr::Number(h);
        let v = f.values(t, &BTreeMap::new()).unwrap();
        let tris = |f: &Family| f.pieces(&v).unwrap().into_iter().map(|p| p.tris).collect::<Vec<_>>();
        assert_eq!(tris(f), tris(&by_hand), "{t}: the triangles of forms drawn where the planes put them");
    }
    // `solve` reads where they go, and each dimension's lengths.
    let r = run(&mut boxed(), "family", json!({"name": "Box", "solve": "Large"}));
    let offset =
        |guid: &str| r["planes"].as_array().unwrap().iter().find(|p| p["guid"] == json!(guid)).unwrap()["offset"].as_f64().unwrap();
    assert_eq!((offset(LEFT), offset(RIGHT), offset(FRONT), offset(BACK), offset(TOP)), (-700.0, 700.0, 400.0, -400.0, 1100.0));
    assert_eq!(r["dimensions"][1]["lengths"], json!([700.0, 700.0]), "EQ");
    assert_eq!(r["dimensions"][2]["lengths"], json!([800.0]));
    // A type flexed after another comes out as it does alone: nothing solved is stored.
    let large = extent(f, "Large");
    extent(f, "Small");
    assert_eq!(extent(f, "Large"), large);
    // A placed component of each type.
    let mut s = boxed();
    let c = run(&mut s, "component", json!({"family": "Box", "type": "Small", "at": [0, 0]}))["id"].as_u64().unwrap();
    assert!(s.model.elements.get(c).is_some());
}

#[test]
fn equality_takes_the_least_change_that_makes_its_segments_equal() {
    let mut s = Session::new();
    let mut fam = the_box();
    // Width's label and the bar's locks gone, Right drawn at 200: EQ alone moves Left and Right as little as it can.
    fam["dimensions"] = json!([
        {"view": "plan", "refs": [{"plane": LEFT}, {"plane": CENTER_LEFT_RIGHT}, {"plane": RIGHT}], "at": [0, -1000, 0], "equal": true}
    ]);
    fam["ref_planes"][1]["start"] = json!([200, -1000, 0]);
    fam["ref_planes"][1]["end"] = json!([200, 1000, 0]);
    run(&mut s, "family", fam);
    let r = run(&mut s, "family", json!({"name": "Box", "solve": "Small"}));
    let offset =
        |guid: &str| r["planes"].as_array().unwrap().iter().find(|p| p["guid"] == json!(guid)).unwrap()["offset"].as_f64().unwrap();
    // Left at -500, Right at 200: each moves 150, to -350 and 350.
    assert!((offset(LEFT) + 350.0).abs() < 1e-9 && (offset(RIGHT) - 350.0).abs() < 1e-9, "{r}");
    assert_eq!(r["dimensions"][0]["lengths"], json!([350.0, 350.0]));
}

#[test]
fn a_locked_segment_keeps_its_length_when_another_label_flexes() {
    let mut s = Session::new();
    let mut fam = the_box();
    // An inner plane 100 inside Right, locked to it.
    fam["ref_planes"].as_array_mut().unwrap().push(json!({"guid": "33333333-3333-4333-8333-000000000001", "name": "Inner", "start": [400, -1000, 0], "end": [400, 1000, 0], "up": [0, 0, 1]}));
    fam["dimensions"].as_array_mut().unwrap().push(json!({"view": "plan", "refs": [{"plane": "33333333-3333-4333-8333-000000000001"}, {"plane": RIGHT}], "at": [0, 500, 0], "locked": [{"segment": 0, "length": 100}]}));
    run(&mut s, "family", fam);
    for (t, right) in [("Small", 300.0), ("Large", 700.0)] {
        let r = run(&mut s, "family", json!({"name": "Box", "solve": t}));
        let inner = r["planes"].as_array().unwrap().iter().find(|p| p["name"] == "Inner").unwrap()["offset"].as_f64().unwrap();
        assert!((inner - (right - 100.0)).abs() < 1e-9, "{t}: {inner}");
    }
}

#[test]
fn a_constraint_that_adds_nothing_is_refused_as_over_constrained_naming_the_others() {
    for extra in [
        // Left to Right locked, which Width's label holds already.
        json!({"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 900, 0], "locked": [{"segment": 0, "length": 1000}]}),
        // Locked between two datums: nothing in it can move.
        json!({"view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"plane": LEFT}], "at": [0, 900, 0], "locked": [{"segment": 0, "length": 500}], "label": null}),
    ] {
        let mut s = boxed();
        let e = s.execute("family", &json!({"name": "Box", "dimension": extra})).unwrap_err();
        assert!(e.contains("over-constrained"), "{e}");
    }
    // EQ with both segments locked and the whole labelled: the last says what the others do.
    let mut s = Session::new();
    let mut fam = the_box();
    fam["dimensions"] = json!([
        {"view": "plan", "refs": [{"plane": LEFT}, {"plane": CENTER_LEFT_RIGHT}, {"plane": RIGHT}], "at": [0, -1000, 0], "equal": true,
         "locked": [{"segment": 0, "length": 500}, {"segment": 1, "length": 500}]}
    ]);
    let e = s.execute("family", &fam).unwrap_err();
    assert!(e.contains("dimension 1 is over-constrained"), "{e}");
    assert!(!s.model.families.contains_key("Box"), "refused, nothing made");
    // A value that can't stand: a labelled length of 0 is refused for its type.
    let mut s = boxed();
    let e = s.execute("family", &json!({"name": "Box", "types": {"Small": {"Width": 0}}})).unwrap_err();
    assert!(e.contains("type `Small`") && e.contains(NOT_SATISFIED), "{e}");
}

#[test]
fn a_label_must_be_a_length_on_one_segment_and_references_parallel_and_seen_edge_on() {
    for (bad, said) in [
        (json!({"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 0, 0], "label": "Nope"}), "Nope"),
        (json!({"view": "plan", "refs": [{"plane": LEFT}, {"plane": FRONT}], "at": [0, 0, 0]}), "parallel"),
        (json!({"view": "front", "refs": [{"plane": FRONT}, {"plane": BACK}], "at": [0, 0, 0]}), "edge on"),
        (
            json!({"view": "plan", "refs": [{"plane": LEFT}, {"plane": CENTER_LEFT_RIGHT}, {"plane": RIGHT}], "at": [0, 0, 0], "label": "Width"}),
            "one segment",
        ),
        (json!({"view": "plan", "refs": [{"plane": LEFT}], "at": [0, 0, 0]}), "2 to"),
        (json!({"view": "plan", "refs": [{"plane": LEFT}, {"plane": LEFT}], "at": [0, 0, 0]}), "twice"),
        (json!({"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [1e300, 0, 0]}), "`at`"),
    ] {
        let e = boxed().execute("family", &json!({"name": "Box", "dimension": bad})).unwrap_err();
        assert!(e.contains(said), "{said}: {e}");
    }
}

#[test]
fn renaming_a_parameter_carries_its_label_and_deleting_a_plane_takes_it_out_of_its_dimensions() {
    let mut s = boxed();
    run(&mut s, "family", json!({"name": "Box", "parameter": {"name": "Width", "rename": "Breadth"}}));
    assert_eq!(s.model.families["Box"].dimensions[0].label.as_deref(), Some("Breadth"));
    // Deleting a labelled parameter is refused while a dimension carries it.
    assert!(s.execute("family", &json!({"name": "Box", "delete_parameter": ["Breadth"]})).is_err());
    // The bar goes: its two locks go with it; Top can't go while the post is locked to it... it can: the lock goes with it.
    let before = s.model.families["Box"].dimensions.len();
    let r = run(&mut s, "family", json!({"name": "Box", "delete_form": [BAR]}));
    assert_eq!(r["deleted_dimensions"], json!(2));
    assert_eq!(s.model.families["Box"].dimensions.len(), before - 2);
    let r = run(&mut s, "family", json!({"name": "Box", "delete_ref_plane": [TOP]}));
    assert_eq!(r["deleted_dimensions"], json!(2), "Height's and the post's lock");
    // Each dimension has its id and GUID, kept by an edit that lists them again.
    let f = &s.model.families["Box"];
    assert!(f.dimensions.iter().all(|d| d.id != 0 && !d.guid.is_nil()));
    let list = serde_json::to_value(&f.dimensions).unwrap();
    let ids: Vec<u64> = f.dimensions.iter().map(|d| d.id).collect();
    run(&mut s, "family", json!({"name": "Box", "dimensions": list}));
    assert_eq!(s.model.families["Box"].dimensions.iter().map(|d| d.id).collect::<Vec<_>>(), ids);
}

#[test]
fn a_reload_with_a_dimension_measuring_to_nothing_is_refused_unless_told_to_delete_it() {
    let mut s = boxed();
    let mut incoming = the_box();
    // The new version has no Top: Height's dimension and the post's lock measure to nothing.
    incoming["ref_planes"].as_array_mut().unwrap().pop();
    let planes = incoming.as_object_mut().unwrap().remove("ref_planes").unwrap();
    incoming["planes"] = planes;
    let name = incoming.as_object_mut().unwrap().remove("name").unwrap();
    let p = json!({"definition": incoming, "name": name, "overwrite": "version"});
    let e = s.execute("family", &p).unwrap_err();
    assert!(e.contains(crate::family::constraints::INVALID_DIMENSIONS) && e.contains("dimension 5, 8"), "{e}");
    assert!(s.model.families["Box"].planes.iter().any(|q| q.guid == g(TOP)), "the project's family as it was");
    let mut p = p;
    p["invalid_dimensions"] = json!("delete");
    let r = run(&mut s, "family", p);
    assert_eq!(r["deleted_dimensions"], json!(2));
    assert_eq!(s.model.families["Box"].dimensions.len(), 6);
    // From a file too.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Box.json");
    let mut bad = the_box();
    bad["dimensions"][0]["refs"][0]["plane"] = json!("44444444-4444-4444-8444-000000000009");
    let planes = bad.as_object_mut().unwrap().remove("ref_planes").unwrap();
    bad["planes"] = planes;
    std::fs::write(&file, bad.to_string()).unwrap();
    let e = Session::new().execute("family", &json!({"path": file.to_string_lossy()})).unwrap_err();
    assert!(e.contains(crate::family::constraints::INVALID_DIMENSIONS), "{e}");
    let mut s = Session::new();
    run(&mut s, "family", json!({"path": file.to_string_lossy(), "invalid_dimensions": "delete"}));
    assert_eq!(s.model.families["Box"].dimensions.len(), 7);
}

#[test]
fn a_project_file_with_a_dimension_measuring_to_nothing_opens_with_it_left_out() {
    let s = boxed();
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let dims = &mut v["families"]["Box"]["dimensions"];
    dims[2]["refs"][0]["plane"] = json!("44444444-4444-4444-8444-000000000009");
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert_eq!(left_out, vec!["/families/Box/dimensions/2".to_string()]);
    assert_eq!(m.families["Box"].dimensions.len(), 7);
    // A file of 17 holds no dimensions and reads as it was; this build writes 18 or later.
    const { assert!(crate::model::FORMAT >= 18) };
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(back.families["Box"].dimensions, s.model.families["Box"].dimensions);
}

#[test]
fn dragging_a_plane_changes_its_label_or_is_refused_where_the_constraints_hold_it() {
    let s = boxed();
    let f = &s.model.families["Box"];
    let v = f.values("Small", &BTreeMap::new()).unwrap();
    // Right dragged out to 400: Width (EQ about the centre) becomes 800, Left goes to -400.
    let edits = f.drag_plane(&v, g(RIGHT), 400.0).unwrap();
    assert!(edits.contains(&Edit::Parameter("Width".into(), 800.0)), "{edits:?}");
    assert!(edits.iter().any(|e| matches!(e, Edit::Plane(p, x) if *p == g(LEFT) && (x + 400.0).abs() < 1e-9)), "{edits:?}");
    // Locked (Family Types' Lock), Width can't change: the drag is refused, saying what holds it.
    let mut locked = f.clone();
    locked.dimensions[0].locked = vec![crate::model::Lock { segment: 0, length: 600.0 }];
    let e = locked.drag_plane(&v, g(RIGHT), 400.0).unwrap_err();
    assert!(e.over && e.message.starts_with(NOT_SATISFIED) && e.dims.contains(&0), "{e:?}");
    // Remove Constraints takes them off: then it moves.
    locked.remove_constraints(&e.dims);
    assert!(locked.drag_plane(&v, g(RIGHT), 400.0).is_ok());
    // A datum stays.
    assert!(f.drag_plane(&v, CENTER_LEFT_RIGHT, 5.0).is_err());
}

#[test]
fn align_moves_a_form_end_onto_a_plane_and_a_reporting_label_measures() {
    let mut s = boxed();
    // A reporting parameter labelling Left to Right reads Width; a formula reading it follows.
    run(
        &mut s,
        "family",
        json!({"name": "Box", "parameter": {"name": "Span", "kind": "length", "reporting": true, "value": 0},
               "dimension": {"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 1200, 0], "label": "Span"}}),
    );
    run(&mut s, "family", json!({"name": "Box", "parameter": {"name": "Half", "kind": "length", "formula": "Span / 2"}}));
    let f = s.model.families["Box"].clone();
    let v = f.values("Large", &BTreeMap::new()).unwrap();
    assert_eq!((v["Span"].clone(), v["Half"].clone()), (crate::family::Val::Number(1400.0), crate::family::Val::Number(700.0)));
    // A label can't be worked out from what a dimension measures.
    let e = s.execute("family", &json!({"name": "Box", "parameter": {"name": "Height", "formula": "Half"}})).unwrap_err();
    assert!(e.contains("which a dimension measures"), "{e}");
    // Align: the post's start (0, on Ref. Level) onto Top: it is free, and takes Top's height.
    let edits = crate::family::constraints::align(
        &f,
        &v,
        &crate::family::DimRef::Plane(g(TOP)),
        &crate::family::DimRef::Form(g(POST), crate::family::Extent::Start),
    )
    .unwrap();
    assert_eq!(edits, vec![Edit::End(1, crate::family::Extent::Start, 1100.0)]);
    // Its end is locked to Top already: refused.
    assert!(
        crate::family::constraints::align(
            &f,
            &v,
            &crate::family::DimRef::Plane(g(BACK)),
            &crate::family::DimRef::Form(g(POST), crate::family::Extent::End)
        )
        .is_err()
    );
}

#[test]
fn hostile_dimensions_never_panic() {
    let mut s = boxed();
    let many: Vec<Value> = (0..1001).map(|_| json!({"refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 0, 0]})).collect();
    let refs: Vec<Value> = (0..65).map(|_| json!({"plane": LEFT})).collect();
    for p in [
        json!({"name": "Box", "dimensions": many}),
        json!({"name": "Box", "dimension": {"refs": refs, "at": [0, 0, 0]}}),
        json!({"name": "Box", "dimension": {"refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [f64::MAX, 0, 0], "locked": [{"segment": 9, "length": -1}]}}),
        json!({"name": "Box", "dimension": {"refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 1e300}]}}),
        json!({"name": "Box", "dimension": {"refs": [{"plane": 5}, {"form": 6, "end": "start"}], "at": [0, 0, 0], "view": "nowhere"}}),
        json!({"name": "Box", "dimension": {"refs": [{"plane": LEFT, "form": BAR}], "at": [0, 0, 0]}}),
        json!({"name": "Box", "dimension": {"refs": "x"}, "delete_dimension": "x"}),
        json!({"name": "Box", "types": {"Small": {"Width": 1e300}}}),
        json!({"name": "Box", "types": {"Small": {"Width": -1}}}),
        json!({"name": "Box", "invalid_dimensions": "maybe", "dimension": {"refs": [{"plane": "44444444-4444-4444-8444-000000000009"}, {"plane": LEFT}], "at": [0, 0, 0]}}),
        json!({"name": "Box", "solve": "Nope"}),
    ] {
        let before = s.model.families["Box"].clone();
        assert!(s.execute("family", &p).is_err(), "{p}");
        assert_eq!(s.model.families["Box"], before);
    }
    // A long chain of planes, each locked to the next: solved, within its caps.
    let mut fam = the_box();
    let mut planes = Vec::new();
    let mut dims = Vec::new();
    for k in 0..200 {
        let guid = format!("55555555-5555-4555-8555-{k:012}");
        planes.push(json!({"guid": guid, "start": [k as f64 * 10.0, -10, 0], "end": [k as f64 * 10.0, 10, 0], "up": [0, 0, 1]}));
        if k > 0 {
            let prev = format!("55555555-5555-4555-8555-{:012}", k - 1);
            dims.push(json!({"refs": [{"plane": prev}, {"plane": guid}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 10}]}));
        }
    }
    fam["name"] = json!("Chain");
    fam["ref_planes"] = json!(planes);
    fam["forms"] = json!([]);
    fam["dimensions"] = json!(dims);
    run(&mut s, "family", fam);
}

#[test]
fn a_move_shifts_what_it_takes_and_is_refused_where_a_constraint_holds_it() {
    use crate::family::constraints::{MoveRefused, Moving, Stays};
    let s = boxed();
    let f = &s.model.families["Box"];
    let v = f.values("Small", &BTreeMap::new()).unwrap();
    let post = f.forms.iter().position(|x| x.guid == g(POST)).unwrap();
    // The post across the plan: each of its sketch's points moved by as much, its ends staying.
    let moving = Moving { forms: vec![post], ..Moving::default() };
    let edits = f.move_by(&v, &moving, [100.0, 50.0, 0.0]).unwrap();
    let points: Vec<[f64; 2]> = edits
        .iter()
        .filter_map(|e| match e {
            Edit::Point(i, _, _, p) if *i == post => Some(*p),
            _ => None,
        })
        .collect();
    assert_eq!(points.len(), 8, "{edits:?}");
    assert!(points.iter().all(|p| (p[0].abs() - 50.0).abs() < 1e-9 || (p[0] - 150.0).abs() < 1e-9), "{points:?}");
    assert!(points.iter().any(|p| p == &[150.0, 100.0]) && points.iter().any(|p| p == &[50.0, 0.0]), "{points:?}");
    assert!(!edits.iter().any(|e| matches!(e, Edit::End(..))), "{edits:?}");
    // Up: its end is locked to Top, which Height holds: refused, naming what holds it.
    let e = f.move_by(&v, &moving, [0.0, 0.0, 100.0]).unwrap_err();
    assert!(matches!(&e, MoveRefused::Unsat(u) if u.message.starts_with(NOT_SATISFIED) && !u.dims.is_empty()), "{e:?}");
    // Right, which Width and EQ hold, refused; Left and Right together across x keep Width, but EQ about the centre holds them.
    let planes = |ps: &[&str]| Moving { planes: ps.iter().map(|p| g(p)).collect(), ..Moving::default() };
    assert!(f.move_by(&v, &planes(&[RIGHT]), [100.0, 0.0, 0.0]).is_err());
    assert!(f.move_by(&v, &planes(&[LEFT, RIGHT]), [100.0, 0.0, 0.0]).is_err());
    // Along themselves they slide freely: nothing to solve but where they are.
    assert!(f.move_by(&v, &planes(&[LEFT, RIGHT]), [0.0, 100.0, 0.0]).is_ok());
    // A datum stays.
    let datum = Moving { planes: vec![CENTER_LEFT_RIGHT], ..Moving::default() };
    assert!(matches!(f.move_by(&v, &datum, [10.0, 0.0, 0.0]), Err(MoveRefused::Stays(Stays::Datum(_)))));
}

#[test]
fn a_reference_line_drawn_on_a_plane_a_drag_or_a_move_moves_goes_with_it() {
    use crate::family::constraints::Moving;
    let mut s = Session::new();
    const WALL: &str = "33333333-3333-4333-8333-000000000001";
    const ON: &str = "33333333-3333-4333-8333-000000000002";
    run(
        &mut s,
        "family",
        json!({"name": "W", "parameters": [{"name": "Gap", "kind": "length", "value": 1800}], "types": {"T": {}},
               "ref_planes": [{"guid": WALL, "name": "Wall", "start": [-2000, 1800, 0], "end": [2000, 1800, 0], "up": [0, 0, 1]}],
               "ref_lines": [{"guid": ON, "host": {"plane": WALL}, "start": [-300, 1800, 500], "end": [300, 1800, 500]}],
               "dimensions": [{"view": "plan", "refs": [{"plane": CENTER_FRONT_BACK}, {"plane": WALL}], "at": [0, 900, 0], "label": "Gap"}]}),
    );
    let f = &s.model.families["W"];
    let v = f.values("T", &BTreeMap::new()).unwrap();
    let line_y = |edits: &[Edit]| {
        edits.iter().find_map(|e| match e {
            Edit::LineEnds(g_, a, b) if *g_ == g(ON) => Some((a[1], b[1])),
            _ => None,
        })
    };
    // Dragged (F4): Gap takes the new length, and the line goes with Wall.
    let n = f.planes.iter().find(|p| p.guid == g(WALL)).unwrap().frame().z;
    let to = 2000.0 * n[1];
    let edits = f.drag_plane(&v, g(WALL), to).unwrap();
    assert_eq!(line_y(&edits), Some((2000.0, 2000.0)), "{edits:?}");
    // Moved with its label taken off (FE-7): the same.
    let mut free = f.clone();
    free.dimensions.clear();
    let edits = free.move_by(&v, &Moving { planes: vec![g(WALL)], ..Moving::default() }, [0.0, 200.0, 0.0]).unwrap();
    assert_eq!(line_y(&edits), Some((2000.0, 2000.0)), "{edits:?}");
}

#[test]
fn a_copy_gives_new_guids_and_takes_the_constraints_among_what_it_copies() {
    use crate::family::constraints::{CopyRefused, Copying, DimRef, Moving};
    let s = boxed();
    let f = &s.model.families["Box"];
    let bar = f.forms.iter().position(|x| x.guid == g(BAR)).unwrap();
    // The bar alone: a new GUID for it and each of its lines, its numbers as they are; its locks to Left and Right dropped.
    let c = f.copies(&Copying { forms: vec![bar], ..Copying::default() }).unwrap();
    let copy = &c.forms[0];
    assert_ne!(copy.guid, g(BAR));
    assert_eq!(c.guids[&g(BAR)], copy.guid);
    assert_eq!((copy.from.clone(), copy.to.clone(), copy.host), (f.forms[bar].from.clone(), f.forms[bar].to.clone(), f.forms[bar].host));
    for (a, b) in f.forms[bar].all_lines().zip(copy.all_lines()) {
        assert_eq!(a.curve, b.curve);
        assert!(a.guid != b.guid && !b.guid.is_nil() && b.id == 0);
    }
    assert!(c.dimensions.is_empty(), "{:?}", c.dimensions);
    // The bar, Left and Right: both Align locks and Width's label come along, naming the copies; EQ, which measures
    // Center (Left/Right), doesn't.
    let c = f.copies(&Copying { forms: vec![bar], planes: vec![g(LEFT), g(RIGHT)], ..Copying::default() }).unwrap();
    assert_eq!(c.planes.len(), 2);
    assert!(c.planes.iter().all(|p| p.name.is_empty() && p.guid != g(LEFT) && p.guid != g(RIGHT)), "a name is one plane's");
    assert_eq!(c.dimensions.len(), 3, "{:?}", c.dimensions);
    let width = c.dimensions.iter().find(|d| d.label.is_some()).unwrap();
    assert_eq!(width.label.as_deref(), Some("Width"));
    assert_eq!(width.refs, vec![DimRef::Plane(c.guids[&g(LEFT)]), DimRef::Plane(c.guids[&g(RIGHT)])]);
    let locks: Vec<_> = c.dimensions.iter().filter(|d| d.alignment).collect();
    assert_eq!(locks.len(), 2);
    assert!(locks.iter().all(|d| d.refs.iter().all(|r| !r.names(g(BAR)) && !r.names(g(LEFT)) && !r.names(g(RIGHT)))), "{locks:?}");
    assert!(c.selected.is_empty(), "none was selected");
    // Added and moved together, the family solves: the copy's Width holds the copies' planes.
    let mut with = f.clone();
    let n = with.forms.len();
    with.planes.extend(c.planes.iter().cloned());
    with.forms.extend(c.forms.iter().cloned());
    with.dimensions.extend(c.dimensions.iter().cloned());
    let v = with.values("Small", &BTreeMap::new()).unwrap();
    let moving = Moving { forms: vec![n], planes: c.planes.iter().map(|p| p.guid).collect(), ..Moving::default() };
    with.move_by(&v, &moving, [0.0, 2000.0, 0.0]).unwrap();
    for (t, width) in [("Small", 600.0), ("Large", 1400.0)] {
        let v = with.values(t, &BTreeMap::new()).unwrap();
        let flexed = with.flexed(&v).unwrap();
        let x = |guid: Guid| flexed.planes.iter().find(|p| p.guid == guid).unwrap().start[0];
        assert!((x(c.planes[1].guid) - x(c.planes[0].guid) - width).abs() < 1e-6, "{t}: {} {}", x(c.planes[0].guid), x(c.planes[1].guid));
    }
    // A datum isn't copied.
    let e = f.copies(&Copying { planes: vec![CENTER_LEFT_RIGHT], ..Copying::default() }).unwrap_err();
    assert!(matches!(e, CopyRefused::Datum(_)), "{e:?}");
}

#[test]
fn a_reporting_label_is_one_dimensions_and_isnt_copied_as_a_constraint() {
    use crate::family::constraints::Copying;
    let mut s = boxed();
    run(
        &mut s,
        "family",
        json!({"name": "Box", "parameter": {"name": "Span", "kind": "length", "reporting": true, "value": 0},
               "dimension": {"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 1200, 0], "label": "Span"}}),
    );
    let f = s.model.families["Box"].clone();
    let span = f.dimensions.iter().find(|d| d.label.as_deref() == Some("Span")).unwrap().guid;
    // Left and Right copied: Width's label comes along, Span's (a reporting one, which holds nothing) doesn't.
    let c = f.copies(&Copying { planes: vec![g(LEFT), g(RIGHT)], ..Copying::default() }).unwrap();
    assert_eq!(c.dimensions.iter().map(|d| d.label.as_deref()).collect::<Vec<_>>(), vec![Some("Width")]);
    // Selected, it is copied without its label: Span still reads the original.
    let c = f.copies(&Copying { planes: vec![g(LEFT), g(RIGHT)], dimensions: vec![span], ..Copying::default() }).unwrap();
    assert_eq!(c.dimensions.len(), 2);
    assert_eq!(c.selected.len(), 1);
    let copy = c.dimensions.iter().find(|d| c.selected.contains(&d.guid)).unwrap();
    assert_eq!(copy.label, None);
    // Two dimensions reporting one parameter: refused.
    let e = s
        .execute("family", &json!({"name": "Box", "dimension": {"view": "plan", "refs": [{"plane": FRONT}, {"plane": BACK}], "at": [1200, 0, 0], "label": "Span"}}))
        .unwrap_err();
    assert!(e.contains("reports `Span` already"), "{e}");
}

#[test]
fn a_refusals_dimensions_renumbered_are_named_so() {
    let u = crate::family::Unsat {
        message: format!("{NOT_SATISFIED}: dimensions 3, 12 and dimension 12 hold it; dimension 4 too"),
        dims: vec![2, 11],
        over: true,
    };
    let r = u.renumbered(&|k| if k == 11 { 1 } else { k });
    assert_eq!(r.message, format!("{NOT_SATISFIED}: dimensions 3, 2 and dimension 2 hold it; dimension 4 too"));
    assert_eq!(r.dims, vec![2, 1]);
}

#[test]
fn a_file_with_two_dimensions_reporting_one_parameter_still_opens_and_warns() {
    let mut s = boxed();
    run(
        &mut s,
        "family",
        json!({"name": "Box", "parameter": {"name": "Span", "kind": "length", "reporting": true, "value": 0},
               "dimension": {"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 1200, 0], "label": "Span"}}),
    );
    // As a build before the rule could save it: a second dimension reporting Span.
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let dims = v["families"]["Box"]["dimensions"].as_array_mut().unwrap();
    dims.push(json!({"view": "plan", "refs": [{"plane": FRONT}, {"plane": BACK}], "at": [1200, 0, 0], "label": "Span"}));
    let n = dims.len();
    let (m, _) = Model::read(&v.to_string()).unwrap_or_else(|e| panic!("the project opens: {e}"));
    let f = &m.families["Box"];
    let twice = f.reported_twice();
    assert_eq!((twice.len(), twice[0].0.as_str(), twice[0].1.last()), (1, "Span", Some(&(n - 1))), "{twice:?}");
    let vals = f.values("Large", &BTreeMap::new()).unwrap();
    assert_eq!(vals["Span"], crate::family::Val::Number(800.0), "it reads the last: Depth");
    // The family's file loads into another project, and says what it reads.
    let mut fam = v["families"]["Box"].clone();
    fam["name"] = json!("Box");
    let path = std::env::temp_dir().join(format!("bimcraft-two-reports-{}.json", std::process::id()));
    std::fs::write(&path, fam.to_string()).unwrap();
    let mut other = Session::new();
    let r = run(&mut other, "family", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let warned = |r: &Value| {
        r["warnings"]
            .as_array()
            .is_some_and(|w| w.iter().any(|w| w.as_str().is_some_and(|w| w.contains(&format!("it reads dimension {n}, the last")))))
    };
    assert!(warned(&r), "{r}");
    assert!(warned(&run(&mut other, "family", json!({"name": "Box"}))));
    // Edited in passing (a parameter's value), it stays as it is; a third report is refused.
    run(&mut other, "family", json!({"name": "Box", "parameter": {"name": "Width", "value": 1100}}));
    let e = other
        .execute("family", &json!({"name": "Box", "dimension": {"view": "front", "refs": [{"plane": REF_LEVEL}, {"plane": TOP}], "at": [-800, -1000, 0], "label": "Span"}}))
        .unwrap_err();
    assert!(e.contains("reports `Span` already"), "{e}");
}
