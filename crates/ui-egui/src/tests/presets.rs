//! BIMCraft's Tools palette as the user has it: the project's scale, the tools taken out of it (Edit Tools), and named presets of
//! all of it with where the palettes are.

use egui_kittest::kittest::NodeT;

use super::*;
use crate::dock::{Docking, Palette, Side, Target};
use crate::presets::{MOST_PANELS, PANEL_KEY, Preset, Scale, panels_from_json, presets_from_json};
use crate::style::Profile;
use std::collections::BTreeSet;

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

#[test]
fn panels_hidden_reads_back_as_written_and_a_hostile_list_reads_as_sense() {
    let written = json!(["tab:View/Phasing", "tab:plugin:Areas/Rooms"]);
    assert_eq!(panels_from_json(&written), BTreeSet::from(["tab:View/Phasing".to_string(), "tab:plugin:Areas/Rooms".to_string()]));
    let long = format!("tab:View/{}", "x".repeat(PANEL_KEY));
    let hostile = json!(["", "nonsense", "tab:", long, 7, "tab:View/Phasing"]);
    assert_eq!(panels_from_json(&hostile), BTreeSet::from(["tab:View/Phasing".to_string()]));
    let many = Value::Array((0..1000).map(|i| json!(format!("tab:View/P{i}"))).collect());
    assert_eq!(panels_from_json(&many).len(), MOST_PANELS);
}

#[test]
fn a_preset_without_panels_reads_with_none_hidden() {
    let kept = presets_from_json(&json!([{"name": "Old"}]));
    let p = &kept[0];
    assert!(p.panels_hidden.is_empty());
    assert!(p.hidden.is_empty());
    assert_eq!(p.profile, None, "an old preset leaves the window's profile alone");
}

#[test]
fn a_preset_profile_reads_back_and_an_old_preset_leaves_the_profile_alone() {
    let mut p = Preset {
        name: "Documentation".into(),
        docking: Docking::revit(),
        panels_hidden: BTreeSet::from(["tab:View/Phasing".to_string()]),
        hidden: BTreeSet::new(),
        profile: Some(Profile::Rev),
        scale: Scale::Team,
        compact: false,
    };
    let kept = presets_from_json(&json!([p.to_json()]));
    let back = &kept[0];
    assert_eq!(back.profile, Some(Profile::Rev));
    assert_eq!(back.panels_hidden, p.panels_hidden);
    p.profile = None;
    let kept = presets_from_json(&json!([p.to_json()]));
    let back = &kept[0];
    assert_eq!(back.profile, None);
    assert!(!p.to_json().as_object().unwrap().contains_key("profile"), "none is not written");
}

#[test]
fn resetting_a_preset_gives_the_standard_layout_of_its_profile_and_keeps_it() {
    let mut p = Preset {
        name: "Doc".into(),
        docking: Docking::revit(),
        panels_hidden: BTreeSet::from(["tab:View/Phasing".to_string()]),
        hidden: BTreeSet::from(["Door".to_string()]),
        profile: Some(Profile::Rev),
        scale: Scale::Sketch,
        compact: true,
    };
    p.reset_to_standard(Profile::BimCraft);
    assert_eq!(p.docking, Docking::revit(), "the standard view of its own profile, not the window's");
    assert_eq!(p.profile, Some(Profile::Rev));
    assert!(p.panels_hidden.is_empty() && p.hidden.is_empty());
    assert_eq!((p.scale, p.compact), (Scale::Team, false));
    assert_eq!(p.name, "Doc");
    p.profile = Some(Profile::BimCraft);
    p.reset_to_standard(Profile::Rev);
    assert_eq!(p.docking, Docking::default());
    // An old preset has no profile to keep: the window's standard view is used and none is written.
    p.profile = None;
    p.reset_to_standard(Profile::Rev);
    assert_eq!(p.docking, Docking::revit());
    assert_eq!(p.profile, None);
}

#[test]
fn a_panel_taken_out_in_edit_tools_leaves_the_tab_and_comes_back() {
    for rev in [false, true] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        // Hide the panel with its box.
        if rev {
            press(&mut h, "Team");
        }
        press(&mut h, "Edit Tools");
        let node = h.get_by_role_and_label(Role::CheckBox, "Build");
        assert_eq!(node.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True), "rev {rev}: shown to begin with");
        node.scroll_to_me();
        h.run();
        let at = h.get_by_role_and_label(Role::CheckBox, "Build").rect().center();
        click_at(&mut h, at);
        press(&mut h, "Done");
        assert!(h.state().panels_hidden.contains("tab:Architecture/Build"), "rev {rev}");
        assert!(h.query_by_label("Wall").is_none(), "the panel and its tools left the tab (rev {rev})");
        // The same box brings it back. The ribbon scrolls sideways, where a pointer click on a hidden panel's box is
        // brittle; there the setting the box writes is put back, as the box does.
        if rev {
            h.state_mut().panels_hidden.remove("tab:Architecture/Build");
            h.run();
        } else {
            press(&mut h, "Edit Tools");
            let node = h.get_by_role_and_label(Role::CheckBox, "Build");
            assert_eq!(node.accesskit_node().toggled(), Some(egui::accesskit::Toggled::False), "the hidden panel's box is clear");
            node.scroll_to_me();
            h.run();
            let at = h.get_by_role_and_label(Role::CheckBox, "Build").rect().center();
            click_at(&mut h, at);
            press(&mut h, "Done");
        }
        assert!(!h.state().panels_hidden.contains("tab:Architecture/Build"), "rev {rev}");
        assert!(h.query_by_label("Wall").is_some(), "rev {rev}");
        // Hidden again for Find, set directly: the tool Find picks raises a contextual tab over the architecture one.
        h.state_mut().panels_hidden.insert("tab:Architecture/Build".to_string());
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::K);
        h.run();
        h.event(egui::Event::Text("wall".to_string()));
        h.run();
        key(&mut h, Key::Enter);
        assert!(matches!(h.state().tool, Tool::Wall(_)), "Find still has the tool (rev {rev})");
        key(&mut h, Key::Escape);
    }
}

