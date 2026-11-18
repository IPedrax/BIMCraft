//! Sketch mode: an outline drawn as lines, Finish Edit Mode solving them into the floor's (or a ceiling's, a roof's, a
//! shaft's) outline and holes, and what's wrong with them shown on the lines.

use super::*;
use bimcraft_core::sketch::SketchLine;
use egui_kittest::kittest::NodeT;

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

#[test]
fn a_sketch_line_picked_alone_has_a_palette_of_what_can_be_done_to_it() {
    let (mut h, _) = floored();
    press(&mut h, "Edit Boundary");
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("Add Vertex").is_none(), "nothing picked, no palette");
    click(&mut h, P2(4000.0, 6000.0));
    assert_eq!(picked(&h).len(), 1);
    press(&mut h, "Add Vertex");
    assert!(matches!(h.state().tool, Tool::Vertex { delete: false }));
}

/// A box of 200 mm walls with a 100 mm core between 50 mm finishes: Pick Walls' faces are 50 and 100 mm from the
/// centre line.
fn core_box() -> String {
    r#"type {"category": "wall", "name": "C60", "layers": [{"function": "finish", "thickness": 50}, {"function": "structure", "thickness": 100}, {"function": "finish", "thickness": 50}]}
wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true, "type": "C60"}"#
        .to_string()
}

/// The same walls round an L, six of them, their location lines at their centres.
fn core_l() -> String {
    r#"type {"category": "wall", "name": "C60", "layers": [{"function": "finish", "thickness": 50}, {"function": "structure", "thickness": 100}, {"function": "finish", "thickness": 50}]}
wall {"points": [[0, 0], [8000, 0], [8000, 3000], [4000, 3000], [4000, 6000], [0, 6000]], "closed": true, "type": "C60"}"#
        .to_string()
}

/// Where the sketch's one line lies across the y axis, by its middle.
fn line_y(h: &Harness<BimApp>, i: usize) -> f64 {
    lines(h).get(i).map_or(f64::NAN, |l| l.middle().1)
}

#[test]
fn pick_walls_puts_a_line_on_the_core_or_the_finish_face_under_the_cursor() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    // From inside the room, the south wall's core face is 50 mm into it.
    click(&mut h, P2(4000.0, 50.0));
    assert_eq!(lines(&h).len(), 1);
    assert!((line_y(&h, 0) - 50.0).abs() < 1e-6, "{:?}", lines(&h));
    // From outside, the other core face.
    click(&mut h, P2(4000.0, -50.0));
    assert_eq!(lines(&h).len(), 2);
    assert!((line_y(&h, 1) + 50.0).abs() < 1e-6, "{:?}", lines(&h));
    // The finish instead of the core.
    press(&mut h, "Extend into wall (to core)");
    click(&mut h, P2(4000.0, 50.0));
    assert_eq!(lines(&h).len(), 3);
    assert!((line_y(&h, 2) - 100.0).abs() < 1e-6, "{:?}", lines(&h));
}

#[test]
fn space_flips_a_picked_wall_line_to_the_other_face() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    key(&mut h, Key::Space);
    click(&mut h, P2(4000.0, 50.0));
    assert!((line_y(&h, 0) + 50.0).abs() < 1e-6, "Space puts it on the other core face: {:?}", lines(&h));
}

#[test]
fn pick_walls_offset_moves_the_line_away_from_the_wall() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    type_into(&mut h, "Offset:", "100");
    click(&mut h, P2(4000.0, 50.0));
    assert!((line_y(&h, 0) - 150.0).abs() < 1e-6, "{:?}", lines(&h));
    type_into(&mut h, "Offset:", "-100");
    click(&mut h, P2(4000.0, 50.0));
    assert!((line_y(&h, 1) + 50.0).abs() < 1e-6, "{:?}", lines(&h));
}

#[test]
fn walls_outline_follows_the_face_options_and_finish_makes_the_floor() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Walls' Outline");
    press(&mut h, "DRAW");
    let ls = lines(&h);
    assert_eq!(ls.len(), 4, "{ls:?}");
    assert!(ls.iter().all(|l| [l.start(), l.end()].iter().all(|p| p.0 == 50.0 || p.0 == 7950.0 || p.1 == 50.0 || p.1 == 5950.0)), "{ls:?}");
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    let f = ids(&h, "floor")[0];
    assert_eq!(field(&h, f, "area_m2"), json!(46.61));
}

