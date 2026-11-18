//! External evaluator integration: opt in with synthetic fixtures and the pinned vanilla CLI.
use super::*;
use bimcraft_core::family::Family;
use std::collections::BTreeMap;
use std::path::Path;

fn capture(h: &mut Harness<BimApp>, name: &str) {
    let Ok(folder) = std::env::var("SOURCE_EDITOR_CAPTURES") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    h.run();
    h.render().unwrap().save(Path::new(&folder).join(format!("{name}.png"))).unwrap();
}

fn family(h: &Harness<BimApp>) -> Family {
    h.state().active_family().unwrap().draft_family().unwrap()
}
fn answer(h: &mut Harness<BimApp>, path: &Path) {
    crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("save dialog").send(Some(vec![path.to_path_buf()])).unwrap();
    h.run();
}
fn same_source(a: &Family, b: &Family) {
    assert_eq!(a.solidcraft, b.solidcraft);
    assert_eq!(a.dimension_parameters, b.dimension_parameters);
    assert_eq!(a.nested, b.nested);
    assert_eq!(a.nested_instances, b.nested_instances);
    for (a, b) in a.parameters.iter().zip(&b.parameters) {
        assert_eq!((&a.name, a.kind, a.instance, &a.group), (&b.name, b.kind, b.instance, &b.group));
    }
}
fn save(h: &mut Harness<BimApp>, path: &Path) {
    h.state_mut().family_save(true);
    h.run();
    answer(h, path);
    assert!(path.exists(), "{}", h.state().status.text);
}
fn checkpoint(h: &mut Harness<BimApp>, root: &Path, name: &str) {
    h.state_mut().session.execute("save", &json!({"path":root.join(format!("{name}.bimcraft"))})).unwrap();
}

