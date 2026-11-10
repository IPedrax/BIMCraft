//! Select All Instances from the canvas and from a type in the Project Browser.
use super::*;

#[test]
fn select_instances_canvas_menu_offers_view_and_project_scopes() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    h.state_mut().session.execute("graphics", &json!({"view":"{3D}","hide_elements":[walls[1],walls[2],walls[3]]})).unwrap();
    press(&mut h, "Default 3D View");
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [walls[0]]);
    right_click_at(&mut h, at);
    choose_item(&mut h, "Select All Instances");
    choose_item(&mut h, "In Entire Project");
    assert_eq!(h.state().selection, walls);
    right_click_at(&mut h, at);
    choose_item(&mut h, "Select All Instances");
    choose_item(&mut h, "Visible in View");
    assert_eq!(h.state().selection, [walls[0]]);
}

#[test]
fn select_instances_browser_type_needs_no_selected_instance() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    type_into(&mut h, "Search the Project Browser", "wall Generic - 200mm");
    let at = h.get_by_label("Generic - 200mm").rect().center();
    right_click_at(&mut h, at);
    choose_item(&mut h, "Select All Instances");
    choose_item(&mut h, "In Entire Project");
    assert_eq!(h.state().selection, walls);
    assert_eq!(h.state().tool, Tool::Modify);
    let level = h.state().session.model.levels()[0].0;
    h.state_mut().session.execute("graphics", &json!({"view":level,"hide_elements":[walls[1]]})).unwrap();
    h.run();
    let at = h.get_by_label("Generic - 200mm").rect().center();
    right_click_at(&mut h, at);
    choose_item(&mut h, "Select All Instances");
    choose_item(&mut h, "Visible in View");
    assert_eq!(h.state().selection, [walls[0], walls[2], walls[3]]);
}

#[test]
fn select_instances_browser_family_types_do_not_select_another_familys_same_named_type() {
    let mut h = app("");
    let mut placed = Vec::new();
    for family in ["First", "Second"] {
        h.state_mut()
            .session
            .execute("family", &json!({"name":family,"types":{"A":{}},"forms":[{"profile":[[0,0],[100,0],[100,100],[0,100]],"to":100}]}))
            .unwrap();
        for x in [0, 200] {
            placed.push(h.state_mut().session.execute("component", &json!({"family":family,"at":[x,0]})).unwrap()["id"].as_u64().unwrap());
        }
    }
    h.run();
    type_into(&mut h, "Search the Project Browser", "First A");
    let at = h.get_by_label("A").rect().center();
    right_click_at(&mut h, at);
    choose_item(&mut h, "Select All Instances");
    choose_item(&mut h, "In Entire Project");
    assert_eq!(h.state().selection, placed[..2]);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn select_instances_visible_scope_respects_temporary_hidden_elements() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    press(&mut h, "Default 3D View");
    h.state_mut().selection = vec![walls[1]];
    shortcut(&mut h, "HH");
    h.state_mut().selection = vec![walls[0]];
    shortcut(&mut h, "SA");
    assert!(!h.state().selection.contains(&walls[1]));
    assert_eq!(h.state().selection.len(), 3);
}

#[test]
fn select_instances_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_T15_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for profile in [crate::style::Profile::Rev, crate::style::Profile::BimCraft] {
        for (name, theme, light_canvas) in [
            ("light", egui::ThemePreference::Light, false),
            ("dark", egui::ThemePreference::Dark, false),
            ("mixed", egui::ThemePreference::Dark, true),
        ] {
            let mut h = harness(BOX, 0.25, profile);
            h.ctx.options_mut(|o| o.theme_preference = theme);
            h.state_mut().light_canvas = light_canvas;
            h.state_mut().selection = ids(&h, "wall")[..1].to_vec();
            h.run();
            let at = h.state().screen(P2(4000.0, 3000.0));
            right_click_at(&mut h, at);
            choose_item(&mut h, "Select All Instances");
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("t15-{profile:?}-{name}-canvas.png"))).unwrap();
            key(&mut h, Key::Escape);
            key(&mut h, Key::Escape);
            type_into(&mut h, "Search the Project Browser", "wall Generic - 200mm");
            let at = h.get_by_label("Generic - 200mm").rect().center();
            right_click_at(&mut h, at);
            choose_item(&mut h, "Select All Instances");
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("t15-{profile:?}-{name}-browser.png"))).unwrap();
        }
    }
}

#[test]
fn select_instances_visible_scope_uses_the_named_3d_view() {
    let mut h = app(BOX);
    let walls = ids(&h, "wall");
    let view = h
        .state_mut()
        .session
        .execute("camera", &json!({"eye":[12000,-12000,6000],"target":[4000,3000,1000],"name":"Selection camera"}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    h.state_mut().session.execute("graphics", &json!({"view":view,"hide_elements":[walls[1]]})).unwrap();
    h.state_mut().open_3d(Some(view));
    h.run();
    h.state_mut().selection = vec![walls[0]];
    shortcut(&mut h, "SA");
    assert_eq!(h.state().selection, [walls[0], walls[2], walls[3]]);
}
