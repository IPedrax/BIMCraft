use super::*;

#[test]
fn stage4_renga_interface_keeps_model_selection_and_tool_when_switching() {
    let mut h = app("wire {\"points\":[[0,0],[3000,0]],\"offset\":3000}");
    let wire = ids(&h, "wire")[0];
    h.state_mut().set_tool(Tool::Run("wire", None));
    h.state_mut().selection = vec![wire];
    let before = h.state().session.model.to_json().unwrap();
    press(&mut h, "Minimal interface");
    assert_eq!(h.state().selection, vec![wire]);
    assert_eq!(h.state().tool, Tool::Run("wire", None));
    press(&mut h, "Switch to 3D");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert_eq!(h.state().tool, Tool::Run("wire", None));
    press(&mut h, "Revit interface");
    assert_eq!(h.state().selection, vec![wire]);
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
}

#[test]
fn stage4_minimal_interface_survives_new_project_and_restart() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(path.clone());
    press(&mut h, "Minimal interface");
    h.state_mut().reopened();
    h.run();
    assert!(h.state().minimal);
    let mut restored = BimApp::new(Session::new());
    restored.use_settings(path);
    assert!(restored.minimal);
}

#[test]
fn stage4_minimal_search_selects_tools_without_editing_the_model() {
    let mut h = app(BOX);
    press(&mut h, "Minimal interface");
    let before = h.state().session.model.to_json().unwrap();
    type_role(&mut h, Role::TextInput, "Search tools", "cable car");
    press(&mut h, "Cable Carrier");
    assert_eq!(h.state().tool, Tool::Run("cablecarrier", None));
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Search tools").value().as_deref(), Some(""));
    type_role(&mut h, Role::TextInput, "Search tools", "no such tool");
    assert!(h.query_by_label("No matching tools").is_some());
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    type_role(&mut h, Role::TextInput, "Search tools", "electrical equipment");
    press(&mut h, "Electrical Equipment");
    assert_eq!(h.state().tool, Tool::Fixture("equipment"));
    type_role(&mut h, Role::TextInput, "Search tools", "smoke");
    press(&mut h, "Smoke Detector");
    assert_eq!(h.state().tool, Tool::Fixture("smokedetector"));
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
}

#[test]
fn stage4_minimal_all_tools_places_equipment_from_the_existing_discipline_menu() {
    let mut h = app(BOX);
    press(&mut h, "Minimal interface");
    press(&mut h, "All Tools");
    press(&mut h, "Low Voltage ⏵");
    press(&mut h, "Smoke Detector");
    assert_eq!(h.state().tool, Tool::Fixture("smokedetector"));
    h.state_mut().component_offset = "2500".into();
    h.run();
    stage4_click_world(&mut h, [3000.0, 2000.0, 2500.0], false);
    assert_eq!(ids(&h, "component").len(), 1);
}

#[cfg(feature = "visual-tests")]
#[test]
#[ignore = "renders stage 4 in three looks for visual inspection"]
fn stage4_visual_acceptance() {
    let output =
        std::env::var_os("DUMP").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("../../target/stage4-visual"));
    std::fs::create_dir_all(&output).unwrap();
    for (name, theme, light_canvas) in [
        ("light", egui::ThemePreference::Light, false),
        ("dark", egui::ThemePreference::Dark, false),
        ("dark-light-canvas", egui::ThemePreference::Dark, true),
    ] {
        let mut session = Session::new();
        session.script(BOX).unwrap();
        let level = *session.model.elements.iter().find(|(_, element)| matches!(element, Element::Level(_))).unwrap().0;
        session.execute("modify", &json!({"id":level,"view_range":{"top":4500,"cut":1500,"bottom":0,"depth":0}})).unwrap();
        let wire = session.execute("wire", &json!({"points":[[1000,1000],[6000,1000]],"offset":3000,"end_offset":3300})).unwrap()["ids"][0]
            .as_u64()
            .unwrap();
        session.execute("cable_carrier", &json!({"from_wire":wire,"type":"Tray 100 x 50","placement":"bottom"})).unwrap();
        let mut devices = Vec::new();
        for (family, x, offset) in [("Fire Alarm Panel", 2000, 1000), ("Smoke Detector", 4000, 4000), ("Fire Alarm Panel", 5500, 3270)] {
            let device =
                session.execute("component", &json!({"family":family,"at":[x,2500],"offset":offset})).unwrap()["id"].as_u64().unwrap();
            session.execute("wire_connect", &json!({"wire":wire,"device":device,"point":[0,0,0],"in_carrier":true})).unwrap();
            devices.push(device);
        }
        let mut state = BimApp::new(session);
        state.theme_wanted = Some(theme);
        state.light_canvas = light_canvas;
        let mut h = Harness::builder()
            .with_size([1280.0, 800.0])
            .with_max_steps(32)
            .renderer(egui_kittest::wgpu::WgpuTestRenderer::new())
            .build_ui_state(|ui, state: &mut BimApp| state.show(ui), state);
        h.run();
        h.render().unwrap().save(output.join(format!("floor-plan-{name}.png"))).unwrap();
        press(&mut h, "Switch to 3D");
        h.render().unwrap().save(output.join(format!("floor-3d-{name}.png"))).unwrap();
        press(&mut h, "Minimal interface");
        h.render().unwrap().save(output.join(format!("minimal-3d-{name}.png"))).unwrap();
        press(&mut h, "Switch to 2D");
        h.render().unwrap().save(output.join(format!("minimal-plan-{name}.png"))).unwrap();
        press(&mut h, "Revit interface");
        press(&mut h, "Switch to 3D");
        h.state_mut().selection = vec![wire, devices[0]];
        h.state_mut().act(crate::context::Action::ConnectEquipmentToWire);
        h.run();
        h.render().unwrap().save(output.join(format!("connection-{name}.png"))).unwrap();
        press(&mut h, "Cancel");
        let result = h.state_mut().session.execute("wire", &json!({"points":[[1000,4000],[6000,4000]],"offset":3000})).unwrap();
        h.state_mut().selection = vec![result["ids"][0].as_u64().unwrap()];
        h.state_mut().act(crate::context::Action::CreateCarrierFromWire);
        h.run();
        h.render().unwrap().save(output.join(format!("carrier-placement-{name}.png"))).unwrap();
        press(&mut h, "Cancel");
        shortcut(&mut h, "FA");
        press(&mut h, "Place on Face");
        let at = h.state().screen3([4000.0, -100.0, 1800.0]);
        h.hover_at(at);
        h.run();
        h.render().unwrap().save(output.join(format!("equipment-face-preview-{name}.png"))).unwrap();
    }
}

