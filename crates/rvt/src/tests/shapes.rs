//! A family type's graphics read as its shape (`graphics.rs`, `shapes.rs`), from trees the fixture writes in the layout Revit's
//! own schema gives: a box's solid, and an instance that draws it.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use crate::graphics::{self, Layouts};
use crate::project_fixture::{Objects, Project, V, box_tree, graphics_block, instance_tree, p3, root, tree_bytes};
use crate::schema::Schema;
use crate::shapes::{Curve, End, Node, Patch, Shape, Surface, along, chain, split, triangles};

fn schema() -> Schema {
    Schema::parse(&Project::schema_bytes())
}

/// The one symbol a tree places and its transform, where it places one and draws nothing of its own.
fn instance(tree: &graphics::Tree, l: &Layouts) -> Result<(u32, crate::placement::Transform), String> {
    let d = crate::shapes::drawing(tree, l)?;
    match (&d.placed[..], &d.body) {
        ([one], None) => Ok(*one),
        _ => Err(format!("it places {} symbols, and {} of its own", d.placed.len(), if d.body.is_some() { "solids" } else { "nothing" })),
    }
}

fn shape_of(objects: &Objects) -> Result<Shape, String> {
    let s = schema();
    let l = Layouts::of(&s).ok_or("no GElement")?;
    let bytes = tree_bytes(objects);
    graphics::read(&bytes, 0, &l).and_then(|t| crate::shapes::shape(&t, &l))
}

fn extents(tris: &[[[f64; 3]; 3]]) -> [f64; 6] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in tris.iter().flatten() {
        for k in 0..3 {
            b[k] = b[k].min(p[k]);
            b[k + 3] = b[k + 3].max(p[k]);
        }
    }
    b
}