#[test]
fn draw_does_not_lay_a_second_copy_of_an_outline_already_in_the_sketch() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Walls' Outline");
    press(&mut h, "DRAW");
    let drawn = lines(&h).len();
    assert_eq!(drawn, 4, "{:?}", lines(&h));
    assert!(h.query_by_label("DRAW").is_some(), "the panel stays until Walls' Outline is left");
    press(&mut h, "Walls' Outline");
    press(&mut h, "DRAW");
    assert_eq!(lines(&h).len(), drawn, "the same outline is not drawn twice: {:?}", lines(&h));
    assert!(h.state().status.text.contains("already"), "{}", h.state().status.text);
}

#[test]
fn walls_outline_picks_walls_with_the_arrow_and_esc_clears_them() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Walls' Outline");
    click_with(&mut h, P2(4000.0, 0.0), Modifiers::COMMAND);
    assert_eq!(h.state().outline_walls.len(), 1, "the south wall is picked");
    assert_eq!(h.state().selection.len(), 1, "and lit as the selection");
    key(&mut h, Key::Escape);
    assert!(h.state().outline_walls.is_empty(), "Esc clears the picked walls");
    assert!(h.state().selection.is_empty());
    assert!(h.state().outline_pending, "Esc leaves Walls' Outline on");
}

#[test]
fn picked_walls_with_offset_close_their_corners() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    type_into(&mut h, "Offset:", "25");
    for p in [P2(4000.0, 0.0), P2(8000.0, 3000.0), P2(4000.0, 6000.0), P2(0.0, 3000.0)] {
        click(&mut h, p);
    }
    let ls = lines(&h);
    assert_eq!(ls.len(), 4, "{ls:?}");
    for l in &ls {
        for q in [l.start(), l.end()] {
            assert!(
                ls.iter().any(|other| other != l && (other.start().dist(q) < 1e-6 || other.end().dist(q) < 1e-6)),
                "every end meets another line: {ls:?}"
            );
        }
    }
    press(&mut h, "Finish Edit Mode");
    assert_eq!(ids(&h, "floor").len(), 1, "{}", h.state().status.text);
}

#[test]
fn picked_walls_of_an_l_with_offset_close_every_corner() {
    let mut h = app(&core_l());
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    type_into(&mut h, "Offset:", "25");
    for p in [P2(4000.0, 0.0), P2(8000.0, 1500.0), P2(6000.0, 3000.0), P2(4000.0, 4500.0), P2(2000.0, 6000.0), P2(0.0, 3000.0)] {
        click(&mut h, p);
    }
    let ls = lines(&h);
    assert_eq!(ls.len(), 6, "{ls:?}");
    for l in &ls {
        for q in [l.start(), l.end()] {
            assert!(
                ls.iter().any(|other| other != l && (other.start().dist(q) < 1e-6 || other.end().dist(q) < 1e-6)),
                "every end meets another line: {ls:?}"
            );
        }
    }
    press(&mut h, "Finish Edit Mode");
    assert_eq!(ids(&h, "floor").len(), 1, "{}", h.state().status.text);
}

#[test]
fn a_picked_boundary_line_shows_its_wall_face_and_flips_and_undoes() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    click(&mut h, P2(4000.0, 50.0));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    click(&mut h, P2(4000.0, 50.0));
    assert!(h.get_all_by_label("Extend into wall (to core)").count() >= 1);
    // The checkbox in Properties puts it on the finish, and Ctrl+Z takes that back as one step.
    {
        let checkbox = h.get_all_by_label("Extend into wall (to core)").last().expect("the line's own checkbox");
        checkbox.click();
    }
    h.run();
    assert!((line_y(&h, 0) - 100.0).abs() < 1e-6, "{:?}", lines(&h));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!((line_y(&h, 0) - 50.0).abs() < 1e-6, "{:?}", lines(&h));
    // Undo clears the sketch's picked lines; pick it again for the palette.
    click(&mut h, P2(4000.0, 50.0));
    // Flip, from the palette over the line, puts it on the other side.
    press(&mut h, "Flip");
    assert!((line_y(&h, 0) + 50.0).abs() < 1e-6, "{:?}", lines(&h));
}

