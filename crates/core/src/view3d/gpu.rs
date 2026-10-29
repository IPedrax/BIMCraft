//! What a graphics card needs to draw the 3D view, and what picking needs when there is no picture to read: the scene's triangles
//! as vertices relative to the middle of what is drawn (millimetres far from the origin would overflow f32's resolution, so the
//! shader never sees them), and a bounding volume hierarchy to pick through. No graphics API is in here, so the core stays free
//! of one; the desktop's `gpu.rs` draws it. What a card can't draw (hidden line and wireframe) is refused with a reason, and
//! the CPU picture is drawn instead. Surfaces seen through come after the opaque ones (from [`Mesh::clear_from`]), as the
//! processor draws them in a pass of their own.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{Camera, DATUM, EDGE, LevelMarks, P3, Scene, add, cross, dot, level_marks, mul, sub, unit};
use crate::geom::{self, P2};
use crate::graphics::{Look, VisualStyle};
use crate::model::Id;
use crate::render::Rgb;

/// The most triangles a mesh holds: the card takes 44 bytes a vertex, so that is 211 MB, under the 256 MiB a buffer may be.
pub const MAX_TRIS: usize = 1_600_000;

/// The most of the textures' pixels a mesh holds, bytes: a storage buffer may be 128 MiB.
const MAX_TEXELS: usize = 64 << 20;

/// One vertex: 52 bytes of 32-bit words here, 44 on the card, which takes the normals as 16-bit fractions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// From [`Mesh::origin`], in mm.
    pub position: [f32; 3],
    /// Unit, the curve's where the surface is one, else the face's.
    pub normal: [f32; 3],
    /// Unit, the triangle's own, turned out as its corners wind: a texture lies across it.
    pub face: [f32; 3],
    /// The colour the view shades it in: red, green and blue in the low three bytes.
    pub color: u32,
    /// One more than the place of its element in [`Mesh::owners`].
    pub owner: u32,
    /// One more than its plane's number: where it changes between pixels, an edge is drawn.
    pub plane: u32,
    /// Its texture's place in [`Mesh::skins`], or 0 for none.
    pub tex: u32,
}

/// A texture as a card looks it up (see [`texture::Sampler::at`](crate::texture::Sampler::at)): the tile's size, turn as
/// `(sin, cos)` and offset, where each level of its mip chain starts in [`Mesh::texels`], and its width and height.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skin {
    pub size: [f32; 2],
    pub turn: [f32; 2],
    pub offset: [f32; 2],
    pub levels: Vec<(u32, u32, u32)>,
    /// Millimetres a pixel of the finest level covers.
    texel: f64,
}

/// What a pick finds: the element, and which of its faces (as paint names them; empty where it is on none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub id: Id,
    pub face: &'static str,
}

/// How the card draws a view through a camera: the picture's axes (unit, in the model), where the target is from
/// [`Mesh::origin`], pixels a mm, and how far from the target the picture reaches in depth, mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub back: [f32; 3],
    pub target: [f32; 3],
    pub scale: f32,
    pub depth: f32,
}

struct Tri {
    p: [P3; 3],
    hit: Hit,
    /// Seen through: the nearest of these in front of what is opaque is what a pixel shows.
    clear: bool,
}

/// An element a vertex belongs to: the colour its edges are drawn in, and how see-through the view makes it, percent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Owner {
    pub id: Id,
    pub edge: Rgb,
    pub transparency: u8,
}

struct Node {
    lo: P3,
    hi: P3,
    /// A leaf's triangles, by place in `order`; or the two nodes it splits into.
    first: usize,
    count: usize,
    children: Option<[usize; 2]>,
}

/// How near a pixel's surface is, as the CPU keeps it (an f32), and what it shows.
type Found = Option<(f32, Hit)>;

struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
}

/// A view's shaded surfaces as a card draws them, with the elements' edge colours, the levels it marks and what picks.
pub struct Mesh {
    /// The middle of what is drawn, in the model, mm.
    pub origin: P3,
    /// From [`origin`](Self::origin) to the furthest of it, mm.
    pub radius: f64,
    /// The opaque surfaces' vertices, then from [`clear_from`](Self::clear_from) on those seen through.
    pub vertices: Vec<Vertex>,
    pub clear_from: usize,
    /// Each element a vertex belongs to.
    pub owners: Vec<Owner>,
    /// The textures, by the number a vertex holds (the first, 0, is none), and their pixels: each a word of red, green and blue.
    pub skins: Vec<Skin>,
    pub texels: Vec<u32>,
    levels: Vec<(Id, String, [P3; 4])>,
    level_colors: BTreeMap<Id, Rgb>,
    tris: Vec<Tri>,
    bvh: OnceLock<Bvh>,
}