fn area(t: &[[f64; 3]; 3]) -> f64 {
    let (u, v) = ([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
    let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() / 2.0
}

#[test]
fn a_boxs_graphics_read_as_its_six_faces_in_twelve_triangles_in_millimetres() {
    let s = shape_of(&box_tree(500, [2.0, 3.0, 4.0], None)).unwrap();
    assert_eq!((s.solids, s.faces, s.triangles.len()), (1, 6, 12));
    let b = extents(&s.triangles);
    let want = [0.0, 0.0, 0.0, 609.6, 914.4, 1219.2];
    assert!(b.iter().zip(want).all(|(a, w)| (a - w).abs() < 0.02), "{b:?}");
    // The triangles cover the box's six faces, no more and no less.
    let surface: f64 = s.triangles.iter().map(area).sum();
    let (a, b, c) = (609.6, 914.4, 1219.2);
    assert!((surface - 2.0 * (a * b + b * c + c * a)).abs() < 1.0, "{surface}");
    assert_eq!(s.whole, [0.0, 0.0, 0.0, 2.0, 3.0, 4.0]);
}

#[test]
fn two_faces_that_put_a_point_of_their_edge_in_different_places_give_no_shape() {
    let mut tree = box_tree(500, [2.0, 3.0, 4.0], None);
    // The top face's plane, raised by a tenth of a foot: its edges no longer meet the sides'.
    if let Some((_, fields)) = tree.get_mut(&5) {
        fields[2] = V::Inline("Plane", vec![V::Byte(0), p3([0.0, 0.0, 4.1]), p3([1.0, 0.0, 0.0]), p3([0.0, 1.0, 0.0])]);
    }
    let why = shape_of(&tree).unwrap_err();
    assert!(why.contains("different places"), "{why}");
}

#[test]
fn solids_that_fall_short_of_the_box_the_graphics_give_round_them_give_no_shape() {
    // The graphics say their solids reach a foot further east than the solids read do: something is not read.
    let mut tree = box_tree(500, [2.0, 3.0, 4.0], None);
    tree.insert(2, root(500, vec![V::Own(3, "Geometry")], [0.0, 0.0, 0.0, 3.0, 3.0, 4.0], [0.0, 0.0, 0.0, 3.0, 3.0, 4.0]));
    let why = shape_of(&tree).unwrap_err();
    assert!(why.contains("not the box"), "{why}");
}

#[test]
fn an_instances_graphics_name_the_symbol_it_draws_and_the_transform_it_draws_it_by() {
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    // Turned a right angle: its X is north and its Y west.
    let axes = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let bytes = tree_bytes(&instance_tree(600, 500, axes, [10.0, 20.0, 3.0], [7.0, 20.0, 3.0, 10.0, 22.0, 7.0]));
    let tree = graphics::read(&bytes, 0, &l).unwrap();
    let (symbol, t) = instance(&tree, &l).unwrap();
    assert_eq!((symbol, t.axes, t.origin), (500, axes, [10.0, 20.0, 3.0]));
    // Read as a symbol's (a family nesting another), it holds no solid of its own: it places the one it nests, read later.
    let nests = crate::shapes::shape(&tree, &l).unwrap();
    assert_eq!((nests.placed, nests.triangles.len()), (vec![(500, t)], 0));
    // A symbol's tree places nothing: it draws solids of its own.
    let bytes = tree_bytes(&box_tree(500, [1.0, 1.0, 1.0], None));
    let d = crate::shapes::drawing(&graphics::read(&bytes, 0, &l).unwrap(), &l).unwrap();
    assert!(d.placed.is_empty() && d.body.is_some_and(|b| b.triangles.len() == 12));
}

#[test]
fn a_tree_is_found_by_its_elements_id_before_the_tag_and_in_its_root() {
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let tree = tree_bytes(&box_tree(500, [1.0, 1.0, 1.0], None));
    let mut bytes = vec![0x5a; 33];
    let first = bytes.len() + 18;
    bytes.extend(graphics_block(500, &tree));
    bytes.extend([0x11; 7]);
    // The tag of `GElement` where the id before it is not the root's, and a tree of an id the file does not declare.
    let mut wrong = graphics_block(501, &tree);
    wrong[0] = 0x77;
    bytes.extend(wrong);
    bytes.extend(graphics_block(502, &tree_bytes(&box_tree(502, [1.0, 1.0, 1.0], None))));
    let ids: BTreeSet<u32> = [500, 501].into();
    assert_eq!(graphics::roots(&bytes, &l, &ids), [(500, first)]);
    assert!(graphics::read(&bytes, first, &l).is_ok());
}

#[test]
fn a_tree_that_breaks_the_rules_is_refused_saying_why() {
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let refused = |bytes: &[u8], says: &str| {
        let why = graphics::read(bytes, 0, &l).map(|_| ()).unwrap_err();
        assert!(why.contains(says), "{says}: {why}");
    };
    let good = tree_bytes(&box_tree(500, [1.0, 1.0, 1.0], None));
    // Cut short.
    refused(&good[..good.len() - 3], "ends");
    // The root's one node of a class the schema does not have: after its GInfo (20 bytes), the count and the index.
    let mut bad = good.clone();
    bad[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
    refused(&bad, "schema does not have");
    // A face owned twice.
    let mut tree = box_tree(500, [1.0, 1.0, 1.0], None);
    if let Some((_, fields)) = tree.get_mut(&3) {
        fields[1] = V::Counted((4..10).chain([4]).map(|i| V::Own(i, "Face")).collect());
    }
    refused(&tree_bytes(&tree), "owned twice");
    // A count past what a list may hold.
    let mut bad = good.clone();
    bad[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    refused(&bad, "a list of");
}

#[test]
fn hostile_graphics_never_panic() {
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let good = tree_bytes(&box_tree(500, [2.0, 3.0, 4.0], None));
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for at in 0..good.len() {
        for value in [0u8, 0xff, 0x7f, good[at] ^ 0x55, next() as u8] {
            let mut bad = good.clone();
            bad[at] = value;
            let _ = graphics::read(&bad, 0, &l).and_then(|t| crate::shapes::shape(&t, &l));
        }
        let _ = graphics::read(&good[..at], 0, &l).and_then(|t| crate::shapes::shape(&t, &l));
    }
    // Words of eight bytes overwritten with numbers that are no lengths or coordinates.
    for _ in 0..2_000 {
        let mut bad = good.clone();
        let at = (next() as usize) % (bad.len() - 8);
        let word = [f64::NAN.to_bits(), f64::INFINITY.to_bits(), 1e300f64.to_bits(), next()][(next() % 4) as usize];
        bad[at..at + 8].copy_from_slice(&word.to_le_bytes());
        let _ = graphics::read(&bad, 0, &l).and_then(|t| crate::shapes::shape(&t, &l));
    }
    let _ = graphics::roots(&good, &l, &[500].into());
}

#[test]
fn a_cylinders_face_is_cut_every_fifteen_degrees_on_its_surface() {
    // Half a cylinder of radius 1 ft and 2 ft tall: u round it, v up it.
    let s = Surface::Cylinder { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], r: 1.0 };
    let mut out = Vec::new();
    triangles(&s, &[vec![[0.0, 0.0], [PI, 0.0], [PI, 2.0], [0.0, 2.0]]], &mut out).unwrap();
    assert_eq!(out.len(), 24);
    assert!(out.iter().flatten().all(|p| (p[0].hypot(p[1]) - 304.8).abs() < 1e-6 && (-1e-6..=609.6 + 1e-6).contains(&p[2])));
    // Its facets' area is the polygon's of 12 sides, a little under the half cylinder's.
    let surface: f64 = out.iter().map(area).sum();
    let polygon = 12.0 * 2.0 * 304.8 * (PI / 24.0).sin() * 609.6;
    assert!((surface - polygon).abs() < 1.0, "{surface} {polygon}");
}

#[test]
fn a_surface_of_revolution_turns_its_profile_about_its_axis() {
    // An arc of radius 1 ft in the plane y = 0 turned about z: a sphere.
    let profile = Curve::Arc { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0], r: 1.0 };
    let s = Surface::Revolved { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], profile };
    let mut out = Vec::new();
    triangles(&s, &[vec![[0.0, 0.0], [PI / 2.0, 0.0], [PI / 2.0, PI / 4.0], [0.0, PI / 4.0]]], &mut out).unwrap();
    assert!(out.len() >= 12, "{}", out.len());
    let r = |p: &[f64; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    assert!(out.iter().flatten().all(|p| (r(p) - 304.8).abs() < 1e-6 && p[0] >= -1e-9 && p[1] >= -1e-9 && p[2] >= -1e-9));
    // A line turned about z: a cone's side, from the axis out.
    let line = Curve::Line { origin: [0.0; 3], dir: [1.0, 0.0, 1.0] };
    let s = Surface::Revolved { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], profile: line };
    let mut out = Vec::new();
    triangles(&s, &[vec![[0.0, 0.5], [PI, 0.5], [PI, 1.0], [0.0, 1.0]]], &mut out).unwrap();
    assert!(out.iter().flatten().all(|p| (p[0].hypot(p[1]) - p[2]).abs() < 1e-6));
}

#[test]
fn a_surface_of_revolution_turning_an_ellipse_is_cut_every_fifteen_degrees_both_ways() {
    // An ellipse 2 ft along x and 1 ft along z, in the plane y = 0, turned about z: a spheroid, (r / 2)² + z² = 1 in feet.
    let profile = Curve::Ellipse { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0], rx: 2.0, ry: 1.0 };
    let s = Surface::Revolved { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], profile };
    let mut out = Vec::new();
    triangles(&s, &[vec![[0.0, 0.0], [PI / 2.0, 0.0], [PI / 2.0, PI / 2.0], [0.0, PI / 2.0]]], &mut out).unwrap();
    // Six steps round, six up.
    assert!(out.len() >= 72, "{}", out.len());
    let on = |p: &[f64; 3]| ((p[0].hypot(p[1]) / 609.6).powi(2) + (p[2] / 304.8).powi(2) - 1.0).abs() < 1e-9;
    assert!(out.iter().flatten().all(on));
    let top = out.iter().flatten().map(|p| p[2]).fold(f64::MIN, f64::max);
    let out_most = out.iter().flatten().map(|p| p[0].hypot(p[1])).fold(f64::MIN, f64::max);
    assert!((top - 304.8).abs() < 1e-6 && (out_most - 609.6).abs() < 1e-6, "{top} {out_most}");
}

