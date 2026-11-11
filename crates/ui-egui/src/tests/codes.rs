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