#[test]
fn a_grip_moved_on_a_picked_boundary_line_makes_it_an_ordinary_line() {
    let mut h = app(&core_box());
    press(&mut h, "Floor");
    press(&mut h, "Pick Walls");
    click(&mut h, P2(4000.0, 50.0));
    key(&mut h, Key::Escape);
    click(&mut h, P2(4000.0, 50.0));
    assert_eq!(h.state().sketch.as_ref().map(|sk| sk.picks.len()), Some(1));
    drag(&mut h, P2(50.0, 50.0), P2(-500.0, -500.0));
    assert_eq!(h.state().sketch.as_ref().map(|sk| sk.picks.len()), Some(0), "{:?}", lines(&h));
    // It is a line like any other: the wall's face no longer has a say in it.
    assert!(line_of(&h, P2(-500.0, -500.0), P2(7950.0, 50.0)).is_some(), "{:?}", lines(&h));
}

/// A roof over the box's footprint, its east and west edges at 45° and the others at the roof's 30°.
const SLOPED: &str = r#"roof {"boundary": [[0,0],[8000,0],[8000,6000],[0,6000]], "slope": 30, "slopes": [0, 45, 0, 45]}"#;

#[test]
fn several_roof_lines_show_mixed_slopes_and_edit_together_in_one_finish() {
    for rev in [false, true] {
        let script = r#"roof {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]], "slopes":[20,45,30,45], "gables":[2]}"#;
        let mut h = if rev { app(script) } else { bimcraft(script) };
        let roof = ids(&h, "roof")[0];
        let original = h.state().session.model.clone();
        let steps = h.state().session.history().len();
        for cancel in [true, false] {
            h.state_mut().selection = vec![roof];
            h.state_mut().edit_boundary();
            h.run();
            key(&mut h, Key::Escape);
            click(&mut h, P2(4000.0, 0.0));
            click_with(&mut h, P2(4000.0, 6000.0), Modifiers::CTRL);
            assert_eq!(h.state().sketch.as_ref().unwrap().picked, [0, 2].into());
            assert_eq!(h.state().session.history().len(), steps);
            if let Ok(folder) = std::env::var("BIMCRAFT_ROOF_MULTI_DUMP") {
                std::fs::create_dir_all(&folder).unwrap();
                for (look, dark, light) in [("light", false, false), ("dark", true, false), ("dark-light-canvas", true, true)] {
                    h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
                    h.state_mut().light_canvas = light;
                    h.run();
                    h.render()
                        .unwrap()
                        .save(std::path::Path::new(&folder).join(format!("multi-{}-{look}.png", if rev { "rev" } else { "bimcraft" })))
                        .unwrap();
                }
            }
            assert_eq!(
                h.get_by_role_and_label(Role::CheckBox, "Defines Slope").accesskit_node().toggled(),
                Some(egui::accesskit::Toggled::Mixed)
            );
            pick(&mut h, Role::CheckBox, "Defines Slope");
            assert!(h.state().sketch.as_ref().unwrap().lines.iter().all(|l| l.defines_slope == Some(true)));
            assert_eq!(h.get_by_role_and_label(Role::TextInput, "Slope").value().as_deref(), Some("Varies"));
            let draft = h.state().sketch.as_ref().unwrap().lines.clone();
            type_into(&mut h, "Slope", "400%");
            key(&mut h, Key::Enter);
            assert!(h.state().status.error);
            assert_eq!(h.state().sketch.as_ref().unwrap().lines, draft);
            for typed in ["22.5", "6:12", "50%"] {
                type_into(&mut h, "Slope", typed);
                key(&mut h, Key::Enter);
                let lines = &h.state().sketch.as_ref().unwrap().lines;
                let expected = if typed == "22.5" { 22.5 } else { 0.5f64.atan().to_degrees() };
                for i in [0, 2] {
                    assert!((lines[i].slope.unwrap() - expected).abs() < 1e-10);
                }
                for i in [1, 3] {
                    assert_eq!(lines[i], draft[i]);
                }
            }
            assert_eq!(*h.state().session.model, *original);
            press(&mut h, if cancel { "Cancel Edit Mode" } else { "Finish Edit Mode" });
            assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
            if cancel {
                assert_eq!(*h.state().session.model, *original);
            }
        }
        assert_eq!(h.state().session.history().len(), steps + 1);
        let edited = h.state().session.model.clone();
        let Some(Element::Roof(r)) = edited.elements.get(roof) else { panic!("roof") };
        assert!(r.gables.is_empty());
        assert_eq!(r.slopes[1], 45.0);
        assert_eq!(r.slopes[3], 45.0);
        let (reopened, left) = bimcraft_core::model::Model::read(&edited.to_json().unwrap()).unwrap();
        assert!(left.is_empty());
        assert_eq!(reopened.roof_faces(r).unwrap(), edited.roof_faces(r).unwrap());
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        assert_eq!(*h.state().session.model, *original);
        h.state_mut().session.execute("redo", &json!({})).unwrap();
        assert_eq!(*h.state().session.model, *edited);
    }
}