#[test]
fn a_face_on_a_ruled_surface_reads_as_the_curves_it_runs_between_each_along_its_own_range() {
    // A cube a foot across whose top is a ruled surface between two lines a foot long, across its south and north edges; the north
    // one's parameters run from -1 to 0 with its origin at the east end, so a point a part `u` along both is `u` feet east.
    let mut objects = box_tree(500, [1.0; 3], None);
    let line = |at: [f64; 3], ends: [f64; 2]| {
        ("GLine", vec![crate::project_fixture::ginfo(0), V::Seq(ends.map(V::F64).to_vec()), p3(at), p3([1.0, 0.0, 0.0])])
    };
    objects.insert(900, line([0.0, 0.0, 1.0], [0.0, 1.0]));
    objects.insert(901, line([1.0, 1.0, 1.0], [-1.0, 0.0]));
    let ruled = V::Inline("RuledSurf", vec![V::Byte(0), V::Own(900, "GLine"), V::Own(901, "GLine"), p3([0.0; 3]), p3([0.0; 3])]);
    let Some((_, top)) = objects.get_mut(&5) else { panic!("no top face") };
    top[2] = ruled;
    let s = shape_of(&objects).unwrap();
    assert_eq!(s.triangles.len(), 12);
    let e = extents(&s.triangles);
    assert!(e.iter().zip([0.0, 0.0, 0.0, 304.8, 304.8, 304.8]).all(|(a, b)| (a - b).abs() < 1e-6), "{e:?}");
    let surface: f64 = s.triangles.iter().map(area).sum::<f64>() / (304.8 * 304.8);
    assert!((surface - 6.0).abs() < 1e-6, "{surface}");
}

#[test]
fn a_ruled_surface_between_two_arcs_is_cut_along_them_and_straight_across_and_one_to_a_point_is_a_cone() {
    // A quarter of a cone's slice: an arc of 2 ft at the floor and one of 1 ft a foot up, each a quarter turn.
    let arc = |z: f64, r: f64| Curve::Arc { center: [0.0, 0.0, z], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], r };
    let square = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let s = Surface::Ruled { ends: [End::Curve(arc(0.0, 2.0), [0.0, PI / 2.0]), End::Curve(arc(1.0, 1.0), [0.0, PI / 2.0])] };
    let mut out = Vec::new();
    triangles(&s, std::slice::from_ref(&square), &mut out).unwrap();
    // Six steps of fifteen degrees round, none needed up the straight lines.
    assert!(out.len() >= 12, "{}", out.len());
    // On the cone: 2 ft out at the floor, 1 ft a foot up.
    let on = |p: &[f64; 3]| (p[0].hypot(p[1]) / 304.8 - (2.0 - p[2] / 304.8)).abs() < 1e-9;
    assert!(out.iter().flatten().all(on), "{out:?}");
    let angles: BTreeSet<i64> = out.iter().flatten().map(|p| (p[1].atan2(p[0]).to_degrees() * 1e6).round() as i64).collect();
    assert!(angles.len() >= 7, "{angles:?}");
    // The upper end a point: a cone, its radius 2 ft at the floor and none a foot up.
    let s = Surface::Ruled { ends: [End::Curve(arc(0.0, 2.0), [0.0, PI / 2.0]), End::Point([0.0, 0.0, 1.0])] };
    let mut out = Vec::new();
    triangles(&s, &[square], &mut out).unwrap();
    assert!(out.iter().flatten().all(|p| (p[0].hypot(p[1]) / 304.8 - 2.0 * (1.0 - p[2] / 304.8)).abs() < 1e-9), "{out:?}");
}

#[test]
fn a_face_on_a_cone_is_cut_round_its_axis_and_straight_down_its_side() {
    // A cone with its apex at the origin, its axis up and its half-angle 45 degrees: `v` feet down its side from the apex, `u`
    // round from x. A quarter of it, 1 to 2 ft down its side.
    let s = Surface::Cone { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], half_angle: PI / 4.0 };
    let mut out = Vec::new();
    triangles(&s, &[vec![[0.0, 1.0], [PI / 2.0, 1.0], [PI / 2.0, 2.0], [0.0, 2.0]]], &mut out).unwrap();
    assert!(out.len() >= 12, "{}", out.len());
    // On it: as far out as up, and as far from the apex as `v` says, between 1 and 2 ft.
    let ft = |p: &[f64; 3]| p.map(|k| k / 304.8);
    assert!(out.iter().flatten().map(ft).all(|p| (p[0].hypot(p[1]) - p[2]).abs() < 1e-9), "{out:?}");
    let slant: Vec<f64> = out.iter().flatten().map(ft).map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()).collect();
    assert!(slant.iter().all(|d| (1.0 - 1e-9..=2.0 + 1e-9).contains(d)), "{slant:?}");
    assert!(slant.iter().any(|d| (d - 2.0).abs() < 1e-9) && slant.iter().any(|d| (d - 1.0).abs() < 1e-9));
    // Its point at u = 0, v = 2: 2 sin 45 out along x, 2 cos 45 up.
    let want = [2.0 * (PI / 4.0).sin(), 0.0, 2.0 * (PI / 4.0).cos()];
    assert!(out.iter().flatten().map(ft).any(|p| p.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9)));
}

