//! Where the project is on Earth: a latitude and longitude for its origin, the angle to true north, and a projected CRS (UTM
//! unless another is named), so the project's millimetres can meet maps, surveys and OpenStreetMap's degrees. Revit's
//! Manage > Location, and IFC's `IfcSite` position with `IfcMapConversion`.
//!
//! The projections are `proj-core`'s (pure Rust, EPSG registry, checked against PROJ). The project's plane is mapped to the
//! grid by one rotation and one scale taken at its origin, which holds over a site of some kilometres.

use proj_core::{Transform, lookup_epsg};
use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::model::MAX_COORD;

/// The latitudes UTM is defined for, degrees.
pub const MIN_LATITUDE: f64 = -80.0;
pub const MAX_LATITUDE: f64 = 84.0;

/// The project's place: its origin (0, 0) at `elevation` is at `latitude` and `longitude` (degrees), and true north is
/// `true_north` degrees counter-clockwise from the project's +y.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    /// Height of the origin above sea level, mm.
    #[serde(default)]
    pub elevation: f64,
    /// The angle from the project's +y to true north, degrees counter-clockwise: 0, +y is true north; 90, true north is -x.
    #[serde(default)]
    pub true_north: f64,
    /// The EPSG code of the projected CRS the project's grid is in (31983 is SIRGAS 2000 / UTM zone 23S), where WGS 84's
    /// UTM zone of the place (`utm_epsg`) isn't wanted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<u32>,
}

/// What a [`Location`] maps to: its projected CRS, and the project's origin and rotation in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub epsg: u32,
    pub name: String,
    /// The origin's easting and northing, metres.
    pub easting: f64,
    pub northing: f64,
    /// The angle from true north clockwise to grid north, degrees (PROJ's meridian convergence).
    pub convergence: f64,
    /// The point scale at the origin: a metre on the ground is this many on the grid.
    pub scale: f64,
    /// The angle from the grid's east to the project's +x, degrees counter-clockwise.
    pub rotation: f64,
}

/// The standard UTM zone of a place, with Norway's and Svalbard's exceptions.
pub fn zone_of(longitude: f64, latitude: f64) -> u8 {
    if !longitude.is_finite() || !latitude.is_finite() {
        return 31;
    }
    let mut zone = (((longitude + 180.0) / 6.0).floor() as i64 + 1).clamp(1, 60) as u8;
    if (56.0..64.0).contains(&latitude) && (3.0..12.0).contains(&longitude) {
        zone = 32;
    }
    if (72.0..84.0).contains(&latitude) {
        zone = match longitude {
            l if (0.0..9.0).contains(&l) => 31,
            l if (9.0..21.0).contains(&l) => 33,
            l if (21.0..33.0).contains(&l) => 35,
            l if (33.0..42.0).contains(&l) => 37,
            _ => zone,
        };
    }
    zone
}

/// The EPSG code of WGS 84 / UTM for a place: 326zz north of the equator, 327zz south.
pub fn utm_epsg(longitude: f64, latitude: f64) -> u32 {
    (if latitude >= 0.0 { 32600 } else { 32700 }) + u32::from(zone_of(longitude, latitude))
}

/// An angle in degrees as IFC's compound plane angle: degrees, minutes, seconds and millionths of a second, all of the angle's sign.
pub fn compound(degrees: f64) -> [i64; 4] {
    let micro = (degrees.abs() * 3.6e9).round().min(3.6e9 * 360.0) as i64;
    let sign = if degrees < 0.0 { -1 } else { 1 };
    [
        sign * (micro / 3_600_000_000),
        sign * (micro % 3_600_000_000 / 60_000_000),
        sign * (micro % 60_000_000 / 1_000_000),
        sign * (micro % 1_000_000),
    ]
}

/// The degrees of an IFC compound plane angle (degrees, minutes, seconds and, if there, millionths of a second): the sign of its
/// first part that isn't zero.
pub fn from_compound(parts: &[f64]) -> Option<f64> {
    if !(3..=4).contains(&parts.len()) || parts.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let sign = if parts.iter().find(|p| **p != 0.0).is_some_and(|p| *p < 0.0) { -1.0 } else { 1.0 };
    let unit = [1.0, 60.0, 3600.0, 3.6e9];
    Some(sign * parts.iter().zip(unit).map(|(p, u)| p.abs() / u).sum::<f64>())
}