#[test]
#[ignore = "requires SOURCE_EDITOR_FIXTURES and BIMCRAFT_SOLIDCRAFT_CLI (vanilla 0.44.1)"]
fn source_editor_native_controls_roundtrip() {
    let root = std::path::PathBuf::from(std::env::var("SOURCE_EDITOR_FIXTURES").unwrap());
    let cases: Value = serde_json::from_slice(&std::fs::read(root.join("cases.json")).unwrap()).unwrap();
    for case in cases.as_array().unwrap() {
        let key = case["key"].as_str().unwrap();
        let name = case["name"].as_str().unwrap();
        let param = case["parameter"].as_str().unwrap();
        let changed = case["changed"].as_str().unwrap();
        let mut h = app("");
        h.set_size(vec2(1800., 1100.));
        h.state_mut().session.execute("open", &json!({"path":root.join(format!("{key}.bimcraft"))})).unwrap();
        if key == "cabinet" {
            h.state_mut()
                .session
                .execute("family", &json!({"name":name,"dimension_parameters":{"Width":"Width","Height":"Height","Depth":"Depth"}}))
                .unwrap();
        }
        h.run();
        checkpoint(&mut h, &root, &format!("{key}-initial"));
        let original = h.state().session.model.families[name].clone();
        let original_value = original.values("Standard", &BTreeMap::new()).ok().and_then(|v| v.get(param).cloned());
        let was = h.state().session.model.to_json().unwrap();
        h.state_mut().edit_family(name);
        h.run();
        same_source(&family(&h), &original);
        capture(&mut h, &format!("{key}-edit-family"));
        assert!(h.state().active_family().unwrap().problem().is_none(), "{:?}", h.state().active_family().unwrap().problem());
        save(&mut h, &root.join(format!("{key}-unchanged.bimcf")));
        h.state_mut().family_save(false);
        h.run();
        let saved: Value = serde_json::from_slice(&std::fs::read(root.join(format!("{key}-unchanged.bimcf"))).unwrap()).unwrap();
        let saved: Family = serde_json::from_value({
            let mut s = saved;
            s.as_object_mut().unwrap().remove("name");
            s
        })
        .unwrap();
        same_source(&saved, &original);
        assert_eq!(h.state().session.model.to_json().unwrap(), was, "Save doesn't load into project");
        press(&mut h, "Family Types");
        type_into(&mut h, &format!("Parameter Value {param}"), changed);
        capture(&mut h, &format!("{key}-parameters"));
        press(&mut h, "OK");
        let edited = family(&h);
        same_source(&edited, &original);
        press(&mut h, "Undo");
        same_source(&family(&h), &original);
        press(&mut h, "Redo");
        assert_eq!(family(&h), edited);
        save(&mut h, &root.join(format!("{key}-edited.bimcf")));
        press(&mut h, "Load into Project");
        assert!(h.query_by_label("Family Already Exists").is_some());
        capture(&mut h, &format!("{key}-reload-dialog"));
        press(&mut h, "Cancel");
        assert_eq!(h.state().session.model.to_json().unwrap(), was);
        press(&mut h, "Load into Project");
        press(&mut h, "Overwrite the existing version");
        let kept = &h.state().session.model.families[name];
        same_source(kept, &original);
        if let Some(v) = original_value {
            assert_eq!(kept.values("Standard", &BTreeMap::new()).unwrap()[param], v);
        }
        checkpoint(&mut h, &root, &format!("{key}-version"));
        press(&mut h, "Load into Project");
        press(&mut h, "Overwrite the existing version and its parameter values");
        same_source(&h.state().session.model.families[name], &original);
        checkpoint(&mut h, &root, &format!("{key}-values"));
        let after = h.state().session.model.to_json().unwrap();
        press(&mut h, "Close Family");
        h.state_mut().session.execute("undo", &json!({})).unwrap();
        checkpoint(&mut h, &root, &format!("{key}-project-undo"));
        h.state_mut().session.execute("redo", &json!({})).unwrap();
        assert_eq!(h.state().session.model.to_json().unwrap(), after);
        checkpoint(&mut h, &root, &format!("{key}-project-redo"));
        h.state_mut().session.execute("open", &json!({"path":root.join(format!("{key}-values.bimcraft"))})).unwrap();
        same_source(&h.state().session.model.families[name], &original);
        // Open the standalone saved definition into an empty project, save unchanged, then load.
        let mut fresh = app("");
        fresh.set_size(vec2(1800., 1100.));
        fresh.state_mut().open_family_file(&root.join(format!("{key}-edited.bimcf")));
        fresh.run();
        same_source(&family(&fresh), &original);
        capture(&mut fresh, &format!("{key}-saved-reopened"));
        save(&mut fresh, &root.join(format!("{key}-fresh-save.bimcf")));
        fresh.state_mut().family_save(false);
        fresh.run();
        press(&mut fresh, "Load into Project and Close");
        if fresh.query_by_label("Family Already Exists").is_some() {
            press(&mut fresh, "Overwrite the existing version and its parameter values");
        }
        assert!(fresh.state().active_family().is_none(), "{}", fresh.state().status.text);
        same_source(&fresh.state().session.model.families[name], &original);
        checkpoint(&mut fresh, &root, &format!("{key}-fresh-project"));
        println!("PASS editor workflow {key}");
    }
}

#[test]
#[ignore = "requires SOURCE_EDITOR_FIXTURES and BIMCRAFT_SOLIDCRAFT_CLI (vanilla 0.44.1)"]
fn source_editor_refuses_bound_rename_delete_and_fresh_source_saves() {
    let root = std::path::PathBuf::from(std::env::var("SOURCE_EDITOR_FIXTURES").unwrap());
    for (key, param) in [("cabinet", "Width"), ("source-casing", "RO_Width")] {
        let mut h = app("");
        h.set_size(vec2(1800., 1100.));
        h.state_mut().open_family_file(&root.join(format!("{key}.json")));
        h.run();
        let original = family(&h);
        assert!(original.solidcraft.is_some());
        save(&mut h, &root.join(format!("{key}-source-fresh.bimcf")));
        press(&mut h, "Family Types");
        press(&mut h, &format!("{param} (default)"));
        press(&mut h, "Edit Parameter");
        type_into(&mut h, "Name:", "Renamed input");
        press_near(&mut h, "OK", "Name:");
        assert!(h.query_by_label_contains("cannot be renamed here").is_some());
        press_near(&mut h, "Cancel", "Name:");
        press(&mut h, "Delete Parameter");
        assert!(h.query_by_label_contains("cannot be deleted here").is_some());
        press(&mut h, "Cancel");
        same_source(&family(&h), &original);
        press(&mut h, "Load into Project and Close");
        assert!(h.state().active_family().is_none());
        println!("PASS source preservation and binding refusals {key}");
    }
}