#[test]
fn a_hermite_spline_is_the_cubic_through_its_nodes_with_their_tangents() {
    // The parabola z = x² for x from 0 to 2 ft, as three nodes with their tangents: a cubic Hermite spline is any cubic exactly. A
    // ruled surface between it and the same a foot north.
    let parabola = |y: f64| {
        let nodes = [0.0, 1.0, 2.0].map(|t: f64| Node { t, at: [t, y, t * t], tangent: [1.0, 0.0, 2.0 * t] });
        End::Curve(Curve::Spline(nodes.to_vec().into()), [0.0, 2.0])
    };
    let s = Surface::Ruled { ends: [parabola(0.0), parabola(1.0)] };
    let mut out = Vec::new();
    triangles(&s, &[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]], &mut out).unwrap();
    let ft = |p: &[f64; 3]| p.map(|k| k / 304.8);
    assert!(out.iter().flatten().map(ft).all(|p| (p[2] - p[0] * p[0]).abs() < 1e-9), "{out:?}");
    // Cut a third of a node's span at most along it: two spans, six cuts.
    let xs: BTreeSet<i64> = out.iter().flatten().map(ft).map(|p| (p[0] * 1e6).round() as i64).collect();
    assert!(xs.len() >= 7, "{xs:?}");
}

#[test]
fn a_ruled_surface_between_hermite_splines_is_read_from_its_nodes_and_one_not_read_is_said() {
    // The cube's top as a ruled surface between two splines that are straight lines a foot long: two nodes each.
    use crate::project_fixture::ginfo;
    let node = |t: f64, at: [f64; 3]| V::Seq(vec![V::F64(t), p3(at), p3([1.0, 0.0, 0.0])]);
    let spline = |nodes: Vec<V>, periodic: u8| {
        ("GHermiteSpline", vec![ginfo(0), V::Seq(vec![V::F64(0.0), V::F64(1.0)]), V::Byte(periodic), V::Counted(nodes)])
    };
    let top = |a: (&'static str, Vec<V>), b: (&'static str, Vec<V>)| {
        let mut objects = box_tree(500, [1.0; 3], None);
        objects.insert(900, a);
        objects.insert(901, b);
        let ruled = V::Inline(
            "RuledSurf",
            vec![V::Byte(0), V::Own(900, "GHermiteSpline"), V::Own(901, "GHermiteSpline"), p3([0.0; 3]), p3([0.0; 3])],
        );
        if let Some((_, face)) = objects.get_mut(&5) {
            face[2] = ruled;
        }
        objects
    };
    let south = || vec![node(0.0, [0.0, 0.0, 1.0]), node(1.0, [1.0, 0.0, 1.0])];
    let north = || vec![node(0.0, [0.0, 1.0, 1.0]), node(1.0, [1.0, 1.0, 1.0])];
    let s = shape_of(&top(spline(south(), 0), spline(north(), 0))).unwrap();
    // The top cut in thirds along the splines, six triangles; the sides, which are cut as they are, two each.
    assert_eq!(s.triangles.len(), 16);
    let surface: f64 = s.triangles.iter().map(area).sum::<f64>() / (304.8 * 304.8);
    assert!((surface - 6.0).abs() < 1e-6, "{surface}");
    // One that closes on itself, and one whose nodes do not go on in their parameter, are not read.
    let why = shape_of(&top(spline(south(), 1), spline(north(), 0))).unwrap_err();
    assert!(why.contains("periodic"), "{why}");
    let back = vec![node(1.0, [0.0, 0.0, 1.0]), node(0.0, [1.0, 0.0, 1.0])];
    let why = shape_of(&top(spline(back, 0), spline(north(), 0))).unwrap_err();
    assert!(why.contains("parameter"), "{why}");
}

/// The surface z = u² v + v³ (feet), at most a cubic each way, which a bicubic Hermite surface through its nodes is exactly: its
/// point, its derivatives along u and v and its mixed one at (u, v).
fn cubic(u: f64, v: f64) -> Patch {
    Patch {
        at: [u, v, u * u * v + v * v * v],
        du: [1.0, 0.0, 2.0 * u * v],
        dv: [0.0, 1.0, u * u + 3.0 * v * v],
        twist: [0.0, 0.0, 2.0 * u],
    }
}

#[test]
fn a_hermite_surface_is_the_bicubic_through_its_grid_of_nodes_inside_its_faces_too() {
    // Nodes at u = 0, 1, 2 and v = 0, 0.5, 1.5 (spaced unevenly), u running fastest.
    let (us, vs) = ([0.0, 1.0, 2.0], [0.0, 0.5, 1.5]);
    let nodes: Vec<Patch> = vs.iter().flat_map(|v| us.iter().map(|u| cubic(*u, *v))).collect();
    let s = Surface::Hermite { u: us.to_vec().into(), v: vs.to_vec().into(), nodes: nodes.into() };
    let mut out = Vec::new();
    triangles(&s, &[vec![[0.0, 0.0], [2.0, 0.0], [2.0, 1.5], [0.0, 1.5]]], &mut out).unwrap();
    let ft = |p: &[f64; 3]| p.map(|k| k / 304.8);
    assert!(out.iter().flatten().map(ft).all(|p| (p[2] - (p[0] * p[0] * p[1] + p[1].powi(3))).abs() < 1e-9), "{out:?}");
    // Points inside it, not only round its edges.
    let inner = out.iter().flatten().map(ft).filter(|p| p[0] > 0.01 && p[0] < 1.99 && p[1] > 0.01 && p[1] < 1.49).count();
    assert!(inner > 0, "{out:?}");
}