#[test]
fn a_hidden_panel_is_still_listed_in_edit_tools_with_its_tools_tickable() {
    let mut h = bimcraft(BOX);
    h.state_mut().panels_hidden.insert("tab:Architecture/Build".to_string());
    h.run();
    press(&mut h, "Edit Tools");
    let panel = h.get_by_role_and_label(Role::CheckBox, "Build");
    assert_eq!(panel.accesskit_node().toggled(), Some(egui::accesskit::Toggled::False), "the panel is drawn, unticked");
    h.get_by_role_and_label(Role::CheckBox, "Wall").click();
    h.run();
    press(&mut h, "Done");
    assert!(h.state().tools_hidden.contains("Wall"), "its tools are ticked on their own");
    assert!(h.state().panels_hidden.contains("tab:Architecture/Build"));
}

#[test]
fn a_tab_whose_panels_are_all_hidden_says_so_and_the_button_brings_them_back() {
    let mut h = bimcraft(BOX);
    for panel in ["Select", "Build", "Circulation", "Room & Area", "Opening", "Datum"] {
        h.state_mut().panels_hidden.insert(format!("tab:Architecture/{panel}"));
    }
    h.run();
    assert!(h.query_by_label("Every panel of this tab is hidden.").is_some());
    h.get_by_label("Show the panels").scroll_to_me();
    h.run();
    let at = h.get_by_label("Show the panels").rect().center();
    click_at(&mut h, at);
    assert!(h.state().panels_hidden.iter().all(|k| !k.starts_with("tab:Architecture/")));
    assert!(h.query_by_label("Wall").is_some());
}

#[test]
fn a_preset_switches_palettes_panels_tools_and_scale_together() {
    let mut h = bimcraft(BOX);
    let a = Preset {
        name: "Design".into(),
        docking: Docking::default(),
        panels_hidden: BTreeSet::from(["tab:View/Phasing".to_string()]),
        hidden: BTreeSet::from(["Window".to_string()]),
        profile: Some(Profile::BimCraft),
        scale: Scale::Sketch,
        compact: false,
    };
    let mut b = a.clone();
    b.name = "Docs".into();
    b.docking = Docking::revit();
    b.panels_hidden.clear();
    b.hidden.clear();
    b.profile = Some(Profile::Rev);
    b.scale = Scale::Team;
    b.compact = true;
    h.state_mut().presets = vec![a.clone(), b.clone()];
    // From the chip, as a user switches, in either order.
    press(&mut h, "Team");
    press(&mut h, "Docs");
    assert_eq!(h.state().profile, Profile::Rev);
    assert_eq!(h.state().docking, Docking::revit());
    assert!(h.state().panels_hidden.is_empty() && h.state().tools_hidden.is_empty());
    assert_eq!((h.state().scale, h.state().compact), (Scale::Team, true));
    press(&mut h, "Team");
    press(&mut h, "Design");
    assert_eq!(h.state().profile, Profile::BimCraft);
    assert_eq!(h.state().docking, Docking::default());
    assert!(h.state().panels_hidden.contains("tab:View/Phasing"));
    assert!(h.state().tools_hidden.contains("Window"));
    assert_eq!((h.state().scale, h.state().compact), (Scale::Sketch, false));
}

