use crate::{engine::Session, ids::Guid, library::Kind, ods};
use serde_json::json;

fn project(width: f64) -> Session {
    let mut s = Session::new();
    s.execute("type", &json!({"category":"wire","name":"Transfer cable","width":width,"depth":width,"round":true})).unwrap();
    s
}

fn guid(s: &Session, name: &str) -> Guid {
    s.model.library.guid(&Kind::Type("wire".into()), name).unwrap()
}

fn rows(s: &Session) -> Vec<Vec<String>> {
    let all = ods::catalogue_rows(&s.model);
    vec![all[0].clone(), all.iter().find(|r| r.get(2).map(String::as_str) == Some("Transfer cable")).unwrap().clone()]
}

#[test]
fn foreign_conflict_requires_choice_and_update_preserves_local_identity() {
    let donor = project(9.0);
    let mut recipient = project(6.0);
    let old_guid = guid(&recipient, "Transfer cable");
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("donor.ods");
    let bytes = ods::write(&rows(&donor)).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let before = recipient.model.clone();
    assert!(recipient.execute("catalogue_import", &json!({"path":path})).is_err());
    assert_eq!(recipient.model, before);
    recipient.execute("catalogue_import", &json!({"path":path,"conflicts":"update"})).unwrap();
    assert_eq!(recipient.model.types.runs["wire"]["Transfer cable"].width, 9.0);
    assert_eq!(guid(&recipient, "Transfer cable"), old_guid);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn separate_copy_keeps_original_and_redo_restores_copy_identity() {
    let mut s = project(6.0);
    let original = guid(&s, "Transfer cable");
    let mut rows = rows(&s);
    rows[1][3] = "12".into();
    rows[1][4] = "12".into();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("copy.ods");
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path,"conflicts":"copy"})).unwrap();
    let copy = guid(&s, "Transfer cable (2)");
    assert_ne!(copy, original);
    assert_eq!(s.model.types.runs["wire"]["Transfer cable"].width, 6.0);
    assert_eq!(s.model.types.runs["wire"]["Transfer cable (2)"].width, 12.0);
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.model.types.runs["wire"].contains_key("Transfer cable (2)"));
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(guid(&s, "Transfer cable (2)"), copy);
}

#[test]
fn foreign_unique_and_equivalent_products_receive_correct_local_identity() {
    let donor = project(6.0);
    let mut s = Session::new();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("new.ods");
    std::fs::write(&path, ods::write(&rows(&donor)).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    let local = guid(&s, "Transfer cable");
    assert_ne!(local, guid(&donor, "Transfer cable"));
    assert!(ods::pending_products(&s.model).iter().any(|(_, e)| e.guid == local));
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    assert_eq!(guid(&s, "Transfer cable"), local);
    assert_eq!(s.model.types.runs["wire"].keys().filter(|n| n.starts_with("Transfer cable")).count(), 1);
}

#[test]
fn bound_catalogue_accepts_copied_unique_products_and_rewrites_their_guid() {
    for closed in [false, true] {
        let donor = project(6.0);
        let mut s = Session::new();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("linked.ods");
        let project = temp.path().join("project.bimcraft");
        s.execute("catalogue_link", &json!({"path":path})).unwrap();
        s.execute("save", &json!({"path":project})).unwrap();
        let mut exported = ods::read(&std::fs::read(&path).unwrap()).unwrap();
        exported.push(rows(&donor)[1].clone());
        let external = ods::write(&exported).unwrap();
        std::fs::write(&path, &external).unwrap();
        let reply = if closed { s.execute("open", &json!({"path":project})) } else { s.execute("catalogue_sync", &json!({})) }.unwrap();
        assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
        let local = guid(&s, "Transfer cable");
        assert_ne!(local, guid(&donor, "Transfer cable"));
        let rows = ods::read(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rows.iter().find(|r| r[2] == "Transfer cable").unwrap()[0], local.to_string());
    }
}
