//! Plan views: what a level's plan shows, as a display list of filled rings and polylines in
//! pixels, drawn to SVG or PNG. Drafting conventions: walls and columns the cut plane passes
//! through get a heavy outline and grey poché, and below it a thin one (faint past the view
//! range's bottom); doors a leaf and swing, windows glass lines, stairs their risers up to the cut
//! line and a walk line, roofs their ridge and hips (dashed overhead, below the view range's top),
//! rooms a tag, grids a dashed line with a bubble at each end, dimensions witness lines and ticks
//! (an arc for an angle, an arrow for a radius), door and window tags their mark in a box, text
//! notes their lines, spot elevations a triangle and their height, sections their line and head,
//! elevations a marker, sloped floors their slope arrow, shafts their outline and a cross; all of
//! it inside the plan's crop region. A level's reflected ceiling plan cuts its walls higher, over
//! the doors, and looks up at its ceilings: their outlines and tiles. Sections and elevations draw the
//! same way along their line and up: the cut in poché, levels and grids, notes and spot
//! elevations, over the 3D view's hidden-line picture of what lies beyond.

use std::f64::consts::PI;
use std::fmt::Write;

use tiny_skia::{Color, FillRule, IntSize, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform};

use crate::geom::{self, P2};
use crate::graphics::Look;
use crate::model::{
    CEILING_CUT, Ceiling, DimKind, Dimension, Element, Floor, Function, Id, Layer, Model, Note, Opening, Pattern, PlanRange, Railing, Ramp,
    Roof, Section, Shaft, Stair, TOL, Tag, Wall,
};
use crate::units::Units;
use crate::view3d::{self, Camera, Clip, Image, Scene};
use crate::walls::{self, Axis};
use crate::{font, sheet, stairs};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const PAPER: Rgb = Rgb(255, 255, 255);
/// Wall poché, the grey of cut walls.
pub const POCHE: Rgb = Rgb(168, 168, 168);
/// A cut compound wall's layers that aren't its structure: lighter.
pub const LIGHT_POCHE: Rgb = Rgb(212, 212, 212);
const INK: Rgb = Rgb(25, 25, 25);
const FAINT: Rgb = Rgb(120, 120, 120);
const GRID: Rgb = Rgb(60, 100, 150);
/// Room for grid bubbles and the view title around the drawing, px.
const MARGIN: f64 = 70.0;
const BUBBLE: f32 = 11.0;

/// What a primitive draws of its element, for the view's overrides: lines and fills in projection
/// (what lies below the cut plane, beyond a section's line, annotation) or where the view cuts
/// it; lettering, which takes a line colour and no weight; and hatching, which takes neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
    #[default]
    Projection,
    Cut,
    Text,
    Pattern,
}

pub enum Prim {
    Fill {
        rings: Vec<Vec<(f32, f32)>>,
        color: Rgb,
    },
    Line {
        pts: Vec<(f32, f32)>,
        width: f32,
        color: Rgb,
        dash: Option<[f32; 2]>,
        closed: bool,
    },
    /// A picture, its top left corner `at`, a pixel to a pixel: a section's on a sheet.
    Image {
        at: (f32, f32),
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
}

impl Prim {
    /// The same, `dx` px right and `dy` down.
    pub(crate) fn moved(self, dx: f32, dy: f32) -> Prim {
        let mv = |pts: Vec<(f32, f32)>| pts.into_iter().map(|(x, y)| (x + dx, y + dy)).collect();
        match self {
            Prim::Fill { rings, color } => Prim::Fill { rings: rings.into_iter().map(mv).collect(), color },
            Prim::Line { pts, width, color, dash, closed } => Prim::Line { pts: mv(pts), width, color, dash, closed },
            Prim::Image { at, width, height, rgba } => Prim::Image { at: (at.0 + dx, at.1 + dy), width, height, rgba },
        }
    }

    /// An image's corners, clockwise from its top left.
    fn corners(at: (f32, f32), width: u32, height: u32) -> Vec<(f32, f32)> {
        let (w, h) = (width as f32, height as f32);
        vec![at, (at.0 + w, at.1), (at.0 + w, at.1 + h), (at.0, at.1 + h)]
    }

    /// What of it lies inside the box from pixel `lo` to `hi`: a fill's rings clipped, a line in
    /// the pieces inside, a picture's whole pixels inside.
    fn cropped(self, lo: (f32, f32), hi: (f32, f32)) -> Vec<Prim> {
        let mm = |p: (f32, f32)| P2(f64::from(p.0), f64::from(p.1));
        let px = |p: P2| (p.0 as f32, p.1 as f32);
        match self {
            Prim::Fill { rings, color } => {
                let rings: Vec<Vec<(f32, f32)>> = rings.iter().map(|r| clip_ring(r, lo, hi)).filter(|r| r.len() >= 3).collect();
                if rings.is_empty() { Vec::new() } else { vec![Prim::Fill { rings, color }] }
            }
            Prim::Line { pts, width, color, dash, closed } => {
                let inside = |p: &(f32, f32)| (lo.0..=hi.0).contains(&p.0) && (lo.1..=hi.1).contains(&p.1);
                if pts.iter().all(inside) {
                    return vec![Prim::Line { pts, width, color, dash, closed }];
                }
                let close = closed.then(|| pts.last().copied().zip(pts.first().copied())).flatten();
                let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
                for (a, b) in pts.iter().copied().zip(pts.iter().copied().skip(1)).chain(close) {
                    let Some((p, q)) = clip(mm(a), mm(b), mm(lo), mm(hi)) else { continue };
                    let (p, q) = (px(p), px(q));
                    match runs.last_mut() {
                        Some(run) if run.last() == Some(&p) => run.push(q),
                        _ => runs.push(vec![p, q]),
                    }
                }
                runs.into_iter().map(|pts| Prim::Line { pts, width, color, dash, closed: false }).collect()
            }
            Prim::Image { at, width, height, rgba } => {
                let side = |lo: f32, hi: f32, from: f32, n: u32| {
                    let first = (lo - from).ceil().clamp(0.0, n as f32) as u32;
                    (first, ((hi - from).floor().clamp(0.0, n as f32) as u32).max(first))
                };
                let ((x0, x1), (y0, y1)) = (side(lo.0, hi.0, at.0, width), side(lo.1, hi.1, at.1, height));
                if x1 == x0 || y1 == y0 {
                    return Vec::new();
                }
                let row = (x1 - x0) as usize * 4;
                let mut kept = Vec::with_capacity(row * (y1 - y0) as usize);
                for y in y0..y1 {
                    let from = (y as usize * width as usize + x0 as usize) * 4;
                    kept.extend_from_slice(rgba.get(from..from + row).unwrap_or_default());
                }
                vec![Prim::Image { at: (at.0 + x0 as f32, at.1 + y0 as f32), width: x1 - x0, height: y1 - y0, rgba: kept }]
            }
        }
    }
}

/// Model millimetres (y up) to image pixels (y down). In a section or elevation, the first
/// coordinate runs along its line from its start, the second up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Pixels per millimetre.
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub fn px(&self, p: P2) -> (f32, f32) {
        (
            (f64::from(self.width) / 2.0 + (p.0 - self.cx) * self.scale) as f32,
            (f64::from(self.height) / 2.0 - (p.1 - self.cy) * self.scale) as f32,
        )
    }

    /// The model point at a pixel: the inverse of [`View::px`].
    pub fn mm(&self, (x, y): (f32, f32)) -> P2 {
        P2(
            self.cx + (f64::from(x) - f64::from(self.width) / 2.0) / self.scale,
            self.cy - (f64::from(y) - f64::from(self.height) / 2.0) / self.scale,
        )
    }
}

pub struct Drawing {
    pub view: View,
    pub prims: Vec<Prim>,
    /// The element each primitive draws, for picking and highlighting.
    pub owners: Vec<Option<Id>>,
    /// What each primitive draws of its element, for overrides.
    pub parts: Vec<Part>,
    /// The element being drawn now, and the part of it.
    owner: Option<Id>,
    part: Part,
}

impl Drawing {
    /// Nothing yet, through `view`.
    pub fn new(view: View) -> Drawing {
        Drawing { view, prims: Vec::new(), owners: Vec::new(), parts: Vec::new(), owner: None, part: Part::Projection }
    }
    /// What the primitives pushed next draw.
    pub(crate) fn own(&mut self, owner: Option<Id>) {
        self.owner = owner;
    }
    /// The part of their element the primitives pushed next draw.
    pub(crate) fn part(&mut self, part: Part) {
        self.part = part;
    }
    pub(crate) fn push(&mut self, p: Prim) {
        self.prims.push(p);
        self.owners.push(self.owner);
        self.parts.push(self.part);
    }
    /// `rings` filled, each turned the same way round, so where they overlap or meet they fill as one.
    pub(crate) fn fill(&mut self, rings: &[Vec<P2>], color: Rgb) {
        if rings.is_empty() {
            return;
        }
        let rings = rings
            .iter()
            .map(|r| {
                let mut r: Vec<P2> = if geom::area(r) < 0.0 { r.iter().rev().copied().collect() } else { r.clone() };
                r.dedup();
                r.iter().map(|p| self.view.px(*p)).collect()
            })
            .collect();
        self.push(Prim::Fill { rings, color });
    }
    pub(crate) fn line(&mut self, pts: &[P2], width: f32, color: Rgb, closed: bool) {
        let pts = pts.iter().map(|p| self.view.px(*p)).collect();
        self.push(Prim::Line { pts, width, color, dash: None, closed });
    }
    /// `text` centred on `at` px, capitals `height` px tall.
    pub(crate) fn text(&mut self, text: &str, at: (f32, f32), height: f32, color: Rgb) {
        self.text_turned(text, at, height, 0.0, color);
    }

    /// What's drawn cropped to the box from pixel `lo` to `hi`, but for the primitives of the
    /// elements `keep` takes.
    pub(crate) fn crop(&mut self, lo: (f32, f32), hi: (f32, f32), keep: impl Fn(Option<Id>) -> bool) {
        let (prims, owners, parts) = (std::mem::take(&mut self.prims), std::mem::take(&mut self.owners), std::mem::take(&mut self.parts));
        for ((prim, owner), part) in prims.into_iter().zip(owners).zip(parts) {
            for piece in if keep(owner) { vec![prim] } else { prim.cropped(lo, hi) } {
                self.prims.push(piece);
                self.owners.push(owner);
                self.parts.push(part);
            }
        }
    }

    /// `text` centred on `(x, y)` px, capitals `height` px tall, turned `angle` radians clockwise
    /// on screen (y runs down).
    fn text_turned(&mut self, text: &str, (x, y): (f32, f32), height: f32, angle: f32, color: Rgb) {
        let (s, c) = angle.sin_cos();
        let w = font::width(text, height);
        let part = std::mem::replace(&mut self.part, Part::Text);
        for pts in font::strokes(text, -w / 2.0, height / 2.0, height) {
            let pts = pts.into_iter().map(|(a, b)| (x + a * c - b * s, y + a * s + b * c)).collect();
            self.push(Prim::Line { pts, width: (height / 10.0).max(1.0), color, dash: None, closed: false });
        }
        self.part = part;
    }

    /// The element drawn nearest the pixel `at`, within `tol` px. Inside a fill beats lying on a
    /// line, so a grid line along a wall doesn't hide the wall; of equally near ones the last
    /// drawn, which shows on top, wins.
    pub fn pick(&self, at: (f32, f32), tol: f32) -> Option<Id> {
        self.pick_all(at, tol).first().copied()
    }

    /// Every element drawn within `tol` px of `at`, in the order [`Drawing::pick`] prefers them:
    /// what Tab cycles through.
    pub fn pick_all(&self, at: (f32, f32), tol: f32) -> Vec<Id> {
        let p = P2(f64::from(at.0), f64::from(at.1));
        let ring = |pts: &[(f32, f32)]| -> Vec<P2> { pts.iter().map(|&(x, y)| P2(f64::from(x), f64::from(y))).collect() };
        let edges = |pts: &[P2], closed: bool| {
            let open = pts.windows(2).filter_map(|w| match w {
                [a, b] => Some(geom::dist_to_segment(p, *a, *b)),
                _ => None,
            });
            let close = match (closed, pts.first(), pts.last()) {
                (true, Some(a), Some(b)) => Some(geom::dist_to_segment(p, *b, *a)),
                _ => None,
            };
            open.chain(close).fold(f64::INFINITY, f64::min)
        };
        // Each element's nearest primitive: its distance, and how late it's drawn.
        let mut near: std::collections::BTreeMap<Id, (f64, usize)> = std::collections::BTreeMap::new();
        for (order, (prim, owner)) in self.prims.iter().zip(&self.owners).enumerate() {
            let Some(id) = *owner else { continue };
            let d = match prim {
                Prim::Fill { rings, .. } => {
                    let rings: Vec<Vec<P2>> = rings.iter().map(|r| ring(r)).collect();
                    if rings.iter().filter(|r| geom::contains(r, p)).count() % 2 == 1 {
                        -1.0
                    } else {
                        rings.iter().map(|r| edges(r, true)).fold(f64::INFINITY, f64::min)
                    }
                }
                Prim::Line { pts, width, closed, .. } => (edges(&ring(pts), *closed) - f64::from(*width) / 2.0).max(0.0),
                Prim::Image { at, width, height, .. } => {
                    let r = ring(&Prim::corners(*at, *width, *height));
                    if geom::contains(&r, p) { -1.0 } else { edges(&r, true) }
                }
            };
            if d <= f64::from(tol) && near.get(&id).is_none_or(|&(b, _)| d <= b) {
                near.insert(id, (d, order));
            }
        }
        let mut found: Vec<(Id, (f64, usize))> = near.into_iter().collect();
        found.sort_by(|a, b| a.1.0.total_cmp(&b.1.0).then(b.1.1.cmp(&a.1.1)));
        found.into_iter().map(|(id, _)| id).collect()
    }

