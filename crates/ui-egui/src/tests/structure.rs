//! The Structure tab, driven the way a user drives it.

use super::*;

/// A tool from the Structure tab.
fn structural(h: &mut Harness<BimApp>, label: &str) {
    press(h, "Structure");
    press(h, label);
}

/// The strokes in the accent colour, a tool's preview, as the closed outlines (on screen) the window drew.
fn previewed(h: &Harness<BimApp>) -> Vec<Vec<Pos2>> {
    fn walk(s: &egui::Shape, out: &mut Vec<Vec<Pos2>>) {
        match s {
            egui::Shape::Path(p) if p.closed && p.stroke.color == egui::epaint::ColorMode::Solid(crate::style::pal().accent) => {
                out.push(p.points.clone())
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// Replace the contents of the text field of `role` named `label`.
fn type_into_role(h: &mut Harness<BimApp>, role: Role, label: &str, text: &str) {
    h.get_by_role_and_label(role, label).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by_role_and_label(role, label).type_text(text);
    h.run();
}

fn axis(h: &Harness<BimApp>, id: Id) -> Value {
    field(h, id, "axis")
}

#[test]
fn the_structure_tab_has_the_structure_tools() {
    let mut h = app(BOX);
    press(&mut h, "Structure");
    for label in ["Column", "Beam", "Floor: Structural", "Isolated Footing", "Wall Footing"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // The Architecture tab's tools are not here.
    assert!(h.query_by_label("Door").is_none());
}

#[test]
fn beams_go_between_two_clicks_with_their_tops_at_the_level_and_chain_on() {
    let mut h = app(BOX);
    structural(&mut h, "Beam");
    assert!(h.query_by_label("Modify | Place Beam").is_some());
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 1000.0));
    let beams = ids(&h, "beam");
    assert_eq!(beams.len(), 1);
    assert_eq!((field(&h, beams[0], "type"), field(&h, beams[0], "justify")), (json!("Beam 200 x 500"), json!("top")));
    assert_eq!(axis(&h, beams[0]), json!([[1000.0, 1000.0, -250.0], [5000.0, 1000.0, -250.0]]));
    // Chain, as the walls': the next one starts where it ended.
    assert_eq!(h.state().tool, Tool::Run("beam", Some(P2(5000.0, 1000.0))));
    click(&mut h, P2(5000.0, 4000.0));
    assert_eq!(ids(&h, "beam").len(), 2);
    // Without it, two clicks make one.
    press(&mut h, "Chain");
    key(&mut h, Key::Escape);
    click(&mut h, P2(1000.0, 5000.0));
    click(&mut h, P2(3000.0, 5000.0));
    assert_eq!((ids(&h, "beam").len(), h.state().tool.clone()), (3, Tool::Run("beam", None)));
    // A click where it started makes nothing; Escape drops the start, then the tool.
    click(&mut h, P2(100.0, 100.0));
    click(&mut h, P2(100.0, 100.0));
    assert_eq!(ids(&h, "beam").len(), 3);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Run("beam", None));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    assert_eq!(h.state().session.history().last(), Some(&"Beam"));
}

#[test]
fn the_beam_tools_options_set_its_justification_height_and_type() {
    let mut h = app(BOX);
    structural(&mut h, "Beam");
    press(&mut h, "Chain");
    h.get_by_role_and_label(Role::ComboBox, "Justification:").click();
    h.run();
    press(&mut h, "Centre");
    type_into(&mut h, "Offset:", "2800");
    press(&mut h, "Type Selector");
    press(&mut h, "Beam 150 x 400");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 1000.0));
    let beam = ids(&h, "beam")[0];
    assert_eq!((field(&h, beam, "type"), field(&h, beam, "justify")), (json!("Beam 150 x 400"), json!("centre")));
    assert_eq!(axis(&h, beam), json!([[1000.0, 1000.0, 2800.0], [5000.0, 1000.0, 2800.0]]));
    // What isn't a height is said, and makes nothing.
    type_into(&mut h, "Offset:", "high");
    click(&mut h, P2(1000.0, 2000.0));
    click(&mut h, P2(5000.0, 2000.0));
    assert!(h.state().status.error && h.state().status.text.contains("high"), "{:?}", h.state().status);
    assert_eq!(ids(&h, "beam").len(), 1);
}

