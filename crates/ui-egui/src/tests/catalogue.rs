use super::*;
use egui_kittest::kittest::NodeT;

#[test]
fn catalogue_import_conflicts_are_chosen_in_the_dialog_and_cancel_is_safe() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("copy.ods");
    let mut h = app("");
    let rows = ods::catalogue_rows(&h.state().session.model);
    let mut row = rows.iter().find(|row| row[2] == "Fire Alarm Panel: 300 x 400").unwrap().clone();
    row[3] = "500".into();
    std::fs::write(&path, ods::write(&[rows[0].clone(), row]).unwrap()).unwrap();
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    let before = h.state().session.model.clone();
    press(&mut h, "Import Catalogue...");
    crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).unwrap().send(None).unwrap();
    h.run();
    assert_eq!(h.state().session.model, before);
    press(&mut h, "Import Catalogue...");
    crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).unwrap().send(Some(vec![path.clone()])).unwrap();
    h.run();
    assert!(h.state().status.error);
    assert_eq!(h.state().session.model, before);
    press(&mut h, "Create separate product");
    press(&mut h, "Import Catalogue...");
    crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).unwrap().send(Some(vec![path])).unwrap();
    h.run();
    assert!(!h.state().status.error, "{}", h.state().status.text);
    assert!(h.state().session.model.families["Fire Alarm Panel"].types.contains_key("300 x 400 (2)"));
    assert_eq!(
        h.state().session.model.families["Fire Alarm Panel"].types["300 x 400"],
        before.families["Fire Alarm Panel"].types["300 x 400"]
    );
}

#[test]
fn catalogue_accounts_for_a_selected_imported_product_from_plan_and_3d() {
    for three_d in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("imported.ods");
        let mut h = app("");
        if three_d {
            press(&mut h, "Default 3D View");
        }
        let mut rows = ods::catalogue_rows(&h.state().session.model);
        let mut row = rows.iter().find(|row| row[2] == "Fire Alarm Panel: 300 x 400").unwrap().clone();
        row[0].clear();
        row[2] = "Fire Alarm Panel: Imported panel".into();
        rows.push(row);
        std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
        h.state_mut().session.execute("catalogue_import", &json!({"path":path})).unwrap();
        h.run();
        let families = h.state().session.model.families.clone();
        press(&mut h, "Fire Alarm Panel: Imported panel");
        press(&mut h, "New product");
        assert_eq!(h.get_by_role_and_label(Role::TextInput, "Product name").accesskit_node().value().as_deref(), Some(""));
        assert_eq!(ods::pending_products(&h.state().session.model).len(), 1);
        press(&mut h, "Fire Alarm Panel: Imported panel");
        type_role(&mut h, Role::TextInput, "System", "СОУЭ");
        type_role(&mut h, Role::SpinButton, "Quantity", "3");
        press(&mut h, "Add to specification");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        let items: Vec<_> = h
            .state()
            .session
            .model
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Component(c) if c.provision.is_some() => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].family, "Fire Alarm Panel");
        assert_eq!(items[0].type_name, "Imported panel");
        let provision = items[0].provision.as_ref().unwrap();
        assert_eq!(provision.placement, bimcraft_core::model::Placement::Nonspatial);
        assert_eq!(provision.quantity, 3);
        assert_eq!(provision.system, "СОУЭ");
        assert_eq!(h.state().session.model.families, families);
        assert!(ods::pending_products(&h.state().session.model).is_empty());
    }
}

