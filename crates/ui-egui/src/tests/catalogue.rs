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

#[test]
fn carrier_dialog_checks_highlights_and_saves_project_policy() {
    let mut h = app("");
    for (name, p) in [
        ("type", json!({"category":"cablecarrier","name":"Small box","width":10,"depth":10,"round":false})),
        ("type", json!({"category":"wire","name":"Large cable","width":20,"depth":20,"round":true})),
    ] {
        h.state_mut().session.execute(name, &p).unwrap();
    }
    let reply = h.state_mut().session.execute("cable_carrier", &json!({"type":"Small box","points":[[0,0],[3000,0]]})).unwrap();
    let carrier = reply.get("id").or_else(|| reply["ids"].get(0)).unwrap().clone();
    h.state_mut().session.execute("wire", &json!({"type":"Large cable","points":[[0,0],[3000,0]],"carrier":carrier})).unwrap();
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    press(&mut h, "Cable carriers");
    press(&mut h, "Check and highlight carriers");
    assert_eq!(h.state().selection.len(), 1);
    assert_eq!(h.state().session.model.elements.get(h.state().selection[0]).unwrap().category(), "cablecarrier");
    press(&mut h, "Automatic carrier sizing");
    press(&mut h, "Apply carrier settings");
    assert!(h.state().session.model.carrier_settings.as_ref().unwrap().auto);
}

#[test]
fn selected_carrier_compartments_can_be_configured_without_editing_json() {
    let mut h = app("");
    let reply = h.state_mut().session.execute("cable_carrier", &json!({"type":"Tray 100 x 50","points":[[0,0],[3000,0]]})).unwrap();
    let value = reply.get("id").or_else(|| reply["ids"].get(0)).unwrap().clone();
    let guid: Guid = serde_json::from_value(value).unwrap();
    let id = h.state().session.model.elements.id(guid).unwrap();
    h.state_mut().selection = vec![id];
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    press(&mut h, "Cable carriers");
    press(&mut h, "Carrier compartments");
    type_into(&mut h, "Compartment name:", "Data");
    press(&mut h, "Apply compartment");
    assert!(h.state().session.model.elements.get(id).unwrap().run().unwrap().compartments.contains_key("Data"));
}

#[test]
fn project_carrier_type_can_be_selected_in_the_catalogue_dialog() {
    let mut h = app("");
    h.state_mut()
        .session
        .execute("type", &json!({"category":"cablecarrier","name":"Project tray","width":150,"depth":75,"round":false}))
        .unwrap();
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    press(&mut h, "Cable carriers");
    h.get_by_role_and_label(Role::ComboBox, "Project carrier type").click();
    h.run();
    press(&mut h, "Project tray");
    assert_eq!(h.state().session.model.carrier_settings.as_ref().unwrap().default_type, "Project tray");
}

#[test]
fn opening_an_overfilled_carrier_keeps_its_warning_and_highlight() {
    for configured in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("overfilled.bimcraft");
        let mut h = app("");
        let state = h.state_mut();
        state.session.execute("type", &json!({"category":"cablecarrier","name":"Small tray","width":10,"depth":10,"round":false})).unwrap();
        state.session.execute("type", &json!({"category":"wire","name":"Large cable","width":20,"depth":20,"round":true})).unwrap();
        state.session.execute("cable_carrier", &json!({"type":"Small tray","points":[[0,0],[3000,0]]})).unwrap();
        let carrier = *state.session.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
        state.session.execute("wire", &json!({"type":"Large cable","carrier":carrier,"points":[[0,0],[3000,0]]})).unwrap();
        if configured {
            state.session.execute("cable_carrier_check", &json!({"mode":"warn","fill_percent":40,"configure":true})).unwrap();
        }
        state.session.execute("save", &json!({"path":project})).unwrap();
        assert!(state.open_path(&project));
        assert!(state.selection.contains(&carrier), "opening should highlight the overfilled carrier (configured={configured})");
        assert!(state.status.text.starts_with("Cable carrier conflicts:"), "opening should keep visible warnings");
    }
}

