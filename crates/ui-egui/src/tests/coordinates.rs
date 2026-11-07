//! Manage > Coordinates: the locations typed into and shifted, the points, the map system found by search, all a draft applied
//! as one undo step; and Rotate Project North from the ribbon.

use super::*;

fn open(h: &mut Harness<BimApp>) {
    press(h, "Manage");
    from_list(h, "Project Location", "Coordinates");
    assert!(h.state().coordinates_dialog.is_some(), "the button opens the window");
}

/// The window's Manage tab: the one beside Convert, not the ribbon's Manage.
fn manage_tab(h: &mut Harness<BimApp>) {
    press_near(h, "Manage", "Convert");
    assert_eq!(h.state().coordinates_dialog.as_ref().map(|d| d.tab()), Some(crate::coordinates::MANAGE));
}

fn said(h: &Harness<BimApp>) -> Option<String> {
    h.state().coordinates_dialog.as_ref().and_then(|d| d.error()).map(String::from)
}

#[test]
fn locations_are_typed_shifted_and_applied_as_one_undo_step() {
    let mut h = app(BOX);
    let steps = h.state().session.history().len();
    open(&mut h);
    // Shared values are typed in the map's unit, metres without a map system.
    type_role(&mut h, Role::SpinButton, "Location 1 east", "333317.911");
    type_role(&mut h, Role::SpinButton, "Location 1 north", "7394644.037");
    type_role(&mut h, Role::SpinButton, "Location 1 angle", "10");
    press(&mut h, "Add Location");
    type_into(&mut h, "Location 2 name", "Lot B");
    type_role(&mut h, Role::SpinButton, "Location 2 east", "333400");
    // The survey says the site is 5 m higher: every location goes up.
    manage_tab(&mut h);
    type_role(&mut h, Role::SpinButton, "Shift elevation", "5");
    press(&mut h, "Shift All");
    assert!(h.state().session.model.coordinates.locations.len() == 1, "nothing is the project's until OK");
    press(&mut h, "OK");
    assert!(h.state().coordinates_dialog.is_none(), "{:?}", said(&h));
    let co = &h.state().session.model.coordinates;
    let a = &co.locations[&co.active];
    assert!((a.east - 333_317_911.0).abs() < 1e-3 && (a.north - 7_394_644_037.0).abs() < 1e-3, "{a:?}");
    assert_eq!((a.angle, a.elevation), (10.0, 5000.0));
    let b = &co.locations["Lot B"];
    assert_eq!((b.east, b.north, b.angle, b.elevation), (333_400_000.0, a.north, 10.0, 5000.0), "a copy of the active one, shifted");
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert_eq!(h.state().session.history().last(), Some(&"Coordinates"));
}

#[test]
fn another_location_is_made_active_and_the_old_one_taken_away() {
    let mut h = app(BOX);
    open(&mut h);
    press(&mut h, "Add Location");
    type_into(&mut h, "Location 2 name", "Site");
    pick(&mut h, Role::RadioButton, "Location 2 active");
    pick(&mut h, Role::Button, "Delete location 1");
    press(&mut h, "OK");
    assert!(h.state().coordinates_dialog.is_none(), "{:?}", said(&h));
    let co = &h.state().session.model.coordinates;
    assert_eq!(co.active, "Site");
    assert_eq!(co.locations.keys().collect::<Vec<_>>(), ["Site"]);
}

#[test]
fn moving_the_survey_point_alone_keeps_the_building_where_it_is_in_the_world() {
    let mut h = app(BOX);
    h.state_mut().session.execute("location", &json!({"name": "Internal", "east": 500_000_000.0, "angle": 30})).unwrap();
    let before = h.state().session.model.coordinates.to_shared([0.0; 3]);
    open(&mut h);
    press(&mut h, "Points");
    let units = h.state().session.model.units;
    type_role(&mut h, Role::SpinButton, "Survey Point x", &units.length(4000.0));
    press(&mut h, "OK");
    assert!(h.state().coordinates_dialog.is_none(), "{:?}", said(&h));
    let co = &h.state().session.model.coordinates;
    assert!((co.survey[0] - 4000.0).abs() < 1e-6, "{:?}", co.survey);
    let after = co.to_shared([0.0; 3]);
    assert!(before.iter().zip(after).all(|(a, b)| (a - b).abs() < 1e-3), "{before:?} {after:?}");
}