/// The longitude and latitude, degrees, of an easting and northing, metres, in the projected CRS `epsg`.
pub fn lonlat_of_grid(epsg: u32, easting: f64, northing: f64) -> Result<(f64, f64), String> {
    let target = format!("EPSG:{epsg}");
    let def = lookup_epsg(epsg).ok_or_else(|| format!("EPSG:{epsg} is not a CRS this version of BIMCraft knows"))?;
    if def.as_projected().is_none() {
        return Err(format!("EPSG:{epsg} ({}) is not a projected CRS", def.name()));
    }
    let back = Transform::new(&target, "EPSG:4326")
        .or_else(|first| match def.base_geographic_crs_epsg() {
            Some(base) => Transform::from_epsg(epsg, base),
            None => Err(first),
        })
        .map_err(|e| format!("cannot map from {target} ({}): {e}", def.name()))?;
    let (lon, lat) =
        back.convert((easting, northing)).map_err(|e| format!("({easting}, {northing}) is outside {target} ({}): {e}", def.name()))?;
    if lon.is_finite() && lat.is_finite() { Ok((lon, lat)) } else { Err(format!("({easting}, {northing}) is not on the map of {target}")) }
}

/// The longest text [`parse_coordinates`] reads: a pair is under fifty characters, and a clipboard holds anything.
const MAX_COORDINATES: usize = 200;

/// One number of a pair, with the hemisphere it names (`N`, `S`, `E` or `W`, before or after it), as the sign it makes: `23.5505 S` is
/// -23.5505. A decimal comma is a point; a degree sign is dropped.
fn coordinate(part: &str) -> Result<(f64, Option<char>), String> {
    let text = part.trim().replace('°', "");
    let text = text.trim();
    let number = |t: &str| -> Option<f64> {
        let t = t.trim();
        let t = if t.contains('.') { t.to_string() } else { t.replace(',', ".") };
        t.parse::<f64>().ok().filter(|x| x.is_finite())
    };
    if let Some(x) = number(text) {
        return Ok((x, None));
    }
    let hemisphere = |c: char| matches!(c.to_ascii_uppercase(), 'N' | 'S' | 'E' | 'W');
    let (first, last) = (text.chars().next(), text.chars().next_back());
    let tries = [
        first.filter(|c| hemisphere(*c)).map(|c| (c, &text[c.len_utf8()..])),
        last.filter(|c| hemisphere(*c)).map(|c| (c, &text[..text.len() - c.len_utf8()])),
    ];
    for (letter, rest) in tries.into_iter().flatten() {
        if let Some(x) = number(rest) {
            let letter = letter.to_ascii_uppercase();
            if x < 0.0 || rest.trim_start().starts_with('+') {
                return Err(format!("`{text}` has a sign and a hemisphere ({letter}): give one or the other"));
            }
            return Ok((if matches!(letter, 'S' | 'W') { -x } else { x }, Some(letter)));
        }
    }
    Err(format!("`{text}` is not a number of degrees"))
}

/// A latitude and a longitude, degrees, read from the text of a pair as a map gives it: `51.2254809, 6.7700762`, with a `;` or a space
/// for the comma, a decimal comma (`51,2254809; 6,7700762`), degree signs, brackets, and `N`, `S`, `E`, `W` before or after a number
/// (which may then come in either order). The pair is latitude first unless a hemisphere says otherwise; a latitude beyond 90 or a
/// longitude beyond 180 is refused, never taken for a swapped pair.
pub fn parse_coordinates(text: &str) -> Result<(f64, f64), String> {
    let wanted = "give a latitude and longitude in degrees, such as 51.2254809, 6.7700762";
    let t = text.trim();
    if t.is_empty() {
        return Err(wanted.to_string());
    }
    if t.chars().count() > MAX_COORDINATES {
        return Err(format!("that is too long to be a pair of coordinates: {wanted}"));
    }
    let t = t.trim_matches(|c| "()[]{}".contains(c)).trim();
    let parts: Vec<&str> = if t.contains(';') {
        t.split(';').collect()
    } else if t.matches(',').count() == 1 {
        t.split(',').collect()
    } else {
        t.split_whitespace().collect()
    };
    let [first, second] = parts.as_slice() else {
        return Err(format!("two numbers (latitude, longitude) are wanted, not {}: {wanted}", parts.len()));
    };
    let ((a, ha), (b, hb)) = (coordinate(first)?, coordinate(second)?);
    let is_latitude = |h: char| matches!(h, 'N' | 'S');
    // Which is the latitude: what a hemisphere says, else the first.
    let first_is_latitude = match (ha, hb) {
        (Some(x), Some(y)) if is_latitude(x) == is_latitude(y) => {
            return Err(format!(
                "`{first}` and `{second}` are both a {}: one latitude (N, S) and one longitude (E, W) are wanted",
                if is_latitude(x) { "latitude" } else { "longitude" }
            ));
        }
        (Some(x), _) => is_latitude(x),
        (None, Some(y)) => !is_latitude(y),
        (None, None) => true,
    };
    let (latitude, longitude) = if first_is_latitude { (a, b) } else { (b, a) };
    if latitude.abs() > 90.0 {
        return Err(format!("the latitude {latitude} is beyond 90°: a pair is latitude first, then longitude"));
    }
    if longitude.abs() > 180.0 {
        return Err(format!("the longitude {longitude} is beyond 180°"));
    }
    Ok((latitude, longitude))
}

