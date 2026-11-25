//! More of the Modify tools in {3D}: Copy's Multiple, planes and lines, a solid and a void together, views that see the work
//! plane edge on or graze it, an upright work plane, a nested instance, and a drag that turns the view.

use super::family_drag_move::{EDGE, LINE, ed, family, family_with, g, pick, points, status};
use super::family_selection::command;
use super::family_views::{editor, open, view};
use super::*;
use crate::editor::modify::Tool as M;
use crate::editor::select_box::Picked;
use bimcraft_core::family::Expr;
use bimcraft_core::family::views::Which;

const FRONT_BACK: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2";

fn in_3d_of(script: &str) -> Harness<'static, BimApp> {
    let (mut h, _) = open(script, "Movers");
    view(&mut h, None);
    for _ in 0..4 {
        h.run();
    }
    h
}

fn click3(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = h.state().family_screen3(p).expect("{3D} drawn");
    click_at(h, at);
}

fn tool(h: &Harness<BimApp>) -> Option<M> {
    editor(h).modify_in_hand()
}

fn num(e: &Expr) -> f64 {
    match e {
        Expr::Number(x) => *x,
        _ => f64::NAN,
    }
}

#[test]
fn copy_multiple_in_3d_copies_at_each_click() {
    let mut h = in_3d_of(&family_with(""));
    pick(&mut h, &[Picked::Form(0)]);
    let n = family(&h).forms.len();
    command(&mut h, "Copy");
    ed(&mut h).views.modify_options.multiple = true;
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [0.0, 1000.0, 0.0]);
    for _ in 0..3 {
        h.run();
    }
    // (At 2000 north the click fell off the pane: 500 north of the first copy is on it.)
    click3(&mut h, [0.0, 500.0, 0.0]);
    for _ in 0..3 {
        h.run();
    }
    assert_eq!(family(&h).forms.len(), n + 2, "Multiple: a second copy from the third click");
}

#[test]
fn rotate_in_3d_turns_a_plane_alone() {
    let mut h = in_3d_of(&family_with(""));
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    command(&mut h, "Rotate");
    for _ in 0..2 {
        h.run();
    }
    ed(&mut h).views.modify_options.rotate_copy = false;
    // (East of the plane the clicks fell off the pane: from west of its middle, turned towards the south-west.)
    click3(&mut h, [400.0, 0.0, 0.0]);
    click3(&mut h, [400.0, -300.0, 0.0]);
    let p = family(&h).planes.iter().find(|p| p.guid == g(EDGE)).unwrap().clone();
    assert!((p.start[0] - 1000.0).abs() > 1.0 || (p.end[0] - 1000.0).abs() > 1.0, "the plane turned: {}", status(&h));
}

#[test]
fn a_plane_and_a_line_move_in_3d() {
    let mut h = in_3d_of(&family_with(""));
    pick(&mut h, &[Picked::Plane(g(EDGE)), Picked::Line(g(LINE))]);
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [300.0, 100.0, 0.0]);
    let f = family(&h);
    let p = f.planes.iter().find(|p| p.guid == g(EDGE)).unwrap();
    let l = f.lines.iter().find(|l| l.guid == g(LINE)).unwrap();
    assert!((p.start[0] - 1300.0).abs() < 2.0, "plane x");
    assert!((l.start[0] + 200.0 - 300.0).abs() < 2.0 && (l.start[1] - 1300.0).abs() < 2.0, "line");
}

