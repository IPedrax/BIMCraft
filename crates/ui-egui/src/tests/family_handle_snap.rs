//! A shape handle moves its side alone, the view stays as it was while it is dragged, and a side or an end dragged up
//! to a reference plane or another form's side lines up with it; let go there in a plan or an elevation, Align's padlock is
//! offered, and a click on it locks the two.
use super::family_handle_params::{STARTER, drag, handle, opened_on, press_move, release, screen, status};
use super::family_views::{click_family, editor};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{DimRef, Expr, Extent};
use bimcraft_core::ids::Guid;
use std::collections::BTreeMap;

const RIGHT: &str = "44444444-4444-4444-8444-000000000001";
const SLAB: &str = "44444444-4444-4444-8444-000000000002";
const BLOCK: &str = "44444444-4444-4444-8444-000000000003";
const SLAB_EAST: &str = "44444444-4444-4444-8444-000000000011";

fn g(s: &str) -> Guid {
    serde_json::from_value(serde_json::json!(s)).unwrap()
}

/// A slab at ±500 by ±300, 200 tall, a block beside it from x = 1200 to 1500, 450 tall, and a plane across x at 800: numbers all.
fn beside() -> String {
    let ring = |x: [i32; 2], first: u8| {
        let line = |k: u8, a: [i32; 2], b: [i32; 2]| {
            format!(
                r#"{{"guid": "44444444-4444-4444-8444-0000000000{:02}", "start": [{}, {}], "end": [{}, {}]}}"#,
                first + k,
                a[0],
                a[1],
                b[0],
                b[1]
            )
        };
        [
            line(0, [x[0], -300], [x[1], -300]),
            line(1, [x[1], -300], [x[1], 300]),
            line(2, [x[1], 300], [x[0], 300]),
            line(3, [x[0], 300], [x[0], -300]),
        ]
        .join(", ")
    };
    format!(
        r#"family {{"name": "Beside", "category": "generic", "types": {{"T": {{}}}}, "ref_planes": [{{"guid": "{RIGHT}", "name": "Right", "start": [800, -1000, 0], "end": [800, 1000, 0], "up": [0, 0, 1]}}], "forms": [{{"guid": "{SLAB}", "sketch": [{}], "from": 0, "to": 200}}, {{"guid": "{BLOCK}", "sketch": [{}], "from": 0, "to": 450}}]}}"#,
        ring([-500, 500], 10),
        ring([1200, 1500], 20)
    )
}

/// Form `i`'s least and greatest along axis `k`, as type `ty` builds it.
fn span(h: &Harness<BimApp>, ty: &str, i: usize, k: usize) -> (f64, f64) {
    let f = editor(h).draft_family().unwrap();
    let values = f.values(ty, &BTreeMap::new()).unwrap();
    let shape = f.shapes(&values).into_iter().find(|s| s.index == i).unwrap_or_else(|| panic!("form {i} builds"));
    (shape.min[k], shape.max[k])
}

/// The corners of form `i`'s sketch as `values` size it, in its host's axes: where its lines start and end.
fn sketch_corners(f: &bimcraft_core::family::Family, values: &BTreeMap<String, bimcraft_core::family::Val>, i: usize) -> Vec<[f64; 2]> {
    let mut num = |e: &Expr| crate::adaptive_editor::worth(e, values).ok_or_else(|| format!("{e:?}"));
    f.forms[i]
        .sketch
        .iter()
        .filter_map(|l| l.curve(&mut num).ok())
        .flat_map(|l| [l.curve.start(), l.curve.end()])
        .map(|p| [p.0, p.1])
        .collect()
}

