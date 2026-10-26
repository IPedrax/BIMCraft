//! The Draw panel of detail lines, sketches and walls: each shape clicked out, Fillet Arc, Pick Lines, the Options
//! Bar's Radius, Sides and Offset; Join into Polyline and Explode Polyline; and Split on a detail line.

use super::*;
use bimcraft_core::curves::Curve;

/// The plan of the box of walls with the Detail Line tool in hand.
fn detailing() -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    h
}

fn curve(h: &Harness<BimApp>, id: Id) -> Curve {
    match h.state().session.model.elements.get(id) {
        Some(Element::DetailLine(l)) => l.curve.clone(),
        e => panic!("{id} is {e:?}"),
    }
}

/// The shapes of the detail lines, in the order made.
fn kinds(h: &Harness<BimApp>) -> Vec<&'static str> {
    ids(h, "line").into_iter().map(|id| curve(h, id).kind()).collect()
}

#[test]
fn each_shape_of_the_draw_panel_makes_its_detail_lines_from_its_clicks() {
    let mut h = detailing();
    let cases: [(&str, &[P2], &[&str]); 9] = [
        ("Rectangle", &[P2(1000.0, 1000.0), P2(3000.0, 2000.0)], &["line"; 4]),
        ("Inscribed Polygon", &[P2(4000.0, 3000.0), P2(5000.0, 3000.0)], &["line"; 6]),
        ("Circle", &[P2(4000.0, 3000.0), P2(4000.0, 3500.0)], &["circle"]),
        ("Start-End-Radius Arc", &[P2(1000.0, 4000.0), P2(3000.0, 4000.0), P2(2000.0, 5000.0)], &["arc"]),
        ("Center-ends Arc", &[P2(6000.0, 1000.0), P2(7000.0, 1000.0), P2(6000.0, 1500.0)], &["arc"]),
        ("Ellipse", &[P2(4000.0, 3000.0), P2(5500.0, 3000.0), P2(4000.0, 3600.0)], &["ellipse"]),
        ("Partial Ellipse", &[P2(1000.0, 500.0), P2(3000.0, 500.0), P2(2000.0, 1200.0)], &["partial ellipse"]),
        ("Circumscribed Polygon", &[P2(6000.0, 4000.0), P2(6500.0, 4000.0)], &["line"; 6]),
        ("Line", &[P2(500.0, 5500.0), P2(1500.0, 5500.0)], &["line"]),
    ];
    for (label, clicks, made) in cases {
        let before = ids(&h, "line").len();
        press(&mut h, label);
        assert_eq!(h.state().shape().label(), label);
        for p in clicks {
            click(&mut h, *p);
        }
        // Esc ends a chain; with none under way it would put the tool down.
        if h.state().points().is_some_and(|p| !p.is_empty()) {
            key(&mut h, Key::Escape);
        }
        let now = kinds(&h);
        assert_eq!(&now[before..], made, "{label}");
    }
    // The circle went round its centre, through the point clicked.
    let circle = ids(&h, "line").into_iter().find(|id| curve(&h, *id).kind() == "circle").map(|id| curve(&h, id));
    assert_eq!(circle, Some(Curve::Circle { centre: P2(4000.0, 3000.0), radius: 500.0 }));
    // A rectangle is one undo step, though four lines.
    let n = ids(&h, "line").len();
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(2000.0, 2000.0));
    assert_eq!(ids(&h, "line").len(), n + 4);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(ids(&h, "line").len(), n);
}

