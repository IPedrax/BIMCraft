//! F6: Blend, Sweep and Swept Blend, solid and void. Each builds the solid a hand-made one has (its volume and its
//! triangles), faces out, flexes with its types, saves and reads again, goes out to IFC, takes constraints on its sketches
//! on its plane (a blend's base and top, a sweep's path) and its ends, and refuses what can't be built, saying why.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::Family;
use crate::family::forms::{FormKind, mapped};
use crate::geom::P2;
use crate::model::Model;
use crate::view3d::{P3, cross, dot};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

fn line(a: [f64; 2], b: [f64; 2]) -> Value {
    json!({"start": a, "end": b})
}

fn arc(a: [f64; 2], b: [f64; 2], turn: f64) -> Value {
    json!({"start": a, "end": b, "turn": turn})
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Value> {
    vec![line([x0, y0], [x1, y0]), line([x1, y0], [x1, y1]), line([x1, y1], [x0, y1]), line([x0, y1], [x0, y0])]
}

fn sq(h: f64) -> Vec<Value> {
    rect(-h, -h, h, h)
}

/// A family "S" of `forms`, its parameter `W` 1000 (type B: 600) and `L` 2000 (type B: 3000).
fn made(forms: Value) -> Result<Session, String> {
    let mut s = Session::new();
    s.execute(
        "family",
        &json!({"name": "S", "category": "generic",
                "parameters": [{"name": "W", "value": 1000}, {"name": "L", "value": 2000}],
                "types": {"A": {}, "B": {"W": 600, "L": 3000}}, "forms": forms}),
    )?;
    Ok(s)
}

fn family(s: &Session) -> &Family {
    &s.model.families["S"]
}

/// Form `i`'s triangles for type `t`, and the volume they enclose (positive: they face out).
fn solid(f: &Family, t: &str, i: usize) -> (Vec<[P3; 3]>, f64) {
    let values = f.values(t, &BTreeMap::new()).unwrap();
    let piece = f.solid(i, &values, &mut HashMap::new()).unwrap().unwrap();
    let volume = piece.tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum();
    (piece.tris, volume)
}

/// Whether every edge of `tris` is shared by exactly two of them, once each way: a closed, consistently turned solid.
fn watertight(tris: &[[P3; 3]]) -> bool {
    let key = |p: &P3| p.map(|x| (x * 1e3).round() as i64);
    let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
    for t in tris {
        for k in 0..3 {
            *edges.entry((key(&t[k]), key(&t[(k + 1) % 3]))).or_default() += 1;
        }
    }
    edges.iter().all(|((a, b), n)| *n == 1 && edges.get(&(*b, *a)) == Some(&1))
}

fn frustum(h: f64, a1: f64, a2: f64) -> f64 {
    h / 3.0 * (a1 + a2 + (a1 * a2).sqrt())
}

#[test]
fn a_blend_of_two_squares_is_the_frustum_made_by_hand() {
    let s = made(json!([{"sketch": sq(500.0), "top": sq(250.0), "from": 0, "to": 1000}])).unwrap();
    let f = family(&s);
    assert_eq!(f.forms[0].kind(), FormKind::Blend);
    let (tris, v) = solid(f, "A", 0);
    assert!((v - frustum(1000.0, 1e6, 2.5e5)).abs() < 1e-3, "volume {v}");
    // Two caps of two triangles, four sides of two: corner to corner, as a frustum made by hand has them.
    assert_eq!(tris.len(), 12);
    assert!(watertight(&tris));
    let mut corners: Vec<[i64; 3]> = tris.iter().flatten().map(|p| p.map(|x| x.round() as i64)).collect();
    corners.sort();
    corners.dedup();
    let mut want: Vec<[i64; 3]> = [(-500, -500, 0), (500, -500, 0), (500, 500, 0), (-500, 500, 0)]
        .into_iter()
        .chain([(-250, -250, 1000), (250, -250, 1000), (250, 250, 1000), (-250, 250, 1000)])
        .map(|(x, y, z)| [x, y, z])
        .collect();
    want.sort();
    assert_eq!(corners, want);
}

#[test]
fn a_blend_flexes_with_its_types_and_twists() {
    let half = |e: &str| json!(format!("{e} / 2"));
    let base = vec![
        json!({"start": [half("-W"), half("-W")], "end": [half("W"), half("-W")]}),
        json!({"start": [half("W"), half("-W")], "end": [half("W"), half("W")]}),
        json!({"start": [half("W"), half("W")], "end": [half("-W"), half("W")]}),
        json!({"start": [half("-W"), half("W")], "end": [half("-W"), half("-W")]}),
    ];
    let mut s = made(json!([{"sketch": base, "top": sq(100.0), "from": 0, "to": "L"}])).unwrap();
    let f = family(&s);
    for (t, w, l) in [("A", 1000.0, 2000.0), ("B", 600.0, 3000.0)] {
        let (_, v) = solid(f, t, 0);
        assert!((v - frustum(l, w * w, 40_000.0)).abs() < 1e-2, "{t}: {v}");
    }
    // A quarter twist of a square on a square: the same corners, the sides now crossing round, less volume.
    let straight = solid(family(&s), "A", 0).1;
    let id = family(&s).forms[0].id;
    run(&mut s, "family", json!({"name": "S", "form": {"id": id, "twist": 1}}));
    let (tris, twisted) = solid(family(&s), "A", 0);
    assert!(watertight(&tris));
    assert!(twisted > 0.0 && twisted < straight - 1.0, "{twisted} against {straight}");
    // Twist is a blend's and a swept blend's alone.
    let e = made(json!([{"sketch": sq(100.0), "to": 100, "twist": 1}])).err().unwrap();
    assert!(e.contains("twist"), "{e}");
}

#[test]
fn a_blend_of_a_square_and_a_circle_keeps_every_corner_of_each() {
    let (a, b) = (
        vec![P2(-1.0, -1.0), P2(1.0, -1.0), P2(1.0, 1.0), P2(-1.0, 1.0)],
        (0..8).map(|k| f64::from(k) * std::f64::consts::FRAC_PI_4).map(|t| P2(t.cos(), t.sin())).collect::<Vec<_>>(),
    );
    let (ra, rb) = mapped(&a, &b, 0);
    assert_eq!(ra.len(), rb.len());
    assert!(a.iter().all(|p| ra.iter().any(|q| q.dist(*p) < 1e-9)));
    assert!(b.iter().all(|p| rb.iter().any(|q| q.dist(*p) < 1e-9)));
    let s = made(json!([{"sketch": sq(500.0), "top": [{"centre": [0, 0], "radius": 300}], "from": 0, "to": 800}])).unwrap();
    let (tris, v) = solid(family(&s), "A", 0);
    assert!(watertight(&tris));
    // Between the prism of the square and that of the circle.
    let (small, big) = (std::f64::consts::PI * 300.0 * 300.0 * 800.0 * 0.9, 1e6 * 800.0);
    assert!(v > small / 2.0 && v < big, "{v}");
}

#[test]
fn a_sweep_along_a_bent_path_is_its_profile_times_the_path() {
    // An L in plan, 2000 then 1000; a 200 by 200 profile standing on the path (x ±100, y 0 to 200).
    let path = vec![line([0.0, 0.0], [2000.0, 0.0]), line([2000.0, 0.0], [2000.0, 1000.0])];
    let s = made(json!([{"sketch": path, "sweep": {"profile": rect(-100.0, 0.0, 100.0, 200.0)}}])).unwrap();
    let f = family(&s);
    assert_eq!(f.forms[0].kind(), FormKind::Sweep);
    let (tris, v) = solid(f, "A", 0);
    assert!(watertight(&tris));
    assert!((v - 40_000.0 * 3000.0).abs() < 1.0, "volume {v}");
    // Mitred: the outer corner of the bend at (2100, -100), the inner at (1900, 100), from the floor to 200 up.
    let has = |p: [f64; 3]| tris.iter().flatten().any(|q| (0..3).all(|k| (q[k] - p[k]).abs() < 1e-6));
    for p in [[2100.0, -100.0, 0.0], [1900.0, 100.0, 200.0], [0.0, -100.0, 0.0], [2100.0, 1000.0, 200.0]] {
        assert!(has(p), "{p:?}");
    }
    // Along an arc and round a circle: the profile's area times the chords' length (its middle on the path).
    let chords = |r: f64, turn: f64| {
        let n = (turn / 360.0 * 32.0).ceil();
        n * 2.0 * r * (turn.to_radians() / n / 2.0).sin()
    };
    let s =
        made(json!([{"sketch": [arc([1000.0, 0.0], [0.0, 1000.0], 90.0)], "sweep": {"profile": rect(-50.0, -50.0, 50.0, 50.0)}}])).unwrap();
    let (tris, v) = solid(family(&s), "A", 0);
    assert!(watertight(&tris));
    assert!((v - 10_000.0 * chords(1000.0, 90.0)).abs() < 1.0, "{v}");
    let s = made(json!([{"sketch": [{"centre": [0, 0], "radius": 1000}], "sweep": {"profile": rect(-50.0, -50.0, 50.0, 50.0)}}])).unwrap();
    let (tris, v) = solid(family(&s), "A", 0);
    assert!(watertight(&tris));
    assert!((v - 10_000.0 * chords(1000.0, 360.0)).abs() < 1.0, "{v}");
}

#[test]
fn a_sweeps_profile_may_have_a_hole_and_its_path_flexes() {
    let mut profile = rect(-100.0, -100.0, 100.0, 100.0);
    profile.extend(rect(-50.0, -50.0, 50.0, 50.0));
    let s = made(json!([{"sketch": [{"start": [0, 0], "end": ["L", 0]}],
                          "sweep": {"profile": profile}}]))
    .unwrap();
    for (t, l) in [("A", 2000.0), ("B", 3000.0)] {
        let (tris, v) = solid(family(&s), t, 0);
        assert!(watertight(&tris), "{t}");
        assert!((v - 30_000.0 * l).abs() < 1.0, "{t}: {v}");
    }
}

#[test]
fn a_swept_blend_on_a_straight_line_is_a_frustum_on_its_side() {
    let s = made(json!([{"sketch": [line([0.0, 0.0], [1000.0, 0.0])],
                          "sweep": {"profile": rect(-100.0, 0.0, 100.0, 200.0), "end": rect(-50.0, 0.0, 50.0, 100.0)}}]))
    .unwrap();
    let f = family(&s);
    assert_eq!(f.forms[0].kind(), FormKind::SweptBlend);
    let (tris, v) = solid(f, "A", 0);
    assert!(watertight(&tris));
    assert_eq!(tris.len(), 12);
    assert!((v - frustum(1000.0, 40_000.0, 10_000.0)).abs() < 1e-2, "{v}");
    // Along an arc: closed, out-facing, the volume between that of its small and its big profile swept.
    let s = made(json!([{"sketch": [arc([1000.0, 0.0], [0.0, 1000.0], 90.0)],
                          "sweep": {"profile": rect(-100.0, 0.0, 100.0, 200.0), "end": [{"centre": [0, 100], "radius": 50}]}}]))
    .unwrap();
    let (tris, v) = solid(family(&s), "A", 0);
    assert!(watertight(&tris));
    assert!(v > 7_000.0 * 1500.0 && v < 40_000.0 * 1600.0, "{v}");
}

#[test]
fn void_blends_and_sweeps_cut_the_solids_before_them() {
    let s = made(json!([
        {"sketch": sq(500.0), "to": 1000},
        {"sketch": sq(200.0), "top": sq(100.0), "from": -10, "to": 1010, "void": true},
        {"sketch": [line([-600.0, 0.0], [600.0, 0.0])], "sweep": {"profile": rect(-50.0, 400.0, 50.0, 500.0)}, "void": true}
    ]))
    .unwrap();
    let f = family(&s);
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let pieces = f.pieces(&values).unwrap();
    assert_eq!(pieces.len(), 1);
    let v: f64 = pieces[0].tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum();
    // The box less the blend's hole (its part inside the box) and the sweep's groove across its middle.
    assert!(v < 1e9 - 4e7 && v > 1e9 - 1.6e8 - 1e7, "{v}");
}

#[test]
fn blends_and_sweeps_save_read_again_and_go_out_to_ifc() {
    let mut s = made(json!([
        {"sketch": sq(500.0), "top": [{"centre": [0, 0], "radius": 200}], "to": 900, "twist": 2},
        {"sketch": [line([0.0, 0.0], [1000.0, 0.0]), arc([1000.0, 0.0], [1000.0, 1000.0], 180.0)],
         "sweep": {"profile": rect(-50.0, 0.0, 50.0, 100.0)}},
        {"sketch": [line([0.0, -2000.0], [1000.0, -2000.0])], "sweep": {"profile": sq(100.0), "end": sq(50.0)}, "material": "Concrete"}
    ]))
    .unwrap();
    let text = s.model.to_json().unwrap();
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["S"].forms, s.model.families["S"].forms);
    for (k, form) in s.model.families["S"].forms.iter().enumerate() {
        assert!(form.all_lines().all(|l| !l.guid.is_nil() && l.id != 0), "form {k}: every line a part");
    }
    run(&mut s, "component", json!({"family": "S", "type": "A", "at": [0, 0]}));
    let ifc = crate::ifc::export(&s.model);
    let sets = ifc.matches("IFCTRIANGULATEDFACESET").count() + ifc.matches("IFCPOLYGONALFACESET").count();
    assert!(sets >= 3, "{sets} face sets");
    // A build of 28 must refuse it: it says 29 or later.
    const { assert!(crate::model::FORMAT >= 29) };
    let f = crate::model::FORMAT;
    assert!(text.contains(&format!("\"format\": {f}")) || text.contains(&format!("\"format\":{f}")));
}