#[test]
fn the_starter_boxs_side_moves_alone_and_width_still_moves_it() {
    for which in [Some(Which::Plan), None] {
        let mut h = opened_on(STARTER, "Starter", which);
        let steps = editor(&h).steps();
        let z = if which.is_none() { 375.0 } else { 0.0 };
        // The east side (x 500, `Width / 2`) out 100: it alone goes, to 600; the west side stays at -500.
        drag(&mut h, [500.0, 0.0, z], [100.0, 0.0, 0.0], false);
        let (west, east) = span(&h, "1000 x 600", 0, 0);
        assert!((east - 600.0).abs() < 15.0, "{which:?}: east at {east}, {:?}", status(&h));
        assert_eq!(west, -500.0, "{which:?}: the west side stays");
        assert_eq!(editor(&h).steps(), steps + 1, "one step");
        // No parameter was set for it: Width is what it was in every type, and still moves both sides.
        let f = editor(&h).draft_family().unwrap();
        assert!(f.types.values().all(|t| t.get("Width").is_none_or(|v| v.as_f64() == Some(1400.0))), "{:?}", f.types);
        let (wide_west, wide_east) = span(&h, "Wide", 0, 0);
        assert_eq!(wide_west, -700.0);
        assert!((wide_east - (700.0 + east - 500.0)).abs() < 1e-6, "Wide's east side moved as far: {wide_east}");
        // Its numbers are still Width's, the move added.
        let x = match &f.forms[0].sketch[1].curve {
            bimcraft_core::family::sketch::FormCurve::Line { start, .. } => start[0].clone(),
            _ => panic!(),
        };
        assert!(matches!(&x, Expr::Formula(s) if s.starts_with("(Width / 2) + ")), "{x:?}");
        // Undo takes it back whole.
        h.state_mut().family_undo();
        h.run();
        assert_eq!(span(&h, "1000 x 600", 0, 0), (-500.0, 500.0));
    }
}

#[test]
fn a_drag_that_takes_the_form_past_what_the_view_framed_leaves_the_view_as_it_was() {
    let mut h = opened_on(STARTER, "Starter", Some(Which::Elevation(Side::Front)));
    let marks = |h: &Harness<BimApp>| [screen(h, [0.0, 0.0, 0.0]), screen(h, [1000.0, 0.0, 1000.0])];
    let before = marks(&h);
    // The top (End, Height 750) up 1000: taller than the room the view was fitted to.
    let at = drag(&mut h, [0.0, -300.0, 750.0], [0.0, 0.0, 1000.0], true);
    assert_eq!(marks(&h), before, "mid-drag, the view as it was");
    release(&mut h, at);
    for _ in 0..3 {
        h.run();
    }
    let (_, top) = span(&h, "1000 x 600", 0, 2);
    assert!((top - 1750.0).abs() < 20.0, "the top at {top}: {:?}", status(&h));
    assert_eq!(marks(&h), before, "let go, the view as it was");
    // Zoom to Fit frames what the family takes up now.
    h.state_mut().active_family_mut().unwrap().zoom_to_fit();
    for _ in 0..3 {
        h.run();
    }
    assert_ne!(marks(&h), before, "fitted afresh when asked");
}

#[test]
fn a_side_dragged_up_to_a_plane_lines_up_with_it_and_its_padlock_locks_them() {
    let mut h = opened_on(&beside(), "Beside", Some(Which::Plan));
    // Past reach of anything: where the hand leaves it, and no padlock.
    drag(&mut h, [500.0, 0.0, 0.0], [150.0, 0.0, 0.0], false);
    let (_, east) = span(&h, "T", 0, 0);
    assert!((east - 650.0).abs() < 15.0 && east != 800.0, "east at {east}");
    assert!(editor(&h).views.align_lock.is_none(), "nothing to lock");
    // Up to a few points short of Right (x 800): on it exactly, mid-drag too.
    let at = drag(&mut h, [east, 0.0, 0.0], [800.0 - east - 12.0, 0.0, 0.0], true);
    assert_eq!(span(&h, "T", 0, 0).1, 800.0, "mid-drag, lined up: {:?}", status(&h));
    release(&mut h, at);
    assert_eq!(span(&h, "T", 0, 0), (-500.0, 800.0), "{:?}", status(&h));
    // Its padlock, open, beside the side: a click locks the two.
    let lock = editor(&h).views.align_lock.expect("the padlock offered");
    assert_eq!((lock.0, lock.1, lock.3), (DimRef::Plane(g(RIGHT)), DimRef::Sketch(g(SLAB), g(SLAB_EAST)), Which::Plan));
    for _ in 0..2 {
        h.run();
    }
    let spot = editor(&h).views.offer_shown.expect("the padlock shown");
    click_at(&mut h, spot);
    let f = editor(&h).draft_family().unwrap();
    let locks: Vec<_> = f.dimensions.iter().filter(|d| d.alignment).map(|d| d.refs.clone()).collect();
    assert_eq!(locks, vec![vec![DimRef::Plane(g(RIGHT)), DimRef::Sketch(g(SLAB), g(SLAB_EAST))]], "{:?}", status(&h));
    // Locked, the side has no handle of its own: the plane moves it.
    h.state_mut().active_family_mut().unwrap().select_form(0);
    for _ in 0..2 {
        h.run();
    }
    assert_eq!(editor(&h).views.shapes_shown.len(), 3, "{:?}", editor(&h).views.shapes_shown);
}

