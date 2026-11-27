//! Find and the generic command form driven as a person drives them, headless: Rune's `Runtime` on the app's real state and view, a
//! key or a press at a window point, a paste into the field that has the focus, and the elements and the engine read back.

use rune::{Env, Key, KeyEvent, Mods, Runtime};

use super::{on_key, view};
use crate::app::App;

fn window() -> Runtime<App> {
    let mut rt = Runtime::new(App::sample(), view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    for _ in 0..3 {
        rt.frame(0.0);
    }
    rt
}

/// A press on an element keyed `key`. A dialog opens on a spring and takes presses once it has come to rest (the kit's own dialog tests
/// frame 60 times at 1/60 s before they press), and `rt.frame(0.0)` does not move the clock, so the clock is run first.
fn tap(rt: &mut Runtime<App>, key: &str) {
    settle(rt);
    let at = rt.find(key).unwrap_or_else(|| panic!("an element keyed {key}"));
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

fn press(rt: &mut Runtime<App>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    for _ in 0..2 {
        rt.frame(0.0);
    }
}

fn ctrl() -> Mods {
    Mods { cmd: true, ..Mods::default() }
}

fn text_of(rt: &Runtime<App>, key: &str) -> Option<String> {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.text)
}

fn settle(rt: &mut Runtime<App>) {
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn no_dialog_is_drawn_until_one_is_open() {
    let rt = window();
    assert!(rt.find("find-input").is_none() && rt.find("form-run").is_none());
    assert!(rt.find("find").is_some(), "the top bar has a Find button");
}

#[test]
fn ctrl_k_opens_find_and_what_is_typed_lists_rows_and_a_press_takes_one() {
    let mut rt = window();
    press(&mut rt, Key::Command('k'), ctrl());
    assert!(rt.state.dialogs.find.is_some(), "Ctrl+K opens Find");
    assert!(rt.find("find-input").is_some());
    tap(&mut rt, "find-input");
    rt.paste("wall");
    rt.frame(0.0);
    rt.frame(0.0);
    assert_eq!(rt.state.dialogs.find.as_ref().map(|f| f.edit.text.as_str()), Some("wall"), "what is typed goes to its field");
    assert!(rt.find("hit-0").is_some(), "the best match is a row");
    tap(&mut rt, "hit-0");
    assert_eq!(rt.state.tool, Some("wall"), "a tool is put in hand");
    assert!(rt.state.dialogs.find.is_none() && rt.find("find-input").is_none(), "Find closes");

    press(&mut rt, Key::Command('k'), ctrl());
    assert!(rt.state.dialogs.find.is_some());
    press(&mut rt, Key::Command('k'), ctrl());
    assert!(rt.state.dialogs.find.is_none(), "Ctrl+K again closes it");
}

#[test]
fn escape_closes_a_dialog() {
    let mut rt = window();
    tap(&mut rt, "find");
    assert!(rt.state.dialogs.find.is_some(), "the Find button opens it");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.dialogs.find.is_none(), "Escape closes it");
    rt.update(|s, _| {
        s.open_command("level");
    });
    rt.frame(0.0);
    assert!(rt.find("form-run").is_some());
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.dialogs.form.is_none() && rt.find("form-run").is_none(), "Escape closes a form");
}

