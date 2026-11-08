//! F7a: a family's dimensions in its plans and elevations, as Revit's family editor has them. Aligned Dimension (DI) picks
//! reference planes and extrusion ends one after another and a click on nothing places the string; a dimension selected shows
//! a padlock under each segment and, with two or more, the EQ toggle over it, and the Options Bar's Label gives it a length
//! parameter (or Create Parameter…). Align (AL) moves what is picked second onto what is picked first and leaves a padlock,
//! which locks them. Flexing a type solves them (`bimcraft_core::family::constraints`); what can't hold says "Constraints are
//! not satisfied", with Remove Constraints or Cancel; a label, lock or EQ that adds nothing to the others is refused as
//! over-constrained. The marks are the core's (`constraints::marks`), painted over the view's picture. Angular Dimension
//! (F7c) picks lines that cross (reference lines, plane traces, lines of forms) and a click on nothing places its arc; its
//! label is an Angle parameter.
use super::views::{PICKED, Views};
use super::*;
pub(super) use bimcraft_core::family::constraints::DimMark;
use bimcraft_core::family::constraints::{self, Edit, NOT_SATISFIED, Unsat};
use bimcraft_core::family::views::{Staged, Which};
use bimcraft_core::family::{DimKind, DimRef, DimView, FamilyDimension, Spot, Val};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::Lock;
use bimcraft_core::render::View;
use bimcraft_core::units::Units;
use egui::{Painter, Pos2, Vec2};

/// What T19 says of a label, lock or EQ that adds nothing to the constraints there are.
pub(crate) const OVER: &str = msg!("The dimension is over-constrained");
/// The Options Bar's label for no label, and its entry for a new parameter.
const NONE: &str = msg!("<None>");
const CREATE: &str = msg!("Create Parameter…");
/// Revit's title for what can't hold (the core says it in the same words, `NOT_SATISFIED`).
const UNSATISFIED: &str = msg!("Constraints are not satisfied");

/// How near a click must be to what it picks, in pixels.
const NEAR: f32 = 7.0;

/// "Constraints are not satisfied": why, the dimensions in it (by GUID), and what was being done, which Remove Constraints
/// does again once they are taken off.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Unsatisfied {
    pub(crate) message: String,
    pub(crate) dims: Vec<Guid>,
    pub(crate) retry: Retry,
}

/// What was being done when the constraints couldn't hold.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Retry {
    /// A plane dragged to an offset along its normal.
    Drag(Guid, f64),
    /// Align, of the second reference onto the first.
    Align(DimRef, DimRef),
    /// Family Types' Apply (or OK, which closes it).
    Types { ok: bool },
}

/// Create Parameter…, for the selected dimension's label: the Parameter Properties a label needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Creating {
    dim: Guid,
    name: String,
    instance: bool,
    reporting: bool,
    error: Option<String>,
}

/// What a click on the dimensions asked, done once the view is drawn.
pub(super) enum Clicked {
    Select(Guid),
    Lock(Guid, usize),
    Equal(Guid),
    Unlock(Guid),
    AlignLock,
}

/// An arc's start angle and its sweep, on screen (radians).
type Sweep = (f32, f32);

/// An angular dimension's arc on screen: where its lines cross, its radius in pixels, and each segment's start angle and
/// sweep (screen radians, y down), the short way round.
fn arcs(m: &DimMark, screen: &dyn Fn(P2) -> Pos2) -> Option<(Pos2, f32, Vec<Sweep>)> {
    let v = screen(*m.points.first()?);
    let rays: Vec<Pos2> = m.points.iter().skip(1).map(|p| screen(*p)).collect();
    let r = (*rays.first()? - v).length();
    if !(r > 1.0 && r.is_finite()) {
        return None;
    }
    let angle = |p: Pos2| (p.y - v.y).atan2(p.x - v.x);
    let tau = std::f32::consts::TAU;
    let segs = rays
        .windows(2)
        .map(|w| {
            let (a, b) = (angle(w[0]), angle(w[1]));
            let mut sweep = (b - a).rem_euclid(tau);
            if sweep > std::f32::consts::PI {
                sweep -= tau;
            }
            (a, sweep)
        })
        .collect();
    Some((v, r, segs))
}

/// The point at `angle` (screen radians), `r` pixels from `v`.
fn polar(v: Pos2, r: f32, angle: f32) -> Pos2 {
    v + vec2(angle.cos(), angle.sin()) * r
}

/// What an angle reads: degrees, to two places at most.
fn degrees(a: f64) -> String {
    let s = format!("{a:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}°")
}

/// Where a selected dimension's segment's padlock and its EQ toggle are, on screen.
fn lock_at(m: &DimMark, i: usize, screen: &dyn Fn(P2) -> Pos2) -> Option<Pos2> {
    // An angle's: inside its arc, on its middle, clear of the lines either side.
    if m.kind == DimKind::Angular {
        let (v, r, segs) = arcs(m, screen)?;
        let (a, sweep) = *segs.get(i)?;
        return Some(polar(v, (r - 20.0).max(r * 0.55), a + sweep / 2.0));
    }
    let (a, b) = (screen(*m.points.get(i)?), screen(*m.points.get(i + 1)?));
    // A radius's or a diameter's: beside its leader past the circle, clear of it.
    if m.kind != DimKind::Aligned {
        let along = (b - a).normalized();
        return along.is_finite().then(|| b + along * 9.0 + down(a, b) * 12.0);
    }
    Some(a + (b - a) * 0.5 + down(a, b) * 16.0)
}

fn equal_at(m: &DimMark, screen: &dyn Fn(P2) -> Pos2) -> Option<Pos2> {
    // An angular string's: past its first end, on round its arc.
    if m.kind == DimKind::Angular {
        let (v, r, segs) = arcs(m, screen)?;
        let (a, sweep) = *segs.first()?;
        let back = (22.0 + 16.0) / r;
        return Some(polar(v, r, a - back * sweep.signum()));
    }
    let (a, b) = (screen(*m.points.first()?), screen(*m.points.last()?));
    Some(a + (b - a) * 0.5 - down(a, b) * 30.0)
}