#[test]
fn a_side_lines_up_with_another_forms_side_and_an_end_with_its_end() {
    // The slab's east side up to the block's west side (x 1200), past the plane.
    let mut h = opened_on(&beside(), "Beside", Some(Which::Plan));
    drag(&mut h, [500.0, 0.0, 0.0], [690.0, 0.0, 0.0], false);
    assert_eq!(span(&h, "T", 0, 0).1, 1200.0, "{:?}", status(&h));
    let lock = editor(&h).views.align_lock.expect("the padlock offered");
    assert!(matches!((lock.0, lock.1), (DimRef::Sketch(b, _), DimRef::Sketch(s, l)) if b == g(BLOCK) && s == g(SLAB) && l == g(SLAB_EAST)));
    // In an elevation, the slab's End (200) up to the block's (450).
    let mut h = opened_on(&beside(), "Beside", Some(Which::Elevation(Side::Front)));
    drag(&mut h, [0.0, -300.0, 200.0], [0.0, 0.0, 240.0], false);
    assert_eq!(span(&h, "T", 0, 2).1, 450.0, "{:?}", status(&h));
    let lock = editor(&h).views.align_lock.expect("the padlock offered");
    assert_eq!((lock.0, lock.1), (DimRef::Form(g(BLOCK), Extent::End), DimRef::Form(g(SLAB), Extent::End)));
}

#[test]
fn in_3d_a_side_and_an_end_line_up_too_once_let_go() {
    let mut h = opened_on(&beside(), "Beside", None);
    let camera = editor(&h).orbit.camera.map(|c| (c.azimuth, c.elevation));
    // The east side, at mid-depth, to a little short of Right.
    drag(&mut h, [500.0, 0.0, 100.0], [290.0, 0.0, 0.0], false);
    assert_eq!(span(&h, "T", 0, 0), (-500.0, 800.0), "{:?}", status(&h));
    // The End, up to the block's.
    h.state_mut().active_family_mut().unwrap().select_form(0);
    for _ in 0..3 {
        h.run();
    }
    let (at, free) = handle(&h, [150.0, 0.0, 200.0]);
    assert!(free);
    let to = at + (screen(&h, [150.0, 0.0, 440.0]) - screen(&h, [150.0, 0.0, 200.0]));
    press_move(&mut h, at, to);
    release(&mut h, to);
    assert_eq!(span(&h, "T", 0, 2).1, 450.0, "{:?}", status(&h));
    assert_eq!(editor(&h).orbit.camera.map(|c| (c.azimuth, c.elevation)), camera, "the view didn't turn");
}

