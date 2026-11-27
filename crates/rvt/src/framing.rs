//! Structural framing, along the lines their graphics draw or from their records' boxes, and structural foundations from their
//! boxes. The same reading gives a pipe's, a duct's and a cable tray's line where their data holds none ([`centre_line`]).
//!
//! The file's record says where an element is and the box that holds it; a beam's location line, and a foundation's shape, are not
//! read from its data (that would take a finding in it). What a member's graphics draw holds its line, though: a member is a
//! straight solid of one section along its line (its ends cut where it joins others), so the faces along it are square to its
//! line, and those at its ends are what little is left. So:
//!
//! - A **framing member** whose graphics draw such a solid ([`line_of`]) is a beam along it: the line through the middle of its
//!   top, from end to end, at any angle in plan, sloped or standing; its level the one at or below its lower end; its section
//!   what it draws across that line. [`crate::imported`] then draws it as its graphics draw it.
//! - Any other member whose box runs along an axis (across it at most a third of its length, and less deep than it is long) is a
//!   beam along the box's longer horizontal side, through the middle of the box across it, its top the box's top on the level
//!   at or below it, its section the box across it and as deep as the box. A member whose box doesn't (one that runs across the
//!   axes in plan, whose box is the envelope of a diagonal; a brace or a post that stands) is left out, with why: its box would
//!   be a beam of another width and direction.
//! - A **foundation** is an isolated footing of its box: its middle in plan, its top the box's top on the level at or below it,
//!   as thick as the box, its type the box's width and depth. A wall foundation or a mat is its box too.
//!
//! The notes say which way each came in. Nothing new is read from the file: the record's category and box, the levels, and the
//! graphics the components' reading reads.

use std::collections::BTreeMap;

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::{Column, ColumnType, Element, Justify, Run, RunType};

use crate::import::{Facts, Import, kind, level_at, micron, mm, point};
use crate::inventory::Gathered;
use crate::records::Record;

/// `OST_StructuralFraming`.
pub(crate) const OST_STRUCTURAL_FRAMING: i64 = -2_001_320;
/// `OST_StructuralFoundation`.
pub(crate) const OST_STRUCTURAL_FOUNDATION: i64 = -2_001_300;

/// A size in millimetres as a type's name shows it: to the tenth, without trailing zeros.
fn size(v: f64) -> String {
    format!("{}", (v * 10.0).round() / 10.0 + 0.0)
}

/// The run a box gives along an axis: its two ends' middles and how wide across it is, in feet; none where its box runs across
/// the axes (wider than a third of its length) or stands (as deep as it is long).
pub(crate) fn along(b: &[f64; 6]) -> Option<([f64; 2], [f64; 2], f64)> {
    let (dx, dy, dz) = (b[3] - b[0], b[4] - b[1], b[5] - b[2]);
    let (length, across) = (dx.max(dy), dx.min(dy));
    if across > length / 3.0 || dz >= length {
        return None;
    }
    let (mx, my) = ((b[0] + b[3]) / 2.0, (b[1] + b[4]) / 2.0);
    Some(if dx >= dy { ([b[0], my], [b[3], my], across) } else { ([mx, b[1]], [mx, b[4]], across) })
}

/// The eigenvalues of a symmetric 3x3 matrix and their eigenvectors (the columns of the second), by Jacobi's rotations: the same
/// in every build.
fn eigen(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..64 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        let scale = a[0][0].abs() + a[1][1].abs() + a[2][2].abs();
        if off <= 1e-15 * scale.max(f64::MIN_POSITIVE) {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q] == 0.0 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = if theta == 0.0 { 1.0 } else { theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt()) };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in &mut a {
                let (kp, kq) = (row[p], row[q]);
                row[p] = c * kp - s * kq;
                row[q] = s * kp + c * kq;
            }
            for k in 0..3 {
                let (pk, qk) = (a[p][k], a[q][k]);
                a[p][k] = c * pk - s * qk;
                a[q][k] = s * pk + c * qk;
            }
            for row in &mut v {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(a, a).sqrt();
    (l.is_finite() && l > 1e-12).then(|| a.map(|v| v / l))
}

/// A straight member's line, as its solid gives it: the ends of the line through the middle of its top, millimetres, and its
/// section across that line, as wide (level across it) and as deep (square to it and to that, `up`) as what it draws; and its
/// diameter, where its section is round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Line {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub width: f64,
    pub depth: f64,
    pub up: [f64; 3],
    pub diameter: Option<f64>,
}

