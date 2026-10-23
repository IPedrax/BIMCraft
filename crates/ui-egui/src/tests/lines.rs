//! Annotate > Detail Line, drawn in a line style, and Manage > Line Styles: the project's line styles and line patterns,
//! edited as a draft and applied as one step.

use super::*;
use bimcraft_core::lines::{LineStyle, Segment};
use bimcraft_core::pen::Pen;

#[test]
fn detail_lines_are_clicked_out_in_the_options_bars_style_and_finished_with_enter_or_the_first_point() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    assert!(h.query_by_label("Modify | Place Detail Lines").is_some());
    choose(&mut h, "Line Style", "Wide Lines");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(5000.0, 1000.0));
    click(&mut h, P2(5000.0, 3000.0));
    assert!(ids(&h, "line").is_empty(), "nothing is made until it is finished");
    key(&mut h, Key::Enter);
    let first = ids(&h, "line");
    assert_eq!(first.len(), 1);
    assert_eq!((field(&h, first[0], "style"), field(&h, first[0], "closed")), (json!("Wide Lines"), json!(null)));
    assert_eq!(field(&h, first[0], "points").as_array().map(Vec::len), Some(3));
    assert_eq!(h.state().tool, Tool::DetailLine(Vec::new()), "ready for the next one");
    // Back on the first point, it closes.
    for p in [P2(6000.0, 1000.0), P2(7000.0, 1000.0), P2(7000.0, 2000.0), P2(6000.0, 1000.0)] {
        click(&mut h, p);
    }
    let lines = ids(&h, "line");
    assert_eq!(lines.len(), 2);
    assert_eq!(field(&h, lines[1], "closed"), json!(true));
    // Picked by its line, its style changed in Properties.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(3000.0, 1000.0));
    assert_eq!(h.state().selection, first);
    choose(&mut h, "Line Style", "Hidden Lines");
    assert_eq!(field(&h, first[0], "style"), json!("Hidden Lines"));
    // Its shortcut.
    key(&mut h, Key::Escape);
    shortcut(&mut h, "DL");
    assert_eq!(h.state().tool, Tool::DetailLine(Vec::new()));
}

#[test]
fn line_styles_and_patterns_are_made_changed_and_deleted_in_one_step() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Line Styles");
    assert!(h.state().line_styles_dialog.is_some());
    // A pattern of its own: two dashes, the second shorter, then a dot.
    press(&mut h, "Patterns");
    press(&mut h, "New Pattern");
    type_into(&mut h, "Pattern 1: Name", "Fence");
    press(&mut h, "Add Segment");
    press(&mut h, "Add Segment");
    choose(&mut h, "Segment 3", "Dot");
    type_into(&mut h, "Segment 2 Length", "0.5");
    // A style in it, in pen 6.
    press(&mut h, "Styles");
    press(&mut h, "New Style");
    type_into(&mut h, "Style 1: Name", "Fence Line");
    choose(&mut h, "Fence Line: Line Weight", "6 · 0.22 mm");
    choose(&mut h, "Fence Line: Pattern", "Fence");
    // A style a project starts with, thinner.
    choose(&mut h, "Wide Lines: Line Weight", "8 · 0.28 mm");
    assert!(!h.state().session.model.line_styles.contains_key("Fence Line"), "nothing is the project's until OK");
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().line_styles_dialog.is_none(), "OK closes it");
    let m = &h.state().session.model;
    assert_eq!(
        m.line_patterns["Fence"].0,
        [Segment::Dash(3.0), Segment::Space(0.5), Segment::Dot, Segment::Space(1.5)],
        "a new pattern starts as a dash and a space; a segment added, a dash, then a space"
    );
    assert_eq!(m.line_styles["Fence Line"], LineStyle { pen: Pen(6), color: [0, 0, 0], pattern: Some("Fence".into()) });
    assert_eq!(m.line_styles["Wide Lines"].pen, Pen(8));
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert_eq!(h.state().session.history().last(), Some(&"Line Styles"));
    // Renamed and deleted; a style in use is refused, saying why, and the dialog stays open to fix it.
    h.state_mut().session.execute("detail_line", &json!({"points": [[0, 0], [1000, 0]], "style": "Thin Lines"})).unwrap();
    press(&mut h, "Line Styles");
    type_into(&mut h, "Fence Line: Name", "Boundary");
    press(&mut h, "Thin Lines: Delete");
    press(&mut h, "OK");
    assert!(h.state().line_styles_dialog.is_some() && h.state().status.error, "{}", h.state().status.text);
    assert!(h.state().session.model.line_styles.contains_key("Fence Line"), "all or nothing");
    press(&mut h, "Cancel");
    press(&mut h, "Line Styles");
    type_into(&mut h, "Fence Line: Name", "Boundary");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    assert!(m.line_styles.contains_key("Boundary") && !m.line_styles.contains_key("Fence Line"));
}
