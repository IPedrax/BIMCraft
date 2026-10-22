use super::*;
use bimcraft_core::autosave::backup_path;
use std::time::{Duration, Instant};

fn enable(h: &mut Harness<BimApp>, root: &std::path::Path) {
    let ctx = h.ctx.clone();
    h.state_mut().enable_autosave(root.to_path_buf(), &ctx);
}

fn until(h: &mut Harness<BimApp>, ready: impl Fn(&Harness<BimApp>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready(h) {
        assert!(Instant::now() < deadline, "autosave did not finish: {}", h.state().status.text);
        h.step();
        std::thread::sleep(Duration::from_millis(1));
    }
    h.run();
}

fn due(h: &mut Harness<BimApp>) {
    h.state_mut().autosave.next_due = Instant::now();
    h.step();
}

fn fixture(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, Vec<u8>) {
    let project = dir.join("house.bimcraft");
    let backup = backup_path(&project);
    let mut s = Session::new();
    s.execute("save", &json!({"path": project})).unwrap();
    let original = std::fs::read(&project).unwrap();
    s.execute("wall", &json!({"points": [[0,0],[4000,0]]})).unwrap();
    s.execute("autosave", &json!({"path": backup})).unwrap();
    (project, backup, original)
}

#[test]
fn background_autosave_keeps_the_editing_session_dirty_and_moves_out_of_temp_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = app("");
    enable(&mut h, &dir.path().join("temp"));
    until(&mut h, |h| h.state().autosave.discovering == 0);
    h.state_mut().session.execute("wall", &json!({"points": [[0,0],[4000,0]]})).unwrap();
    let model = Arc::clone(&h.state().session.model);
    let history = h.state().session.history();
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy);
    let temp = h.state().autosave.active.clone().unwrap();
    assert!(temp.starts_with(dir.path().join("temp")) && temp.exists());
    assert!(h.state().dirty());
    assert!(Arc::ptr_eq(&model, &h.state().session.model));
    assert_eq!(history, h.state().session.history());
    let project = dir.path().join("house.bimcraft");
    assert!(h.state_mut().save_to(&project));
    until(&mut h, |_| !temp.exists());
    assert!(!h.state().dirty());
    until(&mut h, |h| h.state().autosave.discovering == 0);
    h.state_mut().session.execute("wall", &json!({"points": [[0,1000],[4000,1000]]})).unwrap();
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy);
    assert!(backup_path(&project).exists());
}

#[test]
fn save_as_preserves_and_offers_a_backup_already_beside_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    let (project, backup, _) = fixture(dir.path());
    let bytes = std::fs::read(&backup).unwrap();
    let mut h = app("");
    enable(&mut h, &dir.path().join("temp"));
    until(&mut h, |h| h.state().autosave.discovering == 0);
    assert!(h.state_mut().save_to(&project));
    until(&mut h, |h| h.query_by_label("Recover").is_some());
    assert_eq!(std::fs::read(backup).unwrap(), bytes);
}

#[test]
fn damaged_original_still_offers_its_healthy_backup() {
    let dir = tempfile::tempdir().unwrap();
    let (project, _, _) = fixture(dir.path());
    std::fs::write(&project, b"damaged original").unwrap();
    let mut h = app("");
    enable(&mut h, &dir.path().join("temp"));
    assert!(!h.state_mut().open_path(&project));
    until(&mut h, |h| h.query_by_label("Recover").is_some());
    press(&mut h, "Recover");
    assert_eq!(walls(&h).len(), 1);
    assert!(h.state().dirty());
    assert_eq!(h.state().session.path.as_ref(), Some(&project));
}

#[test]
fn opening_offers_recover_discard_and_postpone_and_protects_postponed_data() {
    let dir = tempfile::tempdir().unwrap();
    let (project, backup, original) = fixture(dir.path());
    let recovery_bytes = std::fs::read(&backup).unwrap();
    let mut h = app("");
    enable(&mut h, &dir.path().join("temp"));
    assert!(h.state_mut().open_path(&project));
    until(&mut h, |h| h.query_by_label("Postpone").is_some());
    for label in ["Recover", "Discard", "Postpone"] {
        assert!(h.query_by_label(label).is_some());
    }
    press(&mut h, "Postpone");
    assert!(walls(&h).is_empty() && !h.state().dirty());
    h.state_mut().session.execute("wall", &json!({"points": [[0,1000],[4000,1000]]})).unwrap();
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy);
    assert_eq!(std::fs::read(&backup).unwrap(), recovery_bytes, "postponed recovery must not be overwritten");
    assert_eq!(std::fs::read(&project).unwrap(), original);
    assert!(h.state_mut().save_to(&project));
    h.run();
    assert_eq!(std::fs::read(&backup).unwrap(), recovery_bytes, "Save also preserves an unresolved recovery");
    press(&mut h, "File");
    press(&mut h, "Recover Autosave…");
    press(&mut h, "Recover");
    assert_eq!(walls(&h).len(), 1);
    assert_eq!(walls(&h)[0].0, P2(0.0, 0.0));
    assert!(h.state().dirty());
    assert_eq!(h.state().session.path.as_ref(), Some(&project));
    assert!(h.state_mut().save_to(&project));
    until(&mut h, |_| !backup.exists());
}

