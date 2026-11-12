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

#[test]
fn rp_starts_the_reference_plane_tool_as_revits_does() {
    let mut h = app(BOX);
    shortcut(&mut h, "RP");
    assert!(matches!(h.state().tool, Tool::ReferencePlane(None)), "{:?}", h.state().tool);
    assert!(h.query_by_label("Modify | Place Reference Plane").is_some());
    // Keyboard Shortcuts lists it, by its name.
    assert!(crate::shortcuts().iter().any(|(letters, name)| *letters == "RP" && *name == "Reference Plane"));
}

#[test]
fn the_ref_plane_button_stands_beside_set_on_architecture_and_starts_the_reference_plane_tool() {
    // Wide enough for the whole tab: the button itself, after Set.
    let mut h = app(BOX);
    h.set_size(vec2(2400.0, 900.0));
    h.run();
    press(&mut h, "Architecture");
    let (set, rp) = (h.get_by_label("Set").rect(), h.get_by_label("Ref Plane").rect());
    assert!(rp.left() >= set.right() - 1.0 && (rp.center().y - set.center().y).abs() < 2.0, "beside Set: {set:?} {rp:?}");
    press(&mut h, "Ref Plane");
    assert!(matches!(h.state().tool, Tool::ReferencePlane(None)), "{:?}", h.state().tool);
    // Where the window folds the panel, its list has it.
    let mut h = app(BOX);
    press(&mut h, "Architecture");
    from_list(&mut h, "Datum", "Ref Plane");
    assert!(matches!(h.state().tool, Tool::ReferencePlane(None)), "{:?}", h.state().tool);
}

#[test]
fn properties_show_revits_level_and_elevation_from_level_for_a_component_not_work_plane_based() {
    let mut h = app(&format!(
        "{BOX}\nfamily {{\"name\": \"Box\", \"types\": {{\"A\": {{}}}}, \"forms\": [{{\"profile\": [[0, 0], [600, 0], [600, 600], [0, 600]], \"to\": 50}}]}}\n\
         component {{\"family\": \"Box\", \"at\": [2000, 3000], \"offset\": 450}}"
    ));
    let c = *ids(&h, "component").last().unwrap();
    h.state_mut().selection = vec![c];
    h.run();
    assert!(h.query_by_label("Level").is_some());
    assert!(h.query_by_label("Height Offset From Level").is_none());
    let elevation = h.get_by_role_and_label(Role::TextInput, "Elevation from Level").value().unwrap_or_default();
    assert_eq!(elevation, "450", "the same value, under Revit's name");
    for gone in ["Schedule Level", "Work Plane", "Offset from Host"] {
        assert!(h.query_by_label(gone).is_none(), "{gone}");
    }
}

/// Edit Work Plane on a selected work plane-based component (T12): the Work Plane dialog, naming its plane, puts it on a
/// named reference plane, a face picked in the 3D view, or the level of a line picked, by `edit_work_plane`.
#[test]
fn edit_work_plane_puts_a_selected_component_on_a_named_plane_a_picked_face_or_a_picked_lines_level() {
    let script = format!(
        "{BOX}\n{SIGN}\nfloor {{\"boundary\": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], \"offset\": 300}}\n\
         component {{\"family\": \"Sign\", \"at\": [2000, 3000]}}"
    );
    let mut h = app(&script);
    let c = *ids(&h, "component").last().unwrap();
    assert_eq!(field(&h, c, "work_plane"), json!("Level : Level 1"));
    h.state_mut().selection = vec![c];
    h.state_mut().raise_context();
    h.run();
    press(&mut h, "Edit Work Plane");
    assert!(h.state().work_plane.as_ref().is_some_and(|d| d.components == vec![c]), "the dialog, for the component");
    assert!(h.query_by_label("Pick three points").is_none(), "a component isn't put on three points");
    if let Some(d) = h.state_mut().work_plane.as_mut() {
        d.name = "Reference Plane : Ramp".into();
    }
    h.run();
    press(&mut h, "OK");
    assert!(h.state().work_plane.is_none(), "{:?}", h.state().status);
    assert_eq!(field(&h, c, "work_plane"), json!("Reference Plane : Ramp"));
    // Pick a plane: the floor's top, clicked in the 3D view, which the pick opens.
    press(&mut h, "Edit Work Plane");
    pick(&mut h, Role::RadioButton, "Pick a plane");
    press(&mut h, "OK");
    assert_eq!(h.state().sheet, Sheet::View3D);
    let at = h.state().screen3([4000.0, 4500.0, 300.0]);
    click_at(&mut h, at);
    assert!(h.state().work_plane.is_none(), "{:?}", h.state().status);
    assert_eq!(field(&h, c, "work_plane"), json!("Floor : Generic - 200mm : Top"));
    // Pick a line: a wall, whose level is taken.
    h.state_mut().selection = vec![c];
    h.state_mut().raise_context();
    h.run();
    press(&mut h, "Edit Work Plane");
    pick(&mut h, Role::RadioButton, "Pick a line and use the work plane it was sketched in");
    press(&mut h, "OK");
    let half = {
        let m = &h.state().session.model;
        m.thickness(m.wall(ids(&h, "wall")[0]).unwrap()) / 2.0
    };
    let at = h.state().screen3([4000.0, -half, 1500.0]);
    click_at(&mut h, at);
    assert!(h.state().work_plane.is_none(), "{:?}", h.state().status);
    assert_eq!(field(&h, c, "work_plane"), json!("Level : Level 1"));
}

/// The project's Edit Work Plane dialog, rendered for a look (`BIMCRAFT_T12_DUMP`: a folder).
#[test]
fn component_edit_work_plane_picture() {
    let Ok(folder) = std::env::var("BIMCRAFT_T12_DUMP") else { return };
    let mut h = app(&format!("{BOX}\n{SIGN}\ncomponent {{\"family\": \"Sign\", \"at\": [2000, 3000], \"host\": {{\"plane\": \"Ramp\"}}}}"));
    h.set_size(vec2(1600.0, 1000.0));
    let c = *ids(&h, "component").last().unwrap();
    h.state_mut().selection = vec![c];
    h.state_mut().raise_context();
    h.run();
    press(&mut h, "Edit Work Plane");
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("t12-component-edit-work-plane.png")).unwrap();
}
