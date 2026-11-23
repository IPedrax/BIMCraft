//! Roof framing in the window: Structure > Frame and Remove Framing on the selected roofs with the walls and floors, a roof's
//! Framing in Properties, a framing type's roof fields and the fields floors and roofs share in Type Properties, and Roofs >
//! Framing and Framing Lines in Visibility/Graphics.

use super::*;

/// A 2x4 wall type and a wall of it; a 2x10 roof type (11 mm of sheathing over a 235 mm structure layer) and a gable roof of
/// it, 6000 by 4000, sloping 30° to eaves along y 0 and y 4000 from a ridge along y 2000, on two beams near its eaves whose
/// tops are at its rafters' underside on their centre lines.
const ROOF: &str = "type {\"category\": \"wall\", \"name\": \"2x4\", \"layers\": [{\"function\": \"structure\", \"material\": \"Timber\", \"thickness\": 89}]}\n\
     wall {\"points\": [[0,8000],[8000,8000]], \"type\": \"2x4\"}\n\
     type {\"category\": \"roof\", \"name\": \"2x10 Roof\", \"layers\": [{\"function\": \"substrate\", \"material\": \"Timber\", \"thickness\": 11}, {\"function\": \"structure\", \"material\": \"Timber\", \"thickness\": 235}]}\n\
     type {\"category\": \"beam\", \"name\": \"3-2x10\", \"width\": 114, \"depth\": 235}\n\
     beam {\"points\": [[0,300],[6000,300]], \"type\": \"3-2x10\", \"offset\": 2573.205}\n\
     beam {\"points\": [[0,3700],[6000,3700]], \"type\": \"3-2x10\", \"offset\": 2573.205}\n\
     roof {\"boundary\": [[0,0],[6000,0],[6000,4000],[0,4000]], \"level\": \"Level 1\", \"offset\": 2400, \"slope\": 30, \"gables\": [1, 3], \"type\": \"2x10 Roof\"}";

/// The roof with the default framing type in the project.
fn with_type() -> String {
    format!("{ROOF}\ntype {{\"category\": \"framing\", \"name\": \"2x4 @ 406\"}}")
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

#[test]
fn frame_and_remove_framing_act_on_the_selected_roofs_with_the_walls_and_floors_each_as_one_step() {
    let mut h = app(ROOF);
    let (wall, roof) = (ids(&h, "wall")[0], ids(&h, "roof")[0]);
    select(&mut h, &[wall, roof]);
    let steps = h.state().session.history().len();
    structural(&mut h, "Frame");
    assert_eq!((field(&h, wall, "framing"), field(&h, roof, "framing")), (json!("2x4 @ 406"), json!("2x4 @ 406")));
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    let said = &h.state().status;
    assert!(!said.error && said.text.contains("1 walls and 1 roofs framed as 2x4 @ 406"), "{}", said.text);
    // A roof alone says so.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(field(&h, roof, "framing"), Value::Null);
    select(&mut h, &[roof]);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(!said.error && said.text.contains("1 roofs framed as 2x4 @ 406"), "{}", said.text);
    // Remove Framing takes it off the walls and roofs selected, in a step.
    select(&mut h, &[wall, roof]);
    structural(&mut h, "Frame");
    let steps = h.state().session.history().len();
    structural(&mut h, "Remove Framing");
    assert_eq!((field(&h, wall, "framing"), field(&h, roof, "framing")), (Value::Null, Value::Null));
    assert_eq!(h.state().session.history().len(), steps + 1);
    // With nothing to frame selected, it says what to select.
    let beams = ids(&h, "beam");
    select(&mut h, &beams);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("walls, floors or roofs"), "{}", said.text);
    structural(&mut h, "Remove Framing");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("walls, floors or roofs"), "{}", said.text);
    // Find runs the same on a roof.
    select(&mut h, &[roof]);
    let listed: Vec<String> = h.state().find_matches("fram").into_iter().map(|(l, ..)| l).collect();
    assert!(listed.contains(&"Frame".to_string()) && listed.contains(&"Remove Framing".to_string()), "{listed:?}");
    assert!(h.state_mut().execute_shortcut_action("Frame"));
    assert_eq!(field(&h, roof, "framing"), json!("2x4 @ 406"));
    assert!(h.state_mut().execute_shortcut_action("Remove Framing"));
    assert_eq!(field(&h, roof, "framing"), Value::Null);
}

