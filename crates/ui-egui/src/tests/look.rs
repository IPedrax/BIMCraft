//! The window's look: the Floating and Compact layouts, and the cards round the view.

use super::*;

/// The left edge of the ribbon's first tab.
fn first_tab(h: &Harness<BimApp>) -> f32 {
    h.get_by_label("Architecture").rect().left()
}

#[test]
fn the_compact_layout_puts_the_cards_flush_and_the_settings_file_keeps_it() {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-look-{}", std::process::id())).join("settings.json");
    let _ = std::fs::remove_file(&file);
    let mut h = bimcraft(BOX);
    h.state_mut().use_settings(file.clone());
    h.run();
    let floating = first_tab(&h);
    press(&mut h, "Settings");
    // Below the Settings page's fold: pressed as a screen reader does.
    h.get_by_role_and_label(Role::RadioButton, "Compact").click_accesskit();
    h.run();
    press(&mut h, "Close");
    assert!(h.state().compact);
    let flush = first_tab(&h);
    assert_eq!(floating - flush, f32::from(crate::style::GAP), "the ribbon's card no longer stands off the window's edge");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept["layout"], "compact");
    // A window started on the same file is laid out as this one was left, and Floating puts the cards back.
    let mut again = bimcraft(BOX);
    again.state_mut().use_settings(file.clone());
    again.run();
    assert!(again.state().compact);
    assert_eq!(first_tab(&again), flush);
    press(&mut again, "Settings");
    again.get_by_role_and_label(Role::RadioButton, "Floating").click_accesskit();
    again.run();
    press(&mut again, "Close");
    assert_eq!(first_tab(&again), floating);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_click_on_a_card_or_between_cards_is_no_click_on_the_view() {
    let mut h = bimcraft(BOX);
    tool(&mut h, "Wall");
    let walls = ids(&h, "wall").len();
    let view = h.state().view_rect;
    // On the Options Bar's card, on the status bar's, and in the gap between the Properties card and the view.
    let options = h.get_by_label("Modify | Place Wall").rect().center();
    for at in [options, pos2(view.center().x, view.bottom() + 20.0), pos2(view.left() - 3.0, view.center().y)] {
        click_at(&mut h, at);
        click_at(&mut h, at + vec2(40.0, 0.0));
    }
    assert_eq!(ids(&h, "wall").len(), walls, "no wall was drawn");
}

#[test]
fn the_views_tabs_and_its_control_bar_float_over_it_and_a_click_on_them_is_no_click_on_the_view() {
    let mut h = bimcraft(BOX);
    let view = h.state().view_rect;
    let tab = h.state().tab_rects.first().map(|(_, r)| *r).unwrap();
    let bar = h.get_by_label("Temporary Hide/Isolate").rect().union(h.get_by_label("Reveal Hidden Elements").rect());
    // The view runs under both: no band of its own above or below it.
    assert!(view.contains_rect(tab) && view.contains_rect(bar), "{tab:?} and {bar:?} in {view:?}");
    assert!(view.bottom() - bar.bottom() < 20.0 && tab.top() - view.top() < 20.0);
    // Clicks on them are theirs: the wall tool starts no wall from the tab or the bar.
    tool(&mut h, "Wall");
    for at in [tab.center(), bar.center()] {
        click_at(&mut h, at);
        click_at(&mut h, at + vec2(0.0, 2.0));
    }
    assert_eq!(ids(&h, "wall").len(), 4);
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // A fitted plan keeps clear of them: the house is between the tabs and the bar.
    let (south, north) = (h.state().screen(P2(4000.0, 0.0)), h.state().screen(P2(4000.0, 6000.0)));
    assert!(north.y > tab.bottom() && south.y < bar.top(), "{north:?} {south:?}");
    // Compact, they are bars round the view again.
    press(&mut h, "Settings");
    // Below the Settings page's fold: pressed as a screen reader does.
    h.get_by_role_and_label(Role::RadioButton, "Compact").click_accesskit();
    h.run();
    press(&mut h, "Close");
    let view = h.state().view_rect;
    let tab = h.state().tab_rects.first().map(|(_, r)| *r).unwrap();
    assert!(tab.bottom() <= view.top() && h.get_by_label("Reveal Hidden Elements").rect().top() >= view.bottom());
}

#[test]
fn the_options_bar_floats_over_the_view_while_it_has_something_on_it_and_the_view_never_moves() {
    let mut h = bimcraft(BOX);
    let view = h.state().view_rect;
    assert!(h.query_by_label("Location Line:").is_none(), "with nothing to set there's no bar");
    tool(&mut h, "Wall");
    assert_eq!(h.state().view_rect, view, "the bar coming takes no room from the view");
    let field = h.get_by_label("Location Line:").rect();
    assert!(view.contains_rect(field), "it lies over the view: {field:?} in {view:?}");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(0.0, 3000.0));
    assert_eq!(h.state().selection.len(), 1);
    assert_eq!(h.state().view_rect, view, "nor does a selection's");
}

#[test]
fn a_click_on_the_floating_options_bar_is_no_click_on_the_view_under_it() {
    let mut h = bimcraft(BOX);
    tool(&mut h, "Wall");
    let label = h.get_by_label("Location Line:").rect();
    // On the bar's label, and on the bar just left of it.
    for at in [label.center(), label.left_center() - vec2(4.0, 0.0)] {
        click_at(&mut h, at);
    }
    assert!(ids(&h, "wall").len() == 4, "no wall was started from the bar");
    // Off it, the tool works as ever.
    click(&mut h, P2(2000.0, 3000.0));
    click(&mut h, P2(5000.0, 3000.0));
    assert_eq!(ids(&h, "wall").len(), 5);
}

/// What shows in the gaps between the cards is the view's own ground, so in a dark interface with a light canvas it is paper, not
/// the interface's dark.
#[test]
fn the_ground_under_the_cards_is_the_canvas_in_all_three_looks() {
    use egui::{Color32, ThemePreference};
    for (theme, light_canvas, paper) in
        [(ThemePreference::Light, false, true), (ThemePreference::Dark, false, false), (ThemePreference::Dark, true, true)]
    {
        let mut h = bimcraft(BOX);
        h.state_mut().light_canvas = light_canvas;
        h.ctx.set_theme(theme);
        h.run();
        h.run();
        let ground = h.state().backdrop();
        assert_eq!(ground == Color32::WHITE, paper, "{theme:?}, light canvas {light_canvas}: {ground:?}");
        assert_eq!(ground, crate::canvas::color(bimcraft_core::render::PAPER));
    }
}
