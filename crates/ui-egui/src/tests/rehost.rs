use super::*;

const MODEL: &str = r#"
@a = wall {"points": [[0,0],[6000,0]]}
@b = wall {"points": [[6000,3000],[0,3000]]}
window {"wall": "@a.ids.0", "offset": 2000, "mark": "KEEP", "flip_facing": true}
"#;

#[test]
fn pick_new_host_is_a_contextual_action_and_one_undo() {
    let mut h = app(MODEL);
    let id = ids(&h, "window")[0];
    let before = field(&h, id, "host");
    h.state_mut().selection = vec![id];
    h.state_mut().tab = Tab::Modify;
    h.run();
    press(&mut h, "Pick New Host");
    click(&mut h, P2(3500.0, 3000.0));
    assert_ne!(field(&h, id, "host"), before, "{:?}", h.state().status);
    assert_eq!(field(&h, id, "mark"), json!("KEEP"));
    assert_eq!(field(&h, id, "flip_facing"), json!(true));
    h.state_mut().exec("undo", &json!({})).unwrap();
    assert_eq!(field(&h, id, "host"), before);
}

fn selected(profile: crate::style::Profile) -> (Harness<'static, BimApp>, Id) {
    let mut h = harness(MODEL, 0.25, profile);
    let id = ids(&h, "window")[0];
    h.state_mut().selection = vec![id];
    h.state_mut().tab = Tab::Modify;
    h.run();
    (h, id)
}

#[test]
fn pick_new_host_hover_cancel_and_invalid_click_leave_the_model_untouched() {
    for profile in [crate::style::Profile::Rev, crate::style::Profile::BimCraft] {
        let (mut h, _) = selected(profile);
        let before = Arc::clone(&h.state().session.model);
        let history = h.state().session.history().len();
        press(&mut h, "Pick New Host");
        hover(&mut h, P2(3500.0, 3000.0));
        assert!(Arc::ptr_eq(&before, &h.state().session.model));
        click(&mut h, P2(20.0, 3000.0));
        assert!(Arc::ptr_eq(&before, &h.state().session.model));
        assert!(h.state().status.error);
        key(&mut h, Key::Escape);
        assert_eq!(h.state().tool, Tool::Modify);
        assert!(Arc::ptr_eq(&before, &h.state().session.model));
        assert_eq!(h.state().session.history().len(), history);
    }
}

#[test]
fn pick_new_host_does_not_commit_a_preview_after_another_edit_or_view_change() {
    for change_view in [false, true] {
        let (mut h, id) = selected(crate::style::Profile::Rev);
        let host = field(&h, id, "host");
        press(&mut h, "Pick New Host");
        hover(&mut h, P2(3500.0, 3000.0));
        if change_view {
            h.state_mut().open_sheet(Sheet::View3D);
            h.run();
            assert_eq!(h.state().tool, Tool::Modify);
        } else {
            h.state_mut().exec("modify", &json!({"id": id, "mark": "NEW"})).unwrap();
            h.run();
            click(&mut h, P2(3500.0, 3000.0));
            assert_eq!(field(&h, id, "mark"), json!("NEW"));
        }
        assert_eq!(field(&h, id, "host"), host);
    }
}

#[test]
fn pick_new_host_is_disabled_outside_plans_or_for_pinned_openings() {
    let (mut h, id) = selected(crate::style::Profile::Rev);
    assert!(h.state().context_items().iter().flatten().any(|i| i.action == Action::PickNewHost && i.enabled));
    h.state_mut().exec("pin", &json!({"ids": [id]})).unwrap();
    assert!(h.state().context_items().iter().flatten().any(|i| i.action == Action::PickNewHost && !i.enabled));
    h.state_mut().exec("unpin", &json!({"ids": [id]})).unwrap();
    h.state_mut().open_sheet(Sheet::View3D);
    assert!(h.state().context_items().iter().flatten().any(|i| i.action == Action::PickNewHost && !i.enabled));
}

#[test]
fn pick_new_host_cancels_when_the_selection_changes() {
    let (mut h, _) = selected(crate::style::Profile::Rev);
    press(&mut h, "Pick New Host");
    let before = Arc::clone(&h.state().session.model);
    h.state_mut().selection = ids(&h, "wall");
    h.run();
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(Arc::ptr_eq(&before, &h.state().session.model));
}

#[test]
fn pick_new_host_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_REHOST_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (profile, name) in [(crate::style::Profile::Rev, "rev"), (crate::style::Profile::BimCraft, "bimcraft")] {
        for (theme, light_canvas, look) in
            [(egui::Theme::Light, false, "light"), (egui::Theme::Dark, false, "dark"), (egui::Theme::Dark, true, "light-canvas")]
        {
            let (mut h, id) = selected(profile);
            h.set_size(vec2(1600.0, 1000.0));
            h.ctx.set_theme(theme);
            h.state_mut().theme_wanted =
                Some(if theme == egui::Theme::Dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            h.state_mut().light_canvas = light_canvas;
            h.run();
            let save = |h: &mut Harness<BimApp>, stage: &str| {
                h.run();
                h.render().unwrap().save(std::path::Path::new(&folder).join(format!("{name}-{look}-{stage}.png"))).unwrap();
            };
            save(&mut h, "before");
            press(&mut h, "Pick New Host");
            hover(&mut h, P2(3500.0, 3000.0));
            save(&mut h, "preview");
            click(&mut h, P2(3500.0, 3000.0));
            assert_eq!(field(&h, id, "host"), json!(ids(&h, "wall")[1]));
            save(&mut h, "after");
        }
    }
}
