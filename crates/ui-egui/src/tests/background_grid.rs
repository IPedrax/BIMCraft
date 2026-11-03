use super::*;

#[test]
fn background_grid_menu_and_instance_editor_change_only_the_active_view() {
    let mut h = app("");
    pick(&mut h, Role::Button, "Background Grids");
    press(&mut h, "Main grid");
    assert!(h.state().background_grid().main.enabled);
    pick(&mut h, Role::Button, "Background Grids");
    press(&mut h, "Grid snapping");
    h.key_press(Key::Escape);
    h.run();
    let app = h.state();
    let level = app.plan_key().unwrap();
    let view = bimcraft_core::render::View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600 };
    let found = app.snap(&app.session.model, level, &view, view.px(P2(1230.0, -670.0)), &[], false);
    assert_eq!(found.at, P2(1000.0, -1000.0));
    let kept = app.background_grid();
    topmost(&mut h, "Level 2");
    assert!(!h.state().background_grid().main.enabled);
    assert!(h.state().grid_snapping, "snapping stays enabled when switching views");
    let empty_view = h.state();
    let level = empty_view.plan_key().unwrap();
    let found = empty_view.snap(&empty_view.session.model, level, &view, view.px(P2(1234.0, -674.0)), &[], false);
    assert_eq!(found.at, P2(1230.0, -670.0), "a view without enabled grids does not grid-snap");
    topmost(&mut h, "Level 1");
    assert_eq!(h.state().background_grid(), kept);
    // Properties is a short palette: scroll its instance parameters to Graphics.
    h.hover_at(pos2(180.0, 420.0));
    h.event(Event::MouseWheel {
        phase: TouchPhase::Move,
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -1500.0),
        modifiers: Modifiers::NONE,
    });
    h.run();
    press(&mut h, "Edit Background Grids");
    assert!(h.state().background_grid_dialog.is_some());
    type_into(&mut h, "Main spacing X", "2.5 m");
    key(&mut h, Key::Enter);
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(h.state().background_grid().spacing[0], 2500.0);
    press(&mut h, "Cancel");
    pick(&mut h, Role::Button, "Background Grids");
    press(&mut h, "Grid settings…");
    assert!(h.state().background_grid_dialog.is_some());
    press(&mut h, "Cancel");
}

#[test]
fn background_grid_templates_disable_view_parameters_and_bottom_menu() {
    use egui_kittest::kittest::NodeT;
    let mut h = app(
        "template {\"name\":\"Grid template\",\"controls\":[\"background_grid\"],\"background_grid\":{\"main\":{\"enabled\":true},\"rotation\":30}}\ngraphics {\"view\":\"Level 1\",\"template\":\"Grid template\"}",
    );
    assert_eq!(h.state().background_grid().rotation, 30.0);
    h.hover_at(pos2(180.0, 420.0));
    h.event(Event::MouseWheel {
        phase: TouchPhase::Move,
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -1500.0),
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!(h.get_by_label("Edit Background Grids").accesskit_node().is_disabled());
    pick(&mut h, Role::Button, "Background Grids");
    assert!(h.get_by_label("Main grid").accesskit_node().is_disabled());
    assert!(!h.get_by_label("Grid snapping").accesskit_node().is_disabled());
    assert!(h.get_by_label("Grid settings…").accesskit_node().is_disabled());
    let steps = h.state().session.history().len();
    press(&mut h, "Grid snapping");
    assert!(h.state().grid_snapping);
    assert_eq!(h.state().session.history().len(), steps, "global snapping is not a view graphics edit");
    key(&mut h, Key::Escape);
    // The template editor changes its followers, without unlocking their instance fields.
    shortcut(&mut h, "VT");
    h.get_all_by_label("Edit Background Grids").find(|n| !n.accesskit_node().is_disabled()).unwrap().click();
    h.run();
    type_into(&mut h, "Main spacing X", "2 m");
    key(&mut h, Key::Enter);
    press(&mut h, "OK");
    // Its OK puts them in the View Templates dialog's draft, which its own OK sends.
    assert_eq!(h.state().background_grid().spacing[0], 1000.0);
    assert!(h.state().templates_dialog.is_some());
    press(&mut h, "OK");
    assert_eq!(h.state().background_grid().spacing[0], 2000.0);
}

#[test]
fn background_grid_preferences_live_in_the_project_and_cancel_discards_drafts() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("grid.bimcraft");
    let settings = dir.path().join("settings.json");
    let mut h = app("");
    std::fs::write(&settings, "{}").unwrap();
    h.state_mut().use_settings(settings.clone());
    h.state_mut()
        .run("graphics", json!({"view":"Level 1", "background_grid":{"main":{"enabled":true}, "rotation":25, "spacing":[304.8,609.6]}}));
    h.run();
    let grid = h.state().background_grid();
    h.state_mut().grid_snapping = true;
    h.run();
    h.state_mut().open_background_grid();
    h.run();
    type_into(&mut h, "Main spacing X", "9000");
    key(&mut h, Key::Enter);
    press(&mut h, "Cancel");
    assert_eq!(h.state().background_grid(), grid);
    assert!(h.state_mut().save_to(&project));
    let mut again = app("");
    again.state_mut().use_settings(settings.clone());
    assert!(again.state().grid_snapping);
    assert!(again.state_mut().open_path(&project));
    assert!(again.state().grid_snapping, "Open preserves the global snap preference");
    assert_eq!(again.state().background_grid(), grid);
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    assert!(kept.get("background_grid").is_none());
    assert_eq!(kept["grid_snapping"], true);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(project).unwrap()).unwrap();
    assert!(!saved.to_string().contains("snapping"), "the project does not store the global snap preference");
}

