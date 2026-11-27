//! Ctrl+C, Ctrl+X, Ctrl+V and the menu's clipboard items, driven as a person drives them, headless, over Rune's memory clipboard standing
//! for the system's: the text a copy leaves on it, the text a paste reads from it, and this window's own copy where it holds none.

use bimcraft_core::model::Element;
use rune::{Env, Key, KeyEvent, Mods, Runtime};

use super::{on_key, poll, view};
use crate::app::App;
use crate::context::Action;

fn window(mem: &rune::clipboard::MemoryClipboard) -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.use_clipboard(mem);
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

fn press(rt: &mut Runtime<App>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    for _ in 0..3 {
        rt.frame(0.0);
    }
}

fn ctrl() -> Mods {
    Mods { cmd: true, ..Mods::default() }
}

fn select_a_wall(rt: &mut Runtime<App>) -> u64 {
    let wall = rt.state.session.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    rt.update(|s, _| s.select(vec![wall]));
    rt.frame(0.0);
    wall
}

fn count(rt: &Runtime<App>) -> usize {
    rt.state.session.model.elements.iter().count()
}

#[test]
fn ctrl_c_leaves_the_selection_as_text_on_the_system_clipboard_and_ctrl_x_takes_it_away() {
    let mem = rune::clipboard::memory();
    let mut rt = window(&mem);
    let wall = select_a_wall(&mut rt);
    press(&mut rt, Key::Command('c'), ctrl());
    let text = mem.text().map(|t| t.to_string()).expect("the system clipboard holds the copy");
    assert!(bimcraft_core::clipboard::is_clipboard(&text), "BIMCraft's own text: {text:.40}");
    assert_eq!(rt.state.clip_text.as_deref(), Some(text.as_str()), "and the window keeps it too");
    assert!(rt.state.session.model.elements.get(wall).is_some(), "a copy leaves the wall");
    press(&mut rt, Key::Command('x'), ctrl());
    assert!(rt.state.session.model.elements.get(wall).is_none(), "Ctrl+X takes it away");
    assert!(rt.state.selection.is_empty());
}

#[test]
fn ctrl_v_reads_the_system_clipboard_another_window_filled_and_waits_for_a_click_in_a_plan() {
    let mem = rune::clipboard::memory();
    let mut first = window(&mem);
    select_a_wall(&mut first);
    press(&mut first, Key::Command('c'), ctrl());
    assert!(mem.text().is_some());
    let mut other = window(&mem);
    assert!(other.state.clip_text.is_none(), "this window never copied");
    press(&mut other, Key::Command('v'), ctrl());
    assert!(other.state.pasting.is_some(), "Ctrl+V in a plan puts the copy down by hand: it waits for a click");
    press(&mut other, Key::Escape, Mods::default());
    assert!(other.state.pasting.is_none(), "Escape gives it up");
}

#[test]
fn a_menu_item_pastes_aligned_from_the_system_clipboard_and_from_the_windows_own_copy_when_it_holds_none() {
    let mem = rune::clipboard::memory();
    let mut rt = window(&mem);
    select_a_wall(&mut rt);
    rt.update(|s, _| s.act(Action::CopyToClipboard));
    rt.frame(0.0);
    rt.frame(0.0);
    assert!(mem.text().is_some(), "the menu's Copy reaches the system clipboard too");
    let before = count(&rt);
    rt.update(|s, _| s.act(Action::PasteSamePlace));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    assert!(count(&rt) > before, "pasted from the system clipboard: {:?}", rt.state.message);
    mem.set_text("");
    let before = count(&rt);
    rt.update(|s, _| s.act(Action::PasteSamePlace));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    assert!(count(&rt) > before, "an empty system clipboard: the window's own copy: {:?}", rt.state.message);
}

#[test]
fn aligned_to_selected_levels_from_the_menu_opens_the_list_and_its_rows_and_paste_work_by_press() {
    let mem = rune::clipboard::memory();
    let mut rt = window(&mem);
    select_a_wall(&mut rt);
    rt.update(|s, _| s.act(Action::CopyToClipboard));
    rt.update(|s, _| s.act(Action::PasteSelectedLevels));
    for _ in 0..3 {
        rt.frame(0.0);
    }
    assert!(rt.state.dialogs.choose.is_some(), "the list of levels opens: {:?}", rt.state.message);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let at = rt.find("choose-row-0").expect("a row for the first level");
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    rt.frame(0.0);
    assert_eq!(rt.state.dialogs.choose.as_ref().map(|c| c.picked.len()), Some(1), "a press ticks the row");
    let before = count(&rt);
    let at = rt.find("choose-ok").expect("Paste");
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    rt.frame(0.0);
    rt.frame(0.0);
    assert!(rt.state.dialogs.choose.is_none(), "the list closes: {:?}", rt.state.message);
    assert!(count(&rt) > before, "and the copy is on the ticked level");
}
