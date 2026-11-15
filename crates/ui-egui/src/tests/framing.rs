//! Wood framing in the window: Structure > Frame and Remove Framing on the selected walls, a wall's Framing in Properties, a
//! framing type's Type Properties, the Project Browser's framing types, and the schedule of their members.

use super::*;

/// A 2x4 wall type, one 89 mm structure layer of timber, and a box of walls of it.
const STUDS: &str = "type {\"category\": \"wall\", \"name\": \"2x4\", \"layers\": [{\"function\": \"structure\", \"material\": \"Timber\", \"thickness\": 89}]}\n\
     wall {\"points\": [[0,0],[8000,0],[8000,6000],[0,6000]], \"closed\": true, \"type\": \"2x4\"}";

/// The box with the default framing type in the project.
fn with_type() -> String {
    format!("{STUDS}\ntype {{\"category\": \"framing\", \"name\": \"2x4 @ 406\"}}")
}

/// The framing type's Type Properties, from the button beside the wall's Framing in Properties.
fn edit_framing_type(h: &mut Harness<BimApp>) {
    h.get_by_label("Edit Framing Type").scroll_to_me();
    h.run();
    press(h, "Edit Framing Type");
}

/// Structure > `label`.
fn structural(h: &mut Harness<BimApp>, label: &str) {
    press(h, "Structure");
    press(h, label);
}

fn select(h: &mut Harness<BimApp>, ids: &[Id]) {
    h.state_mut().selection = ids.to_vec();
    h.run();
}

fn framing(h: &Harness<BimApp>, id: Id) -> Value {
    field(h, id, "framing")
}

fn framing_type(h: &Harness<BimApp>, name: &str) -> Value {
    json!(h.state().session.model.types.framing.get(name))
}

#[test]
fn frame_and_remove_framing_act_on_the_selected_walls_each_as_one_step() {
    let mut h = app(STUDS);
    let walls = ids(&h, "wall");
    select(&mut h, &walls[..2]);
    let steps = h.state().session.history().len();
    structural(&mut h, "Frame");
    assert_eq!(
        (framing(&h, walls[0]), framing(&h, walls[1]), framing(&h, walls[2])),
        (json!("2x4 @ 406"), json!("2x4 @ 406"), Value::Null)
    );
    assert!(h.state().session.model.types.framing.contains_key("2x4 @ 406"), "the project's first framing type, made");
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    let said = &h.state().status;
    assert!(!said.error && said.text.contains("2 walls"), "{}", said.text);
    // Undone, the walls are as they were, the type gone with them.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(framing(&h, walls[0]), Value::Null);
    assert!(h.state().session.model.types.framing.is_empty());
    // A wall framed, the other selected with it: Remove Framing takes it off, in a step.
    structural(&mut h, "Frame");
    let steps = h.state().session.history().len();
    structural(&mut h, "Remove Framing");
    assert_eq!((framing(&h, walls[0]), framing(&h, walls[1])), (Value::Null, Value::Null));
    assert_eq!(h.state().session.history().len(), steps + 1);
    assert!(h.state().session.model.types.framing.contains_key("2x4 @ 406"), "the type stays in the project");
}

#[test]
fn frame_wants_walls_selected_and_says_why_the_project_refuses_one() {
    let mut h = app(STUDS);
    let steps = h.state().session.history().len();
    for label in ["Frame", "Remove Framing"] {
        structural(&mut h, label);
        let said = &h.state().status;
        assert!(said.error && said.text.contains("Select"), "{label}: {}", said.text);
    }
    assert_eq!(h.state().session.history().len(), steps, "nothing changed");
    // Walls among other things: the walls are framed, the rest left as they are.
    let mut h = app(&format!("{STUDS}\nstructural_column {{\"at\": [3000, 3000]}}"));
    let (walls, column) = (ids(&h, "wall"), ids(&h, "structuralcolumn")[0]);
    select(&mut h, &[walls[0], column]);
    structural(&mut h, "Frame");
    assert_eq!(framing(&h, walls[0]), json!("2x4 @ 406"));
    // A wall of a type with no structure layer: the project says why, and nothing changes.
    let mut h = app(BOX);
    let wall = ids(&h, "wall")[0];
    select(&mut h, &[wall]);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("structure layer"), "{}", said.text);
    assert_eq!(framing(&h, wall), Value::Null);
    assert!(h.state().session.model.types.framing.is_empty());
}

#[test]
fn with_several_framing_types_frame_drops_them_down() {
    let mut h = app(&format!("{}\ntype {{\"category\": \"framing\", \"name\": \"Partition\", \"spacing\": 609.6}}", with_type()));
    let walls = ids(&h, "wall");
    select(&mut h, &walls[1..3]);
    structural(&mut h, "Frame");
    assert_eq!(framing(&h, walls[1]), Value::Null, "the button drops the types down");
    press(&mut h, "Partition");
    assert_eq!((framing(&h, walls[1]), framing(&h, walls[2])), (json!("Partition"), json!("Partition")));
}

