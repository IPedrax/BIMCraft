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
    }
}
