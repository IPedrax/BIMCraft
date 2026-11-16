//! The caps of a 3D view's section box: where its faces cut the model's solids, each piece filled as a section at the view's scale
//! fills it (the view's cut pattern override, else each layer's material's cut pattern) and outlined, so a box shows what it cuts
//! solid rather than hollow. The solids are found once for a model ([`CapSource`]) and each face's cut once for where it stands:
//! moving one face of the box cuts again only along that face, and the scene and the card's mesh are left as they are.

use std::sync::{Arc, Mutex};

use super::{
    Camera, EDGE, Image, P3, Solid, VisualStyle, add, cross, dot, layered_with, mix, mul, outline, solids_cancellable, sub, unit, weighed,
};
use crate::geom::{self, P2};
use crate::graphics::{Detail, Look};
use crate::model::{Element, Id, Layer, Model};
use crate::render::{PAPER, Rgb};
use crate::walls::Axis;

/// How many faces' cuts a source keeps: the box's six, and as many again where faces were.
const KEPT: usize = 12;

/// A solid the box may cut, with what it needs to be filled: its layer, the box round it, and the way its courses run in plan.
struct Piece {
    solid: Solid,
    layer: Option<usize>,
    lo: P3,
    hi: P3,
    along: P2,
}

/// One face of the box's cuts: each piece it cuts, and the rings it cuts it into, in the face's plane ([`plane_of`]).
type Cuts = Arc<Vec<(usize, Vec<P2>)>>;

/// What a model's caps are cut from: the solids a section cuts (not a mass, which a section outlines, nor a component's form,
/// which it shows whole), in their layers, and the cuts already made.
pub struct CapSource {
    pieces: Vec<Piece>,
    faces: Mutex<Vec<((usize, u64), Cuts)>>,
}

/// A triangle of a cap: its corners (wound so that its face turns out of the box), the element it cuts, the colour it is filled with, the box's face it is on (its normal, out
/// of the box) and which cap it is of, each its own plane, so an edge shows between two.
#[derive(Clone, Debug, PartialEq)]
pub struct CapTri {
    pub p: [P3; 3],
    pub owner: Id,
    pub color: Rgb,
    pub normal: P3,
    pub cap: u32,
}

/// What a section box's faces show of what they cut: its caps' triangles, and over them their outlines and patterns' strokes,
/// each in its colour (the outlines in the view's edge colour of their element).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caps {
    pub tris: Vec<CapTri>,
    pub lines: Vec<([P3; 2], Rgb, Id)>,
}

impl Caps {
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }
}

