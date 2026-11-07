//! The Family Editor's Family Types greys what a formula works out, showing what the type previewed comes to, and a form's
//! Visible is a tick with Revit's "=" at its end: tied to a yes/no parameter, the tick is greyed, showing the association.

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

fn ticked(h: &Harness<BimApp>, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn family_types_greys_a_value_a_formula_works_out_showing_what_the_type_comes_to() {
    let mut h = app(&format!("{BOX}\n{CABINET}"));
    h.set_size(vec2(1800.0, 900.0));
    h.state_mut().edit_family("Cabinet");
    h.run();
    press(&mut h, "Family Types");
    // Parameters: Height is worked out, so its value is shown, greyed, as type A comes to; Width is typed.
    assert!(disabled(&h, "Parameter Value 2"), "a formula's value takes no typing");
    assert_eq!(value(&h, "Parameter Value 2").as_deref(), Some("500"));
    assert!(!disabled(&h, "Parameter Value 1"));
    // Types: Height's row is there, greyed, with what each type comes to; the type's own tick is typed.
    press_near(&mut h, "Types", "Parameter Name 1");
    assert!(disabled(&h, "Type Value Height"));
    assert_eq!(value(&h, "Type Value Height").as_deref(), Some("500"));
    assert!(!disabled(&h, "Type Value Show Top"));
    // The type previewed, picked in Types: B.
    press(&mut h, "Type");
    press_near(&mut h, "B", "Type");
    h.run();
    assert_eq!(value(&h, "Type Value Height").as_deref(), Some("300"), "type B: 600 / 2");
    // Loaded into the project, the family is as it was: no type sets Height.
    press(&mut h, "OK");
    press(&mut h, "Load into Project");
    let f = &h.state().session.model.families["Cabinet"];
    assert!(f.types.values().all(|t| !t.contains_key("Height")), "{:?}", f.types);
    assert_eq!(f.parameters[1].driven(), Some("Width / 2"));
}

#[test]
fn a_forms_visible_tied_to_a_yes_no_parameter_through_equals_greys_its_tick() {
    let mut h = app(&format!("{BOX}\n{CABINET}"));
    h.set_size(vec2(1800.0, 900.0));
    h.state_mut().edit_family("Cabinet");
    h.run();
    // The second form, picked in the Project Browser, is selected: Properties show its own rows (T11).
    press(&mut h, "Symbols");
    h.get_by_label("2. Extrusion").scroll_to_me();
    h.run();
    press(&mut h, "2. Extrusion");
    // Tied to Show Top: greyed, ticked as type A has it.
    assert!(disabled(&h, "Visible"), "tied, the tick takes no clicks");
    assert!(ticked(&h, "Visible"));
    let equals = "Associate Family Parameter: Visible";
    // Properties scrolls: the row is brought into view before its "=" is pressed.
    let press_equals = |h: &mut Harness<BimApp>| {
        h.get_by_label(equals).scroll_to_me();
        h.run();
        press(h, equals);
    };
    // "=", <None>, OK: untied, it keeps what it came to and takes clicks.
    press_equals(&mut h);
    press(&mut h, "<None>");
    press_near(&mut h, "OK", "<None>");
    assert!(!disabled(&h, "Visible") && ticked(&h, "Visible"));
    // A click hides it in every type.
    h.get_by_label("Visible").scroll_to_me();
    h.run();
    press(&mut h, "Visible");
    assert!(!ticked(&h, "Visible"));
    // "=" again, Show Top, OK: tied once more, greyed.
    press_equals(&mut h);
    press_near(&mut h, "Show Top", "<None>");
    press_near(&mut h, "OK", "<None>");
    assert!(disabled(&h, "Visible") && ticked(&h, "Visible"));
    press(&mut h, "Load into Project");
    let f = &h.state().session.model.families["Cabinet"];
    assert_eq!(f.forms[1].visible, Some(bimcraft_core::family::Expr::Formula("Show Top".into())));
    // Each type's tick: B's is clear, so the family worked out for B leaves the top out.
    let values = h.state().session.model.families["Cabinet"].values("B", &Default::default()).unwrap();
    assert_eq!(values.get("Show Top"), Some(&bimcraft_core::family::Val::Number(0.0)));
}
