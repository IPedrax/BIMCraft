//! F7d: pivots. A reference line or a reference plane pinned to a point and turned about it by an Angle parameter on the
//! pivot, within limits; what is drawn on it, locked to it or nested on it following (T26's door); over-constraint named;
//! the file.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::sketch::FormCurve;
use crate::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, Expr, Family, PivotAt, PivotPoint};
use crate::ids::Guid;
use crate::model::Model;

const HINGE: &str = "88888888-8888-4888-8888-000000000001";
const LEAF: &str = "88888888-8888-4888-8888-000000000002";
const PANEL: &str = "88888888-8888-4888-8888-000000000003";
const SLAB: &str = "88888888-8888-4888-8888-000000000004";
const EDGE: &str = "88888888-8888-4888-8888-000000000005";
const FIN: &str = "88888888-8888-4888-8888-000000000006";

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    match s.execute(c, &p) {
        Ok(r) => panic!("{c} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

fn values(f: &Family, t: &str) -> BTreeMap<String, crate::family::Val> {
    f.values(t, &BTreeMap::new()).unwrap()
}

fn tris(f: &Family, t: &str) -> Vec<Vec<[[f64; 3]; 3]>> {
    f.pieces(&values(f, t)).unwrap().into_iter().map(|p| p.tris).collect()
}

/// Whether two builds are the same solid within a millionth of a millimetre: piece by piece, the same corners, as many
/// triangles and the same area (a build may split a face along the other diagonal where its corners differ by 1e-12).
fn same(a: &[Vec<[[f64; 3]; 3]>], b: &[Vec<[[f64; 3]; 3]>]) -> bool {
    let close = |p: &[f64; 3], q: &[f64; 3]| p.iter().zip(q).all(|(u, v)| (u - v).abs() < 1e-6);
    let area = |t: &[[f64; 3]; 3]| {
        let (u, v) = (crate::view3d::sub(t[1], t[0]), crate::view3d::sub(t[2], t[0]));
        let c = crate::view3d::cross(u, v);
        crate::view3d::dot(c, c).sqrt() / 2.0
    };
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            let corners =
                |s: &Vec<[[f64; 3]; 3]>, o: &Vec<[[f64; 3]; 3]>| s.iter().flatten().all(|p| o.iter().flatten().any(|q| close(p, q)));
            x.len() == y.len()
                && corners(x, y)
                && corners(y, x)
                && (x.iter().map(area).sum::<f64>() - y.iter().map(area).sum::<f64>()).abs() < 1e-6
        })
}

/// The hinge: a plane square to x at -450, crossing Center (Front/Back) at (-450, 0).
fn hinge() -> Value {
    json!({"guid": HINGE, "name": "Hinge", "start": [-450, -1000, 0], "end": [-450, 1000, 0], "up": [0, 0, 1]})
}

/// A door's swing as F7d draws it: a reference line on Ref. Level, drawn a little off the hinge (from (-440, 5), 890 long,
/// at 0°), pivoted at its start on where Hinge crosses Center (Front/Back), its swing `Swing Angle` from 0 to 90; a fin 300
/// tall on its vertical face, from 100 to 800 along it.
fn swing() -> Value {
    json!({
        "name": "Swing",
        "parameters": [{"name": "Swing Angle", "kind": "angle", "value": 0}],
        "types": {"Shut": {"Swing Angle": 0}, "Half": {"Swing Angle": 45}, "Open": {"Swing Angle": 90}},
        "ref_planes": [hinge()],
        "ref_lines": [{"guid": LEAF, "start": [-440, 5, 0], "end": [450, 5, 0],
                       "pivot": {"to": {"planes": [HINGE, CENTER_FRONT_BACK]}, "parameter": "Swing Angle", "min": 0, "max": 90}}],
        "forms": [{"guid": FIN, "host": {"line": LEAF, "face": "vertical"}, "profile": [[100, 0], [800, 0], [800, 300], [100, 300]], "to": 40}]
    })
}