const LEFT: &str = "00000000-0000-4000-8000-0000000000a1";
const RIGHT: &str = "00000000-0000-4000-8000-0000000000a2";
const BLEND: &str = "00000000-0000-4000-8000-0000000000b1";
const TOP_E: &str = "00000000-0000-4000-8000-0000000000c1";
const TOP_W: &str = "00000000-0000-4000-8000-0000000000c2";
const PATH: &str = "00000000-0000-4000-8000-0000000000d1";
const SWEEP: &str = "00000000-0000-4000-8000-0000000000d2";
const PROFILE: &str = "00000000-0000-4000-8000-0000000000d3";

fn plane(guid: &str, x: f64) -> Value {
    json!({"guid": guid, "start": [x, -1000, 0], "end": [x, 1000, 0], "up": [0, 0, 1]})
}

fn lock(a: Value, b: Value) -> Value {
    json!({"view": "plan", "refs": [a, b], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true})
}

#[test]
fn constraints_reach_a_blends_top_and_ends_and_a_sweeps_path() {
    // Left and Right labelled W; the blend's top's west and east lines locked to them; its second end labelled H.
    let top = vec![
        json!({"start": [-500, -100], "end": [500, -100]}),
        json!({"guid": TOP_E, "start": [500, -100], "end": [500, 100]}),
        json!({"start": [500, 100], "end": [-500, 100]}),
        json!({"guid": TOP_W, "start": [-500, 100], "end": [-500, -100]}),
    ];
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "C", "parameters": [{"name": "W", "value": 1000}, {"name": "H", "value": 700}],
        "types": {"Small": {"W": 600, "H": 400}, "Big": {"W": 1400, "H": 900}},
        "ref_planes": [plane(LEFT, -500.0), plane(RIGHT, 500.0)],
        "forms": [{"guid": BLEND, "sketch": sq(300.0), "top": top, "from": 0, "to": 700}],
        "dimensions": [
            {"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, -800, 0], "label": "W"},
            {"view": "front", "refs": [{"plane": crate::family::REF_LEVEL.to_string()}, {"form": BLEND, "end": "end"}], "at": [800, 0, 0], "label": "H"},
            {"view": "front", "refs": [{"plane": crate::family::REF_LEVEL.to_string()}, {"form": BLEND, "end": "start"}], "at": [0, 0, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true},
            lock(json!({"plane": LEFT}), json!({"form": BLEND, "line": TOP_W})),
            lock(json!({"plane": RIGHT}), json!({"form": BLEND, "line": TOP_E}))
        ]}),
    );
    let f = &s.model.families["C"];
    for (t, w, h) in [("Small", 600.0, 400.0), ("Big", 1400.0, 900.0)] {
        let (tris, v) = solid_of(f, t);
        // The top follows the planes, the base (300 square, its corners at the top's xy in no type) stays.
        let top_xs: Vec<f64> = tris.iter().flatten().filter(|p| (p[2] - h).abs() < 1e-6).map(|p| p[0]).collect();
        assert!(top_xs.iter().any(|x| (x - w / 2.0).abs() < 1e-6) && top_xs.iter().any(|x| (x + w / 2.0).abs() < 1e-6), "{t}: {top_xs:?}");
        let base_xs: Vec<f64> = tris.iter().flatten().filter(|p| p[2].abs() < 1e-6).map(|p| p[0].abs()).collect();
        assert!(base_xs.iter().all(|x| (x - 300.0).abs() < 1e-6), "{t}: {base_xs:?}");
        assert!(v > 0.0);
    }
    // A sweep's path line locked to a plane follows it; a dimension to its profile's line is refused (F6d).
    let mut s = Session::new();
    let fam = |dims: Value| {
        json!({"name": "P", "parameters": [{"name": "W", "value": 1000}], "types": {"Small": {"W": 600}, "Big": {"W": 1400}},
               "ref_planes": [plane(LEFT, -500.0), plane(RIGHT, 500.0)],
               "forms": [{"guid": SWEEP, "sketch": [{"guid": PATH, "start": [-500, -400], "end": [-500, 400]}],
                          "sweep": {"profile": [{"guid": PROFILE, "start": [-50, 0], "end": [50, 0]}, {"start": [50, 0], "end": [50, 100]},
                                                {"start": [50, 100], "end": [-50, 100]}, {"start": [-50, 100], "end": [-50, 0]}]}}],
               "dimensions": dims})
    };
    run(
        &mut s,
        "family",
        fam(json!([{"view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, -800, 0], "label": "W"},
                   lock(json!({"plane": LEFT}), json!({"form": SWEEP, "line": PATH}))])),
    );
    for (t, w) in [("Small", 600.0), ("Big", 1400.0)] {
        let (tris, _) = solid_of(&s.model.families["P"], t);
        let xs: Vec<f64> = tris.iter().flatten().map(|p| p[0]).collect();
        let mid = (xs.iter().copied().fold(f64::INFINITY, f64::min) + xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)) / 2.0;
        assert!((mid + w / 2.0).abs() < 1e-6, "{t}: the path at {mid}");
    }
    let e = err(&mut Session::new(), "family", fam(json!([lock(json!({"plane": LEFT}), json!({"form": SWEEP, "line": PROFILE}))])));
    assert!(e.contains("profile line"), "{e}");
}

