//! Manage > Building Codes: the code stairs and railings are held to, and the rules the project changes of it.

use super::*;
use bimcraft_core::codes::Discipline;

#[test]
fn building_codes_choose_a_code_and_change_its_rules() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    assert!(h.state().codes_dialog.is_some(), "the button opens the dialog");

    // The IBC, with a width of the project's own.
    {
        let m = h.state().session.model.clone();
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.tab_mut(Discipline::Architectural).set(&m, "ibc");
        d.stairs_mut().insert("width_min".into(), serde_json::json!(914.4));
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
    h.state_mut().codes_dialog.as_mut().unwrap().stairs_mut().insert("riser_min".into(), serde_json::json!(500));
    press(&mut h, "OK");
    assert!(h.state().codes_dialog.is_some());
    press(&mut h, "Cancel");
    assert!(h.state().codes_dialog.is_none());
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
        let m = h.state().session.model.clone();
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.tab_mut(Discipline::Architectural).set(&m, "nbr-9050");
    }
    h.run();
    assert!(h.query_by_label("Custom").is_none(), "the code's own rules");
    h.state_mut().codes_dialog.as_mut().unwrap().stairs_mut().insert("width_min".into(), json!(1100.0));
    h.run();
    assert!(h.query_by_label("Custom").is_some(), "a rule changed");
    h.state_mut().codes_dialog.as_mut().unwrap().name = "City Hall".into();
    h.run();
    press(&mut h, "Save Code");
    let m = &h.state().session.model;
    assert_eq!(
        (m.building_code().code.as_str(), m.building_code().based_on.as_str(), m.stair_rules().width_min),
        ("City Hall", "nbr-9050", 1100.0)
    );
    assert_eq!(h.state_mut().codes_dialog.as_mut().unwrap().tab_mut(Discipline::Architectural).code, "City Hall");
    assert!(h.query_by_label("Custom").is_none(), "saved, it is a code like the others");
    // A name BIMCraft has is refused inside the dialog.
    h.state_mut().codes_dialog.as_mut().unwrap().stairs_mut().insert("width_min".into(), json!(1000.0));
    h.state_mut().codes_dialog.as_mut().unwrap().name = "ibc".into();
    h.run();
    press(&mut h, "Save Code");
    assert!(h.state().codes_dialog.as_ref().unwrap().error.contains("another name"));
    press(&mut h, "Undo Changes in This Tab");
    press(&mut h, "Delete");
    let m = &h.state().session.model;
    assert!(m.codes.own.is_empty());
    assert_eq!((m.building_code().code.as_str(), m.stair_rules().width_min), ("nbr-9050", 1100.0), "its rules stay, as the project's own");
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
    h.get_by_label("Add Landings Due").click_accesskit();
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
    press(&mut h, "By Risers");
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
    press(&mut h, "Straight");
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
    // A rule changed: the code's own number beside it, seven inches as the project writes a length, on the row of the
    // riser's least and most.
    h.state_mut().codes_dialog.as_mut().unwrap().stairs_mut().insert("riser_max".into(), json!(200.0));
    h.run();
    let code = bimcraft_core::units::Units::Metres.with_unit(177.8);
    assert!(code.ends_with(" m"), "{code}");
    assert!(h.query_by_label(&format!("Changed. The code says max. {code}")).is_some(), "{code}");
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

