//! Coordinates: where the model stands in the world ([the area](../../../docs/areas/coordinates.md)).
//!
//! Geometry is stored in the file's own internal coordinates, near their origin, and every number shown, typed, imported or
//! exported in shared coordinates goes through the active site's transform: the bias that keeps far map coordinates (a UTM
//! northing is 7.5e9 mm) out of the geometry's math.
//!
//! - The **site** the project is at is its georeference (`Model::georeference`, IFC's map conversion): where the internal
//!   origin is on the map, how far the project's axes are turned from the map's and the grid's scale. None is the internal
//!   coordinates as they are. A project may keep other **named sites** beside it (Revit's), each where it would put the
//!   project; activating one swaps it in as the georeference.
//! - The **Project Base Point** is a reference people choose: elevations and spot coordinates measure from it, and links can
//!   meet at it. Moving it moves no geometry.
//! - The **Survey Point** is a control point, usually a benchmark near the building. A site's **location** is the Survey
//!   Point's shared east, north and elevation in it, with the site's angle and scale: what Revit shows and people type. Moving
//!   the point alone ([`Move::Point`]) changes only it, since every site is anchored at the internal origin; moving it with
//!   the shared system ([`Move::System`]) keeps its numbers, so every site is solved again and the building moves in the
//!   world.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::{Georeference, MAX_COORD, MAX_MAP, MAX_TEXT, Model};

/// The name of the site a project starts with: the internal coordinates, unmoved.
pub const INTERNAL: &str = bimcraft_i18n::msg!("Internal");
/// The three points' names, as the library lists them.
pub const BASE_POINT: &str = "Project Base Point";
pub const SURVEY_POINT: &str = "Survey Point";
pub const INTERNAL_ORIGIN: &str = "Internal Origin";
/// How many sites a project holds.
pub const MAX_LOCATIONS: usize = 1000;

/// A point, mm: x, y and z.
pub type P3 = [f64; 3];

/// The site of a project with no georeference: the internal coordinates as they are.
pub static NONE: Georeference = Georeference {
    crs: String::new(),
    description: None,
    geodetic_datum: None,
    vertical_datum: None,
    map_projection: None,
    map_zone: None,
    eastings: 0.0,
    northings: 0.0,
    height: 0.0,
    rotation: 0.0,
    scale: 1.0,
};

/// One of the three points a project is set out from, as a view shows and picks it: by its library entry
/// (`library::Kind::CoordinatePoint`), whose id the selection holds, as an element's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Point {
    /// The file's own 0, 0, 0: shown, never moved.
    Internal,
    Base,
    Survey,
}

impl Point {
    pub const ALL: [Point; 3] = [Point::Internal, Point::Base, Point::Survey];

    /// Its name, as the library and the interface call it.
    pub fn name(self) -> &'static str {
        match self {
            Point::Internal => INTERNAL_ORIGIN,
            Point::Base => BASE_POINT,
            Point::Survey => SURVEY_POINT,
        }
    }

    /// Its subcategory of Site, in Visibility/Graphics.
    pub fn subcategory(self) -> &'static str {
        match self {
            Point::Internal => "internalorigin",
            Point::Base => "projectbasepoint",
            Point::Survey => "surveypoint",
        }
    }

    /// Where it is, internal mm.
    pub fn at(self, co: &Coordinates) -> P3 {
        match self {
            Point::Internal => [0.0; 3],
            Point::Base => co.base,
            Point::Survey => co.survey,
        }
    }

    /// Its library id.
    pub fn id(self, m: &Model) -> Option<crate::model::Id> {
        m.library.id(&crate::library::Kind::CoordinatePoint, self.name())
    }

    /// The point library id `id` is, if it is one.
    pub fn of(m: &Model, id: crate::model::Id) -> Option<Point> {
        let e = m.library.get(id).filter(|e| e.kind == crate::library::Kind::CoordinatePoint)?;
        Point::ALL.into_iter().find(|p| p.name() == e.name)
    }
}

/// A site as Revit shows it: the Survey Point's shared coordinates in it, mm, the angle from project north to true north
/// (degrees, clockwise, so east, positive: the georeference's `rotation`) and the scale from ground to grid in plan (heights
/// are never scaled).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Location {
    pub east: f64,
    pub north: f64,
    pub elevation: f64,
    pub angle: f64,
    pub scale: f64,
}

impl Georeference {
    /// Project north's turn to the map's axes: (cos, sin) of the rotation.
    fn turn(&self) -> (f64, f64) {
        let a = self.rotation.to_radians();
        (a.cos(), a.sin())
    }

