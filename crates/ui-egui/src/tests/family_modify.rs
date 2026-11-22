//! The Modify panel's tools in a family that isn't adaptive. Move, Copy, Rotate, Mirror (Pick and Draw Axis), Scale and
//! Offset on what is selected in a plan, by clicks and the Options Bar, each one step of the history; what can't be done says
//! why (a form for Scale, a sketch's tool outside a sketch, a constraint asked about as "Constraints are not satisfied"). In
//! sketch mode the same on the lines picked, with Trim/Extend Single Element and Split Element.
use super::family_drag_move::{A, A_LINE, CENTER_LR, EDGE, ed, family, family_with, g, near, pick, plan, plan_of, points, status};
use super::family_selection::command;
use super::family_views::editor;
use super::*;
use crate::editor::modify::Tool as M;
use crate::editor::select_box::Picked;
use bimcraft_core::family::views::Which;

/// A click at family point `p` (on Ref. Level) in the view shown.
fn click(h: &mut Harness<BimApp>, p: [f64; 2]) {
    let at = editor(h).views.screen_of([p[0], p[1], 0.0]).expect("a view shown");
    click_at(h, at);
}

fn tool(h: &Harness<BimApp>) -> Option<M> {
    editor(h).modify_in_hand()
}

fn plane_x(h: &Harness<BimApp>, guid: &str) -> f64 {
    family(h).planes.iter().find(|p| p.guid == g(guid)).map(|p| p.start[0]).expect("the plane")
}

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    near(a[0], b[0]) && near(a[1], b[1])
}

#[test]
fn move_by_two_clicks_moves_the_selection_as_one_step_and_puts_the_tool_down() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    command(&mut h, "Move");
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }), "{}", status(&h));
    assert!(h.state().prompt().contains("point to move from"), "{}", h.state().prompt());
    click(&mut h, [300.0, 300.0]);
    assert!(h.state().prompt().contains("point to move to"), "{}", h.state().prompt());
    click(&mut h, [600.0, 500.0]);
    let after = points(&h, 0);
    for (a, b) in before.iter().zip(&after) {
        assert!(same(*b, [a[0] + 300.0, a[1] + 200.0]), "{a:?} → {b:?} {}", status(&h));
    }
    assert_eq!(editor(&h).steps(), 1);
    assert_eq!(tool(&h), None);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)]);
    h.state_mut().family_undo();
    h.run();
    assert_eq!(points(&h, 0), before);
}

#[test]
fn what_temporary_hide_hides_isnt_moved_or_rotated() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Temporary Hide Element");
    pick(&mut h, &[Picked::Form(0), Picked::Form(1)]);
    let (a, b) = (points(&h, 0), points(&h, 1));
    command(&mut h, "Move");
    click(&mut h, [300.0, 300.0]);
    click(&mut h, [300.0, 800.0]);
    assert!(same(points(&h, 0)[0], [a[0][0], a[0][1] + 500.0]), "{:?} {}", points(&h, 0), status(&h));
    assert_eq!(points(&h, 1), b, "the hidden box stays");
    command(&mut h, "Rotate");
    click(&mut h, [700.0, 800.0]);
    click(&mut h, [300.0, 1200.0]);
    assert_eq!(points(&h, 1), b, "the hidden box stays");
    assert_eq!(editor(&h).steps(), 2, "{}", status(&h));
}

#[test]
fn copy_with_multiple_makes_a_copy_at_each_click_and_escape_steps_back() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let n = family(&h).forms.len();
    command(&mut h, "Copy");
    ed(&mut h).views.modify_options.multiple = true;
    // Steps the plan shows whole, the second copy's click still on the canvas.
    click(&mut h, [300.0, 300.0]);
    click(&mut h, [300.0, 800.0]);
    click(&mut h, [300.0, 1300.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), n + 2, "{}", status(&h));
    // Each copy a step further, the original where it was.
    assert_eq!(points(&h, 0)[0], [100.0, 100.0]);
    assert!(same(points(&h, 1)[0], [100.0, 600.0]), "{:?}", points(&h, 1));
    assert!(same(points(&h, 2)[0], [100.0, 1100.0]), "{:?}", points(&h, 2));
    assert_eq!(editor(&h).steps(), 2);
    // Still copying; Escape lets the start go, then the tool.
    assert!(matches!(tool(&h), Some(M::Move { copy: true, from: Some(_) })));
    key(&mut h, Key::Escape);
    assert_eq!(tool(&h), Some(M::Move { copy: true, from: None }));
    key(&mut h, Key::Escape);
    assert_eq!(tool(&h), None);
}

