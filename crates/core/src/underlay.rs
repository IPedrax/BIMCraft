//! Underlays, as Archicad's Trace & Reference: another view drawn under (or over) the one on screen, in one colour, as its
//! own settings draw it (its view range, its graphics, its crop), never picked, only snapped to. Any plan, section or
//! elevation may show any other: a plan of the floor below, the plan in a section, an elevation in another. They are fitted
//! by the internal origin, and may be moved and turned about it; what a view shows under it is the view's own, saved with it
//! ([`Underlay`], each view's `underlay`). The desktop makes it away from the interface's thread, after the view itself.

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::graphics;
use crate::model::{Element, Guid, Id, Key, MAX_COORD, Model};
use crate::render::{self, Drawing, Part, Prim, Rgb, View};
use crate::view3d::{Image, MAX_SIDE};

/// A picture an underlay holds: a section's or an elevation's, in its colour, row by row red, green, blue and alpha.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The colour an underlay is drawn in until it is given another: an orange that reads on white and on a dark canvas.
pub const COLOR: [u8; 3] = [230, 126, 34];
/// How far an underlay's fills lie toward the paper: a light wash of its colour, so its lines read over them.
const WASH: f64 = 0.7;

/// What a view shows under it: another view (`view`, by GUID), in `color`, its fills or not, its annotation or not, behind
/// what the view draws or in `front` of it, moved by `offset` (mm, in the view's own axes: a plan's east and north, a
/// section's along and up) and turned by `angle` degrees counter-clockwise about where the internal origin falls in the
/// view. Kept while it is off (`on`), so a view turns it back on as it was.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Underlay {
    pub view: Guid,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub on: bool,
    #[serde(default = "color", skip_serializing_if = "is_color")]
    pub color: [u8; 3],
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub fills: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub hide_annotation: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub front: bool,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub offset: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
}

fn yes() -> bool {
    true
}
fn is_yes(b: &bool) -> bool {
    *b
}
fn color() -> [u8; 3] {
    COLOR
}
fn is_color(c: &[u8; 3]) -> bool {
    *c == COLOR
}
fn is_origin(p: &P2) -> bool {
    *p == P2(0.0, 0.0)
}
fn is_zero(a: &f64) -> bool {
    *a == 0.0
}

impl Underlay {
    /// View `view` shown under another, as an underlay starts: on, in [`COLOR`], with its fills and without its annotation,
    /// behind, where the internal origin puts it.
    pub fn of(view: Guid) -> Underlay {
        Underlay { view, on: true, color: COLOR, fills: true, hide_annotation: true, front: false, offset: P2(0.0, 0.0), angle: 0.0 }
    }

    /// Where it is put back to by Reset Position: no offset, no turn.
    pub fn placed(&self) -> bool {
        is_origin(&self.offset) && self.angle == 0.0
    }

    /// Its colour, as the drawing takes it.
    pub fn rgb(&self) -> Rgb {
        Rgb(self.color[0], self.color[1], self.color[2])
    }

    /// Turned by `by` degrees counter-clockwise about `about` (the view's own coordinates): where its points went, they turn
    /// on with them.
    pub fn turned(&mut self, by: f64, about: P2) {
        let (sin, cos) = by.to_radians().sin_cos();
        let o = self.offset - about;
        self.offset = about + P2(o.0 * cos - o.1 * sin, o.0 * sin + o.1 * cos);
        self.angle = normal(self.angle + by);
    }
}

/// `a` degrees in (-180, 180], rounded to a millionth, so turning by and back comes home.
pub fn normal(a: f64) -> f64 {
    let a = (a + 180.0).rem_euclid(360.0) - 180.0;
    let a = if a <= -180.0 { a + 360.0 } else { a };
    (a * 1e6).round() / 1e6
}

/// What kind of view `id` is, as an underlay sees it: a plan (a level's own or a plan view), or a section or an elevation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plan,
    Section,
}

/// The kind of view `id` is, none for one that can't show an underlay or be one (a 3D view, a schedule, a sheet).
pub fn kind(m: &Model, id: impl Key) -> Option<Kind> {
    match m.elements.get(id)? {
        Element::Level(_) | Element::PlanView(_) => Some(Kind::Plan),
        Element::Section(_) | Element::Elevation(_) => Some(Kind::Section),
        _ => None,
    }
}

/// The underlay view `id` holds, on or off.
pub fn of(m: &Model, id: impl Key) -> Option<&Underlay> {
    of_element(m.elements.get(id)?)
}

