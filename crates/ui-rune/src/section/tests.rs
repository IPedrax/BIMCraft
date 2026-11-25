use std::time::{Duration, Instant};

use super::*;
use crate::app::App;
use crate::views::ViewKind;

/// The id of the section or elevation the browser lists as `label`.
fn section_id(app: &App, label: &str) -> Id {
    app.views
        .groups
        .iter()
        .flat_map(|g| &g.entries)
        .find_map(|e| match e.kind {
            ViewKind::Section(id) if e.label == label => Some(id),
            _ => None,
        })
        .expect("a section of that name")
}

/// Waits for the picture of the newest request and takes it in: false when none came.
fn wait(cut: &mut Cut) -> bool {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if cut.poll() {
            return true;
        }
        if !cut.busy() {
            return cut.poll();
        }
        assert!(Instant::now() < deadline, "the section's thread did not answer");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn shown(app: &App, size: (f32, f32)) -> Cut {
    let mut cut = Cut::new();
    cut.show(section_id(app, "Section 1"), size);
    cut.ask(&app.session.model, &[]);
    cut
}

/// A point of the canvas where something is picked, and what.
fn something(cut: &Cut) -> Option<((f32, f32), Id)> {
    let (w, h) = cut.size;
    (0..(h as u32))
        .step_by(4)
        .flat_map(|y| (0..(w as u32)).step_by(4).map(move |x| (x as f32, y as f32)))
        .find_map(|p| Some((p, cut.pick(p.0, p.1)?)))
}

/// An element the picture itself shows (not the drawing over it): found by its pixels.
fn in_picture(cut: &Cut) -> Option<Id> {
    let image = cut.frame.as_ref()?.image.as_ref()?;
    (0..image.height)
        .step_by(4)
        .flat_map(|y| (0..image.width).step_by(4).map(move |x| (x, y)))
        .find_map(|(x, y)| image.pick(f64::from(x), f64::from(y)))
}

#[test]
fn a_section_is_made_on_a_thread_of_its_own_and_comes_in_framed() {
    let app = App::sample();
    let mut cut = shown(&app, (800.0, 600.0));
    assert!(cut.frame.is_none(), "nothing is on screen before the thread answers");
    assert!(wait(&mut cut), "the picture came: {:?}", cut.error);
    assert!(cut.error.is_none() && cut.revision == 1 && !cut.busy());
    let f = cut.frame.as_ref().expect("a frame");
    assert_eq!((f.view.width, f.view.height), (800, 600), "framed for the canvas");
    assert!(f.image.as_ref().is_some_and(|i| (i.width, i.height) == (800, 600)), "the picture is the canvas's size, a pixel to a point");
    assert!(!f.drawing.prims.is_empty(), "the drawing over it holds the levels, the grids and the cut");
    assert_eq!(cut.camera, Camera::from_view(f.view), "the camera is where the fit put it");
    assert!(
        (cut.live_scale.get() - 1.0).abs() < 1e-6 && cut.live_offset.get().iter().all(|v| v.abs() < 1e-3),
        "the picture on screen is the one made for the camera"
    );
    cut.ask(&app.session.model, &[]);
    assert!(!cut.busy(), "the same request asks nothing of the thread");
}

#[test]
fn the_wheel_and_a_pan_move_the_picture_at_once_and_the_thread_makes_the_exact_one() {
    let app = App::sample();
    let mut cut = shown(&app, (800.0, 600.0));
    assert!(wait(&mut cut));
    let scale = cut.camera.scale;
    cut.zoom((400.0, 300.0), 2.0);
    assert!((cut.live_scale.get() - 2.0).abs() < 1e-6, "the old picture is shown twice as big, at once: {}", cut.live_scale.get());
    cut.ask(&app.session.model, &[]);
    assert!(wait(&mut cut));
    let f = cut.frame.as_ref().expect("a frame");
    assert!((f.view.scale - scale * 2.0).abs() < 1e-9, "the exact picture is for the camera: {} against {}", f.view.scale, scale);
    assert!((cut.live_scale.get() - 1.0).abs() < 1e-6 && cut.live_offset.get()[0].abs() < 1e-3, "and needs no moving");

    cut.followed = (0.0, 0.0);
    cut.pan(30.0, -10.0);
    let offset = cut.live_offset.get();
    assert!((offset[0] - 30.0).abs() < 1e-3 && (offset[1] + 10.0).abs() < 1e-3, "the picture follows the pointer: {offset:?}");
    cut.pan(f32::NAN, 0.0);
    assert_eq!(cut.live_offset.get(), offset, "a pointer that is no number moves nothing");
}

#[test]
fn a_click_finds_what_the_picture_shows_and_still_finds_it_after_a_zoom_that_has_no_new_picture_yet() {
    let app = App::sample();
    let mut cut = shown(&app, (800.0, 600.0));
    assert!(wait(&mut cut));
    let (at, id) = something(&cut).expect("the section shows something");
    assert!(app.session.model.elements.get(id).is_some(), "what is picked is an element of the model");
    cut.zoom(at, 3.0);
    let back = cut.drawn_at(at);
    assert!(
        (back.0 - at.0).abs() < 1e-3 && (back.1 - at.1).abs() < 1e-3,
        "the point the wheel zooms about is where it was in the picture: {back:?}"
    );
    assert!(something(&cut).is_some(), "the picture on screen, three times as big, still picks");
    assert!(cut.pick(f32::NAN, 0.0).is_none(), "a point that is no number is under nothing");
    let (w, h) = cut.size;
    assert!(!cut.in_box((0.0, 0.0), (w, h), true).is_empty(), "a box over the canvas touches what is on it");
    assert!(cut.in_box((-1e9, -1e9), (-1e8, -1e8), true).is_empty(), "a box far from the picture holds nothing");
}

#[test]
fn what_is_selected_is_tinted_in_the_picture_and_that_asks_for_no_new_geometry() {
    let app = App::sample();
    let mut cut = shown(&app, (800.0, 600.0));
    assert!(wait(&mut cut));
    let id = in_picture(&cut).expect("the picture shows an element");
    let plain = cut.frame.as_ref().map(|f| f.image.as_ref().map(|i| i.rgba.clone())).expect("a frame");
    cut.ask(&app.session.model, &[id]);
    assert!(wait(&mut cut));
    let f = cut.frame.as_ref().expect("a frame");
    assert_eq!(f.request.tints, vec![(id, SELECT)]);
    assert!(f.image.as_ref().map(|i| i.rgba.clone()) != plain, "the selected element's pixels are tinted");
    assert_eq!(f.view, cut.camera.view(800.0, 600.0), "the camera did not move");
}

#[test]
fn a_canvas_that_is_no_size_a_picture_can_have_changes_nothing_and_a_stop_lets_it_all_go() {
    let app = App::sample();
    let mut cut = shown(&app, (800.0, 600.0));
    assert!(wait(&mut cut));
    assert!(!cut.resize(800.2, 600.2), "less than half a point is no change");
    for bad in [(f32::NAN, 600.0), (800.0, f32::INFINITY), (1.0, 600.0), (800.0, -5.0)] {
        assert!(!cut.resize(bad.0, bad.1), "{bad:?}");
    }
    assert_eq!(cut.size, (800.0, 600.0));
    assert!(cut.resize(1000.0, 700.0) && cut.size == (1000.0, 700.0));
    cut.ask(&app.session.model, &[]);
    assert!(wait(&mut cut));
    assert_eq!(cut.frame.as_ref().map(|f| (f.view.width, f.view.height)), Some((1000, 700)), "framed again for the new size");

    let id = cut.id;
    cut.show(id.expect("a section"), (500.0, 500.0));
    assert!(cut.frame.is_some(), "the section on screen is shown again, not made again");
    cut.stop();
    assert!(cut.id.is_none() && cut.frame.is_none() && !cut.busy(), "another kind of view lets the thread and the picture go");
}

#[test]
fn nothing_moves_before_the_first_picture_and_a_region_zooms_to_its_box() {
    let app = App::sample();
    let mut cut = shown(&app, (800.0, 600.0));
    let camera = cut.camera;
    cut.zoom((10.0, 10.0), 2.0);
    cut.pan(20.0, 20.0);
    assert!(!cut.zoom_region((0.0, 0.0), (100.0, 100.0)), "no region before there is a view to look at");
    assert_eq!(cut.camera, camera, "the camera is not fitted yet");
    assert!(wait(&mut cut));
    assert!(!cut.zoom_region((10.0, 10.0), (10.0, 90.0)), "a box with no width");
    let scale = cut.camera.scale;
    assert!(cut.zoom_region((100.0, 100.0), (500.0, 400.0)) && cut.camera.scale > scale);
}

#[test]
fn a_job_cancelled_makes_nothing_and_another_model_is_made_again() {
    let app = App::sample();
    let request = |model: &Arc<Model>| Request {
        model: Arc::clone(model),
        id: section_id(&app, "Section 1"),
        view: None,
        size: [400, 300],
        tints: Vec::new(),
    };
    let mut renderer = Renderer::default();
    assert!(renderer.render(request(&app.session.model), &|| true).is_none(), "cancelled before it starts");
    let first = renderer.render(request(&app.session.model), &|| false).expect("a frame");
    let again = renderer.render(request(&app.session.model), &|| false).expect("a frame");
    assert_eq!(first.view, again.view);
    assert_eq!(
        first.image.as_ref().map(|i| i.rgba.clone()),
        again.image.as_ref().map(|i| i.rgba.clone()),
        "the same request, the same picture"
    );
    let mut other = App::sample();
    other.session.execute("wall", &serde_json::json!({"points": [[20000, 0], [22000, 0]]})).expect("a wall");
    assert!(!Arc::ptr_eq(&other.session.model, &app.session.model));
    assert!(renderer.render(request(&other.session.model), &|| false).is_some(), "a new model has its work made again");
}