#[test]
fn a_solid_and_a_void_move_together_in_3d() {
    let script = family_with("").replace(
        r#""to": 300}], "dimensions""#,
        r#""to": 300}, {"void": true, "sketch": [{"start": [200, 200], "end": [400, 200]}, {"start": [400, 200], "end": [400, 400]}, {"start": [400, 400], "end": [200, 400]}, {"start": [200, 400], "end": [200, 200]}], "from": -10, "to": 310}], "dimensions""#,
    );
    let mut h = in_3d_of(&script);
    assert_eq!(family(&h).forms.len(), 3);
    pick(&mut h, &[Picked::Form(0), Picked::Form(2)]);
    for _ in 0..3 {
        h.run();
    }
    let (a, v) = (points(&h, 0)[0], points(&h, 2)[0]);
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [300.0, 0.0, 0.0]);
    let (a2, v2) = (points(&h, 0)[0], points(&h, 2)[0]);
    assert!((a2[0] - a[0] - 300.0).abs() < 2.0, "solid moved");
    assert!((v2[0] - v[0] - 300.0).abs() < 2.0, "the void selected with it moved too");
}

#[test]
fn a_view_that_sees_the_work_plane_edge_on_refuses_the_click() {
    let mut h = in_3d_of(&family_with(""));
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Move");
    let rect = ed(&mut h).orbit.rect;
    // Level, parallel, seen exactly edge on.
    for (elev, lens) in [(0.0, None), (0.0, Some(35.0)), (10.0, Some(35.0)), (0.001, None)] {
        {
            let e = ed(&mut h);
            let c = e.orbit.camera.expect("a camera");
            e.orbit.camera = Some(bimcraft_core::view3d::Camera { elevation: elev, lens, ..c });
            e.orbit.fitted = None;
        }
        h.run();
        let at = rect.center() - egui::vec2(0.0, rect.height() * 0.4);
        click_at(&mut h, at);
        // Seen edge on, the click is refused, saying so, and no point is taken; looked down on at 10 degrees in
        // perspective, it lands on the plane.
        if elev < 1.0 {
            assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }), "{elev} {lens:?}");
            assert!(status(&h).contains("edge on"), "{elev} {lens:?}: {}", status(&h));
        } else {
            assert!(matches!(tool(&h), Some(M::Move { from: Some(_), .. })), "{elev} {lens:?}: {}", status(&h));
        }
        key(&mut h, Key::Escape);
        command(&mut h, "Move");
    }
}

#[test]
fn a_grazing_view_refuses_the_click() {
    let mut h = in_3d_of(&family_with(""));
    pick(&mut h, &[Picked::Form(0)]);
    let before = points(&h, 0)[0];
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    {
        let e = ed(&mut h);
        let c = e.orbit.camera.expect("a camera");
        e.orbit.camera = Some(bimcraft_core::view3d::Camera { elevation: 0.01, lens: None, ..c });
        e.orbit.fitted = None;
    }
    h.run();
    let rect = ed(&mut h).orbit.rect;
    click_at(&mut h, rect.center() - egui::vec2(0.0, rect.height() * 0.3));
    // A view that all but grazes the plane sees it edge on: the second click is refused, nothing moved, the first kept.
    assert_eq!(points(&h, 0)[0], before);
    assert!(status(&h).contains("edge on"), "{}", status(&h));
    assert!(matches!(tool(&h), Some(M::Move { from: Some(_), .. })));
}

#[test]
fn rotate_on_an_upright_plane_refuses_to_take_a_form_off_its_level() {
    let mut h = in_3d_of(&family_with(""));
    ed(&mut h).views.set_work_plane(bimcraft_core::family::PlaneRef::Plane(g(FRONT_BACK)));
    pick(&mut h, &[Picked::Form(0)]);
    let before = serde_json::to_value(&family(&h).forms[0]).unwrap();
    command(&mut h, "Rotate");
    for _ in 0..2 {
        h.run();
    }
    ed(&mut h).views.modify_options.rotate_copy = false;
    // Centre is (300, 150) on the plane: x 300, z 150. East of it, then above it.
    click3(&mut h, [700.0, 0.0, 150.0]);
    click3(&mut h, [300.0, 0.0, 550.0]);
    // A turn about the upright plane's normal would take the box off Ref. Level, which stays: refused, saying so.
    let after = serde_json::to_value(&family(&h).forms[0]).unwrap();
    assert_eq!(before, after);
    assert!(status(&h).contains("Ref. Level") && status(&h).contains("datum"), "{}", status(&h));
}

