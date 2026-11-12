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

#[test]
fn a_selected_stair_s_grips_drag_its_width_its_turn_and_its_foot() {
    let mut h = app(BOX);
    let p = json!({"start": [2000, 1000], "angle": 90, "width": 1000, "landings": [{"at": 6, "turn": -90}]});
    let id = h.state_mut().run("stair", p).unwrap()["id"].as_u64().unwrap();
    h.run();
    click(&mut h, P2(2000.0, 1480.0));
    assert_eq!(h.state().selection, [id]);
    // Its right side, halfway up the first run, out 200: 400 wider about its middle.
    drag(&mut h, P2(2500.0, 1700.0), P2(2700.0, 1700.0));
    assert_eq!(field(&h, id, "width"), json!(1400.0));
    // The end of the first run, two treads back: the landing after 4 risers.
    drag(&mut h, P2(2000.0, 2400.0), P2(2000.0, 1840.0));
    assert_eq!(field(&h, id, "landings"), json!([{"at": 4.0, "turn": -90.0}]));
    // Its foot, to another place: the stair goes with it.
    drag(&mut h, P2(2000.0, 1000.0), P2(3000.0, 1200.0));
    let start = field(&h, id, "start");
    let (x, y) = (start[0].as_f64().unwrap(), start[1].as_f64().unwrap());
    assert!((x - 3000.0).abs() < 30.0 && (y - 1200.0).abs() < 30.0, "{start}");
    assert_eq!(h.state().session.history(), ["Wall", "Stair", "Modify", "Modify", "Modify"], "a step for each drag");
}

#[test]
fn the_stair_tool_balances_its_winders_into_the_flights() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    h.state_mut().stair_winders = 3;
    h.state_mut().stair_balance = 2;
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    click(&mut h, P2(3000.0, 3500.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "landings")), (1, json!([{"at": 8.0, "turn": -90.0, "winders": 3, "balance": 2}])));
}

#[test]
fn the_stair_tool_gives_the_run_after_the_turn_the_landings_its_code_asks() {
    let mut h = app(BOX);
    // A code that lets a flight hold five risers.
    h.state_mut().run("building_code", json!({"stairs": {"flight_risers_max": 5}}));
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    click(&mut h, P2(3000.0, 3500.0));
    let st = ids(&h, "stair");
    // 17 risers: the first run stops at 5, where a landing is due; the 12 after it in three flights.
    assert_eq!(field(&h, st[0], "landings"), json!([{"at": 5.0, "turn": -90.0}, {"at": 9.0, "turn": 0.0}, {"at": 13.0, "turn": 0.0}]));
    let found = h.state_mut().run("stair_check", json!({})).unwrap();
    assert_eq!(found["count"], 0, "{found}");
}

#[test]
fn the_stair_tool_turns_by_any_angle_and_chains_run_after_run() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    press(&mut h, "Any Angle");
    // North, then toward the north-east: 45 to the right.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    click(&mut h, P2(3000.0, 4960.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "landings")), (1, json!([{"at": 8.0, "turn": -45.0}])));

    // Chained: three risers north, eleven treads east, then south with the two risers left.
    press(&mut h, "Any Angle");
    press(&mut h, "Chain");
    click(&mut h, P2(4000.0, 500.0));
    click(&mut h, P2(4000.0, 1060.0));
    assert_eq!(ids(&h, "stair").len(), 1, "a landing is due: the tool waits");
    click(&mut h, P2(7580.0, 2060.0));
    assert_eq!(ids(&h, "stair").len(), 1, "the second run stops short of the top");
    click(&mut h, P2(7900.0, 1000.0));
    let all = ids(&h, "stair");
    assert_eq!(all.len(), 2);
    let made = *all.iter().find(|id| **id != st[0]).unwrap();
    assert_eq!(field(&h, made, "landings"), json!([{"at": 3.0, "turn": -90.0}, {"at": 15.0, "turn": -90.0}]));
}

#[test]
fn the_stair_tool_makes_a_spiral_about_a_centre_clicked() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    press(&mut h, "Spiral");
    click(&mut h, P2(4000.0, 3000.0));
    assert!(ids(&h, "stair").is_empty());
    // Its first riser 1500 east of the centre: it winds counter-clockwise, north first.
    click(&mut h, P2(5500.0, 3000.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "spiral"), field(&h, st[0], "angle")), (1, json!({"radius": 1500.0}), json!(90.0)));
    assert_eq!(field(&h, st[0], "start"), json!([5500.0, 3000.0]));
    press(&mut h, "Clockwise");
    click(&mut h, P2(4000.0, 3000.0));
    click(&mut h, P2(4000.0, 4200.0));
    let other = *ids(&h, "stair").iter().find(|id| **id != st[0]).unwrap();
    assert_eq!(
        (field(&h, other, "spiral"), field(&h, other, "angle")),
        (json!({"radius": -1200.0}), json!(0.0)),
        "north of the centre, east first"
    );
}

