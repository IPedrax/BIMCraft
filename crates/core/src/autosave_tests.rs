//! Recovery must never replace the user's saved project or advance their undo history.
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::autosave::{Notice, Worker, backup_path};
use crate::engine::Session;

fn dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-recovery-test-{}", crate::ids::Guid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn binary_catalogue_write_replaces_the_complete_file_and_preserves_it_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalogue.ods");
    std::fs::write(&path, b"previous catalogue").unwrap();
    let data = b"PK\x03\x04\x00\xff\x80binary catalogue";
    crate::autosave::atomic_write_bytes(&path, data).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), data);

    let occupied = dir.path().join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("keep"), b"keep").unwrap();
    assert!(crate::autosave::atomic_write_bytes(&occupied, data).is_err());
    assert_eq!(std::fs::read(occupied.join("keep")).unwrap(), b"keep");
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2, "failed writes leave no staging file");
}

fn notice(worker: &mut Worker) -> Notice {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(note) = worker.poll() {
            return note;
        }
        assert!(std::time::Instant::now() < deadline, "worker did not reply");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn background_session_snapshot_keeps_an_unexported_product_and_its_original_identity() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let backup = temp.path().join("recovery.bimcraft");
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("equipment", &json!({"name":"Unexported software"})).unwrap();
    let original = Arc::clone(&s.model);
    let mut worker = Worker::start(temp.path().join("temp"), || {}).unwrap();
    worker.save_session(backup.clone(), &s).unwrap();
    s.execute("equipment", &json!({"name":"Later edit"})).unwrap();
    match notice(&mut worker) {
        Notice::Saved { snapshot, result, .. } => {
            result.unwrap();
            assert!(snapshot.ptr_eq(&Arc::downgrade(&original)));
        }
        other => panic!("{other:?}"),
    }
    let mut recovered = Session::new();
    let reply = recovered.execute("open", &json!({"path":backup})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    let rows = crate::ods::catalogue_rows(&recovered.model);
    assert!(rows.iter().any(|row| row[2].ends_with(": Unexported software")));
    assert!(!rows.iter().any(|row| row[2].ends_with(": Later edit")));
    assert!(!recovered.model.library.entries().any(|(_, entry)| entry.placeholder));
    recovered.model.check().unwrap();
}

#[test]
fn a_model_only_background_snapshot_does_not_invent_unexported_products() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let backup = temp.path().join("recovery.bimcraft");
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("equipment", &json!({"name":"Committed software","product":{"article":"ORIGINAL"}})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let mut worker = Worker::start(temp.path().join("temp"), || {}).unwrap();
    worker.save(backup.clone(), Arc::clone(&s.model)).unwrap();
    match notice(&mut worker) {
        Notice::Saved { result, .. } => result.unwrap(),
        other => panic!("{other:?}"),
    }
    let mut rows = crate::ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    rows.iter_mut().find(|row| row[2].ends_with(": Committed software")).unwrap()[9] = "REPLACEMENT".into();
    std::fs::write(&catalogue, crate::ods::write(&rows).unwrap()).unwrap();
    let mut recovered = Session::new();
    let reply = recovered.execute("open", &json!({"path":backup})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(
        crate::ods::catalogue_rows(&recovered.model).iter().find(|row| row[2].ends_with(": Committed software")).unwrap()[9],
        "REPLACEMENT"
    );
}

#[test]
fn background_session_snapshot_preserves_catalogue_cancellations_and_original_identity() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let backup = temp.path().join("recovery.bimcraft");
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("equipment", &json!({"name":"Cancelled software"})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    let original = Arc::clone(&s.model);
    let mut worker = Worker::start(temp.path().join("temp"), || {}).unwrap();
    worker.save_session(backup.clone(), &s).unwrap();
    s.execute("equipment", &json!({"name":"Later edit"})).unwrap();
    match notice(&mut worker) {
        Notice::Saved { snapshot, result, .. } => {
            result.unwrap();
            assert!(snapshot.ptr_eq(&Arc::downgrade(&original)), "completion must identify the editing snapshot, not its serialized copy");
        }
        other => panic!("{other:?}"),
    }
    let mut recovered = Session::new();
    let reply = recovered.execute("open", &json!({"path":backup})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(recovered.model.elements.len(), original.elements.len());
    assert!(!crate::ods::catalogue_rows(&recovered.model).iter().any(|row| row[2].ends_with(": Later edit")));
    let rows = crate::ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    assert!(!rows.iter().any(|row| row[2].ends_with(": Cancelled software")));
}

#[test]
fn autosave_and_recover_are_commands_and_preserve_the_project() {
    let dir = dir();
    let project = dir.join("house.bimcraft");
    let backup = backup_path(&project);
    assert_eq!(backup.parent(), project.parent());
    let mut s = Session::new();
    s.execute("save", &json!({"path": project})).unwrap();
    let original = std::fs::read(&project).unwrap();
    s.execute("wall", &json!({"points": [[0,0],[4000,0]]})).unwrap();
    let snapshot = Arc::clone(&s.model);
    let history = s.history();
    s.execute("autosave", &json!({"path": backup})).unwrap();
    assert!(Arc::ptr_eq(&snapshot, &s.model));
    assert_eq!(s.history(), history);
    assert_eq!(s.path.as_ref(), Some(&project));
    assert_eq!(std::fs::read(&project).unwrap(), original);
    let mut recovered = Session::new();
    recovered.execute("recover", &json!({"path": backup, "project": project})).unwrap();
    assert_eq!(recovered.path.as_ref(), Some(&project));
    assert_eq!(recovered.model.to_json().unwrap(), snapshot.to_json().unwrap());
    recovered.execute("recover", &json!({"path": backup})).unwrap();
    assert!(recovered.path.is_none(), "an untitled recovery must ask for Save As");
    std::fs::write(&backup, b"truncated").unwrap();
    let before = Arc::clone(&recovered.model);
    assert!(recovered.execute("recover", &json!({"path": backup, "project": project})).is_err());
    assert!(Arc::ptr_eq(&before, &recovered.model));
    assert!(recovered.path.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn worker_keeps_a_consistent_snapshot_and_serializes_cleanup_after_writes() {
    let dir = dir();
    let backup = backup_path(&dir.join("house.bimcraft"));
    let mut worker = Worker::start(dir.join("temp"), || {}).unwrap();
    let mut s = Session::new();
    let snapshot = Arc::clone(&s.model);
    assert!(worker.save(backup.clone(), Arc::clone(&snapshot)).unwrap());
    assert!(!worker.save(backup.clone(), Arc::clone(&snapshot)).unwrap(), "never queue more than one snapshot");
    s.execute("wall", &json!({"points": [[0,0],[4000,0]]})).unwrap();
    match notice(&mut worker) {
        Notice::Saved { result, .. } => result.unwrap(),
        other => panic!("{other:?}"),
    }
    let mut loaded = Session::new();
    loaded.execute("open", &json!({"path": backup})).unwrap();
    assert_eq!(loaded.model.to_json().unwrap(), snapshot.to_json().unwrap());
    assert!(worker.save(backup.clone(), Arc::clone(&s.model)).unwrap());
    worker.remove(backup.clone()).unwrap();
    assert!(matches!(notice(&mut worker), Notice::Saved { result: Ok(()), .. }));
    assert!(matches!(notice(&mut worker), Notice::Removed { result: Ok(()), .. }));
    assert!(!backup.exists(), "a late writer must not resurrect a backup after Save/Discard");
    drop(worker);
}

#[test]
fn startup_finds_orphaned_untitled_backups_but_skips_live_sessions() {
    let dir = dir();
    let backup = backup_path(&dir.join("untitled"));
    let mut active = Worker::start(dir.clone(), || {}).unwrap();
    active.save(backup.clone(), Session::new().model).unwrap();
    assert!(matches!(notice(&mut active), Notice::Saved { result: Ok(()), .. }));
    let mut other = Worker::start(dir.clone(), || {}).unwrap();
    other.discover(None, 1).unwrap();
    match notice(&mut other) {
        Notice::Discovered { result, .. } => assert!(result.unwrap().is_empty()),
        other => panic!("{other:?}"),
    }
    // Drop is nonblocking. The worker drains its queue, then releases its file locks.
    drop(active);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        other.discover(None, 2).unwrap();
        if let Notice::Discovered { result, .. } = notice(&mut other)
            && !result.unwrap().is_empty()
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    other.remove(backup.clone()).unwrap();
    assert!(matches!(notice(&mut other), Notice::Removed { result: Ok(()), .. }));
    assert!(!backup.exists());
}

#[test]
fn a_killed_process_leaves_a_recoverable_snapshot_and_releases_its_lock() {
    const CHILD: &str = "BIMCRAFT_AUTOSAVE_CRASH_TEST";
    if let Some(path) = std::env::var_os(CHILD) {
        let dir = std::path::PathBuf::from(path);
        let backup = backup_path(&dir.join("untitled"));
        let mut worker = Worker::start(dir.clone(), || {}).unwrap();
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0,0],[4000,0]]})).unwrap();
        worker.save(backup, s.model).unwrap();
        assert!(matches!(notice(&mut worker), Notice::Saved { result: Ok(()), .. }));
        std::fs::write(dir.join("ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    }
    let dir = dir();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "autosave_tests::a_killed_process_leaves_a_recoverable_snapshot_and_releases_its_lock", "--nocapture"])
        .env(CHILD, &dir)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !dir.join("ready").exists() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child failed to autosave before being killed");
        }
        assert!(child.try_wait().unwrap().is_none(), "child exited before saving");
        std::thread::sleep(Duration::from_millis(1));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let mut worker = Worker::start(dir.clone(), || {}).unwrap();
    worker.discover(None, 1).unwrap();
    let Notice::Discovered { result, .. } = notice(&mut worker) else { panic!("expected recovery discovery") };
    let found = result.unwrap();
    assert_eq!(found.len(), 1);
    let mut recovered = Session::new();
    recovered.execute("recover", &json!({"path": found[0].path})).unwrap();
    assert_eq!(recovered.model.elements.values().filter(|e| e.category() == "wall").count(), 1);
    assert!(recovered.path.is_none());
}

