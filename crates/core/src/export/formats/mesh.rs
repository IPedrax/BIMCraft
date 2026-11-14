//! The model as triangles: Wavefront OBJ and binary STL, of what a 3D view shows (the scene's triangles, as glTF writes them).
//!
//! - **OBJ** is text, in metres and y up as glTF (a point `(x, y, z)` mm at `(x, z, -y)` thousandths, so Blender and the others
//!   open it standing), an object (`o`) for each element named by its id and category, normals, and each corner's colour after
//!   its position (`v x y z r g b`, read by Blender and MeshLab), so no `.mtl` file beside it is needed.
//! - **STL** is binary, in millimetres and z up, as 3D printing takes it: a facet's normal and corners, no colour. Triangles
//!   with no area are left out.
//!
//! Both stop when the export is cancelled, between triangles.

use std::collections::HashMap;
use std::fmt::Write;

use super::super::{Content, Ctx, Family, Format, Options, Out, Prepared, Takes};
use crate::graphics::Look;
use crate::model::{Id, Model};
use crate::view3d::{Facet, P3, Scene};

/// The most triangles a file holds: an OBJ's take about 150 bytes each, so this keeps it under 400 MB.
pub const MAX_TRIS: usize = 2_500_000;

/// How often, in triangles, a writer asks whether to stop.
const ASK_EVERY: usize = 16_384;

/// The triangles of the model one item holds, as its 3D view (or the default one) shows them, with the model they are of.
fn triangles(items: &[Prepared], ctx: &Ctx) -> Result<(std::sync::Arc<Model>, Vec<Facet>), String> {
    let [item] = items else { return Err("a model file holds one project".to_string()) };
    let Content::Model { model, view } = &item.content else { return Err(format!("{} isn't a model to write", item.title)) };
    let seen = crate::link::seen(model);
    let look = match view {
        Some(v) => Look::of(&seen, *v),
        None => Look::of_3d(&seen),
    };
    let scene = Scene::new_cancellable(&seen, ctx.cancelled).ok_or("cancelled")?;
    let n = scene.shown(&look);
    if n == 0 {
        return Err("the 3D view shows nothing to export: draw something, or show again what Visibility/Graphics hides there".into());
    }
    if n > MAX_TRIS {
        return Err(format!(
            "the 3D view shows {n} triangles, more than a file is given ({MAX_TRIS}); hide some of the model there and export again"
        ));
    }
    let facets: Vec<Facet> = scene.facets(&look).collect();
    Ok((seen, facets))
}

/// Whether the triangle has an area: its normals are zero when it hasn't.
fn has_area(f: &Facet) -> bool {
    f.normals.iter().any(|n| n.iter().any(|v| *v != 0.0))
}

/// A face's own normal, unit, as its corners wind; zero for no area.
fn face_normal(c: &[P3; 3]) -> [f32; 3] {
    let (a, b) = ([c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]], [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]]);
    let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 && len.is_finite() { n.map(|v| (v / len + 0.0) as f32) } else { [0.0; 3] }
}

/// Stops when asked to, every [`ASK_EVERY`] triangles.
fn check(i: usize, ctx: &Ctx) -> Result<(), String> {
    if i.is_multiple_of(ASK_EVERY) && (ctx.cancelled)() {
        return Err("cancelled".to_string());
    }
    Ok(())
}

pub struct Obj;

impl Format for Obj {
    fn id(&self) -> &'static str {
        "obj"
    }
    fn label(&self) -> &'static str {
        "OBJ"
    }
    fn extension(&self) -> &'static str {
        "obj"
    }
    fn family(&self) -> Family {
        Family::Model
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Model]
    }
    fn write(&self, items: &[Prepared], _: &Options, ctx: &Ctx) -> Result<Vec<Out>, String> {
        let (m, facets) = triangles(items, ctx)?;
        Ok(vec![Out { suffix: None, bytes: obj(&m, &facets, ctx)?.into_bytes() }])
    }
}

