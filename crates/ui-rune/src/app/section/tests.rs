use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::section::SELECT;

/// The first section the browser lists, opened.
fn open_section(app: &mut App) -> ViewKind {
    let kind = app
        .views
        .groups
        .iter()
        .find(|g| g.label == "Sections (Building Section)")
        .and_then(|g| g.entries.first())
        .map(|e| e.kind)
        .expect("the sample has a section");
    assert!(app.open_view(kind));
    kind
}

/// Waits for the thread's picture and takes it in.
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
        assert!(Instant::now() < deadline, "the section's thread did not answer");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A point of the canvas where the picture shows an element, and which.
fn on_picture(app: &App) -> Option<((f32, f32), u64)> {
    let image = app.cut.frame.as_ref()?.image.as_ref()?;
    (0..image.height).step_by(4).flat_map(|y| (0..image.width).step_by(4).map(move |x| (x, y))).find_map(|(x, y)| {
        let id = image.pick(f64::from(x), f64::from(y))?;
        (app.session.model.elements.get(id).is_some()).then_some(((x as f32, y as f32), id))
    })
}

#[test]
fn opening_a_section_asks_for_its_picture_and_leaving_it_lets_the_thread_go() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let kind = open_section(&mut app);
    assert_eq!(
        app.cut.id,
        match kind {
            ViewKind::Section(id) => Some(id),
            _ => None,
        }
    );
    wait(&mut app);
    assert!(app.cut.frame.is_some() && app.cut.error.is_none(), "{:?}", app.cut.error);
    assert!(app.drawing.is_none(), "the plan has nothing to draw while a section is shown");

    assert!(app.open_view(ViewKind::Plan(level)));
    assert!(app.cut.id.is_none() && app.cut.frame.is_none() && !app.views_busy(), "a plan lets the section's thread and picture go");
    assert!(app.drawing.is_some());
}

#[test]
fn a_click_in_a_section_selects_what_the_picture_shows_and_the_plan_shows_it_selected_too() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    open_section(&mut app);
    wait(&mut app);
    let ((x, y), id) = on_picture(&app).expect("the picture shows an element");
    app.section_hover(x, y);
    assert!(app.hovered.is_some(), "the pointer is over something");
    app.section_release((x, y), (x, y), How::Replace);
    assert!(!app.selection.is_empty(), "a click on what the picture shows ({id}) takes it");
    let selected = app.selection.clone();
    wait(&mut app);
    let tinted: Vec<u64> = app.cut.frame.as_ref().map(|f| f.request.tints.iter().map(|(id, _)| *id).collect()).unwrap_or_default();
    assert_eq!(tinted, selected);
    assert!(app.cut.frame.as_ref().is_some_and(|f| f.request.tints.iter().all(|(_, c)| *c == SELECT)));

    assert!(app.open_view(ViewKind::Plan(level)));
    assert_eq!(app.selection, selected, "the selection is the model's: the plan has it too");

    // A click on nothing takes nothing, and Ctrl adds.
    open_section(&mut app);
    wait(&mut app);
    app.section_release((1.0, 1.0), (1.0, 1.0), How::Replace);
    app.section_release((x, y), (x, y), How::Add);
    assert!(!app.selection.is_empty());
}

#[test]
fn a_box_over_the_section_selects_what_it_holds_and_a_region_zooms_to_it() {
    let mut app = App::sample();
    open_section(&mut app);
    wait(&mut app);
    let (w, h) = app.cut.size;
    // Right to left: it takes what it touches.
    app.section_sweep((w - 4.0, h - 4.0), (4.0, 4.0));
    assert!(app.rubber.is_some(), "the box shows while the button is held");
    app.section_release((w - 4.0, h - 4.0), (4.0, 4.0), How::Replace);
    assert!(app.rubber.is_none() && !app.selection.is_empty(), "a box over the canvas takes what it touches or holds: {:?}", app.selection);
    app.selection.clear();

    let scale = app.cut.camera.scale;
    app.region = true;
    app.section_sweep((100.0, 100.0), (300.0, 250.0));
    app.section_release((100.0, 100.0), (300.0, 250.0), How::Replace);
    assert!(!app.region && app.cut.camera.scale > scale, "the region zoomed the view");
    assert!(app.selection.is_empty(), "and selected nothing");
    wait(&mut app);
    assert!((app.cut.live_scale.get() - 1.0).abs() < 1e-6, "the exact picture is in");
}

#[test]
fn a_command_makes_the_picture_again_and_the_zoom_buttons_work_on_the_section() {
    let mut app = App::sample();
    open_section(&mut app);
    wait(&mut app);
    let (revision, plan_camera) = (app.cut.revision, app.camera);
    assert!(app.run("wall", &json!({"points": [[20000, 0], [22000, 0]]})), "refused: {:?}", app.message);
    wait(&mut app);
    assert!(app.cut.revision > revision);
    assert!(
        app.cut.frame.as_ref().is_some_and(|f| std::sync::Arc::ptr_eq(&f.request.model, &app.session.model)),
        "a picture of the model as it is"
    );

    // What Zoom to Fit frames is of the model as it is now (the new wall is something the section sees), so the baseline is its fit.
    app.zoom_fit();
    wait(&mut app);
    let scale = app.cut.camera.scale;
    app.zoom_by(2.0);
    assert!((app.cut.live_scale.get() - 2.0).abs() < 1e-6, "the picture on screen is twice as big at once");
    wait(&mut app);
    assert!((app.cut.camera.scale - scale * 2.0).abs() < 1e-9 && (app.cut.live_scale.get() - 1.0).abs() < 1e-6);
    app.zoom_fit();
    wait(&mut app);
    assert!((app.cut.camera.scale - scale).abs() < 1e-9, "Zoom to Fit frames the section again");
    assert_eq!(app.camera, plan_camera, "the plan's camera is the plan's");

    app.section_zoom(100.0, 100.0, f32::NAN);
    app.section_hover(f32::NAN, 0.0);
    app.section_sweep((f32::NAN, 0.0), (1.0, 1.0));
    assert!(app.hovered.is_none() && app.rubber.is_none(), "a pointer that is no number does nothing");
}

#[test]
fn a_section_that_goes_falls_back_to_the_plan_and_the_thread_stops() {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let kind = open_section(&mut app);
    wait(&mut app);
    let ViewKind::Section(id) = kind else { panic!("a section") };
    assert!(app.run("delete", &json!({"ids": [id]})), "refused: {:?}", app.message);
    assert_eq!(app.view, ViewKind::Plan(level));
    assert!(app.cut.id.is_none() && !app.views_busy(), "the section is gone: so is its thread's work");
}
