//! Walls of other shapes, through what they make: slanted walls, curved slanted ones, an edited profile, and curved and slanted
//! curtain walls, in the 3D scene, a plan, a section and IFC.

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};
use crate::render::{Prim, View, draw};
use crate::view3d::{Solid, solids};

fn session(script: &str) -> Session {
    let mut s = Session::new();
    s.script(script).unwrap_or_else(|e| panic!("{e}"));
    s
}

fn walls(s: &Session) -> Vec<Id> {
    s.model.elements.iter().filter_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).collect()
}

/// The points of the solids of element `id`, in the model.
fn points(s: &Session, id: Id) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for solid in solids(&s.model).iter().filter(|x| x.owner() == id) {
        let mut tris = Vec::new();
        solid.tris(&mut tris);
        out.extend(tris.into_iter().flat_map(|t| t.p));
    }
    out
}

/// The plan of the first level, round `centre`, at 1 px per 10 mm.
fn plan(s: &Session, centre: P2) -> crate::render::Drawing {
    let level = s.model.levels()[0].0;
    draw(&s.model, level, View { angle: 0.0, scale: 0.1, cx: centre.0, cy: centre.1, width: 1400, height: 1400 })
}

/// The span across `y` of what `id` draws in a plan, in model mm (the view's y runs down).
fn plan_span_y(d: &crate::render::Drawing, id: Id, view: (P2, f64)) -> (f64, f64) {
    let (centre, scale) = view;
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (p, o) in d.prims.iter().zip(&d.owners) {
        if *o != Some(id) {
            continue;
        }
        if let Prim::Fill { .. } = p {
            for (_, y) in p.points() {
                let model = centre.1 - (f64::from(y) - 700.0) / scale;
                lo = lo.min(model);
                hi = hi.max(model);
            }
        }
    }
    (lo, hi)
}

#[test]
fn a_slanted_wall_leans_in_3d_and_a_plan_cuts_it_where_it_has_leant_to() {
    let s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": 20}"#);
    let id = walls(&s)[0];
    let pts = points(&s, id);
    let lean = 3000.0 * 20f64.to_radians().tan();
    let top_y = pts.iter().filter(|p| (p[2] - 3000.0).abs() < 1e-6).map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    assert!((top_y - (lean + 100.0 / 20f64.to_radians().cos())).abs() < 1e-6, "{top_y}");
    // The plan cuts at 1200 mm: the wall has leant 437 mm north by then.
    let d = plan(&s, P2(3000.0, 0.0));
    let (lo, hi) = plan_span_y(&d, id, (P2(3000.0, 0.0), 0.1));
    let at = 1200.0 * 20f64.to_radians().tan();
    assert!((lo - (at - 106.4)).abs() < 15.0 && (hi - (at + 106.4)).abs() < 15.0, "{lo} {hi}");
}

#[test]
fn a_slanted_wall_with_a_window_goes_to_ifc_as_face_sets_and_its_window_leans_with_it() {
    let s = session(
        r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": -15}
           window {"wall": 100002, "offset": 3000, "slanted": true}"#,
    );
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCWALL("), "a wall");
    assert!(ifc.contains("IFCTRIANGULATEDFACESET"), "of face sets");
    assert!(ifc.contains("IFCOPENINGELEMENT"), "its window's opening");
    // The window stands where the wall has leant to at its sill (south, as it leans to its interior).
    let window = s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Window(_)).then_some(*id)).unwrap_or_default();
    let e = s.model.elements.get(window).unwrap_or_else(|| panic!("no window"));
    let (Element::Door(o) | Element::Window(o)) = e else { panic!("not a window") };
    let (at, _, z) = s.model.opening_frame(o).unwrap_or_else(|| panic!("no frame"));
    assert!((at.1 + z * 15f64.to_radians().tan()).abs() < 1e-6, "{at:?} at {z}");
}

