//! Coordinates: where the model stands in the world ([the plan](../../../docs/coordinates-plan.md)).
//!
//! Geometry is stored in the file's own internal coordinates, near their origin, and every number shown, typed, imported or
//! exported in shared coordinates goes through the active location's transform: the bias that keeps far map coordinates
//! (a UTM northing is 7.5e9 mm) out of the geometry's math.
//!
//! - The **Project Base Point** is a reference people choose: elevations and spot coordinates measure from it, and links can
//!   meet at it. Moving it moves no geometry.
//! - The **Survey Point** is a control point, usually a benchmark near the building, whose shared coordinates each location
//!   gives. There is no hidden shared origin besides it, and no clipped or unclipped state: moving it either keeps the
//!   building where it is in the world ([`Move::Point`]) or carries the shared system with it ([`Move::System`]).
//! - A **location** (Revit's named site) is one position of the building in the shared system: the Survey Point's east,
//!   north and elevation, the angle from project north to true north, and the scale from the model's ground distances to
//!   the map's grid ones. A project has at least one, and one is active.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::MAX_COORD;

/// The name of the location a project starts with: the internal coordinates, unmoved.
pub const INTERNAL: &str = bimcraft_i18n::msg!("Internal");
/// The two points' names, as the library lists them.
pub const BASE_POINT: &str = "Project Base Point";
pub const SURVEY_POINT: &str = "Survey Point";
/// How far a location's shared coordinates may lie from their origin, mm: 100,000 km, past any map grid's false
/// eastings and northings.
pub const MAX_SHARED: f64 = 1e11;
/// How many locations a project holds.
pub const MAX_LOCATIONS: usize = 1000;
/// The scale from ground to grid it may have: a map projection's is within a few parts in a thousand of 1.
pub const SCALE: std::ops::RangeInclusive<f64> = 0.9..=1.1;

/// A point, mm: x, y and z.
pub type P3 = [f64; 3];

/// One position of the building in the shared system.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    /// The Survey Point's shared coordinates, mm.
    #[serde(default)]
    pub east: f64,
    #[serde(default)]
    pub north: f64,
    #[serde(default)]
    pub elevation: f64,
    /// From project north to true north, degrees, clockwise (east) positive: at 30, true north is 30 degrees east of the
    /// top of a plan drawn to project north.
    #[serde(default)]
    pub angle: f64,
    /// Grid distance over ground distance, in plan; heights are not scaled.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f64,
}

fn one() -> f64 {
    1.0
}

fn is_one(x: &f64) -> bool {
    *x == 1.0
}

impl Default for Location {
    fn default() -> Location {
        Location { east: 0.0, north: 0.0, elevation: 0.0, angle: 0.0, scale: 1.0 }
    }
}

impl Location {
    /// Whether its numbers are within bounds; else what to fix.
    pub fn check(&self, name: &str) -> Result<(), String> {
        for (key, v) in [("east", self.east), ("north", self.north), ("elevation", self.elevation)] {
            if !(v.is_finite() && v.abs() <= MAX_SHARED) {
                return Err(format!("location `{name}`: {key} must be a number of mm within ±{MAX_SHARED:e}, not {v}"));
            }
        }
        if !(self.angle.is_finite() && self.angle.abs() <= 360.0) {
            return Err(format!("location `{name}`: the angle to true north must be degrees from -360 to 360, not {}", self.angle));
        }
        if !(self.scale.is_finite() && SCALE.contains(&self.scale)) {
            return Err(format!(
                "location `{name}`: the scale (grid over ground) must be from {} to {}, not {}",
                SCALE.start(),
                SCALE.end(),
                self.scale
            ));
        }
        Ok(())
    }

    /// Project north's turn to the map's axes: (cos, sin) of the angle.
    fn turn(&self) -> (f64, f64) {
        let a = self.angle.to_radians();
        (a.cos(), a.sin())
    }