/// The first piece of `f` as type `t` builds it, flexed by its constraints, and its volume.
fn solid_of(f: &Family, t: &str) -> (Vec<[P3; 3]>, f64) {
    let values = f.values(t, &BTreeMap::new()).unwrap();
    let tris = f.pieces(&values).unwrap().swap_remove(0).tris;
    let volume = tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum();
    (tris, volume)
}

#[test]
fn what_cant_be_blended_or_swept_is_refused_saying_why() {
    let two = [rect(-400.0, -400.0, -100.0, 400.0), rect(100.0, -400.0, 400.0, 400.0)].concat();
    let cases: Vec<(Value, &str)> = vec![
        // A blend's top of two loops.
        (json!([{"sketch": sq(500.0), "top": two, "to": 100}]), "loop"),
        // Ends at one place.
        (json!([{"sketch": sq(500.0), "top": sq(100.0), "from": 50, "to": 50}]), "one place"),
        // A top and a sweep at once; a sweep with an axis.
        (json!([{"sketch": sq(500.0), "top": sq(100.0), "sweep": {"profile": sq(10.0)}, "to": 100}]), "one of them"),
        // A path that branches, one in pieces, one that crosses itself, a line of no length.
        (
            json!([{"sketch": [line([0.0, 0.0], [100.0, 0.0]), line([100.0, 0.0], [200.0, 0.0]), line([100.0, 0.0], [100.0, 100.0])], "sweep": {"profile": sq(10.0)}}]),
            "branches",
        ),
        (json!([{"sketch": [line([0.0, 0.0], [100.0, 0.0]), line([300.0, 0.0], [400.0, 0.0])], "sweep": {"profile": sq(10.0)}}]), "pieces"),
        (
            json!([{"sketch": [line([0.0, 0.0], [1000.0, 0.0]), line([1000.0, 0.0], [1000.0, 500.0]), line([1000.0, 500.0], [500.0, -500.0])], "sweep": {"profile": sq(10.0)}}]),
            "crosses itself",
        ),
        (json!([{"sketch": [line([0.0, 0.0], [0.0, 0.0])], "sweep": {"profile": sq(10.0)}}]), "no length"),
        // A profile too big for the bend; a hairpin.
        (json!([{"sketch": [arc([100.0, 0.0], [0.0, 100.0], 90.0)], "sweep": {"profile": sq(150.0)}}]), "too big"),
        (
            json!([{"sketch": [line([0.0, 0.0], [1000.0, 0.0]), line([1000.0, 0.0], [0.0, 1.0])], "sweep": {"profile": sq(10.0)}}]),
            "turns back",
        ),
        // A profile that crosses itself, none at all.
        (
            json!([{"sketch": [line([0.0, 0.0], [1000.0, 0.0])], "sweep": {"profile": [line([0.0, 0.0], [100.0, 100.0]), line([100.0, 100.0], [100.0, 0.0]), line([100.0, 0.0], [0.0, 100.0]), line([0.0, 100.0], [0.0, 0.0])]}}]),
            "intersect",
        ),
        (json!([{"sketch": [line([0.0, 0.0], [1000.0, 0.0])], "sweep": {"profile": []}}]), "no lines"),
        // A swept blend along two lines.
        (
            json!([{"sketch": [line([0.0, 0.0], [100.0, 0.0]), line([100.0, 0.0], [100.0, 100.0])], "sweep": {"profile": sq(10.0), "end": sq(5.0)}}]),
            "one line or one arc",
        ),
        // A number that isn't one.
        (json!([{"sketch": [{"start": [0, 0], "end": ["W / 0", 0]}], "sweep": {"profile": sq(10.0)}}]), "number"),
        (json!([{"sketch": sq(500.0), "top": sq(100.0), "to": 1e300}]), "usable"),
        (json!([{"sketch": sq(500.0), "top": sq(100.0), "to": 100, "twist": 99999}]), "twist"),
    ];
    for (forms, says) in cases {
        let e = made(forms.clone()).err().unwrap_or_else(|| panic!("{forms} should have been refused"));
        assert!(e.to_lowercase().contains(says), "{forms}: {e}");
    }
}