/// A round section's faces along it look this many ways at the least: a polygon of fewer sides is no circle.
const ROUND_SIDES: usize = 8;

/// How far from one circle round its middle (a part of its radius) a round section's corners may lie: a polygon's middle, taken
/// as its box's, is a little off the circle's.
const ROUND: f64 = 0.03;

/// The line through the middle of a run (a pipe's, a duct's, a cable tray's), feet in the project's axes, and its section, feet:
/// round, or as wide and as deep as it draws.
pub(crate) struct Centre {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub width: f64,
    pub depth: f64,
    pub round: bool,
}

/// The run element `r`'s graphics draw ([`crate::generic::member_drawn`]: its record's box holds its line, which runs on into the
/// fittings at its ends), along their middle ([`line_of`]); or why not.
pub(crate) fn centre_line(g: &Gathered, r: &Record) -> Result<Centre, String> {
    let solid = crate::generic::member_drawn(g, r)?;
    let line = line_of(&solid.triangles).ok_or("what it draws is no straight run of one section")?;
    let half = line.depth / 2.0;
    let feet = |p: [f64; 3]| std::array::from_fn(|k| (p[k] - line.up[k] * half) / 304.8);
    let (width, depth) = line.diameter.map_or((line.width, line.depth), |d| (d, d));
    Ok(Centre { start: feet(line.start), end: feet(line.end), width: width / 304.8, depth: depth / 304.8, round: line.diameter.is_some() })
}

/// The faces along a member, square to its line, hold this much of all it draws, at the least, for what is left to be taken as its
/// ends wherever it lies: cut square, mitred where it joins another, plumb and level where a rafter sits, or rounded. A solid that
/// turns or tapers holds less.
const MOSTLY: f64 = 0.9;

/// The faces along a member hold this much of all it draws at the least, where the rest lies at its ends ([`ENDS`]) or through it.
const ALONG: f64 = 0.5;

/// How far from either end of a member (times its section's greater side) a face that does not run along it may reach, where the
/// faces along it are less than [`MOSTLY`] of it. A face past that makes it no prism of one section (one that tapers or turns
/// along it), but for a hole through it, which lies within its section ([`INSIDE`]).
const ENDS: f64 = 2.0;

/// How far within the outline of a member's section (millimetres) a face that does not run along it lies, where it is a hole
/// through it: an opening in its web, a bolt's hole.
const INSIDE: f64 = 1.0;

/// How far from square to a member's first line (the sine) a face may be and still be taken as one along it, when that line is
/// found again from those faces alone: an end cut plumb or level, or rounded, pulls the first a little its way.
const NEARLY: f64 = 0.1;

/// How far from square to its line (the sine) a face along a member is, at the most: a small round run's two end circles are drawn
/// each on its own, a little turned from the other, so its sides twist a fraction of a degree off its line.
const SQUARE: f64 = 5e-3;

/// How far from its end (millimetres, and a part of its section's greater side besides) a cap that cuts a run square lies, at the
/// most: a cap may be a shallow cone, but an inner flange's face, further in, is no cap.
const CAP: (f64, f64) = (0.5, 0.02);

/// How far from square to its line (the cosine) a cap may lean, at the least.
const CAP_LEAN: f64 = 0.9;

/// A triangle's face: twice its area, its normal and the triangle's place.
type Face = (f64, [f64; 3], usize);

/// The direction in which `faces` (each one's twice area, normal and triangle) weigh least, of those `keep` keeps: the eigenvector
/// of the least eigenvalue of the sum of each one's area by its normal by itself ([`eigen`]), east first, else north, else up, so
/// a member drawn either way runs the same way.
fn least(faces: &[Face], keep: impl Fn(&[f64; 3]) -> bool) -> Option<[f64; 3]> {
    principal(faces, keep).into_iter().next()
}

/// The three directions `faces` (those `keep` keeps) weigh in, least first: the eigenvectors of the sum of each one's area by its
/// normal by itself ([`eigen`]), each east first, else north, else up.
fn principal(faces: &[Face], keep: impl Fn(&[f64; 3]) -> bool) -> Vec<[f64; 3]> {
    let mut m = [[0.0; 3]; 3];
    for (twice, u, _) in faces.iter().filter(|f| keep(&f.1)) {
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell += twice * u[r] * u[c];
            }
        }
    }
    let (values, vectors) = eigen(m);
    let mut order = [0, 1, 2];
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    order
        .into_iter()
        .filter_map(|k| unit([vectors[0][k], vectors[1][k], vectors[2][k]]))
        .map(|axis| {
            let first = axis.iter().copied().find(|v| v.abs() > 1e-9).unwrap_or(1.0);
            if first < 0.0 { axis.map(|v| -v) } else { axis }
        })
        .collect()
}