/// The room a padlock centred at `c` takes on screen, shut or open (its shackle lifted), with a hair round it.
pub(crate) fn lock_rect(c: Pos2) -> Rect {
    Rect::from_min_max(pos2(c.x - 7.0, c.y - 12.0), pos2(c.x + 7.0, c.y + 8.0))
}

/// Whether the segment from `a` to `b` passes through `r` (Liang-Barsky).
pub(crate) fn crosses(a: Pos2, b: Pos2, r: Rect) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x - r.min.x), (d.x, r.max.x - a.x), (-d.y, a.y - r.min.y), (d.y, r.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Where an Align padlock goes on screen: beside the aligned line through `base` (running along `dir`), a few pixels off it
/// to its outer side (away from what `inside` says is a form's inside, where one is), slid along the line as little as
/// takes it clear of every line of `lines` and of the padlocks `taken`; on the other side only where the outer one has no
/// room. So a padlock never covers a line, the one it locks among them, or another padlock.
pub(super) fn padlock_spot(base: Pos2, dir: Vec2, inside: &dyn Fn(Pos2) -> bool, lines: &[[Pos2; 2]], taken: &[Pos2]) -> Pos2 {
    let dir = if dir.is_finite() && dir.length_sq() > 0.0 { dir.normalized() } else { vec2(1.0, 0.0) };
    let n = vec2(-dir.y, dir.x);
    let out = if inside(base + n * 6.0) && !inside(base - n * 6.0) { -n } else { n };
    let free = |c: Pos2| {
        let r = lock_rect(c);
        !lines.iter().any(|[a, b]| crosses(*a, *b, r)) && taken.iter().all(|t| !lock_rect(*t).intersects(r))
    };
    let mut first = None;
    for side in [out, -out] {
        // Off the line by half the padlock's width along `side` and a gap, whatever way the line runs.
        let off = 4.0 + 7.0 * side.x.abs() + 12.0 * side.y.abs();
        for k in 0..13 {
            let slide = if k % 2 == 0 { (k / 2) as f32 } else { -((k / 2 + 1) as f32) } * 14.0;
            let c = base + side * off + dir * slide;
            first.get_or_insert(c);
            if free(c) {
                return c;
            }
        }
    }
    first.unwrap_or(base)
}

/// Whether `p` is inside the closed ring `ring` (even-odd).
pub(super) fn within(ring: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    for k in 0..ring.len() {
        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Square to the line from `a` to `b` on screen, toward the bottom of the page (to the right for an upright one).
fn down(a: Pos2, b: Pos2) -> Vec2 {
    let d = (b - a).normalized();
    let n = vec2(-d.y, d.x);
    if n.y < -1e-6 || (n.y.abs() <= 1e-6 && n.x < 0.0) { -n } else { n }
}

/// How far `p` is from the segment `a`–`b`.
fn off(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t - p).length()
}

/// The ink dimensions are drawn in over the view's paper.
fn ink() -> Color32 {
    if crate::style::canvas_dark() { Color32::from_gray(225) } else { Color32::from_gray(30) }
}

/// A padlock at `c`: shut, or open (its shackle lifted to one side).
pub(super) fn padlock(painter: &Painter, c: Pos2, locked: bool, color: Color32) {
    let body = Rect::from_center_size(c + vec2(0.0, 2.5), vec2(10.0, 7.5));
    painter.rect_filled(body, 1.5, color);
    let lift = if locked { 0.0 } else { 3.0 };
    let (l, r) = (body.left() + 2.5, body.right() - 2.5);
    let top = body.top() - 4.5 - lift;
    let pts: Vec<Pos2> = (0..=8)
        .map(|k| {
            let a = std::f32::consts::PI * k as f32 / 8.0;
            pos2((l + r) / 2.0 - a.cos() * (r - l) / 2.0, top - a.sin() * 2.0 + 2.0)
        })
        .collect();
    let mut shackle = vec![pos2(l, body.top() - if locked { 0.0 } else { lift + 1.0 })];
    shackle.extend(pts);
    shackle.push(pos2(r, body.top()));
    painter.add(egui::Shape::line(shackle, Stroke::new(1.6, color)));
}

/// Dimension mark `m` painted: its line, a tick and a short witness at each reference, each segment's length (EQ, or its
/// label and length) over it; selected, a padlock under each segment and the EQ toggle over a string. An Align lock is its
/// padlock alone, shut.
pub(super) fn paint(painter: &Painter, m: &DimMark, screen: &dyn Fn(P2) -> Pos2, selected: bool, units: Units, lock: Option<Pos2>) {
    let color = if selected { PICKED } else { ink() };
    if m.alignment {
        if let Some(at) = lock {
            padlock(painter, at, true, if selected { PICKED } else { pal().accent });
        }
        return;
    }
    if m.kind == DimKind::Angular {
        paint_angular(painter, m, screen, selected, color);
        return;
    }
    if m.kind != DimKind::Aligned {
        paint_radial(painter, m, screen, selected, units, color);
        return;
    }
    let pts: Vec<Pos2> = m.points.iter().map(|p| screen(*p)).collect();
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return };
    let along = (last - first).normalized();
    let along = if along.is_finite() && along.length_sq() > 0.0 { along } else { vec2(1.0, 0.0) };
    let n = vec2(-along.y, along.x);
    let stroke = Stroke::new(1.0, color);
    painter.line_segment([first - along * 8.0, last + along * 8.0], stroke);
    for p in &pts {
        painter.line_segment([*p - n * 9.0, *p + n * 9.0], stroke);
        painter.line_segment([*p - (along + n) * 4.0, *p + (along + n) * 4.0], Stroke::new(1.6, color));
    }
    let up = -down(first, last);
    let angle = {
        let a = along.y.atan2(along.x);
        // Read from below or from the right, as a drawing's dimensions are: up the page, never down it.
        if a > std::f32::consts::FRAC_PI_2 - 1e-3 {
            a - std::f32::consts::PI
        } else if a < -std::f32::consts::FRAC_PI_2 - 1e-3 {
            a + std::f32::consts::PI
        } else {
            a
        }
    };
    let font = egui::FontId::proportional(12.0);
    for (i, w) in pts.windows(2).enumerate() {
        let len = m.lengths.get(i).copied().unwrap_or(0.0);
        let text = match &m.label {
            _ if m.equal && m.lengths.len() > 1 => t("EQ").to_string(),
            Some(l) => format!("{l} = {}", units.length(len)),
            None => units.length(len),
        };
        let galley = painter.layout_no_wrap(text, font.clone(), color);
        let mid = w[0] + (w[1] - w[0]) * 0.5 + up * 9.0;
        let size = galley.size();
        // Centred on `mid`, turned with the line.
        let (s, c) = angle.sin_cos();
        let half = vec2(size.x / 2.0, size.y);
        let corner = mid - vec2(half.x * c - half.y * s, half.x * s + half.y * c);
        painter.add(egui::epaint::TextShape::new(corner, galley, color).with_angle(angle));
        if selected && let Some(at) = lock_at(m, i, screen) {
            let locked = m.locked.get(i).copied().unwrap_or(false);
            padlock(painter, at, locked, if locked { pal().accent } else { pal().muted });
        }
    }
    if selected
        && pts.len() > 2
        && let Some(at) = equal_at(m, screen)
    {
        let tone = if m.equal { pal().accent } else { pal().muted };
        let r = Rect::from_center_size(at, vec2(22.0, 14.0));
        painter.rect_stroke(r, 2.0, Stroke::new(1.0, tone), StrokeKind::Inside);
        painter.text(at, egui::Align2::CENTER_CENTER, "EQ", egui::FontId::proportional(10.0), tone);
        if !m.equal {
            painter.line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.0, tone));
        }
    }
}

