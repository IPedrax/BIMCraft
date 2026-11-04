use serde_json::json;

use crate::{engine::Session, model::Element, ods};

#[test]
fn accounting_for_an_imported_product_reuses_its_type_and_undo_restores_the_pending_entry() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("devices.ods");
    let mut s = Session::new();
    let mut rows = ods::catalogue_rows(&s.model);
    let mut row = rows.iter().find(|row| row[2] == "Fire Alarm Panel: 300 x 400").unwrap().clone();
    row[0].clear();
    row[2] = "Fire Alarm Panel: Imported panel".into();
    rows.push(row);
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    let guid = s.model.library.guid(&crate::library::Kind::FamilyType, "Fire Alarm Panel: Imported panel").unwrap();
    let families_before = s.model.families.clone();
    let result = s.execute("equipment", &json!({"type":guid,"quantity":2,"system":"СПС"})).unwrap();
    let Element::Component(component) = &s.model.elements[result["id"].as_u64().unwrap()] else { panic!("component expected") };
    assert_eq!(component.family, "Fire Alarm Panel");
    assert_eq!(component.type_name, "Imported panel");
    assert_eq!(component.provision.as_ref().unwrap().quantity, 2);
    assert_eq!(component.provision.as_ref().unwrap().system, "СПС");
    assert_eq!(component.provision.as_ref().unwrap().placement, crate::model::Placement::Nonspatial);
    assert_eq!(s.model.families, families_before);
    assert!(ods::pending_products(&s.model).is_empty());
    s.execute("undo", &json!({})).unwrap();
    assert!(ods::pending_products(&s.model).iter().any(|entry| entry.1.guid == guid));
    s.execute("redo", &json!({})).unwrap();
    assert!(ods::pending_products(&s.model).is_empty());
    let before = s.model.clone();
    for invalid in [
        json!({"type":guid,"name":"Duplicate"}),
        json!({"type":guid,"product":{"article":"unexpected"}}),
        json!({"type":guid,"quantity":0}),
        json!({"type":"Not a catalogue product"}),
    ] {
        assert!(s.execute("equipment", &invalid).is_err());
        assert!(std::sync::Arc::ptr_eq(&s.model, &before));
    }
    s.model.check().unwrap();
}

#[test]
fn saved_unexported_catalogue_identities_reject_hostile_metadata() {
    let model = Session::new().model;
    let mut file: serde_json::Value = serde_json::from_str(&model.to_json().unwrap()).unwrap();
    for invalid in [json!([100000]), json!((0..=ods::MAX_ROWS).map(|_| crate::ids::Guid::new()).collect::<Vec<_>>())] {
        file["catalogue_unexported"] = invalid;
        assert!(crate::model::Model::read(&file.to_string()).unwrap().0.check().is_err());
    }
    let guid = crate::ids::Guid::new();
    file["catalogue_retired"] = json!([guid]);
    file["catalogue_unexported"] = json!([guid]);
    assert!(crate::model::Model::read(&file.to_string()).unwrap().0.check().is_err());
}

#[test]
fn saving_before_the_first_catalogue_poll_keeps_new_equipment_billable() {
    for command in ["save", "autosave"] {
        let temp = tempfile::tempdir().unwrap();
        let catalogue = temp.path().join("catalogue.ods");
        let project = temp.path().join("project.bimcraft");
        let mut s = Session::new();
        s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
        let original = std::fs::read(&catalogue).unwrap();
        s.execute("equipment", &json!({"name":"Unsynchronized software","quantity":2,"system":"СОУЭ"})).unwrap();
        let row = ods::catalogue_rows(&s.model).into_iter().find(|row| row[2].ends_with(": Unsynchronized software")).unwrap();
        let before = s.model.clone();
        s.execute(command, &json!({"path":project})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&s.model, &before));
        assert_eq!(std::fs::read(&catalogue).unwrap(), original);
        let mut reopened = Session::new();
        let reply = reopened.execute("open", &json!({"path":project})).unwrap();
        assert!(reply.get("catalogue_not_loaded").is_none(), "{command}: {reply}");
        assert!(
            !reopened.model.library.entries().any(|(_, entry)| entry.guid.to_string() == row[0] && entry.placeholder),
            "saving before the first poll must not exclude a new product from schedules"
        );
        assert!(ods::catalogue_rows(&reopened.model).contains(&row));
        assert!(
            ods::read(&std::fs::read(&catalogue).unwrap())
                .unwrap()
                .iter()
                .any(|written| written.first() == row.first() && written.get(2) == row.get(2))
        );
        assert_eq!(std::fs::read(reply["catalogue"]["backup"].as_str().unwrap()).unwrap(), original);
        reopened.model.check().unwrap();
    }
}

