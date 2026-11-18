use super::*;
use bimcraft_core::model::Opening;

const WALL: &str = include_str!("window_drag.script");

fn opening(m: &bimcraft_core::model::Model, id: Id) -> Opening {
    let Some(Element::Window(w)) = m.elements.get(id) else { panic!("window {id} missing") };
    w.clone()
}

fn capture(h: &mut Harness<BimApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run();
    h.event(button(from, true));
    h.run();
    h.hover_at(to);
    h.run();
    assert!(h.state().orbit.placement_drag_active(), "window body took the primary drag");
}

#[test]
fn hosted_window_drag_moves_the_cut_and_commits_once() {
    let mut h = app(WALL);
    press(&mut h, "Default 3D View");
    let id = ids(&h, "window")[0];
    let wall = ids(&h, "wall")[0];
    let from = h.state().screen3([2000.0, 0.0, 1500.0]);
    let to = h.state().screen3([3600.0, 0.0, 1850.0]);
    click_at(&mut h, from);
    assert_eq!(h.state().selection, [id]);
    let base = Arc::clone(&h.state().session.model);
    let original = opening(&base, id);
    let guid = base.elements.guid(id);
    let history: Vec<String> = h.state().session.history().into_iter().map(str::to_owned).collect();
    let camera = h.state().orbit.camera.unwrap();
    capture(&mut h, from, to);
    let (trial, scene) = h.state().orbit.scene.as_ref().unwrap();
    let moved = opening(trial, id);
    assert!((moved.offset - 3600.0).abs() <= 1.0 && (moved.sill - 1250.0).abs() <= 1.0, "{moved:?}");
    assert_eq!(moved, Opening { offset: moved.offset, sill: moved.sill, ..original.clone() });
    let image = scene.render(&camera, &[]);
    for (q, expected) in [([2000.0, 0.0, 1500.0], wall), ([3600.0, 0.0, 1850.0], id)] {
        let (x, y, _) = camera.project(q);
        assert_eq!(image.pick(x, y), Some(expected));
    }
    assert!(Arc::ptr_eq(&base, &h.state().session.model));
    assert_eq!(h.state().session.history(), history);
    h.event(button(to, false));
    h.run();
    assert_eq!(opening(&h.state().session.model, id), moved);
    assert_eq!(h.state().orbit.camera, Some(camera));
    assert_eq!(h.state().session.history().len(), history.len() + 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(opening(&h.state().session.model, id), original);
    press(&mut h, "Redo");
    assert_eq!(opening(&h.state().session.model, id), moved);
    let path = std::env::temp_dir().join(format!("window-drag-{}.bimcraft", std::process::id()));
    assert!(h.state_mut().save_to(&path));
    assert!(h.state_mut().open_path(&path));
    assert_eq!(h.state().session.model.elements.guid(id), guid);
    assert_eq!(opening(&h.state().session.model, id), moved);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn hosted_window_drag_shift_escape_and_invalid_release() {
    let mut h = app(WALL);
    press(&mut h, "Default 3D View");
    let id = ids(&h, "window")[0];
    let at = |h: &Harness<BimApp>, x, z| h.state().screen3([x, 0.0, z]);
    let from = at(&h, 2000.0, 1500.0);
    let to = at(&h, 3300.0, 1900.0);
    click_at(&mut h, from);
    let base = Arc::clone(&h.state().session.model);
    let n = h.state().session.history().len();
    capture(&mut h, from, to);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.run();
    let w = opening(&h.state().orbit.scene.as_ref().unwrap().0, id);
    assert_eq!(w.sill, 900.0);
    assert!((w.offset - 3300.0).abs() <= 1.0);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Escape);
    h.run();
    h.event(button(to, false));
    h.run();
    assert!(!h.state().orbit.placement_drag_active());
    assert!(Arc::ptr_eq(&base, &h.state().session.model));
    assert_eq!(h.state().selection, [id]);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    capture(&mut h, from, to);
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT });
    h.run();
    assert_eq!(opening(&h.state().session.model, id), w);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    let now = Arc::clone(&h.state().session.model);
    let from = at(&h, w.offset, 1500.0);
    for to in [at(&h, 6200.0, 1500.0), at(&h, 90.0, 1500.0), at(&h, 4000.0, 400.0)] {
        capture(&mut h, from, to);
        assert!(h.state().status.error, "{}", h.state().status.text);
        h.event(button(to, false));
        h.run();
        assert!(Arc::ptr_eq(&now, &h.state().session.model));
        assert_eq!(h.state().session.history().len(), n + 1);
    }
}

