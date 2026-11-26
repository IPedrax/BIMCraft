use std::time::{Duration, Instant};

use bimcraft_core::model::Element;
use bimcraft_core::render::View;

use super::*;
use crate::app::App;
use crate::views::ViewKind;

/// The id of the sheet the browser lists as `label`.
fn sheet_id(app: &App, label: &str) -> Id {
    app.views
        .groups
        .iter()
        .flat_map(|g| &g.entries)
        .find_map(|e| match e.kind {
            ViewKind::Sheet(id) if e.label == label => Some(id),
            _ => None,
        })
        .expect("a sheet of that name")
}

/// Waits for the picture of the newest request and takes it in: false when none came.
fn wait(p: &mut Paper) -> bool {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if p.poll() {
            return true;
        }
        if !p.busy() {
            return p.poll();
        }
        assert!(Instant::now() < deadline, "the sheet's thread did not answer");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn shown(app: &App, size: (f32, f32)) -> Paper {
    let mut p = Paper::new();
    p.show(sheet_id(app, "A101 - Ground Floor"), size);
    p.ask(&app.session.model);
    p
}

/// A point of the canvas where something of the sheet is picked, and what.
fn something(p: &Paper) -> Option<((f32, f32), Id)> {
    let (w, h) = p.size;
    (0..(h as u32))
        .step_by(3)
        .flat_map(|y| (0..(w as u32)).step_by(3).map(move |x| (x as f32, y as f32)))
        .find_map(|q| Some((q, p.pick(q.0, q.1)?)))
}

#[test]
fn a_paper_is_framed_whole_and_clear_of_the_edges() {
    let c = framed(P2(420.0, 297.0), (900.0, 640.0));
    assert!((c.cx - 210.0).abs() < 1e-9 && (c.cy - 148.5).abs() < 1e-9, "the paper's middle");
    assert!((c.scale - ((640.0 - 48.0) / 297.0)).abs() < 1e-9, "the height is the tighter fit: {}", c.scale);
    let v = c.view(900.0, 640.0);
    let (left, top) = v.px(P2(0.0, 297.0));
    let (right, bottom) = v.px(P2(420.0, 0.0));
    assert!(left >= 23.9 && top >= 23.9 && right <= 900.0 - 23.9 && bottom <= 640.0 - 23.9, "{left} {top} {right} {bottom}");
    let tiny = framed(P2(420.0, 297.0), (10.0, f32::NAN));
    assert!(tiny.scale.is_finite() && tiny.scale > 0.0, "a canvas with no room still frames");
}

#[test]
fn a_sheet_is_painted_on_a_thread_of_its_own_and_comes_in_framed_at_the_paper_scale() {
    let app = App::sample();
    let mut p = shown(&app, (800.0, 600.0));
    assert!(p.frame.is_none(), "nothing is on screen before the thread answers");
    assert!(wait(&mut p), "the picture came: {:?}", p.error);
    assert!(p.error.is_none() && p.revision == 1 && !p.busy());
    let f = p.frame.as_ref().expect("a frame");
    assert_eq!(f.paper, P2(420.0, 297.0), "an A3 sheet");
    let [x, y, w, h] = f.at.expect("the paper is on the canvas");
    assert_eq!(f.rgba.len(), (w as usize) * (h as usize) * 4, "the picture is as big as the paper's visible part");
    assert!(x >= 0.0 && y >= 0.0 && x + w <= 800.0 && y + h <= 600.0, "inside the canvas: {:?}", f.at);
    assert!(f.rgba.chunks(4).any(|px| px[..3] != [255, 255, 255]), "the title block and the views are drawn on the white paper");
    assert!(!f.drawing.prims.is_empty());
    assert_eq!(p.camera, framed(f.paper, (800.0, 600.0)));
    assert!((p.live_scale.get() - 1.0).abs() < 1e-6 && p.live_offset.get().iter().all(|v| v.abs() < 1e-3));
    p.ask(&app.session.model);
    assert!(!p.busy(), "the same request asks nothing of the thread");
}

#[test]
fn the_wheel_moves_the_picture_at_once_and_the_thread_paints_the_exact_one() {
    let app = App::sample();
    let mut p = shown(&app, (800.0, 600.0));
    assert!(wait(&mut p));
    let scale = p.camera.scale;
    p.zoom((400.0, 300.0), 2.0);
    assert!((p.live_scale.get() - 2.0).abs() < 1e-6, "the old picture, twice as big, at once");
    p.ask(&app.session.model);
    assert!(wait(&mut p));
    let f = p.frame.as_ref().expect("a frame");
    assert!((f.view.scale - scale * 2.0).abs() < 1e-9, "painted for the camera");
    assert!((p.live_scale.get() - 1.0).abs() < 1e-6);
    p.followed = (0.0, 0.0);
    p.pan(30.0, -10.0);
    let offset = p.live_offset.get();
    assert!((offset[0] - 30.0).abs() < 1e-3 && (offset[1] + 10.0).abs() < 1e-3, "{offset:?}");
    p.fit();
    p.ask(&app.session.model);
    assert!(wait(&mut p));
    assert_eq!(p.camera, framed(P2(420.0, 297.0), (800.0, 600.0)), "Zoom to Fit frames the whole paper again");
}

#[test]
fn a_click_finds_a_view_of_the_sheet_and_the_frame_round_it_is_on_the_canvas() {
    let app = App::sample();
    let mut p = shown(&app, (800.0, 600.0));
    assert!(wait(&mut p));
    let (at, id) = something(&p).expect("the sheet has something to pick");
    assert!(app.session.model.elements.get(id).is_some());
    let viewport = p.frame.as_ref().and_then(|f| {
        f.drawing.owners.iter().flatten().copied().find(|o| matches!(app.session.model.elements.get(*o), Some(Element::Viewport(_))))
    });
    let viewport = viewport.expect("the sheet has a viewport");
    let boxes = p.boxes(&[viewport]);
    assert_eq!(boxes.len(), 1, "a view is one frame");
    let [l, t, r, b] = boxes[0];
    assert!(r > l && b > t && l >= 0.0 && t >= 0.0 && r <= 800.0 && b <= 600.0, "on the paper, on the canvas: {l} {t} {r} {b}");
    p.zoom(at, 3.0);
    let back = p.drawn_at(at).expect("a point of the drawing");
    let before = p.frame.as_ref().map(|f| (f.view.mm(at), f.paper)).expect("a frame");
    assert!(((back.0 / DENSITY as f32) as f64 - before.0.0).abs() < 1e-2, "the point the wheel zooms about is where it was on the paper");
    assert!(p.pick(f32::NAN, 0.0).is_none() && p.boxes(&[7]).is_empty());
    assert!(!p.in_box((0.0, 0.0), (800.0, 600.0), true).is_empty() && p.in_box((-1e9, -1e9), (-1e8, -1e8), true).is_empty());
}

#[test]
fn a_canvas_that_is_no_size_a_picture_can_have_changes_nothing_and_a_stop_lets_it_all_go() {
    let app = App::sample();
    let mut p = shown(&app, (800.0, 600.0));
    assert!(wait(&mut p));
    assert!(!p.resize(800.2, 600.2));
    for bad in [(f32::NAN, 600.0), (800.0, f32::INFINITY), (1.0, 600.0), (800.0, -5.0)] {
        assert!(!p.resize(bad.0, bad.1), "{bad:?}");
    }
    assert!(p.resize(1000.0, 700.0));
    p.ask(&app.session.model);
    assert!(wait(&mut p));
    assert_eq!(p.camera, framed(P2(420.0, 297.0), (1000.0, 700.0)), "framed again for the new size");
    p.zoom((10.0, 10.0), 2.0);
    assert!(p.zoom_region((100.0, 100.0), (500.0, 400.0)) && !p.zoom_region((10.0, 10.0), (10.0, 90.0)));
    let id = p.id.expect("a sheet");
    p.show(id, (500.0, 500.0));
    assert!(p.frame.is_some(), "the sheet on screen is shown again, not made again");
    p.stop();
    assert!(p.id.is_none() && p.frame.is_none() && !p.busy());
}

#[test]
fn a_job_cancelled_paints_nothing_and_a_sheet_that_is_not_one_answers_nothing() {
    let app = App::sample();
    let id = sheet_id(&app, "A101 - Ground Floor");
    let view = framed(P2(420.0, 297.0), (400.0, 300.0)).view(400.0, 300.0);
    let request = |id: Id| Request { model: Arc::clone(&app.session.model), id, view };
    let mut renderer = Renderer::default();
    assert!(renderer.render(request(id), &|| true).is_none(), "cancelled before it starts");
    let first = renderer.render(request(id), &|| false).expect("a frame");
    let again = renderer.render(request(id), &|| false).expect("a frame");
    assert_eq!(first.rgba, again.rgba, "the same request paints the same picture, from the drawing it kept");
    assert!(Arc::ptr_eq(&first.drawing, &again.drawing), "the drawing is made once for a model");
    let level = app.levels[0].0;
    assert!(renderer.render(request(level), &|| false).is_none(), "a level is no sheet");
    // A camera that has the paper off the canvas has no picture of it.
    let far = View { cx: 1e7, ..view };
    let off = renderer.render(Request { view: far, ..request(id) }, &|| false).expect("a frame");
    assert!(off.at.is_none() && off.rgba.is_empty());
}
