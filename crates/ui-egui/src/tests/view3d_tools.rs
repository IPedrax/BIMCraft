//! The 3D view's tools: Temporary Hide/Isolate, Hide in View, and the like, picked in the 3D view as a user picks them.
use super::*;

const TWO: &str = r#"wall {"points": [[0,0],[8000,0]], "height": 3000}
wall {"points": [[0,3000],[8000,3000]], "height": 3000}"#;

/// What a click at model point `q` in the 3D view selects.
fn picks(h: &mut Harness<BimApp>, q: [f64; 3]) -> Vec<Id> {
    h.state_mut().selection.clear();
    h.run();
    let at = h.state().screen3(q);
    click_at(h, at);
    h.state().selection.clone()
}

fn hide_and_isolate(h: &mut Harness<BimApp>) {
    press(h, "Default 3D View");
    let walls = ids(h, "wall");
    let (front, back) = (walls[0], walls[1]);
    // From the default corner, south-west and up: the south wall's face, and the north wall's over it.
    let on_front = [4000.0, -100.0, 1500.0];
    let on_back = [4000.0, 3000.0, 2900.0];
    assert_eq!(picks(h, on_front), [front]);
    for letters in ["HH", "HI"] {
        let hidden = if letters == "HH" { front } else { back };
        h.state_mut().selection = vec![front];
        shortcut(h, letters);
        for _ in 0..4 {
            h.run();
        }
        assert!(h.state().temporary().is_some_and(|t| t.hides()), "{letters} hides something for now");
        let shown = h.state_mut().view_model();
        let look = bimcraft_core::graphics::Look::of_3d_view(&shown, h.state().active_3d());
        assert!(!look.shows(hidden), "{letters}: the view's look hides {hidden}");
        // A click where the hidden wall was finds what is behind it, or nothing: never the hidden wall.
        let at = if hidden == front { on_front } else { on_back };
        assert!(!picks(h, at).contains(&hidden), "{letters}: the hidden wall is still picked");
        shortcut(h, "HR");
        for _ in 0..4 {
            h.run();
        }
        assert_eq!(picks(h, at), [hidden], "{letters}: back after HR");
    }
}

#[test]
fn temporary_hide_and_isolate_hide_what_the_3d_view_draws_and_picks() {
    let mut h = app(TWO);
    hide_and_isolate(&mut h);
}

#[test]
fn temporary_hide_and_isolate_hide_what_the_card_draws_and_picks() {
    let Some(gpu) = crate::gpu::headless() else {
        eprintln!("no graphics adapter here: the card's hide and isolate are not driven");
        return;
    };
    let mut h = app(TWO);
    h.state_mut().card = crate::gpu::Card::with(gpu);
    press(&mut h, "Default 3D View");
    assert!(h.state().orbit.on_card, "{:?}", h.state().card.note);
    hide_and_isolate(&mut h);
    assert!(h.state().orbit.on_card);
}

#[test]
fn a_right_click_puts_what_is_selected_on_another_level_where_it_stands() {
    let mut h = app(&format!("{TWO}\nlevel {{\"elevation\": 3000, \"name\": \"Upper\"}}"));
    press(&mut h, "Default 3D View");
    let front = ids(&h, "wall")[0];
    let on_front = h.state().screen3([4000.0, -100.0, 1500.0]);
    click_at(&mut h, on_front);
    assert_eq!(h.state().selection, [front]);
    let base = |h: &Harness<BimApp>| {
        let m = &h.state().session.model;
        let w = m.wall(front).unwrap();
        (w.level, m.base_z(w), m.top_z(w))
    };
    let (level, bottom, top) = base(&h);
    right_click_at(&mut h, on_front);
    choose_item(&mut h, "Change Base Level");
    choose_item(&mut h, "Upper");
    let (now, b, t) = base(&h);
    assert_ne!(now, level, "on the upper level now");
    assert_eq!((b, t), (bottom, top), "where it stood");
    assert_eq!(h.state().session.model.wall(front).unwrap().base_offset, -3000.0);
    // A door stands on its wall's level: nothing to offer.
    h.state_mut().exec("door", &json!({"at": [2000, 0]})).unwrap();
    h.state_mut().selection = ids(&h, "door");
    right_click_at(&mut h, on_front);
    assert!(h.query_by_label("Change Base Level").is_none());
}

