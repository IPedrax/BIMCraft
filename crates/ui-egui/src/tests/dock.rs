//! Palettes that dock and float, views that float off the tab bar, the ribbon folded to its tabs and File > Settings, driven
//! the way a user drives them: menus, drags and clicks.

use super::*;
use crate::dock::{Arrange, Docking, Edge, Palette, Policy, Side, Target};
use egui::{Rect, Vec2};
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
    assert_eq!(*docking(&h), Docking::revit(), "Rev's: Properties over the Project Browser, on the left");
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
    from_list(&mut h, "Windows", "User Interface");
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
    // The page runs below the fold now, so these are pressed as a screen reader does.
    h.get_by_role_and_label(Role::CheckBox, "Minimize the ribbon to its tabs").click_accesskit();
    h.run();
    assert!(h.state().ribbon_min);
    h.get_by_role_and_label(Role::CheckBox, "Project Browser").click_accesskit();
    h.run();
    assert!(!docking(&h).shown(Palette::Browser));
    h.get_by_label("Reset Palette Layout").click_accesskit();
    h.run();
    assert_eq!(*docking(&h), Docking::revit(), "Rev's layout, in Rev");
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
    assert_eq!(Docking::from_json(&d.to_json(&Palette::known()), Policy::Live, &Palette::known()), d);
    d.take(Palette::Properties);
    let back = Docking::from_json(&d.to_json(&Palette::known()), Policy::Live, &Palette::known());
    assert_eq!(back, d);
    assert!(!back.shown(Palette::Properties), "closed stays closed");
    // A palette named twice, one it doesn't know, numbers out of range, an empty stack.
    let hostile = json!({
        "left": [{"tabs": ["Properties", "Properties", "Toolbox"], "active": 9}, {"tabs": []}],
        "right": "nonsense",
        "floats": [{"tabs": ["Project Browser"], "at": [1e300, -1e300], "size": [0, 1e9]}, {"tabs": ["Properties"], "at": [0, 0], "size": [100, 100]}],
    });
    let d = Docking::from_json(&hostile, Policy::Live, &Palette::known());
    assert_eq!(d.left.len(), 2, "and the Tools palette, which the file doesn't name, first");
    assert_eq!(d.left[0].tabs, [Palette::Tools]);
    assert_eq!((d.left[1].tabs.as_slice(), d.left[1].active), ([Palette::Properties].as_slice(), 0));
    let f = d.float_of(Palette::Browser).unwrap();
    assert!(f.at.x <= 20_000.0 && f.at.y >= -20_000.0 && f.size.x >= 120.0 && f.size.y <= 4000.0, "{f:?}");
    assert_eq!(d.floats.len(), 1, "Properties is placed once");
    // A palette it neither places nor closes, as one added since would be, docks on the left; nonsense is the default.
    let d = Docking::from_json(&json!({"left": [{"tabs": ["Project Browser"]}]}), Policy::Live, &Palette::known());
    assert_eq!(d.docked(Palette::Properties), Some(Side::Left));
    assert_eq!(Docking::from_json(&json!([1, 2]), Policy::Live, &Palette::known()), Docking::default());
}

#[test]
fn a_palette_dropped_at_the_top_of_another_goes_above_it_in_its_own_stack_in_both_profiles() {
    for rev in [true, false] {
        let mut h = if rev { app(BOX) } else { bimcraft(BOX) };
        let (side, first, second) =
            if rev { (Side::Left, "Properties", "Project Browser") } else { (Side::Right, "Inspector", "Navigator") };
        let stacks = |h: &Harness<BimApp>| docking(h).side(side).iter().map(|s| s.tabs.clone()).collect::<Vec<_>>();
        assert_eq!(stacks(&h), [vec![Palette::Properties], vec![Palette::Browser]], "rev {rev}: as they start");
        // The second's title to the top edge of the first's header: above it, no tabs made.
        let header = h.state().drop_headers.iter().find(|(p, _, _)| *p == Palette::Properties).map(|(_, r, _)| *r).unwrap();
        let from = at_start(&h, &format!("{second} ·"));
        drag_screen(&mut h, from, pos2(header.center().x, header.top() + 2.0), PointerButton::Primary);
        assert_eq!(stacks(&h), [vec![Palette::Browser], vec![Palette::Properties]], "rev {rev}: {second} above {first}");
    }
}