/// A radial or diameter dimension's mark painted (F7b): its line from the centre (or across the circle), an arrow where it
/// meets the circle, and `R` or `⌀` and its length (or its label and length) over it, read up the page; selected, its
/// padlock under it.
fn paint_radial(painter: &Painter, m: &DimMark, screen: &dyn Fn(P2) -> Pos2, selected: bool, units: Units, color: Color32) {
    let (Some(a), Some(b)) = (m.points.first().map(|p| screen(*p)), m.points.last().map(|p| screen(*p))) else { return };
    let along = (b - a).normalized();
    if !along.is_finite() || along.length_sq() == 0.0 {
        return;
    }
    let stroke = Stroke::new(1.0, color);
    let n = vec2(-along.y, along.x);
    // The leader runs on past the circle to where its text is, clear of the circle.
    let end = b + along * 18.0;
    painter.line_segment([a, end], stroke);
    let arrow = |tip: Pos2, way: Vec2| {
        painter.add(egui::Shape::convex_polygon(vec![tip, tip - way * 9.0 + n * 3.0, tip - way * 9.0 - n * 3.0], color, Stroke::NONE));
    };
    arrow(b, along);
    if m.kind == DimKind::Diameter {
        arrow(a, -along);
    } else {
        painter.circle_filled(a, 2.0, color);
    }
    let len = m.lengths.first().copied().unwrap_or(0.0);
    let sign = if m.kind == DimKind::Diameter { "⌀" } else { "R" };
    let text = match &m.label {
        Some(l) => format!("{l} = {}", units.length(len)),
        None => format!("{sign} {}", units.length(len)),
    };
    // Level, read as a note is, beyond the leader's end on its side.
    let (anchor, at) =
        if along.x >= 0.0 { (egui::Align2::LEFT_CENTER, end + vec2(4.0, 0.0)) } else { (egui::Align2::RIGHT_CENTER, end - vec2(4.0, 0.0)) };
    painter.text(at, anchor, text, egui::FontId::proportional(12.0), color);
    if selected && let Some(at) = lock_at(m, 0, screen) {
        let locked = m.locked.first().copied().unwrap_or(false);
        padlock(painter, at, locked, if locked { pal().accent } else { pal().muted });
    }
}

/// An angular dimension's mark painted (F7c): an arc between each two of its lines at the radius it was placed at, an arrow
/// at each end of each arc, a short witness along each line where the arc meets it, and each angle (or EQ, or its label and
/// angle) level just outside the middle of its arc; selected, a padlock inside each arc and the EQ toggle past a string's
/// first end.
fn paint_angular(painter: &Painter, m: &DimMark, screen: &dyn Fn(P2) -> Pos2, selected: bool, color: Color32) {
    let Some((v, r, segs)) = arcs(m, screen) else { return };
    let stroke = Stroke::new(1.0, color);
    let font = egui::FontId::proportional(12.0);
    for (i, (a, sweep)) in segs.iter().enumerate() {
        let n = ((sweep.abs() * r / 4.0).ceil() as usize).clamp(2, 256);
        let pts: Vec<Pos2> = (0..=n).map(|k| polar(v, r, a + sweep * k as f32 / n as f32)).collect();
        painter.add(egui::Shape::line(pts, stroke));
        // The arrows, pointing out along the arc at each end.
        let turn = sweep.signum();
        for (at, out) in [(*a, -turn), (a + sweep, turn)] {
            let tip = polar(v, r, at);
            let tangent = vec2(-at.sin(), at.cos()) * out;
            let radial = vec2(at.cos(), at.sin());
            let back = tip - tangent * 9.0;
            painter.add(egui::Shape::convex_polygon(vec![tip, back + radial * 3.0, back - radial * 3.0], color, Stroke::NONE));
        }
        let len = m.lengths.get(i).copied().unwrap_or(0.0);
        let text = match &m.label {
            _ if m.equal && m.lengths.len() > 1 => t("EQ").to_string(),
            Some(l) => format!("{l} = {}", degrees(len)),
            None => degrees(len),
        };
        let galley = painter.layout_no_wrap(text, font.clone(), color);
        let mid = a + sweep / 2.0;
        let dir = vec2(mid.cos(), mid.sin());
        let size = galley.size();
        // Its nearest corner a few pixels off the arc, whichever way the middle points.
        let reach = (dir.x.abs() * size.x + dir.y.abs() * size.y) / 2.0;
        let centre = v + dir * (r + 5.0 + reach);
        painter.galley(centre - size / 2.0, galley, color);
        if selected && let Some(at) = lock_at(m, i, screen) {
            let locked = m.locked.get(i).copied().unwrap_or(false);
            padlock(painter, at, locked, if locked { pal().accent } else { pal().muted });
        }
    }
    // A witness along each line, across the arc.
    for p in m.points.iter().skip(1) {
        let d = (screen(*p) - v).normalized();
        if d.is_finite() {
            painter.line_segment([v + d * (r - 6.0).max(0.0), v + d * (r + 6.0)], stroke);
        }
    }
    if selected
        && segs.len() > 1
        && let Some(at) = equal_at(m, screen)
    {
        let tone = if m.equal { pal().accent } else { pal().muted };
        let rect = Rect::from_center_size(at, vec2(22.0, 14.0));
        painter.rect_stroke(rect, 2.0, Stroke::new(1.0, tone), StrokeKind::Inside);
        painter.text(at, egui::Align2::CENTER_CENTER, "EQ", egui::FontId::proportional(10.0), tone);
        if !m.equal {
            painter.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.0, tone));
        }
    }
}

