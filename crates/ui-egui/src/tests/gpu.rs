//! The 3D view on the graphics card: the setting that asks for it, and what the view does with a card (drawn on it, picked through
//! its mesh, handed back to the processor for what the card can't draw). A runner with no graphics adapter skips the ones that
//! need a card, and says so.

use super::*;
use crate::gpu::{Card, headless};
use egui_kittest::kittest::NodeT;

#[test]
fn drawing_on_the_card_is_on_until_turned_off_kept_in_the_settings_and_survives_a_new_project() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    assert!(h.state().card.wanted, "on from the start");
    h.state_mut().card.wanted = false;
    h.run();
    let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["gpu_3d"], false);
    h.state_mut().reopened();
    assert!(!h.state().card.wanted, "File > New keeps it");
    let mut again = app("");
    again.state_mut().use_settings(file);
    assert!(!again.state().card.wanted, "and so does the next start");
}

#[test]
fn the_performance_page_says_when_there_is_no_card_and_leaves_the_check_box_greyed() {
    let mut h = app("");
    press(&mut h, "Settings");
    press(&mut h, "Performance");
    assert!(h.query_by_label("This copy has no graphics card it can draw the 3D view on").is_some());
    let on_card = h.get_by_role_and_label(Role::CheckBox, "Draw the 3D view on the graphics card");
    assert!(on_card.accesskit_node().is_disabled(), "nothing to turn on");
}

#[test]
fn the_3d_view_on_the_card_picks_what_it_shows_as_the_processors_does() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    assert!(h.state().orbit.on_card, "the card drew it: {:?}", h.state().card.note);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let door = find(&h, |e| matches!(e, Element::Door(_)));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall], "the south wall's face");
    let at = h.state().screen3([2000.0, -25.0, 1000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [door], "the door, through its opening");
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    assert!(h.state().selection.is_empty(), "the background selects nothing");
    assert!(h.state().orbit.on_card, "and it is still the card's picture");
}

#[test]
fn hidden_line_stays_on_the_card_and_picks_what_it_shows() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(BOX);
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    h.state_mut().run("visual_style", json!({"style": "hidden_line"}));
    h.run();
    assert!(h.state().orbit.on_card, "the card draws it: {:?}", h.state().card.note);
    assert_eq!(h.state().card.note, None);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall]);
}

#[test]
fn wireframe_stays_on_the_card_and_picks_what_it_shows() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(BOX);
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    h.state_mut().run("visual_style", json!({"style": "wireframe"}));
    h.run();
    assert!(h.state().orbit.on_card, "the card draws it: {:?}", h.state().card.note);
    assert_eq!(h.state().card.note, None);
    // The south wall's top outer edge: within two pixels of it, the wall; in the middle of a face, nothing.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let empty = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, empty);
    assert!(h.state().selection.is_empty(), "no line there, whatever face it is: {:?}", h.state().selection);
    let at = h.state().screen3([5000.0, -100.0, 3000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall]);
    assert!(h.state().orbit.on_card);
}

/// The mesh the view draws from now.
fn mesh_now(h: &Harness<BimApp>) -> std::sync::Arc<bimcraft_core::view3d::gpu::Mesh> {
    match h.state().orbit.mesh.as_ref() {
        Some((_, Ok(mesh))) => std::sync::Arc::clone(mesh),
        other => panic!("no mesh: {}", other.is_some()),
    }
}

