//! Where the site puts things on Earth, for the fetches: a projector built once from the active site's map system (upstream's
//! `crs.rs` and `Georeference`), the UTM zone it suggests, and a pair of coordinates pasted from a map.

use serde_json::{Value, json};

use crate::coordinates::Frame;
use crate::engine::Session;
use crate::geo::{Projector, parse_coordinates, utm_epsg, zone_of};
use crate::geom::P2;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// PROJ's answers for WGS 84 / UTM: (latitude, longitude, EPSG, easting, northing), metres.
const POINTS: &[(f64, f64, u32, f64, f64)] = &[
    (48.8584, 2.2945, 32631, 448252.0013753649, 5411954.909947274),
    (-23.5505, -46.6333, 32723, 333287.91510186717, 7394588.3185580345),
    (63.4305, 10.3951, 32632, 569618.6286081383, 7034313.849012988),
    (-33.8688, 151.2093, 32756, 334368.633648097, 6250948.345385009),
    (0.0, 3.0, 32631, 500000.0, 0.0),
    (45.0, 3.0, 32631, 500000.0, 4982950.400226552),
];

#[test]
fn a_crs_projector_gives_proj_s_numbers_point_after_point() {
    for &(lat, lon, code, e, n) in POINTS {
        let p = crate::crs::Projector::new(code).unwrap();
        let (x, y) = p.from_wgs84(lon, lat).unwrap();
        assert!((x - e * 1000.0).abs() < 1.0 && (y - n * 1000.0).abs() < 1.0, "EPSG:{code}: {x} {y} against {e} {n}");
        let (lo, la) = p.to_wgs84(x, y).unwrap();
        assert!((lo - lon).abs() < 1e-9 && (la - lat).abs() < 1e-9, "{lo} {la}");
        assert_eq!(crate::crs::from_wgs84(code, lon, lat).unwrap(), (x, y), "the one-point function is the same");
    }
    let p = crate::crs::Projector::new(32723).unwrap();
    assert!(p.from_wgs84(f64::NAN, 0.0).is_err() && p.from_wgs84(0.0, 91.0).is_err() && p.to_wgs84(f64::INFINITY, 0.0).is_err());
    assert!(crate::crs::Projector::new(4326).is_err(), "a geographic CRS has no grid");
    assert!(crate::crs::Projector::new(999_999).is_err());
}

#[test]
fn zones_follow_the_longitude_with_norways_and_svalbards_exceptions() {
    assert_eq!(zone_of(2.2945, 48.8584), 31);
    assert_eq!(zone_of(-46.6333, -23.5505), 23);
    assert_eq!(zone_of(151.2093, -33.8688), 56);
    assert_eq!(zone_of(5.32, 60.39), 32, "Bergen is in Norway's widened zone 32");
    assert_eq!(zone_of(15.6, 78.2), 33, "Svalbard has no zones 32, 34 and 36");
    assert_eq!(zone_of(-180.0, 0.0), 1);
    assert_eq!(zone_of(180.0, 0.0), 60);
    assert_eq!(zone_of(f64::NAN, 0.0), 31);
    assert_eq!((utm_epsg(-46.6333, -23.5505), utm_epsg(2.2945, 48.8584)), (32723, 32631));
}

/// A project placed at São Paulo on UTM 23S, turned 20 degrees (true north east of project north), its origin 760 m up.
fn placed() -> Session {
    let mut s = Session::new();
    let name = s.model.coordinates.active.clone();
    run(&mut s, "location", json!({"name": name, "crs": "EPSG:32723", "latitude": -23.5505, "longitude": -46.6333, "elevation": 760_000, "angle": 20}));
    s
}