#[test]
fn a_nested_instance_moves_in_3d() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = open(&family_with(""), "Movers");
    view(&mut h, Some(Which::Plan));
    let path = dir.path().join("Small.json");
    std::fs::write(
        &path,
        serde_json::json!({"name": "Small", "category": "generic", "types": {"Short": {}},
        "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": 100}]})
        .to_string(),
    )
    .unwrap();
    let doc = h.state().active_doc().unwrap();
    h.state_mut().nest_family_file(doc, &path);
    h.run();
    ed(&mut h).component_tool();
    h.run();
    ed(&mut h).placing = Some(("Small".into(), "Short".into()));
    let pos = editor(&h).views.screen_of([200.0, -400.0, 0.0]).unwrap();
    click_at(&mut h, pos);
    h.run();
    key(&mut h, Key::Escape);
    assert_eq!(editor(&h).nested_instances.len(), 1);
    view(&mut h, None);
    for _ in 0..4 {
        h.run();
    }
    pick(&mut h, &[Picked::Nested(0)]);
    let at = family(&h).nested_instances[0].at.clone();
    command(&mut h, "Move");
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [300.0, 0.0, 0.0]);
    let now = family(&h).nested_instances[0].at.clone();
    assert!((num(&now[0]) - num(&at[0]) - 300.0).abs() < 2.0);
}

#[test]
fn a_drag_with_a_tool_in_hand_turns_the_view_and_places_nothing() {
    let mut h = in_3d_of(&family_with(""));
    pick(&mut h, &[Picked::Form(0)]);
    command(&mut h, "Move");
    let cam = editor(&h).orbit.camera;
    let rect = editor(&h).orbit.rect;
    let (a, b) = (rect.center(), rect.center() + egui::vec2(200.0, 60.0));
    // A slow drag: many frames.
    h.hover_at(a);
    h.run();
    h.event(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for t in 1..=30 {
        h.hover_at(a + (b - a) * (t as f32 / 30.0));
        h.run();
    }
    h.event(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    let moved = editor(&h).orbit.camera != cam;
    assert!(moved, "the drag turned the view");
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }), "and placed no point");
    // A quick flick turns it too, and places no point either.
    let cam = editor(&h).orbit.camera;
    flick(&mut h, a, b, Modifiers::NONE);
    h.run();
    assert_ne!(editor(&h).orbit.camera, cam);
    assert_eq!(tool(&h), Some(M::Move { copy: false, from: None }));
}

#[test]
fn modify_in_3d_on_an_upright_plane_screenshots() {
    let Ok(dir) = std::env::var("BIMCRAFT_MODIFY_3D_DUMP") else { return };
    for (name, tool, first, then) in [
        ("move-upright", "Move", [0.0, 0.0, 0.0], [200.0, 0.0, 500.0]),
        ("mirror-upright", "Mirror - Draw Axis", [0.0, 0.0, 0.0], [0.0, 0.0, 500.0]),
    ] {
        let mut h = app(&family_with(""));
        h.set_size(vec2(1400.0, 900.0));
        h.state_mut().edit_family("Movers");
        h.ctx.set_theme(egui::ThemePreference::Light);
        for _ in 0..4 {
            h.run();
        }
        view(&mut h, None);
        for _ in 0..4 {
            h.run();
        }
        ed(&mut h).views.set_work_plane(bimcraft_core::family::PlaneRef::Plane(g(FRONT_BACK)));
        pick(&mut h, &[Picked::Form(0)]);
        command(&mut h, tool);
        for _ in 0..2 {
            h.run();
        }
        click3(&mut h, first);
        let hover = h.state().family_screen3(then).expect("{3D} drawn");
        h.hover_at(hover);
        for _ in 0..3 {
            h.run();
        }
        h.render().unwrap().save(std::path::Path::new(&dir).join(format!("modify-3d-{name}-light.png"))).unwrap();
    }
}
