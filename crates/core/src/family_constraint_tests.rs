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
    // A file of 17 holds no dimensions and reads as it was; this build writes 19 or later.
    const { assert!(crate::model::FORMAT >= 19) };
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
