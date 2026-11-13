//! What a steel model's bolts come in as: a body an `IfcMappedItem` maps comes in in its map's axes, so copies of one shape
//! are one type; a straight extrusion in it comes in as the form it is (a profile on a plane, from and to), meshed only where
//! it is drawn. Written by hand, as Tekla writes a bolt: a fastener whose body maps a shape of the file's.

use crate::family::Expr;
use crate::family::sketch::FormCurve;
use crate::ifc_geometry_tests::{G, mesh, volume};

/// A shape of the file's, `items` (solids), behind an `IfcRepresentationMap` at the origin.
fn shape(g: &mut G, items: &[u32]) -> u32 {
    let list = items.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
    let rep = g.add(&format!("IFCSHAPEREPRESENTATION($,'Body','SweptSolid',({list}))"));
    g.add(&format!("IFCREPRESENTATIONMAP(#5,#{rep})"))
}

/// A cylinder `r` across and `depth` long, up the map's z.
fn rod(g: &mut G, r: f64, depth: f64) -> u32 {
    let profile = g.add(&format!("IFCCIRCLEPROFILEDEF(.AREA.,$,$,{r:?})"));
    g.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},#5,#8,{depth:?})"))
}

/// A hexagon `s` across its flats, `depth` deep, as a nut's outline: a closed polyline profile.
fn hexagon(g: &mut G, s: f64, depth: f64) -> u32 {
    let r = s / 3f64.sqrt();
    let pts: Vec<u32> = (0..=6)
        .map(|k| {
            let a = std::f64::consts::PI / 3.0 * f64::from(k % 6);
            g.add(&format!("IFCCARTESIANPOINT(({:?},{:?}))", r * a.cos(), r * a.sin()))
        })
        .collect();
    let list = pts.iter().map(|p| format!("#{p}")).collect::<Vec<_>>().join(",");
    let line = g.add(&format!("IFCPOLYLINE(({list}))"));
    let profile = g.add(&format!("IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{line})"));
    g.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},#5,#8,{depth:?})"))
}

/// A fastener named `name` whose body is map `map`, placed at `at` with its z along `z` and its x along `x`.
fn bolt(g: &mut G, name: &str, map: u32, at: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u32 {
    let op = g.add("IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#4,$,$)");
    let item = g.add(&format!("IFCMAPPEDITEM(#{map},#{op})"));
    let rep = g.add(&format!("IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#{item}))"));
    let shape = g.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"));
    let o = g.add(&format!("IFCCARTESIANPOINT(({:?},{:?},{:?}))", at[0], at[1], at[2]));
    let zd = g.add(&format!("IFCDIRECTION(({:?},{:?},{:?}))", z[0], z[1], z[2]));
    let xd = g.add(&format!("IFCDIRECTION(({:?},{:?},{:?}))", x[0], x[1], x[2]));
    let axes = g.add(&format!("IFCAXIS2PLACEMENT3D(#{o},#{zd},#{xd})"));
    let place = g.add(&format!("IFCLOCALPLACEMENT(#6,#{axes})"));
    let id = g.next + 1;
    let made = g.add(&format!("IFCMECHANICALFASTENER('{id:0>22}',$,'{name}',$,$,#{place},#{shape},$,$,$,$)"));
    g.contained.push(made);
    made
}

const UP: [f64; 3] = [0.0, 0.0, 1.0];
const EAST: [f64; 3] = [1.0, 0.0, 0.0];

#[test]
fn a_mapped_straight_extrusion_comes_in_as_the_form_it_is_and_draws_as_the_solid() {
    let mut g = G::new();
    let r = rod(&mut g, 8.0, 50.0);
    let map = shape(&mut g, &[r]);
    bolt(&mut g, "Bolt", map, [1000.0, 2000.0, 0.0], UP, EAST);
    let i = g.import();
    let m = &i.model;
    let f = &m.families["Bolt"];
    assert_eq!(f.forms.len(), 1, "{:?}", i.skipped);
    let form = &f.forms[0];
    // A circle 8 mm across on a plane, from and to 50 mm apart: no triangles of its own.
    assert!(form.mesh.is_empty());
    assert!(matches!(&form.sketch[..], [l] if matches!(l.curve, FormCurve::Circle { radius: Expr::Number(r), .. } if r == 8.0)));
    let (Expr::Number(from), Expr::Number(to)) = (&form.from, &form.to) else { panic!("{:?} {:?}", form.from, form.to) };
    assert_eq!(to - from, 50.0);
    // Drawn, it is the rod: its volume within what a polygon of the family's sides takes off a circle.
    let v = volume(&mesh(m, "Bolt"));
    let solid = std::f64::consts::PI * 64.0 * 50.0;
    assert!((v / solid - 1.0).abs() < 0.01, "{v} for {solid}");
}

#[test]
fn a_profile_of_lines_comes_in_as_its_lines_and_a_solid_of_another_kind_as_triangles() {
    let mut g = G::new();
    let nut = hexagon(&mut g, 30.0, 16.0);
    let map = shape(&mut g, &[nut]);
    bolt(&mut g, "Nut", map, [0.0, 0.0, 0.0], UP, EAST);
    let i = g.import();
    let m = &i.model;
    let form = &m.families["Nut"].forms[0];
    assert_eq!(form.sketch.len(), 6, "a line for each side");
    assert!(form.mesh.is_empty());
    // A hexagon 30 mm across its flats has an area of s²·√3/2, exactly, its sides being its sides.
    let v = volume(&mesh(m, "Nut"));
    let solid = 900.0 * 3f64.sqrt() / 2.0 * 16.0;
    assert!((v - solid).abs() < 1e-3 * solid, "{v} for {solid}");
}