#[test]
fn a_section_box_moved_keeps_the_mesh_and_picks_through_the_new_box_and_an_edit_makes_a_new_one() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(BOX);
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    let first = mesh_now(&h);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let high = h.state().screen3([5000.0, -100.0, 2600.0]);
    click_at(&mut h, high);
    assert_eq!(h.state().selection, [wall], "no box: the wall is there");
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    h.state_mut().run("section_box", json!({"min": [-1000, -1000, -1000], "max": [11000, 7000, 1500]}));
    h.run();
    assert!(std::sync::Arc::ptr_eq(&first, &mesh_now(&h)), "nothing but the box changed: the same mesh");
    assert!(h.state().orbit.on_card);
    click_at(&mut h, high);
    assert!(!h.state().selection.contains(&wall), "cut away by the new box, which the mesh wasn't made under");
    // Undone, the same mesh again, and the wall comes back.
    h.state_mut().run("undo", json!({}));
    h.run();
    assert!(std::sync::Arc::ptr_eq(&first, &mesh_now(&h)));
    click_at(&mut h, high);
    assert_eq!(h.state().selection, [wall]);
    // A wall is another matter.
    h.state_mut().run("wall", json!({"points": [[0, 9000], [3000, 9000]]}));
    h.run();
    assert!(!std::sync::Arc::ptr_eq(&first, &mesh_now(&h)), "made again for what changed");
}

#[test]
fn a_view_being_made_keeps_the_last_picture_quietly_then_faded_with_its_bar_and_goes_on_from_there() {
    use crate::orbit::Waiting;
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().orbit.waiting, None);
    // An edit, its scene being made.
    h.state_mut().orbit.hold_build = true;
    h.state_mut().run("wall", json!({"points": [[0, 9000], [3000, 9000]]}));
    // One frame: a wait quiet at first asks for a repaint after a moment, so run() would chase it to its step limit.
    h.step();
    assert!(matches!(h.state().orbit.waiting, Some(Waiting::Quiet | Waiting::Faded)), "the last picture stays");
    let moment = std::time::Duration::from_millis(50);
    let long = std::time::Duration::from_secs(1);
    assert_eq!(Waiting::after(moment, true, false), Waiting::Quiet, "a moment: as it was");
    assert_eq!(Waiting::after(long, true, false), Waiting::Faded);
    assert_eq!(Waiting::after(moment, true, true), Waiting::Faded, "something wrong is said at once");
    assert_eq!(Waiting::after(moment, false, false), Waiting::Bare, "with no picture, the words at once");
    assert!(h.query_by_label("Zoom to Fit").is_some(), "its bar stays");
    // Longer: faded, and it says so.
    h.state_mut().orbit.waiting_since = std::time::Instant::now().checked_sub(long);
    h.run();
    assert_eq!(h.state().orbit.waiting, Some(Waiting::Faded));
    assert!(h.query_by_label("Zoom to Fit").is_some());
    // Made: back, and picking.
    h.state_mut().orbit.hold_build = false;
    h.state_mut().orbit.requested = None;
    h.run();
    assert_eq!(h.state().orbit.waiting, None);
    assert_eq!(h.state().orbit.waiting_since, None, "the next wait starts afresh");
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall]);
}

#[test]
fn a_view_with_no_picture_yet_says_it_is_being_made_on_the_paper() {
    use crate::orbit::Waiting;
    let mut h = app(BOX);
    h.state_mut().orbit.hold_build = true;
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().orbit.waiting, Some(Waiting::Bare));
}

#[test]
fn a_section_box_s_grip_changes_colour_under_the_pointer_and_picks_nothing_behind_it() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    h.state_mut().run("section_box", json!({"min": [-1000, -1000, -1000], "max": [9000, 7000, 3500]}));
    h.run();
    // The south face's grip, which lies out in front of the wall.
    let (lo, hi) = bimcraft_core::graphics::Look::of_3d(&h.state().session.model).section_box().unwrap();
    let grips = bimcraft_core::view3d::box_grips((lo, hi));
    let k = 2;
    let on = h.state().screen3(grips[k]);
    h.hover_at(on);
    h.run();
    h.run();
    assert_eq!(h.state().orbit.hot_grip, Some(k));
    let off = h.state().orbit.rect.min + vec2(4.0, 4.0);
    h.hover_at(off);
    h.run();
    h.run();
    assert_eq!(h.state().orbit.hot_grip, None);
    let plain = crate::section_box::grip_colours(k, None, None);
    let hot = crate::section_box::grip_colours(k, None, Some(k));
    let held = crate::section_box::grip_colours(k, Some(k), None);
    assert_ne!(plain.0, hot.0, "under the pointer, another fill");
    assert_ne!(hot.0, held.0, "and held another again");
    assert_eq!(crate::section_box::grip_colours(k, None, Some(k + 1)), plain, "only the one under the pointer");
}

