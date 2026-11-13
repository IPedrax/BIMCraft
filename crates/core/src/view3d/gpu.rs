//! What a graphics card needs to draw the 3D view, and what picking needs when there is no picture to read: the scene's triangles
//! as vertices relative to the middle of what is drawn (millimetres far from the origin would overflow f32's resolution, so the
//! shader never sees them), and a bounding volume hierarchy to pick through. No graphics API is in here, so the core stays free
//! of one; the desktop's `gpu.rs` draws it. What a machine can't hold is refused with a reason, and
//! the CPU picture is drawn instead. A section box is not built in: the mesh holds all it would hold without one, and a card cuts
//! each pixel to the box, so moving a face builds nothing. Surfaces seen through come after the opaque ones (from [`Mesh::clear_from`]), as the
//! processor draws them in a pass of their own.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{
    Boxed, Camera, DATUM, EDGE, LevelMarks, NEAR, P3, Scene, Surface, add, cross, dot, level_marks, levels_in, mul, on_level, sub,
    surface_of, unit, weighed,
};
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
    /// Where its shape's origin is, from [`Mesh::origin`], mm.
    pub at: [f32; 3],
    /// Where its shape's x, y and z axes go, each square to the others: turned, and for a run's tube stretched. A normal
    /// goes along each by its share over the axis's length squared, which keeps it square to a stretched surface.
    pub axes: [[f32; 3]; 3],
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

/// A run of [`Mesh::vertices`] (whole triangles), in the order the card draws them, and a box round it, from [`Mesh::origin`],
/// mm. A view leaves out a run that lies wholly outside it, which can't reach a pixel, and draws the rest in the same order, so
/// the picture stays the same to the pixel. Copies are left out each on its own ([`Mesh::visible`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub range: std::ops::Range<usize>,
    pub lo: P3,
    pub hi: P3,
}

/// What of a mesh a view can see: runs of [`Mesh::vertices`] and of [`Mesh::instances`], in order, each apart from the next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Visible {
    pub vertices: Vec<std::ops::Range<usize>>,
    pub instances: Vec<std::ops::Range<usize>>,
}

/// What a frame draws while the view moves, when all it sees is more than it has time for: the nearest of it, and a box for each
/// group or copy left over, drawn from [`Mesh::cube`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nearest {
    pub seen: Visible,
    pub stand_ins: Vec<Instance>,
}

/// The colour a box standing in for what isn't drawn yet is drawn in.
const STAND_IN: u32 = 200 | 200 << 8 | 200 << 16;

/// At most this many triangles to a group, and at most this part of the mesh's width across.
const GROUP_TRIANGLES: usize = 1024;
const GROUP_WIDE: f64 = 1.0 / 16.0;
/// What a view draws of the triangles drawn of their own is one run across a gap of at most this many left out: what is
/// between draws nothing in view, and fewer draws cost less than it does.
const GAP_TRIANGLES: usize = 2048;

/// Cells across the mesh's box, along each axis, that a view judges first: one wholly in view or wholly out settles all in it.
const CELLS: f64 = 12.0;

/// What lies in one cell of the mesh's box: a sphere round all of it, and the groups and copies whose middles are in it.
#[derive(Default)]
struct Cell {
    middle: P3,
    reach: f64,
    groups: Vec<u32>,
    copies: Vec<u32>,
}

/// A cube from 0 to 1 on each axis, as twelve triangles, two to a face.
fn cube() -> Vec<[P3; 3]> {
    let mut tris = Vec::with_capacity(12);
    for axis in 0..3 {
        for side in [0.0, 1.0] {
            let corner = |a: f64, b: f64| {
                let mut p = [0.0; 3];
                p[axis] = side;
                p[(axis + 1) % 3] = a;
                p[(axis + 2) % 3] = b;
                p
            };
            let (p, q, r, t) = (corner(0.0, 0.0), corner(1.0, 0.0), corner(1.0, 1.0), corner(0.0, 1.0));
            // Each face turned outward, so its normal points away from the middle.
            if side > 0.5 {
                tris.extend([[p, q, r], [p, r, t]]);
            } else {
                tris.extend([[p, r, q], [p, t, r]]);
            }
        }
    }
    tris
}

/// The middle of a box and how far its corners are from it.
fn sphere((lo, hi): (P3, P3)) -> (P3, f64) {
    let half = mul(sub(hi, lo), 0.5);
    (add(lo, half), dot(half, half).sqrt())
}

/// The groups and copies (spheres from the origin) by the cell of the mesh's box (`radius` round the origin) their middles
/// are in, each cell with a sphere round all it holds.
fn cells(groups: &[(P3, f64)], copies: &[(P3, f64)], radius: f64) -> Vec<Cell> {
    let side = (2.0 * radius / CELLS).max(1.0);
    let key = |p: P3| p.map(|v| ((v + radius) / side).floor().clamp(0.0, CELLS) as i64);
    // Each cell's box so far, its groups and its copies.
    type Bin = (Option<(P3, P3)>, Vec<u32>, Vec<u32>);
    let mut by: BTreeMap<[i64; 3], Bin> = BTreeMap::new();
    let mut add_to = |(middle, reach): (P3, f64), k: u32, copy: bool| {
        let cell = by.entry(key(middle)).or_default();
        let (lo, hi) = (middle.map(|v| v - reach), middle.map(|v| v + reach));
        cell.0 =
            Some(cell.0.map_or((lo, hi), |(l, h)| (std::array::from_fn(|i| l[i].min(lo[i])), std::array::from_fn(|i| h[i].max(hi[i])))));
        if copy { cell.2.push(k) } else { cell.1.push(k) }
    };
    groups.iter().enumerate().for_each(|(k, s)| add_to(*s, k as u32, false));
    copies.iter().enumerate().for_each(|(k, s)| add_to(*s, k as u32, true));
    by.into_values()
        .filter_map(|(b, groups, copies)| {
            let (middle, reach) = sphere(b?);
            Some(Cell { middle, reach, groups, copies })
        })
        .collect()
}

/// Runs of triangles in order, each with its box: a run ends where `starts` says a new one starts, and when it holds `most` or
/// would grow wider than `wide` across any axis.
fn grouped(boxes: impl Iterator<Item = (P3, P3)>, starts: impl Fn(usize) -> bool, most: usize, wide: f64) -> Vec<Group> {
    let size = 3;
    let mut groups: Vec<Group> = Vec::new();
    for (k, (lo, hi)) in boxes.enumerate() {
        let item = k * size..(k + 1) * size;
        if let Some(g) = groups.last_mut().filter(|_| !starts(k)) {
            let (l, h): (P3, P3) = (std::array::from_fn(|i| g.lo[i].min(lo[i])), std::array::from_fn(|i| g.hi[i].max(hi[i])));
            if g.range.len() < most * size && (0..3).all(|i| h[i] - l[i] <= wide) {
                *g = Group { range: g.range.start..item.end, lo: l, hi: h };
                continue;
            }
        }
        groups.push(Group { range: item, lo, hi });
    }
    groups
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

/// An edge a wireframe draws: its ends in the model, the element it is of (numbered as a vertex's owner is), and the face it
/// picks, as the triangle it is an edge of has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wire {
    pub ends: [P3; 2],
    pub owner: u32,
    face: &'static str,
}

/// A scan's point a card draws: where it is in the model, its colour as a vertex packs one, and its element, numbered as a
/// vertex's owner is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub at: P3,
    pub color: u32,
    pub owner: u32,
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

