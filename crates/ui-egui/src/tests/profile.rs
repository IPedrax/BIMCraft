//! The interface's profiles: BIMCraft's own look (the default) and Rev, flat and square in light chrome, with the File tab
//! leading the ribbon's tabs and the Options Bar docked under the ribbon.

use egui::Shape;
use egui_kittest::kittest::NodeT;

use super::*;
use crate::style::Profile;

/// The band across the top of the window: the app bar's ground, as the frame painted it (the topmost band as wide as the window).
fn app_bar_ground(h: &Harness<BimApp>) -> egui::Color32 {
    fn walk(s: &Shape, out: &mut Vec<(f32, egui::Color32)>) {
        match s {
            Shape::Rect(r) if r.rect.width() >= 1000.0 && r.rect.height() < 60.0 => out.push((r.rect.top(), r.fill)),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, fill)| fill).expect("the app bar")
}

/// Relative luminance, 0 (black) to 1 (white).
fn luminance(c: egui::Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

#[test]
fn the_profile_is_picked_in_settings_and_the_settings_file_keeps_it() {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-profile-{}", std::process::id())).join("settings.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    // A profile this version doesn't know (a newer one's) leaves BIMCraft's, the default.
    std::fs::write(&file, r#"{"profile": "from-a-newer-version"}"#).unwrap();
    let mut h = bimcraft(BOX);
    h.state_mut().use_settings(file.clone());
    h.run();
    assert_eq!(h.state().profile, Profile::BimCraft);
    press(&mut h, "Settings");
    pick(&mut h, Role::RadioButton, "Rev");
    press(&mut h, "Close");
    assert_eq!(h.state().profile, Profile::Rev);
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept["profile"], "rev");
    // A window started on the same file is in the profile this one was left in, and BIMCraft puts its own look back.
    let mut again = bimcraft(BOX);
    again.state_mut().use_settings(file.clone());
    again.run();
    assert_eq!(again.state().profile, Profile::Rev);
    press(&mut again, "Settings");
    pick(&mut again, Role::RadioButton, "BIMCraft");
    press(&mut again, "Close");
    assert_eq!(again.state().profile, Profile::BimCraft);
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept["profile"], "bimcraft");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn rev_leads_the_ribbons_tabs_with_file_and_docks_the_options_bar_under_the_ribbon_so_the_view_never_moves() {
    let mut h = bimcraft(BOX);
    let (file, tab) = (h.get_by_label("File").rect(), h.get_by_label("Architecture").rect());
    assert!(file.bottom() < tab.top(), "BIMCraft's File is on the app bar, over the tabs: {file:?} {tab:?}");
    h.state_mut().profile = Profile::Rev;
    h.run();
    h.run();
    let (file, tab) = (h.get_by_label("File").rect(), h.get_by_label("Architecture").rect());
    assert!((file.center().y - tab.center().y).abs() < 1.0 && file.right() <= tab.left(), "Rev's File leads the tabs: {file:?} {tab:?}");
    let (view, bar) = (h.state().view_rect, h.state().options_rect);
    assert!(
        bar.bottom() <= view.top() && bar.width() > view.width(),
        "the bar is a strip across the window over the view: {bar:?} {view:?}"
    );
    tool(&mut h, "Wall");
    let field = h.get_by_label("Location Line:").rect();
    assert!(h.state().options_rect.contains_rect(field), "the tool's options are on it: {field:?}");
    assert_eq!(h.state().view_rect, view, "it is there with or without a tool's options, so the view never moves under them");
    // A click on the bar starts no wall.
    click_at(&mut h, field.left_center() - vec2(30.0, 0.0));
    assert_eq!(ids(&h, "wall").len(), 4);
}

#[test]
fn rev_is_flush_light_chrome_whatever_layout_bimcraft_keeps() {
    let mut h = bimcraft(BOX);
    let ground = app_bar_ground(&h);
    assert!(luminance(ground) < 0.1, "BIMCraft's app bar is dark in the light theme: {ground:?}");
    h.state_mut().profile = Profile::Rev;
    h.run();
    h.run();
    let ground = app_bar_ground(&h);
    assert!(luminance(ground) > 0.6, "Rev's is light chrome: {ground:?}");
    // The view's tabs are a strip above the view, as they are Compact, and BIMCraft's layout (Floating) is left as it was.
    let view = h.state().view_rect;
    let tab = h.state().tab_rects.first().map(|(_, r)| *r).unwrap();
    assert!(tab.bottom() <= view.top(), "{tab:?} over {view:?}");
    assert!(!h.state().compact);
    press(&mut h, "Settings");
    let floating = h.get_by_role_and_label(Role::RadioButton, "Floating");
    assert!(floating.accesskit_node().is_disabled(), "Rev has one layout: the choice is BIMCraft's");
}

/// The hairlines in the border's colour the window drew by the row whose value's field is at `field`: (under it, running across
/// the row from its name to the field's end; between the name and the field, running down past it).
fn rules_by(h: &Harness<BimApp>, field: egui::Rect) -> (usize, usize) {
    fn walk(s: &Shape, out: &mut Vec<[egui::Pos2; 2]>) {
        match s {
            Shape::LineSegment { points, stroke } if stroke.color == crate::style::pal().border => out.push(*points),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut lines = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut lines));
    let under = lines
        .iter()
        .filter(|[a, b]| {
            (a.y - b.y).abs() < 0.5
                && a.y > field.bottom()
                && a.y < field.bottom() + 8.0
                && a.x.min(b.x) < field.left() - 60.0
                && a.x.max(b.x) >= field.right() - 1.0
        })
        .count();
    let beside = lines
        .iter()
        .filter(|[a, b]| {
            (a.x - b.x).abs() < 0.5
                && a.x < field.left()
                && a.x > field.left() - 20.0
                && a.y.min(b.y) < field.top()
                && a.y.max(b.y) > field.bottom()
        })
        .count();
    (under, beside)
}

#[test]
fn revs_properties_are_a_ruled_grid_and_apply_stays_at_the_palettes_foot_while_the_rows_scroll() {
    let mut h = bimcraft(BOX);
    click(&mut h, P2(0.0, 3000.0));
    assert_eq!(h.state().selection.len(), 1, "a wall, whose rows run past the palette");
    // The row's field, which its name labels.
    let field = h.get_by_label("Base Offset").rect();
    assert_eq!(rules_by(&h, field), (0, 0), "BIMCraft's rows are not ruled");
    h.state_mut().profile = Profile::Rev;
    h.run();
    h.run();
    let field = h.get_by_label("Base Offset").rect();
    let (under, beside) = rules_by(&h, field);
    assert!(under == 1 && beside >= 1, "a line under the row and one between its name and its value: {under}, {beside}");
    // Apply is at the palette's foot, over the Project Browser's title, though the rows run on past it.
    let (apply, browser) = (h.get_by_label("Apply").rect(), h.get_by_label_contains("Project Browser ·").rect());
    assert!(apply.bottom() <= browser.top() && browser.top() - apply.bottom() < 16.0, "{apply:?} over {browser:?}");
    assert!(field.bottom() < apply.top());
}

#[test]
fn bimcrafts_tools_are_a_palette_in_place_of_the_ribbon_and_dock_where_they_are_put() {
    use crate::dock::{Palette, Side, Target};
    let mut h = bimcraft(BOX);
    assert!(h.query_by_label("Minimize the Ribbon").is_none(), "no ribbon");
    let docking = &h.state().docking;
    assert_eq!(docking.docked(Palette::Tools), Some(Side::Left), "the tools down the left");
    assert_eq!((docking.docked(Palette::Properties), docking.docked(Palette::Browser)), (Some(Side::Right), Some(Side::Right)));
    // A tab picked on the rail, a tool from its rows: the tool's own (contextual) rows come first, named for it.
    tool(&mut h, "Wall");
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    let (context, view) = (h.get_by_label("Modify | Place Wall").rect(), h.state().view_rect);
    assert!(context.right() <= view.left(), "in the column left of the view: {context:?} {view:?}");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    press(&mut h, "Structure");
    assert!(h.query_by_label("Beam").is_some(), "the Structure tab's tools");
    // Moved like any palette: docked on the right, the column is right of the view.
    h.state_mut().docking.put(Palette::Tools, Target::Side(Side::Right));
    h.run();
    h.run();
    assert!(h.get_by_label("Structure").rect().left() >= h.state().view_rect.right());
    // Rev has the ribbon, and no Tools palette.
    h.state_mut().profile = Profile::Rev;
    h.run();
    h.run();
    assert!(!h.state().docking.shown(Palette::Tools) && h.query_by_label("Minimize the Ribbon").is_some());
}

#[test]
fn bimcrafts_inspector_and_navigator_are_the_properties_and_the_browser_on_the_right() {
    let mut h = bimcraft(BOX);
    let (inspector, view) = (h.get_by_label("Inspector").rect(), h.state().view_rect);
    assert!(inspector.left() >= view.right(), "{inspector:?} right of {view:?}");
    assert!(h.query_by_label_contains("Navigator ·").is_some());
    h.state_mut().profile = Profile::Rev;
    h.run();
    assert!(h.query_by_label("Inspector").is_none() && h.query_by_label_contains("Project Browser ·").is_some(), "Rev's own names");
}

#[test]
fn find_runs_a_tool_or_opens_a_view_by_what_is_typed_in_both_profiles() {
    for profile in [Profile::BimCraft, Profile::Rev] {
        let mut h = bimcraft(BOX);
        h.state_mut().profile = profile;
        h.run();
        let find = |h: &mut Harness<BimApp>, text: &str| {
            h.key_press_modifiers(Modifiers::COMMAND, Key::K);
            h.run();
            assert!(h.state().find.is_some(), "Ctrl+K opens Find ({profile:?})");
            h.event(egui::Event::Text(text.to_string()));
            h.run();
            key(h, Key::Enter);
        };
        find(&mut h, "wall");
        assert!(matches!(h.state().tool, Tool::Wall(_)), "{profile:?}");
        assert!(h.state().find.is_none(), "and closes");
        key(&mut h, Key::Escape);
        key(&mut h, Key::Escape);
        let level_2 = h.state().session.model.levels()[1].0;
        find(&mut h, "plan level 2");
        assert_eq!(h.state().sheet, Sheet::Plan(level_2), "{profile:?}");
    }
    // BIMCraft's title bar has a button for it.
    let mut h = bimcraft(BOX);
    press(&mut h, "Find");
    assert!(h.state().find.is_some());
    key(&mut h, Key::Escape);
    assert!(h.state().find.is_none(), "Escape closes it");
}

#[test]
fn the_tools_rail_scrolls_when_the_palette_is_too_short_for_its_tabs() {
    let mut h = bimcraft(BOX);
    h.set_size(vec2(1280.0, 420.0));
    h.run();
    h.run();
    // The rail's own Manage, down the left: the rows name their tab in paint, not with a button.
    let manage = |h: &Harness<BimApp>| h.get_all_by_label("Manage").map(|n| n.rect()).find(|r| r.left() < 60.0).unwrap();
    let before = manage(&h);
    assert!(before.bottom() > 420.0, "the window is too short for every tab: {before:?}");
    h.hover_at(pos2(before.center().x, 200.0));
    h.event(Event::MouseWheel {
        phase: TouchPhase::Move,
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -600.0),
        modifiers: Modifiers::NONE,
    });
    h.run();
    let after = manage(&h);
    assert!(after.bottom() < 400.0, "the wheel over the rail brings it into view: {after:?}");
    click_at(&mut h, after.center());
    assert_eq!(h.state().tab, Tab::Manage);
}
