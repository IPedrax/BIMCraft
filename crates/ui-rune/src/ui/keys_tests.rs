//! Shortcuts typed in the window, driven as a person drives it, headless: Revit's two letters over every kind of view, the chords of
//! the core's table, the keys a dialog has, and the wait a letter makes for its second.

use rune::{Env, Key, KeyEvent, Mods, Runtime};

use super::{on_key, poll, view};
use crate::app::App;
use crate::views::ViewKind;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(poll);
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

fn press(rt: &mut Runtime<App>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

fn type_letters(rt: &mut Runtime<App>, letters: &str) {
    for c in letters.chars() {
        press(rt, Key::Text(c.to_string()), Mods::default());
    }
}

fn ctrl() -> Mods {
    Mods { cmd: true, ..Mods::default() }
}

fn status(rt: &Runtime<App>) -> String {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some("status-line")).and_then(|e| e.text).unwrap_or_default()
}

#[test]
fn two_typed_letters_pick_a_tool_and_the_first_shows_in_the_status_bar_while_it_waits() {
    let mut rt = window();
    type_letters(&mut rt, "W");
    assert!(status(&rt).starts_with("W   |"), "{}", status(&rt));
    assert_eq!(rt.state.tool, None);
    type_letters(&mut rt, "A");
    assert_eq!(rt.state.tool, Some("wall"), "WA is Wall");
    assert!(status(&rt).contains("Tool: Wall") && !status(&rt).starts_with("W   |"), "{}", status(&rt));
    type_letters(&mut rt, "MD");
    assert_eq!(rt.state.tool, None, "MD is Modify");
}

#[test]
fn a_letter_waits_two_seconds_for_the_next_and_then_the_status_bar_forgets_it() {
    let mut rt = window();
    type_letters(&mut rt, "W");
    assert!(status(&rt).starts_with("W   |"));
    for _ in 0..4 {
        rt.frame(1.0);
    }
    assert!(rt.state.typed.is_empty(), "dropped once it had waited");
    assert!(!status(&rt).starts_with("W   |"), "{}", status(&rt));
    type_letters(&mut rt, "A");
    assert_eq!(rt.state.tool, None, "WA was not typed in time");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.typed.is_empty(), "Escape drops it");
}

#[test]
fn the_shortcuts_work_in_every_kind_of_view() {
    for name in ["section", "sheet", "schedule", "3d", "ceiling"] {
        let mut rt = window();
        rt.update(|s, _| {
            assert!(s.open_named(name));
        });
        rt.frame(0.0);
        type_letters(&mut rt, "BM");
        assert_eq!(rt.state.tool, Some("beam"), "BM in the {name}");
        type_letters(&mut rt, "ZF");
        type_letters(&mut rt, "MD");
        assert_eq!(rt.state.tool, None, "MD in the {name}");
    }
}

#[test]
fn a_dialog_has_the_letters_and_a_chord_still_toggles_it() {
    let mut rt = window();
    press(&mut rt, Key::Command('k'), ctrl());
    assert!(rt.state.dialogs.any(), "Ctrl+K opens Find");
    type_letters(&mut rt, "WA");
    assert_eq!((rt.state.tool, rt.state.typed.is_empty()), (None, true), "the letters went to the dialog");
    press(&mut rt, Key::Command('k'), ctrl());
    assert!(!rt.state.dialogs.any());
}

#[test]
fn the_chords_of_the_core_table_run_the_actions_they_name() {
    let mut rt = window();
    press(&mut rt, Key::Function(3), Mods::default());
    assert_eq!(rt.state.view, ViewKind::View3D(None), "F3");
    press(&mut rt, Key::Function(2), Mods::default());
    assert!(matches!(rt.state.view, ViewKind::Plan(_)), "F2");
    let fitted = rt.state.camera;
    rt.state.zoom_by(2.0);
    press(&mut rt, Key::Command('0'), ctrl());
    assert_eq!(rt.state.camera, fitted, "Ctrl+0 is Zoom to Fit");
    press(&mut rt, Key::Command('r'), ctrl());
    assert_eq!(rt.state.tool, Some("rotate"), "Ctrl+R is Rotate");
    press(&mut rt, Key::Command('q'), ctrl());
    assert_eq!(rt.state.tool, Some("rotate"), "a chord the table does not have does nothing");
}

#[test]
fn ctrl_s_saves_through_the_dialog_a_project_with_no_file_asks_for_and_ctrl_o_asks_which_to_open() {
    let dir = std::env::temp_dir().join(format!("bimcraft-rune-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("house.bimc");
    let mut rt = window();
    press(&mut rt, Key::Command('s'), ctrl());
    assert_eq!(rt.file_dialog().map(|(kind, _)| kind), Some(rune::FileDialogKind::Save), "a project with no file asks where");
    rt.resolve_file_dialog(Some(file.clone())).expect("the dialog answers");
    rt.frame(0.0);
    assert!(file.exists(), "saved through the engine: {:?}", rt.state.message);
    press(&mut rt, Key::Command('s'), ctrl());
    assert!(rt.file_dialog().is_none(), "a project with a file saves without asking");
    press(&mut rt, Key::Command('o'), ctrl());
    assert_eq!(rt.file_dialog().map(|(kind, _)| kind), Some(rune::FileDialogKind::Open));
    rt.resolve_file_dialog(None).expect("a cancelled dialog");
    press(&mut rt, Key::Command('p'), ctrl());
    assert_eq!(rt.file_dialog().map(|(kind, _)| kind), Some(rune::FileDialogKind::Save), "Ctrl+P asks where to write the PDF");
    rt.resolve_file_dialog(None).expect("a cancelled dialog");
    std::fs::remove_dir_all(&dir).ok();
}