#[test]
fn new_ods_types_open_the_catalogue_once_without_listing_unused_builtin_types() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("new-equipment.ods");
    let mut h = app("");
    let mut rows = ods::catalogue_rows(&h.state().session.model);
    let mut row = rows.iter().find(|row| row[1] == "family-type" && row[2] == "Fire Alarm Panel: 300 x 400").unwrap().clone();
    row[0].clear();
    row[2] = "Fire Alarm Panel: New project panel".into();
    rows.push(row);
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    h.state_mut().session.execute("catalogue_import", &json!({"path":path})).unwrap();
    h.run();
    assert!(h.state().show_catalogue);
    assert!(h.query_by_label("Fire Alarm Panel: New project panel").is_some());
    press(&mut h, "Close");
    h.state_mut().session.execute("equipment", &json!({"name":"Software"})).unwrap();
    h.run();
    assert!(!h.state().show_catalogue, "the same unused type must not reopen the modal after every edit");
    let project = temp.path().join("pending.bimcraft");
    h.state_mut().session.execute("save", &json!({"path":project})).unwrap();
    assert!(h.state_mut().open_path(&project));
    h.run();
    assert!(h.state().show_catalogue, "reopening the document must remind about its still unassigned products");
    press(&mut h, "Close");
    h.state_mut().session.execute("component", &json!({"family":"Fire Alarm Panel","type":"New project panel","at":[0,0]})).unwrap();
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    assert!(h.query_by_label("Unassigned catalogue products:").is_none());
}

#[test]
fn catalogue_adds_nonspatial_items_from_plan_and_3d() {
    for three_d in [false, true] {
        let mut h = app("");
        if three_d {
            press(&mut h, "Default 3D View");
        }
        press(&mut h, "Manage");
        press(&mut h, "Equipment Catalogue");
        type_role(&mut h, Role::TextInput, "Product name", "ARM licence");
        type_role(&mut h, Role::TextInput, "System", "СОУЭ");
        type_role(&mut h, Role::SpinButton, "Quantity", "3");
        press(&mut h, "Add to specification");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        let items: Vec<_> = h
            .state()
            .session
            .model
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Component(c) if c.provision.is_some() => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].type_name, "ARM licence");
        let provision = items[0].provision.as_ref().unwrap();
        assert_eq!(provision.placement, bimcraft_core::model::Placement::Nonspatial);
        assert_eq!(provision.quantity, 3);
        assert_eq!(provision.system, "СОУЭ");
        assert_eq!(h.get_by_role_and_label(Role::TextInput, "Product name").accesskit_node().value().as_deref(), Some(""));
        assert_eq!(h.get_by_role_and_label(Role::TextInput, "System").accesskit_node().value().as_deref(), Some("СОУЭ"));
    }
}

#[test]
fn removed_catalogue_types_warn_until_the_product_is_restored() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut session = Session::new();
    session.execute("type", &json!({"category":"wire","name":"Removed cable","width":6,"round":true})).unwrap();
    session.execute("catalogue_link", &json!({"path":path})).unwrap();
    session.execute("save", &json!({"path":project})).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut rows = bimcraft_core::ods::read(&original).unwrap();
    rows.retain(|row| row[2] != "Removed cable");
    std::fs::write(&path, bimcraft_core::ods::write(&rows).unwrap()).unwrap();
    let mut h = app("");
    assert!(h.state_mut().open_path(&project));
    h.run();
    assert!(h.state().status.error);
    assert!(h.state().status.text.contains("Missing catalogue product types: 1"), "{}", h.state().status.text);
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    assert!(h.query_by_label("Removed cable").is_some());
    press(&mut h, "Unlink");
    press(&mut h, "Close");
    assert!(h.state().status.error, "Unlinking does not restore missing products");
    let detached = temp.path().join("detached.bimcraft");
    h.state_mut().session.execute("save", &json!({"path":detached})).unwrap();
    assert!(h.state_mut().open_path(&detached));
    h.run();
    assert!(h.state().status.error, "Reopened placeholders must warn without an external link");
    std::fs::write(&path, original).unwrap();
    assert!(h.state_mut().open_path(&project));
    h.run();
    assert!(!h.state().status.error, "{}", h.state().status.text);
}
use bimcraft_core::ods;

