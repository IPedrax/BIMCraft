//! A family's planes, reference lines and forms rotated, mirrored and scaled (`family/constraints/transform.rs`).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::constraints::{NOT_SATISFIED, Named, Stays, Transform, TransformRefused, Transforming};
use crate::family::sketch::FormCurve;
use crate::family::{CENTER_LEFT_RIGHT, Expr, Family};
use crate::ids::Guid;

const EDGE: &str = "fe800000-0000-4000-8000-000000000001";
const SIDE: &str = "fe800000-0000-4000-8000-000000000002";
const LINE: &str = "fe800000-0000-4000-8000-000000000003";
const BOX: &str = "fe800000-0000-4000-8000-000000000004";
const WING: &str = "fe800000-0000-4000-8000-000000000005";
const EAST: &str = "fe800000-0000-4000-8000-000000000101";

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

/// A box on Ref. Level (100 to 500 square, 300 high); a plane Edge at x 1000 across the plan; a plane Side upright facing
/// south at y -2000 with a wing drawn on it; a reference line on Ref. Level; `more` added to the family's JSON.
fn family(more: Value) -> Family {
    let mut f = json!({
        "name": "T", "parameters": [{"name": "Len", "kind": "length", "value": 400}], "types": {"A": {}},
        "ref_planes": [
            {"guid": EDGE, "name": "Edge", "start": [1000, -1500, 0], "end": [1000, 1500, 0], "up": [0, 0, 1]},
            {"guid": SIDE, "name": "Side", "start": [-1000, -2000, 0], "end": [1000, -2000, 0], "up": [0, 0, 1]}
        ],
        "ref_lines": [{"guid": LINE, "start": [-200, 1200, 0], "end": [200, 1200, 0]}],
        "forms": [
            {"guid": BOX, "sketch": [
                {"guid": "fe800000-0000-4000-8000-000000000100", "start": [100, 100], "end": [500, 100]},
                {"guid": EAST, "start": [500, 100], "end": [500, 500]},
                {"guid": "fe800000-0000-4000-8000-000000000102", "start": [500, 500], "end": [100, 500]},
                {"guid": "fe800000-0000-4000-8000-000000000103", "start": [100, 500], "end": [100, 100]}
            ], "from": 0, "to": 300},
            {"guid": WING, "host": {"plane": SIDE}, "profile": [[0, 0], [200, 0], [200, 100], [0, 100]], "from": 0, "to": 50}
        ]
    });
    if let (Some(o), Value::Object(m)) = (f.as_object_mut(), more) {
        o.extend(m);
    }
    let mut s = Session::new();
    s.execute("family", &f).unwrap_or_else(|e| panic!("{e}"));
    s.model.families["T"].clone()
}

fn values(f: &Family) -> BTreeMap<String, crate::family::Val> {
    f.values("A", &BTreeMap::new()).unwrap()
}

/// Form `i`'s sketch's points, as numbers.
fn points(f: &Family, i: usize) -> Vec<[f64; 2]> {
    let v = |e: &Expr| match e {
        Expr::Number(x) => *x,
        _ => f64::NAN,
    };
    f.forms[i]
        .sketch
        .iter()
        .flat_map(|l| match &l.curve {
            FormCurve::Line { start, end, .. } => vec![[v(&start[0]), v(&start[1])], [v(&end[0]), v(&end[1])]],
            FormCurve::Circle { centre, .. } => vec![[v(&centre[0]), v(&centre[1])]],
        })
        .collect()
}

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

/// Every type still solves and builds.
fn builds(f: &Family) {
    let v = values(f);
    f.solve(&v).unwrap();
    f.pieces(&v).unwrap();
}

const UP: [f64; 3] = [0.0, 0.0, 1.0];

#[test]
fn a_form_rotated_in_plan_turns_about_the_centre_and_still_builds() {
    let f = family(json!({}));
    let quarter = Transform::Rotate { centre: [0.0; 3], axis: UP, degrees: 90.0 };
    let t = f.transformed(&values(&f), &Transforming { forms: vec![0], ..Default::default() }, &quarter).unwrap();
    // (x, y) to (−y, x), each a number again.
    for (a, b) in points(&f, 0).into_iter().zip(points(&t.family, 0)) {
        assert!(same(b, [-a[1], a[0]]), "{a:?} → {b:?}");
    }
    assert_eq!(t.forms, vec![0]);
    assert_eq!(t.family.forms[0].guid, g(BOX));
    assert_eq!(t.family.forms[0].from, Expr::Number(0.0));
    assert_eq!(t.family.forms[0].to, Expr::Number(300.0));
    builds(&t.family);
    // Nothing else changed.
    assert_eq!(t.family.forms[1], f.forms[1]);
    assert_eq!(t.family.planes, f.planes);
}

