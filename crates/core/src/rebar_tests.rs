//! Reinforcing bars, through the commands every front end drives: a bar bent along its points, a set of them, the host it goes
//! with, the bar schedule and IFC.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn ids(r: &Value) -> Vec<u64> {
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// The id an element's `host` or `level` field (a GUID) names.
fn id_of(s: &Session, v: &Value) -> Option<u64> {
    serde_json::from_value::<crate::model::Guid>(v.clone()).ok().and_then(|g| s.model.elements.id(g))
}

/// A beam 200 x 500 and 6 m long with its top at the level, so its section spans z -500 to 0 and y -100 to 100.
fn beam(s: &mut Session) -> u64 {
    ids(&run(s, "beam", json!({"points": [[0, 0], [6000, 0]]})))[0]
}

#[test]
fn a_bar_follows_its_points_is_weighed_and_repeats_by_its_step() {
    let mut s = Session::new();
    let host = beam(&mut s);
    let r = run(
        &mut s,
        "rebar",
        json!({"points": [[50, -40, -450], [5950, -40, -450]], "diameter": 12.5, "host": host, "count": 3, "step": [0, 40, 0]}),
    );
    let bars = ids(&r);
    assert_eq!(bars.len(), 3);
    let b = element_json(&s.model, bars[2]);
    assert_eq!((b["category"].as_str(), b["grade"].as_str(), id_of(&s, &b["host"])), (Some("rebar"), Some("CA-50"), Some(host)));
    assert_eq!((b["diameter"].as_f64(), b["length"].as_f64(), b["weight_kg"].as_f64()), (Some(12.5), Some(5900.0), Some(5.684)));
    // Each copy is one `step` from the last.
    assert_eq!(b["path"], json!([[50.0, 40.0, -450.0], [5950.0, 40.0, -450.0]]));
    assert_eq!(element_json(&s.model, bars[0])["path"][0], json!([50.0, -40.0, -450.0]));

    // A hoop: `closed` joins the last point to the first, so its length is its perimeter.
    let hoop = json!({"points": [[100, -70, -30], [100, 70, -30], [100, 70, -470], [100, -70, -470]], "closed": true, "diameter": 8, "grade": "CA-60"});
    let hoop_id = ids(&run(&mut s, "rebar", hoop))[0];
    let h = element_json(&s.model, hoop_id);
    assert_eq!((h["length"].as_f64(), h["weight_kg"].as_f64(), h["grade"].as_str()), (Some(1160.0), Some(0.458), Some("CA-60")));
    assert_eq!(h["path"].as_array().map(Vec::len), Some(5));

    // The bars go with their host; the hoop, with none, stays.
    run(&mut s, "delete", json!({"id": host}));
    assert_eq!(run(&mut s, "elements", json!({"category": "rebar"}))["count"], 1);
}

#[test]
fn a_bar_that_cannot_be_one_is_refused_and_nothing_changes() {
    let mut s = Session::new();
    let host = beam(&mut s);
    let ok = json!({"points": [[0, 0, 0], [1000, 0, 0]], "diameter": 10});
    run(&mut s, "rebar", ok.clone());
    let with = |k: &str, v: Value| {
        let mut p = ok.clone();
        p[k] = v;
        p
    };
    for (p, why) in [
        (json!({"points": [[0, 0, 0]], "diameter": 10}), "at least 2"),
        (json!({"points": [[0, 0, 0], [0, 0, 0]], "diameter": 10}), "too short"),
        (json!({"points": [[0, 0], [1000, 0]], "diameter": 10}), "[x, y, z]"),
        (json!({"points": [[0, 0, 0], [1e300, 0, 0]], "diameter": 10}), "[x, y, z]"),
        (json!({"points": [[0, 0, 0], [1000, 0, 0]]}), "diameter"),
        (with("diameter", json!(0)), "diameter"),
        (with("diameter", json!(-5)), "diameter"),
        (with("diameter", json!(1000)), "diameter"),
        (with("count", json!(0)), "count"),
        (with("count", json!(1e9)), "count"),
        (with("count", json!(2.5)), "count"),
        (with("step", json!([1, 2])), "step"),
        (with("host", json!(999_999)), "no element"),
        (with("host", json!("x")), "host"),
        (with("grade", json!("")), "grade"),
        (with("level", json!("Nowhere")), "level"),
    ] {
        let e = fails(&mut s, "rebar", p.clone());
        assert!(e.contains(why), "{p}: {e}");
    }
    // A host must reinforce something: a level is no host.
    let level = s.model.levels()[0].0;
    assert!(fails(&mut s, "rebar", with("host", json!(level))).contains("host"));
    let _ = host;
}