#[test]
fn unsynchronized_equipment_survives_catalogue_failure_and_later_accepts_external_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    let original = std::fs::read(&catalogue).unwrap();
    s.execute("equipment", &json!({"name":"Unsynchronized software"})).unwrap();
    let row = ods::catalogue_rows(&s.model).into_iter().find(|row| row[2].ends_with(": Unsynchronized software")).unwrap();
    s.execute("save", &json!({"path":project})).unwrap();
    std::fs::write(&catalogue, b"damaged workbook").unwrap();
    let mut reopened = Session::new();
    assert!(reopened.execute("open", &json!({"path":project})).unwrap().get("catalogue_not_loaded").is_some());
    reopened.execute("save", &json!({"path":project})).unwrap();
    std::fs::write(&catalogue, &original).unwrap();
    let mut restored = Session::new();
    let reply = restored.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert!(ods::catalogue_rows(&restored.model).contains(&row));
    assert_eq!(std::fs::read(reply["catalogue"]["backup"].as_str().unwrap()).unwrap(), original);
    assert!(restored.model.catalogue_unexported.is_empty());
    restored.execute("save", &json!({"path":project})).unwrap();
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let external = rows.iter_mut().find(|r| r[0] == row[0]).unwrap();
    external.resize(row.len(), String::new());
    external[9] = "REPLACEMENT-ARTICLE".into();
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let mut accepted = Session::new();
    let reply = accepted.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(ods::catalogue_rows(&accepted.model).iter().find(|r| r[0] == row[0]).unwrap()[9], "REPLACEMENT-ARTICLE");
    accepted.model.check().unwrap();
}

#[test]
fn an_external_conflict_with_an_unexported_product_does_not_discard_either_version() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("equipment", &json!({"name":"Unsynchronized software"})).unwrap();
    let row = ods::catalogue_rows(&s.model).into_iter().find(|row| row[2].ends_with(": Unsynchronized software")).unwrap();
    s.execute("save", &json!({"path":project})).unwrap();
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let mut external = row.clone();
    external[9] = "EXTERNAL-ARTICLE".into();
    rows.push(external);
    let external_bytes = ods::write(&rows).unwrap();
    std::fs::write(&catalogue, &external_bytes).unwrap();
    let mut reopened = Session::new();
    let reply = reopened.execute("open", &json!({"path":project})).unwrap();
    assert!(reply["catalogue_not_loaded"].as_str().unwrap().contains("conflicts"));
    assert!(ods::catalogue_rows(&reopened.model).contains(&row));
    assert_eq!(std::fs::read(&catalogue).unwrap(), external_bytes);
    reopened.execute("catalogue_sync", &json!({})).unwrap();
    assert!(reopened.model.catalogue_unexported.is_empty());
    assert!(
        ods::read(&std::fs::read(&catalogue).unwrap())
            .unwrap()
            .iter()
            .any(|written| written.first() == row.first() && written.get(2) == row.get(2))
    );
    let mut recovered_again = Session::new();
    let reply = recovered_again.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "a saved pending row may already have reached the ODS: {reply}");
    assert!(ods::catalogue_rows(&recovered_again.model).contains(&row));
}

