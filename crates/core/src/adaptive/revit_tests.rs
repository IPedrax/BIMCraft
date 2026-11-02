//! The adaptive point as Revit has it (docs/adaptive-revit-parity.md §1.2 to §1.5): point types with shape handles, numbering,
//! Orients to as it was measured in Revit 2025, and Revit's measurement types on a line.
use super::*;
use crate::family::Expr;

fn near(a: P3, b: P3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

fn adaptive(d: &Definition, number: u32) -> &Node {
    d.nodes
        .iter()
        .find(|n| matches!(&n.operation, Operation::Point { host: Host::Adaptive { number: k, .. }, .. } if *k == number))
        .expect("an adaptive point with that number")
}

/// Four reference points on a 2 m by 1 m rectangle, made adaptive in order: they turn clockwise seen from the family's Z, as the
/// studio's test family does.
fn panel(orients: OrientsTo) -> Definition {
    let mut d = Definition::default();
    let ids: Vec<String> =
        [[0., 0., 0.], [0., 1000., 0.], [2000., 1000., 0.], [2000., 0., 0.]].into_iter().map(|p| d.add_point(p)).collect();
    for id in &ids {
        d.make_adaptive(id).expect("made adaptive");
    }
    for n in &mut d.nodes {
        if let Operation::Point { host: Host::Adaptive { orients: o, .. }, .. } = &mut n.operation {
            *o = orients;
        }
    }
    d
}

/// Frames for the four points at `points`, with the axes `axes` gives each: what a click or a captured host hands over.
fn placed(points: [P3; 4], axes: impl Fn(usize) -> Frame) -> Placement {
    let frames = (0..4).map(|i| (i as u32 + 1, Frame { origin: points[i], ..axes(i) })).collect();
    Placement { frames, ..Placement::default() }
}

fn z_of(r: &Evaluation, number: u32, d: &Definition) -> P3 {
    match r.values.get(&adaptive(d, number).id) {
        Some(Evaluated::Point { frame }) => frame.z,
        other => panic!("point {number} did not evaluate: {other:?} {:?}", r.diagnostics),
    }
}

/// The same rectangle, tilted 30° about X: placed so it winds as the family does, or the other way round.
fn tilted(reversed: bool) -> [P3; 4] {
    let (s, c) = 30f64.to_radians().sin_cos();
    let at = |u: f64, v: f64| [u, v * c, v * s];
    let mut p = [at(0., 0.), at(0., 1000.), at(2000., 1000.), at(2000., 0.)];
    if reversed {
        p = [p[0], p[3], p[2], p[1]];
    }
    p
}

#[test]
fn make_adaptive_numbers_next_and_typing_a_taken_number_swaps_the_two() {
    let mut d = panel(OrientsTo::Instance);
    assert_eq!(d.next_number(), 5);
    let extra = d.add_point([500., 500., 0.]);
    assert_eq!(d.make_adaptive(&extra), Ok(5), "a new adaptive point takes the next number");
    // A new adaptive point orients to the instance, Revit's default.
    assert!(matches!(&adaptive(&d, 5).operation, Operation::Point { host: Host::Adaptive { orients: OrientsTo::Instance, .. }, .. }));
    // Typing 1 on point 5 swaps it with point 1, as Revit does.
    let one = adaptive(&d, 1).id.clone();
    d.set_number(&extra, 1).expect("renumbered");
    assert_eq!(adaptive(&d, 1).id, extra);
    assert_eq!(adaptive(&d, 5).id, one);
    // Only 1..N.
    assert!(d.set_number(&extra, 6).is_err());
    assert!(d.set_number(&extra, 0).is_err());
    // Back to a reference point, the numbers above close up.
    d.make_reference(&adaptive(&d, 2).id.clone()).expect("made a reference point");
    let numbers: BTreeSet<u32> = d
        .nodes
        .iter()
        .filter_map(|n| match &n.operation {
            Operation::Point { host: Host::Adaptive { number, .. }, .. } => Some(*number),
            _ => None,
        })
        .collect();
    assert_eq!(numbers, (1..=4).collect());
}

#[test]
fn the_point_type_is_reference_placement_or_shape_handle_and_a_handle_has_no_number() {
    let mut d = panel(OrientsTo::Instance);
    let p3 = adaptive(&d, 3).id.clone();
    d.set_point_type(&p3, PointType::ShapeHandle).expect("a shape handle");
    assert_eq!(d.point_type(&p3), Some(PointType::ShapeHandle));
    // The numbers close up over it, and the next is 4.
    assert_eq!(d.next_number(), 4);
    assert_eq!(adaptive(&d, 3).id, d.nodes[3].id);
    d.set_point_type(&p3, PointType::Placement).expect("a placement point again");
    assert_eq!(d.number(&p3), Some(4), "made a placement point, it takes the next number");
    d.set_point_type(&p3, PointType::Reference).expect("a reference point");
    assert_eq!(d.point_type(&p3), Some(PointType::Reference));
    assert_eq!(d.next_number(), 4);
}

#[test]
fn a_shape_handle_stays_where_the_family_has_it_in_the_instances_axes_until_dragged() {
    let mut d = panel(OrientsTo::Instance);
    let handle = d.add_point([1000., 500., 300.]);
    d.set_point_type(&handle, PointType::ShapeHandle).expect("a shape handle");
    // The four points placed 5 m east: the handle comes along.
    let p = placed([[5000., 0., 0.], [5000., 1000., 0.], [7000., 1000., 0.], [7000., 0., 0.]], |_| Frame::default());
    let r = d.evaluate(&BTreeMap::new(), &p);
    assert!(r.origin(&handle).is_some_and(|o| near(o, [6000., 500., 300.])), "{:?}", r.origin(&handle));
    // Dragged in the instance, it is where it was dragged.
    let mut p = p;
    p.handles.insert(handle.clone(), Frame::at([6000., 800., 900.]));
    let r = d.evaluate(&BTreeMap::new(), &p);
    assert!(r.origin(&handle).is_some_and(|o| near(o, [6000., 800., 900.])));
}

#[test]
fn the_instances_axes_take_the_familys_points_onto_the_placed_ones_and_flip_with_the_order() {
    let d = panel(OrientsTo::Instance);
    let (s, c) = 30f64.to_radians().sin_cos();
    let r = d.evaluate(&BTreeMap::new(), &placed(tilted(false), |_| Frame::default()));
    // The tilted rectangle is the family's turned 30° about X: so are the instance's axes.
    assert!(near(r.instance.z, [0., -s, c]), "{:?}", r.instance);
    assert!(near(r.instance.x, [1., 0., 0.]), "{:?}", r.instance);
    let r = d.evaluate(&BTreeMap::new(), &placed(tilted(true), |_| Frame::default()));
    assert!(r.instance.z[2] < 0., "placed the other way round, its Z flips: {:?}", r.instance);
}

/// What Revit gave, for each option, on the curved face of a mass: each point's host normal differs.
#[test]
fn orients_to_gives_each_point_its_axes_as_revit_does() {
    let (s, c) = 30f64.to_radians().sin_cos();
    let instance_z = [0., -s, c];
    // Each point's host has its own normal, leaning outward, as a curved face's do.
    let normals: [P3; 4] = [
        [-0.5, 0., 0.866_025_403_784_438_6],
        [-0.5, 0., 0.866_025_403_784_438_6],
        [0.5, 0., 0.866_025_403_784_438_6],
        [0.5, 0., 0.866_025_403_784_438_6],
    ];
    let host = |i: usize| Frame::from_z([0.; 3], normals[i], [0., 1., 0.]).expect("a host frame");
    let z = |o: OrientsTo, reversed: bool| {
        let d = panel(o);
        let pts = tilted(reversed);
        // Reversed, point k is clicked where point k was: its host is the one there.
        let order: [usize; 4] = if reversed { [0, 3, 2, 1] } else { [0, 1, 2, 3] };
        let r = d.evaluate(&BTreeMap::new(), &placed(pts, |i| host(order[i])));
        (1..=4).map(|n| z_of(&r, n, &d)).collect::<Vec<_>>()
    };
    // Instance: one Z for the whole panel, the instance's.
    for p in z(OrientsTo::Instance, false) {
        assert!(near(p, instance_z), "{p:?}");
    }
    for p in z(OrientsTo::InstanceZThenHost, false) {
        assert!(near(p, instance_z), "{p:?}");
    }
    // Host: each point its host's normal, whatever the order.
    for reversed in [false, true] {
        let order: [usize; 4] = if reversed { [0, 3, 2, 1] } else { [0, 1, 2, 3] };
        for (i, p) in z(OrientsTo::Host, reversed).into_iter().enumerate() {
            assert!(near(p, normals[order[i]]), "{p:?}");
        }
    }
    // Host and Loop System: the host's normal, flipped for all when the points wind against the family's.
    for (i, p) in z(OrientsTo::HostAndLoopSystem, false).into_iter().enumerate() {
        assert!(near(p, normals[i]), "{p:?}");
    }
    let order = [0, 3, 2, 1];
    for (i, p) in z(OrientsTo::HostAndLoopSystem, true).into_iter().enumerate() {
        assert!(near(p, mul(normals[order[i]], -1.)), "{p:?}");
    }
    // Global, and Global Z then Host: straight up.
    for o in [OrientsTo::Global, OrientsTo::GlobalZThenHost] {
        for p in z(o, false) {
            assert!(near(p, [0., 0., 1.]), "{o:?} {p:?}");
        }
    }
}

#[test]
fn a_file_from_before_orients_to_keeps_its_points_on_their_frames() {
    let d: Definition = serde_json::from_value(
        serde_json::json!({"nodes": [{"id": "p1", "operation": {"kind": "point", "host": {"type": "adaptive", "number": 1}}}]}),
    )
    .expect("read");
    assert!(matches!(&d.nodes[0].operation, Operation::Point { host: Host::Adaptive { orients: OrientsTo::Host, .. }, .. }));
}

#[test]
fn a_point_on_a_line_is_placed_by_each_of_revits_measurement_types() {
    let mut d = Definition::default();
    let a = d.add_point([0., 0., 0.]);
    let b = d.add_point([3000., 0., 0.]);
    let c = d.add_point([3000., 4000., 0.]);
    let line = d.add_chain(&[a, b, c], false).expect("a polyline of two segments, 3 and 4 m");
    let at = |m: Measurement, v: f64, from_end: bool| {
        let mut d = d.clone();
        let id = d.add_hosted_point(&line, 0.).expect("hosted");
        if let Some(Node { operation: Operation::Point { host: Host::Curve { location, .. }, .. }, .. }) = d.nodes.last_mut() {
            *location = CurveLocation { measurement: m, value: Expr::Number(v), from_end };
        }
        let r = d.evaluate(&BTreeMap::new(), &Placement::default());
        r.origin(&id).ok_or_else(|| format!("{:?}", r.diagnostics))
    };
    let is = |m: Measurement, v: f64, from_end: bool, want: P3| {
        let got = at(m, v, from_end).expect("on the line");
        assert!((0..3).all(|i| (got[i] - want[i]).abs() < 1e-3), "{m:?} {v}: {got:?}, not {want:?}");
    };
    is(Measurement::NormalizedCurveParameter, 0.75, false, [3000., 2000., 0.]);
    // The parameter of a line of two segments runs 0..2.
    is(Measurement::NonNormalizedCurveParameter, 1.5, false, [3000., 2000., 0.]);
    is(Measurement::SegmentLength, 3500., false, [3000., 500., 0.]);
    is(Measurement::NormalizedSegmentLength, 0.5, false, [3000., 500., 0.]);
    is(Measurement::SegmentLength, 1000., true, [3000., 3000., 0.]);
    // Chord length: 5 m in a straight line from the start is the far end; from the end, 5 m back is the start.
    is(Measurement::ChordLength, 5000., false, [3000., 4000., 0.]);
    is(Measurement::ChordLength, 4000., true, [3000., 0., 0.]);
    assert!(at(Measurement::ChordLength, 6000., false).is_err(), "no place on the line is 6 m from its start");
    assert!(at(Measurement::Angle, 30., false).is_err(), "an angle measures arcs and circles only");
    // The names files had before these are read as the same measurements.
    let old: CurveLocation = serde_json::from_value(serde_json::json!({"measurement": "normalized_length", "value": 0.5})).expect("read");
    assert_eq!(old.measurement, Measurement::NormalizedSegmentLength);
    let old: CurveLocation = serde_json::from_value(serde_json::json!({"measurement": "distance", "value": 5})).expect("read");
    assert_eq!(old.measurement, Measurement::SegmentLength);
    let old: CurveLocation = serde_json::from_value(serde_json::json!({"measurement": "parameter", "value": 0.5})).expect("read");
    assert_eq!(old.measurement, Measurement::NormalizedCurveParameter);
}

#[test]
fn a_points_show_options_default_as_revits() {
    let mut d = panel(OrientsTo::Instance);
    let one = adaptive(&d, 1).id.clone();
    let reference = d.add_point([0., 0., 500.]);
    let look = |d: &Definition, id: &str| d.nodes.iter().find(|n| n.id == id).map(|n| n.look()).expect("a point");
    assert_eq!(look(&d, &one), PointLook { number: Show::Always, planes: Show::Always, visible: false });
    assert_eq!(look(&d, &reference), PointLook { number: Show::Never, planes: Show::Never, visible: false });
    if let Some(n) = d.nodes.iter_mut().find(|n| n.id == one) {
        n.appearance.show_number = Some(Show::WhenSelected);
    }
    assert_eq!(look(&d, &one).number, Show::WhenSelected);
}

#[test]
fn a_frames_rotations_read_back_as_the_angles_that_turned_it() {
    for angles in [[0., 0., 0.], [10., 20., 30.], [-45., 5., 170.], [0., 0., -90.]] {
        let f = Frame::default().transformed([0.; 3], angles).expect("turned");
        let back = f.rotations();
        assert!((0..3).all(|i| (back[i] - angles[i]).abs() < 1e-9), "{angles:?} read back as {back:?}");
    }
}

fn frame_of(d: &Definition, id: &str) -> Frame {
    match d.evaluate(&BTreeMap::new(), &d.preview).values.get(id) {
        Some(Evaluated::Point { frame }) => *frame,
        other => panic!("{id} did not evaluate: {other:?}"),
    }
}

#[test]
fn a_point_drawn_on_a_work_plane_lies_on_it_hosted_with_the_planes_axes() {
    let mut d = panel(OrientsTo::Instance);
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    // Ref. Level, the template's default: on it, the family's own axes.
    let on_level = d.add_point_on(&WorkPlane::Datum(Datum::Level), [100., 200., 300.], &r).expect("placed");
    let f = frame_of(&d, &on_level);
    assert!(near(f.origin, [100., 200., 0.]) && near(f.z, [0., 0., 1.]), "{f:?}");
    assert_eq!(d.work_plane(&on_level), Some(WorkPlane::Datum(Datum::Level)));
    // Center (Front/Back): y = 0, its Z square to the plane, its Y up.
    let front = d.add_point_on(&WorkPlane::Datum(Datum::FrontBack), [500., 30., 700.], &r).expect("placed");
    let f = frame_of(&d, &front);
    assert!(near(f.origin, [500., 0., 700.]) && near(f.z, [0., -1., 0.]) && near(f.y, [0., 0., 1.]), "{f:?}");
    // Center (Left/Right): x = 0.
    let side = d.add_point_on(&WorkPlane::Datum(Datum::LeftRight), [40., 600., 700.], &r).expect("placed");
    assert!(near(frame_of(&d, &side).origin, [0., 600., 700.]));
    // On point 1's XZ plane: hosted on it, so it follows point 1.
    let p1 = adaptive(&d, 1).id.clone();
    let on_point = WorkPlane::Point { point: p1.clone(), plane: Plane::Xz };
    let b = d.add_point_on(&on_point, [200., 50., 300.], &r).expect("placed");
    assert!(near(frame_of(&d, &b).origin, [200., 0., 300.]));
    assert_eq!(d.work_plane(&b), Some(on_point));
    if let Some(Node { operation: Operation::Point { host: Host::Adaptive { frame, .. }, .. }, .. }) =
        d.nodes.iter_mut().find(|n| n.id == p1)
    {
        frame.origin = [0., 1000., 0.];
    }
    assert!(near(frame_of(&d, &b).origin, [200., 1000., 300.]), "it moved with its host");
    // A plane of something that is not a point is no work plane.
    assert!(d.add_point_on(&WorkPlane::Point { point: "nothing".into(), plane: Plane::Xy }, [0.; 3], &r).is_err());
}

#[test]
fn the_camera_finds_where_a_pixel_lands_on_any_plane() {
    let cam = crate::view3d::Camera {
        azimuth: crate::view3d::AZIMUTH,
        elevation: crate::view3d::ELEVATION,
        lens: None,
        target: [0., 0., 0.],
        scale: 0.1,
        width: 800,
        height: 600,
    };
    for (origin, normal) in [([0., 0., 0.], [0., 0., 1.]), ([0., 0., 0.], [0., 1., 0.]), ([250., 0., 0.], [1., 0., 0.])] {
        let p = cam.on((400., 300.), origin, normal).expect("the plane is in view");
        let off: f64 = (0..3).map(|i| (p[i] - origin[i]) * normal[i]).sum();
        assert!(off.abs() < 1e-6, "{p:?} is on the plane through {origin:?} square to {normal:?}");
        let (x, y, _) = cam.project(p);
        assert!((x - 400.).abs() < 1e-6 && (y - 300.).abs() < 1e-6, "and under the pixel");
    }
}

/// A camera looking down the family's Y axis, from the front: what a pixel hits on a plane, and how near the eye that is.
fn front_view() -> crate::view3d::Camera {
    crate::view3d::Camera { azimuth: -90., elevation: 10., lens: None, target: [1000., 0., 0.], scale: 0.2, width: 800, height: 600 }
}

#[test]
fn pick_finds_the_plane_under_the_pointer_a_points_or_a_datums() {
    let d = panel(OrientsTo::Instance);
    let r = d.evaluate(&BTreeMap::new(), &d.preview);
    let cam = front_view();
    let room = ([-300., -300., -300.], [2300., 1300., 1300.]);
    let pick = |at: P3| {
        let (x, y, _) = cam.project(at);
        r.pick_plane(&|o, n| cam.on((x, y), o, n), &|p| cam.project(p).2, 70., room)
    };
    // Near point 1, a little up from it: one of point 1's planes standing towards the eye.
    let p1 = adaptive(&d, 1).id.clone();
    let near_p1 = pick([30., 0., 40.]);
    assert!(matches!(&near_p1, Some(WorkPlane::Point { point, .. }) if *point == p1), "{near_p1:?}");
    // Up and away from every point, over Center (Front/Back): that plane.
    assert_eq!(pick([1000., 0., 1000.]), Some(WorkPlane::Datum(Datum::FrontBack)));
    // Outside every datum's rectangle and every point's planes: nothing.
    assert_eq!(pick([5000., 0., 5000.]), None);
}
