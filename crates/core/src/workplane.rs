//! Work planes: the plane a 3D view sketches on, as Revit's Work Plane > Set, and more than Revit's: a level, a surface of
//! the model (a wall's face, a roof's slope), or the plane through three points, each moved off by an offset along its
//! normal. A 3D view keeps its own (`View3D::work_plane`, set by the `work_plane` command); without one it sketches on the
//! lowest level, as Revit's 3D view does.
//!
//! A work plane may lie at any angle, upright too, for what is sketched on a wall (an opening, later). What is set on it is
//! up to the tool: floors, ceilings, roofs and shafts need a level one ([`is_level`]), and stand on the level below it
//! ([`host`]).

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::model::{Guid, Id, MAX_COORD, Model, TOL};
use crate::view3d::{P3, add, cross, dot, mul, sub, unit};
use crate::walls::Plane;

/// A 3D view's work plane: on `level`, through three `points`, or on a `surface` picked on the model; `offset` mm off it,
/// along its normal (up, for a level).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkPlane {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Guid>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<[[f64; 3]; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Surface>,
}

/// A surface of the model a work plane was set on: a point of it and the way it faces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// A plane in space: a point of it, and its normal, a unit vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: P3,
    pub normal: P3,
}

impl Frame {
    /// The level plane at height `z`.
    pub fn flat(z: f64) -> Frame {
        Frame { origin: [0.0, 0.0, z], normal: [0.0, 0.0, 1.0] }
    }

    /// How steep it is, degrees from level: 0 lying level, 90 upright.
    pub fn slope(&self) -> f64 {
        self.normal[2].abs().clamp(0.0, 1.0).acos().to_degrees()
    }

    /// As a plane over the plan, as a sketch is drawn on: none for an upright one.
    pub fn over_plan(&self) -> Option<Plane> {
        let [nx, ny, nz] = self.normal;
        (nz.abs() > 1e-9).then(|| {
            let g = P2(-nx / nz, -ny / nz);
            Plane { z: self.origin[2] - g.dot(P2(self.origin[0], self.origin[1])), g }
        })
    }

    /// Two unit vectors across it: across is level (east on a level plane), up goes up it (north on a level one).
    pub fn axes(&self) -> (P3, P3) {
        let level = cross([0.0, 0.0, 1.0], self.normal);
        let across = if dot(level, level) < 1e-12 { [1.0, 0.0, 0.0] } else { unit(level) };
        (across, unit(cross(self.normal, across)))
    }

    /// The point of it nearest `p`.
    pub fn nearest(&self, p: P3) -> P3 {
        sub(p, mul(self.normal, dot(sub(p, self.origin), self.normal)))
    }

    /// Moved `by` mm along its normal.
    pub fn moved(self, by: f64) -> Frame {
        Frame { origin: add(self.origin, mul(self.normal, by)), ..self }
    }
}

impl From<Plane> for Frame {
    /// A plane over the plan, as a sloped floor's top is, as a plane in space.
    fn from(p: Plane) -> Frame {
        Frame { origin: [0.0, 0.0, p.z], normal: unit([-p.g.0, -p.g.1, 1.0]) }
    }
}

impl WorkPlane {
    /// On level `level`, `offset` mm above it.
    pub fn on(level: Guid, offset: f64) -> WorkPlane {
        WorkPlane { level: Some(level), offset, ..WorkPlane::default() }
    }

    /// The plane it is in `m`, or what's wrong with it, said so as to be fixed.
    pub fn resolve(&self, m: &Model) -> Result<Frame, String> {
        if !self.offset.is_finite() || self.offset.abs() > MAX_COORD {
            return Err(format!("its offset {} isn't a length", self.offset));
        }
        let plane = match (&self.level, &self.points, &self.surface) {
            (Some(level), None, None) => {
                if m.elements.get(*level).is_none() {
                    return Err("its level isn't in the project".into());
                }
                Frame::flat(m.elevation(*level))
            }
            (None, Some(points), None) => through(points)?,
            (None, None, Some(s)) => on_surface(s)?,
            (None, None, None) => return Err("a work plane needs a `level`, three `points` or a `surface`".into()),
            _ => return Err("a work plane is on a `level`, through three `points` or on a `surface`: only one of them".into()),
        };
        Ok(plane.moved(self.offset))
    }

    /// How steep it is, degrees from level: 0 for one on a level.
    pub fn slope(&self, m: &Model) -> f64 {
        self.resolve(m).map_or(0.0, |p| p.slope())
    }
}

/// Whether plane `p` lies level: what a floor, a ceiling, a roof or a shaft can be sketched on.
pub fn is_level(p: &Frame) -> bool {
    p.normal[0].abs() < 1e-9 && p.normal[1].abs() < 1e-9
}

fn place(p: &[f64; 3]) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

/// The plane through three points, facing up where it isn't upright: refused when they lie along a line.
pub fn through(points: &[[f64; 3]; 3]) -> Result<Frame, String> {
    if !points.iter().all(place) {
        return Err("a point of the work plane isn't a place in the project".into());
    }
    let [a, b, c] = *points;
    let (u, v) = (sub(b, a), sub(c, a));
    let n = cross(u, v);
    let size = dot(n, n).sqrt();
    let len = |d: P3| dot(d, d).sqrt();
    let longest = len(u).max(len(v)).max(len(sub(c, b)));
    // A triangle of no area, or one less than a millimetre high: they lie along a line.
    if size.is_nan() || size <= TOL * longest.max(1.0) {
        return Err("the work plane's three points lie along a line: pick three that make a triangle".into());
    }
    let n = mul(n, 1.0 / size);
    let n = if n[2] < -1e-9 { mul(n, -1.0) } else { n };
    Ok(Frame { origin: a, normal: n })
}

/// The plane of a surface picked on the model.
fn on_surface(s: &Surface) -> Result<Frame, String> {
    if !place(&s.point) {
        return Err("the work plane's surface point isn't a place in the project".into());
    }
    let size = dot(s.normal, s.normal).sqrt();
    if !(size.is_finite() && size > 1e-9) {
        return Err("the work plane's surface `normal` is the way it faces, [x, y, z], and can't be [0, 0, 0]".into());
    }
    Ok(Frame { origin: s.point, normal: mul(s.normal, 1.0 / size) })
}

/// The level what's made at `at` on plane `p` stands on, and how far above it it is there: the highest level at or below
/// the plane, else the lowest.
pub fn host(m: &Model, p: Plane, at: P2) -> Option<(Id, f64)> {
    let z = p.at(at);
    let levels = m.levels();
    let (id, l) = levels.iter().rev().find(|(_, l)| l.elevation <= z + TOL).or(levels.first())?;
    z.is_finite().then_some((*id, z - l.elevation))
}
