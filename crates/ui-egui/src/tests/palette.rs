//! The tool palette profile: the window's tools in a column at its left, in place of the ribbon, and the setting that asks for it.

use super::*;
use crate::settings::{Profile, Settings};
use egui::Rect;
use serde_json::json;

fn palette() -> Harness<'static, BimApp> {
    let mut h = app("");
    h.state_mut().profile = Profile::Palette;
    h.run();
    h
}

/// The section of the palette's list named `label`: its header, which is also what opens and folds it. A panel of the section may be
/// named like it (Structure's Structure), but that is a label, not a button; and "Modify" is a tool in the Select panel above and a
/// section below it, so of the buttons the lower one is taken.
fn header(h: &Harness<BimApp>, label: &str) -> egui::Pos2 {
    h.get_all_by_role_and_label(Role::Button, label).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().rect().center()
}

/// Opens (or folds) the section `label`: scrolled to, as a user does when the list runs below the fold, and clicked on its header.
fn toggle(h: &mut Harness<BimApp>, label: &str) {
    h.get_all_by_role_and_label(Role::Button, label).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().scroll_to_me();
    h.run();
    let at = header(h, label);
    click_at(h, at);
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
fn the_palette_is_one_list_with_a_section_for_each_tab_and_no_tab_pills() {
    let mut h = palette();
    for tab in [
        "Architecture",
        "Structure",
        "Plumbing",
        "Electrical",
        "Lightning Protection",
        "Data",
        "Insert",
        "Annotate",
        "Massing",
        "View",
        "Manage",
    ] {
        assert!(h.query_by_label(tab).is_some(), "a section for {tab}");
    }
    // Architecture is open to begin with: its panels, and none of the others' tools.
    for panel in ["Build", "Circulation", "Opening"] {
        assert!(h.query_by_label(panel).is_some(), "{panel}");
    }
    assert!(h.query_by_label("Wall").is_some());
    assert!(h.query_by_label("Beam").is_none(), "Structure is folded");
    // Opening a section adds its tools to the list; the others stay as they are.
    toggle(&mut h, "Structure");
    assert_eq!(h.state().tab, Tab::Structure);
    assert!(h.query_by_label("Beam").is_some());
    assert!(h.query_by_label("Wall").is_some(), "one list: Architecture is still open");
    toggle(&mut h, "Structure");
    assert!(h.query_by_label("Beam").is_none(), "folded again");
    // And what is above the list is not five rows of pills: the list starts under the Select panel.
    let first = h.get_by_label("Select").rect().top();
    assert!(first < 200.0, "the Select panel is at the top of the column: {first}");
}

#[test]
fn the_select_panel_is_shown_once_however_many_sections_are_open() {
    let mut h = palette();
    for tab in ["Structure", "Annotate", "View"] {
        toggle(&mut h, tab);
    }
    assert_eq!(h.get_all_by_label("Select").count(), 1, "the panel's name");
}

#[test]
fn the_modify_section_holds_the_edit_tools_and_a_tool_in_hand_names_it() {
    let mut h = app(BOX);
    h.state_mut().profile = Profile::Palette;
    h.run();
    assert!(h.query_by_label("Move").is_none(), "folded to begin with");
    toggle(&mut h, "Modify");
    for tool in ["Move", "Copy", "Delete", "Align"] {
        assert!(in_column(&h, tool), "{tool}");
    }
    toggle(&mut h, "Modify");
    // A tool in hand: its own panels (the shapes it draws) come at the top, under the tool's name.
    press(&mut h, "Wall");
    assert!(h.query_by_label("Modify | Place Wall").is_some(), "the section is named for the tool");
    assert!(h.query_by_label("Draw").is_none(), "folded: the list does not jump when a tool is picked");
    press(&mut h, "Door");
    assert!(matches!(h.state().tool, Tool::Door), "the tools are where they were");
    press(&mut h, "Wall");
    toggle(&mut h, "Modify | Place Wall");
    assert!(h.query_by_label("Draw").is_some(), "opened, it holds the tool's own panel");
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("Modify | Place Wall").is_none(), "and the name goes with the tool");
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
    toggle(&mut h, "View");
    let before = h.state().sheet;
    h.get_by_role_and_label(Role::Button, "Duplicate View").scroll_to_me();
    h.run();
    press(&mut h, "Duplicate View");
    assert!(h.query_by_label("Duplicate with Detailing").is_some(), "its choices");
    press(&mut h, "Duplicate with Detailing");
    assert_ne!(h.state().sheet, before, "and a choice does what it does on the ribbon: the new plan is open");
}

/// The palette column's right edge, with its margin: what lies right of it is the view.
const COLUMN: f32 = 300.0;

