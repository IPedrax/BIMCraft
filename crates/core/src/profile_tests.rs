//! Steel sections for structural columns and beams, through the commands every front end drives: I sections from the
//! catalogue by name, parametric I, U, L, T and hollow sections; their cuts in plans and sections, their solids in 3D, their
//! schedules' weights, their IFC profiles and what comes back from IFC.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::geom::P2;
use crate::render::{self, Drawing, PAPER, Prim, Rgb, View};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn made(s: &mut Session, id: &str, p: Value) -> u64 {
    let r = run(s, id, p);
    r["id"].as_u64().or_else(|| r["ids"].as_array().and_then(|a| a.first()).and_then(Value::as_u64)).unwrap()
}

/// The colour of the PNG of drawing `d` at model point `p`.
fn pixel(d: &Drawing, p: P2) -> Rgb {
    let pm = tiny_skia::Pixmap::decode_png(&render::png(d).unwrap()).unwrap();
    let (x, y) = d.view.px(p);
    let c = pm.pixel(x as u32, y as u32).unwrap();
    Rgb(c.red(), c.green(), c.blue())
}

/// The IFC4 file of the project.
fn ifc(s: &mut Session) -> String {
    let path = std::env::temp_dir().join(format!("bimcraft-profiles-{}-{}.ifc", std::process::id(), s.model.uid));
    run(s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    text
}

fn entities<'a>(file: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("={name}(");
    file.lines().filter(|l| l.contains(&head)).collect()
}

#[test]
fn a_catalogue_section_is_a_type_by_its_name_and_a_beam_may_name_one_the_project_has_not_got() {
    let mut s = Session::new();
    let r = run(&mut s, "type", json!({"category": "structuralcolumn", "name": "Frame column", "profile": "IPE 550"}));
    assert_eq!(
        r["type"],
        json!({"width": 210.0, "depth": 550.0, "round": false,
               "profile": {"shape": "i", "name": "IPE 550", "web": 11.1, "flange": 17.2, "radius": 24.0}})
    );
    // A beam of a catalogue section the project has no type of: the type comes with it, in steel.
    let b = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "HEA 300"}));
    let t = &s.model.types.runs["beam"]["HEA 300"];
    assert_eq!((t.width, t.depth, t.material.as_str()), (300.0, 290.0, "Metal"));
    assert_eq!(element_json(&s.model, b)["profile"], "HEA 300");
    // A name neither the project nor the catalogue has is still refused.
    assert!(fails(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "IPE 999"})).contains("IPE 999"));
    assert!(fails(&mut s, "type", json!({"category": "beam", "name": "X", "profile": "IPE 999"})).contains("catalogue"));
    // A section is for a structural column's or a beam's type, not a footing's or a pipe's.
    assert!(fails(&mut s, "type", json!({"category": "footing", "name": "F", "profile": "IPE 300"})).contains("structural"));
    assert!(
        fails(&mut s, "type", json!({"category": "pipe", "name": "P", "shape": "CHS", "width": 100, "thickness": 5}))
            .contains("structural")
    );
}

