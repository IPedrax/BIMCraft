//! Walls of other shapes in the desktop: a slanted wall's Angle From Vertical in Properties, and Edit Profile as Revit's: sketch
//! mode on the wall's face in a view that looks at it, Go To View from a plan, Reset Profile.

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
    assert!(h.state().go_to_view.is_none(), "no Go To View: it's sketched where it is");
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

const ALONG: &str = r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}
section {"start": [-1000, -3000], "end": [7000, -3000]}"#;

fn section_of(h: &Harness<BimApp>) -> Id {
    let m = &h.state().session.model;
    m.elements.iter().find_map(|(k, e)| matches!(e, Element::Section(_)).then_some(*k)).unwrap_or_else(|| panic!("no section"))
}

#[test]
fn edit_profile_in_a_plan_asks_which_view_to_go_to_and_sketches_there() {
    let mut h = app(ALONG);
    let (id, _) = wall(&h);
    let view = section_of(&h);
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    press(&mut h, "Edit Profile");
    let go = h.state().go_to_view.clone().unwrap_or_else(|| panic!("Go To View asks: {}", h.state().status.text));
    let sheets: Vec<Sheet> = go.views.iter().map(|(s, _)| *s).collect();
    assert_eq!(sheets, [Sheet::Section(view), Sheet::View3D], "the section along the wall, and the 3D view");
    assert!(h.state().sketch.is_none(), "nothing sketched in the plan");
    press(&mut h, "Open View");
    assert_eq!(h.state().sheet, Sheet::Section(view));
    let sk = h.state().sketch.clone().unwrap_or_else(|| panic!("sketching: {}", h.state().status.text));
    assert_eq!((sk.of, sk.plan, sk.lines.len()), (crate::sketch::Of::Profile(id), Sheet::Section(view), 4));
}

#[test]
fn the_outline_is_sketched_again_from_nothing_and_finish_gives_the_wall_that_shape() {
    let mut h = app(ALONG);
    let (id, _) = wall(&h);
    let view = section_of(&h);
    h.state_mut().open_sheet(Sheet::Section(view));
    h.run();
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    press(&mut h, "Edit Profile");
    // Every line picked and deleted: nothing left of the rectangle.
    h.state_mut().tool = Tool::Modify;
    if let Some(sk) = h.state_mut().sketch.as_mut() {
        sk.picked = (0..sk.lines.len()).collect();
    }
    h.run();
    key(&mut h, Key::Delete);
    assert_eq!(h.state().sketch.as_ref().map(|sk| sk.lines.len()), Some(0), "{}", h.state().status.text);
    // A gable drawn on the wall's face with Line, back to where it began.
    press(&mut h, "Line");
    h.run();
    let gable = [P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 2500.0), P2(3000.0, 4000.0), P2(0.0, 2500.0), P2(0.0, 0.0)];
    for q in gable {
        let at = on_face(&h, view, id, q);
        click_at(&mut h, at);
    }
    key(&mut h, Key::Escape);
    assert_eq!(h.state().sketch.as_ref().map(|sk| sk.lines.len()), Some(5), "{}", h.state().status.text);
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    let (_, w) = wall(&h);
    assert_eq!(w.profile.len(), 1, "{:?}", w.profile);
    for q in &gable[..5] {
        assert!(w.profile[0].iter().any(|p| p.dist(*q) < 1e-6), "{q:?} in {:?}", w.profile);
    }
    assert_eq!(h.state().session.model.top_z(&w), 4000.0);
    // Reset Profile, on the ribbon beside it: the rectangle back.
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Modify | Walls");
    // A narrow ribbon lists the Mode panel's second tool.
    if h.query_by_label("Reset Profile").is_none() {
        press(&mut h, "Mode ▾");
    }
    press(&mut h, "Reset Profile");
    assert!(wall(&h).1.profile.is_empty(), "{}", h.state().status.text);
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