#[test]
fn rotate_turns_the_selection_about_its_centre_by_two_clicks_or_a_typed_angle() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    command(&mut h, "Rotate");
    // Its centre found as the view is drawn: the middle of the box.
    match tool(&h) {
        Some(M::Rotate { centre: Some(c), .. }) => assert!(near(c.0, 300.0) && near(c.1, 300.0), "{c:?}"),
        t => panic!("{t:?}"),
    }
    click(&mut h, [700.0, 300.0]);
    click(&mut h, [300.0, 700.0]);
    // A quarter turn counter-clockwise: (x, y) to (600 − y, x).
    for (a, b) in before.iter().zip(points(&h, 0)) {
        assert!(same(b, [600.0 - a[1], a[0]]), "{a:?} → {b:?} {}", status(&h));
    }
    assert_eq!(editor(&h).steps(), 1);
    assert_eq!(tool(&h), None);
    // Typed: 90 more on the Options Bar, Enter.
    command(&mut h, "Rotate");
    type_into(&mut h, "Angle:", "90");
    key(&mut h, Key::Enter);
    h.run();
    for (a, b) in before.iter().zip(points(&h, 0)) {
        assert!(same(b, [600.0 - a[0], 600.0 - a[1]]), "{a:?} → {b:?} {}", status(&h));
    }
    assert_eq!(editor(&h).steps(), 2);
}

/// The turn (degrees) of line `k` of form `i`'s sketch, if it is an arc.
fn turn_of(h: &Harness<BimApp>, i: usize, k: usize) -> Option<f64> {
    match &family(h).forms[i].sketch[k].curve {
        bimcraft_core::family::sketch::FormCurve::Line { turn: Some(bimcraft_core::family::Expr::Number(x)), .. } => Some(*x),
        _ => None,
    }
}

#[test]
fn mirror_pick_axis_copies_across_a_plane_and_draw_axis_mirrors_in_place() {
    // The box's east side an arc bulging 90° to its right as drawn.
    let script =
        family_with("").replacen(r#""start": [500, 100], "end": [500, 500]}"#, r#""start": [500, 100], "end": [500, 500], "turn": 90}"#, 1);
    let mut h = plan_of(&script);
    assert_eq!(turn_of(&h, 0, 1), Some(90.0));
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    let n = family(&h).forms.len();
    command(&mut h, "Mirror - Pick Axis");
    assert!(h.state().prompt().contains("Pick Axis"), "{}", h.state().prompt());
    // Edge, at x 1000: a mirrored copy (Copy is on at first), selected; each point in its order, the arc the other way.
    click(&mut h, [1000.0, 600.0]);
    assert_eq!(family(&h).forms.len(), n + 1, "{}", status(&h));
    let copy = points(&h, 1);
    assert_eq!(copy.len(), before.len());
    for (a, b) in before.iter().zip(&copy) {
        assert!(same(*b, [2000.0 - a[0], a[1]]), "{a:?} → {b:?}");
    }
    assert_eq!(turn_of(&h, 1, 1), Some(-90.0));
    assert_eq!(points(&h, 0), before);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(1)]);
    // Draw Axis with Copy off: the copy mirrored back across x 1000 in place, point for point, the arc as it was drawn.
    command(&mut h, "Mirror - Draw Axis");
    ed(&mut h).views.modify_options.mirror_copy = false;
    click(&mut h, [1000.0, -100.0]);
    click(&mut h, [1000.0, 100.0]);
    assert_eq!(family(&h).forms.len(), n + 1);
    for (a, b) in before.iter().zip(points(&h, 1)) {
        assert!(same(b, *a), "{a:?} → {b:?} {}", status(&h));
    }
    assert_eq!(turn_of(&h, 1, 1), Some(90.0));
    assert_eq!(editor(&h).steps(), 2);
}

/// A click at family point `p` in the view shown, whatever it is.
fn click3(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    click_at(h, at);
}

/// The family's plan, then its Front elevation shown.
fn front() -> Harness<'static, BimApp> {
    let mut h = plan();
    super::family_views::view(&mut h, Some(Which::Elevation(bimcraft_core::family::views::Side::Front)));
    h
}

fn extents(h: &Harness<BimApp>, i: usize) -> (bimcraft_core::family::Expr, bimcraft_core::family::Expr) {
    let f = family(h);
    (f.forms[i].from.clone(), f.forms[i].to.clone())
}