    /// The elements drawn wholly inside the box between pixels `a` and `b`: Revit's window
    /// selection. With `crossing`, the ones it touches too.
    pub fn in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Id> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let inside = |q: &(f32, f32)| (lo.0..=hi.0).contains(&q.0) && (lo.1..=hi.1).contains(&q.1);
        // Each element: whether all of it is inside, and whether any of it is.
        let mut seen: std::collections::BTreeMap<Id, (bool, bool)> = std::collections::BTreeMap::new();
        for (prim, owner) in self.prims.iter().zip(&self.owners) {
            let Some(id) = *owner else { continue };
            let corners;
            let (rings, closed): (Vec<&[(f32, f32)]>, bool) = match prim {
                Prim::Fill { rings, .. } => (rings.iter().map(Vec::as_slice).collect(), true),
                Prim::Image { at, width, height, .. } => {
                    corners = Prim::corners(*at, *width, *height);
                    (vec![corners.as_slice()], true)
                }
                Prim::Line { pts, closed, .. } => (vec![pts.as_slice()], *closed),
            };
            let all = rings.iter().all(|r| r.iter().all(inside));
            let any = rings.iter().any(|r| {
                let last = closed.then(|| (r.last(), r.first()));
                r.windows(2)
                    .filter_map(|w| Some((w.first()?, w.get(1)?)))
                    .chain(last.into_iter().filter_map(|(p, q)| Some((p?, q?))))
                    .any(|(p, q)| clips(*p, *q, lo, hi))
                    || r.iter().any(inside)
            });
            let e = seen.entry(id).or_insert((true, false));
            *e = (e.0 && all, e.1 || any);
        }
        seen.into_iter().filter(|(_, (all, any))| if crossing { *any } else { *all }).map(|(id, _)| id).collect()
    }
}

/// The outlines of the walls and columns a plan cutting at height `z` cuts, but for those `look` hides.
fn cut_outlines(m: &Model, z: f64, look: &Look) -> Vec<(Id, Vec<P2>)> {
    m.elements
        .iter()
        .filter(|(id, _)| look.shows(**id))
        .filter_map(|(&id, e)| match e {
            Element::Wall(w) if m.spans(w, z) => Some((id, walls::outline(m, id))),
            Element::Column(col) => {
                let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
                (bottom <= z && z < top).then(|| (id, m.column_outline(col)))
            }
            _ => None,
        })
        .collect()
}

/// The walls and columns a plan sees below its cut plane, down to its view depth: their outlines,
/// and their tops; but for those `look` hides.
fn seen_below(m: &Model, r: &PlanRange, look: &Look) -> Vec<(Id, Vec<P2>, f64)> {
    m.elements
        .iter()
        .filter(|(id, _)| look.shows(**id))
        .filter_map(|(&id, e)| {
            let (outline, top) = match e {
                Element::Wall(w) => (walls::outline(m, id), m.top_z(w)),
                Element::Column(col) => (m.column_outline(col), m.span(col.level, col.base_offset, col.top_level, col.height).1),
                _ => return None,
            };
            (r.depth < top && top <= r.cut).then_some((id, outline, top))
        })
        .collect()
}

/// The cut outlines `look` fills in the same colour, together, so no seam shows where two fills
/// meet: each colour, and the outlines it fills.
fn cut_unions(look: &Look, outlines: impl Iterator<Item = (Id, Vec<P2>)>) -> Vec<(Rgb, Vec<Vec<P2>>)> {
    let mut unions: Vec<(Rgb, Vec<Vec<P2>>)> = Vec::new();
    for (id, o) in outlines {
        let color = look.cut_fill(id, POCHE);
        match unions.iter_mut().find(|(c, _)| *c == color) {
            Some((_, rings)) => rings.push(o),
            None => unions.push((color, vec![o])),
        }
    }
    unions
}

/// The plan of `level` fitted into `width` x `height` px.
pub fn plan(m: &Model, level: Id, width: u32, height: u32) -> Drawing {
    draw(m, level, fit(&plan_extent(m, level), width, height))
}

/// What a plan of `level` takes on paper: all it shows, or cropped, its crop region and the grids'
/// ends at its edges.
pub(crate) fn plan_extent(m: &Model, level: Id) -> Vec<P2> {
    match m.level(level).and_then(|l| l.crop) {
        None => plan_shows(m, level),
        Some([lo, hi]) => {
            let grids = m.elements.values().filter_map(|e| match e {
                Element::Grid(g) => clip(g.start, g.end, lo, hi),
                _ => None,
            });
            [lo, hi].into_iter().chain(grids.flat_map(|(a, b)| [a, b])).collect()
        }
    }
}

/// Everything a plan of `level` shows, uncropped: its walls, floors, rooms, dimensions, tags,
/// notes and spot elevations, the grids, and the marks of the sections and elevations.
pub(crate) fn plan_shows(m: &Model, level: Id) -> Vec<P2> {
    let r = m.plan_range(level);
    let look = Look::of(m, level);
    let mut extent: Vec<P2> = cut_outlines(m, r.cut, &look).into_iter().flat_map(|(_, o)| o).collect();
    extent.extend(seen_below(m, &r, &look).into_iter().flat_map(|(_, o, _)| o));
    for (_, e) in m.elements.iter().filter(|(id, _)| look.shows(**id)) {
        match e {
            Element::Floor(Floor { level: l, boundary, .. })
            | Element::Ceiling(Ceiling { level: l, boundary, .. })
            | Element::Shaft(Shaft { level: l, boundary, .. })
                if *l == level =>
            {
                extent.extend(boundary)
            }
            Element::Room(r) if r.level == level => extent.push(r.point),
            Element::Component(c) if c.level == level => extent.extend(m.component_pieces(c).into_iter().flat_map(|p| p.outline)),
            Element::Grid(g) => extent.extend([g.start, g.end]),
            Element::Dimension(dm) if dm.level == level => extent.push(dm.at),
            Element::Section(v) | Element::Elevation(v) => extent.extend([v.start, v.end]),
            Element::Tag(t) if m.level_of(e) == Some(level) => extent.extend(m.anchor(t.host).map(|a| a + t.offset)),
            Element::Stair(Stair { level: l, top_level: t, .. }) | Element::Ramp(Ramp { level: l, top_level: t, .. })
                if *l == level || *t == Some(level) =>
            {
                if let Some(l) = stairs::layout(m, e) {
                    extent.extend(l.runs.iter().flat_map(|r| r.ring(l.width)).chain(l.pads.into_iter().flat_map(|p| p.ring)));
                }
            }
            Element::Railing(rl) if m.level_of(e) == Some(level) => extent.extend(stairs::railing(m, rl).into_iter().map(|(p, _)| p)),
            Element::Roof(r) if r.level == level || m.level_above(level) == Some(r.level) => extent.extend(m.roof_outline(r)),
            Element::Text(n) if n.view == level => extent.extend([Some(n.at), n.leader].into_iter().flatten()),
            Element::Spot(sp) if sp.view == level => extent.push(sp.at),
            _ => {}
        }
    }
    extent
}

/// An element that isn't in the model yet, drawn as a plan would draw it, its primitives owned
/// by `id`: what a tool previews. Annotation, columns, stairs and roofs only.
pub fn sketch(m: &Model, view: View, id: Id, e: &Element) -> Drawing {
    let mut d = Drawing::new(view);
    d.owner = Some(id);
    match e {
        Element::Dimension(dm) => dimension(&mut d, m, dm),
        Element::Tag(t) => tag(&mut d, m, t),
        Element::Text(n) => note(&mut d, n),
        Element::Spot(sp) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
        Element::Section(v) => section_mark(&mut d, v),
        Element::Elevation(v) => elevation_mark(&mut d, v),
        Element::Column(col) => d.line(&m.column_outline(col), 2.0, INK, true),
        Element::Component(c) => component(&mut d, &m.component_pieces(c)),
        Element::Stair(st) => stair(&mut d, m, st, st.level),
        Element::Ramp(r) => ramp(&mut d, m, r, r.level),
        Element::Railing(rl) => d.line(&stairs::railing(m, rl).into_iter().map(|(p, _)| p).collect::<Vec<_>>(), 1.5, INK, false),
        Element::Roof(r) => roof(&mut d, m, r),
        _ => {}
    }
    d
}

/// How far past the lowest and highest heights a section's grids run, mm.
const GRID_PAST: f64 = 600.0;

/// From the lowest level or solid `look` shows to the highest.
fn heights(m: &Model, look: &Look) -> (f64, f64) {
    let solids: Vec<_> = view3d::solids(m).into_iter().filter(|s| look.shows(s.owner())).collect();
    let zs = m.levels().into_iter().map(|(_, l)| l.elevation).chain(solids.iter().flat_map(|s| {
        let (a, b) = s.heights();
        [a, b]
    }));
    let (lo, hi) = zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
    if lo <= hi { (lo, hi) } else { (0.0, 3000.0) }
}

/// The crop region of view `id` as it shows: a cropped plan's corners; a cropped section's or
/// elevation's line, along it, and its crop's heights. None for a view that isn't cropped, or
/// another element.
pub fn crop_region(m: &Model, id: Id) -> Option<[P2; 2]> {
    match m.elements.get(&id)? {
        Element::Level(l) => l.crop,
        Element::Section(v) | Element::Elevation(v) => v.crop.map(|[bottom, top]| [P2(0.0, bottom), P2(v.start.dist(v.end), top)]),
        _ => None,
    }
}

/// The heights section or elevation `id` would crop to: all of the model's, as far as its grids
/// reach past them.
pub(crate) fn section_heights(m: &Model, id: Id) -> [f64; 2] {
    let (lo, hi) = heights(m, &Look::of(m, id));
    [lo - GRID_PAST, hi + GRID_PAST]
}

/// How far along its line, from its start, section or elevation `v` shows: cropped, from its start
/// to its end; else as far as anything it shows reaches (what lies beyond its line, its levels and
/// its grids), and its line.
fn span(m: &Model, v: &Section, look: &Look) -> (f64, f64) {
    let len = v.start.dist(v.end);
    if v.crop.is_some() {
        return (0.0, len);
    }
    let u = (v.end - v.start).unit();
    let along = |q: P2| (q - v.start).dot(u);
    let solids = view3d::solids(m).into_iter().filter(|s| look.shows(s.owner())).flat_map(|s| s.corners()).map(|p| along(P2(p[0], p[1])));
    let levels = level_spans(m, v, look).into_iter().flat_map(|(_, a, b)| [a, b]);
    let grids = m.elements.iter().filter(|(id, _)| look.shows(**id)).filter_map(|(_, e)| match e {
        Element::Grid(g) => geom::line_intersection(v.start, v.end, g.start, g.end).map(along),
        _ => None,
    });
    solids.chain(levels).chain(grids).fold((0.0, len), |(lo, hi), x| (lo.min(x), hi.max(x)))
}

/// The levels section or elevation `v` sees, each from how far along its line to how far: where
/// its extent lies, past its line (within its depth).
fn level_spans(m: &Model, v: &Section, look: &Look) -> Vec<(Id, f64, f64)> {
    let u = (v.end - v.start).unit();
    let past = |q: P2| (q - v.start).dot(v.looking());
    let default = std::cell::OnceCell::new();
    m.levels()
        .into_iter()
        .filter(|(id, _)| look.shows(*id))
        .filter_map(|(id, l)| {
            let [lo, hi] = l.extent.unwrap_or_else(|| *default.get_or_init(|| m.model_extent()));
            let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)];
            let (near, far) = corners.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(past(*q)), b.max(past(*q))));
            if far < -TOL || v.depth.is_some_and(|d| near > d) {
                return None;
            }
            let (a, b) =
                corners.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min((*q - v.start).dot(u)), b.max((*q - v.start).dot(u))));
            Some((id, a, b))
        })
        .collect()
}

/// The heights a section or elevation shows: its crop's, else all of the model's.
fn shown_heights(m: &Model, v: &Section, look: &Look) -> (f64, f64) {
    v.crop.map_or_else(|| heights(m, look), |[bottom, top]| (bottom, top))
}

/// The view section or elevation `id` fits into `width` x `height` px: along its line from its
/// start, and up, with room for its levels' names and its grids' bubbles.
pub fn section_fit(m: &Model, id: Id, width: u32, height: u32) -> View {
    // At no scale, the level names take no room; at the scale that gives, they do.
    let first = section_extent(m, id, f64::INFINITY).map_or_else(|| fit(&[], width, height), |e| fit(&e, width, height));
    section_extent(m, id, first.scale).map_or(first, |e| fit(&e, width, height))
}

/// What section or elevation `id` shows, along its line and up, at `scale` px per mm: from its
/// start to its end, past its lowest and highest heights, with room past the end for the widest
/// level name.
pub(crate) fn section_extent(m: &Model, id: Id, scale: f64) -> Option<[P2; 2]> {
    let v = m.section(id)?;
    let look = Look::of(m, id);
    let ((lo, hi), (first, last)) = (shown_heights(m, v, &look), span(m, v, &look));
    // The widest level name or height.
    let head =
        m.levels().iter().map(|(_, l)| font::width(&l.name, 10.0).max(font::width(&m.units.length(l.elevation), 10.0))).fold(0.0, f32::max)
            + 12.0;
    Some([P2(first, lo - GRID_PAST), P2(last + f64::from(head) / scale, hi + GRID_PAST)])
}

/// The camera, and what it keeps, that show what section or elevation `id` sees through `view`.
pub fn section_camera(m: &Model, id: Id, view: View) -> Option<(Camera, Clip)> {
    let v = m.section(id)?;
    let (u, back) = ((v.end - v.start).unit(), v.looking() * -1.0);
    let at = v.start + u * view.cx;
    let cam = Camera {
        azimuth: back.1.atan2(back.0).to_degrees(),
        elevation: 0.0,
        scale: view.scale,
        target: [at.0, at.1, view.cy],
        width: view.width,
        height: view.height,
    };
    let column = |s: f64| f64::from(view.width) / 2.0 + (s - view.cx) * view.scale;
    let row = |z: f64| f64::from(view.height) / 2.0 - (z - view.cy) * view.scale;
    let rows = v.crop.map_or((f64::NEG_INFINITY, f64::INFINITY), |[bottom, top]| (row(top), row(bottom)));
    let columns = match v.crop {
        Some(_) => (column(0.0), column(v.start.dist(v.end))),
        None => (f64::NEG_INFINITY, f64::INFINITY),
    };
    Some((cam, Clip { near: 0.0, far: v.depth, x: columns, y: rows }))
}

