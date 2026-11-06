//! A project's place on Earth in IFC: `IfcSite`'s position, the context's true north, `IfcProjectedCRS` and `IfcMapConversion`,
//! written and read back. Tested through the entities in the STEP file.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::georef::Location;
use crate::ifc::export;
use crate::import;
use crate::step::{self, Entity, File, V};

fn placed(p: Value) -> Session {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    s.execute("geolocate", &p).unwrap();
    s
}

fn only<'a>(f: &'a File, name: &str) -> (u64, &'a Entity) {
    let all: Vec<_> = f.entities.iter().filter(|(_, e)| e.name == name).collect();
    assert_eq!(all.len(), 1, "one {name}");
    (*all[0].0, all[0].1)
}

fn real(v: &V) -> f64 {
    match v {
        V::Real(x) => *x,
        V::Int(i) => *i as f64,
        other => panic!("{other:?} is not a number"),
    }
}

fn list(v: &V) -> Vec<f64> {
    match v {
        V::List(items) => items.iter().map(real).collect(),
        other => panic!("{other:?} is not a list"),
    }
}

fn text(v: &V) -> &str {
    match v {
        V::Str(s) => s,
        other => panic!("{other:?} is not text"),
    }
}

#[test]
fn a_project_with_no_place_writes_a_bare_site_and_no_map() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let text = export(&s.model);
    assert!(!text.contains("IFCMAPCONVERSION") && !text.contains("IFCPROJECTEDCRS"));
    let f = step::parse(text.as_bytes()).unwrap();
    let (_, site) = only(&f, "IFCSITE");
    assert!(site.args[9..12].iter().all(|v| *v == V::Null), "no latitude, longitude or elevation");
    let (_, context) = only(&f, "IFCGEOMETRICREPRESENTATIONCONTEXT");
    assert_eq!(context.args[5], V::Null, "no true north");
}

#[test]
fn a_located_project_writes_the_sites_position_true_north_and_map_conversion() {
    let s = placed(json!({"latitude": -23.5505, "longitude": -46.6333, "elevation": 760000, "true_north": 20, "crs": 31983}));
    let f = step::parse(export(&s.model).as_bytes()).unwrap();
    let (_, site) = only(&f, "IFCSITE");
    assert_eq!(list(&site.args[9]), [-23.0, -33.0, -1.0, -800_000.0], "latitude: degrees, minutes, seconds, millionths");
    assert_eq!(&list(&site.args[10])[..3], [-46.0, -37.0, -59.0]);
    assert_eq!(real(&site.args[11]), 760_000.0, "elevation in the project's mm");

    // True north is a direction in the project's plane: 20 degrees counter-clockwise from +y is (-sin, cos).
    let (ctx_id, context) = only(&f, "IFCGEOMETRICREPRESENTATIONCONTEXT");
    let V::Ref(north) = context.args[5] else { panic!("true north is a direction") };
    let ratios = list(&f.entities[&north].args[0]);
    assert!((ratios[0] + 20f64.to_radians().sin()).abs() < 1e-9 && (ratios[1] - 20f64.to_radians().cos()).abs() < 1e-9, "{ratios:?}");

    let grid = s.model.location.unwrap().grid().unwrap();
    let (_, crs) = only(&f, "IFCPROJECTEDCRS");
    assert_eq!(text(&crs.args[0]), "EPSG:31983");
    assert!(text(&crs.args[1]).contains("SIRGAS 2000"));
    assert_eq!(text(&crs.args[2]), "SIRGAS 2000", "its geodetic datum");
    let V::Ref(unit) = crs.args[6] else { panic!("a map unit") };
    assert_eq!(f.entities[&unit].name, "IFCSIUNIT");

    let (_, map) = only(&f, "IFCMAPCONVERSION");
    assert_eq!(map.args[0], V::Ref(ctx_id), "from the model context");
    assert!(
        (real(&map.args[2]) - grid.easting).abs() < 1e-6 && (real(&map.args[3]) - grid.northing).abs() < 1e-6,
        "eastings and northings, metres"
    );
    assert!((real(&map.args[4]) - 760.0).abs() < 1e-9, "orthogonal height, metres");
    let theta = grid.rotation.to_radians();
    assert!(
        (real(&map.args[5]) - theta.cos()).abs() < 1e-9 && (real(&map.args[6]) - theta.sin()).abs() < 1e-9,
        "the project's x axis in the grid"
    );
    assert!((real(&map.args[7]) - grid.scale * 0.001).abs() < 1e-12, "mm of the project to metres of the grid, scale included");
}

#[test]
fn utm_writes_its_projection_and_zone() {
    let s = placed(json!({"latitude": 48.8584, "longitude": 2.2945}));
    let f = step::parse(export(&s.model).as_bytes()).unwrap();
    let (_, crs) = only(&f, "IFCPROJECTEDCRS");
    assert_eq!((text(&crs.args[0]), text(&crs.args[2]), text(&crs.args[4]), text(&crs.args[5])), ("EPSG:32631", "WGS84", "UTM", "31N"));
}

