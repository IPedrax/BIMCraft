//! Dimensions placed as a user places them: Linear between two endpoints, horizontal or vertical as the cursor lies, or as
//! the Options Bar or Space says; Aligned between endpoints; Diameter and Arc Length on what is round; and Edit Type on a
//! dimension, which opens Dimension Type Properties.

use super::*;

fn dimension(h: &Harness<BimApp>, id: Id) -> bimcraft_core::model::Dimension {
    match h.state().session.model.elements.get(id) {
        Some(Element::Dimension(d)) => d.clone(),
        e => panic!("{id} is {e:?}"),
    }
}

#[test]
fn linear_dimensions_pick_two_endpoints_and_run_as_the_cursor_lies_or_the_options_bar_says() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Linear");
    assert_eq!(h.state().tool, Tool::Dimension(DimMode::Auto, Vec::new()));
    // The south-west corner's outer face end, and the north-east's: endpoints, not the walls' lines.
    click(&mut h, P2(-95.0, -95.0));
    click(&mut h, P2(8095.0, 6095.0));
    assert!(matches!(&h.state().tool, Tool::Dimension(_, refs) if refs.len() == 2 && refs.iter().all(|r| r.point.is_some())));
    // Below them: horizontal.
    click(&mut h, P2(4000.0, -1000.0));
    let dims = ids(&h, "dimension");
    assert_eq!(field(&h, dims[0], "values"), json!([8200.0]));
    assert_eq!(dimension(&h, dims[0]).orient, Some(bimcraft_core::model::Orient::Horizontal));
    // East of them: vertical.
    click(&mut h, P2(-95.0, -95.0));
    click(&mut h, P2(8095.0, 6095.0));
    click(&mut h, P2(8700.0, 3000.0));
    let dims = ids(&h, "dimension");
    assert_eq!(field(&h, dims[1], "values"), json!([6200.0]));
    // Space steps to Aligned, though the cursor is below.
    key(&mut h, Key::Space);
    let aligned = h.state().tool.clone();
    assert!(matches!(aligned, Tool::Dimension(DimMode::Aligned, _)), "{aligned:?}");
    click(&mut h, P2(-95.0, -95.0));
    click(&mut h, P2(8095.0, 6095.0));
    click(&mut h, P2(4000.0, -1000.0));
    let dims = ids(&h, "dimension");
    let v = field(&h, dims[2], "values")[0].as_f64().unwrap_or_default();
    assert!((v - 8200f64.hypot(6200.0)).abs() < 0.01, "{v}");
    // On to Horizontal, then Vertical.
    key(&mut h, Key::Space);
    assert_eq!(h.state().tool, Tool::Dimension(DimMode::Horizontal, Vec::new()));
    key(&mut h, Key::Space);
    assert_eq!(h.state().tool, Tool::Dimension(DimMode::Vertical, Vec::new()));
    click(&mut h, P2(-95.0, -95.0));
    click(&mut h, P2(8095.0, 6095.0));
    click(&mut h, P2(4000.0, -1000.0));
    let dims = ids(&h, "dimension");
    assert_eq!(field(&h, dims[3], "values"), json!([6200.0]));
}

#[test]
fn diameter_and_arc_length_dimensions_pick_what_is_round_then_where_they_go() {
    let script = format!(
        "{BOX}\ncolumn {{\"at\": [2000, 3000], \"type\": \"Round 300\"}}\nwall {{\"points\": [[8000, 6000], [14000, 6000]], \"arc\": -120}}"
    );
    let mut h = app(&script);
    press(&mut h, "Annotate");
    press(&mut h, "Diameter");
    click(&mut h, P2(2150.0, 3000.0));
    click(&mut h, P2(2600.0, 3600.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[0], "values")), (1, json!([300.0])));
    assert_eq!(field(&h, dims[0], "labels"), json!(["Ø 300"]));
    // What the panel has no room for, its list under its name has.
    press(&mut h, "Annotate");
    if h.query_by_label("Arc Length").is_none() {
        press(&mut h, "Dimension ▾");
    }
    press(&mut h, "Arc Length");
    // The top of the curved wall: it turns 120° clockwise, so bulges north.
    let wall = ids(&h, "wall").into_iter().max().unwrap_or_default();
    let mid = match h.state().session.model.elements.get(wall) {
        Some(Element::Wall(w)) => {
            let axis = bimcraft_core::walls::Axis::of(w);
            axis.at(axis.length() / 2.0, 0.0)
        }
        _ => P2(0.0, 0.0),
    };
    click(&mut h, mid);
    // Inside the arc: its line round the same centre, nearer it.
    click(&mut h, mid - P2(0.0, 800.0));
    let dims = ids(&h, "dimension");
    assert_eq!(dimension(&h, dims[1]).kind, DimKind::ArcLength);
    let v = field(&h, dims[1], "values")[0].as_f64().unwrap_or_default();
    let r = 3000.0 / 60f64.to_radians().sin();
    assert!((v - r * 120f64.to_radians()).abs() < 1.0, "{v}");
}