#[test]
fn saving_immediately_after_undo_does_not_restore_an_exported_product_on_open() {
    for command in ["save", "autosave"] {
        let temp = tempfile::tempdir().unwrap();
        let catalogue = temp.path().join("catalogue.ods");
        let project = temp.path().join("project.bimcraft");
        let mut s = Session::new();
        s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
        s.execute("equipment", &json!({"name":"Cancelled software"})).unwrap();
        s.execute("catalogue_sync", &json!({})).unwrap();
        let original = std::fs::read(&catalogue).unwrap();
        let rows = ods::read(&original).unwrap();
        let guid = rows.iter().find(|row| row[2] == "Specification Items: Cancelled software").unwrap()[0].clone();
        s.execute("undo", &json!({})).unwrap();
        let before = s.model.clone();
        let occupied = temp.path().join("occupied");
        std::fs::create_dir(&occupied).unwrap();
        assert!(s.execute(command, &json!({"path":occupied})).is_err());
        assert!(std::sync::Arc::ptr_eq(&s.model, &before));
        s.execute(command, &json!({"path":project})).unwrap();
        assert_eq!(s.model, before, "saving must not edit the model or its undo state");
        assert_eq!(std::fs::read(&catalogue).unwrap(), original, "saving a project must not rewrite its external catalogue");
        let mut reopened = Session::new();
        let reply = reopened.execute("open", &json!({"path":project})).unwrap();
        assert!(reply.get("catalogue_not_loaded").is_none(), "{command}: {reply}");
        assert!(!ods::catalogue_rows(&reopened.model).iter().any(|row| row[0] == guid));
        assert!(!ods::read(&std::fs::read(&catalogue).unwrap()).unwrap().iter().any(|row| row[0] == guid));
        let backup = reply["catalogue"]["backup"].as_str().unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), original);
        reopened.model.check().unwrap();
        // Copied foreign products are new local entries, never the cancelled product.
        let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
        let mut foreign = rows.iter().find(|row| row[1] == "family-type").unwrap().clone();
        foreign[0] = crate::ids::Guid::new().to_string();
        let foreign_guid = foreign[0].clone();
        foreign[2] = "Fire Alarm Panel: Foreign panel".into();
        rows.push(foreign);
        std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
        let mut imported = Session::new();
        let reply = imported.execute("open", &json!({"path":project})).unwrap();
        assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
        let rows = ods::catalogue_rows(&imported.model);
        assert!(!rows.iter().any(|row| row[0] == guid));
        assert_ne!(rows.iter().find(|row| row[2] == "Fire Alarm Panel: Foreign panel").unwrap()[0], foreign_guid);
    }
}

#[test]
fn saved_cancellations_survive_catalogue_failures_without_affecting_another_file() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("equipment", &json!({"name":"Cancelled software"})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let original = std::fs::read(&catalogue).unwrap();
    let guid = ods::read(&original).unwrap().iter().find(|row| row[2].ends_with(": Cancelled software")).unwrap()[0].clone();
    let guid = crate::ids::Guid::parse(&guid).unwrap();
    s.execute("undo", &json!({})).unwrap();
    s.execute("save", &json!({"path":project})).unwrap();
    std::fs::write(&catalogue, b"broken workbook").unwrap();
    let mut reopened = Session::new();
    assert!(reopened.execute("open", &json!({"path":project})).unwrap().get("catalogue_not_loaded").is_some());
    assert!(reopened.model.catalogue_retired.contains(&guid));
    let before = reopened.model.clone();
    assert!(reopened.execute("catalogue_sync", &json!({})).is_err());
    assert!(std::sync::Arc::ptr_eq(&reopened.model, &before));
    reopened.execute("save", &json!({"path":project})).unwrap();
    let another = temp.path().join("another.ods");
    std::fs::write(&another, &original).unwrap();
    assert!(reopened.execute("catalogue_link", &json!({"path":another})).is_err(), "a different workbook must not inherit cancelled GUIDs");
    assert_eq!(std::fs::read(&another).unwrap(), original);
    assert!(std::sync::Arc::ptr_eq(&reopened.model, &before));
    reopened.execute("catalogue_link", &json!({"path":null})).unwrap();
    assert!(reopened.model.catalogue_retired.is_empty());
    reopened.execute("undo", &json!({})).unwrap();
    assert!(reopened.model.catalogue_retired.contains(&guid));
    std::fs::write(&catalogue, &original).unwrap();
    let reply = reopened.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert!(reopened.model.catalogue_retired.is_empty());
    assert!(!ods::read(&std::fs::read(&catalogue).unwrap()).unwrap().iter().any(|row| row[0] == guid.to_string()));
}

