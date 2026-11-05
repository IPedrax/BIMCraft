//! The owner's Quality of Life list, as a user meets it: what wasn't another test's already.

use super::*;
use egui::accesskit::Role;

/// Two rooms' worth of wall to measure across: `BOX`'s corners are at z 0 and 3000.
fn three_d(script: &str) -> Harness<'static, BimApp> {
    let mut h = app(script);
    press(&mut h, "Default 3D View");
    h
}

#[test]
fn the_finish_of_a_sketch_is_green_and_its_cancel_red_in_every_theme() {
    use crate::style::{AMOLED, DARK, DRACULA, LIGHT, MIDNIGHT};
    for p in [LIGHT, DARK, DRACULA, MIDNIGHT, AMOLED] {
        let (green, red) = (p.context, p.error);
        assert!(green.g() > green.r() && green.g() > green.b(), "{green:?}");
        assert!(red.r() > red.g() && red.r() > red.b(), "{red:?}");
    }
    let mut h = app(BOX);
    h.state_mut().set_tool(Tool::Floor);
    h.run();
    assert!(h.query_by_label("Finish Edit Mode").is_some() && h.query_by_label("Cancel Edit Mode").is_some());
    press(&mut h, "Cancel Edit Mode");
    assert!(h.state().sketch.is_none(), "they still do what they did");
}

#[test]
fn the_ai_button_beside_settings_lets_agents_in_and_shuts_them_out() {
    let mut h = app(BOX);
    assert!(h.state().control.is_none());
    h.get_by_role_and_label(Role::CheckBox, "AI agents").click();
    h.run();
    assert!(h.state().control.is_some() && h.state().agents.listen, "the port is open, and kept");
    h.get_by_role_and_label(Role::CheckBox, "AI agents").click();
    h.run();
    assert!(h.state().control.is_none() && !h.state().agents.listen, "and closed");
}

#[test]
fn the_type_selector_s_list_is_no_wider_than_the_selector() {
    let mut h = app(BOX);
    let wall = find(&h, |e| matches!(e, Element::Wall(_)));
    h.state_mut().selection = vec![wall];
    h.run();
    let selector = h.get_by_label("Type Selector").rect();
    press(&mut h, "Type Selector");
    let names: Vec<String> = h.state().session.model.types.wall.keys().cloned().collect();
    assert!(!names.is_empty());
    for n in &names {
        let row = h.query_all_by_label(n).map(|r| r.rect()).find(|r| r.top() > selector.bottom() - 1.0 || r.bottom() < selector.top() + 1.0);
        let row = row.unwrap_or_else(|| panic!("`{n}` isn't listed"));
        assert!(row.right() <= selector.right() + 0.5, "{n}: {row:?} past the selector's {selector:?}");
    }
}

#[test]
fn a_wall_type_s_sample_shows_each_of_its_layers_by_its_share() {
    let mut h = app(BOX);
    h.state_mut().run(
        "type",
        json!({"category": "wall", "name": "Layered", "thickness": 200.0, "layers": [
            {"function": "finish1", "material": "", "thickness": 50.0},
            {"function": "structure", "material": "", "thickness": 150.0}
        ]}),
    );
    h.run();
    let bands = crate::properties::layer_bands(&h.state().session.model, "Layered");
    assert_eq!(bands.iter().map(|b| b.0).collect::<Vec<_>>(), [0.25, 0.75], "{}", h.state().status.text);
}

#[test]
fn a_line_style_s_list_shows_samples_and_a_row_picks_its_style() {
    let mut h = app(BOX);
    h.state_mut().set_tool(Tool::DetailLine(Vec::new()));
    h.run();
    press(&mut h, "Line Style");
    let other = h.state().session.model.line_styles.keys().find(|n| **n != h.state().line_style).cloned().unwrap();
    press(&mut h, &other);
    assert_eq!(h.state().line_style, other);
}

#[test]
fn the_measure_tool_in_3d_reads_between_two_corners_at_different_heights_whatever_the_work_plane() {
    let mut h = three_d(BOX);
    h.state_mut().set_tool(Tool::Measure(None));
    h.run();
    // From the south wall's outer corner at the ground to the top of the same wall's far end: not on any one level.
    let (a, b): ([f64; 3], [f64; 3]) = ([-100.0, -100.0, 0.0], [8100.0, -100.0, 3000.0]);
    let at = h.state().screen3(a);
    click_at(&mut h, at);
    let first = h.state().orbit.ruler_start.expect("its start");
    let to = h.state().screen3(b);
    click_at(&mut h, to);
    let (p, q) = h.state().orbit.ruler.expect("the ruler");
    assert_eq!(p, first);
    let long = bimcraft_core::dims3d::reading(p, q).0;
    assert!((long - 8200.0f64.hypot(3000.0)).abs() < 1.0, "{long} from {p:?} to {q:?}");
    assert!(h.state().status.text.contains("Measured"), "{}", h.state().status.text);
    // Esc takes the ruler away, then the tool; the camera stayed.
    key(&mut h, Key::Escape);
    assert!(h.state().orbit.ruler.is_none() && matches!(h.state().tool, Tool::Measure(_)));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn over_nothing_the_measure_tool_says_what_it_needs() {
    let mut h = three_d(BOX);
    h.state_mut().set_tool(Tool::Measure(None));
    h.run();
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    assert!(h.state().orbit.ruler_start.is_none());
    assert!(h.state().status.error, "{}", h.state().status.text);
}