#[test]
fn queuing_a_snapshot_and_cleanup_never_waits_for_a_blocked_worker() {
    let dir = dir();
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let mut first = true;
    // The callback is deliberately blocked after discovery. Scheduling a save must still return
    // so editing can proceed, without waiting for serialization, I/O, or the worker's callback.
    let notify = std::sync::Mutex::new(move || {
        if first {
            first = false;
            entered.send(()).unwrap();
            gate.recv().unwrap();
        }
    });
    let mut worker = Worker::start(dir.clone(), move || (notify.lock().unwrap())()).unwrap();
    worker.discover(None, 0).unwrap();
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(notice(&mut worker), Notice::Discovered { .. }));
    let snapshot = Session::new().model;
    let backup = backup_path(&dir.join("untitled"));
    // Use a separate thread with a timeout so a regression cannot hang the test suite.
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        assert!(worker.save(backup.clone(), snapshot).unwrap());
        worker.remove(backup).unwrap();
        done.send(worker).unwrap_or_else(|_| panic!("receiver dropped"));
    });
    let returned = result.recv_timeout(Duration::from_secs(10));
    release.send(()).unwrap();
    let mut worker = returned.expect("scheduling blocked the caller");
    assert!(matches!(notice(&mut worker), Notice::Saved { result: Ok(()), .. }));
    assert!(matches!(notice(&mut worker), Notice::Removed { result: Ok(()), .. }));
}