/// The OBJ file of `facets`, an object each element.
fn obj(m: &Model, facets: &[Facet], ctx: &Ctx) -> Result<String, String> {
    let mut out = format!("# {} — BIMCraft {}\n# Metres, y up.\n", m.name.replace(['\n', '\r'], " "), env!("CARGO_PKG_VERSION"));
    // Corners and normals each written once, numbered from 1 across the file.
    let mut vertex: HashMap<[u32; 6], usize> = HashMap::new();
    let mut normal: HashMap<[u32; 3], usize> = HashMap::new();
    let mut owner: Option<Id> = None;
    for (i, f) in facets.iter().enumerate() {
        check(i, ctx)?;
        if !has_area(f) {
            continue;
        }
        if owner != Some(f.owner) {
            owner = Some(f.owner);
            let category = m.elements.get(f.owner).map_or("element", |e| e.category());
            let _ = writeln!(out, "o {}_{}", f.owner, category.replace(char::is_whitespace, "_"));
        }
        let rgb = [f.color.0, f.color.1, f.color.2].map(|c| f32::from(c) / 255.0);
        let mut corner = [(0, 0); 3];
        for (k, (p, n)) in f.corners.iter().zip(&f.normals).enumerate() {
            let p = [(p[0] / 1000.0 + 0.0) as f32, (p[2] / 1000.0 + 0.0) as f32, (-p[1] / 1000.0 + 0.0) as f32];
            let n = [(n[0] + 0.0) as f32, (n[2] + 0.0) as f32, (-n[1] + 0.0) as f32];
            let key = [p[0], p[1], p[2], rgb[0], rgb[1], rgb[2]].map(f32::to_bits);
            let next = vertex.len() + 1;
            let v = *vertex.entry(key).or_insert_with(|| {
                let _ = writeln!(out, "v {} {} {} {} {} {}", p[0], p[1], p[2], rgb[0], rgb[1], rgb[2]);
                next
            });
            let next = normal.len() + 1;
            let vn = *normal.entry(n.map(f32::to_bits)).or_insert_with(|| {
                let _ = writeln!(out, "vn {} {} {}", n[0], n[1], n[2]);
                next
            });
            corner[k] = (v, vn);
        }
        let _ = writeln!(out, "f {}//{} {}//{} {}//{}", corner[0].0, corner[0].1, corner[1].0, corner[1].1, corner[2].0, corner[2].1);
    }
    if owner.is_none() {
        return Err("the 3D view shows only surfaces with no area, nothing to export".into());
    }
    Ok(out)
}

pub struct Stl;

impl Format for Stl {
    fn id(&self) -> &'static str {
        "stl"
    }
    fn label(&self) -> &'static str {
        "STL"
    }
    fn extension(&self) -> &'static str {
        "stl"
    }
    fn family(&self) -> Family {
        Family::Model
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Model]
    }
    fn write(&self, items: &[Prepared], _: &Options, ctx: &Ctx) -> Result<Vec<Out>, String> {
        let (m, facets) = triangles(items, ctx)?;
        Ok(vec![Out { suffix: None, bytes: stl(&m, &facets, ctx)? }])
    }
}