fn session(def: Value) -> Session {
    let mut s = Session::new();
    run(&mut s, "family", def);
    s
}

/// `f` with no pivot, its line drawn by hand on the hinge at `a` degrees, 890 long.
fn swing_by_hand(f: &Family, a: f64) -> Family {
    let mut x = f.clone();
    x.lines[0].pivot = None;
    x.lines[0].start = [-450.0, 0.0, 0.0];
    x.lines[0].end = [-450.0 + 890.0 * a.to_radians().cos(), 890.0 * a.to_radians().sin(), 0.0];
    x
}

#[test]
fn a_line_pivoted_where_two_planes_cross_swings_by_its_parameter_as_drawn_by_hand() {
    let s = session(swing());
    let f = &s.model.families["Swing"];
    assert!(f.lines[0].pivot.is_some());
    for (t, a) in [("Shut", 0.0), ("Half", 45.0), ("Open", 90.0)] {
        assert!(same(&tris(f, t), &tris(&swing_by_hand(f, a), t)), "{t}: the fin as drawn by hand at {a}°");
        let flexed = f.flexed(&values(f, t)).unwrap();
        let l = &flexed.lines[0];
        assert!((l.start[0] + 450.0).abs() < 1e-6 && l.start[1].abs() < 1e-6, "{t}: on the hinge: {l:?}");
        let len = ((l.end[0] - l.start[0]).powi(2) + (l.end[1] - l.start[1]).powi(2)).sqrt();
        assert!((len - 890.0).abs() < 1e-6, "{t}: its length kept: {len}");
    }
    // Nothing solved is stored, and the order types are flexed in changes nothing.
    let open = tris(f, "Open");
    tris(f, "Shut");
    assert!(same(&tris(f, "Open"), &open));
    assert_eq!(s.model.families["Swing"].lines[0].start, [-440.0, 5.0, 0.0]);
    // A plane its crossing moves takes the hinge along: Hinge labelled 300 from Center (Left/Right)… no, from -450 to -300.
    let mut moved = session(swing());
    run(
        &mut moved,
        "family",
        json!({"name": "Swing", "parameters": [{"name": "Swing Angle", "kind": "angle", "value": 0}, {"name": "Reach", "value": 300}],
               "types": {"Half": {"Swing Angle": 45, "Reach": 300}},
               "dimension": {"view": "plan", "refs": [{"plane": HINGE}, {"plane": CENTER_LEFT_RIGHT}], "at": [0, -500, 0], "label": "Reach"}}),
    );
    let f = &moved.model.families["Swing"];
    let l = f.flexed(&values(f, "Half")).unwrap().lines[0].clone();
    assert!((l.start[0] + 300.0).abs() < 1e-6 && l.start[1].abs() < 1e-6, "the hinge followed its plane: {l:?}");
    // The grip: dragged toward 30°, it writes the parameter; past the limit it stops there; near 45 it snaps to it.
    let f = &s.model.families["Swing"];
    let at = |a: f64| [-450.0 + 500.0 * a.to_radians().cos(), 500.0 * a.to_radians().sin(), 0.0];
    let v = values(f, "Shut");
    let (to, edits) = f.drag_pivot(&v, g(LEAF), at(30.4), true).unwrap();
    assert_eq!(to, 30.0);
    assert_eq!(edits, vec![crate::family::constraints::Edit::Parameter("Swing Angle".into(), 30.0)]);
    assert_eq!(f.drag_pivot(&v, g(LEAF), at(130.0), true).unwrap().0, 90.0, "stopped at its maximum");
    assert_eq!(f.drag_pivot(&v, g(LEAF), at(-20.0), true).unwrap().0, 0.0, "stopped at its minimum");
    assert_eq!(f.drag_pivot(&v, g(LEAF), at(42.0), true).unwrap().0, 45.0, "snapped to 45");
    assert_eq!(f.drag_pivot(&values(f, "Half"), g(LEAF), at(60.0), true).unwrap().0, 60.0, "from where the type has it");
}