#[test]
fn in_an_elevation_a_half_turn_and_a_mirror_across_the_level_put_the_box_below_it() {
    use bimcraft_core::family::Expr;
    // A half turn about a centre placed on the level line at x 300: x 100..500 stays, the box hangs below the level.
    let mut h = front();
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    command(&mut h, "Rotate");
    if let Some(m) = ed(&mut h).views.modify.as_mut() {
        m.tool = M::Rotate { centre: None, start: None, placing: true };
    }
    click3(&mut h, [300.0, 0.0, 0.0]);
    click3(&mut h, [600.0, 0.0, 0.0]);
    click3(&mut h, [0.0, 0.0, 0.0]);
    assert_eq!(extents(&h, 0), (Expr::Number(-300.0), Expr::Number(0.0)), "{}", status(&h));
    for (a, b) in before.iter().zip(points(&h, 0)) {
        assert!(same(b, [600.0 - a[0], a[1]]), "{a:?} → {b:?}");
    }
    // Mirrored across the level line (Draw Axis, Copy off): back above it, its plan as it now is.
    let now = points(&h, 0);
    command(&mut h, "Mirror - Draw Axis");
    ed(&mut h).views.modify_options.mirror_copy = false;
    click3(&mut h, [-500.0, 0.0, 0.0]);
    click3(&mut h, [500.0, 0.0, 0.0]);
    assert_eq!(extents(&h, 0), (Expr::Number(0.0), Expr::Number(300.0)), "{}", status(&h));
    assert_eq!(points(&h, 0), now);
    assert_eq!(editor(&h).steps(), 2);
}

#[test]
fn in_an_elevation_scale_holds_the_depth_it_looks_along() {
    let mut h = front();
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    command(&mut h, "Scale");
    {
        let o = &mut ed(&mut h).views.modify_options;
        o.numerical = true;
        o.factor = "2".into();
    }
    click3(&mut h, [0.0, 0.0, 0.0]);
    let e = family(&h).planes.iter().find(|p| p.guid == g(EDGE)).cloned().unwrap();
    assert!(near(e.start[0], 2000.0) && near(e.end[0], 2000.0), "{e:?} {}", status(&h));
    assert_eq!((e.start[1], e.end[1], e.up), (-1500.0, 1500.0, [0.0, 0.0, 1.0]), "its depth and up kept");
}

#[test]
fn rotate_mirror_and_scale_with_only_a_dimension_selected_say_why() {
    let mut h = plan();
    pick(&mut h, &[Picked::Dimension(g(super::family_drag_move::DIM))]);
    for name in ["Rotate", "Mirror - Pick Axis", "Mirror - Draw Axis", "Scale"] {
        h.state_mut().status = crate::Status::default();
        command(&mut h, name);
        assert!(status(&h).contains("a dimension goes with what it measures"), "{name}: {}", status(&h));
        assert_eq!(tool(&h), None);
    }
    // A hidden form and a dimension: Rotate is picked, but its click says nothing selected is shown; its typed angle too.
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Temporary Hide Element");
    pick(&mut h, &[Picked::Form(1), Picked::Dimension(g(super::family_drag_move::DIM))]);
    command(&mut h, "Rotate");
    assert!(matches!(tool(&h), Some(M::Rotate { centre: None, .. })), "{:?} {}", tool(&h), status(&h));
    type_into(&mut h, "Angle:", "45");
    key(&mut h, Key::Enter);
    h.run();
    assert!(status(&h).contains("nothing selected is shown"), "{}", status(&h));
    assert_eq!(editor(&h).steps(), 0);
}

#[test]
fn rotating_in_plan_a_form_drawn_on_a_datum_says_the_datum_stays() {
    let fb = bimcraft_core::family::CENTER_FRONT_BACK;
    let script = family_with("").replacen(
        r#""forms": ["#,
        &format!(
            r#""forms": [{{"host": {{"plane": "{fb}"}}, "profile": [[0, 0], [200, 0], [200, 100], [0, 100]], "from": 0, "to": 50}}, "#
        ),
        1,
    );
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Rotate");
    click(&mut h, [500.0, 0.0]);
    click(&mut h, [100.0, 400.0]);
    assert!(status(&h).contains("a datum, which stays"), "{}", status(&h));
    assert!(!status(&h).contains("select that plane too"), "{}", status(&h));
}

