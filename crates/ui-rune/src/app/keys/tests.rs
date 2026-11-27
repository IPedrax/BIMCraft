use bimcraft_core::shortcuts;

use super::*;

/// Types the letters one after another, 0.1 s apart from `from`, and says whether each belonged to a shortcut.
fn type_at(app: &mut App, text: &str, from: f64) -> Vec<bool> {
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            app.tick(from + 0.1 * i as f64);
            app.type_key(c)
        })
        .collect()
}

#[test]
fn two_typed_letters_pick_the_tool_their_button_picks() {
    let mut app = App::sample();
    assert_eq!(type_at(&mut app, "WA", 0.0), [true, true]);
    assert_eq!(app.tool, Some("wall"), "WA is the Wall tool");
    assert!(app.typed.is_empty());
    assert_eq!(type_at(&mut app, "WA", 1.0), [true, true]);
    assert_eq!(app.tool, Some("wall"), "typed again it stays in hand: a shortcut picks, a button's press may put it down");
    type_at(&mut app, "MD", 2.0);
    assert_eq!(app.tool, None, "MD is Modify: the tool is put down");
    type_at(&mut app, "DR", 3.0);
    assert_eq!(app.tool, Some("door"), "a second tool takes the first's place");
}

#[test]
fn a_shortcut_of_the_desktop_acts_through_the_buttons_methods() {
    let mut app = App::sample();
    let fitted = app.camera;
    app.zoom_by(2.0);
    assert_ne!(app.camera, fitted);
    type_at(&mut app, "ZF", 0.0);
    assert_eq!(app.camera, fitted, "ZF is Zoom to Fit");
    type_at(&mut app, "3D", 1.0);
    assert_eq!(app.view, ViewKind::View3D(None), "3D is the 3D view");
    let level = app.level.expect("a level");
    app.set_level(level);
    assert_eq!(app.view, ViewKind::Plan(level));
}

#[test]
fn a_letter_that_waited_too_long_or_begins_nothing_is_the_windows_and_escape_drops_what_waits() {
    let mut app = App::sample();
    assert_eq!(type_at(&mut app, "Q", 0.0), [false], "Q begins no shortcut: the window's");
    assert_eq!(type_at(&mut app, "W", 1.0), [true]);
    assert_eq!(app.typed.text(), "W");
    assert_eq!(app.typed_in(), Some(2.0), "it waits two seconds from the letter");
    assert!(app.drop_typed() && app.typed.is_empty() && !app.drop_typed(), "Escape drops what waits");
    type_at(&mut app, "W", 5.0);
    app.tick(8.0);
    assert!(app.expire_typed() && app.typed.is_empty(), "time drops it");
    assert_eq!(app.tool, None);
    type_at(&mut app, "W", 10.0);
    assert_eq!(type_at(&mut app, "A", 13.0), [true], "A after too long a wait begins a new one");
    assert_eq!(app.tool, None, "WA did not run");
}

#[test]
fn an_open_dialog_has_the_letters() {
    let mut app = App::sample();
    app.toggle_find();
    assert!(app.dialogs.any());
    assert_eq!(type_at(&mut app, "WA", 0.0), [false, false]);
    assert_eq!(app.tool, None);
    assert!(app.typed.is_empty());
}

#[test]
fn a_shortcut_works_whatever_the_view_shown() {
    for name in ["section", "elevation", "sheet", "schedule", "ceiling", "3d"] {
        let mut app = App::sample();
        assert!(app.open_named(name), "{name}");
        let shown = app.view;
        type_at(&mut app, "BM", 0.0);
        assert_eq!(app.tool, Some("beam"), "BM in the {name}");
        type_at(&mut app, "ZF", 1.0);
        assert_eq!(app.view, shown, "ZF fits the {name}, it does not leave it");
        type_at(&mut app, "MD", 2.0);
        assert_eq!(app.tool, None, "MD in the {name}");
    }
}

#[test]
fn create_similar_and_select_all_instances_work_on_the_selection() {
    let mut app = App::sample();
    let wall = app.session.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).expect("a wall");
    let door = app.session.model.elements.iter().find(|(_, e)| e.category() == "door").map(|(id, _)| *id).expect("a door");
    app.select(vec![door]);
    type_at(&mut app, "CS", 0.0);
    assert_eq!(app.tool, Some("door"), "Create Similar picks the tool of the selected element's category");
    app.select(vec![wall]);
    type_at(&mut app, "SA", 1.0);
    let walls = app.session.model.elements.iter().filter(|(_, e)| e.category() == "wall").count();
    assert!(app.selection.contains(&wall) && app.selection.len() > 1 && app.selection.len() <= walls, "{:?} of {walls}", app.selection);
    app.select(vec![]);
    type_at(&mut app, "CS", 2.0);
    assert!(app.message.as_deref().is_some_and(|m| m.starts_with("Create Similar")), "{:?}", app.message);
}

#[test]
fn undo_redo_and_delete_are_commands_shortcuts_and_chords() {
    let mut app = App::sample();
    let wall = app.session.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).expect("a wall");
    app.select(vec![wall]);
    assert_eq!(shortcuts::chord("Delete"), Some("cmd.delete"));
    assert!(app.run_action("cmd.delete"));
    assert!(app.session.model.elements.get(wall).is_none(), "Delete deletes");
    assert!(app.run_action("cmd.undo"));
    assert!(app.session.model.elements.get(wall).is_some(), "Ctrl+Z gives it back");
    assert!(app.run_action("cmd.redo"));
    assert!(app.session.model.elements.get(wall).is_none());
    assert!(app.run_action("ui.view.3d") && app.view == ViewKind::View3D(None));
    assert!(app.run_action("ui.view.plan") && matches!(app.view, ViewKind::Plan(_)));
    assert!(app.run_action("ui.find") && app.dialogs.any());
    app.close_dialogs();
    assert!(!app.run_action("ui.palette.properties") && app.message.as_deref().is_some_and(|m| m.ends_with("not in this window yet")));
}

#[test]
fn every_shortcut_of_the_table_does_what_it_names_or_says_it_cannot() {
    let (mut commands, mut commands_done, mut desktop_done) = (0, 0, 0);
    for &(code, target) in shortcuts::table() {
        let mut app = App::sample();
        let wall = app.session.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).expect("a wall");
        app.select(vec![wall]);
        let did = app.run_target(target);
        let said = app.message.as_deref().is_some_and(|m| m.ends_with("not in this window yet"));
        assert!(did || said, "{code} {target:?} did nothing and said nothing: {:?}", app.message);
        match target {
            Target::Command(_) => {
                commands += 1;
                commands_done += usize::from(did);
            }
            Target::Desktop(_) => desktop_done += usize::from(did),
        }
    }
    assert_eq!(commands_done, commands, "every command's shortcut puts its tool in hand or opens its form");
    assert!(desktop_done >= 7, "the desktop's own that this window has: {desktop_done}");
}