#[test]
fn a_map_system_is_found_by_its_code_and_picked() {
    let mut h = app(BOX);
    open(&mut h);
    press(&mut h, "Map System");
    // A dropdown: its search shows once it's open.
    assert!(h.query_by_label("Find a map system").is_none());
    press(&mut h, "Choose a map system");
    type_into(&mut h, "Find a map system", "31983");
    press(&mut h, "EPSG:31983 SIRGAS 2000 / UTM zone 23S, metre");
    assert!(h.query_by_label("Find a map system").is_none(), "a pick closes it");
    press(&mut h, "OK");
    assert!(h.state().coordinates_dialog.is_none(), "{:?}", said(&h));
    assert_eq!(h.state().session.model.coordinates.crs, Some(31983));
}

#[test]
fn a_city_is_found_by_name_and_its_country_s_system_picked_with_it() {
    let mut h = app(BOX);
    open(&mut h);
    press(&mut h, "Map System");
    press(&mut h, "Choose a map system");
    type_into(&mut h, "Find a map system", "sao paulo");
    assert!(h.query_by_label("São Paulo, Brazil").is_some(), "the city, by its name without accents");
    // Its systems under it, Brazil's datum first.
    press(&mut h, "São Paulo: EPSG:31983 SIRGAS 2000 / UTM zone 23S, metre");
    let at = h.state().coordinates_dialog.as_ref().unwrap().lat_lon();
    assert!((at[0] + 23.557).abs() < 0.01 && (at[1] + 46.627).abs() < 0.01, "the place to Place the Survey Point at: {at:?}");
    press(&mut h, "OK");
    assert!(h.state().coordinates_dialog.is_none(), "{:?}", said(&h));
    assert_eq!(h.state().session.model.coordinates.crs, Some(31983));
    assert_eq!(h.state().session.model.coordinates.location().east, 0.0, "the Survey Point stays until Place");
}

#[test]
fn rotate_project_north_aligns_with_true_north_from_the_ribbon() {
    let mut h = app(BOX);
    h.state_mut().session.execute("location", &json!({"name": "Internal", "angle": 12.5})).unwrap();
    press(&mut h, "Manage");
    from_list(&mut h, "Project Location", "Rotate Project North");
    press(&mut h, "Align with True North (12.500°)");
    assert_eq!(h.state().session.model.coordinates.location().angle, 0.0);
    assert_eq!(h.state().session.history().last(), Some(&"Rotate Project North"));
}

#[test]
fn specify_coordinates_steps_aside_for_a_point_picked_on_the_plan_and_comes_back_to_review() {
    let mut h = app(BOX);
    open(&mut h);
    manage_tab(&mut h);
    assert!(h.query_by_label("Specify at x").is_none(), "shared coordinates only: the internal point is picked");
    // Without a map system, shared values are typed in metres.
    type_role(&mut h, Role::SpinButton, "Specify east", "1000");
    type_role(&mut h, Role::SpinButton, "Specify north", "2000");
    type_role(&mut h, Role::SpinButton, "Specify elevation", "50");
    h.get_by_label("Pick Point and Specify").scroll_to_me();
    h.run();
    press(&mut h, "Pick Point and Specify");
    assert_eq!(h.state().tool, Tool::SpecifyPoint);
    assert!(h.query_by_label("Pick Point and Specify").is_none(), "the window steps aside");
    // Near the wall's corner: it snaps there, at the wall's bottom.
    click(&mut h, P2(8030.0, 25.0));
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.query_by_label("Pick Point and Specify").is_some(), "the window is back, to review");
    assert_eq!(h.state().session.model.coordinates.location().east, 0.0, "nothing is the project's until OK");
    press(&mut h, "OK");
    assert!(h.state().coordinates_dialog.is_none(), "{:?}", said(&h));
    let co = &h.state().session.model.coordinates;
    let q = co.to_shared([8000.0, 0.0, 0.0]);
    assert!((q[0] - 1_000_000.0).abs() < 1e-3 && (q[1] - 2_000_000.0).abs() < 1e-3 && (q[2] - 50_000.0).abs() < 1e-3, "{q:?}");
}

#[test]
fn in_the_3d_view_the_point_is_the_corner_picked_top_or_bottom_as_seen() {
    let mut h = app(BOX);
    shortcut(&mut h, "3D");
    open(&mut h);
    manage_tab(&mut h);
    type_role(&mut h, Role::SpinButton, "Specify elevation", "100");
    h.get_by_label("Pick Point and Specify").scroll_to_me();
    h.run();
    press(&mut h, "Pick Point and Specify");
    // The south wall's outer top corner, snapped to.
    let at = h.state().screen3([8100.0, -100.0, 3000.0]) + egui::vec2(1.0, 0.5);
    click_at(&mut h, at);
    assert!(h.query_by_label("Pick Point and Specify").is_some(), "the window is back: {}", h.state().status.text);
    press(&mut h, "OK");
    let q = h.state().session.model.coordinates.to_shared([8100.0, -100.0, 3000.0]);
    assert!((q[2] - 100_000.0).abs() < 1e-3, "{q:?}");
}