#[test]
fn a_form_is_filled_run_and_the_engine_makes_what_it_says() {
    let mut rt = window();
    rt.update(|s, _| {
        s.open_command("level");
    });
    rt.frame(0.0);
    rt.frame(0.0);
    let fields: Vec<String> =
        rt.state.dialogs.form.as_ref().map(|f| f.form.fields.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
    let (elevation, name) =
        (fields.iter().position(|n| n == "elevation").expect("elevation"), fields.iter().position(|n| n == "name").expect("name"));
    tap(&mut rt, &format!("field-{elevation}"));
    rt.paste("9000");
    tap(&mut rt, &format!("field-{name}"));
    rt.paste("Attic");
    rt.frame(0.0);
    tap(&mut rt, "form-run");
    assert!(
        rt.state.levels.iter().any(|(_, n)| n == "Attic"),
        "the engine made the level: {:?}",
        rt.state.dialogs.form.as_ref().map(|f| &f.error)
    );
    assert!(rt.find("form-reply").is_some(), "the reply is shown, in the dialog");
    assert!(rt.find("form-error").is_none());
}

#[test]
fn what_stops_a_run_is_said_inside_the_dialog() {
    let mut rt = window();
    rt.update(|s, _| {
        s.open_command("level");
    });
    rt.frame(0.0);
    rt.frame(0.0);
    tap(&mut rt, "form-run");
    assert_eq!(text_of(&rt, "form-error").as_deref(), Some("elevation is required"));
    assert!(rt.find("form-run").is_some(), "and the buttons are still there to press");
    assert!(rt.state.message.is_none());
}

#[test]
fn a_choice_is_made_from_its_list() {
    let mut rt = window();
    rt.update(|s, _| {
        s.open_command("level");
    });
    rt.frame(0.0);
    rt.frame(0.0);
    let base =
        rt.state.dialogs.form.as_ref().and_then(|f| f.form.fields.iter().position(|x| x.name == "elevation_base")).expect("a choice");
    tap(&mut rt, &format!("choice-{base}-trigger"));
    assert_eq!(rt.state.dialogs.form.as_ref().and_then(|f| f.open), Some(base), "its list opens");
    settle(&mut rt);
    tap(&mut rt, "option2");
    assert_eq!(
        rt.state.dialogs.form.as_ref().map(|f| f.entries[base].choice.as_str()),
        Some("survey"),
        "the third row is the second choice"
    );
    assert_eq!(rt.state.dialogs.form.as_ref().and_then(|f| f.open), None);
    assert!(rt.state.dialogs.form.is_some(), "and the form stays open");
}

#[test]
fn settings_open_from_the_bar_and_from_ctrl_comma_and_close() {
    let mut rt = window();
    tap(&mut rt, "settings");
    assert!(rt.state.dialogs.settings.is_some(), "the Settings button opens it");
    for key in ["language-row", "units-row", "theme-row", "profile-row", "settings-project-info", "settings-close"] {
        assert!(rt.find(key).is_some(), "{key} is in the dialog");
    }
    tap(&mut rt, "settings-close");
    assert!(rt.state.dialogs.settings.is_none() && rt.find("settings-close").is_none());
    press(&mut rt, Key::Command(','), ctrl());
    assert!(rt.state.dialogs.settings.is_some(), "Ctrl+, opens it");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.dialogs.settings.is_none(), "Escape closes it");
}

#[test]
fn a_unit_chosen_in_settings_is_the_projects_through_the_engine() {
    let mut rt = window();
    tap(&mut rt, "settings");
    tap(&mut rt, "units-trigger");
    assert_eq!(rt.state.dialogs.settings.as_ref().and_then(|d| d.open), Some(crate::app::settings::Choice::Units), "its list opens");
    settle(&mut rt);
    tap(&mut rt, "option4");
    assert_eq!(rt.state.session.model.units, bimcraft_core::units::Units::Feet, "the fifth unit is feet");
    assert!(rt.state.dialogs.settings.as_ref().is_some_and(|d| d.open.is_none() && d.error.is_none()));
}

#[test]
fn the_theme_and_the_profile_are_chosen_and_the_window_is_drawn_again() {
    let mut rt = window();
    tap(&mut rt, "settings");
    tap(&mut rt, "theme-trigger");
    settle(&mut rt);
    tap(&mut rt, "option1");
    assert!(!rt.state.dark(), "the second theme is light");
    assert!(rt.find("settings-close").is_some(), "and the dialog is still drawn");
    tap(&mut rt, "profile-trigger");
    settle(&mut rt);
    tap(&mut rt, "option1");
    assert_eq!(rt.state.profile, crate::profile::Profile::Rev);
    assert!(rt.find("ribbon").is_some(), "Rev draws its ribbon");
}

#[test]
fn project_information_is_the_commands_own_form() {
    let mut rt = window();
    tap(&mut rt, "settings");
    tap(&mut rt, "settings-project-info");
    assert!(rt.state.dialogs.settings.is_none(), "Settings gives way");
    assert_eq!(rt.state.dialogs.form.as_ref().map(|f| f.form.id), Some("project_info"));
    assert!(rt.find("form-run").is_some());
}