#[test]
fn a_quick_shift_drag_places_the_window_at_its_original_sill() {
    let mut h = app_at(WALL, 0.016);
    press(&mut h, "Default 3D View");
    let id = ids(&h, "window")[0];
    let from = h.state().screen3([2000.0, 0.0, 1500.0]);
    let to = h.state().screen3([3200.0, 0.0, 1800.0]);
    click_at(&mut h, from);
    let held = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::SHIFT };
    deliver(&mut h, vec![Event::ModifiersChanged(Modifiers::SHIFT), held(from, true)]);
    deliver(&mut h, vec![Event::PointerMoved(to)]);
    assert!(h.state().orbit.placement_drag_active());
    deliver(&mut h, vec![held(to, false), Event::ModifiersChanged(Modifiers::NONE)]);
    let placed = opening(&h.state().session.model, id);
    assert!((placed.offset - 3200.0).abs() < 2.0, "{placed:?}");
    assert_eq!(placed.sill, 900.0);
}

#[test]
fn shift_stamped_on_mouse_up_locks_the_sill_at_commit() {
    let mut h = app(WALL);
    press(&mut h, "Default 3D View");
    let id = ids(&h, "window")[0];
    let from = h.state().screen3([2000.0, 0.0, 1500.0]);
    let to = h.state().screen3([3400.0, 0.0, 1830.0]);
    click_at(&mut h, from);
    capture(&mut h, from, to);
    deliver(&mut h, vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT }]);
    let placed = opening(&h.state().session.model, id);
    assert!((placed.offset - 3400.0).abs() < 2.0);
    assert_eq!(placed.sill, 900.0);
}

#[test]
fn selected_wall_location_line_moves_the_whole_host_with_its_openings() {
    let mut h = app(WALL);
    press(&mut h, "Default 3D View");
    let id = ids(&h, "wall")[0];
    let win = ids(&h, "window")[0];
    let door = ids(&h, "door")[0];
    let face = h.state().screen3([4800.0, -100.0, 1500.0]);
    click_at(&mut h, face);
    assert_eq!(h.state().selection, [id]);
    let base = Arc::clone(&h.state().session.model);
    let cam = h.state().orbit.camera;
    let history = h.state().session.history().len();
    let from = h.state().screen3([2700.0, 0.0, 0.0]);
    let to = h.state().screen3([3200.0, 750.0, 0.0]);
    capture(&mut h, from, to);
    let trial = &h.state().orbit.scene.as_ref().unwrap().0;
    let w = trial.wall(id).unwrap().clone();
    assert!(w.start.dist(P2(500.0, 750.0)) < 2.0 && w.end.dist(P2(8500.0, 750.0)) < 2.0, "{w:?}");
    for open in [win, door] {
        let element = trial.elements.get(open).unwrap();
        let reference = base.elements.get(open).unwrap();
        assert_eq!(element, reference, "same instance, host, size and relative position");
        let (Element::Door(o) | Element::Window(o)) = element else { panic!("opening") };
        let new = trial.opening_frame(o).unwrap().0;
        let old = base.opening_frame(o).unwrap().0;
        assert!((new - old).dist(w.start) < 2.0);
    }
    assert!(Arc::ptr_eq(&base, &h.state().session.model));
    key(&mut h, Key::Escape);
    h.event(button(to, false));
    h.run();
    assert!(Arc::ptr_eq(&base, &h.state().session.model));
    assert_eq!(h.state().selection, [id]);
    capture(&mut h, from, to);
    h.event(button(to, false));
    h.run();
    assert_eq!(h.state().session.history().len(), history + 1);
    assert_eq!(h.state().session.model.wall(id), Some(&w));
    assert_eq!(h.state().orbit.camera, cam);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().session.model.wall(id), base.wall(id));
    press(&mut h, "Redo");
    assert_eq!(h.state().session.model.wall(id), Some(&w));
    let saved = Arc::clone(&h.state().session.model);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("moved-wall.bimcraft");
    assert!(h.state_mut().save_to(&path));
    assert!(h.state_mut().open_path(&path));
    for element in [id, win, door] {
        assert_eq!(h.state().session.model.elements.get(element), saved.elements.get(element));
        assert_eq!(h.state().session.model.elements.guid(element), saved.elements.guid(element));
    }
}