#[test]
fn discard_removes_only_the_backup_and_a_corrupt_recovery_stays_available() {
    let dir = tempfile::tempdir().unwrap();
    let (project, backup, original) = fixture(dir.path());
    std::fs::write(&backup, b"broken snapshot").unwrap();
    let mut h = app("");
    enable(&mut h, &dir.path().join("temp"));
    h.state_mut().open_path(&project);
    until(&mut h, |h| h.query_by_label("Recover").is_some());
    press(&mut h, "Recover");
    assert!(h.state().status.error);
    assert!(h.query_by_label("Postpone").is_some());
    assert!(walls(&h).is_empty());
    press(&mut h, "Discard");
    until(&mut h, |_| !backup.exists());
    assert_eq!(std::fs::read(project).unwrap(), original);
}

#[test]
fn startup_recovers_an_untitled_project_without_adopting_the_temporary_file_as_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let backup = backup_path(&dir.path().join("untitled"));
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0,0],[4000,0]]})).unwrap();
    s.execute("autosave", &json!({"path": backup})).unwrap();
    let mut h = app("");
    enable(&mut h, dir.path());
    until(&mut h, |h| h.query_by_label("Recover").is_some());
    press(&mut h, "Recover");
    assert_eq!(walls(&h).len(), 1);
    assert!(h.state().dirty() && h.state().session.path.is_none());
    let project = dir.path().join("recovered.bimcraft");
    assert!(h.state_mut().save_to(&project));
    until(&mut h, |_| !backup.exists());
}

#[test]
fn autosave_settings_are_editable_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    press(&mut h, "Settings");
    press(&mut h, "Files");
    assert_eq!(h.state().autosave.interval_minutes, 2);
    pick(&mut h, Role::CheckBox, "Autosave in the background");
    assert_eq!(h.state().autosave.interval_minutes, 0);
    press(&mut h, "Close");
    let mut again = app("");
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().autosave.interval_minutes, 0);
    // Invalid input preserves the default, rather than wrapping or disabling recovery.
    std::fs::write(&file, br#"{"autosave_minutes":18446744073709551615}"#).unwrap();
    let mut again = app("");
    again.state_mut().use_settings(file.clone());
    assert_eq!(again.state().autosave.interval_minutes, 2);
    std::fs::write(&file, br#"{"autosave_minutes":5}"#).unwrap();
    again.state_mut().use_settings(file);
    assert_eq!(again.state().autosave.interval_minutes, 5);
}

#[test]
fn resolving_a_postponed_backup_retires_the_temporary_fallback_after_the_next_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (project, backup, _) = fixture(dir.path());
    let mut h = app("");
    enable(&mut h, &dir.path().join("temp"));
    h.state_mut().open_path(&project);
    until(&mut h, |h| h.query_by_label("Postpone").is_some());
    press(&mut h, "Postpone");
    h.state_mut().session.execute("wall", &json!({"points": [[0,1000],[4000,1000]]})).unwrap();
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy);
    let fallback = h.state().autosave.active.clone().unwrap();
    assert!(fallback.exists() && fallback != backup);
    press(&mut h, "File");
    press(&mut h, "Recover Autosave…");
    press(&mut h, "Discard");
    until(&mut h, |h| h.state().autosave.candidates.is_empty());
    assert!(fallback.exists(), "keep the last durable copy until a replacement is saved");
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy && !fallback.exists());
    assert!(backup.exists() && h.state().dirty());
}

#[test]
fn failed_background_write_reports_an_error_and_can_retry_without_interrupting_editing() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = app("");
    enable(&mut h, &dir.path().join("temp"));
    until(&mut h, |h| h.state().autosave.discovering == 0);
    h.state_mut().session.execute("wall", &json!({"points": [[0,0],[4000,0]]})).unwrap();
    let backup = h.state().autosave.active.clone().unwrap();
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy);
    let original = std::fs::read(&backup).unwrap();
    h.state_mut().session.execute("wall", &json!({"points": [[0,1000],[4000,1000]]})).unwrap();
    h.state_mut().autosave.active = Some(dir.path().join("missing-folder").join("snapshot.bimcraft"));
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy);
    assert!(h.state().status.error && h.state().dirty());
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    assert!(h.query_by_label("Recovery copy found").is_none(), "background failure must not open a modal");
    assert_eq!(walls(&h).len(), 2);
    h.state_mut().autosave.active = Some(backup.clone());
    due(&mut h);
    until(&mut h, |h| !h.state().autosave.busy);
    assert_ne!(std::fs::read(backup).unwrap(), original);
}