#[test]
fn a_wall_clicked_then_hidden_for_now_is_gone_from_the_card_with_the_pointer_still_on_it() {
    let Some(gpu) = crate::gpu::headless() else {
        eprintln!("no graphics adapter here");
        return;
    };
    let mut h = app(TWO);
    h.state_mut().card = crate::gpu::Card::with(gpu);
    press(&mut h, "Default 3D View");
    let front = ids(&h, "wall")[0];
    let at = h.state().screen3([4000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [front]);
    shortcut(&mut h, "HH");
    for _ in 0..6 {
        h.hover_at(at);
        h.run();
    }
    let tints = h.state().orbit.tints_shown();
    let mesh_has = h.state().orbit.mesh_shows(front);
    assert!(!tints.iter().any(|(id, _)| *id == front), "the hidden wall is tinted: {tints:?}");
    assert_eq!(mesh_has, Some(false));
}

#[test]
fn the_door_and_window_tools_show_the_opening_on_the_wall_under_the_cursor_and_a_click_places_it() {
    let mut h = app(TWO);
    press(&mut h, "Default 3D View");
    let front = ids(&h, "wall")[0];
    shortcut(&mut h, "DR");
    assert_eq!(h.state().tool, Tool::Door, "the Door tool stays on in the 3D view");
    let at = h.state().screen3([3000.0, -100.0, 1200.0]);
    for _ in 0..3 {
        h.hover_at(at);
        h.run();
    }
    // Its shadow where the cursor is, on the wall: as wide as a door and as tall; the project has none yet.
    let ghost = h.state().opening_ghost().expect("a shadow of the door to come");
    let xs: Vec<f64> = ghost.iter().map(|q| q[0]).collect();
    let (lo, hi) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    assert!(lo < 3000.0 && hi > 3000.0 && hi - lo > 600.0, "about the cursor: {lo}..{hi}");
    assert!(ids(&h, "door").is_empty(), "and isn't the project's yet");
    let undo = h.state().session.history().len();
    click_at(&mut h, at);
    let doors = ids(&h, "door");
    assert_eq!(doors.len(), 1, "a click places it");
    assert_eq!(h.state().session.history().len(), undo + 1, "as one command");
    let m = &h.state().session.model;
    let Some(Element::Door(d)) = m.elements.get(doors[0]) else { panic!() };
    assert_eq!(m.elements.id(d.host), Some(front));
    assert!((d.offset - 3000.0).abs() < 600.0, "where the cursor was along the wall: {}", d.offset);
    // A window, its sill its type's.
    shortcut(&mut h, "WN");
    let at = h.state().screen3([6000.0, -100.0, 1500.0]);
    for _ in 0..3 {
        h.hover_at(at);
        h.run();
    }
    assert!(h.state().opening_ghost().is_some(), "a window's shadow");
    click_at(&mut h, at);
    assert_eq!(ids(&h, "window").len(), 1);
    // Off every wall: nothing to place, and nothing placed.
    shortcut(&mut h, "DR");
    let sky = h.state().orbit.rect.left_top() + egui::vec2(20.0, 20.0);
    h.hover_at(sky);
    h.run();
    h.state_mut().orbit_forget_opening();
    click_at(&mut h, sky);
    assert_eq!(ids(&h, "door").len(), 1);
}

/// Hover at model point `q` in the 3D view for a few frames, then click there.
fn click3(h: &mut Harness<BimApp>, q: [f64; 3]) {
    let at = h.state().screen3(q);
    for _ in 0..3 {
        h.hover_at(at);
        h.run();
    }
    click_at(h, at);
}

fn wall_line(h: &Harness<BimApp>, id: Id) -> (P2, P2) {
    let w = h.state().session.model.wall(id).unwrap();
    (w.start, w.end)
}

#[test]
fn move_rotate_and_mirror_work_in_the_3d_view_with_the_result_shown_before_the_click() {
    let mut h = app(TWO);
    press(&mut h, "Default 3D View");
    let front = ids(&h, "wall")[0];
    h.state_mut().selection = vec![front];
    shortcut(&mut h, "MV");
    assert!(matches!(h.state().tool, Tool::Move(None)), "Move stays on in the 3D view");
    // Its base point on the ground, then where it goes: shown moved before the click, moved by it.
    click3(&mut h, [1000.0, -1000.0, 0.0]);
    assert!(matches!(h.state().tool, Tool::Move(Some(_))));
    let to = h.state().screen3([1000.0, -2000.0, 0.0]);
    for _ in 0..3 {
        h.hover_at(to);
        h.run();
    }
    let ghosts = h.state().edit_ghosts([1000.0, -2000.0, 0.0]);
    let ys: Vec<f64> = ghosts.iter().flat_map(|g| g.ring.iter().map(|q| q[1])).collect();
    let middle = ys.iter().sum::<f64>() / ys.len().max(1) as f64;
    assert!((middle + 1000.0).abs() < 200.0, "its shadow shows it moved: {ys:?}");
    assert_eq!(wall_line(&h, front).0, P2(0.0, 0.0), "the project hasn't moved yet");
    click_at(&mut h, to);
    let (a, _) = wall_line(&h, front);
    assert!(a.dist(P2(0.0, -1000.0)) < 50.0, "moved: {a:?}");
    assert_eq!(h.state().tool, Tool::Modify);
    // Rotate a quarter turn about its middle.
    h.state_mut().selection = vec![front];
    shortcut(&mut h, "RO");
    let Tool::Rotate { centre, .. } = h.state().tool else { panic!("{:?}", h.state().tool) };
    click3(&mut h, [centre.0 + 2000.0, centre.1, 0.0]);
    click3(&mut h, [centre.0, centre.1 + 2000.0, 0.0]);
    let (a, b) = wall_line(&h, front);
    assert!((a.0 - b.0).abs() < 50.0, "turned upright in plan: {a:?} {b:?}");
    // Mirror, drawing its axis: a copy on the far side.
    h.state_mut().selection = vec![front];
    shortcut(&mut h, "DM");
    assert!(matches!(h.state().tool, Tool::Mirror { draw: true, .. }));
    click3(&mut h, [-3000.0, 6000.0, 0.0]);
    click3(&mut h, [9000.0, 6000.0, 0.0]);
    assert_eq!(ids(&h, "wall").len(), 3, "a mirrored copy");
}

#[test]
fn turned_to_a_side_the_3d_view_works_on_a_plane_facing_it_moves_go_up_and_levels_are_clicked() {
    let mut h = app(TWO);
    press(&mut h, "Default 3D View");
    let front = ids(&h, "wall")[0];
    // From the front, as the triad turns it.
    let cam = h.state().orbit.camera.unwrap();
    h.state_mut().orbit.camera = Some(bimcraft_core::triad::turned(cam, bimcraft_core::triad::End::MinusY));
    h.run();
    let y = h.state().orbit.camera.unwrap().target[1];
    h.state_mut().selection = vec![front];
    shortcut(&mut h, "MV");
    assert!(h.state().facing_now().is_some(), "the view faces the front");
    click3(&mut h, [2000.0, y, 0.0]);
    click3(&mut h, [2000.0, y, 1000.0]);
    let m = &h.state().session.model;
    let w = m.wall(front).unwrap();
    assert!((m.base_z(w) - 1000.0).abs() < 20.0, "raised: {}", m.base_z(w));
    assert!(w.start.dist(P2(0.0, 0.0)) < 20.0, "and not moved across: {:?}", w.start);
    // A level where the click is.
    let before = h.state().session.model.levels().len();
    shortcut(&mut h, "LL");
    assert_eq!(h.state().tool, Tool::Level);
    click3(&mut h, [2000.0, y, 4500.0]);
    let levels = h.state().session.model.levels();
    assert_eq!(levels.len(), before + 1);
    assert!(
        levels.iter().any(|(_, l)| (l.elevation - 4500.0).abs() < 30.0),
        "{:?}",
        levels.iter().map(|(_, l)| l.elevation).collect::<Vec<_>>()
    );
    // Orbited away, the view's own plane is back.
    let cam = h.state().orbit.camera.unwrap();
    h.state_mut().orbit.camera = Some(bimcraft_core::view3d::Camera { azimuth: cam.azimuth + 30.0, elevation: 30.0, ..cam });
    assert!(h.state().facing_now().is_none());
}

#[test]
fn in_a_section_a_level_is_placed_by_a_click_copied_up_and_moved_down() {
    let mut h = app_at(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}"), 0.05);
    browse(&mut h, "Section 1");
    let elevations = |h: &Harness<BimApp>| -> Vec<f64> { h.state().session.model.levels().iter().map(|(_, l)| l.elevation).collect() };
    let before = elevations(&h);
    shortcut(&mut h, "LL");
    assert_eq!(h.state().tool, Tool::Level);
    for _ in 0..3 {
        h.run();
    }
    click(&mut h, P2(3000.0, 1500.0));
    let now = elevations(&h);
    assert_eq!(now.len(), before.len() + 1, "{now:?} {:?} {:?}", h.state().status, h.state().tool);
    let made = now.iter().copied().find(|z| !before.contains(z)).unwrap();
    assert!((made - 1500.0).abs() <= 20.0, "at the click's height: {made}");
    // Copied a metre up: a new level, named after it.
    key(&mut h, Key::Escape);
    let level = h.state().session.model.levels().into_iter().find(|(_, l)| l.elevation == made).unwrap().0;
    h.state_mut().selection = vec![level];
    shortcut(&mut h, "CO");
    click(&mut h, P2(3000.0, made));
    click(&mut h, P2(3000.0, made + 1000.0));
    let now = elevations(&h);
    assert!(now.iter().any(|z| (z - made - 1000.0).abs() <= 20.0), "a copy a metre up: {now:?}");
    // Moved 500 down.
    h.state_mut().selection = vec![level];
    shortcut(&mut h, "MV");
    click(&mut h, P2(3000.0, made));
    click(&mut h, P2(3000.0, made - 500.0));
    let z = h.state().session.model.elevation(h.state().session.model.guid(level));
    assert!((z - made + 500.0).abs() <= 20.0, "moved down: {z}");
}

#[test]
fn the_3d_views_properties_open_the_background_dialog_and_ok_sets_it() {
    use bimcraft_core::view3d::background::Background;
    let mut h = app(TWO);
    press(&mut h, "Default 3D View");
    let now = |h: &Harness<BimApp>| crate::background::of(&h.state().session.model, h.state().active_3d());
    assert_eq!(now(&h), Background::default());
    press(&mut h, "None…");
    assert!(h.state().background_dialog.is_some());
    pick(&mut h, Role::RadioButton, "Gradient");
    // A fourth colour, and the horizon with its ground.
    press(&mut h, "Add Color");
    pick(&mut h, Role::CheckBox, "Horizon");
    press(&mut h, "OK");
    assert!(h.state().background_dialog.is_none());
    let b = now(&h);
    assert_eq!(b.fill.kind(), "gradient", "one command sets it");
    let bimcraft_core::view3d::background::Fill::Gradient { stops } = &b.fill else { panic!() };
    assert_eq!(stops.len(), 4);
    assert!(stops.iter().any(|s| s.at == 75.0), "halfway between the last two: {stops:?}");
    assert!(b.ground.is_some(), "the horizon is on");
    // The row says what it is now; Cancel changes nothing.
    press(&mut h, "Gradient…");
    pick(&mut h, Role::RadioButton, "Solid Color");
    press(&mut h, "Cancel");
    assert_eq!(now(&h).fill.kind(), "gradient");
    // Back to none.
    press(&mut h, "Gradient…");
    pick(&mut h, Role::RadioButton, "None");
    pick(&mut h, Role::CheckBox, "Horizon");
    press(&mut h, "OK");
    assert_eq!(now(&h), Background::default());
}

#[test]
fn a_click_on_the_view_control_bar_keeps_the_selection_for_its_menus() {
    let mut h = app(TWO);
    press(&mut h, "Default 3D View");
    let front = ids(&h, "wall")[0];
    let on_front = h.state().screen3([4000.0, -100.0, 1500.0]);
    click_at(&mut h, on_front);
    assert_eq!(h.state().selection, [front]);
    // The glasses, clicked with the pointer as a user does: the press is the bar's, not the view's under it.
    let glasses = h.get_by_label("Temporary Hide/Isolate").rect().center();
    click_at(&mut h, glasses);
    assert_eq!(h.state().selection, [front], "the selection stays");
    let isolate = h.get_by_label("Isolate Element (HI)").rect().center();
    click_at(&mut h, isolate);
    assert!(h.state().temporary().is_some_and(|t| t.hides()), "isolated from the bar's menu");
    // A press anywhere on the bar's card, between its buttons, is no click on the view either.
    h.state_mut().selection = vec![front];
    let bar = h.get_by_label("Temporary Hide/Isolate").rect();
    click_at(&mut h, bar.left_center() - egui::vec2(3.0, 0.0));
    assert_eq!(h.state().selection, [front]);
}

#[test]
fn on_the_plane_facing_a_side_moves_snap_to_levels_and_constrain_keeps_them_square() {
    let mut h = app(&format!("{TWO}\nlevel {{\"elevation\": 3000, \"name\": \"Upper\"}}"));
    press(&mut h, "Default 3D View");
    let front = ids(&h, "wall")[0];
    let cam = h.state().orbit.camera.unwrap();
    h.state_mut().orbit.camera = Some(bimcraft_core::triad::turned(cam, bimcraft_core::triad::End::MinusY));
    h.run();
    let y = h.state().orbit.camera.unwrap().target[1];
    let base_z = |h: &Harness<BimApp>| {
        let m = &h.state().session.model;
        m.base_z(m.wall(front).unwrap())
    };
    // Its foot onto the level, a little short of it: it snaps to the level's height.
    h.state_mut().selection = vec![front];
    shortcut(&mut h, "MV");
    click3(&mut h, [3000.0, y, 0.0]);
    click3(&mut h, [3000.0, y, 2985.0]);
    assert_eq!(base_z(&h), 3000.0, "snapped to the level");
    // Constrain: a move mostly up goes only up.
    h.state_mut().constrain = true;
    h.state_mut().selection = vec![front];
    shortcut(&mut h, "MV");
    click3(&mut h, [3000.0, y, 3000.0]);
    click3(&mut h, [3400.0, y, 4500.0]);
    let w = h.state().session.model.wall(front).unwrap().clone();
    assert!((base_z(&h) - 4500.0).abs() <= 20.0, "up: {}", base_z(&h));
    assert!(w.start.dist(P2(0.0, 0.0)) < 1.0, "and not along: {:?}", w.start);
}
