//! F7c: angular dimensions, reference lines that turn and are moved by constraints, forms on a reference line's face measured
//! and following it, and an arc's radius and centre, solved by the non-linear least-change solve (`family/constraints.rs`).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::constraints::NOT_SATISFIED;
use crate::family::sketch::FormCurve;
use crate::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, DimRef, Expr, Family, REF_LEVEL, Spot};
use crate::ids::Guid;
use crate::model::Model;

const RIG: &str = "66666666-6666-4666-8666-000000000001";
const ARM: &str = "66666666-6666-4666-8666-000000000002";
const TWIN: &str = "66666666-6666-4666-8666-000000000003";
const FIN: &str = "77777777-7777-4777-8777-000000000001";
const FIN_BOTTOM: &str = "77777777-7777-4777-8777-000000000002";
const FIN_TOP: &str = "77777777-7777-4777-8777-000000000004";
const RIGHT: &str = "11111111-1111-4111-8111-000000000002";

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

fn lock(a: Value, b: Value) -> Value {
    json!({"view": "plan", "refs": [a, b], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true})
}

fn values(f: &Family, t: &str) -> BTreeMap<String, crate::family::Val> {
    f.values(t, &BTreeMap::new()).unwrap()
}

fn tris(f: &Family, t: &str) -> Vec<Vec<[[f64; 3]; 3]>> {
    f.pieces(&values(f, t)).unwrap().into_iter().map(|p| p.tris).collect()
}

/// Whether two sets of triangles are the same within a millionth of a millimetre.
fn same(a: &[Vec<[[f64; 3]; 3]>], b: &[Vec<[[f64; 3]; 3]>]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.len() == y.len()
                && x.iter().zip(y).all(|(p, q)| p.iter().flatten().zip(q.iter().flatten()).all(|(u, v)| (u - v).abs() < 1e-6))
        })
}

fn deg(a: f64) -> f64 {
    a.to_radians()
}