#[test]
fn an_edited_profile_draws_in_plan_only_where_it_reaches_the_cut_plane() {
    let mut s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}"#);
    let id = walls(&s)[0];
    // A wall stepping down to 1000 mm over its last half: a plan cutting at 1200 sees its first half only.
    s.execute(
        "modify",
        &serde_json::json!({"id": id, "profile": [[[0, 0], [6000, 0], [6000, 1000], [3000, 1000], [3000, 3000], [0, 3000]]]}),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let d = plan(&s, P2(3000.0, 0.0));
    let xs: Vec<f64> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Fill { .. }))
        .flat_map(|(p, _)| p.points())
        .map(|(x, _)| 3000.0 + (f64::from(x) - 700.0) / 0.1)
        .collect();
    let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!((hi - 3000.0).abs() < 15.0, "{hi}");
    // In 3D it reaches 3000 only over its first half.
    let pts = points(&s, id);
    assert!(pts.iter().filter(|p| p[2] > 2999.0).all(|p| p[0] <= 3000.0 + 1e-6));
    // And to IFC as shells.
    assert!(crate::ifc::export(&s.model).contains("IFCFACETEDBREP") || crate::ifc::export(&s.model).contains("IFCCLOSEDSHELL"));
}

const CW: &str = r#"type {"category": "wall", "name": "CW", "curtain": {"columns": {"segments": [1500]}, "rows": {"segments": [1500]}}}"#;

#[test]
fn a_curved_curtain_wall_stands_in_3d_on_chords_of_its_arc_and_goes_to_ifc() {
    let s = session(&format!("{CW}\nwall {{\"points\": [[-5000, 0], [5000, 0]], \"arc\": 180, \"type\": \"CW\", \"height\": 3000}}"));
    let id = walls(&s)[0];
    let pts = points(&s, id);
    assert!(!pts.is_empty(), "it has parts");
    // Every part stands within a mullion's depth of the arc, inside its circle or just outside it.
    let r = |p: &[f64; 3]| (p[0] * p[0] + p[1] * p[1]).sqrt();
    assert!(pts.iter().all(|p| r(p) > 5000.0 - 400.0 && r(p) < 5000.0 + 400.0), "near the arc");
    // Its panels are flat: the parts of one panel share one plane, which is a chord's.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCCURTAINWALL("));
    assert!(ifc.matches("IFCPLATE(").count() >= 20, "{}", ifc.matches("IFCPLATE(").count());
    // A plan draws its parts round the arc.
    let d = plan(&s, P2(0.0, -2500.0));
    assert!(d.owners.iter().filter(|o| **o == Some(id)).count() > 20);
}

#[test]
fn a_slanted_curtain_wall_tilts_its_parts_up_its_face() {
    let s = session(&format!("{CW}\nwall {{\"points\": [[0, 0], [6000, 0]], \"type\": \"CW\", \"height\": 3000, \"slant\": 25}}"));
    let id = walls(&s)[0];
    let pts = points(&s, id);
    let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    // Leaning north 25°: its top is about 1400 mm north of its base.
    assert!(hi - lo > 1300.0, "{lo} {hi}");
    let tops = pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    assert!(tops <= 3000.0 + 120.0, "{tops}");
    assert!(solids(&s.model).iter().any(|x| x.owner() == id && matches!(x, Solid::Mesh { .. })), "tilted parts are meshes");
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCCURTAINWALL(") && ifc.contains("IFCPLATE("));
}

#[test]
fn walls_joined_at_a_corner_of_unlike_heights_meet_in_plans_above_the_lower_square() {
    let s = session(
        r#"wall {"points": [[4000, 0], [0, 0]], "height": 1000}
           wall {"points": [[0, 0], [0, 4000]], "height": 3000}"#,
    );
    let tall = walls(&s)[1];
    // The plan cuts at 1200, over the lower wall: the taller one's end is square, closing the corner.
    let d = plan(&s, P2(0.0, 0.0));
    let (lo, _) = plan_span_y(&d, tall, (P2(0.0, 0.0), 0.1));
    assert!((lo + 100.0).abs() < 15.0, "{lo}");
}

#[test]
fn a_plan_sees_a_slanted_walls_foot_below_its_cut_in_projection() {
    // Leaning 20° north, 3000 high: cut at 1200 mm, 437 mm north of its foot; its foot's faces at y = ±106 are seen from above.
    let s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": 20}"#);
    let id = walls(&s)[0];
    let d = plan(&s, P2(3000.0, 0.0));
    let model_y = |y: f32| 0.0 - (f64::from(y) - 700.0) / 0.1;
    let half = 100.0 / 20f64.to_radians().cos();
    let foot: Vec<f64> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Line { .. }))
        .flat_map(|(p, _)| p.points())
        .map(|(_, y)| model_y(y))
        .filter(|y| (y + half).abs() < 15.0)
        .collect();
    assert!(!foot.is_empty(), "the foot's outer face, in projection");
}