    /// `p`, internal, in shared coordinates, with the Survey Point at `survey`.
    pub fn to_shared(&self, survey: P3, p: P3) -> P3 {
        let (c, s) = self.turn();
        let (dx, dy, dz) = (p[0] - survey[0], p[1] - survey[1], p[2] - survey[2]);
        // True north turned clockwise from project north: a step up the plan goes east by sin, north by cos.
        [self.east + self.scale * (dx * c - dy * s), self.north + self.scale * (dx * s + dy * c), self.elevation + dz]
    }

    /// `q`, shared, in internal coordinates, with the Survey Point at `survey`: [`Location::to_shared`] undone.
    pub fn to_internal(&self, survey: P3, q: P3) -> P3 {
        let (c, s) = self.turn();
        let (e, n) = ((q[0] - self.east) / self.scale, (q[1] - self.north) / self.scale);
        [survey[0] + e * c + n * s, survey[1] - e * s + n * c, survey[2] + q[2] - self.elevation]
    }
}

/// The project's coordinates: its two points and its locations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coordinates {
    /// The Project Base Point, internal mm.
    #[serde(default)]
    pub base: P3,
    /// The Survey Point, internal mm.
    #[serde(default)]
    pub survey: P3,
    /// The location the shared coordinates are now in.
    pub active: String,
    /// Its locations, by name.
    pub locations: BTreeMap<String, Location>,
    /// The map's coordinate reference system the shared coordinates are in, by EPSG code: none, a local grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<u32>,
}

impl Default for Coordinates {
    fn default() -> Coordinates {
        Coordinates::named(INTERNAL)
    }
}

/// How the Survey Point moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// The point alone: the building stays where it is in the world, and the point's shared coordinates change.
    Point,
    /// The shared system with it: the point keeps its shared coordinates, and the building moves in the world.
    System,
}

/// The frames a point can be given in or asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// The file's own coordinates, where geometry is.
    Internal,
    /// From the Project Base Point, along the project's axes.
    Base,
    /// The active location's shared coordinates.
    Shared,
    /// WGS84 through the CRS: longitude and latitude, degrees, and the elevation, mm.
    Wgs84,
}

impl Frame {
    pub const NAMES: &str = "internal, base, shared, wgs84";

    pub fn parse(s: &str) -> Option<Frame> {
        match s {
            "internal" => Some(Frame::Internal),
            "base" => Some(Frame::Base),
            "shared" | "survey" => Some(Frame::Shared),
            "wgs84" => Some(Frame::Wgs84),
            _ => None,
        }
    }
}