#[test]
fn splines_and_polylines_take_points_until_enter_and_a_tangent_arc_goes_on_from_a_line() {
    let mut h = detailing();
    for (label, kind) in [("Spline", "spline"), ("Spline Through Points", "fitted spline"), ("Polyline", "polyline")] {
        press(&mut h, label);
        for p in [P2(1000.0, 1000.0), P2(2000.0, 2500.0), P2(3500.0, 1500.0), P2(5000.0, 3000.0)] {
            click(&mut h, p);
        }
        assert!(kinds(&h).iter().all(|k| *k != kind), "{label}: nothing until Enter");
        key(&mut h, Key::Enter);
        assert_eq!(kinds(&h).last().copied(), Some(kind), "{label}");
    }
    let fit = curve(&h, ids(&h, "line")[1]);
    for p in [P2(1000.0, 1000.0), P2(2000.0, 2500.0), P2(5000.0, 3000.0)] {
        assert!(fit.nearest(p).1 < 1e-6, "through the points clicked");
    }
    // A line, then an arc going on from its end the way it ran.
    press(&mut h, "Line");
    click(&mut h, P2(1000.0, 5000.0));
    click(&mut h, P2(3000.0, 5000.0));
    key(&mut h, Key::Escape);
    press(&mut h, "Tangent End Arc");
    click(&mut h, P2(3000.0, 5000.0));
    click(&mut h, P2(4000.0, 4000.0));
    let arc = curve(&h, *ids(&h, "line").last().unwrap_or(&0));
    assert_eq!(arc.kind(), "arc");
    assert!(arc.tangent(0.0).dist(P2(1.0, 0.0)) < 1e-6, "leaving east, as the line ran: {:?}", arc.tangent(0.0));
    // Away from any line's end, a tangent arc has nothing to go on from, and says so.
    key(&mut h, Key::Escape);
    let n = ids(&h, "line").len();
    click(&mut h, P2(6000.0, 500.0));
    assert!(h.state().status.error, "{}", h.state().status.text);
    click(&mut h, P2(7000.0, 1500.0));
    assert_eq!(ids(&h, "line").len(), n);
}

#[test]
fn radius_sides_and_offset_on_the_options_bar_shape_what_a_click_makes() {
    let mut h = detailing();
    press(&mut h, "Circle");
    press(&mut h, "Radius:");
    type_role(&mut h, Role::TextInput, "Radius", "600");
    click(&mut h, P2(3000.0, 3000.0));
    assert_eq!(curve(&h, ids(&h, "line")[0]), Curve::Circle { centre: P2(3000.0, 3000.0), radius: 600.0 }, "one click, with Radius");
    press(&mut h, "Inscribed Polygon");
    type_into(&mut h, "Sides:", "4");
    press(&mut h, "Radius:");
    click(&mut h, P2(3000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(ids(&h, "line").len(), 1 + 4, "a square");
    press(&mut h, "Line");
    type_into(&mut h, "Offset:", "200");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(2000.0, 1000.0));
    key(&mut h, Key::Escape);
    let l = curve(&h, *ids(&h, "line").last().unwrap_or(&0));
    assert!(l.start().dist(P2(1000.0, 1200.0)) < 1e-6, "drawn to its left: {l:?}");
}

#[test]
fn fillet_rounds_two_detail_lines_and_pick_lines_draws_along_a_wall() {
    let mut h = detailing();
    press(&mut h, "Line");
    press(&mut h, "Chain");
    for p in [P2(1000.0, 1000.0), P2(4000.0, 1000.0), P2(5000.0, 2000.0), P2(5000.0, 5000.0)] {
        click(&mut h, p);
    }
    let lines = ids(&h, "line");
    assert_eq!(lines.len(), 2);
    press(&mut h, "Fillet Arc");
    press(&mut h, "Radius:");
    type_role(&mut h, Role::TextInput, "Radius", "500");
    click(&mut h, P2(2000.0, 1000.0));
    click(&mut h, P2(5000.0, 4000.0));
    assert_eq!(kinds(&h), ["line", "line", "arc"], "cut back, and the arc between");
    // Each runs on to where the arc touches it: the first out to 500 short of the corner at (5000, 1000).
    assert!(curve(&h, lines[0]).end().dist(P2(4500.0, 1000.0)) < 1e-3, "{:?}", curve(&h, lines[0]));
    assert!(curve(&h, lines[1]).start().dist(P2(5000.0, 1500.0)) < 1e-3, "{:?}", curve(&h, lines[1]));
    // Pick Lines: a detail line along the wall clicked.
    press(&mut h, "Pick Lines");
    click(&mut h, P2(4000.0, 6000.0));
    let along = curve(&h, *ids(&h, "line").last().unwrap_or(&0));
    assert_eq!((along.start().1, along.end().1, along.length()), (6000.0, 6000.0, 8000.0));
}

#[test]
fn the_wall_tool_draws_circles_polygons_and_rectangles_from_the_draw_panel() {
    let mut h = app(BOX);
    let box_walls = walls(&h).len();
    tool(&mut h, "Wall");
    assert!(h.query_by_label("Spline").is_none(), "walls are lines and arcs");
    press(&mut h, "Circle");
    click(&mut h, P2(2000.0, 2000.0));
    click(&mut h, P2(3000.0, 2000.0));
    let arcs: Vec<f64> = h
        .state()
        .session
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) => Some(w.arc),
            _ => None,
        })
        .filter(|a| *a != 0.0)
        .collect();
    assert_eq!(arcs, [180.0, 180.0], "a circle is two half circles");
    press(&mut h, "Inscribed Polygon");
    click(&mut h, P2(6000.0, 4000.0));
    click(&mut h, P2(7000.0, 4000.0));
    assert_eq!(walls(&h).len(), box_walls + 2 + 6);
    press(&mut h, "Rectangle");
    click(&mut h, P2(3500.0, 4000.0));
    click(&mut h, P2(5000.0, 5000.0));
    assert_eq!(walls(&h).len(), box_walls + 2 + 6 + 4);
    // One undo step takes the rectangle away.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(walls(&h).len(), box_walls + 2 + 6);
}

