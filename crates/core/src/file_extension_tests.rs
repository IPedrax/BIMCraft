//! A project is a `.bimc` and its backup a `.bimb`, a template a `.bimt` and a family a `.bimf`; the `.bimcraft` and
//! `.autosave.bimcraft` of earlier versions are still read and still found, and opening one never rewrites it (discussion 310).
use std::path::{Path, PathBuf};
use web_time::Duration;

use serde_json::json;

use crate::autosave::{Backup, Notice, Worker, backup_path, old_backup_path};
use crate::engine::Session;
use crate::files;

fn dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-extension-test-{}", crate::ids::Guid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn found(worker: &mut Worker, project: Option<PathBuf>) -> Vec<Backup> {
    worker.discover(project, 1).unwrap();
    let deadline = web_time::Instant::now() + Duration::from_secs(10);
    loop {
        match worker.poll() {
            Some(Notice::Discovered { result, .. }) => return result.unwrap(),
            Some(other) => panic!("{other:?}"),
            None => {}
        }
        assert!(web_time::Instant::now() < deadline, "worker did not reply");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_project_saves_as_bimc_and_the_names_say_what_each_extension_is() {
    assert_eq!((files::PROJECT, files::BACKUP, files::TEMPLATE, files::FAMILY), ("bimc", "bimb", "bimt", "bimf"));
    assert_eq!(files::file_name("House", files::PROJECT), "House.bimc");
    assert_eq!(files::file_name("A/B\\C", files::FAMILY), "A-B-C.bimf", "a name has no path in it");
    for e in ["bimc", "bimt", "bimb", "bimcraft", "revcraft", "BIMC", "BimCraft"] {
        assert!(files::is_project(Path::new(&format!("/x/a.{e}"))), "{e}");
    }
    for e in ["ifc", "bimf", "json", "rvt", "bimcrafts", "bimp", "png"] {
        assert!(!files::is_project(Path::new(&format!("/x/a.{e}"))), "{e}");
    }
    assert!(!files::is_project(Path::new("/x/bimc")));
    // A path typed in a save dialog keeps one of the extensions offered, else gets the first.
    let named = |path: &str, offered: &[&str]| files::named(PathBuf::from(path), offered);
    assert_eq!(named("/x/plan", &files::SAVED), PathBuf::from("/x/plan.bimc"));
    assert_eq!(named("/x/plan.BIMC", &files::SAVED), PathBuf::from("/x/plan.BIMC"));
    assert_eq!(named("/x/plan.bimt", &files::SAVED), PathBuf::from("/x/plan.bimt"));
    assert_eq!(named("/x/plan.png", &["ifc"]), PathBuf::from("/x/plan.ifc"));
    assert_eq!(named("/x/plan", &[]), PathBuf::from("/x/plan"), "nothing offered, nothing added");
    // Only a `.bimf` is known for a family at sight: a `.json` may be anything, and one that was a project opens as one.
    assert_eq!(files::FAMILIES, ["bimf", "json"]);
    assert!(files::is_family(Path::new("/x/a.bimf")) && files::is_family(Path::new("/x/a.BIMF")));
    assert!(!files::is_family(Path::new("/x/a.json")) && !files::is_family(Path::new("/x/a.bimc")));
}

#[test]
fn the_commands_say_bimc_and_still_name_the_old_extension() {
    let params = |id: &str| crate::commands::ALL.iter().find(|c| c.id == id).map(|c| c.params).unwrap();
    for id in ["open", "insert", "link", "save"] {
        assert!(params(id).contains(".bimc"), "{id}: {}", params(id));
    }
    assert!(params("open").contains(".bimcraft"), "an old file is said to open");
    assert!(params("family").contains(".bimf"), "{}", params("family"));
}

#[test]
fn a_backup_is_a_bimb_beside_its_project_and_the_old_name_is_known() {
    assert_eq!(backup_path(Path::new("/p/house.bimc")), PathBuf::from("/p/house.bimc.bimb"));
    assert_eq!(old_backup_path(Path::new("/p/house.bimcraft")), PathBuf::from("/p/house.bimcraft.autosave.bimcraft"));
}

#[test]
fn every_project_extension_saves_and_opens_and_an_old_file_is_left_as_it_was() {
    let dir = dir();
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    let text = s.model.to_json().unwrap();
    for ext in ["bimc", "bimt", "bimcraft"] {
        let path = dir.join(format!("house.{ext}"));
        s.execute("save", &json!({"path": path})).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "{ext}");
        let mut opened = Session::new();
        opened.execute("open", &json!({"path": path})).unwrap();
        assert_eq!(opened.model.to_json().unwrap(), text, "{ext}");
        assert_eq!(opened.path.as_ref(), Some(&path), "{ext}");
    }
    // A project from before: read through every way a file is read, and never renamed or rewritten for it.
    let old = dir.join("before.bimcraft");
    std::fs::write(&old, &text).unwrap();
    let (bytes, modified) = (std::fs::read(&old).unwrap(), std::fs::metadata(&old).unwrap().modified().unwrap());
    let names = || {
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };
    let before = names();
    let mut opened = Session::new();
    opened.execute("open", &json!({"path": old})).unwrap();
    assert_eq!(opened.path.as_ref(), Some(&old));
    let mut other = Session::new();
    other.execute("insert", &json!({"path": old})).unwrap();
    let walls = |s: &Session| s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Wall(_))).count();
    assert_eq!(walls(&other), 1, "Insert from File reads the old extension");
    other.execute("link", &json!({"path": old})).unwrap();
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
    assert_eq!(std::fs::metadata(&old).unwrap().modified().unwrap(), modified);
    assert_eq!(names(), before, "nothing was written beside it");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recovery_finds_the_backup_of_either_name_beside_a_project_or_among_the_untitled() {
    let dir = dir();
    let write = |path: &Path| Session::new().execute("autosave", &json!({"path": path})).unwrap();
    std::fs::create_dir_all(dir.join("temp")).unwrap();
    let mut worker = Worker::start(dir.join("temp"), || {}).unwrap();
    let project = dir.join("house.bimcraft");
    assert!(found(&mut worker, Some(project.clone())).is_empty());
    write(&old_backup_path(&project));
    let keep = |project: &Path, path: PathBuf| Backup { path, project: Some(project.to_path_buf()) };
    assert_eq!(found(&mut worker, Some(project.clone())), [keep(&project, old_backup_path(&project))], "a backup an earlier version made");
    let project = dir.join("barn.bimc");
    write(&backup_path(&project));
    assert_eq!(found(&mut worker, Some(project.clone())), [keep(&project, backup_path(&project))]);
    // Both beside one project: the new one first, then the old.
    write(&old_backup_path(&project));
    assert_eq!(
        found(&mut worker, Some(project.clone())),
        [keep(&project, backup_path(&project)), keep(&project, old_backup_path(&project))]
    );
    // Untitled work: in the temporary folder, under either name.
    let temp = dir.join("temp");
    let (new, old) = (backup_path(&temp.join("untitled-a")), old_backup_path(&temp.join("untitled-b")));
    assert_eq!(new.file_name().unwrap(), "untitled-a.bimb");
    write(&new);
    write(&old);
    std::fs::write(temp.join("untitled-c.bimb.lock.txt"), b"").unwrap();
    let paths: Vec<PathBuf> = found(&mut worker, None).into_iter().map(|b| b.path).collect();
    assert_eq!(paths, [new, old]);
    std::fs::remove_dir_all(dir).unwrap();
}