#[test]
fn catalogue_dialog_creates_unlinks_and_reconnects_an_existing_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    press(&mut h, "Create...");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).unwrap();
    tx.send(Some(vec![path.clone()])).unwrap();
    h.run();
    assert_eq!(h.state().session.model.catalogue.as_deref(), path.to_str());
    assert!(ods::read(&std::fs::read(&path).unwrap()).is_ok());
    press(&mut h, "Unlink");
    assert!(h.state().session.model.catalogue.is_none());
    assert!(path.is_file(), "unlink keeps the external workbook");
    press(&mut h, "Link Existing...");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).unwrap();
    tx.send(None).unwrap();
    h.run();
    assert!(h.state().session.model.catalogue.is_none(), "cancellation changes nothing");
    press(&mut h, "Link Existing...");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).unwrap();
    tx.send(Some(vec![path.clone()])).unwrap();
    h.run();
    assert_eq!(h.state().session.model.catalogue.as_deref(), path.to_str());
    press(&mut h, "Synchronize");
    assert!(!h.state().status.error, "{}", h.state().status.text);
    press(&mut h, "Close");
    assert!(h.query_by_label("Link Existing...").is_none());
    press(&mut h, "Default 3D View");
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    assert!(h.query_by_label("Link Existing...").is_some(), "the same catalogue is accessible from 3D");
}

#[test]
fn a_bound_catalogue_is_polled_without_a_manual_import() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    let mut h = app("");
    h.state_mut().run("catalogue_link", json!({"path":path})).unwrap();
    let mut rows = ods::read(&std::fs::read(&path).unwrap()).unwrap();
    let mut row = rows.iter().find(|r| r[1] == "wire").unwrap().clone();
    row[0].clear();
    row[2] = "External cable".into();
    rows.push(row);
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    h.ctx.request_repaint();
    h.run();
    assert!(h.state().session.model.types.runs["wire"].contains_key("External cable"));
    let rows = ods::read(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!rows.iter().find(|r| r[2] == "External cable").unwrap()[0].is_empty());
    let history = h.state().session.history();
    for _ in 0..10 {
        h.step();
    }
    assert_eq!(history, h.state().session.history());
    let mut rows = rows;
    let mut added = rows.iter().find(|r| r[2] == "External cable").unwrap().clone();
    added[0].clear();
    added[2] = "Second external cable".into();
    rows.push(added);
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    for _ in 0..10 {
        h.step();
    }
    assert!(h.state().session.model.types.runs["wire"].contains_key("Second external cable"));
}

#[test]
fn changes_in_the_open_model_reach_the_bound_catalogue() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    let mut h = app("");
    h.state_mut().run("catalogue_link", json!({"path":path})).unwrap();
    h.run();
    h.state_mut().run("type", json!({"category":"wire","name":"Model cable","width":8,"round":true})).unwrap();
    h.ctx.request_repaint();
    h.run();
    let rows = ods::read(&std::fs::read(path).unwrap()).unwrap();
    assert!(!rows.iter().find(|r| r[2] == "Model cable").unwrap()[0].is_empty());
}

#[test]
fn opening_with_a_broken_catalogue_shows_a_visible_warning() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut s = Session::new();
    s.execute("catalogue_link", &json!({"path":path})).unwrap();
    s.execute("save", &json!({"path":project})).unwrap();
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"broken").unwrap();
    let mut h = app("");
    assert!(h.state_mut().open_path(&project));
    assert!(h.state().status.error);
    assert!(h.state().status.text.contains("catalogue"), "{}", h.state().status.text);
    let model = h.state().session.model.clone();
    for _ in 0..10 {
        h.step();
    }
    assert_eq!(*h.state().session.model, *model);
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    press(&mut h, "Synchronize");
    assert!(h.state().status.error);
    press(&mut h, "Close");
    std::fs::write(&path, original).unwrap();
    for _ in 0..10 {
        h.step();
    }
    assert!(!h.state().status.error, "{}", h.state().status.text);
}