/// Whether `p` is on angular mark `m`: on one of its arcs, or by an angle's text just outside one.
fn on_arc(m: &DimMark, p: Pos2, screen: &dyn Fn(P2) -> Pos2) -> bool {
    let Some((v, r, segs)) = arcs(m, screen) else { return false };
    let d = p - v;
    let at = d.y.atan2(d.x);
    let tau = std::f32::consts::TAU;
    segs.iter().any(|(a, sweep)| {
        let into = if *sweep >= 0.0 { (at - a).rem_euclid(tau) } else { (a - at).rem_euclid(tau) };
        let near = (d.length() - r).abs() < NEAR || (d.length() > r && d.length() < r + 30.0);
        into <= sweep.abs() + 0.02 && near
    })
}

/// What a click at `p` on the dimensions does, before anything else in the view takes it: a padlock or the EQ toggle of the
/// selected one, an Align lock's padlock, Align's padlock waiting, or a dimension (its line or its text).
pub(super) fn clicked(
    marks: &[DimMark],
    dims: &[FamilyDimension],
    selected: Option<Guid>,
    align_lock: Option<Pos2>,
    locks: &[(usize, Pos2)],
    p: Pos2,
    screen: &dyn Fn(P2) -> Pos2,
) -> Option<Clicked> {
    if align_lock.is_some_and(|at| (at - p).length() < NEAR + 3.0) {
        return Some(Clicked::AlignLock);
    }
    let guid = |m: &DimMark| dims.get(m.index).map(|d| d.guid);
    if let Some(m) = marks.iter().find(|m| guid(m).is_some() && guid(m) == selected) {
        for i in 0..m.lengths.len() {
            if lock_at(m, i, screen).is_some_and(|at| (at - p).length() < NEAR + 1.0) {
                return guid(m).map(|g| Clicked::Lock(g, i));
            }
        }
        let string = if m.kind == DimKind::Angular { m.points.len() > 3 } else { m.points.len() > 2 };
        if string && equal_at(m, screen).is_some_and(|at| (at - p).length() < NEAR + 4.0) {
            return guid(m).map(Clicked::Equal);
        }
    }
    for m in marks.iter().filter(|m| m.alignment) {
        if locks.iter().any(|(i, at)| *i == m.index && lock_rect(*at).contains(p)) {
            return guid(m).map(Clicked::Unlock);
        }
    }
    for m in marks.iter().filter(|m| !m.alignment) {
        if m.kind == DimKind::Angular {
            if on_arc(m, p, screen) {
                return guid(m).map(Clicked::Select);
            }
            continue;
        }
        let pts: Vec<Pos2> = m.points.iter().map(|q| screen(*q)).collect();
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { continue };
        let up = -down(a, b);
        if off(p, a, b) < NEAR || off(p, a + up * 9.0, b + up * 9.0) < NEAR {
            return guid(m).map(Clicked::Select);
        }
    }
    None
}

/// The references of `flexed` view `which` sees edge on besides its planes, each as the segment it is drawn as (the view's
/// mm): extrusion ends, sketch lines and extrusions' sides (F7b), reference lines' faces.
pub(super) fn traces(flexed: &Family, values: &BTreeMap<String, Val>, st: &Staged, which: Which) -> Vec<(DimRef, P2, P2)> {
    let mut out = constraints::end_faces(flexed, values, st, which);
    out.extend(constraints::sketch_faces(flexed, values, st, which));
    out.extend(constraints::line_faces(flexed, st, which));
    out
}

/// The reference under `p` in view `which`: a circle's centre near it (F7b); else an extrusion's end, a sketch line seen edge
/// on or a reference line's face, or a plane (by the view's drawing), the nearer.
pub(super) fn reference_at(
    flexed: &Family,
    values: &BTreeMap<String, Val>,
    st: &Staged,
    which: Which,
    plane: Option<Guid>,
    p: Pos2,
    screen: &dyn Fn(P2) -> Pos2,
) -> Option<DimRef> {
    let centre = constraints::sketch_circles(flexed, values, st, which)
        .into_iter()
        .map(|c| ((screen(c.centre) - p).length(), c.r))
        .filter(|(d, _)| *d < NEAR)
        .min_by(|x, y| x.0.total_cmp(&y.0));
    if let Some((_, DimRef::Sketch(f, l))) = centre {
        return Some(DimRef::Point(f, l, Spot::Centre));
    }
    let end = traces(flexed, values, st, which)
        .into_iter()
        .map(|(r, a, b)| (off(p, screen(a), screen(b)), r))
        .filter(|(d, _)| *d < NEAR)
        .min_by(|x, y| x.0.total_cmp(&y.0));
    match (end, plane) {
        (Some((d, r)), Some(_)) if d < 3.0 => Some(r),
        (_, Some(g)) => Some(DimRef::Plane(g)),
        (Some((_, r)), None) => Some(r),
        (None, None) => None,
    }
}