#[test]
fn scale_is_greyed_for_a_form_saying_why_and_scales_a_plane_by_a_factor() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Scale");
    assert_eq!(tool(&h), None);
    assert!(status(&h).contains("a form is sized by its dimensions"), "{}", status(&h));
    assert!(h.state().status.error);
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    command(&mut h, "Scale");
    assert!(matches!(tool(&h), Some(M::Scale { .. })), "{}", status(&h));
    {
        let o = &mut ed(&mut h).views.modify_options;
        o.numerical = true;
        o.factor = "1.5".into();
    }
    click(&mut h, [0.0, 0.0]);
    assert!(near(plane_x(&h, EDGE), 1500.0), "{} {}", plane_x(&h, EDGE), status(&h));
    assert_eq!(editor(&h).steps(), 1);
}

#[test]
fn offset_copies_a_plane_toward_the_side_clicked_and_refuses_a_form() {
    let mut h = plan();
    command(&mut h, "Offset");
    assert_eq!(tool(&h), Some(M::Offset));
    ed(&mut h).views.modify_options.offset = "250".into();
    // Just west of Edge's trace: a copy 250 west of it.
    let (a, b) = (editor(&h).views.screen_of([1000.0, 0.0, 0.0]).unwrap(), editor(&h).views.screen_of([1100.0, 0.0, 0.0]).unwrap());
    // Clear of Center (Front/Back), whose trace runs along y 0.
    let up = editor(&h).views.screen_of([1000.0, 600.0, 0.0]).unwrap() - a;
    let west = a + up - (b - a).normalized() * 2.0;
    click_at(&mut h, west);
    let xs: Vec<f64> = family(&h).planes.iter().map(|p| p.start[0]).collect();
    assert!(xs.iter().any(|x| near(*x, 1000.0)) && xs.iter().any(|x| near(*x, 750.0)), "{xs:?} {}", status(&h));
    assert_eq!(editor(&h).steps(), 1);
    // A form says what Offset takes.
    click(&mut h, [300.0, 300.0]);
    assert!(status(&h).contains("Offset takes reference planes and reference lines"), "{}", status(&h));
}

#[test]
fn trim_split_and_the_selection_tools_with_nothing_selected_say_why_outside_a_sketch() {
    let mut h = plan();
    for (name, said) in [
        ("Split Element", "works on a sketch's lines"),
        ("Trim/Extend Single Element", "works on a sketch's lines"),
        ("Move", "Select what Move takes first"),
        ("Rotate", "Select what Rotate takes first"),
    ] {
        h.state_mut().status = crate::Status::default();
        command(&mut h, name);
        assert!(status(&h).contains(said), "{name}: {}", status(&h));
        assert!(h.state().status.error);
        assert_eq!(tool(&h), None);
    }
    // In {3D} Move works, on the work plane; Offset, which picks from the view, is a plan's or an elevation's still.
    super::family_views::view(&mut h, None);
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Offset");
    assert!(status(&h).contains("plan or an elevation"), "{}", status(&h));
    assert_eq!(tool(&h), None);
    command(&mut h, "Move");
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }));
}

#[test]
fn rotating_a_form_a_label_ties_to_a_datum_asks_and_remove_constraints_does_it() {
    // Len (100) from Center (Left/Right) to the box's west side.
    let dim = format!(
        r#", {{"refs": [{{"plane": "{CENTER_LR}"}}, {{"form": "{A}", "line": "{A_LINE}3"}}], "view": "plan", "at": [50, 0, 0], "label": "Len"}}"#
    );
    let mut h = plan_of(&family_with(&dim));
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0);
    command(&mut h, "Rotate");
    click(&mut h, [700.0, 300.0]);
    click(&mut h, [300.0, 700.0]);
    // Nothing turned yet: asked.
    assert_eq!(points(&h, 0), before);
    let asked = editor(&h).views.unsatisfied.clone().expect("Constraints are not satisfied");
    assert!(asked.message.contains("ties what is rotated to what isn't"), "{}", asked.message);
    press(&mut h, "Remove Constraints");
    h.run();
    assert!(editor(&h).views.unsatisfied.is_none());
    for (a, b) in before.iter().zip(points(&h, 0)) {
        assert!(same(b, [600.0 - a[1], a[0]]), "{a:?} → {b:?} {}", status(&h));
    }
    // The label taken off; its dimension, which turned to face across Center (Left/Right), deleted.
    assert!(family(&h).dimensions.iter().all(|d| d.label.is_none()), "the label taken off");
    assert_eq!(family(&h).dimensions.len(), 1);
    assert_eq!(editor(&h).steps(), 1, "the constraints and the turn one step");
}