#[test]
fn parametric_sections_are_made_with_their_plates_and_refused_when_they_dont_fit() {
    let mut s = Session::new();
    let u = run(
        &mut s,
        "type",
        json!({"category": "beam", "name": "UPE 200", "shape": "U", "width": 80, "depth": 200, "web": 6, "flange": 11, "radius": 13}),
    );
    assert_eq!(u["type"]["profile"], json!({"shape": "u", "web": 6.0, "flange": 11.0, "radius": 13.0}));
    let l = run(&mut s, "type", json!({"category": "beam", "name": "L 100x10", "shape": "L", "width": 100, "depth": 100, "thickness": 10}));
    assert_eq!(l["type"]["profile"], json!({"shape": "l", "thickness": 10.0}));
    let rhs = run(
        &mut s,
        "type",
        json!({"category": "structuralcolumn", "name": "RHS", "shape": "RHS", "width": 200, "depth": 100, "thickness": 8}),
    );
    assert_eq!(rhs["type"]["round"], false);
    let chs = run(&mut s, "type", json!({"category": "structuralcolumn", "name": "CHS", "shape": "CHS", "width": 168.3, "thickness": 6}));
    assert_eq!((chs["type"]["round"].as_bool(), chs["type"]["depth"].as_f64()), (Some(true), Some(168.3)));
    // Plates that don't fit, and what a shape doesn't take.
    let e = fails(
        &mut s,
        "type",
        json!({"category": "beam", "name": "Bad", "shape": "I", "width": 100, "depth": 200, "web": 120, "flange": 10}),
    );
    assert!(e.contains("web"), "{e}");
    let e = fails(&mut s, "type", json!({"category": "beam", "name": "Bad", "shape": "I", "width": 100, "depth": 200, "flange": 10}));
    assert!(e.contains("web"), "{e}");
    let e = fails(&mut s, "type", json!({"category": "beam", "name": "Bad", "shape": "RHS", "width": 100, "depth": 100, "thickness": 60}));
    assert!(e.contains("hole"), "{e}");
    let e = fails(
        &mut s,
        "type",
        json!({"category": "beam", "name": "Bad", "shape": "I", "width": 100, "depth": 200, "web": 5, "flange": 8, "thickness": 3}),
    );
    assert!(e.contains("thickness"), "{e}");
    assert!(fails(&mut s, "type", json!({"category": "beam", "name": "Bad", "shape": "Z", "width": 100, "depth": 200})).contains("shape"));
    // A plain rectangle again: `shape: rectangle` takes the section away.
    let plain = run(&mut s, "type", json!({"category": "beam", "name": "UPE 200", "shape": "rectangle"}));
    assert!(plain["type"].get("profile").is_none(), "{plain}");
}

#[test]
fn a_steel_column_is_cut_in_plan_as_its_section_and_a_tube_shows_its_hole() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "IPE 550", "profile": "IPE 550"}));
    run(
        &mut s,
        "type",
        json!({"category": "structuralcolumn", "name": "RHS 200x100x8", "shape": "RHS", "width": 200, "depth": 100, "thickness": 8}),
    );
    let c = made(&mut s, "structural_column", json!({"at": [0, 0], "type": "IPE 550"}));
    made(&mut s, "structural_column", json!({"at": [1000, 0], "type": "RHS 200x100x8"}));
    let d = render::draw(&s.model, 100000, View { scale: 1.0, cx: 500.0, cy: 0.0, width: 1400, height: 800 });
    // The flanges along east, 210 wide, 17.2 thick at the ends of its 550 depth; the web up the middle.
    assert_ne!(pixel(&d, P2(60.0, 266.0)), PAPER, "the north flange is cut");
    assert_ne!(pixel(&d, P2(-60.0, -266.0)), PAPER, "the south flange is cut");
    assert_ne!(pixel(&d, P2(0.0, 0.0)), PAPER, "the web is cut");
    // Beside the web, between the flanges: nothing, where a rectangle's poché used to be.
    assert_eq!(pixel(&d, P2(60.0, 0.0)), PAPER);
    assert_eq!(pixel(&d, P2(-60.0, 100.0)), PAPER);
    // Its outline is the I's: with its root fillets, round the box 210 by 550.
    let outline = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(c))
        .find_map(|(p, _)| match p {
            Prim::Fill { rings, .. } => rings.first().cloned(),
            _ => None,
        })
        .unwrap();
    assert!(outline.len() > 12, "{} corners", outline.len());
    // The tube: its walls cut, its hole empty.
    assert_ne!(pixel(&d, P2(1096.0, 0.0)), PAPER, "its east wall");
    assert_eq!(pixel(&d, P2(1000.0, 0.0)), PAPER, "its hole");
    assert_eq!(pixel(&d, P2(1050.0, 20.0)), PAPER, "its hole");
}