/// How much of `a` `b` covers, mm².
fn overlap(a: &[P2], b: &[P2]) -> f64 {
    let outside: f64 = crate::walls::minus(&[a.to_vec()], b).iter().map(|r| crate::geom::area(r).abs()).sum();
    crate::geom::area(a).abs() - outside
}

/// Whether ring `r` crosses itself.
fn crosses_itself(r: &[P2]) -> bool {
    let e: Vec<(P2, P2)> = crate::geom::ring_edges(r).collect();
    let n = e.len();
    (0..n).any(|i| ((i + 2)..n).any(|j| !(i == 0 && j == n - 1) && crate::geom::crossing(e[i].0, e[i].1, e[j].0, e[j].1).is_some()))
}

/// Whether two rings touch: a corner of one on an edge of the other.
fn touch(a: &[P2], b: &[P2]) -> bool {
    a.iter().any(|p| crate::geom::ring_edges(b).any(|(x, y)| crate::geom::dist_to_segment(*p, x, y) < 1e-3))
}

/// Wall `id`'s outline at the heights a join is looked at, and its pieces in 3D.
fn joined(s: &Session, a: Id, b: Id, what: &str, touching: bool) {
    let rings = |id: Id| [1.0, 1000.0, 2000.0, 2999.0].map(|z| crate::walls::outline_at(&s.model, id, z));
    let (ra, rb) = (rings(a), rings(b));
    for (k, (oa, ob)) in ra.iter().zip(&rb).enumerate() {
        for (o, which) in [(oa, "a"), (ob, "b")] {
            assert!(!crosses_itself(o), "{what}, height {k}, wall {which} crosses itself: {o:?}");
            assert!(crate::geom::area(o).abs() > 1e5, "{what}, height {k}, wall {which} is all but gone: {o:?}");
        }
        let both = overlap(oa, ob);
        assert!(both < 1.0, "{what}, height {k}: they overlap by {both} mm²\\n{oa:?}\\n{ob:?}");
        if touching {
            assert!(touch(oa, ob) || touch(ob, oa), "{what}, height {k}: a gap between them\\n{oa:?}\\n{ob:?}");
        }
    }
    // The same shape all the way up: its corners only move.
    for r in [&ra, &rb] {
        assert!(r.iter().all(|o| o.len() == r[0].len()), "{what}: its outline changes shape as it rises: {r:?}");
    }
    // In 3D each is whole: lofted in one piece where its ends move as it rises (an upright one too, where it ends on or meets a
    // slanted one), else its prisms (an upright wall a slanted one ends on: nothing of it moves).
    for id in [a, b] {
        if crate::walls::slant::lofted(&s.model, id, None) {
            let pieces = crate::walls::slant::pieces(&s.model, id, true, None);
            assert_eq!(pieces.len(), 1, "{what}: wall {id} in 3D is {} pieces", pieces.len());
        } else {
            assert!(!crate::walls::prisms(&s.model, id, true).is_empty(), "{what}: wall {id} has no solid");
        }
    }
}

#[test]
fn a_slanted_wall_meets_walls_at_any_angle_in_a_corner_that_closes_at_every_height() {
    for degrees in [60.0, 75.0, 90.0, 105.0, 120.0, 135.0, 150.0, 165.0] {
        for (sa, sb) in [(15.0, 0.0), (0.0, 15.0), (15.0, 15.0), (-15.0, 15.0), (15.0, -15.0), (-15.0, -15.0), (20.0, 10.0)] {
            let r = f64::to_radians(degrees);
            let s = session(&format!(
                r#"wall {{"points": [[8000, 0], [0, 0]], "height": 3000, "slant": {sa}}}
                   wall {{"points": [[0, 0], [{}, {}]], "height": 3000, "slant": {sb}}}"#,
                8000.0 * r.cos(),
                8000.0 * r.sin()
            ));
            // Nearly in line and leaning unlike (opposite ways, or one at 165°), they part as they rise rather than meet in a spike
            // metres long: no overlap, but no contact either.
            let apart = (degrees >= 150.0 && sa * sb < 0.0) || (degrees >= 165.0 && sa != sb);
            joined(&s, 100002, 100003, &format!("an L of {degrees}°, leaning {sa}° and {sb}°"), !apart);
        }
    }
}

