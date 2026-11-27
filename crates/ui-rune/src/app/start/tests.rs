//! The Start dialog's state and handlers with no window: the lists it opens on, New and the sample house through the engine, a file
//! opened or saved put first in the recent files, and a refusal kept for the dialog.

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rune-start-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    dir
}

fn built(app: &App) -> usize {
    app.session.model.elements.iter().count()
}

#[test]
fn start_new_empties_the_project_and_the_sample_house_brings_it_back() {
    let mut app = App::sample();
    assert!(built(&app) > 0);
    app.open_start();
    assert!(app.dialogs.start.is_some() && app.dialogs.any());
    assert!(app.start_new(), "{:?}", app.dialogs.start.as_ref().map(|d| &d.error));
    assert_eq!((built(&app), app.dialogs.start.is_none()), (0, true), "a new project has nothing built and the dialog closes");
    app.open_start();
    assert!(app.start_sample());
    assert!(built(&app) > 0 && app.dialogs.start.is_none());
}

#[test]
fn a_file_saved_or_opened_is_put_first_in_the_recent_files_beside_the_settings() {
    let dir = folder("recent");
    let mut app = App::sample();
    app.prefs.path = Some(dir.join("settings.json"));
    let (a, b) = (dir.join("a.bimc"), dir.join("b.bimc"));
    assert!(app.save_as(a.clone()), "{:?}", app.message);
    assert!(app.save_as(b.clone()), "{:?}", app.message);
    app.open_start();
    let shown = app.dialogs.start.as_ref().map(|d| d.recent.clone()).unwrap_or_default();
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert!(shown[0].ends_with("b.bimc") && shown[1].ends_with("a.bimc"), "newest first: {shown:?}");
    // Opening the older one from the dialog puts it first again.
    assert!(app.start_open(&a), "{:?}", app.dialogs.start.as_ref().map(|d| &d.error));
    app.open_start();
    assert!(app.dialogs.start.as_ref().is_some_and(|d| d.recent.first().is_some_and(|p| p.ends_with("a.bimc"))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_will_not_open_is_said_in_the_dialog_and_the_project_stays() {
    let mut app = App::sample();
    let before = built(&app);
    app.open_start();
    assert!(!app.start_open(Path::new("/no/such/project.bimc")));
    assert!(app.dialogs.start.as_ref().is_some_and(|d| d.error.is_some()), "the dialog stays open with the reason");
    assert_eq!(built(&app), before);
}

#[test]
fn the_templates_the_settings_folder_keeps_are_listed() {
    let dir = folder("templates");
    let mut app = App::sample();
    app.prefs.path = Some(dir.join("settings.json"));
    recent::write(&recent::templates_file(&dir.join("settings.json")), &[PathBuf::from("/t/one.bimt")]).expect("a list");
    app.open_start();
    assert_eq!(app.dialogs.start.as_ref().map(|d| d.templates.clone()), Some(vec![PathBuf::from("/t/one.bimt")]));
    let _ = std::fs::remove_dir_all(&dir);
}
