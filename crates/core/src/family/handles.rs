//! Shape handles on a form's profile. A selected extrusion (solid or void) shows a handle at the middle of
//! each edge of its profile, its outlines' and its holes' alike, pointing out of the solid; dragged, it offsets that edge along
//! its normal. The lines that share its ends follow: each straight one along its own line, extended or trimmed to meet the
//! moved edge (one running on in line with it has its shared end moved with the edge); an arc there still runs through its
//! middle as drawn and through its ends where they now are, so a D's arc grows with its flat side. An arc's handle changes its
//! radius about its centre (its ends go out along their radii and the lines at them follow); a whole circle's, its radius.
//!
//! This is the geometry alone, in the host plane's own axes ([`edge_handles`], [`offset_lines`], [`offset_edge`]), and what
//! constraint holds an edge or an end ([`Family::edge_held`], [`Family::end_held`]); the family document draws the handles and
//! refuses what the constraints won't take. Only the numbers a drag moves are written: a number as a number, a formula as
//! itself with the move added ([`shifted`]: `Width / 2` dragged 40 out is `(Width / 2) + 40`), so that side alone goes
//! and its parameter still moves it; every other keeps what it was, its formula too.
//!
//! What a dragged side or end lines up with is here too ([`Family::handle_targets`], [`snapped`]): the reference planes
//! and the other forms' sides and ends that face its way, each with where it is along it.

use std::collections::BTreeMap;

use super::sketch::{FormCurve, FormLine, chorded};
use super::{DimRef, Expr, Extent, Family, FamilyDimension, Val, constraints};
use crate::geom::P2;
use crate::ids::Guid;
use crate::model::TOL;
use crate::walls::Axis;

/// How far, mm, a number must move to be written: less, and it keeps what it was (a formula too).
const MOVED: f64 = 1e-6;

/// A handle on an edge of a profile: the line's place in the sketch, where the handle is (the middle of a line or an arc, a
/// circle's point at 0°), the way out of the solid there (a unit vector), and whether dragging it changes a radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeHandle {
    pub line: usize,
    pub at: P2,
    pub out: P2,
    pub round: bool,
}

/// Why an edge can't be moved so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The profile it would leave isn't one: lines crossing or overlapping, one of no length or turned back on itself, a loop
    /// no longer closed, an arc of no radius.
    Invalid,
}

/// What holds an edge or an end where it is: a dimension's label (the parameter's name), a lock (a padlock, Align's lock),
/// or equality (EQ).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Held {
    Label(String),
    Locked,
    Equal,
}

/// A line of the sketch read as numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Seg {
    Line { a: P2, b: P2, turn: f64 },
    Circle { c: P2, r: f64 },
}