#[test]
fn edit_type_on_a_dimension_opens_dimension_type_properties_where_a_duplicate_is_made_and_the_dimension_put_on_it() {
    let script = format!("{BOX}\ndimension {{\"refs\": [[100, 3000], [7900, 3000]], \"at\": [4000, 3000]}}");
    let mut h = app(&script);
    let id = ids(&h, "dimension")[0];
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Edit Type");
    assert!(h.state().dimension_type_dialog.is_some());
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Dots");
    press(&mut h, "Create");
    assert!(h.state().dimension_type_dialog.as_ref().is_some_and(|d| d.error().is_none()));
    pick(&mut h, Role::CheckBox, "Center Marks");
    press(&mut h, "OK");
    assert!(h.state().dimension_type_dialog.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    assert!(!m.dimension_types["Dots"].centre_mark);
    assert_eq!(dimension(&h, id).type_name, "Dots", "OK puts the dimension on the type shown");
}

#[test]
fn arc_length_picks_an_arc_then_two_points_on_it_to_measure_between() {
    let on = |deg: f64| P2(3000.0 * deg.to_radians().cos(), 3000.0 * deg.to_radians().sin());
    let (a, b) = (on(30.0), on(60.0));
    let script = format!(
        "{BOX}\ndetail_line {{\"start\": [3000, 0], \"end\": [0, 3000], \"arc\": 90}}\ndetail_line {{\"start\": [0, 0], \"end\": [{}, {}]}}\ndetail_line {{\"start\": [0, 0], \"end\": [{}, {}]}}",
        a.0, a.1, b.0, b.1
    );
    let mut h = app(&script);
    press(&mut h, "Annotate");
    if h.query_by_label("Arc Length").is_none() {
        press(&mut h, "Dimension ▾");
    }
    press(&mut h, "Arc Length");
    click(&mut h, on(45.0));
    click(&mut h, a);
    click(&mut h, b);
    assert!(matches!(&h.state().tool, Tool::Round(DimKind::ArcLength, picked) if picked.len() == 3));
    click(&mut h, on(45.0) * 0.6);
    let dims = ids(&h, "dimension");
    let v = field(&h, dims[0], "values")[0].as_f64().unwrap_or_default();
    assert!((v - 3000.0 * std::f64::consts::PI / 6.0).abs() < 0.01, "{v}");
}

#[test]
fn a_double_click_on_a_dimension_s_number_opens_dimension_text_for_that_segment() {
    let script = format!(
        "{BOX}\ngrid {{\"start\": [2000, -2000], \"end\": [2000, 8000]}}\ndimension {{\"refs\": [[100, 3000], [2000, 3000], [7900, 3000]], \"at\": [4000, 3000]}}"
    );
    let mut h = app_at(&script, 0.05);
    let id = ids(&h, "dimension")[0];
    // On the line, over the second segment.
    double_click(&mut h, P2(5000.0, 3000.0));
    assert!(h.state().dimension_text_dialog.is_some());
    type_into(&mut h, "Below:", "CLEAR");
    press(&mut h, "OK");
    assert!(h.state().dimension_text_dialog.is_none(), "{}", h.state().status.text);
    match h.state().session.model.elements.get(id) {
        Some(Element::Dimension(d)) => assert_eq!((d.texts.len(), d.texts[0].segment, d.texts[0].below.as_str()), (1, 1, "CLEAR")),
        e => panic!("{e:?}"),
    }
}

#[test]
fn a_selected_dimension_shows_padlocks_and_eq_which_lock_its_segments_and_make_them_equal() {
    let script = format!(
        "{BOX}\ngrid {{\"start\": [1000, -2000], \"end\": [1000, 8000]}}\ndimension {{\"refs\": [[0, 3000], [1000, 3000], [8000, 3000]], \"at\": [4000, -1000]}}"
    );
    let mut h = app(&script);
    let id = ids(&h, "dimension")[0];
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Lock segment 2");
    let dim = |h: &Harness<BimApp>| dimension(h, id);
    assert_eq!(dim(&h).locked.iter().map(|l| (l.segment, l.length)).collect::<Vec<_>>(), [(1, 7000.0)]);
    // Pressed again, it unlocks.
    press(&mut h, "Lock segment 2");
    assert!(dim(&h).locked.is_empty());
    // EQ: the grid goes to the middle of the walls.
    press(&mut h, "Equal");
    assert!(dim(&h).equal, "{}", h.state().status.text);
    assert_eq!(field(&h, id, "values"), json!([4000.0, 4000.0]));
}

/// Two walls running north, apart: the west one's centre at x = 0, the east one's at x = 2000; and a dimension between their
/// centre lines, south of them.
const TWO_WALLS: &str = r#"wall {"points": [[0, 0], [0, 4000]]}
wall {"points": [[2000, 0], [2000, 4000]]}
dimension {"refs": [[0, 2000], [2000, 2000]], "at": [1000, -1000]}"#;

#[test]
fn typing_3000_into_a_dimension_between_two_walls_moves_the_second_wall() {
    for own in [false, true] {
        let mut h = if own { bimcraft(TWO_WALLS) } else { app(TWO_WALLS) };
        let id = ids(&h, "dimension")[0];
        h.state_mut().selection = vec![id];
        h.run();
        h.get_by_label("Dimension 2000").click();
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.event(Event::Text("3000".into()));
        h.run();
        key(&mut h, Key::Enter);
        assert_eq!(field(&h, id, "values"), json!([3000.0]), "{}", h.state().status.text);
        let walls = ids(&h, "wall");
        assert_eq!((field(&h, walls[0], "start"), field(&h, walls[1], "start")), (json!([0.0, 0.0]), json!([3000.0, 0.0])));
        assert!(h.query_by_label("Dimension 3000").is_some(), "its number, typed again");
    }
}

#[test]
fn a_locked_segment_shows_its_lock_when_its_dimension_isnt_selected() {
    let mut h = app(TWO_WALLS);
    let id = ids(&h, "dimension")[0];
    assert!(h.query_by_label("Locked segment 1").is_none());
    h.state_mut().run("dimension_constraint", json!({"id": id, "locked": [0]}));
    h.run();
    assert!(h.state().selection.is_empty());
    assert!(h.query_by_label("Locked segment 1").is_some(), "shown, though nothing is selected");
    // A click on it unlocks the segment then, as Revit's padlock does.
    press(&mut h, "Locked segment 1");
    assert!(dimension(&h, id).locked.is_empty(), "{}", h.state().status.text);
}

#[test]
fn align_offers_a_padlock_that_keeps_the_lines_aligned_and_shows_until_a_click_unlocks_it() {
    let script = r#"wall {"points": [[0, 0], [0, 4000]]}
wall {"points": [[500, 6000], [500, 9000]]}"#;
    let mut h = app(script);
    let walls = ids(&h, "wall");
    shortcut(&mut h, "AL");
    // The first wall's centre line, then the second's: it moves onto it.
    click(&mut h, P2(0.0, 2000.0));
    click(&mut h, P2(500.0, 7500.0));
    assert_eq!(field(&h, walls[1], "start"), json!([0.0, 6000.0]), "{}", h.state().status.text);
    press(&mut h, "Lock alignment");
    let locks: Vec<Id> = ids(&h, "dimension");
    assert_eq!(locks.len(), 1, "{}", h.state().status.text);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.query_by_label("Unlock alignment").is_some(), "its lock, shown with the tool put down");
    // The first wall moved, by a command: the second follows.
    h.state_mut().run("move", json!({"ids": [walls[0]], "by": [300, 0]}));
    h.run();
    assert_eq!(field(&h, walls[1], "start"), json!([300.0, 6000.0]));
    // A click on the padlock unlocks it.
    press(&mut h, "Unlock alignment");
    assert!(ids(&h, "dimension").is_empty());
}

