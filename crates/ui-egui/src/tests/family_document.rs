//! The family editor occupies the window for conventional families too.
use super::*;

/// The app on `script` in a window wide enough for the family document's Create tab to show every tool: {3D}'s has a Work
/// Plane panel too, which at the harness's 1280 hands Family Editor's last tools over to its list.
fn wide(script: &str) -> Harness<'static, BimApp> {
    let mut h = app(script);
    h.set_size(vec2(1600.0, 800.0));
    h.run();
    h
}

fn associate(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    press(h, label);
}

#[test]
fn conventional_family_opens_as_a_document_in_both_profiles() {
    for profile in [crate::style::Profile::Rev, crate::style::Profile::BimCraft] {
        let mut h = harness("", 0.25, profile);
        h.set_size(vec2(1600.0, 800.0));
        h.state_mut().edit_family("Table");
        h.run();
        assert!(h.query_by_label("Load into Project").is_some(), "the family document owns the ribbon");
        assert!(h.query_by_label("Load into Project and Close").is_some());
        assert!(h.query_by_label("Family Category and Parameters").is_some());
        assert!(h.query_by_label("Family Types").is_some());
        assert!(h.query_by_label("Close Family").is_some());
    }
}

#[test]
fn closing_a_new_family_document_does_not_change_the_project() {
    let mut h = app("");
    let before = h.state().session.model.to_json().unwrap();
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    press(&mut h, "Close Family");
    assert!(h.state().family_editor.is_none());
    assert!(h.state().session.model.to_json().unwrap() == before);
}

#[test]
fn family_draft_history_never_undoes_the_project() {
    let mut h = wide(BOX);
    let before = h.state().session.model.to_json().unwrap();
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    type_into(&mut h, "Family name", "Draft");
    // Commit the text edit's transaction before asking for document Undo.
    h.get_by_label("Close Family").focus();
    h.run();
    press(&mut h, "Undo");
    assert!(h.query_all_by_label("New Family · {3D}").count() > 0);
    press(&mut h, "Redo");
    assert!(h.query_all_by_label("Draft · {3D}").count() > 0);
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    press(&mut h, "Load into Project and Close");
    assert!(h.state().session.model.families.contains_key("Draft"));
    assert!(h.state().family_editor.is_none());
}

#[test]
fn equals_associates_a_form_property_and_can_create_and_remove_a_parameter() {
    let mut h = wide("");
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    associate(&mut h, "Associate Family Parameter: Form To 1");
    assert!(h.query_by_label("Width").is_some());
    press(&mut h, "Height");
    press(&mut h, "OK");
    associate(&mut h, "Associate Family Parameter: Form To 1");
    press(&mut h, "New Parameter");
    type_into(&mut h, "Name:", "Overall height");
    press(&mut h, "OK");
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    let f = &h.state().session.model.families["New Family"];
    assert_eq!(f.forms[0].to, bimcraft_core::family::Expr::Formula("Overall height".into()));
    assert!(f.parameters.iter().any(|p| p.name == "Overall height" && p.kind == bimcraft_core::family::Kind::Length));
    associate(&mut h, "Associate Family Parameter: Form To 1");
    press(&mut h, "<None>");
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    // Loaded once already, the project has it otherwise now.
    press(&mut h, "Overwrite the existing version");
    assert_eq!(h.state().session.model.families["New Family"].forms[0].to, bimcraft_core::family::Expr::Number(750.));
}

