//! The dialogs' state and handlers with no window: Find and the generic command form, driven by their methods as the elements drive
//! them, and the model read back through the session.

use super::*;

/// Types `text` in the field of the open form called `field`.
fn typed(app: &mut App, field: &str, text: &str) {
    let f = app.dialogs.form.as_mut().expect("a form is open");
    let i = f.form.fields.iter().position(|x| x.name == field).expect("a field of that name");
    f.entries[i].edit.replace_text(text);
}

fn asked(app: &mut App, text: &str) {
    app.dialogs.find.as_mut().expect("Find is open").edit.replace_text(text);
}

#[test]
fn a_form_runs_its_command_through_the_engine_and_says_what_it_replied() {
    let mut app = App::sample();
    assert!(app.open_command("LL"), "a shortcut opens the command's form");
    typed(&mut app, "elevation", "9000");
    typed(&mut app, "name", "Attic");
    assert!(app.run_form(), "{:?}", app.dialogs.form.as_ref().map(|f| &f.error));
    assert!(app.levels.iter().any(|(_, n)| n == "Attic"), "the engine made the level: {:?}", app.levels);
    let f = app.dialogs.form.as_ref().expect("the form stays open");
    assert!(f.error.is_none() && f.reply.as_deref().is_some_and(|r| r.contains("id")), "the reply is shown: {:?}", f.reply);
    assert!(app.message.is_none());
}

#[test]
fn what_stops_a_run_is_said_in_the_dialog_and_never_in_the_status_bar() {
    let mut app = App::sample();
    let levels = app.levels.clone();
    app.open_command("level");
    assert!(!app.run_form(), "a required field left empty");
    let why = app.dialogs.form.as_ref().and_then(|f| f.error.clone()).unwrap_or_default();
    assert_eq!(why, "elevation is required");

    typed(&mut app, "elevation", "not a number");
    assert!(!app.run_form());
    assert!(
        app.dialogs.form.as_ref().and_then(|f| f.error.as_deref()).is_some_and(|e| e.starts_with("elevation: ")),
        "the form's complaint"
    );

    // What the core refuses, given through the rest box (which the form does not read for a number).
    app.dialogs.form.as_mut().expect("open").rest.replace_text(r#"{"elevation": "abc"}"#);
    typed(&mut app, "elevation", "");
    assert!(!app.run_form());
    let f = app.dialogs.form.as_ref().expect("open");
    assert!(
        f.error.as_deref().is_some_and(|e| !e.is_empty()) && f.reply.is_none(),
        "the core's refusal is kept in the form: {:?}",
        f.error
    );
    assert!(app.message.is_none(), "the status bar is left alone");
    assert_eq!(app.levels, levels, "a refused command changes nothing");
}

#[test]
fn a_command_that_replaces_the_project_starts_the_view_again() {
    let mut app = App::sample();
    app.select(app.levels.iter().map(|(id, _)| *id).take(1).collect());
    assert!(app.open_command("new"));
    assert!(app.run_form(), "{:?}", app.dialogs.form.as_ref().map(|f| &f.error));
    let names: Vec<&str> = app.levels.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, ["Level 1", "Level 2"], "a new project");
    assert!(app.selection.is_empty());
    assert!(app.drawing.is_some(), "its first level is drawn");
}

#[test]
fn a_command_the_core_does_not_have_opens_nothing() {
    let mut app = App::sample();
    assert!(!app.open_command("no such command"));
    assert!(!app.dialogs.any());
    assert!(!app.run_form(), "no form, nothing to run");
}

#[test]
fn choices_are_made_and_close_their_list_and_a_dialog_closes_whole() {
    let mut app = App::sample();
    app.open_command("tower");
    let plate = app.dialogs.form.as_ref().and_then(|f| f.form.fields.iter().position(|x| x.name == "plate")).expect("plate is a field");
    app.toggle_choices(plate);
    assert_eq!(app.dialogs.form.as_ref().and_then(|f| f.open), Some(plate));
    app.choose(plate, "small".into());
    let f = app.dialogs.form.as_ref().expect("open");
    assert_eq!((f.open, f.entries[plate].choice.as_str()), (None, "small"));
    app.toggle_choices(plate);
    app.toggle_choices(plate);
    assert_eq!(app.dialogs.form.as_ref().and_then(|f| f.open), None, "the same field again closes its list");
    app.choose(99, "x".into());
    assert!(app.dialogs.any());
    app.close_dialogs();
    assert!(!app.dialogs.any());
}

#[test]
fn find_puts_a_tool_in_hand_opens_a_form_and_shows_a_level() {
    let mut app = App::sample();
    app.toggle_find();
    assert!(app.dialogs.find.as_ref().is_some_and(|f| f.edit.focused), "it opens on its field");
    assert!(app.found().is_empty(), "nothing typed lists nothing");

    asked(&mut app, "wall");
    assert_eq!(app.found().first().map(|h| h.found.clone()), Some(Found::Tool("wall")));
    app.pick_found(0);
    assert_eq!(app.tool, Some("wall"));
    assert!(app.dialogs.find.is_none(), "picking closes it");

    app.toggle_find();
    asked(&mut app, "wall");
    app.pick_current();
    assert_eq!(app.tool, Some("wall"), "a tool already in hand stays in hand");

    app.toggle_find();
    asked(&mut app, "adaptive_move");
    app.pick_current();
    assert_eq!(app.dialogs.form.as_ref().map(|f| f.form.id), Some("adaptive_move"));
    assert!(app.dialogs.find.is_none());

    let (other, name) = app.levels.iter().find(|(id, _)| Some(*id) != app.level).cloned().expect("a second level");
    app.toggle_find();
    asked(&mut app, &name);
    let at = app.found().iter().position(|h| h.found == Found::Level(other)).expect("the level is listed");
    app.pick_found(at);
    assert_eq!(app.level, Some(other));
    assert!(app.dialogs.form.is_none() && app.dialogs.find.is_none(), "a form is replaced by what is picked, never stacked");
}

#[test]
fn the_arrow_keys_stay_inside_the_list_and_ctrl_k_again_closes() {
    let mut app = App::sample();
    app.toggle_find();
    app.find_step(1);
    assert_eq!(app.dialogs.find.as_ref().map(|f| f.at), Some(0), "an empty list has one place");
    asked(&mut app, "wall");
    let n = app.found().len();
    assert!(n > 2);
    app.find_step(-1);
    assert_eq!(app.dialogs.find.as_ref().map(|f| f.at), Some(0));
    app.find_step(1);
    assert_eq!(app.dialogs.find.as_ref().map(|f| f.at), Some(1));
    app.find_step(isize::MAX);
    assert_eq!(app.dialogs.find.as_ref().map(|f| f.at), Some(n - 1));
    app.toggle_find();
    assert!(app.dialogs.find.is_none());
    app.pick_found(0);
    assert!(app.tool.is_none(), "nothing is picked with Find closed");
}