#[test]
fn a_section_cuts_a_steel_beam_as_its_i() {
    let mut s = Session::new();
    // Hung under 3000: its top flange's top at 3000, 300 deep.
    let beam = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "IPE 300", "offset": 3000}));
    let section = made(&mut s, "section", json!({"start": [3000, -2000], "end": [3000, 2000]}));
    let d = render::section(&s.model, section, View { scale: 1.0, cx: 2000.0, cy: 2850.0, width: 600, height: 600 });
    assert!(d.owners.contains(&Some(beam)));
    // Across the line at 2000 along it; the flanges 150 wide and 10.7 thick, the web 7.1.
    assert_ne!(pixel(&d, P2(2000.0, 2850.0)), PAPER, "the web");
    assert_ne!(pixel(&d, P2(2060.0, 2995.0)), PAPER, "the top flange");
    assert_ne!(pixel(&d, P2(1940.0, 2705.0)), PAPER, "the bottom flange");
    assert_eq!(pixel(&d, P2(2050.0, 2850.0)), PAPER, "beside the web");
    assert_eq!(pixel(&d, P2(1950.0, 2850.0)), PAPER, "beside the web");
}

#[test]
fn a_section_cuts_a_t_with_its_flange_on_top_and_one_turned_over_with_it_below() {
    let mut s = Session::new();
    let t = json!({"category": "beam", "name": "T", "shape": "T", "width": 1500, "depth": 1000, "web": 400, "flange": 500});
    run(&mut s, "type", t.clone());
    let mut turned = t;
    (turned["name"], turned["turned"]) = (json!("Inverted T"), json!(true));
    run(&mut s, "type", turned);
    // Both hung under 3000, 1000 deep; the section sees the T 1000 along it and the inverted T 3000 along.
    made(&mut s, "beam", json!({"points": [[0, -1000], [6000, -1000]], "type": "T", "offset": 3000}));
    made(&mut s, "beam", json!({"points": [[0, 1000], [6000, 1000]], "type": "Inverted T", "offset": 3000}));
    let section = made(&mut s, "section", json!({"start": [3000, -2000], "end": [3000, 2000]}));
    let d = render::section(&s.model, section, View { scale: 0.1, cx: 2000.0, cy: 2500.0, width: 600, height: 300 });
    assert_ne!(pixel(&d, P2(400.0, 2750.0)), PAPER, "the T's flange, on top");
    assert_ne!(pixel(&d, P2(1000.0, 2250.0)), PAPER, "the T's web");
    assert_eq!(pixel(&d, P2(400.0, 2250.0)), PAPER, "beside the T's web");
    assert_ne!(pixel(&d, P2(2400.0, 2250.0)), PAPER, "the inverted T's flange, below");
    assert_ne!(pixel(&d, P2(3000.0, 2750.0)), PAPER, "the inverted T's web");
    assert_eq!(pixel(&d, P2(2400.0, 2750.0)), PAPER, "beside the inverted T's web");
}

#[test]
fn a_sloped_steel_beam_in_3d_is_its_section_swept_along_it() {
    let mut s = Session::new();
    // 3000 along, 4000 up: 5000 long, sloping at cos θ = 0.6.
    let beam = made(
        &mut s,
        "beam",
        json!({"points": [[0, 0], [3000, 0]], "type": "IPE 300", "offset": 0, "end_offset": 4000, "justify": "centre"}),
    );
    let solids: Vec<_> = crate::view3d::solids(&s.model).into_iter().filter(|x| x.owner() == beam).collect();
    assert!(solids.len() > 3, "the flanges, the web and the fillets: {}", solids.len());
    // Cut upright across it at its middle, its section shows stretched up by 1 / cos θ: the I's area / 0.6.
    let (b, h, p) = crate::profiles::catalogue("IPE 300").unwrap();
    let area: f64 =
        solids.iter().flat_map(|x| x.cuts(P2(1500.0, -1000.0), P2(0.0, 1.0), 2000.0)).map(|r| crate::geom::area(&r).abs()).sum();
    assert!((area - p.area(b, h) / 0.6).abs() / area < 0.01, "{area} vs {}", p.area(b, h) / 0.6);
    // Its reach: half its depth square to the line, each way, upright: 150 / 0.6.
    let lo = solids.iter().map(|x| x.heights().0).fold(f64::MAX, f64::min);
    let hi = solids.iter().map(|x| x.heights().1).fold(f64::MIN, f64::max);
    assert!((lo + 250.0).abs() < 0.5 && (hi - 4250.0).abs() < 0.5, "{lo} {hi}");
}