#[test]
fn the_place_comes_back_from_the_file() {
    for (p, crs) in [
        (json!({"latitude": -23.5505, "longitude": -46.6333, "elevation": 760000, "true_north": 20, "crs": 31983}), Some(31983)),
        (json!({"latitude": 63.4305, "longitude": 10.3951, "true_north": -135.5}), None),
        (json!({"latitude": 48.8584, "longitude": 2.2945, "elevation": -2500, "true_north": 0, "crs": 2154}), Some(2154)),
    ] {
        let s = placed(p.clone());
        let was = s.model.location.unwrap();
        let got = import::ifc(export(&s.model).as_bytes(), "x").unwrap();
        let l = got.model.location.unwrap_or_else(|| panic!("{p}: no location came back; skipped {:?}", got.skipped));
        assert!((l.latitude - was.latitude).abs() < 1e-8 && (l.longitude - was.longitude).abs() < 1e-8, "{p}: {l:?}");
        assert!((l.elevation - was.elevation).abs() < 1e-3 && (l.true_north - was.true_north).abs() < 1e-6, "{p}: {l:?}");
        assert_eq!(l.crs, crs, "{p}");
        assert!(got.skipped.is_empty(), "{:?}", got.skipped);
    }
}

/// An IFC4 file in mm with a project, a site, and what `extra` adds.
fn file(site: &str, extra: &str) -> String {
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCCARTESIANPOINT((0.,0.,0.));
#4=IFCAXIS2PLACEMENT3D(#3,$,$);
#5=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#4,$);
#6=IFCPROJECT('p',$,'Placed',$,$,$,$,(#5),#2);
#7={site};
{extra}ENDSEC;END-ISO-10303-21;
"
    )
}

#[test]
fn a_file_with_only_a_map_conversion_is_placed_by_inverting_it() {
    // São Paulo in SIRGAS 2000 / UTM 23S, by PROJ: the grid's origin only, no latitude on the site.
    let extra = "#10=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#11=IFCPROJECTEDCRS('EPSG:31983','SIRGAS 2000 / UTM zone 23S','SIRGAS 2000',$,'UTM','23S',#10);
#12=IFCMAPCONVERSION(#5,#11,333287.91510143026,7394588.318636952,760.,1.,0.,0.001);
";
    let got = import::ifc(file("IFCSITE('s',$,'Site',$,$,$,$,$,.ELEMENT.,$,$,$,$,$)", extra).as_bytes(), "x").unwrap();
    let l = got.model.location.unwrap_or_else(|| panic!("no location; skipped {:?}", got.skipped));
    assert!((l.longitude + 46.6333).abs() < 1e-7 && (l.latitude + 23.5505).abs() < 1e-7, "{l:?}");
    assert_eq!(l.crs, Some(31983));
    assert!((l.elevation - 760_000.0).abs() < 1e-3, "{}", l.elevation);
    // The project's x is grid east, so true north is the grid's convergence (0.65 degrees) counter-clockwise from +y.
    assert!((l.true_north - 0.6527479).abs() < 1e-4, "{}", l.true_north);
}

#[test]
fn a_site_position_alone_places_the_project_with_its_true_north() {
    let site = "IFCSITE('s',$,'Site',$,$,$,$,$,.ELEMENT.,(48,51,30,240000),(2,17,40,200000),35000.,$,$)";
    let with_north = file(site, "").replace("#4,$);", "#4,#9);").replace("#7=", "#9=IFCDIRECTION((-0.5,0.8660254037844386));\n#7=");
    let got = import::ifc(with_north.as_bytes(), "x").unwrap();
    let l = got.model.location.unwrap();
    assert!((l.latitude - 48.8584).abs() < 1e-8 && (l.longitude - 2.2945).abs() < 1e-8, "{l:?}");
    assert!((l.elevation - 35_000.0).abs() < 1e-6 && (l.true_north - 30.0).abs() < 1e-6, "{l:?}");
    assert_eq!(l.crs, None);
    // Without any, the project is not placed.
    let got = import::ifc(file("IFCSITE('s',$,'Site',$,$,$,$,$,.ELEMENT.,$,$,$,$,$)", "").as_bytes(), "x").unwrap();
    assert_eq!(got.model.location, None::<Location>);
}

#[test]
fn a_place_the_project_cannot_hold_is_noted_and_left_out() {
    // Latitude 88 is beyond UTM's reach; the rest of the file still opens.
    let site = "IFCSITE('s',$,'Site',$,$,$,$,$,.ELEMENT.,(88,0,0),(10,0,0),$,$,$)";
    let got = import::ifc(file(site, "").as_bytes(), "x").unwrap();
    assert_eq!(got.model.location, None);
    assert!(got.skipped.iter().any(|n| n.contains("location") && n.contains("latitude")), "{:?}", got.skipped);
}