/// The circle or arc of a sketch under `p` in view `which` (near its edge, within an arc's sweep): what Radial and Diameter
/// Dimension pick.
pub(super) fn circle_at(
    flexed: &Family,
    values: &BTreeMap<String, Val>,
    st: &Staged,
    which: Which,
    p: Pos2,
    screen: &dyn Fn(P2) -> Pos2,
) -> Option<constraints::Round> {
    constraints::sketch_circles(flexed, values, st, which)
        .into_iter()
        .filter_map(|c| {
            let (sc, rim) = (screen(c.centre), screen(P2(c.centre.0 + c.radius, c.centre.1)));
            // Within an arc's sweep, seen on screen (a mirror keeps what lies between its ends).
            let px = |q: Pos2| P2(f64::from(q.x), f64::from(q.y));
            let on_screen = constraints::Round { centre: px(sc), arc: c.arc.map(|a| a.map(|q| px(screen(q)))), ..c };
            on_screen.sweeps(px(p)).then_some((((p - sc).length() - (rim - sc).length()).abs(), c))
        })
        .filter(|(d, _)| *d < NEAR)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, c)| c)
}

/// The line under `p` in view `which` an angular dimension may measure to (F7c): a reference line, a straight line of a
/// sketch seen flat, or a plane's trace (by the view's drawing), the nearer.
pub(super) fn line_at(
    flexed: &Family,
    values: &BTreeMap<String, Val>,
    st: &Staged,
    which: Which,
    plane: Option<Guid>,
    p: Pos2,
    screen: &dyn Fn(P2) -> Pos2,
) -> Option<DimRef> {
    let view = DimView::of(which);
    let line = constraints::line_faces(flexed, st, which)
        .into_iter()
        .chain(constraints::sketch_faces(flexed, values, st, which))
        .filter(|(r, ..)| constraints::view_way(flexed, r, values, view).is_ok())
        .map(|(r, a, b)| (off(p, screen(a), screen(b)), r))
        .filter(|(d, _)| *d < NEAR)
        .min_by(|x, y| x.0.total_cmp(&y.0));
    let plane = plane.map(DimRef::Plane).filter(|r| constraints::view_way(flexed, r, values, view).is_ok());
    match (line, plane) {
        (Some((d, r)), Some(_)) if d < 3.0 => Some(r),
        (_, Some(r)) => Some(r),
        (Some((_, r)), None) => Some(r),
        (None, None) => None,
    }
}

/// Whether line `b` may follow `refs` in an angular dimension: it crosses the last of them (they aren't parallel).
pub(super) fn crosses_last(flexed: &Family, values: &BTreeMap<String, Val>, which: Which, refs: &[DimRef], b: &DimRef) -> bool {
    let view = DimView::of(which);
    let Ok(n) = constraints::view_way(flexed, b, values, view) else { return false };
    let Some(last) = refs.last() else { return true };
    match constraints::view_way(flexed, last, values, view) {
        Ok(m) => (n.0 * m.1 - n.1 * m.0).abs() > 1e-6 * n.length() * m.length(),
        Err(_) => false,
    }
}

/// The segment reference `r` of `flexed` is drawn as in view `which`, or the point it is at, to show it picked.
pub(super) fn drawn_as(flexed: &Family, values: &BTreeMap<String, Val>, st: &Staged, which: Which, r: &DimRef) -> Option<(P2, P2)> {
    match r {
        DimRef::Plane(g) => flexed.planes.iter().find(|p| p.guid == *g).and_then(|p| super::views::plane_trace(st, which, &p.frame())),
        DimRef::Point(f, l, _) => constraints::sketch_circles(flexed, values, st, which)
            .into_iter()
            .find(|c| c.r == DimRef::Sketch(*f, *l))
            .map(|c| (c.centre, c.centre)),
        _ => traces(flexed, values, st, which).into_iter().find(|(e, ..)| e == r).map(|(_, a, b)| (a, b)),
    }
}

impl FamilyEditor {
    /// Aligned Dimension (DI): the tool, in a plan or an elevation of a family that isn't adaptive; the selection goes.
    pub(crate) fn dimension_tool(&mut self) {
        if self.editing_2d() && self.sketch.is_none() {
            self.drop_tools();
            self.views.dimensioning = Some(Vec::new());
        }
    }

    /// Radial Dimension or Diameter Dimension (F7b): pick a circle or an arc of a sketch seen flat, then click where it goes;
    /// Angular Dimension (F7c): pick lines that cross, then click where its arc goes.
    pub(crate) fn radial_tool(&mut self, kind: DimKind) {
        if self.editing_2d() && self.sketch.is_none() {
            self.drop_tools();
            self.views.dimensioning = Some(Vec::new());
            self.views.dim_kind = kind;
        }
    }

    /// Align (AL): the tool, as Aligned Dimension's. While a form is sketched, its lines are aligned (F7b): Modify's picks
    /// stay, the Draw panel's shape goes.
    pub(crate) fn align_tool(&mut self) {
        if self.editing_2d() {
            if let Some(s) = self.sketch.as_mut() {
                s.tool = None;
                s.points.clear();
                s.align_from = None;
                self.views.aligning = Some(None);
                return;
            }
            self.drop_tools();
            self.views.aligning = Some(None);
        }
    }

    /// Every tool of the views put down and the selection let go.
    fn drop_tools(&mut self) {
        self.views.drawing = None;
        self.views.picking = None;
        self.views.dimensioning = None;
        self.views.dim_kind = DimKind::Aligned;
        self.views.aligning = None;
        self.views.align_lock = None;
        self.views.plane = None;
        self.views.dim = None;
        self.form = None;
    }

    /// The dimension selected, while the family has it.
    pub(crate) fn selected_dimension(&self) -> Option<Guid> {
        self.views.dim.filter(|g| self.dimensions.iter().any(|d| d.guid == *g))
    }

    /// The draft's family with `change` made to its dimensions, if its constraints still solve for the type previewed: else
    /// why not, nothing changed.
    fn try_dimensions(&mut self, change: impl FnOnce(&mut Vec<FamilyDimension>)) -> Result<(), Unsat> {
        let mut f = self.family().map_err(Unsat::from)?;
        change(&mut f.dimensions);
        let values = self.values();
        f.solve(&values)?;
        self.dimensions = f.dimensions;
        Ok(())
    }

    /// The selected dimension's label set (`None`: taken off). Refused as over-constrained where it adds nothing to what holds
    /// the planes already.
    pub(crate) fn set_label(&mut self, g: Guid, label: Option<String>) -> Result<(), String> {
        self.try_dimensions(|dims| {
            if let Some(d) = dims.iter_mut().find(|d| d.guid == g) {
                d.label = label;
            }
        })
        .map_err(said)
    }