#[test]
fn a_mirror_reflects_a_form_and_turns_its_arcs_the_other_way() {
    let mut f = family(json!({}));
    // One side of the box an arc.
    if let FormCurve::Line { turn, .. } = &mut f.forms[0].sketch[1].curve {
        *turn = Some(Expr::Number(60.0));
    }
    builds(&f);
    let m = Transform::Mirror { point: [0.0; 3], normal: [1.0, 0.0, 0.0] };
    let t = f.transformed(&values(&f), &Transforming { forms: vec![0], ..Default::default() }, &m).unwrap();
    for (a, b) in points(&f, 0).into_iter().zip(points(&t.family, 0)) {
        assert!(same(b, [-a[0], a[1]]), "{a:?} → {b:?}");
    }
    assert!(matches!(&t.family.forms[0].sketch[1].curve, FormCurve::Line { turn: Some(Expr::Number(x)), .. } if *x == -60.0));
    builds(&t.family);
}

#[test]
fn a_plane_mirrored_keeps_its_normal_the_mirror_image_and_what_is_on_it_follows() {
    let f = family(json!({}));
    let side = |f: &Family| f.planes.iter().find(|p| p.guid == g(SIDE)).unwrap().frame();
    // Across y = −1000: Side goes to y 0, facing north (its normal the mirror image of south).
    let m = Transform::Mirror { point: [0.0, -1000.0, 0.0], normal: [0.0, 1.0, 0.0] };
    let t = f.transformed(&values(&f), &Transforming { planes: vec![g(SIDE)], ..Default::default() }, &m).unwrap();
    let (was, now) = (side(&f), side(&t.family));
    assert!((was.z[1] + now.z[1]).abs() < 1e-9 && was.z[1].abs() > 0.99, "{was:?} {now:?}");
    // The wing drawn on it went with it: each corner where the mirror puts it.
    assert_eq!(t.forms, vec![1]);
    let corner = |f: &Family, fr: &crate::family::Frame| {
        let p = points(f, 1);
        fr.point(crate::geom::P2(p[2][0], p[2][1]), 0.0)
    };
    let (a, b) = (m.point(corner(&f, &was)), corner(&t.family, &now));
    assert!((0..3).all(|k| (a[k] - b[k]).abs() < 1e-6), "{a:?} {b:?}");
    builds(&t.family);
}

#[test]
fn a_rotation_that_would_take_a_form_off_its_plane_or_a_datum_is_refused() {
    let f = family(json!({}));
    let v = values(&f);
    let turn = Transform::Rotate { centre: [0.0; 3], axis: UP, degrees: 30.0 };
    // The wing stands on Side, which isn't taken: turned about z it would leave it.
    let e = f.transformed(&v, &Transforming { forms: vec![1], ..Default::default() }, &turn).unwrap_err();
    assert_eq!(e, TransformRefused::Stays(Stays::Hosted(Named::Form(1))));
    // With Side, it goes along.
    let both = Transforming { planes: vec![g(SIDE)], forms: vec![1], ..Default::default() };
    builds(&f.transformed(&v, &both, &turn).unwrap().family);
    let e = f.transformed(&v, &Transforming { planes: vec![CENTER_LEFT_RIGHT], ..Default::default() }, &turn).unwrap_err();
    assert!(matches!(e, TransformRefused::Stays(Stays::Datum(_))), "{e:?}");
    // No axis, no turn.
    let flat = Transform::Rotate { centre: [0.0; 3], axis: [0.0; 3], degrees: 30.0 };
    assert!(matches!(f.transformed(&v, &both, &flat), Err(TransformRefused::Bad(_))));
}