#[test]
fn saved_catalogue_cancellations_require_bounded_real_guids() {
    let model = Session::new().model;
    let mut file: serde_json::Value = serde_json::from_str(&model.to_json().unwrap()).unwrap();
    let active = model.library.entries().next().unwrap().1.guid;
    for invalid in [json!([100000]), json!([active]), json!((0..=ods::MAX_ROWS).map(|_| crate::ids::Guid::new()).collect::<Vec<_>>())] {
        file["catalogue_retired"] = invalid;
        assert!(crate::model::Model::read(&file.to_string()).unwrap().0.check().is_err());
    }
}

#[test]
fn failed_document_replacement_restores_the_original_catalogue_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    let other = temp.path().join("other.bimcraft");
    Session::new().execute("save", &json!({"path":other})).unwrap();
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":path})).unwrap();
    s.execute("equipment", &json!({"name":"Owned software"})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let result: Result<(), String> = s.transaction("Failed replacement", |s| {
        s.execute("open", &json!({"path":other}))?;
        Err("Cancelled".into())
    });
    assert!(result.is_err());
    s.execute("undo", &json!({})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let rows = ods::read(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!rows.iter().any(|row| row[2] == "Specification Items: Owned software"));
}

#[test]
fn undoing_a_new_product_updates_the_linked_ods_and_redo_keeps_its_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    let mut s = Session::new();
    let initial_count = s.model.elements.len();
    s.execute("catalogue_link", &json!({"path":path})).unwrap();
    s.execute("equipment", &json!({"name":"Undoable software"})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let original = ods::read(&std::fs::read(&path).unwrap()).unwrap();
    let original = original.iter().find(|row| row[2] == "Specification Items: Undoable software").unwrap().clone();
    s.execute("undo", &json!({})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let rows = ods::read(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!rows.iter().any(|row| row[0] == original[0]), "synchronization must not resurrect the undone type");
    assert_eq!(s.model.elements.len(), initial_count, "Undo must remove the bill position");
    s.execute("redo", &json!({})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let rows = ods::read(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(rows.iter().find(|row| row[0] == original[0]), Some(&original));
    s.execute("undo", &json!({})).unwrap();
    s.execute("equipment", &json!({"name":"Replacement software"})).unwrap();
    s.execute("catalogue_sync", &json!({})).unwrap();
    let rows = ods::read(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!rows.iter().any(|row| row[0] == original[0]), "clearing Redo must not forget the catalogue's ownership");
}

#[test]
fn adding_nonspatial_equipment_needs_only_a_product_system_and_quantity() {
    let mut s = Session::new();
    let a = s
        .execute(
            "equipment",
            &json!({"name":"АРМ software","quantity":2,"system":"СОУЭ",
        "product":{"description":"Software licence","article":"LIC-01","mass_kg":0.0}}),
        )
        .unwrap();
    let b = s.execute("equipment", &json!({"name":"АРМ software","quantity":3,"system":"СОУЭ"})).unwrap();
    assert_ne!(a["id"], b["id"]);
    let rows = ods::catalogue_rows(&s.model);
    let products: Vec<_> = rows.iter().filter(|r| r[2].ends_with(": АРМ software")).collect();
    assert_eq!(products.len(), 1);
    assert_eq!(products[0][9], "LIC-01");
    let table = s
        .execute(
            "schedule",
            &json!({"name":"Equipment","category":"component","fields":["Type","System","Count"],"sort":["Type","System"],"grouped":true}),
        )
        .unwrap();
    assert_eq!(table["rows"], json!([["АРМ software", "СОУЭ", 5.0]]));
    let before = s.model.clone();
    assert!(s.execute("equipment", &json!({"name":"Bad","quantity":0})).is_err());
    assert_eq!(*s.model, *before);
    s.execute("language", &json!({"code":"ru"})).unwrap();
    let item = s.execute("equipment", &json!({"name":"Саморез"})).unwrap();
    let Element::Component(component) = &s.model.elements[item["id"].as_u64().unwrap()] else { panic!("component expected") };
    assert_eq!(component.family, "Позиции спецификации");
    assert_eq!(component.provision.as_ref().unwrap().quantity, 1);
    let table = s
        .execute(
            "schedule",
            &json!({"name":"Russian equipment","category":"component","fields":["Type","Placement"],
        "filters":[{"field":"Type","op":"equals","value":"Саморез"}]}),
        )
        .unwrap();
    assert_eq!(table["rows"], json!([["Саморез", "Не требует размещения"]]));
}

#[test]
fn equipment_without_placement_counts_in_schedules_but_has_no_geometry() {
    for placement in ["planned", "nonspatial"] {
        let mut s = Session::new();
        let made = s
            .execute(
                "component",
                &json!({"family":"Fire Alarm Panel","type":"300 x 400",
            "provision":{"placement":placement,"quantity":3,"system":"СПС"}}),
            )
            .unwrap();
        let id: crate::ids::Id = serde_json::from_value(made["id"].clone()).unwrap();
        let guid = s.model.guid(id);
        let Element::Component(component) = s.model.elements.get(id).unwrap() else { panic!("component") };
        assert!(s.model.component_pieces(component).is_empty());
        let table = s
            .execute("schedule", &json!({"name":"Equipment","category":"component","fields":["Type","Count","Placement","System"]}))
            .unwrap();
        let label = if placement == "planned" { "Planned" } else { "Placement not required" };
        assert_eq!(table["rows"], json!([["300 x 400", 3.0, label, "СПС"]]));
        let source = s.execute("component", &json!({"family":"Fire Alarm Panel","at":[1000,0]})).unwrap();
        let source_id = source["id"].as_u64().unwrap();
        let mut cache = crate::component_geometry::Components::new(&s.model);
        let Element::Component(unplaced) = s.model.elements.get(id).unwrap() else { panic!("component") };
        assert!(cache.pieces(unplaced).is_empty());
        let Element::Component(placed) = s.model.elements.get(source_id).unwrap() else { panic!("component") };
        assert!(!cache.pieces(placed).is_empty(), "Nonplaced types must not poison the geometry cache");
        let before = s.model.clone();
        let error = s.execute("wire", &json!({"points":[[1000,0],[0,0]],"source":source_id,"devices":[id]})).unwrap_err();
        assert!(error.contains("placed"), "{error}");
        assert_eq!(*s.model, *before);
        s.execute("modify", &json!({"id":id,"at":[2000,1000],"provision":{"placement":"placed","quantity":1,"system":"СПС"}})).unwrap();
        let Element::Component(component) = s.model.elements.get(id).unwrap() else { panic!("component") };
        assert!(!s.model.component_pieces(component).is_empty());
        assert_eq!(s.model.guid(id), guid);
    }
}

#[test]
fn equipment_provisions_keep_hosts_identity_and_grouped_quantities_after_reopening() {
    let mut s = Session::new();
    let temp = tempfile::tempdir().unwrap();
    let host = s.execute("component", &json!({"family":"Fire Alarm Panel","at":[1000,2000]})).unwrap();
    let host_id = host["id"].as_u64().unwrap();
    let host_guid = s.model.guid(host_id);
    s.execute("family", &json!({"name":"Software","category":"generic","types":{"License":{}}})).unwrap();
    let mut ids = Vec::new();
    for (placement, quantity) in [("planned", 2), ("nonspatial", 3)] {
        let reply = s
            .execute(
                "component",
                &json!({"family":"Software",
            "provision":{"placement":placement,"quantity":quantity,"system":"СОУЭ","host":host_id}}),
            )
            .unwrap();
        let id = reply["id"].as_u64().unwrap();
        ids.push((id, s.model.guid(id)));
    }
    let project = temp.path().join("project.bimcraft");
    s.execute("save", &json!({"path":project})).unwrap();
    s.execute("open", &json!({"path":project})).unwrap();
    for (id, guid) in ids {
        assert_eq!(s.model.guid(id), guid);
        let Element::Component(c) = s.model.elements.get(id).unwrap() else { panic!("component") };
        assert_eq!(c.provision.as_ref().unwrap().host, Some(host_guid));
        assert!(!c.is_placed());
    }
    let table = s
        .execute(
            "schedule",
            &json!({"name":"Software","category":"component",
        "fields":["Type","System","Count"],"filters":[{"field":"Family","op":"equals","value":"Software"}],"sort":["Type","System"],"grouped":true}),
        )
        .unwrap();
    assert_eq!(table["rows"], json!([["License", "СОУЭ", 5.0]]));
}

#[test]
fn invalid_equipment_quantities_hosts_and_placement_roll_back() {
    let mut s = Session::new();
    for provision in [
        json!({"placement":"planned","quantity":0}),
        json!({"placement":"nonspatial","quantity":1000001}),
        json!({"placement":"placed","quantity":2}),
        json!({"placement":"planned","quantity":1,"host":100001}),
        json!({"placement":"planned","quantity":1,"host":999998}),
        json!({"placement":"planned","quantity":1,"typo":true}),
    ] {
        let before = s.model.clone();
        let result = s.execute("component", &json!({"family":"Fire Alarm Panel","at":[0,0],"provision":provision}));
        // A level is an existing element but cannot own equipment.
        assert!(result.is_err(), "{provision}: {result:?}");
        assert_eq!(*s.model, *before);
    }
}

#[test]
fn nonplaced_equipment_changes_neither_plan_3d_nor_ifc_geometry() {
    let temp = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    s.execute("wall", &json!({"points":[[0,0],[5000,0],[5000,4000]],"closed":false})).unwrap();
    let mut images = Vec::new();
    for (index, view) in ["Level 1", "{3D}"].into_iter().enumerate() {
        let path = temp.path().join(format!("{index}.png"));
        s.execute("render", &json!({"view":view,"path":path,"width":400,"height":300})).unwrap();
        images.push((view, path.clone(), std::fs::read(path).unwrap()));
    }
    let mut nonplaced = Vec::new();
    for placement in ["planned", "nonspatial"] {
        let result = s
            .execute(
                "component",
                &json!({"family":"Fire Alarm Panel","at":[9000,9000],
            "provision":{"placement":placement,"quantity":10,"system":"СПС"}}),
            )
            .unwrap();
        nonplaced.push(result["id"].as_u64().unwrap());
    }
    for (view, path, before) in images {
        s.execute("render", &json!({"view":view,"path":path,"width":400,"height":300})).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), before, "{view}");
    }
    let placed = s.execute("component", &json!({"family":"Fire Alarm Panel","at":[1000,1000]})).unwrap();
    let ifc = crate::ifc::export(&s.model);
    let placed_guid = s.model.guid(placed["id"].as_u64().unwrap());
    assert!(ifc.contains(&format!("'{}'", crate::ifc::compress(placed_guid.as_u128()))));
    for id in nonplaced {
        let guid = s.model.guid(id);
        assert!(!ifc.contains(&format!("'{}'", crate::ifc::compress(guid.as_u128()))));
    }
}

#[test]
fn catalogue_cannot_replace_a_computed_dimension_with_a_literal() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("devices.ods");
    let mut s = Session::new();
    s.execute(
        "family",
        &json!({
            "name":"Formula device", "category":"alarmpanel",
            "parameters":[
                {"name":"Width", "kind":"length", "value":300, "formula":"Height / 2"},
                {"name":"Height", "kind":"length", "value":600}
            ], "types":{"One":{}}
        }),
    )
    .unwrap();
    let before = s.model.clone();
    let mut rows = ods::catalogue_rows(&s.model);
    let row = rows.iter_mut().find(|r| r[2] == "Formula device: One").unwrap();
    assert!(row[3].is_empty());
    row[3] = "500".into();
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    let error = s.execute("catalogue_import", &json!({"path":path})).unwrap_err();
    assert!(error.contains("editable type length parameter"), "{error}");
    assert_eq!(*s.model, *before);
}

#[test]
fn a_new_device_type_uses_the_existing_family_and_gets_its_own_guid() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("devices.ods");
    let mut s = Session::new();
    assert_eq!(s.execute("catalogue_pending", &json!({})).unwrap()["types"], json!([]));
    let mut rows = ods::catalogue_rows(&s.model);
    let mut row = rows.iter().skip(1).find(|r| r[1] == "family-type" && r[2] == "Fire Alarm Panel: 300 x 400").unwrap().clone();
    row[0].clear();
    row[2] = "Fire Alarm Panel: Alternative".into();
    row[3] = "450".into();
    row[12] = "600".into();
    rows.push(row);
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    let kind = crate::library::Kind::FamilyType;
    let guid = s.model.library.guid(&kind, "Fire Alarm Panel: Alternative").unwrap();
    assert_ne!(Some(guid), s.model.library.guid(&kind, "Fire Alarm Panel: 300 x 400"));
    let pending = s.execute("catalogue_pending", &json!({})).unwrap();
    assert_eq!(pending["types"].as_array().unwrap().len(), 1);
    assert_eq!(pending["types"][0]["guid"], json!(guid));
    let saved = s.model.to_json().unwrap();
    let (restored, _) = crate::model::Model::read(&saved).unwrap();
    assert_eq!(ods::pending_products(&restored).len(), 1);
    s.execute("component", &json!({"family":"Fire Alarm Panel", "type":"Alternative", "at":[0,0]})).unwrap();
    assert_eq!(s.execute("catalogue_pending", &json!({})).unwrap()["types"], json!([]));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.execute("catalogue_pending", &json!({})).unwrap()["types"].as_array().unwrap().len(), 1);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(s.execute("catalogue_pending", &json!({})).unwrap()["types"], json!([]));
    s.execute("save", &json!({"path":temp.path().join("project.bimcraft")})).unwrap();
    assert_eq!(s.model.families["Fire Alarm Panel"].types["Alternative"]["Width"].as_f64(), Some(450.0));
}

