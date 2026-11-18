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
    // An instance's tree holds no solid of its own: what it draws is another's.
    let why = crate::shapes::shape(&tree, &l).unwrap_err();
    assert!(why.contains("GInstance"), "{why}");
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