#[test]
fn esc_while_picking_the_point_brings_the_window_back_unchanged() {
    let mut h = app(BOX);
    open(&mut h);
    manage_tab(&mut h);
    h.get_by_label("Pick Point and Specify").scroll_to_me();
    h.run();
    press(&mut h, "Pick Point and Specify");
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.query_by_label("Pick Point and Specify").is_some(), "the window is back");
}

#[test]
fn rotate_project_north_is_a_tool_on_the_plan_that_turns_the_whole_model_about_the_base_point() {
    let mut h = app(BOX);
    h.state_mut().session.execute("coordinates", &json!({"base": [8000.0, 0.0, 0.0]})).unwrap();
    let wall = ids(&h, "wall")[0];
    press(&mut h, "Manage");
    from_list(&mut h, "Project Location", "Rotate Project North");
    assert!(
        matches!(h.state().tool, Tool::RotateNorth { centre, start: None, place: false } if centre == P2(8000.0, 0.0)),
        "it starts on the Project Base Point: {:?}",
        h.state().tool
    );
    assert_eq!(h.state().session.model.coordinates.location().angle, 0.0, "nothing turns until the angle's end");
    // The angle's start north of the base point, its end west of it: 90° counter-clockwise.
    click(&mut h, P2(8000.0, 4000.0));
    click(&mut h, P2(3000.0, 0.0));
    let m = &h.state().session.model;
    assert_eq!(m.coordinates.location().angle, -90.0, "true north turned back as much: the building stays in the world");
    let Some(Element::Wall(w)) = m.elements.get(wall) else { panic!() };
    assert!(w.start.dist(P2(8000.0, -8000.0)) < 1e-6, "the whole model turned, about the base point: {:?}", w.start);
    assert_eq!(h.state().session.history().last(), Some(&"Rotate Project North"));
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn rotate_project_north_snaps_its_rays_and_space_moves_its_centre() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    from_list(&mut h, "Project Location", "Rotate Project North");
    // Space: the next click is the centre, snapped.
    h.key_press(Key::Space);
    h.run();
    assert!(matches!(h.state().tool, Tool::RotateNorth { place: true, .. }), "{:?}", h.state().tool);
    click(&mut h, P2(30.0, 25.0));
    assert!(matches!(h.state().tool, Tool::RotateNorth { centre, place: false, .. } if centre == P2(0.0, 0.0)), "{:?}", h.state().tool);
    // The angle's start near a corner: on the corner.
    click(&mut h, P2(8030.0, 25.0));
    assert!(matches!(h.state().tool, Tool::RotateNorth { start: Some(s), .. } if s == P2(8000.0, 0.0)), "{:?}", h.state().tool);
}

#[test]
fn space_moves_rotate_s_centre_too() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Rotate");
    h.key_press(Key::Space);
    h.run();
    assert!(matches!(h.state().tool, Tool::Rotate { place: true, .. }), "{:?}", h.state().tool);
}

#[test]
fn rotate_project_north_takes_an_angle_typed_in_the_options_bar() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    from_list(&mut h, "Project Location", "Rotate Project North");
    type_into(&mut h, "Angle:", "30");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session.model.coordinates.location().angle, -30.0);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn each_location_shows_its_id_and_a_new_one_has_none_until_applied() {
    let mut h = app(BOX);
    let id = h.state().session.model.library.id(&bimcraft_core::library::Kind::Location, "Internal").unwrap().to_string();
    open(&mut h);
    assert!(h.query_by_label(&id).is_some(), "the active location's id, {id}, in its row");
    press(&mut h, "Add Location");
    assert!(h.query_by_label("—").is_some(), "a new one has no id yet");
    press(&mut h, "OK");
    let new = h.state().session.model.library.id(&bimcraft_core::library::Kind::Location, "New Location").unwrap().to_string();
    open(&mut h);
    assert!(h.query_by_label(&new).is_some(), "{new}");
}

