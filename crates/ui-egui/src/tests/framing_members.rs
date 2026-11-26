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
    let m = &h.state().session.model;
    assert!(m.wall(id).is_some_and(|w| m.wall_framing(w).is_some()), "{}", h.state().status.text);
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
    let plies = h.get_by_role_and_label(Role::TextInput, "Plies").accesskit_node().id();
    h.get_by(|n| n.id() == plies).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_by(|n| n.id() == plies).type_text("2");
    h.run();
    key(&mut h, Key::Enter);
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
    // Each looked up again before it's clicked: ticking one takes away the warning over the list, and the boxes move.
    let outer = boxes(&h)[0];
    h.get_by(|n| n.id() == outer).click();
    h.run();
    let inner = boxes(&h)[1];
    h.get_by(|n| n.id() == inner).click();
    h.run();
    press(&mut h, "OK");
    let layers = &h.state().session.model.types.wall["Two cores"].layers;
    assert_eq!((layers[0].framed, layers[1].framed), (false, true), "the one ticked last");
}

#[test]
fn frame_opens_the_wall_types_properties_whose_framing_needs_a_layer_marked_framed() {
    // A generic wall's type has no layer marked Framed: its Framing says so and can't be picked.
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]]}"#);
    h.state_mut().selection = vec![100002];
    h.run();
    press(&mut h, "Structure");
    press(&mut h, "Frame");
    assert!(h.state().type_dialog.is_some(), "Frame opens the wall's type");
    assert!(h.query_by_label_contains("No layer is marked Framed").is_some());
    assert!(h.get_by_role_and_label(Role::ComboBox, "Framing").accesskit_node().is_disabled(), "nothing to pick until a layer is marked");
    assert!(h.query_by_label("Remove Framing").is_none(), "no Remove Framing on the ribbon");
}

#[test]
fn framing_a_wall_type_frames_every_wall_of_it_and_its_framed_layer_takes_the_framing_depth() {
    // KVH's 160 mm layer framed as 2x6s, 140 deep: the layer and the type follow.
    let mut h = app(r#"wall {"points": [[0, 0], [4000, 0]], "type": "Timber Frame - 185mm"}
wall {"points": [[0, 3000], [4000, 3000]], "type": "Timber Frame - 185mm"}"#);
    h.state_mut().selection = vec![100002];
    h.run();
    press(&mut h, "Structure");
    press(&mut h, "Frame");
    choose(&mut h, "Framing", bimcraft_core::framing::EXTERIOR_TYPE);
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let t = &m.types.wall["Timber Frame - 185mm"];
    assert_eq!(t.framing.as_deref(), Some(bimcraft_core::framing::EXTERIOR_TYPE), "{}", h.state().status.text);
    assert_eq!((t.layers[1].thickness, t.thickness), (140.0, 165.0), "the framed layer as deep as 2x6s");
    for id in [100002, 100003] {
        assert!(bimcraft_core::framing::wall(m, id).ok().flatten().is_some(), "wall {id} framed with its type");
    }
}

/// The framing type's Type Properties: Structure > Frame opens the selected wall's type, and the button beside its Framing.
fn framing_type_dialog(h: &mut Harness<BimApp>) {
    press(h, "Structure");
    press(h, "Frame");
    h.get_by_label("Edit Framing Type").scroll_to_me();
    h.run();
    press(h, "Edit Framing Type");
}

#[test]
fn a_framing_types_picture_names_rows_the_list_has_and_its_headers_are_edited_there() {
    use crate::framing_diagram::Part;
    let (mut h, _) = framed();
    framing_type_dialog(&mut h);
    // Every row a member of the picture lights is a row of the list.
    for part in [
        Part::Stud,
        Part::TopPlate,
        Part::BottomPlate,
        Part::King,
        Part::Jack,
        Part::Header,
        Part::Cripple,
        Part::Sill,
        Part::Blocking,
        Part::Window,
        Part::Door,
    ] {
        for label in part.rows() {
            assert!(h.query_all(By::new().label(label)).next().is_some(), "{part:?}: no row {label}");
        }
    }
    // Picked in the picture, it's named under it.
    if let Some(f) = h.state_mut().type_dialog.as_mut().and_then(|d| d.framing.as_mut()) {
        f.part = Some(Part::Header);
        f.seek = true;
    }
    h.run();
    assert!(h.query_all(By::new().label("Header")).next().is_some());
    // Target Depth and the first header's depth, typed.
    let set = |h: &mut Harness<BimApp>, label: &str, value: &str| {
        let field = h.query_all(By::new().role(Role::TextInput).label(label)).next().map(|n| n.accesskit_node().id()).unwrap();
        h.get_by(|n| n.id() == field).focus();
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.get_by(|n| n.id() == field).type_text(value);
        h.run();
    };
    set(&mut h, "Header Depth", "235");
    set(&mut h, "Depth", "140");
    press(&mut h, "OK");
    let t = &h.state().session.model.types.framing[bimcraft_core::framing::DEFAULT_TYPE];
    assert_eq!((t.headers[0].depth, t.depth), (235.0, 140.0), "{}", h.state().status.text);
}
