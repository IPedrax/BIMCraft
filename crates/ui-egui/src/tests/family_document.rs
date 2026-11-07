//! The family editor occupies the window for conventional families too.
use super::*;

fn associate(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    press(h, label);
}

#[test]
fn conventional_family_opens_as_a_document_in_both_profiles() {
    for profile in [crate::style::Profile::Rev, crate::style::Profile::BimCraft] {
        let mut h = harness("", 0.25, profile);
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
    let mut h = app(BOX);
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
    let mut h = app("");
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
    assert_eq!(h.state().session.model.families["New Family"].forms[0].to, bimcraft_core::family::Expr::Number(750.));
}

#[test]
fn associations_filter_yesno_and_material_and_preserve_the_project_until_load() {
    let mut h = app("");
    let material = h.state().session.model.materials.keys().next().unwrap().clone();
    assert!(h.state_mut().run("family",json!({"name":"Associations","parameters":[{"name":"Show","kind":"yesno","value":true},{"name":"Finish","kind":"material","value":material},{"name":"Depth","value":100}],"types":{"A":{}},"forms":[{"profile":[[0,0],[100,0],[100,100],[0,100]],"to":100}]})).is_some());
    let before = h.state().session.model.to_json().unwrap();
    h.state_mut().edit_family("Associations");
    h.run();
    associate(&mut h, "Associate Family Parameter: Visible");
    assert!(h.query_by_label("Show").is_some());
    assert!(h.query_by_label("Finish").is_none() && h.query_by_label("Depth").is_none());
    press(&mut h, "Show");
    press(&mut h, "OK");
    associate(&mut h, "Associate Family Parameter: Form Material 1");
    assert!(h.query_by_label("Finish").is_some());
    assert_eq!(h.query_all_by_label("Show").count(), 1, "only the Visible row's own, tied to it");
    press(&mut h, "Finish");
    press(&mut h, "OK");
    assert_eq!(h.get_by_label("Form Material 1").value().as_deref(), Some(material.as_str()));
    assert_eq!(h.state().session.model.to_json().unwrap(), before);
    press(&mut h, "Load into Project");
    let f = &h.state().session.model.families["Associations"];
    assert_eq!(f.forms[0].visible, Some(bimcraft_core::family::Expr::Formula("Show".into())));
    assert_eq!(f.forms[0].material, "Finish");
}

#[test]
fn loading_an_edited_document_keeps_connectors_reference_geometry_and_symbols() {
    use bimcraft_core::family::{Face, PlaneRef, RefLine, Reference};
    use bimcraft_core::ids::Guid;
    let mut h = app("");
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
    type_into(&mut h, "Parameter Value 1", "1200");
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
