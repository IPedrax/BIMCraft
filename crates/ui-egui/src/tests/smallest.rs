//! The window at the least it can be (`window::MIN_SIZE`, which `main` gives as the window's minimum): nothing runs under, over or
//! past anything else there. Each test runs in the window's real geometry (`titlebar::own`: the root `Ui` is the window).

use egui::{Rect, Shape, pos2, vec2};

use super::titlebar::own;
use super::*;
use crate::window::MIN_SIZE;

fn least() -> egui::Vec2 {
    vec2(MIN_SIZE[0], MIN_SIZE[1])
}

fn window(size: egui::Vec2) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), size)
}

/// The cards the window drew: the ribbon, the palettes, the status bar (their ground is the panel colour, and each is wide and tall
/// enough that no button or field is taken for one).
fn cards(h: &Harness<BimApp>) -> Vec<Rect> {
    fn walk(s: &Shape, out: &mut Vec<Rect>) {
        match s {
            Shape::Rect(r) if r.fill == crate::style::pal().panel && r.rect.width() >= 250.0 && r.rect.height() >= 24.0 => out.push(r.rect),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The window brought down to the least a window can be: from the start, or from a bigger one the palettes were sized in.
fn at_the_least(compact: bool, from_bigger: bool) -> Harness<'static, BimApp> {
    let mut h = own(BOX);
    h.state_mut().compact = compact;
    if from_bigger {
        h.run();
    } else {
        h.set_size(least());
    }
    h.set_size(least());
    h.run();
    h.run();
    h
}

#[test]
fn no_card_runs_under_another_or_off_the_window_at_the_least_a_window_can_be() {
    for compact in [false, true] {
        for from_bigger in [false, true] {
            let h = at_the_least(compact, from_bigger);
            let cards = cards(&h);
            assert!(cards.len() >= 4, "the ribbon, two palettes and the status bar (compact={compact}): {cards:?}");
            let w = window(least());
            for (i, a) in cards.iter().enumerate() {
                assert!(w.expand(0.5).contains_rect(*a), "{a:?} is off the window (compact={compact}, from_bigger={from_bigger})");
                for b in &cards[i + 1..] {
                    let both = a.intersect(*b);
                    assert!(
                        !(both.is_positive() && both.width() > 1.0 && both.height() > 1.0),
                        "{a:?} runs under {b:?} (compact={compact}, from_bigger={from_bigger})"
                    );
                }
            }
        }
    }
}

/// The title the app bar paints (the project and the view), as the rectangle it covers.
fn title_of(h: &Harness<BimApp>) -> Rect {
    fn walk(s: &Shape, out: &mut Vec<Rect>) {
        match s {
            Shape::Text(t) if t.galley.text().contains(" · ") && t.visual_bounding_rect().center().y < 34.0 => {
                out.push(t.visual_bounding_rect())
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.pop().expect("the app bar's title")
}

#[test]
fn the_title_stays_between_the_quick_access_and_the_caption_buttons_whatever_the_projects_name() {
    let long = "Long ".repeat(40);
    let names = ["House", "A Project With A Rather Long Name For A Small Window, Its Second Phase", long.as_str()];
    for size in [least(), vec2(1280.0, 800.0)] {
        for name in names {
            let mut h = own(BOX);
            h.set_size(size);
            std::sync::Arc::make_mut(&mut h.state_mut().session.model).name = name.to_string();
            h.run();
            h.run();
            let (gear, captions) = (h.get_by_label("Settings").rect(), h.get_by_label("Minimize").rect());
            let title = title_of(&h);
            let said = format!("{name:?} in {size:?}: the title is {title:?}, between the gear's {gear:?} and the buttons' {captions:?}");
            assert!(title.left() >= gear.right() && title.right() <= captions.left(), "{said}");
            if name == "House" && size.x > 1000.0 {
                assert!((title.center().x - size.x / 2.0).abs() < 1.0, "a title that fits is in the window's middle: {said}");
            }
        }
    }
}

#[test]
fn the_tab_just_picked_is_in_view_though_the_tabs_run_past_the_window() {
    let mut h = own(BOX);
    h.set_size(least());
    h.run();
    let in_view = |h: &Harness<BimApp>, label: &str| {
        let (tab, fold) = (h.get_by_label(label).rect(), h.get_by_label("Minimize the Ribbon").rect());
        assert!(
            tab.left() >= 0.0 && tab.right() <= fold.left(),
            "`{label}` {tab:?} is in the tabs, which end where the fold button {fold:?} starts"
        );
    };
    // A tab far along the row.
    h.state_mut().tab = crate::Tab::Manage;
    h.run();
    h.run();
    in_view(&h, "Manage");
    // And a tool's contextual tab, which comes after them all.
    h.state_mut().tab = crate::Tab::Architecture;
    h.run();
    tool(&mut h, "Wall");
    h.run();
    in_view(&h, "Modify | Place Wall");
}

#[test]
fn the_options_bar_stays_clear_of_the_zoom_buttons_and_inside_the_window_at_the_least_a_window_can_be() {
    let mut h = own(BOX);
    h.set_size(least());
    h.run();
    tool(&mut h, "Wall");
    h.run();
    // The bar itself, and each of its fields, are in the window; none of the view's zoom buttons is under any of them.
    let bar = h.state().options_rect;
    let w = window(least());
    assert!(w.contains_rect(bar), "the bar {bar:?} is off the window");
    for (label, role) in [("Height:", Role::Label), ("Location Line:", Role::Label), ("Chain", Role::CheckBox)] {
        let r = h.get_by_role_and_label(role, label).rect();
        assert!(bar.contains_rect(r), "`{label}` {r:?} is out of the bar {bar:?}");
    }
    for label in ["Zoom to Fit", "Zoom In", "Zoom Out"] {
        let button = h.get_by_label(label).rect();
        assert!(!button.intersect(bar).is_positive(), "`{label}` {button:?} is under the Options Bar {bar:?}");
        assert!(w.contains_rect(button), "`{label}` {button:?} is off the window");
    }
}

/// A project with forty fill patterns, forty region types, forty line patterns and forty line styles, on the window at the least
/// a window can be.
fn crowded() -> Harness<'static, BimApp> {
    let mut h = at_the_least(false, false);
    let s = &mut h.state_mut().session;
    for i in 0..40 {
        let angle = f64::from(i) * 4.0;
        s.execute("fill_pattern", &json!({"name": format!("Hatch {i:02}"), "lines": [{"angle": angle, "spacing": 5.0}]})).unwrap();
        s.execute("filled_region_type", &json!({"name": format!("Region {i:02}"), "foreground": format!("Hatch {i:02}")})).unwrap();
        s.execute(
            "line_pattern",
            &json!({"name": format!("Dashes {i:02}"), "segments": [{"dash": 3.0 + f64::from(i) / 10.0}, {"space": 1.5}]}),
        )
        .unwrap();
        s.execute("line_style", &json!({"name": format!("Style {i:02}"), "pen": 2, "pattern": format!("Dashes {i:02}")})).unwrap();
    }
    h.run();
    h
}

/// A PAT file of many patterns the dialog can't take, to leave a long note in it.
fn long_note(h: &mut Harness<BimApp>) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bad.pat");
    std::fs::write(&file, (0..60).map(|i| format!("*A Pattern With A Rather Long Name Number {i}\nnot numbers\n")).collect::<String>())
        .unwrap();
    h.state_mut().import_patterns(&file);
    h.run();
    h.run();
}

/// Whether each of `labels` and the dialog `modal` itself lies inside the window.
fn dialog_fits(h: &Harness<BimApp>, modal: &str, labels: &[&str], said: &str) {
    let w = window(least());
    let area = h.ctx.memory(|m| m.area_rect(egui::Id::new(modal))).expect("the dialog is open");
    assert!(w.expand(0.5).contains_rect(area), "{said}: the dialog {area:?} runs off the window {w:?}");
    for label in labels {
        // (The ribbon behind the dialog may carry the same word.)
        let found: Vec<Rect> = h.get_all_by_label(label).map(|n| n.rect()).filter(|r| r.intersects(area)).collect();
        assert!(!found.is_empty(), "{said}: `{label}` is not in the dialog {area:?}");
        for r in found {
            assert!(w.expand(0.5).contains_rect(r), "{said}: `{label}` {r:?} is off the window {w:?}");
        }
    }
}

#[test]
fn fill_patterns_and_edit_type_with_forty_patterns_and_a_long_note_stay_inside_the_least_window_with_their_buttons_in_reach() {
    let buttons = ["OK", "Cancel", "Apply"];
    // Manage's dialog: the patterns.
    let mut h = crowded();
    let model = h.state().session.model.clone();
    h.state_mut().fill_patterns_dialog = Some(crate::fillpatterns::FillPatternsDialog::new(&model));
    h.run();
    let patterns = [&["Fill Patterns", "New Pattern", "Import...", "Export..."][..], &buttons].concat();
    dialog_fits(&h, "fill-patterns", &patterns, "Patterns");
    long_note(&mut h);
    dialog_fits(&h, "fill-patterns", &patterns, "Patterns with a long note");
    // A pattern's families open under the list, twenty of them: the list and they share what the window has.
    press(&mut h, "Hatch 00: Edit Lines");
    for _ in 0..20 {
        press(&mut h, "Add Line");
    }
    dialog_fits(&h, "fill-patterns", &[&["Add Line", "Done", "New Pattern"][..], &buttons].concat(), "Patterns with its families open");
    press(&mut h, "Done");
    // Edit Type: the region's own type and, on its Patterns tab, the same list.
    let mut h = crowded();
    let model = h.state().session.model.clone();
    h.state_mut().fill_patterns_dialog = Some(crate::fillpatterns::FillPatternsDialog::edit(&model, "Region 39", Vec::new()));
    h.run();
    dialog_fits(&h, "fill-patterns", &[&["Type Properties"][..], &buttons].concat(), "Edit Type");
    press(&mut h, "Foreground Pattern: Edit Patterns");
    long_note(&mut h);
    dialog_fits(&h, "fill-patterns", &[&["Type Properties", "New Pattern", "Import..."][..], &buttons].concat(), "Edit Type's Patterns");
}

/// Edit Type in a window shorter than the least a window can be (a tiling window manager can give one): the form scrolls in the
/// height left, and OK, Cancel and Apply stay in the window.
#[test]
fn edit_type_scrolls_in_a_window_too_short_for_it_so_its_buttons_stay_in_reach() {
    let short = vec2(MIN_SIZE[0], 400.0);
    let mut h = crowded();
    h.set_size(short);
    let model = h.state().session.model.clone();
    h.state_mut().fill_patterns_dialog = Some(crate::fillpatterns::FillPatternsDialog::edit(&model, "Region 39", Vec::new()));
    h.run();
    h.run();
    let area = h.ctx.memory(|m| m.area_rect(egui::Id::new("fill-patterns"))).expect("the dialog is open");
    assert!(window(short).expand(0.5).contains_rect(area), "the dialog {area:?} runs off the window {short:?}");
    for button in ["OK", "Cancel", "Apply"] {
        // (The ribbon behind the dialog may carry the same word.)
        let found: Vec<Rect> = h.get_all_by_label(button).map(|n| n.rect()).filter(|r| r.intersects(area)).collect();
        assert!(!found.is_empty(), "`{button}` is not in the dialog {area:?}");
        assert!(found.iter().all(|r| window(short).expand(0.5).contains_rect(*r)), "`{button}` {found:?} is off the window {short:?}");
    }
}

#[test]
fn line_styles_with_forty_of_each_stay_inside_the_least_window_with_their_buttons_in_reach() {
    let mut h = crowded();
    let model = h.state().session.model.clone();
    h.state_mut().line_styles_dialog = Some(crate::linestyles::LineStylesDialog::new(&model));
    h.run();
    dialog_fits(&h, "line-styles", &["Line Styles", "New Style", "OK", "Cancel", "Apply"], "Styles");
    press(&mut h, "Patterns");
    dialog_fits(&h, "line-styles", &["Line Styles", "New Pattern", "OK", "Cancel", "Apply"], "Patterns");
    // A pattern opens its own creator, whose list scrolls independently.
    press(&mut h, "Dashes 00: Edit");
    for _ in 0..14 {
        press(&mut h, "Add Segment");
    }
    dialog_fits(
        &h,
        "line-pattern-creator",
        &["Line Pattern Creator", "Add Segment", "Save Pattern", "Cancel Pattern"],
        "a pattern's creator",
    );
}
