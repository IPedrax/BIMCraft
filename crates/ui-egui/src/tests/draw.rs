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
        ("Circle", &[P2(4000.0, 3000.0), P2(4000.0, 3800.0)], &["circle"]),
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
    assert_eq!(circle, Some(Curve::Circle { centre: P2(4000.0, 3000.0), radius: 800.0 }));
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