fn bounds<'a>(points: impl Iterator<Item = &'a P3>) -> Option<(P3, P3)> {
    points.fold(None, |b, p| {
        let (lo, hi) = b.unwrap_or((*p, *p));
        Some((std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i]))))
    })
}

impl Mesh {
    /// The mesh of what `look` shows of `scene`, or what a card can't draw of it as the CPU picture does.
    pub fn new(scene: &Scene, look: &Look) -> Result<Mesh, String> {
        if look.style() != VisualStyle::Shaded {
            return Err("hidden line or wireframe".into());
        }
        if look.section_box().is_some() {
            return Err("a section box, which only the processor cuts yet".into());
        }
        let plain = look.is_plain();
        if scene.scans.iter().any(|s| plain || look.shows(s.owner)) {
            return Err("a point cloud, which only the processor draws yet".into());
        }
        let shown: Vec<usize> = (0..scene.tris.len()).filter(|&i| plain || look.shows(scene.tris[i].owner)).collect();
        if shown.len() > MAX_TRIS {
            return Err(format!("{} triangles, more than the card is given", shown.len()));
        }
        // What the view sees through is drawn after what it doesn't, each in the order the processor draws it.
        let clear = |i: usize| !plain && look.transparency(scene.tris[i].owner) > 0;
        let shown: Vec<usize> = shown.iter().copied().filter(|&i| !clear(i)).chain(shown.iter().copied().filter(|&i| clear(i))).collect();
        let clear_from = shown.iter().take_while(|&&i| !clear(i)).count() * 3;
        // A texture is looked up as the CPU does unless the view colours its surface another way.
        let colour = |t: &super::Tri| if plain { t.color } else { look.surface(t.owner, t.color) };
        let textured = |t: &super::Tri| scene.skins.get(t.tex as usize).is_some_and(Option::is_some) && colour(t) == t.color;
        let levels: Vec<_> = scene.levels.iter().filter(|(id, ..)| plain || look.shows(*id)).cloned().collect();
        let corners = levels.iter().flat_map(|(.., c)| c.iter());
        let points = shown.iter().flat_map(|&i| scene.tris[i].p.iter()).chain(corners);
        let (lo, hi) = bounds(points).unwrap_or(([0.0; 3], [0.0; 3]));
        let origin = mul(add(lo, hi), 0.5);
        let radius = dot(sub(hi, lo), sub(hi, lo)).sqrt() * 0.5;
        let mut mesh = Mesh {
            origin,
            radius,
            vertices: Vec::with_capacity(shown.len() * 3),
            clear_from,
            owners: Vec::new(),
            skins: vec![Skin::default(); scene.skins.len()],
            texels: Vec::new(),
            level_colors: levels.iter().map(|(id, ..)| (*id, look.edge(*id, DATUM))).collect(),
            levels,
            tris: Vec::with_capacity(shown.len()),
            bvh: OnceLock::new(),
        };
        let mut owners: BTreeMap<Id, u32> = BTreeMap::new();
        for i in shown {
            let t = &scene.tris[i];
            let owner = *owners.entry(t.owner).or_insert_with(|| {
                let transparency = if plain { 0 } else { look.transparency(t.owner).min(100) };
                mesh.owners.push(Owner { id: t.owner, edge: look.edge(t.owner, EDGE), transparency });
                mesh.owners.len() as u32
            });
            let Rgb(r, g, b) = colour(t);
            let color = u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16;
            let face = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            let smooth = scene.smooth.get(i).and_then(Option::as_deref);
            let plane = scene.planes.get(i).map_or(i, |p| *p) as u32 + 1;
            let tex = if textured(t) { t.tex } else { 0 };
            for (k, p) in t.p.iter().enumerate() {
                let normal = smooth.map_or(face, |n| unit(n[k])).map(|v| v as f32);
                let position = sub(*p, origin).map(|v| v as f32);
                mesh.vertices.push(Vertex { position, normal, face: face.map(|v| v as f32), color, owner, plane, tex });
            }
            let face = scene.faces.get(i).copied().filter(|f| !f.is_empty()).unwrap_or("");
            mesh.tris.push(Tri { p: t.p, hit: Hit { id: t.owner, face }, clear: clear(i) });
        }
        let used: std::collections::BTreeSet<u32> = mesh.vertices.iter().map(|v| v.tex).filter(|t| *t > 0).collect();
        for id in used {
            let Some(Some(sampler)) = scene.skins.get(id as usize) else { continue };
            let plan = sampler.plan();
            let mut levels = Vec::new();
            for (w, h, rgb) in &plan.levels {
                levels.push((mesh.texels.len() as u32, *w as u32, *h as u32));
                mesh.texels.extend(
                    rgb.as_chunks::<3>().0.iter().map(|p| u32::from(p[0]) | u32::from(p[1]) << 8 | u32::from(p[2]) << 16 | 0xff << 24),
                );
            }
            if mesh.texels.len() * 4 > MAX_TEXELS {
                return Err("textures bigger than the card is given".into());
            }
            let f = |v: f64| v as f32;
            mesh.skins[id as usize] = Skin {
                size: plan.size.map(f),
                turn: [f(plan.turn.0), f(plan.turn.1)],
                offset: plan.offset.map(f),
                levels,
                texel: plan.texel,
            };
        }
        Ok(mesh)
    }