#[test]
fn a_steel_columns_solid_in_3d_is_its_outline_and_a_tubes_has_a_hole() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "HEB 200", "profile": "HEB 200"}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "CHS", "shape": "CHS", "width": 200, "thickness": 10}));
    let i = made(&mut s, "structural_column", json!({"at": [0, 0], "type": "HEB 200"}));
    let tube = made(&mut s, "structural_column", json!({"at": [2000, 0], "type": "CHS"}));
    let solids = crate::view3d::solids(&s.model);
    let of = |id: u64| solids.iter().filter(|x| x.owner() == id).collect::<Vec<_>>();
    // A section along x through the middle: the I's flange-web-flange profile, as wide as its depth along y.
    let cut =
        |id: u64, at: P2| -> f64 { of(id).iter().flat_map(|x| x.cuts(at, P2(0.0, 1.0), 400.0)).map(|r| crate::geom::area(&r).abs()).sum() };
    // Through the web: 200 deep up its 3000 height.
    assert!((cut(i, P2(0.0, -200.0)) - 200.0 * 3000.0).abs() < 1.0);
    // Beside the web, only the two flanges.
    assert!((cut(i, P2(50.0, -200.0)) - 2.0 * 15.0 * 3000.0).abs() < 1.0, "{}", cut(i, P2(50.0, -200.0)));
    // Through the tube's middle: its two walls, 10 each.
    assert!((cut(tube, P2(2000.0, -200.0)) - 2.0 * 10.0 * 3000.0).abs() < 0.02 * 60000.0, "{}", cut(tube, P2(2000.0, -200.0)));
}

#[test]
fn a_schedule_of_steel_lists_each_sections_weight_per_metre_and_its_total_weight() {
    let mut s = Session::new();
    made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "IPE 300"}));
    made(&mut s, "beam", json!({"points": [[0, 3000], [4000, 3000]], "type": "IPE 300"}));
    made(&mut s, "beam", json!({"points": [[0, 6000], [5000, 6000]]}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "HEB 300", "profile": "HEB 300"}));
    made(&mut s, "structural_column", json!({"at": [0, 0], "type": "HEB 300"}));
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Steel", "category": "beam",
               "fields": ["Type", "Profile", "Section Area", "Weight per Metre", {"name": "Length", "total": true}, {"name": "Weight", "total": true}],
               "sort": ["Type"]}),
    );
    // IPE 300: 53.8 cm², 42.2 kg/m; 6 m and 4 m of it. The concrete beam has no section of steel.
    let rows = r["rows"].as_array().unwrap();
    let ipe: Vec<&Value> = rows.iter().filter(|row| row[0] == "IPE 300").collect();
    assert_eq!(ipe.len(), 2);
    assert_eq!((ipe[0][1].as_str(), ipe[0][2].as_f64(), ipe[0][3].as_f64()), (Some("IPE 300"), Some(53.81), Some(42.24)));
    let w: f64 = ipe.iter().map(|row| row[5].as_f64().unwrap()).sum();
    assert!((w - 422.4).abs() < 0.2, "{w}");
    let concrete = rows.iter().find(|row| row[0] == "Beam 200 x 500").unwrap();
    assert!(concrete[1].is_null(), "{concrete:?}");
    // A steel column's weight is its height's.
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Columns", "category": "structuralcolumn", "fields": ["Profile", "Weight per Metre", "Weight"]}),
    );
    let row = &r["rows"][0];
    assert_eq!(row[0], "HEB 300");
    assert!((row[2].as_f64().unwrap() - 3.0 * row[1].as_f64().unwrap()).abs() < 0.05, "{row}");
}