/// How near a pixel's surface is, as the CPU keeps it (an f32), and the triangle it is.
/// The nearest of what a pixel shows: how near, what it is, and its triangle as placed (none for a scan's point).
type Found = Option<(f32, Hit, Option<[P3; 3]>)>;
/// What the search for [`Found`] holds while it runs: with its place in the scene, so the first of as near ones wins.
type Candidate = Option<(f32, usize, Hit, Option<[P3; 3]>)>;

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
    /// The view's edges, silhouettes and ambient shadows, which the card draws as the processor does.
    pub display: super::DisplayOptions,
    /// The triangles drawn of their own, in runs a view can leave out.
    pub groups: Vec<Group>,
    /// Each group's and each copy's middle, from the origin, and how far from it it reaches (a copy's shape as the card places
    /// it), and the cells they lie in.
    /// A cube from 0 to 1 on each axis, in [`shape_vertices`](Self::shape_vertices), opaque, and where its planes are in
    /// [`instance_planes`](Self::instance_planes): what a box standing in for a group or a copy is placed from.
    pub cube: std::ops::Range<usize>,
    pub cube_planes: u32,
    /// Each shape's box, in its axes.
    shape_boxes: Vec<(P3, P3)>,
    group_spheres: Vec<(P3, f64)>,
    copy_spheres: Vec<(P3, f64)>,
    cells: Vec<Cell>,
    /// The section box the view is cut to, least corner and greatest, in the model: what picks, the marks and a card's pixels keep.
    pub section_box: Option<(P3, P3)>,
    /// Hidden line or wireframe: a card paints every surface paper, and only its edges show.
    pub lines: bool,
    /// Wireframe: no surfaces and no level's marks, only [`wires`](Self::wires) on the paper.
    pub wireframe: bool,
    /// The points of the scans the view shows, as many as a picture draws, after the opaque surfaces, in the processor's order.
    pub dots: Vec<Dot>,
    dot_bvh: OnceLock<Bvh>,
    /// The edges a wireframe draws, in the order the processor draws them, which a later one is over an earlier by.
    pub wires: Vec<Wire>,
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
        let wireframe = look.style() == VisualStyle::Wireframe;
        // Only the shapes the surfaces are drawn from are of use.
        let instancing = instancing && !wireframe;
        // Paper faces with their edges: nothing lit, textured or seen through, as a section's picture.
        let lines = look.style() != VisualStyle::Shaded;
        let plain = look.is_plain();
        // A wireframe draws a scan's points no more than the processor's does.
        let scans: Vec<&super::Scan> = scene.scans.iter().filter(|s| !wireframe && (plain || look.shows(s.owner))).collect();
        // What the view shows, an element's run of triangles at a time.
        let mut shown = Vec::new();
        let mut reach: Option<(P3, P3)> = None;
        // How see-through each triangle shown is (`Tri::alpha`), the view asked once an element.
        let mut alphas = vec![0u8; scene.tris.len()];
        let found;
        let spans = if scene.spans.is_empty() {
            found = super::spans_of(&scene.tris);
            &found
        } else {
            &scene.spans
        };
        for (n, span) in spans.iter().enumerate() {
            if n % 1024 == 0 && cancelled() {
                return Ok(None);
            }
            if plain || look.shows(span.id) {
                shown.extend(span.at.clone());
                let view = if plain { 0 } else { look.transparency(span.id) };
                for (a, t) in
                    alphas.get_mut(span.at.clone()).unwrap_or_default().iter_mut().zip(scene.tris.get(span.at.clone()).unwrap_or_default())
                {
                    *a = if lines { 0 } else { t.clearness().max(view) };
                }
                let [lo, hi] = span.bounds;
                reach = Some(match reach {
                    None => (lo, hi),
                    Some((a, b)) => (std::array::from_fn(|k| a[k].min(lo[k])), std::array::from_fn(|k| b[k].max(hi[k]))),
                });
            }
        }
        // A wireframe's box comes from the elements as any view's, and it draws none of their surfaces.
        if wireframe {
            shown.clear();
        }
        // What the view sees through is drawn after what it doesn't, each in the order the processor draws it.
        let clear = |i: usize| alphas.get(i).is_some_and(|a| *a > 0);
        let shown: Vec<usize> = shown.iter().copied().filter(|&i| !clear(i)).chain(shown.iter().copied().filter(|&i| clear(i))).collect();
        // A texture is looked up as the CPU does unless the view colours its surface another way.
        let colour = |t: &super::Tri| {
            if lines {
                crate::render::PAPER
            } else if plain {
                t.color
            } else {
                look.surface(t.owner, t.color)
            }
        };
        let textured =
            |t: &super::Tri| !lines && scene.skins.get(t.texture() as usize).is_some_and(Option::is_some) && colour(t) == t.color;
        let packed = |t: &super::Tri| {
            let Rgb(r, g, b) = colour(t);
            u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16
        };
        let tex_of = |t: &super::Tri| if textured(t) { t.texture() } else { 0 };
        // The copies drawn from their shape: shown, opaque, and in every triangle coloured, textured, faced and curved as the
        // first such copy of the shape is.
        let mut templates: Vec<Option<Template>> = (0..scene.shapes.len()).map(|_| None).collect();
        let mut taken = vec![false; if instancing && !scene.placements.is_empty() { scene.tris.len() } else { 0 }];
        let alpha_at = |i: usize| alphas.get(i).copied().unwrap_or(0).min(100);
        let face_at = |i: usize| scene.faces.get(i).copied().filter(|f| !f.is_empty()).unwrap_or("");
        let smooth_at = |i: usize| scene.smooth.get(i).and_then(Option::as_deref).map(|c| c.map(unit));
        // The copies that may be drawn from their shape, in the scene's order, and whether each is its shape's first, which
        // the others are held to.
        let mut candidates: Vec<(usize, ComponentInstance, bool)> = Vec::new();
        for (n, &(first, place)) in scene.placements.iter().enumerate() {
            if !instancing {
                break;
            }
            if n % 256 == 0 && cancelled() {
                return Ok(None);
            }
            let Some(local) = scene.shapes.get(place.shape) else { continue };
            let Some(tris) = scene.tris.get(first..first + local.len()) else { continue };
            if !(plain || look.shows(place.owner)) {
                continue;
            }
            // One owner for what is seen through of a copy, so one way see-through.
            let mut clear_alphas = (first..first + tris.len()).map(alpha_at).filter(|a| *a > 0);
            if clear_alphas.next().is_some_and(|a| clear_alphas.any(|b| b != a)) {
                continue;
            }
            let leads = match templates.get_mut(place.shape) {
                Some(slot @ None) => {
                    *slot = Some(Template {
                        alpha: (first..first + tris.len()).map(alpha_at).collect(),
                        color: tris.iter().map(packed).collect(),
                        tex: tris.iter().map(tex_of).collect(),
                        face: (first..first + tris.len()).map(face_at).collect(),
                        smooth: (first..first + tris.len()).map(|i| smooth_at(i).map(|c| c.map(|n| place.local_normal(n)))).collect(),
                    });
                    true
                }
                Some(Some(_)) => false,
                None => continue,
            };
            candidates.push((first, place, leads));
        }
        // Each other copy against its shape's first: the same in every triangle's colour, texture, face, see-through and curve.
        let fits = |&(first, place, leads): &(usize, ComponentInstance, bool)| {
            leads
                || templates.get(place.shape).and_then(Option::as_ref).is_some_and(|template| {
                    let len = template.alpha.len();
                    scene.tris.get(first..first + len).is_some_and(|tris| {
                        tris.iter().enumerate().all(|(k, t)| {
                            let near = |a: P3, b: P3| (0..3).all(|i| (a[i] - b[i]).abs() <= 1e-6);
                            let curved = match (template.smooth.get(k).copied().flatten(), smooth_at(first + k)) {
                                (None, None) => true,
                                (Some(own), Some(seen)) => (0..3).all(|c| near(place.normal(own[c]), seen[c])),
                                _ => false,
                            };
                            template.alpha.get(k) == Some(&alpha_at(first + k))
                                && template.color.get(k) == Some(&packed(t))
                                && template.tex.get(k) == Some(&tex_of(t))
                                && template.face.get(k) == Some(&face_at(first + k))
                                && curved
                        })
                    })
                })
        };
        let fitting: Vec<bool> = match (candidates.len() >= 1000).then(super::render_pool).flatten() {
            Some(pool) => {
                use rayon::prelude::*;
                pool.install(|| candidates.par_iter().map(fits).collect())
            }
            None => candidates.iter().map(fits).collect(),
        };
        if cancelled() {
            return Ok(None);
        }
        let mut chosen: Vec<(usize, ComponentInstance)> = Vec::new();
        for (&(first, place, _), fit) in candidates.iter().zip(fitting) {
            let len = scene.shapes.get(place.shape).map_or(0, Vec::len);
            if fit {
                taken.get_mut(first..first + len).into_iter().flatten().for_each(|t| *t = true);
                chosen.push((first, place));
            }
        }
        let is_taken = |i: usize| taken.get(i).copied().unwrap_or(false);
        let clear_from = shown.iter().filter(|&&i| !clear(i) && !is_taken(i)).count() * 3;
        let drawn = shown.iter().filter(|&&i| !is_taken(i)).count();
        let levels: Vec<_> = scene.levels.iter().filter(|(id, ..)| !wireframe && (plain || look.shows(*id))).cloned().collect();
        // The box round what is shown (the box round its elements' boxes) and the levels.
        let corners = levels.iter().flat_map(|(.., c)| c.iter());
        let reached = reach.iter().flat_map(|(lo, hi)| [lo, hi]);
        let scanned = scans.iter().flat_map(|s| s.corners.iter());
        let (lo, hi) = bounds(reached.chain(corners).chain(scanned)).unwrap_or(([0.0; 3], [0.0; 3]));
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
            display: look.display(),
            groups: Vec::new(),
            cube: 0..0,
            cube_planes: 0,
            shape_boxes: Vec::new(),
            group_spheres: Vec::new(),
            copy_spheres: Vec::new(),
            cells: Vec::new(),
            section_box: look.section_box(),
            lines,
            wireframe,
            wires: Vec::new(),
            dots: Vec::new(),
            dot_bvh: OnceLock::new(),
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
        // The last asked: the triangles of an element come together.
        let mut last: Option<((Id, u8), u32)> = None;
        let mut owner_of = |mesh: &mut Mesh, id: Id, alpha: u8| {
            if let Some((key, n)) = last
                && key == (id, alpha)
            {
                return n;
            }
            let n = *owners.entry((id, alpha)).or_insert_with(|| {
                mesh.owners.push(Owner { id, edge: look.edge(id, EDGE), transparency: alpha });
                mesh.owners.len() as u32
            });
            last = Some(((id, alpha), n));
            n
        };
        for (k, i) in shown.into_iter().enumerate() {
            if k % 8192 == 0 && cancelled() {
                return Ok(None);
            }
            let t = &scene.tris[i];
            let alpha = alpha_at(i);
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
                let axes = place.axes.map(|v| v.map(|x| x as f32));
                mesh.instances.push(Instance { at, axes, owner, clear_owner, planes });
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
        if wireframe {
            let Some(edges) = scene.edges_cancellable(cancelled) else { return Ok(None) };
            mesh.wires.try_reserve_exact(edges.len()).map_err(room)?;
            for (k, &(i, ends)) in edges.iter().enumerate() {
                if k % 8192 == 0 && cancelled() {
                    return Ok(None);
                }
                let Some(t) = scene.tris.get(i).filter(|t| look.shows_part(t.owner, t.part)) else { continue };
                let owner = owner_of(&mut mesh, t.owner, 0);
                mesh.wires.push(Wire { ends, owner, face: face_at(i) });
            }
        }
        // The points, every so many as the processor's picture takes them.
        let step = super::every(scans.iter().map(|s| s.points.len()).sum());
        mesh.dots.try_reserve_exact(scans.iter().map(|s| s.points.len().div_ceil(step)).sum()).map_err(room)?;
        for scan in &scans {
            if cancelled() {
                return Ok(None);
            }
            let owner = owner_of(&mut mesh, scan.owner, 0);
            for (i, p) in scan.points.iter().enumerate().step_by(step) {
                if i % 8192 == 0 && cancelled() {
                    return Ok(None);
                }
                let Rgb(r, g, b) = scan.colour(i, plain, look);
                let color = u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16;
                mesh.dots.push(Dot { at: scan.place(*p), color, owner });
            }
        }
        // The cube a box standing in is placed from, its faces' planes numbered past the scene's.
        let start = mesh.shape_vertices.len();
        let base = u32::try_from(scene.tris.len().max(scene.table.next)).map_err(|_| too_many())?.saturating_add(1);
        mesh.cube_planes = u32::try_from(mesh.instance_planes.len()).map_err(|_| too_many())?;
        for (k, t) in cube().iter().enumerate() {
            let face = unit(cross(sub(t[1], t[0]), sub(t[2], t[0]))).map(|v| v as f32);
            for p in t {
                let position = p.map(|v| v as f32);
                mesh.shape_vertices.push(Vertex { position, normal: face, face, color: STAND_IN, owner: 0, plane: 0, tex: 0 });
            }
            mesh.instance_planes.push(base.saturating_add(k as u32 / 2));
        }
        mesh.cube = start..mesh.shape_vertices.len();
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
        if cancelled() {
            return Ok(None);
        }
        let f = |p: [f32; 3]| p.map(f64::from);
        let wide = radius * 2.0 * GROUP_WIDE;
        let triangles =
            mesh.vertices.chunks(3).map(|t| bounds(t.iter().map(|v| f(v.position)).collect::<Vec<_>>().iter()).unwrap_or_default());
        mesh.groups = grouped(triangles, |k| k * 3 == mesh.clear_from, GROUP_TRIANGLES, wide);
        if cancelled() {
            return Ok(None);
        }
        // A copy's reach: its shape's box as the card places it.
        let local: Vec<(P3, P3)> = mesh
            .shapes
            .iter()
            .map(|sh| {
                bounds(
                    mesh.shape_vertices
                        .get(sh.vertices.clone())
                        .unwrap_or_default()
                        .iter()
                        .map(|v| f(v.position))
                        .collect::<Vec<_>>()
                        .iter(),
                )
                .unwrap_or_default()
            })
            .collect();
        mesh.shape_boxes.clone_from(&local);
        let shape_of: Vec<usize> = mesh.shapes.iter().enumerate().flat_map(|(n, sh)| std::iter::repeat_n(n, sh.instances.len())).collect();
        mesh.copy_spheres = mesh
            .instances
            .iter()
            .zip(&shape_of)
            .map(|(c, &n)| {
                let (middle, half) =
                    local.get(n).map_or(([0.0; 3], [0.0; 3]), |(lo, hi)| (mul(add(*lo, *hi), 0.5), mul(sub(*hi, *lo), 0.5)));
                let axes = c.axes.map(f);
                let at: P3 = std::array::from_fn(|i| f64::from(c.at[i]) + (0..3).map(|a| middle[a] * axes[a][i]).sum::<f64>());
                let reach: P3 = std::array::from_fn(|i| (0..3).map(|a| half[a] * axes[a][i].abs()).sum::<f64>());
                (at, dot(reach, reach).sqrt())
            })
            .collect();
        mesh.group_spheres = mesh.groups.iter().map(|g| sphere((g.lo, g.hi))).collect();
        mesh.cells = cells(&mesh.group_spheres, &mesh.copy_spheres, radius);
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

    /// What a card draws of the mesh through `cam`: every group and copy but those wholly past one side of what the picture
    /// reaches, which leave no pixel, each tested as a sphere round it, padded for the card's f32 arithmetic.
    pub fn visible(&self, cam: &Camera) -> Visible {
        self.visible_cut(cam, self.section_box)
    }

    /// [`visible`](Self::visible), cut to section box `cut` (a box being dragged, or the mesh's own): what lies wholly past one
    /// of its sides leaves no pixel either.
    pub fn visible_cut(&self, cam: &Camera, cut: Option<(P3, P3)>) -> Visible {
        let view = self.view(cam);
        let f = |v: [f32; 3]| v.map(f64::from);
        let (right, up, back, target) = (f(view.right), f(view.up), f(view.back), f(view.target));
        let across = view.perspective.map_or(f64::from(view.scale), |p| f64::from(p.focal));
        let (sx, sy) = (2.0 * across / f64::from(cam.width.max(1)), 2.0 * across / f64::from(cam.height.max(1)));
        let pad = 1e-4 * (self.radius + dot(target, target).sqrt()) + 1.0;
        // Each side of the clip volume as a plane: a point `d` from the aim is past it where d·n > k.
        let (r, u) = (mul(right, sx), mul(up, sy));
        let planes: Vec<(P3, f64)> = match view.perspective {
            // Across, past the eye's sight: x > w, with w how far ahead of the eye; in depth, nearer than the near plane
            // (a·ahead + b > ahead) or beyond the far one (a·ahead + b < 0).
            Some(lens) => {
                let (eye, a, b) = (f64::from(lens.eye), f64::from(lens.a), f64::from(lens.b));
                let mut planes =
                    vec![(add(r, back), eye), (add(mul(r, -1.0), back), eye), (add(u, back), eye), (add(mul(u, -1.0), back), eye)];
                planes.push((back, eye - b / (1.0 - a)));
                if a < 0.0 {
                    planes.push((mul(back, -1.0), -b / a - eye));
                }
                planes
            }
            None => {
                let depth = f64::from(view.depth);
                vec![(r, 1.0), (mul(r, -1.0), 1.0), (u, 1.0), (mul(u, -1.0), 1.0), (back, depth), (mul(back, -1.0), depth)]
            }
        };
        let mut planes = planes;
        if let Some((lo, hi)) = cut {
            for i in 0..3 {
                let axis: P3 = std::array::from_fn(|j| if i == j { 1.0 } else { 0.0 });
                let at = self.origin[i] + target[i];
                planes.push((axis, hi[i] - at));
                planes.push((mul(axis, -1.0), at - lo[i]));
            }
        }
        let planes: Vec<(P3, f64, f64)> = planes.into_iter().map(|(n, k)| (n, k, dot(n, n).sqrt())).collect();
        // Out, in, or neither: past one side, or within every side.
        let judge = |(middle, reach): (P3, f64)| {
            let d = sub(middle, target);
            let mut within = true;
            for (n, k, len) in &planes {
                let (along, room) = (dot(d, *n) - k, (reach + pad) * len);
                if along > room + 1e-6 * (along.abs() + k.abs()) {
                    return Some(false);
                }
                within &= along < -room;
            }
            within.then_some(true)
        };
        // All of it within every side: all of it is drawn, as an orbit round the whole model draws it, without looking further.
        if judge(([0.0; 3], self.radius)) == Some(true) {
            return Visible {
                vertices: (!self.vertices.is_empty()).then_some(0..self.vertices.len()).into_iter().collect(),
                instances: (!self.instances.is_empty()).then_some(0..self.instances.len()).into_iter().collect(),
            };
        }
        let (mut own, mut copies) = (vec![false; self.groups.len()], vec![false; self.instances.len()]);
        for cell in &self.cells {
            match judge((cell.middle, cell.reach)) {
                Some(false) => {}
                Some(true) => {
                    cell.groups.iter().for_each(|&g| own.get_mut(g as usize).into_iter().for_each(|s| *s = true));
                    cell.copies.iter().for_each(|&c| copies.get_mut(c as usize).into_iter().for_each(|s| *s = true));
                }
                None => {
                    for (list, spheres, seen) in
                        [(&cell.groups, &self.group_spheres, &mut own), (&cell.copies, &self.copy_spheres, &mut copies)]
                    {
                        for &k in list {
                            if let (Some(s), Some(flag)) = (spheres.get(k as usize), seen.get_mut(k as usize)) {
                                *flag = judge(*s) != Some(false);
                            }
                        }
                    }
                }
            }
        }
        let mut vertices: Vec<std::ops::Range<usize>> = Vec::new();
        for g in self.groups.iter().zip(&own).filter(|(_, seen)| **seen).map(|(g, _)| g) {
            match vertices.last_mut() {
                Some(prev) if g.range.start <= prev.end + GAP_TRIANGLES * 3 => prev.end = g.range.end,
                _ => vertices.push(g.range.clone()),
            }
        }
        let mut instances: Vec<std::ops::Range<usize>> = Vec::new();
        for (k, _) in copies.iter().enumerate().filter(|(_, seen)| **seen) {
            match instances.last_mut() {
                Some(prev) if prev.end == k => prev.end = k + 1,
                _ => instances.push(k..k + 1),
            }
        }
        Visible { vertices, instances }
    }

    /// How many triangles a card draws for `seen`, each copy's counted.
    pub fn cost(&self, seen: &Visible) -> usize {
        let own: usize = seen.vertices.iter().map(|r| r.len() / 3).sum();
        let copies: usize = self
            .shapes
            .iter()
            .map(|sh| {
                let n: usize =
                    seen.instances.iter().map(|r| r.start.max(sh.instances.start)..r.end.min(sh.instances.end)).map(|r| r.len()).sum();
                n * (sh.vertices.len() / 3)
            })
            .sum();
        own + copies
    }

    /// How far ahead of the camera the nearest of a sphere (from the origin) is: from the eye in perspective, else from the
    /// front of what the view reaches.
    fn nearness(&self, cam: &Camera, (middle, reach): (P3, f64)) -> f64 {
        let view = self.view(cam);
        let (back, target) = (view.back.map(f64::from), view.target.map(f64::from));
        let from = view.perspective.map_or(f64::from(view.depth), |p| f64::from(p.eye));
        from - dot(sub(middle, target), back) - reach
    }

    /// What a frame draws of `seen` in `budget` triangles: all of it if it fits; else the nearest groups and copies, in their
    /// order, up to the first that doesn't fit, and a box for each of the rest (its groups', in order, then its copies').
    pub fn nearest_first(&self, cam: &Camera, seen: &Visible, budget: usize) -> Nearest {
        self.nearest_first_held(cam, seen, budget, &|_| true)
    }

    /// [`nearest_first`](Self::nearest_first) where a card holds only the groups `held` says, by their vertices: the others stand
    /// in however near they are.
    pub fn nearest_first_held(
        &self,
        cam: &Camera,
        seen: &Visible,
        budget: usize,
        held: &dyn Fn(&std::ops::Range<usize>) -> bool,
    ) -> Nearest {
        let within = |runs: &[std::ops::Range<usize>], k: usize| {
            let at = runs.partition_point(|r| r.end <= k);
            runs.get(at).is_some_and(|r| r.contains(&k))
        };
        let all_held = self.groups.iter().filter(|g| within(&seen.vertices, g.range.start)).all(|g| held(&g.range));
        if all_held && self.cost(seen) <= budget {
            return Nearest { seen: seen.clone(), stand_ins: Vec::new() };
        }
        let shape_of = |k: usize| self.shapes.iter().position(|sh| sh.instances.contains(&k));
        // Each group and copy seen: how near, which, and its triangles.
        let mut items: Vec<(f64, bool, usize, usize)> = Vec::new();
        for (k, g) in self.groups.iter().enumerate().filter(|(_, g)| within(&seen.vertices, g.range.start) && held(&g.range)) {
            let sphere = self.group_spheres.get(k).copied().unwrap_or_default();
            items.push((self.nearness(cam, sphere), false, k, g.range.len() / 3));
        }
        for r in &seen.instances {
            for k in r.clone() {
                let tris = shape_of(k).and_then(|n| self.shapes.get(n)).map_or(0, |sh| sh.vertices.len() / 3);
                let sphere = self.copy_spheres.get(k).copied().unwrap_or_default();
                items.push((self.nearness(cam, sphere), true, k, tris));
            }
        }
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut own, mut copies) = (vec![false; self.groups.len()], vec![false; self.instances.len()]);
        let mut spent = 0usize;
        for &(_, copy, k, tris) in &items {
            if spent + tris > budget {
                break;
            }
            spent += tris;
            if let Some(flag) = if copy { copies.get_mut(k) } else { own.get_mut(k) } {
                *flag = true;
            }
        }
        let runs = |flags: &[bool], range: &dyn Fn(usize) -> std::ops::Range<usize>| {
            let mut out: Vec<std::ops::Range<usize>> = Vec::new();
            for (k, _) in flags.iter().enumerate().filter(|(_, f)| **f) {
                let r = range(k);
                match out.last_mut() {
                    Some(prev) if prev.end == r.start => prev.end = r.end,
                    _ => out.push(r),
                }
            }
            out
        };
        let vertices = runs(&own, &|k| self.groups.get(k).map_or(0..0, |g| g.range.clone()));
        let instances = runs(&copies, &|k| k..k + 1);
        // A box for each group and copy seen but not drawn: placed from the cube, at least a mm on each side.
        let boxed = |lo: P3, hi: P3, at: P3, axes: [P3; 3], owner: u32| {
            let size: P3 = std::array::from_fn(|a| (hi[a] - lo[a]).max(1.0 / dot(axes[a], axes[a]).sqrt().max(1e-9)));
            let corner: P3 = std::array::from_fn(|i| at[i] + (0..3).map(|a| lo[a] * axes[a][i]).sum::<f64>());
            Instance {
                at: corner.map(|v| v as f32),
                axes: std::array::from_fn(|a| mul(axes[a], size[a]).map(|v| v as f32)),
                owner,
                clear_owner: 0,
                planes: self.cube_planes,
            }
        };
        let mut stand_ins = Vec::new();
        for (g, _) in self.groups.iter().zip(&own).filter(|(g, drawn)| within(&seen.vertices, g.range.start) && !**drawn) {
            let owner = self.vertices.get(g.range.start).map_or(0, |v| v.owner);
            stand_ins.push(boxed(g.lo, g.hi, [0.0; 3], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], owner));
        }
        for r in &seen.instances {
            for k in r.clone().filter(|k| !copies.get(*k).copied().unwrap_or(false)) {
                let (Some(c), Some((lo, hi))) = (self.instances.get(k), shape_of(k).and_then(|n| self.shape_boxes.get(n))) else {
                    continue;
                };
                stand_ins.push(boxed(*lo, *hi, c.at.map(f64::from), c.axes.map(|a| a.map(f64::from)), c.owner));
            }
        }
        Nearest { seen: Visible { vertices, instances }, stand_ins }
    }

    /// The vertices of the nearest group of `seen` that `held` says a card doesn't hold, the next to bring in.
    pub fn nearest_not_held(
        &self,
        cam: &Camera,
        seen: &Visible,
        held: &dyn Fn(&std::ops::Range<usize>) -> bool,
    ) -> Option<std::ops::Range<usize>> {
        let in_view = |k: usize| {
            let at = seen.vertices.partition_point(|r| r.end <= k);
            seen.vertices.get(at).is_some_and(|r| r.contains(&k))
        };
        self.groups
            .iter()
            .zip(&self.group_spheres)
            .filter(|(g, _)| in_view(g.range.start) && !held(&g.range))
            .min_by(|(_, a), (_, b)| self.nearness(cam, **a).total_cmp(&self.nearness(cam, **b)))
            .map(|(g, _)| g.range.clone())
    }

    /// The marks drawn over the surfaces: each shown level's plane and head, `tints` colouring (and showing all four edges of)
    /// those selected.
    pub fn marks(&self, cam: &Camera, tints: &[(Id, Rgb)]) -> Vec<LevelMarks> {
        self.marks_cut(cam, tints, self.section_box)
    }

    /// [`marks`](Self::marks), cut to section box `cut`, as the processor's picture cuts them.
    pub fn marks_cut(&self, cam: &Camera, tints: &[(Id, Rgb)], cut: Option<(P3, P3)>) -> Vec<LevelMarks> {
        let color = |id: Id| {
            let own = tints.iter().find(|(t, _)| *t == id).map(|(_, c)| *c);
            own.map(|c| (c, true)).or_else(|| Some((*self.level_colors.get(&id)?, false)))
        };
        match cut {
            Some(b) => level_marks(&levels_in(&self.levels, b), cam, &color),
            None => level_marks(&self.levels, cam, &color),
        }
    }

    /// What the hierarchy is over: each triangle drawn of its own, then each copy, as boxes.
    fn boxes(&self) -> Vec<(P3, P3)> {
        let own = self.tris.iter().map(|t| bounds(t.p.iter()).unwrap_or_default());
        let copies = self.copies.iter().map(|c| {
            let local = self.locals.get(c.shape).and_then(|l| bounds(l.tris.iter().flatten())).unwrap_or_default();
            let [lo, hi] = c.place.bounds([local.0, local.1]);
            (lo, hi)
        });
        let wires = self.wires.iter().map(|w| bounds(w.ends.iter()).unwrap_or_default());
        own.chain(copies).chain(wires).collect()
    }

    fn bvh(&self) -> &Bvh {
        self.bvh.get_or_init(|| Bvh::over(&self.boxes(), &|| false).unwrap_or_else(Bvh::empty))
    }

    fn dot_boxes(&self) -> Vec<(P3, P3)> {
        self.dots.iter().map(|d| (d.at, d.at)).collect()
    }

    fn dot_bvh(&self) -> &Bvh {
        self.dot_bvh.get_or_init(|| Bvh::over(&self.dot_boxes(), &|| false).unwrap_or_else(Bvh::empty))
    }

    /// Build the picking hierarchy while the mesh is still on a view worker.
    pub fn prepare_picking(&self) {
        self.bvh();
        self.dot_bvh();
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
        let Some(dots) = Bvh::over(&self.dot_boxes(), cancelled) else { return false };
        if cancelled() {
            return false;
        }
        let _ = self.bvh.set(bvh);
        let _ = self.dot_bvh.set(dots);
        true
    }

    /// What the picture shows at pixel `(x, y)`: the nearest surface there (the first drawn where two are as near), and over it
    /// a level's mark where nothing nearer hides it, as the CPU picture's picks do.
    pub fn pick(&self, cam: &Camera, tints: &[(Id, Rgb)], x: f64, y: f64) -> Option<Hit> {
        self.pick_cut(cam, tints, self.section_box, x, y)
    }

    /// [`pick`](Self::pick) through section box `cut`, whichever box the mesh was made under.
    pub fn pick_cut(&self, cam: &Camera, tints: &[(Id, Rgb)], cut: Option<(P3, P3)>, x: f64, y: f64) -> Option<Hit> {
        self.seen(cam, tints, cut, x, y).map(|(hit, _)| hit).filter(|h| !crate::link::is_linked(h.id))
    }

    /// The surface the line of sight through pixel `(x, y)` meets where the picture shows it, as
    /// [`Scene::surface_in`](super::Scene::surface_in) finds it in the processor's picture: a triangle's, or a level's plane where
    /// what [`pick`](Self::pick) finds is the level's mark.
    pub fn surface(&self, cam: &Camera, tints: &[(Id, Rgb)], x: f64, y: f64) -> Option<Surface> {
        self.surface_cut(cam, tints, self.section_box, x, y)
    }

    /// [`surface`](Self::surface) through section box `cut`, whichever box the mesh was made under.
    pub fn surface_cut(&self, cam: &Camera, tints: &[(Id, Rgb)], cut: Option<(P3, P3)>, x: f64, y: f64) -> Option<Surface> {
        match self.seen(cam, tints, cut, x, y)? {
            (hit, Some(p)) => surface_of(cam, (x, y), hit.id, &p),
            (hit, None) => on_level(&self.levels, cam, (x, y), hit.id),
        }
    }

    /// What the picture through section box `cut` shows at pixel `(x, y)`, linked or not, and the triangle it is as the picture
    /// places it (none for a level's mark, a scan's point or a wireframe's line).
    fn seen(&self, cam: &Camera, tints: &[(Id, Rgb)], cut: Option<(P3, P3)>, x: f64, y: f64) -> Option<(Hit, Option<[P3; 3]>)> {
        let (w, h) = (f64::from(cam.width), f64::from(cam.height));
        if !(x >= 0.0 && y >= 0.0 && x < w && y < h) {
            return None;
        }
        if self.wireframe {
            return self.pick_wire(cam, tints, cut, (x.floor() as i64, y.floor() as i64)).map(|hit| (hit, None));
        }
        let centre = (x.floor() + 0.5, y.floor() + 0.5);
        let (opaque, clear) = self.nearest(cam, cut, centre);
        // A level's mark is hidden by what is opaque only, as the processor's are.
        let behind = opaque.map_or(f64::NEG_INFINITY, |(z, ..)| f64::from(z));
        // A surface seen through shows where it is nearer than anything opaque.
        let seen = clear.filter(|(z, ..)| opaque.is_none_or(|(d, ..)| *z > d)).or(opaque);
        let mut hit = seen.map(|(_, hit, tri)| (hit, tri));
        for marks in self.marks_cut(cam, tints, cut) {
            let at = P2(centre.0, centre.1);
            let on_line = marks.lines.iter().find_map(|([a, b], wide)| covers((cam.project(*a), cam.project(*b)), *wide, centre));
            let head: Vec<_> = marks.head.iter().map(|p| cam.project(*p)).collect();
            let ring: Vec<P2> = head.iter().map(|(x, y, _)| P2(*x, *y)).collect();
            let whole = ring.iter().all(|q| q.0.is_finite() && q.1.is_finite());
            let on_head = (whole && geom::contains(&ring, at)).then(|| head.iter().map(|p| p.2).sum::<f64>() / 4.0);
            if on_line.or(on_head).is_some_and(|z| behind <= z + cam.slack(1.5, z)) {
                hit = Some((Hit { id: marks.id, face: "" }, None));
            }
        }
        hit
    }

    /// The line a wireframe's pixel `at` picks: of those drawn within two pixels of it, the last drawn, what is tinted being
    /// drawn after the rest, as the processor's picture finds it.
    fn pick_wire(&self, cam: &Camera, tints: &[(Id, Rgb)], cut: Option<(P3, P3)>, at: (i64, i64)) -> Option<Hit> {
        let size = (cam.width.clamp(1, super::MAX_SIDE) as usize, cam.height.clamp(1, super::MAX_SIDE) as usize);
        let centre = (at.0 as f64 + 0.5, at.1 as f64 + 0.5);
        let project = cam.projector();
        // Two pixels and the one a line's point is floored in.
        let misses = |b: (P3, P3)| outside(&project, b, centre, 3.0);
        let owner = |w: &Wire| self.owners.get(w.owner.checked_sub(1)? as usize);
        let tinted = |id: Id| tints.iter().any(|(t, _)| *t == id);
        let mut best: Option<(bool, usize)> = None;
        self.bvh().walk(&|n| (n.lo, n.hi), &misses, &mut |i| {
            let Some(wire) = self.wires.get(i) else { return };
            let key = (owner(wire).is_some_and(|o| tinted(o.id)), i);
            if best.is_some_and(|b| b >= key) {
                return;
            }
            let Some((p, q)) = super::wire_line(cam, cut, wire.ends, size) else { return };
            if super::wire_pixels(p, q).any(|(px, py)| (px - at.0).abs() <= 2 && (py - at.1).abs() <= 2) {
                best = Some(key);
            }
        });
        let wire = self.wires.get(best?.1)?;
        Some(Hit { id: owner(wire)?.id, face: wire.face })
    }

    /// The point of the model's surface the picture through `cam` shows at pixel `(x, y)`: the nearest surface there, seen
    /// through as `pick` sees it. None where there's nothing, and in a wireframe, which has no surfaces.
    pub fn point(&self, cam: &Camera, x: f64, y: f64) -> Option<P3> {
        let (w, h) = (f64::from(cam.width), f64::from(cam.height));
        if self.wireframe || !(x >= 0.0 && y >= 0.0 && x < w && y < h) {
            return None;
        }
        let centre = (x.floor() + 0.5, y.floor() + 0.5);
        let (opaque, clear) = self.nearest(cam, self.section_box, centre);
        let seen = clear.filter(|(z, ..)| opaque.is_none_or(|(d, ..)| *z > d)).or(opaque);
        seen.map(|(z, ..)| cam.at_depth(centre, f64::from(z)))
    }

    /// The nearest opaque triangle covering the centre of a pixel, and the nearest seen through, as the CPU's rasterizer finds
    /// them: how near, as an f32 as it keeps it, what it is and its triangle as placed (none for a scan's point). The first of
    /// triangles as near, in the scene, wins.
    fn nearest(&self, cam: &Camera, cut: Option<(P3, P3)>, (x, y): (f64, f64)) -> (Found, Found) {
        let bvh = self.bvh();
        // Opaque, then seen through.
        let mut found: [Candidate; 2] = [None, None];
        let project = cam.projector();
        // What the section box keeps, asked as the processor's picture asks it.
        let cut = cut.map(|b| Boxed::new(cam, b));
        let misses = |b: (P3, P3)| outside(&project, b, (x, y), 1e-6);
        let mut test = |p: &[P3; 3], clear: bool, key: usize, hit: Hit| {
            if cut.as_ref().is_some_and(|k| k.misses_corners(p)) {
                return;
            }
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
                let z = wa * a.2 + wb * b.2 + wc * c.2;
                if cut.as_ref().is_some_and(|k| !k.keeps(x, y, z)) {
                    continue;
                }
                let z = z as f32;
                let found = &mut found[usize::from(clear)];
                if found.is_none_or(|(d, j, ..)| z > d || z == d && key < j) {
                    *found = Some((z, key, hit, Some(*p)));
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
        // The points, drawn after the opaque surfaces and hiding as they do: where one is nearer than all of them, the first of
        // those as near. A dot reaches a pixel and a half from its point.
        let size = (cam.width.clamp(1, super::MAX_SIDE) as usize, cam.height.clamp(1, super::MAX_SIDE) as usize);
        let pixel = (x.floor() as i64, y.floor() as i64);
        let mut point: Option<(f32, usize)> = None;
        let near_dot = |b: (P3, P3)| outside(&project, b, (x, y), 2.0);
        self.dot_bvh().walk(&|n| (n.lo, n.hi), &near_dot, &mut |i| {
            let Some(dot) = self.dots.get(i).filter(|d| cut.as_ref().is_none_or(|k| k.holds(d.at))) else { return };
            let seen = project.at(dot.at);
            let Some((x0, y0)) = super::dot_at(seen, size) else { return };
            let wide = super::DOT as i64;
            if (x0..x0 + wide).contains(&pixel.0) && (y0..y0 + wide).contains(&pixel.1) {
                let z = seen.2 as f32;
                if point.is_none_or(|(d, j)| z > d || z == d && i < j) {
                    point = Some((z, i));
                }
            }
        });
        if let Some((z, i)) = point.filter(|(z, _)| found[0].is_none_or(|(d, ..)| *z > d))
            && let Some(id) = self.dots.get(i).and_then(|d| self.owners.get(d.owner.checked_sub(1)? as usize))
        {
            found[0] = Some((z, usize::MAX, Hit { id: id.id, face: "" }, None));
        }
        let hit = |f: Option<(f32, usize, Hit, Option<[P3; 3]>)>| f.map(|(z, _, hit, tri)| (z, hit, tri));
        (hit(found[0]), hit(found[1]))
    }
}

/// Whether the pixel at `(x, y)` is further than `margin` outside a box's picture, which is inside the rectangle its corners
/// make where all of it is in front of the eye.
fn outside(project: &super::Projector, (lo, hi): (P3, P3), (x, y): (f64, f64), margin: f64) -> bool {
    let corners = (0..8).map(|k| [0, 1, 2].map(|a| if k >> a & 1 == 0 { lo[a] } else { hi[a] }));
    if !corners.clone().all(|p| project.ahead(p) >= NEAR) {
        return false;
    }
    let (lo, hi) = corners.map(|p| project.at(p)).fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), (cx, cy, _)| {
        (P2(lo.0.min(cx), lo.1.min(cy)), P2(hi.0.max(cx), hi.1.max(cy)))
    });
    x < lo.0 - margin || x > hi.0 + margin || y < lo.1 - margin || y > hi.1 + margin
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
        picks_tinted(s, cam, &[(s.model.levels().first().unwrap().0, Rgb(0, 120, 215))]);
    }

    /// [`picks_through`] with `tints`.
    fn picks_tinted(s: &Session, cam: Camera, tints: &[(Id, Rgb)]) {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let image = scene.render_in(&cam, tints, &look);
        let (mut agree, mut total) = (0, 0);
        for y in (0..480).step_by(3) {
            for x in (0..640).step_by(3) {
                let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let cpu = image.pick(x, y).map(|id| Hit { id, face: image.face(x, y).unwrap_or("") });
                total += 1;
                // Where a level's mark is picked, the CPU has no face either.
                agree += usize::from(mesh.pick(&cam, tints, x, y) == cpu);
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
    fn the_surface_under_a_pixel_is_the_same_through_the_mesh_as_the_picture_and_an_orbit_or_a_zoom_about_it_keeps_it_there() {
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let near = |a: P3, b: P3, d: f64| (0..3).all(|i| (a[i] - b[i]).abs() <= d);
        for cam in [flat, scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look)] {
            let image = scene.render_in(&cam, &[], &look);
            let (mut found, mut levels) = (0, 0);
            for y in (0..480).step_by(5) {
                for x in (0..640).step_by(5) {
                    let at = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                    match (scene.surface_in(&image, &cam, at), mesh.surface(&cam, &[], at.0, at.1)) {
                        (Some(a), Some(b)) => {
                            assert_eq!(a.id, b.id, "at {at:?}");
                            assert!(near(a.at, b.at, 1e-3) && near(a.normal, b.normal, 1e-9), "{a:?} {b:?}");
                            found += 1;
                            levels += usize::from(s.model.level(a.id).is_some());
                            // The point is on the line of sight through the pixel, and its normal faces the camera.
                            let shown = |c: Camera| {
                                let (px, py, _) = c.project(a.at);
                                (px - at.0).abs() < 1e-6 && (py - at.1).abs() < 1e-6
                            };
                            assert!(shown(cam) && dot(a.normal, cam.toward(a.at)) > 0.0, "{a:?} at {at:?}");
                            assert!(shown(cam.orbit_about(30.0, -10.0, a.at)), "orbited about it, it stays at {at:?}");
                            assert!(shown(cam.zoom_about(1.7, a.at)) && shown(cam.zoom_about(0.4, a.at)), "zoomed toward it");
                        }
                        (None, None) => {}
                        other => panic!("at {at:?} the picture and the mesh differ: {other:?}"),
                    }
                }
            }
            assert!(found > 500 && levels > 0, "{found} surfaces, {levels} on a level's plane");
        }
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
                let [a, b, c] = inst.axes;
                let turn = |v: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| v[0] * a[i] + v[1] * b[i] + v[2] * c[i]) };
                let w = |v: [f32; 3]| 1.0 / (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
                let normal = |n: [f32; 3]| {
                    let v = turn([n[0] * w(a), n[1] * w(b), n[2] * w(c)]);
                    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    v.map(|x| x / l)
                };
                for (k, (l, f)) in local.iter().zip(own).enumerate() {
                    let p = turn(l.position);
                    let plane = mesh.instance_planes[inst.planes as usize + k / 3];
                    assert!((0..3).all(|i| (p[i] + inst.at[i] - f.position[i]).abs() < 0.01), "{k}: {p:?} {:?}", f.position);
                    assert!(
                        (0..3).all(|i| (normal(l.normal)[i] - f.normal[i]).abs() < 1e-4 && (normal(l.face)[i] - f.face[i]).abs() < 1e-4)
                    );
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

    /// The sample with round runs of two materials: level, sloped, upright and at an angle in plan.
    pub(crate) fn with_runs() -> Session {
        let mut s = sample();
        for (command, points, offset, end) in [
            ("pipe", [[1000, 1000], [7000, 1000]], 2600, 2600),
            ("pipe", [[1000, 2000], [6000, 5000]], 2700, 2200),
            ("pipe", [[3000, 3000], [3000, 3000]], 0, 2800),
            ("conduit", [[1000, 4000], [8000, 4000]], 2900, 2900),
            ("conduit", [[2000, 6000], [2000, 1000]], 2800, 2500),
        ] {
            s.execute(command, &json!({"points": points, "offset": offset, "end_offset": end})).unwrap();
        }
        s
    }

    #[test]
    fn round_runs_are_one_tube_placed_stretched_and_turned() {
        let s = with_runs();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
        let copy_of = |id: Id| mesh.instances.iter().any(|i| mesh.owners[i.owner as usize - 1].id == id);
        let runs: Vec<Id> =
            s.model.elements.iter().filter(|(_, e)| matches!(e.category(), "pipe" | "conduit")).map(|(id, _)| *id).collect();
        assert_eq!(runs.len(), 5);
        assert!(runs.iter().all(|id| copy_of(*id)), "every round run a copy");
        let tube = 3 * 4 * crate::runs::SIDES;
        assert!(mesh.shapes.iter().any(|sh| sh.vertices.len() == tube && sh.instances.len() == 5), "one tube: {:?}", mesh.shapes);
        copies_hold_the_flat_triangles(&mesh, &flat);
        picks_as_the_picture(&s);
    }

    /// Where a card puts `d` (from the mesh's origin) in clip space, in f64: as `to_clip` in the shader.
    fn clip_of(mesh: &Mesh, cam: &Camera, d: P3) -> [f64; 4] {
        let view = mesh.view(cam);
        let f = |v: [f32; 3]| v.map(f64::from);
        let d = sub(d, f(view.target));
        let (x, y, z) = (dot(d, f(view.right)), dot(d, f(view.up)), dot(d, f(view.back)));
        let across = view.perspective.map_or(f64::from(view.scale), |p| f64::from(p.focal));
        let (sx, sy) = (2.0 * across / f64::from(cam.width), 2.0 * across / f64::from(cam.height));
        match view.perspective {
            Some(p) => {
                let ahead = f64::from(p.eye) - z;
                [x * sx, y * sy, f64::from(p.a) * ahead + f64::from(p.b), ahead]
            }
            None => [x * sx, y * sy, 0.5 + z * 0.5 / f64::from(view.depth), 1.0],
        }
    }

    /// Whether every one of `points` lies past the same side of the clip volume, so none of what they bound reaches a pixel.
    fn all_outside(clips: &[[f64; 4]]) -> bool {
        let sides: [fn(&[f64; 4]) -> bool; 6] =
            [|c| c[0] > c[3], |c| c[0] < -c[3], |c| c[1] > c[3], |c| c[1] < -c[3], |c| c[2] < 0.0, |c| c[2] > c[3]];
        sides.iter().any(|side| clips.iter().all(side))
    }

    #[test]
    fn groups_hold_what_is_drawn_in_order_and_a_view_leaves_out_only_what_it_cannot_see() {
        let s = with_runs();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let within = |g: &Group, p: P3| (0..3).all(|i| g.lo[i] - 1e-6 <= p[i] && p[i] <= g.hi[i] + 1e-6);
        // The triangles drawn of their own: whole triangles, in the order drawn, never across where those seen through start.
        let mut at = 0;
        for g in &mesh.groups {
            assert_eq!(g.range.start, at);
            assert!(g.range.end > at && g.range.len() % 3 == 0, "{:?}", g.range);
            assert!(!(g.range.start < mesh.clear_from && mesh.clear_from < g.range.end), "{:?} across {}", g.range, mesh.clear_from);
            assert!(mesh.vertices[g.range.clone()].iter().all(|v| within(g, v.position.map(f64::from))));
            at = g.range.end;
        }
        assert_eq!(at, mesh.vertices.len());
        assert!(mesh.groups.len() > 1, "{} groups", mesh.groups.len());
        // What a view leaves out lies wholly past one side of what it sees; seen whole, nothing is left out.
        let fitted = scene.fit_in(AZIMUTH, ELEVATION, 800, 600, &look);
        let near = Camera { scale: fitted.scale * 8.0, target: add(fitted.target, [-3000.0, -2000.0, 0.0]), ..fitted };
        let inside = crate::view3d::inside(&s.model, Camera { lens: Some(35.0), ..fitted });
        let expanded = |ranges: &[std::ops::Range<usize>]| ranges.iter().flat_map(Clone::clone).collect::<std::collections::BTreeSet<_>>();
        for (name, cam) in
            [("fitted", fitted), ("near", near), ("inside", inside), ("fitted in perspective", Camera { lens: Some(35.0), ..fitted })]
        {
            let seen = mesh.visible(&cam);
            let (own, copies) = (expanded(&seen.vertices), expanded(&seen.instances));
            for w in seen.vertices.windows(2).chain(seen.instances.windows(2)) {
                assert!(w[0].end < w[1].start, "{name}: {w:?} in order and apart");
            }
            for t in mesh.vertices.chunks(3).enumerate().filter(|(k, _)| !own.contains(&(k * 3))) {
                let clips: Vec<_> = t.1.iter().map(|v| clip_of(&mesh, &cam, v.position.map(f64::from))).collect();
                assert!(all_outside(&clips), "{name}: triangle {} left out but seen: {clips:?}", t.0);
                assert!((0..3).all(|c| !own.contains(&(t.0 * 3 + c))), "{name}: a triangle split");
            }
            for (k, c) in mesh.instances.iter().enumerate().filter(|(k, _)| !copies.contains(k)) {
                let shape = mesh.shapes.iter().find(|sh| sh.instances.contains(&k)).unwrap();
                let clips: Vec<_> = mesh.shape_vertices[shape.vertices.clone()]
                    .iter()
                    .map(|v| {
                        let p: P3 = std::array::from_fn(|i| {
                            f64::from(c.at[i]) + (0..3).map(|a| f64::from(v.position[a]) * f64::from(c.axes[a][i])).sum::<f64>()
                        });
                        clip_of(&mesh, &cam, p)
                    })
                    .collect();
                assert!(all_outside(&clips), "{name}: copy {k} left out but seen");
            }
            let left_out = mesh.vertices.len() - own.len() + mesh.instances.len() - copies.len();
            match name {
                "fitted" | "fitted in perspective" => assert_eq!(left_out, 0, "{name}: all of it is in view"),
                _ => assert!(left_out > 0, "{name}: something is out of view"),
            }
        }
    }

    #[test]
    fn nearest_first_draws_the_nearest_within_a_budget_and_a_box_for_each_of_the_rest() {
        let s = with_runs();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let fitted = scene.fit_in(AZIMUTH, ELEVATION, 800, 600, &look);
        let expanded = |ranges: &[std::ops::Range<usize>]| ranges.iter().flat_map(Clone::clone).collect::<std::collections::BTreeSet<_>>();
        let tris_of = |k: usize| mesh.shapes.iter().find(|sh| sh.instances.contains(&k)).map_or(0, |sh| sh.vertices.len() / 3);
        // The cube a box is drawn from: twelve triangles, opaque, with planes of its own.
        assert_eq!(mesh.cube.len(), 36);
        for cam in [fitted, Camera { lens: Some(35.0), ..fitted }] {
            let seen = mesh.visible(&cam);
            let all = mesh.cost(&seen);
            assert_eq!(
                all,
                seen.vertices.iter().map(|r| r.len() / 3).sum::<usize>()
                    + seen.instances.iter().flat_map(Clone::clone).map(tris_of).sum::<usize>()
            );
            // Within the budget, all of it as it is.
            let whole = mesh.nearest_first(&cam, &seen, all);
            assert_eq!((whole.seen.clone(), whole.stand_ins.len()), (seen.clone(), 0));
            // Past it, the nearest within it and a box for each group or copy left over, nearer ones never boxed before farther ones drawn.
            let near = mesh.nearest_first(&cam, &seen, all / 3);
            assert!(mesh.cost(&near.seen) <= all / 3 && mesh.cost(&near.seen) > 0, "{} of {all}", mesh.cost(&near.seen));
            let (drawn_own, drawn_copies) = (expanded(&near.seen.vertices), expanded(&near.seen.instances));
            let (seen_own, seen_copies) = (expanded(&seen.vertices), expanded(&seen.instances));
            assert!(drawn_own.is_subset(&seen_own) && drawn_copies.is_subset(&seen_copies));
            let left_groups: Vec<&Group> =
                mesh.groups.iter().filter(|g| seen_own.contains(&g.range.start) && !drawn_own.contains(&g.range.start)).collect();
            let left_copies: Vec<usize> = seen_copies.difference(&drawn_copies).copied().collect();
            assert_eq!(near.stand_ins.len(), left_groups.len() + left_copies.len());
            let far = |k: usize, copy: bool| mesh.nearness(&cam, if copy { mesh.copy_spheres[k] } else { mesh.group_spheres[k] });
            let group_of = |start: usize| mesh.groups.iter().position(|g| g.range.start == start).unwrap();
            let drawn_far = drawn_own.iter().filter(|v| mesh.groups.iter().any(|g| g.range.start == **v)).map(|v| far(group_of(*v), false));
            let drawn_far = drawn_far.chain(drawn_copies.iter().map(|k| far(*k, true))).fold(f64::MIN, f64::max);
            let boxed_near = left_groups.iter().map(|g| far(group_of(g.range.start), false));
            let boxed_near = boxed_near.chain(left_copies.iter().map(|k| far(*k, true))).fold(f64::MAX, f64::min);
            assert!(drawn_far <= boxed_near, "drawn as far as {drawn_far}, boxed from {boxed_near}");
            assert!(!near.stand_ins.is_empty());
            let placed = |c: &Instance, p: [f32; 3]| -> P3 {
                std::array::from_fn(|i| f64::from(c.at[i]) + (0..3).map(|a| f64::from(p[a]) * f64::from(c.axes[a][i])).sum::<f64>())
            };
            let inside = |b: &Instance, p: P3| {
                // The box's own coordinates of `p`: within 0 to 1 along each of its axes.
                let d = sub(p, b.at.map(f64::from));
                (0..3).all(|a| {
                    let axis = b.axes[a].map(f64::from);
                    let t = dot(d, axis) / dot(axis, axis);
                    (-1e-3..=1.0 + 1e-3).contains(&t)
                })
            };
            for (b, g) in near.stand_ins.iter().zip(&left_groups) {
                assert_eq!(b.planes, mesh.cube_planes);
                assert!(mesh.vertices[g.range.clone()].iter().all(|v| inside(b, v.position.map(f64::from))), "a group outside its box");
            }
            for (b, &k) in near.stand_ins[left_groups.len()..].iter().zip(&left_copies) {
                let (c, shape) = (&mesh.instances[k], mesh.shapes.iter().find(|sh| sh.instances.contains(&k)).unwrap());
                assert_eq!(b.owner, c.owner);
                assert!(
                    mesh.shape_vertices[shape.vertices.clone()].iter().all(|v| inside(b, placed(c, v.position))),
                    "copy {k} outside its box"
                );
            }
        }
    }

    /// The sample house with a section box through it: from its least corner to `(x, y, z)` of the way across what it holds.
    fn cut_through(s: &mut Session, [x, y, z]: [f64; 3]) -> (P3, P3) {
        s.execute("section_box", &json!({"on": true})).unwrap();
        let (lo, hi) = Look::of_3d(&s.model).section_box().unwrap();
        let to = [lo[0] + (hi[0] - lo[0]) * x, lo[1] + (hi[1] - lo[1]) * y, lo[2] + (hi[2] - lo[2]) * z];
        s.execute("section_box", &json!({"min": lo, "max": to})).unwrap();
        Look::of_3d(&s.model).section_box().unwrap()
    }

    #[test]
    fn picking_the_mesh_through_a_section_box_finds_what_the_cpu_picture_shows() {
        let mut s = sample();
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        picks_through(&s, flat);
        let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
        picks_through(&s, cam);
        picks_through(&s, crate::view3d::inside(&s.model, cam));
    }

    #[test]
    fn a_section_box_leaves_out_of_a_frame_only_what_lies_wholly_outside_it() {
        let mut s = sample();
        let whole = {
            let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
            let mesh = Mesh::new(&scene, &look).unwrap();
            mesh.cost(&mesh.visible(&scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look)))
        };
        let (lo, hi) = cut_through(&mut s, [0.3, 0.3, 0.3]);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let seen = mesh.visible(&scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look));
        assert!(mesh.cost(&seen) < whole, "{} triangles drawn, of {whole}", mesh.cost(&seen));
        // Any triangle with a corner in the box is drawn.
        for (n, t) in mesh.vertices.as_chunks::<3>().0.iter().enumerate() {
            let inside = t.iter().any(|v| (0..3).all(|i| (lo[i]..=hi[i]).contains(&(f64::from(v.position[i]) + mesh.origin[i]))));
            assert!(!inside || seen.vertices.iter().any(|r| r.contains(&(n * 3))), "triangle {n} is in the box and left out");
        }
    }

    #[test]
    fn a_mesh_picks_through_any_box_it_is_asked_as_a_mesh_made_for_that_box_does() {
        let mut s = sample();
        let (whole, whole_look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let kept = Mesh::new(&whole, &whole_look).unwrap();
        let cam = whole.fit_in(AZIMUTH, ELEVATION, 640, 480, &whole_look);
        let cut = cut_through(&mut s, [0.6, 0.7, 0.45]);
        let own = Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
        let mut differ_from_uncut = 0;
        for y in (0..480).step_by(4) {
            for x in (0..640).step_by(4) {
                let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let asked = kept.pick_cut(&cam, &[], Some(cut), x, y);
                assert_eq!(asked, own.pick(&cam, &[], x, y), "at {x}, {y}");
                differ_from_uncut += usize::from(asked != kept.pick(&cam, &[], x, y));
            }
        }
        assert!(differ_from_uncut > 100, "the box changes what is picked: {differ_from_uncut}");
    }

    #[test]
    fn a_section_box_leaves_out_a_level_past_it_and_a_box_being_dragged_asks_of_the_same_mesh() {
        let mut s = sample();
        let (lo, hi) = cut_through(&mut s, [0.6, 0.7, 0.45]);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let within = |(lo, hi): (P3, P3)| -> Vec<Id> {
            scene.levels.iter().filter(|(.., c)| (lo[2]..=hi[2]).contains(&c[0][2])).map(|(id, ..)| *id).collect()
        };
        let ids = |marks: Vec<LevelMarks>| marks.iter().map(|m| m.id).collect::<Vec<Id>>();
        let kept = ids(mesh.marks(&cam, &[]));
        assert!(!kept.is_empty() && kept.len() < scene.levels.len(), "{} of {} levels", kept.len(), scene.levels.len());
        assert_eq!(kept, within((lo, hi)));
        let off = (
            lo,
            [
                hi[0],
                hi[1],
                kept.iter().filter_map(|k| scene.levels.iter().find(|(id, ..)| id == k)).map(|(.., c)| c[0][2]).fold(f64::MAX, f64::min)
                    - 1.0,
            ],
        );
        assert!(ids(mesh.marks_cut(&cam, &[], Some(off))).len() < kept.len(), "a box dragged below a level leaves it out");
    }

    #[test]
    fn picking_the_mesh_in_hidden_line_finds_what_the_cpu_picture_shows_glass_and_all() {
        let mut s = sample();
        s.execute("visual_style", &json!({"style": "hidden_line"})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        picks_through(&s, flat);
        picks_through(&s, scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look));
        // A section's hidden line is opaque, glass too: nothing is drawn seen through.
        let mesh = Mesh::new(&scene, &look).unwrap();
        assert!(mesh.lines);
        assert_eq!(mesh.clear_from, mesh.vertices.len());
        assert!(mesh.owners.iter().all(|o| o.transparency == 0));
        assert!(mesh.shapes.iter().all(|s| s.clear_from == s.vertices.end));
        assert!(mesh.vertices.iter().all(|v| v.tex == 0), "no texture on a paper face");
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        picks_through(&s, flat);
    }

    #[test]
    fn picking_the_mesh_in_wireframe_finds_what_the_cpu_picture_shows_within_two_pixels_of_a_line() {
        let mut s = sample();
        s.execute("visual_style", &json!({"style": "wireframe"})).unwrap();
        let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
        picks_tinted(&s, flat, &[]);
        // What is tinted is drawn over the rest, so it is what a pixel near both picks.
        picks_tinted(&s, flat, &[(wall, Rgb(255, 160, 0))]);
        picks_tinted(&s, cam, &[]);
        picks_tinted(&s, crate::view3d::inside(&s.model, cam), &[]);
        // Nothing but lines is on the card, and no level's mark.
        let mesh = Mesh::new(&scene, &look).unwrap();
        assert!(mesh.wireframe && mesh.lines);
        assert!(mesh.triangles() == 0 && !mesh.wires.is_empty());
        assert!(mesh.marks(&flat, &[]).is_empty());
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        picks_tinted(&s, flat, &[]);
        // What the view hides has no lines to pick.
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"hidden": true}}})).unwrap();
        picks_tinted(&s, flat, &[]);
    }

    /// A scan of a red face 4 m across standing in the sample house, as a surveyor's points come in: `test` names its file.
    fn scanned(s: &mut Session, test: &str) -> Id {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{test}.xyz"));
        let text: String = (0..=200)
            .flat_map(|i| (0..=150).map(move |k| format!("{} 0 {} 220 30 30\n", f64::from(i) * 0.02, f64::from(k) * 0.02)))
            .collect();
        std::fs::write(&path, text).unwrap();
        let out = s.execute("point_cloud", &json!({"path": path, "at": [4000, 3000]})).unwrap();
        s.model.elements.iter().rev().find(|(_, e)| e.category() == "pointcloud").map(|(id, _)| *id).unwrap_or_else(|| panic!("{out}"))
    }

    #[test]
    fn picking_the_mesh_with_a_scan_finds_what_the_cpu_picture_shows() {
        let mut s = sample();
        let scan = scanned(&mut s, "picking");
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
        picks_through(&s, flat);
        picks_tinted(&s, flat, &[(scan, Rgb(0, 120, 215))]);
        picks_through(&s, cam);
        picks_through(&s, crate::view3d::inside(&s.model, cam));
        // The points are on the card, and the box round the view holds them.
        let mesh = Mesh::new(&scene, &look).unwrap();
        assert_eq!(mesh.dots.len(), 201 * 151);
        let (lo, hi) = scene.scans[0].corners.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
            (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i])))
        });
        let far = (0..3).map(|i| (lo[i] - mesh.origin[i]).abs().max((hi[i] - mesh.origin[i]).abs())).fold(0.0, f64::max);
        assert!(far <= mesh.radius + 1.0, "{far} from the middle, of {}", mesh.radius);
        // Hidden line: paper faces, the points in their colours.
        s.execute("visual_style", &json!({"style": "hidden_line"})).unwrap();
        picks_through(&s, flat);
        s.execute("visual_style", &json!({"style": "shaded"})).unwrap();
        // Seen through, in front of the points and behind them.
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
        picks_through(&s, flat);
        // Cut to a box, a point being kept or not whole.
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        picks_through(&s, flat);
    }

    #[test]
    fn the_mesh_reaches_a_scan_far_past_the_model_so_a_card_s_depth_holds_it() {
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0, 0], [3000, 0]]})).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("far.xyz");
        // Far above: a level's plane reaches a scan across, not up.
        std::fs::write(&path, "0 0 300\n1 0 300\n0 1 300\n0 0 301\n").unwrap();
        s.execute("point_cloud", &json!({"path": path, "origin": "file"})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        assert_eq!(mesh.dots.len(), 4);
        for d in &mesh.dots {
            let off = sub(d.at, mesh.origin);
            assert!(dot(off, off).sqrt() <= mesh.radius + 1.0, "{off:?} from the middle, of {}", mesh.radius);
        }
    }

    #[test]
    fn a_scan_alone_picks_its_points_and_what_the_view_hides_has_none() {
        let mut s = Session::new();
        let scan = scanned(&mut s, "alone");
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        picks_tinted(&s, cam, &[]);
        let mesh = Mesh::new(&scene, &look).unwrap();
        let image = scene.render_in(&cam, &[], &look);
        let on = (0..480)
            .flat_map(|y| (0..640).map(move |x| (f64::from(x) + 0.5, f64::from(y) + 0.5)))
            .find(|&(x, y)| image.pick(x, y) == Some(scan))
            .unwrap();
        assert_eq!(mesh.pick(&cam, &[], on.0, on.1).map(|h| h.id), Some(scan));
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"pointcloud": {"hidden": true}}})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        assert!(Mesh::new(&scene, &look).unwrap().dots.is_empty());
    }

    #[test]
    fn a_wireframe_abandoned_while_its_edges_are_found_makes_no_mesh() {
        let mut s = sample();
        s.execute("visual_style", &json!({"style": "wireframe"})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let asked = Cell::new(0);
        let later = || {
            asked.set(asked.get() + 1);
            asked.get() > 1
        };
        assert!(Mesh::new_cancellable(&scene, &look, &later).unwrap().is_none());
        // What was abandoned isn't kept as the answer.
        assert!(!Mesh::new(&scene, &look).unwrap().wires.is_empty());
    }

    #[test]
    fn what_a_card_does_not_hold_stands_in_however_near_it_is() {
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let seen = mesh.visible(&cam);
        let all = mesh.cost(&seen);
        assert!(mesh.groups.len() >= 2, "{} groups", mesh.groups.len());
        // The first group alone is held: with room for everything, it is all that is drawn of the triangles of their own.
        let first = mesh.groups[0].range.clone();
        let held = |r: &std::ops::Range<usize>| r.start >= first.start && r.end <= first.end;
        let near = mesh.nearest_first_held(&cam, &seen, all, &held);
        let drawn: usize = near.seen.vertices.iter().map(|r| r.len()).sum();
        assert_eq!(drawn, first.len());
        let own_left = mesh.groups.len() - 1;
        assert!(near.stand_ins.len() >= own_left, "{} boxes", near.stand_ins.len());
        // With all held it is as ever.
        let every = mesh.nearest_first_held(&cam, &seen, all, &|_| true);
        assert_eq!(every.seen, seen);
        assert!(every.stand_ins.is_empty());
    }

    #[test]
    fn a_mesh_holds_a_tower_however_many_triangles_it_has() {
        let m = crate::sample::tower::drawn(crate::sample::tower::Plate::REFERENCE, 2, crate::sample::tower::Drawings::default()).unwrap();
        let scene = Scene::new(&m);
        let mesh = Mesh::new(&scene, &Look::of_3d(&m)).unwrap();
        assert!(mesh.triangles() > 1_600_000, "{} triangles, more than it was once given", mesh.triangles());
    }

    #[test]
    fn a_mesh_draws_every_style_and_what_is_seen_through() {
        let mut s = sample();
        for style in ["wireframe", "hidden_line", "shaded"] {
            s.execute("visual_style", &json!({"style": style})).unwrap();
            assert!(Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).is_ok(), "{style}");
        }
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
