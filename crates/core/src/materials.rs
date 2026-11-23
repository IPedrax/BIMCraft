//! What a material holds besides how it looks: the parameters every material has (Mark, Description, Keywords…), the values of
//! the project parameters for the `material` category, and the cube it is previewed on. The `material` command changes them;
//! [`check`] is what `Model::check` asks of them.

use std::collections::BTreeMap;

use bimcraft_i18n::ui_msg;
use serde_json::Value;

use crate::model::{MAX_TEXT, Material, Model, ParameterKind, PatternLayer};
use crate::texture::{Kind, Picture, Sampler, Texture};

/// The category of project parameters a material's values are of.
pub const CATEGORY: &str = "material";

/// The most parameter values a material holds.
pub const MAX_VALUES: usize = 256;

/// The parameters every material has, in the order they are shown: its name, what it holds, and the heading it comes under.
pub const BUILT_IN: [(&str, ParameterKind, &str); 9] = [
    (ui_msg!("Mark"), ParameterKind::Text, ui_msg!("Identity Data")),
    (ui_msg!("Description"), ParameterKind::Text, ui_msg!("Identity Data")),
    (ui_msg!("Class"), ParameterKind::Text, ui_msg!("Identity Data")),
    (ui_msg!("Keywords"), ParameterKind::Text, ui_msg!("Identity Data")),
    (ui_msg!("Comments"), ParameterKind::Text, ui_msg!("Identity Data")),
    (ui_msg!("Manufacturer"), ParameterKind::Text, ui_msg!("Product Information")),
    (ui_msg!("Model"), ParameterKind::Text, ui_msg!("Product Information")),
    (ui_msg!("Cost"), ParameterKind::Number, ui_msg!("Product Information")),
    (ui_msg!("URL"), ParameterKind::Text, ui_msg!("Product Information")),
];

/// What the built-in parameter `name` holds; none for any other name.
pub fn built_in(name: &str) -> Option<ParameterKind> {
    BUILT_IN.iter().find(|(n, ..)| *n == name).map(|(_, k, _)| *k)
}

/// Whether a material can hold a value of `name`: a built-in one, or a project parameter for materials.
pub fn known(m: &Model, name: &str) -> bool {
    built_in(name).is_some() || m.parameters.get(name).is_some_and(|p| p.categories.iter().any(|c| c == CATEGORY))
}

/// Whether `v` is a value of the parameter `name` of a material.
pub fn holds(m: &Model, name: &str, v: &Value) -> bool {
    match built_in(name) {
        Some(ParameterKind::Number) => v.as_f64().is_some_and(|x| x.is_finite() && x >= 0.0),
        Some(_) => v.as_str().is_some_and(|t| t.chars().count() <= MAX_TEXT),
        None => m.parameters.get(name).is_some_and(|p| p.holds(v)),
    }
}

/// `values` set on `material`: null, or text with nothing in it, takes a value away.
pub(crate) fn set_values(m: &Model, material: &mut Material, values: &serde_json::Map<String, Value>) -> Result<(), String> {
    for (name, v) in values {
        if !known(m, name) {
            let own: Vec<&str> = BUILT_IN.iter().map(|(n, ..)| *n).collect();
            return Err(format!(
                "`parameters`: `{name}` isn't a parameter of materials: one of {}, or a project parameter for the `material` category",
                own.join(", ")
            ));
        }
        if v.is_null() || v.as_str().is_some_and(|t| t.trim().is_empty()) {
            material.parameters.remove(name);
        } else if holds(m, name, v) {
            material.parameters.insert(name.clone(), v.clone());
        } else {
            return Err(format!("`parameters`: {v} isn't a value of `{name}`"));
        }
    }
    Ok(())
}

/// What's wrong with the materials' parameters, said so it can be put right.
pub(crate) fn check(m: &Model) -> Result<(), String> {
    for (n, p) in &m.parameters {
        if built_in(n).is_some() && p.categories.iter().any(|c| c == CATEGORY) {
            return Err(format!("parameter `{n}`: materials have a `{n}` of their own; name it otherwise"));
        }
    }
    for (n, x) in &m.materials {
        if x.parameters.len() > MAX_VALUES {
            return Err(format!("material `{n}`: it holds {} parameter values; it holds up to {MAX_VALUES}", x.parameters.len()));
        }
        for (name, v) in &x.parameters {
            if !known(m, name) {
                return Err(format!("material `{n}`: `{name}` isn't a parameter of materials; add it with `parameter`"));
            }
            if !holds(m, name, v) {
                return Err(format!("material `{n}`: {v} isn't a value of `{name}`"));
            }
        }
    }
    Ok(())
}

/// The values of parameters this project doesn't have for materials, or that aren't what they hold, left out: what a file or
/// another project's material may bring.
pub(crate) fn tidy(m: &mut Model) {
    let mut materials = std::mem::take(&mut m.materials);
    for x in materials.values_mut() {
        x.parameters.retain(|name, v| known(m, name) && holds(m, name, v));
    }
    m.materials = materials;
}

impl PatternLayer {
    /// Whether it shows anything.
    pub fn shows(&self) -> bool {
        self.fill.is_some() || self.pattern != crate::model::Pattern::None
    }
}

impl Material {
    /// The project's fill patterns it shows, by name.
    pub fn fills(&self) -> impl Iterator<Item = &String> {
        let back = [&self.cut_background, &self.surface_background].into_iter().flatten().filter_map(|l| l.fill.as_ref());
        [&self.cut_pattern, &self.surface_pattern].into_iter().flatten().chain(back)
    }

    /// Its texture and its image fills, those of pictures: what names the project's pictures.
    pub fn pictures(&self) -> impl Iterator<Item = &Texture> {
        [&self.texture, &self.cut_image, &self.surface_image].into_iter().flatten().filter(|t| t.kind == Kind::Image)
    }