#[cfg(feature = "visual-tests")]
#[test]
#[ignore = "renders the carrier dialog in three looks for visual inspection"]
fn carrier_dialog_visual_acceptance() {
    let output =
        std::env::var_os("DUMP").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("../../target/stage3-visual"));
    std::fs::create_dir_all(&output).unwrap();
    for (name, theme, light_canvas) in [
        ("light", egui::ThemePreference::Light, false),
        ("dark", egui::ThemePreference::Dark, false),
        ("dark-light-canvas", egui::ThemePreference::Dark, true),
    ] {
        let mut s = Session::new();
        s.execute("cable_carrier", &json!({"type":"Tray 100 x 50","points":[[0,0],[3000,0]]})).unwrap();
        let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
        s.execute("type", &json!({"category":"wire","name":"Oversized cable","width":120,"depth":120,"round":true})).unwrap();
        s.execute("wire", &json!({"type":"Oversized cable","carrier":carrier,"points":[[0,0],[3000,0]]})).unwrap();
        s.execute(
            "carrier_section",
            &json!({"type":"Tray 100 x 50","section":{"enclosure":"lidded_box","clear_width":80,"clear_depth":40}}),
        )
        .unwrap();
        let mut state = BimApp::new(s);
        state.theme_wanted = Some(theme);
        state.light_canvas = light_canvas;
        let mut h = Harness::builder()
            .with_size([1280.0, 800.0])
            .with_max_steps(32)
            .renderer(egui_kittest::wgpu::WgpuTestRenderer::new())
            .build_ui_state(|ui, state: &mut BimApp| state.show(ui), state);
        h.run();
        press(&mut h, "Manage");
        press(&mut h, "Equipment Catalogue");
        press(&mut h, "Cable carriers");
        press(&mut h, "SP 76.13330.2016: maximum box fill only");
        press(&mut h, "Apply carrier settings");
        h.render().unwrap().save(output.join(format!("carrier-profile-{name}.png"))).unwrap();
        press(&mut h, "Check and highlight carriers");
        assert_eq!(h.state().selection.len(), 1, "check must select the carrier: {}", h.state().status.text);
        press(&mut h, "Measured product section");
        h.render().unwrap().save(output.join(format!("carrier-measured-{name}.png"))).unwrap();
        press(&mut h, "Carrier compartments");
        type_into(&mut h, "Compartment name:", "Data");
        assert!(h.ctx.content_rect().contains_rect(h.get_by_label("Close").rect()), "expanded dialog must keep Close inside the window");
        h.render().unwrap().save(output.join(format!("carrier-{name}.png"))).unwrap();
    }
}

#[test]
fn carrier_dialog_can_select_the_explicit_box_maximum_profile() {
    use bimcraft_core::carrier_limits::Profile;
    let mut h = app("");
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    press(&mut h, "Cable carriers");
    press(&mut h, "SP 76.13330.2016: maximum box fill only");
    press(&mut h, "Apply carrier settings");
    assert_eq!(h.state().session.model.carrier_settings.as_ref().unwrap().profile, Profile::RuSp76_2016MaxFill);
    assert!(!h.state().status.error, "{}", h.state().status.text);
}

#[test]
fn carrier_dialog_edits_measured_type_section_through_command() {
    let mut h = app("");
    h.state_mut().session.execute("cable_carrier", &json!({"type":"Tray 100 x 50","points":[[0,0],[3000,0]]})).unwrap();
    let id = *h.state().session.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    h.state_mut().selection = vec![id];
    press(&mut h, "Manage");
    press(&mut h, "Equipment Catalogue");
    press(&mut h, "Cable carriers");
    press(&mut h, "Measured product section");
    h.get_by_role_and_label(Role::ComboBox, "Carrier enclosure").click();
    h.run();
    press(&mut h, "Lidded box");
    type_into(&mut h, "Clear width:", "80");
    type_into(&mut h, "Clear depth:", "40");
    press(&mut h, "Apply measured section");
    let measured = h.state().session.model.carrier_section("Tray 100 x 50").unwrap();
    assert_eq!(measured.clear_width, Some(80.0));
    assert_eq!(measured.clear_depth, Some(40.0));
    assert_eq!(measured.enclosure, Some(bimcraft_core::carrier_limits::Enclosure::LiddedBox));
    assert!(!h.state().status.error, "{}", h.state().status.text);
    press(&mut h, "Clear measured section");
    assert!(h.state().session.model.carrier_section("Tray 100 x 50").is_none());
}
