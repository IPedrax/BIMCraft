//! Manage > Object Styles: each model category's pens, cut and in projection, edited as a draft and applied as one command.

use super::*;
use bimcraft_core::pen::Pen;
use bimcraft_core::styles::ObjectStyle;

#[test]
fn object_styles_set_each_categorys_pens_in_one_step() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Object Styles");
    assert!(h.state().object_styles_dialog.is_some());
    // A row for each model category, its pens as a project starts with them; nothing is the project's until OK.
    assert!(h.query_by_label("Walls: Cut").is_some() && h.query_by_label("Railings: Projection").is_some());
    choose(&mut h, "Walls: Cut", "13 · 2.00 mm");
    choose(&mut h, "Doors: Projection", "0 · Hairline");
    assert!(h.state().session.model.object_styles.is_empty());
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().object_styles_dialog.is_none(), "OK closes it");
    let m = &h.state().session.model;
    assert_eq!(m.object_style("wall"), ObjectStyle { projection: Pen(4), cut: Pen(13) });
    assert_eq!(m.object_style("door").projection, Pen(0));
    assert_eq!(m.object_styles.len(), 2, "only what differs from a project's start");
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert_eq!(h.state().session.history().last(), Some(&"Object Styles"));
    // Cancel drops what was edited; Reset, applied, is the project's start again.
    press(&mut h, "Object Styles");
    choose(&mut h, "Floors: Cut", "1 · 0.10 mm");
    press(&mut h, "Cancel");
    assert_eq!(h.state().session.model.object_style("floor").cut, Pen(10));
    press(&mut h, "Object Styles");
    press(&mut h, "Reset");
    press(&mut h, "OK");
    assert!(h.state().session.model.object_styles.is_empty());
}
