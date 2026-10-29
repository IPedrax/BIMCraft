//! Work planes: the plane a 3D view sketches on, as Revit's Work Plane > Set, and more than Revit's: a level, a level with an
//! offset, or the plane through three points. A 3D view keeps its own (`View3D::work_plane`, set by the `work_plane`
//! command); without one it sketches on the lowest level, as Revit's 3D view does.
//!
//! A sketch stays lines over the plan: the plane says how high each point of them is (a [`Plane`], as a floor's top is),
//! so a work plane may slope, up to [`MAX_SLOPE`], but never stand upright. What is made on it stands on the level below it
//! ([`host`]). Floors, ceilings, roofs and shafts need a level one ([`is_level`]); a sloped one is for what comes later.

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::model::{Guid, Id, MAX_SLOPE, Model, TOL};
use crate::walls::Plane;

/// A 3D view's work plane: on `level`, `offset` mm above it; or, with no level, through `points`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkPlane {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Guid>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<[[f64; 3]; 3]>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl WorkPlane {
    /// On level `level`, `offset` mm above it.
    pub fn on(level: Guid, offset: f64) -> WorkPlane {
        WorkPlane { level: Some(level), offset, points: None }
    }

    /// The plane it is in `m`, or what's wrong with it, said so as to be fixed.
    pub fn resolve(&self, m: &Model) -> Result<Plane, String> {
        match (&self.level, &self.points) {
            (Some(_), Some(_)) => Err("a work plane is either on a `level` (with an `offset`) or through three `points`, not both".into()),
            (Some(level), None) => {
                if m.elements.get(*level).is_none() {
                    return Err("its level isn't in the project".into());
                }
                let z = m.elevation(*level) + self.offset;
                if !z.is_finite() {
                    return Err(format!("its offset {} isn't a length", self.offset));
                }
                Ok(Plane::flat(z))
            }
            (None, Some(points)) => through(points),
            (None, None) => Err("a work plane needs a `level` or three `points`".into()),
        }
    }

    /// How steep it is, degrees from level: 0 for one on a level.
    pub fn slope(&self, m: &Model) -> f64 {
        self.resolve(m).map_or(0.0, |p| slope(&p))
    }
}

/// Whether plane `p` lies level: what a floor, a ceiling, a roof or a shaft can be sketched on.
pub fn is_level(p: &Plane) -> bool {
    p.g.0.abs() < 1e-9 && p.g.1.abs() < 1e-9
}

/// How steep plane `p` is, degrees from level.
pub fn slope(p: &Plane) -> f64 {
    p.g.dot(p.g).sqrt().atan().to_degrees()
}

/// The plane through three points: refused when they lie along a line, stand upright, or slope more than a floor may.
pub fn through(points: &[[f64; 3]; 3]) -> Result<Plane, String> {
    if points.iter().flatten().any(|v| !v.is_finite() || v.abs() > crate::model::MAX_COORD) {
        return Err("a point of the work plane isn't a place in the project".into());
    }
    let [a, b, c] = *points;
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let size = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let len = |d: [f64; 3]| d.iter().map(|x| x * x).sum::<f64>().sqrt();
    let longest = len(u).max(len(v)).max(len([c[0] - b[0], c[1] - b[1], c[2] - b[2]]));
    // A triangle of no area, or one less than a millimetre high: they lie along a line.
    if size.is_nan() || size <= TOL * longest.max(1.0) {
        return Err("the work plane's three points lie along a line: pick three that make a triangle".into());
    }
    if n[2].abs() <= size * 1e-6 {
        return Err("the work plane's three points stand upright: a sketch is drawn over the plan, so its plane must not".into());
    }
    let g = P2(-n[0] / n[2], -n[1] / n[2]);
    let p = Plane { z: a[2] - g.dot(P2(a[0], a[1])), g };
    let steep = slope(&p);
    if steep > MAX_SLOPE + 1e-9 {
        return Err(format!("the work plane's three points slope {steep:.1}°: a floor made on it may slope {MAX_SLOPE}° at most"));
    }
    Ok(p)
}

/// The level what's made at `at` on plane `p` stands on, and how far above it it is there: the highest level at or below
/// the plane, else the lowest.
pub fn host(m: &Model, p: Plane, at: P2) -> Option<(Id, f64)> {
    let z = p.at(at);
    let levels = m.levels();
    let (id, l) = levels.iter().rev().find(|(_, l)| l.elevation <= z + TOL).or(levels.first())?;
    z.is_finite().then_some((*id, z - l.elevation))
}