#[test]
fn remove_constraints_refused_again_leaves_neither_the_constraints_off_nor_a_step() {
    let dim = format!(
        r#", {{"refs": [{{"plane": "{CENTER_LR}"}}, {{"form": "{A}", "line": "{A_LINE}3"}}], "view": "plan", "at": [50, 0, 0], "label": "Len"}}"#
    );
    let mut h = plan_of(&family_with(&dim));
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Rotate");
    click(&mut h, [700.0, 300.0]);
    click(&mut h, [300.0, 700.0]);
    assert!(editor(&h).views.unsatisfied.is_some());
    // Meanwhile another lock ties the box's east side (x 500) to Center (Left/Right): the turn asked again would break it.
    let lock: bimcraft_core::family::FamilyDimension = serde_json::from_value(serde_json::json!({
        "refs": [{"plane": CENTER_LR}, {"form": A, "line": format!("{A_LINE}1")}], "view": "plan", "at": [250, -50, 0],
        "locked": [{"segment": 0, "length": 500}]
    }))
    .unwrap();
    ed(&mut h).dimensions.push(lock);
    h.run();
    let before = family(&h);
    press(&mut h, "Remove Constraints");
    h.run();
    let after = family(&h);
    assert_eq!(after.forms, before.forms, "{}", status(&h));
    assert_eq!(after.dimensions, before.dimensions, "the label still on");
    assert!(status(&h).contains("not satisfied"), "{}", status(&h));
}

#[test]
fn a_formula_the_rotation_would_change_refuses_it_leaving_everything_as_it_was() {
    // The box's first corner's x is Len (100): a turn would change it.
    let script = family_with("").replacen(r#""start": [100, 100]"#, r#""start": ["Len", 100]"#, 1);
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Form(0)]);
    let before = family(&h).forms[0].clone();
    command(&mut h, "Rotate");
    click(&mut h, [700.0, 300.0]);
    click(&mut h, [300.0, 700.0]);
    assert!(status(&h).contains("worked out by a formula"), "{}", status(&h));
    assert!(h.state().status.error);
    assert_eq!(family(&h).forms[0], before);
    assert_eq!(editor(&h).steps(), 0);
}