#[test]
fn the_stair_tool_draws_a_stair_along_the_line_clicked_flights_and_landings() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    // A flight north; a landing east, 2500 long; a flight south: two flights standing 2500 apart.
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2400.0));
    assert!(ids(&h, "stair").is_empty(), "short of its top, it goes on");
    press(&mut h, "Landing");
    click(&mut h, P2(3500.0, 2400.0));
    press(&mut h, "Steps");
    click(&mut h, P2(3500.0, 400.0));
    assert!(ids(&h, "stair").is_empty(), "still short of its top");
    // Enter finishes it as drawn: what was clicked stays, and its top is under Level 2 by the risers not drawn.
    h.key_press(Key::Enter);
    h.run();
    let st = ids(&h, "stair");
    assert_eq!(st.len(), 1);
    assert_eq!(
        field(&h, st[0], "path"),
        json!([{"to": [1000.0, 2400.0]}, {"to": [3500.0, 2400.0], "landing": true}, {"to": [3500.0, 400.0]}])
    );
    assert_eq!((field(&h, st[0], "risers"), field(&h, st[0], "landings")), (json!(10), Value::Null));
    let top = field(&h, st[0], "top_offset").as_f64().unwrap();
    assert!((top - 7.0 * 3000.0 / 17.0).abs() < 1e-6, "{top}: below Level 2 by the risers not drawn");
    let m = &h.state().session.model;
    let Some(Element::Stair(made)) = m.elements.get(st[0]) else { panic!("no stair") };
    let l = bimcraft_core::stairs::stair(m, made);
    assert_eq!((l.runs.len(), l.pads.len(), l.runs.iter().map(|r| r.risers).sum::<u32>()), (2, 1, 10));
    assert_eq!(h.state().tool, Tool::Stair(Vec::new()), "ready for the next one");
}

#[test]
fn the_stair_tool_s_corner_is_a_landing_or_winders_and_a_long_flight_finishes_it() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    h.state_mut().stair_winders = 3;
    h.state_mut().stair_line = bimcraft_core::model::StairLine::Left;
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2400.0));
    assert!(ids(&h, "stair").is_empty());
    // East, far past its top: the stair is done by that click.
    click(&mut h, P2(7000.0, 2400.0));
    let st = ids(&h, "stair");
    assert_eq!(st.len(), 1);
    assert_eq!(field(&h, st[0], "path"), json!([{"to": [1000.0, 2400.0]}, {"to": [7000.0, 2400.0], "winders": 3}]));
    assert_eq!((field(&h, st[0], "line"), field(&h, st[0], "start")), (json!("left"), json!([1000.0, 1000.0])));
    let m = &h.state().session.model;
    let Some(Element::Stair(made)) = m.elements.get(st[0]) else { panic!("no stair") };
    assert_eq!(bimcraft_core::stairs::stair(m, made).winders, 3);
}

#[test]
fn the_stair_tool_draws_an_arc_that_leaves_the_stretch_before_it_smoothly() {
    let mut h = app(BOX);
    tool(&mut h, "Stair");
    press(&mut h, "Railings");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(1000.0, 2000.0));
    press(&mut h, "Arc");
    // From going north to a point up and to the right: a quarter turn to the right.
    click(&mut h, P2(2500.0, 3500.0));
    h.key_press(Key::Enter);
    h.run();
    let st = ids(&h, "stair");
    assert_eq!(st.len(), 1);
    let path = field(&h, st[0], "path");
    assert!((path[1]["arc"].as_f64().unwrap() + 90.0).abs() < 1.0, "{path}");
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
    press(&mut h, "Clockwise");
    click(&mut h, P2(4000.0, 3000.0));
    click(&mut h, P2(4000.0, 4200.0));
    let other = *ids(&h, "stair").iter().find(|id| **id != st[0]).unwrap();
    assert_eq!((field(&h, other, "spiral"), field(&h, other, "angle")), (json!({"radius": -1200.0}), json!(0.0)));
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
    h.key_press(Key::Enter);
    h.run();
    let st = ids(&h, "stair");
    assert_eq!((st.len(), field(&h, st[0], "multistorey"), field(&h, st[0], "top_level").is_null()), (1, json!(true), false));
    // The stair of its first storey, 3000 up: 17 risers; the storey over it, 3600 up, has 20.
    let storeys = field(&h, st[0], "storeys");
    assert_eq!((storeys[0]["risers"].as_u64(), storeys[1]["risers"].as_u64()), (Some(17), Some(20)));
}

