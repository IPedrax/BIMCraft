//! The 3D view on the graphics card: the setting that asks for it, and what the view does with a card (drawn on it, picked through
//! its mesh, handed back to the processor for what the card can't draw). A runner with no graphics adapter skips the ones that
//! need a card, and says so.

use super::*;
use crate::gpu::{Card, headless};
use egui_kittest::kittest::NodeT;

#[test]
fn drawing_on_the_card_is_opt_in_kept_in_the_settings_and_survives_a_new_project() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app("");
    h.state_mut().use_settings(file.clone());
    assert!(!h.state().card.wanted, "off until it's asked for");
    h.state_mut().card.wanted = true;
    h.run();
    let kept: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["gpu_3d"], true);
    h.state_mut().reopened();
    assert!(h.state().card.wanted, "File > New keeps it");
    let mut again = app("");
    again.state_mut().use_settings(file);
    assert!(again.state().card.wanted, "and so does the next start");
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
fn what_the_card_cant_draw_is_drawn_on_the_processor_and_the_page_says_why() {
    let Some(gpu) = headless() else {
        eprintln!("no graphics adapter here: the card's 3D view is not driven");
        return;
    };
    let mut h = app(BOX);
    h.state_mut().card = Card::with(gpu);
    press(&mut h, "Default 3D View");
    assert!(h.state().orbit.on_card);
    // Wireframe: the processor draws every edge through the faces.
    h.state_mut().session.execute("visual_style", &json!({"style": "wireframe"})).unwrap();
    h.run();
    assert!(!h.state().orbit.on_card, "wireframe is the processor's");
    let why = h.state().card.note.clone().unwrap_or_default();
    assert!(why.contains("wireframe"), "{why}");
    // The page tells the user.
    press(&mut h, "Settings");
    press(&mut h, "Performance");
    assert!(h.query_by_label_contains("drawn on the processor").is_some());
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
