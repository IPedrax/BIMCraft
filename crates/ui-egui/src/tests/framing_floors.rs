//! Floor framing in the window: Structure > Frame and Remove Framing on the selected floors with the walls, a floor's Framing
//! and Joist Direction in Properties, a framing type's floor fields in Type Properties, and Floors > Framing and Framing Lines
//! in Visibility/Graphics.

use super::*;

/// A 2x4 wall type and a wall of it; a 2x8 floor type (18 mm of subfloor over a 184 mm structure layer) and a floor of it,
/// 4000 by 3000, on two beams under its rims whose tops are at its joists' underside.
const FLOOR: &str = "type {\"category\": \"wall\", \"name\": \"2x4\", \"layers\": [{\"function\": \"structure\", \"material\": \"Timber\", \"thickness\": 89}]}\n\
     wall {\"points\": [[0,6000],[8000,6000]], \"type\": \"2x4\"}\n\
     type {\"category\": \"floor\", \"name\": \"2x8 Floor\", \"layers\": [{\"function\": \"substrate\", \"material\": \"Timber\", \"thickness\": 18}, {\"function\": \"structure\", \"material\": \"Timber\", \"thickness\": 184}]}\n\
     type {\"category\": \"beam\", \"name\": \"3-2x10\", \"width\": 114, \"depth\": 235}\n\
     beam {\"points\": [[0,57],[4000,57]], \"type\": \"3-2x10\", \"offset\": -202}\n\
     beam {\"points\": [[0,2943],[4000,2943]], \"type\": \"3-2x10\", \"offset\": -202}\n\
     floor {\"boundary\": [[0,0],[4000,0],[4000,3000],[0,3000]], \"type\": \"2x8 Floor\"}";

/// The floor with the default framing type in the project.
fn with_type() -> String {
    format!("{FLOOR}\ntype {{\"category\": \"framing\", \"name\": \"2x4 @ 406\"}}")
}

fn structural(h: &mut Harness<BimApp>, label: &str) {
    press(h, "Structure");
    press(h, label);
}

fn select(h: &mut Harness<BimApp>, ids: &[Id]) {
    h.state_mut().selection = ids.to_vec();
    h.run();
}

fn framing_type(h: &Harness<BimApp>, name: &str) -> Value {
    json!(h.state().session.model.types.framing.get(name))
}

/// The Joist Direction row of a floor's Properties.
const DIRECTION: &str = "Joist Direction (°)";

#[test]
fn frame_and_remove_framing_act_on_the_selected_walls_and_floors_each_as_one_step() {
    let mut h = app(FLOOR);
    let (wall, floor) = (ids(&h, "wall")[0], ids(&h, "floor")[0]);
    select(&mut h, &[wall, floor]);
    let steps = h.state().session.history().len();
    structural(&mut h, "Frame");
    assert_eq!((field(&h, wall, "framing"), field(&h, floor, "framing")), (json!("2x4 @ 406"), json!("2x4 @ 406")));
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    let said = &h.state().status;
    assert!(!said.error && said.text.contains("1 walls and 1 floors"), "{}", said.text);
    // A floor alone says so.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(field(&h, floor, "framing"), Value::Null);
    select(&mut h, &[floor]);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(!said.error && said.text.contains("1 floors framed as 2x4 @ 406"), "{}", said.text);
    // Remove Framing takes it off the walls and floors selected, in a step.
    select(&mut h, &[wall, floor]);
    structural(&mut h, "Frame");
    let steps = h.state().session.history().len();
    structural(&mut h, "Remove Framing");
    assert_eq!((field(&h, wall, "framing"), field(&h, floor, "framing")), (Value::Null, Value::Null));
    assert_eq!(h.state().session.history().len(), steps + 1);
    // With nothing to frame selected, it says what to select.
    let beams = ids(&h, "beam");
    select(&mut h, &beams);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("walls, floors or roofs"), "{}", said.text);
}

#[test]
fn frame_says_in_the_status_bar_what_a_floor_couldnt_be_framed_as_the_type_says() {
    // No beams under it: nothing bears its joists.
    let script: String = FLOOR.lines().filter(|l| !l.trim_start().starts_with("beam ")).collect::<Vec<_>>().join("\n");
    let mut h = app(&script);
    let floor = ids(&h, "floor")[0];
    select(&mut h, &[floor]);
    structural(&mut h, "Frame");
    assert_eq!(field(&h, floor, "framing"), json!("2x4 @ 406"), "framed all the same");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("Warnings (1):"), "{}", said.text);
    assert!(said.text.contains(&format!("floor {floor}: nothing bears its joists")), "{}", said.text);
    // A floor the project can't frame: it says why, and nothing changes.
    let mut h = app(&format!("{script}\nfloor {{\"boundary\": [[5000,0],[7000,0],[7000,2000],[5000,2000]]}}"));
    let plain = ids(&h, "floor")[1];
    select(&mut h, &[plain]);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("structure layer"), "{}", said.text);
    assert_eq!(field(&h, plain, "framing"), Value::Null);
}