/// The underlay element `e` holds, when it is a view that does.
pub fn of_element(e: &Element) -> Option<&Underlay> {
    match e {
        Element::Level(l) => l.underlay.as_ref(),
        Element::PlanView(v) => v.underlay.as_ref(),
        Element::Section(v) | Element::Elevation(v) => v.underlay.as_ref(),
        _ => None,
    }
}

/// The slot of view `e`'s underlay; none for an element that isn't a plan, a section or an elevation.
pub fn slot(e: &mut Element) -> Option<&mut Option<Underlay>> {
    match e {
        Element::Level(l) => Some(&mut l.underlay),
        Element::PlanView(v) => Some(&mut v.underlay),
        Element::Section(v) | Element::Elevation(v) => Some(&mut v.underlay),
        _ => None,
    }
}

/// The underlay view `id` shows now: on, of a view that is there and can be one, and not of itself. Deleting a view takes
/// the underlays of it off the views that showed it; one that names a view not there all the same (a file edited by hand)
/// shows none, and says so ([`missing`]).
pub fn shown(m: &Model, id: impl Key) -> Option<&Underlay> {
    let u = of(m, id).filter(|u| u.on)?;
    let me = m.elements.id(id)?;
    let source = m.elements.id(u.view).filter(|s| *s != me)?;
    kind(m, source)?;
    Some(u)
}

/// Whether view `id`'s underlay names a view that isn't there any more.
pub fn missing(m: &Model, id: impl Key) -> bool {
    of(m, id).is_some_and(|u| m.elements.id(u.view).is_none_or(|s| kind(m, s).is_none()))
}

/// What `id`'s underlay may be of: every plan, section and elevation but itself, by id.
pub fn candidates(m: &Model, id: impl Key) -> Vec<Id> {
    let me = m.elements.id(id);
    m.views().into_iter().filter(|v| Some(*v) != me && kind(m, *v).is_some()).collect()
}

/// The view an underlay of plan `id` starts with: the level below its own, else the one above.
pub fn default_source(m: &Model, id: impl Key) -> Option<Id> {
    let level = m.plan_level(id)?;
    let z = m.level(level)?.elevation;
    let mut levels: Vec<(Id, f64)> = m.lookup().levels().filter_map(|l| Some((l, m.level(l)?.elevation))).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    let below = levels.iter().rev().find(|(l, e)| *l != level && *e < z);
    let above = levels.iter().find(|(l, e)| *l != level && *e >= z);
    below.or(above).map(|(l, _)| *l)
}

/// Where the internal origin falls in view `id`'s own coordinates: a plan's are the model's; a section's run along its line
/// from its start, and up from 0.
pub fn origin(m: &Model, id: impl Key) -> Option<P2> {
    match kind(m, id)? {
        Kind::Plan => Some(P2(0.0, 0.0)),
        Kind::Section => {
            let v = m.section(id)?;
            let u = (v.end - v.start).unit();
            Some(P2((P2(0.0, 0.0) - v.start).dot(u), 0.0))
        }
    }
}

fn rotate(p: P2, deg: f64) -> P2 {
    if deg == 0.0 {
        return p;
    }
    let (sin, cos) = deg.to_radians().sin_cos();
    P2(p.0 * cos - p.1 * sin, p.0 * sin + p.1 * cos)
}

/// A point of the underlay's view, in its own coordinates, where it falls in view `target`'s: the internal origins on each
/// other, then turned and moved as `u` says.
pub fn place(m: &Model, target: impl Key, u: &Underlay, p: P2) -> Option<P2> {
    let (from, to) = (origin(m, u.view)?, origin(m, target)?);
    Some(to + u.offset + rotate(p - from, u.angle))
}

/// The view through which the underlay's own view draws so that it falls where `view` (the target's) shows it: as large, as
/// near, turned by the target's turn and the underlay's.
pub fn source_view(m: &Model, target: impl Key, u: &Underlay, view: View) -> Option<View> {
    let (from, to) = (origin(m, u.view)?, origin(m, target)?);
    let c = P2(view.cx, view.cy);
    let cs = from + rotate(c - to - u.offset, -u.angle);
    Some(View { cx: cs.0, cy: cs.1, angle: view.angle + u.angle, ..view })
}

/// An underlay as it is drawn: its lines and fills, in the target view's pixels, and the picture of what a section or an
/// elevation sees past its line, in its colour and clear elsewhere, with its top left corner.
pub struct Underlaid {
    pub drawing: Drawing,
    pub picture: Option<((f32, f32), Picture)>,
    /// Whether it is drawn over the view rather than under it.
    pub front: bool,
}