#[test]
fn each_discipline_has_its_tab_its_sections_and_a_search() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    for tab in ["Overview", "Architectural", "Structural", "Electrical", "Mechanical", "Plumbing"] {
        assert!(h.query_all_by_label(tab).count() >= 1, "{tab}");
    }
    // The architectural tab first, its sections listed.
    assert!(h.query_all_by_label("Stairs and Railings").count() >= 1);
    assert!(h.query_all_by_label("Ramps").count() >= 1);
    // A search keeps the sections that have such a rule.
    h.state_mut().codes_dialog.as_mut().unwrap().search = "headroom".into();
    h.run();
    assert!(h.query_all_by_label("Stairs and Railings").count() >= 1);
    assert_eq!(h.query_all_by_label("Ramps").count(), 0, "no ramp rule is called that");
    h.state_mut().codes_dialog.as_mut().unwrap().search.clear();
    h.state_mut().codes_dialog.as_mut().unwrap().tab = 5;
    h.run();
    assert!(h.query_all_by_label("Fixtures").count() >= 1 && h.query_all_by_label("Storm Drainage").count() >= 1);
    press(&mut h, "Cold Water");
    assert!(h.query_all_by_label("Maximum Speed (m/s)").count() >= 1, "the section's rules, by name");

    // A plumbing rule of the project's own, and an amendment: OK makes them the project's.
    {
        let m = h.state().session.model.clone();
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        let tab = d.tab_mut(Discipline::Plumbing);
        tab.set(&m, "nbr");
        tab.amendments.push("cbpmesp-it22".into());
        tab.rules.get_mut("cold_water").unwrap().insert("max_speed".into(), json!(2.0));
    }
    h.run();
    assert!(h.query_by_label("CBPMESP IT 22 (hydrants and hose reels, São Paulo)").is_some(), "the amendment's chip");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert_eq!(m.rules::<bimcraft_core::water::WaterRules>().max_speed, 2.0);
    assert_eq!(m.codes.plumbing.amendments, ["cbpmesp-it22"]);
    assert_eq!(m.building_code().code, "generic", "the other tabs as they were");
}

#[test]
fn the_overview_picks_a_region_and_keeps_the_site_s_numbers() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    press(&mut h, "Overview");
    assert!(h.query_by_label("Site and Climate").is_some());
    // A new project's codes are the "no building code" preset's; the drop-down says the region picked, not "Pick a region…".
    let brasil = bimcraft_core::codes::presets().iter().position(|p| p.name == "Brasil");
    assert_ne!(h.state().codes_dialog.as_ref().unwrap().region(), brasil);
    press(&mut h, "Region Preset");
    press(&mut h, "Brasil");
    assert_eq!(h.state().codes_dialog.as_ref().unwrap().region(), brasil);
    assert!(h.query_all_by_label("Pick a region…").next().is_none());
    {
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.site.insert("rain_intensity".into(), json!(172.0));
        d.site.insert("sources".into(), json!({"rain_intensity": "NBR 10844 Tabela 5"}));
    }
    h.run();
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert_eq!(m.building_code().code, "nbr-9050");
    assert_eq!(m.codes.base(Discipline::Plumbing).code, "nbr");
    assert_eq!(m.codes.plumbing.amendments, ["cbpmesp-it22"]);
    assert_eq!(m.codes.site.rain_intensity, Some(172.0));
    assert_eq!(m.codes.site.sources["rain_intensity"], "NBR 10844 Tabela 5");
}

/// The kinds of room are rows: a name, the room names that make one, the least area, width and height, and natural light.
#[test]
fn the_kinds_of_room_are_rows_with_their_area_height_and_natural_light() {
    let mut h = app(BOX);
    h.state_mut().run("building_code", json!({"set": "ibc"}));
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    h.state_mut().codes_dialog.as_mut().unwrap().tab_mut(Discipline::Architectural).section = "rooms".into();
    h.run();
    assert!(h.query_by_label("Kind 4 name").is_some(), "the IBC's four kinds, a row each");
    press(&mut h, "Add Row");
    type_into(&mut h, "Kind 5 name", "office");
    type_into(&mut h, "Kind 5 room names", "office, study room");
    type_role(&mut h, Role::SpinButton, "Kind 5 least area", "9");
    type_role(&mut h, Role::SpinButton, "Kind 5 natural light", "12.5");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let rules = m.rules::<bimcraft_core::codes::checks::RoomRules>();
    let k = rules.kinds.iter().find(|k| k.kind == "office").expect("the new kind");
    assert_eq!((k.names.clone(), k.area_min, k.light_min), (vec!["office".to_string(), "study room".to_string()], 9.0, 0.125));
    assert_eq!(rules.kinds.iter().find(|k| k.kind == "habitable").map(|k| k.light_min), Some(0.08), "the IBC's 8 %");
}

