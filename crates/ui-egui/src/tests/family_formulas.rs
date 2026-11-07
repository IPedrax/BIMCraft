//! The Family Editor's Family Types greys what a formula works out, showing what the type previewed comes to.

use super::*;
use egui_kittest::kittest::NodeT;

const CABINET: &str = r#"family {"name": "Cabinet", "category": "generic", "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "formula": "Width / 2"}, {"name": "Show Top", "kind": "yesno", "group": "visibility", "value": true}], "types": {"A": {}, "B": {"Width": 600, "Show Top": false}}, "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", 500], [0, 500]], "to": "Height"}, {"profile": [[2000, 0], [3000, 0], [3000, 500], [2000, 500]], "to": 600, "visible": "Show Top"}]}
component {"family": "Cabinet", "type": "A", "at": [2000, 2000]}"#;

fn disabled(h: &Harness<BimApp>, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().is_disabled()
}

fn value(h: &Harness<BimApp>, label: &str) -> Option<String> {
    h.get_by_label(label).accesskit_node().value().map(|v| v.to_string())
}

#[test]
fn family_types_greys_a_value_a_formula_works_out_showing_what_the_type_comes_to() {
    let mut h = app(&format!("{BOX}\n{CABINET}"));
    h.set_size(vec2(1800.0, 900.0));
    h.state_mut().edit_family("Cabinet");
    h.run();
    // Parameters: Height is worked out, so its value is shown, greyed, as type A comes to; Width is typed.
    assert!(disabled(&h, "Parameter Value 2"), "a formula's value takes no typing");
    assert_eq!(value(&h, "Parameter Value 2").as_deref(), Some("500"));
    assert!(!disabled(&h, "Parameter Value 1"));
    // Types: Height's row is there, greyed, with what each type comes to; the type's own tick is typed.
    press_near(&mut h, "Types", "Parameter Name 1");
    assert!(disabled(&h, "Type Value Height"));
    assert_eq!(value(&h, "Type Value Height").as_deref(), Some("500"));
    assert!(!disabled(&h, "Type Value Show Top"));
    // The type previewed, picked beside the preview: B.
    press(&mut h, "Preview Type");
    press_near(&mut h, "B", "Preview Type");
    h.run();
    assert_eq!(value(&h, "Type Value Height").as_deref(), Some("300"), "type B: 600 / 2");
    // Applied, the family is as it was: no type sets Height.
    press_near(&mut h, "Apply", "Cancel");
    let f = &h.state().session.model.families["Cabinet"];
    assert!(f.types.values().all(|t| !t.contains_key("Height")), "{:?}", f.types);
    assert_eq!(f.parameters[1].driven(), Some("Width / 2"));
}