#[test]
fn the_beam_tool_shows_the_beam_a_click_would_make_and_how_long_it_is() {
    let mut h = app(BOX);
    structural(&mut h, "Beam");
    click(&mut h, P2(1000.0, 1000.0));
    h.hover_at(h.state().screen(P2(5000.0, 1000.0)));
    h.run();
    // Its footprint, 200 wide, from the start to the cursor; and its length.
    let outlines = previewed(&h);
    let (a, b) = (h.state().screen(P2(0.0, 0.0)), h.state().screen(P2(1000.0, 0.0)));
    let width = f64::from(b.x - a.x) / 1000.0 * 200.0;
    assert!(
        outlines.iter().any(|o| {
            let (lo, hi) = o.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
            ((hi - lo) as f64 - width).abs() < 1.5 && o.len() == 4
        }),
        "{outlines:?}"
    );
    assert!(painted_text(&h).contains(&"4000".to_string()), "{:?}", painted_text(&h));
    assert!(ids(&h, "beam").is_empty(), "a sketch isn't an element");
}

#[test]
fn structural_columns_and_footings_go_where_clicked() {
    let mut h = app(BOX);
    structural(&mut h, "Column");
    assert!(h.query_by_label("Modify | Place Structural Column").is_some());
    click(&mut h, P2(4000.0, 3000.0));
    let col = ids(&h, "structuralcolumn");
    assert_eq!((col.len(), field(&h, col[0], "type"), field(&h, col[0], "top_level")), (1, json!("Concrete 300 x 300"), json!(100_001)));
    // The height, as the Column tool's: unconnected, typed.
    h.get_by_role_and_label(Role::ComboBox, "Height:").click();
    h.run();
    press(&mut h, "Unconnected");
    type_into_role(&mut h, Role::TextInput, "Height:", "2500");
    click(&mut h, P2(2000.0, 3000.0));
    let col = ids(&h, "structuralcolumn");
    assert_eq!((field(&h, col[1], "top_level"), field(&h, col[1], "height")), (json!(null), json!(2500.0)));
    assert!(ids(&h, "column").is_empty(), "not the architectural column");
    // A footing: thick 400 below the level, unless the Options Bar says.
    structural(&mut h, "Isolated Footing");
    assert!(h.query_by_label("Modify | Place Isolated Footing").is_some());
    click(&mut h, P2(4000.0, 3000.0));
    let f = ids(&h, "footing");
    assert_eq!((field(&h, f[0], "height"), field(&h, f[0], "base_offset")), (json!(400.0), json!(-400.0)));
    type_into(&mut h, "Thickness:", "600");
    type_into(&mut h, "Top Offset:", "-100");
    click(&mut h, P2(2000.0, 3000.0));
    let f = ids(&h, "footing");
    assert_eq!((field(&h, f[1], "height"), field(&h, f[1], "base_offset")), (json!(600.0), json!(-700.0)));
    // It is seen from the plan it hangs under.
    click(&mut h, P2(2000.0, 3000.0));
    key(&mut h, Key::Escape);
    click(&mut h, P2(2200.0, 3200.0));
    assert!(h.state().selection.len() == 1);
}