#[test]
fn a_bar_schedule_counts_bars_by_grade_diameter_and_length_and_adds_their_weight() {
    let mut s = Session::new();
    let host = beam(&mut s);
    run(
        &mut s,
        "rebar",
        json!({"points": [[50, -40, -450], [5950, -40, -450]], "diameter": 12.5, "host": host, "count": 3, "step": [0, 40, 0]}),
    );
    run(
        &mut s,
        "rebar",
        json!({"points": [[100, -70, -30], [100, 70, -30], [100, 70, -470], [100, -70, -470]], "closed": true, "diameter": 8}),
    );
    let r = run(
        &mut s,
        "schedule",
        json!({
            "name": "Bar Schedule", "category": "rebar",
            "fields": ["Grade", "Diameter", "Length", {"name": "Count", "total": true}, {"name": "Weight", "total": true}],
            "sort": ["Diameter"], "grouped": true
        }),
    );
    assert_eq!(r["rows"], json!([["CA-50", 8.0, 1160.0, 1.0, 0.458], ["CA-50", 12.5, 5900.0, 3.0, 17.052]]));
    assert_eq!(r["totals"], json!([null, null, null, 4.0, 17.51]));
}

#[test]
fn a_bar_survives_ifc_as_a_reinforcing_bar_with_its_points_and_its_host() {
    let mut s = Session::new();
    let host = beam(&mut s);
    run(&mut s, "rebar", json!({"points": [[50, -40, -450], [5950, -40, -450]], "diameter": 12.5, "host": host}));
    run(
        &mut s,
        "rebar",
        json!({"points": [[100, -70, -30], [100, 70, -30], [100, 70, -470], [100, -70, -470]], "closed": true, "diameter": 8, "grade": "CA-60"}),
    );
    let file = crate::ifc::export(&s.model);
    assert_eq!(file.matches("IFCREINFORCINGBAR(").count(), 2, "{file}");
    assert!(file.contains("IFCSWEPTDISKSOLID(") && file.contains("IFCRELAGGREGATES("), "{file}");

    let back = crate::import::ifc(file.as_bytes(), "t").unwrap();
    assert_eq!(back.skipped, Vec::<String>::new());
    let mut bars: Vec<_> = back
        .model
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Rebar(rb) => Some((*id, rb.clone())),
            _ => None,
        })
        .collect();
    bars.sort_by(|a, b| a.1.diameter.total_cmp(&b.1.diameter).reverse());
    assert_eq!(bars.len(), 2);
    let (long, hoop) = (&bars[0].1, &bars[1].1);
    assert_eq!((long.diameter, long.grade.as_str()), (12.5, "CA-50"));
    assert_eq!((hoop.diameter, hoop.grade.as_str()), (8.0, "CA-60"));
    let near =
        |a: &[[f64; 3]], b: &[[f64; 3]]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (0..3).all(|i| (p[i] - q[i]).abs() < 1e-3));
    assert!(near(&long.path, &[[50.0, -40.0, -450.0], [5950.0, -40.0, -450.0]]), "{:?}", long.path);
    assert!(near(
        &hoop.path,
        &[[100.0, -70.0, -30.0], [100.0, 70.0, -30.0], [100.0, 70.0, -470.0], [100.0, -70.0, -470.0], [100.0, -70.0, -30.0]]
    ));
    // The long bar is aggregated to its beam, which it finds again; the hoop has none.
    let beams: Vec<_> = back.model.elements.iter().filter(|(_, e)| matches!(e, Element::Beam(_))).map(|(id, _)| *id).collect();
    assert_eq!(beams.len(), 1);
    assert_eq!(long.host, Some(back.model.guid(beams[0])));
    assert_eq!(hoop.host, None);
}

#[test]
fn the_3d_view_draws_a_bar_as_a_tube_along_each_straight_piece_of_its_path() {
    let mut s = Session::new();
    let hoop = [[100, -70, -30], [100, 70, -30], [100, 70, -470], [100, -70, -470]];
    let id = ids(&run(&mut s, "rebar", json!({"points": hoop, "closed": true, "diameter": 8})))[0];
    let solids = crate::view3d::solids(&s.model);
    let mine: Vec<_> = solids.iter().filter(|x| x.owner() == id).collect();
    assert_eq!(mine.len(), 4);
    assert!(mine.iter().all(|x| matches!(x, crate::view3d::Solid::Mesh { round: true, cut: true, .. })));
    // Its heights are over the level, and half a diameter more either side of its path.
    let (lo, hi) = mine.iter().map(|x| x.heights()).fold((f64::MAX, f64::MIN), |(l, h), (a, b)| (l.min(a), h.max(b)));
    assert!((lo - -474.0).abs() < 1e-6 && (hi - -26.0).abs() < 1e-6, "{lo} {hi}");
}

