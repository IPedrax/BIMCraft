//! The delivery drawing rules a plan's DXF keeps under the `delivery` CAD profile: each room's outline closed on
//! `A-AREA-BNDY` (the room separation lines are not drawn: the outlines of the rooms they part meet along them), a door's
//! swing an `ARC` rather than the chords it is drawn as in the picture, and a block's attributes only its `MARK` (in
//! [`super::blocks`]).

use std::collections::BTreeSet;

use super::*;
use crate::family::Mark;
use crate::rooms::Rooms;

/// The layer a room's outline goes on.
const OUTLINE_LAYER: &str = "A-AREA-BNDY";

/// How far a stroke's points may lie from an arc's chords, mm, and still be that swing: the plan is in pixels of `f32`.
const NEAR: f64 = 1.0;

/// An outline's pen: hundredths of a mm, and its colour.
const OUTLINE_WEIGHT: i32 = 18;
const OUTLINE_COLOR: Rgb = Rgb(0x19, 0x19, 0x19);

/// The outline of each room the drawing letters or draws, closed, written to `ents` in model space; the drawing's extents grown
/// by them. A room that isn't enclosed has none. `shift` is how far the drawing has been moved.
pub(super) fn outlines(d: &Drawing, m: &Model, shift: P2, sh: &mut Shared, ents: &mut String, mut ext: Option<[P2; 2]>) -> Option<[P2; 2]> {
    let mut rooms: BTreeSet<Id> = d.lettering.iter().filter_map(|l| l.owner).collect();
    rooms.extend(d.owners.iter().flatten().copied());
    let found = Rooms::new(m);
    for id in rooms {
        let Some(Element::Room(room)) = m.elements.get(id) else { continue };
        let Some(region) = found.region(room) else { continue };
        let mut pts: Vec<P2> = region.outer.iter().map(|p| P2(p.0 + shift.0, p.1 + shift.1)).collect();
        if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) < 0.01) {
            pts.pop();
        }
        if pts.len() < 3 || !finite(&pts) {
            continue;
        }
        let ink = Ink {
            layer: OUTLINE_LAYER.to_string(),
            ltype: None,
            color: OUTLINE_COLOR,
            shape: Shape::Line { pts, closed: true, weight: OUTLINE_WEIGHT },
        };
        ext = ink.grow(ext);
        sh.layers.insert(ink.layer.clone());
        put(ents, sh.w.handle(), MODEL_SPACE, &ink);
    }
    ext
}

/// The arcs of door or window `owner`'s swing, as its centre and the chords it is drawn as, in the model's axes.
pub(super) fn swings(m: &Model, owner: Id) -> Vec<(P2, Vec<P2>)> {
    let Some(e @ (Element::Door(_) | Element::Window(_))) = m.elements.get(owner) else { return Vec::new() };
    m.opening_marks(e)
        .into_iter()
        .filter_map(|mark| match mark {
            Mark::Arc { centre, pts } if pts.len() >= 2 => Some((centre, pts)),
            _ => None,
        })
        .collect()
}

/// Whether `pts` is `chords` drawn, or the part of them something drawn over the swing has left: every point on the chords (to
/// within [`NEAR`]), in order along them one way. A cut stroke may begin and end between two chords' ends.
fn is_swing(chords: &[P2], pts: &[P2]) -> bool {
    if chords.len() == pts.len() && chords.iter().zip(pts).all(|(c, p)| c.dist(*p) <= NEAR) {
        return true;
    }
    // Where a point stands along the chords: the index of the chord it is nearest, and how far along that chord.
    let along = |p: P2| -> Option<f64> {
        let mut best: Option<(f64, f64)> = None;
        for (i, w) in chords.windows(2).enumerate() {
            let d = crate::geom::dist_to_segment(p, w[0], w[1]);
            let (u, v) = (p - w[0], w[1] - w[0]);
            let len2 = v.0 * v.0 + v.1 * v.1;
            let t = if len2 > 0.0 { ((u.0 * v.0 + u.1 * v.1) / len2).clamp(0.0, 1.0) } else { 0.0 };
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i as f64 + t));
            }
        }
        best.filter(|b| b.0 <= NEAR).map(|b| b.1)
    };
    let at: Option<Vec<f64>> = pts.iter().map(|p| along(*p)).collect();
    let Some(at) = at else { return false };
    let eps = 1e-3;
    pts.len() >= 2 && (at.windows(2).all(|w| w[1] >= w[0] - eps) || at.windows(2).all(|w| w[1] <= w[0] + eps))
}

