use std::time::{Duration, Instant};

use bimcraft_core::view3d::{AZIMUTH, ELEVATION};

use super::*;
use crate::app::App;

/// Waits for the picture of the newest request and takes it in: false when none came.
fn wait(o: &mut Orbit) -> bool {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if o.poll() {
            return true;
        }
        if !o.busy() {
            return o.poll();
        }
        assert!(Instant::now() < deadline, "the 3D view's thread did not answer");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn shown(app: &App, size: (f32, f32)) -> Orbit {
    let mut o = Orbit::new();
    o.show(None, size);
    o.ask(&app.session.model, &[]);
    o
}

/// A point of the canvas where the picture shows an element, and which.
fn something(o: &Orbit) -> Option<((f32, f32), Id)> {
    let (w, h) = o.size;
    (0..(h as u32))
        .step_by(4)
        .flat_map(|y| (0..(w as u32)).step_by(4).map(move |x| (x as f32, y as f32)))
        .find_map(|p| Some((p, o.pick(p.0, p.1)?)))
}

fn rgba(o: &Orbit) -> Vec<u8> {
    o.frame.as_ref().map(|f| f.image.rgba.clone()).unwrap_or_default()
}

#[test]
fn the_model_is_made_on_a_thread_of_its_own_and_comes_in_fitted_from_the_default_side() {
    let app = App::sample();
    let mut o = shown(&app, (800.0, 600.0));
    assert!(o.frame.is_none() && o.camera.is_none(), "nothing before the thread answers");
    assert!(wait(&mut o), "the picture came: {:?}", o.error);
    let f = o.frame.as_ref().expect("a frame");
    assert_eq!((f.image.width, f.image.height), (800, 600), "a pixel to a point");
    assert_eq!(o.camera, Some(f.camera), "the camera is where the fit put it");
    assert_eq!((f.camera.azimuth, f.camera.elevation, f.camera.lens), (AZIMUTH, ELEVATION, None));
    assert!(f.image.rgba.chunks(4).any(|p| p[..3] != [255, 255, 255]), "the house is drawn");
    assert!((o.live_scale.get() - 1.0).abs() < 1e-6 && o.live_offset.get().iter().all(|v| v.abs() < 1e-3));
    o.ask(&app.session.model, &[]);
    assert!(!o.busy(), "the same request asks nothing of the thread");
}

#[test]
fn the_wheel_and_a_pan_move_the_picture_at_once_and_an_orbit_turns_it_when_the_new_one_comes() {
    let app = App::sample();
    let mut o = shown(&app, (800.0, 600.0));
    assert!(wait(&mut o));
    let first = o.camera.expect("a camera");
    let before = rgba(&o);

    o.zoom((400.0, 300.0), 2.0);
    assert!((o.live_scale.get() - 2.0).abs() < 1e-6, "twice as big at once: {}", o.live_scale.get());
    o.ask(&app.session.model, &[]);
    assert!(wait(&mut o));
    assert!((o.frame.as_ref().map_or(0.0, |f| f.camera.scale) - first.scale * 2.0).abs() < 1e-9 * first.scale);
    assert!((o.live_scale.get() - 1.0).abs() < 1e-6);

    o.grab((400.0, 300.0), false);
    o.drag(30.0, -10.0);
    let offset = o.live_offset.get();
    assert!((offset[0] - 30.0).abs() < 1.0 && (offset[1] + 10.0).abs() < 1.0, "the picture follows the pointer: {offset:?}");
    o.release();
    o.drag(100.0, 100.0);
    assert_eq!(o.live_offset.get(), offset, "a press let go moves nothing");

    let panned = o.camera.expect("a camera");
    o.grab((400.0, 300.0), true);
    o.drag(50.0, 20.0);
    let turned = o.camera.expect("a camera");
    assert!((turned.azimuth - panned.azimuth).abs() > 1.0 && (turned.elevation - panned.elevation).abs() > 1.0, "{panned:?} {turned:?}");
    assert_eq!((o.live_scale.get(), o.live_offset.get()), (1.0, [0.0, 0.0]), "a turn has nothing to move the old picture with");
    o.ask(&app.session.model, &[]);
    assert!(wait(&mut o));
    assert_eq!(o.frame.as_ref().map(|f| (f.camera.azimuth, f.camera.elevation)), Some((turned.azimuth, turned.elevation)));
    assert_ne!(rgba(&o), before, "the model is seen from another side");

    let kept = o.camera;
    o.grab((400.0, 300.0), true);
    o.drag(f32::NAN, 0.0);
    o.zoom((f32::NAN, 0.0), 2.0);
    o.zoom((1.0, 1.0), f64::NAN);
    assert_eq!(o.camera, kept, "numbers that are none change nothing");
}

#[test]
fn zoom_to_fit_frames_the_model_again_from_the_side_the_camera_looks_from() {
    let app = App::sample();
    let mut o = shown(&app, (800.0, 600.0));
    assert!(wait(&mut o));
    let fitted = o.camera.expect("a camera");
    o.grab((400.0, 300.0), true);
    o.drag(60.0, 0.0);
    o.release();
    o.zoom((400.0, 300.0), 3.0);
    let moved = o.camera.expect("a camera");
    o.fit();
    o.ask(&app.session.model, &[]);
    assert!(wait(&mut o));
    let again = o.camera.expect("a camera");
    assert_eq!((again.azimuth, again.elevation), (moved.azimuth, moved.elevation), "from the same side");
    assert_ne!(again.scale, moved.scale, "the model fits again: {fitted:?} {moved:?} {again:?}");
    o.ask(&app.session.model, &[]);
    assert!(!o.busy(), "the fit is done: it is not asked for again");
}

#[test]
fn a_click_finds_what_the_picture_shows_and_what_is_selected_is_tinted_in_it() {
    let app = App::sample();
    let mut o = shown(&app, (800.0, 600.0));
    assert!(wait(&mut o));
    let (at, id) = something(&o).expect("the picture shows an element");
    assert!(app.session.model.elements.get(id).is_some());
    let plain = rgba(&o);
    o.ask(&app.session.model, &[id]);
    assert!(o.busy(), "another selection is another picture");
    assert!(wait(&mut o));
    assert_eq!(o.frame.as_ref().map(|f| f.request.tints.clone()), Some(vec![(id, SELECT)]));
    assert_ne!(rgba(&o), plain, "the selected element is tinted");

    o.zoom(at, 3.0);
    assert!(something(&o).is_some(), "the picture on screen, three times as big, still picks");
    assert!(o.pick(f32::NAN, 0.0).is_none());
    let (w, h) = o.size;
    assert!(!o.in_box((0.0, 0.0), (w, h), true).is_empty(), "a box over the canvas touches what is on it");
    assert!(o.in_box((-1e9, -1e9), (-1e8, -1e8), true).is_empty());
}

#[test]
fn a_canvas_that_is_no_size_a_picture_can_have_changes_nothing_and_a_stop_lets_it_all_go() {
    let app = App::sample();
    let mut o = shown(&app, (800.0, 600.0));
    assert!(wait(&mut o));
    assert!(!o.resize(800.2, 600.2));
    for bad in [(f32::NAN, 600.0), (800.0, f32::INFINITY), (1.0, 600.0), (800.0, -5.0)] {
        assert!(!o.resize(bad.0, bad.1), "{bad:?}");
    }
    assert!(o.resize(1000.0, 700.0));
    assert_eq!(o.camera.map(|c| (c.width, c.height)), Some((1000, 700)), "the camera takes the new size at once");
    o.ask(&app.session.model, &[]);
    assert!(wait(&mut o));
    assert_eq!(o.frame.as_ref().map(|f| (f.image.width, f.image.height)), Some((1000, 700)));
    o.show(None, (500.0, 500.0));
    assert!(o.frame.is_some(), "the view on screen is shown again, not made again");
    o.stop();
    assert!(o.shown.is_none() && o.frame.is_none() && o.camera.is_none() && !o.busy());
}

#[test]
fn a_job_cancelled_makes_nothing_and_an_edit_brings_the_scene_up_to_date() {
    let app = App::sample();
    let request = |model: &Arc<Model>| Request {
        model: Arc::clone(model),
        view: None,
        camera: None,
        fit: false,
        size: [400, 300],
        tints: Vec::new(),
    };
    let mut renderer = Renderer::default();
    assert!(renderer.render(request(&app.session.model), &|| true).is_none(), "cancelled before it starts");
    let first = renderer.render(request(&app.session.model), &|| false).expect("a frame");
    let again = renderer.render(request(&app.session.model), &|| false).expect("a frame");
    assert_eq!(first.image.rgba, again.image.rgba, "the same request, the same picture, from the scene it kept");

    let mut other = App::sample();
    other.session.execute("wall", &serde_json::json!({"points": [[20000, 0], [22000, 0]]})).expect("a wall");
    let edited = renderer.render(request(&other.session.model), &|| false).expect("a frame");
    assert_ne!(edited.camera, first.camera, "the model is bigger: it fits differently");
}