#[test]
fn wall_footings_run_under_a_wall_between_two_clicks() {
    let mut h = app(BOX);
    structural(&mut h, "Wall Footing");
    assert!(h.query_by_label("Modify | Place Wall Footing").is_some());
    press(&mut h, "Chain");
    click(&mut h, P2(0.0, 0.0));
    click(&mut h, P2(8000.0, 0.0));
    let strip = ids(&h, "stripfooting");
    assert_eq!(strip.len(), 1);
    assert_eq!(axis(&h, strip[0]), json!([[0.0, 0.0, -150.0], [8000.0, 0.0, -150.0]]));
    type_into(&mut h, "Top Offset:", "-200");
    click(&mut h, P2(8000.0, 0.0));
    click(&mut h, P2(8000.0, 6000.0));
    let strip = ids(&h, "stripfooting");
    assert_eq!(axis(&h, strip[1]), json!([[8000.0, 0.0, -350.0], [8000.0, 6000.0, -350.0]]));
}

#[test]
fn the_structure_tabs_floor_is_a_structural_one_and_the_architecture_tabs_is_not() {
    let mut h = app(BOX);
    structural(&mut h, "Floor: Structural");
    click(&mut h, P2(4000.0, 3000.0));
    let floors = ids(&h, "floor");
    assert_eq!(field(&h, floors[0], "structural"), json!(true));
    tool(&mut h, "Floor");
    click(&mut h, P2(4000.0, 3000.0));
    let floors = ids(&h, "floor");
    assert_eq!(floors.len(), 2);
    assert!(field(&h, floors[1], "structural").is_null(), "an architectural floor says nothing of load");
}

#[test]
fn revits_shortcuts_pick_the_structure_tools() {
    let mut h = app(BOX);
    for (letters, tool) in [("BM", Tool::Run("beam", None)), ("FI", Tool::Stand("footing")), ("FW", Tool::Run("stripfooting", None))] {
        shortcut(&mut h, letters);
        assert_eq!(h.state().tool, tool, "{letters}");
        key(&mut h, Key::Escape);
    }
    // SB is the structural floor; CL the architectural column, but on the Structure tab the structural one, as in Revit.
    shortcut(&mut h, "SB");
    assert_eq!(h.state().tool, Tool::Floor);
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(field(&h, ids(&h, "floor")[0], "structural"), json!(true));
    shortcut(&mut h, "CL");
    assert_eq!(h.state().tool, Tool::Column);
    key(&mut h, Key::Escape);
    press(&mut h, "Structure");
    shortcut(&mut h, "CL");
    assert_eq!(h.state().tool, Tool::Stand("structuralcolumn"));
    // With a Structure tool on, its contextual tab has taken the ribbon: CL still means the structural column.
    shortcut(&mut h, "BM");
    assert_eq!(h.state().tool, Tool::Run("beam", None));
    shortcut(&mut h, "CL");
    assert_eq!(h.state().tool, Tool::Stand("structuralcolumn"));
    // They are on the list, too.
    key(&mut h, Key::Escape);
    let listed: Vec<(&str, &str)> = crate::shortcuts();
    for pair in
        [("BM", "Beam"), ("FI", "Isolated Footing"), ("FW", "Wall Footing"), ("SB", "Floor: Structural"), ("CL", "Structural Column")]
    {
        assert!(listed.contains(&pair), "{pair:?}");
    }
}

#[test]
fn a_selected_beams_palette_sets_its_heights_justification_and_system() {
    let mut h = app(&format!("{BOX}\nbeam {{\"points\": [[1000, 1000], [5000, 1000]], \"offset\": 2800}}"));
    let beam = ids(&h, "beam")[0];
    // Overhead, it is seen dashed in the plan, and picked there.
    click(&mut h, P2(3000.0, 1000.0));
    assert_eq!(h.state().selection, [beam]);
    assert!(h.query_by_label("Modify | Beams").is_some());
    for row in ["Reference Level", "Start Level Offset", "End Level Offset", "Z Justification", "System", "Length"] {
        assert!(h.query_by_label(row).is_some(), "{row}");
    }
    type_into(&mut h, "Start Level Offset", "3000");
    key(&mut h, Key::Enter);
    assert_eq!(axis(&h, beam), json!([[1000.0, 1000.0, 2750.0], [5000.0, 1000.0, 2750.0]]));
    type_into(&mut h, "End Level Offset", "3400");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, beam, "end_offset"), json!(3400.0));
    h.get_by_role_and_label(Role::ComboBox, "Z Justification").click();
    h.run();
    press(&mut h, "Centre");
    assert_eq!(field(&h, beam, "justify"), json!("centre"));
    type_into(&mut h, "System", "Frame A");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, beam, "system"), json!("Frame A"));
    // A length it cannot have is refused in the palette's way, and the beam stays.
    type_into(&mut h, "End Level Offset", "1e300");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error);
    assert_eq!(field(&h, beam, "end_offset"), json!(3400.0));
}