    /// `p`, internal, in this site's shared coordinates: on the map, the grid's scale in plan only.
    pub fn to_shared(&self, p: P3) -> P3 {
        let (c, s) = self.turn();
        [self.eastings + self.scale * (p[0] * c - p[1] * s), self.northings + self.scale * (p[0] * s + p[1] * c), self.height + p[2]]
    }

    /// `q`, shared, in internal coordinates: [`Georeference::to_shared`] undone.
    pub fn to_internal(&self, q: P3) -> P3 {
        let (c, s) = self.turn();
        let (e, n) = ((q[0] - self.eastings) / self.scale, (q[1] - self.northings) / self.scale);
        [e * c + n * s, -e * s + n * c, q[2] - self.height]
    }

    /// Solved so that internal point `at` has shared coordinates `q`, its rotation and scale kept.
    pub fn specify(&mut self, at: P3, q: P3) {
        let now = Georeference { eastings: 0.0, northings: 0.0, height: 0.0, ..self.clone() }.to_shared(at);
        (self.eastings, self.northings, self.height) = (q[0] - now[0], q[1] - now[1], q[2] - now[2]);
    }

    /// Solved, keeping its rotation and scale, so that the internal origin stands at shared point `q`, each coordinate rounded
    /// to the metre: what a drawing or a scan far off acquires, so it lands near the origin and the shown numbers stay round.
    pub fn acquire(&mut self, q: P3) {
        let metre = |x: f64| (x / 1000.0).round() * 1000.0;
        self.specify([0.0; 3], [metre(q[0]), metre(q[1]), metre(q[2])]);
    }

    /// Its map system, where its `crs` names an EPSG code of a projected system this version knows: what WGS84, a map unit
    /// other than the metre, and a place search need.
    pub fn map(&self) -> Option<crate::crs::Info> {
        crate::crs::code(&self.crs).and_then(|c| crate::crs::lookup(c).ok())
    }

    /// It as Revit shows it, with the Survey Point at `survey`.
    pub fn location(&self, survey: P3) -> Location {
        let [east, north, elevation] = self.to_shared(survey);
        Location { east, north, elevation, angle: self.rotation, scale: self.scale }
    }
}

/// The project's two points and its other named sites.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coordinates {
    /// The Project Base Point, internal mm.
    #[serde(default)]
    pub base: P3,
    /// The Survey Point, internal mm.
    #[serde(default)]
    pub survey: P3,
    /// The active site's name: where `Model::georeference` puts the project.
    #[serde(default = "internal")]
    pub active: String,
    /// The other named sites, by name: each where it would put the project, as the georeference does.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sites: BTreeMap<String, Georeference>,
}

fn internal() -> String {
    INTERNAL.to_string()
}

impl Default for Coordinates {
    fn default() -> Coordinates {
        Coordinates::named(INTERNAL)
    }
}

/// What an elevation or a spot coordinate measures from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// The Project Base Point, along the project's axes.
    #[default]
    Base,
    /// The Survey Point: the active site's shared coordinates.
    Survey,
    /// The Internal Origin: the file's own coordinates.
    Internal,
}

impl Origin {
    pub const NAMES: &str = "base (the Project Base Point), survey (the shared coordinates) or internal (the Internal Origin)";
    pub const ALL: [Origin; 3] = [Origin::Base, Origin::Survey, Origin::Internal];

