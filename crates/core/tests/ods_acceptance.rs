//! Stage 2 acceptance through the public command API.
use bimcraft_core::{engine::Session, library::Kind, ods};
use serde_json::json;

#[test]
fn closed_model_replacement_updates_every_instance_and_preserves_real_connections() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut s = Session::new();
    s.execute("type", &json!({"category":"wire","name":"Acceptance cable","width":6,"depth":6,"round":true})).unwrap();
    s.execute("type", &json!({"category":"cablecarrier","name":"Acceptance box","width":100,"depth":100,"round":false})).unwrap();
    let result = s.execute("cable_carrier", &json!({"type":"Acceptance box","points":[[0,0],[3000,0]]})).unwrap();
    let carrier = result.get("id").or_else(|| result.get("ids").and_then(|ids| ids.get(0))).unwrap().clone();
    let device_type = s.model.families["Voice Outlet"].types.keys().next().unwrap().clone();
    let mut devices = Vec::new();
    for x in [0, 1500, 3000] {
        devices.push(s.execute("component", &json!({"family":"Voice Outlet","type":device_type,"at":[x,0]})).unwrap()["id"].clone());
    }
    for device in &devices[1..] {
        s.execute("wire", &json!({"type":"Acceptance cable","points":[[0,0],[3000,0]],"source":devices[0],"devices":[device],"carrier":carrier,"system":"СПС"})).unwrap();
    }
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("save", &json!({"path":project})).unwrap();
    let before = s.model.elements.clone();
    let type_guid = s.model.library.guid(&Kind::Type("wire".into()), "Acceptance cable");
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|row| row[2] == "Acceptance cable").unwrap();
    row[3] = "9".into();
    row[4] = "9".into();
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let mut reopened = Session::new();
    let reply = reopened.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(reopened.model.types.runs["wire"]["Acceptance cable"].width, 9.0);
    assert_eq!(reopened.model.library.guid(&Kind::Type("wire".into()), "Acceptance cable"), type_guid);
    assert_eq!(reopened.model.elements, before, "ids, GUIDs, carrier, source and device references survive replacement");
    assert_eq!(reopened.model.elements.values().filter(|e| e.category() == "wire").count(), 2);
    reopened.execute("save", &json!({"path":project})).unwrap();
    reopened.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(reopened.model.elements, before);
    reopened.model.check().unwrap();
}

#[test]
fn equivalent_numeric_cells_merge_and_library_category_cannot_be_reassigned() {
    let mut s = Session::new();
    s.execute("type", &json!({"category":"wire","name":"Numeric cable","width":6,"depth":6,"round":true})).unwrap();
    let all = ods::catalogue_rows(&s.model);
    let mut row = all.iter().find(|row| row[2] == "Numeric cable").unwrap().clone();
    let local_guid = row[0].clone();
    row[0].clear();
    row[3] = "6.0".into();
    row[4] = "6e0".into();
    let resolved = ods::transfer_rows(&s.model, &[all[0].clone(), row.clone()], Some("error")).unwrap();
    assert_eq!(resolved[1][0], local_guid);
    row[0] = local_guid;
    row[1] = "cablecarrier".into();
    for choice in ["error", "update", "copy", "invalid"] {
        assert!(ods::transfer_rows(&s.model, &[all[0].clone(), row.clone()], Some(choice)).is_err());
    }
}