#[test]
fn a_picked_line_of_a_roofs_sketch_shows_its_slope_in_properties_and_sets_it_in_both_profiles() {
    for rev in [false, true] {
        let script = format!("{BOX}\n{SLOPED}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let roof = ids(&h, "roof")[0];
        h.state_mut().selection = vec![roof];
        h.run();
        h.state_mut().edit_boundary();
        h.run();
        key(&mut h, Key::Escape);
        assert!(h.query_by_role_and_label(Role::TextInput, "Slope").is_none(), "rev {rev}: no line picked, no slope");
        let slope = |h: &Harness<BimApp>| h.get_by_role_and_label(Role::TextInput, "Slope").value().unwrap_or_default();
        // The east line, which the roof's edge there rises along at 45°; the south one has the roof's, blank.
        click(&mut h, P2(8000.0, 3000.0));
        assert_eq!(slope(&h), "45", "rev {rev}");
        click(&mut h, P2(4000.0, 0.0));
        assert_eq!(slope(&h), "", "rev {rev}");
        // The south line given 20°, which Finish sends with the lines: the roof's south edge, number 0, rises at 20°.
        type_into(&mut h, "Slope", "20");
        key(&mut h, Key::Enter);
        assert_eq!(slope(&h), "20", "rev {rev}");
        assert_eq!(field(&h, roof, "slopes").as_array().map(Vec::len), Some(4), "nothing is the roof's until Finish");
        assert_eq!(field(&h, roof, "slopes"), json!([0.0, 45.0, 0.0, 45.0]), "rev {rev}");
        for typed in ["6:12", "6/12", "1/2", " 6 : 12 ", "50%", " 50 % "] {
            type_into(&mut h, "Slope", typed);
            key(&mut h, Key::Enter);
            assert!(!h.state().status.error, "{typed}: rev {rev}: {}", h.state().status.text);
            assert_eq!(slope(&h), "26.57", "{typed}: rev {rev}");
        }
        for typed in [
            "6:0", "6/0", "0:12", "0/12", "-6:12", "6/-12", "6:12:1", "1/2/3", "NaN:12", "6:inf", "60:1", "0%", "-50%", "NaN%", "inf%",
            "400%", "50%%", "%", "6:12%", "1e-324%",
        ] {
            type_into(&mut h, "Slope", typed);
            key(&mut h, Key::Enter);
            assert!(h.state().status.error, "{typed}: rev {rev}");
            assert_eq!(slope(&h), "26.57", "{typed}: rev {rev}");
        }
        pick(&mut h, Role::CheckBox, "Defines Slope");
        assert!(h.query_by_role_and_label(Role::TextInput, "Slope").is_none());
        pick(&mut h, Role::CheckBox, "Defines Slope");
        assert_eq!(slope(&h), "26.57", "disabling an edge preserves its pitch");
        type_into(&mut h, "Slope", "20");
        key(&mut h, Key::Enter);
        // Blank is the roof's again, and a number out of range is refused, saying so, the line keeping the slope it had.
        type_into(&mut h, "Slope", "90");
        key(&mut h, Key::Enter);
        assert!(h.state().status.error && h.state().status.text.contains("from 0 to 75"), "rev {rev}: {}", h.state().status.text);
        assert_eq!(slope(&h), "20", "rev {rev}");
        press(&mut h, "Finish Edit Mode");
        assert!(h.state().sketch.is_none(), "rev {rev}: {}", h.state().status.text);
        assert_eq!(field(&h, roof, "slopes"), json!([20.0, 45.0, 0.0, 45.0]), "rev {rev}");
        assert_eq!(h.state().session.history().last(), Some(&"Edit Boundary"), "rev {rev}");
        // Edit Boundary again hands the lines back with the slopes the edges have; one set blank is the roof's, not the edge's old slope.
        h.state_mut().edit_boundary();
        h.run();
        key(&mut h, Key::Escape);
        click(&mut h, P2(8000.0, 3000.0));
        assert_eq!(slope(&h), "45", "rev {rev}");
        type_into(&mut h, "Slope", "");
        key(&mut h, Key::Backspace);
        key(&mut h, Key::Enter);
        assert_eq!(slope(&h), "", "rev {rev}");
        press(&mut h, "Finish Edit Mode");
        assert_eq!(field(&h, roof, "slopes"), json!([20.0, 0.0, 0.0, 45.0]), "rev {rev}");
        // A floor's sketch has no slope to give.
        let floor = format!("{BOX}\nfloor {{}}");
        let mut f = if rev { app(&floor) } else { bimcraft(&floor) };
        let id = ids(&f, "floor")[0];
        f.state_mut().selection = vec![id];
        f.run();
        f.state_mut().edit_boundary();
        f.run();
        key(&mut f, Key::Escape);
        click(&mut f, P2(4000.0, 6000.0));
        assert!(f.query_by_role_and_label(Role::TextInput, "Slope").is_none(), "rev {rev}");
    }
}

#[test]
fn roof_pitch_in_the_3d_inspector_accepts_ratios_and_percentages_and_keeps_edge_overrides() {
    let mut h = bimcraft(SLOPED);
    let roof = ids(&h, "roof")[0];
    press(&mut h, "Default 3D View");
    h.state_mut().selection = vec![roof];
    h.run();
    let overrides = field(&h, roof, "slopes");
    for (typed, expected) in [
        ("6:12", 26.56505117707799),
        ("6/12", 26.56505117707799),
        ("50%", 26.56505117707799),
        ("6:0", 26.56505117707799),
        ("100%", 45.0),
        ("12.5%", 7.125016348901798),
        ("400%", 7.125016348901798),
        ("22.5", 22.5),
        ("0", 0.0),
    ] {
        type_into(&mut h, "Slope (°)", typed);
        key(&mut h, Key::Enter);
        if h.state().roof_slope_dialog.is_some() {
            press(&mut h, "Apply slope");
        }
        assert!((field(&h, roof, "slope").as_f64().unwrap() - expected).abs() < 1e-12, "{typed}");
        assert_eq!(field(&h, roof, "slopes"), overrides);
        if matches!(typed, "6:0" | "400%") {
            assert!(h.state().status.error);
        }
    }
}

#[test]
fn roof_edge_selection_names_the_scope_and_changes_only_picked_planes() {
    for rev in [false, true] {
        let script =
            r#"roof {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]], "slope":30, "slopes":[0,0,45,0], "gables":[1,3], "overhang":600}"#;
        let mut h = if rev { app(script) } else { bimcraft(script) };
        let roof = ids(&h, "roof")[0];
        h.state_mut().selection = vec![roof];
        h.run();
        let before = h.state().session.model.clone();
        let steps = h.state().session.history().len();
        if let Ok(folder) = std::env::var("BIMCRAFT_ROOF_EDGE_BEFORE") {
            std::fs::create_dir_all(&folder).unwrap();
            h.render().unwrap().save(std::path::Path::new(&folder).join("whole-roof-before.png")).unwrap();
        }
        press(&mut h, "Edit roof pitches");
        assert!(h.query_by_label("Roof edges: 0 selected").is_some());
        assert!(h.state().selection.is_empty(), "the roof body is not the selected sub-element");
        click(&mut h, P2(4000.0, -600.0));
        assert!(h.query_by_label("Roof edges: 1 selected").is_some());
        roof_edge_capture(&mut h, rev, "one");
        assert!(h.query_by_role_and_label(Role::TextInput, "Slope (°)").is_none(), "no competing overall field");
        type_into(&mut h, "Selected pitch", "8:12");
        key(&mut h, Key::Enter);
        assert_eq!(*h.state().session.model, *before, "draft until Apply");
        press(&mut h, "Apply pitches");
        let slopes = field(&h, roof, "slopes");
        assert!((slopes[0].as_f64().unwrap() - (2.0_f64 / 3.0).atan().to_degrees()).abs() < 1e-9);
        assert_eq!(slopes[2], json!(45.0));
        assert_eq!(field(&h, roof, "slope"), json!(30.0), "the common eave datum is unchanged");
        assert_eq!(h.state().session.history().len(), steps + 1);
        h.state_mut().run("undo", json!({}));
        h.run();
        assert_eq!(*h.state().session.model, *before);
        press(&mut h, "Edit roof pitches");
        click(&mut h, P2(4000.0, -600.0));
        click_with(&mut h, P2(4000.0, 6600.0), Modifiers::CTRL);
        assert!(h.query_by_label("Roof edges: 2 selected").is_some());
        roof_edge_capture(&mut h, rev, "two");
        type_into(&mut h, "Selected pitch", "50%");
        key(&mut h, Key::Enter);
        press(&mut h, "Cancel pitch edit");
        assert_eq!(*h.state().session.model, *before);
        assert_eq!(h.state().session.history().len(), steps);
    }
}

#[test]
fn roof_edge_edits_keep_pending_scope_and_cancel_without_touching_project_history() {
    let mut h = app(r#"roof {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]], "slope":30, "gables":[1,3]}"#);
    let roof = ids(&h, "roof")[0];
    h.state_mut().selection = vec![roof];
    h.run();
    let before = h.state().session.model.clone();
    press(&mut h, "Edit roof pitches");
    click(&mut h, P2(4000.0, 0.0));
    type_into(&mut h, "Selected pitch", "6:12");
    key(&mut h, Key::Enter);
    click(&mut h, P2(4000.0, 6000.0));
    assert!(h.query_by_label("Pending pitch edits: edges 1").is_some());
    type_into(&mut h, "Selected pitch", "12:12");
    key(&mut h, Key::Enter);
    assert!(h.query_by_label("Pending pitch edits: edges 1, 3").is_some());
    press(&mut h, "Undo");
    assert_eq!(*h.state().session.model, *before, "project Undo waits for Apply or Cancel");
    press(&mut h, "Apply pitches");
    let after = h.state().session.model.clone();
    assert_eq!(field(&h, roof, "slopes")[2], json!(45.0));
    assert!((field(&h, roof, "slopes")[0].as_f64().unwrap() - (0.5_f64).atan().to_degrees()).abs() < 1e-9);
    press(&mut h, "Undo");
    assert_eq!(*h.state().session.model, *before);
    press(&mut h, "Redo");
    assert_eq!(*h.state().session.model, *after);
    press(&mut h, "Edit roof pitches");
    click(&mut h, P2(4000.0, 0.0));
    type_into(&mut h, "Selected pitch", "40");
    key(&mut h, Key::Enter);
    key(&mut h, Key::Escape);
    assert!(h.state().roof_edge_edit.is_none());
    assert_eq!(*h.state().session.model, *after);
    press(&mut h, "Edit roof pitches");
    h.state_mut().selection = vec![roof];
    h.run();
    assert!(h.state().roof_edge_edit.is_none());
    assert_eq!(h.state().selection, vec![roof]);
    press(&mut h, "Edit roof pitches");
    h.state_mut().session.execute("modify", &json!({"id":roof,"offset":100})).unwrap();
    h.run();
    assert!(h.state().roof_edge_edit.is_none());
    assert!(h.state().status.error);
}

#[test]
fn roof_edge_selection_tracks_actual_eaves_in_3d_on_both_painters() {
    for rev in [false, true] {
        for card in [false, true] {
            let script = r#"roof {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]], "slope":30,"slopes":[0,0,45,0],"gables":[1,3],"overhang":600,"offset":3000}"#;
            let mut h = if std::env::var_os("BIMCRAFT_ROOF_EDGE_DUMP").is_some() {
                let state = egui_kittest::wgpu::create_render_state(
                    egui_kittest::wgpu::default_wgpu_setup(),
                    eframe::egui_wgpu::RendererOptions::PREDICTABLE,
                );
                let mut session = Session::new();
                session.script(script).unwrap();
                let mut app = BimApp::new(session);
                if rev {
                    (app.profile, app.docking) = (crate::style::Profile::Rev, crate::dock::Docking::revit());
                }
                if card {
                    app.card = crate::gpu::Card::with(crate::gpu::Gpu::new(&state).unwrap());
                }
                let mut h = Harness::builder()
                    .with_size([1280.0, 800.0])
                    .with_max_steps(32)
                    .with_step_dt(0.25)
                    .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(state))
                    .build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
                h.run();
                h
            } else {
                let mut h = if rev { app(script) } else { bimcraft(script) };
                if card {
                    let Some(gpu) = crate::gpu::headless() else {
                        eprintln!("no graphics adapter: roof edge GPU picking not exercised");
                        continue;
                    };
                    h.state_mut().card = crate::gpu::Card::with(gpu);
                }
                h
            };
            shortcut(&mut h, "3D");
            assert_eq!(h.state().orbit.on_card, card);
            let roof = ids(&h, "roof")[0];
            h.state_mut().selection = vec![roof];
            h.run();
            press(&mut h, "Edit roof pitches");
            let model = &h.state().session.model;
            let Some(Element::Roof(r)) = model.elements.get(roof) else { panic!("roof") };
            let plane = model.roof_faces_by_edge(r).unwrap().into_iter().find(|(i, _)| *i == 0).unwrap().1.1;
            let at = h.state().screen3([4000.0, -600.0, plane.at(P2(4000.0, -600.0)) + model.roof_rise(r, &plane)]);
            click_at(&mut h, at);
            assert!(h.query_by_label("Roof edges: 1 selected").is_some(), "rev={rev},card={card},at={at:?}");
            assert!(h.state().selection.is_empty());
            roof_edge_capture(&mut h, rev, if card { "3d-card" } else { "3d-software" });
            type_into(&mut h, "Selected pitch", "8:12");
            key(&mut h, Key::Enter);
            press(&mut h, "Apply pitches");
            assert!((field(&h, roof, "slopes")[0].as_f64().unwrap() - (2.0_f64 / 3.0).atan().to_degrees()).abs() < 1e-9);
            assert_eq!(field(&h, roof, "slopes")[2], json!(45.0));
        }
    }
}

fn roof_edge_capture(h: &mut Harness<BimApp>, rev: bool, which: &str) {
    if let Ok(folder) = std::env::var("BIMCRAFT_ROOF_EDGE_DUMP") {
        std::fs::create_dir_all(&folder).unwrap();
        for (look, dark, light) in [("light", false, false), ("dark", true, false), ("dark-light-canvas", true, true)] {
            h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            h.state_mut().light_canvas = light;
            h.run();
            h.render()
                .unwrap()
                .save(std::path::Path::new(&folder).join(format!("edge-{}-{which}-{look}.png", if rev { "rev" } else { "bimcraft" })))
                .unwrap();
        }
    }
}

#[test]
fn overall_roof_pitch_offers_each_choice_and_warns_before_changing_the_eave() {
    for rev in [false, true] {
        let script = r#"roof {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]], "slope":30, "slopes":[0,45,0,45], "overhang":600}"#;
        let mut h = if rev { app(script) } else { bimcraft(script) };
        let roof = ids(&h, "roof")[0];
        h.state_mut().selection = vec![roof];
        h.run();
        let before = h.state().session.model.clone();
        let steps = h.state().session.history().len();
        for choice in ["Cancel", "Escape", "Keep individual edge pitches", "Apply to all defining edges"] {
            assert_eq!(h.get_by_role_and_label(Role::TextInput, "Slope (°)").value().as_deref(), Some("Varies"));
            type_into(&mut h, "Slope (°)", "40");
            key(&mut h, Key::Enter);
            assert_eq!(*h.state().session.model, *before, "opening the choice never changes the roof");
            assert_eq!(h.state().session.history().len(), steps);
            assert!(h.query_by_label("Roof slope change").is_some());
            assert!(h.query_by_label("Eaves move down 157 mm.").is_some());
            if choice == "Cancel" {
                press(&mut h, "Cancel");
                continue;
            }
            if choice == "Escape" {
                key(&mut h, Key::Tab);
                shift_key(&mut h, Key::Tab);
                assert!(h.state().roof_slope_dialog.is_some(), "Tab keeps the pending choice open");
                key(&mut h, Key::Escape);
                assert!(h.state().roof_slope_dialog.is_none());
                assert_eq!(*h.state().session.model, *before);
                assert_eq!(h.state().session.history().len(), steps);
                assert_eq!(h.state().selection, vec![roof], "Escape closes the dialog, not the roof selection");
                continue;
            }
            pick(&mut h, Role::RadioButton, choice);
            if let Ok(folder) = std::env::var("BIMCRAFT_ROOF_CHOICE_DUMP") {
                std::fs::create_dir_all(&folder).unwrap();
                for (look, dark, light) in [("light", false, false), ("dark", true, false), ("dark-light-canvas", true, true)] {
                    h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
                    h.state_mut().light_canvas = light;
                    h.run();
                    h.render()
                        .unwrap()
                        .save(std::path::Path::new(&folder).join(format!(
                            "choice-{}-{}-{look}.png",
                            if rev { "rev" } else { "bimcraft" },
                            if choice.starts_with("Keep") { "keep" } else { "all" }
                        )))
                        .unwrap();
                }
            }
            press(&mut h, "Apply slope");
            assert_eq!(field(&h, roof, "slope"), json!(40.0));
            assert_eq!(
                field(&h, roof, "slopes"),
                if choice.starts_with("Keep") { json!([0.0, 45.0, 0.0, 45.0]) } else { json!([0.0, 40.0, 0.0, 40.0]) }
            );
            assert_eq!(h.state().session.history().len(), steps + 1);
            assert_eq!(
                h.get_by_role_and_label(Role::TextInput, "Slope (°)").value().as_deref(),
                Some(if choice.starts_with("Keep") { "Varies" } else { "40" })
            );
            h.state_mut().session.execute("undo", &json!({})).unwrap();
            h.run();
            assert_eq!(*h.state().session.model, *before);
        }
        type_into(&mut h, "Slope (°)", "40");
        key(&mut h, Key::Enter);
        assert!(h.state().roof_slope_dialog.is_some());
        h.state_mut().session.execute("modify", &json!({"id":roof,"offset":100})).unwrap();
        h.run();
        assert!(h.state().roof_slope_dialog.is_none(), "an external change invalidates the height preview");
        assert_eq!(field(&h, roof, "slope"), json!(30.0));
    }
}

#[test]
fn roof_sketch_defines_slope_can_be_cancelled_then_finished_as_a_shed() {
    for rev in [false, true] {
        let script = format!("{BOX}\n{SLOPED}");
        let mut h = if rev { app(&script) } else { bimcraft(&script) };
        let roof = ids(&h, "roof")[0];
        let original = h.state().session.model.clone();
        let steps = h.state().session.history().len();
        for cancel in [true, false] {
            h.state_mut().selection = vec![roof];
            h.state_mut().edit_boundary();
            h.run();
            key(&mut h, Key::Escape);
            for at in [P2(8000.0, 3000.0), P2(4000.0, 6000.0), P2(0.0, 3000.0)] {
                click(&mut h, at);
                pick(&mut h, Role::CheckBox, "Defines Slope");
                assert!(h.query_by_role_and_label(Role::TextInput, "Slope").is_none());
            }
            click(&mut h, P2(4000.0, 0.0));
            type_into(&mut h, "Slope", if rev { "50%" } else { "6/12" });
            key(&mut h, Key::Enter);
            if !cancel && let Ok(folder) = std::env::var("BIMCRAFT_ROOF_DUMP") {
                std::fs::create_dir_all(&folder).unwrap();
                for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-light-canvas", true, true)] {
                    h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
                    h.state_mut().light_canvas = light_canvas;
                    h.run();
                    h.render()
                        .unwrap()
                        .save(std::path::Path::new(&folder).join(format!("roof-{}-{look}.png", if rev { "rev" } else { "bimcraft" })))
                        .unwrap();
                }
            }
            assert_eq!(*h.state().session.model, *original, "draft changes only until Finish");
            press(&mut h, if cancel { "Cancel Edit Mode" } else { "Finish Edit Mode" });
            if cancel {
                assert_eq!(*h.state().session.model, *original);
                assert_eq!(h.state().session.history().len(), steps);
            }
        }
        assert_eq!(field(&h, roof, "gables"), json!([1, 2, 3]));
        assert_eq!(h.state().session.history().len(), steps + 1);
        let Some(Element::Roof(r)) = h.state().session.model.elements.get(roof) else { panic!("roof") };
        let faces = h.state().session.model.roof_faces(r).unwrap();
        assert_eq!(faces.len(), 1);
        let plane = faces[0].1;
        assert!((plane.at(P2(4000.0, 1219.2)) - plane.at(P2(4000.0, 0.0)) - 609.6).abs() < 1e-6);
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        assert_eq!(*h.state().session.model, *original);
        h.state_mut().session.execute("redo", &json!({})).unwrap();
        assert_eq!(field(&h, roof, "gables"), json!([1, 2, 3]));
    }
}
