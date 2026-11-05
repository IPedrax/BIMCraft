//! The interface's profiles: BIMCraft's own (the default), its tools a palette, and Rev, the same look with the ribbon: the
//! floating cards, the dark app bar and the view's little tabs of both, Revit's palette layout and names.

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
fn revs_file_is_on_the_app_bar_over_the_ribbon_and_its_options_bar_floats_over_the_view() {
    let mut h = app(BOX);
    let (file, tab) = (h.get_by_label("File").rect(), h.get_by_label("Architecture").rect());
    assert!(file.bottom() < tab.top(), "File is on the app bar, over the ribbon's tabs: {file:?} {tab:?}");
    let view = h.state().view_rect;
    assert!(!h.state().options_rect.is_positive(), "no bar while nothing is on it");
    tool(&mut h, "Wall");
    let (field, bar) = (h.get_by_label("Location Line:").rect(), h.state().options_rect);
    assert!(bar.contains_rect(field), "the tool's options are on it: {field:?} {bar:?}");
    assert!(view.contains_rect(bar) && bar.width() < view.width(), "a card over the view: {bar:?} {view:?}");
    assert_eq!(h.state().view_rect, view, "coming, it takes no room, so the view never moves under it");
    // A click on the bar starts no wall.
    click_at(&mut h, field.left_center() - vec2(30.0, 0.0));
    assert_eq!(ids(&h, "wall").len(), 4);
}

/// The bands in the bar's colour the window drew as tall as a ribbon panel's name.
fn name_bands(h: &Harness<BimApp>) -> usize {
    fn walk(s: &Shape, out: &mut usize) {
        match s {
            Shape::Rect(r) if r.fill == crate::style::pal().bar && (r.rect.height() - 16.0).abs() < 0.5 => *out += 1,
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = 0;
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn rev_wears_the_floating_cards_the_dark_app_bar_and_the_views_little_tabs_and_compact_is_its_to_pick() {
    let mut h = bimcraft(BOX);
    let ground = app_bar_ground(&h);
    let mut h2 = app(BOX);
    assert_eq!(app_bar_ground(&h2), ground, "one app bar in both profiles");
    assert!(luminance(ground) < 0.1, "dark in the light theme: {ground:?}");
    assert_eq!(crate::style::pal().panel, crate::style::LIGHT.panel, "one palette in both profiles");
    let gap = f32::from(crate::style::GAP);
    assert!(h2.get_by_label("Architecture").rect().left() > gap, "the ribbon's card stands off the window's edge");
    assert_eq!(name_bands(&h2), 0, "the panels' names are on no bands");
    // The view fills the pane and its tabs float over its top left corner, a card as high as a tab.
    let view = h2.state().view_rect;
    let tab = h2.state().tab_rects.first().map(|(_, r)| *r).unwrap();
    assert!(view.contains_rect(tab), "{tab:?} over {view:?}");
    press(&mut h2, "Settings");
    let floating = h2.get_by_role_and_label(Role::RadioButton, "Floating");
    assert!(!floating.accesskit_node().is_disabled(), "Layout is every profile's");
    h2.get_by_role_and_label(Role::RadioButton, "Compact").click_accesskit();
    h2.run();
    press(&mut h2, "Close");
    assert!(h2.state().compact);
    let (view, tab) = (h2.state().view_rect, h2.state().tab_rects.first().map(|(_, r)| *r).unwrap());
    assert!(tab.bottom() <= view.top(), "Compact's tabs are a strip above the view: {tab:?} {view:?}");
    let _ = &mut h;
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
fn revs_properties_are_the_clean_rows_and_apply_comes_after_them() {
    let mut h = app(BOX);
    click(&mut h, P2(0.0, 3000.0));
    assert_eq!(h.state().selection.len(), 1, "a wall, whose rows run past the palette");
    let field = h.get_by_label("Base Offset").rect();
    assert_eq!(rules_by(&h, field), (0, 0), "no lines between or under the rows");
    let apply = h.get_by_label("Apply").rect();
    assert!(apply.top() > field.bottom() + 40.0, "Apply follows the rows, none of them over it: {apply:?} {field:?}");
    let browser = h.get_by_label_contains("Project Browser ·").rect();
    assert!(apply.top() > browser.top(), "scrolled past the palette's foot with the rows: {apply:?} {browser:?}");
}

/// The docked cards of a side from `from` to `to` across, as the frame painted them: tall, in the panels' colour.
fn palette_cards(h: &Harness<BimApp>, from: f32, to: f32) -> Vec<egui::Rect> {
    fn walk(s: &Shape, out: &mut Vec<egui::Rect>) {
        match s {
            Shape::Rect(r) if r.fill == crate::style::pal().panel && r.rect.height() > 100.0 => out.push(r.rect),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.retain(|r| r.left() >= from && r.left() < to && r.width() < 500.0);
    out
}

#[test]
fn a_long_value_in_a_views_properties_never_pushes_its_card_past_the_one_under_it() {
    // A view's Phasing group holds New Construction, wider than half a palette: Properties over the browser, in both profiles.
    for (mut h, from, to) in [(app(BOX), 0.0, 300.0), (bimcraft(BOX), 900.0, 1280.0)] {
        for look in ["plan", "3D"] {
            if look == "3D" {
                shortcut(&mut h, "3D");
            }
            let cards = palette_cards(&h, from, to);
            let [properties, browser] = cards.as_slice() else { panic!("{look}: two cards: {cards:?}") };
            assert_eq!(properties.right(), browser.right(), "{look}: {properties:?} {browser:?}");
        }
    }
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