fn stage4_click_world(h: &mut Harness<BimApp>, point: [f64; 3], three_d: bool) {
    let at = if three_d { h.state().screen3(point) } else { h.state().screen(P2(point[0], point[1])) };
    click_at(h, at);
}

#[test]
fn stage4_floor_equipment_workflow_preserves_owned_guids_in_both_views() {
    for (three_d, minimal) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut h = app(BOX);
        if minimal {
            press(&mut h, "Minimal interface");
            press(&mut h, "Wire");
            assert_eq!(h.state().tool, Tool::Run("wire", None));
        }
        if three_d {
            press(&mut h, "Switch to 3D");
        }
        shortcut(&mut h, "EW");
        h.state_mut().beam_offset = "3000".into();
        h.run();
        stage4_click_world(&mut h, [1000.0, 1000.0, 3000.0], three_d);
        stage4_click_world(&mut h, [5000.0, 1000.0, 3000.0], three_d);
        let wire = ids(&h, "wire")[0];
        h.state_mut().selection = vec![wire];
        h.state_mut().act(crate::context::Action::CreateCarrierFromWire);
        h.run();
        press(&mut h, "Create Carrier");
        for (x, height) in [(2000.0, 1000.0), (3000.0, 4000.0), (4000.0, 3000.0)] {
            let before = ids(&h, "component");
            shortcut(&mut h, if height > 3000.0 { "DS" } else { "FA" });
            h.state_mut().component_offset = height.to_string();
            h.run();
            stage4_click_world(&mut h, [x, 2000.0, height], three_d);
            let device = ids(&h, "component").into_iter().find(|id| !before.contains(id)).expect("click places equipment");
            h.state_mut().selection = vec![wire, device];
            h.state_mut().act(crate::context::Action::ConnectEquipmentToWire);
            h.run();
            press(&mut h, "Create carrier branches");
            press(&mut h, "Connect");
            assert!(!h.state().status.error, "{}", h.state().status.text);
        }
        let model = &h.state().session.model;
        assert_eq!(model.elements.get(wire).unwrap().run().unwrap().connections.len(), 3);
        let guids: Vec<_> = model.elements.with_guids().map(|(_, guid, _)| guid).collect();
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        h.state_mut().session.execute("redo", &json!({})).unwrap();
        assert_eq!(h.state().session.model.elements.with_guids().map(|(_, guid, _)| guid).collect::<Vec<_>>(), guids);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("floor.bimcraft");
        h.state_mut().session.execute("save", &json!({"path":path})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path":path})).unwrap();
        assert_eq!(reopened.model.elements.with_guids().map(|(_, guid, _)| guid).collect::<Vec<_>>(), guids);
        reopened.model.check().unwrap();
    }
}

#[test]
fn stage4_run_options_draw_slopes_and_verticals_in_both_views() {
    for three_d in [false, true] {
        for vertical in [false, true] {
            let mut h = app(BOX);
            if three_d {
                press(&mut h, "Default 3D View");
            }
            shortcut(&mut h, "EW");
            h.state_mut().beam_offset = "1000".into();
            h.run();
            type_role(&mut h, Role::TextInput, "End Offset:", "3000");
            if vertical {
                press(&mut h, "Vertical");
            }
            stage4_click_world(&mut h, [1000.0, 1000.0, 1000.0], three_d);
            stage4_click_world(&mut h, [if vertical { 1000.0 } else { 3000.0 }, 1000.0, 3000.0], three_d);
            h.run();
            assert!(!h.state().status.error, "{}", h.state().status.text);
            let wire = ids(&h, "wire")[0];
            let model = &h.state().session.model;
            let geometry = model.run_geom(model.elements.get(wire).unwrap()).unwrap();
            assert_eq!(geometry.a[2], 1000.0);
            assert_eq!(geometry.b[2], 3000.0);
            assert_eq!(geometry.vertical(), vertical);
            assert!((geometry.b[0] - if vertical { 1000.0 } else { 3000.0 }).abs() < 20.0);
            assert!((geometry.b[1] - 1000.0).abs() < 20.0);
        }
    }
}