#[test]
fn a_walls_framing_is_picked_in_properties_and_its_type_edited_from_there() {
    let mut h = app(&with_type());
    click(&mut h, P2(4000.0, 0.0));
    let wall = h.state().selection[0];
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Framing").value().as_deref(), Some("None"));
    choose(&mut h, "Framing", "2x4 @ 406");
    assert_eq!(framing(&h, wall), json!("2x4 @ 406"));
    // Edit Framing Type: its Type Properties, a copy made and edited, and OK puts the wall on it.
    edit_framing_type(&mut h);
    assert!(h.state().type_dialog.is_some());
    press(&mut h, "Duplicate…");
    type_into(&mut h, "New type", "Partition");
    press(&mut h, "Create");
    type_into(&mut h, "Spacing", "609.6");
    type_into(&mut h, "Top Plates", "1");
    choose(&mut h, "Corner", "Two Studs");
    type_into(&mut h, "Stock Lengths", "2400, 3600");
    press(&mut h, "OK");
    assert!(h.state().type_dialog.is_none());
    assert_eq!(framing(&h, wall), json!("Partition"));
    let made = framing_type(&h, "Partition");
    assert_eq!((&made["spacing"], &made["top_plates"], &made["corner"]), (&json!(609.6), &json!(1), &json!("two_stud")));
    assert_eq!(made["stock"], json!([2400.0, 3600.0]));
    assert_eq!(made["headers"], framing_type(&h, "2x4 @ 406")["headers"], "the copy has the headers of the type it copies");
    assert_eq!(framing_type(&h, "2x4 @ 406")["spacing"], json!(406.4), "the type it copies is as it was");
    // None takes the framing off.
    choose(&mut h, "Framing", "None");
    assert_eq!(framing(&h, wall), Value::Null);
}

#[test]
fn a_framing_types_properties_say_what_they_cannot_take_and_change_nothing() {
    let mut h = app(&with_type());
    click(&mut h, P2(4000.0, 0.0));
    choose(&mut h, "Framing", "2x4 @ 406");
    edit_framing_type(&mut h);
    let was = framing_type(&h, "2x4 @ 406");
    // Apply with nothing changed changes nothing: 406.4 stays, whatever the field shows of it.
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(framing_type(&h, "2x4 @ 406"), was);
    type_into(&mut h, "Kings", "two");
    press_near(&mut h, "Apply", "Cancel");
    let d = h.state().type_dialog.as_ref().expect("a refusal keeps the dialog open");
    assert!(d.error.as_deref().is_some_and(|e| e.contains("Kings")), "{:?}", d.error);
    // The project's own rules are said in the dialog too.
    type_into(&mut h, "Kings", "9");
    press_near(&mut h, "Apply", "Cancel");
    let d = h.state().type_dialog.as_ref().expect("still open");
    assert!(d.error.as_deref().is_some_and(|e| e.contains("kings")), "{:?}", d.error);
    assert_eq!(framing_type(&h, "2x4 @ 406"), was);
    // Rename follows the wall.
    type_into(&mut h, "Kings", "1");
    press(&mut h, "Rename…");
    type_into(&mut h, "New name", "Bearing");
    press(&mut h, "Rename");
    press(&mut h, "Cancel");
    let wall = h.state().selection[0];
    assert_eq!(framing(&h, wall), json!("Bearing"));
}

#[test]
fn the_project_browser_lists_framing_types_opens_one_and_deletes_one_unused() {
    let mut h = app(&format!("{}\ntype {{\"category\": \"framing\", \"name\": \"Partition\"}}", with_type()));
    type_into(&mut h, "Search the Project Browser", "Partition");
    assert!(h.query_by_label("Framing Types").is_some(), "under Families, a group of their own");
    h.get_by_label("Partition").click_accesskit();
    h.run();
    assert!(h.state().type_dialog.is_some(), "a framing type opens its Type Properties");
    press(&mut h, "Cancel");
    let at = h.get_by_label("Partition").rect().center();
    right_click_at(&mut h, at);
    press(&mut h, "Delete");
    assert!(!h.state().session.model.types.framing.contains_key("Partition"));
}

#[test]
fn framing_members_are_a_schedule_category_and_framing_a_walls_subcategory() {
    let mut h = app(STUDS);
    press(&mut h, "View");
    press(&mut h, "Schedules");
    press(&mut h, "Schedule/Quantities…");
    h.get_by_role_and_label(Role::Button, "Framing Members").click_accesskit();
    h.run();
    press(&mut h, "OK");
    assert_eq!(ids(&h, "schedule").len(), 1);
    for label in ["Role", "Section", "Stock Length", "Framing Type"] {
        assert!(h.query_all_by_label(label).next().is_some(), "the schedule's fields: {label}");
    }
    // Visibility/Graphics: Walls > Framing.
    let mut h = app(STUDS);
    shortcut(&mut h, "VV");
    h.get_by_label("Walls: Unfold").scroll_to_me();
    h.run();
    press(&mut h, "Walls: Unfold");
    assert!(h.query_by_role_and_label(Role::CheckBox, "Framing").is_some());
}