#[test]
fn edit_type_changes_a_beams_section_for_every_beam_of_the_type() {
    let mut h = app("beam {\"points\": [[0, 0], [4000, 0]]}\nbeam {\"points\": [[0, 2000], [4000, 2000]]}");
    let (a, b) = (ids(&h, "beam")[0], ids(&h, "beam")[1]);
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [a]);
    press(&mut h, "Edit Type");
    type_into(&mut h, "Width", "250");
    type_into(&mut h, "Depth", "600");
    press(&mut h, "OK");
    for id in [a, b] {
        assert_eq!((field(&h, id, "width"), field(&h, id, "depth")), (json!(250.0), json!(600.0)));
    }
    // The type selector lists the beam types, and only those.
    press(&mut h, "Type Selector");
    assert!(h.query_by_label("Steel Tube 150").is_some() && h.query_by_label("Rectangular 400 x 400").is_none());
    press(&mut h, "Steel Tube 150");
    assert_eq!(field(&h, a, "round"), json!(true));
}

#[test]
fn create_similar_and_the_view_lists_know_the_structures_categories() {
    let mut h = app(
        "beam {\"points\": [[0, 0], [4000, 0]], \"type\": \"Beam 150 x 400\"}\nfooting {\"at\": [2000, 2000]}\nstructural_column {\"at\": [3000, 3000]}",
    );
    let (beam, footing, column) = (ids(&h, "beam")[0], ids(&h, "footing")[0], ids(&h, "structuralcolumn")[0]);
    for (id, tool) in [(beam, Tool::Run("beam", None)), (footing, Tool::Stand("footing")), (column, Tool::Stand("structuralcolumn"))] {
        h.state_mut().selection = vec![id];
        shortcut(&mut h, "CS");
        assert_eq!(h.state().tool, tool);
        key(&mut h, Key::Escape);
        key(&mut h, Key::Escape);
    }
    // Create Similar picks the type of what it copies.
    h.state_mut().selection = vec![beam];
    shortcut(&mut h, "CS");
    click(&mut h, P2(100.0, 100.0));
    click(&mut h, P2(2100.0, 100.0));
    assert_eq!(field(&h, *ids(&h, "beam").last().unwrap(), "type"), json!("Beam 150 x 400"));
    // Visibility/Graphics lists them by name.
    shortcut(&mut h, "VV");
    for name in ["Beams", "Structural Columns", "Footings", "Strip Footings"] {
        assert!(h.query_by_label(name).is_some(), "{name}");
    }
}

#[test]
fn a_selected_structural_column_shows_its_distances_to_the_walls_and_can_be_dimensioned() {
    let mut h = app(&format!("{BOX}\nstructural_column {{\"at\": [3000, 2000]}}"));
    let column = ids(&h, "structuralcolumn")[0];
    click(&mut h, P2(3000.0, 2000.0));
    assert_eq!(h.state().selection, [column]);
    let text = painted_text(&h);
    for n in ["3000", "2000", "5000", "4000"] {
        assert!(text.contains(&n.to_string()), "{n}: {text:?}");
    }
}

