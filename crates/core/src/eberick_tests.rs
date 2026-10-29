//! A structural model as Eberick writes it, in centimetres, every product named by its mark (V1, P1, L1) with its property
//! sets: the short beams between columns, the landing on its own, the marks and the sets coming in. Written by hand; no one's
//! project is in the tests.

use crate::import::ifc;
use crate::model::{Element, ParameterKind};

/// The file's text, entity by entity.
struct F {
    text: String,
    next: u32,
    contained: Vec<u32>,
}

// Fixed entities: the directions every solid uses, and the storey the products stand in.
const Z: u32 = 10;
const X: u32 = 11;
const Y: u32 = 12;
const STOREY_AT: u32 = 6;

impl F {
    fn new() -> F {
        let mut f = F { text: String::new(), next: 12, contained: Vec::new() };
        f.text.push_str(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.CENTI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Eberick',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCDIRECTION((0.,0.,1.));
#11=IFCDIRECTION((1.,0.,0.));
#12=IFCDIRECTION((0.,1.,0.));
",
        );
        f
    }

    fn add(&mut self, entity: &str) -> u32 {
        self.next += 1;
        self.text.push_str(&format!("#{}={entity};\n", self.next));
        self.next
    }

    /// A product of `class` named `name` (of type `object_type`, `$` for none), its body `solid`, placed at `x`, `y`.
    fn product(&mut self, class: &str, name: &str, object_type: &str, (x, y): (f64, f64), solid: u32, last: &str) -> u32 {
        let rep = self.add(&format!("IFCSHAPEREPRESENTATION($,'Body','SolidModel',(#{solid}))"));
        let shape = self.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"));
        let p = self.add(&format!("IFCCARTESIANPOINT(({x:?},{y:?},0.))"));
        let a = self.add(&format!("IFCAXIS2PLACEMENT3D(#{p},$,$)"));
        let at = self.add(&format!("IFCLOCALPLACEMENT(#{STOREY_AT},#{a})"));
        let id = self.add(&format!("{class}('g{}',$,'{name}',$,{object_type},#{at},#{shape},$,{last})", self.next));
        self.contained.push(id);
        id
    }

    /// A beam as Eberick writes one: its profile `w` across by `h` deep, extruded `length` along the product's x, its middle at
    /// the product's origin.
    fn beam(&mut self, name: &str, object_type: &str, at: (f64, f64), (w, h): (f64, f64), length: f64) -> u32 {
        let profile = self.add(&format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,$,{w:?},{h:?})"));
        let pt = self.add(&format!("IFCCARTESIANPOINT(({:?},0.,{:?}))", -length / 2.0, -h / 2.0));
        let pos = self.add(&format!("IFCAXIS2PLACEMENT3D(#{pt},#{X},#{Y})"));
        let solid = self.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},#{pos},#{Z},{length:?})"));
        self.product("IFCBEAM", name, object_type, at, solid, "$")
    }

    /// A column 20 x 30 and 3 m tall standing at `x`, `y`.
    fn column(&mut self, name: &str, x: f64, y: f64) -> u32 {
        let profile = self.add("IFCRECTANGLEPROFILEDEF(.AREA.,$,$,20.,30.)");
        let solid = self.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},$,#{Z},300.)"));
        self.product("IFCCOLUMN", name, "$", (x, y), solid, "$")
    }

    /// A slab 1 m square and 15 cm thick, its top at the level, of `kind` (`.FLOOR.`, `.LANDING.`).
    fn slab(&mut self, name: &str, x: f64, y: f64, kind: &str) -> u32 {
        let pts: Vec<u32> = [(0., 0.), (100., 0.), (100., 100.), (0., 100.), (0., 0.)]
            .iter()
            .map(|(px, py)| self.add(&format!("IFCCARTESIANPOINT(({px:?},{py:?}))")))
            .collect();
        let line = self.add(&format!("IFCPOLYLINE(({}))", pts.iter().map(|p| format!("#{p}")).collect::<Vec<_>>().join(",")));
        let profile = self.add(&format!("IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{line})"));
        let pt = self.add("IFCCARTESIANPOINT((0.,0.,-15.))");
        let pos = self.add(&format!("IFCAXIS2PLACEMENT3D(#{pt},$,$)"));
        let solid = self.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},#{pos},#{Z},15.)"));
        self.product("IFCSLAB", name, "$", (x, y), solid, kind)
    }

    /// A property set `set` of `props` on `product`.
    fn props(&mut self, product: u32, set: &str, props: &[&str]) {
        let ids: Vec<String> = props.iter().map(|p| format!("#{}", self.add(p))).collect();
        let s = self.add(&format!("IFCPROPERTYSET('s{}',$,'{set}',$,({}))", self.next, ids.join(",")));
        self.add(&format!("IFCRELDEFINESBYPROPERTIES('r{}',$,$,$,(#{product}),#{s})", self.next));
    }

    fn finish(mut self) -> Vec<u8> {
        let all = self.contained.iter().map(|p| format!("#{p}")).collect::<Vec<_>>().join(",");
        self.add(&format!("IFCRELCONTAINEDINSPATIALSTRUCTURE('c',$,$,$,({all}),#7)"));
        self.text.push_str("ENDSEC;END-ISO-10303-21;");
        self.text.into_bytes()
    }
}