#[test]
fn the_app_bars_palettes_menu_brings_a_closed_palette_back_and_resets_the_layout() {
    let mut h = bimcraft(BOX);
    h.state_mut().show_palette(Palette::Tools, false);
    h.state_mut().show_palette(Palette::Browser, false);
    h.run();
    press(&mut h, "Palettes");
    pick(&mut h, Role::CheckBox, "Tools");
    assert!(docking(&h).shown(Palette::Tools), "the Tools palette, which has no shortcut, comes back from the bar");
    press(&mut h, "Palettes");
    press(&mut h, "Reset Palette Layout");
    assert_eq!(docking(&h), &Docking::default(), "the layout as at first, the Navigator back too");
}

// ---------- the windows of views: magnets, columns, rows and tiles ----------

/// The rectangles of the windows off the tab bar, in the order the layout places them.
fn cells(h: &Harness<BimApp>) -> Vec<(Sheet, Rect)> {
    h.state().placements.clone()
}

/// A view's window title, as the hand takes it: 80 points in, on its strip's line.
fn title(h: &Harness<BimApp>, sheet: Sheet) -> Pos2 {
    cells(h).into_iter().find(|(s, _)| *s == sheet).map_or_else(
        || h.state().floating.iter().find(|f| f.sheet == sheet).map_or(Pos2::ZERO, |f| f.at + vec2(80.0, 13.0)),
        |(_, r)| r.min + vec2(80.0, 13.0),
    )
}

/// A view's window title dragged to `to`: the hand takes it on its strip and lets it go there.
fn drag_view(h: &mut Harness<BimApp>, sheet: Sheet, to: Pos2) {
    let from = title(h, sheet);
    drag_screen(h, from, to, PointerButton::Primary);
}

/// The views' area as this frame laid it out, checked: every column of `width` cells and the tabbed pane is equal,
/// and their widths add up to the pane's.
fn assert_columns(h: &Harness<BimApp>, columns: usize, rows: usize) {
    let views = h.state().views_rect;
    assert!(views.width() > 0.0 && views.height() > 0.0, "{views:?}");
    let placements = cells(h);
    let mut parts = placements.iter().map(|(_, r)| *r).collect::<Vec<_>>();
    parts.push(h.state().tab_pane);
    assert_eq!(parts.len(), columns * rows, "{parts:?}");
    if rows == 1 {
        for r in &parts {
            assert!((r.width() - views.width() / columns as f32).abs() < 1e-3, "{r:?} of {views:?}");
        }
        let sum: f32 = parts.iter().map(Rect::width).sum();
        assert!((sum - views.width()).abs() < 1e-3, "{sum} of {}", views.width());
    }
}

#[test]
fn one_window_snapped_left_makes_two_equal_columns() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.left() + 8.0, views.center().y));
    assert_eq!(h.state().arrange, Arrange::Split);
    assert_eq!(h.state().floating[0].edge, Some(Edge::Left));
    assert_eq!(cells(&h).len() + 1, 2, "the window and the tabs: two parts");
    let (_, left) = cells(&h)[0];
    let tab = h.state().tab_pane;
    assert!((left.width() - tab.width()).abs() < 1e-3, "{left:?} {tab:?}");
    assert!((left.width() + tab.width() - views.width()).abs() < 1e-3);
    assert!(left.left() <= views.left() + 1e-3 && left.right() <= tab.left() + 1e-3, "the window on the left, the tabs right");
    // The tabbed pane's own header holds the tabs: no tab bar floats above the split.
    assert!(!h.state().tab_rects.is_empty(), "the tabs are the tabbed pane's header");
    for (_, r) in h.state().tab_rects.clone() {
        assert!(r.top() >= tab.top() - 1e-3 && r.left() >= tab.left() - 1e-3 && r.right() <= tab.right() + 1e-3, "{r:?} of {tab:?}");
    }
}

