//! What a graphics card needs to draw the 3D view, and what picking needs when there is no picture to read: the scene's triangles
//! as vertices relative to the middle of what is drawn (millimetres far from the origin would overflow f32's resolution, so the
//! shader never sees them), and a bounding volume hierarchy to pick through. No graphics API is in here, so the core stays free
//! of one; the desktop's `gpu.rs` draws it. What a card can't draw (hidden line and wireframe) is refused with a reason, and
//! the CPU picture is drawn instead. Surfaces seen through come after the opaque ones (from [`Mesh::clear_from`]), as the
//! processor draws them in a pass of their own.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{Camera, DATUM, EDGE, LevelMarks, NEAR, P3, Scene, add, cross, dot, level_marks, mul, sub, unit, weighed};
use crate::component_geometry::ComponentInstance;
use crate::geom::{self, P2};
use crate::graphics::{Look, VisualStyle};
use crate::model::Id;
use crate::render::Rgb;

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
    /// In perspective: the lens's focal length in px, how far the eye is from the target, mm, and the card's depth (larger
    /// nearer, 1 at [`NEAR`] in front of the eye and 0 as far past the target as the mesh reaches) as `a + b / ahead` for a
    /// point `ahead` mm in front of the eye.
    pub perspective: Option<Perspective>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Perspective {
    pub focal: f32,
    pub eye: f32,
    pub a: f32,
    pub b: f32,
}

struct Tri {
    p: [P3; 3],
    hit: Hit,
    /// Seen through: the nearest of these in front of what is opaque is what a pixel shows.
    clear: bool,
    /// Its place in the scene: of triangles as near, the first there is what a pick finds.
    key: usize,
}

/// A copy of one of [`Mesh::shapes`], as the card places it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    /// Where its family's origin is, from [`Mesh::origin`], mm.
    pub at: [f32; 3],
    /// The sine and cosine of its turn about the vertical.
    pub turn: [f32; 2],
    /// One more than the place of its element in [`Mesh::owners`]; and of its element as the part seen through (with how
    /// see-through), 0 where none is.
    pub owner: u32,
    pub clear_owner: u32,
    /// Where its first triangle's plane number is in [`Mesh::instance_planes`].
    pub planes: u32,
}

/// A shape the card holds once: its vertices in [`Mesh::shape_vertices`] (in its family's axes, each vertex's owner and plane
/// 0, as each copy has its own), those that are opaque first and from `clear_from` those seen through, and its copies in
/// [`Mesh::instances`].
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub vertices: std::ops::Range<usize>,
    pub clear_from: usize,
    pub instances: std::ops::Range<usize>,
}

/// A copy, as a pick places it: the triangles it finds are those the scene holds, to the last bit.
struct Copy {
    place: ComponentInstance,
    /// Its shape's place in [`Mesh::shapes`], and its first triangle's in the scene.
    shape: usize,
    key: usize,
}

/// A shape, as a pick reads it: its triangles in its family's axes, the face each is on, and a hierarchy over them.
struct Local {
    tris: Vec<[P3; 3]>,
    faces: Vec<&'static str>,
    /// Each triangle's place among a copy's in the scene, and whether it is seen through.
    keys: Vec<usize>,
    clear: Vec<bool>,
    bvh: OnceLock<Bvh>,
}

impl Local {
    fn boxes(&self) -> Vec<(P3, P3)> {
        self.tris.iter().map(|t| bounds(t.iter()).unwrap_or_default()).collect()
    }

    fn bvh(&self) -> &Bvh {
        self.bvh.get_or_init(|| Bvh::over(&self.boxes(), &|| false).unwrap_or_else(Bvh::empty))
    }
}