/// A step along the meridian and the parallel, degrees, to measure the grid's convergence and scale with.
const STEP: f64 = 1e-5;
/// WGS 84's semi-major axis, metres, and first eccentricity squared: the ground length of a step along the parallel.
const A: f64 = 6_378_137.0;
const E2: f64 = 0.006_694_379_990_141_316;

/// A [`Location`] ready to convert: the transforms, and where the origin is on the grid.
struct Mapping {
    epsg: u32,
    name: String,
    forward: Transform,
    back: Transform,
    easting: f64,
    northing: f64,
    /// Radians: the project's +x from the grid's east, counter-clockwise.
    rotation: f64,
    scale: f64,
    /// Degrees: from true north clockwise to grid north.
    convergence: f64,
}

/// A [`Location`] ready to turn many longitudes and latitudes into project points: the transform is built once.
pub struct Projector {
    m: Mapping,
}

impl Projector {
    /// The project's plane point, mm, of a longitude and latitude, degrees.
    pub fn point(&self, longitude: f64, latitude: f64) -> Result<P2, String> {
        if !longitude.is_finite() || !latitude.is_finite() || longitude.abs() > 180.0 || latitude.abs() > 90.0 {
            return Err(format!("({longitude}, {latitude}) is not on the Earth"));
        }
        let m = &self.m;
        let (e, n) = m
            .forward
            .convert((longitude, latitude))
            .map_err(|err| format!("({longitude}, {latitude}) is off the map of EPSG:{}: {err}", m.epsg))?;
        let (dx, dy) = ((e - m.easting) / m.scale, (n - m.northing) / m.scale);
        let (s, c) = m.rotation.sin_cos();
        let p = P2((dx * c + dy * s) * 1000.0, (-dx * s + dy * c) * 1000.0);
        if p.0.is_finite() && p.1.is_finite() { Ok(p) } else { Err(format!("({longitude}, {latitude}) cannot be mapped to the project")) }
    }
}

impl Location {
    /// The CRS code in use: the one named, else WGS 84's UTM zone of the place.
    pub fn epsg(&self) -> u32 {
        self.crs.unwrap_or_else(|| utm_epsg(self.longitude, self.latitude))
    }

    /// What is wrong with it, in words that say what to fix.
    pub fn problem(&self) -> Option<String> {
        if !(MIN_LATITUDE..=MAX_LATITUDE).contains(&self.latitude) {
            return Some(format!(
                "`latitude` is degrees from {MIN_LATITUDE} to {MAX_LATITUDE} (where UTM and the national grids are defined), not {}",
                self.latitude
            ));
        }
        if !(-180.0..=180.0).contains(&self.longitude) {
            return Some(format!("`longitude` is degrees from -180 to 180, not {}", self.longitude));
        }
        if !self.elevation.is_finite() || self.elevation.abs() > MAX_COORD {
            return Some(format!("`elevation` is a height in mm within {MAX_COORD}, not {}", self.elevation));
        }
        if !self.true_north.is_finite() || self.true_north.abs() > 360.0 {
            return Some(format!("`true_north` is degrees from -360 to 360, not {}", self.true_north));
        }
        let code = self.epsg();
        let Some(def) = lookup_epsg(code) else {
            return Some(format!(
                "EPSG:{code} is not a CRS this version of BIMCraft knows: name a UTM or national grid (31983 is SIRGAS 2000 / UTM zone 23S)"
            ));
        };
        let Some(projected) = def.as_projected() else {
            return Some(format!(
                "`crs` EPSG:{code} ({}) is not a projected CRS: name a UTM zone or a national grid, as 32723 or 31983",
                def.name()
            ));
        };
        if (projected.linear_unit_to_meter() - 1.0).abs() > 1e-12 {
            return Some(format!("EPSG:{code} ({}) is not in metres: BIMCraft maps in a metre grid", projected.name()));
        }
        None
    }