#[test]
fn associations_filter_yesno_and_material_and_preserve_the_project_until_load() {
    let mut h = wide("");
    let material = h.state().session.model.materials.keys().next().unwrap().clone();
    assert!(h.state_mut().run("family",json!({"name":"Associations","parameters":[{"name":"Lit","kind":"yesno","value":true},{"name":"Finish","kind":"material","value":material},{"name":"Depth","value":100}],"types":{"A":{}},"forms":[{"profile":[[0,0],[100,0],[100,100],[0,100]],"to":100}]})).is_some());
    let before = h.state().session.model.to_json().unwrap();
    h.state_mut().edit_family("Associations");
    h.run();
    associate(&mut h, "Associate Family Parameter: Visible");
    assert!(h.query_by_label("Lit").is_some());
    assert!(h.query_by_label("Finish").is_none() && h.query_by_label("Depth").is_none());
    press(&mut h, "Lit");
    press(&mut h, "OK");
    associate(&mut h, "Associate Family Parameter: Form Material 1");
    assert!(h.query_by_label("Finish").is_some());
    assert_eq!(h.query_all_by_label("Lit").count(), 1, "only the Visible row's own, tied to it");
    press(&mut h, "Finish");
    press(&mut h, "OK");
    assert_eq!(h.get_by_label("Form Material 1").value().as_deref(), Some(material.as_str()));
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    press(&mut h, "Load into Project");
    press(&mut h, "Overwrite the existing version");
    let f = &h.state().session.model.families["Associations"];
    assert_eq!(f.forms[0].visible, Some(bimcraft_core::family::Expr::Formula("Lit".into())));
    assert_eq!(f.forms[0].material, "Finish");
}

#[test]
fn loading_an_edited_document_keeps_connectors_reference_geometry_and_symbols() {
    use bimcraft_core::family::{Face, PlaneRef, RefLine, Reference};
    use bimcraft_core::ids::Guid;
    let mut h = wide("");
    let mut f = h.state().session.model.families.values().find(|f| !f.connectors.is_empty()).unwrap().clone();
    let plane_guid = Guid::from_u128(0xdddddddd_dddd_4ddd_8ddd_dddddddddd11);
    let line_guid = Guid::from_u128(0xdddddddd_dddd_4ddd_8ddd_dddddddddd12);
    let mut plane = f.planes[0].clone();
    plane.guid = plane_guid;
    plane.id = Default::default();
    plane.name = "Offset".into();
    plane.start = [0., 0., 100.];
    plane.end = [100., 0., 100.];
    plane.up = [0., 1., 0.];
    plane.level = false;
    plane.defines_origin = false;
    plane.reference = Reference::Weak;
    f.merge_options(&json!({"work_plane_based":true,"always_vertical":false,"cut_with_voids_when_loaded":true,"shared":true,
        "room_calculation_point":[10,20,30],"cut_in_views":false,"omniclass":{"number":"23.30","title":"Preserved"}}))
        .unwrap();
    f.planes.push(plane);
    f.lines.push(RefLine {
        guid: line_guid,
        start: [0., 0., 100.],
        end: [100., 0., 100.],
        host: PlaneRef::Plane(plane_guid),
        ..Default::default()
    });
    f.forms[0].host = PlaneRef::Line(line_guid, Face::Vertical);
    let mut p = serde_json::to_value(&f).unwrap();
    let map = p.as_object_mut().unwrap();
    for (from, to) in [("planes", "ref_planes"), ("lines", "ref_lines")] {
        let value = map.remove(from).unwrap();
        map.insert(to.into(), value);
    }
    p["name"] = json!("Preserved");
    assert!(h.state_mut().run("family", p).is_some());
    let before = h.state().session.model.families["Preserved"].clone();
    h.state_mut().edit_family("Preserved");
    h.run();
    type_into(&mut h, "Family name", "Preserved renamed");
    press(&mut h, "Load into Project and Close");
    let after = &h.state().session.model.families["Preserved renamed"];
    assert_eq!(after.options_json(), before.options_json());
    assert_eq!(after.connectors, before.connectors);
    assert_eq!(after.planes, before.planes);
    assert_eq!(after.lines, before.lines);
    assert_eq!(after.forms, before.forms);
    assert_eq!(after.symbols, before.symbols);
    assert_eq!(after.parameters, before.parameters);
    h.state().session.model.check().unwrap();
}

#[test]
fn canceling_family_types_does_not_leave_changes_in_document_history() {
    let mut h = app("");
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    type_into(&mut h, "Family name", "Draft");
    press(&mut h, "Family Types");
    type_into(&mut h, "Parameter Value Width", "1200");
    press(&mut h, "Cancel");
    press(&mut h, "Undo");
    assert!(h.query_all_by_label("New Family · {3D}").count() > 0, "Cancel does not create a step that reintroduces canceled text");
}

