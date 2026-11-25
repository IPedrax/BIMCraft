//! The settings' state and handlers with no window: the file in a folder of the test's own, and `App` driven by its methods.

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-rune-app-settings-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    dir
}

#[test]
fn the_file_sets_the_language_and_the_theme_and_the_profile_unless_the_arguments_asked() {
    let dir = folder("start");
    let path = dir.join(settings::FILE);
    std::fs::write(&path, r#"{"language": "pt", "theme": "light", "profile": "rev", "accent": [1, 2, 3]}"#).expect("a file");

    let mut app = App::sample();
    app.use_settings(path.clone(), false);
    assert_eq!(app.profile, Profile::Rev);
    assert!(!app.dark(), "light was chosen");
    assert_eq!(lang::t("Properties"), "Propriedades", "the language is spoken from the first frame");

    let mut asked = App::sample();
    asked.use_settings(path, true);
    assert_eq!(asked.profile, Profile::Bimcraft, "a profile the arguments asked for is not the file's to change");
    lang::set("en");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_choice_is_kept_in_the_file_beside_what_the_egui_app_keeps() {
    let dir = folder("choice");
    let path = dir.join(settings::FILE);
    std::fs::write(&path, r#"{"accent": [9, 9, 9], "shortcuts": {"version": 2}}"#).expect("a file");
    let mut app = App::sample();
    app.use_settings(path.clone(), false);

    assert!(app.set_language("de"));
    assert_eq!(lang::t("Properties"), "Eigenschaften");
    app.set_theme(Theme::Dark);
    app.set_profile(Profile::Rev);
    let file = settings::read(&path).expect("the file");
    assert_eq!((file["language"].as_str(), file["theme"].as_str(), file["profile"].as_str()), (Some("de"), Some("dark"), Some("rev")));
    assert_eq!(file["accent"], json!([9, 9, 9]), "the egui app's keys are still there");
    assert_eq!(file["shortcuts"], json!({"version": 2}));

    assert!(!app.set_language("klingon"), "a language with no catalog is refused");
    assert_eq!(settings::load(&path).language.as_deref(), Some("de"));
    lang::set("en");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn with_no_file_a_choice_still_takes_effect_and_nothing_is_written() {
    let mut app = App::sample();
    assert!(app.prefs.path.is_none());
    app.set_theme(Theme::Light);
    assert!(!app.dark());
    app.set_profile(Profile::Rev);
    assert_eq!(app.profile, Profile::Rev);
    assert!(app.message.is_none());
}

#[test]
fn a_file_that_cannot_be_written_is_said_in_the_dialog_when_it_is_open_and_else_in_the_status_bar() {
    let dir = folder("unwritable");
    // A folder where the file should be: the rename cannot replace it.
    let path = dir.join(settings::FILE);
    std::fs::create_dir_all(&path).expect("a folder in the file's place");
    let mut app = App::sample();
    app.prefs.path = Some(path);
    app.set_theme(Theme::Dark);
    assert!(app.message.is_some(), "no dialog is open, so the status bar says it");
    app.message = None;
    app.toggle_settings();
    app.set_theme(Theme::Light);
    assert!(app.message.is_none() && app.dialogs.settings.as_ref().is_some_and(|d| d.error.is_some()), "the dialog says it");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_units_are_the_projects_and_go_through_the_engine() {
    let mut app = App::sample();
    app.toggle_settings();
    assert!(app.set_units("ft"));
    assert_eq!(app.session.model.units, Units::Feet);
    assert!(!app.set_units("furlong"), "a unit the core does not have is refused before it is asked");
    assert_eq!(app.session.model.units, Units::Feet);
    assert!(app.set_units("mm"));
    assert_eq!(app.session.model.units, Units::Millimetres);
    assert!(app.dialogs.settings.as_ref().is_some_and(|d| d.error.is_none()));
}

#[test]
fn the_window_size_is_kept_once_it_has_held_still_and_never_a_size_no_window_can_be() {
    let dir = folder("window");
    let path = dir.join(settings::FILE);
    let mut app = App::sample();
    app.window_seen(1400.0, 800.0);
    assert_eq!(app.window_due(), None, "with no file there is nothing to keep it in");
    app.use_settings(path.clone(), false);
    app.tick(10.0);
    app.window_seen(1500.0, 900.0);
    assert_eq!(app.window_due(), Some(0.5));
    app.tick(10.25);
    assert!(!app.save_window_if_still(), "a window dragged on its edge is not yet kept");
    app.window_seen(1500.0, 900.0);
    assert_eq!(app.window_due(), Some(0.25), "the same size again does not start the wait over");
    app.window_seen(f32::NAN, 900.0);
    app.window_seen(20.0, 20.0);
    app.window_seen(f32::INFINITY, f32::INFINITY);
    assert_eq!(app.window_due(), Some(0.25), "sizes no window can be are not believed");
    app.tick(10.6);
    assert!(app.save_window_if_still());
    assert_eq!(settings::load(&path).window.size, Some([1500.0, 900.0]));
    assert!(!app.save_window_if_still() && app.window_due().is_none(), "kept once");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn settings_open_and_close_alone_and_a_choice_closes_its_list() {
    let mut app = App::sample();
    app.toggle_find();
    app.toggle_settings();
    assert!(app.dialogs.settings.is_some() && app.dialogs.find.is_none(), "one dialog at a time");
    app.toggle_choice(Choice::Units);
    assert_eq!(app.dialogs.settings.as_ref().and_then(|d| d.open), Some(Choice::Units));
    app.toggle_choice(Choice::Units);
    assert_eq!(app.dialogs.settings.as_ref().and_then(|d| d.open), None);
    app.toggle_choice(Choice::Theme);
    app.set_theme(Theme::Dark);
    assert_eq!(app.dialogs.settings.as_ref().and_then(|d| d.open), None, "choosing closes the list");
    app.toggle_settings();
    assert!(!app.dialogs.any());
    assert!(app.open_command("level"));
    app.toggle_settings();
    assert!(app.dialogs.form.is_none(), "Settings replaces a form");
    app.close_dialogs();
    assert!(!app.dialogs.any());
}