#[test]
fn a_sketch_draws_with_every_shape_and_a_circle_floors_a_round_room() {
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    press(&mut h, "Boundary Line");
    press(&mut h, "Circle");
    click(&mut h, P2(4000.0, 3000.0));
    click(&mut h, P2(6000.0, 3000.0));
    assert_eq!(h.state().sketch.as_ref().map(|s| s.lines.len()), Some(1));
    press(&mut h, "Finish Edit Mode");
    let f = ids(&h, "floor");
    assert_eq!(f.len(), 1);
    let area = field(&h, f[0], "area_m2").as_f64().unwrap_or_default();
    assert!((area - std::f64::consts::PI * 4.0).abs() < 0.01, "{area}");
}

#[test]
fn detail_lines_join_into_a_polyline_explode_back_and_split_where_clicked() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}\ndetail_line {{\"start\": [3000, 1000], \"end\": [3000, 3000]}}"
    ));
    let lines = ids(&h, "line");
    h.state_mut().selection = lines.clone();
    h.run();
    press(&mut h, "Modify | Lines");
    press(&mut h, "Join into Polyline");
    assert_eq!(kinds(&h), ["polyline"]);
    assert_eq!(h.state().selection, vec![lines[0]]);
    press(&mut h, "Explode Polyline");
    assert_eq!(kinds(&h), ["line", "line"]);
    // Split: Modify's tool, on a detail line.
    h.state_mut().selection.clear();
    h.run();
    shortcut(&mut h, "SL");
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(kinds(&h), ["line", "line", "line"], "split in two where clicked");
}

#[test]
fn a_selected_detail_lines_handles_are_grips_dragged_to_reshape_it() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}\ndetail_line {{\"centre\": [5000, 3000], \"radius\": 800}}"
    ));
    let lines = ids(&h, "line");
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(h.state().selection, vec![lines[0]]);
    drag(&mut h, P2(3000.0, 1000.0), P2(3000.0, 2000.0));
    assert_eq!(curve(&h, lines[0]), Curve::line(P2(1000.0, 1000.0), P2(3000.0, 2000.0)), "its end moved, its start stayed");
    // A circle's grip is its centre: it moves whole.
    click(&mut h, P2(5800.0, 3000.0));
    assert_eq!(h.state().selection, vec![lines[1]]);
    drag(&mut h, P2(5000.0, 3000.0), P2(4000.0, 3500.0));
    assert_eq!(curve(&h, lines[1]), Curve::Circle { centre: P2(4000.0, 3500.0), radius: 800.0 });
    // Each drag is one step back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(curve(&h, lines[1]), Curve::Circle { centre: P2(5000.0, 3000.0), radius: 800.0 });
}

