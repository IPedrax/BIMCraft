//! A map's coordinate reference system: a projected CRS of EPSG's dataset, which shared coordinates are in, and the way from
//! them to WGS84 (longitude and latitude), for the map and the sun ([coordinates](../../../docs/areas/coordinates.md)).
//!
//! The definitions are EPSG's, as proj strings (`crs-definitions`, CC0, from the EPSG Geodetic Parameter Dataset of IOGP), and
//! the projections pure Rust (`proj4rs`, a transcription of PROJ.4). Datum shifts are the definitions' seven Helmert parameters,
//! good to a metre or so: plenty for a map and the sun, and survey numbers never go through them, only shown ones.

use proj4rs::proj::Proj;

/// EPSG's WGS84, longitude and latitude.
const WGS84: &str = "+proj=longlat +datum=WGS84 +no_defs";

/// A projected CRS, as the project shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub code: u32,
    /// Its name, as EPSG gives it: "SIRGAS 2000 / UTM zone 23S".
    pub name: String,
    /// Its grid's unit: "metre", "foot", "US survey foot" or "<n> m".
    pub unit: String,
    /// Metres per unit of its grid.
    pub to_meter: f64,
}

impl Info {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"code": self.code, "name": self.name, "unit": self.unit, "to_meter": self.to_meter})
    }

    /// Millimetres per unit of its grid.
    pub fn mm(&self) -> f64 {
        self.to_meter * 1000.0
    }
}

fn def(code: u32) -> Option<crs_definitions::Def> {
    crs_definitions::from_code(u16::try_from(code).ok()?)
}

/// Its name: the first quoted text of its WKT.
fn name_of(d: &crs_definitions::Def) -> String {
    d.wkt.split('"').nth(1).unwrap_or("").to_string()
}

fn unit_of(to_meter: f64) -> String {
    if (to_meter - 1.0).abs() < 1e-12 {
        "metre".into()
    } else if (to_meter - 0.3048).abs() < 1e-12 {
        "foot".into()
    } else if (to_meter - 1200.0 / 3937.0).abs() < 1e-12 {
        "US survey foot".into()
    } else {
        format!("{to_meter} m")
    }
}

/// The projected CRS of EPSG code `code`; else why not.
pub fn lookup(code: u32) -> Result<Info, String> {
    let d = def(code).ok_or_else(|| format!("EPSG:{code} isn't a CRS this version knows"))?;
    let p = Proj::from_proj_string(d.proj4).map_err(|e| format!("EPSG:{code} ({}) can't be projected: {e}", name_of(&d)))?;
    if p.is_latlong() || p.is_geocent() {
        return Err(format!(
            "EPSG:{code} ({}) is geographic, not projected: shared coordinates need a map grid of eastings and northings, as a UTM zone's",
            name_of(&d)
        ));
    }
    Ok(Info { code, name: name_of(&d), unit: unit_of(p.to_meter()), to_meter: p.to_meter() })
}

/// The projected CRSs whose code is `text`, or whose name has every word of it (case and punctuation aside), up to `limit`.
pub fn search(text: &str, limit: usize) -> Vec<Info> {
    let fold = |s: &str| -> String { s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' }).collect() };
    let text = fold(text);
    let words: Vec<&str> = text.split_whitespace().collect();
    let code: Option<u32> = match words.as_slice() {
        [w] => w.strip_prefix("epsg").unwrap_or(w).parse().ok(),
        ["epsg", w] => w.parse().ok(),
        _ => None,
    };
    let mut out: Vec<Info> = code.and_then(|c| lookup(c).ok()).into_iter().collect();
    if words.is_empty() {
        return out;
    }
    for c in 0..=u16::MAX {
        if out.len() >= limit {
            break;
        }
        let Some(d) = crs_definitions::from_code(c) else { continue };
        if Some(u32::from(c)) == code || !d.wkt.starts_with("PROJCS") {
            continue;
        }
        let name = fold(&name_of(&d));
        let has = |w: &&str| name.split_whitespace().any(|n| n == *w || (w.len() > 2 && n.starts_with(*w)));
        if words.iter().all(has)
            && let Ok(info) = lookup(u32::from(c))
        {
            out.push(info);
        }
    }
    out.truncate(limit);
    out
}

fn projs(code: u32) -> Result<(Proj, Proj), String> {
    let d = def(code).ok_or_else(|| format!("EPSG:{code} isn't a CRS this version knows"))?;
    let grid = Proj::from_proj_string(d.proj4).map_err(|e| format!("EPSG:{code}: {e}"))?;
    let geo = Proj::from_proj_string(WGS84).map_err(|e| format!("WGS84: {e}"))?;
    Ok((grid, geo))
}

/// Grid point `e`, `n` (mm) of CRS `code` in WGS84: longitude and latitude, degrees.
pub fn to_wgs84(code: u32, e: f64, n: f64) -> Result<(f64, f64), String> {
    let info = lookup(code)?;
    let (grid, geo) = projs(code)?;
    let mut p = (e / info.mm(), n / info.mm(), 0.0);
    proj4rs::transform::transform(&grid, &geo, &mut p).map_err(|err| format!("EPSG:{code}: {err}"))?;
    let (lon, lat) = (p.0.to_degrees(), p.1.to_degrees());
    if !(lon.is_finite() && lat.is_finite() && lat.abs() <= 90.0) {
        return Err(format!("EPSG:{code}: [{e}, {n}] mm is off its map"));
    }
    Ok((lon, lat))
}

/// WGS84 `lon`, `lat` (degrees) on the grid of CRS `code`, mm.
pub fn from_wgs84(code: u32, lon: f64, lat: f64) -> Result<(f64, f64), String> {
    if !(lat.is_finite() && lat.abs() <= 90.0) {
        return Err(format!("the latitude must be degrees from -90 to 90, not {lat}"));
    }
    if !(lon.is_finite() && lon.abs() <= 180.0) {
        return Err(format!("the longitude must be degrees from -180 to 180, not {lon}"));
    }
    let info = lookup(code)?;
    let (grid, geo) = projs(code)?;
    let mut p = (lon.to_radians(), lat.to_radians(), 0.0);
    proj4rs::transform::transform(&geo, &grid, &mut p).map_err(|err| format!("EPSG:{code}: {err}"))?;
    let (e, n) = (p.0 * info.mm(), p.1 * info.mm());
    if !(e.is_finite() && n.is_finite()) {
        return Err(format!("EPSG:{code}: {lon}, {lat} is off its map"));
    }
    Ok((e, n))
}