#[test]
fn a_tools_options_are_in_the_palette_column_and_not_over_the_view() {
    let mut h = app(BOX);
    h.state_mut().profile = Profile::Palette;
    h.run();
    assert_eq!(h.state().options_rect, Rect::NOTHING, "no tool: nothing to show");
    assert!(h.query_by_label("Location Line:").is_none());
    press(&mut h, "Wall");
    let field = h.get_by_label("Location Line:").rect();
    assert!(field.right() <= COLUMN, "the wall's options are in the column: {field:?}");
    assert_eq!(h.state().options_rect, Rect::NOTHING, "so nothing floats over the view");
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("Location Line:").is_none(), "and they go with the tool");

    // The ribbon's profile is as it was: a card over the view.
    let mut r = app(BOX);
    tool(&mut r, "Wall");
    assert_ne!(r.state().options_rect, Rect::NOTHING);
    assert!(r.get_by_label("Location Line:").rect().left() > COLUMN || r.state().profile == Profile::Ribbon);
}

#[test]
fn a_field_of_the_info_panel_is_typed_into_and_stays_in_the_column() {
    let mut h = app(BOX);
    h.state_mut().profile = Profile::Palette;
    h.run();
    press(&mut h, "Ceiling");
    type_into(&mut h, "Height Offset:", "2500");
    assert_eq!(h.state().ceiling_height, "2500");
    key(&mut h, Key::Escape);
    // The widest of the Options Bar's fields (a note's text, 240 wide) is held to the column.
    toggle(&mut h, "Annotate");
    // The list runs below the fold: scrolled to, as a user does.
    h.get_by_role_and_label(Role::Button, "Text").scroll_to_me();
    h.run();
    pick(&mut h, Role::Button, "Text");
    let text = h.get_by_label("Text:").rect();
    assert!(text.right() <= COLUMN, "{text:?}");
}

/// Whether a tool has a row of its own in the palette's column: on screen, not off to its right.
fn in_column(h: &Harness<BimApp>, label: &str) -> bool {
    let nodes: Vec<_> = h.get_all_by_label(label).collect();
    nodes.len() == 1 && nodes[0].rect().left() < COLUMN && nodes[0].rect().right() <= COLUMN
}

#[test]
fn small_buttons_the_ribbon_sets_side_by_side_are_rows_one_under_the_other_in_the_palette() {
    let mut h = palette();
    toggle(&mut h, "Annotate");
    for tool in ["Aligned", "Linear", "Angular", "Radial", "Diameter", "Arc Length", "Spot Elevation"] {
        assert!(in_column(&h, tool), "{tool}");
    }
    // The Modify section's panel of Move, Copy and the rest.
    toggle(&mut h, "Modify");
    for tool in ["Move", "Copy", "Delete"] {
        assert!(in_column(&h, tool), "{tool}");
    }
}

#[test]
fn a_button_in_the_title_bar_switches_between_the_ribbon_and_the_palette_and_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    assert_eq!(h.state().profile, Profile::Ribbon);
    // On the ribbon the button offers the palette; on the palette, the ribbon.
    press(&mut h, "Tool palette");
    assert_eq!(h.state().profile, Profile::Palette);
    assert!(h.query_by_label("Minimize the Ribbon").is_none(), "the ribbon is gone at once");
    press(&mut h, "Ribbon");
    assert_eq!(h.state().profile, Profile::Ribbon);
    assert!(h.query_by_label("Minimize the Ribbon").is_some());
    press(&mut h, "Tool palette");
    h.run();
    let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["profile"], "palette", "and the next start has it");
}

#[test]
fn tp_switches_the_layout_is_listed_once_and_can_be_given_another_key() {
    let mut h = app("");
    shortcut(&mut h, "TP");
    assert_eq!(h.state().profile, Profile::Palette);
    shortcut(&mut h, "TP");
    assert_eq!(h.state().profile, Profile::Ribbon);

    let keys: Vec<_> = crate::shortcuts().into_iter().filter(|(k, _)| *k == "TP").collect();
    assert_eq!(keys, [("TP", "Tool palette")], "no command or fixture has it too");
    assert_eq!(crate::shortcuts::resolve_action("Tool palette"), Some("Tool palette"), "an action Keyboard Shortcuts can bind");

    h.state_mut().custom_shortcuts.insert("ZP".to_string(), "Tool palette".to_string());
    shortcut(&mut h, "ZP");
    assert_eq!(h.state().profile, Profile::Palette, "a key of the user's own does the same");
}

/// A press at `from`, moved to `to`, let go: what a hand does to a panel's edge.
fn drag(h: &mut Harness<BimApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run();
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    h.hover_at(to);
    h.run();
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
}

/// A palette window of the given width.
fn palette_of(width: f32) -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    h.state_mut().profile = Profile::Palette;
    h.state_mut().palette_width = width;
    h.run();
    h.run();
    h
}