#[test]
fn trim_extend_and_offset_work_on_detail_lines_as_on_walls() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [2500, 1000]}}\ndetail_line {{\"start\": [3000, 1500], \"end\": [3000, 4000]}}\ndetail_line {{\"start\": [4000, 2000], \"end\": [4000, 3000]}}\ndetail_line {{\"start\": [5000, 2000], \"end\": [5000, 3000]}}"
    ));
    let l = ids(&h, "line");
    topmost(&mut h, "Modify");
    press(&mut h, "Trim/Extend to Corner");
    click(&mut h, P2(1500.0, 1000.0));
    click(&mut h, P2(3000.0, 3000.0));
    assert_eq!(curve(&h, l[0]), Curve::line(P2(1000.0, 1000.0), P2(3000.0, 1000.0)), "both run on to the corner");
    assert_eq!(curve(&h, l[1]), Curve::line(P2(3000.0, 1000.0), P2(3000.0, 4000.0)));
    // Several to one boundary: the box's north wall.
    press(&mut h, "Trim/Extend Multiple Elements");
    click(&mut h, P2(6000.0, 6000.0));
    click(&mut h, P2(4000.0, 2500.0));
    click(&mut h, P2(5000.0, 2500.0));
    assert!(curve(&h, l[2]).end().dist(P2(4000.0, 6000.0)) < 1e-3 && curve(&h, l[3]).end().dist(P2(5000.0, 6000.0)) < 1e-3);
    // Offset: a copy beside the line, on the cursor's side.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    shortcut(&mut h, "OF");
    type_into(&mut h, "Offset:", "250");
    let n = ids(&h, "line").len();
    click(&mut h, P2(1500.0, 1100.0));
    assert_eq!(ids(&h, "line").len(), n + 1);
    assert_eq!(curve(&h, *ids(&h, "line").last().unwrap_or(&0)), Curve::line(P2(1000.0, 1250.0), P2(3000.0, 1250.0)));
}

#[test]
fn modify_tools_change_the_sketch_lines_while_sketching() {
    let mut h = app(BOX);
    tool(&mut h, "Floor");
    press(&mut h, "Boundary Line");
    press(&mut h, "Line");
    for (k, p) in [P2(1000.0, 1000.0), P2(3500.0, 1000.0), P2(4000.0, 1500.0), P2(4000.0, 4000.0)].into_iter().enumerate() {
        click(&mut h, p);
        // Two lines apart: Esc ends the first.
        if k == 1 {
            key(&mut h, Key::Escape);
        }
    }
    let lines = |h: &Harness<BimApp>| {
        h.state().sketch.as_ref().map(|s| s.lines.iter().map(|l| l.curve.clone()).collect::<Vec<_>>()).unwrap_or_default()
    };
    assert_eq!(lines(&h).len(), 2);
    shortcut(&mut h, "TR");
    click(&mut h, P2(2000.0, 1000.0));
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(lines(&h), [Curve::line(P2(1000.0, 1000.0), P2(4000.0, 1000.0)), Curve::line(P2(4000.0, 1000.0), P2(4000.0, 4000.0))]);
    shortcut(&mut h, "SL");
    click(&mut h, P2(2000.0, 1000.0));
    assert_eq!(lines(&h).len(), 3, "split in two");
    shortcut(&mut h, "OF");
    type_into(&mut h, "Offset:", "500");
    click(&mut h, P2(4000.0, 2500.0));
    assert_eq!(lines(&h).last(), Some(&Curve::line(P2(3500.0, 1000.0), P2(3500.0, 4000.0))), "a copy to the west, where the cursor is");
}

