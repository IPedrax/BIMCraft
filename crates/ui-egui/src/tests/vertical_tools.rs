use super::*;

const VERTICAL: &str = r#"
section {"name":"Rotated", "start":[1000,2000], "end":[4000,6000]}
detail_line {"view":"Rotated", "start":[0,0], "end":[4000,0]}
detail_line {"view":"Rotated", "start":[0,2000], "end":[4000,2000]}
"#;

fn vertical() -> (Harness<'static, BimApp>, Id) {
    let mut h = app(VERTICAL);
    let id = ids(&h, "section")[0];
    h.state_mut().open_sheet(Sheet::Section(id));
    h.run();
    (h, id)
}

#[test]
fn vertical_tools_dimension_move_and_cancel_use_the_section_plane() {
    let (mut h, view) = vertical();
    h.state_mut().set_tool(Tool::Dimension(DimMode::Aligned, Vec::new()));
    h.run();
    click(&mut h, P2(2000.0, 0.0));
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(4500.0, 1000.0));
    let ds = ids(&h, "dimension");
    assert_eq!(ds.len(), 1, "{}", h.state().status.text);
    assert_eq!(field(&h, ds[0], "values"), json!([2000.0]));
    let line = ids(&h, "line")[1];
    h.state_mut().selection = vec![line];
    h.state_mut().set_tool(Tool::Move(None));
    h.run();
    click(&mut h, P2(2000.0, 2000.0));
    key(&mut h, Key::Escape);
    assert_eq!(field(&h, ds[0], "values"), json!([2000.0]));
    h.state_mut().selection = vec![line];
    h.state_mut().set_tool(Tool::Move(None));
    h.run();
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(2000.0, 2500.0));
    assert_eq!(field(&h, ds[0], "values"), json!([2500.0]));
    assert_eq!(h.state().sheet, Sheet::Section(view));
}

#[test]
fn vertical_tools_draw_trim_numeric_move_history_and_switch_cancel() {
    let (mut h, view) = vertical();
    h.state_mut().set_tool(Tool::DetailLine(Vec::new()));
    h.run();
    click(&mut h, P2(2000.0, 500.0));
    click(&mut h, P2(2000.0, 1800.0));
    let made = *ids(&h, "line").last().unwrap();
    h.state_mut().set_tool(Tool::TrimTo { boundary: None, many: false });
    h.run();
    click(&mut h, P2(1000.0, 2000.0));
    click(&mut h, P2(2000.0, 1000.0));
    let end = |h: &Harness<BimApp>| match h.state().session.model.elements.get(made).unwrap() {
        Element::DetailLine(l) => l.curve.end(),
        _ => panic!(),
    };
    assert!(end(&h).dist(P2(2000.0, 2000.0)) < 0.01, "{}", h.state().status.text);
    h.state_mut().selection = vec![made];
    h.state_mut().set_tool(Tool::Move(None));
    h.run();
    click(&mut h, P2(2000.0, 1000.0));
    let cursor = h.state().screen(P2(2500.0, 1000.0));
    h.hover_at(cursor);
    h.run();
    h.event(Event::Text("250".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert!(end(&h).dist(P2(2250.0, 2000.0)) < 0.01, "{}", h.state().status.text);
    h.state_mut().run("undo", json!({}));
    h.run();
    assert!(end(&h).dist(P2(2000.0, 2000.0)) < 0.01);
    h.state_mut().run("redo", json!({}));
    h.run();
    assert!(end(&h).dist(P2(2250.0, 2000.0)) < 0.01);
    h.state_mut().set_tool(Tool::Dimension(DimMode::Aligned, Vec::new()));
    h.run();
    click(&mut h, P2(1000.0, 2000.0));
    let plan = h.state().session.model.levels()[0].0;
    h.state_mut().open_sheet(Sheet::Plan(plan));
    h.run();
    assert_eq!(h.state().tool, Tool::Modify);
    h.state_mut().open_sheet(Sheet::Section(view));
    h.run();
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn vertical_tools_screenshots_in_both_profiles_and_three_looks() {
    let Ok(folder) = std::env::var("BIMCRAFT_VERTICAL_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (profile, name) in [(crate::style::Profile::Rev, "rev"), (crate::style::Profile::BimCraft, "bimcraft")] {
        for (theme, light, look) in
            [(egui::Theme::Light, false, "light"), (egui::Theme::Dark, false, "dark"), (egui::Theme::Dark, true, "dark-light")]
        {
            let mut h = harness(VERTICAL, 0.25, profile);
            h.ctx.set_theme(theme);
            h.state_mut().light_canvas = light;
            let view = ids(&h, "section")[0];
            let levels = h.state().session.model.levels().iter().map(|(id, _)| *id).collect::<Vec<_>>();
            h.state_mut().run("dimension", json!({"view":view,"refs":[{"id":levels[0]},{"id":levels[1]}],"at":[4400,1500]}));
            h.state_mut().open_sheet(Sheet::Section(view));
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("vertical-{name}-{look}.png"))).unwrap();
        }
    }
}