#[test]
fn stage4_interface_switches_plan_and_3d_without_dropping_the_wire_tool() {
    let mut h = app(BOX);
    let level = h.state().plan_level().unwrap();
    shortcut(&mut h, "EW");
    press(&mut h, "Switch to 3D");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert!(matches!(h.state().tool, Tool::Run("wire", None)));
    press(&mut h, "Switch to 2D");
    assert_eq!(h.state().sheet, Sheet::Plan(level));
    assert!(matches!(h.state().tool, Tool::Run("wire", None)));
}

#[test]
fn stage4_wire_is_drawn_with_real_clicks_in_3d_at_its_workplane_height() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    shortcut(&mut h, "EW");
    h.state_mut().beam_offset = "3000".into();
    h.run();
    for point in [[1000.0, 1000.0, 3000.0], [3000.0, 1000.0, 3000.0]] {
        let at = h.state().screen3(point);
        click_at(&mut h, at);
    }
    let wires = ids(&h, "wire");
    assert_eq!(wires.len(), 1, "3D clicks must execute the same wire command as a plan");
    let geometry = h.state().session.model.run_geom(h.state().session.model.elements.get(wires[0]).unwrap()).unwrap();
    assert!((geometry.a[2] - 3000.0).abs() < 1.0);
    assert!((geometry.b[2] - 3000.0).abs() < 1.0);
    assert!((geometry.length() - 2000.0).abs() < 20.0);
    key(&mut h, Key::Escape);
    shortcut(&mut h, "FA");
    h.state_mut().component_offset = "1000".into();
    h.run();
    let at = h.state().screen3([1500.0, 1500.0, 1000.0]);
    click_at(&mut h, at);
    assert_eq!(ids(&h, "component").len(), 1, "equipment must also be placed by 3D clicks");
    let device = ids(&h, "component")[0];
    assert!(matches!(h.state().session.model.elements.get(device),Some(Element::Component(c)) if c.offset==1000.0));
}

#[test]
fn stage4_drawing_carrier_offers_overlapping_wires_and_requires_confirmation() {
    for three_d in [false, true] {
        let mut h = app("wire {\"points\":[[0,0],[3000,0]],\"offset\":3000}\nwire {\"points\":[[0,0],[3000,0]],\"offset\":3000}");
        let level = h.state().plan_level().unwrap();
        let wires = ids(&h, "wire");
        if three_d {
            press(&mut h, "Default 3D View");
        }
        h.state_mut().set_tool(Tool::Run("cablecarrier", Some(P2(0.0, 0.0))));
        h.state_mut().beam_offset = "3000".into();
        h.state_mut().run_justify = bimcraft_core::model::Justify::Centre;
        h.state_mut().place_run(level, P2(3000.0, 0.0));
        h.run();
        assert!(!h.state().status.error, "{}", h.state().status.text);
        let carrier = ids(&h, "cablecarrier")[0];
        for wire in &wires {
            assert!(h.state().session.model.elements.get(*wire).unwrap().run().unwrap().carrier.is_none());
        }
        press(&mut h, &format!("Wire {}", wires[1]));
        press(&mut h, "Attach Selected Wire");
        let model = &h.state().session.model;
        assert!(model.elements.get(wires[0]).unwrap().run().unwrap().carrier.is_none());
        assert_eq!(model.elements.get(wires[1]).unwrap().run().unwrap().carrier, Some(model.guid(carrier)));
        h.state_mut().selection = vec![carrier];
        h.state_mut().act(crate::context::Action::AttachExistingWire);
        h.run();
        press(&mut h, &format!("Wire {}", wires[0]));
        press(&mut h, "Attach Selected Wire");
        let model = &h.state().session.model;
        let guids: Vec<_> = wires.iter().map(|id| model.guid(*id)).collect();
        for wire in &wires {
            assert_eq!(model.elements.get(*wire).unwrap().run().unwrap().carrier, Some(model.guid(carrier)));
        }
        h.state_mut().session.execute("modify", &json!({"id":carrier,"offset":3500})).unwrap();
        h.run();
        for (wire, guid) in wires.iter().zip(guids) {
            let model = &h.state().session.model;
            assert_eq!(model.guid(*wire), guid);
            let geometry = model.run_geom(model.elements.get(*wire).unwrap()).unwrap();
            assert_eq!(geometry.a[2], 3500.0);
        }
    }
}