#[test]
fn scale_takes_planes_and_lines_and_leaves_forms() {
    let f = family(json!({}));
    let v = values(&f);
    let s = Transform::Scale { centre: [0.0; 3], factor: 2.0, normal: UP };
    let t = f.transformed(&v, &Transforming { planes: vec![g(EDGE)], lines: vec![g(LINE)], ..Default::default() }, &s).unwrap();
    let edge = t.family.planes.iter().find(|p| p.guid == g(EDGE)).unwrap();
    assert_eq!((edge.start, edge.end), ([2000.0, -3000.0, 0.0], [2000.0, 3000.0, 0.0]));
    let line = t.family.lines.iter().find(|l| l.guid == g(LINE)).unwrap();
    assert_eq!((line.start, line.end), ([-400.0, 2400.0, 0.0], [400.0, 2400.0, 0.0]));
    assert_eq!(
        f.transformed(&v, &Transforming { forms: vec![0], ..Default::default() }, &s).unwrap_err(),
        TransformRefused::NotScaled(Named::Form(0))
    );
    // A plane with a form drawn on it would take the form along: refused too.
    let e = f.transformed(&v, &Transforming { planes: vec![g(SIDE)], ..Default::default() }, &s).unwrap_err();
    assert_eq!(e, TransformRefused::NotScaled(Named::Form(1)));
    for bad in [0.0, -1.0, f64::NAN, 1e9] {
        let s = Transform::Scale { centre: [0.0; 3], factor: bad, normal: UP };
        assert!(matches!(
            f.transformed(&v, &Transforming { planes: vec![g(EDGE)], ..Default::default() }, &s),
            Err(TransformRefused::Bad(_))
        ));
    }
    let flat = Transform::Scale { centre: [0.0; 3], factor: 2.0, normal: [0.0; 3] };
    assert!(matches!(
        f.transformed(&v, &Transforming { planes: vec![g(EDGE)], ..Default::default() }, &flat),
        Err(TransformRefused::Bad(_))
    ));
}

#[test]
fn a_constraint_between_what_is_taken_and_what_isnt_refuses_it_and_one_among_what_is_taken_goes_along() {
    // Edge on the box's east side, locked to it (Align's lock); a plain dimension from Center (Left/Right) to Edge.
    let f = family(json!({
        "ref_planes": [{"guid": EDGE, "name": "Edge", "start": [500, -1500, 0], "end": [500, 1500, 0], "up": [0, 0, 1]},
                       {"guid": SIDE, "name": "Side", "start": [-1000, -2000, 0], "end": [1000, -2000, 0], "up": [0, 0, 1]}],
        "dimensions": [
            {"view": "plan", "refs": [{"plane": EDGE}, {"form": BOX, "line": EAST}], "at": [800, 300, 0], "locked": [{"segment": 0, "length": 0}], "alignment": true},
            {"view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"plane": EDGE}], "at": [500, -1200, 0]}
        ]
    }));
    let v = values(&f);
    let turn = Transform::Rotate { centre: [300.0, 300.0, 0.0], axis: UP, degrees: 180.0 };
    let e = f.transformed(&v, &Transforming { forms: vec![0], ..Default::default() }, &turn).unwrap_err();
    match e {
        TransformRefused::Unsat(u) => {
            assert!(u.message.starts_with(NOT_SATISFIED), "{}", u.message);
            assert_eq!(u.dims, vec![0]);
        }
        e => panic!("{e:?}"),
    }
    // Both taken: the lock goes with them, its place turned too; the plain dimension measures again.
    let both = Transforming { planes: vec![g(EDGE)], forms: vec![0], ..Default::default() };
    let t = f.transformed(&v, &both, &turn).unwrap();
    let at = t.family.dimensions[0].at;
    assert!((at[0] - -200.0).abs() < 1e-6 && (at[1] - 300.0).abs() < 1e-6, "{at:?}");
    assert_eq!(t.family.dimensions[1], f.dimensions[1]);
    assert!(t.dropped.is_empty());
    builds(&t.family);
    // Turned a quarter, Edge no longer faces Center (Left/Right): the plain dimension between them is deleted.
    let quarter = Transform::Rotate { centre: [300.0, 300.0, 0.0], axis: UP, degrees: 90.0 };
    let t = f.transformed(&v, &both, &quarter).unwrap();
    assert_eq!(t.dropped, vec![f.dimensions[1].guid]);
    assert_eq!(t.family.dimensions.len(), 1);
    builds(&t.family);
}

#[test]
fn a_formula_the_transform_would_change_refuses_it() {
    let mut f = family(json!({}));
    if let FormCurve::Line { start, .. } = &mut f.forms[0].sketch[0].curve {
        start[0] = Expr::Formula("Len / 4".into());
    }
    let v = values(&f);
    let turn = Transform::Rotate { centre: [0.0; 3], axis: UP, degrees: 90.0 };
    let e = f.transformed(&v, &Transforming { forms: vec![0], ..Default::default() }, &turn).unwrap_err();
    assert_eq!(e, TransformRefused::Stays(Stays::Formula(Named::Form(0))));
    // Mirrored across y = 300 the formula's x stays where it is: kept.
    let m = Transform::Mirror { point: [0.0, 300.0, 0.0], normal: [0.0, 1.0, 0.0] };
    let t = f.transformed(&v, &Transforming { forms: vec![0], ..Default::default() }, &m).unwrap();
    assert!(matches!(&t.family.forms[0].sketch[0].curve, FormCurve::Line { start, .. } if start[0] == Expr::Formula("Len / 4".into())));
}

#[test]
fn a_revolve_mirrored_turns_the_other_way_round_its_axis() {
    let f = family(json!({"forms": [
        {"plane": "xz", "profile": [[100, 0], [300, 0], [300, 500], [100, 500]], "axis": [[0, 0], [0, 1]], "from": 0, "to": 90}
    ]}));
    let m = Transform::Mirror { point: [0.0; 3], normal: [1.0, 0.0, 0.0] };
    let t = f.transformed(&values(&f), &Transforming { forms: vec![0], ..Default::default() }, &m).unwrap();
    assert_eq!((t.family.forms[0].from.clone(), t.family.forms[0].to.clone()), (Expr::Number(-90.0), Expr::Number(0.0)));
    builds(&t.family);
    // What it builds is the mirror image of what it was: x flipped, y and z kept.
    let ext = |f: &Family| {
        let pieces = f.pieces(&values(f)).unwrap();
        let pts: Vec<[f64; 3]> = pieces.iter().flat_map(|p| p.tris.iter().flatten().copied()).collect();
        let lo: [f64; 3] = std::array::from_fn(|k| pts.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min));
        let hi: [f64; 3] = std::array::from_fn(|k| pts.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max));
        (lo, hi)
    };
    let ((lo, hi), (lo2, hi2)) = (ext(&f), ext(&t.family));
    assert!((lo2[0] + hi[0]).abs() < 1.0 && (hi2[0] + lo[0]).abs() < 1.0, "{lo:?} {hi:?} → {lo2:?} {hi2:?}");
    assert!((lo2[1] - lo[1]).abs() < 1.0 && (hi2[1] - hi[1]).abs() < 1.0 && (hi2[2] - hi[2]).abs() < 1.0);
}

