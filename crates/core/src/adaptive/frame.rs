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
/// Which way `points` wind, as a Newell normal (right-handed: counterclockwise seen from where it points); none when there are
/// fewer than three or they are in a line.
pub fn newell(points: &[P3]) -> Option<P3> {
    if points.len() < 3 {
        return None;
    }
    let mut n = [0.; 3];
    let mut size: f64 = 0.;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        size = size.max(norm(sub(b, *a)));
    }
    if !size.is_finite() || norm(n) <= 1e-9 * size * size.max(1.) { None } else { unit(n).ok() }
}

fn centroid(points: &[P3]) -> P3 {
    let sum = points.iter().fold([0.; 3], |s, p| add(s, *p));
    mul(sum, 1. / points.len().max(1) as f64)
}

/// `v` turned the shortest way that takes direction `from` onto `to` (both unit).
fn turned(v: P3, from: P3, to: P3) -> P3 {
    let c = dot(from, to).clamp(-1., 1.);
    let k = cross(from, to);
    let s = norm(k);
    let k = if s < EPS {
        if c > 0. {
            return v;
        }
        fallback(from)
    } else {
        mul(k, 1. / s)
    };
    let (s, c) = if s < EPS { (0., -1.) } else { (s, c) };
    add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1. - c)))
}

/// The eigenvector of the largest eigenvalue of a symmetric 4x4 matrix, by Jacobi's rotations (deterministic).
fn largest_eigenvector(mut a: [[f64; 4]; 4]) -> [f64; 4] {
    let mut v = [[0.; 4]; 4];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.;
    }
    for _ in 0..64 {
        let off: f64 = (0..4).flat_map(|i| (0..4).filter(move |j| *j != i).map(move |j| (i, j))).map(|(i, j)| a[i][j] * a[i][j]).sum();
        if off < 1e-30 {
            break;
        }
        for p in 0..3 {
            for q in p + 1..4 {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2. * a[p][q]);
                let t = if theta >= 0. { 1. } else { -1. } / (theta.abs() + (theta * theta + 1.).sqrt());
                let c = 1. / (t * t + 1.).sqrt();
                let s = t * c;
                for row in &mut a {
                    let (kp, kq) = (row[p], row[q]);
                    row[p] = c * kp - s * kq;
                    row[q] = s * kp + c * kq;
                }
                let (rp, rq) = (a[p], a[q]);
                a[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
                a[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
                for row in &mut v {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    let mut best = 0;
    for i in 1..4 {
        if a[i][i] > a[best][best] + 1e-12 {
            best = i;
        }
    }
    [v[0][best], v[1][best], v[2][best], v[3][best]]
}

/// The rigid motion that best takes `from` onto `to` (point for point, least squares; Horn's quaternion method): its axes are
/// where `from`'s x, y and z go, its origin where `from`'s origin goes. One point moves without turning; points in a line turn
/// the shortest way that lines them up.
pub fn best_fit(from: &[P3], to: &[P3]) -> Frame {
    let n = from.len().min(to.len());
    if n == 0 || from.iter().chain(to).any(|p| p.iter().any(|v| !v.is_finite())) {
        return Frame::default();
    }
    let (from, to) = (&from[..n], &to[..n]);
    let (cf, ct) = (centroid(from), centroid(to));
    let a: Vec<P3> = from.iter().map(|p| sub(*p, cf)).collect();
    let b: Vec<P3> = to.iter().map(|p| sub(*p, ct)).collect();
    let longest = |v: &[P3]| v.iter().copied().max_by(|p, q| norm(*p).total_cmp(&norm(*q))).unwrap_or([0.; 3]);
    let reach = norm(longest(&a));
    let axes: [P3; 3] = if reach < EPS || norm(longest(&b)) < EPS {
        [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
    } else {
        let d = mul(longest(&a), 1. / reach);
        let in_line = a.iter().all(|p| norm(project(*p, d)) <= 1e-6 * reach.max(1.));
        if in_line {
            // Along the line, the placed points' spread weighted by where each family point is on it.
            let along = b.iter().zip(&a).fold([0.; 3], |s, (q, p)| add(s, mul(*q, dot(*p, d))));
            match unit(along) {
                Ok(to) => [turned([1., 0., 0.], d, to), turned([0., 1., 0.], d, to), turned([0., 0., 1.], d, to)],
                Err(_) => [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            }
        } else {
            let mut s = [[0.; 3]; 3];
            for (p, q) in a.iter().zip(&b) {
                for i in 0..3 {
                    for j in 0..3 {
                        s[i][j] += p[i] * q[j];
                    }
                }
            }
            let [[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]] = s;
            let q = largest_eigenvector([
                [xx + yy + zz, yz - zy, zx - xz, xy - yx],
                [yz - zy, xx - yy - zz, xy + yx, zx + xz],
                [zx - xz, xy + yx, -xx + yy - zz, yz + zy],
                [xy - yx, zx + xz, yz + zy, -xx - yy + zz],
            ]);
            let l = q.iter().map(|v| v * v).sum::<f64>().sqrt();
            let [w, x, y, z] = if l < EPS { [1., 0., 0., 0.] } else { q.map(|v| v / l) };
            [
                [1. - 2. * (y * y + z * z), 2. * (x * y + w * z), 2. * (x * z - w * y)],
                [2. * (x * y - w * z), 1. - 2. * (x * x + z * z), 2. * (y * z + w * x)],
                [2. * (x * z + w * y), 2. * (y * z - w * x), 1. - 2. * (x * x + y * y)],
            ]
        }
    };
    let rotated = add(add(mul(axes[0], cf[0]), mul(axes[1], cf[1])), mul(axes[2], cf[2]));
    let fit = Frame { origin: sub(ct, rotated), x: axes[0], y: axes[1], z: axes[2] };
    // Squared up, so a fit off by rounding still passes `checked`.
    Frame::from_z(fit.origin, fit.z, fit.x).unwrap_or_default()
}