impl Underlaid {
    /// One drawing of both, the picture first: what a PNG of it shows.
    pub fn into_drawing(self) -> Drawing {
        let Underlaid { drawing, picture, .. } = self;
        let Some((at, image)) = picture else { return drawing };
        let mut d = Drawing::new(drawing.view);
        d.prims.push(Prim::Image { at, width: image.width, height: image.height, rgba: image.rgba, ink: true });
        d.owners.push(None);
        d.parts.push(Part::Projection);
        d.pens.push(None);
        d.prims.extend(drawing.prims);
        d.owners.extend(drawing.owners);
        d.parts.extend(drawing.parts);
        d.pens.extend(drawing.pens);
        d
    }

    /// Its lines, as segments in the target view's own coordinates, mm: what snapping finds on it.
    pub fn segments(&self) -> Vec<(P2, P2)> {
        segments(&self.drawing)
    }
}

/// The straight pieces of `d`'s lines, mm of its view: what a snap finds on an underlay. Hatching and lettering aren't snapped
/// to.
pub fn segments(d: &Drawing) -> Vec<(P2, P2)> {
    let mut out = Vec::new();
    for (prim, part) in d.prims.iter().zip(&d.parts) {
        let Prim::Line { pts, closed, .. } = prim else { continue };
        if !matches!(part, Part::Projection | Part::Cut) {
            continue;
        }
        let mm: Vec<P2> = pts.iter().map(|q| micron(d.view.mm(*q))).collect();
        out.extend(mm.windows(2).map(|w| (w[0], w[1])));
        if *closed
            && mm.len() > 2
            && let (Some(a), Some(b)) = (mm.last(), mm.first())
        {
            out.push((*a, *b));
        }
    }
    out.retain(|(a, b)| a.dist(*b) > 1e-6 && a.is_finite() && b.is_finite());
    out
}

fn micron(p: P2) -> P2 {
    P2((p.0 * 1e3).round() / 1e3, (p.1 * 1e3).round() / 1e3)
}

/// View `target`'s underlay through `view`, as [`draw_cancellable`] draws it.
pub fn draw(m: &Model, target: impl Key, view: View) -> Option<Underlaid> {
    draw_cancellable(m, None, target, view, &|| false).flatten()
}

