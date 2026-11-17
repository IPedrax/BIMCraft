// Portions of this file are ported from rvt-rs (https://github.com/DrunkOnJava/rvt-rs), Apache-2.0,
// Copyright 2026 Griffin Long, at commit b6086d41b9758fd9b92f42f0a47b8345b9c58056: src/partition_instance_transforms.rs (a
// family instance's placement in its own data, RE-87).
// Changed: rewritten for BIMCraft; the reader takes the bytes of one instance's data and every read is bounds-checked.
//! A family instance's placement: twelve `f64` in its data, a 3 × 3 rotation stored row by row and then the origin, in feet.
//! The rotation's columns are the instance's X, Y and Z axes. The reader takes the first place in the data where nine `f64`
//! form a rotation.

use crate::records::f64_at;

/// How far into an instance's data its transform is looked for.
pub(crate) const WINDOW: usize = 0x1000;
/// How closely the nine values must form a rotation.
const TOLERANCE: f64 = 1e-9;

/// A family instance's placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Unit X, Y and Z axes, in model coordinates.
    pub axes: [[f64; 3]; 3],
    /// Model feet.
    pub origin: [f64; 3],
}

impl Transform {
    /// The plan angle of the X axis, radians, counter-clockwise from east; none for an instance whose Z axis is not the model's
    /// (one lying on its side or hung from a ceiling).
    pub fn plan_angle(&self) -> Option<f64> {
        let z = self.axes[2];
        let upright = z[0].abs() <= TOLERANCE && z[1].abs() <= TOLERANCE && (z[2] - 1.0).abs() <= TOLERANCE;
        upright.then(|| self.axes[0][1].atan2(self.axes[0][0]))
    }
}

/// The nine values at `at` as a rotation, rows first; none unless they are orthonormal with determinant 1.
fn rotation_at(data: &[u8], at: usize) -> Option<[[f64; 3]; 3]> {
    let mut rows = [[0.0; 3]; 3];
    for (i, v) in rows.iter_mut().flatten().enumerate() {
        *v = f64_at(data, at + 8 * i).filter(|v| v.is_finite() && v.abs() <= 1.0 + TOLERANCE)?;
    }
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    for i in 0..3 {
        for j in 0..3 {
            if (dot(rows[i], rows[j]) - if i == j { 1.0 } else { 0.0 }).abs() > TOLERANCE {
                return None;
            }
        }
    }
    let [a, b, c] = rows;
    let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
    ((det - 1.0).abs() <= TOLERANCE).then_some(rows)
}

/// The first transform in an instance's data.
pub fn transform(data: &[u8]) -> Option<Transform> {
    let end = data.len().min(WINDOW);
    (0..end.saturating_sub(96)).find_map(|at| {
        let rows = rotation_at(data, at)?;
        let origin = [f64_at(data, at + 72)?, f64_at(data, at + 80)?, f64_at(data, at + 88)?];
        if !origin.iter().all(|v| v.is_finite() && v.abs() <= crate::records::MAX_FEET) {
            return None;
        }
        let column = |j: usize| [rows[0][j], rows[1][j], rows[2][j]];
        Some(Transform { axes: [column(0), column(1), column(2)], origin })
    })
}
