use super::frame::{EPS, norm, unit};
use super::{Curve, Frame};
use crate::family::Piece;
use crate::geom::{self, P2};
use crate::view3d::{P3, add, cross, dot, mul, sub};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// BIMCraft's native geometry is polygonal. Face references address triangles, not analytic B-rep surfaces.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub vertices: Vec<P3>,
    pub faces: Vec<[usize; 3]>,
    pub closed: bool,
}
impl Mesh {
    pub fn triangles(&self) -> Vec<[P3; 3]> {
        self.faces.iter().map(|f| f.map(|i| self.vertices[i])).collect()
    }
    /// The edges a drawing shows: where two faces meet folding more than `angle` degrees, and where only one face has the edge
    /// (an open border); not the diagonals that split a flat face into triangles.
    pub fn folds(&self, angle: f64) -> Vec<(P3, P3)> {
        let normal = |f: &[usize; 3]| {
            let [a, b, c] = f.map(|i| self.vertices[i]);
            let n = crate::view3d::cross(crate::view3d::sub(b, a), crate::view3d::sub(c, a));
            let l = crate::view3d::dot(n, n).sqrt();
            if l > 0. { crate::view3d::mul(n, 1. / l) } else { n }
        };
        let mut by_edge: std::collections::BTreeMap<(usize, usize), Vec<P3>> = std::collections::BTreeMap::new();
        for f in &self.faces {
            let n = normal(f);
            for k in 0..3 {
                let (a, b) = (f[k], f[(k + 1) % 3]);
                by_edge.entry((a.min(b), a.max(b))).or_default().push(n);
            }
        }
        let fold = angle.to_radians().cos();
        by_edge
            .into_iter()
            .filter(|(_, ns)| ns.len() != 2 || crate::view3d::dot(ns[0], ns[1]) < fold)
            .map(|((a, b), _)| (self.vertices[a], self.vertices[b]))
            .collect()
    }
    pub fn topology(&self) -> String {
        let mut hash = 0xcbf29ce484222325u64;
        for n in [self.vertices.len(), self.faces.len()].into_iter().chain(self.faces.iter().flatten().copied()) {
            for byte in (n as u64).to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        format!("mesh-{hash:016x}")
    }
    pub fn edges(&self) -> Vec<[usize; 2]> {
        self.faces
            .iter()
            .flat_map(|[a, b, c]| [(*a, *b), (*b, *c), (*c, *a)])
            .map(|(a, b)| if a < b { (a, b) } else { (b, a) })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|(a, b)| [a, b])
            .collect()
    }
    pub fn signed_volume(&self) -> f64 {
        let o = self.vertices.first().copied().unwrap_or([0.; 3]);
        self.faces
            .iter()
            .map(|[a, b, c]| dot(sub(self.vertices[*a], o), cross(sub(self.vertices[*b], o), sub(self.vertices[*c], o))) / 6.)
            .sum()
    }
    pub fn area(&self) -> f64 {
        self.triangles().iter().map(|[a, b, c]| norm(cross(sub(*b, *a), sub(*c, *a))) * 0.5).sum()
    }
    pub fn piece(&self, material: String) -> Piece {
        let (bottom, top) = self.vertices.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
        Piece {
            tris: self.triangles(),
            caps: self.faces.len(),
            outline: geom::hull(self.vertices.iter().map(|p| P2(p[0], p[1])).collect()),
            holes: Vec::new(),
            bottom: if bottom.is_finite() { bottom } else { 0. },
            top: if top.is_finite() { top } else { 0. },
            material,
            round: false,
            vis: None,
        }
    }
    pub fn face_frame(&self, index: usize, barycentric: P3) -> Result<Frame, String> {
        if barycentric.iter().any(|v| !v.is_finite() || *v < 0. || *v > 1.) || (barycentric.iter().sum::<f64>() - 1.).abs() > 1e-7 {
            return Err("Face barycentric coordinates must be nonnegative and sum to 1".into());
        }
        let indices = self.faces.get(index).ok_or_else(|| format!("Mesh face {index} does not exist"))?;
        let [a, b, c] = indices.map(|i| self.vertices[i]);
        Frame::from_z(
            add(add(mul(a, barycentric[0]), mul(b, barycentric[1])), mul(c, barycentric[2])),
            cross(sub(b, a), sub(c, a)),
            sub(b, a),
        )
    }
    pub fn edge_frame(&self, index: usize, t: f64) -> Result<Frame, String> {
        if !(0. ..=1.).contains(&t) {
            return Err("Edge parameter must be between 0 and 1".into());
        }
        let [a, b] = *self.edges().get(index).ok_or_else(|| format!("Mesh edge {index} does not exist"))?;
        let normal = self
            .faces
            .iter()
            .filter(|f| f.contains(&a) && f.contains(&b))
            .map(|f| cross(sub(self.vertices[f[1]], self.vertices[f[0]]), sub(self.vertices[f[2]], self.vertices[f[0]])))
            .fold([0.; 3], add);
        let origin = add(mul(self.vertices[a], 1. - t), mul(self.vertices[b], t));
        Frame::from_tangent(origin, sub(self.vertices[b], self.vertices[a]), normal).map(|(f, _)| f)
    }
    pub fn vertex_frame(&self, index: usize) -> Result<Frame, String> {
        let p = *self.vertices.get(index).ok_or_else(|| format!("Mesh vertex {index} does not exist"))?;
        let normal = self
            .faces
            .iter()
            .filter(|f| f.contains(&index))
            .map(|f| cross(sub(self.vertices[f[1]], self.vertices[f[0]]), sub(self.vertices[f[2]], self.vertices[f[0]])))
            .fold([0.; 3], add);
        Frame::from_z(p, normal, [1., 0., 0.])
    }
    fn checked(mut self) -> Result<Self, String> {
        if self.vertices.len() > 4096 || self.faces.len() > 20000 {
            return Err("Adaptive form exceeds the geometry budget".into());
        }
        if self.vertices.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e9) {
            return Err("Form has unusable coordinates".into());
        }
        for f in &self.faces {
            if f.iter().any(|i| *i >= self.vertices.len())
                || norm(cross(sub(self.vertices[f[1]], self.vertices[f[0]]), sub(self.vertices[f[2]], self.vertices[f[0]]))) < EPS
            {
                return Err("Form has a degenerate triangle".into());
            }
        }
        if self.closed {
            if self.signed_volume().abs() < EPS {
                return Err("Closed form has zero volume".into());
            }
            if self.signed_volume() < 0. {
                for f in &mut self.faces {
                    f.swap(1, 2);
                }
            }
            let mut uses = std::collections::BTreeMap::new();
            for [a, b, c] in &self.faces {
                for (a, b) in [(*a, *b), (*b, *c), (*c, *a)] {
                    let key = if a < b { (a, b) } else { (b, a) };
                    let entry = uses.entry(key).or_insert((0, 0));
                    entry.0 += 1;
                    entry.1 += if a < b { 1 } else { -1 };
                }
            }
            if uses.values().any(|(count, direction)| *count != 2 || *direction != 0) {
                return Err("Closed form is not a two-manifold mesh".into());
            }
        }
        Ok(self)
    }
}
/// Smallest-eigenvalue normal of the sampled profile's covariance, without flattening the original points.
pub fn profile_normal(points: &[P3]) -> Result<P3, String> {
    if points.len() < 3 {
        return Err("A profile needs at least three points".into());
    }
    let center = mul(points.iter().copied().fold([0.; 3], add), 1. / points.len() as f64);
    let mut matrix = [[0.; 3]; 3];
    let mut vectors = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    for p in points {
        let d = sub(*p, center);
        for i in 0..3 {
            for j in 0..3 {
                matrix[i][j] += d[i] * d[j];
            }
        }
    }
    for _ in 0..32 {
        let (mut a, mut b) = (0, 1);
        for i in 0..3 {
            for j in i + 1..3 {
                if matrix[i][j].abs() > matrix[a][b].abs() {
                    (a, b) = (i, j);
                }
            }
        }
        if matrix[a][b].abs() < 1e-12 {
            break;
        }
        let angle = 0.5 * (2. * matrix[a][b]).atan2(matrix[b][b] - matrix[a][a]);
        let (s, c) = angle.sin_cos();
        let (aa, bb, ab) = (matrix[a][a], matrix[b][b], matrix[a][b]);
        matrix[a][a] = c * c * aa - 2. * s * c * ab + s * s * bb;
        matrix[b][b] = s * s * aa + 2. * s * c * ab + c * c * bb;
        matrix[a][b] = 0.;
        matrix[b][a] = 0.;
        for i in 0..3 {
            if i != a && i != b {
                let (x, y) = (matrix[i][a], matrix[i][b]);
                matrix[i][a] = c * x - s * y;
                matrix[a][i] = matrix[i][a];
                matrix[i][b] = s * x + c * y;
                matrix[b][i] = matrix[i][b];
            }
            let (x, y) = (vectors[i][a], vectors[i][b]);
            vectors[i][a] = c * x - s * y;
            vectors[i][b] = s * x + c * y;
        }
    }
    let mut index = 0;
    for i in 1..3 {
        if matrix[i][i] < matrix[index][index] {
            index = i;
        }
    }
    let mut normal = unit([vectors[0][index], vectors[1][index], vectors[2][index]])?;
    let sign = if normal[2].abs() > EPS {
        normal[2]
    } else if normal[1].abs() > EPS {
        normal[1]
    } else {
        normal[0]
    };
    if sign < 0. {
        normal = mul(normal, -1.);
    }
    Ok(normal)
}
fn cap(points: &[P3]) -> Result<Vec<[usize; 3]>, String> {
    let normal = profile_normal(points)?;
    let frame = Frame::from_z(points[0], normal, [1., 0., 0.])?;
    let ring = points
        .iter()
        .map(|p| {
            let q = frame.inverse_point(*p);
            P2(q[0], q[1])
        })
        .collect::<Vec<_>>();
    if geom::area(&ring).abs() < EPS {
        return Err("Profile has no area in its best-fit plane".into());
    }
    for i in 0..ring.len() {
        for j in i + 1..ring.len() {
            if j == i + 1 || (i == 0 && j == ring.len() - 1) {
                continue;
            }
            let (a, b, c, d) = (ring[i], ring[(i + 1) % ring.len()], ring[j], ring[(j + 1) % ring.len()]);
            let side = |a: P2, b: P2, p: P2| (b - a).cross(p - a);
            let touch = geom::dist_to_segment(a, c, d) < EPS
                || geom::dist_to_segment(b, c, d) < EPS
                || geom::dist_to_segment(c, a, b) < EPS
                || geom::dist_to_segment(d, a, b) < EPS;
            if touch || (side(a, b, c) * side(a, b, d) < 0. && side(c, d, a) * side(c, d, b) < 0.) {
                return Err("Profile self-intersects in its best-fit projection".into());
            }
        }
    }
    let mut triangles = geom::triangulate(&ring);
    if geom::area(&ring) < 0. {
        for face in &mut triangles {
            face.swap(1, 2);
        }
    }
    if triangles.len() != points.len() - 2 {
        return Err("Profile could not be triangulated as a simple closed chain".into());
    }
    // Ear clipping returns CCW triangles in the projection; lift their original 3D vertices unchanged.
    Ok(triangles)
}
pub fn patch(curve: &Curve) -> Result<Mesh, String> {
    if !curve.closed {
        return Err("Patch requires a closed curve chain".into());
    }
    let vertices = curve.samples();
    let faces = cap(&vertices)?;
    Mesh { vertices, faces, closed: false }.checked()
}
pub fn extrude(curve: &Curve, distance: f64, direction: Option<P3>) -> Result<Mesh, String> {
    if !curve.closed {
        return Err("Extrude requires a closed curve chain".into());
    }
    if !distance.is_finite() || distance.abs() < EPS {
        return Err("Extrusion distance must be finite and nonzero".into());
    }
    let base = curve.samples();
    let n = base.len();
    let direction = unit(direction.unwrap_or(profile_normal(&base)?))?;
    let offset = mul(direction, distance);
    let mut vertices = base.clone();
    vertices.extend(base.iter().map(|p| add(*p, offset)));
    let caps = cap(&base)?;
    let mut faces = Vec::new();
    for [a, b, c] in caps {
        faces.push([a, c, b]);
        faces.push([a + n, b + n, c + n]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        faces.push([i, j, j + n]);
        faces.push([i, j + n, i + n]);
    }
    Mesh { vertices, faces, closed: true }.checked()
}
pub fn loft(profiles: &[Curve]) -> Result<Mesh, String> {
    if profiles.len() < 2 || profiles.len() > 64 {
        return Err("Loft needs 2 to 64 ordered profile chains".into());
    }
    let closed = profiles[0].closed;
    if profiles.iter().any(|c| c.closed != closed) {
        return Err("Loft profiles must all be open or all closed".into());
    }
    // Union of each polyline's native corner parameters retains every corner, even with unequal profile sizes.
    let mut parameters = vec![0.];
    for c in profiles {
        let n = if c.bezier {
            64
        } else if c.closed {
            c.controls.len()
        } else {
            c.controls.len() - 1
        };
        parameters.extend((1..=n).map(|i| i as f64 / n as f64));
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    if closed {
        parameters.pop();
    }
    if parameters.len() > 256 {
        return Err("Loft correspondence exceeds 256 samples per profile".into());
    }
    let n = parameters.len();
    let vertices = profiles.iter().flat_map(|c| parameters.iter().map(|t| c.point(*t))).collect::<Vec<_>>();
    let mut faces = Vec::new();
    for p in 0..profiles.len() - 1 {
        for i in 0..if closed { n } else { n - 1 } {
            let j = (i + 1) % n;
            let (a, b, c, d) = (p * n + i, p * n + j, (p + 1) * n + j, (p + 1) * n + i);
            faces.push([a, b, c]);
            faces.push([a, c, d]);
        }
    }
    if closed {
        for [a, b, c] in cap(&vertices[..n])? {
            faces.push([a, c, b]);
        }
        let offset = (profiles.len() - 1) * n;
        for [a, b, c] in cap(&vertices[offset..])? {
            faces.push([a + offset, b + offset, c + offset]);
        }
    }
    Mesh { vertices, faces, closed }.checked()
}