#[test]
fn moving_the_joined_wall_line_keeps_its_return_connected() {
    let mut h = app(&format!("{WALL}\nwall {{\"points\":[[0,0],[0,4000]]}}"));
    press(&mut h, "Default 3D View");
    let id = find(&h, |e| matches!(e, Element::Wall(w) if w.end == P2(8000.0,0.0)));
    let neighbor = find(&h, |e| matches!(e, Element::Wall(w) if w.end == P2(0.0,4000.0)));
    let face = h.state().screen3([4800.0, -100.0, 1500.0]);
    click_at(&mut h, face);
    assert_eq!(h.state().selection, [id]);
    let from = h.state().screen3([2700.0, 0.0, 0.0]);
    let to = h.state().screen3([3200.0, 750.0, 0.0]);
    drag_screen(&mut h, from, to, PointerButton::Primary);
    let m = &h.state().session.model;
    let wall = m.wall(id).unwrap();
    assert!(wall.start.dist(P2(0.0, 750.0)) < 2.0 && wall.end.dist(P2(8000.0, 750.0)) < 2.0);
    let other = m.wall(neighbor).unwrap();
    assert_eq!(other.start, wall.start);
    assert_eq!(other.end, P2(0.0, 4000.0));
}

#[test]
fn window_selection_is_visible_after_the_pointer_leaves_and_clears_on_wall_selection() {
    let mut h = app(WALL);
    press(&mut h, "Default 3D View");
    let window = h.state().screen3([2000.0, 0.0, 1500.0]);
    let wall = h.state().screen3([4800.0, -100.0, 1500.0]);
    let labeled = |h: &Harness<BimApp>| {
        h.output().shapes.iter().any(|s| {
        matches!(&s.shape, egui::Shape::Text(text) if text.galley.text() == "Window W-01" && h.state().view_rect.contains(text.pos))
    })
    };
    h.hover_at(window);
    h.run();
    assert!(!labeled(&h));
    click_at(&mut h, window);
    assert!(labeled(&h));
    h.hover_at(wall);
    h.run();
    assert!(labeled(&h));
    click_at(&mut h, wall);
    assert_eq!(h.state().selection, [ids(&h, "wall")[0]]);
    assert!(!labeled(&h));
    click_at(&mut h, window);
    assert!(labeled(&h));
    key(&mut h, Key::Escape);
    assert!(!labeled(&h));
}

#[test]
fn a_model_edit_during_a_drag_cancels_its_stale_preview() {
    let mut h = app(WALL);
    press(&mut h, "Default 3D View");
    let id = ids(&h, "window")[0];
    let from = h.state().screen3([2000.0, 0.0, 1500.0]);
    let to = h.state().screen3([3400.0, 0.0, 1830.0]);
    click_at(&mut h, from);
    capture(&mut h, from, to);
    h.state_mut().exec("modify", &json!({"id": id, "sill": 1000})).unwrap();
    let edited = Arc::clone(&h.state().session.model);
    let history = h.state().session.history().len();
    h.event(button(to, false));
    h.run();
    assert!(!h.state().orbit.placement_drag_active());
    assert!(Arc::ptr_eq(&edited, &h.state().session.model));
    assert_eq!(h.state().session.history().len(), history);
}

#[test]
fn releasing_shift_during_a_drag_restores_vertical_movement() {
    let mut h = app(WALL);
    press(&mut h, "Default 3D View");
    let id = ids(&h, "window")[0];
    let from = h.state().screen3([2000.0, 0.0, 1500.0]);
    let to = h.state().screen3([3300.0, 0.0, 1900.0]);
    click_at(&mut h, from);
    capture(&mut h, from, to);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.run();
    assert_eq!(opening(&h.state().orbit.scene.as_ref().unwrap().0, id).sill, 900.0);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    h.event(button(to, false));
    h.run();
    let moved = opening(&h.state().session.model, id);
    assert!((moved.sill - 1300.0).abs() <= 1.0 && (moved.offset - 3300.0).abs() <= 1.0, "{moved:?}");
}