#[test]
fn stage4_connect_equipment_requires_explicit_point_in_plan_and_3d() {
    for three_d in [false, true] {
        let mut h = app(
            "wire {\"points\":[[0,0],[3000,0]],\"offset\":3000}\ncomponent {\"family\":\"Fire Alarm Panel\",\"at\":[0,0],\"offset\":1000}\ncable_carrier {\"points\":[[0,300],[3000,300]],\"offset\":3200}",
        );
        if three_d {
            press(&mut h, "Default 3D View");
        }
        let wire = ids(&h, "wire")[0];
        let device = ids(&h, "component")[0];
        h.state_mut().selection = vec![wire, device];
        h.run();
        let action = h
            .state()
            .context_items()
            .into_iter()
            .flatten()
            .find(|item| item.action.label() == "Connect Equipment to Wire")
            .expect("one main wire and one device offer a physical connection");
        assert!(action.enabled);
        h.state_mut().act(action.action);
        h.run();
        assert_eq!(ids(&h, "wire").len(), 1, "opening the point editor does not connect implicitly");
        press(&mut h, "Connect");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        assert_eq!(ids(&h, "wire").len(), 2);
        h.state_mut().act(action.action);
        h.run();
        press(&mut h, "Second end");
        let carrier = ids(&h, "cablecarrier")[0];
        let route = format!("{carrier}: {}", h.state().session.model.elements.get(carrier).unwrap().run().unwrap().type_name);
        choose(&mut h, "Route carrier", &route);
        pick(&mut h, Role::CheckBox, "Create carrier branches");
        press(&mut h, "Connect");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        assert_eq!(ids(&h, "wire").len(), 4);
        let connections = &h.state().session.model.elements.get(wire).unwrap().run().unwrap().connections;
        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0].end, 0);
        assert_eq!(connections[1].end, 1);
        assert_eq!(connections[0].carrier, None);
        assert_eq!(connections[1].carrier, Some(h.state().session.model.guid(carrier)));
        assert_eq!(connections[1].landing, Some([0.0, 300.0, 3200.0]));
    }
}

#[test]
fn stage4_selected_wire_offers_carrier_creation_in_plan_and_3d() {
    for three_d in [false, true] {
        let mut h = app("wire {\"points\":[[0,0],[3000,0]],\"offset\":2100,\"end_offset\":2700}");
        if three_d {
            press(&mut h, "Default 3D View");
        }
        let wire = ids(&h, "wire")[0];
        h.state_mut().selection = vec![wire];
        h.run();
        let item = h
            .state()
            .context_items()
            .into_iter()
            .flatten()
            .find(|item| item.action.label() == "Create Carrier from Wire")
            .expect("a selected wire offers carrier creation in either view");
        assert!(item.enabled);
        h.state_mut().act(item.action);
        h.run();
        assert!(ids(&h, "cablecarrier").is_empty(), "section placement requires confirmation");
        press(&mut h, "Bottom");
        type_role(&mut h, Role::TextInput, "Lateral Offset:", "10");
        press(&mut h, "Create Carrier");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        let carriers = ids(&h, "cablecarrier");
        assert_eq!(carriers.len(), 1);
        let linked =
            h.state().context_items().into_iter().flatten().find(|item| item.action.label() == "Create Carrier from Wire").unwrap();
        assert!(!linked.enabled, "a linked wire cannot create another carrier");
        let model = &h.state().session.model;
        assert_eq!(model.elements.get(wire).unwrap().run().unwrap().carrier, Some(model.guid(carriers[0])));
        assert_eq!(model.elements.get(wire).unwrap().run().unwrap().local_offset, 10.0);
        let wire_geometry = model.run_geom(model.elements.get(wire).unwrap()).unwrap();
        let carrier_geometry = model.run_geom(model.elements.get(carriers[0]).unwrap()).unwrap();
        assert_eq!(wire_geometry.a[2], 2100.0);
        assert_eq!(wire_geometry.b[2], 2700.0);
        assert!(carrier_geometry.a[2] > wire_geometry.a[2], "bottom placement raises the carrier axis around the fixed wire");
        let (wire_guid, carrier_guid) = (model.guid(wire), model.guid(carriers[0]));
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("carrier-from-wire.bimcraft");
        h.state_mut().session.execute("save", &json!({"path": path})).unwrap();
        let mut reopened = bimcraft_core::engine::Session::new();
        reopened.execute("open", &json!({"path": path})).unwrap();
        assert_eq!(reopened.model.elements.get(wire_guid).unwrap().run().unwrap().carrier, Some(carrier_guid));
        assert_eq!(reopened.model.elements.get(carrier_guid).unwrap().category(), "cablecarrier");
        h.state_mut().selection = vec![wire, carriers[0]];
        assert!(!h.state().context_items().into_iter().flatten().any(|item| item.action.label() == "Create Carrier from Wire"));
    }
}

#[test]
fn stage4_properties_edit_multiple_device_connection_sides() {
    let mut h = app(BOX);
    h.state_mut().set_tool(Tool::Fixture("smokedetector"));
    h.state_mut().component_offset = "2500".into();
    h.run();
    stage4_click_world(&mut h, [3000.0, 2000.0, 2500.0], false);
    let device = ids(&h, "component")[0];
    h.state_mut().set_tool(Tool::Modify);
    h.state_mut().selection = vec![device];
    h.run();
    pick(&mut h, Role::CheckBox, "Top cable entry");
    assert!(!h.state().status.error, "{}", h.state().status.text);
    pick(&mut h, Role::CheckBox, "Right cable entry");
    assert!(!h.state().status.error, "{}", h.state().status.text);
    let Some(bimcraft_core::model::Element::Component(component)) = h.state().session.model.elements.get(device) else {
        panic!("expected equipment");
    };
    use bimcraft_core::model::DeviceSide;
    assert_eq!(component.connection_sides, [DeviceSide::Top, DeviceSide::Right]);
}