fn beams(m: &crate::model::Model) -> Vec<&crate::model::Run> {
    let mut v: Vec<&crate::model::Run> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Beam(r) => Some(r),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.start.0.total_cmp(&b.start.0));
    v
}

#[test]
fn a_beam_shorter_than_its_section_is_a_beam_that_runs_along_its_extrusion() {
    let mut f = F::new();
    f.beam("V1", "$", (0.0, 0.0), (14.0, 40.0), 300.0);
    // 33.81 cm long, 40 cm deep: as long as its section is tall, the body stands taller than it runs.
    f.beam("V10", "$", (500.0, 0.0), (14.0, 40.0), 33.81);
    // 10 cm long, 40 wide: the widest way across is not the way it runs.
    f.beam("V11", "$", (800.0, 0.0), (40.0, 14.0), 10.0);
    let i = ifc(&f.finish(), "x").unwrap();
    i.model.check().unwrap();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let beams = beams(&i.model);
    assert_eq!(beams.len(), 3);
    let at = |b: &crate::model::Run| (b.start.0, b.start.1, b.end.0, b.end.1);
    assert_eq!(at(beams[0]), (-1500.0, 0.0, 1500.0, 0.0));
    let (a, b, c, d) = at(beams[1]);
    assert!((a - 4830.95).abs() < 0.01 && (c - 5169.05).abs() < 0.01 && b == 0.0 && d == 0.0, "{:?}", at(beams[1]));
    assert_eq!(at(beams[2]), (7950.0, 0.0, 8050.0, 0.0));
    let section = |b: &crate::model::Run| {
        let t = &i.model.types.runs["beam"][&b.type_name];
        (t.width, t.depth)
    };
    assert_eq!((section(beams[1]), section(beams[2])), ((140.0, 400.0), (400.0, 140.0)));
}

#[test]
fn a_beam_extruded_straight_up_still_stands_and_is_left_out() {
    let mut f = F::new();
    let profile = f.add("IFCRECTANGLEPROFILEDEF(.AREA.,$,$,14.,40.)");
    let solid = f.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},$,#{Z},300.)"));
    f.product("IFCBEAM", "V9", "$", (0.0, 0.0), solid, "$");
    let i = ifc(&f.finish(), "x").unwrap();
    assert!(beams(&i.model).is_empty());
    assert!(i.skipped.iter().any(|s| s.contains("V9") && s.contains("one over the other")), "{:?}", i.skipped);
}

#[test]
fn a_landing_on_its_own_comes_in_as_a_floor() {
    let mut f = F::new();
    f.slab("L1", 0.0, 0.0, ".FLOOR.");
    f.slab("LE1", 300.0, 0.0, ".LANDING.");
    let i = ifc(&f.finish(), "x").unwrap();
    i.model.check().unwrap();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let floors = i.model.elements.values().filter(|e| e.category() == "floor").count();
    assert_eq!(floors, 2);
}

fn parameter(m: &crate::model::Model, name: &str) -> (ParameterKind, Vec<String>) {
    let p = m.parameters.get(name).unwrap_or_else(|| panic!("no parameter {name}: {:?}", m.parameters.keys().collect::<Vec<_>>()));
    (p.kind, p.categories.clone())
}

fn value(m: &crate::model::Model, cat: &str, name: &str) -> Vec<serde_json::Value> {
    let mut v: Vec<(String, serde_json::Value)> = m
        .elements
        .values()
        .filter(|e| e.category() == cat)
        .filter_map(|e| e.params().and_then(|p| p.get(name)).map(|x| (x.to_string(), x.clone())))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.into_iter().map(|(_, x)| x).collect()
}

