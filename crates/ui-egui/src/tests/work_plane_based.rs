use super::*;

/// A box, a work plane-based family `Sign` that tilts with its plane, and a reference plane `Ramp` rising 30 degrees north.
const SIGN: &str = r#"family {"name": "Sign", "types": {"A": {}}, "options": {"work_plane_based": true, "always_vertical": false}, "forms": [{"profile": [[0, 0], [600, 0], [600, 600], [0, 600]], "to": 50}]}
reference_plane {"name": "Ramp", "start": [0, 0, 0], "end": [8000, 0, 0], "up": [0, 0.8660254037844386, 0.5]}"#;

fn placing_sign(h: &mut Harness<BimApp>) {
    h.state_mut().component = Some(("Sign".into(), "A".into()));
    h.state_mut().set_tool(Tool::Component);
    h.run();
}

#[test]
fn a_work_plane_based_family_is_placed_on_the_placement_plane_and_rotated_after_placement() {
    let mut h = app(&format!("{BOX}\n{SIGN}"));
    placing_sign(&mut h);
    // Its Options Bar: Place on Work Plane, the plan's level as its Placement Plane until another is chosen.
    assert!(h.query_by_label("Place on Work Plane").is_some());
    assert_eq!(combo_value(&h, "Placement Plane:"), "Level : Level 1");
    choose(&mut h, "Placement Plane:", "Reference Plane : Ramp");
    assert_eq!(combo_value(&h, "Placement Plane:"), "Reference Plane : Ramp");
    click(&mut h, P2(2000.0, 3000.0));
    let placed = ids(&h, "component");
    let c = *placed.last().unwrap();
    assert_eq!(field(&h, c, "work_plane"), json!("Reference Plane : Ramp"));
    let z = field(&h, c, "offset").as_f64().unwrap();
    assert!((z - 3000.0 * 0.5 / 0.866_025_403_784_438_6).abs() < 1.0, "on the plane where it was clicked: {z}");
    // Rotate after placement: the next one is selected and Rotate turns it about its point.
    h.state_mut().rotate_after = true;
    h.run();
    click(&mut h, P2(5000.0, 3000.0));
    let c2 = *ids(&h, "component").last().unwrap();
    assert_ne!(c2, c);
    assert_eq!(h.state().selection, vec![c2]);
    assert!(matches!(h.state().tool, Tool::Rotate { centre, .. } if centre.dist(P2(5000.0, 3000.0)) < 1.0), "{:?}", h.state().tool);
}

#[test]
fn properties_show_a_hosted_components_schedule_level_work_plane_and_offset_from_host() {
    let mut h = app(&format!("{BOX}\n{SIGN}\ncomponent {{\"family\": \"Sign\", \"at\": [2000, 3000], \"host\": {{\"plane\": \"Ramp\"}}}}"));
    let c = *ids(&h, "component").last().unwrap();
    h.state_mut().selection = vec![c];
    h.run();
    assert!(h.query_by_label("Schedule Level").is_some());
    assert!(h.query_by_label("Reference Plane : Ramp").is_some(), "its Work Plane, read only");
    assert!(h.query_by_label("Offset from Host").is_some());
    // Offset from Host typed: it moves off the plane along its normal.
    type_into(&mut h, "Offset from Host", "200");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(field(&h, c, "host")["offset"], json!(200.0), "{:?}", h.state().status);
}

#[test]
fn the_reference_plane_tool_draws_an_upright_plane_through_two_clicks() {
    let mut h = app(BOX);
    h.state_mut().set_tool(Tool::ReferencePlane(None));
    h.run();
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(7000.0, 3000.0));
    let planes = ids(&h, "refplane");
    assert_eq!(planes.len(), 1, "{:?}", h.state().status);
    assert_eq!(field(&h, planes[0], "up"), json!([0.0, 0.0, 1.0]));
}

#[test]
fn place_on_face_puts_a_work_plane_based_family_on_the_floor_in_plan_and_the_ceiling_in_a_ceiling_plan() {
    let script = format!(
        "{BOX}\n{SIGN}\nfloor {{\"boundary\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], \"offset\": 300}}\n\
         ceiling {{\"boundary\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}}"
    );
    let mut h = app(&script);
    placing_sign(&mut h);
    assert!(h.query_by_label("Place on Face").is_some());
    press(&mut h, "Place on Face");
    assert!(h.state().place_on_face);
    // In plan: on the floor's top, 300 over Level 1.
    click(&mut h, P2(2000.0, 3000.0));
    let c = *ids(&h, "component").last().unwrap();
    assert_eq!(field(&h, c, "work_plane"), json!("Floor : Generic - 200mm : Top"), "{:?}", h.state().status);
    assert_eq!(field(&h, c, "offset"), json!(300.0));
    // Off the floor: nothing to place on, said so.
    let before = ids(&h, "component").len();
    click(&mut h, P2(-3000.0, 3000.0));
    assert_eq!(ids(&h, "component").len(), before);
    assert!(h.state().status.error, "{:?}", h.state().status);
    // In the ceiling plan: on the ceiling's underside.
    let level = h.state().session.model.levels()[0].0;
    h.state_mut().open_sheet(Sheet::Ceiling(level));
    h.run();
    placing_sign(&mut h);
    click(&mut h, P2(4000.0, 3000.0));
    let c = *ids(&h, "component").last().unwrap();
    assert_eq!(field(&h, c, "work_plane"), json!("Ceiling : Generic - 20mm : Bottom"), "{:?}", h.state().status);
}

#[test]
fn place_on_face_in_the_3d_view_puts_it_on_the_wall_face_clicked() {
    let mut h = app(&format!("{BOX}\n{SIGN}"));
    shortcut(&mut h, "3D");
    placing_sign(&mut h);
    press(&mut h, "Place on Face");
    // The south wall's outer face, 1500 up.
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    let placed = ids(&h, "component");
    let c = *placed.last().unwrap_or_else(|| panic!("nothing placed: {:?}", h.state().status));
    let plane = field(&h, c, "work_plane");
    assert!(plane.as_str().is_some_and(|p| p.starts_with("Wall : Generic - 200mm : ")), "{plane}");
    let at = field(&h, c, "at");
    assert!((at[1].as_f64().unwrap() + 100.0).abs() < 1.0, "on the wall's outer face: {at}");
    assert!((field(&h, c, "offset").as_f64().unwrap() - 1500.0).abs() < 20.0, "where it was clicked");
}