#[test]
fn nonspatial_bill_positions_satisfy_imported_catalogue_types_without_placing_geometry() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("software.ods");
    let mut s = Session::new();
    s.execute("equipment", &json!({"name":"Original software"})).unwrap();
    let mut rows = ods::catalogue_rows(&s.model);
    let mut row = rows.iter().find(|r| r[2] == "Specification Items: Original software").unwrap().clone();
    row[0].clear();
    row[2] = "Specification Items: Imported software".into();
    rows.push(row);
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    let history = s.history();
    assert_eq!(s.execute("catalogue_pending", &json!({})).unwrap()["types"].as_array().unwrap().len(), 1);
    assert_eq!(s.history(), history, "querying the list must not create an undo step");
    s.execute(
        "component",
        &json!({"family":"Specification Items","type":"Imported software",
        "provision":{"placement":"nonspatial","quantity":5,"system":"СОУЭ"}}),
    )
    .unwrap();
    assert_eq!(s.execute("catalogue_pending", &json!({})).unwrap()["types"], json!([]));
}

#[test]
fn equipment_type_dimensions_update_every_instance_and_keep_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("devices.ods");
    let mut s = Session::new();
    for x in [0, 2000] {
        s.execute("component", &json!({"family":"Fire Alarm Panel", "type":"300 x 400", "at":[x,0]})).unwrap();
    }
    let identities: Vec<_> = s.model.elements.iter().map(|(id, _)| (*id, s.model.guid(*id))).collect();
    let type_guid = s.model.library.guid(&crate::library::Kind::FamilyType, "Fire Alarm Panel: 300 x 400");
    let mut rows = ods::catalogue_rows(&s.model);
    let row = rows.iter_mut().skip(1).find(|r| r[1] == "family-type" && r[2] == "Fire Alarm Panel: 300 x 400").unwrap();
    row[3] = "600".into();
    row[4] = "200".into();
    row[12] = "800".into();
    row[8] = "Replacement panel".into();
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    let project = temp.path().join("project.bimcraft");
    s.execute("save", &json!({"path":project})).unwrap();
    s.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(type_guid, s.model.library.guid(&crate::library::Kind::FamilyType, "Fire Alarm Panel: 300 x 400"));
    assert_eq!(identities, s.model.elements.iter().map(|(id, _)| (*id, s.model.guid(*id))).collect::<Vec<_>>());
    let mut count = 0;
    for element in s.model.elements.values() {
        if let Element::Component(component) = element {
            let pieces = s.model.component_pieces(component);
            let min_x = pieces.iter().flat_map(|p| &p.outline).map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = pieces.iter().flat_map(|p| &p.outline).map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            assert!((max_x - min_x - 600.0).abs() < 1e-6);
            let lo = pieces.iter().map(|p| p.bottom).fold(f64::INFINITY, f64::min);
            let hi = pieces.iter().map(|p| p.top).fold(f64::NEG_INFINITY, f64::max);
            assert!((hi - lo - 800.0).abs() < 1e-6);
            count += 1;
        }
    }
    assert_eq!(count, 2);
    let exported = ods::catalogue_rows(&s.model);
    assert_eq!(exported.iter().skip(1).find(|r| r[2] == "Fire Alarm Panel: 300 x 400").unwrap()[8], "Replacement panel");
}