/// View `target`'s underlay through `view` (the target's own framing), none when it shows none; the outer none when
/// `cancelled`. A section's or an elevation's is drawn from `work`, when it is the model's (else from work made here).
pub fn draw_cancellable(
    m: &Model,
    work: Option<&render::SectionWork>,
    target: impl Key,
    view: View,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Option<Underlaid>> {
    let Some(u) = shown(m, target) else { return Some(None) };
    let Some(source) = m.elements.id(u.view) else { return Some(None) };
    let Some(through) = source_view(m, target, u, view) else { return Some(None) };
    let (raw, picture) = match kind(m, source) {
        Some(Kind::Plan) => (render::draw(m, source, through), None),
        Some(Kind::Section) => {
            // A section draws square to its own axes: drawn so, over a square that still covers the view once turned, then
            // turned.
            let turn = through.angle;
            let side = if turn == 0.0 {
                None
            } else {
                let (w, h) = (f64::from(view.width), f64::from(view.height));
                Some(((w * w + h * h).sqrt().ceil() as u32).clamp(1, MAX_SIDE))
            };
            let upright = match side {
                Some(s) => View { angle: 0.0, width: s, height: s, ..through },
                None => View { angle: 0.0, ..through },
            };
            let fresh;
            let work = match work {
                Some(w) => w,
                None => {
                    fresh = render::SectionWork::new_deferred_cancellable(m, cancelled)?;
                    &fresh
                }
            };
            let overlay = render::SectionOverlay::new_cancellable(m, work, source, upright, cancelled)?;
            let picture = render::section_picture_cancellable(m, work, source, upright, &[], cancelled);
            if cancelled() {
                return None;
            }
            let drawing = overlay.draw_cancellable(m, picture.as_ref(), cancelled)?;
            let shift =
                ((f64::from(view.width) - f64::from(upright.width)) / 2.0, (f64::from(view.height) - f64::from(upright.height)) / 2.0);
            let mid = (f64::from(upright.width) / 2.0, f64::from(upright.height) / 2.0);
            let to_view = |(x, y): (f32, f32)| {
                let (dx, dy) = (f64::from(x) - mid.0, f64::from(y) - mid.1);
                let (sin, cos) = turn.to_radians().sin_cos();
                let (rx, ry) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                ((mid.0 + shift.0 + rx) as f32, (mid.1 + shift.1 + ry) as f32)
            };
            let mut d = Drawing::new(view);
            for (((prim, owner), part), pen) in drawing.prims.into_iter().zip(drawing.owners).zip(drawing.parts).zip(drawing.pens) {
                let mut prim = prim;
                if matches!(prim, Prim::Image { .. }) {
                    continue;
                }
                if let Prim::Text { frame, .. } = &mut prim {
                    frame.at = to_view(frame.at);
                    frame.angle -= turn.to_radians() as f32;
                }
                for p in prim.paint_mut() {
                    match p {
                        Prim::Fill { rings, .. } => rings.iter_mut().flatten().for_each(|p| *p = to_view(*p)),
                        Prim::Line { pts, .. } => pts.iter_mut().for_each(|p| *p = to_view(*p)),
                        _ => {}
                    }
                }
                d.prims.push(prim);
                d.owners.push(owner);
                d.parts.push(part);
                d.pens.push(pen);
            }
            let picture = picture.map(|p| {
                let tinted = tint(&p, u.rgb());
                if turn == 0.0 { ((0.0, 0.0), tinted) } else { turned(&tinted, turn, view) }
            });
            (d, picture)
        }
        None => return Some(None),
    };
    if cancelled() {
        return None;
    }
    let mut drawing = restyle(m, u, raw, view);
    drawing = cropped_to(m, target, drawing);
    Some(Some(Underlaid { drawing, picture, front: u.front }))
}

/// What of `raw` an underlay shows, in its colour, owned by nothing (so never picked), through `view`.
fn restyle(m: &Model, u: &Underlay, raw: Drawing, view: View) -> Drawing {
    let ink = u.rgb();
    let wash = |c: u8| (f64::from(c) + (255.0 - f64::from(c)) * WASH).round() as u8;
    let fill = Rgb(wash(ink.0), wash(ink.1), wash(ink.2));
    let annotation = |id: Option<Id>| {
        id.and_then(|id| m.elements.get(id)).is_some_and(|e| {
            let c = e.category();
            graphics::ANNOTATION.contains(&c) || matches!(c, "detailgroup" | "repeatingdetail" | "viewport")
        })
    };
    let mut d = Drawing::new(view);
    for ((prim, owner), part) in raw.prims.into_iter().zip(raw.owners).zip(raw.parts) {
        let text = part == Part::Text;
        if u.hide_annotation && (text || annotation(owner)) {
            continue;
        }
        let pattern = matches!(part, Part::Pattern | Part::LinePattern);
        let prim = match prim {
            Prim::Text { frame, layout, mut fallback } => {
                for p in &mut fallback {
                    if let Prim::Fill { color, .. } | Prim::Line { color, .. } = p {
                        *color = ink;
                    }
                }
                Prim::Text { frame, layout, fallback }
            }
            Prim::Fill { rings, .. } if text => Prim::Fill { rings, color: ink },
            Prim::Fill { rings, .. } if u.fills && pattern => Prim::Fill { rings, color: fill },
            Prim::Fill { rings, .. } if u.fills => Prim::Fill { rings, color: fill },
            Prim::Fill { .. } => continue,
            Prim::Line { .. } if pattern && !u.fills => continue,
            Prim::Line { pts, width, dash, closed, .. } => Prim::Line { pts, width, color: if pattern { fill } else { ink }, dash, closed },
            // A scan's picture: what it shows is the scan's, not the view's.
            Prim::Image { .. } => continue,
        };
        d.prims.push(prim);
        d.owners.push(None);
        d.parts.push(part);
        d.pens.push(None);
    }
    d
}

/// `d` cut to the crop region of view `target`, when it is cropped and squarely on screen: an underlay shows only what the
/// view does.
fn cropped_to(m: &Model, target: impl Key, d: Drawing) -> Drawing {
    let Some([lo, hi]) = (match kind(m, target) {
        Some(Kind::Plan) => m.shown_crop(target),
        Some(Kind::Section) => render::crop_region(m, target),
        None => None,
    }) else {
        return d;
    };
    if d.view.angle != 0.0 {
        return d;
    }
    let (a, b) = (d.view.px(lo), d.view.px(hi));
    let (plo, phi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
    let mut out = Drawing::new(d.view);
    for (((prim, owner), part), pen) in d.prims.into_iter().zip(d.owners).zip(d.parts).zip(d.pens) {
        for piece in prim.cropped(plo, phi) {
            out.prims.push(piece);
            out.owners.push(owner);
            out.parts.push(part);
            out.pens.push(pen);
        }
    }
    out
}

/// A section's picture in one colour: its ink, as dark as each pixel was, and clear where it showed paper.
fn tint(p: &Image, ink: Rgb) -> Picture {
    let mut rgba = Vec::with_capacity(p.rgba.len());
    for px in p.rgba.as_chunks::<4>().0 {
        let dark = 255 - px[0].min(px[1]).min(px[2]);
        // What is all but paper is paper: the faint shading of surfaces would wash the whole picture over.
        let alpha = if dark < 24 { 0 } else { dark.saturating_add(dark / 2).max(96) };
        rgba.extend_from_slice(&[ink.0, ink.1, ink.2, alpha]);
    }
    Picture { width: p.width, height: p.height, rgba }
}

/// Picture `p`, drawn square to its axes over a square centred on `view`, turned `turn` degrees counter-clockwise about its
/// middle and cut to `view`: each of the view's pixels takes the one it came from.
fn turned(p: &Picture, turn: f64, view: View) -> ((f32, f32), Picture) {
    let (w, h) = (view.width.min(MAX_SIDE), view.height.min(MAX_SIDE));
    let (sin, cos) = turn.to_radians().sin_cos();
    let (mid_v, mid_p) = ((f64::from(w) / 2.0, f64::from(h) / 2.0), (f64::from(p.width) / 2.0, f64::from(p.height) / 2.0));
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (f64::from(x) + 0.5 - mid_v.0, f64::from(y) + 0.5 - mid_v.1);
            // The inverse of the turn on screen.
            let (sx, sy) = (dx * cos - dy * sin + mid_p.0, dx * sin + dy * cos + mid_p.1);
            if sx < 0.0 || sy < 0.0 || sx >= f64::from(p.width) || sy >= f64::from(p.height) {
                continue;
            }
            let from = (sy as usize * p.width as usize + sx as usize) * 4;
            let to = (y as usize * w as usize + x as usize) * 4;
            if let (Some(src), Some(dst)) = (p.rgba.get(from..from + 4), rgba.get_mut(to..to + 4)) {
                dst.copy_from_slice(src);
            }
        }
    }
    ((0.0, 0.0), Picture { width: w, height: h, rgba })
}

