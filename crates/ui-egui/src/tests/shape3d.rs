//! Shape Editing in the 3D view (MOD-16): a shaped floor's points and corners have grips that drag up or down, Modify Sub
//! Elements picks what's under a click, and Add Point and Add Split Line click on its top, as in plan.

use super::*;

/// An 8 by 6 m floor with a drain 300 mm down in its middle, selected in the 3D view with a click on its top.
fn drained() -> (Harness<'static, BimApp>, Id) {
    let mut h = app(r#"floor {"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}"#);
    let floor = ids(&h, "floor")[0];
    h.state_mut().run("edit_shape", json!({"id": floor, "points": [{"at": [4000, 3000], "z": -300}]}));
    press(&mut h, "Default 3D View");
    // On the west plane, a quarter of the way down to the drain.
    let at = h.state().screen3([1000.0, 3000.0, -75.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [floor]);
    (h, floor)
}

/// Floor `id`'s shape point at plan point (`x`, `y`): where it is and its height.
fn point_near(h: &Harness<BimApp>, id: Id, x: f64, y: f64, within: f64) -> Option<(P2, f64)> {
    match h.state().session.model.elements.get(id) {
        Some(Element::Floor(f)) => f.shape.points.iter().find(|p| p.at.dist(P2(x, y)) <= within).map(|p| (p.at, p.z)),
        _ => None,
    }
}

#[test]
fn in_3d_a_points_grip_drags_it_up_or_down_and_a_corners_makes_it_a_point() {
    let (mut h, floor) = drained();
    press(&mut h, "Modify Sub Elements");
    assert!(painted(&h).iter().any(|t| t == "-300"), "the drain's height beside it: {:?}", painted(&h));
    assert!(painted(&h).iter().any(|t| t.starts_with("Drag a grip up or down")), "the 3D view's prompt: {:?}", painted(&h));
    let before = h.state().orbit.camera;
    // The drain dragged up to 100 mm down.
    let (a, b) = (h.state().screen3([4000.0, 3000.0, -300.0]), h.state().screen3([4000.0, 3000.0, -100.0]));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let (at, z) = point_near(&h, floor, 4000.0, 3000.0, 0.0).expect("the drain");
    assert!((z + 100.0).abs() <= 2.0 && z.fract() == 0.0, "{at:?} {z}");
    // A corner dragged 50 mm up: a point there now.
    let (a, b) = (h.state().screen3([8000.0, 0.0, 0.0]), h.state().screen3([8000.0, 0.0, 50.0]));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let (_, z) = point_near(&h, floor, 8000.0, 0.0, 0.0).expect("a point at the corner");
    assert!((z - 50.0).abs() <= 2.0, "{z}");
    assert_eq!(h.state().orbit.camera, before, "the grips took the drags, not the camera");
    assert_eq!((h.state().selection.as_slice(), &h.state().tool), ([floor].as_slice(), &Tool::ModifySubElements));
}

#[test]
fn in_3d_a_click_sets_what_it_picks_to_the_elevation_and_adds_points_and_split_lines_on_the_top() {
    let (mut h, floor) = drained();
    // Modify Sub Elements: the drain clicked, a little off it, to the Options Bar's elevation.
    press(&mut h, "Modify Sub Elements");
    type_into(&mut h, "Elevation:", "-40");
    let at = h.state().screen3([4000.0, 3000.0, -300.0]) + vec2(3.0, -2.0);
    click_at(&mut h, at);
    assert_eq!(point_near(&h, floor, 4000.0, 3000.0, 0.0).map(|p| p.1), Some(-40.0));
    // Add Point: on the top where it's clicked.
    press(&mut h, "Add Point");
    type_into(&mut h, "Elevation:", "-20");
    let top = |h: &Harness<BimApp>, x: f64, y: f64| {
        let m = &h.state().session.model;
        let Some(Element::Floor(f)) = m.elements.get(floor) else { panic!("no floor") };
        m.floor_top(f).at(P2(x, y)) + m.shape_height(f, P2(x, y))
    };
    let at = h.state().screen3([6000.0, 4500.0, top(&h, 6000.0, 4500.0)]);
    click_at(&mut h, at);
    let (p, z) = point_near(&h, floor, 6000.0, 4500.0, 20.0).expect("a point where it was clicked");
    assert_eq!(z, -20.0, "{p:?}");
    // Add Split Line: from a corner to the drain, each click taking the one within reach.
    press(&mut h, "Add Split Line");
    let at = h.state().screen3([0.0, 0.0, 0.0]) + vec2(4.0, 3.0);
    click_at(&mut h, at);
    let at = h.state().screen3([4000.0, 3000.0, -40.0]) + vec2(-3.0, 3.0);
    click_at(&mut h, at);
    let lines = field(&h, floor, "shape")["lines"].clone();
    assert_eq!((lines[0]["a"].clone(), lines[0]["b"].clone()), (json!([0.0, 0.0]), json!([4000.0, 3000.0])), "{lines}");
    assert_eq!(h.state().tool, Tool::AddSplitLine(None));
    // Off the floor, a click says where to click.
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    press(&mut h, "Add Point");
    click_at(&mut h, corner);
    assert!(h.state().status.error && h.state().status.text.contains("floor's top"), "{}", h.state().status.text);
}