#[derive(Clone, Default)]
struct PreviewUploads(Arc<std::sync::Mutex<Vec<(egui::TextureId, egui::Color32, bool)>>>);
impl egui_kittest::TestRenderer for PreviewUploads {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        for (id, parts) in &delta.set {
            for part in parts.iter().filter(|p| p.pos.is_none()) {
                let egui::ImageData::Color(image) = &part.image;
                if let Some(first) = image.pixels.first() {
                    let blue =
                        image.pixels.iter().any(|p| i32::from(p.b()) > i32::from(p.r()) + 40 && i32::from(p.b()) > i32::from(p.g()) + 20);
                    self.0.lock().unwrap().push((*id, *first, blue));
                }
            }
        }
        delta.clear();
    }

    fn render(&mut self, _: &egui::Context, _: &egui::FullOutput) -> Result<image::RgbaImage, String> {
        Err("this renderer only keeps uploads".into())
    }
}
impl PreviewUploads {
    fn take(&self, h: &Harness<BimApp>) -> Option<(egui::Color32, bool)> {
        std::mem::take(&mut *self.0.lock().unwrap())
            .into_iter()
            .rev()
            .find_map(|(id, c, blue)| h.ctx.tex_manager().read().meta(id).is_some_and(|m| m.name == "family-preview").then_some((c, blue)))
    }
}
fn family_preview(dark: bool, light_canvas: bool) -> (Harness<'static, BimApp>, PreviewUploads) {
    let mut state = BimApp::new(Session::new());
    state.light_canvas = light_canvas;
    state.theme_wanted = Some(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
    state.new_family(bimcraft_core::family::Category::Generic);
    let uploads = PreviewUploads::default();
    let mut h = Harness::builder()
        .with_size([1280., 800.])
        .renderer(uploads.clone())
        .build_ui_state(|ui, state: &mut BimApp| state.show(ui), state);
    h.run();
    (h, uploads)
}
#[test]
fn family_document_preview_uses_the_canvas_palette_and_redraws_when_it_changes() {
    let (mut h, uploads) = family_preview(true, true);
    assert_eq!(uploads.take(&h).map(|(c, _)| c), Some(crate::canvas::color(bimcraft_core::render::PAPER)));
    h.state_mut().light_canvas = false;
    h.run();
    assert_eq!(uploads.take(&h).map(|(c, _)| c), Some(crate::canvas::color(bimcraft_core::render::PAPER)));
}
#[test]
fn family_document_preview_does_not_draw_the_scratch_projects_levels() {
    let (h, uploads) = family_preview(false, false);
    assert_eq!(uploads.take(&h).map(|(_, blue)| blue), Some(false), "a generic box has no blue project-level marks");
}

#[test]
fn family_browser_scrolls_to_forms_in_a_short_window() {
    let mut h = app("");
    h.set_size(vec2(900., 640.));
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    press(&mut h, "Symbols");
    h.get_by_label("1. Extrusion").scroll_to_me();
    h.run();
    press(&mut h, "1. Extrusion");
    assert!(h.query_by_label("Extrusion Start").is_some(), "a form below the views can be reached and selected from the browser");
}

#[test]
fn a_symbols_rows_are_tied_through_the_dialog_adaptive_rows_and_form_visibility_share() {
    let mut h = app("");
    h.set_size(vec2(1600., 1000.));
    h.state_mut().new_family(bimcraft_core::family::Category::Generic);
    h.run();
    press(&mut h, "Symbols");
    press(&mut h, "Add Arc");
    // Its radius tied to Width: greyed, showing what Width comes to.
    associate(&mut h, "Associate Family Parameter: Symbol Radius 1");
    press(&mut h, "Width");
    press(&mut h, "OK");
    assert_eq!(h.get_by_label("Symbol Radius 1").value().as_deref(), Some("1000"));
    // Its Visible tied to a new Yes/No parameter, which starts from what the row came to: shown.
    associate(&mut h, "Associate Family Parameter: Visible");
    press(&mut h, "New Parameter");
    type_into(&mut h, "Name:", "Swing");
    press(&mut h, "OK");
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    let f = &h.state().session.model.families["New Family"];
    let arc = f.symbols[0].arc.as_ref().unwrap();
    assert_eq!(arc.radius, bimcraft_core::family::Expr::Formula("Width".into()));
    assert_eq!(f.symbols[0].visible, Some(bimcraft_core::family::Expr::Formula("Swing".into())));
    let swing = f.parameters.iter().find(|p| p.name == "Swing").unwrap();
    assert_eq!((swing.kind, &swing.value), (bimcraft_core::family::Kind::YesNo, &json!(true)));
}

#[test]
fn a_load_into_project_refused_stays_said_in_the_document_until_the_draft_changes() {
    let mut h = app("");
    h.set_size(vec2(1600., 1000.));
    let other = h.state().session.model.families.keys().find(|n| *n != "Table").unwrap().clone();
    h.state_mut().edit_family("Table");
    h.run();
    // Renamed onto another family's name: the project refuses it, and the document says so.
    type_into(&mut h, "Family name", &other);
    h.get_by_label("Close Family").focus();
    h.run();
    press(&mut h, "Load into Project");
    let said = |h: &Harness<BimApp>| h.query_all_by_label_contains("already").count();
    assert_eq!(said(&h), 1, "the refusal is said once, in the document");
    // The preview drawn again (its size changed) does not take it away.
    h.set_size(vec2(1500., 950.));
    for _ in 0..5 {
        h.run();
    }
    assert_eq!(said(&h), 1, "still said after the view is drawn again");
    assert!(h.state().session.model.families.contains_key("Table"), "the project's family is as it was");
    // Once the draft changes, what was refused is no longer what is there.
    type_into(&mut h, "Family name", "Dining Table");
    h.get_by_label("Close Family").focus();
    h.run();
    assert_eq!(said(&h), 0);
    press(&mut h, "Load into Project");
    assert!(h.state().session.model.families.contains_key("Dining Table"));
}

/// Every family, adaptive or not, opened as a document and loaded back is the family it was: T2's parameter metadata (group,
/// shared, reporting, `of`), F2's planes, lines and hosts, T5a's options with Default Elevation, T27's symbol subcategories,
/// connectors and an adaptive definition.
#[test]
fn every_family_comes_back_from_the_document_as_it_went_in() {
    use bimcraft_core::family::{Category, Family, Group, Kind, Parameter};
    let mut h = app("");
    h.set_size(vec2(1600., 1000.));
    let mut table = h.state().session.model.families["Table"].clone();
    table.parameters.extend([
        Parameter {
            name: "Fire Rating".into(),
            kind: Kind::Text,
            value: json!("EI 60"),
            group: Some(Group::IdentityData),
            shared: bimcraft_core::model::Guid::parse("6f1c2a3b-4d5e-4f60-8a71-92b3c4d5e6f7"),
            ..Parameter::default()
        },
        Parameter { name: "Reach".into(), reporting: true, value: json!(450.0), ..Parameter::default() },
        Parameter { name: "Leaf".into(), kind: Kind::FamilyType, value: Value::Null, of: Some(Category::Door), ..Parameter::default() },
    ]);
    table.merge_options(&json!({"work_plane_based": true, "always_vertical": false, "default_elevation": 900.0, "shared": true})).unwrap();
    let adaptive = Family {
        types: [("A".into(), Default::default())].into(),
        adaptive: Some(bimcraft_core::adaptive::four_point_panel()),
        ..Family::default()
    };
    let mut put = |name: &str, f: &Family| {
        let mut p = serde_json::to_value(f).unwrap();
        let map = p.as_object_mut().unwrap();
        Family::options_from_fields(map);
        map.insert("options".into(), f.options_json());
        for (from, to) in [("planes", "ref_planes"), ("lines", "ref_lines")] {
            let v = map.remove(from).unwrap_or(json!([]));
            map.insert(to.into(), v);
        }
        p["name"] = json!(name);
        h.state_mut().session.execute("family", &p).unwrap();
    };
    put("Table", &table);
    put("Adaptive panel", &adaptive);
    let names: Vec<String> = h.state().session.model.families.keys().cloned().collect();
    let mut seen = (false, false, false);
    for name in names {
        let was = h.state().session.model.families[&name].clone();
        seen.0 |= was.symbols.iter().any(|y| y.subcategory.is_some());
        seen.1 |= !was.connectors.is_empty();
        seen.2 |= !was.planes.is_empty();
        h.state_mut().edit_family(&name);
        h.run();
        press(&mut h, "Load into Project and Close");
        assert!(h.state().family_editor.is_none(), "{name}: loaded and closed");
        // The same numbers, whether a file wrote them whole (1200) or the editor as numbers (1200.0), and a formula that is a
        // number ("-40") as that number, as the editor's own round trip has them.
        fn alike(v: Value) -> Value {
            match v {
                Value::Number(n) => json!(n.as_f64()),
                Value::String(s) => s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).map_or(Value::String(s), |x| json!(x)),
                Value::Array(a) => Value::Array(a.into_iter().map(alike).collect()),
                Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, alike(v))).collect()),
                other => other,
            }
        }
        // Where two values first differ, as a path into them.
        fn first_diff(path: String, a: &Value, b: &Value) -> Option<String> {
            match (a, b) {
                (Value::Object(x), Value::Object(y)) => x
                    .keys()
                    .chain(y.keys())
                    .find_map(|k| first_diff(format!("{path}.{k}"), x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null))),
                (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                    x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_diff(format!("{path}[{i}]"), p, q))
                }
                _ => (a != b).then(|| format!("{path}: {a} against {b}")),
            }
        }
        let back = alike(serde_json::to_value(&h.state().session.model.families[&name]).unwrap());
        assert_eq!(first_diff(name.clone(), &back, &alike(serde_json::to_value(&was).unwrap())), None);
    }
    assert_eq!(seen, (true, true, true), "the library has subcategories, connectors and planes to keep");
    let back = &h.state().session.model.families["Table"];
    assert_eq!(back.options_json()["default_elevation"], json!(900.0));
    assert!(h.state().session.model.families["Adaptive panel"].adaptive.is_some());
}