#[test]
fn aligned_dimensions_pick_structural_column_faces_and_centre_lines() {
    // Two columns 3000 x 3000, 5000 apart east and 2000 north: big enough to click a line away from its ends.
    let script = format!(
        "{BOX}\ntype {{\"category\": \"structuralcolumn\", \"name\": \"Big\", \"width\": 3000, \"depth\": 3000}}\n\
         structural_column {{\"at\": [3000, 3000], \"type\": \"Big\"}}\n\
         structural_column {{\"at\": [8000, 5000], \"type\": \"Big\"}}"
    );
    let mut h = app(&script);
    press(&mut h, "Annotate");
    press(&mut h, "Aligned");
    // Their east and west faces: 2000.
    click(&mut h, P2(4500.0, 3750.0));
    click(&mut h, P2(6500.0, 5750.0));
    click(&mut h, P2(5500.0, 4000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[0], "values")), (1, json!([2000.0])));
    // Their across lines: 5000.
    click(&mut h, P2(3000.0, 3750.0));
    click(&mut h, P2(8000.0, 5750.0));
    click(&mut h, P2(5500.0, 4000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[1], "values")), (2, json!([5000.0])));
    // Their centre lines along: 2000.
    click(&mut h, P2(3750.0, 3000.0));
    click(&mut h, P2(8750.0, 5000.0));
    click(&mut h, P2(5500.0, 4000.0));
    let dims = ids(&h, "dimension");
    assert_eq!((dims.len(), field(&h, dims[2], "values")), (3, json!([2000.0])));
}