    /// The transforms, and the origin, rotation and scale in the grid.
    fn mapping(&self) -> Result<Mapping, String> {
        if let Some(why) = self.problem() {
            return Err(why);
        }
        let epsg = self.epsg();
        let def = lookup_epsg(epsg).ok_or_else(|| format!("EPSG:{epsg} is not a CRS this version of BIMCraft knows"))?;
        let name = def.name().to_string();
        let target = format!("EPSG:{epsg}");
        // WGS 84's degrees, or, where the registry has no step from WGS 84 to this CRS, its own datum's.
        let forward = Transform::new("EPSG:4326", &target)
            .or_else(|first| match def.base_geographic_crs_epsg() {
                Some(base) => Transform::from_epsg(base, epsg),
                None => Err(first),
            })
            .map_err(|e| format!("cannot map to {target} ({name}): {e}"))?;
        let back = forward.inverse().map_err(|e| format!("cannot map from {target} ({name}): {e}"))?;
        let wrong = |e: proj_core::Error| format!("({}, {}) is outside {target} ({name}): {e}", self.longitude, self.latitude);
        let (easting, northing) = forward.convert((self.longitude, self.latitude)).map_err(wrong)?;
        // Where true north points on the grid, and how long a ground metre is there, from two small steps.
        let (e_up, n_up) = forward.convert((self.longitude, self.latitude + STEP)).map_err(wrong)?;
        let (e_east, n_east) = forward.convert((self.longitude + STEP, self.latitude)).map_err(wrong)?;
        let grid_azimuth = (e_up - easting).atan2(n_up - northing);
        let phi = self.latitude.to_radians();
        let ground = A / (1.0 - E2 * phi.sin().powi(2)).sqrt() * phi.cos() * STEP.to_radians();
        let scale = (e_east - easting).hypot(n_east - northing) / ground;
        if ![easting, northing, grid_azimuth, scale].iter().all(|v| v.is_finite()) || scale <= 0.0 {
            return Err(format!("({}, {}) cannot be mapped to {target} ({name})", self.longitude, self.latitude));
        }
        // The project's +y lies `true_north` degrees clockwise of true north; +x is a quarter turn on.
        let rotation = -(self.true_north.to_radians() + grid_azimuth);
        Ok(Mapping { epsg, name, forward, back, easting, northing, rotation, scale, convergence: -grid_azimuth.to_degrees() })
    }

    /// The grid, or why this place can't be mapped to it.
    pub fn grid(&self) -> Result<Grid, String> {
        let m = self.mapping()?;
        Ok(Grid {
            epsg: m.epsg,
            name: m.name,
            easting: m.easting,
            northing: m.northing,
            convergence: m.convergence,
            scale: m.scale,
            rotation: m.rotation.to_degrees(),
        })
    }

    /// The longitude and latitude, degrees, of a point in the project's plane, mm.
    pub fn to_lonlat(&self, p: P2) -> Result<(f64, f64), String> {
        let m = self.mapping()?;
        let (x, y) = (p.0 / 1000.0, p.1 / 1000.0);
        let (s, c) = m.rotation.sin_cos();
        let (e, n) = (m.easting + m.scale * (x * c - y * s), m.northing + m.scale * (x * s + y * c));
        m.back.convert((e, n)).map_err(|err| format!("({}, {}) mm is off the map of EPSG:{}: {err}", p.0, p.1, m.epsg))
    }

    /// The project's plane point, mm, of a longitude and latitude, degrees.
    pub fn from_lonlat(&self, longitude: f64, latitude: f64) -> Result<P2, String> {
        self.projector()?.point(longitude, latitude)
    }

