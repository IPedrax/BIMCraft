//! Copy, Cut and Paste through the app, with no window: the engine makes what the core's paste says, the clipboard is the text it keeps.

use bimcraft_core::geom::P2;
use bimcraft_core::model::Element;

use crate::app::App;
use crate::clip::Mode;
use crate::select::How;
use crate::views::ViewKind;

fn with_wall() -> (App, u64) {
    let mut app = App::sample();
    let wall = app.session.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    app.select(vec![wall]);
    (app, wall)
}

fn count(app: &App) -> usize {
    app.session.model.elements.iter().count()
}

#[test]
fn copy_keeps_the_selection_on_the_window_s_clipboard_and_queues_it_for_the_system_s_and_says_how_many() {
    let (mut app, wall) = with_wall();
    assert!(app.copy_selection(false), "{:?}", app.message);
    assert_eq!(app.selection, [wall], "a copy leaves the selection");
    let text = app.clip_text.clone().expect("the window's own copy");
    assert_eq!(app.take_clip_out(), Some(text.clone()), "the same text waits for the system clipboard");
    assert_eq!(app.take_clip_out(), None, "once");
    assert!(bimcraft_core::clipboard::is_clipboard(&text));
    assert!(app.message.as_deref().is_some_and(|m| m.contains("Copied")), "{:?}", app.message);
}

#[test]
fn nothing_selected_copies_nothing_and_says_so() {
    let mut app = App::sample();
    assert!(!app.copy_selection(false) && !app.copy_selection(true));
    assert!(app.clip_text.is_none() && app.take_clip_out().is_none());
    assert!(app.message.is_some());
}

#[test]
fn cut_takes_the_elements_out_and_clears_the_selection() {
    let (mut app, wall) = with_wall();
    assert!(app.copy_selection(true), "{:?}", app.message);
    assert!(app.session.model.elements.get(wall).is_none(), "the wall is gone");
    assert!(app.selection.is_empty());
    assert!(app.clip_text.is_some(), "and it is on the clipboard");
    app.pasted_text(None, Mode::SamePlace);
    assert!(app.session.model.elements.iter().any(|(_, e)| matches!(e, Element::Wall(_))), "a paste brings a wall back");
}

#[test]
fn aligned_to_the_same_place_puts_the_copies_where_they_were_and_selects_them() {
    let (mut app, wall) = with_wall();
    app.copy_selection(false);
    let before = count(&app);
    app.pasted_text(None, Mode::SamePlace);
    assert!(count(&app) > before, "{:?}", app.message);
    assert!(!app.selection.is_empty() && !app.selection.contains(&wall), "the pasted ones are selected");
    assert!(app.message.as_deref().is_some_and(|m| m.contains("Pasted")), "{:?}", app.message);
}

#[test]
fn a_paste_with_nothing_copied_or_a_text_that_is_no_clipboard_says_so_and_changes_nothing() {
    let mut app = App::sample();
    let before = count(&app);
    app.pasted_text(None, Mode::SamePlace);
    assert!(app.message.as_deref().is_some_and(|m| m.contains("nothing to paste")), "{:?}", app.message);
    app.message = None;
    app.pasted_text(Some("{\"not\": \"a clipboard\"}".to_string()), Mode::Manual);
    assert!(app.message.is_some() && app.pasting.is_none());
    assert_eq!(count(&app), before);
}

#[test]
fn by_hand_waits_for_a_click_in_a_plan_and_a_click_puts_the_copy_down_and_escape_gives_it_up() {
    let (mut app, _) = with_wall();
    app.copy_selection(false);
    app.pasted_text(None, Mode::Manual);
    assert!(app.pasting.is_some(), "the copy waits for a click");
    assert!(app.escape() && app.pasting.is_none(), "Escape gives it up");
    app.pasted_text(None, Mode::Manual);
    let before = count(&app);
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(P2(-30000.0, -30000.0));
    app.release((x, y), (x, y), How::Replace);
    assert!(app.pasting.is_none(), "one click puts it down");
    assert!(count(&app) > before, "{:?}", app.message);
    assert!(!app.selection.is_empty(), "and the pasted ones are selected");
}

#[test]
fn a_paste_waiting_by_hand_is_given_up_when_the_view_is_not_a_plan() {
    let (mut app, _) = with_wall();
    app.copy_selection(false);
    app.pasted_text(None, Mode::Manual);
    assert!(app.pasting.is_some());
    assert!(app.open_named("3d"));
    assert!(app.pasting.is_none(), "a 3D view has no plan to follow the pointer in");
}

#[test]
fn ctrl_v_in_a_3d_view_pastes_where_they_were_without_waiting_for_a_click() {
    let (mut app, _) = with_wall();
    app.copy_selection(false);
    assert!(app.open_named("3d"));
    let before = count(&app);
    app.pasted_text(None, Mode::Manual);
    assert!(app.pasting.is_none(), "no click to wait for");
    assert!(count(&app) > before, "{:?}", app.message);
    assert!(matches!(app.view, ViewKind::View3D(_)));
}

#[test]
fn aligned_to_selected_levels_lists_the_levels_and_pastes_onto_the_ticked_ones() {
    let (mut app, _) = with_wall();
    app.copy_selection(false);
    app.pasted_text(None, Mode::SelectedLevels);
    let level = {
        let choose = app.dialogs.choose.as_ref().expect("the list of levels");
        assert!(!choose.views && !choose.offered.is_empty());
        choose.offered[0].0
    };
    let before = count(&app);
    assert!(!app.choose_paste(), "nothing ticked pastes nothing");
    assert!(app.dialogs.choose.is_some() && app.message.is_some(), "and the list stays open");
    assert_eq!(count(&app), before);
    app.choose_toggle(level);
    assert!(app.choose_paste(), "{:?}", app.message);
    assert!(app.dialogs.choose.is_none(), "the list closes");
    assert!(count(&app) > before);
    app.close_dialogs();
    assert!(app.dialogs.choose.is_none() && !app.dialogs.any());
}

#[test]
fn a_cancelled_list_leaves_the_model_alone_and_a_stale_tick_is_ignored() {
    let (mut app, _) = with_wall();
    app.copy_selection(false);
    app.pasted_text(None, Mode::SelectedViews);
    assert!(app.dialogs.choose.as_ref().is_some_and(|c| c.views));
    app.choose_toggle(u64::MAX);
    assert!(app.dialogs.choose.as_ref().is_some_and(|c| c.picked.is_empty()), "a row that is not offered is not ticked");
    let before = count(&app);
    app.close_dialogs();
    assert_eq!(count(&app), before);
    assert!(!app.choose_paste(), "no list, no paste");
}
