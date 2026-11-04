use serde_json::json;

use crate::{engine::Session, ods};

#[test]
fn relinking_the_current_catalogue_keeps_open_model_changes_authoritative() {
    let (_temp, mut session, catalogue, _) = project();
    session.execute("type", &json!({"category":"wire","name":"Test cable","width":12})).unwrap();
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|row| row[2] == "Test cable").unwrap();
    row[3] = "18".into();
    row[4] = "18".into();
    let external = ods::write(&rows).unwrap();
    std::fs::write(&catalogue, &external).unwrap();
    let history = session.history();
    let reply = session.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    assert_eq!(session.model.types.runs["wire"]["Test cable"].width, 12.0);
    assert_eq!(session.history(), history);
    assert_eq!(std::fs::read(reply["backup"].as_str().unwrap()).unwrap(), external);
    let rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    assert_eq!(rows.iter().find(|row| row[2] == "Test cable").unwrap()[3], "12");
}

#[test]
fn saving_unsynchronized_changes_to_an_exported_product_preserves_them_on_open() {
    for command in ["save", "autosave"] {
        let (_temp, mut session, catalogue, project) = project();
        let destination = if command == "autosave" { project.with_file_name("recovery.bimcraft") } else { project };
        session.execute("wire", &json!({"type":"Test cable","points":[[0,0],[1000,0]]})).unwrap();
        let elements = session.model.elements.clone();
        let original = std::fs::read(&catalogue).unwrap();
        session.execute("type", &json!({"category":"wire","name":"Test cable","width":12})).unwrap();
        let before = session.model.clone();
        session.execute(command, &json!({"path":destination})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&session.model, &before));
        assert_eq!(std::fs::read(&catalogue).unwrap(), original, "saving does not write the catalogue");
        let mut reopened = Session::new();
        let reply = reopened.execute("open", &json!({"path":destination})).unwrap();
        assert!(reply.get("catalogue_not_loaded").is_none(), "{command}: {reply}");
        assert_eq!(reopened.model.types.runs["wire"]["Test cable"].width, 12.0, "{command}");
        assert_eq!(reopened.model.elements, elements);
        let rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
        assert_eq!(rows.iter().find(|r| r[2] == "Test cable").unwrap()[3], "12");
        assert_eq!(std::fs::read(reply["catalogue"]["backup"].as_str().unwrap()).unwrap(), original);
        reopened.model.check().unwrap();
    }
}

#[test]
fn simultaneous_product_edits_or_external_deletion_preserve_both_versions_until_resolved() {
    for delete in [false, true] {
        let (_temp, mut session, catalogue, project) = project();
        session.execute("type", &json!({"category":"wire","name":"Test cable","width":12})).unwrap();
        session.execute("save", &json!({"path":project})).unwrap();
        let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
        if delete {
            rows.retain(|row| row[2] != "Test cable");
        } else {
            rows.iter_mut().find(|row| row[2] == "Test cable").unwrap()[3] = "18".into();
        }
        let external = ods::write(&rows).unwrap();
        std::fs::write(&catalogue, &external).unwrap();
        let mut reopened = Session::new();
        let reply = reopened.execute("open", &json!({"path":project})).unwrap();
        assert!(reply["catalogue_not_loaded"].as_str().is_some_and(|s| s.contains("conflict")), "{reply}");
        assert_eq!(reopened.model.types.runs["wire"]["Test cable"].width, 12.0);
        assert_eq!(std::fs::read(&catalogue).unwrap(), external);
        // Even saving the unresolved model must retain the original comparison basis.
        reopened.execute("save", &json!({"path":project})).unwrap();
        let reply = reopened.execute("open", &json!({"path":project})).unwrap();
        assert!(reply.get("catalogue_not_loaded").is_some());
        let resolved = reopened.execute("catalogue_sync", &json!({})).unwrap();
        assert_eq!(std::fs::read(resolved["backup"].as_str().unwrap()).unwrap(), external);
        reopened.execute("save", &json!({"path":project})).unwrap();
        assert!(reopened.execute("open", &json!({"path":project})).unwrap().get("catalogue_not_loaded").is_none());
        assert_eq!(reopened.model.types.runs["wire"]["Test cable"].width, 12.0);
    }
}

