//! Where a project lies on a map: IFC's `IfcMapConversion` to an `IfcProjectedCRS` kept on the model as its `georeference`,
//! written back on export, corrected when the import moves a model near the origin, and read and set by the `georeference`
//! command. Written by hand.

use crate::engine::Session;
use crate::import::ifc;
use crate::model::Georeference;
use serde_json::{Value, json};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// An IFC4 file in millimetres with one proxy at `at` (mm), georeferenced to SIRGAS 2000 / UTM 23S in metres by `conversion`'s
/// arguments after its two CRSs.
fn located(conversion: &str, at: [f64; 3]) -> String {
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('0000000000000000000003',$,'Located',$,$,$,$,(#9),#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('0000000000000000000007',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#8=IFCDIRECTION((0.,0.,1.));
#9=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#5,$);
#10=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#11=IFCPROJECTEDCRS('EPSG:31983','SIRGAS 2000 / UTM zone 23S','SIRGAS2000','Imbituba','UTM','23S',#10);
#12=IFCMAPCONVERSION(#9,#11,{conversion});
#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,500.,500.);
#21=IFCEXTRUDEDAREASOLID(#20,$,#8,500.);
#22=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#21));
#23=IFCPRODUCTDEFINITIONSHAPE($,$,(#22));
#24=IFCCARTESIANPOINT(({:?},{:?},{:?}));
#25=IFCAXIS2PLACEMENT3D(#24,$,$);
#26=IFCLOCALPLACEMENT(#6,#25);
#27=IFCBUILDINGELEMENTPROXY('0000000000000000000027',$,'Marker',$,$,#26,#23,$,.ELEMENT.);
#30=IFCRELCONTAINEDINSPATIALSTRUCTURE('0000000000000000000030',$,$,$,(#27),#7);
ENDSEC;END-ISO-10303-21;",
        at[0], at[1], at[2]
    )
}

/// SIRGAS 2000 / UTM 23S, the project's x 30 degrees north of the map's east, a scale of a millimetre to the metre.
const CONVERSION: &str = "333780.5,7394820.25,760.,0.8660254037844387,0.5,0.001";

fn expected() -> Georeference {
    Georeference {
        crs: "EPSG:31983".into(),
        description: Some("SIRGAS 2000 / UTM zone 23S".into()),
        geodetic_datum: Some("SIRGAS2000".into()),
        vertical_datum: Some("Imbituba".into()),
        map_projection: Some("UTM".into()),
        map_zone: Some("23S".into()),
        eastings: 333_780_500.0,
        northings: 7_394_820_250.0,
        height: 760_000.0,
        rotation: 30.0,
        scale: 1.0,
    }
}

/// `g` rounded where floating point leaves it a hair off: lengths to the micron, the angle and the scale to a billionth.
fn rounded(mut g: Georeference) -> Georeference {
    let r = |v: f64, k: f64| (v * k).round() / k + 0.0;
    g.eastings = r(g.eastings, 1e3);
    g.northings = r(g.northings, 1e3);
    g.height = r(g.height, 1e3);
    g.rotation = r(g.rotation, 1e9);
    g.scale = r(g.scale, 1e9);
    g
}

#[test]
fn an_ifc_map_conversion_comes_in_as_the_projects_georeference() {
    let i = ifc(located(CONVERSION, [0.0, 0.0, 0.0]).as_bytes(), "x").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(i.model.georeference.clone().map(rounded), Some(expected()));
    i.model.check().unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn the_georeference_goes_back_out_to_ifc_and_comes_in_again_the_same() {
    let i = ifc(located(CONVERSION, [0.0, 0.0, 0.0]).as_bytes(), "x").unwrap_or_else(|e| panic!("{e}"));
    let out = crate::ifc::export(&i.model);
    // In the map's metres, its x axis as a direction, and the scale from the project's millimetres to them.
    assert!(out.contains("IFCPROJECTEDCRS('EPSG:31983','SIRGAS 2000 / UTM zone 23S','SIRGAS2000','Imbituba','UTM','23S',#"), "{out}");
    let conversion = out.lines().find(|l| l.contains("=IFCMAPCONVERSION(")).unwrap_or_default();
    let numbers: Vec<f64> = conversion.split(',').skip(2).filter_map(|v| v.trim_end_matches(");").parse().ok()).collect();
    let want = [333780.5, 7394820.25, 760.0, 0.8660254037844387, 0.5, 0.001];
    assert_eq!(numbers.len(), want.len(), "{conversion}");
    assert!(numbers.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9), "{conversion}");
    let back = ifc(out.as_bytes(), "x").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.model.georeference.map(rounded), Some(expected()));
    // A project with none writes none.
    let mut plain = i.model.clone();
    (plain.georeference, plain.location) = (None, None);
    assert!(!crate::ifc::export(&plain).contains("IFCMAPCONVERSION"));
}