#[test]
fn a_face_on_a_hermite_surface_is_read_from_its_nodes_and_one_not_read_is_said() {
    // The cube's top as a Hermite surface of four nodes, a plane z = 1 ft: one not built, one that closes on itself, and one whose
    // nodes are not its grid are not read.
    let flat = |u: f64, v: f64| V::Seq(vec![p3([u, v, 1.0]), V::Seq(vec![p3([1.0, 0.0, 0.0]), p3([0.0, 1.0, 0.0])]), p3([0.0; 3])]);
    let top = |periodic: u8, built: u8, nodes: Vec<V>| {
        let mut objects = box_tree(500, [1.0; 3], None);
        let params = || V::Counted(vec![V::F64(0.0), V::F64(1.0)]);
        let surface = V::Inline(
            "HermiteSurf",
            vec![V::Byte(0), V::Seq(vec![V::Byte(periodic), V::Byte(0)]), V::Byte(built), V::Counted(nodes), params(), params()],
        );
        if let Some((_, face)) = objects.get_mut(&5) {
            face[2] = surface;
        }
        objects
    };
    let grid = || vec![flat(0.0, 0.0), flat(1.0, 0.0), flat(0.0, 1.0), flat(1.0, 1.0)];
    let s = shape_of(&top(0, 1, grid())).unwrap();
    let surface: f64 = s.triangles.iter().map(area).sum::<f64>() / (304.8 * 304.8);
    assert!((surface - 6.0).abs() < 1e-6, "{surface}");
    let e = extents(&s.triangles);
    assert!(e.iter().zip([0.0, 0.0, 0.0, 304.8, 304.8, 304.8]).all(|(a, b)| (a - b).abs() < 1e-6), "{e:?}");
    for (objects, says) in [(top(1, 1, grid()), "periodic"), (top(0, 0, grid()), "not built"), (top(0, 1, grid()[..3].to_vec()), "grid")] {
        let why = shape_of(&objects).unwrap_err();
        assert!(why.contains(says), "{why}");
    }
}

#[test]
fn edges_that_meet_only_at_a_pole_are_joined_there() {
    // Half a dome on a sphere of a foot (its profile an arc whose top, v = 90 degrees, is the pole), from v = 0.3 up: two edges up
    // its sides, which meet only at the pole, where their ends are far apart in (u, v), and one round its rim; in the order its loop
    // walks them, the two up its sides both run the wrong way to be joined by their (u, v).
    let profile = Curve::Arc { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0], r: 1.0 };
    let s = Surface::Revolved { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], profile };
    let (rim, top) = (0.3, PI / 2.0);
    let up = |u: f64| (0..=4).map(|k| [u, rim + (top - rim) * f64::from(k) / 4.0]).collect::<Vec<_>>();
    let mut a = up(2.0 * PI);
    a.reverse();
    let b = up(PI);
    let c: Vec<[f64; 2]> = (0..=8).map(|k| [PI + PI * f64::from(k) / 8.0, rim]).collect();
    let outline = chain(vec![a, b, c], &s);
    let mut out = Vec::new();
    triangles(&s, &[outline], &mut out).unwrap();
    // Its area, a half of the sphere's zone above the rim: pi (1 - sin 0.3) square feet, within the cut's error.
    let surface: f64 = out.iter().map(area).sum::<f64>() / (304.8 * 304.8);
    let want = PI * (1.0 - rim.sin());
    assert!((surface - want).abs() < 0.03 * want, "{surface} {want}");
}

#[test]
fn a_flat_face_takes_the_points_of_an_edge_its_curved_neighbour_puts_on_its_curve() {
    // A quarter circle of a foot, the edge between the floor (z = 0) and a cylinder round z, known by its ends alone: on the cylinder
    // it runs along u, so points along it there are on the circle, and the floor takes them.
    let floor = Surface::Plane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let wall = Surface::Cylinder { center: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], r: 1.0 };
    let (mine, theirs) = ([[1.0, 0.0], [0.0, 1.0]], [[0.0, 0.0], [PI / 2.0, 0.0]]);
    let points = along(&floor, &wall, &mine, &theirs).unwrap();
    // Every half of the cylinder's step, 7.5 degrees: twelve cuts.
    assert!(points.len() >= 13, "{}", points.len());
    assert!(points.iter().all(|p| (p[0].hypot(p[1]) - 1.0).abs() < 1e-9), "{points:?}");
    assert_eq!((points.first(), points.last()), (Some(&[1.0, 0.0]), Some(&[0.0, 1.0])));
    // The cylinder cuts its side of the edge at the same points, so the two faces meet with no gap between them.
    let own: Vec<[f64; 2]> = split(&theirs, wall.fine()).into_iter().map(|(p, _)| p).collect();
    assert_eq!(own.len(), points.len());
    assert!(own.iter().zip(&points).all(|(q, p)| wall.at(*q).iter().zip(floor.at(*p)).all(|(a, b)| (a - b).abs() < 1e-9)));
    // A floor the circle does not lie on takes none of them; nor does a curved face, which cuts its own edges.
    let higher = Surface::Plane { origin: [0.0, 0.0, 0.5], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    assert!(along(&higher, &wall, &mine, &theirs).is_none());
    assert!(along(&wall, &floor, &theirs, &mine).is_none());
}

#[test]
fn a_shape_whose_solids_fill_the_box_round_all_its_graphics_reads_though_they_pass_the_tight_one() {
    // A stair's tread: its nosing reaches past the box its graphics give round its solids (`m_tightbBox`), which leaves it out, but
    // its solids fill the other box (`m_bBox`) exactly. Here a cube 2 by 3 by 4 ft, the tight box a foot short of its top.
    let mut objects = box_tree(500, [2.0, 3.0, 4.0], None);
    let cube = [0.0, 0.0, 0.0, 2.0, 3.0, 4.0];
    let short = [0.0, 0.0, 0.0, 2.0, 3.0, 3.0];
    objects.insert(2, root(500, vec![V::Own(3, "Geometry")], cube, short));
    let s = shape_of(&objects).unwrap();
    assert_eq!(s.triangles.len(), 12);
    // Past both boxes, it is not read: the other box a foot short of its top too.
    objects.insert(2, root(500, vec![V::Own(3, "Geometry")], short, short));
    let why = shape_of(&objects).unwrap_err();
    assert!(why.contains("not the box"), "{why}");
}

