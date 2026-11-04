//! Curtain walls in the desktop: the tool, Fixed Grid drawing, the message about a malformed last segment, and Edit Pattern.

use super::*;
use bimcraft_core::model::Wall;

/// The walls, as the model has them.
fn walls(h: &Harness<BimApp>) -> Vec<(Id, Wall)> {
    let m = &h.state().session.model;
    m.elements.iter().filter_map(|(id, e)| if let Element::Wall(w) = e { Some((*id, w.clone())) } else { None }).collect()
}

const FIXED: &str =
    r#"type {"category":"wall","name":"Fixed CW","curtain":{"columns":{"segments":[1000],"fit":"fixed"},"rows":{"segments":[3000]}}}"#;
const LOOSE: &str = r#"type {"category":"wall","name":"Loose CW","curtain":{"columns":{"segments":[1000]},"rows":{"segments":[3000]}}}"#;

fn draw_wall(h: &mut Harness<BimApp>, name: &str, from: P2, to: P2) {
    h.state_mut().types.insert("wall", name.to_string());
    tool(h, "Wall");
    h.state_mut().types.insert("wall", name.to_string());
    click(h, from);
    click(h, to);
}

#[test]
fn the_curtain_wall_tool_draws_a_wall_of_a_curtain_type_on_its_face() {
    let mut h = app("");
    press(&mut h, "Architecture");
    press(&mut h, "Build ▾");
    press(&mut h, "Curtain Wall");
    assert_eq!(h.state().types.get("wall").map(String::as_str), Some("Curtain Wall"));
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(6000.0, 0.0));
    key(&mut h, Key::Escape);
    let w = walls(&h);
    assert_eq!(w.len(), 1, "{}", h.state().status.text);
    assert_eq!(w[0].1.type_name, "Curtain Wall");
    assert_eq!(w[0].1.location, bimcraft_core::model::Location::ExteriorFinish);
    // Back to the plain tool, a plain wall.
    key(&mut h, Key::Escape);
    tool(&mut h, "Wall");
    assert_ne!(h.state().types.get("wall").map(String::as_str), Some("Curtain Wall"));
}

#[test]
fn a_fixed_grid_snaps_the_length_drawn_and_a_chain_goes_on_from_there() {
    let mut h = app(FIXED);
    // The background grid's snapping is off for this drawing, whatever it is set to.
    h.state_mut().grid_snapping = true;
    h.state_mut().types.insert("wall", "Fixed CW".to_string());
    tool(&mut h, "Wall");
    h.state_mut().types.insert("wall", "Fixed CW".to_string());
    assert!(h.state().drawing_fixed_curtain().is_some());
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(3300.0, 0.0));
    click(&mut h, P2(3000.0, 2200.0));
    let w = walls(&h);
    assert_eq!(w.len(), 2, "{}", h.state().status.text);
    assert_eq!(w[0].1.end, P2(3000.0, 0.0));
    assert_eq!(w[1].1.start, P2(3000.0, 0.0), "the chain goes on from the end the wall got");
    assert_eq!(w[1].1.end, P2(3000.0, 2000.0));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().drawing_fixed_curtain(), None);
}

#[test]
fn a_malformed_last_segment_asks_and_removing_the_grid_keeps_the_length() {
    let mut h = app(LOOSE);
    draw_wall(&mut h, "Loose CW", P2(0.0, 0.0), P2(3100.0, 0.0));
    let message = bimcraft_core::curtain::MALFORMED;
    assert!(h.query_by_label(message).is_some(), "{}", h.state().status.text);
    press(&mut h, "Remove last interior grid");
    assert!(h.query_by_label(message).is_none());
    key(&mut h, Key::Escape);
    let (_, w) = walls(&h).remove(0);
    assert!(w.curtain.as_ref().is_some_and(|c| c.absorb_columns));
    assert_eq!(w.end, P2(3100.0, 0.0), "the wall keeps its length");
    assert!(bimcraft_core::curtain::malformed_walls(&h.state().session.model).is_empty());
}

#[test]
fn i_will_fix_it_myself_leaves_the_wall_and_doesnt_ask_again_for_it() {
    let mut h = app(LOOSE);
    draw_wall(&mut h, "Loose CW", P2(0.0, 0.0), P2(3100.0, 0.0));
    let message = bimcraft_core::curtain::MALFORMED;
    press(&mut h, "I will fix it myself");
    assert!(h.query_by_label(message).is_none());
    assert_eq!(walls(&h)[0].1.curtain, None);
    key(&mut h, Key::Escape);
    // Another command: the same wall isn't asked about again; a new malformed one is.
    h.state_mut().run("level", serde_json::json!({"elevation": 9000}));
    h.run();
    assert!(h.query_by_label(message).is_none());
    draw_wall(&mut h, "Loose CW", P2(0.0, 2000.0), P2(3100.0, 2000.0));
    assert!(h.query_by_label(message).is_some());
    press(&mut h, "I will fix it myself");
}