#[test]
fn steel_sections_go_out_as_ifc_profiles_with_a_material_profile_set() {
    let mut s = Session::new();
    made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "IPE 300", "offset": 3000}));
    made(&mut s, "beam", json!({"points": [[0, 2000], [6000, 2000]], "type": "IPE 300", "offset": 3000}));
    run(
        &mut s,
        "type",
        json!({"category": "beam", "name": "UPE 200", "shape": "U", "width": 80, "depth": 200, "web": 6, "flange": 11, "radius": 13}),
    );
    run(
        &mut s,
        "type",
        json!({"category": "beam", "name": "L 100", "shape": "L", "width": 100, "depth": 100, "thickness": 10, "radius": 12}),
    );
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "RHS", "shape": "RHS", "width": 200, "depth": 100, "thickness": 8}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "CHS", "shape": "CHS", "width": 168.3, "thickness": 6}));
    made(&mut s, "beam", json!({"points": [[0, 4000], [6000, 4000]], "type": "UPE 200", "offset": 3000}));
    made(&mut s, "beam", json!({"points": [[0, 5000], [6000, 5000]], "type": "L 100", "offset": 3000}));
    made(&mut s, "structural_column", json!({"at": [0, 0], "type": "RHS"}));
    made(&mut s, "structural_column", json!({"at": [6000, 0], "type": "CHS"}));
    let file = ifc(&mut s);
    // One I profile for the type, which both beams sweep: width, depth, web, flange, fillet radius.
    let i = entities(&file, "IFCISHAPEPROFILEDEF");
    assert_eq!(i.len(), 1, "{file}");
    assert!(i[0].contains("(.AREA.,'IPE 300',") && i[0].ends_with(",150.,300.,7.1,10.7,15.,$,$);"), "{}", i[0]);
    let profile = i[0].split('=').next().unwrap();
    assert_eq!(file.matches(&format!("IFCEXTRUDEDAREASOLID({profile},")).count(), 2);
    assert!(entities(&file, "IFCUSHAPEPROFILEDEF")[0].ends_with(",200.,80.,6.,11.,13.,$,$);"));
    assert!(entities(&file, "IFCLSHAPEPROFILEDEF")[0].ends_with(",100.,100.,10.,12.,$,$);"));
    assert!(entities(&file, "IFCRECTANGLEHOLLOWPROFILEDEF")[0].ends_with(",200.,100.,8.,$,$);"));
    assert!(entities(&file, "IFCCIRCLEHOLLOWPROFILEDEF")[0].ends_with(",84.15,6.);"));
    // None of them is a rectangle.
    assert!(entities(&file, "IFCRECTANGLEPROFILEDEF").is_empty(), "{file}");
    // Each type's material profile set, of its material and its profile, used by its members at their middle.
    let sets = entities(&file, "IFCMATERIALPROFILESET");
    assert_eq!(sets.len(), 5, "one for each type: {file}");
    let ipe_profile = entities(&file, "IFCMATERIALPROFILE").into_iter().find(|l| l.contains(&format!(",{profile},"))).unwrap();
    let steel = entities(&file, "IFCMATERIAL").into_iter().find(|l| l.contains("'Metal'")).unwrap().split('=').next().unwrap();
    assert!(ipe_profile.contains(&format!(",{steel},")), "{ipe_profile}");
    let usages = entities(&file, "IFCMATERIALPROFILESETUSAGE");
    assert_eq!(usages.len(), 5);
    assert_eq!(usages.iter().filter(|u| u.ends_with(",8,$);")).count(), 3, "top-justified beams: {usages:?}");
    assert_eq!(usages.iter().filter(|u| u.ends_with(",5,$);")).count(), 2, "columns: {usages:?}");
    let ipe_usage = usages
        .iter()
        .find(|u| sets.iter().any(|st| st.contains("'IPE 300'") && u.contains(&format!("({},", st.split('=').next().unwrap()))))
        .unwrap();
    let used = ipe_usage.split('=').next().unwrap();
    let rel = entities(&file, "IFCRELASSOCIATESMATERIAL").into_iter().find(|r| r.ends_with(&format!(",{used});"))).unwrap();
    assert_eq!(rel.matches('#').count(), 4, "itself, both beams and the usage: {rel}");
}