/// What section or elevation `id` sees past its line through `view`, in hidden line: the
/// picture its cut, levels and grids ([`section`]) go over.
pub fn section_image(m: &Model, scene: &Scene, id: Id, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    let (cam, clip) = section_camera(m, id, view)?;
    Some(scene.cut_in(&cam, &clip, tints, &Look::of(m, id)))
}

/// What section or elevation `id` draws over its picture through `view`: where its line cuts
/// the solids, in poché; its levels, named past its end; the grids it crosses; its notes and spot
/// elevations; and its name. Cropped, only what lies between its crop's heights.
pub fn section(m: &Model, id: Id, view: View) -> Drawing {
    let mut d = Drawing::new(view);
    let Some(v) = m.section(id) else { return d };
    let look = Look::of(m, id);
    let u = (v.end - v.start).unit();
    let (first, last) = span(m, v, &look);
    // Each element's cut pieces, filled as one so where they meet (a roof's slopes at its ridge)
    // leaves no seam.
    let mut cuts: std::collections::BTreeMap<Id, Vec<Vec<P2>>> = std::collections::BTreeMap::new();
    let solids: Vec<_> = view3d::solids(m).into_iter().filter(|s| look.shows(s.owner())).collect();
    d.part(Part::Cut);
    for s in &solids {
        for (a, b) in s.across(v.start, u) {
            let (a, b) = (a.max(first), b.min(last));
            if b - a > 1e-6 {
                // Level, or along a sloped face.
                let ((a0, a1), (b0, b1)) = (s.z_at(v.start + u * a), s.z_at(v.start + u * b));
                cuts.entry(s.owner()).or_default().push(vec![P2(a, a0), P2(b, b0), P2(b, b1), P2(a, a1)]);
            }
        }
    }
    // Outlines, then poché over their inner halves: joined walls show their union, as in plan,
    // filled as one first so no seam shows where pieces meet.
    for (owner, rings) in &cuts {
        d.owner = Some(*owner);
        for r in rings {
            d.line(r, 3.0, INK, true);
        }
    }
    d.owner = None;
    let mut unions: Vec<(Rgb, Vec<Vec<P2>>)> = Vec::new();
    for (owner, rings) in &cuts {
        let color = look.cut_fill(*owner, POCHE);
        match unions.iter_mut().find(|(c, _)| *c == color) {
            Some((_, all)) => all.extend(rings.iter().cloned()),
            None => unions.push((color, rings.clone())),
        }
    }
    for (color, rings) in unions {
        d.fill(&rings, color);
    }
    for (owner, rings) in &cuts {
        d.owner = Some(*owner);
        d.fill(rings, POCHE);
    }
    // A compound wall's layers where the line cuts it: each its material's cut pattern, between where
    // the line crosses its boundaries; a thin line up each boundary between two, as high as the wall
    // stands there, around its openings.
    for (&wall, e) in &m.elements {
        let Element::Wall(w) = e else { continue };
        let layers = m.type_layers("wall", &w.type_name);
        if layers.is_empty() || !look.shows(wall) {
            continue;
        }
        d.owner = Some(wall);
        let lines = walls::layer_lines(m, wall);
        // Where the line crosses each boundary, along it: a curved wall it cuts twice, twice.
        let crossings: Vec<Vec<f64>> = lines
            .iter()
            .map(|line| {
                let mut xs: Vec<f64> = line
                    .windows(2)
                    .filter_map(|seg| {
                        let &[p, q] = seg else { return None };
                        let x = geom::line_intersection(v.start, v.end, p, q).filter(|x| geom::dist_to_segment(*x, p, q) < 1e-6)?;
                        Some((x - v.start).dot(u))
                    })
                    .collect();
                xs.sort_by(f64::total_cmp);
                xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
                xs
            })
            .collect();
        for (k, layer) in layers.iter().enumerate() {
            let (Some(a), Some(b)) = (crossings.get(k), crossings.get(k + 1)) else { continue };
            // ponytail: a boundary the line only grazes leaves the layer in poché.
            if a.len() != b.len() {
                continue;
            }
            for (&s0, &s1) in a.iter().zip(b) {
                let (s0, s1) = (s0.min(s1).max(first), s0.max(s1).min(last));
                for r in cuts.get(&wall).into_iter().flatten() {
                    let part = geom::clip(&geom::clip(r, |q| q.0 - s0), |q| s1 - q.0);
                    if part.len() >= 3 {
                        let room = (s1 - s0) * d.view.scale;
                        cut_layer(&mut d, m, layer, &part, P2(1.0, 0.0), room);
                    }
                }
            }
        }
        for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
            for seg in line.windows(2) {
                let &[p, q] = seg else { continue };
                let Some(x) = geom::line_intersection(v.start, v.end, p, q).filter(|x| geom::dist_to_segment(*x, p, q) < 1e-6) else {
                    continue;
                };
                let along = (x - v.start).dot(u);
                if !(first..=last).contains(&along) {
                    continue;
                }
                for s in solids.iter().filter(|s| s.owner() == wall && s.over(x)) {
                    let (z0, z1) = s.z_at(x);
                    d.part(Part::Pattern);
                    d.line(&[P2(along, z0), P2(along, z1)], 1.0, INK, false);
                    d.part(Part::Cut);
                }
            }
        }
    }
    // Each layer of a compound floor, roof or ceiling it cuts: its material's cut pattern, a thin
    // line between each two, parallel to its faces.
    for (&owner, rings) in &cuts {
        let layers = match m.elements.get(&owner) {
            Some(Element::Floor(f)) => m.type_layers("floor", &f.type_name),
            Some(Element::Roof(r)) => m.type_layers("roof", &r.type_name),
            Some(Element::Ceiling(c)) => m.type_layers("ceiling", &c.type_name),
            _ => continue,
        };
        let total: f64 = layers.iter().map(|l| l.thickness).sum();
        if total <= 0.0 {
            continue;
        }
        d.owner = Some(owner);
        for r in rings {
            let &[a0, b0, b1, a1] = r.as_slice() else { continue };
            // The line a fraction of the way down from its top to its bottom.
            let at = |f: f64| (a1 + (a0 - a1) * f, b1 + (b0 - b1) * f);
            let mut above = 0.0;
            for (k, layer) in layers.iter().enumerate() {
                let ((p, q), (p2, q2)) = (at(above / total), at((above + layer.thickness) / total));
                let room = (p.1 - p2.1).abs() * d.view.scale;
                cut_layer(&mut d, m, layer, &[p2, q2, q, p], (q - p).unit(), room);
                if k > 0 {
                    d.part(Part::Pattern);
                    d.line(&[p, q], 1.0, INK, false);
                    d.part(Part::Cut);
                }
                above += layer.thickness;
            }
        }
    }
    d.part(Part::Projection);
    let (lo, hi) = shown_heights(m, v, &look);
    if v.crop.is_some() {
        let (a, b) = (d.view.px(P2(first, hi)), d.view.px(P2(last, lo)));
        d.crop(a, b, |_| false);
    }
    // Each level over as much of its extent as the view sees, named past its end.
    for (level, from, to) in level_spans(m, v, &look) {
        let (from, to) = (from.max(first), to.min(last));
        let Some(l) = m.level(level).filter(|l| from < to && ((lo..=hi).contains(&l.elevation) || v.crop.is_none())) else { continue };
        d.owner = Some(level);
        let (a, b) = (d.view.px(P2(from, l.elevation)), d.view.px(P2(to, l.elevation)));
        d.push(Prim::Line { pts: vec![a, b], width: 1.0, color: INK, dash: Some([12.0, 4.0]), closed: false });
        for (text, dy) in [(l.name.clone(), -8.0), (m.units.length(l.elevation), 8.0)] {
            d.text(&text, (b.0 + 8.0 + font::width(&text, 10.0) / 2.0, b.1 + dy), 10.0, INK);
        }
    }
    // Grids running into what it sees: across its line, reaching past it (within its depth).
    let past = |q: P2| (q - v.start).dot(v.looking());
    for (&grid_id, e) in m.elements.iter().filter(|(id, _)| look.shows(**id)) {
        if let Element::Grid(g) = e
            && let Some(x) = geom::line_intersection(v.start, v.end, g.start, g.end)
            && past(g.start).max(past(g.end)) >= -TOL
            && v.depth.is_none_or(|far| past(g.start).min(past(g.end)) <= far)
            && (first..=last).contains(&(x - v.start).dot(u))
        {
            let along = (x - v.start).dot(u);
            d.owner = Some(grid_id);
            grid(&mut d, P2(along, lo - GRID_PAST), P2(along, hi + GRID_PAST), &g.name);
        }
    }
    for (&nid, e) in m.elements.iter().filter(|(id, _)| look.shows(**id)) {
        d.owner = Some(nid);
        match e {
            Element::Text(n) if n.view == id => note(&mut d, n),
            Element::Spot(sp) if sp.view == id => {
                let z = m.spot_value(sp).unwrap_or(sp.at.1);
                spot_mark(&mut d, view.px(P2(sp.at.0, z)), Some(z), m.units);
            }
            _ => {}
        }
    }
    d.owner = None;
    look.dress(&mut d);
    let at = (24.0 + font::width(&v.name, 16.0) / 2.0, view.height as f32 - 26.0);
    d.text(&v.name, at, 16.0, INK);
    d
}

/// Section or elevation `id` through `view`, as a PNG: its picture, and its drawing over it.
pub fn section_png(m: &Model, id: Id, view: View) -> Result<Vec<u8>, String> {
    let image = section_image(m, &Scene::new(m), id, view, &[]).ok_or_else(|| format!("{id} is not a section or an elevation"))?;
    let size = tiny_skia::IntSize::from_wh(image.width, image.height).ok_or("the image size is out of range")?;
    let mut pm = Pixmap::from_vec(image.rgba, size).ok_or("the image size is out of range")?;
    paint(&mut pm, &section(m, id, view));
    pm.encode_png().map_err(|e| e.to_string())
}

/// The plan of `level` as `view` shows it: what a canvas that pans and zooms draws.
pub fn draw(m: &Model, level: Id, view: View) -> Drawing {
    draw_plan(m, level, view, false)
}

/// The reflected ceiling plan of `level` fitted into `width` x `height` px.
pub fn ceiling_plan(m: &Model, level: Id, width: u32, height: u32) -> Drawing {
    draw_ceiling(m, level, fit(&plan_shows(m, level), width, height))
}

/// The reflected ceiling plan of `level` as `view` shows it: cut [`CEILING_CUT`] above the level
/// (over the doors) and looking up, at the ceilings up to the level above, as a floor plan lies
/// (not mirrored). Its grids and its sections' marks; no annotation of the floor plan's.
pub fn draw_ceiling(m: &Model, level: Id, view: View) -> Drawing {
    draw_plan(m, level, view, true)
}