#[test]
fn background_grid_snapping_preserves_object_snaps_and_shift_constraints() {
    let mut h = app("wall {\"points\":[[1230,670],[5230,670]]}");
    h.state_mut().grid_snapping = true;
    h.state_mut().run("graphics", json!({"view":"Level 1", "background_grid":{"main":{"enabled":true},"visible":false}}));
    let app = h.state();
    let level = app.plan_key().unwrap();
    let view = bimcraft_core::render::View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600 };
    let snapped = |p, from: &[P2], square| app.snap(&app.session.model, level, &view, view.px(p), from, square);
    assert_eq!(snapped(P2(1230.0, 670.0), &[], false).at, P2(1230.0, 670.0));
    assert_eq!(snapped(P2(-1340.0, -710.0), &[], false).at, P2(-1000.0, -1000.0));
    let from = P2(-1340.0, -710.0);
    let found = snapped(P2(-2200.0, -800.0), &[from], true);
    assert!((found.at.0 - from.0).abs() < 1e-6 || (found.at.1 - from.1).abs() < 1e-6);
}

#[derive(Clone, Default)]
struct Pictures(Arc<std::sync::Mutex<Vec<(egui::TextureId, egui::ColorImage)>>>);

impl egui_kittest::TestRenderer for Pictures {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        let mut pictures = self.0.lock().unwrap();
        for (id, changes) in &delta.set {
            for change in changes.iter().filter(|c| c.pos.is_none()) {
                let egui::ImageData::Color(image) = &change.image;
                pictures.push((*id, (**image).clone()));
            }
        }
        delta.clear();
    }
}

#[test]
fn background_grid_in_sections_exposes_empty_paper_and_keeps_model_faces_opaque() {
    let mut session = Session::new();
    session.script(&format!("{BOX}\nsection {{\"start\":[-1000,3000],\"end\":[9000,3000]}}\ngraphics {{\"view\":\"Section 1\",\"background_grid\":{{\"main\":{{\"enabled\":true}}}}}}")).unwrap();
    let pictures = Pictures::default();
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .renderer(pictures.clone())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(session));
    h.run();
    browse(&mut h, "Section 1");
    let image = pictures
        .0
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(id, _)| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == "section"))
        .unwrap()
        .1
        .clone();
    assert!(image.pixels.iter().any(|p| p.a() == 0));
    assert!(image.pixels.iter().any(|p| p.a() == 255));
    h.state_mut().run("graphics", json!({"view":"Section 1", "background_grid":{"visible":false}}));
    h.run();
    let pictures = pictures.0.lock().unwrap();
    let image = &pictures.iter().rev().find(|(id, _)| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == "section")).unwrap().1;
    assert!(image.pixels.iter().all(|p| p.a() == 255));
}

#[test]
fn background_grid_spacing_accepts_metric_and_imperial_lengths() {
    for (units, typed, millimetres) in [("mm", "2.5 m", 2500.0), ("ft-in", "2' 6\"", 762.0)] {
        let mut h = app(&format!("units {{\"length\":\"{units}\"}}"));
        h.state_mut().open_background_grid();
        h.run();
        type_into(&mut h, "Main spacing X", typed);
        key(&mut h, Key::Enter);
        press(&mut h, "OK");
        assert!((h.state().background_grid().spacing[0] - millimetres).abs() < 1e-6);
    }
}

#[test]
fn grid_snapping_is_shared_between_manage_snaps_and_the_bottom_menu() {
    use egui_kittest::kittest::NodeT;
    let mut h = app("");
    let model = h.state().session.model.to_json().unwrap();
    press(&mut h, "Manage");
    press(&mut h, "Snaps");
    pick(&mut h, Role::CheckBox, "Grid snapping");
    assert!(h.state().grid_snapping);
    press(&mut h, "Close");
    pick(&mut h, Role::Button, "Background Grids");
    assert_eq!(h.get_by_label("Grid snapping").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    pick(&mut h, Role::CheckBox, "Grid snapping");
    assert!(!h.state().grid_snapping);
    key(&mut h, Key::Escape);
    press(&mut h, "Snaps");
    assert_eq!(h.get_by_label("Grid snapping").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    press(&mut h, "Close");
    assert_eq!(h.state().session.model.to_json().unwrap(), model);
}
