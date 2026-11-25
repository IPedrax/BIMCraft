use serde_json::json;

use crate::engine::Session;
use crate::library::Kind;

#[test]
fn ods_catalogue_round_trips_upstream_carrier_types() {
    let mut s = Session::new();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    for category in ["conduit", "cabletray"] {
        s.execute("type", &json!({"category":category,"name":"Custom","width":30,"depth":30})).unwrap();
    }
    let entries: Vec<_> = ["conduit", "cabletray"]
        .map(|category| Kind::Type(category.into()))
        .into_iter()
        .map(|kind| (s.model.library.guid(&kind, "Custom").unwrap(), kind))
        .collect();
    s.execute("catalogue_export", &json!({"path":path})).unwrap();

    s.execute("type", &json!({"category":"conduit","name":"Custom","width":50,"depth":50})).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    assert_eq!(s.model.types.runs["conduit"]["Custom"].width, 30.0);
    assert_eq!(s.model.types.runs["cabletray"]["Custom"].width, 30.0);
    for (guid, kind) in entries {
        assert_eq!(s.model.library.guid(&kind, "Custom"), Some(guid));
    }
}

#[test]
fn ods_catalogue_respects_file_restrictions() {
    let mut s = Session::new();
    let path = tempfile::tempdir().unwrap();
    let file = path.path().join("catalogue.ods");
    s.execute("catalogue_export", &json!({"path":file})).unwrap();
    s.read_only = Some("read-only test");
    let error = s.execute("catalogue_import", &json!({"path":file})).unwrap_err();
    assert!(error.contains("read-only"), "{error}");
    let new_file = path.path().join("new.ods");
    let params = json!({"path":new_file});
    let error = s.execute("catalogue_export", &params).unwrap_err();
    assert!(error.contains("read-only"), "{error}");
    let error = s.script_dry_run(&format!("catalogue_export {params}")).unwrap_err();
    assert!(error.contains("dry run"), "{error}");
    assert!(!new_file.exists());
}

#[test]
fn ods_catalogue_invalid_guid_rolls_back_the_entire_import() {
    let mut s = Session::new();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    s.execute("type", &json!({"category":"conduit","name":"Custom","width":30,"depth":30})).unwrap();
    let before = s.model.clone();
    let rows = [
        ["GUID", "Category", "Name", "Width_mm", "Depth_mm", "Round", "Material"],
        ["", "cabletray", "New tray", "50", "30", "false", ""],
        ["not-a-guid", "conduit", "Custom", "90", "90", "false", ""],
    ];
    let rows: Vec<Vec<String>> = rows.into_iter().map(|row| row.into_iter().map(String::from).collect()).collect();
    std::fs::write(&path, crate::ods::write(&rows).unwrap()).unwrap();
    assert!(s.execute("catalogue_import", &json!({"path":path})).is_err());
    assert_eq!(s.model, before);
}