/// The two axes a face square to `axis` lies along, in the order its plane's points are (a, b).
fn plane_of(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// A point of the face square to `axis` at `at`, from its plane's (a, b).
fn lift(axis: usize, at: f64, q: P2) -> P3 {
    let (a, b) = plane_of(axis);
    let mut p = [0.0; 3];
    p[axis] = at;
    p[a] = q.0;
    p[b] = q.1;
    p
}

/// The rings level plane `z` cuts `s` into, in plan.
fn level(s: &Solid, z: f64) -> Vec<Vec<P2>> {
    let ring = match s {
        Solid::Box { frame, a, c, z: (z0, z1), .. } => {
            if z < *z0 || z > *z1 {
                return Vec::new();
            }
            [(a.0, c.0), (a.1, c.0), (a.1, c.1), (a.0, c.1)].map(|(s, t)| frame.o + frame.u * s + frame.n * t).to_vec()
        }
        Solid::Slab { face, .. } => {
            let plan: Vec<P2> = face.iter().map(|p| P2(p[0], p[1])).collect();
            geom::clip(&geom::clip(&plan, |q| z - s.z_at(q).0), |q| s.z_at(q).1 - z)
        }
        Solid::Prism { ring, bottom, top, .. } => geom::clip(&geom::clip(ring, |q| z - bottom.at(q)), |q| top.at(q) - z),
        Solid::Mesh { tris, cut: true, .. } => {
            let mut pts = Vec::new();
            for [a, b, c] in tris {
                for (x, y) in [(a, b), (b, c), (c, a)] {
                    let (dx, dy) = (x[2] - z, y[2] - z);
                    if dx.abs() < 1e-9 {
                        pts.push(P2(x[0], x[1]));
                    } else if dx * dy < 0.0 {
                        let t = dx / (dx - dy);
                        pts.push(P2(x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t));
                    }
                }
            }
            geom::hull(pts)
        }
        Solid::Mesh { .. } => return Vec::new(),
    };
    if ring.len() >= 3 && geom::area(&ring).abs() > 1e-6 { vec![ring] } else { Vec::new() }
}

/// The rings the face square to `axis` at `at` cuts piece `p` into, in the face's plane.
fn cut(p: &Piece, axis: usize, at: f64) -> Vec<Vec<P2>> {
    match axis {
        2 => level(&p.solid, at),
        // Upright: a section's line along the face, from before the piece to past it.
        _ => {
            let k = if axis == 0 { 1 } else { 0 };
            let (from, len) = (p.lo[k] - 1.0, p.hi[k] - p.lo[k] + 2.0);
            let (start, u) = if axis == 0 { (P2(at, from), P2(0.0, 1.0)) } else { (P2(from, at), P2(1.0, 0.0)) };
            p.solid.cuts(start, u, len).into_iter().map(|r| r.into_iter().map(|q| P2(q.0 + from, q.1)).collect()).collect()
        }
    }
}

impl CapSource {
    /// The solids of `m` a section box may cut.
    pub fn new(m: &Model) -> CapSource {
        Self::new_cancellable(m, &|| false).unwrap_or(CapSource { pieces: Vec::new(), faces: Mutex::new(Vec::new()) })
    }

    /// [`CapSource::new`], given up when `cancelled` says so.
    pub fn new_cancellable(m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<CapSource> {
        let mut pieces = Vec::new();
        for solid in solids_cancellable(m, cancelled)? {
            if cancelled() {
                return None;
            }
            let owner = solid.owner();
            if matches!(solid, Solid::Mesh { cut: false, .. }) || matches!(m.elements.get(owner), Some(Element::Mass(_))) {
                continue;
            }
            for (solid, layer) in layered_with(m, solid) {
                let corners = solid.corners();
                let Some((lo, hi)) = corners.iter().fold(None, |b: Option<(P3, P3)>, p| {
                    let (lo, hi) = b.unwrap_or((*p, *p));
                    Some((std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i]))))
                }) else {
                    continue;
                };
                // A wall's courses run along it, as in plan.
                let along = m
                    .wall(owner)
                    .map(|w| {
                        let axis = Axis::of(w);
                        let n = corners.len().max(1) as f64;
                        let c = corners.iter().fold(P2(0.0, 0.0), |c, p| P2(c.0 + p[0] / n, c.1 + p[1] / n));
                        axis.tangent(axis.project(c).0)
                    })
                    .filter(|u| u.0.is_finite() && u.1.is_finite() && u.length() > 0.5)
                    .unwrap_or(P2(1.0, 0.0));
                pieces.push(Piece { solid, layer, lo, hi, along });
            }
        }
        Some(CapSource { pieces, faces: Mutex::new(Vec::new()) })
    }

    /// What the face square to `axis` at `at` cuts: made once, kept while it is among the last faces asked for.
    fn face(&self, axis: usize, at: f64) -> Cuts {
        let key = (axis, at.to_bits());
        if let Ok(faces) = self.faces.lock()
            && let Some((_, cuts)) = faces.iter().find(|(k, _)| *k == key)
        {
            return Arc::clone(cuts);
        }
        let cuts: Cuts = Arc::new(
            self.pieces
                .iter()
                .enumerate()
                .filter(|(_, p)| p.lo[axis] <= at && at <= p.hi[axis])
                .flat_map(|(i, p)| cut(p, axis, at).into_iter().map(move |r| (i, r)))
                .collect(),
        );
        if let Ok(mut faces) = self.faces.lock() {
            faces.retain(|(k, _)| *k != key);
            faces.push((key, Arc::clone(&cuts)));
            let over = faces.len().saturating_sub(KEPT);
            faces.drain(..over);
        }
        cuts
    }

    /// [`CapSource::caps`] as view `view` fills them, at its scale and detail (the project's default scale without one).
    pub fn caps_in(&self, m: &Model, view: Option<Id>, look: &Look) -> Caps {
        let n = view.map_or_else(|| m.units.default_scale(), |v| crate::graphics::scale_of(m, v));
        let detail = view.and_then(|v| crate::graphics::graphics_of(m, v)).map_or_else(Detail::default, |g| g.resolved(m).detail);
        self.caps(m, look, (n, detail))
    }

    /// The caps of `look`'s section box, filled as a section at 1 : `n` and `detail` fills what it cuts; none without a box.
    pub fn caps(&self, m: &Model, look: &Look, (n, detail): (f64, Detail)) -> Caps {
        let mut out = Caps::default();
        let Some((lo, hi)) = look.section_box() else { return out };
        if look.style() == VisualStyle::Wireframe {
            return out;
        }
        let plain = look.is_plain();
        for axis in 0..3 {
            let (a, b) = plane_of(axis);
            for (at, out_of_box) in [(lo[axis], -1.0), (hi[axis], 1.0)] {
                let mut normal = [0.0; 3];
                normal[axis] = out_of_box;
                for (i, ring) in self.face(axis, at).iter() {
                    let Some(p) = self.pieces.get(*i) else { continue };
                    let owner = p.solid.owner();
                    if !plain && !look.shows(owner) {
                        continue;
                    }
                    let ring = [(P2(1.0, 0.0), lo[a]), (P2(-1.0, 0.0), -hi[a]), (P2(0.0, 1.0), lo[b]), (P2(0.0, -1.0), -hi[b])]
                        .into_iter()
                        .fold(ring.clone(), |r, (dir, k)| geom::clip(&r, |q| q.dot(dir) - k));
                    if ring.len() < 3 || geom::area(&ring).abs() < 1.0 {
                        continue;
                    }
                    let ring: Vec<P2> = if geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring };
                    let layer: Option<&Layer> = p.layer.and_then(|k| layers_of(m, owner).get(k));
                    let u = if axis == 2 { p.along } else { P2(1.0, 0.0) };
                    let (base, strokes) = crate::render::cap_fill(m, look, (owner, layer), &ring, u, (n, detail));
                    let color = base.unwrap_or_else(|| if plain { p.solid_color() } else { look.cut_fill(owner, p.solid_color()) });
                    let cap = out.tris.len() as u32;
                    let at3 = |q: P2| lift(axis, at, q);
                    for [x, y, z] in geom::triangulate(&ring) {
                        let (Some(x), Some(y), Some(z)) = (ring.get(x), ring.get(y), ring.get(z)) else { continue };
                        // Wound so that its face turns out of the box, as a card tells its front by.
                        let (x, y, z) = (at3(*x), at3(*y), at3(*z));
                        let p = if dot(cross(sub(y, x), sub(z, x)), normal) < 0.0 { [x, z, y] } else { [x, y, z] };
                        out.tris.push(CapTri { p, owner, color, normal, cap });
                    }
                    let edge = if plain { EDGE } else { look.edge(owner, EDGE) };
                    out.lines.extend(geom::ring_edges(&ring).map(|(x, y)| ([at3(x), at3(y)], edge, owner)));
                    out.lines.extend(strokes.into_iter().map(|([x, y], c)| ([at3(x), at3(y)], c, owner)));
                }
            }
        }
        out
    }
}

