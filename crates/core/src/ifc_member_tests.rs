//! An `IfcMember` that frames (a brace, a purlin, a member of no type), part of no element but an assembly, and runs along a
//! line a beam can, comes in as a beam; a mullion, a post, a member a user's type names, one that stands, and one inside a
//! curtain wall stay generic components that remember their class. Written by hand.

use crate::ifc_geometry_tests::G;
use crate::model::Element;

/// A beam's type, the ends of its line in plan, and the heights of its ends over its level (the second where it differs).
type Line = (String, [f64; 2], [f64; 2], f64, Option<f64>);

/// A member of predefined type `kind` (`$` for none) and object type `object` named `name`: a section `w` wide and `d` deep
/// along the line from `a` to `b`, its profile square to it, as exporters write a member.
fn member(g: &mut G, name: &str, kind: &str, object: &str, a: [f64; 3], b: [f64; 3], (w, d): (f64, f64)) -> u32 {
    let along = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let length = (along[0].powi(2) + along[1].powi(2) + along[2].powi(2)).sqrt();
    let z = along.map(|v| v / length);
    // Across the run, level: its profile's x; up the run's side, its y.
    let flat = z[0].hypot(z[1]);
    let x = if flat < 1e-9 { [1.0, 0.0, 0.0] } else { [-z[1] / flat, z[0] / flat, 0.0] };
    let profile = g.add(&format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,$,{w:?},{d:?})"));
    let o = g.add(&format!("IFCCARTESIANPOINT(({:?},{:?},{:?}))", a[0], a[1], a[2]));
    let zd = g.add(&format!("IFCDIRECTION(({:?},{:?},{:?}))", z[0], z[1], z[2]));
    let xd = g.add(&format!("IFCDIRECTION(({:?},{:?},{:?}))", x[0], x[1], x[2]));
    let pos = g.add(&format!("IFCAXIS2PLACEMENT3D(#{o},#{zd},#{xd})"));
    let solid = g.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},#{pos},#8,{length:?})"));
    let rep = g.add(&format!("IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{solid}))"));
    let shape = g.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"));
    let place = g.add("IFCLOCALPLACEMENT(#6,#5)");
    let id = g.next + 1;
    let made = g.add(&format!("IFCMEMBER('{id:0>22}',$,'{name}',$,{object},#{place},#{shape},$,{kind})"));
    g.contained.push(made);
    made
}

#[test]
fn a_member_that_frames_along_a_line_comes_in_as_a_beam_and_any_other_stays_generic() {
    let mut g = G::new();
    // A brace of no predefined type of its own, its type object's a brace, as exporters write one.
    let brace = member(&mut g, "Brace:100 x 100:1", ".NOTDEFINED.", "$", [0.0, 0.0, 0.0], [3000.0, 0.0, 3000.0], (100.0, 100.0));
    let t = g.add(&format!("IFCMEMBERTYPE('{:0>22}',$,'Brace:100 x 100',$,$,$,$,$,$,.BRACE.)", g.next + 1));
    g.add(&format!("IFCRELDEFINESBYTYPE('{:0>22}',$,$,$,(#{brace}),#{t})", g.next + 1));
    member(&mut g, "Purlin", "$", "$", [0.0, 2000.0, 3000.0], [6000.0, 2000.0, 3000.0], (80.0, 160.0));
    member(&mut g, "Post", ".POST.", "$", [0.0, 4000.0, 0.0], [6000.0, 4000.0, 0.0], (100.0, 100.0));
    member(&mut g, "Fin", ".USERDEFINED.", "'Sun fin'", [0.0, 6000.0, 1000.0], [2000.0, 6000.0, 1000.0], (30.0, 400.0));
    member(&mut g, "Standing", ".NOTDEFINED.", "$", [5000.0, 5000.0, 0.0], [5000.0, 5000.0, 3000.0], (100.0, 100.0));
    // A mullion, and a horizontal member of no type inside a curtain wall.
    let mullion = member(&mut g, "Mullion", ".MULLION.", "$", [0.0, 8000.0, 0.0], [0.0, 8000.0, 2000.0], (50.0, 150.0));
    let transom = member(&mut g, "Transom", "$", "$", [0.0, 8000.0, 2000.0], [1500.0, 8000.0, 2000.0], (50.0, 150.0));
    g.contained.retain(|c| *c != mullion && *c != transom);
    let place = g.add("IFCLOCALPLACEMENT(#6,#5)");
    let wall = g.add(&format!("IFCCURTAINWALL('{:0>22}',$,'Curtain Wall',$,$,#{place},$,$,.NOTDEFINED.)", g.next + 1));
    g.contained.push(wall);
    g.add(&format!("IFCRELAGGREGATES('{:0>22}',$,$,$,#{wall},(#{mullion},#{transom}))", g.next + 1));
    let i = g.import();
    let m = &i.model;
    let beams: Vec<Line> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Beam(r) => Some((r.type_name.clone(), [r.start.0, r.start.1], [r.end.0, r.end.1], r.offset, r.end_offset)),
            _ => None,
        })
        .collect();
    assert_eq!(beams.len(), 2, "{beams:?} {:?}", i.skipped);
    let brace = beams.iter().find(|b| b.0 == "Brace:100 x 100").unwrap_or_else(|| panic!("{beams:?}"));
    assert_eq!((brace.1, brace.2, brace.3, brace.4), ([0.0, 0.0], [3000.0, 0.0], 0.0, Some(3000.0)));
    let t = &m.types.runs["beam"]["Brace:100 x 100"];
    assert_eq!((t.width, t.depth), (100.0, 100.0));
    let purlin = beams.iter().find(|b| b.0 != "Brace:100 x 100").unwrap_or_else(|| panic!("{beams:?}"));
    assert_eq!((purlin.1, purlin.2, purlin.3, purlin.4), ([0.0, 2000.0], [6000.0, 2000.0], 3000.0, None));
    let purlin_type = &m.types.runs["beam"][&purlin.0];
    assert_eq!((purlin_type.width, purlin_type.depth), (80.0, 160.0));
    // The rest are generic components that remember they were members.
    let generic: Vec<(String, Option<String>)> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Component(c) => {
                let f = m.families.get(&c.family)?;
                Some((c.family.clone(), f.ifc.as_ref().map(|k| k.class.clone())))
            }
            _ => None,
        })
        .collect();
    for name in ["Post", "Sun fin", "Standing", "Mullion", "Transom"] {
        assert!(generic.iter().any(|(f, k)| f == name && k.as_deref() == Some("IFCMEMBER")), "{name}: {generic:?}");
    }
}