fn read(l: &FormLine, num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Option<Seg> {
    let mut pt = |p: &[Expr; 2]| -> Option<P2> { Some(P2(num(&p[0]).ok()?, num(&p[1]).ok()?)) };
    let seg = match &l.curve {
        FormCurve::Line { start, end, turn } => {
            let (a, b) = (pt(start)?, pt(end)?);
            let turn = match turn {
                Some(t) => num(t).ok()?,
                None => 0.0,
            };
            Seg::Line { a, b, turn }
        }
        FormCurve::Circle { centre, radius } => {
            let c = pt(centre)?;
            Seg::Circle { c, r: num(radius).ok()? }
        }
    };
    let ok = match seg {
        Seg::Line { a, b, turn } => a.is_finite() && b.is_finite() && turn.is_finite(),
        Seg::Circle { c, r } => c.is_finite() && r.is_finite() && r > 0.0,
    };
    ok.then_some(seg)
}

/// The profile's lines (not a revolve's axis) read as numbers, by their places in the sketch; None where one can't be read.
fn read_all(lines: &[FormLine], num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Option<Vec<Option<Seg>>> {
    lines.iter().map(|l| if l.axis { Some(None) } else { read(l, num).map(Some) }).collect()
}

/// An arc's centre and radius; none for a straight line.
fn arc_of(a: P2, b: P2, turn: f64) -> Option<(P2, f64)> {
    if turn == 0.0 {
        return None;
    }
    let ax = Axis::new(a, b, turn);
    Some((ax.centre()?, ax.radius()?))
}

/// Each line's chords (its place in the sketch, its points in turn), and the rings the profile closes into.
type Built = (Vec<(usize, Vec<P2>)>, Vec<Vec<P2>>);

/// The profile as it is built: its lines' sketch lines, chorded, and the rings they close into.
fn rings(lines: &[FormLine], num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Option<Built> {
    let mut chords = Vec::new();
    let mut drawn = Vec::new();
    for (i, l) in lines.iter().enumerate().filter(|(_, l)| !l.axis) {
        let sl = chorded(&l.curve(&mut |e| num(e)).ok()?);
        let pts = match &sl.curve {
            crate::curves::Curve::Poly { points, closed, .. } => {
                let mut p = points.clone();
                if *closed && let Some(first) = p.first().copied() {
                    p.push(first);
                }
                p
            }
            _ => return None,
        };
        chords.push((i, pts));
        drawn.push(sl);
    }
    let solved = crate::sketch::solve(&drawn).ok()?;
    let rings = solved.regions.into_iter().flat_map(|r| std::iter::once(r.outer).chain(r.holes)).collect();
    Some((chords, rings))
}

/// Whether `p` is inside the profile: inside an odd number of its rings (an outline, a hole in it, an island in that…).
fn inside(rings: &[Vec<P2>], p: P2) -> bool {
    rings.iter().filter(|r| crate::geom::contains(r, p)).count() % 2 == 1
}

/// The handles of a profile's edges, one on each line of `lines` (a form's sketch, its numbers read by `num`) but a revolve's
/// axis: where it shows, and the way out of the solid there. None for a sketch that can't be read or isn't a profile.
pub fn edge_handles(lines: &[FormLine], num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Vec<EdgeHandle> {
    let Some(segs) = read_all(lines, num) else { return Vec::new() };
    let Some((chords, rings)) = rings(lines, num) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        let Some(seg) = seg else { continue };
        let Some((_, pts)) = chords.iter().find(|(k, _)| *k == i) else { continue };
        // The chord nearest the handle (as the profile is built), and the way across it to test which side the solid is.
        let m = (pts.len().saturating_sub(1)) / 2;
        let (p, q) = match (pts.get(m), pts.get(m + 1)) {
            (Some(p), Some(q)) => (*p, *q),
            _ => continue,
        };
        let (at, across, round) = match *seg {
            Seg::Line { a, b, turn } => match arc_of(a, b, turn) {
                Some((c, _)) => {
                    let ax = Axis::new(a, b, turn);
                    let mid = ax.at(ax.length() / 2.0, 0.0);
                    (mid, (mid - c).unit(), true)
                }
                None => ((a + b) * 0.5, (b - a).unit().perp(), false),
            },
            Seg::Circle { c, r } => (P2(c.0 + r, c.1), P2(1.0, 0.0), true),
        };
        let chord_mid = (p + q) * 0.5;
        let way = match *seg {
            Seg::Line { a, b, turn } => match arc_of(a, b, turn) {
                Some((c, _)) => (chord_mid - c).unit(),
                None => across,
            },
            Seg::Circle { c, .. } => (chord_mid - c).unit(),
        };
        let eps = (p.dist(q) * 1e-4).clamp(1e-6, 1e-2);
        let sign = if inside(&rings, chord_mid + way * eps) { -1.0 } else { 1.0 };
        out.push(EdgeHandle { line: i, at, out: across * sign, round });
    }
    out
}

/// Whether edge handle `h` of `lines` is in sight from far off along `toward` (a unit vector in the plane's axes, the way
/// to someone looking across the plane): nothing of the profile lies between its side and them, so a view looking that way
/// shows its side as a line of the form's outline or a step in it. A side behind another part of the profile is hidden.
pub fn in_sight(lines: &[FormLine], h: &EdgeHandle, toward: P2, num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> bool {
    let Some((_, rings)) = rings(lines, num) else { return false };
    // Just outside the side, toward someone looking.
    let from = h.at + h.out * 1e-3;
    let reach: f64 = rings.iter().flatten().map(|p| p.dist(from)).fold(0.0, f64::max) * 2.0 + 1.0;
    let to = from + toward * reach;
    let crosses = |a: P2, b: P2| {
        let (d1, d2) = ((to - from).cross(a - from), (to - from).cross(b - from));
        let (d3, d4) = ((b - a).cross(from - a), (b - a).cross(to - a));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    };
    !rings.iter().any(|r| r.iter().zip(r.iter().cycle().skip(1)).any(|(a, b)| crosses(*a, *b)))
}

/// Which line other than `k` ends at `p` (within [`TOL`]), and whether at its end: the one line a closed loop has there, the
/// nearest end where two lie within reach (a corner of another loop close by is not its neighbour).
fn neighbour(segs: &[Option<Seg>], k: usize, p: P2) -> Option<(usize, bool)> {
    let mut near: Vec<(f64, usize, bool)> = Vec::new();
    for (j, s) in segs.iter().enumerate() {
        if j == k {
            continue;
        }
        if let Some(Seg::Line { a, b, .. }) = s {
            for (end, q) in [(false, *a), (true, *b)] {
                if q.dist(p) <= TOL {
                    near.push((q.dist(p), j, end));
                }
            }
        }
    }
    near.sort_by(|x, y| x.0.total_cmp(&y.0));
    match near.as_slice() {
        [] => None,
        [(_, j, end)] => Some((*j, *end)),
        [(d0, j, end), (d1, ..), ..] if d1 - d0 > MOVED => Some((*j, *end)),
        _ => None,
    }
}

/// The turn, in degrees counter-clockwise, of the arc from `a` through `m` to `b`; None where they lie on a line.
fn turn_through(a: P2, m: P2, b: P2) -> Option<f64> {
    let d = 2.0 * ((m - a).cross(b - a));
    if d.abs() < 1e-9 {
        return None;
    }
    let (ma, ba) = (m - a, b - a);
    let (lm, lb) = (ma.dot(ma), ba.dot(ba));
    let c = a + P2(ba.1 * lm - ma.1 * lb, ma.0 * lb - ba.0 * lm) * (1.0 / d);
    let ang = |p: P2| (p.1 - c.1).atan2(p.0 - c.0);
    let tau = std::f64::consts::TAU;
    let sweep = (ang(b) - ang(a)).rem_euclid(tau);
    let to_m = (ang(m) - ang(a)).rem_euclid(tau);
    let turn = if to_m < sweep { sweep } else { sweep - tau };
    turn.is_finite().then(|| turn.to_degrees())
}

/// The formula `inner` and the number `k` of one written `(inner) + k` or `(inner) - k`, as [`shifted`] writes it: the
/// first bracket closing where the last one does, then a sign and a plain number.
fn shift_of(s: &str) -> Option<(&str, f64)> {
    let s = s.trim();
    let (head, tail) = s.split_at(s.rfind(')')? + 1);
    let tail = tail.trim();
    let (sign, k) = match (tail.strip_prefix('+'), tail.strip_prefix('-')) {
        (Some(k), _) => (1.0, k),
        (_, Some(k)) => (-1.0, k),
        _ => return None,
    };
    let k = k.trim();
    if k.is_empty() || !k.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let k: f64 = k.parse().ok()?;
    if !head.starts_with('(') {
        return None;
    }
    let mut depth = 0_i32;
    for (i, c) in head.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && i + 1 != head.len() {
                    return None;
                }
            }
            _ => {}
        }
    }
    let inner = head.get(1..head.len() - 1)?;
    (depth == 0 && k.is_finite()).then_some((inner, sign * k))
}

/// Formula `s` with `d` added to what it works out: `(s) + d`, or `(s) - d` for less. A shift written so before is
/// added to, not wrapped again (`(s) + 100` moved 20 more is `(s) + 120`), and one that comes back to nothing leaves `s`
/// as it was typed.
pub fn shifted(s: &str, d: f64) -> Expr {
    let (inner, was) = shift_of(s).unwrap_or((s.trim(), 0.0));
    let k = ((was + d) * 1e6).round() / 1e6;
    if k.abs() <= MOVED {
        Expr::Formula(inner.to_string())
    } else if k < 0.0 {
        Expr::Formula(format!("({inner}) - {}", -k))
    } else {
        Expr::Formula(format!("({inner}) + {k}"))
    }
}

/// Number `e`, `old` now, made `new`: unchanged within [`MOVED`] (a formula kept), else written as a number, a formula as
/// itself with the move added ([`shifted`]).
fn put(e: &mut Expr, old: f64, new: f64) -> Result<(), Refused> {
    if (new - old).abs() <= MOVED {
        return Ok(());
    }
    if !new.is_finite() {
        return Err(Refused::Invalid);
    }
    *e = match e {
        Expr::Number(_) => Expr::Number((new * 1e6).round() / 1e6),
        Expr::Formula(s) => shifted(s, new - old),
    };
    Ok(())
}

/// Line `j`'s start (or end, `end`) moved from `old` to `new`.
fn put_point(l: &mut FormLine, end: bool, old: P2, new: P2) -> Result<(), Refused> {
    let FormCurve::Line { start, end: e, .. } = &mut l.curve else { return Err(Refused::Invalid) };
    let p = if end { e } else { start };
    put(&mut p[0], old.0, new.0)?;
    put(&mut p[1], old.1, new.1)
}

/// The moves a drag of line `k` by `d` mm (out of the solid, `out`) makes: (line, its end or start, from, to), a circle's or
/// an arc's new radius, and the arcs at its ends turned anew (line, turn before, turn now).
type Moves = (Vec<(usize, bool, P2, P2)>, Option<(f64, f64)>, Vec<(usize, f64, f64)>);

fn moves(segs: &[Option<Seg>], k: usize, out: P2, d: f64) -> Result<Moves, Refused> {
    let seg = segs.get(k).copied().flatten().ok_or(Refused::Invalid)?;
    let (a, b, turn) = match seg {
        Seg::Circle { r, .. } => {
            // Its handle is at 0°: out of the solid is away from the centre round an outline, toward it round a hole.
            let sign = if out.dot(P2(1.0, 0.0)) >= 0.0 { 1.0 } else { -1.0 };
            return Ok((Vec::new(), Some((r, r + sign * d)), Vec::new()));
        }
        Seg::Line { a, b, turn } => (a, b, turn),
    };
    let at_a = neighbour(segs, k, a).ok_or(Refused::Invalid)?;
    let at_b = neighbour(segs, k, b).ok_or(Refused::Invalid)?;
    let mut out_moves = Vec::new();
    let (na, nb, radius) = match arc_of(a, b, turn) {
        Some((c, r)) => {
            let mid = {
                let ax = Axis::new(a, b, turn);
                ax.at(ax.length() / 2.0, 0.0)
            };
            let sign = if out.dot(mid - c) >= 0.0 { 1.0 } else { -1.0 };
            let r2 = r + sign * d;
            if r2.is_nan() || r2 <= TOL {
                return Err(Refused::Invalid);
            }
            let k2 = r2 / r;
            (c + (a - c) * k2, c + (b - c) * k2, Some((r, r2)))
        }
        None => {
            let t = (b - a).unit();
            let shift = out * d;
            let follow = |p: P2, (j, end): (usize, bool)| -> P2 {
                match segs.get(j).copied().flatten() {
                    Some(Seg::Line { a: ja, b: jb, turn: 0.0 }) => {
                        let far = if end { ja } else { jb };
                        let u = p - far;
                        if u.unit().cross(t).abs() < 1e-9 {
                            p + shift
                        } else {
                            // Along its own line to where it meets the edge moved.
                            let s = (p + shift - far).cross(t) / u.cross(t);
                            far + u * s
                        }
                    }
                    _ => p + shift,
                }
            };
            (follow(a, at_a), follow(b, at_b), None)
        }
    };
    out_moves.push((k, false, a, na));
    out_moves.push((k, true, b, nb));
    let ends = |(j, end): (usize, bool)| match segs.get(j).copied().flatten() {
        Some(Seg::Line { a, b, .. }) => Some(if end { b } else { a }),
        _ => None,
    };
    let (pa, pb) = (ends(at_a).ok_or(Refused::Invalid)?, ends(at_b).ok_or(Refused::Invalid)?);
    out_moves.push((at_a.0, at_a.1, pa, na));
    out_moves.push((at_b.0, at_b.1, pb, nb));
    // An arc at an end still runs through its middle as drawn and through its ends where they are now (both, in a D drawn of a
    // line and an arc), so it grows or shrinks with the edge rather than being carried along.
    let mut turns = Vec::new();
    for j in [at_a.0, at_b.0] {
        if turns.iter().any(|(x, _, _)| *x == j) {
            continue;
        }
        if let Some(Seg::Line { a: ja, b: jb, turn: jt }) = segs.get(j).copied().flatten()
            && jt != 0.0
        {
            let moved = |end: bool, was: P2| out_moves.iter().find(|m| m.0 == j && m.1 == end).map_or(was, |m| m.3);
            let ax = Axis::new(ja, jb, jt);
            let mid = ax.at(ax.length() / 2.0, 0.0);
            let now = turn_through(moved(false, ja), mid, moved(true, jb)).ok_or(Refused::Invalid)?;
            turns.push((j, jt, now));
        }
    }
    Ok((out_moves, radius, turns))
}

/// `lines` (a form's sketch, its numbers read by `num`) with line `k` moved `d` mm out of the solid (`out`, its handle's way):
/// a straight edge offset along its normal, the lines at its ends following; an arc's or a circle's radius changed. Only the
/// numbers that move are written; one that is a formula keeps it, with the move added ([`shifted`]). The profile left isn't
/// checked: that is [`offset_edge`]'s.
pub fn offset_lines(
    lines: &[FormLine],
    k: usize,
    out: P2,
    d: f64,
    num: &mut dyn FnMut(&Expr) -> Result<f64, String>,
) -> Result<Vec<FormLine>, Refused> {
    if !d.is_finite() {
        return Err(Refused::Invalid);
    }
    let segs = read_all(lines, num).ok_or(Refused::Invalid)?;
    let (moved, radius, turns) = moves(&segs, k, out, d)?;
    let mut new = lines.to_vec();
    for (j, end, from, to) in moved {
        put_point(&mut new[j], end, from, to)?;
    }
    for (j, was, now) in turns {
        match &mut new[j].curve {
            FormCurve::Line { turn: Some(t), .. } => put(t, was, now)?,
            _ => return Err(Refused::Invalid),
        }
    }
    if let Some((was, now)) = radius
        && let FormCurve::Circle { radius, .. } = &mut new[k].curve
    {
        put(radius, was, now)?;
    }
    Ok(new)
}

/// [`offset_lines`], refused where the profile it leaves isn't one ([`Refused::Invalid`]): a straight line it moves turned
/// back on itself or of no length, or what Finish Edit Mode refuses (`sketch::finish_check`: lines crossing or overlapping,
/// a loop not closed).
pub fn offset_edge(
    lines: &[FormLine],
    k: usize,
    out: P2,
    d: f64,
    num: &mut dyn FnMut(&Expr) -> Result<f64, String>,
) -> Result<Vec<FormLine>, Refused> {
    let new = offset_lines(lines, k, out, d, num)?;
    let (before, after) = (read_all(lines, num).ok_or(Refused::Invalid)?, read_all(&new, num).ok_or(Refused::Invalid)?);
    for (was, now) in before.iter().zip(&after) {
        if let (Some(Seg::Line { a, b, turn }), Some(Seg::Line { a: a2, b: b2, .. })) = (was, now)
            && (a.dist(*a2) > MOVED || b.dist(*b2) > MOVED)
            && (a2.dist(*b2) <= TOL || (*turn == 0.0 && (*b2 - *a2).dot(*b - *a) <= 0.0))
        {
            return Err(Refused::Invalid);
        }
    }
    let drawn: Vec<crate::sketch::SketchLine> =
        new.iter().filter(|l| !l.axis).map(|l| l.curve(&mut |e| num(e))).collect::<Result<_, _>>().map_err(|_| Refused::Invalid)?;
    super::sketch::finish_check(&drawn, None, false).map_err(|_| Refused::Invalid)?;
    Ok(new)
}

impl Family {
    /// What a side or an end of form `own`, dragged by its handle along `way` (a unit vector in the family's axes), can line
    /// up with, for `values`: each reference plane (the datums too) and each other form's straight side, Start and End that
    /// faces that way (`shown`: what the view shows), with where it is along `way`. The form's own sides are none of them.
    pub fn handle_targets(
        &self,
        values: &BTreeMap<String, Val>,
        way: [f64; 3],
        own: Guid,
        shown: &dyn Fn(&DimRef) -> bool,
    ) -> Vec<(DimRef, f64)> {
        let Ok(flexed) = self.flexed(values) else { return Vec::new() };
        let mut refs: Vec<DimRef> = Vec::new();
        let datums = super::planes::datums();
        for p in datums.iter().chain(&flexed.planes).filter(|p| !p.guid.is_nil()) {
            if !refs.contains(&DimRef::Plane(p.guid)) {
                refs.push(DimRef::Plane(p.guid));
            }
        }
        for form in flexed.forms.iter().filter(|x| !x.guid.is_nil() && x.guid != own) {
            refs.extend(form.sketch.iter().filter(|l| !l.axis && !l.guid.is_nil()).map(|l| DimRef::Sketch(form.guid, l.guid)));
            refs.extend([Extent::Start, Extent::End].map(|e| DimRef::Form(form.guid, e)));
        }
        refs.into_iter()
            .filter(|r| shown(r))
            .filter_map(|r| {
                let n = constraints::direction(&flexed, &r, values).ok()??;
                let facing = n[0] * way[0] + n[1] * way[1] + n[2] * way[2];
                if facing.abs() < 1.0 - 1e-9 {
                    return None;
                }
                let at = constraints::place(&flexed, &r, values, way).ok()?;
                at.is_finite().then_some((r, at))
            })
            .collect()
    }
}

/// Of `targets` ([`Family::handle_targets`]), the one nearest `at` along the way and within `reach` mm of it: what a side
/// or an end dragged there lines up with, the first listed of two at one place (a plane before a form's side).
pub fn snapped(targets: &[(DimRef, f64)], at: f64, reach: f64) -> Option<(DimRef, f64)> {
    let mut best: Option<(DimRef, f64)> = None;
    for (r, t) in targets {
        let d = (t - at).abs();
        if d <= reach && best.is_none_or(|(_, b)| d < (b - at).abs() - 1e-9) {
            best = Some((*r, *t));
        }
    }
    best
}

/// Whether sketches `a` and `b`, read by `num`, have the same lines in the same places, within `tol` mm (a radius too, an
/// arc's turn within a thousandth of a degree): what the family document checks a drag against where constraints flex it.
pub fn same_place(a: &[FormLine], b: &[FormLine], tol: f64, num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| match (read(x, num), read(y, num)) {
            (Some(Seg::Line { a, b, turn }), Some(Seg::Line { a: a2, b: b2, turn: t2 })) => {
                a.dist(a2) <= tol && b.dist(b2) <= tol && (turn - t2).abs() <= 1e-3
            }
            (Some(Seg::Circle { c, r }), Some(Seg::Circle { c: c2, r: r2 })) => c.dist(c2) <= tol && (r - r2).abs() <= tol,
            (None, None) => x.axis && y.axis,
            _ => false,
        })
}