#[test]
fn a_products_name_is_its_mark_unless_it_is_only_its_types() {
    let mut f = F::new();
    f.beam("V1", "$", (0.0, 0.0), (14.0, 40.0), 300.0);
    f.beam("V1", "$", (0.0, 400.0), (14.0, 40.0), 300.0);
    // BIMCraft names a product by its type, and says so in the object type too.
    f.beam("Concrete 14 x 40", "'Concrete 14 x 40'", (0.0, 800.0), (14.0, 40.0), 300.0);
    f.column("P1", 1000.0, 0.0);
    f.slab("L1", 2000.0, 0.0, ".FLOOR.");
    let i = ifc(&f.finish(), "x").unwrap();
    let m = &i.model;
    assert_eq!(parameter(m, "Mark"), (ParameterKind::Text, vec!["beam".to_string(), "column".to_string(), "floor".to_string()]));
    assert_eq!(value(m, "beam", "Mark"), ["V1", "V1"]);
    assert_eq!(value(m, "column", "Mark"), ["P1"]);
    assert_eq!(value(m, "floor", "Mark"), ["L1"]);
}

#[test]
fn the_property_sets_of_other_programs_come_in_as_parameters() {
    let mut f = F::new();
    let v1 = f.beam("V1", "$", (0.0, 0.0), (14.0, 40.0), 300.0);
    f.props(
        v1,
        "AltoQi_Eberick_Padr\\S\\co",
        &[
            "IFCPROPERTYSINGLEVALUE('Cobrimento',$,IFCLENGTHMEASURE(3.),$)",
            "IFCPROPERTYSINGLEVALUE('Classe de concreto',$,IFCTEXT('C-25'),$)",
            "IFCPROPERTYSINGLEVALUE('Cargas de parede (G2)',$,IFCREAL(0.5),$)",
            "IFCPROPERTYSINGLEVALUE('Se\\S\\g\\S\\co_bw',$,IFCLENGTHMEASURE(14.),$)",
        ],
    );
    f.props(
        v1,
        "Pset_BeamCommon",
        &["IFCPROPERTYSINGLEVALUE('Reference',$,IFCTEXT('Viga 14 X 40'),$)", "IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)"],
    );
    // A property set with nothing a parameter can hold, and one on something that has no parameters.
    f.props(v1, "Pset_Odd", &["IFCPROPERTYENUMERATEDVALUE('Status',$,(IFCLABEL('NEW')),$)"]);
    let i = ifc(&f.finish(), "x").unwrap();
    let m = &i.model;
    m.check().unwrap();
    assert_eq!(parameter(m, "Cobrimento"), (ParameterKind::Length, vec!["beam".to_string()]));
    // Centimetres in the file, millimetres in the model.
    assert_eq!(value(m, "beam", "Cobrimento"), [30.0]);
    assert_eq!(value(m, "beam", "Classe de concreto"), ["C-25"]);
    // A name a formula can't say loses what it can't: the brackets.
    assert_eq!(parameter(m, "Cargas de parede G2").0, ParameterKind::Number);
    assert_eq!(value(m, "beam", "Cargas de parede G2"), [0.5]);
    assert_eq!(value(m, "beam", "Seção_bw"), [140.0]);
    assert_eq!(value(m, "beam", "Reference"), ["Viga 14 X 40"]);
    // Read into a field already, or not a single value.
    assert!(!m.parameters.contains_key("LoadBearing") && !m.parameters.contains_key("Status"), "{:?}", m.parameters.keys());
}

#[test]
fn what_bimcraft_writes_in_its_own_sets_is_not_read_twice() {
    // The sample house, and what CI adds to it, in a project of its own (its lines name their hosts by id): between them every
    // kind of product and every set BIMCraft has.
    let mut house = crate::engine::Session::new();
    house.script(crate::sample::SAMPLE).unwrap();
    // A plain column and a stair, which the sample may not hold.
    house.script("column {\"at\": [12000, 0]}\nstair {\"start\": [20000, 0], \"end\": [23000, 0]}").unwrap();
    house.execute("parameter", &serde_json::json!({"name": "Fire", "kind": "text", "categories": ["wall"]})).unwrap();
    let wall = house.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    house.execute("modify", &serde_json::json!({"id": wall, "params": {"Fire": "EI60"}})).unwrap();
    let mut coverage = crate::engine::Session::new();
    coverage.script(include_str!("../../../ci/ifc-coverage.txt")).unwrap();
    for (s, parameters) in [(house, vec!["Fire"]), (coverage, Vec::new())] {
        let text = crate::ifc::export(&s.model);
        let i = ifc(text.as_bytes(), "x").unwrap();
        // Only the parameters the project had: no mark from a name that is a type's, no `LoadBearing` read twice.
        assert_eq!(i.model.parameters.keys().collect::<Vec<_>>(), parameters);
    }
}