#[test]
fn in_a_sketch_the_tools_act_on_the_lines_picked_and_split_cuts_the_line_clicked() {
    let mut h = plan();
    ed(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    // Nothing picked: Move says to pick lines first.
    command(&mut h, "Move");
    assert!(status(&h).contains("Pick lines of the sketch first"), "{}", status(&h));
    {
        let s = ed(&mut h).sketch.as_mut().unwrap();
        s.sketch.picked = (0..4).collect();
    }
    h.run();
    command(&mut h, "Move");
    assert!(matches!(tool(&h), Some(M::Move { .. })), "{}", status(&h));
    click(&mut h, [100.0, 100.0]);
    click(&mut h, [300.0, 100.0]);
    let starts: Vec<_> = ed(&mut h).sketch.as_ref().unwrap().sketch.lines.iter().map(|l| l.curve.start()).collect();
    assert!(starts.iter().any(|p| near(p.0, 300.0) && near(p.1, 100.0)), "{starts:?} {}", status(&h));
    // Split the south line (now 300 to 700 along y 100) at x 500.
    command(&mut h, "Split Element");
    assert_eq!(tool(&h), Some(M::Split));
    click(&mut h, [500.0, 100.0]);
    assert_eq!(ed(&mut h).sketch.as_ref().unwrap().sketch.lines.len(), 5, "{}", status(&h));
    // Mirror - Draw Axis, Copy off, across x 0: the lines picked go west.
    {
        let e = ed(&mut h);
        e.views.modify_options.mirror_copy = false;
        let s = e.sketch.as_mut().unwrap();
        s.sketch.picked = (0..5).collect();
    }
    command(&mut h, "Mirror - Draw Axis");
    click(&mut h, [0.0, -100.0]);
    click(&mut h, [0.0, 600.0]);
    let xs: Vec<f64> = ed(&mut h).sketch.as_ref().unwrap().sketch.lines.iter().map(|l| l.curve.start().0).collect();
    assert!(xs.iter().all(|x| *x < 0.0), "{xs:?} {}", status(&h));
    // Finished, the form is where the sketch put it.
    press(&mut h, "Finish Edit Mode");
    assert!(ed(&mut h).sketch.is_none(), "{}", status(&h));
    assert!(points(&h, 0).iter().all(|p| p[0] <= 0.0 && p[0] >= -700.0 - 1e-6), "{:?}", points(&h, 0));
}

#[test]
fn in_a_sketch_offset_copies_a_line_and_trim_extend_single_brings_one_to_a_boundary() {
    let mut h = plan();
    ed(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    command(&mut h, "Offset");
    ed(&mut h).views.modify_options.offset = "50".into();
    // The north line (y 500), clicked just inside: a copy at y 450.
    click(&mut h, [300.0, 495.0]);
    let lines = ed(&mut h).sketch.as_ref().unwrap().sketch.lines.clone();
    assert_eq!(lines.len(), 5, "{}", status(&h));
    let new = lines.last().unwrap();
    assert!(near(new.curve.start().1, 450.0) && near(new.curve.end().1, 450.0), "{:?}", new.curve);
    assert_ne!(new.guid, lines[2].guid);
    // Trim/Extend Single Element: the copy cut back to the east side (x 500)... it runs 500 to 100: extend its start past
    // nothing, so trim its west end to the west side kept east of it.
    {
        let e = ed(&mut h);
        e.modify_tool_on(M::TrimTo { boundary: None, many: false }).unwrap();
    }
    h.run();
    click(&mut h, [100.0, 300.0]);
    assert!(matches!(tool(&h), Some(M::TrimTo { boundary: Some(3), .. })), "{:?}", tool(&h));
    click(&mut h, [300.0, 450.0]);
    let l = ed(&mut h).sketch.as_ref().unwrap().sketch.lines[4].clone();
    let xs = [l.curve.start().0, l.curve.end().0];
    assert!(xs.iter().any(|x| near(*x, 100.0)) && xs.iter().any(|x| near(*x, 500.0)), "{xs:?} {}", status(&h));
}

#[test]
fn undo_delete_and_another_selection_let_go_of_what_a_tool_clicked() {
    // In the views: Move's start goes with an undo, and with another selection.
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Move");
    click(&mut h, [300.0, 300.0]);
    click(&mut h, [400.0, 300.0]);
    command(&mut h, "Rotate");
    if let Some(m) = ed(&mut h).views.modify.as_mut() {
        m.tool = M::Rotate { centre: None, start: None, placing: true };
    }
    click(&mut h, [0.0, 0.0]);
    assert!(matches!(tool(&h), Some(M::Rotate { centre: Some(_), placing: false, .. })));
    h.state_mut().family_undo();
    h.run();
    h.run();
    assert!(matches!(tool(&h), Some(M::Rotate { start: None, placing: false, .. })), "{:?}", tool(&h));
    let centre = match tool(&h) {
        Some(M::Rotate { centre: Some(c), .. }) => c,
        t => panic!("{t:?}"),
    };
    assert!(near(centre.0, 300.0) && near(centre.1, 300.0), "the placed centre let go, the selection's found again: {centre:?}");
    command(&mut h, "Copy");
    click(&mut h, [300.0, 300.0]);
    assert!(matches!(tool(&h), Some(M::Move { from: Some(_), .. })));
    pick(&mut h, &[Picked::Form(1)]);
    assert_eq!(tool(&h), Some(M::Move { copy: true, from: None }));
    // In a sketch: Trim/Extend's boundary goes when its line is deleted, and with Ctrl+Z.
    ed(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    ed(&mut h).modify_tool_on(M::TrimTo { boundary: None, many: true }).unwrap();
    h.run();
    click(&mut h, [100.0, 300.0]);
    assert!(matches!(tool(&h), Some(M::TrimTo { boundary: Some(3), .. })), "{:?}", tool(&h));
    {
        let s = ed(&mut h).sketch.as_mut().unwrap();
        s.sketch.picked = [3].into();
        s.delete_picked();
    }
    h.run();
    assert_eq!(tool(&h), Some(M::TrimTo { boundary: None, many: true }));
    click(&mut h, [500.0, 300.0]);
    assert!(matches!(tool(&h), Some(M::TrimTo { boundary: Some(_), .. })));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(tool(&h), Some(M::TrimTo { boundary: None, many: true }));
}

#[test]
fn sketch_scale_scales_a_circles_radius() {
    let mut h = plan();
    ed(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    {
        let s = ed(&mut h).sketch.as_mut().unwrap();
        s.sketch.lines.push(bimcraft_core::sketch::SketchLine::from(bimcraft_core::curves::Curve::Circle {
            centre: bimcraft_core::geom::P2(300.0, 300.0),
            radius: 50.0,
        }));
        s.sketch.picked = [4].into();
    }
    h.run();
    command(&mut h, "Scale");
    {
        let o = &mut ed(&mut h).views.modify_options;
        o.numerical = true;
        o.factor = "2".into();
    }
    click(&mut h, [300.0, 300.0]);
    let c = ed(&mut h).sketch.as_ref().unwrap().sketch.lines[4].curve.clone();
    assert!(matches!(c, bimcraft_core::curves::Curve::Circle { radius, .. } if near(radius, 100.0)), "{c:?} {}", status(&h));
}

#[test]
fn the_ribbon_greys_a_tool_that_cant_be_used_and_its_button_picks_it() {
    let mut h = plan();
    // Nothing selected: Move greyed; Offset enabled.
    assert!(editor(&h).modify_blocked(&M::Move { copy: false, from: None }).is_some());
    assert!(editor(&h).modify_blocked(&M::Offset).is_none());
    pick(&mut h, &[Picked::Form(0)]);
    press(&mut h, "Rotate");
    assert!(matches!(tool(&h), Some(M::Rotate { .. })), "{}", status(&h));
    assert!(editor(&h).modify_blocked(&M::Split).is_some());
}

/// Each tool at work in plan, saved where `BIMCRAFT_MODIFY_TOOLS_DUMP` says (only then): its marks, the preview of where the click would
/// put the selection, its Options Bar and its prompt.
#[test]
fn family_modify_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_MODIFY_TOOLS_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let start = || {
        let dim = format!(
            r#", {{"refs": [{{"plane": "{CENTER_LR}"}}, {{"form": "{A}", "line": "{A_LINE}3"}}], "view": "plan", "at": [50, 0, 0], "label": "Len"}}"#
        );
        let mut h = app(&family_with(&dim));
        h.set_size(vec2(1600.0, 1000.0));
        h.state_mut().edit_family("Movers");
        h.run();
        super::family_views::view(&mut h, Some(bimcraft_core::family::views::Which::Plan));
        h
    };
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("modify-{name}.png"))).unwrap();
    };
    let hover = |h: &mut Harness<BimApp>, p: [f64; 2]| {
        let at = editor(h).views.screen_of([p[0], p[1], 0.0]).unwrap();
        h.hover_at(at);
        h.run();
    };
    // Move: from the box's corner, the pointer north-east.
    let mut h = start();
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Move");
    click(&mut h, [-1000.0, 100.0]);
    hover(&mut h, [-500.0, 900.0]);
    save(&mut h, "move");
    // Copy with Multiple: two copies made, the third under way.
    command(&mut h, "Copy");
    ed(&mut h).views.modify_options.multiple = true;
    click(&mut h, [-1000.0, 100.0]);
    click(&mut h, [-1000.0, 700.0]);
    hover(&mut h, [-1000.0, 1300.0]);
    save(&mut h, "copy-multiple");
    // Rotate: the box and the plane Edge about their centre, the first ray east, the pointer at 60°.
    let mut h = start();
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Rotate");
    click(&mut h, [-400.0, 300.0]);
    hover(&mut h, [-800.0 + 400.0 * 0.5, 300.0 + 400.0 * 0.866]);
    save(&mut h, "rotate");
    // Mirror - Pick Axis over Edge.
    let mut h = start();
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Mirror - Pick Axis");
    hover(&mut h, [1000.0, 600.0]);
    save(&mut h, "mirror-pick");
    click(&mut h, [1000.0, 600.0]);
    save(&mut h, "mirror-picked");
    // Mirror - Draw Axis: the axis' first point, the pointer at its second.
    let mut h = start();
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Mirror - Draw Axis");
    click(&mut h, [-300.0, -400.0]);
    hover(&mut h, [-300.0, 1000.0]);
    save(&mut h, "mirror-draw");
    // Scale, Graphical, on Edge and the reference line: origin, a point, the pointer half as far.
    let mut h = start();
    pick(&mut h, &[Picked::Plane(g(EDGE)), Picked::Line(g(super::family_drag_move::LINE))]);
    command(&mut h, "Scale");
    click(&mut h, [0.0, 0.0]);
    click(&mut h, [1000.0, 0.0]);
    hover(&mut h, [1400.0, 0.0]);
    save(&mut h, "scale");
    // Scale on a form: greyed, the reason in the status bar.
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Scale");
    save(&mut h, "scale-form-refused");
    // Offset: Edge copied 300 west.
    let mut h = start();
    command(&mut h, "Offset");
    hover(&mut h, [990.0, 600.0]);
    save(&mut h, "offset");
    click(&mut h, [990.0, 600.0]);
    save(&mut h, "offset-done");
    // Rotate the box its label ties to Center (Left/Right): asked.
    let mut h = start();
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Rotate");
    click(&mut h, [700.0, 300.0]);
    click(&mut h, [300.0, 700.0]);
    save(&mut h, "constraints");
    // In a sketch: the lines picked moved, then Split and Trim/Extend Single Element's prompts.
    let mut h = start();
    ed(&mut h).edit_form_sketch(1).unwrap();
    h.run();
    ed(&mut h).sketch.as_mut().unwrap().sketch.picked = (0..4).collect();
    command(&mut h, "Rotate");
    click(&mut h, [-400.0, 300.0]);
    hover(&mut h, [-500.0, 700.0]);
    save(&mut h, "sketch-rotate");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    command(&mut h, "Split Element");
    hover(&mut h, [-800.0, 100.0]);
    save(&mut h, "sketch-split");
    click(&mut h, [-800.0, 100.0]);
    ed(&mut h).modify_tool_on(M::TrimTo { boundary: None, many: true }).unwrap();
    click(&mut h, [-1000.0, 300.0]);
    hover(&mut h, [-700.0, 500.0]);
    save(&mut h, "sketch-trim-multiple");
    command(&mut h, "Offset");
    hover(&mut h, [-800.0, 480.0]);
    save(&mut h, "sketch-offset");
    // In the Front elevation: a half turn about a centre placed on the level line, under way and done; the box below the level.
    let mut h = start();
    super::family_views::view(&mut h, Some(Which::Elevation(bimcraft_core::family::views::Side::Front)));
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Rotate");
    if let Some(m) = ed(&mut h).views.modify.as_mut() {
        m.tool = M::Rotate { centre: None, start: None, placing: true };
    }
    click3(&mut h, [300.0, 0.0, 0.0]);
    click3(&mut h, [700.0, 0.0, 0.0]);
    let at = editor(&h).views.screen_of([300.0, 0.0, -400.0]).unwrap();
    h.hover_at(at);
    h.run();
    save(&mut h, "elevation-rotate");
    click3(&mut h, [-100.0, 0.0, 0.0]);
    // Len ties the box to Center (Left/Right): asked, then done without it.
    press(&mut h, "Remove Constraints");
    save(&mut h, "elevation-rotated");
    // A dimension alone selected: Rotate says why not.
    pick(&mut h, &[Picked::Dimension(g(super::family_drag_move::DIM))]);
    super::family_views::view(&mut h, Some(Which::Plan));
    pick(&mut h, &[Picked::Dimension(g(super::family_drag_move::DIM))]);
    command(&mut h, "Rotate");
    save(&mut h, "dimension-alone-refused");
}

