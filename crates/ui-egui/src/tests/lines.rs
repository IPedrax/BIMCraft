//! Annotate > Detail Line, drawn in a line style, and Manage > Line Styles: the project's line styles and line patterns,
//! edited as a draft and applied as one step.

use super::*;
use bimcraft_core::lines::{LineStyle, Segment};
use bimcraft_core::pen::Pen;

#[test]
fn detail_lines_are_made_a_line_a_click_as_walls_are_or_as_one_polyline() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    assert!(h.query_by_label("Modify | Place Detail Lines").is_some());
    choose(&mut h, "Line Style", "Wide Lines");
    // A line a click, each from the one before.
    click(&mut h, P2(1000.0, 1000.0));
    assert!(ids(&h, "line").is_empty(), "one point is no line yet");
    click(&mut h, P2(5000.0, 1000.0));
    click(&mut h, P2(5000.0, 3000.0));
    let first = ids(&h, "line");
    assert_eq!(first.len(), 2);
    for id in &first {
        assert_eq!(field(&h, *id, "style"), json!("Wide Lines"));
        assert!(field(&h, *id, "start").is_array() && field(&h, *id, "points").is_null(), "a line is its two ends");
    }
    assert_eq!((field(&h, first[1], "start"), field(&h, first[1], "end")), (json!([5000.0, 1000.0]), json!([5000.0, 3000.0])));
    // Enter ends the chain; the next click starts afresh.
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::DetailLine(Vec::new()), "ready for the next one");
    // Back on its first point, the chain closes and ends.
    for p in [P2(6000.0, 1000.0), P2(7000.0, 1000.0), P2(7000.0, 2000.0), P2(6000.0, 1000.0)] {
        click(&mut h, p);
    }
    assert_eq!(ids(&h, "line").len(), 5);
    assert_eq!(h.state().tool, Tool::DetailLine(Vec::new()));
    // Without Chain, two clicks make one line.
    press(&mut h, "Chain");
    for p in [P2(1000.0, 4000.0), P2(2000.0, 4000.0), P2(3000.0, 4000.0)] {
        click(&mut h, p);
    }
    assert_eq!(ids(&h, "line").len(), 6);
    assert_eq!(h.state().tool, Tool::DetailLine(vec![P2(3000.0, 4000.0)]));
    key(&mut h, Key::Escape);
    // Polyline: one line through every point, made by Enter, or closed back on the first.
    press(&mut h, "Polyline");
    for p in [P2(1000.0, 5000.0), P2(2000.0, 5000.0), P2(2000.0, 6000.0)] {
        click(&mut h, p);
    }
    assert_eq!(ids(&h, "line").len(), 6, "nothing is made until it is finished");
    key(&mut h, Key::Enter);
    let lines = ids(&h, "line");
    assert_eq!(lines.len(), 7);
    assert_eq!((field(&h, lines[6], "points").as_array().map(Vec::len), field(&h, lines[6], "closed")), (Some(3), json!(null)));
    for p in [P2(6000.0, 5000.0), P2(7000.0, 5000.0), P2(7000.0, 6000.0), P2(6000.0, 5000.0)] {
        click(&mut h, p);
    }
    let lines = ids(&h, "line");
    assert_eq!(lines.len(), 8);
    assert_eq!(field(&h, lines[7], "closed"), json!(true));
    let first = vec![first[0]];
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
