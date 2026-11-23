//! Copy to Clipboard, Cut and Paste in the window: Ctrl+C, Ctrl+X and Ctrl+V as the window's system hands them over, the
//! paste following the cursor, the Aligned options from the ribbon, and a paste of what another window copied.

use super::*;

fn walls(h: &Harness<BimApp>) -> usize {
    ids(h, "wall").len()
}

/// Ctrl+C: what the window's system sends, and the text it put on the system clipboard.
fn ctrl_c(h: &mut Harness<BimApp>) -> String {
    h.event(Event::Copy);
    h.run();
    h.state().copied.clone().unwrap_or_default()
}

fn ctrl_v(h: &mut Harness<BimApp>, text: &str) {
    h.event(Event::Paste(text.into()));
    h.run();
}

/// What the window put on the system clipboard this frame, if anything.
fn copied_text(h: &Harness<BimApp>) -> Option<String> {
    h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    })
}

/// An error in the status box, from Ctrl+C with nothing selected: the box's message.
fn an_error(h: &mut Harness<BimApp>) -> String {
    h.event(Event::Copy);
    h.run();
    let message = h.state().status.text.clone();
    assert_eq!(message, "select what to copy first");
    assert!(h.state().status.error);
    assert!(h.state().selection.is_empty());
    message
}

/// Select all of a label's text with the mouse, as a person does before Ctrl+C.
fn select_label_text(h: &mut Harness<BimApp>, text: &str) {
    let rect = h.get_by_label(text).rect();
    let a = rect.left_center() + vec2(1.0, 0.0);
    let b = rect.right_center() - vec2(1.0, 0.0);
    drag_screen(h, a, b, PointerButton::Primary);
}

/// The status box's own Copy button: the Copy inside the view's bottom-right corner.
fn status_copy_button(h: &Harness<BimApp>) -> egui::Rect {
    let view = h.state().view_rect;
    h.get_all_by_label("Copy")
        .map(|n| n.rect())
        .filter(|r| view.contains(r.center()) && r.center().y > view.center().y && r.center().x > view.center().x)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the status box has its own Copy button")
}

#[test]
fn the_error_boxs_text_selects_and_ctrl_c_copies_it_without_losing_the_error() {
    let mut h = app(BOX);
    let message = an_error(&mut h);
    select_label_text(&mut h, &message);
    h.event(Event::Copy);
    h.step();
    assert_eq!(copied_text(&h).as_deref(), Some(message.as_str()), "the selected text, not the model");
    assert_eq!(h.state().status.text, message, "the error stays as it was");
    assert!(h.state().status.error);
    assert!(h.state().selection.is_empty(), "no elements were selected");
    assert!(h.state().clip_text.is_none(), "the window's own clipboard is untouched");
}

#[test]
fn the_error_boxs_copy_button_puts_the_whole_message_on_the_clipboard() {
    let mut h = app(BOX);
    let message = an_error(&mut h);
    let at = status_copy_button(&h);
    h.get_all_by_label("Copy").find(|n| n.rect() == at).expect("the status box's Copy button").click();
    h.step();
    assert_eq!(copied_text(&h).as_deref(), Some(message.as_str()), "the whole message, without the title");
    assert_eq!(h.state().status.text, message, "the box stays up, its message unchanged");
    assert!(!h.state().status.text.is_empty());
    assert!(h.state().copied.is_none() && h.state().clip_text.is_none(), "the model's clipboard is untouched");
}

#[test]
fn after_the_error_text_is_deselected_ctrl_c_copies_the_model_again() {
    let mut h = app(BOX);
    let message = an_error(&mut h);
    select_label_text(&mut h, &message);
    h.event(Event::Copy);
    h.step();
    assert_eq!(copied_text(&h).as_deref(), Some(message.as_str()), "the text first");

    // A click in the view clears the label's selection; the next Ctrl+C is the model's again.
    click(&mut h, P2(4000.0, 3000.0));
    click(&mut h, P2(4000.0, 0.0));
    let copied = ctrl_c(&mut h);
    assert!(bimcraft_core::clipboard::is_clipboard(&copied), "the model's clipboard text");
    assert!(!h.state().status.error, "{}", h.state().status.text);
    assert_eq!(walls(&h), 4);
}

#[test]
fn ctrl_c_with_nothing_selected_and_no_text_still_says_select_what_to_copy_first() {
    let mut h = app(BOX);
    h.event(Event::Copy);
    h.run();
    assert_eq!(h.state().status.text, "select what to copy first");
    assert!(h.state().status.error);
    assert!(h.state().copied.is_none() && h.state().clip_text.is_none());
}

