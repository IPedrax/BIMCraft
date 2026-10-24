//! The window's size (issue 37): brought within its screen, kept in the settings once it holds still, and read back at the start.

use egui::{ViewportCommand, ViewportId, vec2};

use super::*;
use crate::settings::{Window, window_in};
use crate::window::{MIN_SIZE, fitted};

const ROOT: ViewportId = ViewportId::ROOT;

fn near(a: egui::Vec2, b: egui::Vec2) -> bool {
    (a - b).abs().max_elem() < 0.01
}

#[test]
fn a_size_is_brought_within_its_screen_and_never_below_the_least_a_window_can_be() {
    let size = vec2(1440.0, 900.0);
    assert_eq!(fitted(size, vec2(1920.0, 1080.0)), None, "it fits");
    assert!(near(fitted(size, vec2(1366.0, 768.0)).unwrap(), vec2(1366.0 * 0.96, 768.0 * 0.9)), "a laptop's screen");
    assert_eq!(fitted(size, vec2(800.0, 600.0)), Some(vec2(MIN_SIZE[0], MIN_SIZE[1])), "a screen below the least a window is");
    assert!(near(fitted(vec2(1000.0, 700.0), vec2(1366.0, 768.0)).unwrap(), vec2(1000.0, 691.2)), "only the side that does not fit");
}

#[test]
fn a_window_bigger_than_its_screen_is_made_smaller_once_it_knows_the_screen() {
    let mut h = app(BOX);
    let sized = |h: &Harness<BimApp>| {
        h.output().viewport_output[&ROOT].commands.iter().find_map(|c| if let ViewportCommand::InnerSize(s) = c { Some(*s) } else { None })
    };
    h.step();
    assert_eq!(sized(&h), None, "the screen is not known: nothing is asked");
    // 1280 x 800 on a 1000 x 600 screen.
    h.input_mut().viewports.get_mut(&ROOT).unwrap().monitor_size = Some(vec2(1000.0, 600.0));
    h.step();
    assert!(near(sized(&h).expect("asked to be smaller"), vec2(960.0, 560.0)), "{:?}", sized(&h));
    h.step();
    assert_eq!(sized(&h), None, "and only once");
    // On a screen it fits, it is left as it is.
    let mut big = app(BOX);
    big.input_mut().viewports.get_mut(&ROOT).unwrap().monitor_size = Some(vec2(2560.0, 1440.0));
    big.step();
    assert_eq!(sized(&big), None);
}

#[test]
fn the_size_a_window_holds_is_kept_in_the_settings_and_a_window_left_as_it_started_writes_nothing() {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-window-{}", std::process::id())).join("settings.json");
    let _ = std::fs::remove_file(&file);
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    for _ in 0..6 {
        h.step();
    }
    assert!(!file.exists(), "as it started: nothing written");
    // Dragged to another size: kept once it has held still for half a second (frames are a quarter of a second apart).
    h.set_size(vec2(1000.0, 700.0));
    h.step();
    assert!(!file.exists(), "not yet");
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(window_in(&file), Some(Window { size: Some([1000.0, 700.0]), maximized: false }));
    // Maximized: the size it was is kept besides, to come back to.
    h.input_mut().viewports.get_mut(&ROOT).unwrap().maximized = Some(true);
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(window_in(&file), Some(Window { size: Some([1000.0, 700.0]), maximized: true }));
    // A window started on that file has it, and does not write it again.
    let mut again = app(BOX);
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().window.kept, Window { size: Some([1000.0, 700.0]), maximized: true });
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_hostile_window_in_the_settings_file_reads_as_sense() {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-window-hostile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("settings.json");
    let none = Window::default();
    for (text, window) in [
        (r#"{"window": {"width": 1000, "height": 700, "maximized": true}}"#, Window { size: Some([1000.0, 700.0]), maximized: true }),
        (r#"{"window": {"width": -5, "height": 700}}"#, none),
        (r#"{"window": {"width": 1e12, "height": 700}}"#, none),
        (r#"{"window": {"width": 10, "height": 700}}"#, none),
        (r#"{"window": {"width": "wide", "height": 700, "maximized": "yes"}}"#, none),
        (r#"{"window": {"width": 1000}}"#, none),
        (r#"{"window": 7}"#, none),
        (r#"{}"#, none),
    ] {
        std::fs::write(&file, text).unwrap();
        assert_eq!(window_in(&file), Some(window), "{text}");
    }
    assert_eq!(window_in(&dir.join("nothing.json")), None, "no file, no window");
    let _ = std::fs::remove_dir_all(&dir);
}