/// Why `u`, held by view `target`, can't be: a view that can't hold one, a view that isn't one to show, itself, a colour or a
/// place that isn't a number or lies out of reach.
pub fn check(m: &Model, target: Id, u: &Underlay) -> Result<(), String> {
    let name = m.view_name(target).unwrap_or_default().to_string();
    if !u.offset.is_finite() || u.offset.0.abs() > MAX_COORD || u.offset.1.abs() > MAX_COORD {
        return Err(format!("view `{name}`: its underlay's offset is out of reach (within {MAX_COORD} mm each way)"));
    }
    if !u.angle.is_finite() {
        return Err(format!("view `{name}`: its underlay's angle isn't a number"));
    }
    if let Some(source) = m.elements.id(u.view) {
        if source == target {
            return Err(format!("view `{name}` can't be its own underlay"));
        }
        if kind(m, source).is_none() {
            return Err(format!("view `{name}`: its underlay, {source}, isn't a plan, a section or an elevation"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod text_carrier_tests {
    use super::*;

    #[test]
    fn text_carrier_underlay_keeps_paint_and_hides_annotation() {
        let m = crate::engine::Session::new().model;
        let view = View { scale: 1.0, cx: 0.0, cy: 0.0, width: 200, height: 100, angle: 0.0 };
        let layout = crate::text::layout(&crate::text::TextType::default(), "Notes", &[]);
        let make = |wrapped: bool| {
            let mut d = Drawing::new(view);
            render::lettering(
                &mut d,
                &layout,
                render::NoteFrame { at: (10.0, 10.0), k: 5.0, angle: 0.2 },
                Rgb(COLOR[0], COLOR[1], COLOR[2]),
            );
            if !wrapped {
                d.prims = d.prims.iter().flat_map(Prim::paint).cloned().collect();
                d.parts = vec![Part::Text; d.prims.len()];
                d.owners = vec![None; d.prims.len()];
            }
            d
        };
        let mut u = Underlay { fills: false, hide_annotation: false, ..Underlay::of(m.guid(100000)) };
        assert_eq!(render::svg(&restyle(&m, &u, make(true), view)), render::svg(&restyle(&m, &u, make(false), view)));
        u.hide_annotation = true;
        assert!(restyle(&m, &u, make(true), view).prims.is_empty());
    }
}