/// The least and greatest corner of what `f` builds.
fn extent(f: &Family) -> ([f64; 3], [f64; 3]) {
    let pieces = f.pieces(&values(f)).unwrap();
    let pts: Vec<[f64; 3]> = pieces.iter().flat_map(|p| p.tris.iter().flatten().copied()).collect();
    let lo = std::array::from_fn(|k| pts.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min));
    let hi = std::array::from_fn(|k| pts.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max));
    (lo, hi)
}

/// Whether `a` and `b` are the same box, to a micron.
fn same_box(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])) -> bool {
    (0..3).all(|k| (a.0[k] - b.0[k]).abs() < 1e-3 && (a.1[k] - b.1[k]).abs() < 1e-3)
}

/// `f`'s form `i` alone, as a family of its own (the other forms left out), for its extent.
fn only(f: &Family, i: usize) -> Family {
    let mut f = f.clone();
    f.forms = vec![f.forms[i].clone()];
    f
}

/// The box `(lo, hi)` transformed by `t`: its corners' least and greatest.
fn moved_box(t: &Transform, (lo, hi): ([f64; 3], [f64; 3])) -> ([f64; 3], [f64; 3]) {
    let corners: Vec<[f64; 3]> = (0..8)
        .map(|c| {
            t.point([
                if c & 1 == 0 { lo[0] } else { hi[0] },
                if c & 2 == 0 { lo[1] } else { hi[1] },
                if c & 4 == 0 { lo[2] } else { hi[2] },
            ])
        })
        .collect();
    let a = std::array::from_fn(|k| corners.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min));
    let b = std::array::from_fn(|k| corners.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max));
    (a, b)
}

#[test]
fn a_transform_that_turns_a_hosts_normal_over_turns_the_forms_extent_over_too() {
    let f = family(json!({}));
    let v = values(&f);
    let only_box = Transforming { forms: vec![0], ..Default::default() };
    let was = extent(&only(&f, 0));
    // A half turn about −y at the origin (Front elevation, the centre on the level line): x −500..−100, z −300..0.
    let half = Transform::Rotate { centre: [0.0; 3], axis: [0.0, -1.0, 0.0], degrees: 180.0 };
    let t = f.transformed(&v, &only_box, &half).unwrap();
    let now = extent(&only(&t.family, 0));
    assert!(same_box(now, moved_box(&half, was)), "{was:?} → {now:?}");
    assert!((now.0[2] + 300.0).abs() < 1e-3 && now.1[2].abs() < 1e-3, "{now:?}");
    // Mirrored across z = 0 (an elevation): below the level, not left as it was.
    let m = Transform::Mirror { point: [0.0; 3], normal: UP };
    let t = f.transformed(&v, &only_box, &m).unwrap();
    let now = extent(&only(&t.family, 0));
    assert!(same_box(now, moved_box(&m, was)), "{was:?} → {now:?}");
}