#[test]
fn sharp_corners_of_slanted_walls_keep_their_shape_up_the_wall() {
    // Under 60° the faces outside the corner cross too far back: the ends square off, as upright walls' do, at every height.
    for degrees in [30.0, 45.0] {
        for (sa, sb) in [(15.0, 0.0), (15.0, 15.0), (-15.0, 15.0)] {
            let r = f64::to_radians(degrees);
            let s = session(&format!(
                r#"wall {{"points": [[4000, 0], [0, 0]], "height": 3000, "slant": {sa}}}
                   wall {{"points": [[0, 0], [{}, {}]], "height": 3000, "slant": {sb}}}"#,
                4000.0 * r.cos(),
                4000.0 * r.sin()
            ));
            let rings = |id: Id| [1.0, 1500.0, 2999.0].map(|z| crate::walls::outline_at(&s.model, id, z));
            for id in [100002, 100003] {
                let r = rings(id);
                assert!(r.iter().all(|o| o.len() == r[0].len() && !crosses_itself(o)), "{degrees}°, {sa}°/{sb}°: {r:?}");
                if s.model.wall(id).is_some_and(|w| w.slant != 0.0) {
                    assert_eq!(crate::walls::slant::pieces(&s.model, id, true, None).len(), 1, "{degrees}°, {sa}°/{sb}°: wall {id}");
                }
            }
        }
    }
}

#[test]
fn a_wall_ending_on_a_slanted_one_stops_at_its_face_at_every_height_whatever_the_angle() {
    for degrees in [45.0, 60.0, 90.0, 120.0, 135.0] {
        for (sa, sb) in [(15.0, 0.0), (-15.0, 0.0), (15.0, 15.0), (15.0, -10.0), (0.0, 15.0)] {
            let r = f64::to_radians(degrees);
            let s = session(&format!(
                r#"wall {{"points": [[-8000, 0], [8000, 0]], "height": 3000, "slant": {sa}}}
                   wall {{"points": [[0, 0], [{}, {}]], "height": 3000, "slant": {sb}}}"#,
                8000.0 * r.cos(),
                8000.0 * r.sin()
            ));
            joined(&s, 100002, 100003, &format!("a T at {degrees}°, the through wall leaning {sa}°, the other {sb}°"), true);
        }
    }
}

#[test]
fn three_walls_one_slanted_share_their_node_at_every_height() {
    let s = session(
        r#"wall {"points": [[0, 0], [3000, 0]], "height": 3000, "slant": 15}
           wall {"points": [[0, 0], [-1500, 2598.0762]], "height": 3000}
           wall {"points": [[0, 0], [-1500, -2598.0762]], "height": 3000, "slant": -10}"#,
    );
    for z in [1.0, 1500.0, 2999.0] {
        let o: Vec<Vec<P2>> = [100002, 100003, 100004].map(|id| crate::walls::outline_at(&s.model, id, z)).to_vec();
        for (i, a) in o.iter().enumerate() {
            assert!(!crosses_itself(a), "{z}: {a:?}");
            for b in o.iter().skip(i + 1) {
                assert!(overlap(a, b) < 1.0, "{z}: overlap {}\\n{a:?}\\n{b:?}", overlap(a, b));
            }
        }
    }
}

/// The corners of wall `id`'s pieces at height `z`, in plan.
fn corners_at(s: &Session, id: Id, z: f64) -> Vec<P2> {
    crate::walls::slant::pieces(&s.model, id, true, None)
        .iter()
        .flatten()
        .flatten()
        .filter(|p| (p[2] - z).abs() < 1e-6)
        .map(|p| P2(p[0], p[1]))
        .collect()
}

#[test]
fn a_short_return_still_closes_the_corner_of_a_steeply_slanted_wall() {
    // Revit's three: a long wall leaning 30° with a 3 m return, leaning against it, with it, or upright. The corner moves 1.7 m by
    // the top, more than half the return: it still closes.
    for sb in [-30.0, 30.0, 0.0] {
        let s = session(&format!(
            r#"wall {{"points": [[8000, 0], [0, 0]], "height": 3000, "slant": 30}}
               wall {{"points": [[0, 0], [0, 3000]], "height": 3000, "slant": {sb}}}"#
        ));
        joined(&s, 100002, 100003, &format!("a 3 m return leaning {sb}°"), true);
    }
}

