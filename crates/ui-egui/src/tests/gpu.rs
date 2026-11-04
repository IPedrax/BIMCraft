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