#[test]
fn stage4_connect_equipment_to_wall_in_plan_and_3d() {
    for three_d in [false, true] {
        let mut h = app(r#"wall {"points":[[0,0],[6000,0]],"height":6000}
component {"family":"Fire Alarm Panel","type":"300 x 400","at":[1000,-100],"connection_sides":["top"]}
wire {"points":[[1040,-102],[1040,-102]],"offset":3000,"end_offset":5000}
cable_carrier {"points":[[1040,-300],[1040,-300]],"offset":3000,"end_offset":5000,"type":"Tray 100 x 50"}"#);
        if three_d {
            press(&mut h, "Default 3D View");
        }
        let wall = ids(&h, "wall")[0];
        let wire = ids(&h, "wire")[0];
        let carrier = ids(&h, "cablecarrier")[0];
        h.state_mut().session.execute("run_mount", &json!({"id":carrier,"host":wall,"face":"interior"})).unwrap();
        h.state_mut().selection = vec![wire, ids(&h, "component")[0]];
        h.state_mut().act(crate::context::Action::ConnectEquipmentToWire);
        h.run();
        choose(&mut h, "Mounting surface", &format!("Wall {wall}"));
        choose(&mut h, "Route carrier", &format!("{carrier}: Tray 100 x 50"));
        type_role(&mut h, Role::TextInput, "Minimum bend radius", "100.4");
        press(&mut h, "Connect");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        let model = &h.state().session.model;
        let connection = &model.elements.get(wire).unwrap().run().unwrap().connections[0];
        assert_eq!(connection.surface.as_ref().unwrap().host, Some(model.guid(wall)));
        assert_eq!(connection.carrier, Some(model.guid(carrier)));
        assert_eq!(connection.min_bend_radius, Some(100.4));
        h.state_mut().act(crate::context::Action::ConnectEquipmentToWire);
        h.run();
        press(&mut h, "Connect");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        let model = &h.state().session.model;
        assert_eq!(model.elements.get(wire).unwrap().run().unwrap().connections[0].min_bend_radius, Some(100.4));
        h.state_mut().session.execute("delete", &json!({"ids":[wall]})).unwrap();
        let before = h.state().session.model.to_json().unwrap();
        h.state_mut().act(crate::context::Action::ConnectEquipmentToWire);
        h.run();
        assert!(h.query_by_label("The mounting wall was deleted. Select a new wall or a fixed point to reconnect.").is_some());
        press(&mut h, "Cancel");
        assert_eq!(h.state().session.model.to_json().unwrap(), before);
    }
}

#[test]
fn stage4_selected_carrier_offers_surface_mount_in_plan_and_3d() {
    for three_d in [false, true] {
        let mut h = app("wall {\"points\":[[0,0],[5000,0]]}\ncable_carrier {\"points\":[[500,-400],[3500,-400]],\"offset\":2000}");
        if three_d {
            press(&mut h, "Default 3D View");
        }
        h.state_mut().selection = ids(&h, "cablecarrier");
        assert_eq!(h.state().selection.len(), 1);
        h.run();
        assert!(h.state().context_items().into_iter().flatten().any(|item| { item.enabled && item.action.label() == "Mount to Surface" }));
        let carrier = h.state().selection[0];
        let wall = ids(&h, "wall")[0];
        h.state_mut().act(crate::context::Action::MountToSurface);
        h.run();
        press(&mut h, "Interior");
        assert!(!h.state().status.error, "{}", h.state().status.text);
        let model = &h.state().session.model;
        let mount = model.elements.get(carrier).unwrap().run().unwrap().mount.as_ref().unwrap();
        assert_eq!(mount.host, model.guid(wall));
        assert_eq!(mount.face, "interior");
    }
}

#[test]
fn stage4_shift_drag_mounts_carrier_on_plan_wall() {
    let mut h = app("wall {\"points\":[[0,0],[5000,0]]}\ncable_carrier {\"points\":[[500,-800],[3500,-800]],\"offset\":2000}");
    let carrier = ids(&h, "cablecarrier")[0];
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![carrier];
    h.run();
    let from = h.state().screen(P2(1800.0, -800.0));
    let to = h.state().screen(P2(1800.0, -75.0));
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::SHIFT });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(from + (to - from) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT });
    h.run();
    let model = &h.state().session.model;
    let mount = model.elements.get(carrier).unwrap().run().unwrap().mount.as_ref().expect("Shift-drag mounts the selected carrier");
    assert_eq!(mount.host, model.guid(wall));
    assert_eq!(mount.face, "interior");
}

