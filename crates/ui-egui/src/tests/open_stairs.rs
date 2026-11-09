//! Existing stair editing controls preserve authored open construction.
use super::*;

#[test]
fn open_stair_width_edit_and_undo_keep_construction_and_identity() {
    let mut h = app(&format!("{BOX}\nstair {{\"start\":[1000,1000],\"angle\":90,\"open\":true}}"));
    let stair = ids(&h, "stair")[0];
    let guid = h.state().session.model.elements.guid(stair);
    click(&mut h, P2(1000.0, 2120.0));
    assert_eq!(h.state().selection, [stair]);
    type_into(&mut h, "Actual Run Width", "1200");
    key(&mut h, Key::Enter);
    assert_eq!(field(&h, stair, "width"), json!(1200.0));
    assert_eq!(field(&h, stair, "open"), json!(true));
    assert_eq!(h.state().session.model.elements.guid(stair), guid);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(field(&h, stair, "width"), json!(1000.0));
    assert_eq!(field(&h, stair, "open"), json!(true));
}
