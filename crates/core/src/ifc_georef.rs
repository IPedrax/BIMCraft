//! Reading where an IFC file puts the project on Earth: the site's latitude, longitude and elevation, the context's true north,
//! and `IfcMapConversion` with its `IfcProjectedCRS`. What the file says is hostile: anything missing or odd leaves a part out, and
//! a place the project can't hold is an error the import notes.

use crate::georef::{self, Location};
use crate::step::{Entity, File, V};

fn real(v: Option<&V>) -> Option<f64> {
    match v? {
        V::Real(x) => Some(*x),
        V::Int(i) => Some(*i as f64),
        V::Typed(_, inner) => real(Some(inner)),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

fn text(v: Option<&V>) -> Option<&str> {
    match v? {
        V::Str(s) => Some(s.trim()).filter(|s| !s.is_empty()),
        V::Typed(_, inner) => text(Some(inner)),
        _ => None,
    }
}

fn entity<'a>(f: &'a File, v: Option<&V>) -> Option<&'a Entity> {
    match v? {
        V::Ref(id) => f.entities.get(id),
        _ => None,
    }
}

/// The first entity named `name`, by number.
fn first<'a>(f: &'a File, name: &str) -> Option<&'a Entity> {
    f.entities.iter().filter(|(_, e)| e.name == name).min_by_key(|(id, _)| **id).map(|(_, e)| e)
}

/// An angle as the degrees of an IfcCompoundPlaneAngleMeasure.
fn degrees(v: Option<&V>) -> Option<f64> {
    let V::List(parts) = v? else { return None };
    georef::from_compound(&parts.iter().map(|p| real(Some(p))).collect::<Option<Vec<_>>>()?)
}

/// How many metres a map unit is: none named, a metre.
fn map_unit(f: &File, v: Option<&V>) -> Result<f64, String> {
    let Some(unit) = entity(f, v) else { return Ok(1.0) };
    let prefix = match unit.args.get(2) {
        Some(V::Enum(p)) => Some(p.as_str()),
        _ => None,
    };
    match (unit.name.as_str(), prefix) {
        ("IFCSIUNIT", None) => Ok(1.0),
        ("IFCSIUNIT", Some("MILLI")) => Ok(1e-3),
        ("IFCSIUNIT", Some("CENTI")) => Ok(1e-2),
        ("IFCSIUNIT", Some("KILO")) => Ok(1e3),
        _ => Err("its map unit isn't a metre, millimetre, centimetre or kilometre".into()),
    }
}

/// Where the file puts the project: `mm` is the file's length unit in mm. `Ok(None)` where it says nothing about it.
pub fn read(f: &File, mm: f64) -> Result<Option<Location>, String> {
    let site = f
        .entities
        .iter()
        .filter(|(_, e)| e.name == "IFCSITE")
        .map(|(id, e)| (*id, e))
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .find_map(|e| Some((degrees(e.args.get(9))?, degrees(e.args.get(10))?, real(e.args.get(11)).map_or(0.0, |z| z * mm))));
    let conversion = first(f, "IFCMAPCONVERSION");
    if site.is_none() && conversion.is_none() {
        return Ok(None);
    }

    // The CRS the map conversion targets, by the EPSG code its name gives.
    let target = conversion.and_then(|c| entity(f, c.args.get(1)));
    let epsg = target
        .and_then(|t| text(t.args.first()))
        .and_then(|n| n.to_ascii_uppercase().strip_prefix("EPSG:").and_then(|c| c.trim().parse::<u32>().ok()));
    let metres = match target {
        Some(t) => map_unit(f, t.args.get(6))?,
        None => 1.0,
    };
    let grid_origin = conversion.and_then(|c| {
        Some((real(c.args.get(2))? * metres, real(c.args.get(3))? * metres, real(c.args.get(4)).map_or(0.0, |h| h * metres)))
    });

    let (latitude, longitude, elevation) = match (site, grid_origin, epsg) {
        (Some(position), _, _) => position,
        (None, Some((e, n, h)), Some(code)) => {
            let (lon, lat) = georef::lonlat_of_grid(code, e, n)?;
            (lat, lon, h * 1000.0)
        }
        _ => return Err("it gives no latitude and longitude, and no map conversion to a CRS with an EPSG code to find them from".into()),
    };

    // The CRS: only where it isn't the place's UTM zone.
    let crs = epsg.filter(|code| *code != georef::utm_epsg(longitude, latitude));
    let mut place = Location { latitude, longitude, elevation, true_north: 0.0, crs };
    if let Some(why) = place.problem() {
        return Err(why);
    }
    // True north: from how the map conversion turns the project against the grid, else from the context's direction.
    let turn = conversion.and_then(|c| Some(real(c.args.get(6))?.atan2(real(c.args.get(5))?)));
    place.true_north = match turn {
        Some(theta) if epsg.is_some() => {
            let rotation = place.grid()?.rotation;
            wrap(rotation - theta.to_degrees())
        }
        _ => north_of_context(f).unwrap_or(0.0),
    };
    place.grid()?;
    Ok(Some(place))
}

/// The angle from the project's +y to true north, degrees counter-clockwise, from the model context's TrueNorth direction.
fn north_of_context(f: &File) -> Option<f64> {
    let project = first(f, "IFCPROJECT")?;
    let V::List(contexts) = project.args.get(7)? else { return None };
    let contexts: Vec<&Entity> = contexts.iter().filter_map(|c| entity(f, Some(c))).collect();
    let context = contexts.iter().find(|c| text(c.args.get(1)) == Some("Model")).or(contexts.first())?;
    let direction = entity(f, context.args.get(5))?;
    let V::List(ratios) = direction.args.first()? else { return None };
    let (dx, dy) = (real(ratios.first())?, real(ratios.get(1))?);
    (dx != 0.0 || dy != 0.0).then(|| wrap((-dx).atan2(dy).to_degrees()))
}

/// An angle in degrees, in (-180, 180].
fn wrap(degrees: f64) -> f64 {
    let d = degrees.rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}