impl FamilyDimension {
    /// What it holds reference `r` (its place) with, if anything: a label (unless a reporting one, `reporting`), a lock on a
    /// segment beside it, Align's lock, or equality.
    fn holds(&self, r: usize, reporting: &dyn Fn(&str) -> bool) -> Option<Held> {
        if let Some(l) = self.label.as_deref().filter(|l| !reporting(l)) {
            return Some(Held::Label(l.to_string()));
        }
        if self.alignment || self.locked.iter().any(|l| l.segment == r || l.segment + 1 == r) {
            return Some(Held::Locked);
        }
        self.equal.then_some(Held::Equal)
    }
}

impl Family {
    /// What holds line `line` of form `form`'s sketch where it is: a dimension measuring to it (or to its ends or centre) with a
    /// label, a lock or EQ, the first found. None when it is free to move.
    pub fn edge_held(&self, form: Guid, line: Guid) -> Option<Held> {
        let reporting = |l: &str| self.parameters.iter().any(|p| p.name == l && p.reporting);
        labelled_first(self.dimensions.iter().filter_map(|d| {
            let r = d.refs.iter().position(|r| match *r {
                DimRef::Sketch(f, l) | DimRef::Point(f, l, _) => f == form && l == line,
                _ => false,
            })?;
            d.holds(r, &reporting)
        }))
    }

