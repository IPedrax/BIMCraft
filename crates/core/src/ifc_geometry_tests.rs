//! Bodies IFC builds by turning, by sweeping along a curve and as advanced B-reps come in from IFC as meshes, each checked by
//! its bounds and its volume: a profile revolved whole and through a quarter turn, a profile swept along a bent line from a
//! fixed direction, part of that line by its parameters, a profile swept round an arc on a plane, a cylinder and a slab with a
//! square hole as advanced B-reps, a quarter disk whose arc is a rational B-spline. A B-rep with a B-spline surface is left out
//! with a note that says so, and so is a body past the faces an item may take. Written by hand.

use crate::import::{Imported, ifc};
use crate::model::{Element, Model};
use crate::view3d::{P3, cross, dot};

/// An IFC4 file's text in millimetres and radians, entity by entity: proxies in one storey, each a body of the items given.
struct G {
    text: String,
    next: u32,
    contained: Vec<u32>,
}

impl G {
    fn new() -> G {
        let mut g = G { text: String::new(), next: 9, contained: Vec::new() };
        g.text.push_str(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('0000000000000000000003',$,'Shapes',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('0000000000000000000007',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#8=IFCDIRECTION((0.,0.,1.));
",
        );
        g
    }

    fn add(&mut self, entity: &str) -> u32 {
        self.next += 1;
        self.text.push_str(&format!("#{}={entity};\n", self.next));
        self.next
    }

    fn point(&mut self, p: [f64; 3]) -> u32 {
        self.add(&format!("IFCCARTESIANPOINT(({:?},{:?},{:?}))", p[0], p[1], p[2]))
    }

    fn refs(ids: &[u32]) -> String {
        ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")
    }

    /// A proxy named `name` at the origin whose body is `items`.
    fn proxy(&mut self, name: &str, items: &[u32]) -> u32 {
        let rep = self.add(&format!("IFCSHAPEREPRESENTATION($,'Body','AdvancedSweptSolid',({}))", G::refs(items)));
        let shape = self.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"));
        let place = self.add("IFCLOCALPLACEMENT(#6,#5)");
        let id = self.next + 1;
        let made = self.add(&format!("IFCBUILDINGELEMENTPROXY('{id:0>22}',$,'{name}',$,$,#{place},#{shape},$,.ELEMENT.)"));
        self.contained.push(made);
        made
    }

    /// A planar advanced face bounded by `outer` and `holes`, its plane's normal `normal` (out of the solid), every edge a
    /// line of its own.
    fn plane_face(&mut self, outer: &[[f64; 3]], holes: &[&[[f64; 3]]], normal: [f64; 3]) -> u32 {
        let mut bounds = Vec::new();
        for (k, ring) in std::iter::once(outer).chain(holes.iter().copied()).enumerate() {
            let pts: Vec<u32> = ring.iter().map(|p| self.point(*p)).collect();
            let vertices: Vec<u32> = pts.iter().map(|p| self.add(&format!("IFCVERTEXPOINT(#{p})"))).collect();
            let mut edges = Vec::new();
            for i in 0..pts.len() {
                let j = (i + 1) % pts.len();
                let line = self.add(&format!("IFCPOLYLINE((#{},#{}))", pts[i], pts[j]));
                let edge = self.add(&format!("IFCEDGECURVE(#{},#{},#{line},.T.)", vertices[i], vertices[j]));
                edges.push(self.add(&format!("IFCORIENTEDEDGE(*,*,#{edge},.T.)")));
            }
            let lp = self.add(&format!("IFCEDGELOOP(({}))", G::refs(&edges)));
            let kind = if k == 0 { "IFCFACEOUTERBOUND" } else { "IFCFACEBOUND" };
            bounds.push(self.add(&format!("{kind}(#{lp},.T.)")));
        }
        let o = self.point(outer[0]);
        let n = self.add(&format!("IFCDIRECTION(({:?},{:?},{:?}))", normal[0], normal[1], normal[2]));
        let at = self.add(&format!("IFCAXIS2PLACEMENT3D(#{o},#{n},$)"));
        let plane = self.add(&format!("IFCPLANE(#{at})"));
        self.add(&format!("IFCADVANCEDFACE(({}),#{plane},.T.)", G::refs(&bounds)))
    }

    fn import(mut self) -> Imported {
        let g = format!("'{:0>22}'", self.next + 1);
        let of = G::refs(&self.contained);
        self.add(&format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({g},$,$,$,({of}),#7)"));
        self.text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        ifc(self.text.as_bytes(), "Shapes").unwrap_or_else(|e| panic!("{e}"))
    }
}

/// The triangles of the component of family `family`, in its own axes.
fn mesh(m: &Model, family: &str) -> Vec<[P3; 3]> {
    let c = m
        .elements
        .values()
        .find_map(|e| match e {
            Element::Component(c) if c.family == family => Some(c),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no component of {family}"));
    m.component_local_pieces(c).into_iter().flat_map(|p| p.tris).collect()
}

/// A closed mesh's volume: its triangles' signed tetrahedra, positive while they face out.
fn volume(tris: &[[P3; 3]]) -> f64 {
    tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum()
}

/// A mesh's least and greatest corner, to the tenth of a millimetre.
fn bounds(tris: &[[P3; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in tris.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let tenth = |v: [f64; 3]| v.map(|x| (x * 10.0).round() / 10.0 + 0.0);
    (tenth(lo), tenth(hi))
}

fn near(got: f64, want: f64, within: f64) -> bool {
    (got - want).abs() <= within * want.abs()
}

#[test]
fn a_profile_revolved_whole_and_through_a_quarter_turn_comes_in_as_its_solid() {
    let mut g = G::new();
    // A rectangle 200 x 500 from the axis out: x from 100 to 300, y from 0 to 500, turned about the y axis.
    let c = g.add("IFCCARTESIANPOINT((200.,250.))");
    let at = g.add(&format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
    let profile = g.add(&format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,#{at},200.,500.)"));
    let y = g.add("IFCDIRECTION((0.,1.,0.))");
    let axis = g.add(&format!("IFCAXIS1PLACEMENT(#4,#{y})"));
    let whole = g.add(&format!("IFCREVOLVEDAREASOLID(#{profile},$,#{axis},{:?})", 2.0 * std::f64::consts::PI));
    g.proxy("Ring", &[whole]);
    let quarter = g.add(&format!("IFCREVOLVEDAREASOLID(#{profile},$,#{axis},{:?})", std::f64::consts::FRAC_PI_2));
    g.proxy("Quarter", &[quarter]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let solid = std::f64::consts::PI * (300.0_f64.powi(2) - 100.0_f64.powi(2)) * 500.0;
    let ring = mesh(&i.model, "Ring");
    assert!(near(volume(&ring), solid, 0.01), "{} against {solid}", volume(&ring));
    assert_eq!(bounds(&ring), ([-300.0, 0.0, -300.0], [300.0, 500.0, 300.0]));
    // A quarter turn about y as IfcOpenShell turns it: from +x toward +z.
    let quarter = mesh(&i.model, "Quarter");
    assert!(near(volume(&quarter), solid / 4.0, 0.01), "{} against {}", volume(&quarter), solid / 4.0);
    assert_eq!(bounds(&quarter), ([0.0, 0.0, 0.0], [300.0, 500.0, 300.0]));
}

#[test]
fn a_profile_swept_along_a_bent_line_from_a_fixed_direction_is_mitred_at_the_bend() {
    let mut g = G::new();
    // 200 across the fixed direction (up), 100 square to it, along 2000 east and 1000 north.
    let profile = g.add("IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,100.)");
    let a = g.point([0.0, 0.0, 0.0]);
    let b = g.point([2000.0, 0.0, 0.0]);
    let c = g.point([2000.0, 1000.0, 0.0]);
    let line = g.add(&format!("IFCPOLYLINE((#{a},#{b},#{c}))"));
    let whole = g.add(&format!("IFCFIXEDREFERENCESWEPTAREASOLID(#{profile},$,#{line},$,$,#8)"));
    g.proxy("Rail", &[whole]);
    // From halfway along its first segment to the end of its second: each segment of a polyline is a parameter of 1.
    let part = g.add(&format!("IFCFIXEDREFERENCESWEPTAREASOLID(#{profile},$,#{line},0.5,2.,#8)"));
    g.proxy("Part", &[part]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let rail = mesh(&i.model, "Rail");
    assert!(near(volume(&rail), 200.0 * 100.0 * 3000.0, 1e-6), "{}", volume(&rail));
    assert_eq!(bounds(&rail), ([0.0, -50.0, -100.0], [2050.0, 1000.0, 100.0]));
    let part = mesh(&i.model, "Part");
    assert!(near(volume(&part), 200.0 * 100.0 * 2000.0, 1e-6), "{}", volume(&part));
    assert_eq!(bounds(&part), ([1000.0, -50.0, -100.0], [2050.0, 1000.0, 100.0]));
}

#[test]
fn a_profile_swept_round_an_arc_on_a_plane_keeps_its_x_along_the_planes_normal() {
    let mut g = G::new();
    // 100 along the plane's normal (up), 40 across: round half a circle of 1000 about the origin.
    let profile = g.add("IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,40.)");
    let circle = g.add("IFCCIRCLE(#5,1000.)");
    let arc = g.add(&format!(
        "IFCTRIMMEDCURVE(#{circle},(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE({:?})),.T.,.PARAMETER.)",
        std::f64::consts::PI
    ));
    let plane = g.add("IFCPLANE(#5)");
    let swept = g.add(&format!("IFCSURFACECURVESWEPTAREASOLID(#{profile},$,#{arc},$,$,#{plane})"));
    g.proxy("Arch", &[swept]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let arch = mesh(&i.model, "Arch");
    // As the importer draws an arc: in chords of 15 degrees, each ring square to the arc where it is.
    let solid = 100.0 * 40.0 * std::f64::consts::PI * 1000.0;
    assert!(near(volume(&arch), solid, 0.015), "{} against {solid}", volume(&arch));
    assert_eq!(bounds(&arch), ([-1020.0, 0.0, -50.0], [1020.0, 1020.0, 50.0]));
}

#[test]
fn a_cylinder_as_an_advanced_brep_comes_in_round() {
    let mut g = G::new();
    let text = "#60=IFCCARTESIANPOINT((500.,0.,0.));
#61=IFCCARTESIANPOINT((500.,0.,1000.));
#62=IFCVERTEXPOINT(#60);
#63=IFCVERTEXPOINT(#61);
#66=IFCCIRCLE(#5,500.);
#67=IFCCARTESIANPOINT((0.,0.,1000.));
#68=IFCAXIS2PLACEMENT3D(#67,$,$);
#69=IFCCIRCLE(#68,500.);
#70=IFCEDGECURVE(#62,#62,#66,.T.);
#71=IFCEDGECURVE(#63,#63,#69,.T.);
#72=IFCPOLYLINE((#60,#61));
#73=IFCEDGECURVE(#62,#63,#72,.T.);
#74=IFCORIENTEDEDGE(*,*,#70,.T.);
#75=IFCORIENTEDEDGE(*,*,#73,.T.);
#76=IFCORIENTEDEDGE(*,*,#71,.F.);
#77=IFCORIENTEDEDGE(*,*,#73,.F.);
#78=IFCEDGELOOP((#74,#75,#76,#77));
#79=IFCFACEOUTERBOUND(#78,.T.);
#80=IFCCYLINDRICALSURFACE(#5,500.);
#81=IFCADVANCEDFACE((#79),#80,.T.);
#82=IFCORIENTEDEDGE(*,*,#70,.F.);
#83=IFCEDGELOOP((#82));
#84=IFCFACEOUTERBOUND(#83,.T.);
#85=IFCPLANE(#5);
#86=IFCADVANCEDFACE((#84),#85,.F.);
#87=IFCORIENTEDEDGE(*,*,#71,.T.);
#88=IFCEDGELOOP((#87));
#89=IFCFACEOUTERBOUND(#88,.T.);
#90=IFCPLANE(#68);
#91=IFCADVANCEDFACE((#89),#90,.T.);
#92=IFCCLOSEDSHELL((#81,#86,#91));
#93=IFCADVANCEDBREP(#92);
";
    g.text.push_str(text);
    g.next = 100;
    g.proxy("Drum", &[93]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let drum = mesh(&i.model, "Drum");
    let solid = std::f64::consts::PI * 500.0 * 500.0 * 1000.0;
    assert!(near(volume(&drum), solid, 0.015), "{} against {solid}", volume(&drum));
    assert_eq!(bounds(&drum), ([-500.0, -500.0, 0.0], [500.0, 500.0, 1000.0]));
    // Round: corners all round its side, not a box's.
    let round = drum.iter().flatten().filter(|p| (p[0].hypot(p[1]) - 500.0).abs() < 1.0).count();
    assert!(round > 40, "{round}");
}

/// A slab 1000 square and 200 thick with a square hole 400 across through its middle, as an advanced B-rep of planar faces;
/// `top` replaces the surface of its top face where given.
fn holed(g: &mut G, top: Option<&str>) -> u32 {
    let ring = |z: f64, a: f64, b: f64| [[a, a, z], [b, a, z], [b, b, z], [a, b, z]];
    let (bottom_out, top_out) = (ring(0.0, 0.0, 1000.0), ring(200.0, 0.0, 1000.0));
    let (bottom_in, top_in) = (ring(0.0, 300.0, 700.0), ring(200.0, 300.0, 700.0));
    let mut faces = vec![g.plane_face(&bottom_out, &[&bottom_in], [0.0, 0.0, -1.0]), g.plane_face(&top_out, &[&top_in], [0.0, 0.0, 1.0])];
    let sides = |(lo, hi): (&[[f64; 3]; 4], &[[f64; 3]; 4]), i: usize| [lo[i], lo[(i + 1) % 4], hi[(i + 1) % 4], hi[i]];
    let normals = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    for (i, n) in normals.into_iter().enumerate() {
        faces.push(g.plane_face(&sides((&bottom_out, &top_out), i), &[], n));
        faces.push(g.plane_face(&sides((&bottom_in, &top_in), i), &[], n.map(|v: f64| -v + 0.0)));
    }
    if let Some(surface) = top {
        let face = faces[1];
        let at = g.text.find(&format!("#{face}=IFCADVANCEDFACE(")).unwrap_or_default();
        let line_end = g.text[at..].find('\n').map_or(g.text.len(), |e| at + e);
        let line = g.text[at..line_end].to_string();
        let plane = line.rsplit_once(",#").and_then(|(_, rest)| rest.split_once(',')).map(|(p, _)| p.to_string()).unwrap_or_default();
        let s = g.add(surface);
        g.text = g.text.replace(&line, &line.replace(&format!(",#{plane},"), &format!(",#{s},")));
    }
    let shell = g.add(&format!("IFCCLOSEDSHELL(({}))", G::refs(&faces)));
    g.add(&format!("IFCADVANCEDBREP(#{shell})"))
}

#[test]
fn a_planar_advanced_brep_keeps_the_holes_in_its_faces() {
    let mut g = G::new();
    let slab = holed(&mut g, None);
    g.proxy("Slab", &[slab]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let slab = mesh(&i.model, "Slab");
    assert!(near(volume(&slab), (1000.0 * 1000.0 - 400.0 * 400.0) * 200.0, 1e-6), "{}", volume(&slab));
    assert_eq!(bounds(&slab), ([0.0, 0.0, 0.0], [1000.0, 1000.0, 200.0]));
}

/// A B-spline surface of degree 1 through four corners, which a face can lie on.
const BSPLINE: &str =
    "IFCBSPLINESURFACEWITHKNOTS(1,1,((#4,#4),(#4,#4)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)";

#[test]
fn a_brep_with_a_bspline_surface_is_left_out_saying_so_and_a_body_with_more_keeps_the_rest() {
    let mut g = G::new();
    let curved = holed(&mut g, Some(BSPLINE));
    g.proxy("Curved", &[curved]);
    // A box beside it in the same body.
    let block = g.add("IFCRECTANGLEPROFILEDEF(.AREA.,$,$,300.,300.)");
    let solid = g.add(&format!("IFCEXTRUDEDAREASOLID(#{block},$,#8,300.)"));
    let curved_too = holed(&mut g, Some(BSPLINE));
    g.proxy("Mixed", &[solid, curved_too]);
    let i = g.import();
    assert!(!i.model.elements.values().any(|e| matches!(e, Element::Component(c) if c.family == "Curved")));
    let left: Vec<&String> = i.skipped.iter().filter(|s| s.contains("B-spline")).collect();
    assert_eq!(left.len(), 2, "{:?}", i.skipped);
    assert!(left.iter().any(|s| s.contains("IFCBUILDINGELEMENTPROXY") && s.contains("no body")), "{left:?}");
    assert!(left.iter().any(|s| s.contains("'Mixed'") && s.contains("part of its body")), "{left:?}");
    let mixed = mesh(&i.model, "Mixed");
    assert!(near(volume(&mixed), 300.0 * 300.0 * 300.0, 1e-6), "{}", volume(&mixed));
}

#[test]
fn a_quarter_disk_whose_arc_is_a_rational_bspline_comes_in_round() {
    let mut g = G::new();
    let text = "#60=IFCCARTESIANPOINT((1000.,0.));
#61=IFCCARTESIANPOINT((1000.,1000.));
#62=IFCCARTESIANPOINT((0.,1000.));
#63=IFCRATIONALBSPLINECURVEWITHKNOTS(2,(#60,#61,#62),.CIRCULARARC.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.,(1.,0.7071067811865476,1.));
#64=IFCCARTESIANPOINT((0.,0.));
#65=IFCPOLYLINE((#62,#64,#60));
#66=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#63);
#67=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#65);
#68=IFCCOMPOSITECURVE((#66,#67),.F.);
#69=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#68);
#70=IFCEXTRUDEDAREASOLID(#69,$,#8,100.);
";
    g.text.push_str(text);
    g.next = 100;
    g.proxy("Fan", &[70]);
    let i = g.import();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let fan = mesh(&i.model, "Fan");
    let solid = std::f64::consts::FRAC_PI_4 * 1000.0 * 1000.0 * 100.0;
    assert!(near(volume(&fan), solid, 0.005), "{} against {solid}", volume(&fan));
    assert_eq!(bounds(&fan), ([0.0, 0.0, 0.0], [1000.0, 1000.0, 100.0]));
    assert!(fan.iter().flatten().all(|p| p[0].hypot(p[1]) <= 1000.5), "a corner past the arc");
}

#[test]
fn a_body_past_the_faces_an_item_may_take_is_left_out_saying_so() {
    let mut g = G::new();
    // A profile of 4000 corners round a circle, turned whole: 32 sides for each of its edges.
    let n = 4000;
    let corners: Vec<u32> = (0..n)
        .map(|k| {
            let t = f64::from(k) / f64::from(n) * 2.0 * std::f64::consts::PI;
            g.add(&format!("IFCCARTESIANPOINT(({:?},{:?}))", 1000.0 + 100.0 * t.cos(), 100.0 * t.sin()))
        })
        .collect();
    let mut ring = corners.clone();
    ring.push(corners[0]);
    let line = g.add(&format!("IFCPOLYLINE(({}))", G::refs(&ring)));
    let profile = g.add(&format!("IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{line})"));
    let y = g.add("IFCDIRECTION((0.,1.,0.))");
    let axis = g.add(&format!("IFCAXIS1PLACEMENT(#4,#{y})"));
    let torus = g.add(&format!("IFCREVOLVEDAREASOLID(#{profile},$,#{axis},{:?})", 2.0 * std::f64::consts::PI));
    g.proxy("Torus", &[torus]);
    let i = g.import();
    assert!(i.model.elements.values().all(|e| !matches!(e, Element::Component(_))));
    assert!(i.skipped.iter().any(|s| s.contains("faces")), "{:?}", i.skipped);
}
