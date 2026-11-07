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
