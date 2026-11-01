//! The tool palette profile: the window's tools in a column at its left, in place of the ribbon, and the setting that asks for it.

use super::*;
use crate::settings::{Profile, Settings};
use serde_json::json;

fn palette() -> Harness<'static, BimApp> {
    let mut h = app("");
    h.state_mut().profile = Profile::Palette;
    h.run();
    h
}

#[test]
fn the_ribbon_is_the_default_and_the_palette_replaces_it() {
    let mut h = app("");
    assert_eq!(h.state().profile, Profile::Ribbon);
    assert!(h.query_by_label("Minimize the Ribbon").is_some(), "the ribbon's fold button");
    h.state_mut().profile = Profile::Palette;
    h.run();
    assert!(h.query_by_label("Minimize the Ribbon").is_none(), "no ribbon");
    assert!(h.query_by_label("Wall").is_some(), "the Architecture tab's tools are in the palette");
    h.state_mut().profile = Profile::Ribbon;
    h.run();
    assert!(h.query_by_label("Minimize the Ribbon").is_some(), "and back");
}

#[test]
fn a_row_of_the_palette_picks_its_tool_as_a_ribbon_button_does() {
    let mut h = palette();
    press(&mut h, "Wall");
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    press(&mut h, "Door");
    assert!(matches!(h.state().tool, Tool::Door));
}

#[test]
fn the_palette_names_each_panel_of_a_tab_and_a_tab_picked_shows_its_tools() {
    let mut h = palette();
    for panel in ["Build", "Circulation", "Opening"] {
        assert!(h.query_by_label(panel).is_some(), "{panel}");
    }
    assert!(h.query_by_label("Beam").is_none(), "a Structure tool is on another tab");
    press(&mut h, "Structure");
    assert_eq!(h.state().tab, Tab::Structure);
    assert!(h.query_by_label("Beam").is_some());
    assert!(h.query_by_label("Wall").is_none(), "and the Architecture tab's are not");
    press(&mut h, "Architecture");
    assert!(h.query_by_label("Wall").is_some());
}

#[test]
fn the_palette_keeps_the_tools_of_the_modify_tab_in_reach() {
    let mut h = palette();
    // The tab's pill, not the Architecture tab's Modify tool: the higher of the two.
    let pill = h.get_all_by_label("Modify").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().rect().center();
    click_at(&mut h, pill);
    assert_eq!(h.state().tab, Tab::Modify);
    assert!(h.query_by_label("Move").is_some());
}

#[test]
fn the_settings_page_picks_the_profile_and_the_settings_file_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    press(&mut h, "Settings");
    pick(&mut h, Role::RadioButton, "Tool palette");
    assert_eq!(h.state().profile, Profile::Palette);
    h.run();
    let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["profile"], "palette");

    let mut again = app("");
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().profile, Profile::Palette, "the next start");
    again.state_mut().reopened();
    assert_eq!(again.state().profile, Profile::Palette, "File > New keeps it");

    pick(&mut h, Role::RadioButton, "Ribbon");
    assert_eq!(h.state().profile, Profile::Ribbon);
}

#[test]
fn a_settings_file_that_names_no_profile_leaves_the_one_there_is() {
    let mut h = app("");
    h.state_mut().use_settings(tempfile::tempdir().unwrap().path().join("settings.json"));
    let base = h.state().settings_kept.clone().unwrap();
    let palette = Settings { profile: Profile::Palette, ..base.clone() };
    for hostile in [json!("tabs"), json!(5), json!(null), json!(["palette"]), json!({}), json!("PALETTE")] {
        let v = json!({"profile": hostile});
        assert_eq!(Settings::from_json(&v, base.clone()).profile, Profile::Ribbon, "{hostile}");
        assert_eq!(Settings::from_json(&v, palette.clone()).profile, Profile::Palette, "{hostile}");
    }
    assert_eq!(Settings::from_json(&json!({"profile": "palette"}), base.clone()).profile, Profile::Palette);
    assert_eq!(Settings::from_json(&json!({"profile": "ribbon"}), palette).profile, Profile::Ribbon);
    assert_eq!(Settings::from_json(&json!({}), base).profile, Profile::Ribbon, "older files have none");
}

#[test]
fn a_tool_that_drops_choices_down_shows_them_beside_its_row() {
    let mut h = app(BOX);
    h.state_mut().profile = Profile::Palette;
    h.run();
    press(&mut h, "View");
    let before = h.state().sheet;
    press(&mut h, "Duplicate View");
    assert!(h.query_by_label("Duplicate with Detailing").is_some(), "its choices");
    press(&mut h, "Duplicate with Detailing");
    assert_ne!(h.state().sheet, before, "and a choice does what it does on the ribbon: the new plan is open");
}