#[test]
fn a_section_box_stays_on_the_card_and_what_it_cuts_away_is_not_picked() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(BOX);
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    h.state_mut().run("section_box", json!({"min": [-1000, -1000, -1000], "max": [11000, 7000, 1500]}));
    h.run();
    assert!(h.state().orbit.on_card, "the card cuts it: {:?}", h.state().card.note);
    assert_eq!(h.state().card.note, None);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let kept = h.state().screen3([5000.0, -100.0, 1000.0]);
    click_at(&mut h, kept);
    assert_eq!(h.state().selection, [wall], "below the top of the box");
    let gone = h.state().screen3([5000.0, -100.0, 2600.0]);
    let corner = h.state().orbit.rect.min + vec2(4.0, 4.0);
    click_at(&mut h, corner);
    click_at(&mut h, gone);
    assert!(!h.state().selection.contains(&wall), "above it the wall is cut away");
    assert!(h.state().orbit.on_card);
}

#[test]
fn a_scan_stays_on_the_card_and_picks_its_points() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(BOX);
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    // A face of points 2 m square, its foot on the level, out in front of the south wall.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scan.xyz");
    let text: String =
        (0..=100).flat_map(|i| (0..=100).map(move |k| format!("{} 0 {}\n", f64::from(i) * 0.02, f64::from(k) * 0.02))).collect();
    std::fs::write(&path, text).unwrap();
    h.state_mut().run("point_cloud", json!({"path": path, "at": [4000, -2000]}));
    h.run();
    assert!(h.state().orbit.on_card, "the card draws it: {:?}", h.state().card.note);
    assert_eq!(h.state().card.note, None);
    let scan = find(&h, |e| matches!(e, Element::PointCloud(_)));
    let at = h.state().screen3([4000.0, -2000.0, 1000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [scan]);
    assert!(h.state().orbit.on_card);
}

#[test]
fn a_card_that_failed_leaves_the_view_on_the_processor_and_the_page_says_why() {
    let mut h = app(BOX);
    h.state_mut().card.failed = true;
    h.state_mut().card.note = Some("the card ran away".into());
    press(&mut h, "Default 3D View");
    assert!(!h.state().orbit.on_card);
    press(&mut h, "Settings");
    press(&mut h, "Performance");
    assert!(h.query_by_label_contains("drawn on the processor").is_some());
    assert!(h.query_by_label_contains("the card ran away").is_some());
}

#[test]
fn the_card_keeps_the_3d_view_when_graphic_display_options_change_it() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(BOX);
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    shortcut(&mut h, "GD");
    pick(&mut h, Role::CheckBox, "Show Edges");
    pick(&mut h, Role::CheckBox, "Show Ambient Shadows");
    press(&mut h, "OK");
    let d = h.state().session.model.default_3d().and_then(|v| match h.state().session.model.elements.get(v) {
        Some(Element::View3D(v)) => Some(v.display),
        _ => None,
    });
    assert_eq!(d.map(|d| (d.edges, d.ambient_shadows)), Some((false, true)));
    assert!(h.state().orbit.on_card, "the card draws them: {:?}", h.state().card.note);
}

#[test]
fn what_the_view_sees_through_is_drawn_on_the_card_and_picked_through_its_mesh() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    h.state_mut().session.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
    h.run();
    assert!(h.state().orbit.on_card, "the card draws it: {:?}", h.state().card.note);
    // The wall seen through is the one in front, as the processor's picture picks it.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    let at = h.state().screen3([5000.0, -100.0, 1500.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall], "the south wall, seen through");
    // The door is opaque behind nothing: it picks as before.
    let door = find(&h, |e| matches!(e, Element::Door(_)));
    let at = h.state().screen3([2000.0, -25.0, 1000.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [door]);
    assert!(h.state().orbit.on_card);
}