/// The status bar's "Unsaved changes" is the active document's: with a family open, whether its draft has changes not yet
/// saved to a file or loaded into the project; with the project active, the project's, whatever the family was.
#[test]
fn the_unsaved_mark_is_the_active_documents() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = app("");
    h.set_size(vec2(1600., 1000.));
    let marked = |h: &Harness<BimApp>| h.query_by_label("Unsaved changes").is_some();
    let rename = |h: &mut Harness<BimApp>, name: &str| {
        type_into(h, "Family name", name);
        h.get_by_label("Close Family").focus();
        h.run();
    };
    assert!(!marked(&h), "a clean project");
    h.state_mut().edit_family("Table");
    h.run();
    assert!(!marked(&h), "a family just opened");
    rename(&mut h, "Dining Table");
    assert!(marked(&h), "the family edited");
    assert!(!h.state().dirty(), "the project is untouched");
    // Saved to a file: nothing left unsaved, though the project hasn't taken it.
    press(&mut h, "Insert");
    press(&mut h, "Save Family");
    let file = dir.path().join("Dining Table.json");
    let tx = crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).expect("a dialog was begun");
    tx.send(Some(vec![file.clone()])).unwrap();
    h.run();
    assert!(file.exists());
    assert!(!marked(&h), "the family saved");
    rename(&mut h, "Kitchen Table");
    assert!(marked(&h), "edited again");
    // Loaded into the project: the family has nothing unsaved, the project has.
    press(&mut h, "Load into Project");
    assert!(h.state().session.model.families.contains_key("Kitchen Table"));
    assert!(h.state().dirty());
    assert!(!marked(&h), "the family loaded");
    // Closed: the project's mark.
    press(&mut h, "Close Family");
    assert!(h.state().family_editor.is_none());
    assert!(marked(&h), "the project's changes");
    // Opened again on a changed project: the family's, which has none; closed, the project's again.
    h.state_mut().edit_family("Kitchen Table");
    h.run();
    assert!(!marked(&h), "a family just opened on a changed project");
    press(&mut h, "Close Family");
    assert!(marked(&h), "the project's changes, after the family");
}