    /// Segment `i` of dimension `g` locked at the length it has now, or unlocked.
    pub(crate) fn toggle_lock(&mut self, g: Guid, i: usize, length: f64) -> Result<(), String> {
        self.try_dimensions(|dims| {
            if let Some(d) = dims.iter_mut().find(|d| d.guid == g) {
                if d.is_locked(i) {
                    d.locked.retain(|l| l.segment != i);
                } else {
                    d.locked.push(Lock { segment: i, length });
                    d.locked.sort_by_key(|l| l.segment);
                }
            }
        })
        .map_err(said)
    }

    /// Dimension `g`'s equality turned on or off.
    pub(crate) fn toggle_equal(&mut self, g: Guid) -> Result<(), String> {
        self.try_dimensions(|dims| {
            if let Some(d) = dims.iter_mut().find(|d| d.guid == g) {
                d.equal = !d.equal;
            }
        })
        .map_err(said)
    }

    /// Dimension `g` taken away.
    pub(crate) fn delete_dimension(&mut self, g: Guid) {
        self.dimensions.retain(|d| d.guid != g);
        if self.views.dim == Some(g) {
            self.views.dim = None;
        }
    }

    /// The draft's dimensions measuring to plane or form `g` lose it, as deleting it does in Revit.
    pub(crate) fn forget_reference(&mut self, g: Guid) {
        let mut f = Family { dimensions: std::mem::take(&mut self.dimensions), ..Family::default() };
        f.forget_reference(g);
        self.dimensions = f.dimensions;
    }

    /// Edits a drag or Align worked out, made to the draft: planes moved along their normals, extrusion ends given numbers, a
    /// label's parameter given its new value in the type previewed (an instance parameter's, its default).
    pub(crate) fn apply_edits(&mut self, edits: Vec<Edit>) {
        for e in edits {
            match e {
                Edit::Plane(g, at) => {
                    if let Some(p) = self.planes.iter_mut().find(|p| p.guid == g) {
                        let z = p.frame().z;
                        let d = at - (p.start[0] * z[0] + p.start[1] * z[1] + p.start[2] * z[2]);
                        for (k, n) in z.iter().enumerate() {
                            p.start[k] += n * d;
                            p.end[k] += n * d;
                        }
                    }
                }
                Edit::End(i, end, v) => {
                    if let Some(f) = self.forms.get_mut(i) {
                        let slot = match end {
                            bimcraft_core::family::Extent::Start => &mut f.from,
                            bimcraft_core::family::Extent::End => &mut f.to,
                        };
                        *slot = number_text(v);
                    }
                }
                Edit::Point(i, k, spot, p) => self.move_sketch_point(i, k, spot, p),
                Edit::Line(g, by) => {
                    // The line and the lines drawn on it, as far down as they go.
                    let mut moving = vec![g];
                    for l in self.lines.iter_mut() {
                        let on = match l.host {
                            PlaneRef::Line(h, _) => moving.contains(&h),
                            PlaneRef::Plane(_) => false,
                        };
                        if l.guid == g || on {
                            for (k, d) in by.iter().enumerate() {
                                l.start[k] += d;
                                l.end[k] += d;
                            }
                            moving.push(l.guid);
                        }
                    }
                }
                Edit::LineEnds(g, start, end) => {
                    if let Some(l) = self.lines.iter_mut().find(|l| l.guid == g) {
                        l.start = start;
                        l.end = end;
                    }
                }
                Edit::Turn(i, k, t) => self.turn_sketch_arc(i, k, t),
                Edit::Parameter(name, v) => {
                    let Some(at) = self.param_at(&name) else { continue };
                    let text = number_text(v);
                    if self.params[at].instance {
                        self.params[at].value = text;
                    } else if let Some((_, set)) = self.types.get_mut(self.picked) {
                        set.insert(name, text);
                    }
                }
            }
        }
    }

    /// Line `k` of form `i`'s sketch, an arc, turning `t` degrees, a number.
    fn turn_sketch_arc(&mut self, i: usize, k: usize, t: f64) {
        use bimcraft_core::family::sketch::FormCurve;
        let Some(ft) = self.forms.get(i) else { return };
        let Ok(mut f) = ft.form() else { return };
        let Some(FormCurve::Line { turn, .. }) = f.sketch.get_mut(k).map(|l| &mut l.curve) else { return };
        *turn = Some(Expr::Number(t));
        let tag = ft.tag;
        self.forms[i] = FormText::again(&f, tag);
    }

    /// Point `spot` of line `k` of form `i`'s sketch put at `p` (its host's axes), a number.
    fn move_sketch_point(&mut self, i: usize, k: usize, spot: Spot, p: [f64; 2]) {
        use bimcraft_core::family::sketch::FormCurve;
        let Some(ft) = self.forms.get(i) else { return };
        let Ok(mut f) = ft.form() else { return };
        let Some(line) = f.sketch.get_mut(k) else { return };
        let at = [Expr::Number(p[0]), Expr::Number(p[1])];
        match (&mut line.curve, spot) {
            (FormCurve::Line { start, .. }, Spot::Start) => *start = at,
            (FormCurve::Line { end, .. }, Spot::End) => *end = at,
            (FormCurve::Circle { centre, .. }, Spot::Centre) => *centre = at,
            _ => return,
        }
        let tag = ft.tag;
        self.forms[i] = FormText::again(&f, tag);
    }

    /// Plane `g` dragged to `to` along its normal, through its constraints: done, or "Constraints are not satisfied".
    pub(super) fn drag_through(&mut self, g: Guid, to: f64) {
        let Ok(f) = self.family() else { return };
        match f.drag_plane(&self.values(), g, to) {
            Ok(edits) => self.apply_edits(edits),
            Err(e) => {
                self.views.drag = None;
                self.unsatisfied(e, Retry::Drag(g, to));
            }
        }
    }