#[test]
fn two_snapped_windows_make_three_equal_columns_in_the_order_they_were_dropped() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.state_mut().float_view(Sheet::View3D, pos2(600.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.left() + 8.0, views.center().y));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, pos2(views.right() - 8.0, views.center().y));
    assert_eq!(h.state().arrange, Arrange::Split);
    assert_eq!(cells(&h).len() + 1, 3);
    assert_eq!(cells(&h).iter().map(|(s, _)| *s).collect::<Vec<_>>(), [two, Sheet::View3D]);
    assert_columns(&h, 3, 1);
    let tabs = h.state().tab_pane;
    let (_, left) = cells(&h)[0];
    let (_, right) = cells(&h)[1];
    assert!(left.left() < tabs.left() && tabs.right() < right.right() + 1e-3, "left, tabs, right: {left:?} {tabs:?} {right:?}");
}

#[test]
fn the_top_magnet_still_docks_a_window_back_into_the_tabs() {
    let (mut h, one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.run();
    let bar = h.state().tab_bar;
    drag_view(&mut h, two, pos2(bar.right() - 4.0, bar.center().y));
    assert!(h.state().floating.is_empty());
    assert_eq!(h.state().views, [one, two]);
    assert_eq!(h.state().arrange, Arrange::Tabs);
}

#[test]
fn tile_views_puts_four_views_in_the_four_corners() {
    let (mut h, _one, two) = two_plans();
    let level = h.state().session.model.levels()[0].0;
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::Ceiling(level));
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.state_mut().float_view(Sheet::View3D, pos2(600.0, 300.0));
    h.run();
    press(&mut h, "View");
    from_list(&mut h, "Windows", "Tile Views");
    assert_eq!(h.state().arrange, Arrange::Tile);
    let views = h.state().views_rect;
    let cells = cells(&h);
    assert_eq!(cells.len(), 4);
    assert!(h.state().floating.is_empty(), "the tile gathered the windows onto the bar");
    assert!(h.state().tab_rects.is_empty(), "no tab bar above the tile: each cell's header names its view");
    assert_eq!(h.state().tab_bar, Rect::NOTHING);
    let (a, b, c, d) = (cells[0].1, cells[1].1, cells[2].1, cells[3].1);
    assert!(a.top() <= views.top() + 1e-3 && b.top() <= views.top() + 1e-3);
    assert!(c.bottom() >= views.bottom() - 1e-3 && d.bottom() >= views.bottom() - 1e-3);
    assert!(a.left() <= views.left() + 1e-3 && c.left() <= views.left() + 1e-3);
    assert!(b.right() >= views.right() - 1e-3 && d.right() >= views.right() - 1e-3);
    for r in [a, b, c, d] {
        assert!((r.width() - views.width() / 2.0).abs() < 1e-3, "{r:?}");
        assert!((r.height() - views.height() / 2.0).abs() < 1e-3, "{r:?}");
    }
    assert!((a.width() + b.width() - views.width()).abs() < 1e-3);
    assert!((a.height() + c.height() - views.height()).abs() < 1e-3);
}