#[test]
fn ctrl_x_with_the_error_text_selected_copies_it_and_does_not_cut_the_model() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    let message = "Something went wrong".to_string();
    h.state_mut().status = crate::Status { text: message.clone(), error: true };
    h.run();
    select_label_text(&mut h, &message);
    h.event(Event::Cut);
    h.step();
    assert_eq!(copied_text(&h).as_deref(), Some(message.as_str()));
    assert_eq!(walls(&h), 4, "no wall was cut");
    assert_eq!(h.state().selection.len(), 1, "the wall stays selected");
}

#[test]
fn the_note_boxs_text_selects_and_copies_as_the_error_boxs_does() {
    let mut h = app(BOX);
    let message = "Saved the project.".to_string();
    h.state_mut().status = crate::Status { text: message.clone(), error: false };
    h.run();
    select_label_text(&mut h, &message);
    h.event(Event::Copy);
    h.step();
    assert_eq!(copied_text(&h).as_deref(), Some(message.as_str()));
    assert_eq!(h.state().status.text, message);
    assert!(!h.state().status.error);
}

#[test]
fn ctrl_v_still_pastes_the_model_while_the_error_text_is_selected() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    let text = ctrl_c(&mut h);
    assert!(bimcraft_core::clipboard::is_clipboard(&text));
    let message = "Something went wrong".to_string();
    h.state_mut().status = crate::Status { text: message.clone(), error: true };
    h.run();
    select_label_text(&mut h, &message);
    ctrl_v(&mut h, &text);
    assert_eq!(h.state().tool, Tool::Paste, "paste goes through the window's handler as before");
}

#[test]
fn the_ribbons_copy_still_copies_the_model_while_the_error_text_is_selected() {
    use crate::context::Action;
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    let message = "Something went wrong".to_string();
    h.state_mut().status = crate::Status { text: message.clone(), error: true };
    h.run();
    select_label_text(&mut h, &message);
    h.state_mut().act(Action::CopyToClipboard);
    h.step();
    let copied = h.state().copied.clone().unwrap_or_default();
    assert!(bimcraft_core::clipboard::is_clipboard(&copied), "the explicit command copies the model, not the text");
    assert_ne!(copied, message);
    assert_eq!(h.state().selection.len(), 1);
}

#[test]
fn ctrl_c_then_ctrl_v_pastes_what_is_selected_where_it_is_clicked() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection.len(), 1);
    let text = ctrl_c(&mut h);
    assert!(bimcraft_core::clipboard::is_clipboard(&text), "the system clipboard has the core's text");
    ctrl_v(&mut h, &text);
    assert_eq!(h.state().tool, Tool::Paste, "the copy follows the cursor");
    assert_eq!(walls(&h), 4);
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(walls(&h), 5, "{}", h.state().status.text);
    assert_eq!(h.state().tool, Tool::Modify);
    let pasted = h.state().selection.clone();
    assert_eq!(pasted.len(), 1, "what was pasted is selected");
    let Some(Element::Wall(w)) = h.state().session.model.elements.get(pasted[0]) else { panic!("not a wall") };
    assert_eq!((w.start.1, w.end.1), (3000.0, 3000.0), "its middle where it was clicked");
}

#[test]
fn ctrl_x_takes_the_selection_away_and_ctrl_v_brings_it_back_by_hand() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    h.event(Event::Cut);
    h.run();
    assert_eq!(walls(&h), 3);
    let text = h.state().copied.clone().unwrap_or_default();
    ctrl_v(&mut h, &text);
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(walls(&h), 4);
}

#[test]
fn what_another_window_copied_is_pasted_here() {
    let mut other = app(BOX);
    click(&mut other, P2(0.0, 3000.0));
    let text = ctrl_c(&mut other);
    let mut h = app("level {\"elevation\": 6000}");
    assert_eq!(walls(&h), 0);
    ctrl_v(&mut h, &text);
    click(&mut h, P2(1000.0, 1000.0));
    assert_eq!(walls(&h), 1, "{}", h.state().status.text);
}

#[test]
fn ctrl_v_with_nothing_of_bimcrafts_on_the_system_clipboard_pastes_this_windows_copy() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    ctrl_c(&mut h);
    ctrl_v(&mut h, "some text from elsewhere");
    assert_eq!(h.state().tool, Tool::Paste);
}