#[test]
fn a_sweep_on_a_level_plane_shows_in_plan_as_the_band_it_covers() {
    // The L of 2000 and 1000, 200 wide: its band, mitred at the bend, is 200 by the path's length.
    let path = vec![line([0.0, 0.0], [2000.0, 0.0]), line([2000.0, 0.0], [2000.0, 1000.0])];
    let s = made(json!([{"sketch": path, "sweep": {"profile": rect(-100.0, 0.0, 100.0, 200.0)}}])).unwrap();
    let f = family(&s);
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let piece = f.pieces(&values).unwrap().swap_remove(0);
    assert!((crate::geom::area(&piece.outline) - 200.0 * 3000.0).abs() < 1e-6, "{:?}", piece.outline);
    assert!(piece.holes.is_empty());
    // Round a circle: a ring, its hole the inside of the band.
    let s = made(json!([{"sketch": [{"centre": [0, 0], "radius": 1000}], "sweep": {"profile": rect(-50.0, 0.0, 50.0, 100.0)}}])).unwrap();
    let f = family(&s);
    let values = f.values("A", &BTreeMap::new()).unwrap();
    let piece = f.pieces(&values).unwrap().swap_remove(0);
    assert_eq!(piece.holes.len(), 1);
    let (outer, inner) = (crate::geom::area(&piece.outline), crate::geom::area(&piece.holes[0]));
    assert!(outer > 0.0 && inner < 0.0 && outer > -inner, "{outer} {inner}");
    // Placed in a project, its plan draws both rings.
    let mut s = s;
    run(&mut s, "component", json!({"family": "S", "type": "A", "at": [0, 0]}));
    let placed = s.model.component_pieces(
        s.model
            .elements
            .iter()
            .find_map(|(_, e)| match e {
                crate::model::Element::Component(c) => Some(c),
                _ => None,
            })
            .unwrap(),
    );
    assert_eq!(placed[0].holes.len(), 1);
}

