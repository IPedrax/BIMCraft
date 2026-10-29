//! Sketch mode: an outline drawn as lines, Finish Edit Mode solving them into the floor's (or a ceiling's, a roof's, a
//! shaft's) outline and holes, and what's wrong with them shown on the lines.

use super::*;
use bimcraft_core::sketch::SketchLine;

/// The plan with the box of walls floored, the floor selected.
fn floored() -> (Harness<'static, BimApp>, Id) {
    let mut h = app(&format!("{BOX}\nfloor {{}}"));
    let f = ids(&h, "floor")[0];
    h.state_mut().selection = vec![f];
    h.run();
    press(&mut h, "Modify | Floors");
    (h, f)
}

fn sketched(h: &Harness<BimApp>) -> usize {
    h.state().sketch.as_ref().map_or(0, |s| s.lines.len())
}

#[test]
fn edit_boundary_draws_a_hole_with_a_rectangle_and_finish_makes_it_the_floors_in_one_step() {
    let (mut h, f) = floored();
    press(&mut h, "Edit Boundary");
    assert!(h.query_by_label("Modify | Edit Boundary").is_some());
    assert_eq!(sketched(&h), 4, "its outline, as lines");
    assert!(h.state().selection.is_empty());
    press(&mut h, "Rectangle");
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(sketched(&h), 8);
    assert_eq!(field(&h, f, "openings"), json!(null), "nothing is the floor's until Finish");
    let steps = h.state().session.history().len();
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    assert_eq!(field(&h, f, "openings").as_array().map(Vec::len), Some(1));
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert_eq!(h.state().session.history().last(), Some(&"Edit Boundary"));
    assert_eq!(h.state().selection, vec![f], "selected again, as Revit leaves it");
}

#[test]
fn finish_names_what_is_wrong_on_the_lines_and_cancel_leaves_the_floor_as_it_was() {
    let (mut h, f) = floored();
    let before = field(&h, f, "boundary");
    press(&mut h, "Edit Boundary");
    // Modify picks a sketch line, and Delete takes it out.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.state().sketch.is_some(), "Esc leaves the sketch open");
    click(&mut h, P2(4000.0, 6000.0));
    assert_eq!(h.state().sketch.as_ref().map(|s| s.picked.len()), Some(1));
    key(&mut h, Key::Delete);
    assert_eq!(sketched(&h), 3);
    press(&mut h, "Finish Edit Mode");
    let st = h.state();
    assert!(st.status.error && st.status.text.contains("The loop is open: 2 line ends don't meet another line."), "{}", st.status.text);
    assert!(st.status.text.contains("at x ") && st.status.text.contains("shown in red"), "{}", st.status.text);
    let sk = st.sketch.as_ref().expect("still sketching");
    let wrong: Vec<&SketchLine> = sk.wrong.iter().map(|&i| &sk.lines[i]).collect();
    assert_eq!(wrong.len(), 2, "the two lines left with a loose end");
    assert!(wrong.iter().all(|l| [l.start(), l.end()].iter().any(|p| p.1 == 6000.0)), "{wrong:?}");
    press(&mut h, "Cancel Edit Mode");
    assert!(h.state().sketch.is_none());
    assert_eq!(field(&h, f, "boundary"), before);
}

#[test]
fn a_floor_is_sketched_line_by_line_closed_on_its_first_point_and_ctrl_z_takes_back_a_line() {
    let mut h = app(BOX);
    press(&mut h, "Floor");
    assert!(h.query_by_label("Modify | Create Floor Boundary").is_some());
    for p in [P2(1000.0, 1000.0), P2(5000.0, 1000.0), P2(5000.0, 4000.0), P2(1000.0, 4000.0), P2(1000.0, 1000.0)] {
        click(&mut h, p);
    }
    assert_eq!(sketched(&h), 4);
    assert_eq!(h.state().tool, Tool::SketchLine(Vec::new()), "closed, the next line starts anew");
    click(&mut h, P2(6000.0, 1000.0));
    click(&mut h, P2(7000.0, 1000.0));
    assert_eq!(sketched(&h), 5);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(sketched(&h), 4, "Ctrl+Z takes back a sketch line, not a step of the model's");
    key(&mut h, Key::Escape);
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    let floors = ids(&h, "floor");
    assert_eq!(floors.len(), 1);
    assert_eq!(field(&h, floors[0], "area_m2"), json!(12.0));
}