#[test]
fn a_deleted_catalogue_type_keeps_geometry_and_identity_but_leaves_schedules() {
    let (_temp, mut session, catalogue, project) = project();
    session.execute("wire", &json!({"type":"Test cable","points":[[0,0],[1000,0]]})).unwrap();
    session.execute("save", &json!({"path":project})).unwrap();
    let elements = session.model.elements.clone();
    let guid = session.model.library.guid(&crate::library::Kind::Type("wire".into()), "Test cable").unwrap();
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let original = rows.iter().find(|r| r[2] == "Test cable").unwrap().clone();
    rows.retain(|r| r[2] != "Test cable");
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let reply = session.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(reply["catalogue"]["placeholders"], json!([guid]));
    assert_eq!(session.model.elements, elements);
    assert!(!ods::catalogue_rows(&session.model).iter().any(|r| r[2] == "Test cable"));
    let schedule = session.execute("schedule", &json!({"name":"Cables","category":"wire"})).unwrap();
    assert_eq!(schedule["rows"], json!([]));
    let elements = session.model.elements.clone();
    session.execute("save", &json!({"path":project})).unwrap();
    session.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(session.model.elements, elements);
    assert!(!ods::catalogue_rows(&session.model).iter().any(|r| r[2] == "Test cable"));
    let mut replacement = original.clone();
    replacement[3] = "99".into();
    replacement[4] = "99".into();
    rows.push(replacement);
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    session.execute("catalogue_sync", &json!({})).unwrap();
    assert_eq!(session.model.types.runs["wire"]["Test cable"].width, 6.0);
    assert!(!ods::catalogue_rows(&session.model).iter().any(|r| r[2] == "Test cable"));
    session.execute("catalogue_link", &json!({"path":null})).unwrap();
    let detached = project.with_file_name("detached.bimcraft");
    session.execute("save", &json!({"path":detached})).unwrap();
    let reply = session.execute("open", &json!({"path":detached})).unwrap();
    assert_eq!(reply["catalogue_placeholders"], json!([guid]));
    rows.pop();
    rows.push(original);
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    session.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(session.model.library.guid(&crate::library::Kind::Type("wire".into()), "Test cable"), Some(guid));
    assert!(ods::catalogue_rows(&session.model).iter().any(|r| r[2] == "Test cable"));
    let schedule = session.execute("schedule", &json!({"name":"Cables"})).unwrap();
    assert_eq!(schedule["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn undoing_an_exported_type_edit_is_preserved_even_before_the_next_catalogue_poll() {
    let (_temp, mut session, catalogue, project) = project();
    session.execute("type", &json!({"category":"wire","name":"Test cable","width":12})).unwrap();
    session.execute("catalogue_sync", &json!({})).unwrap();
    session.execute("undo", &json!({})).unwrap();
    assert_eq!(session.model.types.runs["wire"]["Test cable"].width, 6.0);
    let external = std::fs::read(&catalogue).unwrap();
    session.execute("save", &json!({"path":project})).unwrap();
    let mut reopened = Session::new();
    let reply = reopened.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(reopened.model.types.runs["wire"]["Test cable"].width, 6.0);
    assert_eq!(std::fs::read(reply["catalogue"]["backup"].as_str().unwrap()).unwrap(), external);
}

#[test]
fn pending_existing_product_edits_survive_read_failure_then_allow_later_external_replacement() {
    let (_temp, mut session, catalogue, project) = project();
    session.execute("type", &json!({"category":"wire","name":"Test cable","width":12})).unwrap();
    session.execute("save", &json!({"path":project})).unwrap();
    let original = std::fs::read(&catalogue).unwrap();
    std::fs::write(&catalogue, b"broken workbook").unwrap();
    let mut recovered = Session::new();
    assert!(recovered.execute("open", &json!({"path":project})).unwrap().get("catalogue_not_loaded").is_some());
    recovered.execute("save", &json!({"path":project})).unwrap();
    std::fs::write(&catalogue, &original).unwrap();
    let reply = recovered.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(recovered.model.types.runs["wire"]["Test cable"].width, 12.0);
    recovered.execute("save", &json!({"path":project})).unwrap();
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|r| r[2] == "Test cable").unwrap();
    row[3] = "18".into();
    row[4] = "18".into();
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let reply = recovered.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(recovered.model.types.runs["wire"]["Test cable"].width, 18.0);
}

#[test]
fn saved_pending_product_rows_are_bounded_and_tied_to_their_guid() {
    let (_temp, session, _, _) = project();
    let original = ods::catalogue_rows(&session.model).into_iter().find(|r| r[2] == "Test cable").unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&session.model.to_json().unwrap()).unwrap();
    let mut different_guid = original.clone();
    different_guid[0] = crate::ids::Guid::new().to_string();
    let mut too_large = original.clone();
    too_large[7] = "x".repeat(ods::MAX_CELL + 1);
    for invalid in [vec![], vec![String::new(); 14], different_guid, too_large] {
        file["catalogue_pending_edits"] = json!({original[0].clone(): invalid});
        let (model, left_out) = crate::model::Model::read(&file.to_string()).unwrap();
        assert!(left_out.is_empty());
        assert!(model.check().is_err());
    }
    file["catalogue_pending_edits"] = json!({original[0].clone(): original});
    crate::model::Model::read(&file.to_string()).unwrap().0.check().unwrap();
    file["format"] = json!(11);
    file.as_object_mut().unwrap().remove("catalogue_pending_edits");
    let old = crate::model::Model::read(&file.to_string()).unwrap().0;
    old.check().unwrap();
    assert_eq!(old.format, crate::model::FORMAT);
    assert!(old.catalogue_pending_edits.is_empty());
}