/// The binary STL of `facets`: an 80-byte header, the count, and 50 bytes a triangle.
fn stl(m: &Model, facets: &[Facet], ctx: &Ctx) -> Result<Vec<u8>, String> {
    let mut header = format!("BIMCraft {} {} mm", env!("CARGO_PKG_VERSION"), m.name).into_bytes();
    // A binary STL whose header begins "solid" is taken for a text one.
    if header.starts_with(b"solid") {
        header.insert(0, b'_');
    }
    header.resize(80, b' ');
    let mut body = Vec::with_capacity(facets.len() * 50);
    let mut count = 0u32;
    for (i, f) in facets.iter().enumerate() {
        check(i, ctx)?;
        let n = face_normal(&f.corners);
        if n == [0.0; 3] {
            continue;
        }
        for v in n.into_iter().chain(f.corners.iter().flat_map(|p| p.map(|c| (c + 0.0) as f32))) {
            body.extend(v.to_le_bytes());
        }
        body.extend(0u16.to_le_bytes());
        count += 1;
    }
    if count == 0 {
        return Err("the 3D view shows only surfaces with no area, nothing to export".into());
    }
    let mut out = header;
    out.extend(count.to_le_bytes());
    out.extend(body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::export::{Kind, prepare};
    use std::sync::Arc;

    /// A box of walls 8 m by 6 m, 3 m high (the default level's), as the project to write.
    fn walls() -> Vec<Prepared> {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        let m = s.model.clone();
        vec![prepare(&m, &m, None, Takes::Model).unwrap()]
    }

    #[test]
    fn obj_holds_an_object_each_element_in_metres_y_up() {
        let items = walls();
        assert_eq!(items[0].kind, Kind::Project);
        let out = Obj.write(&items, &Options::default(), &Ctx::quiet()).unwrap();
        let text = String::from_utf8(out[0].bytes.clone()).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("o ")).count(), 4, "a wall each");
        assert!(text.lines().any(|l| l.starts_with("o 1") && l.ends_with("_wall")), "named by id and category");
        let vs: Vec<[f64; 6]> = text
            .lines()
            .filter_map(|l| l.strip_prefix("v "))
            .map(|l| {
                let n: Vec<f64> = l.split(' ').map(|x| x.parse().unwrap()).collect();
                [n[0], n[1], n[2], n[3], n[4], n[5]]
            })
            .collect();
        // Up is y: the walls stand from 0 to 3 m; north is -z; the colours are 0 to 1.
        let top = vs.iter().map(|v| v[1]).fold(f64::MIN, f64::max);
        assert!((top - 3.0).abs() < 1e-3, "{top}");
        assert!(vs.iter().any(|v| (v[2] + 6.0).abs() < 0.2), "the north wall at z = -6 m");
        assert!(vs.iter().all(|v| v[3..].iter().all(|c| (0.0..=1.0).contains(c))));
        // Every face names corners and normals that are there.
        let (nv, nn) = (vs.len(), text.lines().filter(|l| l.starts_with("vn ")).count());
        for f in text.lines().filter_map(|l| l.strip_prefix("f ")) {
            for corner in f.split(' ') {
                let (v, n) = corner.split_once("//").unwrap();
                assert!((1..=nv).contains(&v.parse().unwrap()) && (1..=nn).contains(&n.parse().unwrap()), "{corner}");
            }
        }
    }

    #[test]
    fn stl_is_binary_in_millimetres_its_count_its_size() {
        let out = Stl.write(&walls(), &Options::default(), &Ctx::quiet()).unwrap();
        let b = &out[0].bytes;
        assert!(!b.starts_with(b"solid"));
        let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
        assert!(n >= 4 * 12, "{n} triangles: a wall's box at least each");
        assert_eq!(b.len(), 84 + 50 * n);
        // Every normal unit, every corner within the box, millimetres and z up.
        for t in b[84..].as_chunks::<50>().0 {
            let f: Vec<f32> = t[..48].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
            assert!(((f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt() - 1.0).abs() < 1e-4);
            for p in f[3..].as_chunks::<3>().0 {
                assert!((-500.0..=8500.0).contains(&p[0]) && (-500.0..=6500.0).contains(&p[1]) && (-1.0..=3001.0).contains(&p[2]), "{p:?}");
            }
        }
    }

    #[test]
    fn nothing_to_show_or_a_stop_says_so() {
        let empty = Arc::new(Session::new().model.as_ref().clone());
        let items = vec![prepare(&empty, &empty, None, Takes::Model).unwrap()];
        for f in [&Obj as &dyn Format, &Stl] {
            assert!(f.write(&items, &Options::default(), &Ctx::quiet()).unwrap_err().contains("nothing to export"));
            let stop = Ctx { cancelled: &|| true, progress: &|_, _, _| {} };
            assert_eq!(f.write(&walls(), &Options::default(), &stop).unwrap_err(), "cancelled");
        }
    }
}