/// The sketch's lines.
fn lines(h: &Harness<BimApp>) -> Vec<SketchLine> {
    h.state().sketch.as_ref().map(|s| s.lines.clone()).unwrap_or_default()
}

fn picked(h: &Harness<BimApp>) -> Vec<usize> {
    h.state().sketch.as_ref().map(|s| s.picked.iter().copied().collect()).unwrap_or_default()
}

/// The one line of the sketch from `a` to `b`, either way round.
fn line_of(h: &Harness<BimApp>, a: P2, b: P2) -> Option<usize> {
    let near = |p: P2, q: P2| p.dist(q) < 1.0;
    lines(h).iter().position(|l| (near(l.start(), a) && near(l.end(), b)) || (near(l.start(), b) && near(l.end(), a)))
}

#[test]
fn a_finished_sketchs_lines_have_ids_that_edit_boundary_brings_back() {
    let mut h = app(BOX);
    press(&mut h, "Floor");
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 4000.0));
    assert!(lines(&h).iter().all(|l| l.id == 0), "new lines have no id until Finish");
    press(&mut h, "Finish Edit Mode");
    let f = ids(&h, "floor")[0];
    press(&mut h, "Modify | Floors");
    press(&mut h, "Edit Boundary");
    let ls = lines(&h);
    assert_eq!(ls.len(), 4);
    assert!(ls.iter().all(|l| l.id >= 100_000), "{ls:?}");
    // Kept through another edit: a hole added, Finish keeps the four.
    press(&mut h, "Rectangle");
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(3000.0, 3000.0));
    press(&mut h, "Finish Edit Mode");
    assert_eq!(field(&h, f, "openings").as_array().map(Vec::len), Some(1));
    let kept: Vec<u64> = field(&h, f, "sketch").as_array().unwrap().iter().filter_map(|l| l["id"].as_u64()).collect();
    assert_eq!(kept.len(), 8);
    assert!(ls.iter().all(|l| kept.contains(&l.id)), "{kept:?}");
}

#[test]
fn intersecting_lines_say_so_first() {
    let mut h = app(BOX);
    press(&mut h, "Floor");
    // A bow tie.
    for p in [P2(1000.0, 1000.0), P2(5000.0, 4000.0), P2(5000.0, 1000.0), P2(1000.0, 4000.0), P2(1000.0, 1000.0)] {
        click(&mut h, p);
    }
    press(&mut h, "Finish Edit Mode");
    let text = h.state().status.text.clone();
    assert!(text.starts_with("Lines intersect (1): a loop can't cross itself or another loop."), "{text}");
    assert!(!text.contains("open"), "{text}");
    assert_eq!(h.state().sketch.as_ref().map(|s| s.wrong.len()), Some(2));
}

#[test]
fn sketch_lines_are_picked_by_a_slow_click_and_by_window_and_crossing_boxes() {
    let (mut h, _) = floored();
    press(&mut h, "Edit Boundary");
    key(&mut h, Key::Escape);
    // A press held a second before it comes up, the hand still: a click.
    let at = h.state().screen(P2(4000.0, 0.0));
    h.hover_at(at);
    h.run();
    h.event(button(at, true));
    h.run_steps(4);
    h.event(button(at, false));
    h.run();
    assert_eq!(picked(&h), line_of(&h, P2(0.0, 0.0), P2(8000.0, 0.0)).into_iter().collect::<Vec<_>>());
    key(&mut h, Key::Escape);
    // From the left, a window: the south line wholly inside it.
    drag(&mut h, P2(-500.0, -500.0), P2(8500.0, 500.0));
    assert_eq!(picked(&h), line_of(&h, P2(0.0, 0.0), P2(8000.0, 0.0)).into_iter().collect::<Vec<_>>());
    // From the right, crossing: the east line touched.
    drag(&mut h, P2(8500.0, 3500.0), P2(7500.0, 2500.0));
    assert_eq!(picked(&h), line_of(&h, P2(8000.0, 0.0), P2(8000.0, 6000.0)).into_iter().collect::<Vec<_>>());
    assert!(h.state().selection.is_empty(), "the model's selection is left alone");
}