#[test]
fn a_split_line_keeps_what_measured_to_its_end_on_its_end() {
    use bimcraft_core::family::{DimRef, Spot};
    // A plain dimension from box A's south line's end (500, 100) to Edge (x 1000).
    let dim = format!(
        r#", {{"refs": [{{"form": "{A}", "line": "{A_LINE}0", "point": "end"}}, {{"plane": "{EDGE}"}}], "view": "plan", "at": [750, 0, 0]}}"#
    );
    let mut h = plan_of(&family_with(&dim));
    ed(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    command(&mut h, "Split Element");
    click(&mut h, [300.0, 100.0]);
    let s = ed(&mut h).sketch.as_ref().unwrap();
    assert_eq!(s.sketch.lines.len(), 5, "{}", status(&h));
    let far =
        s.sketch.lines.iter().find(|l| same([l.curve.end().0, l.curve.end().1], [500.0, 100.0]) && l.guid != g(&format!("{A_LINE}0")));
    let far = far.expect("the new half, from the split to the line's end").guid;
    press(&mut h, "Finish Edit Mode");
    assert!(ed(&mut h).sketch.is_none(), "{}", status(&h));
    let f = family(&h);
    let d = f.dimensions.iter().find(|d| matches!(d.refs[0], DimRef::Point(..))).expect("the dimension");
    assert_eq!(d.refs[0], DimRef::Point(g(A), far, Spot::End), "it measures to the same point, now the new half's end");
    assert_eq!(d.refs[1], DimRef::Plane(g(EDGE)));
}
