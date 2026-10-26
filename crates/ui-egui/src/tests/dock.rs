//! Palettes that dock and float, views that float off the tab bar, the ribbon folded to its tabs and File > Settings, driven
//! the way a user drives them: menus, drags and clicks.

use super::*;
use crate::dock::{Docking, Palette, Side, Target};
use egui_kittest::kittest::NodeT;

/// The middle of the widget named `label`.
fn at(h: &Harness<BimApp>, label: &str) -> Pos2 {
    h.get_by_label(label).rect().center()
}

/// The middle of the widget whose name starts with `label`: the Project Browser's title names the project too.
fn at_start(h: &Harness<BimApp>, label: &str) -> Pos2 {
    h.get_by_label_contains(label).rect().center()
}

/// A palette's Dock menu, and an item of it.
fn dock_by_menu(h: &mut Harness<BimApp>, palette: &str, item: &str) {
    press(h, &format!("Dock {palette}"));
    press(h, item);
}

fn docking<'a>(h: &'a Harness<BimApp>) -> &'a Docking {
    &h.state().docking
}

/// The two plans of a box with two levels, Level 1's open.
fn two_plans() -> (Harness<'static, BimApp>, Sheet, Sheet) {
    let h = app(BOX);
    let levels: Vec<Id> = h.state().session.model.levels().iter().map(|(id, _)| *id).collect();
    (h, Sheet::Plan(levels[0]), Sheet::Plan(levels[1]))
}

#[test]
fn the_project_browser_scrolls_from_anywhere_across_its_palette() {
    let mut h = app(BOX);
    let panel = egui::PanelState::load(&h.ctx, egui::Id::new("dock-left")).map(|s| s.outer_rect).unwrap();
    let before = h.get_by_label("Views (all)").rect().top();
    // The wheel by the palette's right edge, where its scroll bar is, not past the end of the longest name.
    let at = pos2(panel.right() - 12.0, h.get_by_label("Views (all)").rect().bottom() + 60.0);
    h.hover_at(at);
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -120.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!(h.get_by_label("Views (all)").rect().top() < before - 20.0, "it scrolled");
}

#[test]
fn palettes_dock_left_right_or_bottom_float_and_close_by_their_menus() {
    let mut h = app(BOX);
    assert_eq!(*docking(&h), Docking::default(), "Properties over the Project Browser, on the left");
    dock_by_menu(&mut h, "Properties", "Dock Right");
    assert_eq!(docking(&h).docked(Palette::Properties), Some(Side::Right));
    assert!(at(&h, "Dock Properties").x > 1000.0, "drawn down the right side");
    dock_by_menu(&mut h, "Project Browser", "Dock Bottom");
    assert_eq!(docking(&h).docked(Palette::Browser), Some(Side::Bottom));
    assert!(at(&h, "Dock Project Browser").y > 500.0, "drawn along the bottom");
    dock_by_menu(&mut h, "Project Browser", "Float");
    assert!(docking(&h).float_of(Palette::Browser).is_some() && docking(&h).docked(Palette::Browser).is_none());
    // Floating, it works as it did: a view picked in it opens.
    browse(&mut h, "{3D}");
    assert_eq!(h.state().sheet, Sheet::View3D);
    // Closed, it comes back where it was last docked.
    press(&mut h, "Close Properties");
    assert!(!docking(&h).shown(Palette::Properties) && h.query_by_label("Dock Properties").is_none());
    press(&mut h, "View");
    press(&mut h, "User Interface");
    pick(&mut h, Role::CheckBox, "Properties");
    assert_eq!(docking(&h).docked(Palette::Properties), Some(Side::Right));
    // PP hides it and shows it again.
    key(&mut h, Key::Escape);
    shortcut(&mut h, "PP");
    assert!(!docking(&h).shown(Palette::Properties));
    shortcut(&mut h, "PP");
    assert!(docking(&h).shown(Palette::Properties));
}