/// A floor plan of `level`, or `up`, its reflected ceiling plan.
fn draw_plan(m: &Model, level: Id, view: View, up: bool) -> Drawing {
    let mut d = Drawing::new(view);
    let look = Look::of(m, level);
    let shown = |id: Id| look.shows(id);
    let mut range = m.plan_range(level);
    if up {
        range.cut = m.elevation(level) + CEILING_CUT;
        range.top = range.top.max(range.cut);
    }
    // Floors whose tops it sees, from the cut plane down to the view depth: their outlines and their
    // openings', the holes shafts cut in them, and a sloped one's slope arrow.
    for (&id, e) in m.elements.iter().filter(|_| !up).filter(|(id, _)| shown(**id)) {
        let Element::Floor(f) = e else { continue };
        let top = m.floor_top(f);
        let (lo, hi) = f.boundary.iter().map(|q| top.at(*q)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
        if hi < range.depth || lo > range.cut {
            continue;
        }
        d.owner = Some(id);
        for ring in std::iter::once(&f.boundary).chain(&f.openings) {
            d.line(ring, 1.0, FAINT, true);
        }
        holes(&mut d, m, &m.floor_slabs(f));
        // Its top layer's surface pattern, faint.
        if let Some(pattern) = m.type_layers("floor", &f.type_name).first().and_then(|l| m.materials.get(&l.material)).map(|x| x.surface) {
            for (face, rise) in m.floor_slabs(f) {
                for piece in m.less_shafts(&face, rise).0 {
                    surface(&mut d, &piece, pattern, FAINT);
                }
            }
        }
        if let Some([tail, head]) = f.arrow.filter(|_| f.slope != 0.0) {
            slope_arrow(&mut d, tail, head, f.slope);
        }
    }
    // Components it sees, in projection, as Revit draws furniture: whole while any of one is under
    // the cut plane and over the view depth.
    for (&id, e) in m.elements.iter().filter(|_| !up).filter(|(id, _)| shown(**id)) {
        let Element::Component(c) = e else { continue };
        let pieces = m.component_pieces(c);
        let (bottom, top) = pieces.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.bottom), hi.max(p.top)));
        if bottom < range.cut && top > range.depth {
            d.owner = Some(id);
            component(&mut d, &pieces);
        }
    }
    // Walls and columns below the cut plane, as seen from it; past the bottom, faint.
    for (id, o, top) in seen_below(m, &range, &look).into_iter().filter(|_| !up) {
        d.owner = Some(id);
        d.line(&o, 1.0, if top > range.bottom { INK } else { FAINT }, true);
    }
    d.part(Part::Cut);
    let outlines = cut_outlines(m, range.cut, &look);
    // Outlines first, then poché over them: each fill covers the inner half of the outlines, so
    // where walls meet at their joins only the edge of their union stays inked. Their union is
    // filled first, as one for each colour the view fills them in, so no seam shows where two
    // fills meet.
    for (id, o) in &outlines {
        d.owner = Some(*id);
        d.line(o, 4.0, INK, true);
    }
    d.owner = None;
    for (color, rings) in cut_unions(&look, outlines.iter().map(|(id, o)| (*id, o.clone()))) {
        d.fill(&rings, color);
    }
    // One fill per wall or column: in one even-odd fill, overlapping walls would cancel out.
    for (id, o) in outlines {
        d.owner = Some(id);
        d.fill(&[o], POCHE);
    }
    // A compound wall's layers: each its material's cut pattern where it's wide enough to hold it,
    // else lighter where it isn't the wall's structure; a thin line between each two.
    for (&id, e) in &m.elements {
        let Element::Wall(w) = e else { continue };
        let layers = m.type_layers("wall", &w.type_name);
        if layers.is_empty() || !m.spans(w, range.cut) || !shown(id) {
            continue;
        }
        d.owner = Some(id);
        let lines = walls::layer_lines(m, id);
        let along = (w.end - w.start).unit();
        for (k, layer) in layers.iter().enumerate() {
            let (Some(a), Some(b)) = (lines.get(k), lines.get(k + 1)) else { continue };
            let ring: Vec<P2> = a.iter().chain(b.iter().rev()).copied().collect();
            let room = layer.thickness * d.view.scale;
            cut_layer(&mut d, m, layer, &ring, along, room);
        }
        // The lines between the layers are detail, as the hatching is: a cut's weight is its outline's.
        d.part(Part::Pattern);
        for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
            d.line(line, 1.0, INK, false);
        }
        d.part(Part::Cut);
    }
    for (&id, e) in &m.elements {
        // The opening is the wall's: with neither the wall nor the opening shown, there is nothing to leave a gap in.
        if let Element::Door(o) | Element::Window(o) = e
            && let Some(w) = m.wall(o.host)
            && m.spans(w, range.cut)
            && (shown(id) || shown(o.host))
        {
            d.owner = Some(id);
            opening(&mut d, m, e, o, w, range.cut, shown(id));
        }
    }
    d.part(Part::Projection);
    // A ceiling plan isn't cropped: the crop region is the floor plan's.
    let crop = m.level(level).and_then(|l| l.crop).filter(|_| !up);
    // Cropped, annotation shows whole while its anchor is inside the crop, and not at all outside it.
    let inside = |p: P2| crop.is_none_or(|[lo, hi]| (lo.0..=hi.0).contains(&p.0) && (lo.1..=hi.1).contains(&p.1));
    for (&id, e) in m.elements.iter().filter(|(id, _)| shown(**id)) {
        d.owner = Some(id);
        match e {
            // Cropped, a grid stops at the crop's edge, with its bubbles there.
            Element::Grid(g) => {
                if let Some((a, b)) = crop.map_or(Some((g.start, g.end)), |[lo, hi]| clip(g.start, g.end, lo, hi)) {
                    grid(&mut d, a, b, &g.name);
                }
            }
            Element::Room(r) if r.level == level && inside(r.point) && !up => {
                let area = m.room_region(r).map_or_else(
                    || m.language.word("NOT ENCLOSED").to_string(),
                    |g| {
                        let (area, unit) = m.units.area(g.area());
                        format!("{area:.2} {unit}")
                    },
                );
                let (x, y) = d.view.px(r.point);
                d.text(&r.name, (x, y - 16.0), 13.0, INK);
                d.text(&r.number, (x, y), 10.0, INK);
                d.text(&area, (x, y + 14.0), 10.0, INK);
            }
            // A shaft the plane cuts: its outline, and a cross.
            Element::Shaft(sh) => {
                let (bottom, top) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
                if bottom <= range.cut && range.cut < top {
                    shaft(&mut d, &sh.boundary);
                }
            }
            // Over the cut plane, under the view range's top: its outline, and its tiles.
            Element::Ceiling(c) if up => {
                let z = m.elevation(c.level) + c.offset;
                if range.cut < z && z <= range.top {
                    ceiling(&mut d, m, c);
                }
            }
            _ if up => {}
            Element::Stair(st) => stair(&mut d, m, st, level),
            Element::Ramp(r) => ramp(&mut d, m, r, level),
            Element::Railing(rl) if m.level_of(e) == Some(level) => railing(&mut d, m, rl, range.cut),
            Element::Roof(r) if r.level == level => roof(&mut d, m, r),
            // Overhead, from below: its edges, round its courtyards too, dashed, while its eaves are under
            // the view range's top.
            Element::Roof(r) => {
                let eave = m
                    .roof_faces(r)
                    .into_iter()
                    .flatten()
                    .flat_map(|(loops, p)| loops.into_iter().flatten().map(move |q| p.at(q)))
                    .fold(f64::INFINITY, f64::min);
                if range.cut < eave && eave <= range.top {
                    for ring in std::iter::once(&m.roof_outline(r)).chain(&r.courtyards) {
                        let pts = ring.iter().map(|p| d.view.px(*p)).collect();
                        d.push(Prim::Line { pts, width: 1.0, color: INK, dash: Some([10.0, 5.0]), closed: true });
                    }
                }
            }
            _ => {}
        }
    }
    // Annotation last, over the model; sections and elevations show on every plan. A floor plan's
    // annotation is its own.
    for (&id, e) in m.elements.iter().filter(|(id, _)| shown(**id)) {
        d.owner = Some(id);
        match e {
            Element::Dimension(_) | Element::Tag(_) | Element::Text(_) | Element::Spot(_) if up => {}
            Element::Dimension(dm) if dm.level == level && inside(dm.at) => dimension(&mut d, m, dm),
            Element::Tag(t) if m.level_of(e) == Some(level) && shown(t.host) && m.anchor(t.host).is_some_and(|a| inside(a + t.offset)) => {
                tag(&mut d, m, t)
            }
            Element::Text(n) if n.view == level && inside(n.at) => note(&mut d, n),
            Element::Spot(sp) if sp.view == level && inside(sp.at) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
            // A section's line is cropped as a line is.
            Element::Section(v) => section_mark(&mut d, v),
            Element::Elevation(v) if inside(v.start + (v.end - v.start) * 0.5) => elevation_mark(&mut d, v),
            _ => {}
        }
    }
    // Grids stop at the crop's edge on their own, and annotation is whole or not there: the rest is cut.
    if let Some([lo, hi]) = crop {
        let (a, b) = (d.view.px(lo), d.view.px(hi));
        d.crop((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)), |o| {
            o.and_then(|id| m.elements.get(&id)).is_some_and(|e| {
                matches!(
                    e,
                    Element::Grid(_)
                        | Element::Room(_)
                        | Element::Dimension(_)
                        | Element::Tag(_)
                        | Element::Text(_)
                        | Element::Spot(_)
                        | Element::Elevation(_)
                )
            })
        });
    }
    d.owner = None;
    look.dress(&mut d);
    let mut title = m.level(level).map_or_else(String::new, |l| l.name.clone());
    if up {
        title = format!("{title} ({})", m.language.word("Ceiling Plan"));
    }
    let at = (24.0 + font::width(&title, 16.0) / 2.0, view.height as f32 - 26.0);
    d.text(&title, at, 16.0, INK);
    d
}

/// A component seen from above: each of its forms' outlines, but where a higher form covers it (a
/// table's top over its legs).
fn component(d: &mut Drawing, pieces: &[crate::family::Piece]) {
    for (i, p) in pieces.iter().enumerate() {
        for (a, b) in geom::ring_edges(&p.outline) {
            let len = a.dist(b);
            if len < 1e-9 {
                continue;
            }
            let u = (b - a) * (1.0 / len);
            let mut hidden: Vec<(f64, f64)> = pieces
                .iter()
                .enumerate()
                .filter(|(j, q)| *j != i && q.top > p.top + 1e-6)
                .flat_map(|(_, q)| view3d::crossings(&q.outline, a, u))
                .map(|(s0, s1)| (s0.max(0.0), s1.min(len)))
                .filter(|(s0, s1)| s1 > s0)
                .collect();
            hidden.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut at = 0.0;
            for (s0, s1) in hidden {
                if s0 > at {
                    d.line(&[a + u * at, a + u * s0], 1.0, INK, false);
                }
                at = f64::max(at, s1);
            }
            if at < len {
                d.line(&[a + u * at, b], 1.0, INK, false);
            }
        }
    }
}

/// The outlines of the holes shafts cut in a floor's or a ceiling's slabs.
fn holes(d: &mut Drawing, m: &Model, slabs: &[(Vec<[f64; 3]>, f64)]) {
    let cut: std::collections::BTreeSet<Id> = slabs.iter().flat_map(|(face, rise)| m.less_shafts(face, *rise).1).collect();
    for id in cut {
        if let Some(Element::Shaft(sh)) = m.elements.get(&id) {
            d.line(&sh.boundary, 1.0, FAINT, true);
        }
    }
}

/// A slope arrow from `tail` up to `head`, and its slope beside its middle.
fn slope_arrow(d: &mut Drawing, tail: P2, head: P2, slope: f64) {
    d.line(&[tail, head], 1.0, INK, false);
    let (a, b) = (d.view.px(tail), d.view.px(head));
    let (x, y) = arrow(d, a, b);
    let text = format!("{}°", figure(slope));
    // Beside it, to its left on screen.
    let clear = font::width(&text, 10.0) / 2.0 * y.abs() + 6.0 * x.abs() + 4.0;
    d.text(&text, ((a.0 + b.0) / 2.0 + y * clear, (a.1 + b.1) / 2.0 - x * clear), 10.0, INK);
}

/// A shaft cut in plan: its outline, and the diagonals of its box across it.
fn shaft(d: &mut Drawing, ring: &[P2]) {
    d.line(ring, 1.0, INK, true);
    let Some((lo, hi)) = bounds(ring) else { return };
    for (from, to) in [(lo, hi), (P2(lo.0, hi.1), P2(hi.0, lo.1))] {
        let u = (to - from).unit();
        for (s0, s1) in view3d::crossings(ring, from - u, u) {
            d.line(&[from - u + u * s0, from - u + u * s1], 1.0, INK, false);
        }
    }
}

/// How far apart a cut pattern's lines are, px: a drafting pattern, the same on screen and on
/// paper whatever the scale.
const GAP: f64 = 6.0;
/// How far apart a surface pattern's lines are, mm: a model pattern, its tiles and courses their
/// size in the building (a tile 200 mm square, a brick 250 by 100).
const MODULE: f64 = 100.0;
/// How wide a layer is to be, px, to show its pattern: narrower, it's filled instead.
const PATTERN_ROOM: f64 = 10.0;

/// A layer where it's cut, along `ring`: where it's `room` px wide or more, its material's cut
/// pattern over the paper, its courses along unit `u`; else, unless it's its element's structure
/// (which stays poché), lighter.
fn cut_layer(d: &mut Drawing, m: &Model, layer: &Layer, ring: &[P2], u: P2, room: f64) {
    let pattern = m.materials.get(&layer.material).map_or(Pattern::None, |x| x.cut);
    if pattern != Pattern::None && room >= PATTERN_ROOM {
        d.fill(&[ring.to_vec()], PAPER);
        let gap = GAP / d.view.scale;
        hatch(d, ring, pattern, u, INK, gap);
    } else if layer.function != Function::Structure {
        d.fill(&[ring.to_vec()], LIGHT_POCHE);
    }
}

/// A surface pattern over `ring`, where its lines are far enough apart to read.
fn surface(d: &mut Drawing, ring: &[P2], pattern: Pattern, color: Rgb) {
    if MODULE * d.view.scale >= GAP / 2.0 {
        hatch(d, ring, pattern, P2(1.0, 0.0), color, MODULE);
    }
}

/// `pattern` over `ring` (model mm), as far as the view shows it: its lines `gap` mm apart, thin,
/// its courses along unit `u`; an insulation's zigzag along the ring's length, side to side.
fn hatch(d: &mut Drawing, ring: &[P2], pattern: Pattern, u: P2, color: Rgb, gap: f64) {
    let part = std::mem::replace(&mut d.part, Part::Pattern);
    hatching(d, ring, pattern, u, color, gap);
    d.part = part;
}