#[test]
fn a_picked_lines_end_is_a_grip_that_takes_the_lines_joined_there_with_it() {
    let (mut h, f) = floored();
    press(&mut h, "Edit Boundary");
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 0.0));
    drag(&mut h, P2(0.0, 0.0), P2(-1000.0, -1000.0));
    assert!(line_of(&h, P2(-1000.0, -1000.0), P2(8000.0, 0.0)).is_some(), "{:?}", lines(&h));
    assert!(line_of(&h, P2(-1000.0, -1000.0), P2(0.0, 6000.0)).is_some(), "{:?}", lines(&h));
    press(&mut h, "Finish Edit Mode");
    assert!(field(&h, f, "boundary").as_array().unwrap().contains(&json!([-1000.0, -1000.0])));
}

#[test]
fn arcs_and_picked_walls_close_loops_too() {
    let mut h = app(BOX);
    press(&mut h, "Floor");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 1000.0));
    press(&mut h, "Start-End-Radius Arc");
    click(&mut h, P2(5000.0, 1000.0));
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 3000.0));
    assert_eq!(lines(&h).len(), 2);
    assert!(lines(&h)[1].curve.kind() == "arc");
    press(&mut h, "Finish Edit Mode");
    let half = ids(&h, "floor");
    assert_eq!(half.len(), 1, "{}", h.state().status.text);
    let area = field(&h, half[0], "area_m2").as_f64().unwrap();
    assert!((area - std::f64::consts::TAU).abs() < 0.1, "{area}");
    // Pick Walls: a line along each wall clicked.
    press(&mut h, "Architecture");
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    for p in [P2(4000.0, 0.0), P2(8000.0, 3000.0), P2(4000.0, 6000.0), P2(0.0, 3000.0)] {
        click(&mut h, p);
    }
    assert_eq!(lines(&h).len(), 4);
    press(&mut h, "Finish Edit Mode");
    assert_eq!(ids(&h, "floor").len(), 2, "{}", h.state().status.text);
}

#[test]
fn a_line_snaps_to_the_middle_of_a_sketch_line() {
    let mut h = app(BOX);
    press(&mut h, "Floor");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 1000.0));
    key(&mut h, Key::Escape);
    click(&mut h, P2(3040.0, 1030.0));
    click(&mut h, P2(3000.0, 4000.0));
    assert_eq!(lines(&h)[1].start(), P2(3000.0, 1000.0), "{:?}", lines(&h));
}

#[test]
fn finish_cancel_and_edit_boundary_stand_at_the_ribbons_right_end() {
    let (mut h, _) = floored();
    let width = h.ctx.content_rect().width();
    let x = |h: &Harness<BimApp>, label: &str| h.get_by_label(label).rect().center().x;
    assert!(x(&h, "Edit Boundary") > width * 0.75, "{} of {width}", x(&h, "Edit Boundary"));
    press(&mut h, "Edit Boundary");
    let (fx, cx) = (x(&h, "Finish Edit Mode"), x(&h, "Cancel Edit Mode"));
    assert!(fx > width * 0.75 && cx > width * 0.75, "{fx} {cx} of {width}");
    assert!(x(&h, "Line") < width * 0.5);
}

