//! The tool palette's info panel for a selection: what it is, its type's total thickness and layers (read only), and Edit Type, which
//! changes them. A tool in hand shows its options there instead.

use super::*;
use crate::settings::Profile;

/// A wall of a compound type (brick, insulation, concrete: 300 in all) from (0,0) to (8000,0).
const CAVITY: &str = concat!(
    r#"type {"category": "wall", "name": "Cavity", "layers": [{"material": "Brick", "thickness": 100, "function": "structure"}, "#,
    r#"{"material": "Insulation", "thickness": 50, "function": "thermal"}, {"material": "Concrete", "thickness": 150, "function": "structure"}]}"#,
    "\n",
    r#"wall {"points": [[0,0],[8000,0]], "type": "Cavity"}"#
);

fn palette_with(script: &str) -> Harness<'static, BimApp> {
    let mut h = app(script);
    h.state_mut().profile = Profile::Palette;
    h.run();
    h
}

/// The column's own `Edit Type`: the Properties palette docked beside it has one too.
fn edit_type_in_column(h: &Harness<BimApp>) -> Vec<egui::Pos2> {
    h.query_all_by_role_and_label(Role::Button, "Edit Type").map(|n| n.rect().center()).filter(|p| p.x < 300.0).collect()
}

#[test]
fn a_selected_wall_shows_its_type_its_thickness_and_layers_and_edit_type_opens_it() {
    let mut h = palette_with(CAVITY);
    assert!(h.query_by_label("Total thickness: 300 mm").is_none(), "nothing selected");
    assert!(edit_type_in_column(&h).is_empty());
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("Wall \u{b7} Cavity").is_some(), "what it is");
    assert!(h.query_by_label("Total thickness: 300 mm").is_some(), "its type's thickness");
    for layer in ["Brick", "Insulation", "Concrete", "100", "50", "150"] {
        assert!(h.query_by_label(layer).is_some(), "{layer}");
    }
    let button = edit_type_in_column(&h);
    assert_eq!(button.len(), 1, "an Edit Type in the column");
    click_at(&mut h, button[0]);
    assert!(h.state().type_dialog.is_some(), "which opens the type's dialog");
}

#[test]
fn a_type_without_layers_shows_its_thickness_only() {
    let mut h = palette_with(BOX);
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("Wall \u{b7} Generic - 200mm").is_some());
    assert!(h.query_by_label("Total thickness: 200 mm").is_some());
    assert_eq!(edit_type_in_column(&h).len(), 1);
}

#[test]
fn several_walls_of_a_type_and_a_mixed_selection_are_said_as_they_are() {
    let mut h = palette_with(BOX);
    let walls: Vec<_> = h.state().session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect();
    h.state_mut().selection = walls;
    h.run();
    assert!(h.query_by_label("Walls (4) \u{b7} Generic - 200mm").is_some());
    let level = find(&h, |e| matches!(e, Element::Level(_)));
    let wall = find(&h, |e| matches!(e, Element::Wall(_)));
    h.state_mut().selection = vec![wall, level];
    h.run();
    // The Properties palette beside the column says it too: the column's is the one that starts left of 300.
    assert_eq!(h.query_all_by_label("Multiple Categories (2)").filter(|n| n.rect().right() <= 300.0).count(), 1);
    assert!(h.query_by_label("Total thickness: 200 mm").is_none());
    assert!(edit_type_in_column(&h).is_empty(), "no one type to edit");
}

#[test]
fn a_tool_in_hand_shows_its_options_and_not_the_selection() {
    let mut h = palette_with(BOX);
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("Total thickness: 200 mm").is_some());
    press(&mut h, "Ceiling");
    assert!(h.query_by_label("Height Offset:").is_some(), "the tool's options");
    assert!(h.query_by_label("Total thickness: 200 mm").is_none(), "and not the wall's");
    key(&mut h, Key::Escape);
}

#[test]
fn with_nothing_to_show_there_is_no_info_strip() {
    let mut h = palette_with(BOX);
    let before = h.get_by_label("Select").rect().top();
    click(&mut h, P2(4000.0, 0.0));
    let during = h.get_by_label("Select").rect().top();
    assert!(during > before + 20.0, "the strip pushes the list down while there is a selection");
    key(&mut h, Key::Escape);
    assert_eq!(h.get_by_label("Select").rect().top(), before, "and takes none when there is nothing selected");
}