#[test]
fn the_code_check_window_offers_stairs_that_fit_and_applies_one() {
    let mut h = app(BOX);
    h.state_mut().run("building_code", json!({"set": "nbr"}));
    let id = h.state_mut().run("stair", json!({"start": [1000, 1000], "angle": 90, "riser": 200, "tread": 240})).unwrap()["id"]
        .as_u64()
        .unwrap();
    h.state_mut().code_findings = Some(id);
    h.run();
    assert!(h.query_by_label("Stairs that fit the rules, the nearest first:").is_some());
    h.get_by_label("Apply 1").click_accesskit();
    h.run();
    assert_eq!(field(&h, id, "risers"), json!(17));
    assert_eq!(field(&h, id, "width"), json!(1200.0));
    assert!(h.query_by_label("Building Code Check").is_none(), "inside the rules, the window goes");
}

#[test]
fn the_stair_tool_climbs_to_a_level_chosen_and_stands_on_each_on_the_way() {
    let mut h = app(BOX);
    let l3 = h.state_mut().run("level", json!({"name": "Level 3", "elevation": 6600})).unwrap()["id"].as_u64().unwrap();
    h.run();
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    assert!(h.query_by_label("Multistorey").is_none(), "to the level above, there is one storey");
    h.state_mut().stair_top = Some(Some(l3));
    h.run();
    press(&mut h, "Multistorey");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2960.0));
    click(&mut h, P2(3000.0, 3500.0));
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "multistorey"), field(&h, st[0], "top_level").is_null()), (1, json!(true), false));
    // The stair of its first storey, 3000 up: 17 risers; the storey over it, 3600 up, has 20.
    let storeys = field(&h, st[0], "storeys");
    assert_eq!((storeys[0]["risers"].as_u64(), storeys[1]["risers"].as_u64()), (Some(17), Some(20)));
    assert_eq!(field(&h, st[0], "landings"), json!([{"at": 8.0, "turn": -90.0}]));
}

#[test]
fn the_code_check_window_cuts_the_floor_over_a_stair_open() {
    let mut h = app(BOX);
    h.state_mut().run("floor", json!({"level": "Level 2", "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}));
    let id = h.state_mut().run("stair", json!({"start": [1000, 1000], "angle": 0})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().code_findings = Some(id);
    h.run();
    assert!(h.query_by_label("Minimum Headroom").is_some(), "the floor over it leaves it no headroom");
    let shafts = ids(&h, "shaft").len();
    h.get_by_label("Cut Opening").click_accesskit();
    h.run();
    assert_eq!(ids(&h, "shaft").len(), shafts + 1);
    assert!(h.query_by_label("Building Code Check").is_none(), "with its headroom, it is inside the rules");
}

#[test]
fn the_stair_tool_draws_a_stair_by_hand_and_a_made_stair_becomes_a_sketch() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    press(&mut h, "Sketch");
    // Four risers, each its left end and its right, going north and widening.
    for (i, half) in [500.0, 550.0, 650.0, 800.0].into_iter().enumerate() {
        let y = 1000.0 + 300.0 * i as f64;
        click(&mut h, P2(3000.0 - half, y));
        click(&mut h, P2(3000.0 + half, y));
    }
    assert!(ids(&h, "stair").is_empty(), "Enter makes it");
    h.key_press(Key::Enter);
    h.run();
    let st = ids(&h, "stair");
    assert_eq!(st.len(), 1);
    let sketch = field(&h, st[0], "sketch");
    assert_eq!(sketch.as_array().map(Vec::len), Some(4));
    assert_eq!(field(&h, st[0], "risers"), json!(4));

    // A stair made by runs, converted from the ribbon; then a riser's end dragged.
    press(&mut h, "Sketch");
    let made = h.state_mut().run("stair", json!({"start": [6000, 1000], "angle": 90, "width": 1000})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().tool = Tool::Modify;
    h.run();
    click(&mut h, P2(6000.0, 1420.0));
    assert_eq!(h.state().selection, [made]);
    press(&mut h, "Convert to Sketch");
    assert_eq!(field(&h, made, "sketch").as_array().map(Vec::len), Some(17));
    drag(&mut h, P2(6500.0, 1560.0), P2(7000.0, 1560.0));
    let riser = &field(&h, made, "sketch")[2]["riser"];
    let x = riser[0][0].as_f64().unwrap().max(riser[1][0].as_f64().unwrap());
    assert!((x - 7000.0).abs() < 30.0, "{riser}");
}

#[test]
fn the_building_codes_dialog_writes_lengths_in_the_project_s_units() {
    let mut h = app(BOX);
    h.state_mut().run("units", json!({"length": "m"})).unwrap();
    h.state_mut().run("building_code", json!({"set": "ibc"})).unwrap();
    h.run();
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    // A rule changed: the code's own number beside it, seven inches as the project writes a length.
    h.state_mut().codes_dialog.as_mut().unwrap().rules.insert("riser_max".into(), json!(200.0));
    h.run();
    let code = bimcraft_core::units::Units::Metres.with_unit(177.8);
    assert!(code.ends_with(" m"), "{code}");
    assert!(h.query_by_label(&format!("Changed: the code says {code}")).is_some(), "{code}");
}

#[test]
fn a_stair_s_properties_say_the_location_line_it_has() {
    let mut h = app(BOX);
    let id = h.state_mut().run("stair", json!({"start": [1000, 1000], "angle": 90})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().selection = vec![id];
    h.run();
    assert_eq!(field(&h, id, "line"), json!("centre"), "said, so Properties show it and not the first of the list");
    assert_eq!(field(&h, id, "offset"), json!(0.0));
}
