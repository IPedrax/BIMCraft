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

#[test]
fn linework_draws_the_edge_clicked_in_the_style_chosen_hides_it_or_puts_it_back() {
    let mut h = app(BOX);
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    // What the view's graphics hold: each edge's style, `None` hidden.
    let marked = |h: &Harness<BimApp>| -> Vec<Option<String>> {
        let m = &h.state().session.model;
        let all = m.elements.values().filter_map(bimcraft_core::graphics::graphics_of_element);
        all.flat_map(|g| g.linework.iter().map(|l| l.style.clone())).collect()
    };
    // LW, as Revit's shortcut; the Options Bar starts on Wide Lines.
    shortcut(&mut h, "LW");
    assert_eq!(h.state().tool, Tool::Linework);
    click(&mut h, P2(4000.0, -60.0));
    assert_eq!(marked(&h), [Some("Wide Lines".to_string())]);
    let m = Arc::clone(&h.state().session.model);
    let edge = m.elements.values().filter_map(bimcraft_core::graphics::graphics_of_element).flat_map(|g| g.linework.clone()).next();
    assert!(edge.is_some_and(|l| l.element == gid(&h, wall) && l.from.1 < 0.0 && l.to.1 < 0.0), "the south face");
    // The same edge again, hidden; then back by category.
    choose(&mut h, "Line Style", "<Invisible lines>");
    click(&mut h, P2(4000.0, -60.0));
    assert_eq!(marked(&h), [None]);
    choose(&mut h, "Line Style", "<By Category>");
    click(&mut h, P2(4000.0, -60.0));
    assert!(marked(&h).is_empty());
    // On the Modify tab, beside Split Element.
    key(&mut h, Key::Escape);
    topmost(&mut h, "Modify");
    press(&mut h, "Linework");
    assert_eq!(h.state().tool, Tool::Linework);
}

#[test]
fn align_lines_a_detail_line_up_with_another_or_with_a_wall() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [5000, 1000]}}\ndetail_line {{\"start\": [2000, 2500], \"end\": [6000, 2500]}}"
    ));
    let lines = ids(&h, "line");
    shortcut(&mut h, "AL");
    // The first line as the reference, then the second goes onto it.
    click(&mut h, P2(3000.0, 1000.0));
    click(&mut h, P2(4000.0, 2500.0));
    assert_eq!((field(&h, lines[1], "start"), field(&h, lines[1], "end")), (json!([2000.0, 1000.0]), json!([6000.0, 1000.0])));
    // A wall as the reference: the line goes onto the face or the centre line picked, its length unchanged.
    click(&mut h, P2(4000.0, 0.0));
    click(&mut h, P2(1500.0, 1000.0));
    let (start, end) = (field(&h, lines[0], "start"), field(&h, lines[0], "end"));
    let (y, y2) = (start[1].as_f64().unwrap_or(f64::NAN), end[1].as_f64().unwrap_or(f64::NAN));
    assert!(y == y2 && y.abs() <= 200.0 && y != 1000.0, "{start} {end}");
    assert_eq!((start[0].clone(), end[0].clone()), (json!(1000.0), json!(5000.0)));
}

#[test]
fn detail_lines_selected_become_a_group_placed_again_from_the_detail_panel_and_ungrouped_from_the_menu() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}\ndetail_line {{\"start\": [1000, 1000], \"end\": [1000, 2000]}}"
    ));
    let groups = |h: &Harness<BimApp>| ids(h, "detailgroup");
    // Selected, then GP: one group where they were, selected.
    click(&mut h, P2(2000.0, 1000.0));
    click_with(&mut h, P2(1000.0, 1600.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection.len(), 2);
    shortcut(&mut h, "GP");
    assert_eq!((ids(&h, "line").len(), groups(&h).len()), (0, 1));
    assert_eq!(h.state().selection, groups(&h));
    assert!(h.state().session.model.detail_groups.contains_key("Group 1"));
    // Annotate > Detail Group places another where clicked, the Options Bar naming it.
    key(&mut h, Key::Escape);
    press(&mut h, "Annotate");
    press(&mut h, "Detail Group");
    assert_eq!(h.state().tool, Tool::DetailGroup);
    assert!(h.query_by_label("Group").is_some());
    click(&mut h, P2(5000.0, 3000.0));
    let all = groups(&h);
    assert_eq!(all.len(), 2);
    assert_eq!(field(&h, all[1], "at"), json!([5000.0, 3000.0]));
    // Picked as one, and the right-click menu takes it apart.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    click(&mut h, P2(5500.0, 2500.0));
    assert_eq!(h.state().selection, [all[1]]);
    let at = h.state().screen(P2(5500.0, 2500.0));
    right_click_at(&mut h, at);
    choose_item(&mut h, "Ungroup");
    assert_eq!((groups(&h).len(), ids(&h, "line").len()), (1, 2));
    assert_eq!(h.state().selection.len(), 2, "its lines, selected");
}

#[test]
fn repeating_detail_lays_the_group_on_the_options_bar_along_two_clicks_every_spacing() {
    let mut h = app(&format!("{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [1000, 1200]}}"));
    click(&mut h, P2(1000.0, 1100.0));
    shortcut(&mut h, "GP");
    key(&mut h, Key::Escape);
    press(&mut h, "Annotate");
    press(&mut h, "Repeating Detail");
    assert_eq!(h.state().tool, Tool::RepeatingDetail(None));
    type_into(&mut h, "Spacing:", "500");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(3000.0, 3000.0));
    let made = ids(&h, "repeatingdetail");
    assert_eq!(made.len(), 1);
    assert_eq!((field(&h, made[0], "type"), field(&h, made[0], "spacing")), (json!("Group 1"), json!(500.0)));
    assert_eq!(h.state().tool, Tool::RepeatingDetail(None), "ready for the next");
}
