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
