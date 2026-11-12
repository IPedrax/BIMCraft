//! Dialog title bars move the whole window; their close buttons discard drafts and their footers stay in reach.
use super::*;

fn area(h: &Harness<BimApp>, id: &str) -> egui::Rect {
    h.ctx.memory(|m| m.area_rect(egui::Id::new(id))).expect("open dialog")
}

fn action(h: &Harness<BimApp>, id: &str, label: &str) -> egui::Rect {
    let r = area(h, id);
    h.get_all_by_role_and_label(Role::Button, label)
        .map(|n| n.rect())
        .find(|button| r.contains_rect(*button) && button.center().x > r.center().x)
        .unwrap_or_else(|| panic!("{label} is not in the dialog {r:?}"))
}

fn close(h: &mut Harness<BimApp>, id: &str) {
    let r = area(h, id);
    let button = h
        .get_all_by_role(Role::Button)
        .find(|n| r.contains_rect(n.rect()) && n.rect().center().x > r.right() - 45.0 && n.rect().center().y < r.top() + 45.0)
        .expect("corner close button");
    assert!(button.rect().center().x > r.right() - 45.0);
    assert!(button.rect().center().y < r.top() + 45.0);
    button.click();
    h.run();
}

#[test]
fn popup_title_bar_moves_vg_and_its_actions_stay_at_the_bottom_on_every_tab() {
    let mut h = app(BOX);
    shortcut(&mut h, "VG");
    let before = area(&h, "visibility-graphics");
    let title = h.get_by_label("Visibility/Graphics Overrides for Floor Plan: Level 1").rect().center();
    let delta = vec2(65.0, 40.0);
    drag_screen(&mut h, title, title + delta, PointerButton::Primary);
    let moved = area(&h, "visibility-graphics");
    assert!((moved.min - before.min - delta).length() < 2.0, "{before:?} -> {moved:?}");
    for tab in ["Model Categories", "Annotation Categories", "Imported Categories", "Filters", "Design Options"] {
        press(&mut h, tab);
        let r = area(&h, "visibility-graphics");
        for label in ["OK", "Cancel", "Apply"] {
            let button = action(&h, "visibility-graphics", label);
            assert!(r.contains_rect(button));
            assert!(r.bottom() - button.bottom() < 25.0, "{tab}: {label}: {button:?} in {r:?}");
        }
    }
    close(&mut h, "visibility-graphics");
    assert!(h.state().vg_dialog.is_none());
}

#[test]
fn popup_corner_close_discards_type_and_nested_vg_drafts() {
    let mut h = app(BOX);
    let model = h.state().session.model.to_json().unwrap();
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.state_mut().edit_type();
    h.run();
    type_role(&mut h, Role::TextInput, "Width", "321");
    close(&mut h, "type-properties");
    assert!(h.state().type_dialog.is_none());
    assert_eq!(h.state().session.model.to_json().unwrap(), model);
    shortcut(&mut h, "VG");
    pick(&mut h, Role::CheckBox, "Doors");
    pick(&mut h, Role::Button, "Floors: Transparency");
    close(&mut h, "vg-cell");
    assert!(h.state().vg_dialog.is_some());
    assert!(h.query_by_label("Surface Transparency").is_none());
    close(&mut h, "visibility-graphics");
    assert!(h.state().vg_dialog.is_none());
    assert_eq!(h.state().session.model.to_json().unwrap(), model);
}

#[test]
fn popup_actions_and_close_stay_visible_in_a_short_window_even_with_a_type_error() {
    let mut h = app(BOX);
    h.set_size(vec2(1000.0, 400.0));
    shortcut(&mut h, "VG");
    let screen = h.ctx.content_rect();
    for label in ["OK", "Cancel", "Apply"] {
        assert!(screen.contains_rect(action(&h, "visibility-graphics", label)));
    }
    assert!(screen.contains_rect(area(&h, "visibility-graphics")));
    close(&mut h, "visibility-graphics");
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.state_mut().edit_type();
    h.run();
    type_role(&mut h, Role::TextInput, "Width", "invalid");
    let apply = action(&h, "type-properties", "Apply").center();
    click_at(&mut h, apply);
    assert!(h.state().type_dialog.is_some());
    for label in ["OK", "Cancel", "Apply"] {
        assert!(screen.contains_rect(action(&h, "type-properties", label)));
    }
    assert!(screen.contains_rect(area(&h, "type-properties")));
    close(&mut h, "type-properties");
    assert!(h.state().type_dialog.is_none());
}

#[test]
fn a_click_beside_an_open_dialog_leaves_it_open_and_only_its_buttons_and_corner_close_it() {
    let mut h = app(BOX);
    shortcut(&mut h, "VG");
    let r = area(&h, "visibility-graphics");
    // Beside the dialog, on the dimmed window behind it, left of it and right of it.
    for pos in [pos2(r.left() - 20.0, r.center().y), pos2(r.right() + 20.0, r.center().y)] {
        click_at(&mut h, pos);
        assert!(h.state().vg_dialog.is_some(), "a click at {pos:?} closed it");
    }
    press(&mut h, "Cancel");
    assert!(h.state().vg_dialog.is_none());
}

/// The family `Box` open as a document, `w` by `h`.
fn family_box(w: f32, height: f32) -> Harness<'static, BimApp> {
    let mut h = app(
        r#"family {"name": "Box", "category": "furniture", "parameters": [{"name": "Width", "value": 1000}], "types": {"A": {}}, "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", 1000], [0, 1000]], "to": 500}]}"#,
    );
    h.set_size(vec2(w, height));
    h.state_mut().edit_family("Box");
    h.run();
    h
}

