//! BIMCraft's Tools palette as the user has it: the project's scale, the tools taken out of it (Edit Tools), and named presets of
//! all of it with where the palettes are.

use egui_kittest::kittest::NodeT;

use super::*;
use crate::dock::{Palette, Side, Target};
use crate::presets::Scale;

/// A settings file of a test's own, in a folder of its own.
fn settings_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("settings.json")
}

fn kept(file: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

/// Whether the window offers the tool named `tool` of the panel named `panel`: its button or row, or (on the ribbon, folded to fit)
/// its panel's name.
fn offered(h: &Harness<BimApp>, tool: &str, panel: &str) -> bool {
    h.query_all_by_label_contains(tool).next().is_some() || h.query_all_by_label_contains(panel).next().is_some()
}

#[test]
fn the_project_scale_leaves_out_what_a_project_does_not_need_yet_in_both_profiles_and_is_switched_at_any_time() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        assert_eq!(h.state().scale, Scale::Team, "everything to begin with, so nobody loses a tool");
        press(&mut h, "Manage");
        assert!(offered(&h, "Design Options", "Design Options"), "rev {rev}");
        // The chip on the title bar, at any time: nothing in the project changes.
        let before = h.state().session.model.clone();
        press(&mut h, "Team");
        pick(&mut h, Role::RadioButton, "Studio");
        assert!(!offered(&h, "Design Options", "Design Options"), "a team's tools wait for Team (rev {rev})");
        press(&mut h, "View");
        assert!(offered(&h, "Place View", "Sheet Composition"), "sheets from Studio up (rev {rev})");
        press(&mut h, "Studio");
        pick(&mut h, Role::RadioButton, "Sketch");
        assert!(!offered(&h, "Place View", "Sheet Composition"), "rev {rev}");
        assert!(Arc::ptr_eq(&before, &h.state().session.model), "the project is as it was");
    }
}

#[test]
fn revs_ribbon_takes_tools_out_as_the_tools_palette_does_and_has_find() {
    let mut h = app(BOX);
    press(&mut h, "Team");
    press(&mut h, "Edit Tools");
    h.get_by_role_and_label(Role::CheckBox, "Window").click();
    h.run();
    press(&mut h, "Done");
    assert!(h.state().tools_hidden.contains("Window") && h.query_by_label("Window").is_none());
    press(&mut h, "Find");
    assert!(h.state().find.is_some(), "Find on Rev's title bar too");
}

#[test]
fn edit_tools_takes_a_tool_out_of_the_palette_and_puts_it_back_and_the_settings_file_keeps_it() {
    let file = settings_file("edit-tools");
    let mut h = bimcraft(BOX);
    h.state_mut().use_settings(file.clone());
    h.run();
    press(&mut h, "Edit Tools");
    let hide = h.get_by_label("Temporary Hide/Isolate").accesskit_node().role();
    assert_eq!(hide, Role::Button, "the view's own buttons stay buttons: Edit Tools is the tools'");
    // Each row is a box to tick: what it shows.
    let door = h.get_by_role_and_label(Role::CheckBox, "Door");
    assert!(door.accesskit_node().toggled().is_some_and(|t| t == egui::accesskit::Toggled::True));
    door.click();
    h.run();
    press(&mut h, "Done");
    assert!(h.query_by_label("Door").is_none(), "out of the palette");
    assert!(h.query_by_label("Window").is_some(), "and nothing else");
    assert_eq!(kept(&file)["tools_hidden"], json!(["Door"]));
    // Find still has it.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.event(egui::Event::Text("door".to_string()));
    h.run();
    key(&mut h, Key::Enter);
    assert!(matches!(h.state().tool, Tool::Door));
    key(&mut h, Key::Escape);
    // Back in.
    press(&mut h, "Edit Tools");
    h.get_by_role_and_label(Role::CheckBox, "Door").click();
    h.run();
    press(&mut h, "Done");
    assert!(h.query_by_label("Door").is_some());
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_preset_keeps_the_palettes_the_tools_and_the_scale_and_brings_them_back() {
    let file = settings_file("presets");
    let mut h = bimcraft(BOX);
    h.state_mut().use_settings(file.clone());
    h.state_mut().scale = Scale::Team;
    h.state_mut().tools_hidden.insert("Window".to_string());
    h.state_mut().docking.put(Palette::Tools, Target::Side(Side::Right));
    h.run();
    press(&mut h, "Settings");
    type_into(&mut h, "Preset name:", "Documentation");
    press(&mut h, "Save Preset");
    press(&mut h, "Close");
    assert_eq!(kept(&file)["presets"][0]["name"], "Documentation");
    // Changed meanwhile, then the preset from the title bar's chip.
    h.state_mut().scale = Scale::Sketch;
    h.state_mut().tools_hidden.clear();
    h.state_mut().docking = crate::dock::Docking::default();
    h.run();
    press(&mut h, "Sketch");
    press(&mut h, "Documentation");
    let s = h.state();
    assert_eq!((s.scale, s.docking.docked(Palette::Tools)), (Scale::Team, Some(Side::Right)));
    assert!(s.tools_hidden.contains("Window"));
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}