#[test]
fn a_click_on_the_world_map_picks_the_latitude_and_longitude_to_place_by() {
    let mut h = app(BOX);
    h.state_mut().session.execute("crs", &json!({"code": 31983})).unwrap();
    h.state_mut().session.execute("location", &json!({"name": "Internal", "east": 333_317_911.0, "north": 7_394_644_037.0})).unwrap();
    open(&mut h);
    press(&mut h, "Map System");
    let before = h.state().coordinates_dialog.as_ref().unwrap().lat_lon();
    // The map opens on the place: its middle is the Survey Point's latitude and longitude.
    let map = h.get_by_label("World map").rect();
    let east = map.center() + egui::vec2(40.0, 0.0);
    click_at(&mut h, east);
    let after = h.state().coordinates_dialog.as_ref().unwrap().lat_lon();
    assert!((after[0] - before[0]).abs() < 0.1 && after[1] > before[1] + 0.5, "east of it: {before:?} then {after:?}");
    // Under the map, scrolled to as a user would.
    h.get_by_label("Place").scroll_to_me();
    h.run();
    press(&mut h, "Place");
    press(&mut h, "OK");
    let l = h.state().session.model.coordinates.location();
    assert!(l.east > 333_317_911.0 + 50_000_000.0, "the Survey Point placed further east: {l:?}");
}

/// The plan with Site's three points shown, the Project Base Point at (2000, 1000) and the Survey Point at (6000, 4000).
fn points_shown() -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    h.state_mut().run("coordinates", json!({"base": [2000.0, 1000.0, 0.0], "survey": [6000.0, 4000.0, 0.0]}));
    h.state_mut().run("graphics", json!({"view": "Level 1", "show": ["internalorigin", "projectbasepoint", "surveypoint"]}));
    h.run();
    h
}

fn point_id(h: &Harness<BimApp>, p: bimcraft_core::coordinates::Point) -> Id {
    p.id(&h.state().session.model).unwrap()
}

#[test]
fn a_point_is_picked_on_the_plan_and_its_properties_say_where_it_is_and_move_it() {
    use bimcraft_core::coordinates::Point;
    let mut h = points_shown();
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(h.state().selection, [point_id(&h, Point::Base)], "picked by its mark");
    assert!(h.query_by_label("Project Base Point").is_some(), "Properties names it");
    type_role(&mut h, Role::SpinButton, "Point X", "2500");
    assert_eq!(h.state().session.model.coordinates.base, [2500.0, 1000.0, 0.0], "typed, it moves");
    // The Internal Origin is shown and never moves.
    click(&mut h, P2(0.0, 0.0));
    assert_eq!(h.state().selection, [point_id(&h, Point::Internal)]);
    assert!(h.query_by_label("Internal Origin").is_some());
}

#[test]
fn a_point_dragged_moves_and_the_survey_point_moves_as_its_clip_says() {
    use bimcraft_core::coordinates::Point;
    let mut h = points_shown();
    let before = h.state().session.model.coordinates.to_shared([0.0; 3]);
    click(&mut h, P2(6000.0, 4000.0));
    assert_eq!(h.state().selection, [point_id(&h, Point::Survey)]);
    // Clipped (the default): the point alone; the world stays.
    drag(&mut h, P2(6000.0, 4000.0), P2(7000.0, 4000.0));
    let co = &h.state().session.model.coordinates;
    assert_eq!(co.survey, [7000.0, 4000.0, 0.0]);
    assert_eq!(co.to_shared([0.0; 3]), before, "the building stays where it is in the world");
    // Unclipped: the shared coordinates go with it.
    h.get_by_role_and_label(Role::RadioButton, "Move the shared coordinates with it").scroll_to_me();
    h.run();
    pick(&mut h, Role::RadioButton, "Move the shared coordinates with it");
    drag(&mut h, P2(7000.0, 4000.0), P2(7000.0, 5000.0));
    let co = &h.state().session.model.coordinates;
    assert_eq!(co.survey, [7000.0, 5000.0, 0.0]);
    assert!((co.to_shared([0.0; 3])[1] - before[1]).abs() > 1.0, "the world moved with it");
}