#[test]
fn with_an_offset_left_on_the_options_bar_a_rectangle_stays_joined_and_a_spline_is_still_drawn() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Filled Region");
    type_into(&mut h, "Offset:", "200");
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 3000.0));
    press(&mut h, "Spline");
    for p in [P2(5000.0, 1000.0), P2(6000.0, 2000.0), P2(5000.0, 3000.0)] {
        click(&mut h, p);
    }
    key(&mut h, Key::Enter);
    assert_eq!(lines(&h).last().map(|l| l.curve.kind()), Some("spline"), "{:?}", lines(&h));
    press(&mut h, "Line");
    type_into(&mut h, "Offset:", "0");
    click(&mut h, P2(5000.0, 3000.0));
    click(&mut h, P2(5000.0, 1000.0));
    key(&mut h, Key::Escape);
    press(&mut h, "Finish Edit Mode");
    let r = ids(&h, "region");
    assert_eq!(r.len(), 1, "{}", h.state().status.text);
}

#[test]
fn leaving_the_sketchs_plan_takes_the_window_back_and_keeps_the_tool_it_was_drawing_with() {
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"));
    let (plan, section) = (h.state().sheet, ids(&h, "section")[0]);
    press(&mut h, "Annotate");
    press(&mut h, "Filled Region");
    press(&mut h, "Rectangle");
    assert!(matches!(h.state().tool, Tool::SketchLine(_)));
    // A tab or the Project Browser opens another view: the window goes back to the plan, and the sketch goes on with its tool.
    h.state_mut().open_sheet(Sheet::Section(section));
    h.run();
    assert_eq!(h.state().sheet, plan, "back in the plan");
    assert!(h.state().sketch.is_some());
    assert!(matches!(h.state().tool, Tool::SketchLine(_)), "{:?}: the tool is the sketch's, not Modify", h.state().tool);
    assert_eq!(h.state().shape(), crate::draw::Shape::Rectangle, "and so is the shape");
}

#[test]
fn a_sketch_whose_plan_was_deleted_does_not_hold_the_tool_in_a_view_that_cannot_use_it() {
    let script =
        format!("{BOX}\nlevel {{\"name\": \"Spare\", \"elevation\": 12000}}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}");
    let mut h = app(&script);
    let section = ids(&h, "section")[0];
    let upper = h.state().session.model.levels().iter().find(|(_, l)| l.name == "Spare").map(|(id, _)| *id).unwrap();
    h.state_mut().open_sheet(Sheet::Plan(upper));
    h.run();
    press(&mut h, "Annotate");
    press(&mut h, "Filled Region");
    press(&mut h, "Rectangle");
    assert!(matches!(h.state().tool, Tool::SketchLine(_)));
    // The plan it began in goes (a script, or an undo that takes the level with it): the sketch stays where it is, and a view that
    // takes no clicks for a sketch takes the window's tool back to Modify, as it does for anything else.
    h.state_mut().session.execute("delete", &json!({"ids": [upper]})).unwrap();
    h.run();
    h.state_mut().open_sheet(Sheet::Section(section));
    h.run();
    assert_eq!(h.state().sheet, Sheet::Section(section), "no plan to go back to");
    assert!(h.state().sketch.is_some());
    assert_eq!(h.state().tool, Tool::Modify, "{:?}: the sketch's plan is gone, so the tool is not held", h.state().tool);
}

#[test]
fn a_project_put_in_the_sessions_place_under_a_sketch_or_a_dialog_takes_both_with_it() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Filled Region");
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 3000.0));
    let model = h.state().session.model.clone();
    h.state_mut().fill_patterns_dialog = Some(crate::fillpatterns::FillPatternsDialog::new(&model));
    h.run();
    // New, through the session as a script or a test does (the control port turns an agent's New away): another project.
    h.state_mut().session.execute("new", &json!({})).unwrap();
    h.run();
    assert!(h.state().sketch.is_none(), "the sketch was of the old project");
    assert!(h.state().fill_patterns_dialog.is_none(), "so was the dialog's draft");
    assert!(h.query_by_label("Finish Edit Mode").is_none());
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(ids(&h, "region").is_empty());
}