#[test]
fn the_site_projector_agrees_with_the_site_s_own_conversion() {
    let s = placed();
    let p = Projector::of(&s.model).unwrap();
    let frames = s.model.frames();
    for q in [P2(0.0, 0.0), P2(150_000.0, -80_000.0), P2(-3_000.5, 12_345.6)] {
        let (lon, lat) = p.lonlat(q).unwrap();
        let want = frames.convert([q.0, q.1, 0.0], Frame::Internal, Frame::Wgs84).unwrap();
        assert!((lon - want[0]).abs() < 1e-9 && (lat - want[1]).abs() < 1e-9, "{q:?}: {lon} {lat} against {want:?}");
        let back = p.point(lon, lat).unwrap();
        assert!(back.dist(q) < 0.01, "{q:?} came back as {back:?}");
        let theirs = frames.convert([lon, lat, 0.0], Frame::Wgs84, Frame::Internal).unwrap();
        assert!(P2(theirs[0], theirs[1]).dist(back) < 0.01);
    }
    // The origin where the location put it; the height of the internal origin on the map.
    let (lon, lat) = p.origin().unwrap();
    assert!((lon - -46.6333).abs() < 1e-6 && (lat - -23.5505).abs() < 1e-6, "{lon} {lat}");
}

#[test]
fn a_project_with_no_map_system_is_refused_saying_how_to_give_it_one() {
    let s = Session::new();
    let e = Projector::of(&s.model).err().unwrap();
    assert!(e.contains("location") && e.contains("crs") && e.contains("latitude") && e.contains("\"Internal\""), "{e}");
    let mut s = Session::new();
    let name = s.model.coordinates.active.clone();
    run(&mut s, "location", json!({"name": name, "east": 1000, "north": 2000}));
    assert!(Projector::of(&s.model).is_err(), "shared coordinates with no map system are not a place on Earth");
}



#[test]
fn a_pair_copied_from_a_map_is_read_as_latitude_then_longitude() {
let want = Ok((51.2254809, 6.7700762));
for text in [
    "51.2254809, 6.7700762",
    "51.2254809,6.7700762",
    "  51.2254809 ,  6.7700762  ",
    "51.2254809; 6.7700762",
    "51.2254809 6.7700762",
    "51.2254809\t6.7700762",
    "(51.2254809, 6.7700762)",
    "[51.2254809, 6.7700762]",
    "51.2254809°, 6.7700762°",
    "51.2254809° N, 6.7700762° E",
    "N 51.2254809, E 6.7700762",
    "N51.2254809 E6.7700762",
    // A decimal comma, as a German or Brazilian map writes it.
    "51,2254809; 6,7700762",
    "51,2254809 6,7700762",
] {
    assert_eq!(parse_coordinates(text), want, "{text:?}");
}
// South and west are negative, however it is said.
assert_eq!(parse_coordinates("23.5505 S, 46.6333 W"), Ok((-23.5505, -46.6333)));
assert_eq!(parse_coordinates("S 23.5505, W 46.6333"), Ok((-23.5505, -46.6333)));
assert_eq!(parse_coordinates("-23.5505, -46.6333"), Ok((-23.5505, -46.6333)));
assert_eq!(parse_coordinates("0, 0"), Ok((0.0, 0.0)));
}

#[test]
fn what_is_not_a_pair_of_coordinates_is_refused_saying_what_was_read() {
for (text, says) in [
    ("", "latitude and longitude"),
    ("51.2254809", "two numbers"),
    ("51.2, 6.7, 100", "two numbers"),
    ("north, east", "number"),
    ("NaN, 1", "number"),
    ("inf, 1", "number"),
    ("1e999, 1", "number"),
    ("95, 6.7", "latitude"),
    ("-91, 6.7", "latitude"),
    ("51.2, 181", "longitude"),
    ("51.2, -200", "longitude"),
    // A hemisphere that says the opposite of the sign, or twice a latitude.
    ("-23.5 N, 46.6 E", "N"),
    ("23.5 N, 46.6 S", "latitude"),
    ("23.5 E, 46.6 W", "latitude"),
] {
    let e = parse_coordinates(text).expect_err(text);
    assert!(e.contains(says), "{text:?}: {e}");
}
// Pasted text is hostile: a long one is refused before it is read.
assert!(parse_coordinates(&"1".repeat(10_000)).is_err());
assert!(parse_coordinates(&format!("{}, 1", "9".repeat(400))).is_err());
}
