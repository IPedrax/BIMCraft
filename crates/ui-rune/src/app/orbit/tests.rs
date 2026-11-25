use std::time::{Duration, Instant};

use super::*;

fn open_3d(app: &mut App) {
    assert!(app.open_view(ViewKind::View3D(None)));
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
        assert!(Instant::now() < deadline, "the 3D view's thread did not answer");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A point of the canvas where the picture shows an element, and which.
fn on_picture(app: &App) -> Option<((f32, f32), u64)> {
    let (w, h) = app.orbit.size;
    (0..(h as u32))
        .step_by(4)
        .flat_map(|y| (0..(w as u32)).step_by(4).map(move |x| (x as f32, y as f32)))
        .find_map(|q| Some((q, app.orbit.pick(q.0, q.1)?)))
}

#[test]
fn opening_the_3d_view_asks_for_its_picture_and_leaving_it_lets_the_thread_go() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    open_3d(&mut app);
    assert_eq!(app.orbit.shown, Some(None));
    assert!(app.status().starts_with("3D View: {3D}"), "{}", app.status());
    wait(&mut app);
    assert!(app.orbit.frame.is_some() && app.orbit.error.is_none(), "{:?}", app.orbit.error);
    assert!(app.drawing.is_none() && app.cut.id.is_none() && app.paper.id.is_none(), "a 3D view is not a section or a sheet");
    assert!(app.open_view(ViewKind::Plan(level)));
    assert!(app.orbit.shown.is_none() && app.orbit.frame.is_none() && !app.views_busy());
}

#[test]
fn a_click_selects_what_the_picture_shows_and_the_box_takes_what_it_holds() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    open_3d(&mut app);
    wait(&mut app);
    let ((x, y), _) = on_picture(&app).expect("the picture shows an element");
    app.orbit_hover(x, y);
    assert!(app.hovered.is_some());
    app.orbit_select((x, y), (x, y), How::Replace);
    assert!(!app.selection.is_empty(), "a click on what the picture shows takes it");
    wait(&mut app);
    let tinted: Vec<u64> = app.orbit.frame.as_ref().map(|f| f.request.tints.iter().map(|(id, _)| *id).collect()).unwrap_or_default();
    assert_eq!(tinted, app.selection, "and it is tinted in the next picture");
    app.selection.clear();

    let (w, h) = app.orbit.size;
    app.orbit_sweep((w - 4.0, h - 4.0), (4.0, 4.0));
    assert!(app.rubber.is_some());
    app.orbit_select((w - 4.0, h - 4.0), (4.0, 4.0), How::Replace);
    assert!(app.rubber.is_none() && !app.selection.is_empty(), "a box right to left takes what it touches");
    let selected = app.selection.clone();
    assert!(app.open_view(ViewKind::Plan(level)));
    assert_eq!(app.selection, selected, "the selection is the model's");
}

#[test]
fn the_zoom_buttons_fit_and_a_drag_work_on_the_3d_view_and_a_hostile_pointer_does_nothing() {
    let mut app = App::sample();
    open_3d(&mut app);
    wait(&mut app);
    let (plan_camera, scale) = (app.camera, app.orbit.camera.map_or(0.0, |c| c.scale));
    app.zoom_by(2.0);
    assert!((app.orbit.live_scale.get() - 2.0).abs() < 1e-6);
    wait(&mut app);
    assert!((app.orbit.camera.map_or(0.0, |c| c.scale) - scale * 2.0).abs() < scale * 1e-6);
    app.orbit_grab((100.0, 100.0), true);
    app.orbit_drag(40.0, 0.0);
    app.orbit_release();
    wait(&mut app);
    let turned = app.orbit.camera.map(|c| c.azimuth);
    app.zoom_fit();
    wait(&mut app);
    assert_eq!(app.orbit.camera.map(|c| c.azimuth), turned, "Zoom to Fit keeps the side it looks from");
    assert_eq!(app.camera, plan_camera, "the plan's camera is the plan's");

    app.orbit_zoom(100.0, 100.0, f32::NAN);
    app.orbit_hover(f32::NAN, 0.0);
    app.orbit_sweep((f32::NAN, 0.0), (1.0, 1.0));
    assert!(app.hovered.is_none() && app.rubber.is_none());
}

#[test]
fn a_command_makes_the_picture_again() {
    let mut app = App::sample();
    open_3d(&mut app);
    wait(&mut app);
    let revision = app.orbit.revision;
    assert!(app.run("wall", &serde_json::json!({"points": [[20000, 0], [22000, 0]]})), "refused: {:?}", app.message);
    wait(&mut app);
    assert!(app.orbit.revision > revision, "the model changed: the view is made again");
    assert!(app.orbit.frame.as_ref().is_some_and(|f| std::sync::Arc::ptr_eq(&f.request.model, &app.session.model)));
}
