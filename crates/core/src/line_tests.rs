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

#[test]
fn detail_lines_trim_and_extend_to_a_corner_or_to_a_boundary() {
    let mut s = Session::new();
    let a = line(&mut s, json!({"start": [0, 0], "end": [800, 0]}));
    let b = line(&mut s, json!({"start": [1000, 200], "end": [1000, 1000]}));
    run(&mut s, "corner", json!({"ids": [a, b], "keep": [[100, 0], [1000, 900]]}));
    assert_eq!(
        (curve(&s, a), curve(&s, b)),
        (Curve::line(P2(0.0, 0.0), P2(1000.0, 0.0)), Curve::line(P2(1000.0, 0.0), P2(1000.0, 1000.0)))
    );
    // Several, to one boundary: a wall's location line.
    let w = run(&mut s, "wall", json!({"points": [[0, 3000], [5000, 3000]]}))["ids"][0].as_u64().unwrap_or_default();
    let c1 = line(&mut s, json!({"start": [2000, 1500], "end": [2000, 2500]}));
    let c2 = line(&mut s, json!({"start": [3000, 4000], "end": [3000, 2000], "style": "Wide Lines"}));
    run(&mut s, "trim_extend", json!({"boundary": w, "ids": [c1, c2], "keep": [[2000, 1600], [3000, 2100]]}));
    assert_eq!(curve(&s, c1), Curve::line(P2(2000.0, 1500.0), P2(2000.0, 3000.0)), "extended to the wall");
    assert_eq!(curve(&s, c2), Curve::line(P2(3000.0, 3000.0), P2(3000.0, 2000.0)), "trimmed back to it, keeping the part picked");
    for (p, says) in [
        (json!({"boundary": w, "ids": [c1], "keep": []}), "keep"),
        (json!({"ids": [c1], "keep": [[0, 0]]}), "boundary"),
        (json!({"boundary": w, "ids": [w], "keep": [[0, 0]]}), "not a detail line"),
    ] {
        let e = s.execute("trim_extend", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn offset_draws_a_detail_line_again_beside_itself_arcs_round_their_centres() {
    let mut s = Session::new();
    let a = line(&mut s, json!({"start": [0, 0], "end": [2000, 0], "arc": -180}));
    let r = run(&mut s, "offset", json!({"id": a, "distance": 200, "toward": [1000, 0]}));
    let copy = curve(&s, r["id"].as_u64().unwrap_or_default());
    assert!(copy.points().iter().all(|p| (p.dist(P2(1000.0, 0.0)) - 800.0).abs() < 1e-3), "inside, round the same centre");
    assert_eq!(curve(&s, a).kind(), "arc", "the original stays");
    let l = line(&mut s, json!({"start": [0, 5000], "end": [1000, 5000]}));
    run(&mut s, "offset", json!({"id": l, "distance": 300, "toward": [500, 4000], "copy": false}));
    assert_eq!(curve(&s, l), Curve::line(P2(0.0, 4700.0), P2(1000.0, 4700.0)), "moved");
    let e = line(&mut s, json!({"centre": [0, 0], "radii": [500, 300]}));
    assert!(s.execute("offset", &json!({"id": e, "distance": 10, "toward": [0, 0]})).unwrap_err().contains("ellipse"));
}

#[test]
fn points_snap_to_detail_lines_ends_centres_quadrants_tangents_and_their_nearest_point() {
    use crate::snap::{Kind, Reach, Snaps, snap};
    let mut s = Session::new();
    line(&mut s, json!({"centre": [0, 0], "radius": 1000}));
    line(&mut s, json!({"start": [3000, 0], "end": [5000, 0], "arc": 90}));
    line(&mut s, json!({"fit": [[0, 3000], [1000, 3500], [2000, 3000]]}));
    let level = s.model.levels().first().map(|(l, _)| *l).unwrap_or_default();
    let reach = Reach { point: 150.0, line: 150.0 };
    let at = |p: P2, from: &[P2], o: &Snaps| snap(&s.model, level, p, reach, from, false, o);
    let all = Snaps::default();
    assert_eq!((at(P2(40.0, -30.0), &[], &all).at, at(P2(40.0, -30.0), &[], &all).kind), (P2(0.0, 0.0), Kind::Centre));
    assert_eq!(at(P2(20.0, 1060.0), &[], &all).kind, Kind::Quadrant);
    assert_eq!(at(P2(20.0, 1060.0), &[], &all).at, P2(0.0, 1000.0));
    assert_eq!((at(P2(3050.0, 20.0), &[], &all).at, at(P2(3050.0, 20.0), &[], &all).kind), (P2(3000.0, 0.0), Kind::Endpoint));
    assert_eq!(at(P2(1990.0, 2950.0), &[], &all).at, P2(2000.0, 3000.0), "a spline's end");
    // From (2000, 0), a line touches the circle at 60° either side of the way back to its centre.
    let t = at(P2(500.0, 880.0), &[P2(2000.0, 0.0)], &all);
    assert_eq!(t.kind, Kind::Tangent);
    assert!(t.at.dist(P2(500.0, 866.025)) < 0.01, "{:?}", t.at);
    // Nothing else near: the nearest point of the line.
    let n = at(P2(707.0, -700.0), &[], &Snaps::only(Kind::Nearest));
    assert_eq!(n.kind, Kind::Nearest);
    assert!((n.at.dist(P2(0.0, 0.0)) - 1000.0).abs() < 0.01, "on the circle: {:?}", n.at);
}

#[test]
fn fillet_rounds_two_walls_with_an_arc_wall_of_the_first_ones_type() {
    let mut s = Session::new();
    let a = run(&mut s, "wall", json!({"points": [[-4000, 0], [0, 0]]}))["ids"][0].as_u64().unwrap_or_default();
    let b = run(&mut s, "wall", json!({"points": [[0, 0], [0, 4000]]}))["ids"][0].as_u64().unwrap_or_default();
    let r = run(&mut s, "fillet", json!({"ids": [a, b], "radius": 1500}));
    let round = r["id"].as_u64().unwrap_or_default();
    let (wa, wb, wr) = (s.model.wall(a).cloned(), s.model.wall(b).cloned(), s.model.wall(round).cloned());
    let (Some(wa), Some(wb), Some(wr)) = (wa, wb, wr) else { panic!("three walls") };
    assert_eq!((wa.start, wa.end), (P2(-4000.0, 0.0), P2(-1500.0, 0.0)));
    assert_eq!((wb.start, wb.end), (P2(0.0, 1500.0), P2(0.0, 4000.0)));
    assert_eq!((wr.start, wr.end, wr.arc.abs(), wr.type_name.clone()), (P2(-1500.0, 0.0), P2(0.0, 1500.0), 90.0, wa.type_name));
}

#[test]
fn dimensions_measure_to_straight_detail_lines_and_radial_ones_to_their_circles_and_arcs() {
    let mut s = Session::new();
    let a = line(&mut s, json!({"start": [0, 0], "end": [0, 3000]}));
    let b = line(&mut s, json!({"start": [2500, 0], "end": [2500, 3000]}));
    let d = run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": b}], "at": [1000, 4000]}));
    let id = d["id"].as_u64().unwrap_or_default();
    let shown = crate::commands::element_json(&s.model, id);
    assert!(shown.to_string().contains("2500"), "{shown}");
    // Its ends too: from the end of one to the end of the other, across.
    let c = line(&mut s, json!({"start": [0, 5000], "end": [1000, 5000]}));
    let e = line(&mut s, json!({"start": [4000, 5000], "end": [4000, 6000]}));
    run(&mut s, "dimension", json!({"refs": [{"id": c, "face": "end"}, {"id": e}], "at": [2000, 5500]}));
    let round = line(&mut s, json!({"centre": [8000, 0], "radius": 1200}));
    run(&mut s, "dimension", json!({"refs": [{"id": round}], "at": [9500, 0], "kind": "radial"}));
    let spline = line(&mut s, json!({"control": [[0, 9000], [1000, 9500], [2000, 9000]]}));
    let err = s.execute("dimension", &json!({"refs": [{"id": spline}, {"id": a}], "at": [0, 0]})).unwrap_err();
    assert!(err.contains("spline"), "{err}");
}

#[test]
fn insulation_is_a_detail_line_drawn_as_batt_insulation_its_width_across() {
    let mut s = Session::new();
    let id = line(&mut s, json!({"start": [0, 0], "end": [3000, 0], "insulation": 200}));
    let Some(Element::DetailLine(l)) = s.model.elements.get(id).cloned() else { panic!("a line") };
    let drawn = l.drawn();
    let (lo, hi) = drawn.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    assert!((lo + 100.0).abs() < 1.0 && (hi - 100.0).abs() < 1.0, "the whole width across: {lo}..{hi}");
    assert!(drawn.len() > 100, "loop after loop");
    // Drawn in the plan as its loops, not its line.
    assert!(drawn_px_rows(&s.model, id) > 3);
    let (back, _) = Model::read(&s.model.to_json().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.elements, s.model.elements, "saved with its width");
    for w in [0.0, -5.0, 1.0e9] {
        assert!(s.execute("detail_line", &json!({"start": [0, 0], "end": [1000, 0], "insulation": w})).is_err(), "{w}");
    }
}

/// How many pixel rows the lines of `id` reach in a plan of the lowest level.
fn drawn_px_rows(m: &Model, id: Id) -> usize {
    let mut rows: Vec<i64> = drawn(m, id).iter().map(|p| p.1.round() as i64).collect();
    rows.sort_unstable();
    rows.dedup();
    rows.len()
}

#[test]
fn a_filled_regions_boundary_lines_each_take_a_line_style_or_none() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "filled_region",
        json!({"sketch": [
            {"start": [0, 0], "end": [2000, 0], "style": "Wide Lines"},
            {"start": [2000, 0], "end": [2000, 2000], "invisible": true},
            {"start": [2000, 2000], "end": [0, 2000]},
            {"start": [0, 2000], "end": [0, 0]},
        ], "line_style": "Thin Lines"}),
    );
    let id = r["id"].as_u64().unwrap_or_default();
    // Its lines' pens: one Wide, two Thin, and none for the invisible one.
    let level = s.model.levels().first().map(|(l, _)| *l).unwrap_or_default();
    let d = crate::render::draw(&s.model, level, View { scale: 0.1, cx: 1000.0, cy: 1000.0, width: 800, height: 800 });
    let pens: Vec<u8> = (d.prims.iter().zip(&d.owners).zip(&d.pens))
        .filter(|((p, o), _)| **o == Some(id) && matches!(p, Prim::Line { .. }))
        .filter_map(|(_, pen)| pen.map(|p| p.0))
        .collect();
    let wide = s.model.line_styles.get("Wide Lines").map(|st| st.pen.0).unwrap_or_default();
    assert_eq!(pens.iter().filter(|p| **p == wide).count(), 1, "{pens:?}");
    assert_eq!(pens.len(), 3, "the invisible one isn't drawn: {pens:?}");
    // A rename follows into the line that names it, and a file holds it by GUID.
    run(&mut s, "line_style", json!({"name": "Wide Lines", "rename": "Heavy"}));
    let file = s.model.to_json().unwrap_or_default();
    assert!(!file.contains("\"style\": \"Heavy\""), "by GUID");
    let (back, _) = Model::read(&file).unwrap_or_else(|e| panic!("{e}"));
    let Some(Element::FilledRegion(fr)) = back.elements.get(id) else { panic!("region") };
    assert_eq!((fr.sketch[0].style.as_deref(), fr.sketch[1].invisible), (Some("Heavy"), true));
    let e = s.execute("filled_region", &json!({"sketch": [{"centre": [0, 0], "radius": 500, "style": "Nope"}]})).unwrap_err();
    assert!(e.contains("Nope"), "{e}");
}

#[test]
fn vertices_are_added_and_deleted_and_splines_converted_by_command() {
    let mut s = Session::new();
    let pl = line(&mut s, json!({"points": [[0, 0], [2000, 0], [2000, 2000]]}));
    run(&mut s, "vertex", json!({"id": pl, "at": [1000, 20]}));
    assert_eq!(curve(&s, pl).vertices().len(), 4);
    run(&mut s, "vertex", json!({"id": pl, "at": [2000, 0], "delete": true}));
    assert_eq!(curve(&s, pl).vertices(), vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(2000.0, 2000.0)]);
    let fit = line(&mut s, json!({"fit": [[0, 0], [1000, 800], [2500, -300], [4000, 500]]}));
    let before = curve(&s, fit);
    let r = run(&mut s, "convert_spline", json!({"id": fit}));
    assert_eq!(r["shape"], json!("spline"));
    let after = curve(&s, fit);
    assert!(before.points().iter().all(|p| after.nearest(*p).1 < 1e-3), "the same curve");
    let circle = line(&mut s, json!({"centre": [0, 0], "radius": 500}));
    for (cmd, p, says) in [
        ("vertex", json!({"id": circle, "at": [500, 0]}), "only polylines and splines"),
        ("convert_spline", json!({"id": circle}), "not a spline"),
        ("vertex", json!({"id": pl, "at": [0, 0], "delete": true}), ""),
    ] {
        let e = s.execute(cmd, &p);
        if !says.is_empty() {
            assert!(e.unwrap_err().contains(says));
        }
    }
}
