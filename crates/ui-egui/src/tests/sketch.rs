//! Sketch mode: an outline drawn as lines, Finish Edit Mode solving them into the floor's (or a ceiling's, a roof's, a
//! shaft's) outline and holes, and what's wrong with them shown on the lines.

use super::*;

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
    assert!(st.status.error && st.status.text.contains("open"), "{}", st.status.text);
    let sk = st.sketch.as_ref().expect("still sketching");
    let wrong: Vec<[P2; 2]> = sk.wrong.iter().map(|&i| sk.lines[i]).collect();
    assert_eq!(wrong.len(), 2, "the two lines left with a loose end");
    assert!(wrong.iter().all(|l| l.iter().any(|p| p.1 == 6000.0)), "{wrong:?}");
    press(&mut h, "Cancel Edit Mode");
    assert!(h.state().sketch.is_none());
    assert_eq!(field(&h, f, "boundary"), before);
}

#[test]
fn a_floor_is_sketched_line_by_line_closed_on_its_first_point_and_ctrl_z_takes_back_a_line() {
    let mut h = app(BOX);
    press(&mut h, "Floor");
    press(&mut h, "Boundary Line");
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
