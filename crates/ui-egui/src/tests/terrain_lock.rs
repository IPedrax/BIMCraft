//! The terrain is locked: a plan or the 3D view will not pick, box or drag it by accident. Ctrl+Alt picks it for a click, and Select
//! Terrain in the view control bar picks it as any element.

use super::*;

/// A wall standing on a toposurface of 20 m by 20 m that rises 2 m from west to east: a plan draws it as contours, and the one at the
/// middle (x = 0, height 0) runs north and south across the wall.
const SCRIPT: &str = r#"toposurface {"points": [[-10000, -10000, -1000], [10000, -10000, 1000], [10000, 10000, 1000], [-10000, 10000, -1000]], "interval": 1000}
                        wall {"points": [[-2000, 0], [2000, 0]]}"#;

fn lock() -> Harness<'static, BimApp> {
    app(SCRIPT)
}

/// A point on that contour, away from the wall.
const ON_TERRAIN: P2 = P2(0.0, -8000.0);

fn terrain(h: &Harness<BimApp>) -> Id {
    ids(h, "toposurface")[0]
}

#[test]
fn a_click_on_the_terrain_alone_picks_nothing_and_says_how_to_pick_it_once() {
    let mut h = lock();
    click(&mut h, ON_TERRAIN);
    assert!(h.state().selection.is_empty(), "locked: {:?}", h.state().selection);
    assert!(h.state().status.text.contains("Ctrl+Alt"), "{}", h.state().status.text);
    h.state_mut().status = Default::default();
    click(&mut h, ON_TERRAIN);
    assert!(h.state().status.text.is_empty(), "said once, not again");
}

#[test]
fn ctrl_alt_picks_the_terrain_for_a_click_and_replaces_the_selection() {
    let mut h = lock();
    let wall = ids(&h, "wall")[0];
    click(&mut h, P2(0.0, 0.0));
    assert_eq!(h.state().selection, vec![wall], "a wall on the terrain is picked as ever");
    click_with(&mut h, ON_TERRAIN, Modifiers::CTRL | Modifiers::ALT);
    assert_eq!(h.state().selection, vec![terrain(&h)], "Ctrl+Alt is not Ctrl's add");
}

#[test]
fn select_terrain_picks_it_as_any_element_and_is_the_windows_not_the_projects() {
    let mut h = lock();
    assert!(!h.state().select_terrain);
    pick(&mut h, Role::CheckBox, "Select Terrain");
    assert!(h.state().select_terrain);
    click(&mut h, ON_TERRAIN);
    assert_eq!(h.state().selection, vec![terrain(&h)]);
    // Nothing of it is in the project.
    assert!(!h.state().session.model.to_json().unwrap().contains("select_terrain"));
}

#[test]
fn there_is_no_select_terrain_box_without_terrain() {
    let h = app(BOX);
    assert!(h.query_by_label("Select Terrain").is_none());
}

#[test]
fn a_box_over_terrain_and_a_wall_takes_the_wall_only_and_a_drag_from_terrain_moves_nothing() {
    let mut h = lock();
    let wall = ids(&h, "wall")[0];
    // Right to left: crossing, so the contour that runs through the box is touched by it.
    drag(&mut h, P2(6000.0, 3000.0), P2(-6000.0, -3000.0));
    assert_eq!(h.state().selection, vec![wall], "the terrain is touched by the box and not taken");
    let before = h.state().session.model.to_json().unwrap();
    drag(&mut h, ON_TERRAIN, P2(3000.0, -8000.0));
    assert_eq!(h.state().session.model.to_json().unwrap(), before, "a drag that began on terrain moved nothing");
    pick(&mut h, Role::CheckBox, "Select Terrain");
    drag(&mut h, P2(6000.0, 3000.0), P2(-6000.0, -3000.0));
    assert!(h.state().selection.contains(&terrain(&h)), "ticked, the box takes it too");
}

#[test]
fn in_3d_a_click_on_the_terrain_picks_nothing_until_ctrl_alt_or_select_terrain() {
    let mut h =
        app(r#"toposurface {"points": [[-10000, -10000, -1000], [10000, -10000, 1000], [10000, 10000, 1000], [-10000, 10000, -1000]]}"#);
    press(&mut h, "Default 3D View");
    assert_eq!(h.state().sheet, Sheet::View3D);
    h.run_steps(3);
    let at = h.state().orbit.rect.center();
    click_at(&mut h, at);
    assert!(h.state().selection.is_empty(), "locked: {:?}", h.state().selection);
    assert!(h.state().status.text.contains("Ctrl+Alt"), "{}", h.state().status.text);
    // Ctrl+Alt for one click.
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::CTRL | Modifiers::ALT });
    }
    h.run();
    assert_eq!(h.state().selection, vec![terrain(&h)]);
    // Or the box.
    h.state_mut().selection.clear();
    pick(&mut h, Role::CheckBox, "Select Terrain");
    click_at(&mut h, at);
    assert_eq!(h.state().selection, vec![terrain(&h)]);
}