impl Piece {
    fn solid_color(&self) -> Rgb {
        match &self.solid {
            Solid::Box { color, .. } | Solid::Slab { color, .. } | Solid::Prism { color, .. } | Solid::Mesh { color, .. } => *color,
        }
    }
}

/// The layers of `owner`'s type, which a piece's layer is the place of.
fn layers_of(m: &Model, owner: Id) -> &[Layer] {
    match m.elements.get(owner) {
        Some(Element::Wall(w)) => m.type_layers("wall", &w.type_name),
        Some(Element::Floor(f)) => m.type_layers("floor", &f.type_name),
        Some(Element::Roof(r)) => m.type_layers("roof", &r.type_name),
        Some(Element::Ceiling(c)) => m.type_layers("ceiling", &c.type_name),
        _ => &[],
    }
}

impl Image {
    /// The picture with `caps` over it, as `cam` sees them: each cap where nothing nearer is, shaded as a face square to its
    /// box face (paper in hidden line), tinted as its element is, and picking it; then its outline and pattern, where nothing
    /// nearer hides them. A wireframe has no depths, and takes none.
    pub fn capped(&mut self, cam: &Camera, caps: &Caps, tints: &[(Id, Rgb)], look: &Look) {
        let (w, h) = (self.width as usize, self.height as usize);
        if caps.is_empty() || self.depth.len() != w * h || look.style() == VisualStyle::Wireframe {
            return;
        }
        let flat = look.style() != VisualStyle::Shaded;
        let (back, right, up) = cam.axes();
        let eye = cam.eye();
        let light = unit(add(add(mul(right, -0.35), mul(up, 0.55)), mul(back, 0.75)));
        let lit = |n: P3, at: P3| {
            let toward = eye.map_or(back, |e| sub(e, at));
            let n = if dot(n, toward) < 0.0 { mul(n, -1.0) } else { n };
            0.62 + 0.38 * dot(n, light).max(0.0)
        };
        let paint = |c: Rgb, k: f64| {
            let v = |x: u8| (f64::from(x) * k).round() as u8;
            Rgb(v(c.0), v(c.1), v(c.2))
        };
        let tinted = |owner: Id, c: Rgb| tints.iter().find(|(id, _)| *id == owner).map_or(c, |(_, t)| mix(c, *t));
        let depth = Arc::make_mut(&mut self.depth);
        let hit = Arc::make_mut(&mut self.hit);
        let owners = Arc::make_mut(&mut self.owners);
        let project = cam.projector();
        let mut slots: Vec<(Id, u32)> = Vec::new();
        for t in &caps.tris {
            let slot = match slots.iter().find(|(id, _)| *id == t.owner) {
                Some((_, s)) => *s,
                None => {
                    let s = owners.len() as u32;
                    owners.push(t.owner);
                    slots.push((t.owner, s));
                    s
                }
            };
            let face = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            let n = if dot(face, t.normal) < 0.0 { mul(face, -1.0) } else { face };
            let color = tinted(t.owner, if flat { PAPER } else { paint(t.color, lit(n, t.p[0])) });
            let (parts, count) = project.in_front(&t.p);
            for part in parts.iter().take(count) {
                let [a, b, c] = weighed(&t.p, part).map(|p| project.at(p));
                let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                if area.is_nan() || area.abs() <= 1e-12 {
                    continue;
                }
                let (x0, x1) = (a.0.min(b.0).min(c.0).max(0.0), a.0.max(b.0).max(c.0).min(w as f64 - 1.0));
                let (y0, y1) = (a.1.min(b.1).min(c.1).max(0.0), a.1.max(b.1).max(c.1).min(h as f64 - 1.0));
                if !(x0 <= x1 && y0 <= y1) {
                    continue;
                }
                for py in y0 as usize..=y1 as usize {
                    for px in x0 as usize..=x1 as usize {
                        let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                        let wa = ((b.0 - x) * (c.1 - y) - (b.1 - y) * (c.0 - x)) / area;
                        let wb = ((c.0 - x) * (a.1 - y) - (c.1 - y) * (a.0 - x)) / area;
                        let wc = 1.0 - wa - wb;
                        if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                            continue;
                        }
                        let z = (wa * a.2 + wb * b.2 + wc * c.2) as f32;
                        let k = py * w + px;
                        if let (Some(d), Some(s), Some(rgb)) = (depth.get_mut(k), hit.get_mut(k), self.rgba.get_mut(4 * k..4 * k + 3))
                            && z > *d
                        {
                            (*d, *s) = (z, slot);
                            rgb.copy_from_slice(&[color.0, color.1, color.2]);
                        }
                    }
                }
            }
        }
        let rgba = &mut self.rgba;
        for ([a, b], c, owner) in &caps.lines {
            let Some([a, b]) = cam.in_front(*a, *b) else { continue };
            let c = tinted(*owner, *c);
            outline((cam.project(a), cam.project(b)), (w, h), 1, &|z| cam.slack(1.5, z), depth, |k| {
                if let Some(px) = rgba.get_mut(4 * k..4 * k + 3) {
                    px.copy_from_slice(&[c.0, c.1, c.2]);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::view3d::{AZIMUTH, ELEVATION, Scene};
    use serde_json::json;

    /// The sample house with a section box from its least corner to `f` of the way across what it holds.
    fn boxed(f: [f64; 3]) -> Session {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        s.execute("section_box", &json!({"on": true})).unwrap();
        let (lo, hi) = Look::of_3d(&s.model).section_box().unwrap();
        let to: [f64; 3] = std::array::from_fn(|k| lo[k] + (hi[k] - lo[k]) * f[k]);
        s.execute("section_box", &json!({"min": lo, "max": to})).unwrap();
        s
    }

    fn caps_of(s: &Session) -> Caps {
        CapSource::new(&s.model).caps(&s.model, &Look::of_3d(&s.model), (100.0, Detail::Fine))
    }

    fn category(s: &Session, id: Id) -> &'static str {
        s.model.elements.get(id).map_or("", Element::category)
    }

    #[test]
    fn a_section_box_caps_what_it_cuts_each_layer_in_its_material_with_its_cut_pattern() {
        let s = boxed([0.6, 0.7, 0.45]);
        let (lo, hi) = Look::of_3d(&s.model).section_box().unwrap();
        let caps = caps_of(&s);
        assert!(!caps.is_empty(), "the box cuts the house");
        // Every cap lies on a face of the box, inside it.
        for t in &caps.tris {
            let on = (0..3).any(|k| t.p.iter().all(|p| (p[k] - lo[k]).abs() < 1e-6) || t.p.iter().all(|p| (p[k] - hi[k]).abs() < 1e-6));
            assert!(on, "{:?} is off the box's faces", t.p);
            for p in t.p {
                assert!((0..3).all(|k| p[k] >= lo[k] - 1e-6 && p[k] <= hi[k] + 1e-6), "{p:?} is outside the box");
            }
        }
        // A wall cut, in more than one colour: its layers'.
        let walls: Vec<&CapTri> = caps.tris.iter().filter(|t| category(&s, t.owner) == "wall").collect();
        assert!(!walls.is_empty(), "the box cuts walls");
        let wall = walls[0].owner;
        let colours: std::collections::BTreeSet<(u8, u8, u8)> =
            walls.iter().filter(|t| t.owner == wall).map(|t| (t.color.0, t.color.1, t.color.2)).collect();
        assert!(colours.len() >= 2, "a compound wall's layers each in their material: {colours:?}");
        // Over them, the layers' cut patterns in ink, besides the outlines in the edges' colour.
        assert!(caps.lines.iter().any(|(_, c, _)| *c == Rgb(25, 25, 25)), "no cut pattern drawn");
        assert!(caps.lines.iter().any(|(_, c, _)| *c == EDGE), "no outline drawn");
        // The floor is cut too, and nothing without a box.
        assert!(caps.tris.iter().any(|t| category(&s, t.owner) == "floor"));
        let mut off = s;
        off.execute("section_box", &json!({"on": false})).unwrap();
        assert!(caps_of(&off).is_empty());
    }

    #[test]
    fn a_view_s_cut_pattern_override_fills_the_caps_of_what_it_names() {
        let mut s = boxed([0.6, 0.7, 0.45]);
        let style = json!({"foreground": {"pattern": "Horizontal", "color": [220, 0, 0]}, "background": {"pattern": "Solid fill", "color": [30, 180, 50]}});
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"wall": {"cut": style}}})).unwrap();
        let caps = caps_of(&s);
        let walls: Vec<&CapTri> = caps.tris.iter().filter(|t| category(&s, t.owner) == "wall").collect();
        assert!(!walls.is_empty() && walls.iter().all(|t| t.color == Rgb(30, 180, 50)), "the background fills them");
        assert!(caps.lines.iter().any(|(_, c, o)| *c == Rgb(220, 0, 0) && category(&s, *o) == "wall"), "the foreground's lines");
        assert!(caps.tris.iter().filter(|t| category(&s, t.owner) == "floor").all(|t| t.color != Rgb(30, 180, 50)), "floors as they were");
        // Hidden in the view, not capped.
        s.execute("graphics", &json!({"view": "{3D}", "categories": {"wall": {"hidden": true}}})).unwrap();
        assert!(caps_of(&s).tris.iter().all(|t| category(&s, t.owner) != "wall"));
    }

    #[test]
    fn moving_one_face_of_the_box_cuts_again_only_along_that_face() {
        let s = boxed([0.6, 0.7, 0.45]);
        let source = CapSource::new(&s.model);
        let look = Look::of_3d(&s.model);
        let (lo, hi) = look.section_box().unwrap();
        let first = source.caps(&s.model, &look, (100.0, Detail::Fine));
        let held: Vec<Cuts> = (0..3).flat_map(|k| [source.face(k, lo[k]), source.face(k, hi[k])]).collect();
        let higher = [hi[0], hi[1], hi[2] + 300.0];
        let moved = source.caps(&s.model, &Look::of_3d(&s.model).with_section_box(Some((lo, higher))), (100.0, Detail::Fine));
        assert_ne!(first, moved, "the caps follow the face");
        for (i, k) in (0..3).flat_map(|k| [(2 * k, lo[k]), (2 * k + 1, hi[k])].map(|(i, at)| (i, (k, at)))) {
            let now = source.face(k.0, k.1);
            assert!(Arc::ptr_eq(&held[i], &now), "face {i} was cut again");
        }
        // And as a box made afresh caps it.
        let mut again = s;
        again.execute("section_box", &json!({"min": lo, "max": higher})).unwrap();
        assert_eq!(caps_of(&again), moved);
    }

    #[test]
    fn the_picture_shows_the_caps_solid_and_picks_what_they_cut() {
        let s = boxed([0.6, 0.7, 0.45]);
        let look = Look::of_3d(&s.model);
        let scene = Scene::new(&s.model);
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 480, 360, &look);
        let hollow = scene.render_in(&cam, &[], &look);
        let mut solid = hollow.clone();
        let caps = caps_of(&s);
        solid.capped(&cam, &caps, &[], &look);
        let changed: Vec<(f64, f64)> = (0..360)
            .flat_map(|y| (0..480).map(move |x| (f64::from(x) + 0.5, f64::from(y) + 0.5)))
            .filter(|&(x, y)| hollow.color(x, y) != solid.color(x, y))
            .collect();
        assert!(changed.len() > 200, "only {} pixels capped", changed.len());
        let capped: std::collections::BTreeSet<Id> = caps.tris.iter().map(|t| t.owner).collect();
        let picked = changed.iter().filter(|(x, y)| solid.pick(*x, *y).is_some_and(|id| capped.contains(&id))).count();
        assert!(picked * 2 > changed.len(), "a cap picks what it cuts: {picked} of {}", changed.len());
    }
}

#[cfg(test)]
mod measurements {
    use super::*;

    /// How long a tower's solids take to find, and a moved face to cut: `cargo test --release -p bimcraft-core caps_on_a_tower -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement"]
    fn caps_on_a_tower() {
        let m = crate::sample::highrise(30).unwrap();
        let (lo, hi) = m.model_box().unwrap();
        let mid: P3 = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
        let at = std::time::Instant::now();
        let source = CapSource::new(&m);
        println!("{} pieces found in {:?}", source.pieces.len(), at.elapsed());
        let look = Look::default().with_section_box(Some((lo, mid)));
        let at = std::time::Instant::now();
        let caps = source.caps(&m, &look, (100.0, Detail::Fine));
        println!("six faces cut: {} triangles, {} lines in {:?}", caps.tris.len(), caps.lines.len(), at.elapsed());
        for step in 1..=3 {
            let top = [mid[0], mid[1], mid[2] - 500.0 * f64::from(step)];
            let at = std::time::Instant::now();
            let caps = source.caps(&m, &Look::default().with_section_box(Some((lo, top))), (100.0, Detail::Fine));
            println!("top moved: {} triangles, {} lines in {:?}", caps.tris.len(), caps.lines.len(), at.elapsed());
        }
    }
}
