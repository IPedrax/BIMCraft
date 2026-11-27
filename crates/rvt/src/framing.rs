//! Structural framing, along the lines their graphics draw or from their records' boxes, and structural foundations from their
//! boxes.
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
/// section across that line, as wide (level across it) and as deep (square to it and to that) as what it draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Line {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub width: f64,
    pub depth: f64,
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

/// How far from square to its line (the sine) a face along a member is, at the most.
const SQUARE: f64 = 1e-3;

/// A triangle's face: twice its area, its normal and the triangle's place.
type Face = (f64, [f64; 3], usize);

/// The direction in which `faces` (each one's twice area, normal and triangle) weigh least, of those `keep` keeps: the eigenvector
/// of the least eigenvalue of the sum of each one's area by its normal by itself ([`eigen`]), east first, else north, else up, so
/// a member drawn either way runs the same way.
fn least(faces: &[Face], keep: impl Fn(&[f64; 3]) -> bool) -> Option<[f64; 3]> {
    let mut m = [[0.0; 3]; 3];
    for (twice, u, _) in faces.iter().filter(|f| keep(&f.1)) {
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell += twice * u[r] * u[c];
            }
        }
    }
    let (values, vectors) = eigen(m);
    let k = (0..3).min_by(|&a, &b| values[a].total_cmp(&values[b]))?;
    let axis = unit([vectors[0][k], vectors[1][k], vectors[2][k]])?;
    let first = axis.iter().copied().find(|v| v.abs() > 1e-9).unwrap_or(1.0);
    Some(if first < 0.0 { axis.map(|v| -v) } else { axis })
}

/// The least and the greatest of `values`.
fn range(values: impl Iterator<Item = f64>) -> (f64, f64) {
    values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

/// The line a straight member's triangles (millimetres) are drawn along: the direction its faces' normals are square to. It is
/// found from all of them ([`least`]), then again from those nearly along that ([`NEARLY`]), which its ends no longer pull. Its
/// section is what the faces along it take in across it, and its length all it draws along it. None where no direction is: a
/// solid that is no prism of one section, whose faces along it are less than [`MOSTLY`] of it and either less than [`ALONG`] or
/// with other faces past its ends that reach its section's outline ([`ENDS`]: one that tapers or turns); or one no longer than it
/// is wide or deep.
pub(crate) fn line_of(tris: &[[[f64; 3]; 3]]) -> Option<Line> {
    let mut faces = Vec::with_capacity(tris.len());
    for (k, t) in tris.iter().enumerate() {
        let n = cross([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
        let twice = dot(n, n).sqrt();
        if twice.is_finite() && twice > 0.0 {
            faces.push((twice, n.map(|v| v / twice), k));
        }
    }
    let mut axis = least(&faces, |_| true)?;
    for _ in 0..3 {
        let first = axis;
        axis = least(&faces, |n| dot(*n, first).abs() < NEARLY)?;
    }
    let (along, ends): (Vec<&Face>, Vec<_>) = faces.iter().partition(|f| dot(f.1, axis).abs() < SQUARE);
    let total: f64 = faces.iter().map(|f| f.0).sum();
    let share = along.iter().map(|f| f.0).sum::<f64>() / total;
    if share.is_nan() || share < ALONG {
        return None;
    }
    // Across it: level, and square to that; a member that stands takes east as level across it.
    let across = unit(cross([0.0, 0.0, 1.0], axis)).filter(|_| axis[2].abs() < 1.0 - 1e-9).unwrap_or([1.0, 0.0, 0.0]);
    let up = cross(axis, across);
    let side = |d: [f64; 3]| range(along.iter().flat_map(|f| tris[f.2]).map(|p| dot(p, d)));
    let (s0, s1) = range(tris.iter().flatten().map(|p| dot(*p, axis)));
    let ((w0, w1), (u0, u1)) = (side(across), side(up));
    let (length, width, depth) = (s1 - s0, w1 - w0, u1 - u0);
    if ![length, width, depth].iter().all(|v| v.is_finite() && *v > 0.0) || length <= width || length <= depth {
        return None;
    }
    // Where its faces along it are less than most of it, the rest lies at its ends, which must leave a length between them, or
    // through it.
    let reach = ENDS * width.max(depth);
    if share < MOSTLY && length <= 2.0 * reach {
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
    if share < MOSTLY && !ends.iter().all(|f| at_an_end(&tris[f.2]) || within(&tris[f.2])) {
        return None;
    }
    let at = |s: f64| std::array::from_fn(|i| micron(axis[i] * s + across[i] * (w0 + w1) / 2.0 + up[i] * u1));
    Some(Line { start: at(s0), end: at(s1), width: micron(width), depth: micron(depth) })
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