/// Amendments are ticked from a list, put in order (the lower wins), and a rule changed goes into the one edited; a new
/// one of the project's own is made empty over the code.
#[test]
fn amendments_are_chosen_ordered_and_hold_the_rules_changed() {
    let mut h = app(BOX);
    h.state_mut().run("building_code", json!({"discipline": "plumbing", "set": "nbr"}));
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    h.state_mut().codes_dialog.as_mut().unwrap().tab = 5;
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Choose Amendments").click_accesskit();
    h.run();
    h.get_all_by(|n| n.role() == Role::CheckBox && n.label().is_some_and(|l| l.starts_with("CBPMESP IT 22")))
        .next()
        .unwrap()
        .click_accesskit();
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Choose Amendments").click_accesskit();
    h.run();
    assert_eq!(h.state().codes_dialog.as_ref().unwrap().tabs[4].target.as_deref(), Some("cbpmesp-it22"), "the one edited");
    // A new amendment of the project's own, last, and so the one edited.
    type_into(&mut h, "New amendment:", "Our city");
    press(&mut h, "Create Amendment");
    let tab = h.state().codes_dialog.as_ref().unwrap().tabs[4].clone();
    assert_eq!(
        (tab.amendments.clone(), tab.target.clone()),
        (vec!["cbpmesp-it22".to_string(), "Our city".to_string()], Some("Our city".into()))
    );
    // Put first: IT 22 is laid over it now.
    h.get_all_by_label("↑").last().unwrap().click_accesskit();
    h.run();
    assert_eq!(h.state().codes_dialog.as_ref().unwrap().tabs[4].amendments, ["Our city", "cbpmesp-it22"]);
    {
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.tabs[4].rules.get_mut("cold_water").unwrap().insert("max_speed".into(), json!(2.5));
    }
    h.run();
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert_eq!(m.codes.plumbing.amendments, ["Our city", "cbpmesp-it22"]);
    assert!(m.codes.plumbing.changes.is_empty(), "not the project's own: the amendment's");
    let ours = m.codes.own.iter().find(|c| c.code == "Our city").unwrap();
    assert_eq!((ours.amends.as_str(), ours.sections["cold_water"]["max_speed"].clone()), ("nbr", json!(2.5)));
    assert_eq!(m.rules::<bimcraft_core::water::WaterRules>().max_speed, 2.5);

    // Taken out of the amendment again: the code's number.
    press(&mut h, "Building Codes");
    {
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.tab = 5;
        d.tabs[4].target = Some("Our city".into());
        d.tabs[4].section = "cold_water".into();
    }
    h.run();
    press(&mut h, "Remove");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert_ne!(m.rules::<bimcraft_core::water::WaterRules>().max_speed, 2.5);
    assert!(m.codes.own.iter().find(|c| c.code == "Our city").unwrap().sections.is_empty());
}

