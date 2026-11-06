//! What a component looks like comes in from IFC as the materials its forms are made of: a surface style an item is styled
//! with straight, as IFC4 lets a styled item say (a named style as the material of its name, one with no name as a material
//! named for its colour), a style on an item inside a mapped representation, as Revit writes a type's geometry, and the faces
//! of a triangulated face set coloured by an indexed colour map, a material for each colour. Written by hand.

use crate::ifc_geometry_tests::{G, mesh, volume};
use crate::model::{Element, Model};

/// The materials of the forms of family `family`, in order, each with its colour and how see-through.
fn looks(m: &Model, family: &str) -> Vec<(String, [u8; 3], u8)> {
    let f = m.families.get(family).unwrap_or_else(|| panic!("no family {family}"));
    let mut out: Vec<(String, [u8; 3], u8)> = f
        .forms
        .iter()
        .map(|form| {
            let x = m.materials.get(&form.material).unwrap_or_else(|| panic!("no material {}", form.material));
            (form.material.clone(), x.color, x.transparency)
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A box `size` mm square from the origin, as an extruded rectangle.
fn cube(g: &mut G, size: f64) -> u32 {
    let c = g.add(&format!("IFCCARTESIANPOINT(({:?},{:?}))", size / 2.0, size / 2.0));
    let at = g.add(&format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
    let profile = g.add(&format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,#{at},{size:?},{size:?})"));
    g.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},$,#8,{size:?})"))
}

/// A surface style named `name` (`$` for none) shading in `rgb`, `clear` see-through, which `item` is styled with straight.
fn style(g: &mut G, item: u32, name: &str, rgb: [f64; 3], clear: &str) {
    let colour = g.add(&format!("IFCCOLOURRGB($,{:?},{:?},{:?})", rgb[0], rgb[1], rgb[2]));
    let shading = g.add(&format!("IFCSURFACESTYLESHADING(#{colour},{clear})"));
    let s = g.add(&format!("IFCSURFACESTYLE({name},.BOTH.,(#{shading}))"));
    g.add(&format!("IFCSTYLEDITEM(#{item},(#{s}),$)"));
}

#[test]
fn a_style_an_item_is_styled_with_straight_is_its_forms_material_named_or_by_colour() {
    let mut g = G::new();
    let red = cube(&mut g, 300.0);
    style(&mut g, red, "'Red Paint'", [1.0, 0.0, 0.0], "$");
    let blue = cube(&mut g, 200.0);
    style(&mut g, blue, "$", [0.0, 0.5, 1.0], "0.25");
    g.proxy("Painted", &[red, blue]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    assert_eq!(
        looks(&i.model, "Painted"),
        vec![("Colour 0080FF 25%".to_string(), [0, 128, 255], 25), ("Red Paint".to_string(), [255, 0, 0], 0)]
    );
}

#[test]
fn a_style_on_an_item_inside_a_mapped_representation_is_its_forms_material() {
    let mut g = G::new();
    // As Revit writes a type's geometry: the solid in a representation map is styled, and the product maps it.
    let inner = cube(&mut g, 400.0);
    style(&mut g, inner, "'Oak'", [0.6, 0.4, 0.2], "$");
    let rep = g.add(&format!("IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{inner}))"));
    let map = g.add(&format!("IFCREPRESENTATIONMAP(#5,#{rep})"));
    let op = g.add("IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#4,$,$)");
    let mapped = g.add(&format!("IFCMAPPEDITEM(#{map},#{op})"));
    g.proxy("Cabinet", &[mapped]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    assert_eq!(looks(&i.model, "Cabinet"), vec![("Oak".to_string(), [153, 102, 51], 0)]);
}

#[test]
fn a_face_set_coloured_by_an_indexed_colour_map_takes_a_material_for_each_colour() {
    let mut g = G::new();
    // A cube 1000 across in twelve triangles, its top two red, the rest grey, half see-through.
    let pts = "(0.,0.,0.),(1000.,0.,0.),(1000.,1000.,0.),(0.,1000.,0.),(0.,0.,1000.),(1000.,0.,1000.),(1000.,1000.,1000.),(0.,1000.,1000.)";
    let tris = "(1,3,2),(1,4,3),(5,6,7),(5,7,8),(1,2,6),(1,6,5),(2,3,7),(2,7,6),(3,4,8),(3,8,7),(4,1,5),(4,5,8)";
    let list = g.add(&format!("IFCCARTESIANPOINTLIST3D(({pts}))"));
    let set = g.add(&format!("IFCTRIANGULATEDFACESET(#{list},$,.T.,({tris}),$)"));
    let colours = g.add("IFCCOLOURRGBLIST(((1.,0.,0.),(0.5,0.5,0.5)))");
    g.add(&format!("IFCINDEXEDCOLOURMAP(#{set},0.5,#{colours},(2,2,1,1,2,2,2,2,2,2,2,2))"));
    g.proxy("Crate", &[set]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    assert_eq!(
        looks(&i.model, "Crate"),
        vec![("Colour 808080 50%".to_string(), [128, 128, 128], 50), ("Colour FF0000 50%".to_string(), [255, 0, 0], 50)]
    );
    let crate_ = mesh(&i.model, "Crate");
    assert!((volume(&crate_) - 1e9).abs() < 1.0, "{}", volume(&crate_));
    let red = i.model.families["Crate"].forms.iter().find(|f| f.material.starts_with("Colour FF0000")).map(|f| f.mesh.len());
    assert_eq!(red, Some(2));
}

#[test]
fn a_colour_map_past_its_colours_or_faces_leaves_those_faces_as_the_element_is() {
    let mut g = G::new();
    let pts = "(0.,0.,0.),(1000.,0.,0.),(1000.,1000.,0.),(0.,1000.,0.),(0.,0.,1000.),(1000.,0.,1000.),(1000.,1000.,1000.),(0.,1000.,1000.)";
    let tris = "(1,3,2),(1,4,3),(5,6,7),(5,7,8),(1,2,6),(1,6,5),(2,3,7),(2,7,6),(3,4,8),(3,8,7),(4,1,5),(4,5,8)";
    let list = g.add(&format!("IFCCARTESIANPOINTLIST3D(({pts}))"));
    let set = g.add(&format!("IFCTRIANGULATEDFACESET(#{list},$,.T.,({tris}),$)"));
    // Index 9 names no colour, and the list is short of the faces by two.
    let colours = g.add("IFCCOLOURRGBLIST(((0.,1.,0.)))");
    g.add(&format!("IFCINDEXEDCOLOURMAP(#{set},$,#{colours},(9,1,1,1,1,1,1,1,1,1))"));
    g.proxy("Odd", &[set]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let m = &i.model;
    let forms = &m.families["Odd"].forms;
    assert_eq!(forms.iter().map(|f| f.mesh.len()).sum::<usize>(), 12);
    let green = forms.iter().find(|f| f.material == "Colour 00FF00").map(|f| f.mesh.len());
    assert_eq!(green, Some(9));
    assert!(m.elements.values().any(|e| matches!(e, Element::Component(c) if c.family == "Odd")));
}