#[test]
fn a_bar_of_another_tool_comes_in_when_its_disks_make_one_line_and_is_left_out_when_they_do_not() {
    // Centimetres, a bar bent once from two disks end to end, one whose second disk starts elsewhere, one of two radii.
    let text = "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.CENTI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Bars',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCCARTESIANPOINT((0.,0.,0.));
#11=IFCCARTESIANPOINT((100.,0.,0.));
#12=IFCCARTESIANPOINT((100.,50.,0.));
#13=IFCCARTESIANPOINT((300.,50.,0.));
#14=IFCPOLYLINE((#10,#11));
#15=IFCPOLYLINE((#11,#12));
#16=IFCPOLYLINE((#13,#12));
#17=IFCSWEPTDISKSOLID(#14,0.6,$,$,$);
#18=IFCSWEPTDISKSOLID(#15,0.6,$,$,$);
#19=IFCSWEPTDISKSOLID(#16,0.6,$,$,$);
#20=IFCSWEPTDISKSOLID(#15,0.4,$,$,$);
#21=IFCSHAPEREPRESENTATION($,'Body','AdvancedSweptSolid',(#17,#18));
#22=IFCSHAPEREPRESENTATION($,'Body','AdvancedSweptSolid',(#17,#19));
#23=IFCSHAPEREPRESENTATION($,'Body','AdvancedSweptSolid',(#17,#20));
#24=IFCPRODUCTDEFINITIONSHAPE($,$,(#21));
#25=IFCPRODUCTDEFINITIONSHAPE($,$,(#22));
#26=IFCPRODUCTDEFINITIONSHAPE($,$,(#23));
#30=IFCREINFORCINGBAR('a',$,'Bent',$,$,#6,#24,$,'CA-60',12.,113.,150.,.NOTDEFINED.,.PLAIN.);
#31=IFCREINFORCINGBAR('b',$,'Apart',$,$,#6,#25,$,$,12.,113.,150.,.NOTDEFINED.,.PLAIN.);
#32=IFCREINFORCINGBAR('c',$,'Two radii',$,$,#6,#26,$,$,12.,113.,150.,.NOTDEFINED.,.PLAIN.);
#40=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#30,#31,#32),#7);
ENDSEC;END-ISO-10303-21;";
    let back = crate::import::ifc(text.as_bytes(), "t").unwrap();
    let bars: Vec<_> = back
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Rebar(rb) => Some(rb),
            _ => None,
        })
        .collect();
    assert_eq!(bars.len(), 1);
    assert_eq!(
        (bars[0].path.clone(), bars[0].diameter, bars[0].grade.as_str()),
        (vec![[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 500.0, 0.0]], 12.0, "CA-60")
    );
    assert_eq!(back.skipped.len(), 2, "{:?}", back.skipped);
    assert!(back.skipped.iter().all(|s| s.contains("disk swept along a line")), "{:?}", back.skipped);
    back.model.check().unwrap();
}

#[test]
fn a_section_draws_the_bars_it_cuts_as_dots_over_the_concrete_they_are_in() {
    use crate::render::{self, Prim, View};
    let mut s = Session::new();
    let beam = beam(&mut s);
    // 20 mm bar through the middle of the beam, 250 under its top.
    let bar = ids(&run(&mut s, "rebar", json!({"points": [[50, 0, -250], [5950, 0, -250]], "diameter": 20, "host": beam})))[0];
    let section = run(&mut s, "section", json!({"start": [3000, -2000], "end": [3000, 2000]}))["id"].as_u64().unwrap();
    let d = render::section(&s.model, section, View { angle: 0.0, scale: 0.1, cx: 2000.0, cy: 0.0, width: 1000, height: 800 });
    let dots: Vec<_> = d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(bar)).map(|(p, _)| p).collect();
    assert!(dots.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color != render::POCHE)), "{} prims", dots.len());
    // Where it is: 2000 along the line, 250 under the level, 20 across.
    let pts: Vec<_> = dots
        .iter()
        .flat_map(|p| match p {
            Prim::Fill { rings, .. } => rings.iter().flatten().map(|q| d.view.mm(*q)).collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    let (lo, hi) = (pts.iter().map(|q| q.0).fold(f64::MAX, f64::min), pts.iter().map(|q| q.0).fold(f64::MIN, f64::max));
    assert!((lo - 1990.0).abs() < 1.5 && (hi - 2010.0).abs() < 1.5, "{lo} {hi}");
}