#[test]
fn frame_counts_walls_floors_and_roofs_together() {
    let script = format!(
        "{ROOF}\ntype {{\"category\": \"floor\", \"name\": \"2x8 Floor\", \"layers\": [{{\"function\": \"structure\", \"material\": \"Timber\", \"thickness\": 184}}]}}\n\
         floor {{\"boundary\": [[10000,0],[12000,0],[12000,2000],[10000,2000]], \"type\": \"2x8 Floor\"}}"
    );
    let mut h = app(&script);
    let (wall, floor, roof) = (ids(&h, "wall")[0], ids(&h, "floor")[0], ids(&h, "roof")[0]);
    select(&mut h, &[floor, roof]);
    structural(&mut h, "Frame");
    assert!(h.state().status.text.contains("1 floors and 1 roofs framed as"), "{}", h.state().status.text);
    select(&mut h, &[wall, floor, roof]);
    structural(&mut h, "Frame");
    assert!(h.state().status.text.contains("1 walls, 1 floors and 1 roofs framed as"), "{}", h.state().status.text);
}

#[test]
fn frame_says_in_the_status_bar_what_a_roof_couldnt_be_framed_as_the_type_says_and_why_one_cant_be() {
    // No beams under it: nothing bears its rafters.
    let script: String = ROOF.lines().filter(|l| !l.trim_start().starts_with("beam ")).collect::<Vec<_>>().join("\n");
    let mut h = app(&script);
    let roof = ids(&h, "roof")[0];
    select(&mut h, &[roof]);
    structural(&mut h, "Frame");
    assert_eq!(field(&h, roof, "framing"), json!("2x4 @ 406"), "framed all the same");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("Warnings ("), "{}", said.text);
    assert!(said.text.contains("nothing bears its rafters"), "{}", said.text);
    // A roof the project can't frame, by extrusion: it says why, and nothing changes.
    let mut h = app(&format!(
        "{script}\nroof {{\"extrusion\": {{\"line\": [[20000,0],[26000,0]], \"profile\": [[0,0],[2000,1000],[4000,0]], \"to\": 4000}}, \
         \"level\": \"Level 1\", \"offset\": 2400, \"type\": \"2x10 Roof\"}}"
    ));
    let extruded = ids(&h, "roof")[1];
    select(&mut h, &[extruded]);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("only a roof by footprint is framed"), "{}", said.text);
    assert_eq!(field(&h, extruded, "framing"), Value::Null);
}

#[test]
fn a_roofs_framing_is_set_in_properties_and_its_type_edited_from_there() {
    let mut h = app(&with_type());
    let roof = ids(&h, "roof")[0];
    select(&mut h, &[roof]);
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Framing").value().as_deref(), Some("None"));
    let steps = h.state().session.history().len();
    choose(&mut h, "Framing", "2x4 @ 406");
    assert_eq!(field(&h, roof, "framing"), json!("2x4 @ 406"), "{}", h.state().status.text);
    assert_eq!(h.state().session.history().len(), steps + 1);
    // A roof's rafters run up its slopes: no direction to give.
    assert!(h.query_by_label("Joist Direction (°)").is_none());
    h.get_by_label("Edit Framing Type").scroll_to_me();
    h.run();
    press(&mut h, "Edit Framing Type");
    assert!(h.state().type_dialog.is_some());
    press(&mut h, "Cancel");
    choose(&mut h, "Framing", "None");
    assert_eq!(field(&h, roof, "framing"), Value::Null);
}

#[test]
fn a_framing_types_roof_fields_are_under_roofs_and_those_floors_and_roofs_share_under_both() {
    let mut h = app(&with_type());
    let roof = ids(&h, "roof")[0];
    select(&mut h, &[roof]);
    choose(&mut h, "Framing", "2x4 @ 406");
    h.get_by_label("Edit Framing Type").scroll_to_me();
    h.run();
    press(&mut h, "Edit Framing Type");
    for group in ["Walls", "Wall Openings", "Wall Joins", "Floors and Roofs", "Floors", "Roofs"] {
        assert!(h.query_all_by_label(group).next().is_some(), "the group {group}");
    }
    let was = framing_type(&h, "2x4 @ 406");
    assert_eq!((&was["ridge"], &was["rim"]), (&json!(true), &json!(true)));
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(framing_type(&h, "2x4 @ 406"), was, "Apply with nothing changed changes nothing");
    for label in ["Ridge Board", "Rim Joists and Eave Boards"] {
        h.get_by_role_and_label(Role::CheckBox, label).scroll_to_me();
        h.run();
        pick(&mut h, Role::CheckBox, label);
    }
    type_into(&mut h, "Lap at Supports", "230");
    press_near(&mut h, "Apply", "Cancel");
    let now = framing_type(&h, "2x4 @ 406");
    assert_eq!(
        (&now["ridge"], &now["rim"], &now["lap"]),
        (&json!(false), &json!(false), &json!(230.0)),
        "{:?}",
        h.state().type_dialog.as_ref().and_then(|d| d.error.clone())
    );
    press(&mut h, "Cancel");
    // Without a ridge board, the roof's ridge is said to be left to frame.
    let r = h.state_mut().session.execute("framing", &json!({"id": roof})).unwrap();
    assert!(r["members"].as_array().into_iter().flatten().all(|m| m["role"] != "ridge"));
    assert!(r["warnings"].as_array().into_iter().flatten().any(|w| w.as_str().is_some_and(|w| w.contains("no ridge board"))), "{r}");
}