#[test]
fn tile_views_of_three_views_leaves_no_empty_cell() {
    let (mut h, _one, two) = two_plans();
    let level = h.state().session.model.levels()[0].0;
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::Ceiling(level));
    h.run();
    press(&mut h, "View");
    from_list(&mut h, "Windows", "Tile Views");
    assert_eq!(h.state().arrange, Arrange::Tile);
    let views = h.state().views_rect;
    let cells = cells(&h);
    assert_eq!(cells.len(), 3);
    assert!(cells[0].1.top() <= views.top() + 1e-3 && cells[1].1.top() <= views.top() + 1e-3, "two in the top row");
    // The one below takes the whole width, the room the two empty cells would have left.
    assert!(cells[2].1.top() >= views.center().y - 1e-3 && cells[2].1.left() <= views.left() + 1e-3, "one across the second row");
    for (i, (_, r)) in cells.iter().enumerate() {
        let width = if i == 2 { views.width() } else { views.width() / 2.0 };
        assert!((r.width() - width).abs() < 1e-3, "{r:?}");
        assert!((r.height() - views.height() / 2.0).abs() < 1e-3, "{r:?}");
    }
}

#[test]
fn a_partial_tile_row_stretches_across_it_without_a_hole() {
    let (mut h, _one, two) = two_plans();
    let level = h.state().session.model.levels()[0].0;
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::Ceiling(level));
    h.state_mut().open_sheet(Sheet::View3D);
    h.state_mut().open_sheet(Sheet::Schedule("room"));
    h.run();
    press(&mut h, "View");
    from_list(&mut h, "Windows", "Tile Views");
    assert_eq!(h.state().arrange, Arrange::Tile);
    let views = h.state().views_rect;
    let cells = cells(&h);
    assert_eq!(cells.len(), 5, "five views in a 3 x 2");
    // The top row is three equal cells; the two below share its whole width.
    for (_, r) in &cells[0..3] {
        assert!((r.width() - views.width() / 3.0).abs() < 1e-3, "{r:?}");
        assert!((r.height() - views.height() / 2.0).abs() < 1e-3, "{r:?}");
        assert!(r.top() <= views.top() + 1e-3, "{r:?}");
    }
    for (i, (_, r)) in cells[3..5].iter().enumerate() {
        assert!((r.width() - views.width() / 2.0).abs() < 1e-3, "{r:?}");
        assert!((r.height() - views.height() / 2.0).abs() < 1e-3, "{r:?}");
        assert!(r.top() >= views.center().y - 1e-3, "{r:?}");
        let left = if i == 0 { views.left() } else { views.center().x };
        assert!((r.left() - left).abs() < 1e-3, "{r:?}");
    }
    assert!((cells[4].1.right() - views.right()).abs() < 1e-3, "the last cell reaches the right edge");
}

#[test]
fn the_error_note_stays_above_the_split_windows() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.left() + 8.0, views.center().y));
    // A note is up, and the window beside the tabs is clicked: it comes to the top of its own order.
    h.state_mut().status = crate::Status { text: "note".into(), error: false };
    h.run();
    let (_, window) = cells(&h).into_iter().find(|(s, _)| *s == two).expect("the snapped window");
    click_at(&mut h, window.min + vec2(80.0, 13.0));
    assert_eq!(h.state().sheet, two);
    // An error replaces the note: its box must come up over the view windows, in the workspace's corner.
    h.state_mut().status = crate::Status { text: "boom".into(), error: true };
    h.run();
    let top = h.ctx.memory(|m| m.areas().top_layer_id(egui::Order::Middle)).expect("the note's layer");
    assert_eq!(top.id, egui::Id::new("warning"));
    let rect = h.ctx.memory(|m| m.area_rect(top.id)).expect("the note's box");
    let views = h.state().views_rect;
    assert!(
        (rect.right() - (views.right() - 16.0)).abs() < 2.0 && (rect.bottom() - (views.bottom() - 16.0)).abs() < 2.0,
        "{rect:?} of {views:?}"
    );
}