    /// Each skin as a card reads it for pictures of `cam`: its size, turn and offset, then (as words) where in
    /// [`texels`](Self::texels) the level a pixel looks up in starts, its width and its height, and a spare.
    pub fn lookups(&self, cam: &Camera) -> Vec<[u32; 12]> {
        let foot = 1.0 / cam.scale.max(1e-9);
        self.skins
            .iter()
            .map(|s| {
                let level = crate::texture::level_for(s.texel, foot, s.levels.len());
                let (start, w, h) = s.levels.get(level).copied().unwrap_or_default();
                let f = |v: f32| v.to_bits();
                [f(s.size[0]), f(s.size[1]), f(s.turn[0]), f(s.turn[1]), f(s.offset[0]), f(s.offset[1]), 0, 0, start, w, h, 0]
            })
            .collect()
    }

    /// How a card draws the mesh through `cam`.
    pub fn view(&self, cam: &Camera) -> View {
        let (back, right, up) = cam.axes();
        let target = sub(cam.target, self.origin);
        let f = |v: P3| v.map(|x| x as f32);
        View {
            right: f(right),
            up: f(up),
            back: f(back),
            target: f(target),
            scale: cam.scale as f32,
            depth: (self.radius + dot(target, target).sqrt() + 1.0) as f32,
        }
    }

    /// The marks drawn over the surfaces: each shown level's plane and head, `tints` colouring (and showing all four edges of)
    /// those selected.
    pub fn marks(&self, cam: &Camera, tints: &[(Id, Rgb)]) -> Vec<LevelMarks> {
        let color = |id: Id| {
            let own = tints.iter().find(|(t, _)| *t == id).map(|(_, c)| *c);
            own.map(|c| (c, true)).or_else(|| Some((*self.level_colors.get(&id)?, false)))
        };
        level_marks(&self.levels, cam, &color)
    }

    fn bvh(&self) -> &Bvh {
        self.bvh.get_or_init(|| {
            let mut bvh = Bvh { nodes: Vec::new(), order: (0..self.tris.len()).collect() };
            if !self.tris.is_empty() {
                bvh.build(&self.tris, 0, self.tris.len());
            }
            bvh
        })
    }

    /// What the picture shows at pixel `(x, y)`: the nearest surface there (the first drawn where two are as near), and over it
    /// a level's mark where nothing nearer hides it, as the CPU picture's picks do.
    pub fn pick(&self, cam: &Camera, tints: &[(Id, Rgb)], x: f64, y: f64) -> Option<Hit> {
        let (w, h) = (f64::from(cam.width), f64::from(cam.height));
        if !(x >= 0.0 && y >= 0.0 && x < w && y < h) {
            return None;
        }
        let centre = (x.floor() + 0.5, y.floor() + 0.5);
        let (opaque, clear) = self.nearest(cam, centre);
        // A level's mark is hidden by what is opaque only, as the processor's are.
        let behind = opaque.map_or(f64::NEG_INFINITY, |(z, _)| f64::from(z));
        let slack = 1.5 / cam.scale.max(1e-9);
        // A surface seen through shows where it is nearer than anything opaque.
        let seen = clear.filter(|(z, _)| opaque.is_none_or(|(d, _)| *z > d)).or(opaque);
        let mut hit = seen.map(|(_, hit)| hit);
        for marks in self.marks(cam, tints) {
            let at = P2(centre.0, centre.1);
            let on_line = marks.lines.iter().find_map(|([a, b], wide)| covers((cam.project(*a), cam.project(*b)), *wide, centre));
            let head: Vec<_> = marks.head.iter().map(|p| cam.project(*p)).collect();
            let ring: Vec<P2> = head.iter().map(|(x, y, _)| P2(*x, *y)).collect();
            let on_head = geom::contains(&ring, at).then(|| head.iter().map(|p| p.2).sum::<f64>() / 4.0);
            if on_line.or(on_head).is_some_and(|z| behind <= z + slack) {
                hit = Some(Hit { id: marks.id, face: "" });
            }
        }
        hit.filter(|h| !crate::link::is_linked(h.id))
    }