#[test]
fn an_upright_wall_joined_to_a_slanted_one_follows_its_face_up_the_wall() {
    // As Revit's: the upright wall's end leans with the slanted wall's face, a sloped end, not a square one.
    let s = session(
        r#"wall {"points": [[6000, 0], [0, 0]], "height": 3000, "slant": 30}
           wall {"points": [[0, 0], [0, 6000]], "height": 3000}"#,
    );
    let upright = 100003;
    assert!(crate::walls::slant::lofted(&s.model, upright, None));
    let lean = 3000.0 * 30f64.to_radians().tan();
    // At its foot it starts at the corner; at its top, where the slanted wall has leant to (south, its exterior).
    let low = corners_at(&s, upright, 0.0).iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let high = corners_at(&s, upright, 3000.0).iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    assert!((low + 100.0 / 30f64.to_radians().cos()).abs() < 1.0, "{low}");
    assert!((high - (low - lean)).abs() < 1.0, "{high} {low}");
    // To IFC as face sets too.
    assert!(crate::ifc::export(&s.model).matches("IFCTRIANGULATEDFACESET").count() >= 2);
}

#[test]
fn a_shape_edited_wall_still_joins_a_slanted_one_at_its_untouched_end() {
    // Edit Profile on the upright wall: a notch at its far end's top and a square hole, its end at the corner untouched.
    let mut s = session(
        r#"wall {"points": [[6000, 0], [0, 0]], "height": 3000, "slant": 30}
           wall {"points": [[0, 0], [0, 8000]], "height": 3000}"#,
    );
    let shaped = 100003;
    let notch = serde_json::json!([
        [[0, 0], [8000, 0], [8000, 2000], [6000, 2000], [6000, 3000], [0, 3000]],
        [[3000, 1000], [4000, 1000], [4000, 2000], [3000, 2000]]
    ]);
    s.execute("modify", &serde_json::json!({"id": shaped, "profile": notch})).unwrap_or_else(|e| panic!("{e}"));
    let pieces = crate::walls::slant::pieces(&s.model, shaped, true, None);
    assert!(pieces.len() >= 4, "round its hole and its notch: {}", pieces.len());
    // Its end still follows the slanted wall's face up to its top.
    let lean = 3000.0 * 30f64.to_radians().tan();
    let low = corners_at(&s, shaped, 0.0).iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let high = corners_at(&s, shaped, 3000.0).iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    assert!((high - (low - lean)).abs() < 1.0, "{high} {low}");
    // Nothing over its notch, nothing in its hole.
    let points: Vec<[f64; 3]> = pieces.iter().flatten().flatten().copied().collect();
    assert!(points.iter().all(|p| !(p[1] > 6000.0 + 1e-6 && p[2] > 2000.0 + 1e-6)), "over the notch");
    let in_hole = |p: &[f64; 3]| p[1] > 3000.0 + 1e-6 && p[1] < 4000.0 - 1e-6 && p[2] > 1000.0 + 1e-6 && p[2] < 2000.0 - 1e-6;
    assert!(!points.iter().any(in_hole), "in the hole");
}