#[test]
fn aligned_to_selected_levels_pastes_onto_each_level_ticked() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    let text = ctrl_c(&mut h);
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Paste");
    press(&mut h, "Aligned to Selected Levels");
    // The system clipboard's answer to the ribbon's request.
    ctrl_v(&mut h, &text);
    pick(&mut h, Role::CheckBox, "Level 2");
    press(&mut h, "OK");
    assert_eq!(walls(&h), 5, "{}", h.state().status.text);
    let up = h.state().session.model.guid(100_001u64);
    assert!(h.state().session.model.elements.values().any(|e| matches!(e, Element::Wall(w) if w.level == up)));
}

#[test]
fn the_right_click_menu_copies_and_pastes_as_the_keys_do() {
    use crate::context::Action;
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    let offered: Vec<Action> = h.state().context_items().concat().into_iter().filter(|i| i.enabled).map(|i| i.action).collect();
    assert!(offered.contains(&Action::CopyToClipboard) && offered.contains(&Action::Cut), "{offered:?}");
    h.state_mut().act(Action::CopyToClipboard);
    h.run();
    let text = h.state().copied.clone().unwrap_or_default();
    assert!(bimcraft_core::clipboard::is_clipboard(&text));
    h.state_mut().act(Action::Paste);
    h.run();
    // The system clipboard's answer to the menu's request.
    ctrl_v(&mut h, &text);
    assert_eq!(h.state().tool, Tool::Paste);
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(walls(&h), 5);
}

/// What the Clipboard panel and a paste following the cursor look like, for a person to look at:
/// `BIMCRAFT_CLIPBOARD_DUMP=<folder>` writes the Paste menu open and the copy of a room's walls under the cursor, light and dark.
#[test]
fn clipboard_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_CLIPBOARD_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
        if dark {
            h.ctx.set_theme(egui::Theme::Dark);
        }
        click(&mut h, P2(4000.0, 0.0));
        click_with(&mut h, P2(8000.0, 3000.0), Modifiers::COMMAND);
        let text = ctrl_c(&mut h);
        h.state_mut().tab = crate::Tab::Modify;
        h.run();
        press(&mut h, "Paste");
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("clipboard-{look}-menu.png"))).unwrap();
        key(&mut h, Key::Escape);
        ctrl_v(&mut h, &text);
        let at = h.state().screen(P2(3000.0, 2500.0));
        h.hover_at(at);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("clipboard-{look}-paste.png"))).unwrap();
        key(&mut h, Key::Escape);
        press(&mut h, "Clipboard ▾");
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("clipboard-{look}-list.png"))).unwrap();
        key(&mut h, Key::Escape);
        press(&mut h, "Default 3D View");
        ctrl_v(&mut h, &text);
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("clipboard-{look}-3d-ask.png"))).unwrap();
        press(&mut h, "Position on Work Plane");
        let at = h.state().screen3([4000.0, -4000.0, 0.0]);
        h.hover_at(at);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("clipboard-{look}-3d-paste.png"))).unwrap();
    }
}

#[test]
fn the_clipboard_panels_list_is_as_narrow_as_a_list_and_stays_in_the_window() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Clipboard ▾");
    for _ in 0..5 {
        h.run();
    }
    let window = h.ctx.content_rect().right();
    let under = h.get_by_label("Clipboard ▾").rect().bottom();
    let rows: Vec<egui::Rect> = ["Paste", "Cut to Clipboard", "Copy to Clipboard"]
        .iter()
        .filter_map(|l| h.get_all_by_label(l).map(|n| n.rect()).find(|r| r.top() >= under))
        .collect();
    assert_eq!(rows.len(), 3, "each tool a row of the list");
    assert!(rows.windows(2).all(|w| w[1].top() >= w[0].bottom() - 0.5), "one under another: {rows:?}");
    for r in &rows {
        assert!(r.right() <= window && r.width() < 400.0, "{r:?} in a window {window} wide");
    }
}