#[test]
fn switching_to_a_rev_preset_turns_the_ribbon_on_and_back_without_rewriting_the_file() {
    let file = settings_file("rev-preset");
    let mut h = bimcraft(BOX);
    h.state_mut().use_settings(file.clone());
    let p = Preset {
        name: "Documentation".into(),
        docking: Docking::default(),
        panels_hidden: BTreeSet::new(),
        hidden: BTreeSet::new(),
        profile: Some(Profile::Rev),
        scale: Scale::Team,
        compact: false,
    };
    h.state_mut().presets = vec![p.clone()];
    h.run();
    press(&mut h, "Team");
    press(&mut h, "Documentation");
    assert_eq!(h.state().profile, Profile::Rev);
    assert_eq!(h.state().docking, Docking::default(), "the Tools palette stays in the layout as the user's intent");
    assert!(h.query_by_label("Minimize the Ribbon").is_some(), "the ribbon is on");
    h.run_steps(5);
    assert_eq!(h.state().docking, Docking::default(), "Rev's profile does not take it out");
    assert_eq!(h.state().presets, vec![p.clone()], "the preset is untouched");
    assert_eq!(kept(&file)["presets"][0], p.to_json());
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn revs_profile_does_not_rewrite_the_saved_palettes_or_the_presets() {
    let file = settings_file("rev-rewrite");
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    let p = Preset {
        name: "Own".into(),
        docking: Docking::default(),
        panels_hidden: BTreeSet::from(["tab:View/Phasing".to_string()]),
        hidden: BTreeSet::new(),
        profile: Some(Profile::Rev),
        scale: Scale::Sketch,
        compact: false,
    };
    h.state_mut().docking = Docking::default();
    h.state_mut().presets = vec![p.clone()];
    h.run();
    let before = std::fs::read_to_string(&file).unwrap();
    h.run_steps(5);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before, "Rev's profile writes nothing");
    assert_eq!(h.state().docking, Docking::default());
    assert_eq!(h.state().presets, vec![p]);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_preset_reset_to_the_standard_view_keeps_its_name_profile_and_leaves_the_window_alone() {
    let mut h = bimcraft(BOX);
    let p = Preset {
        name: "Doc".into(),
        docking: Docking::revit(),
        panels_hidden: BTreeSet::from(["tab:View/Phasing".to_string()]),
        hidden: BTreeSet::from(["Door".to_string()]),
        profile: Some(Profile::Rev),
        scale: Scale::Sketch,
        compact: true,
    };
    h.state_mut().presets = vec![p];
    h.state_mut().scale = Scale::Team;
    h.run();
    let before = (h.state().docking.clone(), h.state().panels_hidden.clone(), h.state().tools_hidden.clone());
    press(&mut h, "Settings");
    press(&mut h, "Edit");
    assert!(h.query_by_label("Preset: Doc").is_some());
    press(&mut h, "Reset to Standard View");
    press(&mut h, "OK");
    let kept = &h.state().presets[0];
    assert_eq!(kept.name, "Doc");
    assert_eq!(kept.profile, Some(Profile::Rev), "the profile is the preset's own");
    assert_eq!(kept.docking, Docking::revit(), "the standard view of its profile");
    assert!(kept.panels_hidden.is_empty() && kept.hidden.is_empty());
    assert_eq!((kept.scale, kept.compact), (Scale::Team, false));
    assert_eq!((h.state().docking.clone(), h.state().panels_hidden.clone(), h.state().tools_hidden.clone()), before);
}

#[test]
fn several_presets_are_kept_and_chosen_from_the_project_scale_menu() {
    let mut h = bimcraft(BOX);
    press(&mut h, "Settings");
    type_into(&mut h, "Preset name:", "Design");
    press(&mut h, "Save Preset");
    assert!(h.state().preset_name.is_empty(), "the field starts blank for the next preset");
    type_into(&mut h, "Preset name:", "Docs");
    press(&mut h, "Save Preset");
    press(&mut h, "Close");
    assert_eq!(h.state().presets.len(), 2, "two names, two presets");
    assert_eq!(h.state().active_preset.as_deref(), Some("Docs"), "the last saved is the one the window wears");
    // The Project scale menu lists every preset, the one applied ticked, to choose any at any time.
    press(&mut h, "Team");
    assert!(h.query_by_label("Design").is_some() && h.query_by_label("Docs").is_some());
    press(&mut h, "Design");
    assert_eq!(h.state().active_preset.as_deref(), Some("Design"));
}

#[test]
fn edit_tools_done_keeps_the_palette_layout_and_panels_in_the_selected_preset() {
    let mut h = bimcraft(BOX);
    let p = Preset {
        name: "Design".into(),
        docking: Docking::default(),
        panels_hidden: BTreeSet::new(),
        hidden: BTreeSet::new(),
        profile: Some(Profile::BimCraft),
        scale: Scale::Sketch,
        compact: false,
    };
    h.state_mut().presets = vec![p.clone()];
    h.state_mut().apply_preset(&p);
    h.run();
    // The user moves a palette and takes a panel out, then presses Done: both go into the preset the window wears.
    h.state_mut().docking.put(Palette::Browser, Target::Side(Side::Bottom));
    press(&mut h, "Edit Tools");
    h.get_by_role_and_label(Role::CheckBox, "Build").scroll_to_me();
    h.run();
    let at = h.get_by_role_and_label(Role::CheckBox, "Build").rect().center();
    click_at(&mut h, at);
    press(&mut h, "Done");
    let kept = &h.state().presets[0];
    assert_eq!(kept.docking.docked(Palette::Browser), Some(Side::Bottom), "the palette layout is kept in the preset");
    assert!(kept.panels_hidden.contains("tab:Architecture/Build"), "the panel taken out is kept too");
    assert_eq!(h.state().active_preset.as_deref(), Some("Design"));
}