/// Whether point `p` lies inside the closed meshes `tris`: a ray from it crosses them an odd number of times.
fn inside(tris: &[[[f64; 3]; 3]], p: [f64; 3]) -> bool {
    let d = [0.5773, 0.5774, 0.5775];
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let hits = tris
        .iter()
        .filter(|[a, b, c]| {
            let (e1, e2) = (sub(*b, *a), sub(*c, *a));
            let h = cross(d, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-12 {
                return false;
            }
            let s = sub(p, *a);
            let u = dot(s, h) / det;
            let q = cross(s, e1);
            let v = dot(d, q) / det;
            let t = dot(e2, q) / det;
            (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && t > 1e-9
        })
        .count();
    hits % 2 == 1
}

/// The triangles of element `id`'s solids.
fn triangles_of(s: &Session, id: Id) -> Vec<[[f64; 3]; 3]> {
    let mut out = Vec::new();
    for solid in solids(&s.model).iter().filter(|x| x.owner() == id) {
        let mut tris = Vec::new();
        solid.tris(&mut tris);
        out.extend(tris.into_iter().map(|t| t.p));
    }
    out
}

#[test]
fn slanted_doors_and_windows_cut_a_slanted_wall_and_lean_with_it() {
    let mut s = session(
        r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": 20}
           door {"wall": 100002, "offset": 1500, "slanted": true}
           window {"wall": 100002, "offset": 4500, "slanted": true}"#,
    );
    let wall = 100002;
    let (door, window) = (100003, 100004);
    let tan = 20f64.to_radians().tan();
    // On the wall's centre where it has leant to at height `z`.
    let on = |x: f64, z: f64| [x, z * tan, z];
    let w = triangles_of(&s, wall);
    assert!(!inside(&w, on(1500.0, 1000.0)), "the door's hole");
    assert!(!inside(&w, on(4500.0, 1600.0)), "the window's hole");
    assert!(inside(&w, on(3000.0, 1000.0)) && inside(&w, on(4500.0, 400.0)) && inside(&w, on(4500.0, 2700.0)), "the wall round them");
    // Each stays in the wall's thickness as it rises.
    let half = 100.0 / 20f64.to_radians().cos() + 30.0;
    for id in [door, window] {
        let pts: Vec<[f64; 3]> = triangles_of(&s, id).into_iter().flatten().collect();
        assert!(!pts.is_empty(), "{id} is drawn");
        for p in &pts {
            assert!((p[1] - p[2] * tan).abs() <= half, "{id}: {p:?} is out of the wall, {} off its centre", p[1] - p[2] * tan);
        }
    }
    // IFC: both, each with the opening that voids the wall.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCDOOR(") && ifc.contains("IFCWINDOW("));
    assert_eq!(ifc.matches("IFCOPENINGELEMENT(").count(), 2);
    // A door moved along the wall keeps cutting where it is.
    s.execute("modify", &serde_json::json!({"id": door, "offset": 2500})).unwrap_or_else(|e| panic!("{e}"));
    let w = triangles_of(&s, wall);
    assert!(!inside(&w, on(2500.0, 1000.0)) && inside(&w, on(1500.0, 1000.0)));
}

#[test]
fn a_curved_slanted_walls_faces_are_one_smooth_surface_in_3d_across_its_pieces() {
    // Lofted in strips round its arc, and round its window: each face is still one surface, which the 3D view shades smoothly
    // and draws no edge across, as an upright curved wall's.
    for script in [
        r#"wall {"points": [[-5000, 0], [5000, 0]], "arc": 120, "height": 3000, "slant": 15}"#,
        r#"wall {"points": [[-5000, 0], [5000, 0]], "arc": 120, "height": 3000, "slant": -20}
           window {"wall": 100002, "offset": 3000}"#,
    ] {
        let s = session(script);
        let id = walls(&s)[0];
        let w = s.model.wall(id).unwrap_or_else(|| panic!("no wall"));
        let centre = crate::walls::Axis::of(w).centre().unwrap_or_else(|| panic!("an arc"));
        let mut tris = Vec::new();
        for solid in solids(&s.model).iter().filter(|x| x.owner() == id) {
            solid.tris(&mut tris);
        }
        let mut keys = std::collections::BTreeSet::new();
        let mut faces = 0;
        for t in &tris {
            let [a, b, c] = t.p;
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len < 1e-6 {
                continue;
            }
            let mid = P2((a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0);
            let radial = (mid - centre).unit();
            // On a face: its normal mostly across the wall, as a cone's is.
            if ((n[0] * radial.0 + n[1] * radial.1) / len).abs() > 0.5 {
                faces += 1;
                keys.insert(t.curve);
            }
        }
        assert!(faces > 20, "{faces} face triangles");
        assert_eq!(keys.len(), 1, "one surface, not one per piece: {} in {script}", keys.len());
        assert!(keys.iter().all(Option::is_some), "a curved one");
    }
}

#[test]
fn doors_and_windows_stand_upright_in_a_slanted_wall_unless_slanted_and_cut_it_the_same_either_way() {
    let mut s = session(
        r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": 20}
           door {"wall": 100002, "offset": 1500}
           window {"wall": 100002, "offset": 4500}"#,
    );
    let wall = 100002;
    let (door, window) = (100003, 100004);
    let tan = 20f64.to_radians().tan();
    let on = |x: f64, z: f64| [x, z * tan, z];
    let holed = |s: &Session| {
        let w = triangles_of(s, wall);
        !inside(&w, on(1500.0, 1000.0))
            && !inside(&w, on(4500.0, 1600.0))
            && inside(&w, on(3000.0, 1000.0))
            && inside(&w, on(4500.0, 400.0))
            && inside(&w, on(4500.0, 2700.0))
    };
    // By default each stands upright, as Revit's Vertical: its parts reach as far across at its head as at its foot,
    // centred where the wall has leant to halfway up it.
    for id in [door, window] {
        let pts: Vec<[f64; 3]> = triangles_of(&s, id).into_iter().flatten().collect();
        assert!(!pts.is_empty(), "{id} is drawn");
        let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[2]), b.max(p[2])));
        let across = |z: f64| {
            let ys: Vec<f64> = pts.iter().filter(|p| (p[2] - z).abs() < 1e-6).map(|p| p[1]).collect();
            (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        };
        let (foot, head) = (across(lo), across(hi));
        assert!((foot.0 - head.0).abs() < 1e-6 && (foot.1 - head.1).abs() < 1e-6, "{id} upright: {foot:?} at foot, {head:?} at head");
        let mid = (foot.0 + foot.1) / 2.0;
        assert!((mid - (lo + hi) / 2.0 * tan).abs() < 1.0, "{id} centred at {mid}, the wall at {}", (lo + hi) / 2.0 * tan);
    }
    assert!(holed(&s), "the wall cut round them");
    let ifc = crate::ifc::export(&s.model);
    assert_eq!(ifc.matches("IFCOPENINGELEMENT(").count(), 2);
    // Slanted, the same holes, and the door leans with the wall.
    s.execute("modify", &serde_json::json!({"ids": [door, window], "slanted": true})).unwrap_or_else(|e| panic!("{e}"));
    assert!(holed(&s), "the same holes");
    let pts: Vec<[f64; 3]> = triangles_of(&s, door).into_iter().flatten().collect();
    assert!(pts.iter().all(|p| (p[1] - p[2] * tan).abs() <= 60.0), "the door leans with the wall");
    // In an upright wall the box changes nothing.
    let mut u = session(
        r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000}
           door {"wall": 100002, "offset": 1500, "slanted": true}"#,
    );
    let leaning = triangles_of(&u, door);
    u.execute("modify", &serde_json::json!({"id": door, "slanted": false})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(triangles_of(&u, door).len(), leaning.len());
}

#[test]
fn a_window_family_cuts_a_slanted_wall_with_its_own_void_leaning_with_it() {
    // An arched window whose void, 400 deep, cuts its host: in a wall leaning 30°, it leans with the wall, so it cuts through
    // the wall all the way up, though the wall has moved 1.4 m across at its head.
    let mut s = session(r#"wall {"points": [[0, 0], [6000, 0]], "height": 3000, "slant": 30}"#);
    let mut profile = vec![serde_json::json!([-500, 0]), serde_json::json!([500, 0])];
    for k in 0..=16 {
        let a = std::f64::consts::PI * f64::from(k) / 16.0;
        profile.push(serde_json::json!([(500.0 * a.cos()).round(), (1000.0 + 500.0 * a.sin()).round()]));
    }
    s.execute(
        "family",
        &serde_json::json!({"name": "Arched", "category": "window", "types": {"A": {}},
        "parameters": [{"name": "Width", "value": 1000}, {"name": "Height", "value": 1500}, {"name": "Sill Height", "value": 900},
            {"name": "Void Depth", "instance": true, "value": 400}],
        "forms": [
            {"void": true, "plane": "xz", "profile": profile, "from": "-Void Depth / 2", "to": "Void Depth / 2"},
            {"profile": [[-500, -40], [500, -40], [500, 40], [-500, 40]], "from": 0, "to": 40, "material": ""}
        ]}),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    s.execute("window", &serde_json::json!({"wall": 100002, "offset": 3000, "family": "Arched"})).unwrap_or_else(|e| panic!("{e}"));
    let tan = 30f64.to_radians().tan();
    let on = |x: f64, z: f64| [x, z * tan, z];
    let w = triangles_of(&s, 100002);
    // Through its rectangle and its arch, high and low: open; beside it and over it: wall.
    for (x, z) in [(3000.0, 1100.0), (3000.0, 2300.0), (2700.0, 1400.0)] {
        assert!(!inside(&w, on(x, z)), "open at {x}, {z}");
    }
    for (x, z) in [(1500.0, 1400.0), (3000.0, 600.0), (3000.0, 2700.0), (2600.0, 2350.0)] {
        assert!(inside(&w, on(x, z)), "wall at {x}, {z}");
    }
}