fn project() -> (tempfile::TempDir, Session, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut session = Session::new();
    session.execute("type", &json!({"category":"wire", "name":"Test cable", "width":6, "round":true})).unwrap();
    session.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    session.execute("save", &json!({"path":project})).unwrap();
    (temp, session, catalogue, project)
}

#[test]
fn deleting_a_device_type_clears_product_data_without_changing_components() {
    let (_temp, mut session, catalogue, project) = project();
    for x in [0, 2000] {
        session.execute("component", &json!({"family":"Fire Alarm Panel","type":"300 x 400","at":[x,0]})).unwrap();
    }
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|row| row[2] == "Fire Alarm Panel: 300 x 400").unwrap();
    row[8] = "Panel brand".into();
    row[11] = "1".into();
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    // The saved geometry is the geometry that must survive the external deletion.
    session.execute("save", &json!({"path":project})).unwrap();
    session.execute("open", &json!({"path":project})).unwrap();
    session.execute("save", &json!({"path":project})).unwrap();
    let elements = session.model.elements.clone();
    let family = session.model.families["Fire Alarm Panel"].clone();
    let guid = session.model.library.guid(&crate::library::Kind::FamilyType, "Fire Alarm Panel: 300 x 400").unwrap();
    assert_eq!(session.model.library.by_guid(guid).unwrap().1.product.as_ref().unwrap().brand, "Panel brand");
    rows.retain(|row| row[2] != "Fire Alarm Panel: 300 x 400");
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let reply = session.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(reply["catalogue_placeholders"], json!([guid]));
    assert_eq!(session.model.elements, elements);
    assert_eq!(session.model.families["Fire Alarm Panel"], family);
    assert!(session.model.library.by_guid(guid).unwrap().1.product.is_none());
    let schedule = session.execute("schedule", &json!({"name":"Panels","category":"component"})).unwrap();
    assert_eq!(schedule["rows"], json!([]));
}

#[test]
fn opening_a_project_reads_external_replacements_and_keeps_type_identity() {
    let (_temp, session, catalogue, project) = project();
    let guid = session.model.library.guid(&crate::library::Kind::Type("wire".into()), "Test cable");
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|r| r[2] == "Test cable").unwrap();
    row[3] = "9".into();
    row[4] = "9".into();
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(reopened.model.types.runs["wire"]["Test cable"].width, 9.0);
    assert_eq!(guid, reopened.model.library.guid(&crate::library::Kind::Type("wire".into()), "Test cable"));
}

