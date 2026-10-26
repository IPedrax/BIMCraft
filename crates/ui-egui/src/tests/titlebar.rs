//! The window's own title bar, where it draws one: Minimize, Maximize and Close in its corner, the app bar's empty space moving
//! the window, its edges resizing it, on the start page too, and Settings asking for the system's bar instead.

use egui::{Event, PointerButton, Pos2, ResizeDirection, ViewportCommand, ViewportId, pos2, vec2};

use super::*;

const ROOT: ViewportId = ViewportId::ROOT;
/// The margin egui_kittest keeps round the app; the system's window has none, so there the bar's corner is the window's.
const INSET: f32 = 8.0;

/// The app as it opens on Windows and Linux: drawing its own title bar. Its frames are short, so two clicks are a double-click.
fn own(script: &str) -> Harness<'static, BimApp> {
    let mut h = app_at(script, 0.05);
    h.state_mut().own_title_bar = true;
    h.run();
    h
}

/// What the window asked of the system over a few frames.
fn asked(h: &mut Harness<BimApp>) -> Vec<ViewportCommand> {
    let mut all = Vec::new();
    for _ in 0..3 {
        h.step();
        all.extend(h.output().viewport_output.get(&ROOT).map(|v| v.commands.clone()).unwrap_or_default());
    }
    all
}

/// A press at `path[0]`, moved along `path`, let go: what the window asked meanwhile.
fn gesture(h: &mut Harness<BimApp>, path: &[Pos2]) -> Vec<ViewportCommand> {
    let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    let mut all = Vec::new();
    let mut frame = |h: &mut Harness<BimApp>, e: Event| {
        h.input_mut().events.push(e);
        h.step();
        all.extend(h.output().viewport_output.get(&ROOT).map(|v| v.commands.clone()).unwrap_or_default());
    };
    frame(h, Event::PointerMoved(path[0]));
    frame(h, button(path[0], true));
    for p in &path[1..] {
        frame(h, Event::PointerMoved(*p));
    }
    frame(h, button(*path.last().unwrap(), false));
    all
}

fn maximized(h: &mut Harness<BimApp>, on: bool) {
    h.input_mut().viewports.entry(ROOT).or_default().maximized = Some(on);
    h.run();
}

fn caption(h: &Harness<BimApp>, label: &str) -> egui::Rect {
    h.get_by_role_and_label(Role::Button, label).rect()
}

#[test]
fn without_its_own_title_bar_the_window_has_no_caption_buttons() {
    let h = app(BOX);
    assert!(h.query_by_label("Minimize").is_none() && h.query_by_label("Maximize").is_none());
}

#[test]
fn minimize_maximize_and_close_sit_in_the_windows_corner_and_ask_the_system() {
    let mut h = own(BOX);
    let (min, max, close) = (caption(&h, "Minimize"), caption(&h, "Maximize"), caption(&h, "Close"));
    assert_eq!((close.right(), close.top()), (1280.0 - INSET, INSET), "Close is in the corner, so a maximized window's corner hits it");
    assert_eq!((min.right(), max.right()), (max.left(), close.left()), "side by side");
    assert!([min, max, close].iter().all(|r| r.width() == crate::titlebar::BUTTON_WIDTH));
    h.get_by_label("Minimize").click();
    assert!(asked(&mut h).contains(&ViewportCommand::Minimized(true)));
    h.get_by_label("Maximize").click();
    assert!(asked(&mut h).contains(&ViewportCommand::Maximized(true)));
    // Maximized, the middle one restores.
    maximized(&mut h, true);
    assert!(h.query_by_label("Maximize").is_none());
    h.get_by_label("Restore Down").click();
    assert!(asked(&mut h).contains(&ViewportCommand::Maximized(false)));
    // Close asks as the system's own Close does, so unsaved work is still asked about.
    h.get_by_label("Close").click();
    assert!(asked(&mut h).contains(&ViewportCommand::Close));
}