#[test]
fn the_tools_palette_has_frame_in_bimcrafts_own_profile() {
    let mut h = bimcraft(STUDS);
    let walls = ids(&h, "wall");
    select(&mut h, &walls[..1]);
    let structure = h.get_all_by_label("Structure").map(|n| n.rect()).find(|r| r.left() < 60.0).unwrap();
    click_at(&mut h, structure.center());
    h.get_by_label("Frame").scroll_to_me();
    h.run();
    press(&mut h, "Frame");
    assert_eq!(framing(&h, walls[0]), json!("2x4 @ 406"), "{} {:?}", h.state().status.text, h.state().selection);
}

#[test]
fn find_has_frame_and_remove_framing() {
    let mut h = app(STUDS);
    let listed: Vec<String> = h.state().find_matches("fram").into_iter().map(|(l, ..)| l).collect();
    assert!(listed.contains(&"Frame".to_string()) && listed.contains(&"Remove Framing".to_string()), "{listed:?}");
    let walls = ids(&h, "wall");
    select(&mut h, &walls[..1]);
    assert!(h.state_mut().execute_shortcut_action("Frame"));
    assert_eq!(framing(&h, walls[0]), json!("2x4 @ 406"));
    assert!(h.state_mut().execute_shortcut_action("Remove Framing"));
    assert_eq!(framing(&h, walls[0]), Value::Null);
}

#[test]
fn framing_speaks_portuguese_and_spanish_and_leaves_no_english() {
    use crate::lang::Lang;
    use egui_kittest::kittest::{By, NodeT};
    let english = |h: &Harness<BimApp>, l: Lang| -> Vec<String> {
        h.query_all(By::new())
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .filter(|s| crate::lang::translated_in(l, s).is_some_and(|x| x != s))
            .collect()
    };
    for (l, tab, frame, remove, framing) in [
        (Lang::Spanish, "Estructura", "Entramar", "Quitar entramado", "Entramado"),
        (Lang::Portuguese, "Estrutura", "Estruturar", "Remover estrutura de madeira", "Estrutura de madeira"),
    ] {
        let mut h = app(&with_type());
        h.state_mut().lang = l;
        h.run();
        press(&mut h, tab);
        assert!(h.query_by_label(frame).is_some() && h.query_by_label(remove).is_some(), "{l:?}");
        let mut missed = english(&h, l);
        // A framed wall's Properties, and its framing type's Type Properties.
        click(&mut h, P2(4000.0, 0.0));
        choose(&mut h, framing, "2x4 @ 406");
        missed.extend(english(&h, l));
        let edit = crate::lang::translated_in(l, "Edit Framing Type").unwrap_or("Edit Framing Type");
        h.get_by_label(edit).scroll_to_me();
        h.run();
        press(&mut h, edit);
        assert!(h.state().type_dialog.is_some(), "{l:?}");
        missed.extend(english(&h, l));
        missed.sort();
        missed.dedup();
        assert!(missed.is_empty(), "{l:?} left in English: {missed:?}");
    }
}

/// What framing looks like in the window, for a person to look at: `BIMCRAFT_FRAMING_DUMP=<folder>` writes the Structure tab with
/// the Framing panel, a framed wall's Properties and its framing type's Type Properties, light and dark, and the Tools palette.
#[test]
fn framing_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FRAMING_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let save = |h: &mut Harness<BimApp>, name: &str| {
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("ui_{name}_{look}.png"))).unwrap();
        };
        let mut h = app(&with_type());
        let walls = ids(&h, "wall");
        select(&mut h, &walls);
        press(&mut h, "Structure");
        press(&mut h, "Frame");
        save(&mut h, "structure");
        click(&mut h, P2(4000.0, 0.0));
        press(&mut h, "Structure");
        h.get_by_label("Edit Framing Type").scroll_to_me();
        save(&mut h, "properties");
        edit_framing_type(&mut h);
        save(&mut h, "typeprops");
        let mut h = bimcraft(&with_type());
        let structure = h.get_all_by_label("Structure").map(|n| n.rect()).find(|r| r.left() < 60.0).unwrap();
        click_at(&mut h, structure.center());
        save(&mut h, "tools");
    }
}

#[test]
fn frame_says_which_openings_it_couldnt_frame_as_the_type_says() {
    // The default 1200 x 1500 window at 900 in a wall 2440 tall: its head reaches the top plates.
    let mut h = app(&format!(
        "{STUDS}\nwall {{\"points\": [[0,8000],[6000,8000]], \"type\": \"2x4\", \"height\": 2440}}\nwindow {{\"at\": [3000, 8000]}}"
    ));
    let walls = ids(&h, "wall");
    let last = *walls.last().unwrap();
    select(&mut h, &[last]);
    structural(&mut h, "Frame");
    let said = &h.state().status;
    assert!(said.error && said.text.contains("Warnings (1):") && said.text.contains("top plates"), "{}", said.text);
}