#[test]
fn a_floors_framing_and_joist_direction_are_set_in_properties() {
    let mut h = app(&with_type());
    let floor = ids(&h, "floor")[0];
    select(&mut h, &[floor]);
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Framing").value().as_deref(), Some("None"));
    assert!(h.query_by_label(DIRECTION).is_none(), "no joists, no direction");
    let steps = h.state().session.history().len();
    choose(&mut h, "Framing", "2x4 @ 406");
    assert_eq!(field(&h, floor, "framing"), json!("2x4 @ 406"));
    assert_eq!(h.state().session.history().len(), steps + 1);
    // Framed, its joists' direction: blank, square to its longest edge, until it's typed.
    assert_eq!(h.get_by_role_and_label(Role::TextInput, DIRECTION).value().unwrap_or_default(), "");
    let way = |h: &Harness<BimApp>| match h.state().session.model.elements.get(floor) {
        Some(Element::Floor(f)) => bimcraft_core::framing::floor::angle(f),
        _ => f64::NAN,
    };
    assert!((way(&h) - 90.0).abs() < 1e-9, "the joists span the short way: {}", way(&h));
    type_into(&mut h, DIRECTION, "0");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, floor, "framing_angle"), json!(0.0), "{}", h.state().status.text);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, DIRECTION).value().unwrap_or_default(), "0");
    // Cleared, it's automatic again; a word is refused, saying why.
    h.get_by_role_and_label(Role::TextInput, DIRECTION).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Backspace);
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, floor, "framing_angle"), Value::Null, "{}", h.state().status.text);
    type_into(&mut h, DIRECTION, "north");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error && h.state().status.text.contains("not a number"), "{}", h.state().status.text);
    assert_eq!(field(&h, floor, "framing_angle"), Value::Null);
    // Its framing type's Type Properties, from beside Framing.
    h.get_by_label("Edit Framing Type").scroll_to_me();
    h.run();
    press(&mut h, "Edit Framing Type");
    assert!(h.state().type_dialog.is_some());
    press(&mut h, "Cancel");
    // None takes the framing off, and the direction with it from Properties.
    choose(&mut h, "Framing", "None");
    assert_eq!(field(&h, floor, "framing"), Value::Null);
    assert!(h.query_by_label(DIRECTION).is_none());
}

#[test]
fn a_framing_types_floor_fields_are_edited_in_type_properties_under_floors() {
    let mut h = app(&with_type());
    let floor = ids(&h, "floor")[0];
    select(&mut h, &[floor]);
    choose(&mut h, "Framing", "2x4 @ 406");
    h.get_by_label("Edit Framing Type").scroll_to_me();
    h.run();
    press(&mut h, "Edit Framing Type");
    // The wall's fields and the floor's are under groups of their own.
    for group in ["Walls", "Wall Openings", "Wall Joins", "Floors and Roofs", "Floors"] {
        assert!(h.query_all_by_label(group).next().is_some(), "the group {group}");
    }
    let was = framing_type(&h, "2x4 @ 406");
    assert_eq!(
        (&was["rim"], &was["lap"], &was["bridging"], &was["trimmers"], &was["under_partitions"]),
        (&json!(true), &json!(0.0), &json!(0.0), &json!(2), &json!(true))
    );
    // Apply with nothing changed changes nothing.
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(framing_type(&h, "2x4 @ 406"), was);
    type_into(&mut h, "Lap at Supports", "300");
    type_into(&mut h, "Bridging Spacing", "1200");
    type_into(&mut h, "Trimmer and Header Plies", "3");
    for label in ["Rim Joists and Eave Boards", "Joists Under Partitions"] {
        h.get_by_role_and_label(Role::CheckBox, label).scroll_to_me();
        h.run();
        pick(&mut h, Role::CheckBox, label);
    }
    press_near(&mut h, "Apply", "Cancel");
    let now = framing_type(&h, "2x4 @ 406");
    assert_eq!(
        (&now["rim"], &now["lap"], &now["bridging"], &now["trimmers"], &now["under_partitions"]),
        (&json!(false), &json!(300.0), &json!(1200.0), &json!(3), &json!(false)),
        "{:?}",
        h.state().type_dialog.as_ref().and_then(|d| d.error.clone())
    );
    assert_eq!(now["spacing"], was["spacing"], "what wasn't typed over is as it was");
    // The project's own rules are said in the dialog, and change nothing.
    type_into(&mut h, "Trimmer and Header Plies", "4");
    press_near(&mut h, "Apply", "Cancel");
    let d = h.state().type_dialog.as_ref().expect("a refusal keeps the dialog open");
    assert!(d.error.as_deref().is_some_and(|e| e.contains("trimmers")), "{:?}", d.error);
    type_into(&mut h, "Trimmer and Header Plies", "3");
    type_into(&mut h, "Bridging Spacing", "100");
    press_near(&mut h, "Apply", "Cancel");
    let d = h.state().type_dialog.as_ref().expect("still open");
    assert!(d.error.as_deref().is_some_and(|e| e.contains("bridging")), "{:?}", d.error);
    assert_eq!(framing_type(&h, "2x4 @ 406"), now);
    press(&mut h, "Cancel");
}

