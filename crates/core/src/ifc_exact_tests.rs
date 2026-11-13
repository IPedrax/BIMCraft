//! What a steel model's bolts come in as: a body an `IfcMappedItem` maps comes in in its map's axes, so copies of one shape
//! are one type; a straight extrusion in it comes in as the form it is (a profile on a plane, from and to), meshed only where
//! it is drawn. Written by hand, as Tekla writes a bolt: a fastener whose body maps a shape of the file's.

use crate::family::Expr;
use crate::family::sketch::FormCurve;
use crate::ifc_geometry_tests::{G, mesh, volume};
use crate::model::{Component, Element, Model};

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
const NORTH: [f64; 3] = [0.0, 1.0, 0.0];
const EAST: [f64; 3] = [1.0, 0.0, 0.0];

/// The components of family `family`.
fn copies<'a>(m: &'a Model, family: &str) -> Vec<&'a Component> {
    m.elements
        .values()
        .filter_map(|e| match e {
            Element::Component(c) if c.family == family => Some(c),
            _ => None,
        })
        .collect()
}

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

#[test]
fn copies_of_one_mapped_shape_moved_turned_or_a_few_hundredths_apart_are_one_type() {
    let mut g = G::new();
    let r = rod(&mut g, 8.0, 50.0);
    let map = shape(&mut g, &[r]);
    // Tekla writes copies of one shape as maps of their own, a few hundredths of a millimetre apart.
    let near = rod(&mut g, 8.03, 50.02);
    let other = shape(&mut g, &[near]);
    bolt(&mut g, "Bolt", map, [0.0, 0.0, 0.0], UP, EAST);
    bolt(&mut g, "Bolt", map, [5000.0, 3000.0, 0.0], UP, NORTH);
    bolt(&mut g, "Bolt", other, [9000.0, 0.0, 0.0], UP, EAST);
    let i = g.import();
    let m = &i.model;
    assert_eq!(m.families["Bolt"].types.len(), 1, "{:?}", m.families["Bolt"].types.keys());
    let placed = copies(m, "Bolt");
    assert_eq!(placed.len(), 3);
    // Each where the file has it, the one turned a quarter turn by its angle, none tilted.
    let mut angles: Vec<f64> = placed.iter().map(|c| c.angle).collect();
    angles.sort_by(f64::total_cmp);
    assert_eq!(angles, [0.0, 0.0, 90.0]);
    assert!(placed.iter().all(|c| c.tilt.is_none()));
    assert!(placed.iter().any(|c| (c.at.0, c.at.1) == (5000.0, 3000.0)));
}

#[test]
fn a_family_with_no_room_for_another_shape_goes_on_in_the_next_rather_than_leaving_it_out() {
    let mut g = G::new();
    // 260 bolts of as many sizes: one form each, four more than a family holds.
    for k in 0..260 {
        let r = rod(&mut g, 5.0 + 0.5 * f64::from(k), 50.0);
        let map = shape(&mut g, &[r]);
        bolt(&mut g, "Bolt", map, [f64::from(k) * 100.0, 0.0, 0.0], UP, EAST);
    }
    let i = g.import();
    let m = &i.model;
    assert!(!i.skipped.iter().any(|s| s.contains("more shapes than one family holds")), "{:?}", i.skipped);
    let most = crate::family::MAX_FORMS;
    assert_eq!(m.families["Bolt"].forms.len(), most);
    assert_eq!(m.families["Bolt (2)"].forms.len(), 260 - most);
    assert_eq!(copies(m, "Bolt").len() + copies(m, "Bolt (2)").len(), 260);
}

#[test]
fn a_bolt_whose_file_gives_its_size_comes_in_in_a_family_of_that_size() {
    let mut g = G::new();
    let r = rod(&mut g, 10.0, 60.0);
    let map = shape(&mut g, &[r]);
    let sized = bolt(&mut g, "Bolt assembly", map, [0.0, 0.0, 0.0], UP, EAST);
    bolt(&mut g, "Bolt assembly", map, [1000.0, 0.0, 0.0], UP, EAST);
    // As Tekla writes a bolt's properties: its name in its `Tekla Bolt` set. The second bolt has none.
    let name = g.add("IFCPROPERTYSINGLEVALUE('Bolt Name',$,IFCLABEL('BOLTM20*60'),$)");
    let set = g.add(&format!("IFCPROPERTYSET('{:0>22}',$,'Tekla Bolt',$,(#{name}))", g.next + 1));
    g.add(&format!("IFCRELDEFINESBYPROPERTIES('{:0>22}',$,$,$,(#{sized}),#{set})", g.next + 1));
    let i = g.import();
    let m = &i.model;
    assert_eq!(copies(m, "Bolt assembly BOLTM20*60").len(), 1, "{:?}", m.families.keys().collect::<Vec<_>>());
    assert_eq!(copies(m, "Bolt assembly").len(), 1, "the one of no size keeps the product's name");
}

#[test]
fn a_bolt_lying_down_is_the_same_type_tilted_and_keeps_its_tilt_through_a_file() {
    let mut g = G::new();
    let r = rod(&mut g, 8.0, 50.0);
    let map = shape(&mut g, &[r]);
    bolt(&mut g, "Bolt", map, [0.0, 0.0, 0.0], UP, EAST);
    // The same bolt lying along x, as through a brace's web: its z east, its x down.
    bolt(&mut g, "Bolt", map, [2000.0, 0.0, 500.0], EAST, [0.0, 0.0, -1.0]);
    let i = g.import();
    let m = &i.model;
    assert_eq!(m.families["Bolt"].types.len(), 1, "one shape, one type, however it lies");
    let placed = copies(m, "Bolt");
    let lying = placed.iter().find(|c| c.tilt.is_some()).expect("the lying one is tilted");
    assert!(placed.iter().any(|c| c.tilt.is_none()), "the upright one isn't");
    // Drawn where it lies: 50 mm along x from its insertion point, 16 across in y and in z.
    let tris: Vec<[[f64; 3]; 3]> = m.component_pieces(lying).into_iter().flat_map(|p| p.tris).collect();
    let span = |k: usize| {
        let (lo, hi) = tris.iter().flatten().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[k]), hi.max(p[k])));
        hi - lo
    };
    assert!((span(0) - 50.0).abs() < 1e-6 && span(1) <= 16.0 + 1e-6 && span(2) <= 16.0 + 1e-6, "{} {} {}", span(0), span(1), span(2));
    // Saved and read back, it keeps its tilt; an upright one writes none.
    let text = m.to_json().unwrap();
    assert_eq!(text.matches("\"tilt\"").count(), 1);
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.to_json().unwrap(), text);
}

#[test]
fn a_tilt_that_isnt_two_square_unit_directions_is_refused() {
    let mut g = G::new();
    let r = rod(&mut g, 8.0, 50.0);
    let map = shape(&mut g, &[r]);
    bolt(&mut g, "Bolt", map, [0.0, 0.0, 0.0], EAST, UP);
    let mut m = g.import().model;
    assert!(m.check().is_ok());
    for c in m.elements.values_mut() {
        if let Element::Component(c) = c {
            c.tilt = Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        }
    }
    let e = m.check().unwrap_err();
    assert!(e.contains("tilt"), "{e}");
}