    /// The nearest opaque triangle covering the centre of a pixel, and the nearest seen through, as the CPU's rasterizer finds
    /// them: how near, as an f32 as it keeps it, and which. The first of triangles as near wins.
    /// The point of the model's surface the picture through `cam` shows at pixel `(x, y)`: the nearest surface there, seen
    /// through as `pick` sees it. None where there's nothing.
    pub fn point(&self, cam: &Camera, x: f64, y: f64) -> Option<P3> {
        let (w, h) = (f64::from(cam.width), f64::from(cam.height));
        if !(x >= 0.0 && y >= 0.0 && x < w && y < h) {
            return None;
        }
        let centre = (x.floor() + 0.5, y.floor() + 0.5);
        let (opaque, clear) = self.nearest(cam, centre);
        let seen = clear.filter(|(z, _)| opaque.is_none_or(|(d, _)| *z > d)).or(opaque);
        seen.map(|(z, _)| cam.at_depth(centre, f64::from(z)))
    }

    fn nearest(&self, cam: &Camera, (x, y): (f64, f64)) -> (Found, Found) {
        let Some(bvh) = Some(self.bvh()).filter(|b| !b.nodes.is_empty()) else { return (None, None) };
        // Opaque, then seen through.
        let mut found: [Option<(f32, usize)>; 2] = [None, None];
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let Some(node) = bvh.nodes.get(n) else { continue };
            // The box's picture is inside the rectangle its corners make.
            let corners = (0..8).map(|k| cam.project([0, 1, 2].map(|a| if k >> a & 1 == 0 { node.lo[a] } else { node.hi[a] })));
            let (lo, hi) = corners.fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), (cx, cy, _)| {
                (P2(lo.0.min(cx), lo.1.min(cy)), P2(hi.0.max(cx), hi.1.max(cy)))
            });
            if x < lo.0 - 1e-6 || x > hi.0 + 1e-6 || y < lo.1 - 1e-6 || y > hi.1 + 1e-6 {
                continue;
            }
            if let Some(children) = node.children {
                stack.extend(children);
                continue;
            }
            for &i in bvh.order.get(node.first..node.first + node.count).unwrap_or_default() {
                let Some(t) = self.tris.get(i) else { continue };
                let [a, b, c] = t.p.map(|p| cam.project(p));
                let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                if area.is_nan() || area.abs() <= 1e-12 {
                    continue;
                }
                let wa = ((b.0 - x) * (c.1 - y) - (b.1 - y) * (c.0 - x)) / area;
                let wb = ((c.0 - x) * (a.1 - y) - (c.1 - y) * (a.0 - x)) / area;
                let wc = 1.0 - wa - wb;
                if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                    continue;
                }
                let z = (wa * a.2 + wb * b.2 + wc * c.2) as f32;
                let found = &mut found[usize::from(t.clear)];
                if found.is_none_or(|(d, j)| z > d || z == d && i < j) {
                    *found = Some((z, i));
                }
            }
        }
        let hit = |f: Option<(f32, usize)>| f.and_then(|(z, i)| Some((z, self.tris.get(i)?.hit)));
        (hit(found[0]), hit(found[1]))
    }
}

/// How near the camera a line from `a` to `b` (each a pixel and its nearness) is where it covers the pixel whose centre is
/// `(x, y)`, drawn `wide` pixels wide as the picture's `outline` does: a sample of it, one to the pixel along its longer side,
/// stamps the pixels within `wide / 2` of it, so the pixel is covered where a sample lies within that of its centre.
fn covers((a, b): ((f64, f64, f64), (f64, f64, f64)), wide: u8, (x, y): (f64, f64)) -> Option<f64> {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil();
    if !steps.is_finite() || steps > 4.0 * f64::from(super::MAX_SIDE) {
        return None;
    }
    let steps = steps.max(1.0);
    let half = f64::from(wide) / 2.0;
    // The samples whose longer-side coordinate is within `half` of the centre's.
    let (along, at, step) =
        if (b.0 - a.0).abs() >= (b.1 - a.1).abs() { (a.0, x, (b.0 - a.0) / steps) } else { (a.1, y, (b.1 - a.1) / steps) };
    let (first, last) = if step.abs() < 1e-12 {
        (0.0, steps)
    } else {
        let (i, j) = ((at - half - along) / step, (at + half - along) / step);
        (i.min(j).floor().max(0.0), i.max(j).ceil().min(steps))
    };
    (first as u64..=last as u64).find_map(|i| {
        let t = i as f64 / steps;
        let (sx, sy, sz) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
        let inside = |s: f64, c: f64| s >= c - half && s < c + half;
        (inside(sx, x) && inside(sy, y)).then_some(sz)
    })
}

