//! Manage > Line Weights: the project's pens, the model's by scale and the annotation's, edited as a draft and applied as one
//! command.

use super::*;

/// The project's line weights.
fn weights(h: &Harness<BimApp>) -> bimcraft_core::pen::LineWeights {
    h.state().session.model.line_weights().into_owned()
}

#[test]
fn line_weights_are_edited_by_scale_and_applied_as_one_step() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Line Weights");
    assert!(h.state().line_weights_dialog.is_some());
    // A column for each scale, the project's units naming it.
    assert!(h.query_by_label("Delete 1 : 10").is_some() && h.query_by_label("Delete 1 : 500").is_some());
    // A pen's width typed, a scale taken away and another added, copying the nearest: nothing is the project's until OK.
    type_role(&mut h, Role::SpinButton, "Pen 10 at 1 : 100", "1.25");
    pick(&mut h, Role::Button, "Delete 1 : 10");
    choose(&mut h, "Add Scale:", "1 : 1000");
    assert_eq!(weights(&h), bimcraft_core::pen::LineWeights::default_for(bimcraft_core::units::Units::Millimetres));
    // The annotation's pens, at any scale.
    press(&mut h, "Annotation Line Weights");
    type_role(&mut h, Role::SpinButton, "Annotation pen 4", "0.3");
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().line_weights_dialog.is_none(), "OK closes it");
    let lw = weights(&h);
    let scales: Vec<f64> = lw.model.iter().map(|c| c.scale).collect();
    assert_eq!(scales, [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0]);
    assert_eq!((lw.column(100.0).unwrap().widths[9], lw.annotation[3]), (1.25, 0.3));
    assert_eq!(lw.column(1000.0).unwrap().widths, lw.column(500.0).unwrap().widths, "a new scale copies the nearest");
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert_eq!(h.state().session.history().last(), Some(&"Line Weights"));
    // Reset, applied, is the project's start again; Cancel drops what was edited.
    press(&mut h, "Line Weights");
    press(&mut h, "Reset");
    press(&mut h, "OK");
    assert_eq!(h.state().session.model.line_weights, None);
    press(&mut h, "Line Weights");
    pick(&mut h, Role::Button, "Delete 1 : 10");
    press(&mut h, "Cancel");
    assert_eq!(h.state().session.model.line_weights, None);
}

#[test]
fn a_project_in_feet_and_inches_lists_its_line_weights_by_the_architects_scales() {
    let mut h = app(&format!("{BOX}\nunits {{\"length\": \"ft-in\"}}"));
    press(&mut h, "Manage");
    press(&mut h, "Line Weights");
    assert!(h.query_by_label("Delete 1/8\" = 1'-0\"").is_some());
    assert!(h.query_by_label("Pen 3 at 1/4\" = 1'-0\"").is_some());
}