#[test]
fn the_wall_tools_fillet_arc_rounds_two_walls() {
    let mut h = app("wall {\"points\": [[0, 0], [4000, 0]]}\nwall {\"points\": [[4000, 0], [4000, 4000]]}");
    tool(&mut h, "Wall");
    press(&mut h, "Fillet Arc");
    press(&mut h, "Radius:");
    type_role(&mut h, Role::TextInput, "Radius", "1000");
    click(&mut h, P2(2000.0, 0.0));
    click(&mut h, P2(4000.0, 2000.0));
    let w = walls(&h);
    assert_eq!(w.len(), 3, "{w:?}");
    assert!(w.contains(&(P2(0.0, 0.0), P2(3000.0, 0.0))) && w.contains(&(P2(4000.0, 1000.0), P2(4000.0, 4000.0))), "{w:?}");
}

#[test]
fn insulation_draws_detail_lines_as_batt_insulation_of_the_width_on_the_options_bar() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Insulation");
    assert!(h.query_by_label("Modify | Place Insulation").is_some());
    type_into(&mut h, "Width:", "150");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(6000.0, 3000.0));
    let l = ids(&h, "line");
    assert_eq!(l.len(), 1);
    assert_eq!(field(&h, l[0], "insulation"), json!(150.0));
    // Detail Line again: plain lines.
    key(&mut h, Key::Escape);
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    click(&mut h, P2(1000.0, 4000.0));
    click(&mut h, P2(6000.0, 4000.0));
    assert!(field(&h, *ids(&h, "line").last().unwrap_or(&0), "insulation").is_null());
}

#[test]
fn a_filled_regions_lines_take_the_line_style_picked_each_its_own() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Filled Region");
    choose(&mut h, "Line Style", "Wide Lines");
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 2000.0));
    // The bottom line picked and made invisible; the rest stay wide.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    click(&mut h, P2(2000.0, 1000.0));
    choose(&mut h, "Line Style", "<Invisible lines>");
    press(&mut h, "Finish Edit Mode");
    let r = ids(&h, "region");
    let Some(Element::FilledRegion(fr)) = h.state().session.model.elements.get(r[0]).cloned() else { panic!("a region") };
    let styles: Vec<(Option<String>, bool)> = fr.sketch.iter().map(|l| (l.style.clone(), l.invisible)).collect();
    assert_eq!(styles.iter().filter(|s| s.1).count(), 1, "{styles:?}");
    assert_eq!(styles.iter().filter(|s| s.0.as_deref() == Some("Wide Lines")).count(), 3, "{styles:?}");
}

#[test]
fn add_and_delete_vertex_and_convert_spline_on_modify_lines() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"points\": [[1000, 1000], [4000, 1000], [4000, 4000]]}}\ndetail_line {{\"fit\": [[1000, 5000], [2500, 5500], [4000, 5000]]}}"
    ));
    let l = ids(&h, "line");
    click(&mut h, P2(2500.0, 1000.0));
    press(&mut h, "Modify | Lines");
    press(&mut h, "Add Vertex");
    click(&mut h, P2(2500.0, 1000.0));
    assert_eq!(curve(&h, l[0]).vertices().len(), 4);
    key(&mut h, Key::Escape);
    click(&mut h, P2(2500.0, 1000.0));
    press(&mut h, "Modify | Lines");
    press(&mut h, "Delete Vertex");
    click(&mut h, P2(4000.0, 1000.0));
    assert_eq!(curve(&h, l[0]).vertices(), vec![P2(1000.0, 1000.0), P2(2500.0, 1000.0), P2(4000.0, 4000.0)]);
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    h.state_mut().selection = vec![l[1]];
    h.run();
    press(&mut h, "Modify | Lines");
    press(&mut h, "Convert Spline");
    assert_eq!(curve(&h, l[1]).kind(), "spline");
}