#[test]
fn the_columns_width_is_the_one_asked_for_and_does_not_drift() {
    for width in [240.0, 280.0, 340.0, 480.0] {
        let h = palette_of(width);
        assert!((h.state().palette_width - width).abs() < 1.0, "asked {width}, is {}", h.state().palette_width);
    }
}

#[test]
fn dragging_the_columns_edge_resizes_it_within_limits() {
    let mut h = palette_of(280.0);
    let edge = |h: &Harness<BimApp>| egui::pos2(h.state().palette_rect.right(), 400.0);
    let from = edge(&h);
    drag(&mut h, from, from + egui::vec2(80.0, 0.0));
    assert!((h.state().palette_width - 360.0).abs() < 4.0, "wider: {}", h.state().palette_width);
    let from = edge(&h);
    drag(&mut h, from, egui::pos2(1250.0, 400.0));
    assert!(
        h.state().palette_width <= crate::settings::PALETTE_WIDTH.end() + 1.0,
        "no wider than the least of the window: {}",
        h.state().palette_width
    );
    let from = edge(&h);
    drag(&mut h, from, egui::pos2(5.0, 400.0));
    assert!(
        h.state().palette_width >= crate::settings::PALETTE_WIDTH.start() - 1.0,
        "no narrower than it can be: {}",
        h.state().palette_width
    );
}

#[test]
fn the_width_is_kept_in_the_settings_and_across_new_and_open() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    h.state_mut().profile = Profile::Palette;
    h.run();
    let edge = egui::pos2(h.state().palette_rect.right(), 400.0);
    drag(&mut h, edge, edge + egui::vec2(60.0, 0.0));
    let width = h.state().palette_width;
    assert!(width > 320.0, "{width}");
    h.run();
    let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert!((kept["palette_width"].as_f64().unwrap() as f32 - width).abs() < 1.0, "{kept}");

    let mut again = app(BOX);
    again.state_mut().use_settings(file);
    again.run();
    again.run();
    assert!((again.state().palette_width - width).abs() < 1.0, "the next start: {}", again.state().palette_width);
    again.state_mut().reopened();
    again.run();
    assert!((again.state().palette_width - width).abs() < 1.0, "File > New keeps it");
}

#[test]
fn a_settings_file_with_a_hostile_width_leaves_the_one_there_is() {
    let base = Settings { palette_width: 333.0, ..default_settings_for_palette() };
    for hostile in [json!("wide"), json!(-5), json!(0), json!(5.0), json!(1.0e9), json!(null), json!([300]), json!({}), json!(true)] {
        let v = json!({"palette_width": hostile});
        assert_eq!(Settings::from_json(&v, base.clone()).palette_width, 333.0, "{hostile}");
    }
    assert_eq!(Settings::from_json(&json!({"palette_width": 400}), base.clone()).palette_width, 400.0);
    assert_eq!(Settings::from_json(&json!({}), base).palette_width, 333.0, "older files have none");
}

fn default_settings_for_palette() -> Settings {
    let mut h = app("");
    h.state_mut().use_settings(tempfile::tempdir().unwrap().path().join("settings.json"));
    h.state().settings_kept.clone().unwrap()
}

#[test]
fn the_width_stays_what_it_is_whatever_tool_is_in_hand_or_tab_is_open() {
    // Wide enough for names, and narrow, where it is icons only and a tool's options float over the view.
    for width in [300.0, 120.0] {
        the_width_stays_over_every_tool(width);
    }
}

fn the_width_stays_over_every_tool(asked: f32) {
    let mut h = palette_of(asked);
    let width = h.state().palette_width;
    // A tool of each tab that has options of its own (drop-downs and fields of every kind), picked as a user does: scrolled to.
    for (tab, tools) in [
        ("Architecture", &["Wall", "Door", "Column", "Roof", "Floor", "Ceiling", "Stair", "Ramp", "Shaft", "Dormer", "Level"][..]),
        ("Structure", &["Column", "Beam", "Isolated Footing", "Wall Footing"][..]),
        ("Plumbing", &["Pipe"][..]),
        ("Electrical", &["Conduit", "Wire"][..]),
        ("Annotate", &["Aligned", "Detail Line", "Detail Group", "Text", "Tag by Category"][..]),
        ("Massing", &["Mass"][..]),
    ] {
        if tab != "Architecture" {
            toggle(&mut h, tab);
        }
        assert!((h.state().palette_width - width).abs() < 0.5, "{tab}: {}", h.state().palette_width);
        for tool in tools {
            // Two sections can have a tool of the name (a Column in Architecture and in Structure): the section opened last is lowest.
            h.get_all_by_role_and_label(Role::Button, tool).last().unwrap().scroll_to_me();
            h.run();
            let at = h.get_all_by_role_and_label(Role::Button, tool).last().unwrap().rect().center();
            click_at(&mut h, at);
            assert!(!matches!(h.state().tool, Tool::Modify), "{tab} > {tool} is in hand");
            assert!((h.state().palette_width - width).abs() < 0.5, "{tab} > {tool}: {}", h.state().palette_width);
            key(&mut h, Key::Escape);
        }
    }
    for tab in ["Insert", "View", "Manage", "Modify"] {
        toggle(&mut h, tab);
        assert!((h.state().palette_width - width).abs() < 0.5, "{tab}: {}", h.state().palette_width);
    }
}

