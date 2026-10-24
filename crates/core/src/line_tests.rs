//! Detail lines of every shape, driven through commands as a front end drives them: made, drawn, saved, moved, split,
//! joined into a polyline and taken apart again; and sketches of circles, ellipses and splines.

use serde_json::{Value, json};

use crate::curves::{Curve, FIT_TOL};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id, Model};
use crate::render::{Drawing, Prim, View};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn line(s: &mut Session, p: Value) -> Id {
    run(s, "detail_line", p)["id"].as_u64().unwrap_or_else(|| panic!("no id"))
}

fn curve(s: &Session, id: Id) -> Curve {
    match s.model.elements.get(id) {
        Some(Element::DetailLine(l)) => l.curve.clone(),
        e => panic!("{id} is {e:?}"),
    }
}

/// The furthest any point of `part` lies from `whole`.
fn strays(part: &Curve, whole: &Curve) -> f64 {
    part.points().iter().map(|p| whole.nearest(*p).1).fold(0.0, f64::max)
}

/// The pixels the lines of `id` pass through in a plan of the lowest level.
fn drawn(m: &Model, id: Id) -> Vec<(f32, f32)> {
    let level = m.levels().first().map(|(l, _)| *l).unwrap_or_default();
    let d: Drawing = crate::render::draw(m, level, View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 800 });
    (d.prims.iter().zip(&d.owners))
        .filter(|(_, o)| **o == Some(id))
        .flat_map(|(p, _)| match p {
            Prim::Line { pts, .. } => pts.clone(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn a_detail_line_takes_every_shape_and_is_drawn_and_saved_as_that_shape() {
    let mut s = Session::new();
    let shapes = [
        (json!({"start": [0, 0], "end": [1000, 0]}), "line"),
        (json!({"start": [0, 0], "end": [2000, 0], "through": [1000, 1000]}), "arc"),
        (json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "arcs": [0, 90], "closed": false}), "polyline"),
        (json!({"centre": [0, 0], "radius": 1500}), "circle"),
        (json!({"centre": [0, 0], "radii": [2000, 800], "angle": 30}), "ellipse"),
        (json!({"centre": [0, 0], "radii": [2000, 800], "span": [0, 180]}), "partial ellipse"),
        (json!({"control": [[0, 0], [1000, 2000], [3000, 2000], [4000, 0]]}), "spline"),
        (json!({"fit": [[0, 0], [1000, 1500], [3000, -500], [4000, 0]]}), "fitted spline"),
    ];
    for (p, kind) in &shapes {
        let id = line(&mut s, p.clone());
        let c = curve(&s, id);
        assert_eq!(c.kind(), *kind, "{p}");
        // What a front end reads of it: its shape and its length.
        let got = crate::commands::element_json(&s.model, id);
        assert_eq!(got["shape"], json!(kind), "{got}");
        assert!((got["length"].as_f64().unwrap_or_default() - c.length()).abs() < 0.01, "{got}");
        // Drawn through its own points.
        let px = drawn(&s.model, id);
        assert!(px.len() >= 2, "{kind} is drawn");
    }
    // A file holds each as it was made.
    let file = s.model.to_json().unwrap_or_default();
    let (back, left) = Model::read(&file).unwrap_or_else(|e| panic!("{e}"));
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(back.elements, s.model.elements);
    // What isn't a shape is refused, saying what one is, and changes nothing.
    let before = s.model.clone();
    for (p, says) in [
        (json!({"centre": [0, 0], "radius": 0.5}), "radius"),
        (json!({"centre": [0, 0]}), "`centre` and `radius`"),
        (json!({"start": [0, 0], "end": [1000, 0], "radius": 500}), "doesn't go with"),
        (json!({"control": [[0, 0], [1000, 0]], "knots": [0, 1]}), "knots"),
        (json!({"fit": [[0, 0], [0, 0]]}), "same"),
        (json!({"points": [[0, 0], [1000, 0]], "arcs": [10, 10]}), "segments"),
    ] {
        let e = s.execute("detail_line", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(s.model.elements, before.elements);
}

#[test]
fn moved_turned_and_mirrored_a_detail_line_keeps_its_shape() {
    let mut s = Session::new();
    let e = line(&mut s, json!({"centre": [1000, 0], "radii": [2000, 800], "angle": 30, "span": [10, 200]}));
    let f = line(&mut s, json!({"fit": [[0, 0], [1000, 1500], [3000, -500]], "start_tangent": [0, 1]}));
    let before = (curve(&s, e), curve(&s, f));
    run(&mut s, "mirror", json!({"ids": [e, f], "axis": [[0, -5000], [0, 5000]], "copy": false}));
    for (id, was) in [(e, &before.0), (f, &before.1)] {
        let now = curve(&s, id);
        assert_eq!(now.kind(), was.kind());
        let flip = |p: P2| P2(-p.0, p.1);
        assert!(now.start().dist(flip(was.start())) < 0.01, "{} starts where its start went", now.kind());
        for p in was.points().iter().step_by(5) {
            assert!(now.nearest(flip(*p)).1 < 0.5, "{} mirrored: {:?} is off it", now.kind(), flip(*p));
        }
    }
    run(&mut s, "rotate", json!({"ids": [e], "angle": 90, "about": [0, 0]}));
    let turned = curve(&s, e);
    let mirrored = before.0.map(|p| P2(-p.0, p.1));
    assert!(turned.start().dist(P2(-mirrored.start().1, mirrored.start().0)) < 0.01);
}

#[test]
fn split_cuts_a_detail_line_of_any_shape_and_opens_a_closed_one() {
    let mut s = Session::new();
    // A line: two lines, the first keeping its id.
    let l = line(&mut s, json!({"start": [0, 0], "end": [4000, 0]}));
    let r = run(&mut s, "split", json!({"id": l, "at": [1000, 30]}));
    let ids: Vec<Id> = r["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    assert_eq!((ids.len(), ids[0]), (2, l));
    assert!(curve(&s, ids[0]).end().dist(P2(1000.0, 0.0)) < 1e-3 && curve(&s, ids[1]).start().dist(P2(1000.0, 0.0)) < 1e-3);
    // A spline by its control points: exactly the same curve, in two.
    let sp = line(&mut s, json!({"control": [[0, 0], [1000, 2000], [2500, -1000], [4000, 1500], [5000, 0]], "style": "Wide Lines"}));
    let whole = curve(&s, sp);
    let at = whole.at(0.4);
    let r = run(&mut s, "split", json!({"id": sp, "at": [at.0, at.1]}));
    let ids: Vec<Id> = r["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    let (a, b) = (curve(&s, ids[0]), curve(&s, ids[1]));
    assert!(strays(&a, &whole) < 1e-6 && strays(&b, &whole) < 1e-6, "exact");
    assert_eq!(field(&s, ids[1], "style"), json!("Wide Lines"), "the new part in the style of the line it came from");
    // Through points: two that still pass through points, within FIT_TOL of it.
    let fit = line(&mut s, json!({"fit": [[0, 0], [1500, 1200], [3000, -800], [4200, 900], [6000, 0]]}));
    let whole = curve(&s, fit);
    let at = whole.at(whole.domain().1 * 0.37);
    let r = run(&mut s, "split", json!({"id": fit, "at": [at.0, at.1]}));
    let ids: Vec<Id> = r["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    for id in &ids {
        let part = curve(&s, *id);
        assert_eq!(part.kind(), "fitted spline");
        assert!(strays(&part, &whole) <= FIT_TOL + crate::curves::FLAT, "{}", strays(&part, &whole));
    }
    // A circle opens where it's cut, and then splits like any other.
    let c = line(&mut s, json!({"centre": [0, 0], "radius": 1000}));
    let r = run(&mut s, "split", json!({"id": c, "at": [0, 1000]}));
    assert_eq!((r["opened"].clone(), r["ids"].as_array().map(Vec::len)), (json!(true), Some(1)));
    let r = run(&mut s, "split", json!({"id": c, "at": [0, -1000]}));
    assert_eq!(r["ids"].as_array().map(Vec::len), Some(2));
    // Off the line, or at its end, nothing; and one undo step each.
    let before = s.model.elements.clone();
    for p in [json!({"id": l, "at": [9000, 9000]}), json!({"id": l, "at": [0, 0]})] {
        assert!(s.execute("split", &p).is_err(), "{p}");
    }
    assert_eq!(s.model.elements, before);
}

fn field(s: &Session, id: Id, key: &str) -> Value {
    serde_json::to_value(s.model.elements.get(id)).map(|v| v[key].clone()).unwrap_or_default()
}

#[test]
fn lines_and_arcs_join_into_one_polyline_and_explode_back_into_them() {
    let mut s = Session::new();
    let a = line(&mut s, json!({"start": [0, 0], "end": [1000, 0], "style": "Medium Lines"}));
    let b = line(&mut s, json!({"start": [1000, 1000], "end": [1000, 0]}));
    let c = line(&mut s, json!({"start": [1000, 1000], "end": [0, 1000], "arc": 90}));
    let total: f64 = [a, b, c].iter().map(|id| curve(&s, *id).length()).sum();
    let r = run(&mut s, "join_polyline", json!({"ids": [a, b, c]}));
    assert_eq!(r["id"], json!(a));
    assert!(s.model.elements.get(b).is_none() && s.model.elements.get(c).is_none(), "the rest are gone");
    let j = curve(&s, a);
    assert_eq!((j.kind(), field(&s, a, "style")), ("polyline", json!("Medium Lines")));
    assert!((j.length() - total).abs() < 1e-6);
    assert_eq!(s.history().last().copied(), Some("Join into Polyline"), "one undo step");
    // Taken apart: a line or an arc a segment.
    let r = run(&mut s, "explode", json!({"id": a}));
    let parts: Vec<Id> = r["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts.iter().map(|id| curve(&s, *id).kind()).collect::<Vec<_>>(), ["line", "line", "arc"]);
    // What doesn't join is refused, saying why.
    let far = line(&mut s, json!({"start": [5000, 0], "end": [6000, 0]}));
    let round = line(&mut s, json!({"centre": [0, 0], "radius": 300}));
    for (ids, says) in [(json!([parts[0], far]), "chain"), (json!([parts[0], round]), "circle"), (json!([parts[0]]), "two")] {
        let e = s.execute("join_polyline", &json!({"ids": ids})).unwrap_err();
        assert!(e.contains(says), "{e}");
    }
    assert!(s.execute("explode", &json!({"id": far})).unwrap_err().contains("one piece"));
}

#[test]
fn a_floor_and_a_filled_region_are_sketched_with_circles_ellipses_and_splines() {
    let mut s = Session::new();
    // A round floor: one circle, a loop on its own.
    let f = run(&mut s, "floor", json!({"sketch": [{"centre": [0, 0], "radius": 2000}]}));
    let area = f["area_m2"].as_f64().unwrap_or_default();
    assert!((area - std::f64::consts::PI * 4.0).abs() < 0.01, "{area}");
    // A region: a rectangle with an elliptical hole and a spline closing a loop with a line.
    let r = run(
        &mut s,
        "filled_region",
        json!({"sketch": [
            {"points": [[-5000, -5000], [5000, -5000], [5000, 5000], [-5000, 5000]], "closed": true},
            {"centre": [0, 0], "radii": [2000, 1000]},
            {"control": [[6000, 0], [7000, 2000], [9000, 2000], [10000, 0]]},
            {"start": [10000, 0], "end": [6000, 0]},
        ]}),
    );
    let area = r["area_m2"].as_f64().unwrap_or_default();
    // The square less the ellipse, and the spline's loop: this Bézier encloses 4.2 m² over its chord.
    let spline = 4.2;
    let want = 100.0 - std::f64::consts::PI * 2.0 + spline;
    assert!((area - want).abs() < 0.05, "{area} vs {want}");
    // A sketch line that isn't one is named.
    let e = s.execute("floor", &json!({"sketch": [{"fit": [[0, 0], [0, 0], [100, 0]]}]})).unwrap_err();
    assert!(e.contains("isn't a line it can draw"), "{e}");
}

#[test]
fn fillet_rounds_the_corner_of_two_detail_lines_and_cuts_them_back() {
    let mut s = Session::new();
    let a = line(&mut s, json!({"start": [-3000, 0], "end": [-100, 0], "style": "Wide Lines"}));
    let b = line(&mut s, json!({"start": [0, 3000], "end": [0, 200]}));
    let r = run(&mut s, "fillet", json!({"ids": [a, b], "radius": 1000}));
    let arc = r["id"].as_u64().unwrap_or_default();
    assert_eq!(curve(&s, a), Curve::line(P2(-3000.0, 0.0), P2(-1000.0, 0.0)));
    assert!(curve(&s, b).end().dist(P2(0.0, 1000.0)) < 1e-6);
    let c = curve(&s, arc);
    assert_eq!((c.kind(), field(&s, arc, "style")), ("arc", json!("Wide Lines")));
    assert!(c.points().iter().all(|p| (p.dist(P2(-1000.0, 1000.0)) - 1000.0).abs() < 1e-6));
    assert_eq!(s.history().last().copied(), Some("Fillet Arc"));
    for (p, says) in [
        (json!({"ids": [a, b], "radius": 99999}), "fit"),
        (json!({"ids": [a, b]}), "radius"),
        (json!({"ids": [a, arc], "radius": 100}), "straight"),
        (json!({"ids": [a], "radius": 100}), "two"),
    ] {
        let e = s.execute("fillet", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
}
