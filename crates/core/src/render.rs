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
use crate::graphics::{Detail, Look, scale_of};
use crate::model::{
    CEILING_CUT, Ceiling, DimKind, Dimension, Element, Floor, Function, Id, Key, Layer, Model, Note, Opening, Pattern, PlanRange, Railing,
    Ramp, Roof, Section, Shaft, Stair, TOL, Tag, Wall,
};
use crate::pen::{Pen, Weights};
use crate::styles::{BY_CATEGORY, ObjectStyle};
use crate::units::Units;
use crate::view3d::{self, Camera, Clip, Image, Scene};
use crate::walls::{self, Axis};
use crate::{font, massing, sheet, stairs};

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
/// A mass's outline, dashed.
const MASS: Rgb = Rgb(88, 112, 168);
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

impl Part {
    /// Whether this part's lines turn their corners mitred, square as a cut wall's corner: all but lettering, whose strokes
    /// turn round, so a V's point doesn't stick out.
    pub(crate) fn mitred(self) -> bool {
        self != Part::Text
    }
}

impl Drawing {
    /// Whether primitive `i`'s corners are mitred ([`Part::mitred`]).
    pub(crate) fn mitred(&self, i: usize) -> bool {
        self.parts.get(i).is_none_or(|p| p.mitred())
    }
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
    /// A picture, its top left corner `at`, a pixel to a pixel: a section's on a sheet. `ink` when all of it is one colour
    /// the view gives (a scan the view fills), not colours of its own: a dark canvas shows it as it is rather than dimmed.
    Image {
        at: (f32, f32),
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        ink: bool,
    },
}

impl Prim {
    /// Its points, px: a fill's rings', a line's, a picture's corners.
    pub fn points(&self) -> Vec<(f32, f32)> {
        match self {
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Image { at, width, height, .. } => Prim::corners(*at, *width, *height),
        }
    }

