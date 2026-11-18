//! Sketch mode in the 3D view: Edit Boundary on a floor, ceiling, roof or shaft selected there keeps the sketch in 3D, its
//! lines on the element's own plane, picked and dragged by their ends there, and drawn on with the Draw panel's tools.

use super::*;
use bimcraft_core::sketch::SketchLine;

#[test]
fn extrusion_roof_edit_profile_commits_an_open_chain_and_cancel_keeps_the_model() {
    let mut h = app(
        "roof {\"extrusion\": {\"line\": [[1000,2000],[7000,2000]], \"profile\": [[0,0],[6000,1500]], \"from\": -500, \"to\": 4000}, \"offset\": 3000}",
    );
    let roof = ids(&h, "roof")[0];
    let before = field(&h, roof, "extrusion");
    h.state_mut().selection = vec![roof];
    h.run();
    press(&mut h, "Modify | Roofs");
    press(&mut h, "Edit Profile");
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert_eq!(lines(&h).len(), 1, "an open profile, not the footprint");
    h.state_mut().sketch.as_mut().unwrap().move_end(P2(6000.0, 1500.0), P2(6000.0, 3000.0));
    h.run();
    assert_eq!(field(&h, roof, "extrusion"), before, "draft only");
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    assert_eq!(h.state().selection, vec![roof]);
    assert_eq!(field(&h, roof, "extrusion")["profile"], json!([[0.0, 0.0], [6000.0, 3000.0]]));
    assert_eq!(field(&h, roof, "extrusion")["from"], before["from"]);
    assert_eq!(field(&h, roof, "extrusion")["to"], before["to"]);
    h.state_mut().run("undo", json!({}));
    assert_eq!(field(&h, roof, "extrusion"), before);
    h.state_mut().run("redo", json!({}));
    let after = field(&h, roof, "extrusion");
    h.run();
    press(&mut h, "Edit Profile");
    h.state_mut().sketch.as_mut().unwrap().lines.clear();
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_some());
    assert!(h.state().status.error);
    assert_eq!(field(&h, roof, "extrusion"), after);
    press(&mut h, "Cancel Edit Mode");
    assert_eq!(field(&h, roof, "extrusion"), after);
    assert_eq!(h.state().selection, vec![roof]);
}

#[test]
fn extrusion_roof_rotated_profile_grips_and_numeric_heights_work_in_both_profiles() {
    for rev in [true, false] {
        let script = "level {\"name\": \"Roof level\", \"elevation\": 2000}\nroof {\"level\": \"Roof level\", \"extrusion\": {\"line\": [[1000,2000],[4600,6800]], \"profile\": [[0,0],[3000,1500],[6000,0]], \"from\": -500, \"to\": 4000}, \"offset\": 1000}";
        let mut h = if rev { wide(script) } else { bimcraft(script) };
        let roof = ids(&h, "roof")[0];
        h.state_mut().selection = vec![roof];
        h.run();
        if rev {
            press(&mut h, "Modify | Roofs");
        }
        assert!(h.query_by_role_and_label(Role::TextInput, "Slope (°)").is_none());
        let history = h.state().session.history().len();
        h.get_by_label("Edit Profile").scroll_to_me();
        h.run();
        press(&mut h, "Edit Profile");
        assert!(h.state().sketch.is_some());
        h.run_steps(4);
        if !rev {
            assert!(h.get_by_label("Split Element").rect().right() < h.state().view_rect.left());
        }
        let frame = bimcraft_core::workplane::roof_profile(&h.state().session.model, roof).unwrap();
        assert_eq!(frame.origin, [1000.0, 2000.0, 3000.0]);
        let at = h.state().screen3(frame.point(P2(1500.0, 750.0)));
        if let Ok(folder) = std::env::var("BIMCRAFT_EXTRUSION_DUMP") {
            std::fs::create_dir_all(&folder).unwrap();
            h.render()
                .unwrap()
                .save(std::path::Path::new(&folder).join(format!("extrusion-{}-start.png", if rev { "rev" } else { "bimcraft" })))
                .unwrap();
        }
        click_at(&mut h, at);
        assert_eq!(picked(&h), vec![0], "rev={rev}, at={at:?}, rect={:?}, tool={:?}", h.state().orbit.rect, h.state().tool);
        let camera = h.state().orbit.camera;
        let a = h.state().screen3(frame.point(P2(3000.0, 1500.0)));
        let b = h.state().screen3(frame.point(P2(3000.0, 2100.0)));
        drag_screen(&mut h, a, b, PointerButton::Primary);
        assert_eq!(h.state().orbit.camera, camera);
        assert!(lines(&h)[0].end().dist(P2(3000.0, 2100.0)) < 20.0);
        assert_eq!(lines(&h)[0].end(), lines(&h)[1].start(), "joined ends move together");
        type_into(&mut h, "End Height", "2400 mm");
        key(&mut h, Key::Enter);
        assert_eq!(lines(&h)[0].end(), P2(3000.0, 2400.0));
        assert_eq!(lines(&h)[1].start(), P2(3000.0, 2400.0));
        assert_eq!(h.state().session.history().len(), history);
        if let Ok(folder) = std::env::var("BIMCRAFT_EXTRUSION_DUMP") {
            std::fs::create_dir_all(&folder).unwrap();
            for (look, dark, light) in [("light", false, false), ("dark", true, false), ("dark-light-canvas", true, true)] {
                h.ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
                h.state_mut().light_canvas = light;
                h.run();
                h.render()
                    .unwrap()
                    .save(std::path::Path::new(&folder).join(format!("extrusion-{}-{look}.png", if rev { "rev" } else { "bimcraft" })))
                    .unwrap();
            }
        }
        h.get_by_label("Finish Edit Mode").scroll_to_me();
        h.run();
        press(&mut h, "Finish Edit Mode");
        assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
        assert_eq!(h.state().session.history().len(), history + 1);
        assert_eq!(field(&h, roof, "extrusion")["profile"], json!([[0.0, 0.0], [3000.0, 2400.0], [6000.0, 0.0]]));
    }
}