#[test]
fn stage4_shift_drag_mounts_carrier_in_3d_without_orbiting() {
    let mut h = app("wall {\"points\":[[0,0],[5000,0]]}\ncable_carrier {\"points\":[[500,-800],[3500,-800]],\"offset\":2000}");
    press(&mut h, "Default 3D View");
    let carrier = ids(&h, "cablecarrier")[0];
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![carrier];
    h.run();
    let camera = h.state().orbit.camera;
    let from = h.state().screen3([1800.0, -800.0, 2000.0]);
    let to = h.state().screen3([2600.0, -100.0, 2300.0]);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::SHIFT });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(from + (to - from) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT });
    h.run();
    let model = &h.state().session.model;
    let mount = model.elements.get(carrier).unwrap().run().unwrap().mount.as_ref().expect("Shift-drag mounts the selected carrier");
    assert_eq!(mount.host, model.guid(wall));
    assert_eq!(mount.face, "interior");
    assert_eq!(h.state().orbit.camera, camera);
}

#[test]
fn stage4_shift_drag_mounts_carrier_on_plan_floor() {
    let mut h = app(
        "floor {\"boundary\":[[0,0],[5000,0],[5000,3000],[0,3000]]}\ncable_carrier {\"points\":[[500,800],[3500,800]],\"offset\":2000}",
    );
    h.state_mut()
        .session
        .execute(
            "floor",
            &json!({
                "level": 100001,
                "boundary": [[0,0],[5000,0],[5000,3000],[0,3000]]
            }),
        )
        .unwrap();
    let carrier = ids(&h, "cablecarrier")[0];
    let floor = ids(&h, "floor")[0];
    h.state_mut().selection = vec![carrier];
    h.run();
    let from = h.state().screen(P2(1800.0, 800.0));
    let to = h.state().screen(P2(1800.0, 1500.0));
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::SHIFT });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(from + (to - from) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT });
    h.run();
    let model = &h.state().session.model;
    let mount = model.elements.get(carrier).unwrap().run().unwrap().mount.as_ref().expect("Shift-drag mounts the selected carrier");
    assert_eq!(mount.host, model.guid(floor));
    assert_eq!(mount.face, "top");
}

#[test]
fn stage41_wire_chain_starts_at_equipment_height() {
    for (three_d, manual) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut h = app(r#"floor {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]]}"#);
        let level = h.state().plan_level().unwrap();
        h.state_mut().run("modify", json!({"id":level,"view_range":{"top":3000,"cut":2300,"bottom":0,"depth":0}})).unwrap();
        h.state_mut().run("component", json!({"family":"Fire Alarm Panel","at":[1000,1000],"offset":200})).unwrap();
        if three_d {
            press(&mut h, "Default 3D View");
        }
        shortcut(&mut h, "EW");
        h.state_mut().beam_offset = "3000".into();
        h.state_mut().run_justify = bimcraft_core::model::Justify::Centre;
        h.state_mut().chain = true;
        h.run();
        stage4_click_world(&mut h, [1000.0, 950.0, 1850.0], three_d);
        assert_eq!(h.state().beam_offset, "1700.000 mm", "3D view: {three_d}");
        let height = if manual {
            h.state_mut().beam_offset = "2200".into();
            2200.0
        } else {
            1700.0
        };
        for point in [[3000.0, 1000.0, height], [5000.0, 1000.0, height]] {
            stage4_click_world(&mut h, point, three_d);
        }
        let wires = ids(&h, "wire");
        assert_eq!(wires.len(), 2);
        let model = &h.state().session.model;
        for id in wires {
            let g = model.run_geom(model.elements.get(id).unwrap()).unwrap();
            assert_eq!([g.a[2], g.b[2]], [height; 2]);
        }
    }
}

#[test]
fn stage41_split_places_a_route_bend_and_undoes_in_one_step() {
    for (command, category, plane) in [
        ("wire", "wire", None),
        ("cable_carrier", "cablecarrier", None),
        ("wire", "wire", Some(2)),
        ("cable_carrier", "cablecarrier", Some(2)),
        ("wire", "wire", Some(1)),
    ] {
        let three_d = plane.is_some();
        let mut h = app(&format!(
            r#"floor {{"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]]}}
{command} {{"points":[[1000,1000],[5000,1000]],"offset":1000,"justify":"centre"}}"#
        ));
        if let Some(axis) = plane {
            press(&mut h, "Default 3D View");
            h.state_mut().work_axis = axis;
        }
        let before = h.state().session.model.to_json().unwrap();
        shortcut(&mut h, "SL");
        stage4_click_world(&mut h, [3000.0, 1000.0, 1000.0], three_d);
        assert_eq!(ids(&h, category).len(), 1);
        assert!(
            matches!(h.state().tool, Tool::SplitRoute(..) | Tool::SplitRoute3D { .. }),
            "{category}, 3D={three_d}: {:?}",
            h.state().tool
        );
        let target = if plane == Some(1) { [3000.0, 1000.0, 2000.0] } else { [3000.0, 2000.0, 1000.0] };
        stage4_click_world(&mut h, target, three_d);
        let pieces = ids(&h, category);
        assert_eq!(pieces.len(), 2, "{category}, 3D={three_d}: {}", h.state().status.text);
        let m = &h.state().session.model;
        let joint = m.run_geom(m.elements.get(pieces[0]).unwrap()).unwrap().b;
        assert_eq!(joint, m.run_geom(m.elements.get(pieces[1]).unwrap()).unwrap().a);
        assert!(joint.into_iter().zip(target).all(|(actual, expected)| (actual - expected).abs() < 20.0), "{joint:?} != {target:?}");
        let after = m.to_json().unwrap();
        h.state_mut().run("undo", json!({})).unwrap();
        assert_eq!(h.state().session.model.to_json().unwrap(), before);
        h.state_mut().run("redo", json!({})).unwrap();
        assert_eq!(h.state().session.model.to_json().unwrap(), after);
    }
}