#[test]
#[ignore = "requires SHARED_CONTROLS_FIXTURES and BIMCRAFT_SOLIDCRAFT_CLI (vanilla 0.44.1)"]
fn shared_cabinet_controls_in_native_properties() {
    let root = std::path::PathBuf::from(std::env::var("SHARED_CONTROLS_FIXTURES").unwrap());
    for layout in ["drawers", "two-door"] {
        let path = root.join(format!("{layout}-overlay-slab/Properties demo.bimcraft"));
        let mut h = app("");
        h.set_size(vec2(1800., 1400.));
        h.state_mut().session.execute("open", &json!({"path":path})).unwrap();
        h.run();
        h.state_mut().session.execute("crop", &json!({"view":"Front","region":[[0,-40],[900,1000]]})).unwrap();
        h.state_mut().session.execute("scale", &json!({"view":"Front","scale":10})).unwrap();
        browse(&mut h, "Front");
        h.state_mut().docking.left.retain(|s| !s.tabs.contains(&crate::dock::Palette::Browser));
        h.run();
        drag_screen(&mut h, pos2(298., 400.), pos2(460., 400.), PointerButton::Primary);
        h.state_mut().fit();
        h.run();
        let id = ids(&h, "component")[0];
        h.state_mut().selection = vec![id];
        h.run();
        capture(&mut h, &format!("{layout}-initial"));
        let param = if layout == "drawers" { "SDH_Base_drawer_count" } else { "SDH_Base_shelf_count" };
        if layout == "two-door" {
            h.get_by_role_and_label(Role::CheckBox, "SDH_Base_show_fronts").click();
            h.run();
        }
        for count in if layout == "drawers" { vec![1, 2, 3, 4] } else { vec![0, 1, 3, 6] } {
            type_into(&mut h, param, &count.to_string());
            key(&mut h, Key::Enter);
            assert!(!h.state().status.error, "{}", h.state().status.text);
            assert_eq!(field(&h, id, "values")[param].as_f64(), Some(count as f64));
            capture(&mut h, &format!("{layout}-count-{count}"));
        }
        if layout == "two-door" {
            h.get_by_role_and_label(Role::CheckBox, "SDH_Base_show_fronts").click();
            h.run();
        }
        for name in ["SDH_Base_use_knobs", "SDH_Base_show_hardware", "SDH_Base_show_fronts"] {
            let before = h.state().session.model.to_json().unwrap();
            h.get_by_role_and_label(Role::CheckBox, name).click();
            h.run();
            assert!(!h.state().status.error, "{}", h.state().status.text);
            let after = h.state().session.model.to_json().unwrap();
            assert_ne!(before, after);
            capture(&mut h, &format!("{layout}-{name}"));
            press(&mut h, "Undo");
            assert_eq!(h.state().session.model.to_json().unwrap(), before);
            press(&mut h, "Redo");
            assert_eq!(h.state().session.model.to_json().unwrap(), after);
            press(&mut h, "Undo");
        }
        for (name, value) in [("SDH_Base_plinth", "140"), ("SDH_Base_toe_kick_setback", "110"), ("SDH_Base_hardware_x_offset", "15")] {
            type_into(&mut h, name, value);
            key(&mut h, Key::Enter);
            assert!(!h.state().status.error, "{}", h.state().status.text);
            assert_eq!(field(&h, id, "values")[name], json!(value.parse::<f64>().unwrap()));
        }
        capture(&mut h, &format!("{layout}-positions"));
        let out = root.join(format!("{layout}-UI-edited.bimcraft"));
        h.state_mut().session.execute("save", &json!({"path":out})).unwrap();
        press(&mut h, "Edit Type");
        assert!(h.query_by_role_and_label(Role::TextInput, "SDH_Base_height").is_some());
        let dialog = h.ctx.memory(|m| m.area_rect(egui::Id::new("family-type-properties"))).unwrap();
        assert!(h.query_all_by_role_and_label(Role::TextInput, param).all(|n| !dialog.contains(n.rect().center())));
        capture(&mut h, &format!("{layout}-edit-type"));
        press(&mut h, "Cancel");
        println!("PASS native Properties {layout}");
    }
}

