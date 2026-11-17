//! Project templates: New asks what to start from, a template picked is a new project with no file, Settings > Templates puts
//! them in order, and Save As > Template… makes one.
use super::*;
use std::path::{Path, PathBuf};

use crate::project_templates::MAX_SHOWN;

/// The system's dialog answered with `path`.
fn answer(h: &mut Harness<BimApp>, path: &Path) {
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a dialog was begun");
    tx.send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}

/// A template of `walls` walls, written at `dir/name`.
fn template(dir: &Path, name: &str, walls: usize) -> PathBuf {
    let path = dir.join(name);
    let mut s = Session::new();
    for i in 0..walls {
        let y = 3000 * i as i64;
        s.execute("wall", &json!({"points": [[0, y], [5000, y]]})).unwrap();
    }
    s.execute("save", &json!({"path": path})).unwrap();
    path
}

/// The app on the box, its templates kept in `dir` and these listed.
fn with_templates(dir: &Path, listed: &[&Path]) -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    h.state_mut().keep_templates_in(dir.join("templates.json"));
    for p in listed {
        h.state_mut().templates.add(p);
    }
    h.state_mut().keep_templates();
    h.run();
    h
}

/// Past "Save changes to …?" when it asks.
fn dont_save(h: &mut Harness<BimApp>) {
    if h.query_by_label("Don't Save").is_some() {
        press(h, "Don't Save");
    }
}

#[test]
fn new_lists_the_templates_and_one_picked_is_a_new_project_with_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let office = template(dir.path(), "Office.bimt", 2);
    let house = template(dir.path(), "House.bimt", 3);
    let bytes = std::fs::read(&office).unwrap();
    let mut h = with_templates(dir.path(), &[&house, &office]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    // The popup: the templates in their order, an empty project, and another from the system's dialog.
    h.get_by_label("Start from a template");
    let top = |h: &Harness<BimApp>, l: &str| h.get_by_label(l).rect().top();
    assert!(top(&h, "House.bimt") < top(&h, "Office.bimt"), "in the list's order");
    h.get_by_label("Empty Project");
    h.get_by_label("Browse…");
    press(&mut h, "Office.bimt");
    dont_save(&mut h);
    assert!(h.query_by_label("Start from a template").is_none(), "the popup is gone");
    assert_eq!(walls(&h).len(), 2, "the template's content");
    assert_eq!(h.state().session.path, None, "a new project, with no file");
    assert!(h.state().status.text.contains("Office.bimt"), "{}", h.state().status.text);
    // Saving asks where; the template is never written over.
    h.state_mut().run("wall", json!({"points": [[0, 9000], [5000, 9000]]}));
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    answer(&mut h, &dir.path().join("Mine"));
    assert_eq!(h.state().session.path.as_deref(), Some(dir.path().join("Mine.bimc").as_path()));
    assert_eq!(std::fs::read(&office).unwrap(), bytes, "the template is as it was");
    // A template already listed keeps its place, and the list is kept in its file.
    let kept = crate::project_templates::Templates::load(&dir.path().join("templates.json"));
    assert_eq!(kept.labels(), ["House.bimt", "Office.bimt"]);
    // Empty Project is the empty project New always made.
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    press(&mut h, "Empty Project");
    dont_save(&mut h);
    assert!(walls(&h).is_empty());
    assert_eq!(h.state().session.path, None);
}

#[test]
fn browse_opens_another_template_and_new_lists_it_first_from_then_on() {
    let dir = tempfile::tempdir().unwrap();
    let office = template(dir.path(), "Office.bimt", 2);
    let shed = template(dir.path(), "Shed.bimt", 1);
    let mut h = with_templates(dir.path(), &[&office]);
    // From the start page too.
    h.state_mut().show_start_page();
    h.run();
    press(&mut h, "New Project");
    press(&mut h, "Browse…");
    answer(&mut h, &shed);
    dont_save(&mut h);
    assert_eq!((walls(&h).len(), h.state().session.path.clone()), (1, None));
    assert!(!h.state().home.shown, "the start page made way for the project");
    let kept = crate::project_templates::Templates::load(&dir.path().join("templates.json"));
    assert_eq!(kept.labels(), ["Shed.bimt", "Office.bimt"], "listed first, so New shows it");
}

#[test]
fn a_template_that_is_gone_is_said_and_taken_off_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let office = template(dir.path(), "Office.bimt", 2);
    let mut h = with_templates(dir.path(), &[&office]);
    std::fs::remove_file(&office).unwrap();
    let before = walls(&h).len();
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    press(&mut h, "Office.bimt");
    dont_save(&mut h);
    assert!(h.state().status.error && h.state().status.text.contains("Office.bimt"), "{}", h.state().status.text);
    assert_eq!(walls(&h).len(), before, "the project stays as it was");
    assert!(h.state().templates().paths().is_empty());
}