#[test]
fn a_palette_dragged_onto_another_stacks_as_its_tab_and_to_an_edge_docks_there() {
    let mut h = app(BOX);
    // The Project Browser's title onto Properties': one stack, two tabs.
    let (from, to) = (at_start(&h, "Project Browser ·"), at(&h, "Properties"));
    drag_screen(&mut h, from, to, PointerButton::Primary);
    let stack = docking(&h).stack_of(Palette::Browser).cloned().unwrap();
    assert_eq!(stack.tabs, [Palette::Properties, Palette::Browser]);
    assert_eq!(stack.shown(), Some(Palette::Browser), "the tab dropped is the one shown");
    assert_eq!(docking(&h).left.len(), 1);
    press(&mut h, "Properties");
    assert_eq!(docking(&h).stack_of(Palette::Properties).unwrap().shown(), Some(Palette::Properties));
    // Its tab to the window's right edge: a stack of its own there.
    let from = at(&h, "Project Browser");
    drag_screen(&mut h, from, pos2(1270.0, 450.0), PointerButton::Primary);
    assert_eq!(docking(&h).docked(Palette::Browser), Some(Side::Right));
    assert_eq!(docking(&h).stack_of(Palette::Properties).unwrap().tabs, [Palette::Properties]);
    // Properties' title into the middle of the view: it floats there, in a window its title moves.
    let from = at(&h, "Properties");
    drag_screen(&mut h, from, pos2(600.0, 400.0), PointerButton::Primary);
    let float = docking(&h).float_of(Palette::Properties).cloned().unwrap();
    // Its corner where it was let go, as far as the window lets it: it stays on screen.
    assert!((float.at.x - 560.0).abs() < 1.0 && float.at.y <= 387.0 && float.at.y + float.size.y <= 800.0, "{float:?}");
    // Its window is named for it too: its title is where the window starts.
    let title = |h: &Harness<BimApp>| docking(h).float_of(Palette::Properties).unwrap().at + vec2(60.0, 13.0);
    let from = title(&h);
    drag_screen(&mut h, from, from + vec2(-100.0, -50.0), PointerButton::Primary);
    let moved = docking(&h).float_of(Palette::Properties).unwrap().at;
    assert!(moved.distance(float.at + vec2(-100.0, -50.0)) < 2.0, "{moved:?}");
    // And back to the left edge.
    let from = title(&h);
    drag_screen(&mut h, from, pos2(12.0, 400.0), PointerButton::Primary);
    assert_eq!(docking(&h).docked(Palette::Properties), Some(Side::Left));
    assert!(docking(&h).floats.is_empty());
    // The strips keep to the view's drawing: let go on the views' tab bar, by the left edge, it floats.
    let bar = h.state().tab_bar;
    let from = at_start(&h, "Project Browser ·");
    drag_screen(&mut h, from, pos2(bar.left() + 8.0, bar.center().y), PointerButton::Primary);
    assert!(docking(&h).float_of(Palette::Browser).is_some(), "{:?}", docking(&h));
}

#[test]
fn a_view_floats_off_the_tab_bar_takes_the_tools_and_docks_back() {
    let (mut h, one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.run();
    assert_eq!(h.state().views, [one, two]);
    // Level 2's tab dragged down into the view: a window of its own, the active view.
    let tab = h.get_all_by_label("Level 2").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().rect().center();
    drag_screen(&mut h, tab, pos2(700.0, 450.0), PointerButton::Primary);
    assert_eq!(h.state().views, [one]);
    assert_eq!(h.state().floating.iter().map(|f| f.sheet).collect::<Vec<_>>(), [two]);
    assert_eq!((h.state().sheet, h.state().front), (two, one), "Level 1 shows behind it");
    // The tools work in it: a wall drawn there is on Level 2.
    tool(&mut h, "Wall");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 1000.0));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    let level = |h: &Harness<BimApp>, at: P2| {
        h.state().session.model.elements.values().find_map(|e| match e {
            Element::Wall(w) if w.start == at => Some(h.state().session.model.elements.id(w.level)),
            _ => None,
        })
    };
    assert_eq!(level(&h, P2(1000.0, 1000.0)), Some(Some(two_id(two))));
    // A click on Level 1, behind it, makes that the active view, the window staying.
    let behind = pos2(400.0, 300.0);
    click_at(&mut h, behind);
    assert_eq!(h.state().sheet, one);
    assert_eq!(h.state().floating.len(), 1);
    // Its window's Dock puts it back on the bar, the active view again.
    press(&mut h, "Dock Level 2");
    assert_eq!((h.state().views.as_slice(), h.state().sheet), ([one, two].as_slice(), two));
    assert!(h.state().floating.is_empty());
}