/// F10: the Delete key takes away what is selected in the family document, as the ribbon's Delete does: a form (and the
/// dimensions measuring to it), a plane of the family's own; a datum stays.
#[test]
fn the_delete_key_takes_a_selected_form_or_plane_away() {
    use super::family_views::{click_family, editor, select_form, view};
    use bimcraft_core::family::views::Which;
    let script = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "ref_planes": [{"guid": "11111111-1111-4111-8111-000000000001", "name": "Edge", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}], "forms": [{"guid": "22222222-2222-4222-8222-000000000001", "host": {"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}, "profile": [[0, 0], [300, 0], [300, 500], [0, 500]], "to": 400}, {"profile": [[600, 0], [800, 0], [800, 300], [600, 300]], "to": 500}], "dimensions": [{"refs": [{"plane": "11111111-1111-4111-8111-000000000001"}, {"form": "22222222-2222-4222-8222-000000000001", "end": "end"}], "view": "front", "at": [0, 0, 700]}]}"#;
    let mut h = wide(script);
    h.set_size(vec2(1600.0, 1200.0));
    h.state_mut().edit_family("Box");
    h.run();
    // The first form, from the Project Browser: gone, and the dimension to it with it.
    select_form(&mut h, 1);
    assert_eq!(editor(&h).selected_form(), Some(0));
    key(&mut h, Key::Delete);
    let f = editor(&h).draft_family().unwrap();
    assert_eq!(f.forms.len(), 1, "the form deleted");
    assert!(f.dimensions.is_empty(), "the dimension measuring to it goes too");
    // A plane of its own, picked in plan.
    view(&mut h, Some(Which::Plan));
    click_family(&mut h, [-400.0, -1000.0, 0.0]);
    assert!(editor(&h).selected_plane().is_some());
    key(&mut h, Key::Delete);
    assert!(!editor(&h).planes.iter().any(|p| p.name == "Edge"), "the plane deleted");
    // A datum stays.
    click_family(&mut h, [0.0, -1000.0, 0.0]);
    assert_eq!(editor(&h).selected_plane(), Some(bimcraft_core::family::CENTER_LEFT_RIGHT));
    key(&mut h, Key::Delete);
    assert_eq!(editor(&h).planes.len(), 3, "the datums stay");
}

/// F10: the Forms tab's Delete takes the dimensions measuring to the form along, as the ribbon's does; a dimension left
/// measuring to nothing refused Load into Project. Its row wraps in the palette, so Delete isn't cut off at its edge.
#[test]
fn the_forms_tabs_delete_forgets_the_dimensions_to_the_form() {
    use super::family_views::editor;
    let script = r#"family {"name": "Box", "category": "generic", "types": {"T": {}}, "ref_planes": [{"guid": "11111111-1111-4111-8111-000000000001", "name": "Edge", "start": [-400, -2000, 0], "end": [-400, 2000, 0], "up": [0, 0, 1]}], "forms": [{"guid": "22222222-2222-4222-8222-000000000001", "host": {"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}, "profile": [[0, 0], [300, 0], [300, 500], [0, 500]], "to": 400}], "dimensions": [{"refs": [{"plane": "11111111-1111-4111-8111-000000000001"}, {"form": "22222222-2222-4222-8222-000000000001", "end": "end"}], "view": "front", "at": [0, 0, 700]}]}"#;
    let mut h = wide(script);
    h.set_size(vec2(1600.0, 1200.0));
    h.state_mut().edit_family("Box");
    h.run();
    // The Forms tab's only form is open: its Delete.
    use egui_kittest::kittest::NodeT;
    let entry = h.get_all_by_label("Delete").filter(|n| n.rect().left() < 300.0).map(|n| n.accesskit_node().id()).next();
    h.get_by(|n| Some(n.id()) == entry).scroll_to_me();
    h.run();
    let at = h.get_by(|n| Some(n.id()) == entry).rect();
    assert!(at.right() <= 290.0, "within the palette: {at:?}");
    click_at(&mut h, at.center());
    let f = editor(&h).draft_family().unwrap();
    assert!(f.forms.is_empty(), "the form deleted");
    assert!(f.dimensions.is_empty(), "and the dimension measuring to it");
    press(&mut h, "Load into Project");
    press(&mut h, "Overwrite the existing version");
    assert!(h.state().session.model.families["Box"].forms.is_empty(), "it loads: {}", h.state().status.text);
}
