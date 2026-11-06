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
    type_into(&mut h, "Find a map system", "31983");
    press(&mut h, "EPSG:31983 SIRGAS 2000 / UTM zone 23S, metre");
    press(&mut h, "OK");
    assert!(h.state().coordinates_dialog.is_none(), "{:?}", said(&h));
    assert_eq!(h.state().session.model.coordinates.crs, Some(31983));
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