/// Three walls running north at `x`, and a dimension across their centre lines below them.
const THREE_WALLS: &str = r#"wall {"points": [[0, 0], [0, 4000]]}
wall {"points": [[2000, 0], [2000, 4000]]}
wall {"points": [[5000, 0], [5000, 4000]]}
dimension {"refs": [[0, 2000], [2000, 2000], [5000, 2000]], "at": [2500, -1000]}"#;

fn pick_dimension_tool(h: &mut Harness<BimApp>) {
    press(h, "Annotate");
    if h.query_by_label("Linear").is_none() {
        press(h, "Dimension ▾");
    }
    press(h, "Linear");
}

#[test]
fn clicking_a_picked_reference_takes_it_out() {
    let mut h = app(THREE_WALLS);
    pick_dimension_tool(&mut h);
    click(&mut h, P2(0.0, 2000.0));
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(5000.0, 2000.0));
    assert!(matches!(&h.state().tool, Tool::Dimension(_, refs) if refs.len() == 3));
    // The third again goes, and the two left make the dimension.
    click(&mut h, P2(5000.0, 2000.0));
    assert!(matches!(&h.state().tool, Tool::Dimension(_, refs) if refs.len() == 2), "{:?}", h.state().tool);
    click(&mut h, P2(1000.0, -1000.0));
    let dims = ids(&h, "dimension");
    assert_eq!(dims.len(), 2, "the script's and the one made");
    assert_eq!(dimension(&h, dims[1]).refs.len(), 2);
}

#[test]
fn backspace_takes_out_the_last_reference() {
    let mut h = app(THREE_WALLS);
    pick_dimension_tool(&mut h);
    click(&mut h, P2(0.0, 2000.0));
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(5000.0, 2000.0));
    key(&mut h, Key::Backspace);
    assert!(matches!(&h.state().tool, Tool::Dimension(_, refs) if refs.len() == 2), "{:?}", h.state().tool);
    click(&mut h, P2(1000.0, -1000.0));
    assert_eq!(ids(&h, "dimension").len(), 2);
}

