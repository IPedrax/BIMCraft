use std::time::{Duration, Instant};

use bimcraft_core::geom::P2;

use super::*;
use crate::paper::framed;

/// The first sheet the browser lists, opened.
fn open_sheet(app: &mut App) -> ViewKind {
    let kind = app
        .views
        .groups
        .iter()
        .find(|g| g.label == "Sheets (all)")
        .and_then(|g| g.entries.first())
        .map(|e| e.kind)
        .expect("the sample has sheets");
    assert!(app.open_view(kind));
    kind
}

fn wait(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if app.poll_views() {
            return;
        }
        if !app.views_busy() {
            app.poll_views();
            return;
        }
        assert!(Instant::now() < deadline, "the sheet's thread did not answer");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn opening_a_sheet_asks_for_its_picture_and_leaving_it_lets_the_thread_go() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let kind = open_sheet(&mut app);
    assert_eq!(
        app.paper.id,
        match kind {
            ViewKind::Sheet(id) => Some(id),
            _ => None,
        }
    );
    assert!(app.status().starts_with("Sheet: A101 - Ground Floor"), "{}", app.status());
    wait(&mut app);
    assert!(app.paper.frame.is_some() && app.paper.error.is_none(), "{:?}", app.paper.error);
    assert!(app.drawing.is_none() && !app.cut.busy() && app.cut.id.is_none(), "a sheet is not a section");
    assert!(app.open_view(ViewKind::Plan(level)));
    assert!(app.paper.id.is_none() && app.paper.frame.is_none() && !app.views_busy());
}

#[test]
fn a_click_on_a_view_of_the_sheet_selects_its_viewport_and_the_plan_has_it_too() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    open_sheet(&mut app);
    wait(&mut app);
    let (w, h) = app.paper.size;
    let (q, id) = (0..(h as u32))
        .step_by(3)
        .flat_map(|y| (0..(w as u32)).step_by(3).map(move |x| (x as f32, y as f32)))
        .find_map(|q| Some((q, app.paper.pick(q.0, q.1)?)))
        .expect("something on the sheet");
    app.paper_hover(q.0, q.1);
    assert_eq!(app.hovered, Some(id));
    app.paper_release(q, q, How::Replace);
    assert_eq!(app.selection, vec![id]);
    assert!(app.open_view(ViewKind::Plan(level)));
    assert_eq!(app.selection, vec![id], "the selection is the model's");
}

#[test]
fn the_zoom_buttons_fit_and_a_region_work_on_the_sheet_and_a_hostile_pointer_does_nothing() {
    let mut app = App::sample();
    open_sheet(&mut app);
    wait(&mut app);
    let plan_camera = app.camera;
    let scale = app.paper.camera.scale;
    app.zoom_by(2.0);
    assert!((app.paper.live_scale.get() - 2.0).abs() < 1e-6);
    wait(&mut app);
    assert!((app.paper.camera.scale - scale * 2.0).abs() < 1e-9);
    app.zoom_fit();
    wait(&mut app);
    assert_eq!(app.paper.camera, framed(app.paper.frame.as_ref().map_or(P2(0.0, 0.0), |f| f.paper), app.paper.size));
    assert_eq!(app.camera, plan_camera, "the plan's camera is the plan's");

    app.region = true;
    app.paper_sweep((100.0, 100.0), (300.0, 250.0));
    app.paper_release((100.0, 100.0), (300.0, 250.0), How::Replace);
    assert!(!app.region && app.paper.camera.scale > scale);
    wait(&mut app);
    app.paper_zoom(100.0, 100.0, f32::NAN);
    app.paper_hover(f32::NAN, 0.0);
    app.paper_sweep((f32::NAN, 0.0), (1.0, 1.0));
    assert!(app.hovered.is_none() && app.rubber.is_none());
}

#[test]
fn a_command_paints_the_sheet_again() {
    let mut app = App::sample();
    open_sheet(&mut app);
    wait(&mut app);
    let revision = app.paper.revision;
    assert!(app.run("text", &serde_json::json!({"text": "Revised", "at": [6300, 1900]})), "refused: {:?}", app.message);
    wait(&mut app);
    assert!(app.paper.revision > revision, "the model changed: the sheet is painted again");
    assert!(app.paper.frame.as_ref().is_some_and(|f| std::sync::Arc::ptr_eq(&f.request.model, &app.session.model)));
}