    pub fn parse(s: &str) -> Option<Origin> {
        match s {
            "base" => Some(Origin::Base),
            "survey" | "shared" => Some(Origin::Survey),
            "internal" => Some(Origin::Internal),
            _ => None,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Origin::Base
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
    /// The site's shared coordinates.
    Shared,
    /// WGS84 through the site's map system: longitude and latitude, degrees, and the elevation, mm.
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

/// The project's points read through one of its sites.
#[derive(Clone, Copy, Debug)]
pub struct Frames<'a> {
    pub co: &'a Coordinates,
    pub site: &'a Georeference,
}

impl Frames<'_> {
    /// `p`, internal, in the site's shared coordinates.
    pub fn to_shared(&self, p: P3) -> P3 {
        self.site.to_shared(p)
    }

    /// `q`, shared, in internal coordinates.
    pub fn to_internal(&self, q: P3) -> P3 {
        self.site.to_internal(q)
    }

    /// The site as Revit shows it: at the Survey Point.
    pub fn location(&self) -> Location {
        self.site.location(self.co.survey)
    }

    /// Internal height `z` as measured from `from`: the site's heights are never scaled.
    pub fn elevation(&self, z: f64, from: Origin) -> f64 {
        match from {
            Origin::Base => z - self.co.base[2],
            Origin::Survey => self.to_shared([0.0, 0.0, z])[2],
            Origin::Internal => z,
        }
    }

    /// Internal plan point `x`, `y` as measured from `from`: east and north.
    pub fn plan(&self, x: f64, y: f64, from: Origin) -> (f64, f64) {
        match from {
            Origin::Base => (x - self.co.base[0], y - self.co.base[1]),
            Origin::Internal => (x, y),
            Origin::Survey => {
                let q = self.to_shared([x, y, 0.0]);
                (q[0], q[1])
            }
        }
    }

    /// The EPSG code WGS84 goes through; else what to do.
    fn code(&self) -> Result<u32, String> {
        self.site.map().map(|i| i.code).ok_or_else(|| {
            "WGS84 needs the site's map system: name a projected EPSG code with `georeference` (`crs`: EPSG:31983)".to_string()
        })
    }

    /// `p` from frame `from` to frame `to`; WGS84 needs a map system.
    pub fn convert(&self, p: P3, from: Frame, to: Frame) -> Result<P3, String> {
        let b = self.co.base;
        let internal = match from {
            Frame::Internal => p,
            Frame::Base => [p[0] + b[0], p[1] + b[1], p[2] + b[2]],
            Frame::Shared => self.to_internal(p),
            Frame::Wgs84 => {
                let (e, n) = crate::crs::from_wgs84(self.code()?, p[0], p[1])?;
                self.to_internal([e, n, p[2]])
            }
        };
        Ok(match to {
            Frame::Internal => internal,
            Frame::Base => [internal[0] - b[0], internal[1] - b[1], internal[2] - b[2]],
            Frame::Shared => self.to_shared(internal),
            Frame::Wgs84 => {
                let q = self.to_shared(internal);
                let (lon, lat) = crate::crs::to_wgs84(self.code()?, q[0], q[1])?;
                [lon, lat, q[2]]
            }
        })
    }
}

impl Coordinates {
    /// Both points at the origin, and the active site of that name.
    pub fn named(name: &str) -> Coordinates {
        Coordinates { base: [0.0; 3], survey: [0.0; 3], active: name.to_string(), sites: BTreeMap::new() }
    }

    pub fn is_default(&self) -> bool {
        *self == Coordinates::default()
    }

    /// The names of its sites: the active one first.
    pub fn names(&self) -> Vec<&str> {
        std::iter::once(&self.active).chain(self.sites.keys()).map(String::as_str).collect()
    }

    /// Whether it is whole and within bounds; else what to fix.
    pub fn check(&self) -> Result<(), String> {
        for (name, p) in [(BASE_POINT, self.base), (SURVEY_POINT, self.survey)] {
            if !p.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD) {
                return Err(format!("the {name} must stand within ±{MAX_COORD:e} mm of the internal origin, not at {p:?}"));
            }
        }
        if self.sites.len() >= MAX_LOCATIONS {
            return Err(format!("the project has {} sites; it holds up to {MAX_LOCATIONS}", self.sites.len() + 1));
        }
        for name in self.sites.keys().chain([&self.active]) {
            if name.trim().is_empty() || name.chars().count() > MAX_TEXT || name.contains(['\n', '\r']) {
                return Err(format!("a site's name is one line of 1 to {MAX_TEXT} characters, not `{name}`"));
            }
        }
        if self.sites.contains_key(&self.active) {
            return Err(format!("the active site `{}` is listed again among the others", self.active));
        }
        for (name, g) in &self.sites {
            g.check().map_err(|e| format!("site `{name}`: {e}"))?;
        }
        Ok(())
    }
}

impl Model {
    /// The active site: the georeference, or the internal coordinates as they are.
    pub fn site(&self) -> &Georeference {
        self.georeference.as_ref().unwrap_or(&NONE)
    }

    /// The site of that name, the active one or another.
    pub fn site_named(&self, name: &str) -> Option<&Georeference> {
        if name == self.coordinates.active { Some(self.site()) } else { self.coordinates.sites.get(name) }
    }

