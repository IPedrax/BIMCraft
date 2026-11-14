//! A camera view open in the 3D tab is drawn and picked as its own Visibility/Graphics say, not as the default `{3D}`'s.

use super::*;

/// The walls, a camera looking at the south wall, and the default 3D view made; the camera view's id.
fn walk(h: &mut Harness<BimApp>) -> Id {
    h.state_mut().run("camera", json!({"view": "{3D}", "eye": [-6000.0, -9000.0, 6000.0], "target": [4000.0, 3000.0, 1500.0]}));
    h.state_mut().run("camera", json!({"name": "Walk", "eye": [4000.0, -8000.0, 1500.0], "target": [4000.0, 3000.0, 1500.0]}));
    find(h, |e| matches!(e, Element::View3D(v) if v.name == "Walk"))
}

/// The south wall's middle, as the 3D view shows it, clicked in Modify: what that selects.
fn click_south_wall(h: &mut Harness<BimApp>) -> Vec<Id> {
    key(h, Key::Escape);
    h.state_mut().selection.clear();
    h.run();
    let at = h.state().screen3([4000.0, -100.0, 1500.0]);
    click_at(h, at);
    h.state().selection.clone()
}

#[test]
fn a_camera_view_draws_and_picks_as_its_own_graphics_say() {
    let mut h = app(BOX);
    let view = walk(&mut h);
    let walls: Vec<Id> = h.state().session.model.elements.iter().filter(|(_, e)| e.category() == "wall").map(|(id, _)| *id).collect();
    let a_wall = |picked: &[Id]| picked.len() == 1 && walls.contains(&picked[0]);
    h.state_mut().open_3d(Some(view));
    h.run();
    assert_eq!(h.state().active_3d(), Some(view));
    let picked = click_south_wall(&mut h);
    assert!(a_wall(&picked), "the camera sees the south wall: {picked:?}");

    h.state_mut().run("graphics", json!({"view": "Walk", "hide": ["walls"]}));
    h.run();
    assert!(click_south_wall(&mut h).is_empty(), "walls hidden in the camera view aren't drawn there, so nothing is picked");

    // The default 3D view still shows them.
    h.state_mut().open_3d(None);
    h.run();
    let picked = click_south_wall(&mut h);
    assert!(a_wall(&picked), "the default 3D view still sees the south wall: {picked:?}");
}

#[test]
fn hiding_in_the_default_3d_view_leaves_a_camera_view_as_it_was() {
    let mut h = app(BOX);
    let view = walk(&mut h);
    h.state_mut().run("graphics", json!({"view": "{3D}", "hide": ["walls"]}));
    h.state_mut().open_3d(Some(view));
    h.run();
    let picked = click_south_wall(&mut h);
    assert!(!picked.is_empty(), "the camera view sees the walls the default 3D view hides");
}