#[test]
fn visibility_graphics_has_roofs_framing_and_framing_lines() {
    let mut h = app(ROOF);
    shortcut(&mut h, "VV");
    h.get_by_label("Roofs: Unfold").scroll_to_me();
    h.run();
    press(&mut h, "Roofs: Unfold");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Framing").is_some());
    assert!(h.query_by_role_and_label(Role::CheckBox, "Framing Lines").is_some());
}

#[test]
fn roof_framing_speaks_portuguese_and_spanish_and_leaves_no_english() {
    use crate::lang::Lang;
    use egui_kittest::kittest::{By, NodeT};
    let english = |h: &Harness<BimApp>, l: Lang| -> Vec<String> {
        h.query_all(By::new())
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .filter(|s| crate::lang::translated_in(l, s).is_some_and(|x| x != s))
            .collect()
    };
    for (l, framing, ridge, rim, shared) in [
        (Lang::Spanish, "Entramado", "Cumbrera", "Viguetas de borde y tablas de alero", "Pisos y cubiertas"),
        (Lang::Portuguese, "Estrutura de madeira", "Cumeeira", "Vigas de borda e tábuas de beiral", "Pisos e telhados"),
    ] {
        let mut h = app(&with_type());
        h.state_mut().lang = l;
        h.run();
        let roof = ids(&h, "roof")[0];
        select(&mut h, &[roof]);
        choose(&mut h, framing, "2x4 @ 406");
        let mut missed = english(&h, l);
        let edit = crate::lang::translated_in(l, "Edit Framing Type").unwrap_or("Edit Framing Type");
        h.get_by_label(edit).scroll_to_me();
        h.run();
        press(&mut h, edit);
        assert!(h.state().type_dialog.is_some(), "{l:?}");
        for label in [ridge, rim, shared] {
            assert!(h.query_all_by_label(label).next().is_some(), "{l:?}: {label}");
        }
        missed.extend(english(&h, l));
        for label in [
            "Roofs",
            "Floors and Roofs",
            "Ridge Board",
            "Rim Joists and Eave Boards",
            "Select the walls, floors or roofs to frame first.",
            "Select the walls, floors or roofs to remove framing from first.",
            "{} roofs framed as {}: {} members.",
            "{} walls and {} roofs framed as {}: {} members.",
            "{} floors and {} roofs framed as {}: {} members.",
            "{} walls, {} floors and {} roofs framed as {}: {} members.",
        ] {
            assert!(crate::lang::translated_in(l, label).is_some_and(|x| x != label), "{l:?}: {label}");
        }
        missed.sort();
        missed.dedup();
        assert!(missed.is_empty(), "{l:?} left in English: {missed:?}");
    }
}

/// What roof framing looks like in the window, for a person to look at: `BIMCRAFT_FRAMING_DUMP=<folder>` writes the Structure
/// tab with a framed roof selected, its Properties and its framing type's Type Properties, light and dark.
#[test]
fn roof_framing_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FRAMING_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let save = |h: &mut Harness<BimApp>, name: &str| {
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("ui_roof_{name}_{look}.png"))).unwrap();
        };
        let mut h = app(&with_type());
        let roof = ids(&h, "roof")[0];
        select(&mut h, &[roof]);
        press(&mut h, "Structure");
        press(&mut h, "Frame");
        save(&mut h, "structure");
        h.get_by_label("Edit Framing Type").scroll_to_me();
        save(&mut h, "properties");
        press(&mut h, "Edit Framing Type");
        h.get_all_by_label("Roofs").last().unwrap().scroll_to_me();
        save(&mut h, "typeprops");
    }
}
