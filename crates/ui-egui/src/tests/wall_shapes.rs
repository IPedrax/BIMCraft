//! Walls of other shapes in the desktop: a slanted wall's Angle From Vertical in Properties, and Edit Profile.

use super::*;

fn wall(h: &Harness<BimApp>) -> (Id, bimcraft_core::model::Wall) {
    let m = &h.state().session.model;
    m.elements
        .iter()
        .find_map(|(id, e)| if let Element::Wall(w) = e { Some((*id, w.clone())) } else { None })
        .unwrap_or_else(|| panic!("no wall"))
}

#[test]
fn a_walls_angle_from_vertical_slants_it_from_properties() {
    let mut h = app(r#"wall {"points": [[0, 0], [5000, 0]], "height": 3000}"#);
    let (id, _) = wall(&h);
    h.state_mut().selection = vec![id];
    h.run();
    type_into(&mut h, "Angle From Vertical (°)", "12");
    key(&mut h, Key::Enter);
    assert_eq!(wall(&h).1.slant, 12.0, "{}", h.state().status.text);
    // Past what a wall may lean, refused, and the wall as it was.
    type_into(&mut h, "Angle From Vertical (°)", "75");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error, "{}", h.state().status.text);
    assert_eq!(wall(&h).1.slant, 12.0);
}

#[test]
fn edit_profile_shapes_a_selected_wall_in_one_step_and_reset_profile_takes_it_back() {
    let mut h = app(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}"#);
    let (id, _) = wall(&h);
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    press(&mut h, "Edit Profile");
    let editor = h.state().profile_editor.clone().unwrap_or_else(|| panic!("Edit Profile is open"));
    assert_eq!(editor.loops, [crate::profile::rectangle(6000.0, 3000.0)], "it starts as the wall's rectangle");
    // A point on the top edge, raised to a gable's peak: picked, then typed.
    press(&mut h, "Point 1 3");
    press(&mut h, "Add Point");
    type_into(&mut h, "Along the Wall", "3000");
    type_into(&mut h, "Up from Base", "4500");
    let undo = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().profile_editor.is_none(), "{}", h.state().status.text);
    let (_, w) = wall(&h);
    assert_eq!(w.profile, [vec![P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 3000.0), P2(3000.0, 4500.0), P2(0.0, 3000.0)]]);
    assert_eq!(h.state().session.history().len(), undo + 1, "one undo step");
    // The wall stands up to its peak now.
    assert_eq!(h.state().session.model.top_z(&w), 4500.0);
    press(&mut h, "Reset Profile");
    assert!(wall(&h).1.profile.is_empty(), "{}", h.state().status.text);
}

#[test]
fn edit_profile_says_why_a_shape_off_the_walls_ends_is_refused_and_stays_open() {
    let mut h = app(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}"#);
    let (id, _) = wall(&h);
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    press(&mut h, "Edit Profile");
    press(&mut h, "Point 1 2");
    type_into(&mut h, "Along the Wall", "5000");
    press(&mut h, "OK");
    let open = h.state().profile_editor.clone().unwrap_or_else(|| panic!("it stays open"));
    assert!(open.error.as_deref().is_some_and(|e| e.contains("ends")), "{:?}", open.error);
    assert!(wall(&h).1.profile.is_empty());
}

#[test]
fn edit_profile_is_offered_only_for_a_straight_upright_wall() {
    let mut h = app(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": 10}"#);
    let (id, _) = wall(&h);
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    assert!(h.query_by_label("Edit Profile").is_none());
}