/// What every copy of a shape must share to be drawn from one: each triangle's colour, texture and face, and its corners'
/// normals where it is curved, in the shape's axes.
struct Template {
    /// How see-through each is: those seen through all alike.
    alpha: Vec<u8>,
    color: Vec<u32>,
    tex: Vec<u32>,
    face: Vec<&'static str>,
    smooth: Vec<Option<[P3; 3]>>,
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
    /// The opaque surfaces' vertices, then from [`clear_from`](Self::clear_from) on those seen through: all but the copies of
    /// [`shapes`](Self::shapes).
    pub vertices: Vec<Vertex>,
    pub clear_from: usize,
    /// The components drawn as one shape placed many times, each opaque: the shapes, their vertices, their copies, and each
    /// copy's triangles' plane numbers (one more than the plane's, as a vertex holds it), which the scene numbers across the
    /// whole model, so the edges between copies, and between a copy and what it touches, are where they are without them.
    pub shapes: Vec<Shape>,
    pub shape_vertices: Vec<Vertex>,
    pub instances: Vec<Instance>,
    pub instance_planes: Vec<u32>,
    copies: Vec<Copy>,
    locals: Vec<Local>,
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
        Self::new_cancellable(scene, look, &|| false).and_then(|mesh| mesh.ok_or_else(|| "mesh build was cancelled".into()))
    }

    /// The same mesh with no shapes: every copy's triangles its own, as they were before the card placed copies. What the
    /// placed copies are held to.
    pub fn flat(scene: &Scene, look: &Look) -> Result<Mesh, String> {
        Self::build(scene, look, &|| false, false).and_then(|mesh| mesh.ok_or_else(|| "mesh build was cancelled".into()))
    }

    /// The same mesh, abandoning a superseded view between batches of triangles.
    pub fn new_cancellable(scene: &Scene, look: &Look, cancelled: &dyn Fn() -> bool) -> Result<Option<Mesh>, String> {
        Self::build(scene, look, cancelled, true)
    }

    /// How many triangles it draws, each copy's counted.
    pub fn triangles(&self) -> usize {
        self.vertices.len() / 3 + self.shapes.iter().map(|s| s.vertices.len() / 3 * s.instances.len()).sum::<usize>()
    }

    fn build(scene: &Scene, look: &Look, cancelled: &dyn Fn() -> bool, instancing: bool) -> Result<Option<Mesh>, String> {
        if cancelled() {
            return Ok(None);
        }
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
        let mut shown = Vec::new();
        for i in 0..scene.tris.len() {
            if i % 8192 == 0 && cancelled() {
                return Ok(None);
            }
            if plain || look.shows(scene.tris[i].owner) {
                shown.push(i);
            }
        }
        // What the view sees through is drawn after what it doesn't, each in the order the processor draws it.
        let clear = |i: usize| scene.tris[i].alpha(look) > 0;
        let shown: Vec<usize> = shown.iter().copied().filter(|&i| !clear(i)).chain(shown.iter().copied().filter(|&i| clear(i))).collect();
        // A texture is looked up as the CPU does unless the view colours its surface another way.
        let colour = |t: &super::Tri| if plain { t.color } else { look.surface(t.owner, t.color) };
        let textured = |t: &super::Tri| scene.skins.get(t.texture() as usize).is_some_and(Option::is_some) && colour(t) == t.color;
        let packed = |t: &super::Tri| {
            let Rgb(r, g, b) = colour(t);
            u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16
        };
        let tex_of = |t: &super::Tri| if textured(t) { t.texture() } else { 0 };
        // The copies drawn from their shape: shown, opaque, and in every triangle coloured, textured, faced and curved as the
        // first such copy of the shape is.
        let mut templates: Vec<Option<Template>> = (0..scene.shapes.len()).map(|_| None).collect();
        let mut chosen: Vec<(usize, ComponentInstance)> = Vec::new();
        let mut taken = vec![false; if instancing && !scene.placements.is_empty() { scene.tris.len() } else { 0 }];
        for (n, &(first, place)) in scene.placements.iter().enumerate() {
            if !instancing {
                break;
            }
            if n % 256 == 0 && cancelled() {
                return Ok(None);
            }
            let Some(local) = scene.shapes.get(place.shape) else { continue };
            let Some(tris) = scene.tris.get(first..first + local.len()) else { continue };
            let alpha_of = |t: &super::Tri| t.alpha(look).min(100);
            if !(plain || look.shows(place.owner)) {
                continue;
            }
            // One owner for what is seen through of a copy, so one way see-through.
            let mut clear_alphas = tris.iter().map(alpha_of).filter(|a| *a > 0);
            if clear_alphas.next().is_some_and(|a| clear_alphas.any(|b| b != a)) {
                continue;
            }
            let face_at = |k: usize| scene.faces.get(first + k).copied().filter(|f| !f.is_empty()).unwrap_or("");
            let smooth_at = |k: usize| scene.smooth.get(first + k).and_then(Option::as_deref).map(|c| c.map(unit));
            let fits = match templates.get(place.shape) {
                Some(None) => {
                    let unturn = |n: P3| [n[0] * place.cos + n[1] * place.sin, -n[0] * place.sin + n[1] * place.cos, n[2]];
                    let template = Template {
                        alpha: tris.iter().map(alpha_of).collect(),
                        color: tris.iter().map(packed).collect(),
                        tex: tris.iter().map(tex_of).collect(),
                        face: (0..tris.len()).map(face_at).collect(),
                        smooth: (0..tris.len()).map(|k| smooth_at(k).map(|c| c.map(unturn))).collect(),
                    };
                    if let Some(slot) = templates.get_mut(place.shape) {
                        *slot = Some(template);
                    }
                    true
                }
                Some(Some(template)) => tris.iter().enumerate().all(|(k, t)| {
                    let near = |a: P3, b: P3| (0..3).all(|i| (a[i] - b[i]).abs() <= 1e-6);
                    let curved = match (template.smooth.get(k).copied().flatten(), smooth_at(k)) {
                        (None, None) => true,
                        (Some(own), Some(seen)) => (0..3).all(|c| near(place.rotate(own[c]), seen[c])),
                        _ => false,
                    };
                    template.alpha.get(k) == Some(&alpha_of(t))
                        && template.color.get(k) == Some(&packed(t))
                        && template.tex.get(k) == Some(&tex_of(t))
                        && template.face.get(k) == Some(&face_at(k))
                        && curved
                }),
                None => false,
            };
            if fits {
                taken.get_mut(first..first + local.len()).into_iter().flatten().for_each(|t| *t = true);
                chosen.push((first, place));
            }
        }
        let is_taken = |i: usize| taken.get(i).copied().unwrap_or(false);
        let clear_from = shown.iter().filter(|&&i| !clear(i) && !is_taken(i)).count() * 3;
        let drawn = shown.iter().filter(|&&i| !is_taken(i)).count();
        let levels: Vec<_> = scene.levels.iter().filter(|(id, ..)| plain || look.shows(*id)).cloned().collect();
        let corners = levels.iter().flat_map(|(.., c)| c.iter());
        let points = shown.iter().flat_map(|&i| scene.tris[i].p.iter()).chain(corners);
        let (lo, hi) = bounds(points).unwrap_or(([0.0; 3], [0.0; 3]));
        let origin = mul(add(lo, hi), 0.5);
        let radius = dot(sub(hi, lo), sub(hi, lo)).sqrt() * 0.5;
        let mut mesh = Mesh {
            origin,
            radius,
            vertices: Vec::new(),
            clear_from,
            shapes: Vec::new(),
            shape_vertices: Vec::new(),
            instances: Vec::new(),
            instance_planes: Vec::new(),
            copies: Vec::new(),
            locals: Vec::new(),
            owners: Vec::new(),
            skins: vec![Skin::default(); scene.skins.len()],
            texels: Vec::new(),
            level_colors: levels.iter().map(|(id, ..)| (*id, look.edge(*id, DATUM))).collect(),
            levels,
            tris: Vec::new(),
            bvh: OnceLock::new(),
        };
        // However many there are: a model too big for this machine's memory is said, not an abort.
        let room = |_| "more than this machine's memory holds".to_string();
        mesh.vertices.try_reserve_exact(drawn.saturating_mul(3)).map_err(room)?;
        mesh.tris.try_reserve_exact(drawn).map_err(room)?;
        let mut owners: BTreeMap<(Id, u8), u32> = BTreeMap::new();
        let mut owner_of = |mesh: &mut Mesh, id: Id, alpha: u8| {
            *owners.entry((id, alpha)).or_insert_with(|| {
                mesh.owners.push(Owner { id, edge: look.edge(id, EDGE), transparency: alpha });
                mesh.owners.len() as u32
            })
        };
        for (k, i) in shown.into_iter().enumerate() {
            if k % 8192 == 0 && cancelled() {
                return Ok(None);
            }
            let t = &scene.tris[i];
            let alpha = t.alpha(look).min(100);
            // The owners numbered in the order they are met, a copy's among them.
            let owner = owner_of(&mut mesh, t.owner, alpha);
            if is_taken(i) {
                continue;
            }
            let color = packed(t);
            let face = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            let smooth = scene.smooth.get(i).and_then(Option::as_deref);
            let plane = scene.planes.get(i).map_or(i, |p| *p) as u32 + 1;
            let tex = tex_of(t);
            for (k, p) in t.p.iter().enumerate() {
                let normal = smooth.map_or(face, |n| unit(n[k])).map(|v| v as f32);
                let position = sub(*p, origin).map(|v| v as f32);
                mesh.vertices.push(Vertex { position, normal, face: face.map(|v| v as f32), color, owner, plane, tex });
            }
            let face = scene.faces.get(i).copied().filter(|f| !f.is_empty()).unwrap_or("");
            mesh.tris.push(Tri { p: t.p, hit: Hit { id: t.owner, face }, clear: clear(i), key: i });
        }
        // Each shape once, in its family's axes, and its copies, in the order of the scene.
        let too_many = || "more copies' triangles than the card counts".to_string();
        for (shape, template) in templates.into_iter().enumerate() {
            if cancelled() {
                return Ok(None);
            }
            let (Some(template), Some(local)) = (template, scene.shapes.get(shape)) else { continue };
            let copies: Vec<&(usize, ComponentInstance)> = chosen.iter().filter(|(_, c)| c.shape == shape).collect();
            if copies.is_empty() {
                continue;
            }
            let start = mesh.shape_vertices.len();
            // What is opaque, then what is seen through, each in the scene's order.
            let seen_through = |k: &usize| template.alpha.get(*k).is_some_and(|a| *a > 0);
            let order: Vec<usize> = (0..local.len()).filter(|k| !seen_through(k)).chain((0..local.len()).filter(seen_through)).collect();
            let clear_alpha = template.alpha.iter().copied().find(|a| *a > 0);
            let mut clear_from = None;
            for &k in &order {
                let Some(t) = local.get(k) else { continue };
                if seen_through(&k) && clear_from.is_none() {
                    clear_from = Some(mesh.shape_vertices.len());
                }
                let face = unit(cross(sub(t[1], t[0]), sub(t[2], t[0])));
                let smooth = template.smooth.get(k).copied().flatten();
                for (c, p) in t.iter().enumerate() {
                    let normal = smooth.map_or(face, |n| n[c]).map(|v| v as f32);
                    mesh.shape_vertices.push(Vertex {
                        position: p.map(|v| v as f32),
                        normal,
                        face: face.map(|v| v as f32),
                        color: template.color.get(k).copied().unwrap_or(0),
                        owner: 0,
                        plane: 0,
                        tex: template.tex.get(k).copied().unwrap_or(0),
                    });
                }
            }
            let index = mesh.shapes.len();
            let first_instance = mesh.instances.len();
            for &&(first, place) in &copies {
                let owner = owner_of(&mut mesh, place.owner, 0);
                let clear_owner = clear_alpha.map_or(0, |a| owner_of(&mut mesh, place.owner, a));
                let planes = u32::try_from(mesh.instance_planes.len()).map_err(|_| too_many())?;
                mesh.instance_planes.try_reserve(local.len()).map_err(room)?;
                for i in order.iter().map(|k| first + k) {
                    mesh.instance_planes.push(scene.planes.get(i).map_or(i, |p| *p) as u32 + 1);
                }
                let at = sub(place.at, origin).map(|v| v as f32);
                mesh.instances.push(Instance { at, turn: [place.sin as f32, place.cos as f32], owner, clear_owner, planes });
                mesh.copies.push(Copy { place, shape: index, key: first });
            }
            let end = mesh.shape_vertices.len();
            mesh.shapes.push(Shape {
                vertices: start..end,
                clear_from: clear_from.unwrap_or(end),
                instances: first_instance..mesh.instances.len(),
            });
            mesh.locals.push(Local {
                tris: order.iter().filter_map(|k| local.get(*k).copied()).collect(),
                faces: order.iter().map(|k| template.face.get(*k).copied().unwrap_or("")).collect(),
                clear: order.iter().map(seen_through).collect(),
                keys: order,
                bvh: OnceLock::new(),
            });
        }
        let used: std::collections::BTreeSet<u32> =
            mesh.vertices.iter().chain(&mesh.shape_vertices).map(|v| v.tex).filter(|t| *t > 0).collect();
        for id in used {
            if cancelled() {
                return Ok(None);
            }
            let Some(Some(sampler)) = scene.skins.get(id as usize) else { continue };
            let plan = sampler.plan();
            let mut levels = Vec::new();
            for (w, h, rgb) in &plan.levels {
                levels.push((mesh.texels.len() as u32, *w as u32, *h as u32));
                mesh.texels.extend(
                    rgb.as_chunks::<3>().0.iter().map(|p| u32::from(p[0]) | u32::from(p[1]) << 8 | u32::from(p[2]) << 16 | 0xff << 24),
                );
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
        if cancelled() { Ok(None) } else { Ok(Some(mesh)) }
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
        let depth = self.radius + dot(target, target).sqrt() + 1.0;
        let perspective = cam.focal().zip(cam.eye_distance()).map(|(focal, eye)| {
            let far = (eye + depth).max(2.0 * NEAR);
            let b = NEAR * far / (far - NEAR);
            Perspective { focal: focal as f32, eye: eye as f32, a: (-NEAR / (far - NEAR)) as f32, b: b as f32 }
        });
        View { right: f(right), up: f(up), back: f(back), target: f(target), scale: cam.scale as f32, depth: depth as f32, perspective }
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

    /// What the hierarchy is over: each triangle drawn of its own, then each copy, as boxes.
    fn boxes(&self) -> Vec<(P3, P3)> {
        let own = self.tris.iter().map(|t| bounds(t.p.iter()).unwrap_or_default());
        let copies = self.copies.iter().map(|c| {
            let local = self.locals.get(c.shape).and_then(|l| bounds(l.tris.iter().flatten())).unwrap_or_default();
            let [lo, hi] = c.place.bounds([local.0, local.1]);
            (lo, hi)
        });
        own.chain(copies).collect()
    }

    fn bvh(&self) -> &Bvh {
        self.bvh.get_or_init(|| Bvh::over(&self.boxes(), &|| false).unwrap_or_else(Bvh::empty))
    }

    /// Build the picking hierarchy while the mesh is still on a view worker.
    pub fn prepare_picking(&self) {
        self.bvh();
    }

    /// Build the hierarchy while a worker owns this mesh, abandoning a superseded snapshot.
    pub fn prepare_picking_cancellable(&self, cancelled: &dyn Fn() -> bool) -> bool {
        if self.bvh.get().is_some() {
            return true;
        }
        if cancelled() {
            return false;
        }
        for local in &self.locals {
            if local.bvh.get().is_none() {
                let Some(bvh) = Bvh::over(&local.boxes(), cancelled) else { return false };
                let _ = local.bvh.set(bvh);
            }
        }
        let Some(bvh) = Bvh::over(&self.boxes(), cancelled) else { return false };
        if cancelled() {
            return false;
        }
        let _ = self.bvh.set(bvh);
        true
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
        // A surface seen through shows where it is nearer than anything opaque.
        let seen = clear.filter(|(z, _)| opaque.is_none_or(|(d, _)| *z > d)).or(opaque);
        let mut hit = seen.map(|(_, hit)| hit);
        for marks in self.marks(cam, tints) {
            let at = P2(centre.0, centre.1);
            let on_line = marks.lines.iter().find_map(|([a, b], wide)| covers((cam.project(*a), cam.project(*b)), *wide, centre));
            let head: Vec<_> = marks.head.iter().map(|p| cam.project(*p)).collect();
            let ring: Vec<P2> = head.iter().map(|(x, y, _)| P2(*x, *y)).collect();
            let whole = ring.iter().all(|q| q.0.is_finite() && q.1.is_finite());
            let on_head = (whole && geom::contains(&ring, at)).then(|| head.iter().map(|p| p.2).sum::<f64>() / 4.0);
            if on_line.or(on_head).is_some_and(|z| behind <= z + cam.slack(1.5, z)) {
                hit = Some(Hit { id: marks.id, face: "" });
            }
        }
        hit.filter(|h| !crate::link::is_linked(h.id))
    }

    /// The nearest opaque triangle covering the centre of a pixel, and the nearest seen through, as the CPU's rasterizer finds
    /// them: how near, as an f32 as it keeps it, and which. The first of triangles as near, in the scene, wins.
    fn nearest(&self, cam: &Camera, (x, y): (f64, f64)) -> (Found, Found) {
        let Some(bvh) = Some(self.bvh()).filter(|b| !b.nodes.is_empty()) else { return (None, None) };
        // Opaque, then seen through.
        let mut found: [Option<(f32, usize, Hit)>; 2] = [None, None];
        let project = cam.projector();
        // Whether the pixel is outside a box's picture, which is inside the rectangle its corners make where all of it is in
        // front of the eye.
        let misses = |(lo, hi): (P3, P3)| {
            let corners = (0..8).map(|k| [0, 1, 2].map(|a| if k >> a & 1 == 0 { lo[a] } else { hi[a] }));
            if !corners.clone().all(|p| project.ahead(p) >= NEAR) {
                return false;
            }
            let (lo, hi) =
                corners.map(|p| project.at(p)).fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), (cx, cy, _)| {
                    (P2(lo.0.min(cx), lo.1.min(cy)), P2(hi.0.max(cx), hi.1.max(cy)))
                });
            x < lo.0 - 1e-6 || x > hi.0 + 1e-6 || y < lo.1 - 1e-6 || y > hi.1 + 1e-6
        };
        let mut test = |p: &[P3; 3], clear: bool, key: usize, hit: Hit| {
            // In front of the eye, as the processor cuts it.
            let (parts, count) = project.in_front(p);
            for part in parts.iter().take(count) {
                let [a, b, c] = weighed(p, part).map(|q| project.at(q));
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
                let found = &mut found[usize::from(clear)];
                if found.is_none_or(|(d, j, _)| z > d || z == d && key < j) {
                    *found = Some((z, key, hit));
                }
            }
        };
        let own = self.tris.len();
        bvh.walk(&|n| (n.lo, n.hi), &misses, &mut |i| {
            if let Some(t) = self.tris.get(i) {
                test(&t.p, t.clear, t.key, t.hit);
            } else if let Some(copy) = i.checked_sub(own).and_then(|k| self.copies.get(k))
                && let Some(local) = self.locals.get(copy.shape)
            {
                // Its shape's hierarchy, each box placed as the copy is.
                let placed = |n: &Node| {
                    let [lo, hi] = copy.place.bounds([n.lo, n.hi]);
                    (lo, hi)
                };
                local.bvh().walk(&placed, &misses, &mut |k| {
                    if let Some(t) = local.tris.get(k) {
                        let hit = Hit { id: copy.place.owner, face: local.faces.get(k).copied().unwrap_or("") };
                        let (clear, key) = (local.clear.get(k).copied().unwrap_or(false), local.keys.get(k).copied().unwrap_or(k));
                        test(&t.map(|q| copy.place.place(q)), clear, copy.key + key, hit);
                    }
                });
            }
        });
        let hit = |f: Option<(f32, usize, Hit)>| f.map(|(z, _, hit)| (z, hit));
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
    fn empty() -> Bvh {
        Bvh { nodes: Vec::new(), order: Vec::new() }
    }

    /// The hierarchy over `boxes`; none when `cancelled`.
    fn over(boxes: &[(P3, P3)], cancelled: &dyn Fn() -> bool) -> Option<Bvh> {
        let mut bvh = Bvh { nodes: Vec::new(), order: (0..boxes.len()).collect() };
        if !boxes.is_empty() {
            bvh.build(boxes, 0, boxes.len(), cancelled)?;
        }
        Some(bvh)
    }

    /// The node for `order[first..first + count]`, split in two at the middle of its longest side while it holds more than 8.
    fn build(&mut self, boxes: &[(P3, P3)], first: usize, count: usize, cancelled: &dyn Fn() -> bool) -> Option<usize> {
        if cancelled() {
            return None;
        }
        let range = first..first + count;
        let corners = self.order[range.clone()].iter().filter_map(|&i| boxes.get(i)).flat_map(|(lo, hi)| [lo, hi]);
        let (lo, hi) = bounds(corners).unwrap_or(([0.0; 3], [0.0; 3]));
        let id = self.nodes.len();
        self.nodes.push(Node { lo, hi, first, count, children: None });
        if count > 8 {
            let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap_or(0);
            let centre = |i: usize| boxes.get(i).map_or(0.0, |(lo, hi)| lo[axis] + hi[axis]);
            self.order[range].select_nth_unstable_by(count / 2, |&a, &b| centre(a).total_cmp(&centre(b)).then(a.cmp(&b)));
            let half = count / 2;
            let children = [self.build(boxes, first, half, cancelled)?, self.build(boxes, first + half, count - half, cancelled)?];
            self.nodes[id].children = Some(children);
        }
        Some(id)
    }

    /// Each item in a leaf whose box, as `bounds` gives it, the pixel isn't outside of (`misses`).
    fn walk(&self, bounds: &dyn Fn(&Node) -> (P3, P3), misses: &dyn Fn((P3, P3)) -> bool, leaf: &mut dyn FnMut(usize)) {
        let mut stack = if self.nodes.is_empty() { Vec::new() } else { vec![0usize] };
        while let Some(n) = stack.pop() {
            let Some(node) = self.nodes.get(n) else { continue };
            if misses(bounds(node)) {
                continue;
            }
            if let Some(children) = node.children {
                stack.extend(children);
                continue;
            }
            for &i in self.order.get(node.first..node.first + node.count).unwrap_or_default() {
                leaf(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::view3d::{AZIMUTH, ELEVATION};
    use serde_json::json;
    use std::cell::Cell;

    fn sample() -> Session {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        s
    }

    #[test]
    fn cancelled_mesh_and_picking_builds_publish_nothing_and_can_retry() {
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let calls = Cell::new(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() > 2
        };
        assert!(Mesh::new_cancellable(&scene, &look, &cancel).unwrap().is_none());
        let mesh = Mesh::new(&scene, &look).unwrap();
        calls.set(0);
        assert!(!mesh.prepare_picking_cancellable(&cancel));
        assert!(mesh.bvh.get().is_none());
        assert!(mesh.prepare_picking_cancellable(&|| false));
        assert!(mesh.bvh.get().is_some());
    }

    /// Whether picking `s`'s 3D view through its mesh finds what the processor's picture shows, at every third pixel.
    fn picks_as_the_picture(s: &Session) {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        picks_through(s, scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look));
    }

    /// Whether picking `s`'s 3D view through its mesh, seen through `cam`, finds what the processor's picture shows.
    fn picks_through(s: &Session, cam: Camera) {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
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
    fn picking_the_mesh_in_perspective_finds_what_the_cpu_picture_shows_from_afar_and_from_within() {
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
        picks_through(&s, cam);
        // The eye inside the house, 1.6 m up, through a wide lens: what is behind it is cut away alike.
        picks_through(&s, crate::view3d::inside(&s.model, cam));
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

    /// Each copy of each shape, placed, against the flat mesh's own triangles of that element: the same count, owner, plane,
    /// colour and texture, the corners within a hundredth of a millimetre and the normals within a 16-bit fraction.
    fn copies_hold_the_flat_triangles(mesh: &Mesh, flat: &Mesh) {
        assert_eq!(mesh.triangles(), flat.triangles());
        assert_eq!(mesh.owners, flat.owners, "the owners numbered alike");
        assert!(flat.shapes.is_empty() && flat.instances.is_empty());
        for shape in &mesh.shapes {
            let local = &mesh.shape_vertices[shape.vertices.clone()];
            for inst in &mesh.instances[shape.instances.clone()] {
                // The flat mesh's opaque ones first, then those seen through: the shape's order.
                let own: Vec<&Vertex> = flat.vertices.iter().filter(|v| v.owner == inst.owner).collect();
                let clear = flat.vertices.iter().filter(|v| inst.clear_owner != 0 && v.owner == inst.clear_owner);
                let own: Vec<&Vertex> = own.into_iter().chain(clear).collect();
                assert_eq!(own.len(), local.len());
                let [sin, cos] = inst.turn;
                let turn = |v: [f32; 3]| [v[0] * cos - v[1] * sin, v[0] * sin + v[1] * cos, v[2]];
                for (k, (l, f)) in local.iter().zip(own).enumerate() {
                    let p = turn(l.position);
                    let plane = mesh.instance_planes[inst.planes as usize + k / 3];
                    assert!((0..3).all(|i| (p[i] + inst.at[i] - f.position[i]).abs() < 0.01), "{k}: {p:?} {:?}", f.position);
                    assert!((0..3).all(|i| (turn(l.normal)[i] - f.normal[i]).abs() < 1e-4 && (turn(l.face)[i] - f.face[i]).abs() < 1e-4));
                    assert_eq!((l.color, l.tex, plane), (f.color, f.tex, f.plane));
                }
            }
        }
    }

    #[test]
    fn the_sample_s_chairs_are_one_shape_placed_again_and_again() {
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
        assert!(mesh.shapes.iter().any(|sh| sh.instances.len() >= 4), "{:?}", mesh.shapes);
        let components = s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Component(_))).count();
        assert_eq!(mesh.instances.len(), components, "every component a copy");
        assert!(mesh.vertices.len() < flat.vertices.len());
        copies_hold_the_flat_triangles(&mesh, &flat);
    }

    /// The sample with three lamps: a frame, and a lens seen through.
    fn with_lamps() -> Session {
        let mut s = sample();
        s.execute("material", &json!({"name": "Lens", "color": [250, 250, 230], "transparency": 60})).unwrap();
        s.execute(
            "family",
            &json!({"name": "Lamp", "types": {"A": {}}, "forms": [
                {"profile": [[0,0],[600,0],[600,600],[0,600]], "from": 2400, "to": 2500, "material": "Concrete"},
                {"profile": [[50,50],[550,50],[550,550],[50,550]], "from": 2300, "to": 2400, "material": "Lens"}]}),
        )
        .unwrap();
        for (x, angle) in [(2000, 0), (5000, 30), (8000, 90)] {
            s.execute("component", &json!({"family": "Lamp", "at": [x, 3000], "angle": angle})).unwrap();
        }
        s
    }

    #[test]
    fn a_copy_seen_through_in_part_is_a_copy_with_two_owners() {
        let s = with_lamps();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
        let lamps: Vec<&Instance> = mesh.instances.iter().filter(|i| i.clear_owner != 0).collect();
        assert_eq!(lamps.len(), 3);
        let shape = mesh.shapes.iter().find(|sh| sh.instances.len() == 3 && sh.clear_from < sh.vertices.end).unwrap();
        assert!(shape.clear_from > shape.vertices.start, "the frame first, then the lens");
        assert!(lamps.iter().all(|l| mesh.owners[l.clear_owner as usize - 1].transparency == 60));
        copies_hold_the_flat_triangles(&mesh, &flat);
        picks_as_the_picture(&s);
    }

    #[test]
    fn a_painted_or_hidden_copy_is_drawn_of_its_own() {
        let mut s = sample();
        let chairs: Vec<Id> = s
            .model
            .elements
            .iter()
            .filter(|(_, e)| matches!(e, crate::model::Element::Component(c) if c.family == "Chair"))
            .map(|(id, _)| *id)
            .collect();
        s.execute("material", &json!({"name": "Test paint", "color": [90, 120, 150]})).unwrap();
        s.execute("paint", &json!({"id": chairs[1], "face": "all", "material": "Test paint"})).unwrap();
        s.execute("graphics", &json!({"view": "{3D}", "elements": {chairs[2].to_string(): {"transparency": 40}}})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
        let copy_of = |id: Id| mesh.instances.iter().any(|i| mesh.owners[i.owner as usize - 1].id == id);
        assert!(copy_of(chairs[0]) && !copy_of(chairs[1]), "the painted chair is its own");
        assert!(!copy_of(chairs[2]), "and the one seen through, which its shape isn't");
        copies_hold_the_flat_triangles(&mesh, &flat);
        picks_as_the_picture(&s);
    }

    #[test]
    fn a_mesh_holds_a_tower_however_many_triangles_it_has() {
        let m = crate::sample::tower::drawn(crate::sample::tower::Plate::REFERENCE, 2, crate::sample::tower::Drawings::default()).unwrap();
        let scene = Scene::new(&m);
        let mesh = Mesh::new(&scene, &Look::of_3d(&m)).unwrap();
        assert!(mesh.triangles() > 1_600_000, "{} triangles, more than it was once given", mesh.triangles());
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
        // Only the glass of the windows is seen through, and it is drawn last.
        let clear: Vec<_> = mesh.owners.iter().filter(|o| o.transparency > 0).collect();
        assert!(!clear.is_empty() && clear.iter().all(|o| s.model.elements.get(o.id).is_some_and(|e| e.category() == "window")));
        assert!(mesh.clear_from < mesh.vertices.len());
    }

    #[test]
    fn a_glass_layer_is_an_owner_of_its_own_with_the_materials_transparency() {
        let mut s = Session::new();
        s.script(
            r#"type {"category": "wall", "name": "Curtain", "layers": [{"material": "Glass", "thickness": 50}, {"material": "Concrete block", "thickness": 100}]}
               wall {"points": [[0,0],[8000,0]], "type": "Curtain"}"#,
        )
        .unwrap();
        let mesh = Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
        let mut alphas: Vec<u8> = mesh.owners.iter().map(|o| o.transparency).collect();
        alphas.sort_unstable();
        assert_eq!(alphas, vec![0, crate::model::GLASS_TRANSPARENCY], "one wall, drawn in two parts");
        assert!(mesh.clear_from > 0 && mesh.clear_from < mesh.vertices.len(), "the opaque layer first, then the glass");
    }
}
