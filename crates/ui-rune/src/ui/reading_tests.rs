//! The typed length and angle through the window, headless: the keys Rune hands the window (`Key::Text`, Tab, Enter, Backspace, Escape).

use rune::{Env, Key, KeyEvent, Mods, Runtime};

use super::view;
use crate::app::App;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = Some(super::on_key);
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

fn key(rt: &mut Runtime<App>, key: Key) {
    rt.key(KeyEvent { key, mods: Mods::default() });
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

#[test]
fn digits_typed_after_a_point_are_a_length_in_the_status_bar_and_escape_gives_them_up() {
    let mut rt = window();
    key(&mut rt, Key::Text("5".into()));
    assert!(rt.state.typed.is_empty(), "with no tool the digit is nobody's");
    rt.update(|s, _| {
        s.set_tool("wall");
        s.picked.push(bimcraft_core::geom::P2(-30000.0, -30000.0));
    });
    key(&mut rt, Key::Text("35".into()));
    key(&mut rt, Key::Text("0".into()));
    assert_eq!(rt.state.typed.length, "350");
    key(&mut rt, Key::Tab);
    key(&mut rt, Key::Text("30".into()));
    assert_eq!((rt.state.typed.length.as_str(), rt.state.typed.angle.as_str()), ("350", "30"));
    let line = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("status-line")).and_then(|e| e.text).unwrap_or_default();
    assert!(line.contains("350") && line.contains("30"), "the status bar reads it: {line}");
    key(&mut rt, Key::Backspace);
    assert_eq!(rt.state.typed.angle, "3");
    key(&mut rt, Key::Escape);
    assert!(rt.state.typed.is_empty() && rt.state.tool == Some("wall"), "Escape gives up what was typed, not the tool");
}
