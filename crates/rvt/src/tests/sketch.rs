//! A sketch's curves closed into regions: what makes a boundary, and what is refused.

use crate::project_fixture::arc_bytes;
use crate::sketch::{Edge, MAX_EDGES, edge_of, from_edges, from_edges_open, from_edges_snapped};

fn edge(a: (f64, f64), b: (f64, f64)) -> Edge {
    Edge { a, b, mid: Vec::new() }
}

/// The edges of the closed polygon through `points`.
fn polygon(points: &[(f64, f64)]) -> Vec<Edge> {
    (0..points.len()).map(|i| edge(points[i], points[(i + 1) % points.len()])).collect()
}

#[test]
fn three_curves_close_a_triangle_at_any_scale_and_two_do_not_close_anything() {
    for scale in [1.0, 1e-5, 1000.0] {
        let pieces = from_edges(&polygon(&[(0.0, 0.0), (10.0 * scale, 0.0), (0.0, 10.0 * scale)])).expect("a triangle");
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].outer.len(), 3, "scale {scale}");
        assert!(pieces[0].holes.is_empty());
    }
    assert!(from_edges(&polygon(&[(0.0, 0.0), (10.0, 0.0)])).is_none());
}

#[test]
fn a_vertex_on_the_line_between_its_neighbours_is_dropped_wherever_the_line_runs() {
    // The side from (3, 4) to (23, 16) is drawn as two curves, meeting at (13, 10); the other two sides are corners.
    let pieces = from_edges(&polygon(&[(3.0, 4.0), (13.0, 10.0), (23.0, 16.0), (3.0, 16.0)])).expect("a triangle with a vertex on a side");
    assert_eq!(pieces[0].outer.len(), 3, "{:?}", pieces[0].outer);
    // A bend that is slight, but not nothing, stays.
    let pieces = from_edges(&polygon(&[(0.0, 0.0), (10.0, 0.001), (20.0, 0.0), (10.0, 10.0)])).expect("a bent side");
    assert_eq!(pieces[0].outer.len(), 4);
}

#[test]
fn a_loop_may_hold_a_lens_of_two_curves_between_the_same_ends_but_not_a_curve_twice() {
    let triangle = polygon(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
    let lens = vec![edge((20.0, 0.0), (30.0, 0.0)), Edge { mid: vec![(25.0, 3.0)], ..edge((20.0, 0.0), (30.0, 0.0)) }];
    let pieces = from_edges(&[triangle.clone(), lens].concat()).expect("a triangle and a lens");
    assert_eq!(pieces.len(), 2);
    // The same curve twice, in either direction, is damage.
    let twice = vec![edge((20.0, 0.0), (30.0, 0.0)), edge((20.0, 0.0), (30.0, 0.0))];
    assert!(from_edges(&[triangle.clone(), twice].concat()).is_none());
    let back = vec![edge((20.0, 0.0), (30.0, 0.0)), edge((30.0, 0.0), (20.0, 0.0))];
    assert!(from_edges(&[triangle.clone(), back].concat()).is_none());
    // A curve with its two ends at one point, and three curves meeting at a point.
    assert!(from_edges(&[triangle.clone(), vec![edge((20.0, 0.0), (20.0, 0.0))]].concat()).is_none());
    let mut y = triangle;
    y.push(edge((0.0, 0.0), (-5.0, -5.0)));
    assert!(from_edges(&y).is_none());
}

#[test]
fn an_arcs_record_is_read_as_its_arc_where_the_line_it_also_reads_as_falls_inside_its_box() {
    // The Snowdon sample's curved floors: the record of a quarter arc about (0, 0) from (5, 0) to (0, 5) also reads as the line
    // (1, 0) to (1, 1.5708), which is inside the box, so taking the line where its ends fit turns the curve into a stub that
    // closes nothing. The box is the arc's own extent, and only the arc fills it.
    let quarter = std::f64::consts::FRAC_PI_2;
    let data = arc_bytes([0.0, quarter], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 5.0, [0.0, 0.0, 0.0]);
    let edge = edge_of(&data, &[0.0, 0.0, 0.0, 5.0, 5.0, 0.0]).expect("the arc");
    for (got, want) in [(edge.a, (5.0, 0.0)), (edge.b, (0.0, 5.0))] {
        assert!((got.0 - want.0).abs() < 1e-12 && (got.1 - want.1).abs() < 1e-12, "{got:?} against {want:?}");
    }
    assert!(!edge.mid.is_empty(), "the arc is drawn as chords");
    // A line record whose box is the line's own stays a line.
    let straight = crate::project_fixture::line_bytes(0.0, 2.0, [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]);
    let edge = edge_of(&straight, &[1.0, 1.0, 0.0, 1.0, 3.0, 0.0]).expect("the line");
    assert_eq!((edge.a, edge.b), ((1.0, 1.0), (1.0, 3.0)));
}

#[test]
fn a_curve_that_closes_nothing_is_left_out_and_the_loops_it_stands_in_still_close() {
    // The structural sample's floors have a line floating inside their rectangle (a sketch line that bounds nothing).
    let mut edges = polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    assert!(from_edges(&edges).is_some());
    edges.push(edge((2.0, 2.0), (2.0, 8.0)));
    assert!(from_edges(&edges).is_none(), "a curve that closes nothing is no boundary");
    let (pieces, dropped) = from_edges_open(&edges).expect("the rectangle");
    assert_eq!(dropped, vec![4]);
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].outer.len(), 4);
    // A ring and a stray line beside it: the ring, and the line dropped.
    let mut edges = polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    edges.push(edge((30.0, 30.0), (30.0, 40.0)));
    let (pieces, dropped) = from_edges_open(&edges).expect("the rectangle");
    assert_eq!((pieces.len(), dropped), (1, vec![4]));
    // Nothing closed at all: no floor.
    assert!(from_edges_open(&[edge((0.0, 0.0), (10.0, 0.0)), edge((10.0, 0.0), (10.0, 10.0))]).is_none());
}