#[test]
fn pending_local_change_to_exported_type_survives_autosave() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = Session::new();
    session.execute("type", &json!({"category":"wire","name":"Recovery cable","width":6,"round":true})).unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    session.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    session.execute("type", &json!({"category":"wire","name":"Recovery cable","width":12})).unwrap();
    let original = Arc::clone(&session.model);
    let ods_bytes = std::fs::read(&catalogue).unwrap();
    let mut worker = Worker::start(temp.path().join("worker"), || {}).unwrap();
    let backup = temp.path().join("recovery.bimcraft");
    assert!(worker.save_session(backup.clone(), &session).unwrap());
    session.execute("type", &json!({"category":"wire","name":"Recovery cable","width":18})).unwrap();
    match notice(&mut worker) {
        Notice::Saved { result, snapshot, .. } => {
            result.unwrap();
            assert!(snapshot.ptr_eq(&Arc::downgrade(&original)));
        }
        other => panic!("expected Saved: {other:?}"),
    }
    assert_eq!(std::fs::read(&catalogue).unwrap(), ods_bytes);
    let mut recovered = Session::new();
    let reply = recovered.execute("open", &json!({"path":backup})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(recovered.model.types.runs["wire"]["Recovery cable"].width, 12.0);
    assert_eq!(session.model.types.runs["wire"]["Recovery cable"].width, 18.0);
    recovered.model.check().unwrap();
}