#[test]
#[ignore = "requires BRIDGE_SHOWCASE_FIXTURES and BIMCRAFT_SOLIDCRAFT_CLI (vanilla 0.44.1)"]
fn source_showcase_properties_and_editor_roundtrip() {
    let root = std::path::PathBuf::from(std::env::var("BRIDGE_SHOWCASE_FIXTURES").unwrap());
    for (fixture, category, parameter, changed) in
        [("Window", "window", "LiteCount", "5"), ("Door", "door", "LowerPanelHeight", "900"), ("Light", "component", "CandleCount", "6")]
    {
        let mut h = app("");
        h.set_size(vec2(1600., 1200.));
        h.state_mut().session.execute("open", &json!({"path":root.join(format!("{fixture}-before.bimcraft"))})).unwrap();
        h.run();
        h.state_mut().docking.left.retain(|s| !s.tabs.contains(&crate::dock::Palette::Browser));
        h.state_mut().open_sheet(Sheet::View3D);
        h.state_mut().fit();
        h.run();
        let id = ids(&h, category)[0];
        h.state_mut().selection = vec![id];
        h.run();
        let before = h.state().session.model.to_json().unwrap();
        type_into(&mut h, parameter, changed);
        key(&mut h, Key::Enter);
        assert!(!h.state().status.error, "{}", h.state().status.text);
        assert_eq!(field(&h, id, "values")[parameter].as_f64(), Some(changed.parse::<f64>().unwrap()));
        let after = h.state().session.model.to_json().unwrap();
        assert_ne!(before, after);
        capture(&mut h, &format!("{fixture}-properties"));
        press(&mut h, "Undo");
        assert_eq!(h.state().session.model.to_json().unwrap(), before);
        press(&mut h, "Redo");
        assert_eq!(h.state().session.model.to_json().unwrap(), after);
        let (name, original) =
            h.state().session.model.families.iter().find(|(_, f)| f.solidcraft.is_some()).map(|(n, f)| (n.clone(), f.clone())).unwrap();
        h.state_mut().edit_family(&name);
        h.run();
        same_source(&family(&h), &original);
        let file = root.join(format!("{fixture}-editor.bimf"));
        save(&mut h, &file);
        let mut saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        saved.as_object_mut().unwrap().remove("name");
        let saved: Family = serde_json::from_value(saved).unwrap();
        same_source(&saved, &original);
        capture(&mut h, &format!("{fixture}-edit-family"));
        if fixture == "Light" {
            press(&mut h, "Family Types");
            for (profile, name) in [(crate::style::Profile::Rev, "rev"), (crate::style::Profile::BimCraft, "bimcraft")] {
                h.state_mut().profile = profile;
                for (theme, light, look) in
                    [(egui::Theme::Light, false, "light"), (egui::Theme::Dark, false, "dark"), (egui::Theme::Dark, true, "dark-light")]
                {
                    h.ctx.set_theme(theme);
                    h.state_mut().light_canvas = light;
                    h.run();
                    capture(&mut h, &format!("source-types-{name}-{look}"));
                }
            }
        }
        println!("PASS {fixture}: native Properties/history and Edit Family/save preserve source");
    }
}