#[test]
fn the_app_bars_empty_space_moves_the_window_and_a_double_click_maximizes_it() {
    let mut h = own(BOX);
    let close = caption(&h, "Close");
    // Between the project's name, in the middle, and the caption buttons.
    let empty = pos2(close.left() - 120.0, close.center().y);
    let moved = gesture(&mut h, &[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
    assert_eq!(moved.iter().filter(|c| matches!(c, ViewportCommand::StartDrag)).count(), 1, "{moved:?}");
    let mut twice = gesture(&mut h, &[empty]);
    twice.extend(gesture(&mut h, &[empty]));
    assert!(twice.contains(&ViewportCommand::Maximized(true)), "{twice:?}");
    // The bar's buttons keep their presses: dragging from Undo doesn't move the window.
    let undo = h.get_by_label("Undo").rect().center();
    let from_undo = gesture(&mut h, &[undo, undo + vec2(12.0, 4.0), undo + vec2(30.0, 8.0)]);
    assert!(!from_undo.iter().any(|c| matches!(c, ViewportCommand::StartDrag)), "{from_undo:?}");
}

#[test]
fn the_windows_edges_and_corners_resize_it_unless_it_is_maximized() {
    use ResizeDirection::*;
    let mut h = own(BOX);
    for (p, dir) in [
        (pos2(1.0, 400.0), West),
        (pos2(1278.0, 400.0), East),
        (pos2(640.0, 1.0), North),
        (pos2(640.0, 798.0), South),
        (pos2(2.0, 2.0), NorthWest),
        (pos2(1277.0, 3.0), NorthEast),
        (pos2(3.0, 797.0), SouthWest),
        (pos2(1278.0, 798.0), SouthEast),
    ] {
        let now = gesture(&mut h, &[p, p + vec2(6.0, 6.0)]);
        assert!(now.contains(&ViewportCommand::BeginResize(dir)), "{dir:?} at {p:?}: {now:?}");
    }
    // Away from the edges, nothing.
    let inside = gesture(&mut h, &[pos2(640.0, 400.0), pos2(650.0, 410.0)]);
    assert!(!inside.iter().any(|c| matches!(c, ViewportCommand::BeginResize(_))), "{inside:?}");
    maximized(&mut h, true);
    let now = gesture(&mut h, &[pos2(1.0, 400.0), pos2(8.0, 400.0)]);
    assert!(!now.iter().any(|c| matches!(c, ViewportCommand::BeginResize(_))), "a maximized window has no edges: {now:?}");
}

#[test]
fn the_start_page_has_the_title_bar_too() {
    let mut h = own("");
    h.state_mut().show_start_page();
    h.run();
    assert!(h.state().home.shown);
    let close = caption(&h, "Close");
    assert_eq!((close.right(), close.top()), (1280.0 - INSET, INSET));
    h.get_by_label("Close").click();
    assert!(asked(&mut h).contains(&ViewportCommand::Close));
    let p = pos2(1279.0, 400.0);
    assert!(gesture(&mut h, &[p, p + vec2(6.0, 0.0)]).contains(&ViewportCommand::BeginResize(ResizeDirection::East)));
    let empty = pos2(640.0, close.center().y);
    let moved = gesture(&mut h, &[empty, empty + vec2(12.0, 4.0), empty + vec2(30.0, 8.0)]);
    assert!(moved.contains(&ViewportCommand::StartDrag), "{moved:?}");
}

#[test]
fn settings_can_ask_for_the_systems_title_bar_and_keep_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = own(BOX);
    h.state_mut().use_settings(file.clone());
    assert!(!crate::settings::system_title_bar_in(&file), "nothing kept yet: the window's own bar");
    press(&mut h, "Settings");
    h.get_by_role_and_label(Role::CheckBox, "Use the system's title bar").click();
    let now = asked(&mut h);
    assert!(now.contains(&ViewportCommand::Decorations(true)), "the system's bar comes back at once: {now:?}");
    press(&mut h, "Close");
    assert!(h.query_by_label("Minimize").is_none(), "and the window's own caption buttons go");
    assert!(crate::settings::system_title_bar_in(&file), "kept for the next start");
    // Off again: the window's own.
    press(&mut h, "Settings");
    h.get_by_role_and_label(Role::CheckBox, "Use the system's title bar").click();
    assert!(asked(&mut h).contains(&ViewportCommand::Decorations(false)));
    assert!(!crate::settings::system_title_bar_in(&file));
    // A file that says something else keeps the window's own.
    std::fs::write(&file, r#"{"system_title_bar": "yes"}"#).unwrap();
    assert!(!crate::settings::system_title_bar_in(&file));
}