#[test]
fn dock_all_views_puts_every_view_back_on_the_bar() {
    let (mut h, _one, two) = two_plans();
    let level = h.state().session.model.levels()[0].0;
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::Ceiling(level));
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.run();
    press(&mut h, "View");
    from_list(&mut h, "Windows", "Tile Views");
    let active = h.state().sheet;
    from_list(&mut h, "Windows", "Dock All Views");
    assert_eq!(h.state().arrange, Arrange::Tabs);
    assert!(h.state().floating.is_empty());
    assert_eq!(h.state().views.len(), 4);
    assert_eq!(h.state().tab_rects.len(), 4);
    assert_eq!(h.state().sheet, active);
    assert!(cells(&h).is_empty());
    assert_eq!(h.state().tab_pane, h.state().views_rect);
}

#[test]
fn a_window_dragged_out_of_its_column_floats_free_and_then_the_tabs_come_back() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.state_mut().float_view(Sheet::View3D, pos2(600.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.left() + 8.0, views.center().y));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, pos2(views.right() - 8.0, views.center().y));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, views.center());
    let right = h.state().floating.iter().find(|f| f.sheet == Sheet::View3D).unwrap();
    assert_eq!(right.edge, None);
    assert_eq!(cells(&h).len() + 1, 2, "the left window and the tabs again");
    let views = h.state().views_rect;
    drag_view(&mut h, two, views.center());
    assert_eq!(h.state().arrange, Arrange::Tabs);
    assert!(cells(&h).is_empty());
}

#[test]
fn a_resized_window_keeps_the_columns_and_rows_equal() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.state_mut().float_view(Sheet::View3D, pos2(600.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.left() + 8.0, views.center().y));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, pos2(views.right() - 8.0, views.center().y));
    let kept: Vec<(Pos2, Vec2)> = h.state().floating.iter().map(|f| (f.at, f.size)).collect();
    h.set_size(vec2(1600.0, 800.0));
    h.run();
    assert_columns(&h, 3, 1);
    assert_ne!(h.state().views_rect, views);
    assert_eq!(h.state().floating.iter().map(|f| (f.at, f.size)).collect::<Vec<_>>(), kept, "free positions and sizes kept");
}

#[test]
fn one_window_snapped_bottom_makes_two_equal_rows_top_down() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.center().x, views.bottom() - 8.0));
    assert_eq!(h.state().arrange, Arrange::Split);
    assert_eq!(h.state().floating[0].edge, Some(Edge::Bottom));
    assert_eq!(cells(&h).len() + 1, 2);
    let views = h.state().views_rect;
    let (_, below) = cells(&h)[0];
    let tabs = h.state().tab_pane;
    assert!((below.height() - tabs.height()).abs() < 1e-3, "{below:?} {tabs:?}");
    assert!((below.height() + tabs.height() - views.height()).abs() < 1e-3);
    assert!((tabs.width() - views.width()).abs() < 1e-3 && (below.width() - views.width()).abs() < 1e-3, "full width rows");
    assert!(tabs.top() <= views.top() + 1e-3 && below.bottom() >= views.bottom() - 1e-3);
}

#[test]
fn two_windows_snapped_down_make_three_equal_rows_with_the_new_one_at_the_bottom() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.state_mut().float_view(Sheet::View3D, pos2(600.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.center().x, views.bottom() - 8.0));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, pos2(views.center().x, views.bottom() - 8.0));
    assert_eq!(h.state().arrange, Arrange::Split);
    assert_eq!(cells(&h).len() + 1, 3);
    assert_eq!(cells(&h).iter().map(|(s, _)| *s).collect::<Vec<_>>(), [two, Sheet::View3D]);
    let views = h.state().views_rect;
    let tabs = h.state().tab_pane;
    let (a, b) = (cells(&h)[0].1, cells(&h)[1].1);
    assert!((tabs.height() - views.height() / 3.0).abs() < 1e-3);
    assert!((a.height() - views.height() / 3.0).abs() < 1e-3 && (b.height() - views.height() / 3.0).abs() < 1e-3);
    assert!(tabs.top() < a.top() && a.top() < b.top(), "top down: tabs, first window, the new one at the bottom");
    assert!((tabs.height() + a.height() + b.height() - views.height()).abs() < 1e-3);
    for r in [tabs, a, b] {
        assert!((r.width() - views.width()).abs() < 1e-3, "{r:?}");
    }
}

