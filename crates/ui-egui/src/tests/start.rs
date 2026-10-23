use super::*;
use std::time::{Duration, Instant};

/// The app on its start page, its recent files kept in `dir`.
fn home(dir: &std::path::Path, files: &[&std::path::Path]) -> Harness<'static, BimApp> {
    let mut h = app("");
    h.state_mut().keep_recent_in(dir.join("recent.json"));
    for f in files.iter().rev() {
        h.state_mut().note_recent(f);
    }
    h.state_mut().show_start_page();
    h.run();
    h
}

/// Frames until every tile's picture is drawn, or found not to be.
fn settle(h: &mut Harness<BimApp>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !h.state().home.settled() {
        assert!(Instant::now() < deadline, "the pictures were not drawn");
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    h.run();
}

fn project(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "closed": true})).unwrap();
    s.execute("save", &json!({"path": path})).unwrap();
    path
}

#[test]
fn the_start_page_offers_new_and_open_and_says_where_the_source_is() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = home(dir.path(), &[]);
    assert!(h.query_by_label("Architecture").is_none(), "no ribbon behind it");
    h.get_by_label("Open…");
    h.get_by_label("No recent projects yet");
    h.get_by_label("Source code on GitHub");
    h.get_by_label("This copy doesn't look for updates.");
    assert!(h.query_by_label("Back to Project").is_none(), "an empty project is nothing to go back to");
    press(&mut h, "New Project");
    assert!(!h.state().home.shown);
    h.get_by_label("Architecture");
    // Home brings it back over the project, which Back leaves as it was.
    tool(&mut h, "Wall");
    press(&mut h, "Home");
    assert!(h.state().home.shown);
    h.state_mut().run("wall", json!({"points": [[0, 0], [3000, 0]]}));
    h.run();
    let back = format!("Back to {}", h.state().session.model.name);
    press(&mut h, &back);
    assert!(!h.state().home.shown);
    assert_eq!(walls(&h).len(), 1);
}

#[test]
fn ctrl_d_and_the_back_arrow_go_between_the_start_page_and_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = home(dir.path(), &[]);
    // Opened on no project, there is nothing behind it to go back to.
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    assert!(h.state().home.shown);
    assert!(h.query_by_label("Back").is_none());
    press(&mut h, "New Project");
    h.state_mut().run("wall", json!({"points": [[0, 0], [3000, 0]]}));
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    assert!(h.state().home.shown, "Ctrl+D brings it up over the project");
    press(&mut h, "Back");
    assert!(!h.state().home.shown, "the arrow at the top goes back");
    press(&mut h, "Home");
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    assert!(!h.state().home.shown, "and so does Ctrl+D");
    assert_eq!(walls(&h).len(), 1);
    // In a field the keys are the field's: Ctrl+D there doesn't leave the project.
    h.get_by_label("View Name").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    assert!(!h.state().home.shown, "typing, Ctrl+D stays in the field");
}

#[test]
fn recent_files_are_tiles_with_a_picture_that_open_with_a_click() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (project(dir.path(), "office.bimcraft"), project(dir.path(), "school.bimcraft"));
    let gone = dir.path().join("gone.bimcraft");
    let mut h = home(dir.path(), &[&a, &b, &gone]);
    settle(&mut h);
    assert!(h.state().home.has_picture(&std::path::absolute(&a).unwrap()));
    assert!(!h.state().home.has_picture(&std::path::absolute(&gone).unwrap()));
    // A file that's gone can be taken off the list.
    h.get_by_label("gone.bimcraft").click_secondary();
    h.run();
    press(&mut h, "Remove from List");
    assert!(h.query_by_label("gone.bimcraft").is_none());
    assert_eq!(h.state().recent.paths().len(), 2);
    press(&mut h, "school.bimcraft");
    assert!(!h.state().home.shown);
    assert_eq!(h.state().session.path.as_deref().and_then(|p| p.file_name()), b.file_name());
    assert_eq!(walls(&h).len(), 4);
}

#[test]
fn the_footer_says_whether_an_update_is_out() {
    use bimcraft_update::testing::{Fake, release_json};
    use bimcraft_update::{LATEST_URL, Updater, Version};
    let dir = tempfile::tempdir().unwrap();
    let offer = |tag: &str| {
        let net = std::sync::Arc::new(Fake::default().answer(LATEST_URL, release_json(tag, None)));
        Updater::new(net, Version::new(0, 4, 0), None, || {}).inline()
    };
    let mut h = home(dir.path(), &[]);
    h.state_mut().updates = Some(offer("v0.5.0"));
    h.run();
    h.get_by_label("Check for updates").click();
    h.run();
    h.get_by_label("BIMCraft 0.5.0 is available");
    h.get_by_label("Download");
    h.state_mut().updates = Some(offer("v0.4.0"));
    h.state_mut().updates.as_mut().unwrap().check_now();
    h.run();
    h.get_by_label("BIMCraft 0.4.0 is up to date");
}