#[test]
fn a_spline_finishes_with_a_double_click_and_a_sketched_one_from_the_right_click_menu() {
    // Short frames, for the clicks to come quick enough to be a double click.
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    press(&mut h, "Spline Through Points");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(2000.0, 2500.0));
    double_click(&mut h, P2(3500.0, 1500.0));
    assert_eq!(kinds(&h), ["fitted spline"]);
    let fit = curve(&h, ids(&h, "line")[0]);
    assert_eq!(fit.end(), P2(3500.0, 1500.0), "it ends where the double click was");
    assert!(h.state().points().is_some_and(Vec::is_empty));
    // While sketching, right click > Finish Drawing; Enter would have done the same.
    key(&mut h, Key::Escape);
    press(&mut h, "Annotate");
    press(&mut h, "Filled Region");
    press(&mut h, "Spline");
    for p in [P2(1000.0, 4000.0), P2(2000.0, 5000.0), P2(3000.0, 4000.0)] {
        click(&mut h, p);
    }
    let pos = h.state().screen(P2(3000.0, 3000.0));
    right_click_at(&mut h, pos);
    choose_item(&mut h, "Finish Drawing");
    let sk = h.state().sketch.as_ref().map(|s| s.lines.iter().map(|l| l.curve.kind()).collect::<Vec<_>>());
    assert_eq!(sk, Some(vec!["spline"]));
}

#[test]
fn a_rectangles_width_and_height_a_radius_and_a_sketchs_lengths_are_typed() {
    let mut h = detailing();
    press(&mut h, "Rectangle");
    click(&mut h, P2(1000.0, 1000.0));
    hover(&mut h, P2(2000.0, 1500.0));
    h.event(Event::Text("2400".into()));
    h.run();
    key(&mut h, Key::Tab);
    h.event(Event::Text("1200".into()));
    h.run();
    key(&mut h, Key::Enter);
    let corners: Vec<P2> = ids(&h, "line").into_iter().map(|id| curve(&h, id).start()).collect();
    assert_eq!(corners, [P2(1000.0, 1000.0), P2(3400.0, 1000.0), P2(3400.0, 2200.0), P2(1000.0, 2200.0)]);
    // Only the height typed: the width is the cursor's.
    click(&mut h, P2(5000.0, 1000.0));
    hover(&mut h, P2(5800.0, 500.0));
    key(&mut h, Key::Tab);
    h.event(Event::Text("300".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(curve(&h, *ids(&h, "line").last().unwrap_or(&0)).start(), P2(5000.0, 700.0), "down, as the cursor was");
    // A circle's radius.
    press(&mut h, "Circle");
    click(&mut h, P2(3000.0, 4000.0));
    hover(&mut h, P2(3500.0, 4000.0));
    h.event(Event::Text("750".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(curve(&h, *ids(&h, "line").last().unwrap_or(&0)), Curve::Circle { centre: P2(3000.0, 4000.0), radius: 750.0 });
    // In a sketch, a line's length typed as a detail line's is; an arc's radius after its ends.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    press(&mut h, "Filled Region");
    press(&mut h, "Line");
    click(&mut h, P2(1000.0, 1000.0));
    hover(&mut h, P2(2000.0, 1000.0));
    h.event(Event::Text("1600".into()));
    h.run();
    key(&mut h, Key::Enter);
    let sketched = |h: &Harness<BimApp>| h.state().sketch.as_ref().map(|s| s.lines.clone()).unwrap_or_default();
    assert_eq!(sketched(&h).first().map(|l| l.end()), Some(P2(2600.0, 1000.0)));
    key(&mut h, Key::Escape);
    press(&mut h, "Start-End-Radius Arc");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(2600.0, 1000.0));
    hover(&mut h, P2(1800.0, 1100.0));
    h.event(Event::Text("1000".into()));
    h.run();
    key(&mut h, Key::Enter);
    let arc = sketched(&h).last().map(|l| l.curve.clone());
    assert!(arc.as_ref().is_some_and(|a| a.nearest(P2(1800.0, 1400.0)).1 < 1e-3), "rising 400 mm toward the cursor: {arc:?}");
}

#[test]
fn trim_extend_to_corner_joins_two_selected_lines_at_once_and_asks_for_picks_otherwise() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [2500, 1000]}}\ndetail_line {{\"start\": [3000, 1500], \"end\": [3000, 4000]}}"
    ));
    let l = ids(&h, "line");
    h.state_mut().selection = l.clone();
    h.run();
    shortcut(&mut h, "TR");
    assert_eq!(curve(&h, l[0]), Curve::line(P2(1000.0, 1000.0), P2(3000.0, 1000.0)), "run on to the corner");
    assert_eq!(curve(&h, l[1]), Curve::line(P2(3000.0, 1000.0), P2(3000.0, 4000.0)));
    assert_eq!(h.state().tool, Tool::Modify, "done: no picks asked for");
    // One selected: the picks.
    h.state_mut().selection = vec![l[0]];
    h.run();
    shortcut(&mut h, "TR");
    assert_eq!(h.state().tool, Tool::Trim(None));
    // While sketching, the two lines picked.
    key(&mut h, Key::Escape);
    tool(&mut h, "Floor");
    press(&mut h, "Boundary Line");
    for (a, b) in [(P2(1000.0, 1000.0), P2(3500.0, 1000.0)), (P2(4000.0, 1500.0), P2(4000.0, 4000.0))] {
        click(&mut h, a);
        click(&mut h, b);
        key(&mut h, Key::Escape);
    }
    key(&mut h, Key::Escape);
    if let Some(sk) = &mut h.state_mut().sketch {
        sk.picked = [0, 1].into_iter().collect();
    }
    shortcut(&mut h, "TR");
    let ends: Vec<P2> = h.state().sketch.as_ref().map(|s| s.lines.iter().map(|l| l.curve.end()).collect()).unwrap_or_default();
    assert_eq!(ends, [P2(4000.0, 1000.0), P2(4000.0, 4000.0)]);
}