#[test]
fn one_tab_goes_out_as_a_file_and_comes_into_another_project_alone() {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-codes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plumbing.xml");
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    let tabs = {
        let m = h.state().session.model.clone();
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        let tab = d.tab_mut(Discipline::Plumbing);
        tab.set(&m, "nbr");
        tab.rules.get_mut("drainage").unwrap().insert("main_min_dn".into(), json!(150.0));
        vec![tab.clone()]
    };
    h.state_mut().export_code(&path, &tabs, Some(Discipline::Plumbing), "Our plumbing");
    assert!(h.state().codes_dialog.as_ref().unwrap().error.is_empty(), "{}", h.state().codes_dialog.as_ref().unwrap().error);
    assert!(h.state().session.model.codes.is_empty(), "exported from a copy: the project is as it was");

    let mut other = app(BOX);
    other.state_mut().run("building_code", json!({"set": "din"})).unwrap();
    other.state_mut().import_code(&path, false);
    other.run();
    let d = other.state_mut().codes_dialog.as_mut().unwrap();
    assert_eq!(d.tab, 5, "the Plumbing tab shows what came in");
    assert_eq!(d.tab_mut(Discipline::Plumbing).code, "Our plumbing");
    assert_eq!(d.tab_mut(Discipline::Architectural).code, "din", "the other tabs as they were");
    press(&mut other, "OK");
    let m = &other.state().session.model;
    assert_eq!(m.rules::<bimcraft_core::drainage::DrainageRules>().main_min_dn, 150.0);
    assert_eq!(m.building_code().code, "din");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_table_is_edited_and_rows_are_pasted_from_a_spreadsheet() {
    let rows = json!([[20.0, 1.5], [25.0, 1.7]]);
    let text = crate::codes::as_tsv(&rows);
    assert_eq!(text, "20.0\t1.5\n25.0\t1.7\n");
    assert_eq!(crate::codes::from_tsv("20\t1.5\n25\t1.9\n32\t2.1", &rows), Some(json!([[20, 1.5], [25, 1.9], [32, 2.1]])));
    let objects = json!([{"kind": "wc", "uhc": 6.0}]);
    let text = crate::codes::as_tsv(&objects);
    assert_eq!(text, "kind\tuhc\nwc\t6.0\n");
    assert_eq!(
        crate::codes::from_tsv("kind\tuhc\nwc\t6\nlav\t1", &objects),
        Some(json!([{"kind": "wc", "uhc": 6}, {"kind": "lav", "uhc": 1}]))
    );

    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    h.state_mut().codes_dialog.as_mut().unwrap().tab = 5;
    h.run();
    press(&mut h, "Cold Water");
    {
        let d = h.state_mut().codes_dialog.as_mut().unwrap();
        d.table = Some(("cold_water".into(), "pipes".into()));
        d.pasted = "20\t1.5\n25\t1.7\n32\t2.1".into();
    }
    h.run();
    // Below the rules, where the dialog scrolls: clicked as an assistive tool clicks it.
    h.get_by_label("Use Pasted Rows").click_accesskit();
    h.run();
    let d = h.state_mut().codes_dialog.as_mut().unwrap();
    assert_eq!(d.tab_mut(Discipline::Plumbing).rules["cold_water"]["pipes"], json!([[20, 1.5], [25, 1.7], [32, 2.1]]));
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert_eq!(m.rules::<bimcraft_core::water::WaterRules>().pipes.len(), 3, "three pipes, as pasted");
}

#[test]
fn a_door_outside_its_code_lists_the_rules_in_the_popup_too() {
    let mut h = app(BOX);
    h.state_mut().run("building_code", json!({"set": "ibc"}));
    h.state_mut().run("wall", json!({"points": [[0, 0], [6000, 0]]}));
    h.state_mut().run("type", json!({"category": "door", "name": "Narrow 700", "width": 700, "height": 2000}));
    let id = h.state_mut().run("door", json!({"at": [2000, 0], "type": "Narrow 700"})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().selection = vec![id];
    h.run();
    h.get_by_label("Outside 2 rules").click_accesskit();
    h.run();
    assert!(h.query_by_label("Building Code Check").is_some());
    assert_eq!(h.query_all_by_label("The rule:").count(), 2);
    assert!(h.query_by_label("The door is too narrow to pass through. Use a wider door type.").is_some(), "advice for a door");
}

/// What Manage > Building Codes looks like, for a person to look at: `BIMCRAFT_CODES_DUMP=<folder>` writes the architectural
/// tab with a rule changed and its card, the plumbing tab with a table open, and the Overview, light and dark.
#[test]
fn building_codes_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_CODES_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let save = |h: &mut Harness<BimApp>, name: &str| {
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run_steps(8);
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("codes-{name}-{look}.png"))).unwrap();
        };
        let mut h = app(BOX);
        h.state_mut().run("building_code", json!({"set": "nbr-9050", "stairs": {"width_min": 1100}}));
        press(&mut h, "Manage");
        press(&mut h, "Building Codes");
        h.set_size(egui::vec2(1400.0, 1000.0));
        h.run_steps(8);
        h.state_mut().codes_dialog.as_mut().unwrap().card = Some(("stairs".into(), "riser_min".into()));
        save(&mut h, "architectural");
        {
            let m = h.state().session.model.clone();
            let d = h.state_mut().codes_dialog.as_mut().unwrap();
            d.tab = 5;
            let tab = d.tab_mut(Discipline::Plumbing);
            tab.set(&m, "nbr");
            tab.amend(&m, vec!["cbpmesp-it22".into()]);
            tab.section = "cold_water".into();
            d.table = Some(("cold_water".into(), "equivalent_lengths".into()));
            d.card = Some(("cold_water".into(), "max_speed".into()));
        }
        save(&mut h, "plumbing");
        {
            let m = h.state().session.model.clone();
            let d = h.state_mut().codes_dialog.as_mut().unwrap();
            d.tab = 1;
            let tab = d.tab_mut(Discipline::Architectural);
            tab.set(&m, "ibc");
            tab.section = "rooms".into();
            d.card = None;
            d.table = None;
        }
        save(&mut h, "rooms");
        {
            let m = h.state().session.model.clone();
            let d = h.state_mut().codes_dialog.as_mut().unwrap();
            d.tab = 3;
            let tab = d.tab_mut(Discipline::Electrical);
            tab.set(&m, "nbr-5410");
            tab.amend(&m, vec!["fecoergs".into(), "cemig".into()]);
            tab.section = "building_demand".into();
        }
        save(&mut h, "electrical");
        h.state_mut().codes_dialog.as_mut().unwrap().tab_mut(Discipline::Electrical).section = "@systems".into();
        save(&mut h, "electrical-settings");
        h.state_mut().codes_dialog.as_mut().unwrap().tab = 0;
        save(&mut h, "overview");
    }
}