#[test]
fn edit_type_opens_type_properties_on_the_selected_walls_type_and_apply_changes_it() {
    let mut rt = window();
    rt.update(|s, _| {
        let made = s.session.execute("type", &serde_json::json!({"category": "wall", "name": "Rune Test Wall", "thickness": 200}));
        assert!(made.is_ok(), "{made:?}");
        let wall = s.session.execute("wall", &serde_json::json!({"points": [[50000, 50000], [53000, 50000]], "type": "Rune Test Wall"}));
        assert!(wall.is_ok(), "{wall:?}");
    });
    let model = rt.state.session.model.clone();
    let id = model
        .elements
        .iter()
        .map(|(id, _)| *id)
        .find(|id| bimcraft_core::element_json(&model, *id)["type"] == "Rune Test Wall")
        .expect("the wall");
    rt.update(|s, _| s.select(vec![id]));
    rt.frame(0.0);
    rt.frame(0.0);
    tap(&mut rt, "edit-type");
    assert!(rt.state.dialogs.types.is_some(), "Edit Type opens it");
    let i = rt.state.dialogs.types.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == "thickness")).expect("thickness");
    tap(&mut rt, &format!("type-field-{i}"));
    press(&mut rt, Key::Command('a'), ctrl());
    rt.paste("350");
    rt.frame(0.0);
    tap(&mut rt, "type-apply");
    assert_eq!(rt.state.session.model.types.wall["Rune Test Wall"].thickness, 350.0, "the type changed through the engine");
    assert!(rt.find("type-done").is_some() && rt.find("type-error").is_none());
    tap(&mut rt, "type-close");
    assert!(rt.state.dialogs.types.is_none());
}

#[test]
fn edit_type_with_nothing_selected_says_so_in_the_status_bar() {
    let mut rt = window();
    assert!(rt.find("edit-type").is_none(), "the palette has no type to edit with nothing selected");
    rt.update(|s, _| {
        s.open_type_of_selection();
    });
    assert!(rt.state.dialogs.types.is_none());
    assert!(rt.state.message.is_some());
}

#[test]
fn the_manage_button_lists_the_managers_and_a_press_opens_one() {
    let mut rt = window();
    tap(&mut rt, "manage");
    assert_eq!(rt.state.dialogs.find.as_ref().map(|f| f.edit.text.as_str()), Some("manage"), "Find opens on the word");
    assert!(rt.find("hit-11").is_some(), "the twelve managers are listed");
    tap(&mut rt, "hit-0");
    assert_eq!(rt.state.dialogs.library.as_ref().map(|d| d.info.source), Some(crate::library::Source::Materials));
    assert!(rt.state.dialogs.find.is_none() && rt.find("lib-apply").is_some());
    tap(&mut rt, "lib-close");
    assert!(rt.state.dialogs.library.is_none());
}

#[test]
fn an_entry_of_a_manager_is_edited_and_applied_through_the_engine() {
    let mut rt = window();
    rt.update(|s, _| {
        let made = s.session.execute("text_type", &serde_json::json!({"name": "Rune Text", "size": 3}));
        assert!(made.is_ok(), "{made:?}");
        s.open_library(crate::library::Source::TextTypes);
        s.library_select("Rune Text");
    });
    rt.frame(0.0);
    rt.frame(0.0);
    let i = rt.state.dialogs.library.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == "size")).expect("size");
    tap(&mut rt, &format!("lib-field-{i}"));
    press(&mut rt, Key::Command('a'), ctrl());
    rt.paste("7");
    rt.frame(0.0);
    tap(&mut rt, "lib-apply");
    assert_eq!(rt.state.session.model.text_types["Rune Text"].size, 7.0, "the entry changed through the engine");
    assert!(rt.find("lib-done").is_some() && rt.find("lib-error").is_none());
    assert!(rt.find("lib-delete").is_some() && rt.find("lib-rename").is_some(), "a text type is renamed and deleted");
}

#[test]
fn the_locations_manager_offers_the_forms_of_the_commands_that_go_with_it() {
    let mut rt = window();
    rt.update(|s, _| s.open_library(crate::library::Source::Locations));
    rt.frame(0.0);
    rt.frame(0.0);
    let n = crate::library::Source::Locations.also().len();
    assert!(rt.find(&format!("lib-also-{}", n - 1)).is_some() && rt.find("lib-also-0").is_some(), "one button for each command");
    tap(&mut rt, "lib-also-1");
    assert_eq!(rt.state.dialogs.form.as_ref().map(|f| f.form.id), Some("specify_coordinates"), "Specify Coordinates at a Point");
    assert!(rt.state.dialogs.library.is_none(), "dialogs are open one at a time");
}