#[test]
fn a_square_to_circle_swept_blend_along_an_arc_never_folds() {
    // A quarter arc about the origin, radius 1000, counter-clockwise: at angle θ the path is at 1000·(cos θ, sin θ), runs
    // along (-sin θ, cos θ), and a profile's x (to the right looking along it) is outward, (cos θ, sin θ). The square
    // start stands on the path (x ±200, y 0 to 400), the round end low in it (centre at y 100, radius 100).
    let chords = 8.0 * 2.0 * 1000.0 * (std::f64::consts::FRAC_PI_2 / 8.0 / 2.0).sin();
    for twist in [0, 1, -2, 5] {
        let s = made(json!([{"sketch": [arc([1000.0, 0.0], [0.0, 1000.0], 90.0)], "twist": twist,
                              "sweep": {"profile": rect(-200.0, 0.0, 200.0, 400.0), "end": [{"centre": [0, 100], "radius": 100}]}}]))
        .unwrap();
        let (tris, v) = solid(family(&s), "A", 0);
        assert!(watertight(&tris), "twist {twist}");
        let (round, square) = (std::f64::consts::PI * 100.0 * 100.0 * chords, 160_000.0 * chords);
        assert!(v > round && v < square, "twist {twist}: {v} between {round} and {square}");
        // Every side triangle faces away from the middle of the profile where it is (the square's middle turning into the
        // circle's along the path): a fold or a groove would face some back in.
        let mut sides = 0;
        for t in &tris {
            let n = cross(std::array::from_fn(|k| t[1][k] - t[0][k]), std::array::from_fn(|k| t[2][k] - t[0][k]));
            let len = dot(n, n).sqrt();
            if len < 1e-6 {
                continue;
            }
            let n = n.map(|x| x / len);
            let c: P3 = std::array::from_fn(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0);
            let theta = c[1].atan2(c[0]).clamp(0.0, std::f64::consts::FRAC_PI_2);
            let (sin, cos) = theta.sin_cos();
            let way = [-sin, cos, 0.0];
            if dot(n, way).abs() > 0.9 {
                continue; // a cap
            }
            let f = theta / std::f64::consts::FRAC_PI_2;
            let (mx, my) = (0.0, 200.0 + (100.0 - 200.0) * f);
            let middle = [(1000.0 + mx) * cos, (1000.0 + mx) * sin, my];
            let out: P3 = std::array::from_fn(|k| c[k] - middle[k]);
            assert!(dot(n, out) > 0.0, "twist {twist}: a side triangle at {c:?} faces in, {n:?}");
            sides += 1;
        }
        assert!(sides > 100, "{sides}");
    }
}