fn hatching(d: &mut Drawing, ring: &[P2], pattern: Pattern, u: P2, color: Rgb, gap: f64) {
    let (w, h) = (d.view.width as f32, d.view.height as f32);
    let (lo, hi) = (d.view.mm((0.0, h)), d.view.mm((w, 0.0)));
    let ring = [(P2(1.0, 0.0), lo.0), (P2(-1.0, 0.0), -hi.0), (P2(0.0, 1.0), lo.1), (P2(0.0, -1.0), -hi.1)]
        .into_iter()
        .fold(ring.to_vec(), |r, (n, k)| geom::clip(&r, |q| q.dot(n) - k));
    if ring.len() < 3 || pattern == Pattern::None {
        return;
    }
    if pattern == Pattern::Solid {
        d.fill(&[ring], color);
        return;
    }
    let span = |dir: P2| ring.iter().map(|q| q.dot(dir)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
    let n = u.perp();
    let mut segment = |d: &mut Drawing, from: P2, dir: P2, s0: f64, s1: f64| {
        for (a, b) in view3d::crossings(&ring, from, dir) {
            let (a, b) = (a.max(s0), b.min(s1));
            if b > a {
                d.line(&[from + dir * a, from + dir * b], 0.5, color, false);
            }
        }
    };
    // Lines along `dir`, `step` apart across it, `shift` along it from line to line.
    let lines = |d: &mut Drawing, dir: P2, step: f64, seg: &mut dyn FnMut(&mut Drawing, P2, P2, f64, f64)| {
        let ((a0, _), (c0, c1)) = (span(dir), span(dir.perp()));
        if (c1 - c0) / step > 2000.0 {
            return;
        }
        let mut k = (c0 / step).ceil();
        while k * step <= c1 {
            seg(d, dir.perp() * (k * step) + dir * (a0 - 1.0), dir, f64::NEG_INFINITY, f64::INFINITY);
            k += 1.0;
        }
    };
    let turned = |deg: f64| {
        let (sin, cos) = deg.to_radians().sin_cos();
        P2(u.0 * cos - u.1 * sin, u.0 * sin + u.1 * cos)
    };
    match pattern {
        Pattern::None | Pattern::Solid => {}
        Pattern::Diagonal => lines(d, turned(45.0), gap, &mut segment),
        Pattern::Crosshatch => {
            lines(d, turned(45.0), gap * 1.4, &mut segment);
            lines(d, turned(-45.0), gap * 1.4, &mut segment);
        }
        Pattern::Horizontal => lines(d, u, gap, &mut segment),
        Pattern::Vertical => lines(d, n, gap, &mut segment),
        Pattern::Grid => {
            lines(d, u, gap * 2.0, &mut segment);
            lines(d, n, gap * 2.0, &mut segment);
        }
        // Courses along `u`, their joints staggered half a brick from course to course.
        Pattern::Brick => {
            lines(d, u, gap, &mut segment);
            let ((a0, a1), (c0, c1)) = (span(u), span(n));
            if (a1 - a0) / gap * (c1 - c0) / gap > 40_000.0 {
                return;
            }
            let mut k = (c0 / gap).floor();
            while k * gap <= c1 {
                let stagger = if (k as i64).rem_euclid(2) == 0 { 0.0 } else { gap * 1.25 };
                let mut j = ((a0 - stagger) / (gap * 2.5)).floor();
                while j * gap * 2.5 + stagger <= a1 {
                    let from = u * (j * gap * 2.5 + stagger) + n * (k * gap);
                    segment(d, from, n, 0.0, gap);
                    j += 1.0;
                }
                k += 1.0;
            }
        }
        // Stipple: a dot every so often, each nudged its own way.
        Pattern::Concrete => {
            let ((a0, a1), (c0, c1)) = (span(u), span(n));
            let step = gap * 1.3;
            if (a1 - a0) / step * (c1 - c0) / step > 40_000.0 {
                return;
            }
            let dot = 0.8 / d.view.scale;
            let mut k = (c0 / step).floor();
            while k * step <= c1 {
                let mut j = (a0 / step).floor();
                while j * step <= a1 {
                    let h = crate::model::mix((j as i64 as u64).wrapping_mul(0x9E37) ^ (k as i64 as u64));
                    let (x, y) = ((h & 0xffff) as f64 / 65535.0, ((h >> 16) & 0xffff) as f64 / 65535.0);
                    let at = u * ((j + x) * step) + n * ((k + y) * step);
                    if geom::contains(&ring, at) {
                        d.line(&[at, at + u * dot], 0.8, color, false);
                    }
                    j += 1.0;
                }
                k += 1.0;
            }
        }
        // A batt's zigzag along the ring's longer way, touching its sides.
        Pattern::Insulation => {
            let (along, across) = if span(u).1 - span(u).0 >= span(n).1 - span(n).0 { (u, n) } else { (n, u) };
            let ((a0, a1), (c0, c1)) = (span(along), span(across));
            let wave = (c1 - c0).max(gap);
            if (a1 - a0) / wave > 5000.0 {
                return;
            }
            let mut i = 0.0;
            while a0 + i * wave / 2.0 <= a1 {
                let side = |i: f64| if (i as i64).rem_euclid(2) == 0 { c0 } else { c1 };
                let (p, q) =
                    (along * (a0 + i * wave / 2.0) + across * side(i), along * (a0 + (i + 1.0) * wave / 2.0) + across * side(i + 1.0));
                let len = p.dist(q);
                if len > 0.0 {
                    segment(d, p, (q - p).unit(), 0.0, len);
                }
                i += 1.0;
            }
        }
    }
}

/// A ceiling in its reflected ceiling plan: its outline and its openings', the holes shafts cut in
/// it, and with a grid, its tiles: lines through its middle, a tile apart, stopping at the holes.
fn ceiling(d: &mut Drawing, m: &Model, c: &Ceiling) {
    for ring in std::iter::once(&c.boundary).chain(&c.openings) {
        d.line(ring, 1.0, INK, true);
    }
    let slabs = m.ceiling_slabs(c);
    holes(d, m, &slabs);
    // Its bottom layer's surface pattern.
    if let Some(pattern) = m.type_layers("ceiling", &c.type_name).last().and_then(|l| m.materials.get(&l.material)).map(|x| x.surface) {
        for (face, rise) in &slabs {
            for piece in m.less_shafts(face, *rise).0 {
                surface(d, &piece, pattern, INK);
            }
        }
    }
    let (Some(t), Some((lo, hi))) = (m.types.ceiling.get(&c.type_name), bounds(&c.boundary)) else { return };
    // ponytail: tiles square to the axes; a ceiling's own grid angle when rooms turn.
    if t.grid <= 0.0 || (hi.0 - lo.0).max(hi.1 - lo.1) / t.grid > 2000.0 {
        return;
    }
    let pieces: Vec<Vec<P2>> = slabs.iter().flat_map(|(face, rise)| m.less_shafts(face, *rise).0).collect();
    let mid = (lo + hi) * 0.5;
    for (u, across, (a, b)) in
        [(P2(0.0, 1.0), P2(1.0, 0.0), (lo.0 - mid.0, hi.0 - mid.0)), (P2(1.0, 0.0), P2(0.0, 1.0), (lo.1 - mid.1, hi.1 - mid.1))]
    {
        let n = ((a / t.grid).ceil() as i64, (b / t.grid).floor() as i64);
        for k in n.0..=n.1 {
            // A micron off the grid: a line along an edge between two pieces (where a hole cuts the
            // ceiling up) pairs with neither and would vanish.
            let from = mid + across * (k as f64 * t.grid + 1e-3) - u * (hi - lo).length();
            for piece in &pieces {
                for (s0, s1) in view3d::crossings(piece, from, u) {
                    d.line(&[from + u * s0, from + u * s1], 1.0, INK, false);
                }
            }
        }
    }
}

/// The corners of the box around `ring`.
fn bounds(ring: &[P2]) -> Option<(P2, P2)> {
    let first = *ring.first()?;
    Some(ring.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))))
}

/// The part of the segment from `p` to `q` inside the box from `lo` to `hi` (Liang-Barsky); its
/// ends as they were where they're inside.
pub(crate) fn clip(p: P2, q: P2, lo: P2, hi: P2) -> Option<(P2, P2)> {
    let d = q - p;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (pk, qk) in [(-d.0, p.0 - lo.0), (d.0, hi.0 - p.0), (-d.1, p.1 - lo.1), (d.1, hi.1 - p.1)] {
        if pk == 0.0 {
            if qk < 0.0 {
                return None;
            }
        } else if pk < 0.0 {
            t0 = t0.max(qk / pk);
        } else {
            t1 = t1.min(qk / pk);
        }
    }
    (t0 <= t1).then(|| (if t0 > 0.0 { p + d * t0 } else { p }, if t1 < 1.0 { p + d * t1 } else { q }))
}

/// Whether the segment from `p` to `q` passes through the box from `lo` to `hi`.
fn clips(p: (f32, f32), q: (f32, f32), lo: (f32, f32), hi: (f32, f32)) -> bool {
    let mm = |p: (f32, f32)| P2(f64::from(p.0), f64::from(p.1));
    clip(mm(p), mm(q), mm(lo), mm(hi)).is_some()
}

/// The part of a ring inside the box from `lo` to `hi`, side by side (Sutherland-Hodgman): exact
/// for the convex rings plans fill.
fn clip_ring(ring: &[(f32, f32)], lo: (f32, f32), hi: (f32, f32)) -> Vec<(f32, f32)> {
    let mut pts = ring.to_vec();
    // Each side of the box: which coordinate, where, and whether inside lies above it.
    for (x, bound, above) in [(true, lo.0, true), (true, hi.0, false), (false, lo.1, true), (false, hi.1, false)] {
        let at = |p: (f32, f32)| if x { p.0 } else { p.1 };
        let inside = |p: (f32, f32)| if above { at(p) >= bound } else { at(p) <= bound };
        let cross = |a: (f32, f32), b: (f32, f32)| {
            let t = (bound - at(a)) / (at(b) - at(a));
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let n = pts.len();
        let mut kept = Vec::with_capacity(n + 4);
        for i in 0..n {
            let (a, b) = (pts[(i + n - 1) % n], pts[i]);
            match (inside(a), inside(b)) {
                (true, true) => kept.push(b),
                (true, false) => kept.push(cross(a, b)),
                (false, true) => kept.extend([cross(a, b), b]),
                (false, false) => {}
            }
        }
        pts = kept;
    }
    pts
}

/// A view fitting `pts` into the image, leaving the margin free.
fn fit(pts: &[P2], width: u32, height: u32) -> View {
    let lo = pts.iter().fold(P2(f64::MAX, f64::MAX), |a, p| P2(a.0.min(p.0), a.1.min(p.1)));
    let hi = pts.iter().fold(P2(f64::MIN, f64::MIN), |a, p| P2(a.0.max(p.0), a.1.max(p.1)));
    let (lo, hi) = if pts.is_empty() { (P2(-5000.0, -5000.0), P2(5000.0, 5000.0)) } else { (lo, hi) };
    let room = |px: u32| (f64::from(px) - 2.0 * MARGIN).max(1.0);
    let scale = (room(width) / (hi.0 - lo.0).max(1000.0)).min(room(height) / (hi.1 - lo.1).max(1000.0));
    View { scale, cx: (lo.0 + hi.0) / 2.0, cy: (lo.1 + hi.1) / 2.0, width, height }
}

/// A door or window the cut plane at height `cut` passes through: a gap in the wall, jambs, and
/// a leaf with its swing, or glass.
fn opening(d: &mut Drawing, m: &Model, e: &Element, o: &Opening, w: &Wall, cut: f64, shown: bool) {
    let Some(t) = m.opening_type(e) else { return };
    let bottom = m.own_base(w) + o.sill;
    if !(bottom <= cut && cut < bottom + t.height) {
        return;
    }
    let (axis, (ext, int)) = (Axis::of(w), walls::faces(m, w));
    let (s0, s1) = (o.offset - t.width / 2.0, o.offset + t.width / 2.0);
    // Clear a little past the faces too, where the outlines' outer halves are; along an arc, an arc.
    let past = 3.0 / d.view.scale;
    let gap: Vec<P2> = axis.points(s0, s1, int - past).into_iter().chain(axis.points(s1, s0, ext + past)).collect();
    // The opening is the wall's: a hidden door or window leaves it, outlined, owned by no one.
    let owner = d.owner;
    if !shown {
        d.own(None);
    }
    d.fill(&[gap], PAPER);
    for s in [s0, s1] {
        d.line(&[axis.at(s, int), axis.at(s, ext)], 2.0, INK, false);
    }
    d.own(owner);
    if !shown {
        return;
    }
    // Of a family: the forms the cut plane passes through, outlined, and the lines of its symbols (a door's swing).
    if o.family.is_some() {
        for piece in m.opening_pieces(e).iter().filter(|p| p.bottom <= cut && cut < p.top) {
            d.line(&piece.outline, 1.5, INK, true);
        }
        d.part(Part::Projection);
        for stroke in m.opening_strokes(e) {
            d.line(&stroke, 1.0, INK, false);
        }
        d.part(Part::Cut);
        return;
    }
    if matches!(e, Element::Door(_)) {
        // Swinging to the wall's left (its exterior side), or flipped, its right; hung on the jamb
        // toward its start, or flipped, its end.
        let side = if o.flip_facing { int } else { ext };
        let (hinge, free) = if o.flip_hand { (s1, s0) } else { (s0, s1) };
        let pivot = axis.at(hinge, side);
        let out = axis.tangent(hinge).perp() * if o.flip_facing { -1.0 } else { 1.0 };
        let shut = axis.at(free, side);
        let open = pivot + out * pivot.dist(shut);
        d.line(&[pivot, open], 1.5, INK, false);
        // What the leaf sweeps is seen, not cut.
        d.part(Part::Projection);
        d.line(&arc(pivot, open, shut, 24), 1.0, INK, false);
        d.part(Part::Cut);
    } else {
        for across in [int, (ext + int) / 2.0, ext] {
            d.line(&axis.points(s0, s1, across), 1.0, INK, false);
        }
    }
}

/// A stair in the plan of `level`. From its own level: its risers up to the cut plane, a cut line
/// across the run there, the rest (and the landings over the cut plane) dashed, and its walk line
/// marked UP. From the level it climbs to: all of it, and the walk line marked DN.
fn stair(d: &mut Drawing, m: &Model, st: &Stair, level: Id) {
    let up = st.level == level;
    if !up && st.top_level != Some(level) {
        return;
    }
    let (_, each, _) = m.flight(st);
    let l = stairs::stair(m, st);
    let half = st.width / 2.0;
    // The riser the cut plane passes through, counted from the first; from above, none.
    let rise = m.plan_range(level).cut - m.elevation(st.level);
    let cut = if up && each > 0.0 { (rise / each).floor() } else { f64::INFINITY };
    // Risers below the run drawn.
    let mut below = 0.0;
    for run in &l.runs {
        let at = |along: f64, across: f64| run.start + run.u * along + run.u.perp() * across;
        for k in 0..run.risers {
            let k = f64::from(k);
            segment(d, at(k * st.tread, -half), at(k * st.tread, half), below + k > cut);
        }
        let line = ((cut - below + 0.5) * st.tread).clamp(0.0, run.length);
        for side in [-half, half] {
            if line > 0.0 {
                segment(d, at(0.0, side), at(line, side), false);
            }
            if line < run.length {
                segment(d, at(line, side), at(run.length, side), true);
            }
        }
        if cut >= below && line < run.length {
            d.line(&[at(line - 0.4 * st.tread, -half), at(line + 0.4 * st.tread, half)], 1.0, INK, false);
        }
        below += f64::from(run.risers);
    }
    for pad in &l.pads {
        let over = up && pad.z - m.elevation(st.level) > rise;
        for (a, b) in geom::ring_edges(&pad.ring) {
            segment(d, a, b, over);
        }
    }
    walk(d, m, &l.walk, up);
}

/// A ramp in the plan of `level`: from its own level, its runs' and landings' outlines, dashed
/// where they're over the cut plane, and its walk line marked UP; from the level it climbs to, all
/// of it, marked DN.
fn ramp(d: &mut Drawing, m: &Model, r: &Ramp, level: Id) {
    let up = r.level == level;
    if !up && r.top_level != Some(level) {
        return;
    }
    let l = stairs::ramp(m, r);
    let cut = if up { m.plan_range(level).cut } else { f64::INFINITY };
    let rings = l.runs.iter().map(|run| (run.ring(r.width), run.z.0)).chain(l.pads.iter().map(|p| (p.ring.clone(), p.z)));
    for (ring, z) in rings {
        for (a, b) in geom::ring_edges(&ring) {
            segment(d, a, b, z > cut);
        }
    }
    walk(d, m, &l.walk, up);
}

/// A railing in a plan cutting at height `cut`: its line, solid where what it runs along is under
/// the cut plane, dashed over it.
fn railing(d: &mut Drawing, m: &Model, rl: &Railing, cut: f64) {
    for w in stairs::railing(m, rl).windows(2) {
        let &[(a, za), (b, zb)] = w else { continue };
        let parts = if (za > cut) != (zb > cut) {
            let mid = a + (b - a) * ((cut - za) / (zb - za));
            vec![(a, mid, za > cut), (mid, b, zb > cut)]
        } else {
            vec![(a, b, za > cut)]
        };
        for (p, q, over) in parts {
            let pts = vec![d.view.px(p), d.view.px(q)];
            d.push(Prim::Line { pts, width: 1.5, color: INK, dash: over.then_some([6.0, 4.0]), closed: false });
        }
    }
}

/// A line from `a` to `b`, one pixel wide; dashed, `over`.
fn segment(d: &mut Drawing, a: P2, b: P2, over: bool) {
    let pts = vec![d.view.px(a), d.view.px(b)];
    d.push(Prim::Line { pts, width: 1.0, color: INK, dash: over.then_some([6.0, 4.0]), closed: false });
}

/// A stair's or a ramp's walk line, its arrow at the top, marked UP at its foot; from above, the
/// other way, marked DN.
fn walk(d: &mut Drawing, m: &Model, line: &[(P2, f64)], up: bool) {
    let mut pts: Vec<P2> = line.iter().map(|(p, _)| *p).collect();
    pts.dedup_by(|a, b| a.dist(*b) < 1e-6);
    if !up {
        pts.reverse();
    }
    let (Some(&first), Some(&second), [.., before, last]) = (pts.first(), pts.get(1), pts.as_slice()) else { return };
    let label = m.language.word(if up { "UP" } else { "DN" });
    d.line(&pts, 1.0, INK, false);
    arrow(d, d.view.px(*before), d.view.px(*last));
    let (a, b) = (d.view.px(first), d.view.px(second));
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    // Before the walk line's start, clear of it however long the word is.
    let back = font::width(label, 9.0) / 2.0 * x.abs() + 5.0 * y.abs() + 8.0;
    d.text(label, (a.0 - x * back, a.1 - y * back), 9.0, INK);
}

/// An arrowhead at pixel `b`, pointing from `a`; the way it points, on screen.
fn arrow(d: &mut Drawing, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let head = vec![b, (b.0 - x * 10.0 - y * 4.0, b.1 - y * 10.0 + x * 4.0), (b.0 - x * 10.0 + y * 4.0, b.1 - y * 10.0 - x * 4.0)];
    d.push(Prim::Fill { rings: vec![head], color: INK });
    (x, y)
}

/// A roof in the plan of its level: its outline, and where its slopes meet, its ridges, hips and
/// valleys.
// ponytail: openings aren't drawn; a dormer's roof covers its own. Draw them for skylights.
fn roof(d: &mut Drawing, m: &Model, r: &Roof) {
    let faces = m.roof_faces(r).map_or_else(|_| vec![m.roof_outline(r)], |f| f.into_iter().flat_map(|(loops, _)| loops).collect());
    for f in faces {
        d.line(&f, 1.0, INK, true);
    }
}

/// The short arc around `center` from `from` to `to`, as `n` segments.
fn arc(center: P2, from: P2, to: P2, n: u32) -> Vec<P2> {
    let (a, b) = (from - center, to - center);
    let start = a.1.atan2(a.0);
    let mut sweep = b.1.atan2(b.0) - start;
    if sweep > PI {
        sweep -= 2.0 * PI;
    } else if sweep < -PI {
        sweep += 2.0 * PI;
    }
    (0..=n)
        .map(|i| {
            let t = start + sweep * f64::from(i) / f64::from(n);
            center + P2(t.cos(), t.sin()) * a.length()
        })
        .collect()
}

/// A dimension, of its kind.
fn dimension(d: &mut Drawing, m: &Model, dim: &Dimension) {
    match dim.kind {
        DimKind::Aligned => aligned(d, m, dim),
        DimKind::Angular => angular(d, m, dim),
        DimKind::Radial => radial(d, m, dim),
    }
}

/// A number as a dimension shows it: to the hundredth, without trailing zeros.
fn figure(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0 + 0.0)
}

