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
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    // A narrow ribbon folds the Mode panel into its list.
    if h.query_by_label("Reset Profile").is_none() {
        press(&mut h, "Mode ▾");
    }
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
    // Its end corner past the wall's end.
    press(&mut h, "Point 1 2");
    type_into(&mut h, "Along the Wall", "7000");
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

/// Where a point of wall `id`'s face, `[along, up]`, shows in section `view` as last drawn.
fn on_face(h: &Harness<BimApp>, view: Id, id: Id, q: P2) -> egui::Pos2 {
    let m = &h.state().session.model;
    let w = m.wall(id).unwrap_or_else(|| panic!("no wall"));
    let p = w.start + (w.end - w.start).unit() * q.0;
    let shown = h.state().canvas.view(view).unwrap_or_else(|| panic!("the section isn't drawn"));
    let (cam, _) = bimcraft_core::render::section_camera(m, view, shown).unwrap_or_else(|| panic!("no camera"));
    let (x, y, _) = cam.project([p.0, p.1, m.own_base(w) + q.1]);
    h.state().canvas.rect.min + egui::vec2(x as f32, y as f32)
}

fn click_at(h: &mut Harness<BimApp>, at: egui::Pos2) {
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run();
    }
}

#[test]
fn edit_profile_in_a_section_sketches_on_the_walls_face_and_a_hole_drawn_there_is_cut() {
    let mut h = app(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}
           section {"start": [-1000, -3000], "end": [7000, -3000]}"#);
    let (id, _) = wall(&h);
    let view = {
        let m = &h.state().session.model;
        m.elements.iter().find_map(|(k, e)| matches!(e, Element::Section(_)).then_some(*k)).unwrap_or_else(|| panic!("no section"))
    };
    h.state_mut().open_sheet(Sheet::Section(view));
    h.run();
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    press(&mut h, "Edit Profile");
    let sk = h.state().sketch.clone().unwrap_or_else(|| panic!("sketching: {}", h.state().status.text));
    assert_eq!(sk.of, crate::sketch::Of::Profile(id));
    assert_eq!(sk.plan, Sheet::Section(view), "on the section it began in");
    assert_eq!(sk.lines.len(), 4, "the wall's rectangle");
    assert!(h.state().profile_editor.is_none(), "no dialog: it's sketched where it is");
    // The Line tool, on the wall's face: a 1 m square hole, its points clicked in the section.
    h.run();
    for q in [P2(1000.0, 1000.0), P2(2000.0, 1000.0), P2(2000.0, 2000.0), P2(1000.0, 2000.0), P2(1000.0, 1000.0)] {
        let at = on_face(&h, view, id, q);
        click_at(&mut h, at);
    }
    key(&mut h, Key::Escape);
    let lines = h.state().sketch.as_ref().map_or(0, |sk| sk.lines.len());
    assert_eq!(lines, 8, "{}", h.state().status.text);
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    let (_, w) = wall(&h);
    assert_eq!(w.profile.len(), 2, "its outline and the hole: {:?}", w.profile);
    let hole = &w.profile[1];
    for q in [P2(1000.0, 1000.0), P2(2000.0, 2000.0)] {
        assert!(hole.iter().any(|p| p.dist(q) < 1e-6), "{hole:?}");
    }
    assert_eq!(h.state().selection, vec![id], "the wall stays selected");
}

#[test]
fn edit_profile_in_the_3d_view_sketches_on_the_walls_face() {
    let mut h = app(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}"#);
    let (id, _) = wall(&h);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    press(&mut h, "Edit Profile");
    let sk = h.state().sketch.clone().unwrap_or_else(|| panic!("sketching: {}", h.state().status.text));
    assert_eq!((sk.of, sk.plan), (crate::sketch::Of::Profile(id), Sheet::View3D));
    assert_eq!(h.state().sketch_surface().map(|f| f.world(P2(1000.0, 500.0))), Some([1000.0, 0.0, 500.0]));
    // Finished as it is, the wall's rectangle: no profile at all.
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    assert!(wall(&h).1.profile.is_empty());
}
