//! Manage > Coordinates: the locations typed into and shifted, the points, the map system found by search, all a draft applied
//! as one undo step; and Rotate Project North from the ribbon.

use super::*;

fn open(h: &mut Harness<BimApp>) {
    press(h, "Manage");
    from_list(h, "Project Location", "Coordinates");
    assert!(h.state().coordinates_dialog.is_some(), "the button opens the window");
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