/// An angular dimension: an arc through `at` between its two lines, witness lines out along them
/// to it, ticks at its ends, and the angle outside its middle.
fn angular(d: &mut Drawing, m: &Model, dim: &Dimension) {
    let Some((o, from, sweep)) = m.angle_sector(dim) else { return };
    let r = dim.at.dist(o);
    if r <= 0.0 {
        return;
    }
    let px = 1.0 / d.view.scale;
    let ray = |t: f64| P2(t.cos(), t.sin());
    let arc: Vec<P2> = (0..=48).map(|i| o + ray(from + sweep * f64::from(i) / 48.0) * r).collect();
    d.line(&arc, 1.0, INK, false);
    let lines: Vec<(P2, P2)> = dim.refs.iter().filter_map(|x| m.reference_line(x).ok()).collect();
    for t in [from, from + sweep] {
        let u = ray(t);
        // How far out along this ray its line reaches: the witness line runs from there to the arc.
        let reach = lines
            .iter()
            .filter(|(a, b)| geom::parallel(*a, *b, o, o + u))
            .flat_map(|(a, b)| [(*a - o).dot(u), (*b - o).dot(u)])
            .fold(0.0, f64::max);
        if r > reach + 3.0 * px {
            d.line(&[o + u * (reach + 3.0 * px), o + u * (r + 4.0 * px)], 1.0, INK, false);
        }
        let (p, k) = (o + u * r, (u + u.perp()) * (3.0 * px));
        d.line(&[p - k, p + k], 2.0, INK, false);
    }
    // Outside the arc's middle, clear of it however the text sits across the ray.
    let (text, mid) = (format!("{}°", figure(sweep.to_degrees())), ray(from + sweep / 2.0));
    let clear = f64::from(font::width(&text, 10.0) / 2.0) * mid.0.abs() + 5.0 * mid.1.abs() + 4.0;
    d.text(&text, d.view.px(o + mid * (r + clear * px)), 10.0, INK);
}

/// A radial dimension: a line from a round element's centre (marked with a cross) toward `at`,
/// an arrow where it meets the edge, and the radius past its end.
fn radial(d: &mut Drawing, m: &Model, dim: &Dimension) {
    let Some(Ok((c, r))) = dim.refs.first().map(|x| m.reference_arc(x)) else { return };
    let u = if dim.at.dist(c) > 0.0 { (dim.at - c).unit() } else { P2(1.0, 0.0) };
    let (edge, end) = (c + u * r, if dim.at.dist(c) > r { dim.at } else { c + u * r });
    d.line(&[c, end], 1.0, INK, false);
    let (a, b) = (d.view.px(c), d.view.px(edge));
    // Inside a circle too small for it, the arrow comes in from outside.
    if r * d.view.scale > 14.0 || end == edge {
        arrow(d, a, b);
    } else {
        arrow(d, d.view.px(end), b);
    }
    for (dx, dy) in [(4.0, 0.0), (0.0, 4.0)] {
        d.push(Prim::Line { pts: vec![(a.0 - dx, a.1 - dy), (a.0 + dx, a.1 + dy)], width: 1.0, color: INK, dash: None, closed: false });
    }
    let text = format!("R {}", m.units.length(r));
    let (x, y) = d.view.px(end);
    let w = font::width(&text, 10.0) / 2.0 + 6.0;
    d.text(&text, (x + u.0 as f32 * w, y - u.1 as f32 * w), 10.0, INK);
}

/// A text note: its lines down from its top left corner, turned as it is, and its leader with an
/// arrow at its end.
pub(crate) fn note(d: &mut Drawing, n: &Note) {
    let h = (n.size * sheet::DENSITY) as f32;
    // Counter-clockwise in the view, clockwise on screen.
    let a = -(n.angle.to_radians() as f32);
    let (sin, cos) = a.sin_cos();
    let at = d.view.px(n.at);
    let turn = |(x, y): (f32, f32)| (at.0 + x * cos - y * sin, at.1 + x * sin + y * cos);
    for (i, line) in n.text.lines().enumerate() {
        let mid = turn((font::width(line, h) / 2.0, h / 2.0 + i as f32 * h * 1.6));
        d.text_turned(line, mid, h, a, INK);
    }
    if let Some(to) = n.leader {
        let (from, to) = (turn((-h * 0.4, h / 2.0)), d.view.px(to));
        d.push(Prim::Line { pts: vec![from, to], width: 1.0, color: INK, dash: None, closed: false });
        arrow(d, from, to);
    }
}

/// A spot elevation's mark at pixel `at`: a triangle standing on the point, and the height it
/// reads over a line beside it, in `units`.
fn spot_mark(d: &mut Drawing, (x, y): (f32, f32), z: Option<f64>, units: Units) {
    d.push(Prim::Fill { rings: vec![vec![(x, y), (x + 5.0, y - 8.0), (x - 5.0, y - 8.0)]], color: INK });
    let text = z.map_or_else(|| "?".into(), |z| height(units, z));
    let w = font::width(&text, 10.0);
    d.push(Prim::Line { pts: vec![(x - 5.0, y - 8.0), (x + 9.0 + w, y - 8.0)], width: 1.0, color: INK, dash: None, closed: false });
    d.text(&text, (x + 7.0 + w / 2.0, y - 16.0), 10.0, INK);
}

/// A height as a spot elevation reads it, signed: ±0, +3000, -600, +9'-10 1/8".
fn height(units: Units, z: f64) -> String {
    let shown = units.length(z);
    match shown.strip_prefix('-') {
        Some(_) => shown,
        // What rounds to nothing reads as nothing, either side.
        None if units.length(0.0) == shown => format!("±{shown}"),
        None => format!("+{shown}"),
    }
}

/// An aligned dimension: a witness line from each reference to the dimension line, a tick where
/// it crosses, and each segment's length over its middle, readable from below or from the right.
fn aligned(d: &mut Drawing, m: &Model, dim: &Dimension) {
    let (n, cross) = m.crossings(dim);
    let (Some(&(lo, _)), Some(&(hi, _))) = (cross.first(), cross.last()) else { return };
    // Along the dimension line, and along the references; `px` mm make a pixel.
    let u = n.perp();
    let px = 1.0 / d.view.scale;
    let on = |s: f64| dim.at + n * s;
    d.line(&[on(lo - 4.0 * px), on(hi + 4.0 * px)], 1.0, INK, false);
    for &(s, (a, b)) in &cross {
        let c = on(s);
        // The reference's extent along it, from the dimension line: the witness line runs from
        // just off its nearer end to just past the dimension line.
        let (ta, tb) = ((a - c).dot(u), (b - c).dot(u));
        let (from, to) = match (ta.min(tb), ta.max(tb)) {
            (near, _) if near > 0.0 => (near - 3.0 * px, -4.0 * px),
            (_, near) if near < 0.0 => (near + 3.0 * px, 4.0 * px),
            _ => (-4.0 * px, 4.0 * px),
        };
        d.line(&[c + u * from, c + u * to], 1.0, INK, false);
        d.line(&[c - (u + n) * (3.0 * px), c + (u + n) * (3.0 * px)], 2.0, INK, false);
    }
    // The line's angle on screen (y down): it runs east or north, so the text reads from below
    // or from the right.
    let angle = (-n.1).atan2(n.0);
    // The text's up, on screen.
    let up = (angle.sin() as f32, -angle.cos() as f32);
    for pair in cross.windows(2) {
        let [(s0, _), (s1, _)] = pair else { continue };
        let (x, y) = d.view.px(on((s0 + s1) / 2.0));
        d.text_turned(&m.units.length(s1 - s0), (x + up.0 * 9.0, y + up.1 * 9.0), 10.0, angle as f32, INK);
    }
}

/// A door's or window's tag: its mark in a box for a door, in a hexagon for a window, over a
/// paper fill; with a leader, a line to the opening's centre.
fn tag(d: &mut Drawing, m: &Model, t: &Tag) {
    let Some(anchor) = m.anchor(t.host) else { return };
    let head = anchor + t.offset;
    if t.leader {
        d.line(&[anchor, head], 1.0, INK, false);
    }
    let text = m.tag_text(t);
    let (x, y) = d.view.px(head);
    let (w, h) = ((font::width(&text, 10.0) / 2.0 + 5.0).max(8.0), 8.0);
    let ring = if matches!(m.elements.get(&t.host), Some(Element::Window(_))) {
        vec![(x - w - 4.0, y), (x - w, y - h), (x + w, y - h), (x + w + 4.0, y), (x + w, y + h), (x - w, y + h)]
    } else {
        vec![(x - w, y - h), (x + w, y - h), (x + w, y + h), (x - w, y + h)]
    };
    d.push(Prim::Fill { rings: vec![ring.clone()], color: PAPER });
    d.push(Prim::Line { pts: ring, width: 1.0, color: INK, dash: None, closed: true });
    d.text(&text, (x, y), 10.0, INK);
}

/// The way a section or elevation looks, on screen (y down).
fn looking_px(v: &Section) -> (f32, f32) {
    let l = v.looking();
    (l.0 as f32, -l.1 as f32)
}

/// A section's line in plan, dashed, with its head at its start (a flag pointing the way it
/// looks, its name behind it) and a tick at its end.
fn section_mark(d: &mut Drawing, v: &Section) {
    let (a, b) = (d.view.px(v.start), d.view.px(v.end));
    d.push(Prim::Line { pts: vec![a, b], width: 1.5, color: INK, dash: Some([18.0, 5.0]), closed: false });
    let (lx, ly) = looking_px(v);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    d.push(Prim::Fill { rings: vec![vec![a, (a.0 + ux * 12.0, a.1 + uy * 12.0), (a.0 + lx * 12.0, a.1 + ly * 12.0)]], color: INK });
    d.push(Prim::Line { pts: vec![b, (b.0 + lx * 9.0, b.1 + ly * 9.0)], width: 1.5, color: INK, dash: None, closed: false });
    let back = font::width(&v.name, 10.0) / 2.0 + 8.0;
    d.text(&v.name, (a.0 - ux * back, a.1 - uy * back), 10.0, INK);
}