#[test]
fn a_face_whose_loops_bound_two_regions_and_a_hole_is_cut_as_each() {
    let s = Surface::Plane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let square = |x: f64, y: f64, w: f64| vec![[x, y], [x + w, y], [x + w, y + w], [x, y + w]];
    // Two squares 2 ft across side by side, the second with a hole 1 ft across, named first.
    let loops = [square(3.0, 1.0, 1.0), square(0.0, 0.0, 2.0), square(2.5, 0.5, 2.0)];
    let mut out = Vec::new();
    triangles(&s, &loops, &mut out).unwrap();
    let mm2 = 304.8 * 304.8;
    let surface: f64 = out.iter().map(area).sum::<f64>() / mm2;
    assert!((surface - (4.0 + 4.0 - 1.0)).abs() < 1e-6, "{surface}");
}

#[test]
fn a_face_of_more_corners_than_a_triangulation_takes_is_refused_before_its_loops_are_compared() {
    let s = Surface::Plane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let n = bimcraft_core::triangulation::MAX_CORNERS + 1;
    let ring: Vec<[f64; 2]> = (0..n).map(|k| [(2.0 * PI * k as f64 / n as f64).cos(), (2.0 * PI * k as f64 / n as f64).sin()]).collect();
    let mut out = Vec::new();
    let why = triangles(&s, &[ring], &mut out).unwrap_err();
    assert!(why.contains("more corners"), "{why}");
}

#[test]
fn an_inline_object_follows_the_last_object_queued_when_its_owner_is_written() {
    // Two instances side by side under the root, each owning only its info, inline: neither info follows its own instance, both
    // follow the second, the last queued when either is written (the layout of a symbol's image beside the filters after it).
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut objects = instance_tree(600, 500, axes, [1.0, 2.0, 3.0], [0.0; 6]);
    let second = instance_tree(600, 501, axes, [4.0, 5.0, 6.0], [0.0; 6]);
    objects.insert(4, second[&3].clone());
    objects.insert(2, root(600, vec![V::Own(3, "GInstance"), V::Own(4, "GInstance")], [0.0; 6], [0.0; 6]));
    let tree = graphics::read(&tree_bytes(&objects), 0, &l).unwrap();
    for (index, symbol) in [(3, 500), (4, 501)] {
        let info = &tree.get(index).unwrap().inline[0];
        let named = format!("{:?}", l.get(info, "m_symbolId"));
        assert!(named.contains(&format!("Int({symbol})")), "{index}: {named}");
    }
}

#[test]
fn an_inline_object_may_own_inline_objects_of_its_own() {
    // A fill's pattern, inline, owns its grids inline (the layout of a filled region drawn in a family), beside an instance whose
    // info is inline too: the pattern and the info follow the instance, the last queued, and the grids follow it too, after them.
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut objects = instance_tree(600, 501, axes, [4.0, 5.0, 6.0], [0.0; 6]);
    objects.insert(4, objects[&3].clone());
    let grids = V::Counted(vec![V::Inline("FillGrid", vec![V::F64(0.5)]), V::Inline("FillGrid", vec![V::F64(1.5)])]);
    objects.insert(3, ("GFilling", vec![crate::project_fixture::ginfo(0), V::Inline("FillPatternData", vec![V::F64(2.0), grids])]));
    objects.insert(2, root(600, vec![V::Own(3, "GFilling"), V::Own(4, "GInstance")], [0.0; 6], [0.0; 6]));
    let tree = graphics::read(&tree_bytes(&objects), 0, &l).unwrap();
    let pattern = &tree.get(3).unwrap().inline[0];
    assert_eq!(l.get(pattern, "m_scale"), Some(&graphics::Value::Real(2.0)));
    let angles: Vec<_> = pattern.inline.iter().map(|g| l.get(g, "m_angle").cloned()).collect();
    assert_eq!(angles, [Some(graphics::Value::Real(0.5)), Some(graphics::Value::Real(1.5))]);
    let named = format!("{:?}", l.get(&tree.get(4).unwrap().inline[0], "m_symbolId"));
    assert!(named.contains("Int(501)"), "{named}");
}

#[test]
fn a_meshs_points_are_four_byte_floats_and_its_facets_two_byte_indices() {
    // Kind 6 is a four-byte float and kind 3 a two-byte integer: a mesh's points and facets (the layout of a light's mesh), read
    // right, leave the stream where the next object starts.
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let points =
        V::Counted(vec![V::Seq(vec![V::F32(0.5), V::F32(1.5), V::F32(-2.0)]), V::Seq(vec![V::F32(1.0), V::F32(0.0), V::F32(0.25)])]);
    let facets = V::Counted(vec![V::Seq(vec![V::U16(0), V::U16(1), V::U16(65535)])]);
    let mut objects = Objects::new();
    objects.insert(2, root(600, vec![V::Own(3, "GInstance")], [0.0; 6], [0.0; 6]));
    objects.insert(3, ("GInstance", vec![crate::project_fixture::ginfo(0), V::Inline("FloatMesh", vec![points, facets])]));
    let bytes = tree_bytes(&objects);
    let tree = graphics::read(&bytes, 0, &l).unwrap();
    let mesh = &tree.get(3).unwrap().inline[0];
    let point = |k: usize| l.get(mesh, "m_pointsArr").and_then(|p| p.list()?.get(k)?.point());
    assert_eq!((point(0), point(1)), (Some([0.5, 1.5, -2.0]), Some([1.0, 0.0, 0.25])));
    let facet = format!("{:?}", l.get(mesh, "m_facetsArr"));
    assert!(facet.contains("Int(0), Int(1), Int(65535)"), "{facet}");
}