#[test]
fn esc_clears_the_whole_chain() {
    let mut h = app(THREE_WALLS);
    pick_dimension_tool(&mut h);
    click(&mut h, P2(0.0, 2000.0));
    click(&mut h, P2(2000.0, 2000.0));
    key(&mut h, Key::Escape);
    assert!(matches!(&h.state().tool, Tool::Dimension(_, refs) if refs.is_empty()), "{:?}", h.state().tool);
}

/// The dimension of `TWO_WALLS`, selected.
fn two_walls_selected() -> (Harness<'static, BimApp>, Id) {
    let mut h = app(TWO_WALLS);
    let id = ids(&h, "dimension")[0];
    h.state_mut().selection = vec![id];
    h.run();
    (h, id)
}

#[test]
fn selecting_a_wall_highlights_its_dimension_and_enter_moves_it() {
    let mut h = app(TWO_WALLS);
    let walls = ids(&h, "wall");
    // The west wall, whose number of the dimension across is highlighted.
    click(&mut h, P2(0.0, 2000.0));
    assert_eq!(h.state().selection, vec![walls[0]]);
    h.get_by_label("Dimension 2000").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("3000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, walls[0], "start"), json!([-1000.0, 0.0]), "{}", h.state().status.text);
    assert_eq!(field(&h, walls[1], "start"), json!([2000.0, 0.0]), "the second wall stayed");
    assert_eq!(h.state().selection, vec![walls[0]], "the selection stays on the element");
}

#[test]
fn selecting_a_dimension_does_not_highlight_its_numbers() {
    let (mut h, id) = two_walls_selected();
    // Numbers show as ever for a selected dimension, and typing one works.
    assert!(h.query_by_label("Dimension 2000").is_some());
    h.get_by_label("Dimension 2000").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("3000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, id, "values"), json!([3000.0]), "{}", h.state().status.text);
    let walls = ids(&h, "wall");
    assert_eq!((field(&h, walls[0], "start"), field(&h, walls[1], "start")), (json!([0.0, 0.0]), json!([3000.0, 0.0])));
}

#[test]
fn pressing_eq_on_a_wall_chain_moves_inner_walls_and_keeps_ends() {
    let mut h = app(THREE_WALLS);
    let id = ids(&h, "dimension")[0];
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Equal");
    assert!(dimension(&h, id).equal, "{}", h.state().status.text);
    let walls = ids(&h, "wall");
    assert_eq!((field(&h, walls[0], "start")[0].as_f64(), field(&h, walls[2], "start")[0].as_f64()), (Some(0.0), Some(5000.0)));
    assert_eq!(field(&h, walls[1], "start")[0].as_f64(), Some(2500.0));
    assert_eq!(field(&h, id, "values"), json!([2500.0, 2500.0]));
}

#[test]
fn typing_a_value_on_an_equal_chain_updates_every_number() {
    let mut h = app(THREE_WALLS);
    let id = ids(&h, "dimension")[0];
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Equal");
    let first = h.get_all_by_label("Dimension 2500").next().map(|n| n.rect().center()).unwrap();
    click_at(&mut h, first);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("4000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, id, "values"), json!([4000.0, 4000.0]), "{}", h.state().status.text);
    let walls = ids(&h, "wall");
    assert_eq!(field(&h, walls[0], "start")[0].as_f64(), Some(0.0));
    assert_eq!(field(&h, walls[2], "start")[0].as_f64(), Some(8000.0));
}

#[test]
fn text_icon_under_the_number_opens_the_text_dialog() {
    let (mut h, id) = two_walls_selected();
    let before = dimension(&h, id);
    press(&mut h, "Dimension text");
    assert!(h.state().dimension_text_dialog.is_some());
    press(&mut h, "Cancel");
    assert!(h.state().dimension_text_dialog.is_none());
    assert_eq!(dimension(&h, id), before, "cancelling changes nothing");
}

#[test]
fn clearing_replace_restores_the_true_number() {
    let (mut h, id) = two_walls_selected();
    press(&mut h, "Dimension text");
    type_into(&mut h, "Replace With Text:", "X");
    press(&mut h, "OK");
    assert_eq!(field(&h, id, "labels"), json!(["X"]), "{}", h.state().status.text);
    press(&mut h, "Dimension text");
    h.get_by_label("Replace With Text:").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Backspace);
    h.run();
    press(&mut h, "OK");
    assert_eq!(field(&h, id, "labels"), json!(["2000"]), "{}", h.state().status.text);
}

#[test]
fn edit_dimension_references_from_the_context_menu() {
    let mut h = app(THREE_WALLS);
    let id = ids(&h, "dimension")[0];
    h.state_mut().selection = vec![id];
    h.run();
    h.state_mut().start_dimension_refs_edit();
    h.run();
    assert!(h.state().dimension_refs_edit.is_some());
    // The west wall's reference goes; a click away confirms the line where it is clicked.
    click(&mut h, P2(0.0, 2000.0));
    click(&mut h, P2(1000.0, -1500.0));
    assert!(h.state().dimension_refs_edit.is_none(), "{}", h.state().status.text);
    let dim = dimension(&h, id);
    assert_eq!(dim.refs.len(), 2);
    assert_eq!(field(&h, id, "values"), json!([3000.0]), "{}", h.state().status.text);
}

#[test]
fn esc_leaves_an_edited_dimension_as_it_was() {
    let mut h = app(THREE_WALLS);
    let id = ids(&h, "dimension")[0];
    let before = dimension(&h, id);
    h.state_mut().selection = vec![id];
    h.run();
    h.state_mut().start_dimension_refs_edit();
    h.run();
    click(&mut h, P2(0.0, 2000.0));
    key(&mut h, Key::Escape);
    assert!(h.state().dimension_refs_edit.is_none());
    assert_eq!(dimension(&h, id), before);
}

#[test]
fn clicking_eq_with_the_pointer_keeps_the_dimension_selected_and_evens_the_chain() {
    let mut h = app(THREE_WALLS);
    let id = ids(&h, "dimension")[0];
    h.state_mut().selection = vec![id];
    h.run();
    let at = h.get_by_label("Equal").rect().center();
    click_at(&mut h, at);
    assert_eq!(h.state().selection, vec![id], "the dimension stays selected, so the click is the button's");
    assert!(dimension(&h, id).equal, "{}", h.state().status.text);
    let walls = ids(&h, "wall");
    assert_eq!(field(&h, walls[1], "start")[0].as_f64(), Some(2500.0));
    assert_eq!(field(&h, id, "values"), json!([2500.0, 2500.0]));
}

#[test]
fn esc_cancels_a_typed_value_and_keeps_the_selection() {
    let mut h = app(TWO_WALLS);
    let walls = ids(&h, "wall");
    let id = ids(&h, "dimension")[0];
    click(&mut h, P2(0.0, 2000.0));
    h.get_by_label("Dimension 2000").click();
    h.run();
    h.event(Event::Text("3000".into()));
    h.run();
    key(&mut h, Key::Escape);
    assert_eq!(h.state().selection, vec![walls[0]], "the element stays selected");
    assert_eq!(field(&h, id, "values"), json!([2000.0]), "nothing changed");
    assert_eq!(field(&h, walls[0], "start"), json!([0.0, 0.0]));
}

#[test]
fn the_temporary_wall_dimension_has_a_button_that_makes_it_permanent() {
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]]}"#);
    click(&mut h, P2(2000.0, 0.0));
    assert!(h.query_by_label("Make Dimension Permanent").is_some(), "the button is under the temporary number");
    press(&mut h, "Make Dimension Permanent");
    let dims = ids(&h, "dimension");
    assert_eq!(dims.len(), 1, "{}", h.state().status.text);
    assert_eq!(dimension(&h, dims[0]).refs.len(), 2);
    assert_eq!(field(&h, dims[0], "values"), json!([4000.0]));
}