/// `axis` found again from the faces nearly along it, three times over ([`NEARLY`]): the ends no longer pull it.
fn refined(faces: &[Face], mut axis: [f64; 3]) -> Option<[f64; 3]> {
    for _ in 0..3 {
        let first = axis;
        axis = least(faces, |n| dot(*n, first).abs() < NEARLY)?;
    }
    Some(axis)
}

/// The least and the greatest of `values`.
fn range(values: impl Iterator<Item = f64>) -> (f64, f64) {
    values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

/// The line a straight member's triangles (millimetres) are drawn along: the direction its faces' normals are square to. It is
/// found from all of them ([`least`]), then again from those nearly along that ([`NEARLY`]), which its ends no longer pull. Its
/// section is what the faces along it take in across it, and its length all it draws along it. None where no direction is: a
/// solid that is no prism of one section, whose faces along it are less than [`MOSTLY`] of it and either less than [`ALONG`] or
/// with other faces past its ends that reach its section's outline ([`ENDS`]: one that tapers or turns) and are not its two caps
/// ([`CAP`]: a short run's between two fittings, cut square or a shallow cone); or one no longer than it is wide or deep, but for a
/// round one cut square at both ends, however short ([`stub`]).
pub(crate) fn line_of(tris: &[[[f64; 3]; 3]]) -> Option<Line> {
    let mut faces = Vec::with_capacity(tris.len());
    for (k, t) in tris.iter().enumerate() {
        let n = cross([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
        let twice = dot(n, n).sqrt();
        if twice.is_finite() && twice > 0.0 {
            faces.push((twice, n.map(|v| v / twice), k));
        }
    }
    let axis = refined(&faces, least(&faces, |_| true)?)?;
    drawn_along(tris, &faces, axis, false).or_else(|| stub(tris, &faces))
}

/// A round run shorter than it is wide (a pipe's or a duct's stub between two fittings): its two caps outweigh its sides, so the way
/// its faces weigh least lies across it. It runs the way one of the three its faces weigh in lies ([`principal`]: its flat caps
/// face it exactly), about which its sides are round and all else it draws is its two caps. Its two end circles may be drawn a
/// little turned from each other, which over so short a length leans its sides well off its line, so a side is any face but a cap.
fn stub(tris: &[[[f64; 3]; 3]], faces: &[Face]) -> Option<Line> {
    facing(faces).into_iter().chain(principal(faces, |_| true)).find_map(|a| drawn_along(tris, faces, a, true))
}

/// The way most of `faces`' area faces, either way: a stub's two flat caps, where it is about as long as it is wide and its faces
/// weigh about the same every way, so the three ways they weigh in ([`principal`]) are any three.
fn facing(faces: &[Face]) -> Option<[f64; 3]> {
    let mut by: BTreeMap<[i64; 3], (f64, [f64; 3])> = BTreeMap::new();
    for (twice, n, _) in faces {
        let first = n.iter().copied().find(|v| v.abs() > 1e-9).unwrap_or(1.0);
        let n = if first < 0.0 { n.map(|v| -v) } else { *n };
        let held = by.entry(n.map(|v| (v * 1e3).round() as i64)).or_insert((0.0, n));
        held.0 += twice;
    }
    by.into_values().max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, n)| n)
}

/// The line of a member drawn by `tris` (their `faces`) along `axis`, as [`line_of`] says; `short`, only where it is round and cut
/// square at both ends, whatever its length, its sides all it draws but its caps.
fn drawn_along(tris: &[[[f64; 3]; 3]], faces: &[Face], axis: [f64; 3], short: bool) -> Option<Line> {
    let lean = if short { CAP_LEAN } else { SQUARE };
    let (along, ends): (Vec<&Face>, Vec<_>) =
        faces.iter().partition(|f| if short { dot(f.1, axis).abs() <= lean } else { dot(f.1, axis).abs() < lean });
    let total: f64 = faces.iter().map(|f| f.0).sum();
    let share = along.iter().map(|f| f.0).sum::<f64>() / total;
    if share.is_nan() || (share < ALONG && !short) || along.is_empty() {
        return None;
    }
    // Across it: level, and square to that; a member that stands takes east as level across it.
    let across = unit(cross([0.0, 0.0, 1.0], axis)).filter(|_| axis[2].abs() < 1.0 - 1e-9).unwrap_or([1.0, 0.0, 0.0]);
    let up = cross(axis, across);
    let side = |d: [f64; 3]| range(along.iter().flat_map(|f| tris[f.2]).map(|p| dot(p, d)));
    let (s0, s1) = range(tris.iter().flatten().map(|p| dot(*p, axis)));
    let ((w0, w1), (u0, u1)) = (side(across), side(up));
    let (length, width, depth) = (s1 - s0, w1 - w0, u1 - u0);
    if ![length, width, depth].iter().all(|v| v.is_finite() && *v > 0.0) || (!short && (length <= width || length <= depth)) {
        return None;
    }
    // Cut square, all it draws but its faces along it is its two caps, at its two ends: it is a prism of one section however short.
    let deep = CAP.0 + CAP.1 * width.max(depth);
    let cap = |f: &&Face| {
        let (lo, hi) = range(tris[f.2].iter().map(|p| dot(*p, axis)));
        dot(f.1, axis).abs() > CAP_LEAN && (hi <= s0 + deep || lo >= s1 - deep)
    };
    let square = ends.iter().all(cap);
    if short && !square {
        return None;
    }
    // Where its faces along it are less than most of it, the rest lies at its ends, which must leave a length between them, or
    // through it.
    let reach = ENDS * width.max(depth);
    if share < MOSTLY && !square && length <= 2.0 * reach {
        return None;
    }
    let at_an_end = |t: &[[f64; 3]; 3]| {
        let (lo, hi) = range(t.iter().map(|p| dot(*p, axis)));
        hi <= s0 + reach || lo >= s1 - reach
    };
    // A hole through it (a web's openings) lies within its section; a face that tapers or turns reaches the section's outline.
    let within = |t: &[[f64; 3]; 3]| {
        t.iter().all(|p| {
            let (w, u) = (dot(*p, across), dot(*p, up));
            w0 + INSIDE < w && w < w1 - INSIDE && u0 + INSIDE < u && u < u1 - INSIDE
        })
    };
    if share < MOSTLY && !square && !ends.iter().all(|f| at_an_end(&tris[f.2]) || within(&tris[f.2])) {
        return None;
    }
    // Round where the faces along it look many ways and their corners lie on one circle round its middle.
    let (wc, uc) = ((w0 + w1) / 2.0, (u0 + u1) / 2.0);
    let radii = range(along.iter().flat_map(|f| tris[f.2]).map(|p| (dot(p, across) - wc).hypot(dot(p, up) - uc)));
    let mut ways: Vec<[f64; 3]> = Vec::new();
    for f in &along {
        if ways.len() < ROUND_SIDES && !ways.iter().any(|n| dot(*n, f.1) > 0.9999) {
            ways.push(f.1);
        }
    }
    let round = ways.len() >= ROUND_SIDES && radii.1 > 0.0 && radii.1 - radii.0 <= ROUND * radii.1;
    if short && !round {
        return None;
    }
    let at = |s: f64| std::array::from_fn(|i| micron(axis[i] * s + across[i] * wc + up[i] * u1));
    let diameter = round.then(|| micron(2.0 * radii.1));
    Some(Line { start: at(s0), end: at(s1), width: micron(width), depth: micron(depth), up, diameter })
}

/// Makes the beam of member `r` from `start` to `end` in plan, its top `offset` (and `end_offset`) over `level`, of a section
/// `width` by `depth`.
fn beam(
    cx: &mut Import,
    r: u32,
    (start, end): (P2, P2),
    (offset, end_offset): (f64, Option<f64>),
    level: Guid,
    (width, depth): (f64, f64),
) {
    let types = cx.model.types.runs.entry("beam".to_string()).or_default();
    let new = RunType { width, depth, round: false, material: String::new(), profile: None, clear_section: None };
    let same = |t: &RunType| !t.round && (t.width - width).abs() < 0.05 && (t.depth - depth).abs() < 0.05 && t.material.is_empty();
    let type_name = kind(types, &format!("Imported {} x {}", size(width), size(depth)), same, new);
    let run = Run {
        imported: None,
        type_name,
        level,
        start,
        end,
        offset,
        end_offset,
        justify: Justify::Top,
        join: true,
        system: String::new(),
        params: Default::default(),
    };
    let Some(made) = Element::of_run("beam", run) else { return };
    let id = cx.model.insert(made);
    cx.from.insert(id, format!("Structural framing member {r}"));
}

/// Makes the beam member `r`'s graphics draw, along their line ([`line_of`]); or says why not.
fn drawn(cx: &mut Import, f: &Facts, g: &Gathered, r: &Record) -> Result<(), String> {
    let solid = crate::generic::member_drawn(g, r)?;
    let line = line_of(&solid.triangles).ok_or("what it draws is no straight member of one section")?;
    let (low, high) = if line.start[2] <= line.end[2] { (line.start[2], line.end[2]) } else { (line.end[2], line.start[2]) };
    let (level, elevation) = level_at(cx, f, low / 304.8)?;
    let base = mm(elevation);
    let (offset, end_offset) = (micron(line.start[2] - base), micron(line.end[2] - base));
    let sloped = (high - low).abs() >= 0.001;
    let ends = (P2(line.start[0], line.start[1]), P2(line.end[0], line.end[1]));
    beam(cx, r.id, ends, (offset, sloped.then_some(end_offset)), level, (line.width, line.depth));
    Ok(())
}

pub(crate) fn import(cx: &mut Import, f: &Facts, g: &Gathered) {
    for r in f.of_category(OST_STRUCTURAL_FRAMING) {
        let label = "structural framing member";
        let why = match drawn(cx, f, g, r) {
            Ok(()) => {
                cx.note(label, r.id, "comes in as a beam along the line its graphics draw, their section across it: its type is not read");
                continue;
            }
            Err(why) => why,
        };
        let Some((a, b, across)) = along(&r.bbox) else {
            let mut reason = String::from(
                "its box is no beam's: it runs across the axes in plan or stands, and its line is not read, so a beam of its box would \
                 have another width and direction",
            );
            if why != crate::generic::NO_GRAPHICS {
                reason.push_str("; nor is it read from its graphics: ");
                reason.push_str(&why);
            }
            cx.left_out(label, r.id, &reason);
            continue;
        };
        let (level, elevation) = match level_at(cx, f, r.bbox[5]) {
            Ok(l) => l,
            Err(why) => {
                cx.left_out(label, r.id, &why);
                continue;
            }
        };
        let (width, depth) = (mm(across), mm(r.bbox[5] - r.bbox[2]));
        let ends = (point(a[0], a[1]), point(b[0], b[1]));
        beam(cx, r.id, ends, (micron(mm(r.bbox[5]) - mm(elevation)), None), level, (width, depth));
        cx.note(
            label,
            r.id,
            "comes in as a beam along its box's longer side, its section the box across it: its line, slope, section and type are not read",
        );
    }
    for r in f.of_category(OST_STRUCTURAL_FOUNDATION) {
        let label = "structural foundation";
        let (level, elevation) = match level_at(cx, f, r.bbox[5]) {
            Ok(l) => l,
            Err(why) => {
                cx.left_out(label, r.id, &why);
                continue;
            }
        };
        let (width, depth) = (mm(r.bbox[3] - r.bbox[0]), mm(r.bbox[4] - r.bbox[1]));
        let same = |t: &ColumnType| !t.round && (t.width - width).abs() < 0.05 && (t.depth - depth).abs() < 0.05;
        let new = ColumnType { width, depth, round: false, profile: None };
        let type_name = kind(&mut cx.model.types.footing, &format!("Imported {} x {}", size(width), size(depth)), same, new);
        let footing = Column {
            imported: None,
            room_bounding: true,
            type_name,
            level,
            at: point((r.bbox[0] + r.bbox[3]) / 2.0, (r.bbox[1] + r.bbox[4]) / 2.0),
            angle: 0.0,
            top_level: None,
            height: mm(r.bbox[5] - r.bbox[2]),
            base_offset: micron(mm(r.bbox[2]) - mm(elevation)),
            top_offset: 0.0,
            attach_top: None,
            attach_base: None,
            params: Default::default(),
        };
        let id = cx.model.insert(Element::Footing(footing));
        cx.from.insert(id, format!("Structural foundation {}", r.id));
        cx.note(label, r.id, "comes in as an isolated footing of its box: its shape and type are not read");
    }
}
