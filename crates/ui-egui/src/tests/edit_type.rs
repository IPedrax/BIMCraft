//! A double click on an element picked in Modify opens its settings, as Edit Type in the right-click menu does: its type's thickness,
//! materials and layers.

use super::*;
use crate::settings::Profile;

#[test]
fn a_double_click_on_a_wall_opens_its_type_to_change_its_layers() {
    for profile in [Profile::Ribbon, Profile::Palette] {
        let mut h = app_at(BOX, 0.05);
        h.state_mut().profile = profile;
        h.run();
        let south = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
        assert!(h.state().type_dialog.is_none());
        double_click(&mut h, P2(4000.0, 0.0));
        assert!(h.state().type_dialog.is_some(), "{profile:?}: Type Properties is open");
        assert_eq!(h.state().selection, [south], "{profile:?}: and the wall is what is selected");
    }
}

#[test]
fn a_double_click_on_a_wall_already_selected_does_the_same() {
    let mut h = app_at(BOX, 0.05);
    click(&mut h, P2(4000.0, 0.0));
    assert!(h.state().type_dialog.is_none(), "one click only selects");
    double_click(&mut h, P2(4000.0, 0.0));
    assert!(h.state().type_dialog.is_some());
}

#[test]
fn a_double_click_that_means_something_else_does_that() {
    let mut h = app_at(BOX, 0.05);
    // On empty ground: nothing to open.
    double_click(&mut h, P2(4000.0, 3000.0));
    assert!(h.state().type_dialog.is_none(), "empty ground");
    // With a drawing tool in hand the double click is the tool's (it ends a chain), not the wall's.
    h.state_mut().tool = Tool::Wall(Vec::new());
    h.run();
    double_click(&mut h, P2(4000.0, 0.0));
    assert!(h.state().type_dialog.is_none(), "a tool in hand");
}