/// A tool's button in the palette: the one of the row (or the icon) of that name, scrolled to.
fn tool_button(h: &mut Harness<BimApp>, label: &str) -> egui::Rect {
    h.get_all_by_role_and_label(Role::Button, label).last().unwrap().scroll_to_me();
    h.run();
    h.get_all_by_role_and_label(Role::Button, label).last().unwrap().rect()
}

#[test]
fn from_about_150_points_up_the_palette_shows_names_and_below_it_only_icons() {
    for (width, names) in [(280.0, true), (200.0, true), (170.0, true), (140.0, false), (100.0, false), (88.0, false)] {
        let mut h = palette_of_script(BOX, width);
        let wall = tool_button(&mut h, "Wall");
        if names {
            assert!(wall.width() > 100.0, "{width}: a row, {} wide", wall.width());
        } else {
            assert!(wall.width() < 50.0, "{width}: an icon, {} wide", wall.width());
        }
        assert!((h.state().palette_width - width).abs() < 2.0, "asked {width}: the content made it {}", h.state().palette_width);
        // Whichever, it picks the tool, and the name is what a screen reader hears.
        let at = wall.center();
        click_at(&mut h, at);
        assert!(matches!(h.state().tool, Tool::Wall(_)), "{width}: a click on the button picks the tool");
    }
}

#[test]
fn the_icons_are_a_grid_that_holds_every_tool_in_the_column() {
    let mut h = palette_of_script(BOX, 100.0);
    for tool in ["Modify", "Wall", "Door", "Window", "Column", "Roof", "Floor", "Ceiling", "Stair", "Ramp", "Railing", "Room"] {
        let r = tool_button(&mut h, tool);
        assert!(r.left() >= 0.0 && r.right() <= 110.0, "{tool}: {r:?}");
    }
    // More than one to a row: the icons are laid in a grid, not one under the other.
    let (wall, door) = (tool_button(&mut h, "Wall").center(), tool_button(&mut h, "Door").center());
    assert!((wall.y - door.y).abs() < 2.0 && door.x > wall.x, "Wall at {wall:?}, Door at {door:?}");
}

#[test]
fn where_the_palette_is_icons_only_a_tools_options_float_over_the_view_as_the_ribbons_do() {
    let mut narrow = palette_of_script(BOX, 100.0);
    press_tool(&mut narrow, "Wall");
    assert_ne!(narrow.state().options_rect, Rect::NOTHING, "the Options Bar floats");
    let field = narrow.get_by_label("Location Line:").rect();
    assert!(field.left() > 110.0, "over the view, not in the column: {field:?}");
    // The name of the tool leads it, as it does on the ribbon: the column has no section to say it.
    assert!(narrow.query_by_label("Modify | Place Wall").is_some());

    let mut wide = palette_of_script(BOX, 280.0);
    press_tool(&mut wide, "Wall");
    assert_eq!(wide.state().options_rect, Rect::NOTHING, "in the column when it fits");
    assert!(wide.get_by_label("Location Line:").rect().right() <= COLUMN);
}

#[test]
fn a_selection_has_no_summary_in_an_icons_only_column() {
    let mut h = palette_of_script(BOX, 100.0);
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.query_by_label("Total thickness: 200 mm").is_none(), "no room for it: Properties has the rest");
    let mut wide = palette_of_script(BOX, 280.0);
    click(&mut wide, P2(4000.0, 0.0));
    assert!(wide.query_by_label("Total thickness: 200 mm").is_some());
}

/// A tool's button picked as a user does, scrolled to.
fn press_tool(h: &mut Harness<BimApp>, label: &str) {
    let at = tool_button(h, label).center();
    click_at(h, at);
}

/// A palette window of `width`, on a model of `script`.
fn palette_of_script(script: &str, width: f32) -> Harness<'static, BimApp> {
    let mut h = app(script);
    h.state_mut().profile = Profile::Palette;
    h.state_mut().palette_width = width;
    h.ctx.data_mut(|d| d.remove::<egui::PanelState>(egui::Id::new("tool-palette")));
    h.run();
    h.run();
    h
}