#[test]
fn the_structure_speaks_portuguese_and_spanish_and_leaves_no_english() {
    use crate::lang::Lang;
    use egui_kittest::kittest::{By, NodeT};
    let english = |h: &Harness<BimApp>, l: Lang| -> Vec<String> {
        h.query_all(By::new())
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .filter(|s| crate::lang::translated_in(l, s).is_some_and(|x| x != s))
            .collect()
    };
    let mut h = app(&format!(
        "{BOX}\nbeam {{\"points\": [[1000, 1000], [5000, 1000]], \"offset\": 2800}}\nfooting {{\"at\": [2000, 2000]}}\nstructural_column {{\"at\": [3000, 3000]}}\nstrip_footing {{\"points\": [[0, 0], [8000, 0]]}}"
    ));
    assert!(english(&h, Lang::Spanish).contains(&"Structure".to_string()), "the probe finds what it looks for");
    let expected = [
        (Lang::Portuguese, "Estrutura", ["Pilar", "Viga", "Laje estrutural", "Sapata isolada", "Sapata corrida"]),
        (Lang::Spanish, "Estructura", ["Columna", "Viga", "Losa estructural", "Zapata aislada", "Zapata corrida"]),
    ];
    for (l, tab, labels) in expected {
        h.state_mut().lang = l;
        h.run();
        press(&mut h, tab);
        let mut missed = english(&h, l);
        for label in labels {
            assert!(h.query_by_label(label).is_some(), "{label}");
            press(&mut h, label);
            missed.extend(english(&h, l));
            // With a start picked, the Options Bar and the contextual tab.
            click(&mut h, P2(100.0, 100.0));
            missed.extend(english(&h, l));
            key(&mut h, Key::Escape);
            key(&mut h, Key::Escape);
        }
        // What they made, selected: the palette, the contextual tab and the type dialog.
        for (point, edit_type) in [(P2(3000.0, 1000.0), true), (P2(2000.0, 2000.0), false), (P2(3000.0, 3000.0), false)] {
            click(&mut h, point);
            missed.extend(english(&h, l));
            if edit_type {
                press(&mut h, crate::lang::translated_in(l, "Edit Type").unwrap_or("Edit Type"));
                missed.extend(english(&h, l));
                press(&mut h, crate::lang::translated_in(l, "Cancel").unwrap_or("Cancel"));
            }
            key(&mut h, Key::Escape);
        }
        missed.sort();
        missed.dedup();
        assert!(missed.is_empty(), "{l:?}: {missed:?}");
    }
    h.state_mut().lang = Lang::English;
    h.run();
    use crate::lang::category;
    assert_eq!(
        [category("beam", false), category("structuralcolumn", true), category("footing", true)],
        ["Beam", "Structural Columns", "Footings"]
    );
    h.state_mut().lang = Lang::Portuguese;
    h.run();
    assert_eq!(
        [category("beam", true), category("structuralcolumn", false), category("stripfooting", false)],
        ["Vigas", "Pilar estrutural", "Sapata corrida"]
    );
}

#[test]
fn a_duplicated_beam_type_keeps_its_shape_and_its_material() {
    let mut h = app("beam {\"points\": [[0, 0], [4000, 0]], \"type\": \"Steel Tube 150\", \"offset\": 2800}");
    let beam = ids(&h, "beam")[0];
    click(&mut h, P2(2000.0, 0.0));
    press(&mut h, "Edit Type");
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Steel Tube 200");
    press(&mut h, "Create");
    // A round type has a diameter, not a width and a depth.
    type_into(&mut h, "Diameter", "200");
    press(&mut h, "OK");
    let made = h.state().session.model.types.runs["beam"]["Steel Tube 200"].clone();
    assert_eq!((made.width, made.depth, made.round, made.material.as_str()), (200.0, 200.0, true, "Metal"));
    assert_eq!(field(&h, beam, "type"), json!("Steel Tube 200"), "OK puts the selected beam on it");
}