/// An elevation's marker in plan, at its line's middle: a circle with a flag pointing the way it
/// looks, and its name behind it.
fn elevation_mark(d: &mut Drawing, v: &Section) {
    const R: f32 = 9.0;
    let c = d.view.px(v.start + (v.end - v.start) * 0.5);
    let ring: Vec<(f32, f32)> = (0..32)
        .map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + R * t.cos() as f32, c.1 + R * t.sin() as f32)
        })
        .collect();
    d.push(Prim::Fill { rings: vec![ring.clone()], color: PAPER });
    d.push(Prim::Line { pts: ring, width: 1.2, color: INK, dash: None, closed: true });
    let (lx, ly) = looking_px(v);
    let side = (-ly * R * 0.8, lx * R * 0.8);
    let flag = vec![(c.0 + side.0, c.1 + side.1), (c.0 + lx * (R + 7.0), c.1 + ly * (R + 7.0)), (c.0 - side.0, c.1 - side.1)];
    d.push(Prim::Fill { rings: vec![flag], color: INK });
    let back = R + 6.0 + font::width(&v.name, 10.0) / 2.0 * lx.abs() + 6.0 * ly.abs();
    d.text(&v.name, (c.0 - lx * back, c.1 - ly * back), 10.0, INK);
}

/// A grid line, dashed, with a bubble holding its name beyond each end.
fn grid(d: &mut Drawing, start: P2, end: P2, name: &str) {
    let (a, b) = (d.view.px(start), d.view.px(end));
    d.push(Prim::Line { pts: vec![a, b], width: 1.0, color: GRID, dash: Some([14.0, 4.0]), closed: false });
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let dir = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    for ((x, y), k) in [(a, -BUBBLE), (b, BUBBLE)] {
        let c = (x + dir.0 * k, y + dir.1 * k);
        let ring = (0..32).map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + BUBBLE * t.cos() as f32, c.1 + BUBBLE * t.sin() as f32)
        });
        d.push(Prim::Line { pts: ring.collect(), width: 1.2, color: GRID, dash: None, closed: true });
        d.text(name, c, 10.0, GRID);
    }
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// SVG path data for a polyline or ring.
fn path_data(pts: &[(f32, f32)], closed: bool) -> String {
    let mut s = String::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        let _ = write!(s, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { " L" });
    }
    if closed {
        s.push_str(" Z");
    }
    s
}