#[test]
fn the_3d_view_shows_the_points_its_graphics_show_and_a_click_picks_one() {
    use bimcraft_core::coordinates::Point;
    let mut h = app(BOX);
    h.state_mut().run("coordinates", json!({"base": [2000.0, 1000.0, 0.0]}));
    shortcut(&mut h, "3D");
    assert!(h.state().points_in_3d().is_empty(), "hidden until the view's graphics show them");
    h.state_mut().run("graphics", json!({"view": "{3D}", "show": ["projectbasepoint"]}));
    h.run();
    assert_eq!(h.state().points_in_3d().iter().map(|(p, _)| *p).collect::<Vec<_>>(), [Point::Base]);
    let at = h.state().screen3([2000.0, 1000.0, 0.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [point_id(&h, Point::Base)]);
}

#[test]
fn the_status_bar_reads_the_cursor_out_in_shared_coordinates_once_they_are_not_the_file_s_own() {
    let mut h = app(BOX);
    assert_eq!(h.state().shared_readout(P2(1000.0, 2000.0)), None, "the same numbers: nothing more to say");
    h.state_mut().run("location", json!({"name": "Internal", "east": 333_000_000.0, "north": 7_394_000_000.0}));
    assert_eq!(h.state().shared_readout(P2(1000.0, 2000.0)).as_deref(), Some("E 333001.000 m  N 7394002.000 m"));
    h.state_mut().run("crs", json!({"code": 2263}));
    let s = h.state().shared_readout(P2(0.0, 0.0)).unwrap();
    assert!(s.ends_with("US ft"), "in the map system's unit: {s}");
}

#[test]
fn the_manage_tab_sits_before_convert_with_specify_shift_acquire_and_reset() {
    let mut h = app(BOX);
    open(&mut h);
    // On the tabs' row: Convert's.
    let row = h.get_by_label("Convert").rect().center().y;
    let x = |h: &Harness<BimApp>, l: &str| {
        h.get_all_by_label(l).map(|n| n.rect().center()).find(|c| (c.y - row).abs() < 4.0).map_or(f32::NAN, |c| c.x)
    };
    assert!(x(&h, "Map System") < x(&h, "Manage") && x(&h, "Manage") < x(&h, "Convert"), "Manage right before Convert");
    manage_tab(&mut h);
    for label in ["Pick Point and Specify", "Shift All", "Pick in the View", "Reset Coordinates…"] {
        assert!(h.query_by_label(label).is_some(), "{label} on Manage");
    }
    press(&mut h, "Locations");
    assert!(h.query_by_label("Shift All").is_none(), "Shift All moved off Locations");
}

#[test]
fn reset_asks_first_then_puts_the_coordinates_back_and_pins_nothing_it_needn_t() {
    let mut h = app(BOX);
    h.state_mut().run("location", json!({"name": "Internal", "east": 500_000_000.0, "north": 7_000_000_000.0, "angle": 20}));
    h.state_mut().run("coordinates", json!({"base": [1000.0, 2000.0, 0.0]}));
    open(&mut h);
    manage_tab(&mut h);
    h.get_by_label("Reset Coordinates…").scroll_to_me();
    h.run();
    press(&mut h, "Reset Coordinates…");
    assert!(h.query_by_label("Reset the project's coordinates?").is_some(), "it asks first");
    // Cancel: nothing changes.
    press_near(&mut h, "Cancel", "Reset the project's coordinates?");
    assert!(h.query_by_label("Reset the project's coordinates?").is_none());
    assert_eq!(h.state().session.model.coordinates.location().east, 500_000_000.0);
    press(&mut h, "Reset Coordinates…");
    press(&mut h, "Reset");
    let co = &h.state().session.model.coordinates;
    assert_eq!((co.location().east, co.location().north, co.location().angle, co.base), (0.0, 0.0, 0.0, [0.0; 3]));
    assert!(h.state().coordinates_dialog.as_ref().is_some_and(|d| d.tab() == crate::coordinates::MANAGE), "the window stays, on Manage");
    assert_eq!(h.state().session.history().last(), Some(&"Coordinates"));
}

#[test]
fn acquire_from_the_manage_tab_takes_a_drawing_s_coordinates() {
    let mut h = app(BOX);
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-manage-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("square.dxf");
    let pairs = |list: &[(i32, &str)]| list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>();
    let line = pairs(&[(0, "LINE"), (8, "0"), (10, "500000"), (20, "7000000"), (11, "500004"), (21, "7000000")]);
    let header = pairs(&[(9, "$INSUNITS"), (70, "6")]);
    std::fs::write(&path, format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{line}0\nENDSEC\n0\nEOF\n")).unwrap();
    h.state_mut().run("import_cad", json!({"path": path.to_string_lossy(), "at": [4000, 3000]}));
    open(&mut h);
    manage_tab(&mut h);
    press(&mut h, "Acquire from square (CAD drawing)");
    let q = h.state().session.model.coordinates.to_shared([4000.0, 3000.0, 0.0]);
    assert!((q[0] - 500_002_000.0).abs() < 1e-3 && (q[1] - 7_000_000_000.0).abs() < 1e-3, "{q:?}");
    assert!(h.state().coordinates_dialog.is_some(), "the window stays open, showing them");
}