    /// The same, `dx` px right and `dy` down.
    pub(crate) fn moved(self, dx: f32, dy: f32) -> Prim {
        let mv = |pts: Vec<(f32, f32)>| pts.into_iter().map(|(x, y)| (x + dx, y + dy)).collect();
        match self {
            Prim::Fill { rings, color } => Prim::Fill { rings: rings.into_iter().map(mv).collect(), color },
            Prim::Line { pts, width, color, dash, closed } => Prim::Line { pts: mv(pts), width, color, dash, closed },
            Prim::Image { at, width, height, rgba, ink } => Prim::Image { at: (at.0 + dx, at.1 + dy), width, height, rgba, ink },
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
            Prim::Image { at, width, height, rgba, ink } => {
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
                vec![Prim::Image { at: (at.0 + x0 as f32, at.1 + y0 as f32), width: x1 - x0, height: y1 - y0, rgba: kept, ink }]
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
    /// The pen each line is drawn in, for [`Drawing::weigh`] to size; none for a fill, a picture, lettering's strokes and
    /// what is sized already (a view's lines on a sheet).
    pub pens: Vec<Option<Pen>>,
    /// What the pens draw in this view, mm on paper: the project's line weights at its scale.
    pub(crate) weights: Weights,
    /// The element being drawn now, and the part of it.
    owner: Option<Id>,
    part: Part,
    /// How big annotation is drawn, times the pixels it is drawn in: see [`annotation`].
    pub(crate) k: f32,
    /// How many times [`sheet::DENSITY`] pixels a mm of the paper the view stands for is, so the pens' size: `k`, not
    /// clamped ([`paper_of`]).
    paper: f32,
    /// How much of what the view cuts it draws.
    detail: Detail,
}

impl Drawing {
    /// Nothing yet, through `view`.
    pub fn new(view: View) -> Drawing {
        Drawing {
            view,
            prims: Vec::new(),
            owners: Vec::new(),
            parts: Vec::new(),
            pens: Vec::new(),
            weights: Weights::default(),
            owner: None,
            part: Part::Projection,
            k: 1.0,
            paper: 1.0,
            detail: Detail::Fine,
        }
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
        self.push_pen(p, None);
    }
    fn push_pen(&mut self, p: Prim, pen: Option<Pen>) {
        self.prims.push(p);
        self.owners.push(self.owner);
        self.parts.push(self.part);
        self.pens.push(pen);
    }
    /// A line through pixels `pts` in `pen`, `dash`ed if it says.
    pub(crate) fn stroke(&mut self, pts: Vec<(f32, f32)>, pen: Pen, color: Rgb, dash: Option<[f32; 2]>, closed: bool) {
        let width = self.width(pen);
        self.push_pen(Prim::Line { pts, width, color, dash, closed }, Some(pen));
    }
    /// Each line's width as its pen draws it in this view, as annotation where `annotation` says its owner is, else as
    /// the model: the last step of drawing a view, once its overrides have set their pens.
    pub(crate) fn weigh(&mut self, annotation: impl Fn(Option<Id>) -> bool) {
        let (weights, k) = (self.weights, self.paper);
        for ((prim, owner), pen) in self.prims.iter_mut().zip(&self.owners).zip(&self.pens) {
            if let (Prim::Line { width, .. }, Some(pen)) = (prim, pen) {
                *width = (weights.mm(*pen, annotation(*owner)) * sheet::DENSITY) as f32 * k;
            }
        }
    }
    /// What a link builds, first, so what the project draws lies over it: each kept in the order it was drawn in.
    pub(crate) fn linked_under(&mut self) {
        let under = |o: &Option<Id>| o.is_some_and(crate::link::is_linked);
        if !self.owners.iter().any(under) {
            return;
        }
        let prims = std::mem::take(&mut self.prims);
        let mut all: Vec<(Prim, Option<Id>, Part, Option<Pen>)> = prims
            .into_iter()
            .zip(std::mem::take(&mut self.owners))
            .zip(std::mem::take(&mut self.parts))
            .zip(std::mem::take(&mut self.pens))
            .map(|(((p, o), part), pen)| (p, o, part, pen))
            .collect();
        all.sort_by_key(|(_, o, ..)| !under(o));
        for (p, o, part, pen) in all {
            self.prims.push(p);
            self.owners.push(o);
            self.parts.push(part);
            self.pens.push(pen);
        }
    }
    /// Each line marked [`BY_CATEGORY`] in the pen its owner's object style gives, cut or in projection as its part is: before
    /// a view's overrides set theirs.
    pub(crate) fn by_category(&mut self, style: impl Fn(Option<Id>) -> ObjectStyle) {
        for ((pen, owner), part) in self.pens.iter_mut().zip(&self.owners).zip(&self.parts) {
            if *pen == Some(BY_CATEGORY) {
                let st = style(*owner);
                *pen = Some(if *part == Part::Cut { st.cut } else { st.projection });
            }
        }
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
    /// `pts` joined in `pen`, as wide as it is on the paper the view stands for.
    pub(crate) fn line(&mut self, pts: &[P2], pen: Pen, color: Rgb, closed: bool) {
        let pts = pts.iter().map(|p| self.view.px(*p)).collect();
        self.stroke(pts, pen, color, None, closed);
    }
    /// [`Drawing::line`], dashed.
    pub(crate) fn dashed(&mut self, pts: &[P2], pen: Pen, color: Rgb, closed: bool) {
        let pts = pts.iter().map(|p| self.view.px(*p)).collect();
        self.stroke(pts, pen, color, Some([10.0, 5.0]), closed);
    }
    /// How many pixels wide `pen` draws here as the model: 0 for a hairline, which every painter draws as thin as it
    /// can. [`Drawing::weigh`] sizes annotation's at the end.
    pub(crate) fn width(&self, pen: Pen) -> f32 {
        (self.weights.mm(pen, false) * sheet::DENSITY) as f32 * self.paper
    }
    /// `px` pixels of annotation, as big as the view's scale makes them.
    fn px(&self, px: f32) -> f32 {
        px * self.k
    }

    /// The primitives pushed since `from`, grown `k` times about pixel `about`: annotation is laid out one pixel to the
    /// pixel and made as big as its view's scale says.
    fn grow(&mut self, from: usize, about: (f32, f32)) {
        let k = self.k;
        let at = |p: &mut (f32, f32)| *p = (about.0 + (p.0 - about.0) * k, about.1 + (p.1 - about.1) * k);
        for prim in self.prims.iter_mut().skip(from).filter(|_| k != 1.0) {
            match prim {
                Prim::Fill { rings, .. } => rings.iter_mut().flatten().for_each(at),
                Prim::Line { pts, .. } => pts.iter_mut().for_each(at),
                Prim::Image { .. } => {}
            }
        }
    }

    /// `text` centred on `at` px, capitals `height` px tall.
    pub(crate) fn text(&mut self, text: &str, at: (f32, f32), height: f32, color: Rgb) {
        self.text_turned(text, at, height, 0.0, color);
    }

    /// What's drawn cropped to the box from pixel `lo` to `hi`, but for the primitives of the
    /// elements `keep` takes.
    pub(crate) fn crop(&mut self, lo: (f32, f32), hi: (f32, f32), keep: impl Fn(Option<Id>) -> bool) {
        let (prims, owners, parts) = (std::mem::take(&mut self.prims), std::mem::take(&mut self.owners), std::mem::take(&mut self.parts));
        let pens = std::mem::take(&mut self.pens);
        for (((prim, owner), part), pen) in prims.into_iter().zip(owners).zip(parts).zip(pens) {
            for piece in if keep(owner) { vec![prim] } else { prim.cropped(lo, hi) } {
                self.prims.push(piece);
                self.owners.push(owner);
                self.parts.push(part);
                self.pens.push(pen);
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
            // What a link builds is drawn and snapped to, never picked.
            let Some(id) = owner.filter(|id| !crate::link::is_linked(*id)) else { continue };
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
                Prim::Image { at, width, height, rgba, .. } => drawn_near(p, (*at, *width, *height), rgba, tol),
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
            let Some(id) = owner.filter(|id| !crate::link::is_linked(*id)) else { continue };
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

/// How far pixel point `p` is from the nearest pixel a picture (its corner, width and height) draws, of those within `tol`: -1
/// on one, and none where all within it are clear. A picture picks where it isn't clear: a scan's where its points are.
fn drawn_near(p: P2, (at, width, height): ((f32, f32), u32, u32), rgba: &[u8], tol: f32) -> f64 {
    let (x, y) = (p.0 - f64::from(at.0), p.1 - f64::from(at.1));
    let r = f64::from(tol).ceil() + 1.0;
    let span = |v: f64, n: u32| ((v - r).floor().max(0.0) as u32, (v + r).ceil().clamp(0.0, f64::from(n)) as u32);
    let ((x0, x1), (y0, y1)) = (span(x, width), span(y, height));
    let mut best = f64::INFINITY;
    for py in y0..y1 {
        for px in x0..x1 {
            let alpha = (py as usize * width as usize + px as usize) * 4 + 3;
            if rgba.get(alpha).is_none_or(|a| *a == 0) {
                continue;
            }
            let (dx, dy) = ((x - f64::from(px) - 0.5).abs() - 0.5, (y - f64::from(py) - 0.5).abs() - 0.5);
            if dx <= 0.0 && dy <= 0.0 {
                return -1.0;
            }
            best = best.min(dx.max(0.0).hypot(dy.max(0.0)));
        }
    }
    best
}

/// The outlines of the walls and columns a plan cutting at height `z` cuts, but for those `look` hides.
fn cut_outlines(m: &Model, z: f64, look: &Look, index: &walls::Index) -> Vec<(Id, Vec<P2>)> {
    m.elements
        .iter()
        .filter(|(id, _)| look.shows(**id))
        .filter_map(|(&id, e)| match e {
            Element::Wall(w) if m.spans(w, z) => Some((id, walls::outline_in(m, id, Some(index)))),
            _ => stands(m, e).filter(|(_, bottom, top)| *bottom <= z && z < *top).map(|(outline, ..)| (id, outline)),
        })
        .collect()
}

/// What a column (a structural one, a footing) or a run (a beam, a strip footing) takes in plan, and the heights it reaches:
/// its outline, its bottom and its top.
fn stands(m: &Model, e: &Element) -> Option<(Vec<P2>, f64, f64)> {
    if let Some(col) = e.column() {
        let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
        return Some((m.column_outline(e), bottom, top));
    }
    let g = m.run_geom(e)?;
    let (bottom, top) = g.z_span();
    Some((g.footprint(), bottom, top))
}

/// The walls and columns a plan sees below its cut plane, down to its view depth: their outlines,
/// and their tops; but for those `look` hides.
fn seen_below(m: &Model, r: &PlanRange, look: &Look, index: &walls::Index) -> Vec<(Id, Vec<P2>, f64)> {
    m.elements
        .iter()
        .filter(|(id, _)| look.shows(**id))
        .filter_map(|(&id, e)| {
            let (outline, top) = match e {
                Element::Wall(w) => {
                    let top = m.top_z(w);
                    // Joining a wall scans its neighbours. Reject other storeys before
                    // building their outlines; a plan only needs walls in its depth range.
                    if !(r.depth < top && top <= r.cut) {
                        return None;
                    }
                    (walls::outline_in(m, id, Some(index)), top)
                }
                _ => {
                    let (outline, _, top) = stands(m, e)?;
                    (outline, top)
                }
            };
            // A footing or a run hangs under the level it is drawn on, its top flush with the view's bottom: that is seen.
            let flush = if matches!(e, Element::Footing(_) | crate::model::run_arm!(_)) { TOL } else { 0.0 };
            (r.depth - flush < top && top <= r.cut).then_some((id, outline, top))
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

/// The object style of `owner`'s category; with no owner, what a project's categories start with.
fn style_of(m: &Model, owner: Option<Id>) -> ObjectStyle {
    m.object_style(owner.and_then(|id| m.elements.get(id)).map_or("", Element::category))
}

/// Whether what `owner` is draws in the annotation's pens: a dimension, a tag, text, a spot elevation, a grid, a level, a
/// room's tag, the mark of a section or an elevation. The rest is the model's.
fn annotates(m: &Model, owner: Option<Id>) -> bool {
    matches!(
        owner.and_then(|id| m.elements.get(id)),
        Some(
            Element::Dimension(_)
                | Element::Tag(_)
                | Element::Text(_)
                | Element::Spot(_)
                | Element::Grid(_)
                | Element::Level(_)
                | Element::Room(_)
                | Element::Section(_)
                | Element::Elevation(_)
        )
    )
}

/// What of each cut outline is the edge of what the view cuts, as runs of points: its edges, but for the stretches
/// another outline lies against on their outer side, where two walls meet at a join or one runs into another; where two
/// runs meet, each carried on `reach` mm round the corner ([`turn_corners`]).
fn cut_edges(outlines: &[(Id, Vec<P2>)], reach: f64) -> Vec<(Id, Vec<Vec<P2>>)> {
    // How far past an edge, outward, a point is looked for in another outline: mm.
    const PAST: f64 = 1.0;
    let boxes: Vec<(P2, P2)> = outlines
        .iter()
        .map(|(_, o)| {
            o.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), p| {
                (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
            })
        })
        .collect();
    let near = |i: usize, j: usize| {
        let ((a, b), (c, d)) = (boxes[i], boxes[j]);
        i != j && a.0 <= d.0 + PAST && c.0 <= b.0 + PAST && a.1 <= d.1 + PAST && c.1 <= b.1 + PAST
    };
    let mut out = Vec::with_capacity(outlines.len());
    for (i, (id, o)) in outlines.iter().enumerate() {
        let others: Vec<&[P2]> = (0..outlines.len()).filter(|&j| near(i, j)).map(|j| outlines[j].1.as_slice()).collect();
        if others.is_empty() {
            let mut ring = o.clone();
            ring.extend(o.first().copied());
            out.push((*id, vec![ring]));
            continue;
        }
        // Outward is to the right of a counter-clockwise ring.
        let ccw = geom::area(o) > 0.0;
        let mut runs: Vec<Vec<P2>> = Vec::new();
        for (a, b) in geom::ring_edges(o) {
            let len = a.dist(b);
            if len <= 0.0 {
                continue;
            }
            // Cut where another outline's edge crosses it, or starts or ends on it.
            let mut ts = vec![0.0, 1.0];
            for (c, e) in others.iter().flat_map(|r| geom::ring_edges(r)) {
                ts.extend(geom::crossing(a, b, c, e));
                for p in [c, e] {
                    if geom::dist_to_segment(p, a, b) < 1e-6 * len.max(1.0) {
                        ts.push(geom::param_on_segment(p, a, b));
                    }
                }
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y) * len < 1e-6);
            let out_of = ((b - a).perp() * if ccw { -1.0 } else { 1.0 }).unit() * PAST;
            for w in ts.windows(2) {
                let (p, q) = (a + (b - a) * w[0], a + (b - a) * w[1]);
                let probe = (p + q) * 0.5 + out_of;
                if others.iter().any(|r| geom::contains(r, probe)) {
                    continue;
                }
                match runs.last_mut() {
                    Some(run) if run.last().is_some_and(|l| l.dist(p) < 1e-9) => run.push(q),
                    _ => runs.push(vec![p, q]),
                }
            }
        }
        // A run that ends where the first starts goes on round.
        if runs.len() > 1
            && let (Some(first), Some(last)) = (runs.first().and_then(|r| r.first().copied()), runs.last().and_then(|r| r.last().copied()))
            && first.dist(last) < 1e-9
        {
            let mut last = runs.pop().unwrap_or_default();
            last.extend(runs[0].iter().skip(1));
            runs[0] = last;
        }
        out.push((*id, runs));
    }
    turn_corners(&mut out, reach);
    out
}

/// A run of [`cut_edges`] in its category's pen: one that comes back to where it starts (an outline nothing lies against)
/// closed, so none of its corners is a line's round end.
fn ink_cut(d: &mut Drawing, run: &[P2]) {
    match run.split_last() {
        Some((last, ring)) if ring.len() > 2 && ring[0].dist(*last) < 1e-9 => d.line(ring, BY_CATEGORY, INK, true),
        _ => d.line(run, BY_CATEGORY, INK, false),
    }
}

/// Each run that ends where another ends (two walls' outlines meeting at a corner) carried on `reach` mm (or the other's
/// first edge, if shorter) along the other: so the corner is a join inside a line, mitred square, not two ends meeting with
/// a notch between them. What's carried on draws over the other's line, in its place.
fn turn_corners(runs: &mut [(Id, Vec<Vec<P2>>)], reach: f64) {
    // Every end: its point, and the way its run goes from it.
    let ends: Vec<(P2, P2)> = runs
        .iter()
        .flat_map(|(_, rs)| rs)
        .filter(|r| r.len() >= 2 && r.first().zip(r.last()).is_some_and(|(a, b)| a.dist(*b) > 1e-9))
        .flat_map(|r| {
            let n = r.len();
            [(r[0], r[1]), (r[n - 1], r[n - 2])]
        })
        .collect();
    let on = |at: P2, from: P2| {
        ends.iter().find(|(p, q)| p.dist(at) < 1e-6 && q.dist(from) > 1e-9).map(|(p, q)| {
            let len = p.dist(*q);
            *p + (*q - *p) * (reach.min(len) / len)
        })
    };
    for r in runs.iter_mut().flat_map(|(_, rs)| rs.iter_mut()) {
        let n = r.len();
        if n < 2 || r[0].dist(r[n - 1]) <= 1e-9 {
            continue;
        }
        let (head, tail) = (on(r[0], r[1]), on(r[n - 1], r[n - 2]));
        if let Some(q) = tail {
            r.push(q);
        }
        if let Some(q) = head {
            r.insert(0, q);
        }
    }
}

/// The plan of `level` fitted into `width` x `height` px, as an image of it is made: with its name lettered at its bottom left.
pub fn plan(m: &Model, level: impl Key, width: u32, height: u32) -> Drawing {
    let view = fit(&plan_extent(m, level), width, height, scale_of(m, level));
    draw_plan(m, level, view, false, annotation(m, level, view), scale_of(m, level), true)
}

/// What a plan of `level` takes on paper: all it shows, or cropped, its crop region and the grids'
/// ends at its edges.
pub(crate) fn plan_extent(m: &Model, level: impl Key) -> Vec<P2> {
    match m.crop_of(level) {
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
pub(crate) fn plan_shows(m: &Model, plan: impl Key) -> Vec<P2> {
    // The plan's own guid says what it is: its graphics, range and annotation; its level is what it shows of the model.
    let key = m.guid(plan);
    let level = m.plan_level(key).map_or(key, |l| m.guid(l));
    let r = m.plan_range(key);
    let look = Look::of(m, key);
    let index = walls::Index::new(m);
    let mut extent: Vec<P2> = cut_outlines(m, r.cut, &look, &index).into_iter().flat_map(|(_, o)| o).collect();
    extent.extend(seen_below(m, &r, &look, &index).into_iter().flat_map(|(_, o, _)| o));
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
            Element::Mass(ms) if m.mass_cut_by(ms, r.cut) => extent.extend(m.mass_outline(ms, r.cut)),
            Element::Component(c) if c.level == level => extent.extend(m.component_pieces(c).into_iter().flat_map(|p| p.outline)),
            Element::Grid(g) => extent.extend([g.start, g.end]),
            Element::Dimension(dm) if m.annotation_in(e, key) => extent.push(dm.at),
            Element::Section(v) | Element::Elevation(v) => extent.extend([v.start, v.end]),
            Element::Tag(t) if m.annotation_in(e, key) => extent.extend(m.anchor(t.host).map(|a| a + t.offset)),
            Element::Stair(Stair { level: l, top_level: t, .. }) | Element::Ramp(Ramp { level: l, top_level: t, .. })
                if *l == level || *t == Some(level) =>
            {
                if let Some(l) = stairs::layout(m, e) {
                    extent.extend(l.runs.iter().flat_map(|r| r.ring(l.width)).chain(l.pads.into_iter().flat_map(|p| p.ring)));
                }
            }
            Element::Railing(rl) if m.level_of(e) == Some(level) => extent.extend(stairs::railing(m, rl).into_iter().map(|(p, _)| p)),
            crate::model::run_arm!(_) if m.level_of(e) == Some(level) => {
                extent.extend(m.run_geom(e).map(|g| g.footprint()).unwrap_or_default())
            }
            Element::Roof(r) if r.level == level || m.level_above(level) == Some(r.level) => extent.extend(m.roof_outline(r)),
            Element::Text(n) if m.annotation_in(e, key) => extent.extend([Some(n.at), n.leader].into_iter().flatten()),
            Element::Spot(sp) if m.annotation_in(e, key) => extent.push(sp.at),
            _ => {}
        }
    }
    extent
}

/// An element that isn't in the model yet, drawn as a plan would draw it, its primitives owned
/// by `id`: what a tool previews. Annotation, doors, windows, columns, components, stairs, ramps, railings, roofs and masses only.
pub fn sketch(m: &Model, view: View, id: Id, e: &Element) -> Drawing {
    let mut d = Drawing::new(view);
    d.owner = Some(id);
    // Annotation as big as the view it will be in draws it.
    let on = m.annotation_plan(e);
    d.k = on.map_or(1.0, |v| annotation(m, v, view));
    // Its pens as the view it will be in draws them: annotation's, or the plan of its level's.
    let n = on.or_else(|| m.level_of(e)).map_or_else(|| m.units.default_scale(), |v| scale_of(m, v));
    (d.weights, d.paper) = (m.line_weights().at(n), paper_of(n, view.scale));
    match e {
        Element::Dimension(dm) => dimension(&mut d, m, dm),
        Element::Tag(t) => tag(&mut d, m, t),
        Element::Text(n) => note(&mut d, n),
        Element::Spot(sp) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
        Element::Section(v) => section_mark(&mut d, v),
        Element::Elevation(v) => elevation_mark(&mut d, v),
        Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) => d.line(&m.column_outline(e), Pen::HEAVY, INK, true),
        crate::model::run_arm!(_) => {
            if let Some(g) = m.run_geom(e) {
                d.line(&g.footprint(), Pen::HEAVY, INK, true)
            }
        }
        Element::Component(c) => component(&mut d, &m.component_pieces(c)),
        // Drawn at its sill, where the cut plane would pass through it whatever the plan's cut is.
        Element::Door(o) | Element::Window(o) => {
            if let Some(w) = m.wall(o.host) {
                opening(&mut d, m, e, o, w, m.own_base(w) + o.sill, true);
            }
        }
        Element::Stair(st) => stair(&mut d, m, st, st.level),
        Element::Ramp(r) => ramp(&mut d, m, r, r.level),
        Element::Railing(rl) => d.line(&stairs::railing(m, rl).into_iter().map(|(p, _)| p).collect::<Vec<_>>(), Pen::MEDIUM, INK, false),
        Element::Roof(r) => roof(&mut d, m, r),
        Element::Mass(ms) => d.dashed(&ms.boundary, Pen::MEDIUM, MASS, true),
        _ => {}
    }
    let annotation = on.is_some() || matches!(e, Element::Section(_) | Element::Elevation(_));
    let style = m.object_style(e.category());
    d.by_category(|_| style);
    d.weigh(|_| annotation);
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
pub fn crop_region(m: &Model, id: impl Key) -> Option<[P2; 2]> {
    match m.elements.get(id)? {
        Element::Level(l) => l.crop,
        Element::PlanView(v) => v.crop,
        Element::Section(v) | Element::Elevation(v) => v.crop.map(|[bottom, top]| [P2(0.0, bottom), P2(v.start.dist(v.end), top)]),
        _ => None,
    }
}

/// The heights section or elevation `id` would crop to: all of the model's, as far as its grids
/// reach past them.
pub(crate) fn section_heights(m: &Model, id: impl Key) -> [f64; 2] {
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

/// Where level `level` ends in section or elevation `view`, along its line from its start: its near
/// end and its far one, as far as the view shows. None where it doesn't show it.
pub fn level_ends(m: &Model, level: impl Key, view: impl Key) -> Option<(f64, f64)> {
    let v = m.section(view)?;
    let look = Look::of(m, view);
    let (first, last) = span(m, v, &look);
    let level = m.elements.id(level)?;
    let (_, a, b) = level_spans(m, v, &look).into_iter().find(|(id, ..)| *id == level)?;
    let (a, b) = (a.max(first), b.min(last));
    (a < b).then_some((a, b))
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
/// start, and up, with room for its levels' names and its grids' bubbles, which are as big as its
/// scale makes them.
pub fn section_fit(m: &Model, id: impl Key, width: u32, height: u32) -> View {
    let Some([lo, hi]) = section_bare(m, id) else { return fit(&[], width, height, m.units.default_scale()) };
    let (n, names) = (scale_of(m, id), level_names(m));
    fit_by(|scale| [lo, P2(hi.0 + f64::from(annotation_of(n, scale) * names) / scale, hi.1)], width, height, n)
}

/// What section or elevation `id` shows, along its line and up, at `scale` px per mm, its annotation `k` times the size it is
/// laid out at: from its start to its end, past its lowest and highest heights, with room past the end for the widest
/// level name.
pub(crate) fn section_extent(m: &Model, id: impl Key, scale: f64, k: f32) -> Option<[P2; 2]> {
    let [lo, hi] = section_bare(m, id)?;
    Some([lo, P2(hi.0 + f64::from(k * level_names(m)) / scale, hi.1)])
}

/// Section or elevation `id` along its line, past its lowest and highest heights: all it shows but the names of its levels.
/// Cropped, from its start to its end; else as far as anything it shows reaches.
fn section_bare(m: &Model, id: impl Key) -> Option<[P2; 2]> {
    let v = m.section(id)?;
    let look = Look::of(m, id);
    let ((lo, hi), (first, last)) = (shown_heights(m, v, &look), span(m, v, &look));
    Some([P2(first, lo - GRID_PAST), P2(last, hi + GRID_PAST)])
}

/// The room, px, the widest level name or height takes past the end of a section's line, drawn at the size it is laid out at.
fn level_names(m: &Model) -> f32 {
    m.levels().iter().map(|(_, l)| font::width(&l.name, 10.0).max(font::width(&m.units.length(l.elevation), 10.0))).fold(0.0, f32::max)
        + 12.0
}

/// The camera, and what it keeps, that show what section or elevation `id` sees through `view`.
pub fn section_camera(m: &Model, id: impl Key, view: View) -> Option<(Camera, Clip)> {
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
pub fn section_image(m: &Model, scene: &Scene, id: impl Key, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    let (cam, clip) = section_camera(m, id, view)?;
    Some(scene.cut_in(&cam, &clip, tints, &Look::of(m, id)))
}

/// What section or elevation `id` draws over its picture through `view`: where its line cuts
/// the solids, in poché; its levels, named past its end; the grids it crosses; its notes and spot
/// elevations; and its name. Cropped, only what lies between its crop's heights.
pub fn section(m: &Model, id: impl Key, view: View) -> Drawing {
    section_in(m, id, view, annotation(m, id, view), scale_of(m, id))
}

/// Section or elevation `id` as `section` draws it, its annotation `k` times the size it is laid out at: a sheet's, at
/// paper size. Its pens are the line weights' at the scale `k` and `view` make, a viewport's.
pub fn section_at(m: &Model, id: impl Key, view: View, k: f32) -> Drawing {
    section_in(m, id, view, k, f64::from(k) * sheet::DENSITY / view.scale)
}

/// [`section_at`], its pens the line weights' at 1 : `n`.
fn section_in(m: &Model, id: impl Key, view: View, k: f32, n: f64) -> Drawing {
    let mut d = Drawing::new(view);
    (d.k, d.paper, d.detail, d.weights) = (k, paper_of(n, view.scale), detail_of(m, id), m.line_weights().at(n));
    let Some(v) = m.section(id) else { return d };
    let look = Look::of(m, id);
    let u = (v.end - v.start).unit();
    let (first, last) = span(m, v, &look);
    // Each element's cut pieces, filled as one so where they meet (a roof's slopes at its ridge)
    // leaves no seam.
    let mut cuts: std::collections::BTreeMap<Id, Vec<Vec<P2>>> = std::collections::BTreeMap::new();
    let solids: Vec<_> = view3d::solids(m).into_iter().filter(|s| look.shows(s.owner())).collect();
    d.part(Part::Cut);
    // A mass is outlined where the line cuts it, below, not filled.
    for s in solids.iter().filter(|s| !matches!(m.elements.get(s.owner()), Some(Element::Mass(_)))) {
        // Cut from where the view begins, then back along the line from its start.
        for ring in s.cuts(v.start + u * first, u, last - first) {
            cuts.entry(s.owner()).or_default().push(ring.into_iter().map(|q| P2(q.0 + first, q.1)).collect());
        }
    }
    // Poché, then the edge of what is cut over it: joined walls show their union, as in plan, filled
    // as one first so no seam shows where pieces meet.
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
    let pieces: Vec<(Id, Vec<P2>)> = cuts.iter().flat_map(|(owner, rings)| rings.iter().map(|r| (*owner, r.clone()))).collect();
    for (owner, runs) in cut_edges(&pieces, 1.0 / d.view.scale) {
        d.owner = Some(owner);
        for run in runs {
            ink_cut(&mut d, &run);
        }
    }
    // A compound wall's layers where the line cuts it: each its material's cut pattern, between where
    // the line crosses its boundaries; a thin line up each boundary between two, as high as the wall
    // stands there, around its openings.
    for (&wall, e) in &m.elements {
        let Element::Wall(w) = e else { continue };
        let layers = m.type_layers("wall", &w.type_name);
        if layers.is_empty() || !look.shows(wall) || d.detail == Detail::Coarse {
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
                    d.line(&[P2(along, z0), P2(along, z1)], Pen::PROJECTION, INK, false);
                    d.part(Part::Cut);
                }
            }
        }
    }
    // Each layer of a compound floor, roof or ceiling it cuts: its material's cut pattern, a thin
    // line between each two, parallel to its faces.
    let coarse = d.detail == Detail::Coarse;
    for (&owner, rings) in cuts.iter().filter(|_| !coarse) {
        let layers = match m.elements.get(owner) {
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
                    d.line(&[p, q], Pen::PROJECTION, INK, false);
                    d.part(Part::Cut);
                }
                above += layer.thickness;
            }
        }
    }
    for (&mid, e) in m.elements.iter().filter(|(id, _)| look.shows(**id)) {
        let Element::Mass(ms) = e else { continue };
        let (z0, z1) = m.span(ms.level, ms.base_offset, ms.top_level, ms.height);
        d.owner = Some(mid);
        // Lofted, where the line cuts its skin.
        if ms.lofted() {
            for line in
                massing::section_lines(&massing::tris(&ms.footprint(), &ms.top_ring(), (z0, z1)), v.start + u * first, u, last - first)
            {
                let line: Vec<P2> = line.into_iter().map(|q| P2(q.0 + first, q.1)).collect();
                d.dashed(&line, BY_CATEGORY, MASS, false);
            }
            continue;
        }
        for (s0, s1) in view3d::crossings(&ms.boundary, v.start, u) {
            let (a, b) = (s0.max(first), s1.min(last));
            if b - a > 1e-6 {
                d.dashed(&[P2(a, z0), P2(b, z0), P2(b, z1), P2(a, z1)], BY_CATEGORY, MASS, true);
            }
        }
    }
    d.owner = None;
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
        d.stroke(vec![a, b], Pen::PROJECTION, INK, Some([12.0, 4.0]), false);
        for (text, dy) in [(l.name.clone(), -8.0), (m.units.length(l.elevation), 8.0)] {
            let h = d.px(10.0);
            d.text(&text, (b.0 + d.px(8.0) + font::width(&text, h) / 2.0, b.1 + d.px(dy)), h, INK);
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
            Element::Text(n) if m.same(n.view, id) => note(&mut d, n),
            Element::Spot(sp) if m.same(sp.view, id) => {
                let z = m.spot_value(sp).unwrap_or(sp.at.1);
                spot_mark(&mut d, view.px(P2(sp.at.0, z)), Some(z), m.units);
            }
            _ => {}
        }
    }
    d.owner = None;
    d.by_category(|o| style_of(m, o));
    look.dress(&mut d);
    d.linked_under();
    d.weigh(|o| annotates(m, o));
    let at = (24.0 + font::width(&v.name, 16.0) / 2.0, view.height as f32 - 26.0);
    d.text_turned(&v.name, at, 16.0, 0.0, INK);
    d
}

/// Section or elevation `id` through `view`, as a PNG: its picture, and its drawing over it.
pub fn section_png(m: &Model, id: impl Key, view: View) -> Result<Vec<u8>, String> {
    let image = section_image(m, &Scene::new(m), id, view, &[]).ok_or_else(|| format!("{id} is not a section or an elevation"))?;
    let size = tiny_skia::IntSize::from_wh(image.width, image.height).ok_or("the image size is out of range")?;
    let mut pm = Pixmap::from_vec(image.rgba, size).ok_or("the image size is out of range")?;
    paint(&mut pm, &section(m, id, view));
    pm.encode_png().map_err(|e| e.to_string())
}

/// The plan of `level` as `view` shows it: what a canvas that pans and zooms draws.
pub fn draw(m: &Model, level: impl Key, view: View) -> Drawing {
    draw_plan(m, level, view, false, annotation(m, level, view), scale_of(m, level), false)
}

/// The reflected ceiling plan of `level` fitted into `width` x `height` px.
pub fn ceiling_plan(m: &Model, level: impl Key, width: u32, height: u32) -> Drawing {
    let view = fit(&plan_shows(m, level), width, height, scale_of(m, level));
    draw_plan(m, level, view, true, annotation(m, level, view), scale_of(m, level), true)
}

/// The reflected ceiling plan of `level` as `view` shows it: cut [`CEILING_CUT`] above the level
/// (over the doors) and looking up, at the ceilings up to the level above, as a floor plan lies
/// (not mirrored). Its grids and its sections' marks; no annotation of the floor plan's.
pub fn draw_ceiling(m: &Model, level: impl Key, view: View) -> Drawing {
    draw_plan(m, level, view, true, annotation(m, level, view), scale_of(m, level), false)
}

/// How big the annotation of view `id` is drawn through `view`, times the size it is laid out at. A view with a scale
/// holds it as paper at that scale would: its lettering and marks a fixed size on paper, which `scale` mm of the model per
/// mm of paper and `view` showing `view.scale` pixels to the mm make of it ([`sheet::DENSITY`] pixels to a mm of paper); on
/// a sheet, where the viewport sets the scale, it is 1. A view with no scale of its own is at the units' default
/// ([`crate::units::Units::default_scale`]). A shortcut: clamped to a tenth and ten times, so a hostile scale or zoom
/// can't draw lettering a screen tall; widen it if a plan at 1 : 10000 on screen wants more. The pens aren't clamped
/// ([`paper_of`], [`Drawing::width`]): a line is as wide as on the paper the view stands for, however far in or out.
pub fn annotation(m: &Model, id: impl Key, view: View) -> f32 {
    annotation_of(scale_of(m, id), view.scale)
}

/// [`annotation`] for a view at scale 1 : `n` shown at `scale` pixels to the mm.
fn annotation_of(n: f64, scale: f64) -> f32 {
    (n * scale / sheet::DENSITY).clamp(0.1, 10.0) as f32
}

/// [`annotation_of`] unclamped, what sizes the pens: but for a hostile scale or zoom, 0 (a hairline) when it isn't a
/// number and no more than a million.
fn paper_of(n: f64, scale: f64) -> f32 {
    let k = n * scale / sheet::DENSITY;
    if k.is_finite() && k > 0.0 { k.min(1e6) as f32 } else { 0.0 }
}

/// The level of detail view `id` draws at: its own, or its template's where that controls it.
fn detail_of(m: &Model, id: impl Key) -> Detail {
    crate::graphics::graphics_of(m, id).map_or_else(Detail::default, |g| g.resolved(m).detail)
}

/// Plan `level` as `draw` draws it, its annotation `k` times the size it is laid out at: a sheet's, at paper size.
pub fn draw_at(m: &Model, level: impl Key, view: View, k: f32) -> Drawing {
    draw_plan(m, level, view, false, k, f64::from(k) * sheet::DENSITY / view.scale, false)
}

/// The points of the scans a plan sees, from its cut plane down to its view depth, as Revit draws a point cloud in plan: from
/// above, each a dot [`view3d::DOT`] pixels wide in its colour (or the view's), the highest at each pixel on top, no more than
/// [`view3d::DRAWN`] of them among the scans. Each scan is one picture, no bigger than its dots and clear where it has none, so
/// it picks where its points are.
fn scans(d: &mut Drawing, m: &Model, range: &PlanRange, look: &Look) {
    let placed: Vec<(Id, &crate::model::PointCloud, &crate::cloud::Cloud)> = m
        .elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::PointCloud(pc) if look.shows(id) => Some((id, pc, m.clouds.get(&pc.cloud)?)),
            _ => None,
        })
        .collect();
    let step = view3d::every(placed.iter().map(|(_, _, c)| c.len()).sum());
    let (w, h) = (i64::from(d.view.width), i64::from(d.view.height));
    let (dot, half) = (view3d::DOT as i64, view3d::DOT as f64 / 2.0);
    let grey = Rgb(crate::cloud::GREY, crate::cloud::GREY, crate::cloud::GREY);
    for (id, pc, cloud) in placed {
        let place = m.cloud_place(pc);
        let colours = cloud.colours();
        // Each point it sees: its dot's corner, px, its height and its colour.
        let mut seen: Vec<(i64, i64, f64, Rgb)> = Vec::new();
        for (i, p) in cloud.points().iter().enumerate().step_by(step) {
            let [x, y, z] = place(*p);
            if !(range.depth - TOL..=range.cut).contains(&z) {
                continue;
            }
            let (px, py) = d.view.px(P2(x, y));
            let (x0, y0) = ((f64::from(px) - half).round(), (f64::from(py) - half).round());
            if !(x0 > -(dot as f64) && y0 > -(dot as f64) && x0 < w as f64 && y0 < h as f64) {
                continue;
            }
            let base = colours.and_then(|c| c.get(i)).map_or(grey, |c| Rgb(c[0], c[1], c[2]));
            seen.push((x0 as i64, y0 as i64, z, look.surface(id, base)));
        }
        let Some((lo, hi)) = seen.iter().fold(None, |b: Option<((i64, i64), (i64, i64))>, &(x, y, ..)| {
            let ((x0, y0), (x1, y1)) = b.unwrap_or(((x, y), (x, y)));
            Some(((x0.min(x), y0.min(y)), (x1.max(x), y1.max(y))))
        }) else {
            continue;
        };
        let (lo, hi) = ((lo.0.max(0), lo.1.max(0)), ((hi.0 + dot).min(w), (hi.1 + dot).min(h)));
        let (pw, ph) = ((hi.0 - lo.0).max(0) as usize, (hi.1 - lo.1).max(0) as usize);
        let mut top = vec![f64::NEG_INFINITY; pw * ph];
        let mut rgba = vec![0; pw * ph * 4];
        for (x0, y0, z, c) in seen {
            for py in y0.max(lo.1)..(y0 + dot).min(hi.1) {
                for px in x0.max(lo.0)..(x0 + dot).min(hi.0) {
                    let k = (py - lo.1) as usize * pw + (px - lo.0) as usize;
                    if let (Some(t), Some(out)) = (top.get_mut(k), rgba.get_mut(4 * k..4 * k + 4))
                        && z > *t
                    {
                        *t = z;
                        out.copy_from_slice(&[c.0, c.1, c.2, 255]);
                    }
                }
            }
        }
        d.own(Some(id));
        d.push(Prim::Image { at: (lo.0 as f32, lo.1 as f32), width: pw as u32, height: ph as u32, rgba, ink: look.filled(id) });
    }
    d.own(None);
}

/// A floor plan of `plan`, or `up`, its reflected ceiling plan; `titled`, with its name lettered at its bottom left (an image of
/// it has it; a window shows the name in the view's tab, and a sheet under its viewport). Its annotation `k` times the size it
/// is laid out at, its pens the line weights' at 1 : `n`.
fn draw_plan(m: &Model, plan: impl Key, view: View, up: bool, k: f32, n: f64, titled: bool) -> Drawing {
    // The plan's own guid says what it is: its graphics, range, crop and annotation; its level is what it shows of the model.
    let key = m.guid(plan);
    let level = m.plan_level(key).map_or(key, |l| m.guid(l));
    let index = walls::Index::new(m);
    let mut d = Drawing::new(view);
    (d.k, d.paper, d.detail, d.weights) = (k, paper_of(n, view.scale), detail_of(m, key), m.line_weights().at(n));
    let look = Look::of(m, key);
    let shown = |id: Id| look.shows(id);
    let mut range = m.plan_range(key);
    if up {
        range.cut = m.elevation(level) + CEILING_CUT;
        range.top = range.top.max(range.cut);
    }
    // The scans it sees, under all the rest.
    if !up {
        scans(&mut d, m, &range, &look);
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
            d.line(ring, BY_CATEGORY, FAINT, true);
        }
        holes(&mut d, m, &m.floor_slabs(f));
        // Its top layer's surface pattern, faint, in a fine view.
        if let Some(pattern) = m
            .type_layers("floor", &f.type_name)
            .first()
            .and_then(|l| m.materials.get(&l.material))
            .map(|x| x.surface)
            .filter(|_| d.detail == Detail::Fine)
        {
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
    let mut components = crate::component_geometry::Components::new(m);
    for (&id, e) in m.elements.iter().filter(|_| !up).filter(|(id, _)| shown(**id)) {
        let Element::Component(c) = e else { continue };
        let (bottom, top) = components.heights(c);
        if bottom < range.cut && top > range.depth {
            d.owner = Some(id);
            component(&mut d, &components.pieces(c));
        }
    }
    // Walls and columns below the cut plane, as seen from it; past the bottom, faint.
    for (id, o, top) in seen_below(m, &range, &look, &index).into_iter().filter(|_| !up) {
        d.owner = Some(id);
        d.line(&o, BY_CATEGORY, if top > range.bottom - TOL { INK } else { FAINT }, true);
    }
    // A run's centre line, faint and dashed, where it is seen but not cut: Revit's mark of a beam, and where to pick it.
    for (&id, e) in m.elements.iter().filter(|_| !up).filter(|(id, _)| shown(**id)) {
        let Some(g) = m.run_geom(e).filter(|g| !g.vertical()) else { continue };
        let (lo, hi) = g.z_span();
        let seen = range.depth - TOL < hi && hi <= range.cut;
        let over = range.cut < lo && lo <= range.top;
        if seen || over {
            d.owner = Some(id);
            let pts = [P2(g.a[0], g.a[1]), P2(g.b[0], g.b[1])].iter().map(|p| d.view.px(*p)).collect();
            d.stroke(pts, BY_CATEGORY, FAINT, Some([14.0, 5.0]), false);
        }
    }
    d.part(Part::Cut);
    let outlines = cut_outlines(m, range.cut, &look, &index);
    // Poché first, then the outlines over it, but for where walls meet at their joins: only the edge
    // of their union is inked (`cut_edges`). The union is filled first, as one for each colour the
    // view fills them in, so no seam shows where two fills meet.
    // What a link builds is filled apart, for [`Drawing::linked_under`] to put under the project's.
    let (under, own): (Vec<_>, Vec<_>) = outlines.iter().partition(|(id, _)| crate::link::is_linked(*id));
    for (group, owner) in [(under, Some(crate::link::UNION)), (own, None)] {
        d.owner = owner;
        for (color, rings) in cut_unions(&look, group.into_iter().map(|(id, o)| (*id, o.clone()))) {
            d.fill(&rings, color);
        }
    }
    // One fill per wall or column: in one even-odd fill, overlapping walls would cancel out.
    for (id, o) in &outlines {
        d.owner = Some(*id);
        d.fill(std::slice::from_ref(o), POCHE);
    }
    for (id, runs) in cut_edges(&outlines, 1.0 / d.view.scale) {
        d.owner = Some(id);
        for run in runs {
            ink_cut(&mut d, &run);
        }
    }
    // A compound wall's layers: each its material's cut pattern where it's wide enough to hold it,
    // else lighter where it isn't the wall's structure; a thin line between each two.
    for (&id, e) in &m.elements {
        let Element::Wall(w) = e else { continue };
        let layers = m.type_layers("wall", &w.type_name);
        if layers.is_empty() || !m.spans(w, range.cut) || !shown(id) || d.detail == Detail::Coarse {
            continue;
        }
        d.owner = Some(id);
        let lines = walls::layer_lines_in(m, id, Some(&index));
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
            d.line(line, Pen::PROJECTION, INK, false);
        }
        d.part(Part::Cut);
    }
    for (&id, e) in &m.elements {
        // The opening is the wall's: with neither the wall nor the opening shown, there is nothing to leave a gap in.
        if let Element::Door(o) | Element::Window(o) = e
            && let Some(w) = m.wall(o.host)
            && m.spans(w, range.cut)
            && (shown(id) || m.elements.id(o.host).is_some_and(shown))
        {
            d.owner = Some(id);
            opening(&mut d, m, e, o, w, range.cut, shown(id));
        }
    }
    d.part(Part::Projection);
    // A ceiling plan isn't cropped: the crop region is the floor plan's.
    let crop = m.crop_of(key).filter(|_| !up);
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
                let from = d.prims.len();
                d.text(&r.name, (x, y - 16.0), 13.0, INK);
                d.text(&r.number, (x, y), 10.0, INK);
                d.text(&area, (x, y + 14.0), 10.0, INK);
                d.grow(from, (x, y));
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
            // A mass the plane passes through: its outline, dashed.
            Element::Mass(ms) if m.mass_cut_by(ms, range.cut) => d.dashed(&m.mass_outline(ms, range.cut), BY_CATEGORY, MASS, true),
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
                        d.stroke(pts, BY_CATEGORY, INK, Some([10.0, 5.0]), true);
                    }
                }
            }
            // Overhead too: a run over the cut plane and under the view range's top, dashed.
            crate::model::run_arm!(_) => {
                if let Some(g) = m.run_geom(e).filter(|g| range.cut < g.z_span().0 && g.z_span().0 <= range.top) {
                    let pts = g.footprint().iter().map(|p| d.view.px(*p)).collect();
                    d.stroke(pts, BY_CATEGORY, INK, Some([10.0, 5.0]), true);
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
            Element::Dimension(dm) if m.annotation_in(e, key) && inside(dm.at) => dimension(&mut d, m, dm),
            Element::Tag(t)
                if m.annotation_in(e, key)
                    && m.elements.id(t.host).is_some_and(shown)
                    && m.anchor(t.host).is_some_and(|a| inside(a + t.offset)) =>
            {
                tag(&mut d, m, t)
            }
            Element::Text(n) if m.annotation_in(e, key) && inside(n.at) => note(&mut d, n),
            Element::Spot(sp) if m.annotation_in(e, key) && inside(sp.at) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
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
            o.and_then(|id| m.elements.get(id)).is_some_and(|e| {
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
    d.by_category(|o| style_of(m, o));
    look.dress(&mut d);
    d.linked_under();
    d.weigh(|o| annotates(m, o));
    if titled {
        let mut title = m.view_name(key).map_or_else(String::new, str::to_string);
        if up {
            title = format!("{title} ({})", m.language.word("Ceiling Plan"));
        }
        let at = (24.0 + font::width(&title, 16.0) / 2.0, view.height as f32 - 26.0);
        d.text_turned(&title, at, 16.0, 0.0, INK);
    }
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
                    d.line(&[a + u * at, a + u * s0], BY_CATEGORY, INK, false);
                }
                at = f64::max(at, s1);
            }
            if at < len {
                d.line(&[a + u * at, b], BY_CATEGORY, INK, false);
            }
        }
    }
}

/// The outlines of the holes shafts cut in a floor's or a ceiling's slabs.
fn holes(d: &mut Drawing, m: &Model, slabs: &[(Vec<[f64; 3]>, f64)]) {
    let cut: std::collections::BTreeSet<Id> = slabs.iter().flat_map(|(face, rise)| m.less_shafts(face, *rise).1).collect();
    for id in cut {
        if let Some(Element::Shaft(sh)) = m.elements.get(id) {
            d.line(&sh.boundary, Pen::PROJECTION, FAINT, true);
        }
    }
}

/// A slope arrow from `tail` up to `head`, and its slope beside its middle.
fn slope_arrow(d: &mut Drawing, tail: P2, head: P2, slope: f64) {
    d.line(&[tail, head], Pen::PROJECTION, INK, false);
    let (a, b) = (d.view.px(tail), d.view.px(head));
    let (x, y) = arrow(d, a, b);
    let text = format!("{}°", figure(slope));
    // Beside it, to its left on screen.
    let clear = font::width(&text, d.px(10.0)) / 2.0 * y.abs() + d.px(6.0) * x.abs() + d.px(4.0);
    d.text(&text, ((a.0 + b.0) / 2.0 + y * clear, (a.1 + b.1) / 2.0 - x * clear), d.px(10.0), INK);
}

/// A shaft cut in plan: its outline, and the diagonals of its box across it.
fn shaft(d: &mut Drawing, ring: &[P2]) {
    d.line(ring, BY_CATEGORY, INK, true);
    let Some((lo, hi)) = bounds(ring) else { return };
    for (from, to) in [(lo, hi), (P2(lo.0, hi.1), P2(hi.0, lo.1))] {
        let u = (to - from).unit();
        for (s0, s1) in view3d::crossings(ring, from - u, u) {
            d.line(&[from - u + u * s0, from - u + u * s1], Pen::PROJECTION, INK, false);
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
    if pattern != Pattern::None && room >= PATTERN_ROOM && d.detail == Detail::Fine {
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
                d.line(&[from + dir * a, from + dir * b], Pen::FINE, color, false);
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
                        d.line(&[at, at + u * dot], Pen::STIPPLE, color, false);
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
        d.line(ring, BY_CATEGORY, INK, true);
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
                    d.line(&[from + u * s0, from + u * s1], Pen::PROJECTION, INK, false);
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

/// A view fitting `pts` into the image, leaving the margin free: see [`fit_by`].
fn fit(pts: &[P2], width: u32, height: u32, n: f64) -> View {
    let lo = pts.iter().fold(P2(f64::MAX, f64::MAX), |a, p| P2(a.0.min(p.0), a.1.min(p.1)));
    let hi = pts.iter().fold(P2(f64::MIN, f64::MIN), |a, p| P2(a.0.max(p.0), a.1.max(p.1)));
    let (lo, hi) = if pts.is_empty() { (P2(-5000.0, -5000.0), P2(5000.0, 5000.0)) } else { (lo, hi) };
    fit_by(|_| [lo, hi], width, height, n)
}

/// A view fitting what `extent` says is shown at a scale (px per mm) into the image, leaving the margin free: `MARGIN` px
/// round it, and as many times that as the annotation of a view at 1 : `n` is bigger than on screen ([`annotation_of`]), which
/// would otherwise outgrow it. The annotation's size and so the margin follow the scale the view comes out at, so that is
/// found by halving: the largest scale at which the view and its margin fit.
fn fit_by(extent: impl Fn(f64) -> [P2; 2], width: u32, height: u32, n: f64) -> View {
    let fits = |scale: f64| {
        let [lo, hi] = extent(scale);
        let room = |px: u32| (f64::from(px) - 2.0 * MARGIN * f64::from(annotation_of(n, scale)).max(1.0)).max(1.0);
        (hi.0 - lo.0).max(1000.0) * scale <= room(width) && (hi.1 - lo.1).max(1000.0) * scale <= room(height)
    };
    // A view is at least a metre across, so no scale is past a pixel to the mm of the image's smaller side.
    let (mut fits_at, mut too_big) = (0.0, f64::from(width.min(height).max(1)) / 1000.0);
    for _ in 0..48 {
        let mid = (fits_at + too_big) / 2.0;
        *(if fits(mid) { &mut fits_at } else { &mut too_big }) = mid;
    }
    let [lo, hi] = extent(fits_at);
    View { scale: fits_at, cx: (lo.0 + hi.0) / 2.0, cy: (lo.1 + hi.1) / 2.0, width, height }
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
        d.line(&[axis.at(s, int), axis.at(s, ext)], Pen::HEAVY, INK, false);
    }
    d.own(owner);
    if !shown || d.detail == Detail::Coarse {
        return;
    }
    // Of a family: the forms the cut plane passes through, outlined, and the lines of its symbols (a door's swing).
    if o.family.is_some() {
        for piece in m.opening_pieces(e).iter().filter(|p| p.bottom <= cut && cut < p.top) {
            d.line(&piece.outline, BY_CATEGORY, INK, true);
        }
        d.part(Part::Projection);
        for stroke in m.opening_strokes(e) {
            d.line(&stroke, BY_CATEGORY, INK, false);
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
        d.line(&[pivot, open], BY_CATEGORY, INK, false);
        // What the leaf sweeps is seen, not cut.
        d.part(Part::Projection);
        d.line(&arc(pivot, open, shut, 24), BY_CATEGORY, INK, false);
        d.part(Part::Cut);
    } else {
        for across in [int, (ext + int) / 2.0, ext] {
            d.line(&axis.points(s0, s1, across), Pen::PROJECTION, INK, false);
        }
    }
}

/// A stair in the plan of `level`. From its own level: its risers up to the cut plane, a cut line
/// across the run there, the rest (and the landings over the cut plane) dashed, and its walk line
/// marked UP. From the level it climbs to: all of it, and the walk line marked DN.
fn stair(d: &mut Drawing, m: &Model, st: &Stair, level: impl Key) {
    let level = m.guid(level);
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
            d.line(&[at(line - 0.4 * st.tread, -half), at(line + 0.4 * st.tread, half)], Pen::PROJECTION, INK, false);
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
fn ramp(d: &mut Drawing, m: &Model, r: &Ramp, level: impl Key) {
    let level = m.guid(level);
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
            d.stroke(pts, BY_CATEGORY, INK, over.then_some([6.0, 4.0]), false);
        }
    }
}

/// A line from `a` to `b`, one pixel wide; dashed, `over`.
fn segment(d: &mut Drawing, a: P2, b: P2, over: bool) {
    let pts = vec![d.view.px(a), d.view.px(b)];
    d.stroke(pts, BY_CATEGORY, INK, over.then_some([6.0, 4.0]), false);
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
    d.line(&pts, Pen::PROJECTION, INK, false);
    arrow(d, d.view.px(*before), d.view.px(*last));
    let (a, b) = (d.view.px(first), d.view.px(second));
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    // Before the walk line's start, clear of it however long the word is.
    let back = font::width(label, d.px(9.0)) / 2.0 * x.abs() + d.px(5.0) * y.abs() + d.px(8.0);
    d.text(label, (a.0 - x * back, a.1 - y * back), d.px(9.0), INK);
}

/// An arrowhead at pixel `b`, pointing from `a`; the way it points, on screen.
fn arrow(d: &mut Drawing, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let (long, wide) = (d.px(10.0), d.px(4.0));
    let head = vec![b, (b.0 - x * long - y * wide, b.1 - y * long + x * wide), (b.0 - x * long + y * wide, b.1 - y * long - x * wide)];
    d.push(Prim::Fill { rings: vec![head], color: INK });
    (x, y)
}

/// A roof in the plan of its level: its outline, and where its slopes meet, its ridges, hips and
/// valleys.
// ponytail: openings aren't drawn; a dormer's roof covers its own. Draw them for skylights.
fn roof(d: &mut Drawing, m: &Model, r: &Roof) {
    let faces = m.roof_faces(r).map_or_else(|_| vec![m.roof_outline(r)], |f| f.into_iter().flat_map(|(loops, _)| loops).collect());
    for f in faces {
        d.line(&f, BY_CATEGORY, INK, true);
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
    // Millimetres to a pixel of annotation.
    let px = f64::from(d.k) / d.view.scale;
    let ray = |t: f64| P2(t.cos(), t.sin());
    let arc: Vec<P2> = (0..=48).map(|i| o + ray(from + sweep * f64::from(i) / 48.0) * r).collect();
    d.line(&arc, Pen::PROJECTION, INK, false);
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
            d.line(&[o + u * (reach + 3.0 * px), o + u * (r + 4.0 * px)], Pen::PROJECTION, INK, false);
        }
        let (p, k) = (o + u * r, (u + u.perp()) * (3.0 * px));
        d.line(&[p - k, p + k], Pen::HEAVY, INK, false);
    }
    // Outside the arc's middle, clear of it however the text sits across the ray.
    let (text, mid) = (format!("{}°", figure(sweep.to_degrees())), ray(from + sweep / 2.0));
    let clear = f64::from(font::width(&text, 10.0) / 2.0) * mid.0.abs() + 5.0 * mid.1.abs() + 4.0;
    d.text(&text, d.view.px(o + mid * (r + clear * px)), d.px(10.0), INK);
}

/// A radial dimension: a line from a round element's centre (marked with a cross) toward `at`,
/// an arrow where it meets the edge, and the radius past its end.
fn radial(d: &mut Drawing, m: &Model, dim: &Dimension) {
    let Some(Ok((c, r))) = dim.refs.first().map(|x| m.reference_arc(x)) else { return };
    let u = if dim.at.dist(c) > 0.0 { (dim.at - c).unit() } else { P2(1.0, 0.0) };
    let (edge, end) = (c + u * r, if dim.at.dist(c) > r { dim.at } else { c + u * r });
    d.line(&[c, end], Pen::PROJECTION, INK, false);
    let (a, b) = (d.view.px(c), d.view.px(edge));
    // Inside a circle too small for it, the arrow comes in from outside.
    if r * d.view.scale > f64::from(d.px(14.0)) || end == edge {
        arrow(d, a, b);
    } else {
        arrow(d, d.view.px(end), b);
    }
    for (dx, dy) in [(d.px(4.0), 0.0), (0.0, d.px(4.0))] {
        d.stroke(vec![(a.0 - dx, a.1 - dy), (a.0 + dx, a.1 + dy)], Pen::PROJECTION, INK, None, false);
    }
    let text = format!("R {}", m.units.length(r));
    let (x, y) = d.view.px(end);
    let w = font::width(&text, d.px(10.0)) / 2.0 + d.px(6.0);
    d.text(&text, (x + u.0 as f32 * w, y - u.1 as f32 * w), d.px(10.0), INK);
}

/// A text note: its lines down from its top left corner, turned as it is, and its leader with an
/// arrow at its end.
pub(crate) fn note(d: &mut Drawing, n: &Note) {
    let h = d.px((n.size * sheet::DENSITY) as f32);
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
        d.stroke(vec![from, to], Pen::PROJECTION, INK, None, false);
        arrow(d, from, to);
    }
}

/// A spot elevation's mark at pixel `at`: a triangle standing on the point, and the height it
/// reads over a line beside it, in `units`.
fn spot_mark(d: &mut Drawing, (x, y): (f32, f32), z: Option<f64>, units: Units) {
    let from = d.prims.len();
    d.push(Prim::Fill { rings: vec![vec![(x, y), (x + 5.0, y - 8.0), (x - 5.0, y - 8.0)]], color: INK });
    let text = z.map_or_else(|| "?".into(), |z| height(units, z));
    let w = font::width(&text, 10.0);
    d.stroke(vec![(x - 5.0, y - 8.0), (x + 9.0 + w, y - 8.0)], Pen::PROJECTION, INK, None, false);
    d.text(&text, (x + 7.0 + w / 2.0, y - 16.0), 10.0, INK);
    d.grow(from, (x, y));
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
    let px = f64::from(d.k) / d.view.scale;
    let on = |s: f64| dim.at + n * s;
    d.line(&[on(lo - 4.0 * px), on(hi + 4.0 * px)], Pen::PROJECTION, INK, false);
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
        d.line(&[c + u * from, c + u * to], Pen::PROJECTION, INK, false);
        d.line(&[c - (u + n) * (3.0 * px), c + (u + n) * (3.0 * px)], Pen::HEAVY, INK, false);
    }
    // The line's angle on screen (y down): it runs east or north, so the text reads from below
    // or from the right.
    let angle = (-n.1).atan2(n.0);
    // The text's up, on screen.
    let up = (angle.sin() as f32, -angle.cos() as f32);
    for pair in cross.windows(2) {
        let [(s0, _), (s1, _)] = pair else { continue };
        let (x, y) = d.view.px(on((s0 + s1) / 2.0));
        d.text_turned(&m.units.length(s1 - s0), (x + up.0 * d.px(9.0), y + up.1 * d.px(9.0)), d.px(10.0), angle as f32, INK);
    }
}

/// A tag: a door's mark in a box, a window's in a hexagon, over a paper fill; any other element's label as plain
/// text over a paper fill; with a leader, a line to the point it names.
fn tag(d: &mut Drawing, m: &Model, t: &Tag) {
    let Some(anchor) = m.anchor(t.host) else { return };
    let head = anchor + t.offset;
    if t.leader {
        d.line(&[anchor, head], Pen::PROJECTION, INK, false);
    }
    let text = m.tag_text(t);
    let (x, y) = d.view.px(head);
    let from = d.prims.len();
    let (w, h) = ((font::width(&text, 10.0) / 2.0 + 5.0).max(8.0), 8.0);
    let ring = if matches!(m.elements.get(t.host), Some(Element::Window(_))) {
        vec![(x - w - 4.0, y), (x - w, y - h), (x + w, y - h), (x + w + 4.0, y), (x + w, y + h), (x - w, y + h)]
    } else {
        vec![(x - w, y - h), (x + w, y - h), (x + w, y + h), (x - w, y + h)]
    };
    d.push(Prim::Fill { rings: vec![ring.clone()], color: PAPER });
    // Openings' marks sit in a box or a hexagon; anything else's label is plain text.
    if matches!(m.elements.get(t.host), Some(Element::Door(_) | Element::Window(_))) {
        d.stroke(ring, Pen::PROJECTION, INK, None, true);
    }
    d.text(&text, (x, y), 10.0, INK);
    d.grow(from, (x, y));
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
    d.stroke(vec![a, b], Pen::MEDIUM, INK, Some([18.0, 5.0]), false);
    let (lx, ly) = looking_px(v);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let from = d.prims.len();
    d.push(Prim::Fill { rings: vec![vec![a, (a.0 + ux * 12.0, a.1 + uy * 12.0), (a.0 + lx * 12.0, a.1 + ly * 12.0)]], color: INK });
    let back = font::width(&v.name, 10.0) / 2.0 + 8.0;
    d.text(&v.name, (a.0 - ux * back, a.1 - uy * back), 10.0, INK);
    d.grow(from, a);
    let from = d.prims.len();
    d.stroke(vec![b, (b.0 + lx * 9.0, b.1 + ly * 9.0)], Pen::MEDIUM, INK, None, false);
    d.grow(from, b);
}

/// An elevation's marker in plan, at its line's middle: a circle with a flag pointing the way it
/// looks, and its name behind it.
fn elevation_mark(d: &mut Drawing, v: &Section) {
    const R: f32 = 9.0;
    let c = d.view.px(v.start + (v.end - v.start) * 0.5);
    let from = d.prims.len();
    let ring: Vec<(f32, f32)> = (0..32)
        .map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + R * t.cos() as f32, c.1 + R * t.sin() as f32)
        })
        .collect();
    d.push(Prim::Fill { rings: vec![ring.clone()], color: PAPER });
    d.stroke(ring, Pen::BUBBLE, INK, None, true);
    let (lx, ly) = looking_px(v);
    let side = (-ly * R * 0.8, lx * R * 0.8);
    let flag = vec![(c.0 + side.0, c.1 + side.1), (c.0 + lx * (R + 7.0), c.1 + ly * (R + 7.0)), (c.0 - side.0, c.1 - side.1)];
    d.push(Prim::Fill { rings: vec![flag], color: INK });
    let back = R + 6.0 + font::width(&v.name, 10.0) / 2.0 * lx.abs() + 6.0 * ly.abs();
    d.text(&v.name, (c.0 - lx * back, c.1 - ly * back), 10.0, INK);
    d.grow(from, c);
}