    /// The same, to change.
    pub fn pictures_mut(&mut self) -> impl Iterator<Item = &mut Texture> {
        [&mut self.texture, &mut self.cut_image, &mut self.surface_image].into_iter().flatten().filter(|t| t.kind == Kind::Image)
    }

    /// The project's fill patterns it shows, by name, to change.
    pub fn fills_mut(&mut self) -> impl Iterator<Item = &mut String> {
        let back = [&mut self.cut_background, &mut self.surface_background].into_iter().flatten().filter_map(|l| l.fill.as_mut());
        [&mut self.cut_pattern, &mut self.surface_pattern].into_iter().flatten().chain(back)
    }
}

/// How the cube is first seen: turned and tilted, radians, so that three faces show.
pub const CUBE_VIEW: [f64; 2] = [std::f64::consts::FRAC_PI_4, 0.6];
/// The widest a cube is drawn, pixels.
pub const MAX_CUBE: usize = 1024;
/// How long the cube's edge stands for, mm: what a texture's tiles are measured against.
const EDGE: f64 = 1000.0;
/// Samples across a pixel, for edges without steps.
const SAMPLES: usize = 2;

/// `material` on a cube, as the 3D view shows it (its colour, its texture, how see-through it is), `side` pixels square, RGBA
/// with what is round the cube clear. `view` is its turn about the vertical and its tilt, radians.
pub fn cube(material: &Material, images: &BTreeMap<String, Picture>, side: usize, view: [f64; 2]) -> Vec<u8> {
    let side = side.min(MAX_CUBE);
    let [yaw, pitch] = if view.iter().all(|a| a.is_finite()) { view } else { CUBE_VIEW };
    let (sy, cy, sp, cp) = (yaw.sin(), yaw.cos(), pitch.sin(), pitch.cos());
    // The view's axes in the cube's: across, up, and towards the eye.
    let right = [cy, sy, 0.0];
    let up = [sy * sp, -cy * sp, cp];
    let eye = [-sy * cp, cy * cp, sp];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // A light over the viewer's left shoulder, in the view's axes.
    let light = [-0.42, 0.56, 0.71];
    let sampler = material.texture.as_ref().and_then(|t| Sampler::new(t, material.color, images));
    // The cube's corners stay inside the picture however it is turned.
    let scale = side as f64 / (3.0f64.sqrt() * 1.04);
    let middle = side as f64 / 2.0;
    let foot = EDGE / (scale * SAMPLES as f64).max(1.0);
    let alpha = 1.0 - f64::from(material.transparency.min(100)) / 100.0;
    // The colour a face shows at a point of it: `axis` is the one it faces along.
    let shade = |q: [f64; 3], axis: usize| -> [f64; 3] {
        let (u, v) = match axis {
            0 => (q[1], q[2]),
            1 => (q[0], q[2]),
            _ => (q[0], q[1]),
        };
        let base = sampler.as_ref().map_or(material.color, |s| {
            let c = s.at((u + 0.5) * EDGE, (v + 0.5) * EDGE, foot);
            [c.0, c.1, c.2]
        });
        let mut normal = [0.0; 3];
        normal[axis] = 1.0;
        let lit = dot(normal, right) * light[0] + dot(normal, up) * light[1] + dot(normal, eye) * light[2];
        // Either side of the axis: the face towards the eye is the one lit.
        let lit = if dot(normal, eye) < 0.0 { -lit } else { lit };
        let k = 0.52 + 0.48 * lit.max(0.0);
        base.map(|c| f64::from(c) * k)
    };
    let mut out = vec![0u8; side * side * 4];
    for (k, pixel) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (px, py) = ((k % side.max(1)) as f64, (k / side.max(1)) as f64);
        let (mut sum, mut cover) = ([0.0f64; 3], 0.0f64);
        for i in 0..SAMPLES * SAMPLES {
            let at = |n: usize| (n as f64 + 0.5) / SAMPLES as f64;
            let (x, y) = ((px + at(i % SAMPLES) - middle) / scale, (middle - py - at(i / SAMPLES)) / scale);
            let on = [0, 1, 2].map(|a| right[a] * x + up[a] * y);
            // Where the line of sight enters the cube and leaves it, along the eye's axis.
            let (mut far, mut near, mut far_axis, mut near_axis) = (f64::NEG_INFINITY, f64::INFINITY, 0, 0);
            let mut misses = false;
            for a in 0..3 {
                if eye[a].abs() < 1e-9 {
                    misses |= on[a].abs() > 0.5;
                    continue;
                }
                let (t0, t1) = ((-0.5 - on[a]) / eye[a], (0.5 - on[a]) / eye[a]);
                let (lo, hi) = (t0.min(t1), t0.max(t1));
                if lo > far {
                    (far, far_axis) = (lo, a);
                }
                if hi < near {
                    (near, near_axis) = (hi, a);
                }
            }
            if misses || far > near {
                continue;
            }
            let point = |t: f64| [0, 1, 2].map(|a| on[a] + eye[a] * t);
            let front = shade(point(near), near_axis);
            if alpha >= 1.0 {
                (0..3).for_each(|c| sum[c] += front[c]);
                cover += 1.0;
            } else {
                // The far side through the near one.
                let back = shade(point(far), far_axis);
                (0..3).for_each(|c| sum[c] += front[c] * alpha + back[c] * alpha * (1.0 - alpha));
                cover += alpha + alpha * (1.0 - alpha);
            }
        }
        if cover > 0.0 {
            for c in 0..3 {
                pixel[c] = (sum[c] / cover).round().clamp(0.0, 255.0) as u8;
            }
            pixel[3] = (cover / (SAMPLES * SAMPLES) as f64 * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}