    /// The points read through the active site.
    pub fn frames(&self) -> Frames<'_> {
        Frames { co: &self.coordinates, site: self.site() }
    }

    /// The active site set to `g`: none where it is the internal coordinates as they are.
    pub fn set_site(&mut self, g: Georeference) {
        self.georeference = (g != NONE).then_some(g);
    }

    /// Site `name` made the active one, the georeference, and the one that was kept beside it. False when there is none of
    /// that name.
    pub fn activate_site(&mut self, name: &str) -> bool {
        if name == self.coordinates.active {
            return true;
        }
        let Some(g) = self.coordinates.sites.remove(name) else { return false };
        let was = self.georeference.take().unwrap_or_else(|| NONE.clone());
        let co = &mut self.coordinates;
        co.sites.insert(std::mem::replace(&mut co.active, name.to_string()), was);
        self.set_site(g);
        true
    }

    /// The Survey Point moved to `to`, as `how` says: alone, every site stays; with the shared system, every site is solved so
    /// that the point keeps its numbers in it.
    pub fn move_survey(&mut self, to: P3, how: Move) {
        let from = self.coordinates.survey;
        if how == Move::System {
            let mut g = self.site().clone();
            g.specify(to, g.to_shared(from));
            self.set_site(g);
            for g in self.coordinates.sites.values_mut() {
                g.specify(to, g.to_shared(from));
            }
        }
        self.coordinates.survey = to;
    }

    /// Everything about the coordinates, as the `coordinates` command reports it: the three points, internal and shared, the
    /// active site, every site as Revit shows it, and the georeference; each point and site with its id and GUID.
    pub fn coordinates_report(&self) -> Value {
        use crate::library::Kind;
        let ids = |kind: &Kind, name: &str| (self.library.id(kind, name), self.library.guid(kind, name));
        let f = self.frames();
        let point = |p: Point| {
            let at = p.at(&self.coordinates);
            let (id, guid) = ids(&Kind::CoordinatePoint, p.name());
            let mut v = json!({"internal": at, "shared": f.to_shared(at), "id": id, "guid": guid});
            if let Ok(w) = f.convert(at, Frame::Internal, Frame::Wgs84) {
                v["wgs84"] = json!(w);
            }
            v
        };
        let sites = std::iter::once((&self.coordinates.active, self.site())).chain(&self.coordinates.sites);
        let locations: serde_json::Map<String, Value> = sites
            .map(|(name, g)| {
                let mut v = json!(g.location(self.coordinates.survey));
                if !g.crs.is_empty() {
                    v["crs"] = json!(g.crs);
                }
                let (id, guid) = ids(&Kind::Location, name);
                (v["id"], v["guid"]) = (json!(id), json!(guid));
                (name.clone(), v)
            })
            .collect();
        json!({
            "internal_origin": point(Point::Internal),
            "base": point(Point::Base),
            "survey": point(Point::Survey),
            "active": self.coordinates.active,
            "location": f.location(),
            "locations": locations,
            "georeference": self.georeference,
            "crs": self.site().map().map(|i| i.json()),
        })
    }
}

/// Whether shared coordinate `x` is one a map holds, mm.
pub fn on_map(x: f64) -> bool {
    x.is_finite() && x.abs() <= MAX_MAP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: P3, b: P3) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    fn site() -> Georeference {
        Georeference {
            eastings: 333_000_000.0,
            northings: 7_400_000_000.0,
            height: 760_000.0,
            rotation: 17.3,
            scale: 0.99961,
            ..NONE.clone()
        }
    }

    #[test]
    fn to_shared_and_back_is_the_same_point() {
        let g = site();
        for p in [[0.0, 0.0, 0.0], [12_345.6, -9_876.5, 3000.0], [-1e6, 1e6, -2e3]] {
            let q = g.to_shared(p);
            assert!(near(g.to_internal(q), p), "{p:?} → {q:?}");
        }
    }

    #[test]
    fn the_georeference_is_the_internal_origins_map_point_and_a_location_the_survey_points() {
        let g = site();
        assert!(near(g.to_shared([0.0; 3]), [g.eastings, g.northings, g.height]));
        let survey = [1200.0, -340.0, 15.0];
        let l = g.location(survey);
        assert!(near([l.east, l.north, l.elevation], g.to_shared(survey)));
        assert_eq!((l.angle, l.scale), (g.rotation, g.scale));
    }

    #[test]
    fn specify_puts_a_point_where_it_is_asked_and_keeps_the_turn() {
        let mut g = site();
        g.specify([4000.0, 2000.0, 300.0], [1.0, 2.0, 3.0]);
        assert!(near(g.to_shared([4000.0, 2000.0, 300.0]), [1.0, 2.0, 3.0]));
        assert_eq!((g.rotation, g.scale), (17.3, 0.99961));
    }
}