/// A grid line, dashed, with a bubble holding its name beyond each end.
fn grid(d: &mut Drawing, start: P2, end: P2, name: &str) {
    let (a, b) = (d.view.px(start), d.view.px(end));
    d.stroke(vec![a, b], Pen::PROJECTION, GRID, Some([14.0, 4.0]), false);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let dir = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    for ((x, y), k) in [(a, -BUBBLE), (b, BUBBLE)] {
        let from = d.prims.len();
        let c = (x + dir.0 * k, y + dir.1 * k);
        let ring = (0..32).map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + BUBBLE * t.cos() as f32, c.1 + BUBBLE * t.sin() as f32)
        });
        d.stroke(ring.collect(), Pen::BUBBLE, GRID, None, true);
        d.text(name, c, 10.0, GRID);
        d.grow(from, (x, y));
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
    for (i, p) in d.prims.iter().enumerate() {
        let _ = match p {
            Prim::Fill { rings, color } => {
                let data: Vec<String> = rings.iter().map(|r| path_data(r, true)).collect();
                write!(s, r#"<path d="{}" fill="{}"/>"#, data.join(" "), hex(*color))
            }
            Prim::Line { pts, width, color, dash, closed } => {
                let dash = dash.map_or_else(String::new, |[a, b]| format!(r#" stroke-dasharray="{a} {b}""#));
                // A hairline (pen 0) is one pixel of whatever shows the SVG, at any zoom.
                let width = if *width > 0.0 { format!(r#"{width}""#) } else { r#"1" vector-effect="non-scaling-stroke""#.to_string() };
                write!(
                    s,
                    r#"<path d="{}" fill="none" stroke="{}" stroke-width="{width} stroke-linecap="round" stroke-linejoin="{}"{dash}/>"#,
                    path_data(pts, *closed),
                    hex(*color),
                    if d.mitred(i) { "miter" } else { "round" }
                )
            }
            Prim::Image { at, width, height, rgba, .. } => match picture(*width, *height, rgba).and_then(|pm| pm.encode_png().ok()) {
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
    for (i, p) in d.prims.iter().enumerate() {
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
                    line_join: if d.mitred(i) { LineJoin::Miter } else { LineJoin::Round },
                    dash: dash.and_then(|[a, b]| StrokeDash::new(vec![a, b], 0.0)),
                    ..Stroke::default()
                };
                if let Some(path) = path(&[pts], *closed) {
                    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Prim::Image { at, width, height, rgba, .. } => {
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
        let d = plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(6000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "solid wall");
        assert_eq!(pixel(&png, d.view.px(P2(2000.0, 0.0))), (255, 255, 255), "the door's gap");
        assert_eq!(pixel(&png, d.view.px(P2(1000.0, 5000.0))), (255, 255, 255), "the room, away from its tag");
    }

    #[test]
    fn wall_corners_close_solid_without_a_notch() {
        let s = house();
        let d = plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        // Where the top and right walls overlap, and the square their ends stretch over, inside their outline.
        assert_eq!(pixel(&png, d.view.px(P2(8000.0, 6000.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert_eq!(pixel(&png, d.view.px(P2(8020.0, 6020.0))), (POCHE.0, POCHE.1, POCHE.2));
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
        let d = plan(&s.model, 100000, 800, 600);
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
        let d = plan(&s.model, 100001, 800, 600);
        assert!(!d.prims.is_empty(), "the grid and the view title still show");
        assert!(!d.prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)));
    }

    #[test]
    fn a_plan_drawn_at_a_given_view_maps_the_model_through_it() {
        let s = house();
        let fitted = plan(&s.model, 100000, 800, 600);
        let window = draw(&s.model, 100000, fitted.view);
        let n = window.prims.len();
        assert!(n < fitted.prims.len() && fitted.owners[..n] == window.owners[..], "the same display list, with an image's title after it");
        assert!(fitted.owners[n..].iter().all(Option::is_none));
        // Zoomed in twice on the house's top right corner: the corner lands mid-image.
        let view = View { scale: fitted.view.scale * 2.0, cx: 8000.0, cy: 6000.0, ..fitted.view };
        let png = png(&draw(&s.model, 100000, view)).unwrap();
        assert_eq!(pixel(&png, (400.0, 300.0)), (POCHE.0, POCHE.1, POCHE.2));
        let back = view.mm(view.px(P2(1234.0, -567.0)));
        assert!(back.dist(P2(1234.0, -567.0)) < 0.01, "{back:?}");
    }

    #[test]
    fn an_image_of_a_plan_letters_its_name_and_the_window_and_a_sheet_do_not() {
        let s = house();
        // The lettering is lines of no element's, drawn last.
        let title = |d: &Drawing| d.owners.last() == Some(&None);
        let fitted = plan(&s.model, 100000, 800, 600);
        assert!(title(&fitted), "an image names its view at its bottom left");
        assert!(title(&ceiling_plan(&s.model, 100000, 800, 600)));
        assert!(!title(&draw(&s.model, 100000, fitted.view)), "the tab names it in the window");
        assert!(!title(&draw_ceiling(&s.model, 100000, fitted.view)));
        assert!(!title(&draw_at(&s.model, 100000, fitted.view, 1.0)), "and the viewport under it on a sheet");
    }

    #[test]
    fn every_primitive_names_the_element_it_draws() {
        let s = house();
        let d = plan(&s.model, 100000, 800, 600);
        assert_eq!(d.owners.len(), d.prims.len());
        // Walls 100002 to 100005, then the door, the window, the room and the grid.
        for id in 100_002..=100_009 {
            assert!(d.owners.contains(&Some(id)), "element {id} draws nothing");
        }
        assert_eq!(d.owners.last(), Some(&None), "the view title belongs to no element");
    }

    #[test]
    fn pick_finds_the_element_drawn_nearest_a_point() {
        let s = house();
        let d = plan(&s.model, 100000, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick(at(2000.0, 0.0), 4.0), Some(100006), "the door, drawn over its wall");
        assert_eq!(d.pick(at(6000.0, 0.0), 4.0), Some(100002), "the wall");
        assert_eq!(d.pick(at(0.0, 3000.0), 4.0), Some(100005), "the wall, under a grid line on its centre line");
        let (x, y) = at(6000.0, -100.0);
        assert_eq!(d.pick((x, y + 3.0), 4.0), Some(100002), "just outside the wall's face");
        assert_eq!(d.pick(at(8000.0, 3000.0), 4.0), Some(100007), "the window");
        assert_eq!(d.pick(at(4000.0, 3000.0), 10.0), Some(100008), "the room, by its tag");
        assert_eq!(d.pick(at(0.0, -1500.0), 4.0), Some(100009), "the grid line");
        assert_eq!(d.pick(at(4000.0, 1200.0), 4.0), None, "the floor of the room, away from its tag");
    }

    #[test]
    fn pick_all_lists_what_is_near_and_boxes_select_inside_or_crossing() {
        let s = house();
        let d = plan(&s.model, 100000, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick_all(at(2000.0, 0.0), 4.0), [100006, 100002], "the door, then its wall: Tab's order");
        // A box around the window, across the wall it's in.
        let (a, b) = (at(7000.0, 2000.0), at(9000.0, 4000.0));
        assert_eq!(d.in_box(a, b, false), [100007], "a window selects what is all inside");
        assert_eq!(d.in_box(b, a, true), [100003, 100007], "a crossing box also what it touches");
    }

    #[test]
    fn a_tag_on_a_wall_or_a_room_draws_its_label_as_plain_text_over_the_plan() {
        let mut s = house();
        // The wall's type, and the room's area, each a little up and to the right of the point it names.
        let first = |cat: &str| s.model.elements.iter().find(|(_, e)| e.category() == cat).map(|(id, _)| *id).unwrap();
        let (wall_id, room_id) = (first("wall"), first("room"));
        s.script(&format!("tag {{\"ids\": [{wall_id}], \"leader\": true}}\ntag {{\"ids\": [{room_id}], \"label\": \"Area\"}}")).unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        let tag_on = |host: Id| {
            s.model.elements.iter().find_map(|(id, e)| match e {
                Element::Tag(t) if t.host == s.model.guid(host) => Some((*id, s.model.anchor(host).unwrap() + t.offset)),
                _ => None,
            })
        };
        let (wall, room) = (tag_on(wall_id), tag_on(room_id));
        let ((wall_tag, wall_head), (room_tag, room_head)) = (wall.unwrap(), room.unwrap());
        assert_eq!((wall_head, room_head), (P2(4400.0, 400.0), P2(4400.0, 3400.0)));
        // Picked where it is drawn, and no box round it: the paper fill stops short of a ring the way a door's box has.
        assert_eq!(d.pick(d.view.px(wall_head), 2.0), Some(wall_tag));
        assert_eq!(d.pick(d.view.px(room_head), 2.0), Some(room_tag));
        let (x, y) = d.view.px(wall_head);
        let (r, g, b) = pixel(&png, (x, y - 8.0));
        assert!(r > 200 && g > 200 && b > 200, "no box above a plain label: {:?}", (r, g, b));
        // Its leader runs from the wall's middle up to the label.
        let (lx, ly) = d.view.px(P2(4200.0, 200.0));
        let (r, g, b) = pixel(&png, (lx, ly));
        assert!(r < 200 && g < 200 && b < 200, "the leader line");
    }

    #[test]
    fn dimensions_and_tags_draw_over_the_plan() {
        let mut s = house();
        // The outer faces of the left and right walls, measured below the house; then every door and window tagged.
        s.script("dimension {\"refs\": [[-100, 3000], [8100, 3000]], \"at\": [4000, -1500]}\ntag {}").unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 200 && g < 200 && b < 200
        };
        assert!(dark(d.view.px(P2(1000.0, -1500.0))), "the dimension line");
        assert!(dark(d.view.px(P2(-100.0, -1000.0))), "a witness line, down from the left wall's outer face");
        assert_eq!(d.pick(d.view.px(P2(1000.0, -1500.0)), 4.0), Some(100010));
        // The door's tag, a box outside the wall (the door swings in). The window faces its wall's
        // exterior side, its left: this house is drawn counter-clockwise, so that's in, and so is its tag.
        let (x, y) = d.view.px(P2(2000.0, -500.0));
        assert!(dark((x, y - 8.0)), "the top of the door tag's box");
        assert_eq!(d.pick((x, y), 2.0), Some(100011));
        assert_eq!(d.pick(d.view.px(P2(7500.0, 3000.0)), 2.0), Some(100012));
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
        let view = section_fit(&s.model, 100010, 800, 600);
        let png = section_png(&s.model, 100010, view).unwrap();
        // Along the section from its start at x = -1000; up from Level 1.
        let at = |x: f64, z: f64| view.px(P2(x + 1000.0, z));
        assert_eq!(pixel(&png, at(40.0, 1500.0)), (POCHE.0, POCHE.1, POCHE.2), "the west wall, cut (beside grid 1, on its centre line)");
        assert_eq!(pixel(&png, at(4000.0, 1500.0)), (255, 255, 255), "the north wall beyond, in hidden line");
        let d = section(&s.model, 100010, view);
        assert_eq!(d.pick(at(0.0, 1500.0), 2.0), Some(100005), "the cut is the wall's to pick");
        assert_eq!(d.pick(at(4000.0, 3000.0), 3.0), Some(100001), "Level 2's line");
        assert_eq!(d.pick(at(0.0, 3700.0), 3.0), Some(100009), "grid 1, where it crosses the section");
    }

    #[test]
    fn a_wall_cut_in_section_draws_the_pen_it_draws_cut_in_plan() {
        let s = cut();
        let view = section_fit(&s.model, 100010, 800, 600);
        let d = section(&s.model, 100010, view);
        let widest = (d.prims.iter().zip(&d.owners))
            .filter_map(|(p, o)| match p {
                Prim::Line { width, .. } if *o == Some(100005) => Some(*width),
                _ => None,
            })
            .fold(0.0, f32::max);
        assert!((widest - d.width(Pen::CUT)).abs() < 1e-4, "{widest} against {}", d.width(Pen::CUT));
    }

    #[test]
    fn elevations_show_the_walls_they_look_at_and_cut_nothing() {
        let s = cut();
        let Some(Element::Elevation(e)) = s.model.elements.get(100011) else { panic!("no elevation") };
        let start = e.start;
        let view = section_fit(&s.model, 100011, 800, 600);
        let png = section_png(&s.model, 100011, view).unwrap();
        // The south wall's face, between the door and the corner, from the south.
        let p = view.px(P2(6000.0 - start.0, 1500.0));
        assert_eq!(pixel(&png, p), (255, 255, 255));
        let d = section(&s.model, 100011, view);
        assert!(d.prims.iter().all(|p| !matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "no cut");
        assert_eq!(d.pick(view.px(P2(0.0 - start.0, 3700.0)), 3.0), Some(100009), "grid 1 runs into what it sees");
        assert_eq!(
            view3d::Scene::new(&s.model)
                .cut(&section_camera(&s.model, 100011, view).unwrap().0, &section_camera(&s.model, 100011, view).unwrap().1, &[])
                .pick(f64::from(p.0), f64::from(p.1)),
            Some(100_002)
        );
    }

    #[test]
    fn plans_mark_sections_and_elevations() {
        let s = cut();
        let d = plan(&s.model, 100000, 800, 600);
        assert_eq!(d.pick(d.view.px(P2(-500.0, 3000.0)), 3.0), Some(100010), "the section's line");
        let Some(Element::Elevation(e)) = s.model.elements.get(100011) else { panic!("no elevation") };
        let mid = e.start + (e.end - e.start) * 0.5;
        assert_eq!(d.pick(d.view.px(mid), 3.0), Some(100011), "the elevation's marker");
    }

    #[test]
    fn plans_cut_columns_draw_stairs_and_show_roofs_from_both_sides() {
        let mut s = house();
        // Walls 3 to 6, door 7, window 8, room 9, grid 10; then a column 11, a stair 12 and a roof 13.
        s.script("column {\"at\": [6000, 4000]}\nstair {\"start\": [1000, 1500], \"end\": [1000, 2500]}\nroof {}").unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(6000.0, 4000.0))), (POCHE.0, POCHE.1, POCHE.2), "the column, cut");
        // The stair: its first treads, and its arrow up the middle.
        assert_eq!(d.pick(d.view.px(P2(1400.0, 1500.0 + 280.0)), 2.0), Some(100011), "a tread");
        // The roof overhead, dashed, from below; seen from its own level, its hips run to the corners.
        assert_eq!(d.pick(d.view.px(P2(4000.0, -600.0)), 2.0), Some(100012), "the roof's edge, overhead");
        let up = plan(&s.model, 100001, 800, 600);
        let corner = up.view.px(P2(-600.0, -600.0));
        let hip = up.view.px(P2(-600.0 + 600.0, -600.0 + 600.0));
        assert_eq!(up.pick(((corner.0 + hip.0) / 2.0, (corner.1 + hip.1) / 2.0), 2.0), Some(100012), "a hip");
    }

    #[test]
    fn sections_cut_roofs_along_their_slope() {
        let mut s = house();
        s.script("roof {}\nsection {\"start\": [4000, -2000], \"end\": [4000, 8000]}").unwrap();
        // The roof 11, the section 12 (running north, looking west): its cut of the roof at the ridge.
        let view = section_fit(&s.model, 100011, 800, 600);
        let d = section(&s.model, 100011, view);
        let ridge = 3000.0 + 3100.0 * 30f64.to_radians().tan();
        let at = view.px(P2(2000.0 + 3000.0, ridge + 100.0));
        assert_eq!(d.pick(at, 2.0), Some(100010));
        let png = section_png(&s.model, 100011, view).unwrap();
        assert_eq!(pixel(&png, at), (POCHE.0, POCHE.1, POCHE.2));
    }

    #[test]
    fn plans_draw_what_their_view_range_sees() {
        let mut s = house();
        // A low wall across the room (11), 900 mm tall; a floor (12); the roof over it all (13).
        s.script("wall {\"points\": [[1000, 4500], [5000, 4500]], \"height\": 900}\nfloor {}\nroof {}").unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        // Cut at 1200: the window (900 to 2400 up) is open, and the low wall below the cut is seen from
        // above, its outline thin and no poché.
        assert_eq!(pixel(&png, d.view.px(P2(8050.0, 3000.0))), (255, 255, 255), "the window's opening");
        assert_eq!(d.pick(d.view.px(P2(3000.0, 4600.0)), 2.0), Some(100010));
        assert_eq!(pixel(&png, d.view.px(P2(3000.0, 4500.0))), (255, 255, 255));
        assert!(d.owners.contains(&Some(100012)), "the roof overhead, under the level above");
        // Cut above the window's head, the wall runs solid past it; with the top under the roof's
        // eaves, the roof isn't seen.
        s.execute("modify", &serde_json::json!({"id": 100000, "view_range": {"cut": 2500, "top": 2600}})).unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        assert_eq!(pixel(&png_of(&d), d.view.px(P2(8000.0, 3000.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert!(!d.owners.contains(&Some(100012)));
        // From Level 2, the floor below shows once the view depth reaches down to it.
        assert!(!plan(&s.model, 100001, 800, 600).owners.contains(&Some(100011)));
        s.execute("modify", &serde_json::json!({"id": 100001, "view_range": {"depth": -3000}})).unwrap();
        assert!(plan(&s.model, 100001, 800, 600).owners.contains(&Some(100011)));
    }

    fn png_of(d: &Drawing) -> Vec<u8> {
        png(d).unwrap()
    }

    #[test]
    fn crop_regions_clip_plans_and_sections() {
        let mut s = house();
        s.execute("crop", &serde_json::json!({"view": 100000, "region": [[-1000, -1000], [4000, 4000]]})).unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        assert!((d.view.cx - 1500.0).abs() < 1.0 && (d.view.cy - 1500.0).abs() < 1.0, "fitted to the crop: {:?}", d.view);
        assert!(!d.owners.contains(&Some(100003)), "the east wall lies outside");
        assert_eq!(d.pick(d.view.px(P2(3000.0, 0.0)), 2.0), Some(100002), "the south wall, up to the crop's edge");
        assert_eq!(d.pick(d.view.px(P2(5000.0, 0.0)), 2.0), None);
        // Annotation is whole or not there: the room's tag, its point on the crop's edge, keeps its name whole.
        assert!(d.owners.contains(&Some(100008)));
        let name = d.view.px(P2(4000.0 + 300.0, 3000.0));
        assert!(d.pick((name.0, name.1 - 16.0 * d.k), 3.0) == Some(100008), "the name past the crop's edge");
        // Grid 1 stops at the crop's edge, its bubble there.
        let (x, y) = d.view.px(P2(0.0, 4000.0));
        assert_eq!(d.pick((x + 11.0 * d.k, y - 11.0 * d.k), 2.0), Some(100009));
        // A section cropped to the ground floor's lower part: Level 2 isn't in it, and the cut wall stops.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
        s.execute("crop", &serde_json::json!({"view": 100010, "region": [[0, -100], [5000, 2000]]})).unwrap();
        let view = section_fit(&s.model, 100010, 800, 600);
        let d = section(&s.model, 100010, view);
        assert!(!d.owners.contains(&Some(100001)) && d.owners.contains(&Some(100000)), "only the levels inside");
        let png = section_png(&s.model, 100010, view).unwrap();
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
        let d = plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        let dark = |at: (f32, f32)| {
            let (r, g, b) = pixel(&png, at);
            r < 200 && g < 200 && b < 200
        };
        // The note: its first letter's upright, just right of and below its top left corner.
        let (x, y) = d.view.px(P2(1000.0, 5000.0));
        assert_eq!(d.pick((x + 1.0, y + 6.0), 2.0), Some(100010));
        // The spot elevation's mark: a triangle standing on the point.
        let (x, y) = d.view.px(P2(2000.0, 2000.0));
        assert!(dark((x, y - 4.0)), "its triangle");
        assert_eq!(d.pick((x, y - 3.0), 2.0), Some(100012));
        // The angular dimension's arc, 1000 mm out from the corner at 45° / 2 round from east.
        let r = P2(2500.0, 1000.0).length();
        let mid = P2(22.5f64.to_radians().cos(), 22.5f64.to_radians().sin()) * r;
        assert_eq!(d.pick(d.view.px(mid), 2.0), Some(100014));
        // The radial one's line, from the column's centre toward `at`.
        let along = P2(6000.0, 4000.0) + (P2(6800.0, 4800.0) - P2(6000.0, 4000.0)) * 0.5;
        assert_eq!(d.pick(d.view.px(along), 2.0), Some(100016));
        // In a section: a spot elevation at its point, and a note.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nspot {\"at\": [3000, 2000], \"view\": 100017}\ntext {\"text\": \"N\", \"at\": [5000, 2500], \"view\": 100017}").unwrap();
        let view = section_fit(&s.model, 100017, 800, 600);
        let d = section(&s.model, 100017, view);
        let (x, y) = view.px(P2(3000.0, 2000.0));
        assert_eq!(d.pick((x, y - 3.0), 2.0), Some(100018));
        assert!(d.owners.contains(&Some(100019)));
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
               floor {"level": 100001, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}
               shaft {"boundary": [[3000, 3000], [4000, 3000], [4000, 5000], [3000, 5000]]}"#,
        )
        .unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        let ramp: Vec<&Prim> = d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(100010)).map(|(p, _)| p).collect();
        assert!(ramp.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == INK)), "its slope arrow's head");
        // Cut by Level 1's plan, the shaft shows its outline and a cross; from Level 2, the hole in its floor.
        assert_eq!(d.pick(d.view.px(P2(3500.0, 4000.0)), 2.0), Some(100012), "where its cross crosses");
        let up = plan(&s.model, 100001, 800, 600);
        assert!(!up.owners.contains(&Some(100012)));
        assert_eq!(up.pick(up.view.px(P2(3500.0, 3000.0)), 2.0), Some(100011), "the hole's edge");
    }

    #[test]
    fn ceiling_plans_look_up_at_ceilings_over_walls_cut_above_the_doors() {
        let mut s = house();
        // A tiled ceiling (11) in the room, its grid through its middle: 600 mm squares from (4000, 3000).
        s.script(r#"ceiling {"point": [4000, 1500], "type": "Tile 600 x 600"}"#).unwrap();
        let d = ceiling_plan(&s.model, 100000, 800, 600);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, d.view.px(P2(2000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "cut at 2300 mm, over the door");
        assert_eq!(pixel(&png, d.view.px(P2(8050.0, 3000.0))), (255, 255, 255), "the window's opening, cut");
        let (r, g, b) = pixel(&png, d.view.px(P2(4000.0, 1500.0)));
        assert!(r < 200 && g < 200 && b < 200, "a grid line: {:?}", (r, g, b));
        assert_eq!(d.pick(d.view.px(P2(4300.0, 1500.0)), 2.0), None, "the middle of a tile");
        assert!(d.owners.contains(&Some(100010)) && !d.owners.contains(&Some(100008)), "the ceiling; no room tag");
        assert!(!plan(&s.model, 100000, 800, 600).owners.contains(&Some(100010)), "a floor plan looks down");
        // A section cuts it.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
        let view = section_fit(&s.model, 100011, 800, 600);
        assert_eq!(section(&s.model, 100011, view).pick(view.px(P2(5000.0, 2610.0)), 2.0), Some(100010));
    }

    #[test]
    fn plans_draw_stairs_that_turn_ramps_and_railings() {
        let mut s = house();
        // A stair (11) turning right at a landing and its railings (12, 13); a ramp (14) turning back
        // at a landing; a railing (15) along a path.
        s.script(
            r#"stair {"start": [1000, 1000], "angle": 90, "landings": [{"at": 8, "turn": -90}]}
               railing {"host": 100010}
               ramp {"start": [-6000, -3000], "angle": 0, "height": 600, "landings": [{"at": 300, "turn": 180}]}
               railing {"path": [[0, 7000], [4000, 7000]]}"#,
        )
        .unwrap();
        let d = plan(&s.model, 100000, 800, 600);
        let at = |x, y| d.view.px(P2(x, y));
        assert_eq!(d.pick(at(2340.0, 3200.0), 2.0), Some(100010), "a riser of the run past the landing");
        assert_eq!(d.pick(at(550.0, 2000.0), 1.0), Some(100011), "the left railing, in from the stair's edge");
        assert_eq!(d.pick(at(-4200.0, -3600.0), 2.0), Some(100013), "the ramp's edge");
        assert_eq!(d.pick(at(2000.0, 7000.0), 2.0), Some(100014));
        let drawn = |d: &Drawing, id, f: &dyn Fn(&Prim) -> bool| d.prims.iter().zip(&d.owners).any(|(p, o)| *o == Some(id) && f(p));
        let arrow = |p: &Prim| matches!(p, Prim::Fill { color, .. } if *color == INK);
        assert!(drawn(&d, 100010, &arrow) && drawn(&d, 100013, &arrow), "walk lines ending in arrows");
        assert!(drawn(&d, 100010, &|p| matches!(p, Prim::Line { dash: Some(_), .. })), "above the cut plane, dashed");
        let up = plan(&s.model, 100001, 800, 600);
        assert!(up.owners.contains(&Some(100010)) && !up.owners.contains(&Some(100013)), "from above: the stair, not the ramp");
    }

    #[test]
    fn svg_and_png_are_well_formed() {
        let s = house();
        let d = plan(&s.model, 100000, 640, 480);
        let svg = svg(&d);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"), "{svg}");
        assert_eq!(svg.matches("<path").count(), d.prims.len());
        let pm = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
        assert_eq!((pm.width(), pm.height()), (640, 480));
    }

    /// How tall the lettering element `id` draws is, px: its tallest stroke.
    fn lettering(d: &Drawing, id: Id) -> f32 {
        (d.prims.iter().zip(&d.owners).zip(&d.parts))
            .filter(|((_, o), p)| **o == Some(id) && **p == Part::Text)
            .filter_map(|((prim, _), _)| match prim {
                Prim::Line { pts, .. } => {
                    Some(pts.iter().map(|p| p.1).fold(f32::MIN, f32::max) - pts.iter().map(|p| p.1).fold(f32::MAX, f32::min))
                }
                _ => None,
            })
            .fold(0.0, f32::max)
    }

    /// Eight by six metres of walls built of layers, with a door (7) and a window (8).
    fn layered_house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true, "type": "Exterior - 300mm"}
               door {"at": [2000, 0]}
               window {"at": [8000, 3000]}"#,
        )
        .unwrap();
        s
    }

    /// How wide the bubble element `id` draws is (its widest closed line), px.
    fn bubble(d: &Drawing, id: Id) -> f32 {
        (d.prims.iter().zip(&d.owners))
            .filter(|(_, o)| **o == Some(id))
            .filter_map(|(prim, _)| match prim {
                Prim::Line { pts, closed: true, .. } => {
                    Some(pts.iter().map(|p| p.0).fold(f32::MIN, f32::max) - pts.iter().map(|p| p.0).fold(f32::MAX, f32::min))
                }
                _ => None,
            })
            .fold(0.0, f32::max)
    }

    /// How far, px, the middle of the bubble element `id` draws is from the nearer of the line's `ends`.
    fn bubble_gap(d: &Drawing, id: Id, ends: [(f32, f32); 2]) -> f32 {
        (d.prims.iter().zip(&d.owners))
            .filter(|(_, o)| **o == Some(id))
            .filter_map(|(prim, _)| match prim {
                Prim::Line { pts, closed: true, .. } => {
                    let n = pts.len() as f32;
                    Some((pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n))
                }
                _ => None,
            })
            .flat_map(|c| ends.map(|e| (c.0 - e.0).hypot(c.1 - e.1)))
            .fold(f32::MAX, f32::min)
    }

    #[test]
    fn a_views_scale_sizes_its_annotation_as_paper_at_that_scale_holds_it() {
        let mut s = house();
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 50})).unwrap();
        // A tenth of a pixel to the mm: at 1 : 50, annotation is the size it has been all along (10 px lettering is
        // 2 mm of paper, DENSITY px to the mm).
        let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        let grid = |s: &Session, view: View| {
            let d = draw(&s.model, 100000, view);
            (lettering(&d, 100009), bubble(&d, 100009))
        };
        let (text, ring) = grid(&s, view);
        assert!((text - 10.0).abs() < 1.0 && (ring - 22.0).abs() < 1.0, "{text} {ring}");
        // The bubble stands where it did, its middle a bubble's radius, times k, past the grid's end.
        let ends = [view.px(P2(0.0, -2000.0)), view.px(P2(0.0, 8000.0))];
        assert!((bubble_gap(&draw(&s.model, 100000, view), 100009, ends) - 11.0).abs() < 0.5);
        for (n, k) in [(50, 1.0), (100, 2.0), (200, 4.0), (25, 0.5)] {
            s.execute("scale", &serde_json::json!({"view": 100000, "scale": n})).unwrap();
            let (t, r) = grid(&s, view);
            assert!((t - text * k).abs() < 1.0 && (r - ring * k).abs() < 1.0, "1 : {n}: {t} {r}");
            let gap = bubble_gap(&draw(&s.model, 100000, view), 100009, ends);
            assert!((gap - 11.0 * k).abs() < 0.5, "1 : {n}: the bubble grows about the grid's end: {gap}");
        }
        // Zoomed in twice, the same view's annotation is twice the size: a screen showing 1 mm as 0.2 px, at 1 : 100.
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
        let (t, _) = grid(&s, View { scale: 0.2, ..view });
        assert!((t - text * 4.0).abs() < 2.0, "{t}");
    }

    #[test]
    fn a_sections_scale_sizes_its_annotation_and_a_sheet_keeps_paper_size() {
        let mut s = house();
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
        // The section's own line, from its start, and up.
        let view = View { scale: 0.1, cx: 5000.0, cy: 1500.0, width: 1200, height: 800 };
        s.execute("scale", &serde_json::json!({"view": 100010, "scale": 50})).unwrap();
        let none = lettering(&section(&s.model, 100010, view), 100009);
        assert!((none - 10.0).abs() < 1.0, "{none}");
        s.execute("scale", &serde_json::json!({"view": 100010, "scale": 100})).unwrap();
        let hundred = lettering(&section(&s.model, 100010, view), 100009);
        assert!((hundred - 2.0 * none).abs() < 1.0, "{hundred}");
        // On paper, at the viewport's scale, annotation is paper size whatever the view's own says.
        s.script("sheet {\"name\": \"Plans\"}\nplace {\"sheet\": \"A101\", \"view\": 100000, \"scale\": 100}").unwrap();
        // The sheet is 12, its viewport 13.
        let before = png(&crate::sheet::draw(&s.model, 100011).unwrap()).unwrap();
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 20})).unwrap();
        assert_eq!(png(&crate::sheet::draw(&s.model, 100011).unwrap()).unwrap(), before);
    }

    /// How many of the hatching lines (thin), then of the lines between layers, element `id` draws.
    fn layer_detail(d: &Drawing, id: Id) -> (usize, usize) {
        let lines = |wide: bool| {
            (d.prims.iter().zip(&d.owners).zip(&d.parts))
                .filter(|((prim, o), p)| {
                    **o == Some(id)
                        && **p == Part::Pattern
                        && matches!(prim, Prim::Line { width, .. } if (*width >= d.width(Pen::PROJECTION) - 1e-4) == wide)
                })
                .count()
        };
        (lines(false), lines(true))
    }

    /// How many primitives element `id` draws.
    fn drawn_by(d: &Drawing, id: Id) -> usize {
        d.owners.iter().filter(|o| **o == Some(id)).count()
    }

    #[test]
    fn a_plans_detail_level_takes_patterns_then_layers_then_symbols_away() {
        let mut s = layered_house();
        s.script(
            r#"type {"category": "floor", "name": "Tiled", "layers": [{"material": "Ceramic tile", "thickness": 20}, {"material": "Concrete", "thickness": 180}]}
               floor {"type": "Tiled"}"#,
        )
        .unwrap();
        // Walls 3 to 6, the door 7, the window 8, the floor 9; layers 100 mm wide are 20 px, wide enough for their patterns.
        let view = View { scale: 0.2, cx: 4000.0, cy: 3000.0, width: 2000, height: 1400 };
        let all = |s: &Session| {
            let d = draw(&s.model, 100000, view);
            (layer_detail(&d, 100002), drawn_by(&d, 100006), drawn_by(&d, 100007), layer_detail(&d, 100008).0)
        };
        let (fine, fine_door, fine_window, fine_floor) = all(&s);
        assert!(fine.0 > 0 && fine.1 > 0 && fine_floor > 0, "{fine:?} {fine_floor}: its patterns, and the lines between layers");
        s.execute("detail", &serde_json::json!({"view": 100000, "detail": "medium"})).unwrap();
        // The layers stay, the patterns go.
        assert_eq!(all(&s), ((0, fine.1), fine_door, fine_window, 0));
        s.execute("detail", &serde_json::json!({"view": 100000, "detail": "coarse"})).unwrap();
        let (coarse, door, window, floor) = all(&s);
        assert_eq!((coarse, floor), ((0, 0), 0), "one poché");
        assert!(door < fine_door && window < fine_window, "an opening is its gap and jambs: {door} {window}");
        assert!(door >= 3 && window >= 3, "which are still there");
        // The ceiling plan is the same level's.
        assert!(drawn_by(&ceiling_plan(&s.model, 100000, 800, 600), 100002) > 0);
    }

    /// The extremes of what `d` draws, px: left, top, right, bottom.
    fn drawn_extent(d: &Drawing) -> [f32; 4] {
        let mut e = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let pts = d.prims.iter().flat_map(|p| match p {
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect::<Vec<_>>(),
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Image { .. } => Vec::new(),
        });
        for (x, y) in pts {
            e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
        }
        e
    }

    #[test]
    fn a_big_scale_leaves_room_round_a_view_for_the_annotation_it_makes_big() {
        let mut s = house();
        // The section is 11, the elevation 12.
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nelevation {\"facing\": \"south\"}").unwrap();
        let (w, h) = (1000, 800);
        let mut reached = Vec::new();
        for n in [10, 20, 100, 500, 5000] {
            for view in [100000, 100010, 100011] {
                s.execute("scale", &serde_json::json!({"view": view, "scale": n})).unwrap();
            }
            let (flat, ceiling) = (plan(&s.model, 100000, w, h), ceiling_plan(&s.model, 100000, w, h));
            let cut = |id| section(&s.model, id, section_fit(&s.model, id, w, h));
            for (what, d) in [("plan", flat), ("ceiling plan", ceiling), ("section", cut(11)), ("elevation", cut(12))] {
                let [l, t, r, b] = drawn_extent(&d);
                assert!(l >= 0.0 && t >= 0.0 && r <= w as f32 && b <= h as f32, "1 : {n}, {what}: {l} {t} {r} {b} in {w} x {h}");
                if what == "plan" {
                    reached.push((n, d.view.scale));
                }
            }
        }
        // The room is the annotation's, no more: a plan at 1 : 20 is as large as one at 1 : 10, whose annotation is smaller
        // than the least room, and at 1 : 500 not so small that the drawing is a dot.
        let at = |n| reached.iter().find(|(k, _)| *k == n).map(|(_, s)| *s).unwrap();
        assert_eq!(at(20), at(10));
        assert!(at(500) > at(10) / 4.0 && at(500) < at(10), "{reached:?}");
    }

    #[test]
    fn a_tools_sketch_of_annotation_is_the_size_its_view_would_draw_it() {
        let mut s = house();
        let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        let spot = Element::Spot(crate::model::Spot { view: s.model.guid(100000), at: P2(4000.0, 3000.0), host: None });
        let text = |s: &Session| lettering(&sketch(&s.model, view, 99, &spot), 99);
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 50})).unwrap();
        let none = text(&s);
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
        assert!((text(&s) - 2.0 * none).abs() < 1.0, "{none} {}", text(&s));
    }

    #[test]
    fn a_sections_detail_level_takes_the_same_away_of_what_it_cuts() {
        let mut s = layered_house();
        s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
        // The section is 9.
        let view = section_fit(&s.model, 100008, 1600, 1000);
        // Walls 6 and 4 are cut, west and east.
        let all = |s: &Session| layer_detail(&section(&s.model, 100008, view), 100005);
        let fine = all(&s);
        assert!(fine.0 > 0 && fine.1 > 0, "{fine:?}");
        s.execute("detail", &serde_json::json!({"view": "Section 1", "detail": "medium"})).unwrap();
        assert_eq!(all(&s), (0, fine.1));
        s.execute("detail", &serde_json::json!({"view": 100008, "detail": "coarse"})).unwrap();
        assert_eq!(all(&s), (0, 0));
    }

    /// The widest line wall `id` draws through `view`, px.
    fn widest(s: &Session, view: View, id: Id) -> f32 {
        let d = draw(&s.model, 100000, view);
        (d.prims.iter().zip(&d.owners))
            .filter_map(|(p, o)| match p {
                Prim::Line { width, .. } if *o == Some(id) => Some(*width),
                _ => None,
            })
            .fold(0.0, f32::max)
    }

    fn first_wall(s: &Session) -> Id {
        s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).unwrap()
    }

    #[test]
    fn a_line_is_its_pens_width_on_the_paper_its_view_stands_for() {
        let mut s = house();
        let wall = first_wall(&s);
        // A tenth of a pixel to the mm at 1 : 50 is 5 px to a mm of paper: a cut wall's outline, pen 10, is as wide as the
        // project's line weights say at 1 : 50.
        let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 50})).unwrap();
        let at_50 = widest(&s, view, wall);
        let mm = s.model.line_weights().at(50.0).mm(Pen::CUT, false) as f32;
        assert!((at_50 - mm * 5.0).abs() < 1e-4, "{at_50}");
        // At 1 : 100 the same screen stands for half the paper, so the pen is twice the pixels; zoomed in twice, twice again.
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
        assert!((widest(&s, view, wall) - 3.5).abs() < 1e-4, "pen 10, 0.35 mm: {}", widest(&s, view, wall));
        assert!((widest(&s, View { scale: 0.2, ..view }, wall) - 7.0).abs() < 1e-4);
        // However far in or out: a pixel to the mm is 20 times the paper (35 px), a 500th of one a 25th (0.07 px).
        assert!((widest(&s, View { scale: 1.0, ..view }, wall) - 35.0).abs() < 1e-3, "{}", widest(&s, View { scale: 1.0, ..view }, wall));
        assert!((widest(&s, View { scale: 0.002, ..view }, wall) - 0.07).abs() < 1e-5);
        // A weight overridden is a pen too.
        s.execute("graphics", &serde_json::json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 13}}}})).unwrap();
        assert!((widest(&s, view, wall) - 0.7 * 5.0 * 2.0).abs() < 1e-4, "pen 13, 0.7 mm: {}", widest(&s, view, wall));
    }

    #[test]
    fn pen_0_is_a_hairline_at_any_scale_and_zoom_and_still_prints() {
        let mut s = house();
        let wall = first_wall(&s);
        s.execute("graphics", &serde_json::json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 0}}}})).unwrap();
        for (n, zoom) in [(50, 0.1), (200, 0.1), (100, 0.5)] {
            s.execute("scale", &serde_json::json!({"view": 100000, "scale": n})).unwrap();
            let view = View { scale: zoom, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
            assert_eq!(widest(&s, view, wall), 0.0, "1 : {n} at {zoom}");
        }
        // Width 0 is the thinnest line the device draws: the PNG draws it, the SVG a pixel that zooming doesn't widen, the
        // PDF its own hairline (`0 w`).
        let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        let d = draw(&s.model, 100000, view);
        let img = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
        // The north wall, 200 mm thick on its axis: its outside face.
        let (x, y) = view.px(P2(5000.0, 6100.0));
        let dark = (y as u32 - 3..y as u32 + 3).filter_map(|y| img.pixel(x as u32, y)).filter(|c| c.red() < 128).count();
        assert!(dark >= 1, "the wall's outside face is drawn");
        assert!(svg(&d).contains(r#"stroke-width="1" vector-effect="non-scaling-stroke""#));
        assert!(crate::pdf::contents(&d, P2(200.0, 160.0)).0.contains(" 0 w "));
    }

    #[test]
    fn a_view_always_has_a_scale_and_one_without_its_own_is_at_1_100() {
        let mut s = house();
        let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        let none = png(&draw(&s.model, 100000, view)).unwrap();
        assert_eq!(s.execute("scale", &serde_json::json!({"view": 100000})).unwrap()["scale"], 100.0);
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
        assert_eq!(png(&draw(&s.model, 100000, view)).unwrap(), none);
        let e = s.execute("scale", &serde_json::json!({"view": 100000, "scale": null})).unwrap_err();
        assert!(e.contains("always has a scale"), "{e}");
    }

    #[test]
    fn a_cut_walls_outline_is_inked_over_its_poche_and_not_across_its_joins() {
        let s = house();
        // At 1 : 100 a cut outline (pen 10, 0.35 mm on paper) is 35 mm of the model wide; at a fifth of a pixel to the mm, 7 px.
        let view = View { scale: 0.2, cx: 6000.0, cy: 1000.0, width: 1000, height: 800 };
        let d = draw(&s.model, 100000, view);
        let png = png(&d).unwrap();
        // Two pixels inside the south wall's outer face (its axis on y = 0, 200 mm thick): the outline, whole.
        assert_eq!(pixel(&png, view.px(P2(6000.0, -90.0))), (INK.0, INK.1, INK.2), "the outline over the poché");
        assert_eq!(pixel(&png, view.px(P2(6000.0, 90.0))), (INK.0, INK.1, INK.2), "and on its inner face");
        // Where two walls meet at a corner, the mitre between them is poché, no line.
        assert_eq!(pixel(&png, view.px(P2(8000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2));
        assert_eq!(pixel(&png, view.px(P2(8040.0, 40.0))), (POCHE.0, POCHE.1, POCHE.2));
    }

    #[test]
    fn lettering_turns_its_corners_round_while_a_pens_lines_turn_them_square() {
        let view = View { scale: 1.0, cx: 0.0, cy: 0.0, width: 400, height: 200 };
        let mut d = Drawing::new(view);
        d.text("VW", (200.0, 100.0), 80.0, INK);
        d.line(&[P2(-150.0, 0.0), P2(-100.0, 50.0), P2(-50.0, 0.0)], Pen::MEDIUM, INK, false);
        let svg = svg(&d);
        let joins: Vec<&str> = svg.split("stroke-linejoin=\"").skip(1).filter_map(|s| s.split('"').next()).collect();
        assert_eq!(joins.last(), Some(&"miter"), "the pen's line");
        assert!(joins.len() > 2 && joins[..joins.len() - 1].iter().all(|j| *j == "round"), "{joins:?}");
        let (pdf, _) = crate::pdf::contents(&d, P2(100.0, 50.0));
        assert!(pdf.contains(" 1 j ") && pdf.trim_end().rsplit('\n').next().is_some_and(|l| l.contains(" 0 j ")), "{pdf}");
    }

    #[test]
    fn a_wall_on_its_own_is_inked_all_round_as_one_closed_line_square_at_every_corner() {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0, 0], [4000, 0]]}"#).unwrap();
        // Half a pixel to the mm: the outline, 35 mm of the model wide at 1 : 100, reaches 17.5 mm past each corner, so 15 mm
        // out each way is ink at a square corner and paper at a round one (21 mm from it).
        let view = View { scale: 0.5, cx: 2000.0, cy: 0.0, width: 2400, height: 400 };
        let d = draw(&s.model, 100000, view);
        let cut: Vec<bool> = d
            .prims
            .iter()
            .zip(&d.parts)
            .filter_map(|(p, part)| match p {
                Prim::Line { closed, .. } if *part == Part::Cut => Some(*closed),
                _ => None,
            })
            .collect();
        assert_eq!(cut, [true], "one closed line, no ends");
        let png = png(&d).unwrap();
        for (x, y) in [(-15.0, -115.0), (4015.0, -115.0), (4015.0, 115.0), (-15.0, 115.0)] {
            assert_eq!(pixel(&png, view.px(P2(x, y))), (INK.0, INK.1, INK.2), "the corner by ({x}, {y}), square");
        }
    }

    #[test]
    fn where_two_walls_cut_outlines_meet_at_a_corner_they_join_square_with_no_notch() {
        let s = house();
        // The house's south-east corner: the south wall's axis on y = 0, the east's on x = 8000, both 200 mm thick, so the
        // outer corner is at (8100, -100). The outline, 35 mm of the model wide at 1 : 100, reaches 17.5 mm past it each way,
        // a square corner: 15 mm out each way is inside that square, outside a rounded one (21 mm from the corner, past its
        // 17.5 mm radius). Half a pixel to the mm, to see it.
        let view = View { scale: 0.5, cx: 8000.0, cy: 0.0, width: 1000, height: 800 };
        let d = draw(&s.model, 100000, view);
        let png = png(&d).unwrap();
        assert_eq!(pixel(&png, view.px(P2(8115.0, -115.0))), (INK.0, INK.1, INK.2), "the outer corner, square");
        // Each wall's run goes on round the corner into the other's: one line, joined, not two ends meeting.
        let corner = view.px(P2(8100.0, -100.0));
        let through = d.prims.iter().zip(&d.parts).any(|(p, part)| match p {
            Prim::Line { pts, .. } if *part == Part::Cut => {
                pts.len() > 2 && pts[1..pts.len() - 1].iter().any(|q| (q.0 - corner.0).abs() < 0.01 && (q.1 - corner.1).abs() < 0.01)
            }
            _ => false,
        });
        assert!(through, "a run turns the corner");
    }
}