#[test]
fn stage41_connected_carrier_move_dialog_commits_only_the_chosen_edit() {
    for (three_d, choice) in
        [false, true].into_iter().flat_map(|view| ["Cancel", "Keep Connection", "Disconnect Equipment"].map(|choice| (view, choice)))
    {
        let mut h = app(r#"wire {"points":[[0,0],[3000,0]],"offset":3000}"#);
        let wire = ids(&h, "wire")[0];
        let carrier =
            h.state_mut().run("cable_carrier", json!({"from_wire":wire,"type":"Tray 100 x 50"})).unwrap()["ids"][0].as_u64().unwrap();
        let device =
            h.state_mut().run("component", json!({"family":"Fire Alarm Panel","at":[0,0],"offset":1000})).unwrap()["id"].as_u64().unwrap();
        h.state_mut().run("wire_connect", json!({"wire":wire,"device":device,"point":[0,0,0],"in_carrier":true})).unwrap();
        let before = h.state().session.model.to_json().unwrap();
        let before_device = h.state().session.model.elements.get(device).unwrap().clone();
        if three_d {
            press(&mut h, "Default 3D View");
        }
        h.state_mut().selection = vec![carrier];
        shortcut(&mut h, "MV");
        stage4_click_world(&mut h, [1500.0, 0.0, 3000.0], three_d);
        stage4_click_world(&mut h, [1500.0, 300.0, 3000.0], three_d);
        h.run();
        assert_eq!(h.state().session.model.to_json().unwrap(), before);
        press(&mut h, choice);
        let m = &h.state().session.model;
        if choice == "Cancel" {
            assert_eq!(m.to_json().unwrap(), before);
        } else {
            assert!(!h.state().status.error, "{}", h.state().status.text);
            let run = m.elements.get(wire).unwrap().run().unwrap();
            assert!((run.start.1 - 300.0).abs() < 15.0, "3D={three_d}: {}", run.start.1);
            assert_eq!(run.connections.is_empty(), choice == "Disconnect Equipment");
            assert_eq!(m.elements.get(device), Some(&before_device));
            let after = m.to_json().unwrap();
            let (opened, left_out) = bimcraft_core::model::Model::read(&after).unwrap();
            assert!(left_out.is_empty());
            for id in [wire, carrier] {
                let expected = m.elements.get(id).unwrap();
                let actual = opened.elements.get(id).unwrap();
                assert_eq!(opened.guid(id), m.guid(id));
                assert_eq!(actual.run().unwrap().carrier, expected.run().unwrap().carrier);
                assert_eq!(actual.run().unwrap().connections, expected.run().unwrap().connections);
                let actual = opened.run_geom(actual).unwrap();
                let expected = m.run_geom(expected).unwrap();
                for (a, b) in actual.a.into_iter().chain(actual.b).zip(expected.a.into_iter().chain(expected.b)) {
                    assert!((a - b).abs() < 1e-6);
                }
            }
            h.state_mut().run("undo", json!({})).unwrap();
            assert_eq!(h.state().session.model.to_json().unwrap(), before);
            h.state_mut().run("redo", json!({})).unwrap();
            assert_eq!(h.state().session.model.to_json().unwrap(), after);
        }
    }
}

#[test]
fn stage41_3d_move_raises_a_run_in_each_vertical_work_plane() {
    for axis in [0, 1] {
        let mut h = app(r#"wire {"points":[[0,0],[3000,0]],"offset":3000,"justify":"centre"}"#);
        let wire = ids(&h, "wire")[0];
        press(&mut h, "Default 3D View");
        h.state_mut().selection = vec![wire];
        h.state_mut().work_axis = axis;
        shortcut(&mut h, "MV");
        stage4_click_world(&mut h, [1500.0, 0.0, 3000.0], true);
        stage4_click_world(&mut h, [1500.0, 0.0, 3200.0], true);
        let model = &h.state().session.model;
        let geometry = model.run_geom(model.elements.get(wire).unwrap()).unwrap();
        assert!((geometry.a[2] - 3200.0).abs() < 15.0, "axis {axis}: {:?}", geometry.a);
    }
}

#[test]
fn stage41_3d_places_equipment_on_visible_wall_face() {
    let mut h = app(r#"wall {"points":[[0,0],[8000,0]],"height":3000}"#);
    press(&mut h, "Default 3D View");
    shortcut(&mut h, "FA");
    press(&mut h, "Place on Face");
    let before = h.state().session.model.to_json().unwrap();
    stage4_click_world(&mut h, [4000.0, -100.0, 1800.0], true);
    let device = ids(&h, "component")[0];
    let model = &h.state().session.model;
    let Some(Element::Component(component)) = model.elements.get(device) else { panic!("expected wall equipment") };
    assert!((component.at.1 + 100.0).abs() < 1.0);
    assert!((component.offset - 300.0).abs() < 15.0);
    assert!(component.angle.abs() < 0.01);
    let after = model.to_json().unwrap();
    let (opened, left_out) = bimcraft_core::model::Model::read(&after).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(opened.guid(device), model.guid(device));
    let Some(Element::Component(saved)) = opened.elements.get(device) else { panic!("expected saved equipment") };
    assert!((saved.at - component.at).length() < 1e-6);
    assert!((saved.offset - component.offset).abs() < 1e-6);
    assert!((saved.angle - component.angle).abs() < 1e-6);
    h.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    h.state_mut().run("redo", json!({})).unwrap();
    assert_eq!(h.state().session.model.to_json().unwrap(), after);
}

#[test]
fn stage41_3d_places_equipment_on_floor_top() {
    let mut h = app(r#"floor {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]]}"#);
    press(&mut h, "Default 3D View");
    shortcut(&mut h, "FA");
    press(&mut h, "Place on Face");
    stage4_click_world(&mut h, [4000.0, 3000.0, 0.0], true);
    let model = &h.state().session.model;
    let device = ids(&h, "component")[0];
    let Some(Element::Component(component)) = model.elements.get(device) else { panic!("expected floor equipment") };
    let bottom = model.component_local_pieces(component).iter().map(|piece| piece.bottom).min_by(f64::total_cmp).unwrap();
    assert!((component.at.0 - 4000.0).abs() < 20.0);
    assert!((component.at.1 - 3000.0).abs() < 20.0);
    assert!((model.elevation(component.level) + component.offset + bottom).abs() < 1e-6);
}

#[test]
fn stage41_3d_places_equipment_under_ceiling() {
    let mut h = app(r#"ceiling {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]],"offset":2600}"#);
    press(&mut h, "Default 3D View");
    let state = h.state_mut();
    let camera = state.orbit.camera.unwrap();
    let scene = bimcraft_core::view3d::Scene::new(&state.session.model);
    state.orbit.camera = Some(scene.fit(camera.azimuth, -35.0, camera.width, camera.height));
    h.run();
    shortcut(&mut h, "FA");
    press(&mut h, "Place on Face");
    stage4_click_world(&mut h, [4000.0, 3000.0, 2600.0], true);
    let model = &h.state().session.model;
    let device = ids(&h, "component")[0];
    let Some(Element::Component(component)) = model.elements.get(device) else { panic!("expected ceiling equipment") };
    let top = model.component_local_pieces(component).iter().map(|piece| piece.top).max_by(f64::total_cmp).unwrap();
    assert!((component.at.0 - 4000.0).abs() < 20.0);
    assert!((component.at.1 - 3000.0).abs() < 20.0);
    assert!((model.elevation(component.level) + component.offset + top - 2600.0).abs() < 1e-6);
}

#[test]
fn stage41_3d_draws_a_wire_in_each_work_plane() {
    for (axis, ortho) in [(0, false), (1, false), (0, true), (1, true), (2, true)] {
        let mut h = app(r#"floor {"boundary":[[0,0],[8000,0],[8000,6000],[0,6000]]}"#);
        press(&mut h, "Default 3D View");
        shortcut(&mut h, "EW");
        h.state_mut().work_axis = axis;
        h.state_mut().work_coordinate = "1000".into();
        h.state_mut().beam_offset = "1000".into();
        h.state_mut().constrain = ortho;
        h.state_mut().run_justify = bimcraft_core::model::Justify::Centre;
        h.run();
        let a = [1000.0, 1000.0, 1000.0];
        let mut b = [1000.0, 1000.0, 2000.0];
        if axis == 2 {
            b = [3000.0, 2000.0, 1000.0];
        } else {
            b[1 - axis] = 3000.0;
        }
        stage4_click_world(&mut h, a, true);
        assert!(ids(&h, "wire").is_empty());
        let clicked = b;
        if ortho {
            b[if axis == 2 { 1 } else { 2 }] = 1000.0;
        }
        let at = h.state().screen3(clicked);
        h.hover_at(at);
        h.run();
        let expected = [h.state().screen3(a), h.state().screen3(b)];
        assert!(
            h.output().shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::LineSegment { points, stroke }
                    if (stroke.width - 1.5).abs() < f32::EPSILON
                        && points.iter().zip(expected).all(|(actual, expected)| actual.distance(expected) < 2.0)
            )),
            "missing route preview on axis {axis}, Ortho={ortho}"
        );
        assert!(ids(&h, "wire").is_empty());
        stage4_click_world(&mut h, clicked, true);
        let wire = ids(&h, "wire")[0];
        let m = &h.state().session.model;
        let g = m.run_geom(m.elements.get(wire).unwrap()).unwrap();
        for (actual, expected) in g.a.into_iter().chain(g.b).zip(a.into_iter().chain(b)) {
            assert!((actual - expected).abs() < 20.0, "axis {axis}: {actual} != {expected}");
        }
    }
}