#[test]
fn fillet_previews_its_arc_and_offset_its_copy_before_the_click() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [4000, 1000]}}\ndetail_line {{\"start\": [5000, 2000], \"end\": [5000, 5000]}}"
    ));
    let l = ids(&h, "line");
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    press(&mut h, "Fillet Arc");
    click(&mut h, P2(2000.0, 1000.0));
    // The first pick is held, lit as the selection is, until the second.
    assert_eq!(h.state().drawing.fillet.len(), 1);
    click(&mut h, P2(5000.0, 4000.0));
    let shown = h.state().shape_preview(P2(4500.0, 1500.0));
    assert_eq!(shown.iter().map(Curve::kind).collect::<Vec<_>>(), ["line", "arc", "line"], "the arc nearest the cursor");
    let r = h.state().fillet_radius_at(P2(4500.0, 1500.0)).unwrap_or_default();
    assert!(r > 100.0 && r < 2000.0, "{r}");
    assert_eq!(ids(&h, "line"), l, "nothing made yet");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // Offset's ghost: Modify > Offset shows where the copy goes; then the click makes it there.
    shortcut(&mut h, "OF");
    type_into(&mut h, "Offset:", "300");
    hover(&mut h, P2(2000.0, 900.0));
    click(&mut h, P2(2000.0, 900.0));
    assert_eq!(curve(&h, *ids(&h, "line").last().unwrap_or(&0)), Curve::line(P2(1000.0, 700.0), P2(4000.0, 700.0)));
}

#[test]
fn show_centre_marks_new_arcs_and_circles_and_properties_turns_it_off() {
    let mut h = detailing();
    press(&mut h, "Circle");
    press(&mut h, "Show Centre");
    click(&mut h, P2(3000.0, 3000.0));
    click(&mut h, P2(3500.0, 3000.0));
    let c = ids(&h, "line")[0];
    assert_eq!(field(&h, c, "centre_mark"), json!(true));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    h.state_mut().selection = vec![c];
    h.run();
    press(&mut h, "Center Mark Visible");
    assert_eq!(field(&h, c, "centre_mark"), json!(null), "off, and so not written");
}

