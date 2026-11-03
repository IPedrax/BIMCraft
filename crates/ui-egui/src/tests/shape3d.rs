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
    assert!(
        painted(&h).iter().any(|t| t.starts_with("Click a point, a corner or an edge to select it")),
        "the 3D view's prompt: {:?}",
        painted(&h)
    );
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
fn in_3d_a_grip_held_mid_drag_previews_the_creases_its_release_will_leave() {
    let (mut h, floor) = drained();
    press(&mut h, "Modify Sub Elements");
    let (a, b) = (h.state().screen3([4000.0, 3000.0, -300.0]), h.state().screen3([4000.0, 3000.0, -100.0]));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in [0.5, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    // Held: the model as it was, the preview with the drain up, its creases down to it.
    assert_eq!(point_near(&h, floor, 4000.0, 3000.0, 0.0).map(|p| p.1), Some(-300.0));
    let preview = h.state().shape_preview.clone().expect("a preview while the grip is held");
    let z = preview.shape.points[0].z;
    assert!((z + 100.0).abs() <= 2.0, "{z}");
    let creases = h.state().session.model.shape_creases(&preview);
    assert_eq!(creases.iter().flatten().filter(|p| (p[0], p[1]) == (4000.0, 3000.0)).count(), 4, "{creases:?}");
    // Let go: the shape it previewed, and no preview.
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    assert_eq!(point_near(&h, floor, 4000.0, 3000.0, 0.0).map(|p| p.1), Some(z));
    assert!(h.state().shape_preview.is_none());
}

#[test]
fn in_3d_a_click_picks_what_the_elevation_then_sets_and_adds_points_and_split_lines_on_the_top() {
    let (mut h, floor) = drained();
    // Modify Sub Elements: the drain clicked, a little off it, picked; the Options Bar's Elevation typed, Enter.
    press(&mut h, "Modify Sub Elements");
    let at = h.state().screen3([4000.0, 3000.0, -300.0]) + vec2(3.0, -2.0);
    click_at(&mut h, at);
    assert_eq!(h.state().shape_z, "-300");
    type_into(&mut h, "Elevation:", "-40");
    key(&mut h, Key::Enter);
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

#[test]
fn a_flat_roof_by_footprint_is_shaped_as_a_floor_is_and_a_pitched_one_isnt() {
    let mut h = app(r#"roof {"level": 100001, "slope": 0, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}
roof {"level": 100001, "boundary": [[10000, 0], [18000, 0], [18000, 6000], [10000, 6000]]}"#);
    let roofs = ids(&h, "roof");
    topmost(&mut h, "Level 2");
    // The pitched one: Gable Edge, but no shape.
    click(&mut h, P2(18000.0, 3000.0));
    assert_eq!(h.state().selection, [roofs[1]]);
    assert!(h.query_by_label("Gable Edge").is_some() && h.query_by_label("Add Point").is_none());
    // The flat one: Add Point on its top, 40 mm down.
    click(&mut h, P2(8000.0, 3000.0));
    assert_eq!(h.state().selection, [roofs[0]]);
    assert!(h.query_by_label("Gable Edge").is_some());
    press(&mut h, "Add Point");
    type_into(&mut h, "Elevation:", "-40");
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(field(&h, roofs[0], "shape")["points"][0]["z"], json!(-40.0), "{}", field(&h, roofs[0], "shape"));
    assert!(painted(&h).iter().any(|t| t == "-40"), "{:?}", painted(&h));
    // In 3D its point's grip drags it, as a floor's.
    press(&mut h, "Default 3D View");
    press(&mut h, "Modify Sub Elements");
    let top = |h: &Harness<BimApp>, x: f64, y: f64| {
        let m = &h.state().session.model;
        let f = m.shapeable(roofs[0]).expect("a flat roof");
        m.floor_top(&f).at(P2(x, y)) + m.shape_height(&f, P2(x, y))
    };
    let at = top(&h, 4000.0, 3000.0);
    let (a, b) = (h.state().screen3([4000.0, 3000.0, at]), h.state().screen3([4000.0, 3000.0, at + 20.0]));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    let z = field(&h, roofs[0], "shape")["points"][0]["z"].as_f64().unwrap();
    assert!((z + 20.0).abs() <= 2.0, "{z}");
}

#[test]
fn spot_slope_reads_the_fall_of_a_shaped_floor_where_its_clicked() {
    let mut h = app(r#"floor {"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}"#);
    let floor = ids(&h, "floor")[0];
    h.state_mut().run("edit_shape", json!({"id": floor, "points": [{"at": [4000, 3000], "z": -60}]}));
    press(&mut h, "Annotate");
    press(&mut h, "Spot Slope");
    assert_eq!(h.state().tool, Tool::SpotSlope);
    click(&mut h, P2(1000.0, 3000.0));
    let spots = ids(&h, "spot");
    assert_eq!(spots.len(), 1);
    assert_eq!((field(&h, spots[0], "slope"), field(&h, spots[0], "toward")), (json!(1.5), json!([1.0, 0.0])));
    assert_eq!(h.state().tool, Tool::SpotSlope, "ready for the next one");
}

/// A click on point `label`'s height label, `text` typed over it and Enter.
fn retype_height(h: &mut Harness<BimApp>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text(text.into()));
    h.run();
    key(h, Key::Enter);
}

#[test]
fn a_points_height_label_clicked_takes_a_height_typed_in_plan_and_in_3d() {
    let mut h = app(r#"floor {"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}"#);
    let floor = ids(&h, "floor")[0];
    h.state_mut().run("edit_shape", json!({"id": floor, "points": [{"at": [4000, 3000], "z": -30}]}));
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Modify Sub Elements");
    retype_height(&mut h, "Point height -30", "-45");
    assert_eq!(point_near(&h, floor, 4000.0, 3000.0, 0.0).map(|p| p.1), Some(-45.0));
    // Under Add Point a click on a label is the label's: no point there.
    press(&mut h, "Add Point");
    h.get_by_label("Point height -45").click();
    h.run();
    key(&mut h, Key::Escape);
    assert_eq!(field(&h, floor, "shape")["points"].as_array().map(Vec::len), Some(1));
    assert_eq!(point_near(&h, floor, 4000.0, 3000.0, 0.0).map(|p| p.1), Some(-45.0), "Esc leaves it");
    // In 3D alike.
    press(&mut h, "Default 3D View");
    press(&mut h, "Modify Sub Elements");
    retype_height(&mut h, "Point height -45", "-120");
    assert_eq!(point_near(&h, floor, 4000.0, 3000.0, 0.0).map(|p| p.1), Some(-120.0));
    assert_eq!(field(&h, floor, "shape")["points"].as_array().map(Vec::len), Some(1));
}

#[test]
fn in_3d_add_point_snaps_onto_an_edge_near_the_cursor() {
    let (mut h, floor) = drained();
    press(&mut h, "Add Point");
    type_into(&mut h, "Elevation:", "-10");
    // A few pixels off the south edge, a quarter of the way along.
    let at = h.state().screen3([2000.0, 0.0, 0.0]) + vec2(2.0, 3.0);
    click_at(&mut h, at);
    let (p, z) = point_near(&h, floor, 2000.0, 0.0, 200.0).expect("a point on the edge");
    assert!(p.1.abs() < 1.0 && z == -10.0, "{p:?} {z}");
}

#[test]
fn in_plan_add_point_snaps_onto_an_edge_near_the_cursor() {
    let mut h = app(r#"floor {"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}"#);
    let floor = ids(&h, "floor")[0];
    h.state_mut().run("edit_shape", json!({"id": floor, "points": [{"at": [4000, 3000], "z": -300}]}));
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Add Point");
    type_into(&mut h, "Elevation:", "-10");
    // Just off a crease from a corner to the drain: onto it.
    click(&mut h, P2(2040.0, 1480.0));
    let (p, z) = point_near(&h, floor, 2040.0, 1480.0, 100.0).expect("a point on the crease");
    assert!((p.1 - 0.75 * p.0).abs() < 1.0 && z == -10.0, "{p:?} {z}");
}

#[test]
fn in_3d_a_picked_edges_arrow_drags_both_its_ends_up_together() {
    let (mut h, floor) = drained();
    press(&mut h, "Modify Sub Elements");
    // The south edge picked at its middle: its height on the Options Bar, its label there.
    let mid = h.state().screen3([4000.0, 0.0, 0.0]);
    click_at(&mut h, mid + vec2(0.0, 2.0));
    assert!(matches!(h.state().shape_picked.as_slice(), [crate::shape::Sub::Edge(..)]), "{:?}", h.state().shape_picked);
    assert_eq!(h.state().shape_z, "0");
    // Its ▲, beside the middle, dragged 100 mm up: both corners up, each a point now.
    let up = mid + vec2(-12.0, -9.0);
    let by = h.state().screen3([4000.0, 0.0, 100.0]) - mid;
    drag_screen(&mut h, up, up + by, PointerButton::Primary);
    for x in [0.0, 8000.0] {
        let (_, z) = point_near(&h, floor, x, 0.0, 0.0).expect("a point at the corner");
        assert!((z - 100.0).abs() <= 2.0, "{x}: {z}");
    }
    assert!((h.state().shape_z.parse::<f64>().unwrap() - 100.0).abs() <= 2.0, "{}", h.state().shape_z);
}