#[test]
fn new_lists_ten_and_settings_lists_them_all_to_put_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<PathBuf> = (0..MAX_SHOWN + 2).map(|n| template(dir.path(), &format!("T{n:02}.bimt"), 1)).collect();
    let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    let mut h = with_templates(dir.path(), &refs);
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    assert!(h.query_by_label("T09.bimt").is_some() && h.query_by_label("T10.bimt").is_none(), "the first ten");
    h.get_by_label("2 more in Settings > Templates; move one up to list it here.");
    press(&mut h, "Cancel");
    assert!(h.query_by_label("Start from a template").is_none());
    // Settings > Templates: all twelve. The last, moved up twice, is among the ten.
    h.state_mut().settings_page = Some(crate::settings::Page::Templates);
    h.run();
    for n in 0..MAX_SHOWN + 2 {
        h.get_by_label(&format!("T{n:02}.bimt"));
    }
    h.get_by_label("Not listed by New:");
    // The last is below the list's fold.
    h.get_by_label("T11.bimt").scroll_to_me();
    h.run();
    press(&mut h, "T11.bimt");
    press(&mut h, "Move Up");
    press(&mut h, "Move Up");
    assert_eq!(h.state().templates().shown().last(), Some(&std::path::absolute(&paths[11]).unwrap()));
    // Remove takes the one picked off; the file is left alone.
    h.get_by_label("T00.bimt").scroll_to_me();
    h.run();
    press(&mut h, "T00.bimt");
    press(&mut h, "Remove");
    assert_eq!(h.state().templates().paths().len(), MAX_SHOWN + 1);
    assert!(paths[0].exists());
    let kept = crate::project_templates::Templates::load(&dir.path().join("templates.json"));
    assert_eq!(kept, *h.state().templates(), "kept in its file");
    // Add… puts one at the end.
    let more = template(dir.path(), "More.bimt", 1);
    press(&mut h, "Add…");
    answer(&mut h, &more);
    assert_eq!(h.state().templates().paths().last(), Some(&std::path::absolute(&more).unwrap()));
    press(&mut h, "Close");
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    assert!(h.query_by_label("T11.bimt").is_some() && h.query_by_label("T00.bimt").is_none());
}

#[test]
fn save_as_template_writes_a_bimt_lists_it_and_the_project_keeps_its_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = with_templates(dir.path(), &[]);
    let ctx = h.ctx.clone();
    h.state_mut().save_as(&ctx, None);
    answer(&mut h, &dir.path().join("House"));
    let project = dir.path().join("House.bimc");
    assert_eq!(h.state().session.path.as_deref(), Some(project.as_path()));
    h.state_mut().run("wall", json!({"points": [[0, 9000], [5000, 9000]]}));
    h.run();
    // File > Save As > Template….
    press(&mut h, "File");
    press(&mut h, "Save As ⏵");
    press(&mut h, "Template…");
    answer(&mut h, &dir.path().join("Studio"));
    let template = dir.path().join("Studio.bimt");
    assert!(template.exists());
    assert_eq!(h.state().session.path.as_deref(), Some(project.as_path()), "the project keeps its own file");
    assert!(h.state().dirty(), "the project itself is still unsaved");
    assert_eq!(h.state().templates().labels(), ["Studio.bimt"]);
    // New from it: the walls it was saved with.
    let n = walls(&h).len();
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    press(&mut h, "Studio.bimt");
    dont_save(&mut h);
    assert_eq!((walls(&h).len(), h.state().session.path.clone()), (n, None));
}

/// What New's popup and Settings > Templates look like, for a person to look at: `BIMCRAFT_TEMPLATES_DUMP=<folder>` writes both
/// with twelve templates kept, in the three looks.
#[test]
fn templates_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_TEMPLATES_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let names = ["Residential", "Office", "Structure", "MEP", "Interiors", "Site", "Landscape", "School", "Hospital", "Retail"];
    let mut paths: Vec<PathBuf> = names.iter().map(|n| template(dir.path(), &format!("{n}.bimt"), 1)).collect();
    paths.push(template(dir.path(), "Warehouse.bimt", 1));
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    paths.push(template(&other, "Office.bimt", 1));
    let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-light-canvas", true, true)] {
        let mut h = with_templates(dir.path(), &refs);
        h.state_mut().light_canvas = light_canvas;
        h.ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        h.run();
        h.render().unwrap().save(Path::new(&folder).join(format!("new-{look}.png"))).unwrap();
        press(&mut h, "Cancel");
        h.state_mut().settings_page = Some(crate::settings::Page::Templates);
        h.run();
        h.get_by_label("Warehouse.bimt").scroll_to_me();
        h.run();
        press(&mut h, "Warehouse.bimt");
        h.render().unwrap().save(Path::new(&folder).join(format!("settings-{look}.png"))).unwrap();
        press(&mut h, "Close");
        press(&mut h, "File");
        press(&mut h, "Save As ⏵");
        h.render().unwrap().save(Path::new(&folder).join(format!("save-as-{look}.png"))).unwrap();
    }
}
