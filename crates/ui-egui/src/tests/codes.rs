//! Manage > Building Codes: the code stairs and railings are held to, and the rules the project changes of it.

use super::*;

#[test]
fn building_codes_choose_a_code_and_change_its_rules() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    assert!(h.state().codes_dialog.is_some(), "the button opens the dialog");

    // The IBC, with a width of the project's own.
    {
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.selected = "ibc".into();
        d.rules = serde_json::to_value(&bimcraft_core::codes::code("ibc").unwrap().stairs).unwrap().as_object().unwrap().clone();
        d.rules.insert("width_min".into(), serde_json::json!(914.4));
    }
    h.run();
    press(&mut h, "OK");
    assert!(h.state().codes_dialog.is_none(), "OK closes it");
    let m = &h.state().session.model;
    assert_eq!(m.building_code().code, "ibc");
    assert_eq!(m.stair_rules().width_min, 914.4);
    assert_eq!(m.stair_rules().riser_max, 177.8, "the rest is the code's");

    // Rules that cannot be keep the dialog open.
    press(&mut h, "Building Codes");
    h.state_mut().codes_dialog.as_mut().unwrap().rules.insert("riser_min".into(), serde_json::json!(500));
    press(&mut h, "OK");
    assert!(h.state().codes_dialog.is_some());
    press(&mut h, "Cancel");
    assert!(h.state().codes_dialog.is_none());
}

#[test]
fn the_stair_tool_s_winders_make_the_turn_of_treads() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    h.state_mut().stair_winders = 3;
    // 7 treads north, then to the right: three winders where the landing would be.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    click(&mut h, P2(3000.0, 3500.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "landings")), (1, json!([{"at": 8.0, "turn": -90.0, "winders": 3}])));
}

#[test]
fn the_stair_tool_draws_by_its_location_line() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    h.state_mut().stair_line = bimcraft_core::model::StairLine::Left;
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    click(&mut h, P2(3000.0, 3500.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "line"), field(&h, st[0], "start")), (1, json!("left"), json!([1000.0, 1000.0])));
    // Climbing north with the line its left edge, the stair lies east of the clicks.
    let m = &h.state().session.model;
    let Some(Element::Stair(s)) = m.elements.get(st[0]) else { panic!("no stair") };
    assert_eq!(s.foot(), P2(1500.0, 1000.0));
}

#[test]
fn a_stair_outside_its_code_lists_the_rules_in_a_popup() {
    let mut h = app(BOX);
    h.state_mut().run("building_code", json!({"set": "nbr"}));
    let id =
        h.state_mut().run("stair", json!({"start": [1000, 1000], "angle": 90, "riser": 200, "tread": 240, "width": 1200})).unwrap()["id"]
            .as_u64()
            .unwrap();
    h.state_mut().selection = vec![id];
    h.run();
    // The row is far down a palette that scrolls: clicked as an assistive tool clicks it, wherever it is.
    h.get_by_label("Outside 2 rules").click_accesskit();
    h.run();
    assert!(h.query_by_label("Building Code Check").is_some(), "the window");
    assert_eq!(h.query_all_by_label("The rule:").count(), 2, "each with what the stair has and what the rule asks");
    // Inside the rules again, the window goes.
    h.state_mut().run("building_code", json!({"set": "generic"}));
    h.run();
    assert!(h.query_by_label("Building Code Check").is_none());
}

#[test]
fn a_code_changed_is_custom_and_is_saved_under_a_name_and_deleted() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    {
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.selected = "nbr".into();
        d.rules = serde_json::to_value(&bimcraft_core::codes::code("nbr").unwrap().stairs).unwrap().as_object().unwrap().clone();
    }
    h.run();
    assert!(h.query_by_label("Custom").is_none(), "the code's own rules");
    h.state_mut().codes_dialog.as_mut().unwrap().rules.insert("width_min".into(), json!(1100.0));
    h.run();
    assert!(h.query_by_label("Custom").is_some(), "a rule changed");
    h.state_mut().codes_dialog.as_mut().unwrap().name = "City Hall".into();
    h.run();
    press(&mut h, "Save Code");
    let m = &h.state().session.model;
    assert_eq!(
        (m.building_code().code.as_str(), m.building_code().based_on.as_str(), m.stair_rules().width_min),
        ("City Hall", "nbr", 1100.0)
    );
    assert_eq!(h.state().codes_dialog.as_ref().unwrap().selected, "City Hall");
    assert!(h.query_by_label("Custom").is_none(), "saved, it is a code like the others");
    // A name BIMCraft has is refused inside the dialog.
    h.state_mut().codes_dialog.as_mut().unwrap().rules.insert("width_min".into(), json!(1000.0));
    h.state_mut().codes_dialog.as_mut().unwrap().name = "ibc".into();
    h.run();
    press(&mut h, "Save Code");
    assert!(h.state().codes_dialog.as_ref().unwrap().error.contains("another name"));
    press(&mut h, "Reset to the Code's Rules");
    press(&mut h, "Delete");
    let m = &h.state().session.model;
    assert!(m.building_codes.is_empty());
    assert_eq!((m.building_code().code.as_str(), m.stair_rules().width_min), ("nbr", 1100.0), "its rules stay, as the project's own");
}

#[test]
fn a_stair_s_landings_and_turns_are_edited_in_properties() {
    let mut h = app(BOX);
    let id = h.state_mut().run("stair", json!({"start": [1000, 1000], "angle": 90})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().selection = vec![id];
    h.run();
    assert_eq!(field(&h, id, "landings"), json!(null), "one run");
    h.get_by_label("Add Landing").click_accesskit();
    h.run();
    // 3000 up in 17 risers: the landing where the run is halved.
    assert_eq!(field(&h, id, "landings"), json!([{"at": 9.0, "turn": 0.0}]));
    assert!(h.query_by_label("Turn 1").is_some());
    h.get_by_label("Remove").click_accesskit();
    h.run();
    assert_eq!(field(&h, id, "landings"), json!(null));
    // Under a code that asks a landing sooner than the stair has one.
    h.state_mut().run("building_code", json!({"set": "generic", "stairs": {"flight_risers_max": 6}}));
    h.run();
    h.get_by_label("By the Code").click_accesskit();
    h.run();
    assert_eq!(field(&h, id, "landings"), json!([{"at": 6.0, "turn": 0.0}, {"at": 11.0, "turn": 0.0}]));
}

#[test]
fn a_stair_selected_flips_and_shows_its_code_check_from_the_ribbon() {
    let mut h = app(BOX);
    h.state_mut().run("building_code", json!({"set": "nbr"}));
    let id =
        h.state_mut().run("stair", json!({"start": [1000, 1000], "angle": 90, "riser": 200, "tread": 240, "width": 1200})).unwrap()["id"]
            .as_u64()
            .unwrap();
    h.run();
    // Picked on the plan, on a riser.
    click(&mut h, P2(1000.0, 1480.0));
    assert_eq!(h.state().selection, [id]);
    press(&mut h, "Flip");
    assert_eq!(field(&h, id, "angle"), json!(-90.0), "it climbs the other way");
    press(&mut h, "Code Check");
    assert!(h.query_by_label("Building Code Check").is_some());
}
