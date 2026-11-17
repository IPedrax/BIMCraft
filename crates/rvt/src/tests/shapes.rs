//! A family type's graphics read as its shape (`graphics.rs`, `shapes.rs`), from trees the fixture writes in the layout Revit's
//! own schema gives: a box's solid, and an instance that draws it.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use crate::graphics::{self, Layouts};
use crate::project_fixture::{Objects, Project, V, box_tree, graphics_block, instance_tree, p3, root, tree_bytes};
use crate::schema::Schema;
use crate::shapes::{Curve, Shape, Surface, triangles};

fn schema() -> Schema {
    Schema::parse(&Project::schema_bytes())
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
    let (symbol, t) = graphics::instance(&tree, &l).unwrap();
    assert_eq!((symbol, t.axes, t.origin), (500, axes, [10.0, 20.0, 3.0]));
    // An instance's tree holds no solid of its own: what it draws is another's.
    let why = crate::shapes::shape(&tree, &l).unwrap_err();
    assert!(why.contains("GInstance"), "{why}");
    // A symbol's tree is no instance.
    let bytes = tree_bytes(&box_tree(500, [1.0, 1.0, 1.0], None));
    let why = graphics::instance(&graphics::read(&bytes, 0, &l).unwrap(), &l).unwrap_err();
    assert!(why.contains("not an instance"), "{why}");
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