#[test]
fn visibility_graphics_has_floors_framing_and_framing_lines() {
    let mut h = app(FLOOR);
    shortcut(&mut h, "VV");
    h.get_by_label("Floors: Unfold").scroll_to_me();
    h.run();
    press(&mut h, "Floors: Unfold");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Framing").is_some());
    assert!(h.query_by_role_and_label(Role::CheckBox, "Framing Lines").is_some());
}

#[test]
fn floor_framing_speaks_portuguese_and_spanish_and_leaves_no_english() {
    use crate::lang::Lang;
    use egui_kittest::kittest::{By, NodeT};
    let english = |h: &Harness<BimApp>, l: Lang| -> Vec<String> {
        h.query_all(By::new())
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .filter(|s| crate::lang::translated_in(l, s).is_some_and(|x| x != s))
            .collect()
    };
    for (l, framing, direction) in
        [(Lang::Spanish, "Entramado", "Dirección de viguetas (°)"), (Lang::Portuguese, "Estrutura de madeira", "Direção das vigotas (°)")]
    {
        let mut h = app(&with_type());
        h.state_mut().lang = l;
        h.run();
        let floor = ids(&h, "floor")[0];
        select(&mut h, &[floor]);
        choose(&mut h, framing, "2x4 @ 406");
        assert!(h.query_by_label(direction).is_some(), "{l:?}");
        let mut missed = english(&h, l);
        let edit = crate::lang::translated_in(l, "Edit Framing Type").unwrap_or("Edit Framing Type");
        h.get_by_label(edit).scroll_to_me();
        h.run();
        press(&mut h, edit);
        assert!(h.state().type_dialog.is_some(), "{l:?}");
        missed.extend(english(&h, l));
        for label in [
            "Floors",
            "Floors and Roofs",
            "Rim Joists and Eave Boards",
            "Lap at Supports",
            "Bridging Spacing",
            "Trimmer and Header Plies",
            "Joists Under Partitions",
        ] {
            assert!(crate::lang::translated_in(l, label).is_some_and(|x| x != label), "{l:?}: {label}");
        }
        missed.sort();
        missed.dedup();
        assert!(missed.is_empty(), "{l:?} left in English: {missed:?}");
    }
}

/// What floor framing looks like in the window, for a person to look at: `BIMCRAFT_FRAMING_DUMP=<folder>` writes the Structure
/// tab with a framed floor selected, its Properties and its framing type's Type Properties, light and dark.
#[test]
fn floor_framing_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FRAMING_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let save = |h: &mut Harness<BimApp>, name: &str| {
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("ui_floor_{name}_{look}.png"))).unwrap();
        };
        let mut h = app(&with_type());
        let floor = ids(&h, "floor")[0];
        select(&mut h, &[floor]);
        press(&mut h, "Structure");
        press(&mut h, "Frame");
        type_into(&mut h, DIRECTION, "90");
        key(&mut h, Key::Enter);
        save(&mut h, "structure");
        h.get_by_label("Edit Framing Type").scroll_to_me();
        save(&mut h, "properties");
        press(&mut h, "Edit Framing Type");
        h.get_all_by_label("Floors").last().unwrap().scroll_to_me();
        save(&mut h, "typeprops");
    }
}