impl Coordinates {
    /// Both points at the origin, and one location of that name at zero.
    pub fn named(name: &str) -> Coordinates {
        Coordinates {
            base: [0.0; 3],
            survey: [0.0; 3],
            active: name.to_string(),
            locations: BTreeMap::from([(name.to_string(), Location::default())]),
            crs: None,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Coordinates::default()
    }

    /// The active location: the first, should the name be missing (which [`Coordinates::check`] refuses).
    pub fn location(&self) -> Location {
        self.locations.get(&self.active).or_else(|| self.locations.values().next()).cloned().unwrap_or_default()
    }

    /// `p`, internal, in the active location's shared coordinates.
    pub fn to_shared(&self, p: P3) -> P3 {
        self.location().to_shared(self.survey, p)
    }

    /// `q`, shared, in internal coordinates.
    pub fn to_internal(&self, q: P3) -> P3 {
        self.location().to_internal(self.survey, q)
    }

    /// `p` from frame `from` to frame `to`; WGS84 needs a CRS.
    pub fn convert(&self, p: P3, from: Frame, to: Frame) -> Result<P3, String> {
        let crs = || self.crs.ok_or("WGS84 needs the project's map grid: set one with `crs`");
        let internal = match from {
            Frame::Internal => p,
            Frame::Base => [p[0] + self.base[0], p[1] + self.base[1], p[2] + self.base[2]],
            Frame::Shared => self.to_internal(p),
            Frame::Wgs84 => {
                let (e, n) = crate::crs::from_wgs84(crs()?, p[0], p[1])?;
                self.to_internal([e, n, p[2]])
            }
        };
        Ok(match to {
            Frame::Internal => internal,
            Frame::Base => [internal[0] - self.base[0], internal[1] - self.base[1], internal[2] - self.base[2]],
            Frame::Shared => self.to_shared(internal),
            Frame::Wgs84 => {
                let q = self.to_shared(internal);
                let (lon, lat) = crate::crs::to_wgs84(crs()?, q[0], q[1])?;
                [lon, lat, q[2]]
            }
        })
    }

    /// The Survey Point moved to `to`, as `how` says.
    pub fn move_survey(&mut self, to: P3, how: Move) {
        if how == Move::Point {
            let from = self.survey;
            for l in self.locations.values_mut() {
                let at = l.to_shared(from, to);
                (l.east, l.north, l.elevation) = (at[0], at[1], at[2]);
            }
        }
        self.survey = to;
    }

    /// The active location solved so that internal point `at` has shared coordinates `shared`; turned to `angle` first, when
    /// given.
    pub fn specify(&mut self, at: P3, shared: P3, angle: Option<f64>) {
        let survey = self.survey;
        let Some(l) = self.locations.get_mut(&self.active) else { return };
        if let Some(a) = angle {
            l.angle = a;
        }
        let now = Location { east: 0.0, north: 0.0, elevation: 0.0, ..l.clone() }.to_shared(survey, at);
        (l.east, l.north, l.elevation) = (shared[0] - now[0], shared[1] - now[1], shared[2] - now[2]);
    }

    /// Whether it is whole and within bounds; else what to fix.
    pub fn check(&self) -> Result<(), String> {
        for (name, p) in [(BASE_POINT, self.base), (SURVEY_POINT, self.survey)] {
            if !p.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD) {
                return Err(format!("the {name} must stand within ±{MAX_COORD:e} mm of the internal origin, not at {p:?}"));
            }
        }
        if self.locations.is_empty() {
            return Err("the project needs a location".into());
        }
        if self.locations.len() > MAX_LOCATIONS {
            return Err(format!("the project has {} locations; it holds up to {MAX_LOCATIONS}", self.locations.len()));
        }
        if !self.locations.contains_key(&self.active) {
            let names: Vec<&str> = self.locations.keys().map(String::as_str).collect();
            return Err(format!("the active location `{}` isn't one of the project's: {}", self.active, names.join(", ")));
        }
        for (name, l) in &self.locations {
            if name.trim().is_empty() {
                return Err("a location needs a name".into());
            }
            l.check(name)?;
        }
        if let Some(code) = self.crs {
            crate::crs::lookup(code)?;
        }
        Ok(())
    }

    /// Everything about it, as the `coordinates` command reports it.
    pub fn report(&self) -> serde_json::Value {
        let point = |p: P3| {
            let mut v = serde_json::json!({"internal": p, "shared": self.to_shared(p)});
            if let Ok(w) = self.convert(p, Frame::Internal, Frame::Wgs84) {
                v["wgs84"] = serde_json::json!(w);
            }
            v
        };
        serde_json::json!({
            "internal_origin": point([0.0; 3]),
            "base": point(self.base),
            "survey": point(self.survey),
            "active": self.active,
            "location": self.location(),
            "locations": self.locations,
            "crs": self.crs.and_then(|c| crate::crs::lookup(c).ok()).map(|i| i.json()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: P3, b: P3) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn to_shared_and_back_is_the_same_point() {
        let l = Location { east: 333_000_000.0, north: 7_400_000_000.0, elevation: 760_000.0, angle: 17.3, scale: 0.99961 };
        let survey = [1200.0, -340.0, 15.0];
        for p in [[0.0, 0.0, 0.0], [12_345.6, -9_876.5, 3000.0], [-1e6, 1e6, -2e3]] {
            let q = l.to_shared(survey, p);
            assert!(near(l.to_internal(survey, q), p), "{p:?} → {q:?}");
        }
    }

    #[test]
    fn the_survey_point_has_its_locations_coordinates() {
        let l = Location { east: 5.0, north: 6.0, elevation: 7.0, angle: 45.0, scale: 1.0 };
        assert_eq!(l.to_shared([10.0, 20.0, 30.0], [10.0, 20.0, 30.0]), [5.0, 6.0, 7.0]);
    }
}