impl Bvh {
    /// The node for `order[first..first + count]`, split in two at the middle of its longest side while it holds more than 8.
    fn build(&mut self, tris: &[Tri], first: usize, count: usize) -> usize {
        let range = first..first + count;
        let (lo, hi) = bounds(self.order[range.clone()].iter().flat_map(|&i| tris[i].p.iter())).unwrap_or(([0.0; 3], [0.0; 3]));
        let id = self.nodes.len();
        self.nodes.push(Node { lo, hi, first, count, children: None });
        if count > 8 {
            let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap_or(0);
            let centre = |i: usize| tris[i].p.iter().map(|p| p[axis]).sum::<f64>();
            self.order[range].select_nth_unstable_by(count / 2, |&a, &b| centre(a).total_cmp(&centre(b)).then(a.cmp(&b)));
            let half = count / 2;
            let children = [self.build(tris, first, half), self.build(tris, first + half, count - half)];
            self.nodes[id].children = Some(children);
        }
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::view3d::{AZIMUTH, ELEVATION};
    use serde_json::json;

    fn sample() -> Session {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        s
    }

    /// Whether picking `s`'s 3D view through its mesh finds what the processor's picture shows, at every third pixel.
    fn picks_as_the_picture(s: &Session) {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let tints = [(s.model.levels().first().unwrap().0, Rgb(0, 120, 215))];
        let image = scene.render_in(&cam, &tints, &look);
        let (mut agree, mut total) = (0, 0);
        for y in (0..480).step_by(3) {
            for x in (0..640).step_by(3) {
                let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let cpu = image.pick(x, y).map(|id| Hit { id, face: image.face(x, y).unwrap_or("") });
                total += 1;
                // Where a level's mark is picked, the CPU has no face either.
                agree += usize::from(mesh.pick(&cam, &tints, x, y) == cpu);
            }
        }
        assert_eq!(agree, total, "only {agree} of {total} pixels pick what the picture shows");
    }

    #[test]
    fn picking_the_mesh_finds_what_the_cpu_picture_shows() {
        picks_as_the_picture(&sample());
    }

    #[test]
    fn picking_through_what_the_view_sees_through_finds_what_the_cpu_picture_shows() {
        let mut s = sample();
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
        let mesh = Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
        assert!(mesh.clear_from > 0 && mesh.clear_from < mesh.vertices.len(), "opaque first, then the walls");
        let wall = mesh.owners.iter().find(|o| s.model.elements.get(o.id).is_some_and(|e| e.category() == "wall")).unwrap();
        assert_eq!(wall.transparency, 50);
        picks_as_the_picture(&s);
        // Everything seen through: nothing opaque to stand in front of.
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"floors": {"transparency": 80}, "roofs": {"transparency": 30}}}))
            .unwrap();
        picks_as_the_picture(&s);
    }

    #[test]
    fn a_mesh_is_refused_for_what_only_the_processor_draws() {
        let mut s = sample();
        let scene = Scene::new(&s.model);
        assert!(Mesh::new(&scene, &Look::of_3d(&s.model)).is_ok());
        s.execute("visual_style", &json!({"style": "wireframe"})).unwrap();
        let scene = Scene::new(&s.model);
        let err = Mesh::new(&scene, &Look::of_3d(&s.model)).err().unwrap();
        assert!(err.contains("wireframe"), "{err}");
        s.execute("visual_style", &json!({"style": "shaded"})).unwrap();
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
        assert!(Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).is_ok(), "the card draws what is seen through");
    }

    #[test]
    fn vertices_are_near_the_origin_whatever_the_model_is_far_from_it() {
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        assert_eq!(mesh.vertices.len() % 3, 0);
        let reach = mesh.radius as f32 * 1.001 + 1.0;
        assert!(mesh.vertices.iter().all(|v| v.position.iter().all(|c| c.abs() <= reach)));
        assert!(mesh.vertices.iter().all(|v| v.owner >= 1 && v.owner as usize <= mesh.owners.len() && v.plane >= 1));
        assert_eq!(mesh.clear_from, mesh.vertices.len(), "nothing seen through");
    }
}