fn two_id(sheet: Sheet) -> Id {
    match sheet {
        Sheet::Plan(id) => id,
        s => panic!("{s:?}"),
    }
}

#[test]
fn a_floating_views_title_drags_it_back_onto_the_bar_and_the_last_tab_stays() {
    let (mut h, one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.run();
    // The window's title, dropped on the tab bar before Level 1's tab.
    let title = h.state().floating[0].at + vec2(80.0, 13.0);
    let bar = h.state().tab_rects[0].1.left_center() + vec2(2.0, 0.0);
    drag_screen(&mut h, title, bar, PointerButton::Primary);
    assert_eq!(h.state().views, [two, one]);
    assert!(h.state().floating.is_empty());
    // The bar never empties: with one tab left, it can't float.
    press(&mut h, "Close Level 1");
    let only = h.state().tab_rects[0].1.center();
    drag_screen(&mut h, only, pos2(700.0, 450.0), PointerButton::Primary);
    assert_eq!(h.state().views, [two]);
    assert!(h.state().floating.is_empty());
}

#[test]
fn a_view_tabs_menu_floats_it_and_closing_its_window_closes_it() {
    let (mut h, one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.run();
    let tab = h.state().tab_rects[1].1.center();
    h.hover_at(tab);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: tab, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
    press(&mut h, "Float");
    assert_eq!(h.state().floating.iter().map(|f| f.sheet).collect::<Vec<_>>(), [two]);
    press(&mut h, "Close Level 2");
    assert!(h.state().floating.is_empty());
    assert_eq!((h.state().views.as_slice(), h.state().sheet), ([one].as_slice(), one));
}

#[test]
fn double_clicking_a_ribbon_tab_folds_the_ribbon_to_its_tabs() {
    let mut h = app_at(BOX, 0.05);
    assert!(h.query_by_label("Wall").is_some());
    let tab = at(&h, "Architecture");
    h.hover_at(tab);
    h.run();
    for pressed in [true, false, true, false] {
        h.event(Event::PointerButton { pos: tab, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
    assert!(h.state().ribbon_min && h.query_by_label("Wall").is_none(), "only the tabs");
    // A tab clicked shows its tools over the view until one is picked.
    h.run_steps(15);
    press(&mut h, "Structure");
    assert!(h.query_by_label("Beam").is_some());
    press(&mut h, "Beam");
    assert_eq!(h.state().tool, Tool::Run("beam", None));
    assert!(h.query_by_label("Beam").is_none(), "folded again");
    // Or until a click lands elsewhere, or Esc.
    press(&mut h, "Structure");
    click_at(&mut h, pos2(700.0, 500.0));
    assert!(h.query_by_label("Beam").is_none());
    press(&mut h, "Structure");
    key(&mut h, Key::Escape);
    assert!(h.query_by_label("Beam").is_none());
    // Its arrow opens it again.
    press(&mut h, "Expand the Ribbon");
    assert!(!h.state().ribbon_min && h.query_by_label("Beam").is_some());
    press(&mut h, "Minimize the Ribbon");
    assert!(h.state().ribbon_min);
}

#[test]
fn settings_open_from_the_file_menu_and_set_the_theme_the_ribbon_and_the_palettes() {
    let mut h = app(BOX);
    press(&mut h, "File");
    h.get_by_label_contains("Settings…").click();
    h.run();
    assert!(h.query_by_label("User Interface").is_some() && h.query_by_label("Performance").is_some());
    pick(&mut h, Role::RadioButton, "Dark");
    assert!(crate::style::dark());
    pick(&mut h, Role::CheckBox, "Minimize the ribbon to its tabs");
    assert!(h.state().ribbon_min);
    pick(&mut h, Role::CheckBox, "Project Browser");
    assert!(!docking(&h).shown(Palette::Browser));
    press(&mut h, "Reset Palette Layout");
    assert_eq!(*docking(&h), Docking::default());
    // What is still to come shows, greyed out.
    press(&mut h, "Performance");
    assert!(h.get_by_label("Smooth lines (anti-aliasing)").accesskit_node().is_disabled());
    press(&mut h, "Close");
    assert!(h.state().settings_page.is_none());
    // Ctrl+, opens it too, and the gear in the Quick Access Toolbar.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    assert!(h.state().settings_page.is_some());
    press(&mut h, "Close");
    press(&mut h, "Settings");
    assert!(h.state().settings_page.is_some());
}

#[test]
fn the_settings_file_keeps_the_window_as_it_was_left() {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-settings-{}", std::process::id())).join("settings.json");
    let _ = std::fs::remove_file(&file);
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    h.run();
    assert!(!file.exists(), "nothing changed, nothing written");
    dock_by_menu(&mut h, "Properties", "Dock Right");
    press(&mut h, "Minimize the Ribbon");
    press(&mut h, "Settings");
    pick(&mut h, Role::RadioButton, "Dark");
    press(&mut h, "Close");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!((kept["theme"].as_str(), kept["ribbon_minimized"].as_bool()), (Some("dark"), Some(true)));
    // A window started on the same file is as this one was left.
    let mut again = app(BOX);
    again.state_mut().use_settings(file.clone());
    again.run();
    assert_eq!(again.state().docking.docked(Palette::Properties), Some(Side::Right));
    assert!(again.state().ribbon_min && again.ctx.options(|o| o.theme_preference) == egui::ThemePreference::Dark);
    assert_eq!(crate::settings::language_in(&file), Some(crate::lang::Lang::English));
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn dark_ui_with_light_canvas_is_saved_and_old_settings_keep_their_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    // Existing settings have no canvas preference, so dark still means a dark drawing.
    std::fs::write(&file, r#"{"theme":"dark"}"#).unwrap();
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    h.run();
    assert!(crate::canvas::color(bimcraft_core::render::PAPER).r() < 40);
    press(&mut h, "View");
    press(&mut h, "Dark Mode");
    pick(&mut h, Role::RadioButton, "Dark UI, Light Canvas");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept["theme"], "dark");
    assert_eq!(kept["light_canvas"], true);
    let mut again = app(BOX);
    again.state_mut().use_settings(file.clone());
    again.run();
    assert!(crate::style::dark());
    assert_eq!(crate::canvas::color(bimcraft_core::render::PAPER), egui::Color32::WHITE);
    press(&mut again, "View");
    press(&mut again, "Dark Mode");
    pick(&mut again, Role::RadioButton, "Dark");
    assert!(crate::canvas::color(bimcraft_core::render::PAPER).r() < 40);
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept["light_canvas"], false);
}

#[test]
fn a_layout_reads_back_as_written_and_a_hostile_one_reads_as_sense() {
    let mut d = Docking::default();
    d.put(Palette::Browser, Target::With(Palette::Properties));
    d.put(Palette::Properties, Target::Float(pos2(40.0, 50.0)));
    assert_eq!(Docking::from_json(&d.to_json()), d);
    d.take(Palette::Properties);
    let back = Docking::from_json(&d.to_json());
    assert_eq!(back, d);
    assert!(!back.shown(Palette::Properties), "closed stays closed");
    // A palette named twice, one it doesn't know, numbers out of range, an empty stack.
    let hostile = json!({
        "left": [{"tabs": ["Properties", "Properties", "Toolbox"], "active": 9}, {"tabs": []}],
        "right": "nonsense",
        "floats": [{"tabs": ["Project Browser"], "at": [1e300, -1e300], "size": [0, 1e9]}, {"tabs": ["Properties"], "at": [0, 0], "size": [100, 100]}],
    });
    let d = Docking::from_json(&hostile);
    assert_eq!(d.left.len(), 1);
    assert_eq!((d.left[0].tabs.as_slice(), d.left[0].active), ([Palette::Properties].as_slice(), 0));
    let f = d.float_of(Palette::Browser).unwrap();
    assert!(f.at.x <= 20_000.0 && f.at.y >= -20_000.0 && f.size.x >= 120.0 && f.size.y <= 4000.0, "{f:?}");
    assert_eq!(d.floats.len(), 1, "Properties is placed once");
    // A palette it neither places nor closes, as one added since would be, docks on the left; nonsense is the default.
    let d = Docking::from_json(&json!({"left": [{"tabs": ["Project Browser"]}]}));
    assert_eq!(d.docked(Palette::Properties), Some(Side::Left));
    assert_eq!(Docking::from_json(&json!([1, 2])), Docking::default());
}
