//! The 3D view's section box in the window (fork issue 1): BX and Properties > Extents turn it on and off and type its faces,
//! and in the view a drag from a face's grip moves that face, cutting away what's past it.

use super::*;
use egui_kittest::kittest::NodeT;

fn boxed(h: &Harness<BimApp>) -> Option<([f64; 3], [f64; 3])> {
    let m = &h.state().session.model;
    m.default_3d().and_then(|v| m.section_box(v))
}

#[test]
fn bx_turns_the_section_box_on_round_the_model_and_properties_type_its_faces() {
    let mut h = app(BOX);
    shortcut(&mut h, "BX");
    assert_eq!(h.state().sheet, Sheet::View3D, "in the 3D view");
    let (lo, hi) = boxed(&h).expect("on");
    assert!(lo[0] < 0.0 && hi[0] > 8000.0 && hi[2] > 3000.0, "round the walls: {lo:?} {hi:?}");
    // Nothing selected, Properties is the view's: its Section Box ticked, and its top a length to type.
    let ticked = h.get_by_role_and_label(Role::CheckBox, "Section Box").accesskit_node().toggled();
    assert_eq!(ticked, Some(egui::accesskit::Toggled::True));
    type_into(&mut h, "Top", "1500");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(boxed(&h).map(|b| b.1[2]), Some(1500.0));
    // Unticked, it's off, its extents kept; BX brings it back as it was.
    pick(&mut h, Role::CheckBox, "Section Box");
    assert_eq!(boxed(&h), None);
    shortcut(&mut h, "BX");
    assert_eq!(boxed(&h).map(|b| b.1[2]), Some(1500.0));
}

#[test]
fn a_drag_from_a_section_box_grip_moves_its_face_and_what_it_cuts_away_is_not_picked() {
    let mut h = app(BOX);
    h.state_mut().run("section_box", json!({"min": [-1000, -1000, -1000], "max": [9000, 7000, 4000]}));
    press(&mut h, "Default 3D View");
    let before = h.state().orbit.camera;
    // The top's grip, dragged down 2.5 m.
    let (a, b) = (h.state().screen3([4000.0, 3000.0, 4000.0]), h.state().screen3([4000.0, 3000.0, 1500.0]));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let top = boxed(&h).map(|b| b.1[2]).unwrap();
    assert!((top - 1500.0).abs() <= 20.0, "{top}");
    assert_eq!(boxed(&h).map(|b| b.0), Some([-1000.0, -1000.0, -1000.0]), "the other faces stay");
    assert_eq!(h.state().orbit.camera, before, "the grip took the drag, not the camera");
    // A wall's face below the box's top picks the wall; above it there's nothing to pick.
    let south = find(&h, |e| matches!(e, Element::Wall(w) if w.start.1 == 0.0 && w.end.1 == 0.0));
    let at = h.state().screen3([5000.0, -100.0, 1000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [south]);
    let at = h.state().screen3([5000.0, -100.0, 2500.0]);
    click_at(&mut h, at);
    assert!(h.state().selection.is_empty(), "{:?}", h.state().selection);
}

#[test]
fn a_plans_section_box_in_properties_makes_it_show_what_the_box_keeps() {
    let mut h = app(BOX);
    h.state_mut().run("section_box", json!({"min": [-1000, -1000, 500], "max": [4000, 7000, 1500]}));
    let level = h.state().session.model.levels()[0].0;
    // Nothing selected on Level 1's plan, Properties is the plan's; its row is below the palette's fold, so it is pressed as a
    // screen reader does.
    h.get_by_role_and_label(Role::CheckBox, "Section Box").click_accesskit();
    h.run();
    let m = &h.state().session.model;
    assert_eq!(m.shown_range(level).cut, 1500.0);
    assert_eq!(m.shown_crop(level), Some([P2(-1000.0, -1000.0), P2(4000.0, 7000.0)]));
}