/// The box's south wall copied, and the default 3D view open.
fn copied_into_3d(h: &mut Harness<BimApp>) -> String {
    click(h, P2(4000.0, 0.0));
    let text = ctrl_c(h);
    press(h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    text
}

#[test]
fn ctrl_v_in_3d_asks_whether_to_paste_in_the_same_place() {
    let mut h = app(BOX);
    let text = copied_into_3d(&mut h);
    ctrl_v(&mut h, &text);
    assert_eq!(walls(&h), 4, "nothing pasted before the answer");
    press(&mut h, "Same Place");
    assert_eq!(walls(&h), 5, "{}", h.state().status.text);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn ctrl_v_in_3d_positioned_by_hand_goes_where_the_work_plane_is_clicked() {
    let mut h = app(BOX);
    let text = copied_into_3d(&mut h);
    ctrl_v(&mut h, &text);
    press(&mut h, "Position on Work Plane");
    assert_eq!(h.state().tool, Tool::Paste, "the copy follows the cursor over the work plane");
    assert!(h.state().prompt().contains("work plane"), "{}", h.state().prompt());
    assert!(!h.state().options_rect.is_positive(), "no Options Bar: nothing is on it");
    let at = h.state().screen3([4000.0, -3000.0, 0.0]);
    h.hover_at(at);
    h.run();
    click_at(&mut h, at);
    assert_eq!(walls(&h), 5, "{}", h.state().status.text);
    let pasted = h.state().selection.clone();
    let Some(Element::Wall(w)) = h.state().session.model.elements.get(pasted[0]) else { panic!("not a wall") };
    assert!((w.start.1 + 3000.0).abs() < 20.0 && (w.end.1 + 3000.0).abs() < 20.0, "its middle where clicked: {w:?}");
    assert!(((w.start.0 + w.end.0) / 2.0 - 4000.0).abs() < 20.0, "{w:?}");
    assert_eq!(w.level, h.state().session.model.guid(100_000u64), "on the work plane's level");
}

#[test]
fn ctrl_v_in_3d_cancelled_pastes_nothing() {
    let mut h = app(BOX);
    let text = copied_into_3d(&mut h);
    ctrl_v(&mut h, &text);
    press(&mut h, "Cancel");
    assert_eq!(walls(&h), 4);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn aligned_to_picked_level_lights_the_level_under_the_cursor() {
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, -1000], \"end\": [9000, -1000]}}"));
    click(&mut h, P2(4000.0, 0.0));
    let text = ctrl_c(&mut h);
    browse(&mut h, "Section 1");
    h.state_mut().tab = crate::Tab::Modify;
    h.run();
    press(&mut h, "Paste");
    press(&mut h, "Aligned to Picked Level");
    // The system clipboard's answer to the ribbon's request, unless the section's frames outlasted the wait for it and the
    // window's own copy was taken.
    if h.state().paste_asked.is_some() {
        ctrl_v(&mut h, &text);
    }
    assert_eq!(h.state().tool, Tool::PickLevel, "{:?} {}", h.state().sheet, h.state().status.text);
    assert!(h.state().prompt().contains("level line"), "{}", h.state().prompt());
    let levels: Vec<Id> = ids(&h, "level");
    let rect = h.state().view_rect;
    let lit = |h: &Harness<BimApp>| h.state().cut.tints.iter().map(|(id, _)| *id).find(|id| levels.contains(id));
    let mut found = None;
    let x = rect.center().x;
    let mut y = rect.top() + 4.0;
    while y < rect.bottom() && found.is_none() {
        h.hover_at(pos2(x, y));
        h.run();
        found = lit(&h);
        y += 2.0;
    }
    let Some(level) = found else { panic!("no level lit under the cursor down the view") };
    assert!(h.state().cut.tints.iter().all(|(id, _)| levels.contains(id)), "only levels light while one is picked");
    click_at(&mut h, pos2(x, y - 2.0));
    assert_eq!(walls(&h), 5, "{}", h.state().status.text);
    let pasted = h.state().selection.clone();
    let Some(Element::Wall(w)) = h.state().session.model.elements.get(pasted[0]) else { panic!("not a wall") };
    assert_eq!(w.level, h.state().session.model.guid(level), "onto the level lit");
}

#[test]
fn a_familys_copy_on_the_system_clipboard_isnt_pasted_as_the_projects_older_one() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    ctrl_c(&mut h);
    // A family's copy made since, in this window or another: the project's paste refuses it, saying where it goes.
    let source = bimcraft_core::family::clipboard::Source { family: "Box".into(), copy: Guid::new() };
    let clip = bimcraft_core::family::clipboard::Clip::default();
    let text = bimcraft_core::family::clipboard::text(&source, bimcraft_core::family::clipboard::Copied::Elements(&clip)).unwrap();
    ctrl_v(&mut h, &text);
    assert_ne!(h.state().tool, Tool::Paste, "nothing follows the cursor");
    assert!(h.state().status.error && h.state().status.text.contains("family's copy"), "{}", h.state().status.text);
    assert_eq!(walls(&h), 4);
}
