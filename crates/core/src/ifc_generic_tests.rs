//! What has no element of BIMCraft's comes in from IFC as a component of a generic family that remembers the class it came
//! from, and goes back out as that class: a covering that is no ceiling, a duct fitting, a curtain wall's panel and mullions
//! (the curtain wall itself left out, its parts in), terrain, and a member of a type its object type names. Written by hand.

use crate::engine::Session;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-generic-{}-{name}", std::process::id()))
}

/// An IFC4 file's text, entity by entity, in millimetres, its products in one storey.
struct F {
    text: String,
    next: u32,
    contained: Vec<u32>,
}

const STOREY: u32 = 7;
const STOREY_AT: u32 = 6;
const Z: u32 = 10;

impl F {
    fn new() -> F {
        let mut f = F { text: String::new(), next: 10, contained: Vec::new() };
        f.text.push_str(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('3vB2YO$MX4xv5uCqZZG05x',$,'Generic',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('2FCZDorxHDT8NI01kdXi8P',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCDIRECTION((0.,0.,1.));
",
        );
        f
    }

    fn add(&mut self, entity: &str) -> u32 {
        self.next += 1;
        self.text.push_str(&format!("#{}={entity};\n", self.next));
        self.next
    }

    /// A GlobalId of IFC's 22 digits, made from `n`.
    fn global(n: u32) -> String {
        format!("'{n:0>22}'")
    }

    /// A product of `class` named `name`, of object type `object_type` (`$` for none), at `at`: a box `size` big from its
    /// corner there, or no body for none; `tail` its attributes after its tag. In the storey, unless `contained` is false.
    fn product(&mut self, class: &str, name: &str, object_type: &str, at: [f64; 3], size: Option<[f64; 3]>, tail: &str) -> u32 {
        let shape = match size {
            Some([x, y, z]) => {
                let c = self.add(&format!("IFCCARTESIANPOINT(({:?},{:?}))", x / 2.0, y / 2.0));
                let p = self.add(&format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
                let profile = self.add(&format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,#{p},{x:?},{y:?})"));
                let solid = self.add(&format!("IFCEXTRUDEDAREASOLID(#{profile},$,#{Z},{z:?})"));
                let rep = self.add(&format!("IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{solid}))"));
                format!("#{}", self.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))")))
            }
            None => "$".into(),
        };
        let p = self.add(&format!("IFCCARTESIANPOINT(({:?},{:?},{:?}))", at[0], at[1], at[2]));
        let a = self.add(&format!("IFCAXIS2PLACEMENT3D(#{p},$,$)"));
        let place = self.add(&format!("IFCLOCALPLACEMENT(#{STOREY_AT},#{a})"));
        let g = F::global(self.next + 1);
        self.add(&format!("{class}({g},$,'{name}',$,{object_type},#{place},{shape},$,{tail})"))
    }

    fn contain(&mut self, id: u32) -> u32 {
        self.contained.push(id);
        id
    }

    /// A type object of `class` named `name` of predefined type `kind`, and the products of it.
    fn typed(&mut self, class: &str, name: &str, kind: &str, products: &[u32]) {
        let t = self.add(&format!("{class}({},$,'{name}',$,$,$,$,$,$,{kind})", F::global(self.next + 1)));
        let of: Vec<String> = products.iter().map(|p| format!("#{p}")).collect();
        self.add(&format!("IFCRELDEFINESBYTYPE({},$,$,$,({}),#{t})", F::global(self.next + 1), of.join(",")));
    }

    fn aggregate(&mut self, whole: u32, parts: &[u32]) {
        let of: Vec<String> = parts.iter().map(|p| format!("#{p}")).collect();
        self.add(&format!("IFCRELAGGREGATES({},$,$,$,#{whole},({}))", F::global(self.next + 1), of.join(",")));
    }

    fn finish(mut self) -> String {
        let of: Vec<String> = self.contained.iter().map(|p| format!("#{p}")).collect();
        let g = F::global(self.next + 1);
        self.add(&format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({g},$,$,$,({}),#{STOREY})", of.join(",")));
        self.text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        self.text
    }
}

/// A covering of stone cladding, a straight duct with an elbow at its end, a curtain wall of a glazed panel between two
/// mullions, terrain under it all, a sun fin of a member type its object type names, and a floor covering with no body.
fn building() -> String {
    let mut f = F::new();
    let cladding = f.product("IFCCOVERING", "Cladding:Stone:1", "$", [0.0, -100.0, 0.0], Some([2000.0, 20.0, 2700.0]), ".CLADDING.");
    f.contain(cladding);
    f.typed("IFCCOVERINGTYPE", "Cladding:Stone", ".CLADDING.", &[cladding]);
    let duct = f.product("IFCDUCTSEGMENT", "Duct", "$", [0.0, 1000.0, 2500.0], Some([3000.0, 300.0, 200.0]), ".RIGIDSEGMENT.");
    f.contain(duct);
    let elbow = f.product("IFCDUCTFITTING", "Elbow:90", "$", [3000.0, 1000.0, 2500.0], Some([300.0, 300.0, 200.0]), ".BEND.");
    f.contain(elbow);
    let wall = f.product("IFCCURTAINWALL", "Curtain Wall:Storefront", "$", [0.0, 3000.0, 0.0], None, ".NOTDEFINED.");
    f.contain(wall);
    let panel = f.product("IFCPLATE", "System Panel:Glazed:1", "$", [25.0, 3000.0, 0.0], Some([1000.0, 20.0, 2000.0]), ".CURTAIN_PANEL.");
    let left = f.product("IFCMEMBER", "Mullion:50 x 150:1", "$", [-25.0, 2935.0, 0.0], Some([50.0, 150.0, 2000.0]), ".MULLION.");
    let right = f.product("IFCMEMBER", "Mullion:50 x 150:2", "$", [1025.0, 2935.0, 0.0], Some([50.0, 150.0, 2000.0]), ".MULLION.");
    f.aggregate(wall, &[panel, left, right]);
    f.typed("IFCPLATETYPE", "System Panel:Glazed", ".CURTAIN_PANEL.", &[panel]);
    f.typed("IFCMEMBERTYPE", "Mullion:50 x 150", ".MULLION.", &[left, right]);
    let terrain =
        f.product("IFCGEOGRAPHICELEMENT", "Terrain", "$", [-5000.0, -5000.0, -500.0], Some([15000.0, 15000.0, 500.0]), ".TERRAIN.");
    f.contain(terrain);
    let fin = f.product("IFCMEMBER", "Fin", "'Sun fin'", [2100.0, -400.0, 0.0], Some([30.0, 400.0, 2700.0]), ".USERDEFINED.");
    f.contain(fin);
    let bare = f.product("IFCCOVERING", "Bare", "$", [0.0, 0.0, 0.0], None, ".FLOORING.");
    f.contain(bare);
    f.finish()
}

/// Each component's family: what it remembers of IFC, by the component's family and type.
fn generics(s: &mut Session) -> BTreeMap<String, Value> {
    let components = run(s, "elements", json!({"category": "component"}));
    let mut out = BTreeMap::new();
    for e in components["elements"].as_array().into_iter().flatten() {
        let family = e["family"].as_str().unwrap_or_default().to_string();
        let f = run(s, "family", json!({"name": family}));
        assert_eq!(f["family"]["category"], "generic", "{f}");
        let key = format!("{family}:{}", e["type"].as_str().unwrap_or_default());
        out.entry(key).or_insert_with(|| f["family"]["ifc"].clone());
    }
    out
}

/// The entities of a STEP file by their number: their class, and their attributes' text.
fn entities(step: &str) -> BTreeMap<u64, (String, String)> {
    step.lines()
        .filter_map(|l| {
            let (id, rest) = l.strip_prefix('#')?.split_once('=')?;
            let (class, args) = rest.split_once('(')?;
            Some((id.parse().ok()?, (class.to_string(), args.trim_end_matches(';').trim_end_matches(')').to_string())))
        })
        .collect()
}

/// The `#n` references in `text`.
fn references(text: &str) -> Vec<u64> {
    text.split('#').skip(1).filter_map(|r| r.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()).collect()
}

#[test]
fn what_has_no_element_comes_in_as_a_generic_component_of_its_class_and_goes_back_out_as_it() {
    let path = temp("in.ifc");
    std::fs::write(&path, building()).unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    // The duct is a duct, as ever; the rest of what has a body comes in as generic components, and the reply counts them.
    assert_eq!(r["elements"]["duct"], 1, "{r}");
    assert_eq!(r["elements"]["component"], 7, "{r}");
    assert_eq!(r["generic"], 7, "{r}");
    let classes = |c: &str, p: &str| json!({"class": c, "predefined": p});
    let want = BTreeMap::from([
        ("Cladding:Stone".to_string(), classes("IFCCOVERING", "CLADDING")),
        ("Elbow:90".to_string(), classes("IFCDUCTFITTING", "BEND")),
        ("System Panel:Glazed".to_string(), classes("IFCPLATE", "CURTAIN_PANEL")),
        ("Mullion:50 x 150".to_string(), classes("IFCMEMBER", "MULLION")),
        ("Terrain:Terrain".to_string(), classes("IFCGEOGRAPHICELEMENT", "TERRAIN")),
        // A product of its own type, as its object type says, remembers it.
        ("Sun fin:Sun fin".to_string(), json!({"class": "IFCMEMBER", "predefined": "USERDEFINED", "object_type": "Sun fin"})),
    ]);
    assert_eq!(generics(&mut s), want);
    // Left out: the curtain wall, whose parts came in, and the covering with no body; nothing else.
    let skipped: Vec<String> = r["skipped"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert!(skipped.iter().any(|l| l.starts_with("1 IFCCURTAINWALL: ") && l.contains("parts came in")), "{skipped:?}");
    assert!(skipped.iter().any(|l| l.starts_with("1 IFCCOVERING: ") && l.contains("no body")), "{skipped:?}");

    // Saved, each is its class again, of its predefined type, typed by a type object of its class's.
    let out = temp("out.ifc");
    run(&mut s, "save", json!({"path": out}));
    let step = std::fs::read_to_string(&out).unwrap();
    let all = entities(&step);
    let of = |class: &str| all.iter().filter(|(_, (c, _))| c == class).map(|(id, (_, a))| (*id, a.clone())).collect::<Vec<_>>();
    let typed_by = |id: u64| {
        all.values()
            .filter(|(c, a)| {
                c == "IFCRELDEFINESBYTYPE" && references(a.split_once(",(").map_or("", |x| x.1).split(')').next().unwrap()).contains(&id)
            })
            .filter_map(|(_, a)| references(a).last().and_then(|t| all.get(t)).map(|(c, _)| c.clone()))
            .collect::<Vec<_>>()
    };
    let contained: Vec<u64> = all
        .values()
        .filter(|(c, _)| c == "IFCRELCONTAINEDINSPATIALSTRUCTURE")
        .flat_map(|(_, a)| references(a.split_once(",(").map_or("", |x| x.1).split(')').next().unwrap()))
        .collect();
    for (class, predefined, n) in [
        ("IFCCOVERING", ".CLADDING.", 1),
        ("IFCDUCTFITTING", ".BEND.", 1),
        ("IFCPLATE", ".CURTAIN_PANEL.", 1),
        ("IFCMEMBER", ".MULLION.", 2),
        ("IFCMEMBER", ".USERDEFINED.", 1),
        ("IFCGEOGRAPHICELEMENT", ".TERRAIN.", 1),
    ] {
        let found: Vec<(u64, String)> = of(class).into_iter().filter(|(_, a)| a.ends_with(predefined)).collect();
        assert_eq!(found.len(), n, "{class} {predefined}: {step}");
        for (id, _) in found {
            assert_eq!(typed_by(id), [format!("{class}TYPE")], "#{id} {class}");
            assert!(contained.contains(&id), "#{id} {class} is in the storey");
        }
    }
    // The type objects carry the predefined type, but the one a product of its own makes, which IFC asks to say what it is.
    let member_types: Vec<String> = of("IFCMEMBERTYPE").into_iter().map(|(_, a)| a.rsplit(',').next().unwrap().to_string()).collect();
    assert_eq!(member_types.iter().filter(|k| *k == ".MULLION.").count(), 1, "{member_types:?}");
    assert!(of("IFCMEMBER").iter().any(|(_, a)| a.contains("'Sun fin'")), "the object type goes back out");
    assert_eq!(of("IFCDUCTSEGMENT").len(), 1);
    assert_eq!(of("IFCCURTAINWALL").len(), 0, "the curtain wall was left out; its parts are what stand");

    // Open again: the same elements, the same families remembering the same.
    let again = run(&mut s, "open", json!({"path": out}));
    let _ = std::fs::remove_file(&out);
    assert_eq!(again["elements"], r["elements"], "{again}");
    assert_eq!(again["generic"], 7, "{again}");
    assert_eq!(generics(&mut s), want);
}

#[test]
fn a_part_of_what_came_in_stays_its_and_an_assembly_with_a_body_of_its_own_comes_in_when_its_parts_have_none() {
    let mut f = F::new();
    // A beam (a run) made of a part with a body: the part is the beam's, not a component beside it.
    let beam = f.product("IFCBEAM", "B1", "$", [0.0, 0.0, 3000.0], Some([4000.0, 200.0, 400.0]), ".BEAM.");
    f.contain(beam);
    let part = f.product("IFCBUILDINGELEMENTPART", "Haunch", "$", [0.0, 0.0, 2900.0], Some([300.0, 200.0, 100.0]), ".NOTDEFINED.");
    f.aggregate(beam, &[part]);
    // An assembly whose parts have no body, but which has one: it comes in itself.
    let truss = f.product("IFCELEMENTASSEMBLY", "Truss", "$", [0.0, 5000.0, 3000.0], Some([4000.0, 100.0, 800.0]), "$,.TRUSS.");
    f.contain(truss);
    let chord = f.product("IFCMEMBER", "Chord", "$", [0.0, 5000.0, 3000.0], None, ".CHORD.");
    f.aggregate(truss, &[chord]);
    let path = temp("parts.ifc");
    std::fs::write(&path, f.finish()).unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["elements"]["beam"], 1, "{r}");
    assert_eq!(r["generic"], 1, "{r}");
    assert_eq!(
        generics(&mut s),
        BTreeMap::from([("Truss:Truss".to_string(), json!({"class": "IFCELEMENTASSEMBLY", "predefined": "TRUSS"}))])
    );
    // Its assembly place comes ahead of its predefined type, as IFC4 has it.
    let out = temp("parts-out.ifc");
    run(&mut s, "save", json!({"path": out}));
    let step = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(step.lines().any(|l| l.contains("=IFCELEMENTASSEMBLY(") && l.ends_with(",$,.TRUSS.);")), "{step}");
    assert!(step.lines().any(|l| l.contains("=IFCELEMENTASSEMBLYTYPE(") && l.ends_with(",.TRUSS.);")), "{step}");
}

#[test]
fn a_class_ifc4_lacks_or_a_type_it_doesnt_take_goes_out_as_a_proxy_or_not_defined() {
    let mut s = Session::new();
    // As a file of IFC4X3, or one a hand wrote, may leave a family: a kerb IFC4 has no class for, a plate of a type it lacks.
    let form = json!([{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}]);
    run(
        &mut s,
        "family",
        json!({"name": "Kerb", "category": "generic", "types": {"K": {}}, "forms": form,
        "ifc": {"class": "IFCKERB", "predefined": "NOTDEFINED"}}),
    );
    run(
        &mut s,
        "family",
        json!({"name": "Sheet", "category": "generic", "types": {"S": {}}, "forms": form,
        "ifc": {"class": "IFCPLATE", "predefined": "BASE_PLATE"}}),
    );
    run(&mut s, "component", json!({"family": "Kerb", "type": "K", "at": [0, 0]}));
    run(&mut s, "component", json!({"family": "Sheet", "type": "S", "at": [1000, 0]}));
    let out = temp("fallback.ifc");
    run(&mut s, "save", json!({"path": out}));
    let step = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    // The kerb is a proxy that says what it was; the plate a plate of no type IFC4 has.
    assert!(
        step.lines().any(|l| l.contains("=IFCBUILDINGELEMENTPROXY(") && l.contains("'IFCKERB'") && l.ends_with(".USERDEFINED.);")),
        "{step}"
    );
    assert!(step.lines().any(|l| l.contains("=IFCPLATE(") && l.ends_with(".NOTDEFINED.);")), "{step}");
    assert!(!step.contains("IFCKERB("), "{step}");
}

#[test]
fn a_family_remembers_only_an_ifc_class_and_types_it_can_write() {
    let mut s = Session::new();
    let form = json!([{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}]);
    for (ifc, says) in [
        (json!({"class": "IFCPLATE);#1=IFCWALL(", "predefined": "SHEET"}), "class"),
        (json!({"class": "plate"}), "class"),
        (json!({"class": "IFCPLATE", "predefined": "SHEET.)"}), "predefined"),
        (json!({"class": "IFCPLATE", "predefined": ""}), "predefined"),
        (json!({"class": format!("IFC{}", "A".repeat(200))}), "class"),
        (json!({"class": "IFCPLATE", "object_type": "x".repeat(10_000)}), "object_type"),
        (json!({"class": "IFCPLATE", "extra": 1}), "extra"),
    ] {
        let e = s
            .execute("family", &json!({"name": "P", "category": "generic", "types": {"T": {}}, "forms": form, "ifc": ifc}))
            .expect_err("refused");
        assert!(e.contains(says), "{ifc}: {e}");
    }
    assert!(s.model.families.get("P").is_none());
    // One it can write is kept, saved to a family's file and loaded back.
    let ifc = json!({"class": "IFCPLATE", "predefined": "SHEET", "object_type": "Steel sheet"});
    run(&mut s, "family", json!({"name": "P", "category": "generic", "types": {"T": {}}, "forms": form, "ifc": ifc}));
    let file = temp("plate.json");
    run(&mut s, "family", json!({"name": "P", "save": file}));
    let mut t = Session::new();
    run(&mut t, "family", json!({"path": file}));
    let _ = std::fs::remove_file(&file);
    assert_eq!(run(&mut t, "family", json!({"name": "P"}))["family"]["ifc"], ifc);
    // Written as IFC writes it, whatever its case.
    run(&mut t, "family", json!({"name": "P", "ifc": {"class": "IfcMember", "predefined": "mullion"}}));
    assert_eq!(run(&mut t, "family", json!({"name": "P"}))["family"]["ifc"], json!({"class": "IFCMEMBER", "predefined": "MULLION"}));
}