/// The open dialog `id`'s height over `frames` frames, each frame's.
fn heights(h: &mut Harness<BimApp>, id: egui::Id, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|_| {
            h.step();
            h.ctx.memory(|m| m.area_rect(id)).expect("open dialog").height()
        })
        .collect()
}

fn family_dialog(h: &mut Harness<BimApp>, which: crate::editor::DocDialog) -> egui::Id {
    h.state_mut().active_family_mut().unwrap().dialog = Some(which);
    egui::Id::new(("family-dialog", which as u8))
}

/// The widget named `label` furthest right: the dialog's, over the Properties palette's on the left.
fn right(h: &Harness<BimApp>, label: &str) -> egui::Rect {
    h.get_all_by_label(label).map(|n| n.rect()).max_by(|a, b| a.center().x.total_cmp(&b.center().x)).unwrap()
}

/// Whether the dialog's height settles at once (from the frame after its sizing pass) and stays.
fn settles(h: &mut Harness<BimApp>, id: egui::Id) -> f32 {
    let seen = heights(h, id, 6);
    assert!(seen[1..].windows(2).all(|w| (w[0] - w[1]).abs() < 0.5), "it keeps its size: {seen:?}");
    seen[5]
}

#[test]
fn family_dialogs_are_as_tall_as_what_they_hold_and_stay_so() {
    use crate::editor::DocDialog;
    // Each dialog's last widget before its buttons.
    for (which, last) in [(DocDialog::Category, "Default Elevation"), (DocDialog::Types, "Descending")] {
        let mut h = family_box(1600.0, 1000.0);
        let id = family_dialog(&mut h, which);
        let height = settles(&mut h, id);
        let r = h.ctx.memory(|m| m.area_rect(id)).unwrap();
        let (end, ok) = (right(&h, last), right(&h, "OK"));
        assert!(ok.top() - end.bottom() < 40.0, "{which:?}: no blank band over the buttons: {end:?} then {ok:?}");
        assert!(r.bottom() - ok.bottom() < 25.0, "{which:?}: the buttons at its bottom: {ok:?} in {r:?}");
        assert!(height < 1000.0 - 100.0, "{which:?}: well short of the window: {height}");
    }
}

#[test]
fn a_dialog_shrinks_again_when_what_it_holds_does() {
    let mut h = family_box(1600.0, 1000.0);
    let id = family_dialog(&mut h, crate::editor::DocDialog::Category);
    let base = settles(&mut h, id);
    // Room Calculation Point's three rows come and go.
    let tick = |h: &mut Harness<BimApp>| {
        let at = right(h, "Room Calculation Point").center();
        h.get_all_by_label("Room Calculation Point").find(|n| n.rect().center() == at).unwrap().click();
        h.run();
    };
    tick(&mut h);
    let taller = settles(&mut h, id);
    assert!(taller > base + 50.0, "{base} -> {taller}");
    tick(&mut h);
    assert!((settles(&mut h, id) - base).abs() < 1.0, "back to its height");
    // A filter narrowing the category list to one.
    let at = right(&h, "Filter").center();
    h.get_all_by_label("Filter").find(|n| n.rect().center() == at).unwrap().focus();
    h.run();
    h.get_all_by_label("Filter").find(|n| n.rect().center() == at).unwrap().type_text("door");
    h.run();
    let narrowed = settles(&mut h, id);
    assert!(narrowed < base - 60.0, "{base} -> {narrowed}");
}

#[test]
fn a_long_dialog_is_capped_at_the_window_and_scrolls_with_its_buttons_in_reach() {
    let mut h = family_box(1400.0, 480.0);
    let id = family_dialog(&mut h, crate::editor::DocDialog::Category);
    let height = settles(&mut h, id);
    let r = h.ctx.memory(|m| m.area_rect(id)).unwrap();
    assert!(height <= 480.0 - 8.0 && r.top() >= 4.0 && r.bottom() <= 476.0, "within the window, a margin round it: {r:?}");
    let ok = right(&h, "OK");
    assert!(r.contains_rect(ok) && ok.bottom() <= 480.0, "its buttons in reach: {ok:?} in {r:?}");
    assert!(right(&h, "Default Elevation").top() > ok.top(), "the rest of the form is below, scrolled to");
}

#[test]
fn every_dialog_keeps_its_size_from_frame_to_frame_with_its_buttons_at_the_bottom() {
    use crate::editor::DocDialog;
    let check = |h: &mut Harness<BimApp>, id: egui::Id, name: &str| {
        settles(h, id);
        let r = h.ctx.memory(|m| m.area_rect(id)).unwrap();
        let bottom =
            h.get_all_by_role(Role::Button).map(|n| n.rect()).filter(|b| r.contains_rect(*b)).map(|b| b.bottom()).fold(0.0, f32::max);
        assert!(r.bottom() - bottom < 25.0, "{name}: buttons at the bottom of {r:?}, lowest at {bottom}");
    };
    let mut h = app(BOX);
    shortcut(&mut h, "VG");
    check(&mut h, egui::Id::new("visibility-graphics"), "Visibility/Graphics");
    let mut h = app(BOX);
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    h.state_mut().edit_type();
    h.run();
    check(&mut h, egui::Id::new("type-properties"), "Type Properties");
    for which in [DocDialog::Category, DocDialog::Types] {
        let mut h = family_box(1600.0, 1000.0);
        let id = family_dialog(&mut h, which);
        check(&mut h, id, &format!("{which:?}"));
    }
}