#[test]
fn columns_and_rows_mix_in_one_layout() {
    let (mut h, _one, two) = two_plans();
    let level = h.state().session.model.levels()[0].0;
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::Ceiling(level));
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(300.0, 300.0));
    h.state_mut().float_view(Sheet::Ceiling(level), pos2(900.0, 300.0));
    h.state_mut().float_view(Sheet::View3D, pos2(600.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.left() + 8.0, views.center().y));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::Ceiling(level), pos2(views.center().x, views.bottom() - 8.0));
    assert_eq!(h.state().arrange, Arrange::Split);
    let views = h.state().views_rect;
    let cells = cells(&h);
    assert_eq!(cells.len(), 3);
    assert_eq!(cells.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [two, Sheet::View3D, Sheet::Ceiling(level)]);
    let tabs = h.state().tab_pane;
    let (left, right, below) = (cells[0].1, cells[1].1, cells[2].1);
    assert!((left.width() - views.width() / 3.0).abs() < 1e-3);
    assert!((tabs.width() - views.width() / 3.0).abs() < 1e-3 && (right.width() - views.width() / 3.0).abs() < 1e-3);
    assert!((left.height() - views.height() / 2.0).abs() < 1e-3 && (tabs.height() - views.height() / 2.0).abs() < 1e-3);
    assert!((below.width() - views.width()).abs() < 1e-3 && (below.height() - views.height() / 2.0).abs() < 1e-3);
    assert!(left.left() <= views.left() + 1e-3, "{left:?}");
    assert!(tabs.left() >= left.right() - 1e-3 && right.left() >= tabs.right() - 1e-3, "{tabs:?} {right:?}");
    assert!(below.top() >= tabs.bottom() - 1e-3, "the bottom row runs under the columns");
}

#[test]
fn the_bottom_left_corner_belongs_to_the_bottom_zone() {
    let (mut h, _one, two) = two_plans();
    h.state_mut().open_sheet(two);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run();
    h.state_mut().float_view(two, pos2(500.0, 300.0));
    h.state_mut().float_view(Sheet::View3D, pos2(600.0, 300.0));
    h.run();
    let views = h.state().views_rect;
    drag_view(&mut h, two, pos2(views.left() + 8.0, views.center().y));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, pos2(views.right() - 8.0, views.center().y));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, pos2(views.left() + 8.0, views.bottom() - 8.0));
    assert_eq!(h.state().floating.iter().find(|f| f.sheet == Sheet::View3D).unwrap().edge, Some(Edge::Bottom));
    let views = h.state().views_rect;
    drag_view(&mut h, Sheet::View3D, pos2(views.left() + 8.0, views.bottom() - 2.0 * 36.0));
    assert_eq!(h.state().floating.iter().find(|f| f.sheet == Sheet::View3D).unwrap().edge, Some(Edge::Left));
}

#[test]
fn resetting_the_palette_layout_brings_panels_and_tools_back() {
    let mut h = bimcraft(BOX);
    h.state_mut().docking.put(Palette::Tools, Target::Side(Side::Right));
    h.state_mut().tools_hidden.insert("Door".to_string());
    h.state_mut().panels_hidden.insert("tab:View/Phasing".to_string());
    h.run();
    press(&mut h, "Palettes");
    press(&mut h, "Reset Palette Layout");
    assert_eq!(*docking(&h), Docking::default(), "the standard layout of the profile");
    assert!(h.state().tools_hidden.is_empty(), "every tool is shown again");
    assert!(h.state().panels_hidden.is_empty(), "every panel is shown again");
}