#[test]
fn ends_that_miss_by_a_fraction_of_a_millimetre_still_close_the_loop() {
    // The Snowdon sample has a floor whose two ends miss by 0.06 mm (0.0002 ft): a drawing's rounding, not a gap.
    let mut edges = polygon(&[(0.0, 0.0), (30.0, 0.0), (30.0, 40.0), (0.0, 40.0)]);
    edges[3].b = (0.0002, -0.0001);
    assert!(from_edges(&edges).is_none(), "a miss is not a vertex");
    let pieces = from_edges_snapped(&edges).expect("the loop, its ends snapped");
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].outer.len(), 4);
    // A gap of a centimetre is a gap.
    let mut edges = polygon(&[(0.0, 0.0), (30.0, 0.0), (30.0, 40.0), (0.0, 40.0)]);
    edges[3].b = (0.03, 0.0);
    assert!(from_edges_snapped(&edges).is_none());
}

#[test]
fn a_sketch_holds_up_to_the_most_curves_it_is_read_from() {
    let ring = |n: usize| -> Vec<(f64, f64)> {
        (0..n)
            .map(|k| {
                (1000.0 * (std::f64::consts::TAU * k as f64 / n as f64).cos(), 1000.0 * (std::f64::consts::TAU * k as f64 / n as f64).sin())
            })
            .collect()
    };
    assert_eq!(from_edges(&polygon(&ring(MAX_EDGES))).map(|p| p[0].outer.len()), Some(MAX_EDGES));
    assert!(from_edges(&polygon(&ring(MAX_EDGES + 1))).is_none());
}

#[test]
fn an_arc_that_turns_more_than_a_full_turn_is_no_curve_of_a_sketch() {
    // The frame is turned (x north, y west) so that its arc is not also a line that fits the box: the line it would read as
    // starts at (0, 1) and runs west, out of the box.
    let bbox = [0.0, 0.0, 0.0, 10.0, 10.0, 0.0];
    let arc = |end: f64| arc_bytes([0.0, end], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], 4.0, [6.0, 6.0, 0.0]);
    let some = edge_of(&arc(6.0), &bbox).expect("an arc of nearly a turn");
    assert!(some.mid.len() > 10, "drawn as chords");
    assert!(edge_of(&arc(7.0), &bbox).is_none());
}