    /// Align: `target` moved onto `reference`, with its padlock left at `at` (the view's mm) in view `which`.
    pub(super) fn align_through(&mut self, reference: DimRef, target: DimRef, at: P2, which: Which) {
        let Ok(f) = self.family() else { return };
        match constraints::align(&f, &self.values(), &reference, &target) {
            Ok(edits) => {
                self.apply_edits(edits);
                self.views.align_lock = Some((reference, target, at, which));
            }
            Err(e) if e.dims.is_empty() && !e.message.starts_with(NOT_SATISFIED) => {
                self.views.status = Some(e.message);
            }
            Err(e) => {
                self.views.align_pending = Some((reference, target, at, which));
                self.unsatisfied(e, Retry::Align(reference, target));
            }
        }
    }

    /// Align's padlock clicked: the two references it aligned locked together.
    pub(super) fn lock_alignment(&mut self) {
        let Some((reference, target, at, which)) = self.views.align_lock.take() else { return };
        let Some(st) = self.views.staged_for(which) else { return };
        let lock = FamilyDimension {
            id: 0,
            guid: Guid::new(),
            view: DimView::of(which),
            refs: vec![reference, target],
            at: st.from_view(which, at),
            label: None,
            locked: vec![Lock { segment: 0, length: 0.0 }],
            equal: false,
            alignment: true,
            kind: DimKind::Aligned,
        };
        if let Err(e) = self.try_dimensions(|dims| dims.push(lock)) {
            self.views.status = Some(said(e));
        }
    }

    /// "Constraints are not satisfied" asked, about what `e` says.
    pub(crate) fn unsatisfied(&mut self, e: Unsat, retry: Retry) {
        let dims = e.dims.iter().filter_map(|k| self.dimensions.get(*k).map(|d| d.guid)).collect();
        self.views.unsatisfied = Some(Unsatisfied { message: e.message, dims, retry });
    }

    /// The constraints of dimensions `dims` taken off (Remove Constraints): labels, locks and equality; Align's locks go.
    fn remove_constraints(&mut self, dims: &[Guid]) {
        let places: Vec<usize> = dims.iter().filter_map(|g| self.dimensions.iter().position(|d| d.guid == *g)).collect();
        let mut f = Family { dimensions: std::mem::take(&mut self.dimensions), ..Family::default() };
        f.remove_constraints(&places);
        self.dimensions = f.dimensions;
    }

    /// Why Family Types' Apply or OK can't take the draft's values: a type its constraints can't be solved for. Asked as
    /// "Constraints are not satisfied"; true when it was.
    pub(super) fn constraints_refusal(&mut self, ok: bool) -> bool {
        let Ok(f) = self.family() else { return false };
        for (name, _) in &self.types {
            let Ok(values) = f.values(name, &self.preview_params) else { continue };
            if let Err(e) = f.solve(&values) {
                let e = Unsat { message: format!("{name}: {}", e.message), ..e };
                self.unsatisfied(e, Retry::Types { ok });
                return true;
            }
        }
        false
    }

    /// Revit's "Constraints are not satisfied": what can't hold, the dimensions in it highlighted in the view; Remove
    /// Constraints takes theirs off and does what was being done, Cancel leaves the draft as it was before it.
    pub(crate) fn constraints_dialog(&mut self, ctx: &Context) {
        let Some(u) = self.views.unsatisfied.clone() else { return };
        let (mut remove, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("family-constraints"), t(UNSATISFIED)).width(480.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(&u.message);
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    remove = ui.add_enabled(!u.dims.is_empty(), egui::Button::new(t("Remove Constraints"))).clicked();
                });
            });
        });
        if remove {
            self.views.unsatisfied = None;
            self.remove_constraints(&u.dims);
            match u.retry {
                Retry::Drag(g, to) => self.drag_through(g, to),
                Retry::Align(a, b) => {
                    if let Some((_, _, at, which)) = self.views.align_pending.take() {
                        self.align_through(a, b, at, which);
                    }
                }
                Retry::Types { ok } => {
                    self.document.dialog_saved = Some(self.draft());
                    if ok {
                        self.dialog = None;
                        self.document.dialog_saved = None;
                        self.types_closed();
                    }
                }
            }
        } else if cancel || r.should_close() {
            self.views.unsatisfied = None;
            self.views.align_pending = None;
        }
    }

    /// Create Parameter…: Parameter Properties for the label of the selected dimension, a length (Type or Instance, reporting
    /// or not), made with the dimension's length as its value and given to it as its label.
    pub(crate) fn create_parameter_dialog(&mut self, ctx: &Context) {
        let Some(mut c) = self.views.creating.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("family-dimension-parameter"), t("Parameter Properties")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                Grid::new("family-dimension-parameter-grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    let l = ui.label(t("Name"));
                    ui.add(TextEdit::singleline(&mut c.name).desired_width(220.0)).labelled_by(l.id);
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut c.instance, false, t("Type"));
                        ui.radio_value(&mut c.instance, true, t("Instance"));
                    });
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut c.reporting, t("Reporting Parameter"));
                    ui.end_row();
                });
                if let Some(e) = &c.error {
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if cancel || r.should_close() {
            return;
        }
        if ok {
            match self.create_label(&c) {
                Ok(()) => return,
                Err(e) => c.error = Some(e),
            }
        }
        self.views.creating = Some(c);
    }

    /// The parameter `c` asks for made and given to its dimension as its label: refused, the draft as it was, where the name
    /// is taken or empty or the label would over-constrain.
    fn create_label(&mut self, c: &Creating) -> Result<(), String> {
        let name = c.name.trim();
        if name.is_empty() {
            return Err(t("a parameter needs a name").to_string());
        }
        if self.param_at(name).is_some() {
            return Err(tf("there's a parameter `{}` already", &[name]));
        }
        let angular = self.dimensions.iter().any(|d| d.guid == c.dim && d.kind == DimKind::Angular);
        let length = self.dimension_length(c.dim).unwrap_or(if angular { 90.0 } else { 1000.0 });
        let kind = if angular { "angle" } else { "length" };
        let p: Parameter =
            serde_json::from_value(json!({"name": name, "kind": kind, "instance": c.instance, "reporting": c.reporting, "value": length}))
                .map_err(|e| e.to_string())?;
        self.params.push(ParamText::of(&p));
        match self.set_label(c.dim, Some(name.to_string())) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.params.pop();
                Err(e)
            }
        }
    }

    /// What form `i`'s start and end are, where the type previewed's dimensions set them.
    pub(crate) fn driven_ends(&self, i: usize) -> [Option<f64>; 2] {
        let Some(s) = self.family().ok().filter(Family::constrained).and_then(|f| f.solve(&self.values()).ok()) else { return [None; 2] };
        let at = |e: bimcraft_core::family::Extent| s.ends.iter().find(|(k, x, _)| *k == i && *x == e).map(|(.., v)| *v);
        [at(bimcraft_core::family::Extent::Start), at(bimcraft_core::family::Extent::End)]
    }

    /// Dimension `g`'s first segment's length, flexed for the type previewed.
    pub(crate) fn dimension_length(&self, g: Guid) -> Option<f64> {
        let f = self.family().ok()?;
        let at = f.dimensions.iter().position(|d| d.guid == g)?;
        f.measured(&self.values()).ok()?.get(at)?.first().copied()
    }

    /// The Options Bar of a dimension selected: Label, a length parameter of the family's (an angle, for an angular one), none,
    /// or Create Parameter….
    pub(crate) fn dimension_options(&mut self, ui: &mut Ui, g: Guid) -> Option<String> {
        let d = self.dimensions.iter().find(|d| d.guid == g)?.clone();
        let one = d.segments() == 1 && !d.alignment;
        let holds = if d.kind == DimKind::Angular { Holds::Angle } else { Holds::Length };
        let lengths: Vec<String> = self.params.iter().filter(|p| p.kind == holds).map(|p| p.name.trim().to_string()).collect();
        let l = ui.label(t("Label"));
        let shown = d.label.clone().unwrap_or_else(|| t(NONE).to_string());
        let mut picked: Option<Option<String>> = None;
        let mut create = false;
        ui.add_enabled_ui(one, |ui| {
            let list = ComboBox::from_id_salt("family-dimension-label").selected_text(shown).width(200.0).show_ui(ui, |ui| {
                if ui.selectable_label(d.label.is_none(), t(NONE)).clicked() {
                    picked = Some(None);
                }
                for name in &lengths {
                    if ui.selectable_label(d.label.as_deref() == Some(name.as_str()), name).clicked() {
                        picked = Some(Some(name.clone()));
                    }
                }
                if ui.selectable_label(false, t(CREATE)).clicked() {
                    create = true;
                }
            });
            list.response.labelled_by(l.id).on_disabled_hover_text(t("Only a dimension of one segment takes a label."));
        });
        if create {
            self.views.creating = Some(Creating { dim: g, ..Creating::default() });
        }
        match picked {
            Some(label) if label != d.label => self.set_label(g, label).err(),
            _ => None,
        }
    }
}

