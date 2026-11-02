//! Right-handed local frames and six local degrees of freedom; millimetres and degrees.
use crate::view3d::{P3, add, cross, dot, mul, sub};
use serde::{Deserialize, Serialize};

pub const EPS: f64 = 1e-8;
pub fn norm(v: P3) -> f64 {
    dot(v, v).sqrt()
}
pub fn unit(v: P3) -> Result<P3, String> {
    let n = norm(v);
    if !n.is_finite() || n < EPS { Err("A direction is zero or non-finite".into()) } else { Ok(mul(v, 1.0 / n)) }
}
pub fn project(v: P3, normal: P3) -> P3 {
    sub(v, mul(normal, dot(v, normal)))
}
pub fn fallback(normal: P3) -> P3 {
    let axes = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let mut axis = axes[0];
    for a in axes.into_iter().skip(1) {
        if dot(a, normal).abs() < dot(axis, normal).abs() {
            axis = a;
        }
    }
    let projected = project(axis, normal);
    let n = norm(projected);
    mul(projected, 1.0 / n)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    #[serde(default)]
    pub origin: P3,
    #[serde(default = "x_axis")]
    pub x: P3,
    #[serde(default = "y_axis")]
    pub y: P3,
    #[serde(default = "z_axis")]
    pub z: P3,
}
fn x_axis() -> P3 {
    [1., 0., 0.]
}
fn y_axis() -> P3 {
    [0., 1., 0.]
}
fn z_axis() -> P3 {
    [0., 0., 1.]
}
impl Default for Frame {
    fn default() -> Self {
        Self { origin: [0.; 3], x: x_axis(), y: y_axis(), z: z_axis() }
    }
}
impl Frame {
    pub fn at(origin: P3) -> Self {
        Self { origin, ..Self::default() }
    }
    pub fn vector(&self, v: P3) -> P3 {
        add(add(mul(self.x, v[0]), mul(self.y, v[1])), mul(self.z, v[2]))
    }
    pub fn point(&self, p: P3) -> P3 {
        add(self.origin, self.vector(p))
    }
    pub fn inverse_point(&self, p: P3) -> P3 {
        self.inverse_vector(sub(p, self.origin))
    }
    pub fn inverse_vector(&self, v: P3) -> P3 {
        [dot(v, self.x), dot(v, self.y), dot(v, self.z)]
    }
    /// This frame, given in `root`'s axes (flipped across their x when `mirrored`, as a mirrored component is), in the axes
    /// `root` is given in: a component's frame in the project.
    pub fn from_local(self, root: Frame, mirrored: bool) -> Result<Self, String> {
        let flip = |p: P3| if mirrored { [p[0], -p[1], p[2]] } else { p };
        Self::from_z(root.point(flip(self.origin)), root.vector(flip(self.z)), root.vector(flip(self.x)))
    }
    /// The inverse of [`Frame::from_local`]: a frame in the project, in a component's own axes.
    pub fn into_local(self, root: Frame, mirrored: bool) -> Result<Self, String> {
        let flip = |p: P3| if mirrored { [p[0], -p[1], p[2]] } else { p };
        Self::from_z(flip(root.inverse_point(self.origin)), flip(root.inverse_vector(self.z)), flip(root.inverse_vector(self.x)))
    }
    pub fn checked(self) -> Result<Self, String> {
        if self.origin.iter().chain(self.x.iter()).chain(self.y.iter()).chain(self.z.iter()).any(|v| !v.is_finite() || v.abs() > 1e9) {
            return Err("Frame contains a non-finite or out-of-range coordinate".into());
        }
        if [self.x, self.y, self.z].iter().any(|v| (norm(*v) - 1.).abs() > 1e-6)
            || dot(self.x, self.y).abs() > 1e-6
            || dot(self.x, self.z).abs() > 1e-6
            || dot(self.y, self.z).abs() > 1e-6
            || dot(cross(self.x, self.y), self.z) < 1. - 1e-6
        {
            return Err("Frame must have orthonormal, right-handed axes".into());
        }
        Ok(self)
    }
    /// Translate in the host's axes, then postmultiply Rx * Ry * Rz: intrinsic local XYZ rotations.
    pub fn transformed(self, offset: P3, rotations: P3) -> Result<Self, String> {
        self.checked()?;
        if offset.iter().chain(rotations.iter()).any(|v| !v.is_finite()) {
            return Err("Transform values must be finite".into());
        }
        let mut f = Self { origin: self.point(offset), ..self };
        let (s, c) = rotations[0].to_radians().sin_cos();
        let (y, z) = (f.y, f.z);
        f.y = add(mul(y, c), mul(z, s));
        f.z = add(mul(y, -s), mul(z, c));
        let (s, c) = rotations[1].to_radians().sin_cos();
        let (x, z) = (f.x, f.z);
        f.x = add(mul(x, c), mul(z, -s));
        f.z = add(mul(x, s), mul(z, c));
        let (s, c) = rotations[2].to_radians().sin_cos();
        let (x, y) = (f.x, f.y);
        f.x = add(mul(x, c), mul(y, s));
        f.y = add(mul(x, -s), mul(y, c));
        f.checked()
    }
    pub fn from_z(origin: P3, z: P3, x_hint: P3) -> Result<Self, String> {
        let z = unit(z)?;
        let projected = project(x_hint, z);
        let x = if norm(projected) < EPS { fallback(z) } else { unit(projected)? };
        Self { origin, x, y: unit(cross(z, x))?, z }.checked()
    }
    pub fn from_tangent(origin: P3, tangent: P3, z_hint: P3) -> Result<(Self, bool), String> {
        let x = unit(tangent)?;
        let z = project(z_hint, x);
        let used = norm(z) < EPS;
        let z = if used { fallback(x) } else { unit(z)? };
        Ok((Self { origin, x, y: unit(cross(z, x))?, z }.checked()?, used))
    }
}
/// Spherical interpolation of normals. At antipodes a deterministic great circle avoids a zero normal.
pub fn interpolate_normal(a: P3, b: P3, t: f64) -> Result<(P3, bool), String> {
    let (a, b) = (unit(a)?, unit(b)?);
    if t == 0. {
        return Ok((a, false));
    }
    if t == 1. {
        return Ok((b, false));
    }
    let cosine = dot(a, b).clamp(-1., 1.);
    if cosine > 0.999999 {
        return Ok((unit(add(mul(a, 1. - t), mul(b, t)))?, false));
    }
    if cosine < -0.999999 {
        let projected = project(b, a);
        let sine = norm(projected);
        let used = sine < EPS;
        let p = if used { fallback(a) } else { unit(projected)? };
        let angle = if used { std::f64::consts::PI * t } else { sine.atan2(cosine) * t };
        return Ok((unit(add(mul(a, angle.cos()), mul(p, angle.sin())))?, used));
    }
    let angle = cosine.acos();
    let denominator = angle.sin();
    Ok((unit(add(mul(a, ((1. - t) * angle).sin() / denominator), mul(b, (t * angle).sin() / denominator)))?, false))
}