#[test]
fn a_wall_that_fits_asks_nothing_and_opening_a_file_doesnt_ask() {
    let mut h = app(&format!("{LOOSE}\nwall {{\"points\": [[0, 0], [3100, 0]], \"type\": \"Loose CW\", \"height\": 3000}}"));
    assert!(h.query_by_label(bimcraft_core::curtain::MALFORMED).is_none(), "already so when it opened");
    draw_wall(&mut h, "Loose CW", P2(0.0, 2000.0), P2(3000.0, 2000.0));
    assert!(h.query_by_label(bimcraft_core::curtain::MALFORMED).is_none());
}

#[test]
fn edit_pattern_adds_segments_and_sets_each_panel_and_mullion() {
    let mut h = app(LOOSE);
    h.state_mut().open_curtain_editor("Loose CW");
    h.run();
    press(&mut h, "Add Column");
    type_into(&mut h, "Column B", "650");
    press(&mut h, "Panel B1");
    choose(&mut h, "Panel Type", "Spandrel");
    press(&mut h, "Vertical B1");
    choose(&mut h, "Mullion Type", "None");
    press(&mut h, "Horizontal A1");
    choose(&mut h, "Join", "Mitre");
    choose(&mut h, "Column Fit", "Fixed Grid");
    choose(&mut h, "Column Justification", "Centre");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let l = m.types.wall["Loose CW"].curtain.as_ref().unwrap();
    assert_eq!(l.columns.segments, [1000.0, 650.0]);
    assert_eq!(l.panels[0][0].as_deref(), Some("Glazed"));
    assert_eq!(l.panels[0][1].as_deref(), Some("Spandrel"));
    assert_eq!(l.vertical[0][1], None);
    assert!(l.vertical[0][0].is_some());
    assert_eq!(l.horizontal[0][0].as_ref().unwrap().join, bimcraft_core::curtain::Join::Mitre);
    assert_eq!(l.columns.fit, bimcraft_core::curtain::Fit::Fixed);
    assert_eq!(l.columns.justify, bimcraft_core::curtain::Justify::Centre);
    assert!(h.state().curtain_editor.is_none());
}

#[test]
fn cancel_leaves_the_pattern_as_it_was() {
    let mut h = app(LOOSE);
    let before = h.state().session.model.types.wall["Loose CW"].clone();
    h.state_mut().open_curtain_editor("Loose CW");
    h.run();
    press(&mut h, "Add Row");
    press(&mut h, "Cancel");
    assert_eq!(h.state().session.model.types.wall["Loose CW"], before);
}

#[test]
fn type_properties_of_a_curtain_wall_opens_edit_pattern_and_shows_a_thickness_it_cant_type() {
    let mut h = app(LOOSE);
    h.state_mut().type_dialog = Some(crate::properties::TypeDialog::new(&h.state().session.model, "wall", "Loose CW".into(), Vec::new()));
    h.run();
    assert!(h.query_by_label("Up").is_none());
    press(&mut h, "Edit…");
    assert!(h.state().curtain_editor.is_some());
    // Over Type Properties, its own Cancel is the later one.
    h.get_all_by_label("Cancel").last().unwrap().click();
    h.run();
    assert!(h.state().curtain_editor.is_none() && h.state().type_dialog.is_some());
    // Duplicate keeps the pattern.
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Copy CW");
    press(&mut h, "Create");
    let m = &h.state().session.model;
    assert_eq!(m.types.wall.get("Copy CW").and_then(|t| t.curtain.clone()), m.types.wall["Loose CW"].curtain);
}

#[test]
fn the_browser_edits_a_panel_and_a_mullions_type() {
    let mut h = app("");
    let mullion = crate::properties::TypeDialog::new(&h.state().session.model, "mullion", "50 x 150".into(), Vec::new());
    h.state_mut().type_dialog = Some(mullion);
    h.run();
    type_into(&mut h, "Y Offset", "20");
    type_into(&mut h, "Width", "60");
    press(&mut h, "OK");
    let t = &h.state().session.model.types.mullion["50 x 150"];
    assert_eq!((t.width(), t.offset_y), (60.0, 20.0), "{}", h.state().status.text);
}

#[test]
fn a_curtain_wall_shows_no_wall_layers_a_plain_one_does_in_both_profiles() {
    for rev in [false, true] {
        let script = "wall {\"points\":[[0,0],[6000,0]],\"type\":\"Curtain Wall\"}\nwall {\"points\":[[0,3000],[6000,3000]]}";
        let mut h = if rev { app(script) } else { bimcraft(script) };
        let w = walls(&h);
        let (curtain, plain) =
            (w.iter().find(|x| x.1.type_name == "Curtain Wall").unwrap().0, w.iter().find(|x| x.1.type_name != "Curtain Wall").unwrap().0);
        h.state_mut().selection = vec![plain];
        h.run();
        assert!(h.query_by_label("Wall layers").is_some(), "rev {rev}: a plain wall has them");
        h.state_mut().selection = vec![curtain];
        h.run();
        assert!(h.query_by_label("Wall layers").is_none(), "rev {rev}: not a curtain wall");
    }
}