impl Views {
    /// The staged model of view `which`, while it is the one drawn.
    fn staged_for(&self, which: Which) -> Option<&Staged> {
        self.cache.as_ref().filter(|c| c.which == which).map(|c| &c.staged)
    }
}

/// What `e` says, as the desktop says it: T19's words for a constraint that adds nothing.
fn said(e: Unsat) -> String {
    if e.over && !e.message.starts_with(NOT_SATISFIED) { format!("{}: {}", t(OVER), e.message) } else { e.message }
}

/// Whether reference `b` may join `refs` in a dimension: parallel to those that face a way (a point faces none, so it may).
pub(super) fn parallel(flexed: &Family, values: &BTreeMap<String, Val>, refs: &[DimRef], b: &DimRef) -> bool {
    let way = |r: &DimRef| constraints::direction(flexed, r, values);
    let Ok(n) = way(b) else { return false };
    let Some(n) = n else { return true };
    refs.iter().all(|r| match way(r) {
        Ok(Some(m)) => {
            let c = [n[1] * m[2] - n[2] * m[1], n[2] * m[0] - n[0] * m[2], n[0] * m[1] - n[1] * m[0]];
            (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() < 1e-6
        }
        Ok(None) => true,
        Err(_) => false,
    })
}

/// Whether some of `refs` faces a way a dimension between them may measure along: points alone face none.
pub(super) fn has_way(flexed: &Family, values: &BTreeMap<String, Val>, refs: &[DimRef]) -> bool {
    refs.iter().any(|r| matches!(constraints::direction(flexed, r, values), Ok(Some(_))))
}

/// A new dimension in view `which` between `refs`, put in order along the way it measures (its first reference that faces
/// one), its line through `at`. A radial or diameter one (`kind`) measures its one circle.
pub(super) fn new_dimension(
    flexed: &Family,
    values: &BTreeMap<String, Val>,
    st: &Staged,
    which: Which,
    refs: &[DimRef],
    at: P2,
    kind: DimKind,
) -> FamilyDimension {
    let mut placed: Vec<(f64, DimRef)> = Vec::new();
    let m = refs.iter().find_map(|r| constraints::direction(flexed, r, values).ok().flatten());
    for r in refs {
        let s = m.and_then(|m| constraints::place(flexed, r, values, m).ok()).unwrap_or(0.0);
        placed.push((s, *r));
    }
    if kind == DimKind::Aligned {
        placed.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    FamilyDimension {
        id: 0,
        guid: Guid::new(),
        view: DimView::of(which),
        refs: placed.into_iter().map(|(_, r)| r).collect(),
        at: st.from_view(which, at),
        label: None,
        locked: Vec::new(),
        equal: false,
        alignment: false,
        kind,
    }
}

/// The marks of the dimensions of `flexed` (`drawn` flexed) in view `which`, and of `preview` (a dimension being placed) too.
pub(super) fn marks_of(
    drawn: &Family,
    flexed: &Family,
    values: &BTreeMap<String, Val>,
    st: &Staged,
    which: Which,
    preview: Option<FamilyDimension>,
) -> Vec<DimMark> {
    match preview {
        None => constraints::marks(drawn, flexed, values, st, which),
        Some(d) => {
            let mut f = flexed.clone();
            f.dimensions.push(d);
            constraints::marks(drawn, &f, values, st, which)
        }
    }
}

/// Where a view's millimetres are on screen, for `view` shown from `min`.
pub(super) fn screen_of(view: View, min: Pos2) -> impl Fn(P2) -> Pos2 {
    move |q: P2| {
        let (x, y) = view.px(q);
        min + vec2(x, y)
    }
}
