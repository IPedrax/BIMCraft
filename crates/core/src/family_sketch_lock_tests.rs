//! F7b: a form's sketch lines, their points and circles, and reference lines' faces as references of a family's dimensions,
//! solved as a type flexes: a sketched form locked to its planes flexes with them (`family/constraints.rs`).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::constraints::{self, Edit, NOT_SATISFIED};
use crate::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, DimRef, Expr, Family, Spot};
use crate::ids::Guid;
use crate::model::Model;

const LEFT: &str = "11111111-1111-4111-8111-000000000001";
const RIGHT: &str = "11111111-1111-4111-8111-000000000002";
const FRONT: &str = "11111111-1111-4111-8111-000000000003";
const BACK: &str = "11111111-1111-4111-8111-000000000004";
const SLAB: &str = "33333333-3333-4333-8333-000000000001";
const SOUTH: &str = "33333333-3333-4333-8333-000000000002";
const EAST: &str = "33333333-3333-4333-8333-000000000003";
const NORTH: &str = "33333333-3333-4333-8333-000000000004";
const WEST: &str = "33333333-3333-4333-8333-000000000005";

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

fn plane(guid: &str, name: &str, start: [f64; 3], end: [f64; 3]) -> Value {
    json!({"guid": guid, "name": name, "start": start, "end": end, "up": [0, 0, 1]})
}

fn lock(a: Value, b: Value) -> Value {
    json!({"view": "plan", "refs": [a, b], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true})
}

fn line(l: &str) -> Value {
    json!({"form": SLAB, "line": l})
}