/// The starter box (its sides `Width / 2` and `Depth / 2` either way of the centre) and a plane across x at 800.
fn starter_and_plane() -> String {
    STARTER.replacen(
        r#""types":"#,
        &format!(r#""ref_planes": [{{"guid": "{RIGHT}", "name": "Right", "start": [800, -1000, 0], "end": [800, 1000, 0], "up": [0, 0, 1]}}], "types":"#),
        1,
    )
}

/// A face locked to a plane by Align holds the face there, not its corners: the side next to it, dragged by its handle, takes
/// the corner along the locked face, and the solid keeps its square corners. For a box drawn with formulas and one with numbers.
#[test]
fn a_face_aligned_and_locked_lets_its_corners_slide_along_it() {
    for (script, name, ty) in [(starter_and_plane(), "Starter", "1000 x 600"), (beside(), "Beside", "T")] {
        let mut h = opened_on(&script, name, Some(Which::Plan));
        // Align: Right, then the box's east side; its padlock clicked.
        shortcut(&mut h, "AL");
        click_family(&mut h, [800.0, 700.0, 0.0]);
        click_family(&mut h, [500.0, 0.0, 0.0]);
        assert_eq!(span(&h, ty, 0, 0), (-500.0, 800.0), "{name}: aligned, {:?}", status(&h));
        for _ in 0..2 {
            h.run();
        }
        let spot = editor(&h).views.offer_shown.expect("the padlock shown");
        click_at(&mut h, spot);
        assert_eq!(editor(&h).draft_family().unwrap().dimensions.iter().filter(|d| d.alignment).count(), 1, "{name}: locked");
        key(&mut h, Key::Escape);
        key(&mut h, Key::Escape);
        // The aligned corners keep what placed them across the face: the starter's still Depth's.
        if name == "Starter" {
            let f = editor(&h).draft_family().unwrap();
            let ys: Vec<String> = f.forms[0]
                .sketch
                .iter()
                .flat_map(|l| match &l.curve {
                    bimcraft_core::family::sketch::FormCurve::Line { start, end, .. } => [start[1].clone(), end[1].clone()],
                    _ => panic!(),
                })
                .map(|e| format!("{e:?}"))
                .collect();
            assert!(ys.iter().all(|y| y.contains("Depth")), "the y of every corner is still Depth's: {ys:?}");
        }
        // The north side (y 300), its middle now at x 150, out 100.
        h.state_mut().active_family_mut().unwrap().select_form(0);
        for _ in 0..2 {
            h.run();
        }
        drag(&mut h, [150.0, 300.0, 0.0], [0.0, 100.0, 0.0], false);
        let f = editor(&h).draft_family().unwrap();
        let values = f.values(ty, &BTreeMap::new()).unwrap();
        let corners = sketch_corners(&f, &values, 0);
        let off = |v: f64, want: &[f64]| want.iter().all(|w| (v - w).abs() > 1.0);
        let bad: Vec<_> = corners.iter().filter(|p| off(p[0], &[-500.0, 800.0]) || off(p[1], &[-300.0, 400.0])).collect();
        assert!(bad.is_empty(), "{name}: square corners at x -500 / 800, y -300 / 400: {bad:?} of {corners:?}, {:?}", status(&h));
        assert_eq!(f.dimensions.iter().filter(|d| d.alignment).count(), 1, "{name}: still locked");
    }
}

/// A snap the profile can't take: the side refused where it would line up, the form as it was, and no padlock offered nor
/// "Aligned" said, though the hand let go within reach of the target.
#[test]
fn a_snap_the_profile_refuses_offers_no_padlock() {
    let mut h = opened_on(&beside(), "Beside", Some(Which::Plan));
    // The slab's west side (x -500) out past its east side, to a few points short of Right (x 800): it would turn the slab
    // inside out.
    let at = drag(&mut h, [-500.0, 0.0, 0.0], [1288.0, 0.0, 0.0], true);
    assert!(!status(&h).0.contains("padlock"), "{:?}", status(&h));
    release(&mut h, at);
    assert_eq!(span(&h, "T", 0, 0), (-500.0, 500.0), "as it was: {:?}", status(&h));
    assert!(editor(&h).views.align_lock.is_none(), "no padlock beside a side that isn't there");
    assert!(status(&h).1, "said why: {:?}", status(&h));
}

/// A corner whose two ends are written differently (a number and a formula that come to one place in every type) stays one
/// corner when a side dragged moves it: the number takes the place, the formula the move added, and each type builds.
#[test]
fn a_corner_written_two_ways_stays_closed_in_every_type() {
    let line = |a: [&str; 2], b: [&str; 2]| format!(r#"{{"start": [{}, {}], "end": [{}, {}]}}"#, a[0], a[1], b[0], b[1]);
    let w = r#""Width / 2""#;
    let sketch = [
        line(["-500", "-300"], ["500", "-300"]),
        line([w, "-300"], [w, "300"]),
        line(["500", "300"], ["-500", "300"]),
        line(["-500", "300"], ["-500", "-300"]),
    ]
    .join(", ");
    let script = format!(
        r#"family {{"name": "Mixed", "category": "generic", "parameters": [{{"name": "Width", "value": 1000}}], "types": {{"A": {{}}, "B": {{"Width": 1000}}}}, "forms": [{{"sketch": [{sketch}], "from": 0, "to": 200}}]}}"#
    );
    let mut h = opened_on(&script, "Mixed", Some(Which::Plan));
    drag(&mut h, [500.0, 0.0, 0.0], [40.0, 0.0, 0.0], false);
    for ty in ["A", "B"] {
        let (west, east) = span(&h, ty, 0, 0);
        assert!(west == -500.0 && (east - 540.0).abs() < 2.0, "{ty}: {west} {east} {:?}", status(&h));
    }
    let f = editor(&h).draft_family().unwrap();
    for ty in ["A", "B"] {
        let values = f.values(ty, &BTreeMap::new()).unwrap();
        assert!(f.pieces(&values).is_ok(), "{ty} builds");
    }
}