    /// What holds form `form`'s Start or End (`end`) where it is, as [`Family::edge_held`] says for a line.
    pub fn end_held(&self, form: Guid, end: Extent) -> Option<Held> {
        let reporting = |l: &str| self.parameters.iter().any(|p| p.name == l && p.reporting);
        labelled_first(self.dimensions.iter().filter_map(|d| {
            let r = d.refs.iter().position(|r| *r == DimRef::Form(form, end))?;
            d.holds(r, &reporting)
        }))
    }
}

/// Of what holds a side or an end, a label first, whichever dimension is listed first: its drag goes through the label (a
/// lock or EQ listed before it hides nothing).
fn labelled_first(held: impl Iterator<Item = Held>) -> Option<Held> {
    let all: Vec<Held> = held.collect();
    all.iter().find(|h| matches!(h, Held::Label(_))).cloned().or_else(|| all.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Expr {
        Expr::Number(x)
    }

    /// A closed loop of straight lines through `corners`, numbers all.
    fn ring(corners: &[(f64, f64)]) -> Vec<FormLine> {
        let pts: Vec<[Expr; 2]> = corners.iter().map(|&(x, y)| [n(x), n(y)]).collect();
        super::super::sketch::profile_lines(&pts)
    }

    fn num() -> impl FnMut(&Expr) -> Result<f64, String> {
        |e: &Expr| match e {
            Expr::Number(x) => Ok(*x),
            Expr::Formula(f) if f == "W" => Ok(1000.0),
            Expr::Formula(f) => match shift_of(f) {
                Some(("W", k)) => Ok(1000.0 + k),
                _ => Err(format!("{f}?")),
            },
        }
    }

    fn ends(l: &FormLine) -> (P2, P2) {
        match &l.curve {
            FormCurve::Line { start, end, .. } => {
                let v = |e: &Expr| match e {
                    Expr::Number(x) => *x,
                    _ => f64::NAN,
                };
                (P2(v(&start[0]), v(&start[1])), P2(v(&end[0]), v(&end[1])))
            }
            FormCurve::Circle { .. } => panic!("a line"),
        }
    }

    fn near(a: P2, b: P2) -> bool {
        a.dist(b) < 1e-6
    }

    /// The L: 1000 by 1000 less its top right quarter, counter-clockwise from the origin.
    const L: [(f64, f64); 6] = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 500.0), (500.0, 500.0), (500.0, 1000.0), (0.0, 1000.0)];

    #[test]
    fn a_rectangles_handles_sit_at_its_edges_middles_pointing_out() {
        let lines = ring(&[(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)]);
        let h = edge_handles(&lines, &mut num());
        assert_eq!(h.len(), 4);
        let want = [
            (P2(200.0, 0.0), P2(0.0, -1.0)),
            (P2(400.0, 150.0), P2(1.0, 0.0)),
            (P2(200.0, 300.0), P2(0.0, 1.0)),
            (P2(0.0, 150.0), P2(-1.0, 0.0)),
        ];
        for (h, (at, out)) in h.iter().zip(want) {
            assert!(near(h.at, at) && near(h.out, out) && !h.round, "{h:?}");
        }
        // Drawn clockwise, the same.
        let cw = ring(&[(0.0, 0.0), (0.0, 300.0), (400.0, 300.0), (400.0, 0.0)]);
        let h = edge_handles(&cw, &mut num());
        assert!(near(h[0].out, P2(-1.0, 0.0)) && near(h[1].out, P2(0.0, 1.0)), "{h:?}");
    }

    #[test]
    fn an_l_shapes_handles_point_out_of_its_inner_corner_too() {
        let h = edge_handles(&ring(&L), &mut num());
        assert_eq!(h.len(), 6);
        // The two edges of the notch face up and right, into the notch.
        assert!(near(h[2].at, P2(750.0, 500.0)) && near(h[2].out, P2(0.0, 1.0)), "{:?}", h[2]);
        assert!(near(h[3].at, P2(500.0, 750.0)) && near(h[3].out, P2(1.0, 0.0)), "{:?}", h[3]);
    }

    #[test]
    fn a_side_behind_another_part_of_the_profile_is_out_of_sight() {
        let lines = ring(&L);
        let h = edge_handles(&lines, &mut num());
        // The notch's wall, from the south: behind the L's foot. From the north: in sight across the notch.
        assert!(!in_sight(&lines, &h[3], P2(0.0, -1.0), &mut num()));
        assert!(in_sight(&lines, &h[3], P2(0.0, 1.0), &mut num()));
        // The east and west sides, either way.
        for k in [1, 5] {
            assert!(in_sight(&lines, &h[k], P2(0.0, -1.0), &mut num()) && in_sight(&lines, &h[k], P2(0.0, 1.0), &mut num()));
        }
    }

    #[test]
    fn a_holes_handles_point_into_the_hole() {
        let mut lines = ring(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]);
        lines.extend(ring(&[(400.0, 400.0), (600.0, 400.0), (600.0, 600.0), (400.0, 600.0)]));
        let h = edge_handles(&lines, &mut num());
        assert_eq!(h.len(), 8);
        // The hole's bottom edge, at y 400: out of the solid is up, into the hole.
        assert!(near(h[4].at, P2(500.0, 400.0)) && near(h[4].out, P2(0.0, 1.0)), "{:?}", h[4]);
        // Offset 50 out of the solid: the hole's bottom edge goes up, the hole shrinks.
        let moved = offset_edge(&lines, 4, h[4].out, 50.0, &mut num()).unwrap();
        assert_eq!(ends(&moved[4]), (P2(400.0, 450.0), P2(600.0, 450.0)));
        assert_eq!(ends(&moved[5]).0, P2(600.0, 450.0));
        assert_eq!(ends(&moved[7]).1, P2(400.0, 450.0));
    }

    #[test]
    fn an_edge_offset_stretches_its_neighbours_along_their_own_lines() {
        let lines = ring(&L);
        let h = edge_handles(&lines, &mut num());
        // The notch's floor (line 2, y 500) up 200: the right side grows to 700, the notch's wall shrinks from 500.
        let moved = offset_edge(&lines, 2, h[2].out, 200.0, &mut num()).unwrap();
        assert_eq!(ends(&moved[2]), (P2(1000.0, 700.0), P2(500.0, 700.0)));
        assert_eq!(ends(&moved[1]), (P2(1000.0, 0.0), P2(1000.0, 700.0)));
        assert_eq!(ends(&moved[3]), (P2(500.0, 700.0), P2(500.0, 1000.0)));
        // The rest as they were.
        assert_eq!(moved[0], lines[0]);
        assert_eq!(moved[4], lines[4]);
        assert_eq!(moved[5], lines[5]);
    }

    #[test]
    fn a_slanted_neighbour_is_trimmed_along_its_own_line_and_one_in_line_has_its_end_moved() {
        // A trapezoid: its top (line 2) down 100 meets the slanted sides further in.
        let lines = ring(&[(0.0, 0.0), (1000.0, 0.0), (800.0, 400.0), (200.0, 400.0)]);
        let moved = offset_edge(&lines, 2, P2(0.0, 1.0), -100.0, &mut num()).unwrap();
        assert!(near(ends(&moved[2]).0, P2(850.0, 300.0)) && near(ends(&moved[2]).1, P2(150.0, 300.0)), "{:?}", ends(&moved[2]));
        assert!(near(ends(&moved[1]).1, P2(850.0, 300.0)));
        // A bottom of two lines in line: the left half down 100 drags the right half's shared end with it.
        let lines = ring(&[(0.0, 0.0), (500.0, 0.0), (1000.0, 0.0), (1000.0, 400.0), (0.0, 400.0)]);
        let moved = offset_edge(&lines, 0, P2(0.0, -1.0), 100.0, &mut num()).unwrap();
        assert_eq!(ends(&moved[0]), (P2(0.0, -100.0), P2(500.0, -100.0)));
        assert_eq!(ends(&moved[1]), (P2(500.0, -100.0), P2(1000.0, 0.0)));
        assert_eq!(ends(&moved[4]).1, P2(0.0, -100.0));
    }

    #[test]
    fn an_arcs_handle_changes_its_radius_about_its_centre() {
        // A slot's right end: a half circle of radius 100 about (500, 100).
        let mut lines = ring(&[(0.0, 0.0), (500.0, 0.0), (500.0, 200.0), (0.0, 200.0)]);
        if let FormCurve::Line { turn, .. } = &mut lines[1].curve {
            *turn = Some(n(180.0));
        }
        let h = edge_handles(&lines, &mut num());
        assert!(h[1].round && near(h[1].at, P2(600.0, 100.0)) && near(h[1].out, P2(1.0, 0.0)), "{:?}", h[1]);
        let moved = offset_edge(&lines, 1, h[1].out, 50.0, &mut num()).unwrap();
        assert!(near(ends(&moved[1]).0, P2(500.0, -50.0)) && near(ends(&moved[1]).1, P2(500.0, 250.0)), "{:?}", ends(&moved[1]));
        assert!(near(ends(&moved[0]).1, P2(500.0, -50.0)) && near(ends(&moved[2]).0, P2(500.0, 250.0)));
        // A whole circle's handle, its radius.
        let disc = vec![super::super::sketch::circle_line(&super::super::Circle { centre: [n(0.0), n(0.0)], radius: n(80.0) })];
        let h = edge_handles(&disc, &mut num());
        assert!(h[0].round && near(h[0].at, P2(80.0, 0.0)) && near(h[0].out, P2(1.0, 0.0)));
        let moved = offset_edge(&disc, 0, h[0].out, -30.0, &mut num()).unwrap();
        assert!(matches!(&moved[0].curve, FormCurve::Circle { radius: Expr::Number(r), .. } if (*r - 50.0).abs() < 1e-9));
        assert_eq!(offset_edge(&disc, 0, h[0].out, -80.0, &mut num()), Err(Refused::Invalid));
    }

    #[test]
    fn a_ds_flat_side_offset_grows_the_d_its_arc_keeping_its_middle() {
        // A D: a line from (-100, 0) to (100, 0) and a half circle back over the top through (0, 100).
        let mut lines = ring(&[(-100.0, 0.0), (100.0, 0.0)]);
        if let FormCurve::Line { turn, .. } = &mut lines[1].curve {
            *turn = Some(n(180.0));
        }
        let h = edge_handles(&lines, &mut num());
        assert!(near(h[0].out, P2(0.0, -1.0)), "{h:?}");
        let moved = offset_edge(&lines, 0, h[0].out, 50.0, &mut num()).unwrap();
        assert_eq!(ends(&moved[0]), (P2(-100.0, -50.0), P2(100.0, -50.0)));
        assert_eq!(ends(&moved[1]), (P2(100.0, -50.0), P2(-100.0, -50.0)));
        let Some(Seg::Line { a, b, turn }) = read(&moved[1], &mut num()) else { panic!() };
        assert!(turn > 180.0, "taller, not carried along: {turn}");
        let ax = Axis::new(a, b, turn);
        assert!(near(ax.at(ax.length() / 2.0, 0.0), P2(0.0, 100.0)), "its top stays");
        // A slot's straight side out: each round end still runs through its tip and the side's new end.
        let mut slot = ring(&[(0.0, 0.0), (500.0, 0.0), (500.0, 200.0), (0.0, 200.0)]);
        for k in [1, 3] {
            if let FormCurve::Line { turn, .. } = &mut slot[k].curve {
                *turn = Some(n(180.0));
            }
        }
        let moved = offset_edge(&slot, 0, P2(0.0, -1.0), 20.0, &mut num()).unwrap();
        let Some(Seg::Line { a, b, turn }) = read(&moved[1], &mut num()) else { panic!() };
        assert_eq!(a, P2(500.0, -20.0));
        let (c, r) = arc_of(a, b, turn).unwrap();
        assert!((c.dist(P2(600.0, 100.0)) - r).abs() < 1e-6, "still through the round end's tip: {c:?} {r}");
    }

    #[test]
    fn a_corner_of_another_loop_close_by_is_not_a_neighbour() {
        // Two squares, the second's corner 0.5 mm from the first's: each edge finds its own loop's line.
        let mut lines = ring(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
        lines.extend(ring(&[(100.5, 100.0), (200.0, 100.0), (200.0, 200.0), (100.5, 200.0)]));
        let segs = read_all(&lines, &mut num()).unwrap();
        assert_eq!(neighbour(&segs, 1, P2(100.0, 100.0)), Some((2, false)));
        assert_eq!(neighbour(&segs, 4, P2(100.5, 100.0)), Some((7, true)));
    }

    #[test]
    fn a_formula_a_drag_moves_keeps_itself_with_the_move_added_and_one_it_leaves_stays() {
        // The right side at x = W: moving the top keeps W where it was; moving the right side adds the move to it, and to
        // the ends of the lines that follow, so that side alone goes and W still moves it.
        let w = || Expr::Formula("W".into());
        let corners = [[n(0.0), n(0.0)], [w(), n(0.0)], [w(), n(300.0)], [n(0.0), n(300.0)]];
        let lines = super::super::sketch::profile_lines(&corners);
        let moved = offset_edge(&lines, 2, P2(0.0, 1.0), 100.0, &mut num()).unwrap();
        match &moved[2].curve {
            FormCurve::Line { start, .. } => assert_eq!(start, &[w(), n(400.0)]),
            _ => panic!(),
        }
        let out = |s: &str| Expr::Formula(s.into());
        let moved = offset_edge(&lines, 1, P2(1.0, 0.0), 100.0, &mut num()).unwrap();
        let xs = |lines: &[FormLine]| -> Vec<Expr> {
            lines
                .iter()
                .flat_map(|l| match &l.curve {
                    FormCurve::Line { start, end, .. } => [start[0].clone(), end[0].clone()],
                    _ => panic!(),
                })
                .collect()
        };
        // The bottom's end, the right side's two ends, the top's start; the left side's x stays 0.
        let by = |s: &str| vec![n(0.0), out(s), out(s), out(s), out(s), n(0.0), n(0.0), n(0.0)];
        assert_eq!(xs(&moved), by("(W) + 100"));
        // Dragged again, the move adds to the one before; back by as much, the formula as it was typed.
        let again = offset_edge(&moved, 1, P2(1.0, 0.0), -30.0, &mut num()).unwrap();
        assert_eq!(xs(&again), by("(W) + 70"));
        let inward = offset_edge(&again, 1, P2(1.0, 0.0), -120.0, &mut num()).unwrap();
        assert_eq!(xs(&inward), by("(W) - 50"));
        let back = offset_edge(&inward, 1, P2(1.0, 0.0), 50.0, &mut num()).unwrap();
        assert_eq!(xs(&back), xs(&lines));
    }

    #[test]
    fn a_shift_is_folded_only_into_one_written_as_a_shift() {
        let f = |e: Expr| match e {
            Expr::Formula(s) => s,
            Expr::Number(_) => panic!(),
        };
        assert_eq!(f(shifted("Width / 2", 40.0)), "(Width / 2) + 40");
        assert_eq!(f(shifted("(Width / 2) + 40", 2.5)), "(Width / 2) + 42.5");
        assert_eq!(f(shifted("(Width / 2) + 40", -40.0)), "Width / 2");
        // Not a shift of ours: brackets that close before the end, or no number after them.
        assert_eq!(f(shifted("(a) * (b) + 4", 1.0)), "((a) * (b) + 4) + 1");
        assert_eq!(f(shifted("(a) + b", 1.0)), "((a) + b) + 1");
    }

    /// A slab at ±500 by ±300, a block beside it from x = 1200 to 1500, and a plane across x at 800.
    fn beside() -> (Family, BTreeMap<String, Val>, [Guid; 3]) {
        use serde_json::json;
        let g = |k: u8| -> Guid { serde_json::from_value(json!(format!("44444444-4444-4444-8444-0000000000{k:02}"))).unwrap() };
        let ring = |x: [f64; 2], first: u8| {
            json!([
                {"guid": g(first), "start": [x[0], -300], "end": [x[1], -300]},
                {"guid": g(first + 1), "start": [x[1], -300], "end": [x[1], 300]},
                {"guid": g(first + 2), "start": [x[1], 300], "end": [x[0], 300]},
                {"guid": g(first + 3), "start": [x[0], 300], "end": [x[0], -300]}
            ])
        };
        let f: Family = serde_json::from_value(json!({
            "types": {"T": {}},
            "planes": [{"guid": g(1), "name": "Right", "start": [800, -1000, 0], "end": [800, 1000, 0], "up": [0, 0, 1]}],
            "forms": [
                {"guid": g(2), "sketch": ring([-500.0, 500.0], 10), "from": 0, "to": 200},
                {"guid": g(3), "sketch": ring([1200.0, 1500.0], 20), "from": 0, "to": 450}
            ]
        }))
        .unwrap();
        let values = f.values("T", &BTreeMap::new()).unwrap();
        (f, values, [g(1), g(2), g(3)])
    }

    #[test]
    fn a_side_dragged_lines_up_with_the_planes_and_the_sides_that_face_its_way() {
        let (f, values, [plane, slab, block]) = beside();
        let line =
            |k: u8| -> Guid { serde_json::from_value(serde_json::json!(format!("44444444-4444-4444-8444-0000000000{k:02}"))).unwrap() };
        let mut across = f.handle_targets(&values, [1.0, 0.0, 0.0], slab, &|_| true);
        across.sort_by(|a, b| a.1.total_cmp(&b.1));
        let want = vec![
            (DimRef::Plane(super::super::CENTER_LEFT_RIGHT), 0.0),
            (DimRef::Plane(plane), 800.0),
            (DimRef::Sketch(block, line(23)), 1200.0),
            (DimRef::Sketch(block, line(21)), 1500.0),
        ];
        assert_eq!(across.len(), want.len(), "{across:?}");
        for ((r, at), (wr, wat)) in across.iter().zip(&want) {
            assert_eq!(r, wr);
            assert!((at - wat).abs() < 1e-6, "{r:?} at {at}");
        }
        // Up: Ref. Level and the block's Start and End; none of the slab's own.
        let mut up = f.handle_targets(&values, [0.0, 0.0, 1.0], slab, &|_| true);
        up.sort_by(|a, b| a.1.total_cmp(&b.1));
        let ats: Vec<f64> = up.iter().map(|t| t.1).collect();
        assert_eq!(ats, [0.0, 0.0, 450.0], "{up:?}");
        assert!(up.contains(&(DimRef::Form(block, Extent::End), 450.0)));
        assert!(up.iter().all(|(r, _)| !r.names(slab)));
        // What the view hides is no target.
        let shown = f.handle_targets(&values, [1.0, 0.0, 0.0], slab, &|r| *r != DimRef::Plane(plane));
        assert!(!shown.iter().any(|(r, _)| *r == DimRef::Plane(plane)));
        // The nearest within reach is taken, a plane before a side at the same place; none past it.
        assert_eq!(snapped(&across, 790.0, 20.0), Some((DimRef::Plane(plane), 800.0)));
        assert_eq!(snapped(&across, 700.0, 20.0), None);
        assert_eq!(snapped(&across, 1195.0, 20.0).map(|t| t.1), Some(1200.0));
    }

    #[test]
    fn a_drag_that_would_leave_no_profile_is_refused() {
        let lines = ring(&L);
        // The notch's floor down past the bottom: the right side turned back on itself.
        assert_eq!(offset_edge(&lines, 2, P2(0.0, 1.0), -600.0, &mut num()), Err(Refused::Invalid));
        // To nothing: the right side 0 long.
        assert_eq!(offset_edge(&lines, 2, P2(0.0, 1.0), -500.0, &mut num()), Err(Refused::Invalid));
        // The notch's wall right past the right side: the floor turned back, the top crossing it.
        assert_eq!(offset_edge(&lines, 3, P2(1.0, 0.0), 700.0, &mut num()), Err(Refused::Invalid));
        // A hole's edge out across the outline: the lines cross.
        let mut holed = ring(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]);
        holed.extend(ring(&[(400.0, 400.0), (600.0, 400.0), (600.0, 600.0), (400.0, 600.0)]));
        assert_eq!(offset_edge(&holed, 5, P2(-1.0, 0.0), -500.0, &mut num()), Err(Refused::Invalid));
    }
}