/// The classic rig: a reference line on Ref. Level from the origin, 1000 long, drawn at 30° from Center (Front/Back), its
/// start locked to both centre planes, the angle labelled `Angle`. A fin (an extrusion 50 deep) drawn on its vertical face,
/// 600 along it from 200 and `Height` tall, its top line labelled from Ref. Level in the front view.
fn the_rig() -> Value {
    let (c, s) = (deg(30.0).cos(), deg(30.0).sin());
    json!({
        "name": "Rig",
        "parameters": [{"name": "Angle", "kind": "angle", "value": 30}, {"name": "Height", "kind": "length", "value": 300}],
        "types": {"Zero": {"Angle": 0, "Height": 300}, "Thirty": {"Angle": 30, "Height": 300}, "Right": {"Angle": 90, "Height": 450}},
        "ref_lines": [{"guid": RIG, "start": [0, 0, 0], "end": [1000.0 * c, 1000.0 * s, 0]}],
        "forms": [{"guid": FIN, "host": {"line": RIG, "face": "vertical"}, "sketch": [
            {"guid": FIN_BOTTOM, "start": [200, 0], "end": [800, 0]},
            {"guid": "77777777-7777-4777-8777-000000000003", "start": [800, 0], "end": [800, 300]},
            {"guid": FIN_TOP, "start": [800, 300], "end": [200, 300]},
            {"guid": "77777777-7777-4777-8777-000000000005", "start": [200, 300], "end": [200, 0]}
        ], "from": 0, "to": 50}],
        "dimensions": [
            {"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_FRONT_BACK}, {"line": RIG, "face": "vertical"}], "at": [483, 129, 0], "label": "Angle"},
            lock(json!({"plane": CENTER_LEFT_RIGHT}), json!({"line": RIG, "point": "start"})),
            lock(json!({"plane": CENTER_FRONT_BACK}), json!({"line": RIG, "point": "start"})),
            {"view": "front", "refs": [{"plane": REF_LEVEL}, {"form": FIN, "line": FIN_BOTTOM}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true},
            {"view": "front", "refs": [{"plane": REF_LEVEL}, {"form": FIN, "line": FIN_TOP}], "at": [-300, 0, 0], "label": "Height"}
        ]
    })
}

fn rig() -> Session {
    let mut s = Session::new();
    run(&mut s, "family", the_rig());
    s
}

/// `f` without its dimensions, its line drawn by hand at `a` degrees and its fin `h` tall.
fn rig_by_hand(f: &Family, a: f64, h: f64) -> Family {
    let mut x = f.clone();
    x.dimensions.clear();
    x.lines[0].end = [1000.0 * deg(a).cos(), 1000.0 * deg(a).sin(), 0.0];
    let n = Expr::Number;
    x.forms[0].set_corners(&[[n(200.0), n(0.0)], [n(800.0), n(0.0)], [n(800.0), n(h)], [n(200.0), n(h)]]);
    x
}

#[test]
fn the_reference_line_rig_turns_its_fin_through_its_angles_as_drawn_by_hand() {
    let s = rig();
    let f = &s.model.families["Rig"];
    for (t, a, h) in [("Zero", 0.0, 300.0), ("Thirty", 30.0, 300.0), ("Right", 90.0, 450.0)] {
        assert!(same(&tris(f, t), &tris(&rig_by_hand(f, a, h), t)), "{t}: the fin as drawn by hand at {a}°");
        let flexed = f.flexed(&values(f, t)).unwrap();
        let l = &flexed.lines[0];
        let len = ((l.end[0] - l.start[0]).powi(2) + (l.end[1] - l.start[1]).powi(2)).sqrt();
        assert!((len - 1000.0).abs() < 1e-6 && l.start.iter().all(|v| v.abs() < 1e-9), "{t}: the line keeps its start and length: {l:?}");
        let m = f.measured(&values(f, t)).unwrap();
        assert!((m[0][0] - a).abs() < 1e-9, "{t}: the angle measured is {}", m[0][0]);
        assert!((m[4][0] - h).abs() < 1e-6, "{t}: the fin's height measured is {}", m[4][0]);
    }
    // Path independence: Right after Zero comes out as Right alone; nothing solved is stored.
    let right = tris(f, "Right");
    tris(f, "Zero");
    assert_eq!(tris(f, "Right"), right);
    assert_eq!(s.model.families["Rig"].lines[0].start, [0.0; 3]);
    // `solve` lists where the line went.
    let r = run(&mut rig(), "family", json!({"name": "Rig", "solve": "Right"}));
    let end = r["lines"][0]["end"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    assert!(end[0].abs() < 1e-6 && (end[1] - 1000.0).abs() < 1e-6, "{r}");
    // An angle of 180 or more, or below 0, can't stand.
    for bad in [180.0, -10.0] {
        let e = rig().execute("family", &json!({"name": "Rig", "types": {"Zero": {"Angle": bad, "Height": 300}}})).unwrap_err().to_string();
        assert!(e.contains(NOT_SATISFIED), "{bad}: {e}");
    }
}

#[test]
fn angles_that_repeat_others_are_over_constrained_and_what_cant_hold_is_refused() {
    let mut s = rig();
    // A second angle between the same two, locked: the label holds it already.
    let e = s
        .execute(
            "family",
            &json!({"name": "Rig", "dimension": {"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_FRONT_BACK}, {"line": RIG, "face": "vertical"}], "at": [400, 300, 0], "locked": [{"segment": 0, "length": 30}]}}),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("dimension 6 is over-constrained: dimension 1 hold"), "{e}");
    // The line's end held on two planes as well: its angle is set twice.
    let e = s
        .execute(
            "family",
            &json!({"name": "Rig", "ref_planes": [{"guid": RIGHT, "name": "Right", "start": [866.0254037844386, -1000, 0], "end": [866.0254037844386, 1000, 0], "up": [0, 0, 1]}],
                    "dimensions": [the_rig()["dimensions"][0], the_rig()["dimensions"][1], the_rig()["dimensions"][2],
                                   {"view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"line": RIG, "point": "end"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 866.0254037844386}]},
                                   {"view": "plan", "refs": [{"plane": CENTER_FRONT_BACK}, {"line": RIG, "point": "end"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 500}]}]}),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("over-constrained") && e.contains("dimension 5"), "{e}");
    // The end locked to a plane of its own: the line and the plane share the change, least, and the end stays on it.
    let mut v = the_rig();
    v["ref_planes"] = json!([{"guid": RIGHT, "name": "Right", "start": [866.0254037844386, -1000, 0], "end": [866.0254037844386, 1000, 0], "up": [0, 0, 1]}]);
    v["dimensions"].as_array_mut().unwrap().push(lock(json!({"plane": RIGHT}), json!({"line": RIG, "point": "end"})));
    v["types"] = json!({"Thirty": {"Angle": 30, "Height": 300}});
    let mut s = Session::new();
    run(&mut s, "family", v);
    let f = &s.model.families["Rig"];
    let mut at60 = values(f, "Thirty");
    at60.insert("Angle".into(), crate::family::Val::Number(60.0));
    let flexed = f.flexed(&at60).unwrap();
    let (l, right) = (flexed.lines[0].clone(), flexed.planes.iter().find(|p| p.guid == g(RIGHT)).unwrap().start[0]);
    assert!((l.end[0] - right).abs() < 1e-6 && (l.end[1] / l.end[0] - deg(60.0).tan()).abs() < 1e-9, "{l:?} {right}");
    assert!(right < 866.0 && right > 500.0, "both gave way: {right}");
    // The end held 866 from Center (Left/Right), a datum: at 60° the line stretches to it; at 90° no length reaches it.
    let mut v = the_rig();
    v["dimensions"].as_array_mut().unwrap().push(
        json!({"view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"line": RIG, "point": "end"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 866.0254037844386}]}),
    );
    v["types"] = json!({"Thirty": {"Angle": 30, "Height": 300}});
    let mut s = Session::new();
    run(&mut s, "family", v);
    let f = &s.model.families["Rig"];
    let l = f.flexed(&at60).unwrap().lines[0].clone();
    assert!((l.end[0] - 866.0254037844386).abs() < 1e-6 && (l.end[1] - 1500.0).abs() < 1e-6, "{l:?}");
    let e = s.execute("family", &json!({"name": "Rig", "types": {"Thirty": {"Angle": 90, "Height": 300}}})).unwrap_err().to_string();
    assert!(e.contains(NOT_SATISFIED), "{e}");
}

#[test]
fn a_fan_keeps_its_angles_equal_and_a_line_aligned_across_stays_parallel() {
    let (c40, s40, c80, s80) = (deg(40.0).cos(), deg(40.0).sin(), deg(80.0).cos(), deg(80.0).sin());
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({
            "name": "Fan",
            "parameters": [{"name": "Total", "kind": "angle", "value": 80}],
            "types": {"Narrow": {"Total": 60}, "Wide": {"Total": 120}},
            "ref_lines": [
                {"guid": ARM, "start": [0, 0, 0], "end": [1000.0 * c40, 1000.0 * s40, 0]},
                {"guid": TWIN, "start": [0, 0, 0], "end": [1000.0 * c80, 1000.0 * s80, 0]}
            ],
            "dimensions": [
                lock(json!({"plane": CENTER_LEFT_RIGHT}), json!({"line": ARM, "point": "start"})),
                lock(json!({"plane": CENTER_FRONT_BACK}), json!({"line": ARM, "point": "start"})),
                lock(json!({"plane": CENTER_LEFT_RIGHT}), json!({"line": TWIN, "point": "start"})),
                lock(json!({"plane": CENTER_FRONT_BACK}), json!({"line": TWIN, "point": "start"})),
                {"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_FRONT_BACK}, {"line": ARM, "face": "vertical"}, {"line": TWIN, "face": "vertical"}], "at": [600, 400, 0], "equal": true},
                {"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_FRONT_BACK}, {"line": TWIN, "face": "vertical"}], "at": [300, 500, 0], "label": "Total"}
            ]
        }),
    );
    let f = &s.model.families["Fan"];
    for (t, total) in [("Narrow", 60.0), ("Wide", 120.0)] {
        let m = f.measured(&values(f, t)).unwrap();
        assert!((m[4][0] - total / 2.0).abs() < 1e-9 && (m[4][1] - total / 2.0).abs() < 1e-9, "{t}: {:?}", m[4]);
        let flexed = f.flexed(&values(f, t)).unwrap();
        let twin = flexed.lines[1].end;
        assert!((twin[0] - 1000.0 * deg(total).cos()).abs() < 1e-6 && (twin[1] - 1000.0 * deg(total).sin()).abs() < 1e-6, "{t}: {twin:?}");
    }
    // A line measured across from a plane, with nothing turning it, keeps its way: it moves, never swings.
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({
            "name": "Rail",
            "parameters": [{"name": "Off", "kind": "length", "value": 300}],
            "types": {"A": {"Off": 500}},
            "ref_lines": [{"guid": ARM, "start": [300, -500, 0], "end": [300, 500, 0]}],
            "dimensions": [{"view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"line": ARM, "face": "vertical"}], "at": [0, 600, 0], "label": "Off"}]
        }),
    );
    let f = &s.model.families["Rail"];
    let l = f.flexed(&values(f, "A")).unwrap().lines[0].clone();
    assert_eq!((l.start, l.end), ([500.0, -500.0, 0.0], [500.0, 500.0, 0.0]));
}

#[test]
fn an_angle_between_two_sketch_lines_sets_the_slope_of_a_wedge() {
    const WEDGE: &str = "88888888-8888-4888-8888-000000000001";
    const FLOOR: &str = "88888888-8888-4888-8888-000000000002";
    const SLOPE: &str = "88888888-8888-4888-8888-000000000003";
    const BACKS: &str = "88888888-8888-4888-8888-000000000004";
    let t30 = 1000.0 * deg(30.0).tan();
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({
            "name": "Wedge",
            "parameters": [{"name": "Pitch", "kind": "angle", "value": 30}],
            "types": {"Low": {"Pitch": 20}, "Steep": {"Pitch": 45}},
            "ref_planes": [{"guid": RIGHT, "name": "Right", "start": [1000, -1000, 0], "end": [1000, 1000, 0], "up": [0, 0, 1]}],
            "forms": [{"guid": WEDGE, "sketch": [
                {"guid": FLOOR, "start": [0, 0], "end": [1000, 0]},
                {"guid": SLOPE, "start": [1000, 0], "end": [0, t30]},
                {"guid": BACKS, "start": [0, t30], "end": [0, 0]}
            ], "to": 100}],
            "dimensions": [
                lock(json!({"plane": CENTER_FRONT_BACK}), json!({"form": WEDGE, "line": FLOOR})),
                lock(json!({"plane": CENTER_LEFT_RIGHT}), json!({"form": WEDGE, "line": BACKS})),
                lock(json!({"plane": RIGHT}), json!({"form": WEDGE, "line": FLOOR, "point": "end"})),
                {"view": "plan", "kind": "angular", "refs": [{"form": WEDGE, "line": FLOOR}, {"form": WEDGE, "line": SLOPE}], "at": [800, 50, 0], "label": "Pitch"}
            ]
        }),
    );
    let f = &s.model.families["Wedge"];
    for (t, a) in [("Low", 20.0), ("Steep", 45.0)] {
        let flexed = f.flexed(&values(f, t)).unwrap();
        let FormCurve::Line { end, .. } = &flexed.forms[0].sketch[1].curve else { panic!("a line") };
        let y = crate::family::worth(&end[1], &values(f, t)).unwrap();
        assert!((y - 1000.0 * deg(a).tan()).abs() < 1e-6, "{t}: the top at {y}");
        let m = f.measured(&values(f, t)).unwrap()[3][0];
        assert!((m - a).abs() < 1e-6, "{t}: measures {m}");
    }
}

/// An arch: an arc from (0, 0) to (1000, 0) turning 90° (bulging below), closed by a base line back along Center
/// (Front/Back), its ends locked to Center (Left/Right) and Right.
fn arch(extra: Value, parameters: Value, types: Value) -> Session {
    const ARCH: &str = "99999999-9999-4999-8999-000000000001";
    const CURVE: &str = "99999999-9999-4999-8999-000000000002";
    const BASE: &str = "99999999-9999-4999-8999-000000000003";
    let mut dims = vec![
        lock(json!({"plane": CENTER_FRONT_BACK}), json!({"form": ARCH, "line": BASE})),
        lock(json!({"plane": CENTER_LEFT_RIGHT}), json!({"form": ARCH, "line": CURVE, "point": "start"})),
        lock(json!({"plane": RIGHT}), json!({"form": ARCH, "line": CURVE, "point": "end"})),
    ];
    let extra = extra.to_string().replace("ARCH", ARCH).replace("CURVE", CURVE);
    dims.push(serde_json::from_str(&extra).unwrap());
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({
            "name": "Arch",
            "parameters": parameters,
            "types": types,
            "ref_planes": [{"guid": RIGHT, "name": "Right", "start": [1000, -1000, 0], "end": [1000, 1000, 0], "up": [0, 0, 1]}],
            "forms": [{"guid": ARCH, "sketch": [
                {"guid": CURVE, "start": [0, 0], "end": [1000, 0], "turn": 90},
                {"guid": BASE, "start": [1000, 0], "end": [0, 0]}
            ], "to": 100}],
            "dimensions": dims
        }),
    );
    s
}

fn turn(f: &Family, t: &str) -> f64 {
    let flexed = f.flexed(&values(f, t)).unwrap();
    let FormCurve::Line { turn: Some(a), .. } = &flexed.forms[0].sketch[0].curve else { panic!("an arc") };
    crate::family::worth(a, &values(f, t)).unwrap()
}

#[test]
fn an_arcs_radius_takes_a_label_and_its_centre_a_dimension() {
    // Its radius labelled: the ends held 1000 apart, the turn solves.
    let s = arch(
        json!({"view": "plan", "kind": "radial", "refs": [{"form": "ARCH", "line": "CURVE"}], "at": [500, -800, 0], "label": "R"}),
        json!([{"name": "R", "kind": "length", "value": 707.1067811865476}]),
        json!({"Flat": {"R": 800}, "Round": {"R": 600}}),
    );
    let f = &s.model.families["Arch"];
    for (t, r) in [("Flat", 800.0), ("Round", 600.0)] {
        let want = 2.0 * (500.0_f64 / r).asin().to_degrees();
        assert!((turn(f, t) - want).abs() < 1e-6, "{t}: turns {} for {want}", turn(f, t));
        assert!((f.measured(&values(f, t)).unwrap()[3][0] - r).abs() < 1e-6);
        let mut h = f.clone();
        h.dimensions.clear();
        if let FormCurve::Line { turn, .. } = &mut h.forms[0].sketch[0].curve {
            *turn = Some(Expr::Number(want));
        }
        assert!(same(&tris(f, t), &tris(&h, t)), "{t}: as drawn by hand");
    }
    // A radius shorter than half the way between its held ends can't be.
    let e = arch(
        json!({"view": "plan", "kind": "radial", "refs": [{"form": "ARCH", "line": "CURVE"}], "at": [500, -800, 0], "label": "R"}),
        json!([{"name": "R", "kind": "length", "value": 707.1067811865476}]),
        json!({"Flat": {"R": 800}}),
    )
    .execute("family", &json!({"name": "Arch", "types": {"Flat": {"R": 400}}}))
    .unwrap_err()
    .to_string();
    assert!(e.contains(NOT_SATISFIED), "{e}");
    // Its centre measured from Center (Front/Back): the rise sets the turn.
    let s = arch(
        json!({"view": "plan", "refs": [{"plane": CENTER_FRONT_BACK}, {"form": "ARCH", "line": "CURVE", "point": "centre"}], "at": [-200, 0, 0], "label": "Rise"}),
        json!([{"name": "Rise", "kind": "length", "value": 500}]),
        json!({"Square": {"Rise": 500}, "Low": {"Rise": 200}}),
    );
    let f = &s.model.families["Arch"];
    for (t, rise) in [("Square", 500.0), ("Low", 200.0)] {
        let want = 2.0 * (500.0_f64 / rise).atan().to_degrees();
        assert!((turn(f, t) - want).abs() < 1e-6, "{t}: turns {} for {want}", turn(f, t));
    }
}

#[test]
fn a_file_keeps_its_angles_and_a_command_names_a_lines_end_by_its_id() {
    let s = rig();
    const { assert!(crate::model::FORMAT >= 28) };
    let text = s.model.to_json().unwrap();
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Rig"].dimensions, s.model.families["Rig"].dimensions);
    // A file of 26 holds none of them: read and written as it was.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["format"] = json!(26);
    v["families"]["Rig"]["dimensions"] = json!([]);
    let (old, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty() && old.families["Rig"].dimensions.is_empty());
    // A line's end given by the line's id.
    let mut s = rig();
    let id = s.model.families["Rig"].lines[0].id;
    run(
        &mut s,
        "family",
        json!({"name": "Rig", "dimension": {"view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"line": id, "point": "end"}], "at": [0, -300, 0]}}),
    );
    let d = s.model.families["Rig"].dimensions.last().cloned().unwrap();
    assert_eq!(d.refs[1], DimRef::LinePoint(g(RIG), Spot::End));
    // What an angular dimension can't take is refused.
    let refused = |refs: Value, said: &str| {
        let e = rig()
            .execute("family", &json!({"name": "Rig", "dimension": {"view": "plan", "kind": "angular", "refs": refs, "at": [100, 300, 0]}}))
            .unwrap_err()
            .to_string();
        assert!(e.contains(said), "{said}: {e}");
    };
    refused(json!([{"plane": CENTER_FRONT_BACK}, {"line": RIG, "point": "end"}]), "an angular dimension measures between");
    refused(json!([{"plane": CENTER_FRONT_BACK}, {"plane": REF_LEVEL}]), "an angular dimension measures between");
    refused(
        json!([{"plane": CENTER_LEFT_RIGHT}, {"plane": RIGHT}]),
        "no longer has", // no such plane yet
    );
    let e = rig()
        .execute(
            "family",
            &json!({"name": "Rig", "ref_planes": [{"guid": RIGHT, "start": [500, -1000, 0], "end": [500, 1000, 0], "up": [0, 0, 1]}], "dimension": {"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"plane": RIGHT}], "at": [100, 300, 0]}}),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("parallel"), "{e}");
    // A length labels no angle, an angle no length.
    let e = rig()
        .execute(
            "family",
            &json!({"name": "Rig", "dimension": {"view": "plan", "kind": "angular", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"line": RIG, "face": "vertical"}], "at": [100, 500, 0], "label": "Height"}}),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("an angular dimension's label is an angle"), "{e}");
}