    /// Many places at once: the same, with the transform built the once.
    pub fn projector(&self) -> Result<Projector, String> {
        Ok(Projector { m: self.mapping()? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(latitude: f64, longitude: f64, true_north: f64) -> Location {
        Location { latitude, longitude, elevation: 0.0, true_north, crs: None }
    }

    /// PROJ's answers for WGS 84 / UTM: (latitude, longitude, EPSG, easting, northing).
    const POINTS: &[(f64, f64, u32, f64, f64)] = &[
        (48.8584, 2.2945, 32631, 448252.0013753649, 5411954.909947274),
        (-23.5505, -46.6333, 32723, 333287.91510186717, 7394588.3185580345),
        (63.4305, 10.3951, 32632, 569618.6286081383, 7034313.849012988),
        (10.0, 6.0001, 32632, 171082.23771862575, 1106908.7543760345),
        (45.0, 8.4, 32631, 925602.8102132763, 4997153.326776612),
        (-33.8688, 151.2093, 32756, 334368.633648097, 6250948.345385009),
        (0.0, 3.0, 32631, 500000.0, 0.0),
        (45.0, 3.0, 32631, 500000.0, 4982950.400226552),
    ];

    #[test]
    fn angles_are_written_and_read_as_ifc_compound_angles() {
        assert_eq!(compound(-23.5505), [-23, -33, -1, -800_000]);
        assert_eq!(compound(48.8584), [48, 51, 30, 240_000]);
        assert_eq!(compound(0.0), [0, 0, 0, 0]);
        assert_eq!(compound(-0.5), [0, -30, 0, 0]);
        for degrees in [-23.5505, 48.8584, -0.000_000_3, 179.999_999_9, -123.456_789_012] {
            let back = from_compound(&compound(degrees).map(|p| p as f64)).unwrap();
            assert!((back - degrees).abs() < 1e-9, "{degrees} came back as {back}");
        }
        assert_eq!(from_compound(&[-0.0, -30.0, 0.0]), Some(-0.5), "three parts, the sign of the first that isn't zero");
        assert_eq!(from_compound(&[1.0, 2.0]), None);
        assert_eq!(from_compound(&[f64::NAN, 0.0, 0.0]), None);
    }

    #[test]
    fn the_origin_lands_where_proj_puts_it_to_a_millimetre() {
        for &(lat, lon, epsg, e, n) in POINTS {
            // Named, since (45, 8.4) is in zone 32 and PROJ's answer is zone 31's, far from its central meridian.
            let g = Location { crs: Some(epsg), ..at(lat, lon, 0.0) }.grid().unwrap();
            assert_eq!(g.epsg, epsg, "{lat} {lon}");
            assert!((g.easting - e).abs() < 1e-3 && (g.northing - n).abs() < 1e-3, "{lat} {lon}: {} {}", g.easting, g.northing);
        }
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
        assert_eq!((utm_epsg(-46.6333, -23.5505), utm_epsg(2.2945, 48.8584)), (32723, 32631));
    }

    #[test]
    fn the_grid_has_proj_s_convergence_and_scale() {
        // (location, PROJ's meridian convergence in degrees, its point scale)
        for (l, convergence, scale) in [
            (at(-23.5505, -46.6333, 0.0), 0.6527479008603797, 0.9999433233092226),
            (at(63.4305, 10.3951, 0.0), 1.2478164570111563, 0.9996593558535252),
            (Location { crs: Some(32631), ..at(45.0, 8.4, 0.0) }, 3.8240917855595637, 1.0018280262260442),
            (Location { crs: Some(2154), ..at(48.8584, 2.2945, 0.0) }, -0.5119162782452813, 0.9998938936465761),
        ] {
            let g = l.grid().unwrap();
            assert!((g.convergence - convergence).abs() < 1e-6, "{l:?}: {}", g.convergence);
            assert!((g.scale - scale).abs() < 1e-7, "{l:?}: {}", g.scale);
        }
    }

    #[test]
    fn another_crs_can_be_named_by_its_epsg_code() {
        // SIRGAS 2000 / UTM zone 23S, São Paulo: PROJ's answer for 4326 and for SIRGAS 2000 alike.
        let l = Location { crs: Some(31983), ..at(-23.5505, -46.6333, 0.0) };
        let g = l.grid().unwrap();
        assert_eq!(g.epsg, 31983);
        assert!(g.name.contains("SIRGAS 2000") && g.name.contains("23S"), "{}", g.name);
        assert!((g.easting - 333287.91510143026).abs() < 1e-2 && (g.northing - 7394588.318636952).abs() < 1e-2, "{g:?}");
        // The neighbour's zone, for a site near the edge.
        let l = Location { crs: Some(32632), ..at(48.8584, 5.9, 0.0) };
        assert_eq!(l.grid().unwrap().epsg, 32632);
        assert_eq!(at(48.8584, 5.9, 0.0).grid().unwrap().epsg, 32631);
    }

    #[test]
    fn the_origin_is_where_the_location_says_and_points_round_trip() {
        let l = at(-23.5505, -46.6333, 17.0);
        let (lon, lat) = l.to_lonlat(P2(0.0, 0.0)).unwrap();
        assert!((lon + 46.6333).abs() < 1e-9 && (lat + 23.5505).abs() < 1e-9);
        for p in [P2(3_000_000.0, -2_000_000.0), P2(-250.0, 1234.5), P2(0.0, 0.0)] {
            let (lon, lat) = l.to_lonlat(p).unwrap();
            let q = l.from_lonlat(lon, lat).unwrap();
            assert!((q.0 - p.0).abs() < 1.0 && (q.1 - p.1).abs() < 1.0, "{p:?} came back as {q:?}");
        }
    }

    #[test]
    fn a_point_one_kilometre_away_is_about_a_kilometre_away_on_the_ground() {
        // Ground distance by the haversine formula: the grid scale and the rotation must not show in it.
        let l = at(-23.5505, -46.6333, 33.0);
        let (lon0, lat0) = l.to_lonlat(P2(0.0, 0.0)).unwrap();
        let (lon1, lat1) = l.to_lonlat(P2(600_000.0, 800_000.0)).unwrap();
        let (p0, p1, dl) = (lat0.to_radians(), lat1.to_radians(), (lon1 - lon0).to_radians());
        let h = ((p1 - p0) / 2.0).sin().powi(2) + p0.cos() * p1.cos() * (dl / 2.0).sin().powi(2);
        let metres = 2.0 * 6_371_000.0 * h.sqrt().asin();
        assert!((metres - 1000.0).abs() < 1.5, "{metres}");
    }

    #[test]
    fn a_projector_gives_what_from_lonlat_gives_and_refuses_what_it_refuses() {
        let l = at(-23.5505, -46.6333, 17.0);
        let projector = l.projector().unwrap();
        for (lon, lat) in [(-46.6333, -23.5505), (-46.6, -23.5), (-46.7, -23.6), (-46.6330, -23.5502)] {
            let (a, b) = (projector.point(lon, lat).unwrap(), l.from_lonlat(lon, lat).unwrap());
            assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "{lon} {lat}: {a:?} {b:?}");
        }
        assert!(projector.point(f64::NAN, 0.0).is_err() && projector.point(0.0, 95.0).is_err(), "off the Earth");
        assert!(at(95.0, 0.0, 0.0).projector().is_err(), "a place that cannot be mapped has no projector");
    }

    #[test]
    fn true_north_turns_the_project_against_the_compass() {
        // On a central meridian there is no convergence: +y is north at 0 degrees, and at 90 it is east, so +x is south.
        let up = at(10.0, 3.0, 0.0);
        let (lon, lat) = up.to_lonlat(P2(0.0, 1_000_000.0)).unwrap();
        assert!((lon - 3.0).abs() < 1e-7 && lat > 10.0 + 0.0089 && lat < 10.0 + 0.0091, "{lon} {lat}");
        let turned = at(10.0, 3.0, 90.0);
        let (lon, lat) = turned.to_lonlat(P2(0.0, 1_000_000.0)).unwrap();
        assert!(lon > 3.0 && (lat - 10.0).abs() < 1e-6, "+y is east: {lon} {lat}");
        let (lon, lat) = turned.to_lonlat(P2(1_000_000.0, 0.0)).unwrap();
        assert!((lon - 3.0).abs() < 1e-6 && lat < 10.0, "+x is south: {lon} {lat}");
    }

    #[test]
    fn a_location_that_cannot_be_mapped_is_refused_with_what_to_fix() {
        assert_eq!(at(10.0, 3.0, 0.0).problem(), None);
        assert_eq!(Location { crs: Some(31983), ..at(-23.5, -46.6, 0.0) }.problem(), None);
        for (l, says) in [
            (at(85.0, 3.0, 0.0), "latitude"),
            (at(f64::NAN, 3.0, 0.0), "latitude"),
            (at(10.0, 181.0, 0.0), "longitude"),
            (at(10.0, 3.0, f64::INFINITY), "true_north"),
            (Location { elevation: 1e12, ..at(10.0, 3.0, 0.0) }, "elevation"),
            (Location { crs: Some(4326), ..at(10.0, 3.0, 0.0) }, "projected"),
            (Location { crs: Some(999_999), ..at(10.0, 3.0, 0.0) }, "EPSG:999999"),
            (Location { crs: Some(2263), ..at(40.7, -74.0, 0.0) }, "metre"),
        ] {
            assert!(l.problem().is_some_and(|p| p.contains(says)), "{l:?}: {:?}", l.problem());
        }
    }
}