#[test]
fn a_section_shows_its_words_as_text_and_its_tables_as_tables() {
    let mut h = app(BOX);
    h.state_mut().run("building_code", json!({"discipline": "electrical", "set": "nbr-5410", "amendments": ["copel"]})).unwrap();
    press(&mut h, "Manage");
    press(&mut h, "Building Codes");
    h.state_mut().codes_dialog.as_mut().unwrap().tab = 3;
    h.run();
    press(&mut h, "Service Categories");
    // Who the section is and its source are the code's, beside each rule: not rules of their own.
    for word in ["Standard", "Id", "Not covered"] {
        assert!(h.query_by_label(word).is_none(), "{word} is no rule");
    }
    assert_eq!(h.query_all_by_label("Source").count(), 1, "Source is the column of each rule's source, no rule");
    assert!(h.query_by_label("How it works:").is_some() && h.query_by_label("Not covered:").is_some());
    // A table of tables: a card for each supply system and use, its rows a table with a column for each of the utility's.
    h.state_mut().codes_dialog.as_mut().unwrap().table = Some(("entrance".into(), "tables".into()));
    h.run();
    assert!(h.query_by_label("127/220 · individual").is_some());
    assert!(h.query_all_by_label("Breaker (A)").count() >= 1, "a column named in words, with its unit");
    assert!(h.query_by_label("Delete this row").is_some() || h.query_all_by_label("×").count() > 0);
}

#[test]
fn keys_are_said_in_words_with_their_units() {
    use crate::codes::tree::human;
    assert_eq!(human("breaker_a"), "Breaker (A)");
    assert_eq!(human("connection_mm2"), "Connection (mm²)");
    assert_eq!(human("up_to"), "Up to");
    assert_eq!(human("installed_kw"), "Installed (kW)");
}