/// A box of planes: Left and Right labelled Width with EQ about Center (Left/Right), Front and Back labelled Depth with EQ
/// about Center (Front/Back). A slab on Ref. Level sketched as a rectangle at ±500 by ±300 (drawn as the default type has
/// it), each of its four lines locked to the plane it lies on.
fn the_slab() -> Value {
    json!({
        "name": "Slab",
        "parameters": [{"name": "Width", "kind": "length", "value": 1000}, {"name": "Depth", "kind": "length", "value": 600}],
        "types": {"Small": {"Width": 600, "Depth": 400}, "Medium": {"Width": 1000, "Depth": 600}, "Large": {"Width": 1400, "Depth": 900}},
        "ref_planes": [
            plane(LEFT, "Left", [-500.0, -1000.0, 0.0], [-500.0, 1000.0, 0.0]),
            plane(RIGHT, "Right", [500.0, -1000.0, 0.0], [500.0, 1000.0, 0.0]),
            plane(FRONT, "Front", [-1000.0, -300.0, 0.0], [1000.0, -300.0, 0.0]),
            plane(BACK, "Back", [-1000.0, 300.0, 0.0], [1000.0, 300.0, 0.0])
        ],
        "forms": [{"guid": SLAB, "sketch": [
            {"guid": SOUTH, "start": [-500, -300], "end": [500, -300]},
            {"guid": EAST, "start": [500, -300], "end": [500, 300]},
            {"guid": NORTH, "start": [500, 300], "end": [-500, 300]},
            {"guid": WEST, "start": [-500, 300], "end": [-500, -300]}
        ], "from": 0, "to": 200}],
        "dimensions": [
            {"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, -800, 0], "label": "Width"},
            {"view": "plan", "refs": [{"plane": LEFT}, {"plane": CENTER_LEFT_RIGHT}, {"plane": RIGHT}], "at": [0, -1000, 0], "equal": true},
            {"view": "plan", "refs": [{"plane": FRONT}, {"plane": BACK}], "at": [800, 0, 0], "label": "Depth"},
            {"view": "plan", "refs": [{"plane": FRONT}, {"plane": CENTER_FRONT_BACK}, {"plane": BACK}], "at": [1000, 0, 0], "equal": true},
            lock(json!({"plane": LEFT}), line(WEST)),
            lock(json!({"plane": RIGHT}), line(EAST)),
            lock(json!({"plane": FRONT}), line(SOUTH)),
            lock(json!({"plane": BACK}), line(NORTH))
        ]
    })
}

fn slab() -> Session {
    let mut s = Session::new();
    run(&mut s, "family", the_slab());
    s
}

fn values(f: &Family, t: &str) -> BTreeMap<String, crate::family::Val> {
    f.values(t, &BTreeMap::new()).unwrap()
}

fn tris(f: &Family, t: &str) -> Vec<Vec<[[f64; 3]; 3]>> {
    f.pieces(&values(f, t)).unwrap().into_iter().map(|p| p.tris).collect()
}

/// `f` without its dimensions, its first form's sketch made the rectangle `w` by `d` about the origin, drawn by hand.
fn by_hand(f: &Family, w: f64, d: f64) -> Family {
    let mut h = f.clone();
    h.dimensions.clear();
    let (x, y) = (w / 2.0, d / 2.0);
    let n = Expr::Number;
    h.forms[0].set_corners(&[[n(-x), n(-y)], [n(x), n(-y)], [n(x), n(y)], [n(-x), n(y)]]);
    h
}

#[test]
fn a_rectangle_locked_to_four_planes_flexes_through_its_types_as_the_slab_drawn_by_hand() {
    let s = slab();
    let f = &s.model.families["Slab"];
    for (t, w, d) in [("Small", 600.0, 400.0), ("Medium", 1000.0, 600.0), ("Large", 1400.0, 900.0)] {
        assert_eq!(tris(f, t), tris(&by_hand(f, w, d), t), "{t}: triangle for triangle the slab drawn there by hand");
    }
    // Nothing solved is stored: Large after Small comes out as Large alone, and the file keeps the sketch as drawn.
    let large = tris(f, "Large");
    tris(f, "Small");
    assert_eq!(tris(f, "Large"), large);
    assert_eq!(f.forms[0].sketch[1].curve, by_hand(f, 1000.0, 600.0).forms[0].sketch[1].curve);
    // `solve` reads where the sketch's lines go.
    let r = run(&mut slab(), "family", json!({"name": "Slab", "solve": "Large"}));
    let east = r["forms"][0]["sketch"].as_array().unwrap().iter().find(|l| l["guid"] == json!(EAST)).unwrap().clone();
    assert_eq!((east["start"].clone(), east["end"].clone()), (json!([700.0, -450.0]), json!([700.0, 450.0])));
    // A component of each type, and its box: where the sketch went.
    let mut s = slab();
    let c = run(&mut s, "component", json!({"family": "Slab", "type": "Large", "at": [0, 0]}))["id"].as_u64().unwrap();
    let f = &s.model.families["Slab"];
    let [lo, hi] = f.conservative_bounds(&values(f, "Large")).unwrap();
    assert!(lo[0] <= -700.0 && hi[0] >= 700.0 && lo[1] <= -450.0 && hi[1] >= 450.0, "{lo:?} {hi:?}");
    assert!(s.model.elements.get(c).is_some());
}

#[test]
fn a_formula_profile_unlocked_keeps_flexing_and_a_lock_wins_over_a_formula() {
    let mut v = the_slab();
    // The south line's ends as formulas of Depth, and the north line's too: the locks to Front and Back set them anyway.
    v["forms"][0]["sketch"][0]["start"] = json!([-500, "-Depth / 2 - 50"]);
    v["forms"][0]["sketch"][0]["end"] = json!([500, "-Depth / 2 - 50"]);
    v["forms"][0]["sketch"][1]["start"] = json!([500, "-Depth / 2 - 50"]);
    v["forms"][0]["sketch"][3]["end"] = json!([-500, "-Depth / 2 - 50"]);
    let mut s = Session::new();
    run(&mut s, "family", v);
    let f = &s.model.families["Slab"];
    assert_eq!(tris(f, "Large"), tris(&by_hand(f, 1400.0, 900.0), "Large"), "the lock wins");
    // Unlocked from Front, the formula sets the south line again.
    let mut free = f.clone();
    free.dimensions.retain(|d| !d.refs.contains(&DimRef::Sketch(g(SLAB), g(SOUTH))));
    let flexed = free.flexed(&values(&free, "Large")).unwrap();
    let (lo, _) = (flexed.forms[0].loops(&mut |e| crate::family::worth(e, &values(&free, "Large"))).unwrap(), ());
    let ys: Vec<f64> = lo[0].outer.iter().map(|p| p.1).collect();
    assert!(ys.iter().any(|y| (y + 500.0).abs() < 1e-6), "-Depth/2 - 50 for Large: {ys:?}");
}

#[test]
fn a_circles_radius_and_diameter_take_labels_and_its_centre_follows_planes() {
    let mut s = Session::new();
    const DISC: &str = "44444444-4444-4444-8444-000000000001";
    const RING: &str = "44444444-4444-4444-8444-000000000002";
    const HOOP: &str = "44444444-4444-4444-8444-000000000003";
    const HOOP_LINE: &str = "44444444-4444-4444-8444-000000000004";
    run(
        &mut s,
        "family",
        json!({
            "name": "Disc",
            "parameters": [{"name": "R", "kind": "length", "value": 100}, {"name": "Off", "kind": "length", "value": 300}, {"name": "D", "kind": "length", "value": 80}],
            "types": {"A": {"R": 150, "Off": 200, "D": 60}, "B": {"R": 50, "Off": 450, "D": 120}},
            "ref_planes": [plane(LEFT, "Left", [-500.0, -1000.0, 0.0], [-500.0, 1000.0, 0.0]), plane(FRONT, "Front", [-1000.0, -300.0, 0.0], [1000.0, -300.0, 0.0])],
            "forms": [
                {"guid": DISC, "sketch": [{"guid": RING, "centre": [-200, 0], "radius": 100}], "to": 50},
                {"guid": HOOP, "sketch": [{"guid": HOOP_LINE, "centre": [0, 500], "radius": 40}], "to": 30}
            ],
            "dimensions": [
                {"view": "plan", "kind": "radial", "refs": [{"form": DISC, "line": RING}], "at": [0, 0, 0], "label": "R"},
                {"view": "plan", "refs": [{"plane": LEFT}, {"form": DISC, "line": RING, "point": "centre"}], "at": [0, -600, 0], "label": "Off"},
                lock(json!({"plane": FRONT}), json!({"form": DISC, "line": RING, "point": "centre"})),
                {"view": "plan", "kind": "diameter", "refs": [{"form": HOOP, "line": HOOP_LINE}], "at": [0, 600, 0], "label": "D"}
            ]
        }),
    );
    let f = &s.model.families["Disc"];
    for (t, r, off, d) in [("A", 150.0, 200.0, 60.0), ("B", 50.0, 450.0, 120.0)] {
        let mut h = f.clone();
        h.dimensions.clear();
        let n = Expr::Number;
        h.forms[0].set_circle(&crate::family::Circle { centre: [n(-500.0 + off), n(-300.0)], radius: n(r) });
        h.forms[1].set_circle(&crate::family::Circle { centre: [n(0.0), n(500.0)], radius: n(d / 2.0) });
        assert_eq!(tris(f, t), tris(&h, t), "{t}");
        assert_eq!(f.measured(&values(f, t)).unwrap()[0], vec![r], "a radial dimension measures the radius");
        assert_eq!(f.measured(&values(f, t)).unwrap()[3], vec![d], "a diameter one the diameter");
    }
    // A radius of 0 can't stand; an EQ or a second reference on a radial one is refused.
    let mut bad = s.clone();
    let e = bad.execute("family", &json!({"name": "Disc", "types": {"A": {"R": 0}}})).unwrap_err().to_string();
    assert!(e.contains(NOT_SATISFIED), "{e}");
    let e = bad
        .execute("family", &json!({"name": "Disc", "dimension": {"kind": "radial", "refs": [{"form": DISC, "line": RING}, {"plane": LEFT}], "at": [0, 0, 0]}}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("measures one circle"), "{e}");
    let e = bad
        .execute(
            "family",
            &json!({"name": "Disc", "dimension": {"refs": [{"plane": LEFT}, {"form": DISC, "line": RING}], "at": [0, 0, 0]}}),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("a circle; it is measured by its centre or by a radial dimension"), "{e}");
}

#[test]
fn a_line_held_at_both_ends_keeps_no_soft_row_and_a_lock_against_labels_is_over_constrained() {
    let mut s = slab();
    // The west line's two ends locked to Left one by one besides the line itself: its parallel row adds nothing, and isn't refused.
    let point = |spot: &str| json!({"form": SLAB, "line": WEST, "point": spot});
    let mut v = the_slab();
    let dims = v["dimensions"].as_array_mut().unwrap();
    dims.retain(|d| d["refs"][1] != line(WEST));
    dims.push(lock(json!({"plane": LEFT}), point("start")));
    dims.push(lock(json!({"plane": LEFT}), point("end")));
    run(&mut s, "family", json!({"name": "Slab", "dimensions": v["dimensions"]}));
    let f = &s.model.families["Slab"];
    assert_eq!(tris(f, "Large"), tris(&by_hand(f, 1400.0, 900.0), "Large"));
    // A labelled gap from the east line to Right, while it is locked to Right and Width is labelled: it adds nothing.
    let e = s
        .execute(
            "family",
            &json!({"name": "Slab", "parameter": {"name": "Gap", "kind": "length", "value": 10}, "dimension": {"view": "plan", "refs": [line(EAST), {"plane": RIGHT}], "at": [0, 0, 0], "label": "Gap"}}),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("over-constrained"), "{e}");
    // A dimension of points alone has no way to measure along.
    let e = s
        .execute("family", &json!({"name": "Slab", "dimension": {"view": "plan", "refs": [point("start"), point("end")], "at": [0, 0, 0]}}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("points alone"), "{e}");
}

#[test]
fn a_form_given_again_keeps_the_dimensions_of_the_lines_it_keeps_and_drops_those_of_the_lines_gone() {
    let mut s = slab();
    let f = s.model.families["Slab"].clone();
    // The same four lines, the east one moved: its lock stays, and wins.
    let mut sketch = serde_json::to_value(&f.forms[0].sketch).unwrap();
    sketch[1]["start"] = json!([450, -300]);
    sketch[1]["end"] = json!([450, 300]);
    sketch[0]["end"] = json!([450, -300]);
    sketch[2]["start"] = json!([450, 300]);
    let r = run(&mut s, "family", json!({"name": "Slab", "form": {"guid": SLAB, "sketch": sketch}}));
    assert_eq!(r["deleted_dimensions"], json!(0));
    assert_eq!(s.model.families["Slab"].dimensions.len(), 8);
    // A triangle instead: the east line goes, and its lock with it; the others keep theirs.
    // (The diagonal a line of its own: one given no GUID would take the east line's by its place, as F5 keeps them.)
    let tri = json!([f.forms[0].sketch[0], {"guid": "33333333-3333-4333-8333-000000000009", "start": [500, -300], "end": [-500, 300]}, f.forms[0].sketch[3]]);
    let r = run(&mut s, "family", json!({"name": "Slab", "form": {"guid": SLAB, "sketch": tri}}));
    assert_eq!(r["deleted_dimensions"], json!(2), "the locks to the east and north lines");
    let f = &s.model.families["Slab"];
    assert_eq!(f.dimensions.len(), 6);
    assert!(f.dimensions.iter().any(|d| d.refs.contains(&DimRef::Sketch(g(SLAB), g(WEST)))));
    assert!(!f.dimensions.iter().any(|d| d.refs.iter().any(|r| r.names(g(EAST)))));
    // Deleting the form takes the rest.
    let r = run(&mut s, "family", json!({"name": "Slab", "delete_form": [SLAB]}));
    assert_eq!(r["deleted_dimensions"], json!(2));
}

#[test]
fn a_command_names_a_sketch_line_by_its_ids_and_what_isnt_one_is_refused() {
    let mut s = slab();
    let f = s.model.families["Slab"].clone();
    let (form, east) = (f.forms[0].id, f.forms[0].sketch[1].id);
    run(
        &mut s,
        "family",
        json!({"name": "Slab", "dimension": {"view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"form": form, "line": east}], "at": [0, 900, 0]}}),
    );
    let d = s.model.families["Slab"].dimensions.last().cloned().unwrap();
    assert_eq!(d.refs[1], DimRef::Sketch(g(SLAB), g(EAST)), "ids read as the GUIDs they name");
    assert_eq!(s.model.families["Slab"].measured(&values(&f, "Large")).unwrap().last(), Some(&vec![700.0]));
    // An arc, a line's centre, a line seen the way a view looks, a line of another form and a line it hasn't are refused.
    let refused = |s: &mut Session, refs: Value, view: &str, said: &str| {
        let e = s
            .execute("family", &json!({"name": "Slab", "dimension": {"view": view, "refs": refs, "at": [0, 0, 0]}}))
            .unwrap_err()
            .to_string();
        assert!(e.contains(said), "{said}: {e}");
    };
    refused(&mut s, json!([{"plane": LEFT}, {"form": SLAB, "line": WEST, "point": "centre"}]), "plan", "a straight line has no centre");
    refused(&mut s, json!([{"plane": LEFT}, line(WEST)]), "left", "the way its view looks");
    refused(&mut s, json!([{"plane": FRONT}, line(WEST)]), "plan", "aren't parallel");
    refused(&mut s, json!([{"plane": LEFT}, line(LEFT)]), "plan", "no longer has");
    let mut arc = the_slab();
    arc["forms"][0]["sketch"][0]["turn"] = json!(30);
    arc["dimensions"] = json!([lock(json!({"plane": FRONT}), line(SOUTH))]);
    arc["name"] = json!("Arc");
    let e = s.execute("family", &arc).unwrap_err().to_string();
    assert!(e.contains("an arc; a dimension measures to a straight line"), "{e}");
    // A form drawn on a reference line's face is measured (F7c): this one's sketch turned a quarter, so its lines lie across
    // the planes they are locked to.
    let mut on_line = the_slab();
    on_line["name"] = json!("OnLine");
    on_line["ref_lines"] = json!([{"guid": "55555555-5555-4555-8555-000000000001", "start": [0, -2000, 0], "end": [0, 2000, 0]}]);
    on_line["forms"][0]["host"] = json!({"line": "55555555-5555-4555-8555-000000000001", "face": "horizontal"});
    let e = s.execute("family", &on_line).unwrap_err().to_string();
    assert!(e.contains("aren't parallel"), "{e}");
}

#[test]
fn a_reference_lines_face_is_a_reference_and_moves_with_its_plane() {
    let mut s = slab();
    const RAIL: &str = "55555555-5555-4555-8555-000000000002";
    // A reference line on Ref. Level along y at x = 200; the slab's east line 100 short of it, labelled Gap.
    let r = run(
        &mut s,
        "family",
        json!({"name": "Slab", "parameter": {"name": "Gap", "kind": "length", "value": 100}, "ref_lines": [{"guid": RAIL, "start": [800, -1000, 0], "end": [800, 1000, 0]}],
               "dimensions": [lock(json!({"plane": LEFT}), line(WEST)), lock(json!({"plane": FRONT}), line(SOUTH)), lock(json!({"plane": BACK}), line(NORTH)),
                              {"view": "plan", "refs": [line(EAST), {"line": RAIL, "face": "vertical"}], "at": [0, 600, 0], "label": "Gap"}]}),
    );
    assert!(r.get("name").is_some());
    let f = &s.model.families["Slab"];
    let v = values(f, "Medium");
    let flexed = f.flexed(&v).unwrap();
    let crate::family::sketch::FormCurve::Line { start, .. } = &flexed.forms[0].sketch[1].curve else { panic!("a line") };
    assert_eq!(start[0], Expr::Number(700.0), "100 short of the line at 800");
    // Align moves the east line onto the reference line: its ends, and the ends of the lines joined to them.
    let mut free = f.clone();
    free.dimensions.pop();
    let edits =
        constraints::align(&free, &v, &DimRef::Line(g(RAIL), crate::family::Face::Vertical), &DimRef::Sketch(g(SLAB), g(EAST))).unwrap();
    let moved: Vec<(usize, Spot, [f64; 2])> =
        edits.iter().filter_map(|e| if let Edit::Point(0, k, spot, p) = e { Some((*k, *spot, *p)) } else { None }).collect();
    assert_eq!(moved.len(), 4, "{edits:?}");
    assert!(moved.iter().all(|(_, _, p)| p[0] == 800.0), "{moved:?}");
    // Locked to Left, which Width and EQ hold, the west line can't be aligned elsewhere: asked.
    let held = &slab().model.families["Slab"];
    let e = constraints::align(held, &values(held, "Medium"), &DimRef::Plane(CENTER_LEFT_RIGHT), &DimRef::Sketch(g(SLAB), g(WEST)))
        .unwrap_err();
    assert!(e.message.starts_with(NOT_SATISFIED) && e.over, "{}", e.message);
}

#[test]
fn a_file_keeps_its_sketch_dimensions_and_a_copy_of_the_family_measures_its_own_lines() {
    let s = slab();
    let text = s.model.to_json().unwrap();
    assert!(
        text.contains(&format!("\"format\": {}", crate::model::FORMAT)) || text.contains(&format!("\"format\":{}", crate::model::FORMAT))
    );
    const { assert!(crate::model::FORMAT >= 28) };
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Slab"].dimensions, s.model.families["Slab"].dimensions);
    // A sketch line a file's dimension names that the form hasn't: left out, as F7a's planes are.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["families"]["Slab"]["forms"][0]["sketch"].as_array_mut().unwrap().remove(1);
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert_eq!(left_out, vec!["/families/Slab/dimensions/5".to_string()]);
    assert_eq!(m.families["Slab"].dimensions.len(), 7);
    // The family loaded again under another name: its parts get GUIDs of their own, and its dimensions follow them.
    let mut s = slab();
    let file = tempfile::tempdir().unwrap();
    let path = file.path().join("Slab.json");
    run(&mut s, "family", json!({"name": "Slab", "save": path.to_string_lossy()}));
    run(&mut s, "family", json!({"name": "Copy", "path": path.to_string_lossy()}));
    let copy = &s.model.families["Copy"];
    assert!(copy.dangling_dimensions().is_empty());
    assert_eq!(tris(copy, "Large"), tris(&by_hand(copy, 1400.0, 900.0), "Large"));
}
