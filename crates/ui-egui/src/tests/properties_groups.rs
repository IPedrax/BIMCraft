//! A component's Properties show each parameter group once. The instance parameters of its family a group the element's
//! own fields already head (Other: where it is, its rotation) are listed in that group, not under a second header of the name.
use super::*;
use bimcraft_core::model::Element;
use egui::accesskit::Role;

const PROJECT: &str = r#"family {"name": "Lamp", "category": "generic", "parameters": [{"name": "Lit", "kind": "yesno", "instance": true}, {"name": "Depth", "kind": "length", "value": 300, "instance": true}], "types": {"T": {}}, "forms": [{"profile": [[0, 0], [600, 0], [600, 400], [0, 400]], "to": 800}]}
component {"family": "Lamp", "type": "T", "at": [5000, 0]}"#;

fn selected() -> Harness<'static, BimApp> {
    let mut h = app(PROJECT);
    h.set_size(vec2(1600.0, 1400.0));
    let lamp = find(&h, |e| matches!(e, Element::Component(_)));
    h.state_mut().selection = vec![lamp];
    h.run();
    h.run();
    h
}

#[test]
fn a_components_properties_head_other_once_its_yes_no_parameter_under_it() {
    let mut h = selected();
    assert_eq!(h.get_all_by_label("Other").count(), 1, "one Other");
    // Lit, under Other, above Dimensions, which holds Depth.
    let (lit, dims, depth) = (
        h.get_by_role_and_label(Role::CheckBox, "Lit").rect().top(),
        h.get_by_label("Dimensions").rect().top(),
        h.get_by_label("Depth").rect().top(),
    );
    let other = h.get_by_label("Other").rect().top();
    assert!(other < lit && lit < dims && dims < depth, "Other {other} < Lit {lit} < Dimensions {dims} < Depth {depth}");
    // It still edits: Lit ticked.
    h.get_by_role_and_label(Role::CheckBox, "Lit").scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Lit").click();
    h.run();
    let Some(Element::Component(c)) = h.state().session.model.elements.values().find(|e| matches!(e, Element::Component(_))).cloned()
    else {
        panic!()
    };
    assert_eq!(c.params.get("Lit"), Some(&json!(true)), "{:?}", c.params);
    // Folded, Other hides its yes or no parameter with its own rows.
    h.get_by_label("Other").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::CheckBox, "Lit").is_none(), "folded with Other");
    assert!(h.query_by_label("Depth").is_some(), "Dimensions stays open");
}