#[test]
fn typing_a_temporary_wall_dimension_moves_both_joined_ends_evenly() {
    let script = r#"wall {"points": [[0, 0], [4000, 0]]}
wall {"points": [[0, 0], [0, 3000]]}
wall {"points": [[4000, 0], [4000, 3000]]}"#;
    let mut h = app(script);
    let walls = ids(&h, "wall");
    click(&mut h, P2(2000.0, 0.0));
    h.get_by_label("Temporary dimension 4000").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("6000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, walls[0], "start"), json!([-1000.0, 0.0]), "{}", h.state().status.text);
    assert_eq!(field(&h, walls[0], "end"), json!([5000.0, 0.0]));
}

#[test]
fn tab_picks_a_dimension_segment_and_delete_takes_it_out() {
    let mut h = app(THREE_WALLS);
    let id = ids(&h, "dimension")[0];
    // Over the first segment, Tab highlights it; a click selects it.
    h.hover_at(h.state().screen(P2(1000.0, -1000.0)));
    h.run();
    key(&mut h, Key::Tab);
    assert!(h.state().dimension_segment_hover.is_some(), "Tab picked the segment under the cursor");
    click(&mut h, P2(1000.0, -1000.0));
    assert_eq!(h.state().dimension_segment, Some((id, 0)));
    // Delete takes the segment out: the chain measures across where it was.
    key(&mut h, Key::Delete);
    assert_eq!(dimension(&h, id).refs.len(), 2, "{}", h.state().status.text);
    assert_eq!(field(&h, id, "values"), json!([5000.0]));
}

#[test]
fn an_equal_chain_shows_its_numbers_and_eq_over_it() {
    let mut h = app(THREE_WALLS);
    let id = ids(&h, "dimension")[0];
    h.state_mut().run("dimension_constraint", json!({"id": id, "equal": true}));
    h.run();
    // Its numbers are numbers, not EQ.
    assert_eq!(field(&h, id, "labels"), json!(["2500", "2500"]));
    // Over an equal chain, EQ shows above its middle; away from it, with nothing selected, it goes.
    h.hover_at(h.state().screen(P2(2500.0, -1000.0)));
    h.run();
    h.run();
    assert!(h.query_by_label("Equal").is_some(), "EQ shows while the equal chain is hovered");
    h.hover_at(h.state().screen(P2(2500.0, 2000.0)));
    h.run();
    h.run();
    assert!(h.query_by_label("Equal").is_none(), "EQ goes when the pointer leaves, nothing selected");
    // Selected, it stays over the chain: struck through when the chain isn't equal, and a click turns it on.
    h.state_mut().selection = vec![id];
    h.run();
    h.state_mut().run("dimension_constraint", json!({"id": id, "equal": false}));
    h.run();
    assert!(h.query_by_label("Equal").is_some(), "a selected dimension shows its EQ even when it isn't equal");
    h.get_by_label("Equal").click();
    h.run();
    assert!(dimension(&h, id).equal, "{}", h.state().status.text);
    assert_eq!(h.state().selection, vec![id], "clicking EQ keeps the dimension selected");
    // A click on the equal one turns it off.
    h.get_by_label("Equal").click();
    h.run();
    assert!(!dimension(&h, id).equal);
}

#[test]
fn a_permanent_dimension_at_a_joined_end_measures_to_the_perpendicular_wall() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    // The south wall, both of whose ends are joined to the west and east walls.
    click(&mut h, P2(4000.0, 0.0));
    press(&mut h, "Make Dimension Permanent");
    let d = ids(&h, "dimension")[0];
    let mut refs: Vec<Id> = dimension(&h, d).refs.iter().filter_map(|r| h.state().session.model.elements.id(r.id)).collect();
    refs.sort();
    let mut sides = vec![walls[1], walls[3]];
    sides.sort();
    assert_eq!(refs, sides, "the dimension measures to the perpendicular walls");
    // Changing it moves the whole east wall, which stays square, and the west wall stays put.
    h.state_mut().run("dimension_value", json!({"id": d, "value": 9000}));
    h.run();
    assert_eq!(field(&h, walls[1], "start"), json!([9000.0, 0.0]), "{}", h.state().status.text);
    assert_eq!(field(&h, walls[1], "end"), json!([9000.0, 6000.0]), "the whole wall moved and stays square");
    assert_eq!(field(&h, walls[3], "start"), json!([0.0, 6000.0]), "the west wall stayed");
    assert_eq!(field(&h, d, "values"), json!([9000.0]));
}