#[test]
fn a_form_on_a_reference_lines_face_mirrored_with_the_line_stays_on_its_side() {
    const LEAF: &str = "fe800000-0000-4000-8000-000000000201";
    let f = family(json!({
        "ref_lines": [{"guid": LEAF, "start": [200, 900, 0], "end": [800, 900, 0]}],
        "forms": [{"host": {"line": LEAF, "face": "vertical"}, "profile": [[100, 0], [500, 0], [500, 300], [100, 300]], "from": 0, "to": 300}]
    }));
    let v = values(&f);
    let was = extent(&f);
    let m = Transform::Mirror { point: [0.0; 3], normal: [1.0, 0.0, 0.0] };
    let t = f.transformed(&v, &Transforming { lines: vec![g(LEAF)], ..Default::default() }, &m).unwrap();
    assert_eq!(t.forms, vec![0]);
    let now = extent(&t.family);
    assert!(same_box(now, moved_box(&m, was)), "{was:?} → {now:?}");
}

#[test]
fn a_blend_and_a_revolve_turned_over_build_where_the_transform_puts_them() {
    let f = family(json!({"forms": [
        {"sketch": [{"start": [100, 100], "end": [500, 100]}, {"start": [500, 100], "end": [500, 500]}, {"start": [500, 500], "end": [100, 500]}, {"start": [100, 500], "end": [100, 100]}],
         "top": [{"centre": [300, 300], "radius": 100}], "from": 0, "to": 400},
        {"plane": "xz", "profile": [[100, 0], [300, 0], [300, 500], [100, 500]], "axis": [[0, 0], [0, 1]], "from": 0, "to": 90}
    ]}));
    let v = values(&f);
    for t in [
        Transform::Rotate { centre: [0.0; 3], axis: [0.0, -1.0, 0.0], degrees: 180.0 },
        Transform::Rotate { centre: [0.0; 3], axis: [1.0, 0.0, 0.0], degrees: 180.0 },
        Transform::Mirror { point: [0.0; 3], normal: UP },
        Transform::Mirror { point: [0.0; 3], normal: [0.0, 1.0, 0.0] },
    ] {
        for i in 0..2 {
            let was = extent(&only(&f, i));
            let Ok(done) = f.transformed(&v, &Transforming { forms: vec![i], ..Default::default() }, &t) else {
                // Refused is fine (it leaves the plane it is drawn on); a wrong place isn't.
                continue;
            };
            let now = extent(&only(&done.family, i));
            assert!(same_box(now, moved_box(&t, was)), "form {i} {t:?}: {was:?} → {now:?}");
        }
    }
}

#[test]
fn scale_holds_the_depth_the_view_looks_along() {
    // A reference line drawn on Side (y −2000), scaled ×2 in a Front elevation (looking along y): it stays on Side.
    const ON: &str = "fe800000-0000-4000-8000-000000000301";
    let f = family(json!({"ref_lines": [{"guid": ON, "host": {"plane": SIDE}, "start": [-200, -2000, 100], "end": [200, -2000, 100]}],
                          "forms": []}));
    let v = values(&f);
    let s = Transform::Scale { centre: [0.0; 3], factor: 2.0, normal: [0.0, 1.0, 0.0] };
    let t = f.transformed(&v, &Transforming { lines: vec![g(ON)], ..Default::default() }, &s).unwrap();
    let l = t.family.lines.iter().find(|l| l.guid == g(ON)).unwrap();
    assert_eq!((l.start, l.end), ([-400.0, -2000.0, 200.0], [400.0, -2000.0, 200.0]));
    // Edge (x 1000, along y) scaled there: across doubled, its depth extent and up kept.
    let t = f.transformed(&v, &Transforming { planes: vec![g(EDGE)], ..Default::default() }, &s).unwrap();
    let e = t.family.planes.iter().find(|p| p.guid == g(EDGE)).unwrap();
    assert_eq!((e.start, e.end, e.up), ([2000.0, -1500.0, 0.0], [2000.0, 1500.0, 0.0], [0.0, 0.0, 1.0]));
}