#[test]
fn angular_dimensions_still_turn_a_pivoted_line_and_a_pivot_with_no_parameter_keeps_its_own_angle() {
    // No parameter: its own angle, 30.
    let mut def = swing();
    let tilted = json!([-440.0 + 890.0 * 10f64.to_radians().cos(), 5.0 + 890.0 * 10f64.to_radians().sin(), 0]);
    def["ref_lines"][0]["end"] = tilted.clone();
    def["ref_lines"][0]["pivot"] = json!({"to": {"planes": [HINGE, CENTER_FRONT_BACK]}, "angle": 30, "from": {"plane": CENTER_FRONT_BACK}});
    let s = session(def.clone());
    let f = &s.model.families["Swing"];
    assert!(same(&tris(f, "Shut"), &tris(&swing_by_hand(f, 30.0), "Shut")), "its own angle turns it");
    // An angular dimension labelled from Center (Front/Back) sets the angle instead, its own angle a soft wish.
    def["parameters"] = json!([{"name": "Swing Angle", "kind": "angle", "value": 0}, {"name": "Opening", "kind": "angle", "value": 60}]);
    def["types"] = json!({"Shut": {"Opening": 60}});
    def["dimensions"] = json!([{"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_FRONT_BACK}, {"line": LEAF, "face": "vertical"}], "at": [0, 200, 0], "label": "Opening"}]);
    let s = session(def);
    let f = &s.model.families["Swing"];
    assert!(same(&tris(f, "Shut"), &tris(&swing_by_hand(f, 60.0), "Shut")), "the angular label turns it");
    let m = f.measured(&values(f, "Shut")).unwrap();
    assert!((m[0][0] - 60.0).abs() < 1e-9, "{m:?}");
    // The grip of a pivot with no parameter writes its own angle.
    let mut def = swing();
    def["ref_lines"][0]["pivot"] = json!({"to": {"planes": [HINGE, CENTER_FRONT_BACK]}});
    let s = session(def);
    let f = &s.model.families["Swing"];
    let (_, edits) = f.drag_pivot(&values(f, "Shut"), g(LEAF), [-450.0, 500.0, 0.0], true).unwrap();
    assert_eq!(edits, vec![crate::family::constraints::Edit::PivotAngle(g(LEAF), 90.0)]);
}

/// A swinging panel as a plane: Panel, drawn along x through the origin (normal -y), pivoted at its start on where Hinge
/// crosses Center (Front/Back), `Swing Angle` from 0 to 90. A slab drawn on it, and a curb on Ref. Level whose edge is locked
/// to it by Align.
fn panel() -> Value {
    json!({
        "name": "Panel",
        "parameters": [{"name": "Swing Angle", "kind": "angle", "value": 0}],
        "types": {"Shut": {"Swing Angle": 0}, "Half": {"Swing Angle": 45}, "Open": {"Swing Angle": 90}},
        "ref_planes": [hinge(), {"guid": PANEL, "name": "Panel", "start": [-450, 0, 0], "end": [450, 0, 0], "up": [0, 0, 1],
                                 "pivot": {"to": {"planes": [HINGE, CENTER_FRONT_BACK]}, "parameter": "Swing Angle", "min": 0, "max": 90}}],
        "forms": [
            {"guid": SLAB, "host": {"plane": PANEL}, "profile": [[-350, 0], [350, 0], [350, 2000], [-350, 2000]], "to": 40},
            {"guid": FIN, "sketch": [
                {"guid": EDGE, "start": [-300, 0], "end": [300, 0]},
                {"guid": "88888888-8888-4888-8888-000000000011", "start": [300, 0], "end": [300, -80]},
                {"guid": "88888888-8888-4888-8888-000000000012", "start": [300, -80], "end": [-300, -80]},
                {"guid": "88888888-8888-4888-8888-000000000013", "start": [-300, -80], "end": [-300, 0]}
            ], "to": 20}
        ],
        "dimensions": [{"view": "plan", "refs": [{"plane": PANEL}, {"form": FIN, "line": EDGE}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true}]
    })
}

/// `p` turned `a` degrees about the vertical through (-450, 0).
fn turned(p: [f64; 3], a: f64) -> [f64; 3] {
    let (c, s) = (a.to_radians().cos(), a.to_radians().sin());
    let (x, y) = (p[0] + 450.0, p[1]);
    [-450.0 + x * c - y * s, x * s + y * c, p[2]]
}

#[test]
fn a_pivoted_plane_swings_with_what_is_drawn_on_it_and_what_is_locked_to_it() {
    let s = session(panel());
    let f = &s.model.families["Panel"];
    let shut = tris(f, "Shut");
    for (t, a) in [("Shut", 0.0), ("Half", 45.0), ("Open", 90.0)] {
        // The slab drawn on it is the shut one turned by hand about the hinge.
        let by_hand: Vec<Vec<[[f64; 3]; 3]>> = vec![shut[0].iter().map(|tri| tri.map(|p| turned(p, a))).collect()];
        let now = tris(f, t);
        assert!(same(&now[..1], &by_hand), "{t}: the slab turned by hand at {a}°");
        let flexed = f.flexed(&values(f, t)).unwrap();
        let p = flexed.planes.iter().find(|p| p.guid == g(PANEL)).unwrap();
        let n = p.frame().z;
        // The curb's locked edge lies on the plane as it turns: both its ends.
        let FormCurve::Line { start, end, .. } = &flexed.forms[1].sketch[0].curve else { panic!() };
        let num = |e: &Expr| if let Expr::Number(v) = e { *v } else { panic!("{e:?}") };
        for q in [start, end] {
            let q = [num(&q[0]), num(&q[1]), 0.0];
            let off = (q[0] + 450.0) * n[0] + q[1] * n[1];
            assert!(off.abs() < 1e-6, "{t}: the curb's edge on the plane: {off}");
        }
        let along = n[0] * a.to_radians().cos() + n[1] * a.to_radians().sin();
        assert!(along.abs() < 1e-9 && n[2].abs() < 1e-9, "{t}: the plane runs at {a}°: {n:?}");
    }
    // Dragged square to itself, a pivoted plane says how it moves.
    let e = f.drag_plane(&values(f, "Shut"), g(PANEL), 200.0).unwrap_err();
    assert!(e.message.contains("pivots"), "{}", e.message);
    // Its grip writes the parameter.
    let (to, _) = f.drag_pivot(&values(f, "Shut"), g(PANEL), [-450.0, 400.0, 0.0], true).unwrap();
    assert_eq!(to, 90.0);
}

#[test]
fn values_past_a_pivots_limits_are_refused_naming_the_parameter_and_the_range() {
    let mut s = session(swing());
    let e = err(&mut s, "family", json!({"name": "Swing", "types": {"Wide": {"Swing Angle": 120}}}));
    assert!(e.contains("`Swing Angle` is 120°; reference line 1's pivot turns from 0° to 90°"), "{e}");
    // Limits that can't be: past one another, past a turn.
    let mut def = swing();
    def["ref_lines"][0]["pivot"]["min"] = json!(100);
    let e = Session::new().execute("family", &def).unwrap_err();
    assert!(e.contains("its pivot's minimum, 100°, is past its maximum, 90°"), "{e}");
    def["ref_lines"][0]["pivot"]["min"] = json!(-1e300);
    let e = Session::new().execute("family", &def).unwrap_err();
    assert!(e.contains("within ±360°"), "{e}");
    // A pivot's own angle past its limits.
    let mut def = swing();
    def["ref_lines"][0]["pivot"] = json!({"to": {"planes": [HINGE, CENTER_FRONT_BACK]}, "angle": 95, "max": 90});
    let e = Session::new().execute("family", &def).unwrap_err();
    assert!(e.contains("its angle is 95°"), "{e}");
    // Its parameter is an angle, and not a reporting one.
    let mut def = swing();
    def["parameters"] = json!([{"name": "Swing Angle", "value": 0}]);
    def["types"] = json!({"Shut": {}});
    let e = Session::new().execute("family", &def).unwrap_err();
    assert!(e.contains("a swing is an angle"), "{e}");
}

#[test]
fn pivots_that_repeat_other_constraints_are_named_as_over_constrained() {
    // Its start Align-locked to both planes as well: the pivot holds that already.
    let mut def = swing();
    let lock = |p: Value| json!({"view": "plan", "refs": [{"plane": p}, {"line": LEAF, "point": "start"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true});
    def["dimensions"] = json!([lock(json!(HINGE)), lock(json!(CENTER_FRONT_BACK))]);
    let e = Session::new().execute("family", &def).unwrap_err();
    assert!(e.contains("reference line 1's pivot is over-constrained: dimension 1"), "{e}");
    // An angle label on the same line besides its parameter.
    let mut def = swing();
    def["ref_lines"][0]["end"] = json!([-440.0 + 890.0 * 10f64.to_radians().cos(), 5.0 + 890.0 * 10f64.to_radians().sin(), 0]);
    def["parameters"] = json!([{"name": "Swing Angle", "kind": "angle", "value": 0}, {"name": "Opening", "kind": "angle", "value": 30}]);
    def["types"] = json!({"Shut": {}});
    def["dimensions"] = json!([{"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_FRONT_BACK}, {"line": LEAF, "face": "vertical"}], "at": [0, 200, 0], "label": "Opening"}]);
    let e = Session::new().execute("family", &def).unwrap_err();
    assert!(e.contains("reference line 1's pivot is over-constrained: dimension 1 hold"), "{e}");
    // A plane pivoted at a fixed point can't be moved by Align.
    let mut def = panel();
    def["ref_planes"][1]["pivot"]["to"] = json!({"fixed": [-450, 0, 0]});
    def["dimensions"] = json!([{"view": "plan", "refs": [{"plane": PANEL}, {"plane": CENTER_FRONT_BACK}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true}]);
    def["forms"] = json!([]);
    let e = Session::new().execute("family", &def).unwrap_err();
    assert!(e.contains("reference plane `Panel`'s pivot is over-constrained: dimension 1"), "{e}");
    // A pivot on itself, a cycle of pivots, planes that don't cross along its axis.
    let mut def = swing();
    def["ref_lines"][0]["pivot"]["to"] = json!({"line": LEAF, "point": "end"});
    assert!(Session::new().execute("family", &def).unwrap_err().contains("pins it to itself"));
    let mut def = swing();
    def["ref_lines"] = json!([
        {"guid": LEAF, "start": [0, 0, 0], "end": [500, 0, 0], "pivot": {"to": {"line": FIN, "point": "end"}}},
        {"guid": FIN, "start": [0, 100, 0], "end": [500, 100, 0], "pivot": {"to": {"line": LEAF, "point": "end"}}}
    ]);
    def["forms"] = json!([]);
    assert!(Session::new().execute("family", &def).unwrap_err().contains("a cycle of pivots"));
    let mut def = swing();
    def["ref_lines"][0]["pivot"]["to"] = json!({"planes": [HINGE, crate::family::REF_LEVEL]});
    assert!(Session::new().execute("family", &def).unwrap_err().contains("doesn't hold its axis"));
}

#[test]
fn a_file_keeps_its_pivots_and_a_file_of_28_reads_unchanged() {
    let mut s = session(swing());
    run(&mut s, "family", panel());
    let text = s.model.to_json().unwrap();
    assert!(
        text.contains(&format!("\"format\": {}", crate::model::FORMAT)) || text.contains(&format!("\"format\":{}", crate::model::FORMAT))
    );
    assert_eq!(crate::model::FORMAT, 31);
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Swing"].lines, s.model.families["Swing"].lines);
    assert_eq!(back.families["Panel"].planes, s.model.families["Panel"].planes);
    let pivot = back.families["Swing"].lines[0].pivot.clone().unwrap();
    assert_eq!(pivot.to, PivotPoint::Crossing(g(HINGE), CENTER_FRONT_BACK));
    assert_eq!((pivot.at, pivot.min, pivot.max), (PivotAt::Start, Some(0.0), Some(90.0)));
    // Read and written again it says the same.
    assert_eq!(back.to_json().unwrap(), text);
    // A file of 28 (no pivot) reads as it was, its families unchanged.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["format"] = json!(28);
    v["families"]["Swing"]["lines"][0].as_object_mut().unwrap().remove("pivot");
    v["families"]["Panel"]["planes"][4].as_object_mut().unwrap().remove("pivot");
    let (old, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(old.format, crate::model::FORMAT);
    let mut want = s.model.families["Swing"].clone();
    want.lines[0].pivot = None;
    assert_eq!(old.families["Swing"], want);
    // A pivot naming a plane the file hasn't is left out, said.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["families"]["Swing"]["lines"][0]["pivot"]["to"] = json!({"planes": ["88888888-8888-4888-8888-0000000000ff", CENTER_FRONT_BACK]});
    let (opened, left_out) = Model::read(&v.to_string()).unwrap();
    assert_eq!(left_out, vec!["/families/Swing/lines/0/pivot".to_string()]);
    assert!(opened.families["Swing"].lines[0].pivot.is_none());
    // Deleting the hinge plane takes the pivot off; a plane's pivot named by ids resolves to GUIDs.
    let mut d = session(swing());
    run(&mut d, "family", json!({"name": "Swing", "delete_ref_plane": [HINGE]}));
    assert!(d.model.families["Swing"].lines[0].pivot.is_none());
    let mut d = session(swing());
    let hinge_id = d.model.families["Swing"].planes[3].id;
    let line_id = d.model.families["Swing"].lines[0].id;
    run(
        &mut d,
        "family",
        json!({"name": "Swing", "ref_line": {"id": line_id, "pivot": {"to": {"planes": [hinge_id, CENTER_FRONT_BACK]}, "at": {"along": 100}}}}),
    );
    let p = d.model.families["Swing"].lines[0].pivot.clone().unwrap();
    assert_eq!((p.to, p.at), (PivotPoint::Crossing(g(HINGE), CENTER_FRONT_BACK), PivotAt::Along(100.0)));
    run(&mut d, "family", json!({"name": "Swing", "ref_line": {"id": line_id, "pivot": null}}));
    assert!(d.model.families["Swing"].lines[0].pivot.is_none());
    // A parameter renamed is renamed in the pivot.
    let mut d = session(swing());
    run(&mut d, "family", json!({"name": "Swing", "parameter": {"name": "Swing Angle", "rename": "Opening"}}));
    assert_eq!(d.model.families["Swing"].lines[0].pivot.as_ref().unwrap().parameter.as_deref(), Some("Opening"));
}

/// T26: a door whose leaf is a nested family placed on the vertical face of a hinge line, the line pivoted where Hinge
/// crosses Center (Front/Back), its swing the instance parameter `Swing Angle`, 0 to 90.
fn door(pivoted: bool, drawn_at: f64) -> Session {
    let mut s = Session::new();
    let (c, sn) = (drawn_at.to_radians().cos(), drawn_at.to_radians().sin());
    let mut line = json!({"guid": LEAF, "start": [-450, 0, 0], "end": [-450.0 + 900.0 * c, 900.0 * sn, 0]});
    if pivoted {
        line["pivot"] = json!({"to": {"planes": [HINGE, CENTER_FRONT_BACK]}, "parameter": "Swing Angle", "min": 0, "max": 90});
    }
    run(
        &mut s,
        "family",
        json!({"name": "Swing Door", "category": "door",
               "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 2100},
                              {"name": "Swing Angle", "kind": "angle", "value": 0, "instance": true}],
               "types": {"900 x 2100": {}},
               "ref_planes": [hinge()], "ref_lines": [line],
               "symbols": [{"line": [[-450, 0], [450, 0]]}]}),
    );
    run(
        &mut s,
        "family",
        json!({"name": "Swing Door", "nest": {"name": "Leaf", "definition": {"category": "generic",
               "parameters": [{"name": "W", "value": 900}, {"name": "H", "value": 2100}], "types": {"Leaf": {}},
               "forms": [{"profile": [[0, 0], ["W", 0], ["W", "H"], [0, "H"]], "to": 40}]}}}),
    );
    run(
        &mut s,
        "family",
        json!({"name": "Swing Door", "nested_instance": {"family": "Leaf", "type": "Leaf", "host": {"line": LEAF, "face": "vertical"}, "at": [0, 0]}}),
    );
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    s
}

#[test]
fn t26_a_nested_leaf_on_a_hinge_swings_by_an_instance_parameter_as_drawn_by_hand_and_in_ifc() {
    let mut pivoted = door(true, 0.0);
    let f = pivoted.model.families["Swing Door"].clone();
    let at = |a: f64| {
        let mut inst = BTreeMap::new();
        inst.insert("Swing Angle".to_string(), json!(a));
        f.values("900 x 2100", &inst).unwrap()
    };
    let leaf = |f: &Family, v: &BTreeMap<String, crate::family::Val>| f.pieces(v).unwrap().into_iter().map(|p| p.tris).collect::<Vec<_>>();
    let by_hand = door(false, 45.0);
    let hand = &by_hand.model.families["Swing Door"];
    let hand_values = hand.values("900 x 2100", &BTreeMap::new()).unwrap();
    assert!(same(&leaf(&f, &at(45.0)), &leaf(hand, &hand_values)), "the leaf at 45° as drawn by hand");
    assert!(!same(&leaf(&f, &at(0.0)), &leaf(hand, &hand_values)), "and not shut");
    // Its far corner, 900 out from the hinge at 45°.
    let far = [-450.0 + 900.0 * 45f64.to_radians().cos(), 900.0 * 45f64.to_radians().sin()];
    let reaches = leaf(&f, &at(45.0)).iter().flatten().flatten().any(|p| (p[0] - far[0]).abs() < 1e-6 && (p[1] - far[1]).abs() < 1e-6);
    assert!(reaches, "a corner of the leaf at {far:?}");
    // Placed in a wall at 45°, its IFC body is the hand-drawn door's.
    let wall = *pivoted.model.elements.keys().next_back().unwrap();
    run(&mut pivoted, "door", json!({"wall": wall, "offset": 3000, "family": "Swing Door"}));
    let door_id = *pivoted.model.elements.keys().next_back().unwrap();
    run(&mut pivoted, "modify", json!({"ids": [door_id], "params": {"Swing Angle": 45}}));
    let mut hand = door(false, 45.0);
    let wall = *hand.model.elements.keys().next_back().unwrap();
    run(&mut hand, "door", json!({"wall": wall, "offset": 3000, "family": "Swing Door"}));
    let points = |s: &Session| -> Vec<String> {
        crate::ifc::export(&s.model)
            .lines()
            .filter(|l| l.contains("IFCCARTESIANPOINTLIST3D"))
            .map(|l| l.split_once('=').map_or(l, |x| x.1).to_string())
            .collect()
    };
    assert_eq!(points(&pivoted), points(&hand), "the IFC body at 45° is the hand-drawn one");
    // Past its limits an instance's value is refused, naming them.
    let e = err(&mut pivoted, "modify", json!({"ids": [door_id], "params": {"Swing Angle": 120}}));
    assert!(e.contains("`Swing Angle` is 120°; reference line 1's pivot turns from 0° to 90°"), "{e}");
}