#[test]
fn an_elements_tree_draws_the_symbols_it_places_and_the_solids_it_holds_whatever_else_it_holds() {
    // A light's tree: a group (of the point its light leaves from) beside the instance; read as the instance alone.
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let axes = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let draws = |objects: &Objects| crate::shapes::drawing(&graphics::read(&tree_bytes(objects), 0, &l).unwrap(), &l);
    let mut objects = instance_tree(600, 500, axes, [1.0, 2.0, 3.0], [0.0; 6]);
    objects.insert(4, objects[&3].clone());
    objects.insert(3, ("GGroup", vec![crate::project_fixture::ginfo(0), V::Counted(vec![])]));
    objects.insert(2, root(600, vec![V::Own(3, "GGroup"), V::Own(4, "GInstance")], [0.0; 6], [0.0; 6]));
    let d = draws(&objects).unwrap();
    assert_eq!((d.placed.len(), d.placed[0].0, d.placed[0].1.origin, d.body.is_none()), (1, 500, [1.0, 2.0, 3.0], true));
    // A rail's tree: a solid of its own where it stands beside the symbol it places.
    let mut objects = box_tree(600, [1.0, 1.0, 1.0], None);
    objects.insert(1000, instance_tree(600, 500, axes, [1.0, 2.0, 3.0], [0.0; 6])[&3].clone());
    objects.insert(2, root(600, vec![V::Own(3, "Geometry"), V::Own(1000, "GInstance")], [0.0; 6], [0.0; 6]));
    let d = draws(&objects).unwrap();
    assert_eq!((d.placed.len(), d.body.map(|b| b.triangles.len())), (1, Some(12)));
    // Two symbols placed (a railing's balusters).
    let mut objects = instance_tree(600, 500, axes, [1.0, 2.0, 3.0], [0.0; 6]);
    objects.insert(4, instance_tree(600, 501, axes, [4.0, 2.0, 3.0], [0.0; 6])[&3].clone());
    objects.insert(2, root(600, vec![V::Own(3, "GInstance"), V::Own(4, "GInstance")], [0.0; 6], [0.0; 6]));
    let d = draws(&objects).unwrap();
    let placed: Vec<(u32, [f64; 3])> = d.placed.iter().map(|(s, t)| (*s, t.origin)).collect();
    assert_eq!(placed, [(500, [1.0, 2.0, 3.0]), (501, [4.0, 2.0, 3.0])]);
    // A tree with neither is no drawing.
    objects.insert(2, root(600, vec![], [0.0; 6], [0.0; 6]));
    let why = draws(&objects).unwrap_err();
    assert!(why.contains("no solid and no instance"), "{why}");
}

#[test]
fn the_box_round_a_trees_solids_may_take_in_the_points_and_lines_drawn_with_them() {
    // A stair's support: its solid, a point above it and a line reaching west of it, the box its graphics give round its solids
    // taking in both. What is read, its solid and its marks, fills that box; a solid alone would not.
    let tight = [-1.0, 0.0, 0.0, 2.0, 3.0, 6.0];
    let mut objects = box_tree(500, [2.0, 3.0, 4.0], None);
    let point = ("GPoint", vec![crate::project_fixture::ginfo(0), p3([1.0, 1.0, 6.0]), V::U32(6), V::U32(2), V::U32(0)]);
    let line = (
        "GLine",
        vec![crate::project_fixture::ginfo(0), V::Seq(vec![V::F64(-1.0), V::F64(2.0)]), p3([0.0, 0.0, 0.0]), p3([1.0, 0.0, 0.0])],
    );
    objects.insert(1000, point);
    objects.insert(1001, line);
    objects.insert(2, root(500, vec![V::Own(3, "Geometry"), V::Own(1000, "GPoint"), V::Own(1001, "GLine")], tight, tight));
    let s = shape_of(&objects).unwrap();
    assert_eq!(s.triangles.len(), 12);
    let marks = extents(&[[s.marks[0], s.marks[1], s.marks[2]]]);
    assert!(s.marks.len() >= 3 && marks[2] < 0.01 && (marks[5] - 1828.8).abs() < 0.01, "{:?}", s.marks);
    // Raised further than its marks reach, the box holds something unread.
    objects.insert(
        2,
        root(500, vec![V::Own(3, "Geometry"), V::Own(1000, "GPoint"), V::Own(1001, "GLine")], tight, [-1.0, 0.0, 0.0, 2.0, 3.0, 7.0]),
    );
    let why = shape_of(&objects).unwrap_err();
    assert!(why.contains("not the box"), "{why}");
}

#[test]
fn a_symbol_drawn_as_a_mesh_reads_as_its_facets() {
    // A tetrahedron as a mesh (`GPolyMesh` over its faceted topology): four points, feet, and four facets of three indices.
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let corners = [[0.0f32, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
    let tetra = |facets: Vec<[u16; 3]>| {
        let points = V::Counted(corners.iter().map(|p| V::Seq(p.iter().map(|v| V::F32(*v)).collect())).collect());
        let facets = V::Counted(facets.iter().map(|f| V::Seq(f.iter().map(|i| V::U16(*i)).collect())).collect());
        let mut objects = Objects::new();
        let b = [0.0, 0.0, 0.0, 2.0, 3.0, 4.0];
        objects.insert(2, root(500, vec![V::Own(3, "GPolyMesh")], b, b));
        objects.insert(3, ("GPolyMesh", vec![crate::project_fixture::ginfo(0), V::Inline("FloatMesh", vec![points, facets])]));
        graphics::read(&tree_bytes(&objects), 0, &l).and_then(|t| crate::shapes::shape(&t, &l))
    };
    let shape = tetra(vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]).unwrap();
    assert_eq!(shape.triangles.len(), 4);
    let b = extents(&shape.triangles);
    assert!(b.iter().zip([0.0, 0.0, 0.0, 609.6, 914.4, 1219.2]).all(|(a, w)| (a - w).abs() < 0.01), "{b:?}");
    // A facet naming a point the mesh does not have is refused.
    let why = tetra(vec![[0, 2, 1], [0, 1, 9]]).unwrap_err();
    assert!(why.contains("point 9"), "{why}");
}

#[test]
fn a_mesh_reaching_outside_the_box_round_the_solids_is_none_of_them() {
    // A light: its solid, and a mesh (its photometric web) reaching above the box its graphics give round its solids.
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let mut objects = box_tree(500, [2.0, 3.0, 4.0], None);
    let points = V::Counted(
        [[0.0f32, 0.0, 0.0], [1.0, 0.0, 9.0], [0.0, 1.0, 9.0]].iter().map(|p| V::Seq(p.iter().map(|v| V::F32(*v)).collect())).collect(),
    );
    let facets = V::Counted(vec![V::Seq(vec![V::U16(0), V::U16(1), V::U16(2)])]);
    objects.insert(1000, ("GPolyMesh", vec![crate::project_fixture::ginfo(0), V::Inline("FloatMesh", vec![points, facets])]));
    let b = [0.0, 0.0, 0.0, 2.0, 3.0, 4.0];
    objects.insert(2, root(500, vec![V::Own(3, "Geometry"), V::Own(1000, "GPolyMesh")], [0.0, 0.0, 0.0, 2.0, 3.0, 9.0], b));
    let shape = graphics::read(&tree_bytes(&objects), 0, &l).and_then(|t| crate::shapes::shape(&t, &l)).unwrap();
    assert_eq!((shape.solids, shape.triangles.len()), (1, 12));
}

/// The shapes of the symbols whose trees are `trees`, each read and then nested ([`crate::shapes::nest`]).
fn nested(trees: Vec<(u32, Objects)>) -> std::collections::BTreeMap<u32, Result<Shape, String>> {
    let s = schema();
    let l = Layouts::of(&s).unwrap();
    let mut shapes = trees
        .into_iter()
        .map(|(id, objects)| (id, graphics::read(&tree_bytes(&objects), 0, &l).and_then(|t| crate::shapes::shape(&t, &l))))
        .collect();
    crate::shapes::nest(&mut shapes);
    shapes
}

/// The volume a closed mesh holds, cubic millimetres: positive where its triangles turn counter-clockwise seen from outside.
fn volume(tris: &[[[f64; 3]; 3]]) -> f64 {
    let det = |[a, b, c]: &[[f64; 3]; 3]| {
        a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])
    };
    tris.iter().map(det).sum::<f64>() / 6.0
}