#[test]
fn a_revision_cloud_is_one_cloud_round_a_rectangle_or_the_points_clicked_of_the_revision_typed() {
    let mut h = app(BOX);
    press(&mut h, "Annotate");
    press(&mut h, "Revision Cloud");
    type_into(&mut h, "Revision:", "Rev 3");
    click(&mut h, P2(1000.0, 1000.0));
    click(&mut h, P2(3000.0, 2500.0));
    let c = ids(&h, "line");
    assert_eq!(c.len(), 1, "one cloud round the rectangle");
    assert_eq!(
        (field(&h, c[0], "cloud"), field(&h, c[0], "revision"), field(&h, c[0], "closed")),
        (json!(500.0), json!("Rev 3"), json!(true))
    );
    // With Line, one cloud through the points, closed on the first.
    press(&mut h, "Line");
    for p in [P2(4000.0, 1000.0), P2(6000.0, 1000.0), P2(5000.0, 3000.0), P2(4000.0, 1000.0)] {
        click(&mut h, p);
    }
    assert_eq!(ids(&h, "line").len(), 2);
    let second = curve(&h, ids(&h, "line")[1]);
    assert!(second.is_closed() && second.vertices().len() == 3, "{second:?}");
    // The Detail Line button draws plain lines again.
    key(&mut h, Key::Escape);
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    click(&mut h, P2(1000.0, 5000.0));
    click(&mut h, P2(2000.0, 5000.0));
    assert_eq!(field(&h, *ids(&h, "line").last().unwrap_or(&0), "cloud"), json!(null));
}

#[test]
fn pick_faces_stands_a_wall_on_the_face_of_a_mass_clicked() {
    let mut h = app(r#"mass {"boundary": [[0,0],[6000,0],[6000,4000],[0,4000]], "height": 3000}"#);
    shortcut(&mut h, "WA");
    press(&mut h, "Pick Faces");
    assert!(h.query_by_label("Pick Lines").is_some(), "beside Pick Lines");
    click(&mut h, P2(3000.0, 0.0));
    let w = walls(&h);
    assert_eq!(w.len(), 1, "{w:?}");
    let (a, b) = w[0];
    assert!(a.1.abs() < 300.0 && b.1.abs() < 300.0 && (a.0 - b.0).abs() > 5000.0, "along the south face: {w:?}");
    // Away from any face, nothing, and a word why.
    click(&mut h, P2(3000.0, 2000.0));
    assert_eq!(walls(&h).len(), 1);
    assert!(h.state().status.error);
}

#[test]
fn a_fillets_radius_is_typed_once_its_two_lines_are_picked() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [4000, 1000]}}\ndetail_line {{\"start\": [5000, 2000], \"end\": [5000, 5000]}}"
    ));
    let lines = ids(&h, "line");
    press(&mut h, "Annotate");
    press(&mut h, "Detail Line");
    press(&mut h, "Fillet Arc");
    click(&mut h, P2(2000.0, 1000.0));
    click(&mut h, P2(5000.0, 4000.0));
    hover(&mut h, P2(4500.0, 1500.0));
    // Digits are the radius, not a shortcut; Enter makes the fillet of that radius.
    h.event(Event::Text("500".into()));
    h.run();
    assert_eq!(h.state().typed_length, "500");
    key(&mut h, Key::Enter);
    assert_eq!(kinds(&h), ["line", "line", "arc"]);
    assert!(curve(&h, lines[0]).end().dist(P2(4500.0, 1000.0)) < 1e-3, "{:?}", curve(&h, lines[0]));
    assert!(curve(&h, lines[1]).start().dist(P2(5000.0, 1500.0)) < 1e-3, "{:?}", curve(&h, lines[1]));
    assert!(h.state().drawing.fillet.is_empty() && h.state().typed_length.is_empty(), "ready for the next");
}