pub fn svg(d: &Drawing) -> String {
    let (w, h) = (d.view.width, d.view.height);
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="{}"/>"#,
        hex(PAPER)
    );
    for p in &d.prims {
        let _ = match p {
            Prim::Fill { rings, color } => {
                let data: Vec<String> = rings.iter().map(|r| path_data(r, true)).collect();
                write!(s, r#"<path d="{}" fill="{}"/>"#, data.join(" "), hex(*color))
            }
            Prim::Line { pts, width, color, dash, closed } => {
                let dash = dash.map_or_else(String::new, |[a, b]| format!(r#" stroke-dasharray="{a} {b}""#));
                write!(
                    s,
                    r#"<path d="{}" fill="none" stroke="{}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"{dash}/>"#,
                    path_data(pts, *closed),
                    hex(*color)
                )
            }
            Prim::Image { at, width, height, rgba } => match picture(*width, *height, rgba).and_then(|pm| pm.encode_png().ok()) {
                Some(png) => write!(
                    s,
                    r#"<image x="{}" y="{}" width="{width}" height="{height}" href="data:image/png;base64,{}"/>"#,
                    at.0,
                    at.1,
                    crate::commands::base64(&png)
                ),
                None => Ok(()),
            },
        };
    }
    s.push_str("</svg>\n");
    s
}

fn path(rings: &[&[(f32, f32)]], closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for ring in rings {
        for (i, &(x, y)) in ring.iter().enumerate() {
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        if closed {
            pb.close();
        }
    }
    pb.finish()
}

pub fn png(d: &Drawing) -> Result<Vec<u8>, String> {
    let mut pm = Pixmap::new(d.view.width, d.view.height).ok_or("the image size is out of range")?;
    pm.fill(Color::from_rgba8(PAPER.0, PAPER.1, PAPER.2, 255));
    paint(&mut pm, d);
    pm.encode_png().map_err(|e| e.to_string())
}

/// A picture's pixels as a pixmap.
fn picture(width: u32, height: u32, rgba: &[u8]) -> Option<Pixmap> {
    Pixmap::from_vec(rgba.to_vec(), IntSize::from_wh(width, height)?)
}

/// The display list drawn over what `pm` holds.
fn paint(pm: &mut Pixmap, d: &Drawing) {
    for p in &d.prims {
        let mut paint = Paint { anti_alias: true, ..Paint::default() };
        match p {
            Prim::Fill { rings, color } => {
                paint.set_color_rgba8(color.0, color.1, color.2, 255);
                let rings: Vec<&[(f32, f32)]> = rings.iter().map(Vec::as_slice).collect();
                if let Some(path) = path(&rings, true) {
                    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
            Prim::Line { pts, width, color, dash, closed } => {
                paint.set_color_rgba8(color.0, color.1, color.2, 255);
                let stroke = Stroke {
                    width: *width,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    dash: dash.and_then(|[a, b]| StrokeDash::new(vec![a, b], 0.0)),
                    ..Stroke::default()
                };
                if let Some(path) = path(&[pts], *closed) {
                    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Prim::Image { at, width, height, rgba } => {
                if let Some(img) = picture(*width, *height, rgba) {
                    pm.draw_pixmap(
                        at.0.round() as i32,
                        at.1.round() as i32,
                        img.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    fn house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [2000, 0]}
               window {"at": [8000, 3000]}
               room {"point": [4000, 3000], "name": "Living"}
               grid {"start": [0, -2000], "end": [0, 8000]}"#,
        )
        .unwrap();
        s
    }

    fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
        let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
        let c = pm.pixel(x as u32, y as u32).unwrap();
        (c.red(), c.green(), c.blue())
    }

    #[test]
    fn plans_fill_cut_walls_with_poche_and_open_them_at_doors() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(6000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "solid wall");
        assert_eq!(pixel(&png, d.view.px(P2(2000.0, 0.0))), (255, 255, 255), "the door's gap");
        assert_eq!(pixel(&png, d.view.px(P2(1000.0, 5000.0))), (255, 255, 255), "the room, away from its tag");
    }

    #[test]
    fn wall_corners_close_solid_without_a_notch() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        // Where the top and right walls overlap, and the square their ends stretch over.
        assert_eq!(pixel(&png, d.view.px(P2(8000.0, 6000.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert_eq!(pixel(&png, d.view.px(P2(8050.0, 6050.0))), (POCHE.0, POCHE.1, POCHE.2));
    }

    #[test]
    fn walls_meet_at_any_angle_without_a_seam_and_show_their_layers_and_arcs() {
        let mut s = Session::new();
        s.script(
            r#"type {"category": "wall", "name": "Cavity", "layers": [{"function": "finish", "thickness": 100}, {"function": "structure", "thickness": 200}]}
               wall {"points": [[0, 0], [0, 4000], [4000, 6309.401]], "type": "Cavity"}
               wall {"points": [[6000, 0], [10000, 0]], "arc": 90}"#,
        )
        .unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        let at = |x: f64, y: f64| pixel(&png, d.view.px(P2(x, y)));
        // Where the two walls meet at 120°, solid: no seam down their mitre.
        assert_eq!(at(0.0, 4000.0), (POCHE.0, POCHE.1, POCHE.2));
        // Going north, its exterior is west: the finish there lighter than the structure, a line between.
        assert_eq!((at(-100.0, 2000.0), at(50.0, 2000.0)), ((LIGHT_POCHE.0, LIGHT_POCHE.1, LIGHT_POCHE.2), (POCHE.0, POCHE.1, POCHE.2)));
        assert!(at(-50.0, 2000.0).0 < 100);
        // A quarter circle turning left from west to east bulges south: its crown solid, its chord's middle clear.
        assert_eq!((at(8000.0, 2000.0 - 2828.427), at(8000.0, 0.0)), ((POCHE.0, POCHE.1, POCHE.2), (PAPER.0, PAPER.1, PAPER.2)));
    }

    #[test]
    fn a_plan_of_another_level_leaves_these_walls_out() {
        let s = house();
        let d = plan(&s.model, 2, 800, 600);
        assert!(!d.prims.is_empty(), "the grid and the view title still show");
        assert!(!d.prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)));
    }

    #[test]
    fn a_plan_drawn_at_a_given_view_maps_the_model_through_it() {
        let s = house();
        let fitted = plan(&s.model, 1, 800, 600);
        assert_eq!(draw(&s.model, 1, fitted.view).prims.len(), fitted.prims.len(), "the same display list");
        // Zoomed in twice on the house's top right corner: the corner lands mid-image.
        let view = View { scale: fitted.view.scale * 2.0, cx: 8000.0, cy: 6000.0, ..fitted.view };
        let png = png(&draw(&s.model, 1, view)).unwrap();
        assert_eq!(pixel(&png, (400.0, 300.0)), (POCHE.0, POCHE.1, POCHE.2));
        let back = view.mm(view.px(P2(1234.0, -567.0)));
        assert!(back.dist(P2(1234.0, -567.0)) < 0.01, "{back:?}");
    }

    #[test]
    fn every_primitive_names_the_element_it_draws() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        assert_eq!(d.owners.len(), d.prims.len());
        // Walls 3 to 6, then the door, the window, the room and the grid.
        for id in 3..=10 {
            assert!(d.owners.contains(&Some(id)), "element {id} draws nothing");
        }
        assert_eq!(d.owners.last(), Some(&None), "the view title belongs to no element");
    }

    #[test]
    fn pick_finds_the_element_drawn_nearest_a_point() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick(at(2000.0, 0.0), 4.0), Some(7), "the door, drawn over its wall");
        assert_eq!(d.pick(at(6000.0, 0.0), 4.0), Some(3), "the wall");
        assert_eq!(d.pick(at(0.0, 3000.0), 4.0), Some(6), "the wall, under a grid line on its centre line");
        let (x, y) = at(6000.0, -100.0);
        assert_eq!(d.pick((x, y + 3.0), 4.0), Some(3), "just outside the wall's face");
        assert_eq!(d.pick(at(8000.0, 3000.0), 4.0), Some(8), "the window");
        assert_eq!(d.pick(at(4000.0, 3000.0), 10.0), Some(9), "the room, by its tag");
        assert_eq!(d.pick(at(0.0, -1500.0), 4.0), Some(10), "the grid line");
        assert_eq!(d.pick(at(4000.0, 1200.0), 4.0), None, "the floor of the room, away from its tag");
    }

    #[test]
    fn pick_all_lists_what_is_near_and_boxes_select_inside_or_crossing() {
        let s = house();
        let d = plan(&s.model, 1, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick_all(at(2000.0, 0.0), 4.0), [7, 3], "the door, then its wall: Tab's order");
        // A box around the window, across the wall it's in.
        let (a, b) = (at(7000.0, 2000.0), at(9000.0, 4000.0));
        assert_eq!(d.in_box(a, b, false), [8], "a window selects what is all inside");
        assert_eq!(d.in_box(b, a, true), [4, 8], "a crossing box also what it touches");
    }

    #[test]
    fn dimensions_and_tags_draw_over_the_plan() {
        let mut s = house();
        // The outer faces of the left and right walls, measured below the house; then every door and window tagged.
        s.script("dimension {\"refs\": [[-100, 3000], [8100, 3000]], \"at\": [4000, -1500]}\ntag {}").unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 200 && g < 200 && b < 200
        };
        assert!(dark(d.view.px(P2(1000.0, -1500.0))), "the dimension line");
        assert!(dark(d.view.px(P2(-100.0, -1000.0))), "a witness line, down from the left wall's outer face");
        assert_eq!(d.pick(d.view.px(P2(1000.0, -1500.0)), 4.0), Some(11));
        // The door's tag, a box outside the wall (the door swings in). The window faces its wall's
        // exterior side, its left: this house is drawn counter-clockwise, so that's in, and so is its tag.
        let (x, y) = d.view.px(P2(2000.0, -500.0));
        assert!(dark((x, y - 8.0)), "the top of the door tag's box");
        assert_eq!(d.pick((x, y), 2.0), Some(12));
        assert_eq!(d.pick(d.view.px(P2(7500.0, 3000.0)), 2.0), Some(13));
    }

    /// The house with a section (11) across it, west to east at y = 3000 so it looks north, and
    /// the South elevation (12).
    fn cut() -> Session {
        let mut s = house();
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nelevation {\"facing\": \"south\"}").unwrap();
        s
    }

    #[test]
    fn sections_cut_walls_in_poche_and_show_what_is_beyond() {
        let s = cut();
        let view = section_fit(&s.model, 11, 800, 600);
        let png = section_png(&s.model, 11, view).unwrap();
        // Along the section from its start at x = -1000; up from Level 1.
        let at = |x: f64, z: f64| view.px(P2(x + 1000.0, z));
        assert_eq!(pixel(&png, at(70.0, 1500.0)), (POCHE.0, POCHE.1, POCHE.2), "the west wall, cut (beside grid 1, on its centre line)");
        assert_eq!(pixel(&png, at(4000.0, 1500.0)), (255, 255, 255), "the north wall beyond, in hidden line");
        let d = section(&s.model, 11, view);
        assert_eq!(d.pick(at(0.0, 1500.0), 2.0), Some(6), "the cut is the wall's to pick");
        assert_eq!(d.pick(at(4000.0, 3000.0), 3.0), Some(2), "Level 2's line");
        assert_eq!(d.pick(at(0.0, 3700.0), 3.0), Some(10), "grid 1, where it crosses the section");
    }

    #[test]
    fn elevations_show_the_walls_they_look_at_and_cut_nothing() {
        let s = cut();
        let Some(Element::Elevation(e)) = s.model.elements.get(&12) else { panic!("no elevation") };
        let start = e.start;
        let view = section_fit(&s.model, 12, 800, 600);
        let png = section_png(&s.model, 12, view).unwrap();
        // The south wall's face, between the door and the corner, from the south.
        let p = view.px(P2(6000.0 - start.0, 1500.0));
        assert_eq!(pixel(&png, p), (255, 255, 255));
        let d = section(&s.model, 12, view);
        assert!(d.prims.iter().all(|p| !matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "no cut");
        assert_eq!(d.pick(view.px(P2(0.0 - start.0, 3700.0)), 3.0), Some(10), "grid 1 runs into what it sees");
        assert_eq!(
            view3d::Scene::new(&s.model)
                .cut(&section_camera(&s.model, 12, view).unwrap().0, &section_camera(&s.model, 12, view).unwrap().1, &[])
                .pick(f64::from(p.0), f64::from(p.1)),
            Some(3)
        );
    }

    #[test]
    fn plans_mark_sections_and_elevations() {
        let s = cut();
        let d = plan(&s.model, 1, 800, 600);
        assert_eq!(d.pick(d.view.px(P2(-500.0, 3000.0)), 3.0), Some(11), "the section's line");
        let Some(Element::Elevation(e)) = s.model.elements.get(&12) else { panic!("no elevation") };
        let mid = e.start + (e.end - e.start) * 0.5;
        assert_eq!(d.pick(d.view.px(mid), 3.0), Some(12), "the elevation's marker");
    }

    #[test]
    fn plans_cut_columns_draw_stairs_and_show_roofs_from_both_sides() {
        let mut s = house();
        // Walls 3 to 6, door 7, window 8, room 9, grid 10; then a column 11, a stair 12 and a roof 13.
        s.script("column {\"at\": [6000, 4000]}\nstair {\"start\": [1000, 1500], \"end\": [1000, 2500]}\nroof {}").unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(6000.0, 4000.0))), (POCHE.0, POCHE.1, POCHE.2), "the column, cut");
        // The stair: its first treads, and its arrow up the middle.
        assert_eq!(d.pick(d.view.px(P2(1400.0, 1500.0 + 280.0)), 2.0), Some(12), "a tread");
        // The roof overhead, dashed, from below; seen from its own level, its hips run to the corners.
        assert_eq!(d.pick(d.view.px(P2(4000.0, -600.0)), 2.0), Some(13), "the roof's edge, overhead");
        let up = plan(&s.model, 2, 800, 600);
        let corner = up.view.px(P2(-600.0, -600.0));
        let hip = up.view.px(P2(-600.0 + 600.0, -600.0 + 600.0));
        assert_eq!(up.pick(((corner.0 + hip.0) / 2.0, (corner.1 + hip.1) / 2.0), 2.0), Some(13), "a hip");
    }

    #[test]
    fn sections_cut_roofs_along_their_slope() {
        let mut s = house();
        s.script("roof {}\nsection {\"start\": [4000, -2000], \"end\": [4000, 8000]}").unwrap();
        // The roof 11, the section 12 (running north, looking west): its cut of the roof at the ridge.
        let view = section_fit(&s.model, 12, 800, 600);
        let d = section(&s.model, 12, view);
        let ridge = 3000.0 + 3100.0 * 30f64.to_radians().tan();
        let at = view.px(P2(2000.0 + 3000.0, ridge + 100.0));
        assert_eq!(d.pick(at, 2.0), Some(11));
        let png = section_png(&s.model, 12, view).unwrap();
        assert_eq!(pixel(&png, at), (POCHE.0, POCHE.1, POCHE.2));
    }

    #[test]
    fn plans_draw_what_their_view_range_sees() {
        let mut s = house();
        // A low wall across the room (11), 900 mm tall; a floor (12); the roof over it all (13).
        s.script("wall {\"points\": [[1000, 4500], [5000, 4500]], \"height\": 900}\nfloor {}\nroof {}").unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        // Cut at 1200: the window (900 to 2400 up) is open, and the low wall below the cut is seen from
        // above, its outline thin and no poché.
        assert_eq!(pixel(&png, d.view.px(P2(8050.0, 3000.0))), (255, 255, 255), "the window's opening");
        assert_eq!(d.pick(d.view.px(P2(3000.0, 4600.0)), 2.0), Some(11));
        assert_eq!(pixel(&png, d.view.px(P2(3000.0, 4500.0))), (255, 255, 255));
        assert!(d.owners.contains(&Some(13)), "the roof overhead, under the level above");
        // Cut above the window's head, the wall runs solid past it; with the top under the roof's
        // eaves, the roof isn't seen.
        s.execute("modify", &serde_json::json!({"id": 1, "view_range": {"cut": 2500, "top": 2600}})).unwrap();
        let d = plan(&s.model, 1, 800, 600);
        assert_eq!(pixel(&png_of(&d), d.view.px(P2(8050.0, 3000.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert!(!d.owners.contains(&Some(13)));
        // From Level 2, the floor below shows once the view depth reaches down to it.
        assert!(!plan(&s.model, 2, 800, 600).owners.contains(&Some(12)));
        s.execute("modify", &serde_json::json!({"id": 2, "view_range": {"depth": -3000}})).unwrap();
        assert!(plan(&s.model, 2, 800, 600).owners.contains(&Some(12)));
    }

    fn png_of(d: &Drawing) -> Vec<u8> {
        png(d).unwrap()
    }

    #[test]
    fn crop_regions_clip_plans_and_sections() {
        let mut s = house();
        s.execute("crop", &serde_json::json!({"view": 1, "region": [[-1000, -1000], [4000, 4000]]})).unwrap();
        let d = plan(&s.model, 1, 800, 600);
        assert!((d.view.cx - 1500.0).abs() < 1.0 && (d.view.cy - 1500.0).abs() < 1.0, "fitted to the crop: {:?}", d.view);
        assert!(!d.owners.contains(&Some(4)), "the east wall lies outside");
        assert_eq!(d.pick(d.view.px(P2(3000.0, 0.0)), 2.0), Some(3), "the south wall, up to the crop's edge");
        assert_eq!(d.pick(d.view.px(P2(5000.0, 0.0)), 2.0), None);
        // Annotation is whole or not there: the room's tag, its point on the crop's edge, keeps its name whole.
        assert!(d.owners.contains(&Some(9)));
        let name = d.view.px(P2(4000.0 + 300.0, 3000.0));
        assert!(d.pick((name.0, name.1 - 16.0), 3.0) == Some(9), "the name past the crop's edge");
        // Grid 1 stops at the crop's edge, its bubble there.
        let (x, y) = d.view.px(P2(0.0, 4000.0));
        assert_eq!(d.pick((x + 11.0, y - 11.0), 2.0), Some(10));
        // A section cropped to the ground floor's lower part: Level 2 isn't in it, and the cut wall stops.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
        s.execute("crop", &serde_json::json!({"view": 11, "region": [[0, -100], [5000, 2000]]})).unwrap();
        let view = section_fit(&s.model, 11, 800, 600);
        let d = section(&s.model, 11, view);
        assert!(!d.owners.contains(&Some(2)) && d.owners.contains(&Some(1)), "only the levels inside");
        let png = section_png(&s.model, 11, view).unwrap();
        // (Beside grid 1, on the wall's centre line.)
        assert_eq!(pixel(&png, view.px(P2(950.0, 1500.0))), (POCHE.0, POCHE.1, POCHE.2), "the west wall, cut");
        assert_eq!(pixel(&png, view.px(P2(950.0, 2500.0))), (255, 255, 255), "above the crop");
    }

    #[test]
    fn notes_spot_elevations_and_dimensions_draw_in_their_views() {
        let mut s = house();
        // A note (11), a floor (12) and a spot elevation on it (13); a slanted wall (14) and an angular
        // dimension (15) between it and the south wall; a round column (16) and a radial dimension (17).
        s.script(
            r#"text {"text": "NOTE", "at": [1000, 5000]}
               floor {}
               spot {"at": [2000, 2000]}
               wall {"points": [[0, 0], [3000, 3000]], "type": "Interior - 100mm"}
               dimension {"kind": "angular", "refs": [[4000, 0], [1500, 1500]], "at": [2500, 1000]}
               column {"at": [6000, 4000], "type": "Round 300"}
               dimension {"kind": "radial", "refs": [[6150, 4000]], "at": [6800, 4800]}"#,
        )
        .unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 200 && g < 200 && b < 200
        };
        // The note: its first letter's upright, just right of and below its top left corner.
        let (x, y) = d.view.px(P2(1000.0, 5000.0));
        assert_eq!(d.pick((x + 1.0, y + 6.0), 2.0), Some(11));
        // The spot elevation's mark: a triangle standing on the point.
        let (x, y) = d.view.px(P2(2000.0, 2000.0));
        assert!(dark((x, y - 4.0)), "its triangle");
        assert_eq!(d.pick((x, y - 3.0), 2.0), Some(13));
        // The angular dimension's arc, 1000 mm out from the corner at 45° / 2 round from east.
        let r = P2(2500.0, 1000.0).length();
        let mid = P2(22.5f64.to_radians().cos(), 22.5f64.to_radians().sin()) * r;
        assert_eq!(d.pick(d.view.px(mid), 2.0), Some(15));
        // The radial one's line, from the column's centre toward `at`.
        let along = P2(6000.0, 4000.0) + (P2(6800.0, 4800.0) - P2(6000.0, 4000.0)) * 0.5;
        assert_eq!(d.pick(d.view.px(along), 2.0), Some(17));
        // In a section: a spot elevation at its point, and a note.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nspot {\"at\": [3000, 2000], \"view\": 18}\ntext {\"text\": \"N\", \"at\": [5000, 2500], \"view\": 18}").unwrap();
        let view = section_fit(&s.model, 18, 800, 600);
        let d = section(&s.model, 18, view);
        let (x, y) = view.px(P2(3000.0, 2000.0));
        assert_eq!(d.pick((x, y - 3.0), 2.0), Some(19));
        assert!(d.owners.contains(&Some(20)));
    }

    #[test]
    fn spot_elevations_read_signed_heights_in_the_project_units() {
        assert_eq!(
            [height(Units::Millimetres, 0.3), height(Units::Millimetres, 3000.0), height(Units::Millimetres, -600.0)],
            ["±0", "+3000", "-600"]
        );
        assert_eq!([height(Units::Metres, 0.0), height(Units::FeetInches, 3000.0)], ["±0.00", "+9'-10 1/8\""]);
    }

    #[test]
    fn plans_show_slope_arrows_and_shafts_and_the_holes_they_cut() {
        let mut s = house();
        // A ramp (11) rising east; an upper floor (12); a shaft (13) from Level 1 up through it.
        s.script(
            r#"floor {"boundary": [[1000, 500], [7000, 500], [7000, 2000], [1000, 2000]], "slope": 5, "arrow": [[1000, 1250], [7000, 1250]]}
               floor {"level": 2, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}
               shaft {"boundary": [[3000, 3000], [4000, 3000], [4000, 5000], [3000, 5000]]}"#,
        )
        .unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let ramp: Vec<&Prim> = d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(11)).map(|(p, _)| p).collect();
        assert!(ramp.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == INK)), "its slope arrow's head");
        // Cut by Level 1's plan, the shaft shows its outline and a cross; from Level 2, the hole in its floor.
        assert_eq!(d.pick(d.view.px(P2(3500.0, 4000.0)), 2.0), Some(13), "where its cross crosses");
        let up = plan(&s.model, 2, 800, 600);
        assert!(!up.owners.contains(&Some(13)));
        assert_eq!(up.pick(up.view.px(P2(3500.0, 3000.0)), 2.0), Some(12), "the hole's edge");
    }

    #[test]
    fn ceiling_plans_look_up_at_ceilings_over_walls_cut_above_the_doors() {
        let mut s = house();
        // A tiled ceiling (11) in the room, its grid through its middle: 600 mm squares from (4000, 3000).
        s.script(r#"ceiling {"point": [4000, 1500], "type": "Tile 600 x 600"}"#).unwrap();
        let d = ceiling_plan(&s.model, 1, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(2000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "cut at 2300 mm, over the door");
        assert_eq!(pixel(&png, d.view.px(P2(8050.0, 3000.0))), (255, 255, 255), "the window's opening, cut");
        let (r, g, b) = pixel(&png, d.view.px(P2(4000.0, 1500.0)));
        assert!(r < 200 && g < 200 && b < 200, "a grid line: {:?}", (r, g, b));
        assert_eq!(d.pick(d.view.px(P2(4300.0, 1500.0)), 2.0), None, "the middle of a tile");
        assert!(d.owners.contains(&Some(11)) && !d.owners.contains(&Some(9)), "the ceiling; no room tag");
        assert!(!plan(&s.model, 1, 800, 600).owners.contains(&Some(11)), "a floor plan looks down");
        // A section cuts it.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
        let view = section_fit(&s.model, 12, 800, 600);
        assert_eq!(section(&s.model, 12, view).pick(view.px(P2(5000.0, 2610.0)), 2.0), Some(11));
    }

    #[test]
    fn plans_draw_stairs_that_turn_ramps_and_railings() {
        let mut s = house();
        // A stair (11) turning right at a landing and its railings (12, 13); a ramp (14) turning back
        // at a landing; a railing (15) along a path.
        s.script(
            r#"stair {"start": [1000, 1000], "angle": 90, "landings": [{"at": 8, "turn": -90}]}
               railing {"host": 11}
               ramp {"start": [-6000, -3000], "angle": 0, "height": 600, "landings": [{"at": 300, "turn": 180}]}
               railing {"path": [[0, 7000], [4000, 7000]]}"#,
        )
        .unwrap();
        let d = plan(&s.model, 1, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick(at(2340.0, 3200.0), 2.0), Some(11), "a riser of the run past the landing");
        assert_eq!(d.pick(at(550.0, 2000.0), 1.0), Some(12), "the left railing, in from the stair's edge");
        assert_eq!(d.pick(at(-4200.0, -3600.0), 2.0), Some(14), "the ramp's edge");
        assert_eq!(d.pick(at(2000.0, 7000.0), 2.0), Some(15));
        let drawn = |d: &Drawing, id, f: &dyn Fn(&Prim) -> bool| d.prims.iter().zip(&d.owners).any(|(p, o)| *o == Some(id) && f(p));
        let arrow = |p: &Prim| matches!(p, Prim::Fill { color, .. } if *color == INK);
        assert!(drawn(&d, 11, &arrow) && drawn(&d, 14, &arrow), "walk lines ending in arrows");
        assert!(drawn(&d, 11, &|p| matches!(p, Prim::Line { dash: Some(_), .. })), "above the cut plane, dashed");
        let up = plan(&s.model, 2, 800, 600);
        assert!(up.owners.contains(&Some(11)) && !up.owners.contains(&Some(14)), "from above: the stair, not the ramp");
    }

    #[test]
    fn svg_and_png_are_well_formed() {
        let s = house();
        let d = plan(&s.model, 1, 640, 480);
        let svg = svg(&d);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"), "{svg}");
        assert_eq!(svg.matches("<path").count(), d.prims.len());
        let pm = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
        assert_eq!((pm.width(), pm.height()), (640, 480));
    }
}