/// `ink` as the arc it is the chords of, if one of `arcs` (moved by `shift` like the drawing) is: counter-clockwise from the
/// angle of its first end to its last, or the other way round for one drawn clockwise (a mirrored door). A swing that something
/// has cut into pieces is the whole arc in each piece: the pieces make one arc ([`super::blocks`] keeps it once).
pub(super) fn as_arc(ink: &Ink, arcs: &[(P2, Vec<P2>)], shift: P2) -> Option<Ink> {
    let Shape::Line { pts, closed: false, weight } = &ink.shape else { return None };
    arcs.iter().find_map(|(centre, chords)| {
        let moved = |p: &P2| P2(p.0 + shift.0, p.1 + shift.1);
        let chords: Vec<P2> = chords.iter().map(moved).collect();
        if !is_swing(&chords, pts) {
            return None;
        }
        let c = moved(centre);
        // The whole swing's ends, not the stroke's: whatever cut a piece off (a room's lettering over it) does not shorten the arc.
        let (a, b, z) = (chords[0] - c, chords[1] - c, chords[chords.len() - 1] - c);
        let radius = a.0.hypot(a.1);
        if !(radius.is_finite() && radius > 0.0) {
            return None;
        }
        let angle = |v: P2| v.1.atan2(v.0).to_degrees().rem_euclid(360.0);
        let turns_left = a.0 * b.1 - a.1 * b.0 > 0.0;
        let (from, to) = if turns_left { (angle(a), angle(z)) } else { (angle(z), angle(a)) };
        Some(Ink {
            layer: ink.layer.clone(),
            ltype: ink.ltype.clone(),
            color: ink.color,
            shape: Shape::Arc { centre: c, radius, from, to, weight: *weight },
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter circle of 25 chords about the origin, radius 800, from 90 degrees round to 180.
    fn swing() -> (P2, Vec<P2>) {
        let pts = (0..=24).map(|i| {
            let a = (90.0 + 90.0 * i as f64 / 24.0f64).to_radians();
            P2(800.0 * a.cos(), 800.0 * a.sin())
        });
        (P2(0.0, 0.0), pts.collect())
    }

    fn ink(pts: Vec<P2>) -> Ink {
        Ink { layer: "A-DOOR".into(), ltype: None, color: Rgb(0, 0, 0), shape: Shape::Line { pts, closed: false, weight: 18 } }
    }

    #[test]
    fn a_whole_swing_is_an_arc() {
        let (c, chords) = swing();
        let got = as_arc(&ink(chords.clone()), &[(c, chords)], P2(0.0, 0.0)).expect("an arc");
        let Shape::Arc { from, to, radius, .. } = got.shape else { panic!("not an arc") };
        assert!((from - 90.0).abs() < 0.1 && (to - 180.0).abs() < 0.1 && (radius - 800.0).abs() < 0.1, "{from} {to} {radius}");
    }

    /// A swing something drawn over it has cut into pieces (a room's lettering) is the whole arc in each piece, so a block keeps one.
    #[test]
    fn a_swing_cut_into_pieces_is_one_whole_arc() {
        let (c, chords) = swing();
        let mid = |i: usize| P2((chords[i].0 + chords[i + 1].0) / 2.0, (chords[i].1 + chords[i + 1].1) / 2.0);
        // The first piece ends in the middle of the fourth chord; the second begins in the middle of the tenth.
        let mut first: Vec<P2> = chords[..=3].to_vec();
        first.push(mid(3));
        let mut second = vec![mid(9)];
        second.extend(chords[10..].iter().copied());
        for piece in [first, second] {
            let got = as_arc(&ink(piece), &[(c, chords.clone())], P2(0.0, 0.0)).expect("an arc");
            let Shape::Arc { from, to, radius, .. } = got.shape else { panic!("not an arc") };
            assert!((from - 90.0).abs() < 0.1 && (to - 180.0).abs() < 0.1 && (radius - 800.0).abs() < 0.1, "{from} {to} {radius}");
        }
    }

    #[test]
    fn a_stroke_off_the_circle_is_not_an_arc() {
        let (c, chords) = swing();
        let line = vec![P2(0.0, 0.0), P2(-500.0, 500.0), P2(-800.0, 0.0)];
        assert!(as_arc(&ink(line), &[(c, chords)], P2(0.0, 0.0)).is_none());
    }
}