/// What a structural element is and where, and its section, whatever its id.
fn described(m: &crate::model::Model, category: &str) -> Vec<String> {
    let mut out: Vec<String> = m
        .elements
        .iter()
        .filter(|(_, e)| e.category() == category)
        .map(|(id, e)| {
            let j = element_json(m, *id);
            let profile = match category {
                "beam" => m.run_type(e).and_then(|t| t.profile.clone()),
                _ => m.column_type(e).and_then(|t| t.profile.clone()),
            };
            let angle = e.column().map(|c| format!("{:.1}", c.angle)).unwrap_or_default();
            format!("{} {} {} {:?} {}", j["width"], j["depth"], j["round"], profile, angle)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn steel_sections_come_back_from_ifc() {
    let mut s = Session::new();
    made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "IPE 300", "offset": 3000}));
    run(
        &mut s,
        "type",
        json!({"category": "beam", "name": "UPE 200", "shape": "U", "width": 80, "depth": 200, "web": 6, "flange": 11, "radius": 13}),
    );
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "HEB 300", "profile": "HEB 300"}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "IPE 550", "profile": "IPE 550"}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "RHS", "shape": "RHS", "width": 200, "depth": 100, "thickness": 8}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "CHS", "shape": "CHS", "width": 168.3, "thickness": 6}));
    made(&mut s, "beam", json!({"points": [[0, 4000], [6000, 4000]], "type": "UPE 200", "offset": 3000, "end_offset": 3500}));
    run(
        &mut s,
        "type",
        json!({"category": "beam", "name": "Inverted T", "shape": "T", "width": 1500, "depth": 1000, "web": 400, "flange": 500, "turned": true}),
    );
    made(&mut s, "beam", json!({"points": [[0, 8000], [6000, 8000]], "type": "Inverted T"}));
    // Square, its flanges along 30°; and one whose depth runs east.
    made(&mut s, "structural_column", json!({"at": [0, 0], "type": "HEB 300", "angle": 30}));
    made(&mut s, "structural_column", json!({"at": [3000, 0], "type": "IPE 550", "angle": 90}));
    made(&mut s, "structural_column", json!({"at": [6000, 0], "type": "RHS", "angle": 45}));
    made(&mut s, "structural_column", json!({"at": [9000, 0], "type": "CHS"}));
    let path = std::env::temp_dir().join(format!("bimcraft-profiles-{}-round.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]), "{r}");
    for category in ["beam", "structuralcolumn"] {
        assert_eq!(described(&t.model, category), described(&s.model, category), "{category}");
    }
    // The steel's material comes back on the beam's type.
    assert_eq!(t.model.types.runs["beam"]["IPE 300"].material, "Metal");
    assert!(t.model.check().is_ok());
}

#[test]
fn a_project_with_steel_sections_saves_and_opens_again() {
    let mut s = Session::new();
    made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "IPE 300"}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "CHS", "shape": "CHS", "width": 168.3, "thickness": 6}));
    let path = std::env::temp_dir().join(format!("bimcraft-profiles-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert!(r.get("left_out").is_none_or(|l| l.as_array().is_none_or(|a| a.is_empty())), "{r}");
    assert_eq!(t.model.types, s.model.types);
}

#[test]
fn hostile_sections_are_refused_without_a_panic_or_a_broken_model() {
    let mut s = Session::new();
    for p in [
        json!({"category": "beam", "name": "H", "profile": 5}),
        json!({"category": "beam", "name": "H", "profile": "x".repeat(10_000)}),
        json!({"category": "beam", "name": "H", "shape": "I", "width": 100, "depth": 200, "web": 1e300, "flange": 5}),
        json!({"category": "beam", "name": "H", "shape": "I", "width": 100, "depth": 200, "web": -5, "flange": 5}),
        json!({"category": "beam", "name": "H", "shape": "I", "width": 100, "depth": 200, "web": "thick", "flange": 5}),
        json!({"category": "beam", "name": "H", "shape": "CHS", "width": 100, "thickness": 0}),
        json!({"category": "beam", "name": "H", "shape": "L", "width": 100, "depth": 100, "thickness": 100}),
        json!({"category": "beam", "name": "H", "shape": ["I"]}),
        json!({"category": "beam", "name": "H", "shape": "T", "width": 100, "depth": 200, "web": 100, "flange": 10}),
        json!({"category": "beam", "name": "H", "shape": "T", "width": 100, "depth": 200, "web": 10, "flange": 200}),
        json!({"category": "beam", "name": "H", "shape": "T", "width": 100, "depth": 200, "web": 10, "flange": 10, "turned": "yes"}),
        json!({"category": "beam", "name": "H", "shape": "T", "width": 100, "depth": 200, "web": 10, "flange": 10, "thickness": 5}),
    ] {
        fails(&mut s, "type", p);
    }
    assert!(s.model.check().is_ok());
}

/// A foundation beam as a structural program writes an inverted T (#298): IFC's T, its flange on top, turned half round in
/// its plane by the profile's own axes, in metres. Written by hand; no one's project is in the tests.
const INVERTED_T: &str = "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Foundations',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#8=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#30),#7);
#10=IFCCARTESIANPOINT((0.,0.));
#11=IFCDIRECTION((-1.,0.));
#12=IFCAXIS2PLACEMENT2D(#10,#11);
#13=IFCTSHAPEPROFILEDEF(.AREA.,'T',#12,1.,1.5,0.4,0.5,$,$,$,$,$);
#14=IFCCARTESIANPOINT((0.,0.5,0.));
#15=IFCDIRECTION((0.,0.,-1.));
#16=IFCDIRECTION((1.,0.,0.));
#17=IFCAXIS2PLACEMENT3D(#14,#15,#16);
#18=IFCEXTRUDEDAREASOLID(#13,#17,#15,6.);
#19=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#18));
#20=IFCPRODUCTDEFINITIONSHAPE($,$,(#19));
#21=IFCCARTESIANPOINT((2.,0.,0.));
#22=IFCDIRECTION((0.,1.,0.));
#23=IFCAXIS2PLACEMENT3D(#21,#22,#16);
#24=IFCLOCALPLACEMENT($,#23);
#30=IFCBEAM('b',$,'731',$,$,#24,#20,$,$);
ENDSEC;END-ISO-10303-21;";

#[test]
fn an_inverted_t_foundation_beam_comes_from_ifc_as_a_t_turned_over() {
    let i = crate::import::ifc(INVERTED_T.as_bytes(), "x").unwrap();
    i.model.check().unwrap();
    assert!(i.skipped.is_empty(), "{:?}", i.skipped);
    let beams: Vec<_> = i.model.elements.iter().filter(|(_, e)| e.category() == "beam").collect();
    assert_eq!(beams.len(), 1);
    let t = beams.first().and_then(|(_, e)| i.model.run_type(e)).unwrap();
    let p = t.profile.clone().expect("a T section, not a rectangle");
    assert_eq!((p.shape, p.turned, t.width, t.depth), (crate::profiles::Shape::T, true, 1500.0, 1000.0));
    assert_eq!((p.web, p.flange), (400.0, 500.0));
}