/// Optional rendered evidence in both profiles and all three canvas looks.
#[test]
fn selected_window_pictures() {
    let Ok(folder) = std::env::var("BIMCRAFT_DRAG_DUMP") else { return };
    let folder = std::path::Path::new(&folder);
    std::fs::create_dir_all(folder).unwrap();
    for (profile_name, profile) in [("rev", crate::style::Profile::Rev), ("bimcraft", crate::style::Profile::BimCraft)] {
        for (look, theme, light_canvas) in [
            ("light", egui::ThemePreference::Light, false),
            ("dark", egui::ThemePreference::Dark, false),
            ("dark-light-canvas", egui::ThemePreference::Dark, true),
        ] {
            let mut h = harness(WALL, 0.25, profile);
            h.ctx.set_theme(theme);
            h.state_mut().light_canvas = light_canvas;
            h.run();
            press(&mut h, "Default 3D View");
            let window = h.state().screen3([2000.0, 0.0, 1500.0]);
            click_at(&mut h, window);
            let wall = h.state().screen3([4800.0, -100.0, 1500.0]);
            h.hover_at(wall);
            h.run();
            h.render().unwrap().save(folder.join(format!("selected-window-{profile_name}-{look}.png"))).unwrap();
        }
    }
}

#[test]
fn the_card_picks_and_drags_the_selected_window_and_wall_location_line() {
    let Some(gpu) = crate::gpu::headless() else {
        eprintln!("no graphics adapter here: the card's placement drag is not driven");
        return;
    };
    let mut h = bimcraft(WALL);
    h.state_mut().card = crate::gpu::Card::with(gpu);
    press(&mut h, "Default 3D View");
    assert!(h.state().orbit.on_card, "{:?}", h.state().card.note);
    let id = ids(&h, "window")[0];
    let from = h.state().screen3([2000.0, 0.0, 1500.0]);
    let to = h.state().screen3([3200.0, 0.0, 1800.0]);
    click_at(&mut h, from);
    assert_eq!(h.state().selection, [id]);
    capture(&mut h, from, to);
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT });
    h.run();
    let moved = opening(&h.state().session.model, id);
    assert_eq!(moved.sill, 900.0);
    assert!((moved.offset - 3200.0).abs() <= 1.0);
    let wall = ids(&h, "wall")[0];
    let face = h.state().screen3([4800.0, -100.0, 1500.0]);
    click_at(&mut h, face);
    assert_eq!(h.state().selection, [wall]);
    let from = h.state().screen3([2700.0, 0.0, 0.0]);
    let to = h.state().screen3([3200.0, 750.0, 0.0]);
    drag_screen(&mut h, from, to, PointerButton::Primary);
    assert!(h.state().session.model.wall(wall).unwrap().start.dist(P2(500.0, 750.0)) < 2.0);
    assert_eq!(opening(&h.state().session.model, id), moved);
    assert!(h.state().orbit.on_card);
}

#[test]
fn a_disallowed_wall_join_leaves_location_line_drag_free() {
    for detach_selected in [true, false] {
        let mut h = app(&format!("{WALL}\nwall {{\"points\":[[0,0],[0,4000]]}}"));
        press(&mut h, "Default 3D View");
        let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.end == P2(8000.0, 0.0)));
        let neighbor = find(&h, |e| matches!(e, Element::Wall(w) if w.end == P2(0.0, 4000.0)));
        let detached = if detach_selected { wall } else { neighbor };
        h.state_mut().exec("modify", &json!({"id": detached, "join": false})).unwrap();
        h.run();
        let face = h.state().screen3([4800.0, -100.0, 1500.0]);
        click_at(&mut h, face);
        let from = h.state().screen3([2700.0, 0.0, 0.0]);
        let to = h.state().screen3([3200.0, 750.0, 0.0]);
        drag_screen(&mut h, from, to, PointerButton::Primary);
        let m = &h.state().session.model;
        assert!(m.wall(wall).unwrap().start.dist(P2(500.0, 750.0)) < 2.0, "{:?}", m.wall(wall));
        assert_eq!(m.wall(neighbor).unwrap().start, P2(0.0, 0.0));
    }
}