#[test]
fn a_model_brought_near_the_origin_keeps_where_it_lies_on_the_map() {
    // The proxy 200 km east and 300 km north of the file's origin: the import moves the model near the origin, and the map point
    // of the project's new origin moves with it, turned by the rotation (none here) and scaled.
    let i = ifc(located("333780.5,7394820.25,760.,1.,0.,0.001", [2.0e8 + 1234.0, 3.0e8 + 5678.0, 0.0]).as_bytes(), "x")
        .unwrap_or_else(|e| panic!("{e}"));
    let moved = i.moved.unwrap_or_else(|| panic!("not moved"));
    let g = i.model.georeference.clone().map(rounded).unwrap_or_else(|| panic!("no georeference"));
    assert_eq!((g.eastings, g.northings), (333_780_500.0 - moved.0, 7_394_820_250.0 - moved.1));
    assert_eq!((g.rotation, g.scale), (0.0, 1.0));
}

#[test]
fn the_georeference_command_says_sets_and_clears_it_and_refuses_what_it_cannot_be() {
    let mut s = Session::new();
    assert_eq!(run(&mut s, "georeference", json!({})), json!({"georeference": null}));
    assert!(s.execute("georeference", &json!({"eastings": 5})).unwrap_err().contains("crs"));
    let set = run(&mut s, "georeference", json!({"crs": "EPSG:31983", "eastings": 333780500, "northings": 7394820250.0, "rotation": 30}));
    assert_eq!(set["georeference"]["crs"], "EPSG:31983");
    assert_eq!(set["georeference"]["scale"], 1.0);
    // What isn't given keeps its value.
    let moved = run(&mut s, "georeference", json!({"height": 760000}));
    assert_eq!(
        (moved["georeference"]["eastings"].as_f64(), moved["georeference"]["height"].as_f64()),
        (Some(333_780_500.0), Some(760_000.0))
    );
    for bad in [
        json!({"scale": 0}),
        json!({"scale": -1}),
        json!({"eastings": 1e300}),
        json!({"rotation": "north"}),
        json!({"crs": ""}),
        json!({"crs": "x".repeat(crate::model::MAX_TEXT + 1)}),
        json!({"datum": "SIRGAS2000"}),
    ] {
        assert!(s.execute("georeference", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.model.georeference.as_ref().map(|g| g.height), Some(760_000.0));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.georeference.as_ref().map(|g| g.height), Some(0.0));
    assert_eq!(run(&mut s, "georeference", json!({"clear": true})), json!({"georeference": null}));
    assert!(s.model.georeference.is_none());
}

#[test]
fn the_georeference_is_kept_in_the_project_file() {
    let mut s = Session::new();
    run(&mut s, "georeference", json!({"crs": "EPSG:31983", "eastings": 333780500, "northings": 7394820250.0, "rotation": 30}));
    let text = s.model.to_json().unwrap_or_else(|e| panic!("{e}"));
    let (back, left_out) = crate::model::Model::read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.georeference, s.model.georeference);
}