#[test]
fn a_broken_external_catalogue_does_not_prevent_opening_the_last_saved_model() {
    let (_temp, _session, catalogue, project) = project();
    std::fs::write(&catalogue, b"broken spreadsheet").unwrap();
    let mut reopened = Session::new();
    let reply = reopened.execute("open", &json!({"path":project})).unwrap();
    assert!(reply["catalogue_not_loaded"].is_string());
    assert_eq!(reopened.model.types.runs["wire"]["Test cable"].width, 6.0);
    assert_eq!(std::fs::read(&catalogue).unwrap(), b"broken spreadsheet");
}

#[test]
fn an_open_project_protects_existing_rows_accepts_new_ones_and_backs_up_external_edits() {
    let (temp, mut session, catalogue, _project) = project();
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|r| r[2] == "Test cable").unwrap();
    row[3] = "9".into();
    row[4] = "9".into();
    let mut added = row.clone();
    added[0].clear();
    added[2] = "Added cable".into();
    rows.push(added);
    let external = ods::write(&rows).unwrap();
    std::fs::write(&catalogue, &external).unwrap();
    let reply = session.execute("catalogue_sync", &json!({})).unwrap();
    assert_eq!(session.model.types.runs["wire"]["Test cable"].width, 6.0);
    assert_eq!(session.model.types.runs["wire"]["Added cable"].width, 9.0);
    let rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let added = rows.iter().find(|r| r[2] == "Added cable").unwrap();
    assert!(!added[0].is_empty());
    let backup = std::path::PathBuf::from(reply["backup"].as_str().unwrap());
    assert!(backup.starts_with(temp.path()));
    assert_eq!(std::fs::read(backup).unwrap(), external);
    let history = session.history();
    let unchanged = session.execute("catalogue_sync", &json!({})).unwrap();
    assert!(unchanged["backup"].is_null(), "An unchanged workbook needs no new backup");
    assert_eq!(session.history(), history, "Polling must not fill the undo history");
}

#[test]
fn synchronization_preserves_print_sheets_sheet_names_and_other_archive_entries() {
    use std::io::{Cursor, Read, Write};
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    let (_temp, mut session, catalogue, _project) = project();
    let original = std::fs::read(&catalogue).unwrap();
    let mut input = ZipArchive::new(Cursor::new(original)).unwrap();
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let print_sheet = "<table:table table:name=\"Печать\"><table:table-row><table:table-cell office:value-type=\"string\"><text:p>Штамп и спецификация</text:p></table:table-cell></table:table-row></table:table>";
    for i in 0..input.len() {
        let mut entry = input.by_index(i).unwrap();
        if entry.name().unwrap() == "content.xml" {
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            xml = xml
                .replace("table:name=\"Каталог\"", "table:name=\"База\"")
                .replace("</office:spreadsheet>", &format!("{print_sheet}</office:spreadsheet>"));
            output.start_file("content.xml", SimpleFileOptions::default()).unwrap();
            output.write_all(xml.as_bytes()).unwrap();
        } else {
            output.raw_copy_file(entry).unwrap();
        }
    }
    output.start_file("styles.xml", SimpleFileOptions::default()).unwrap();
    output.write_all(b"untouched styles").unwrap();
    std::fs::write(&catalogue, output.finish().unwrap().into_inner()).unwrap();
    session.execute("catalogue_sync", &json!({})).unwrap();
    let mut result = ZipArchive::new(Cursor::new(std::fs::read(&catalogue).unwrap())).unwrap();
    let mut xml = String::new();
    result.by_name("content.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert!(xml.contains(print_sheet));
    assert!(xml.contains("table:name=\"База\""));
    let mut styles = Vec::new();
    result.by_name("styles.xml").unwrap().read_to_end(&mut styles).unwrap();
    assert_eq!(styles, b"untouched styles");
}