/// The plan, and the box of walls floored 200 mm up, its floor being sketched in the 3D view.
fn sketching() -> (Harness<'static, BimApp>, Sheet, Id) {
    let mut h = app(&format!("{BOX}\nfloor {{\"offset\": 200}}"));
    let plan = h.state().sheet;
    let f = ids(&h, "floor")[0];
    press(&mut h, "Default 3D View");
    h.state_mut().selection = vec![f];
    h.run();
    press(&mut h, "Modify | Floors");
    press(&mut h, "Edit Boundary");
    (h, plan, f)
}

fn lines(h: &Harness<BimApp>) -> Vec<SketchLine> {
    h.state().sketch.as_ref().map(|s| s.lines.clone()).unwrap_or_default()
}

fn picked(h: &Harness<BimApp>) -> Vec<usize> {
    h.state().sketch.as_ref().map(|s| s.picked.iter().copied().collect()).unwrap_or_default()
}

/// Where a point of the floor's plane shows in the 3D view.
fn on_floor(h: &Harness<BimApp>, p: P2) -> Pos2 {
    h.state().screen3([p.0, p.1, 200.0])
}

#[test]
fn edit_boundary_in_3d_keeps_the_sketch_there_and_a_corner_dragged_over_the_floors_plane_moves_it() {
    let (mut h, _, f) = sketching();
    assert!(h.query_by_label("Modify | Edit Boundary").is_some());
    assert_eq!(lines(&h).len(), 4, "its outline, as lines");
    h.run_steps(4);
    assert_eq!(h.state().sheet, Sheet::View3D, "the sketch stays in the 3D view: {}", h.state().status.text);
    // Esc puts Line down; the south line, picked in Modify where it shows on the floor's plane.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    let all = lines(&h);
    let low = all.iter().map(|l| l.start().1.min(l.end().1)).fold(f64::INFINITY, f64::min);
    let south = all.iter().position(|l| l.start().1 == low && l.end().1 == low).unwrap();
    let at = on_floor(&h, all[south].middle());
    click_at(&mut h, at);
    assert_eq!(picked(&h), vec![south]);
    assert!(h.state().selection.is_empty(), "a click while sketching picks a line, not an element");
    // Its end dragged a metre out each way: the joined line follows, and the camera stays.
    let corner = all[south].start();
    let to = P2(corner.0 - 1000.0, corner.1 - 1000.0);
    let before = h.state().orbit.camera;
    let (a, b) = (on_floor(&h, corner), on_floor(&h, to));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert_eq!(h.state().orbit.camera, before, "the grip took the drag, not the camera");
    let ends: Vec<P2> = lines(&h).iter().flat_map(|l| [l.start(), l.end()]).collect();
    assert_eq!(ends.iter().filter(|p| p.dist(to) <= 20.0).count(), 2, "{ends:?}");
    assert!(ends.iter().all(|p| p.dist(corner) > 1.0), "{ends:?}");
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    let m = &h.state().session.model;
    let Some(Element::Floor(fl)) = m.elements.get(f) else { panic!("the floor") };
    assert!(fl.boundary.iter().any(|p| p.dist(to) <= 20.0), "{:?}", fl.boundary);
    assert_eq!(h.state().selection, vec![f], "selected again, as Revit leaves it");
}

#[test]
fn in_3d_the_draw_panels_rectangle_draws_a_hole_on_the_floors_plane() {
    let (mut h, _, f) = sketching();
    let before = h.state().orbit.camera;
    press(&mut h, "Rectangle");
    let a = on_floor(&h, P2(2000.0, 2000.0));
    click_at(&mut h, a);
    let b = on_floor(&h, P2(4000.0, 3000.0));
    click_at(&mut h, b);
    assert_eq!(lines(&h).len(), 8, "{}", h.state().status.text);
    assert_eq!(h.state().orbit.camera, before, "clicks draw; they don't turn the view");
    let corners: Vec<P2> = lines(&h)[4..].iter().map(SketchLine::start).collect();
    assert!(corners.iter().any(|p| p.dist(P2(2000.0, 2000.0)) <= 20.0), "on the floor's plane: {corners:?}");
    press(&mut h, "Finish Edit Mode");
    assert!(h.state().sketch.is_none(), "{}", h.state().status.text);
    assert_eq!(field(&h, f, "openings").as_array().map(Vec::len), Some(1));
}

#[test]
fn a_sketch_begun_in_3d_holds_the_window_there_and_cancel_leaves_the_floor_as_it_was() {
    let (mut h, plan, f) = sketching();
    let before = field(&h, f, "boundary");
    h.state_mut().open_sheet(plan);
    h.run();
    assert_eq!(h.state().sheet, Sheet::View3D, "back in the view the sketch began in");
    assert!(h.state().status.error);
    // Delete takes out the line picked; Cancel drops the sketch, the floor as it was.
    key(&mut h, Key::Escape);
    let mid = lines(&h)[0].middle();
    let at = on_floor(&h, mid);
    click_at(&mut h, at);
    key(&mut h, Key::Delete);
    assert_eq!(lines(&h).len(), 3);
    press(&mut h, "Cancel Edit Mode");
    assert!(h.state().sketch.is_none());
    assert_eq!(field(&h, f, "boundary"), before);
    assert_eq!(h.state().selection, vec![f]);
}