#[test]
fn a_symbol_that_nests_another_draws_the_nested_ones_shape_placed_by_its_transform() {
    let foot = 304.8f64.powi(3);
    let near = |a: [f64; 6], b: [f64; 6]| a.iter().zip(b).all(|(x, y)| (x - y * 304.8).abs() < 0.02);
    // A table (600) nesting its leg (500, a foot cube) turned a right angle at (10, 20, 3): from 9 to 10 east, 20 to 21 north.
    let turned = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let leg = [9.0, 20.0, 3.0, 10.0, 21.0, 4.0];
    let shapes = nested(vec![(600, instance_tree(600, 500, turned, [10.0, 20.0, 3.0], leg)), (500, box_tree(500, [1.0; 3], None))]);
    let s = shapes[&600].as_ref().unwrap();
    assert_eq!((s.triangles.len(), s.materials.len(), s.placed.len()), (12, 12, 0));
    assert!(near(extents(&s.triangles), leg), "{:?}", extents(&s.triangles));
    assert!((volume(&s.triangles) - foot).abs() < 1.0);
    // Mirrored (its x turned west), its faces still face out.
    let mirrored = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let shapes = nested(vec![
        (600, instance_tree(600, 500, mirrored, [1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0, 1.0, 1.0])),
        (500, box_tree(500, [1.0; 3], None)),
    ]);
    let s = shapes[&600].as_ref().unwrap();
    assert!((volume(&s.triangles) - foot).abs() < 1.0, "{}", volume(&s.triangles));
    // A solid of its own beside the nested one: both.
    let mut table = box_tree(600, [1.0; 3], None);
    table.insert(1000, instance_tree(600, 500, turned, [10.0, 20.0, 3.0], leg)[&3].clone());
    let all = [0.0, 0.0, 0.0, 10.0, 21.0, 4.0];
    table.insert(2, root(600, vec![V::Own(3, "Geometry"), V::Own(1000, "GInstance")], all, all));
    let shapes = nested(vec![(600, table.clone()), (500, box_tree(500, [1.0; 3], None))]);
    let s = shapes[&600].as_ref().unwrap();
    assert_eq!((s.triangles.len(), s.solids), (24, 2));
    assert!(near(extents(&s.triangles), all));
    // The box its graphics give round their solids reaching further than both: something is not read.
    table.insert(2, root(600, vec![V::Own(3, "Geometry"), V::Own(1000, "GInstance")], all, [0.0, 0.0, 0.0, 12.0, 21.0, 4.0]));
    let why = nested(vec![(600, table), (500, box_tree(500, [1.0; 3], None))])[&600].clone().unwrap_err();
    assert!(why.contains("not the box"), "{why}");
}

#[test]
fn a_symbol_nesting_one_that_is_not_read_or_that_nests_it_again_has_no_shape() {
    let square = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let cube = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let nests = |id: u32, symbol: u32| (id, instance_tree(id, symbol, square, [0.0; 3], cube));
    // The symbol it nests: none in the file.
    let why = nested(vec![nests(600, 700)])[&600].clone().unwrap_err();
    assert!(why.contains("no graphics"), "{why}");
    // One that nests it in turn.
    let shapes = nested(vec![nests(600, 601), nests(601, 600)]);
    let why = shapes[&600].clone().unwrap_err();
    assert!(why.contains("nests itself"), "{why}");
    // Too deep a chain: each nests the next, the last a cube.
    let mut trees: Vec<(u32, Objects)> = (0..12).map(|k| nests(600 + k, 601 + k)).collect();
    trees.push((612, box_tree(612, [1.0; 3], None)));
    let shapes = nested(trees);
    assert!(shapes[&611].as_ref().is_ok_and(|s| s.triangles.len() == 12));
    let why = shapes[&600].clone().unwrap_err();
    assert!(why.contains("deep"), "{why}");
    // One placing it where no model reaches: each number finite, its corners past the model's coordinates.
    let far = [1e300, 1e300, 1e300, 1e300, 1e300, 1e300];
    let shapes = nested(vec![(600, instance_tree(600, 500, square, [1e300; 3], far)), (500, box_tree(500, [1.0; 3], None))]);
    let why = shapes[&600].clone().unwrap_err();
    assert!(why.contains("usable"), "{why}");
}
