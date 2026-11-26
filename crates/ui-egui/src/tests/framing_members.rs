//! Framing in the desktop: Tab picks a framed wall's members in a plan and in 3D, Properties resizes one, Delete leaves it
//! out and Reset Members brings it back; the wall type's layers mark the one framed; the Framing drop-down.

use super::*;
use egui_kittest::kittest::{By, NodeT};

/// A framed 2x4 wall, 4 m east from the origin, 2700 tall, selected.
const FRAMED: &str = r#"wall {"points": [[0, 0], [4000, 0]], "type": "Timber Frame - 114mm", "height": 2700}
frame {"ids": [100002]}"#;

fn framed() -> (Harness<'static, BimApp>, Id) {
    let mut h = app(FRAMED);
    let id = 100002;
    assert!(h.state().session.model.wall(id).is_some_and(|w| w.framing.is_some()), "{}", h.state().status.text);
    h.state_mut().selection = vec![id];
    h.run();
    (h, id)
}

#[test]
fn tab_picks_the_framing_member_under_the_cursor_in_a_plan_and_back_to_the_wall() {
    let (mut h, id) = framed();
    // The start stud: 38 mm from the wall's start, across its 89 mm layer.
    let at = h.state().screen(P2(19.0, 0.0));
    h.hover_at(at);
    h.run();
    assert_eq!(h.state().framing_aim, Some((id, "stud/start".to_string())), "what Tab would pick, marked");
    key(&mut h, Key::Tab);
    assert_eq!(h.state().framing_member, Some((id, "stud/start".to_string())));
    assert_eq!(h.state().selection, [id], "the wall stays selected");
    assert!(h.query_by_label("Framing Member: Stud").is_some(), "Properties shows the member");
    // Tab again: back to the whole wall (nothing else of the wall's is under it at the cut plane).
    key(&mut h, Key::Tab);
    assert_eq!(h.state().framing_member, None);
    // Selecting something else lets it go.
    key(&mut h, Key::Tab);
    h.state_mut().selection.clear();
    h.run();
    assert_eq!(h.state().framing_member, None);
}

#[test]
fn delete_leaves_the_picked_member_out_never_the_wall_and_reset_members_brings_it_back() {
    let (mut h, id) = framed();
    let at = h.state().screen(P2(19.0, 0.0));
    h.hover_at(at);
    h.run();
    key(&mut h, Key::Tab);
    key(&mut h, Key::Delete);
    let m = &h.state().session.model;
    assert!(m.wall(id).is_some(), "the wall is still there");
    let f = bimcraft_core::framing::wall(m, id).ok().flatten().unwrap();
    assert!(!f.members.iter().any(|x| x.key == "stud/start"), "the stud isn't");
    assert_eq!(h.state().framing_member, None);
    h.run();
    assert!(h.query_by_label("1 framing members edited").is_some());
    press(&mut h, "Reset Members");
    assert!(h.state().session.model.wall(id).is_some_and(|w| w.framing_edits.is_empty()));
}

#[test]
fn properties_doubles_a_picked_stud() {
    let (mut h, id) = framed();
    h.state_mut().framing_member = Some((id, "stud/start".to_string()));
    h.run();
    let plies = h.get_by_role_and_label(Role::SpinButton, "Plies").accesskit_node().id();
    h.get_by(|n| n.id() == plies).focus();
    h.run();
    h.key_press(Key::ArrowUp);
    h.run();
    let e = h.state().session.model.wall(id).map(|w| w.framing_edits.clone()).unwrap_or_default();
    assert_eq!(e.first().map(|x| (x.key.as_str(), x.plies)), Some(("stud/start", Some(2))), "{e:?}");
}

#[test]
fn tab_in_3d_picks_the_member_the_cursors_ray_meets_first() {
    let (mut h, id) = framed();
    browse(&mut h, "{3D}");
    h.run();
    h.state_mut().selection = vec![id];
    h.run();
    // The start stud's middle, 1350 up.
    let at = h.state().screen3([19.0, 0.0, 1350.0]);
    h.hover_at(at);
    h.run();
    key(&mut h, Key::Tab);
    let picked = h.state().framing_member.clone();
    assert!(picked.as_ref().is_some_and(|(w, k)| *w == id && k == "stud/start"), "{picked:?}");
}

#[test]
fn a_wall_types_layers_mark_the_structure_layer_framed_one_at_most() {
    let mut h = app(
        r#"type {"category": "wall", "name": "Two cores", "layers": [{"function": "structure", "material": "Concrete", "thickness": 150}, {"function": "structure", "material": "Pine", "thickness": 89}]}
wall {"points": [[0, 0], [4000, 0]], "type": "Two cores"}"#,
    );
    h.state_mut().selection = vec![100002];
    h.run();
    press(&mut h, "Edit Type");
    let boxes = |h: &Harness<BimApp>| {
        h.query_all(By::new().role(Role::CheckBox).label("Framed")).map(|n| n.accesskit_node().id()).collect::<Vec<_>>()
    };
    let (outer, inner) = (boxes(&h)[0], boxes(&h)[1]);
    h.get_by(|n| n.id() == outer).click();
    h.run();
    h.get_by(|n| n.id() == inner).click();
    h.run();
    press(&mut h, "OK");
    let layers = &h.state().session.model.types.wall["Two cores"].layers;
    assert_eq!((layers[0].framed, layers[1].framed), (false, true), "the one ticked last");
}

#[test]
fn the_framing_drop_down_offers_the_starter_types_and_says_when_a_wall_has_nothing_to_frame() {
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]], "type": "Timber Frame - 185mm"}
wall {"points": [[0, 3000], [4000, 3000]]}"#);
    h.state_mut().selection = vec![100002];
    h.run();
    let combo = h.get_by_role_and_label(Role::ComboBox, "Framing").accesskit_node().id();
    h.get_by(|n| n.id() == combo).click();
    h.run();
    press(&mut h, bimcraft_core::framing::EUROPEAN_TYPE);
    let w = h.state().session.model.wall(100002).cloned().unwrap();
    assert_eq!(w.framing.as_deref(), Some(bimcraft_core::framing::EUROPEAN_TYPE));
    // A generic wall has no structure layer: a hint, not a drop-down that refuses.
    h.state_mut().selection = vec![100003];
    h.run();
    assert!(h.query_by_label("Type Generic - 200mm has no Structure layer to frame").is_some());
}
