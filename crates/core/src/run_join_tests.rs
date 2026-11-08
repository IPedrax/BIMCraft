//! Runs joining: the mitres and cuts their outlines take where they meet, the flags that turn them off, what
//! follows what when one end is dragged, and the one body and one set of snaps the plan, the 3D view, IFC and
//! the picks all take from that outline.

use crate::engine::Session;
use crate::geom::{self, P2};
use crate::model::Run;
use crate::snap::{Kind, Reach, Snaps, snap};
use serde_json::json;

fn session(script: &str) -> Session {
    let mut s = Session::new();
    s.script(script).unwrap();
    s
}

/// The ids of the runs of `category`, in id order (the order the script made them in).
fn runs(s: &Session, category: &str) -> Vec<u64> {
    let mut ids: Vec<u64> = s.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids
}

/// Run `id` as it stands in the model.
fn run(s: &Session, id: u64) -> Run {
    s.model.run_of(id).cloned().unwrap_or_else(|| panic!("run {id} is there"))
}

/// Run `id`'s outline in plan, as every view fills and cuts it.
fn outline(s: &Session, id: u64) -> Vec<P2> {
    crate::runs::outline(&s.model, id)
}

fn near(a: P2, b: P2) -> bool {
    a.dist(b) < 1e-6
}

/// Two 600 mm strips leaving the origin, east and north: a corner, as `walls_meet_at_any_angle_in_a_mitre`
/// makes of two walls.
fn corner() -> (Session, Vec<u64>) {
    let s = session(r#"strip_footing {"points": [[4000, 0], [0, 0], [0, 4000]]}"#);
    let ids = runs(&s, "stripfooting");
    assert_eq!(ids.len(), 2, "the chain makes two strips: {ids:?}");
    (s, ids)
}

#[test]
fn two_flats_meeting_at_a_corner_mitre_into_each_other() {
    let (s, ids) = corner();
    // The faces cross on the bisector, 300 / sin(half the angle) from the corner, both ways.
    let half = 45f64.to_radians();
    let inner = P2(half.cos(), half.sin()) * (300.0 / half.sin());
    for id in ids {
        let o = outline(&s, id);
        assert!(o.iter().any(|p| near(*p, inner)) && o.iter().any(|p| near(*p, inner * -1.0)), "{id}: {o:?}");
        // Mitred, each is as big as its length times its width: nothing overlaps, nothing's missing.
        assert!((geom::area(&o).abs() - 4000.0 * 600.0).abs() < 1e-3, "{id}: {}", geom::area(&o));
    }
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn a_wall_over_one_of_two_joined_footings_does_not_break_their_mitre() {
    let plain = session(r#"strip_footing {"points": [[4000, 0], [0, 0], [0, 4000]]}"#);
    let overlaid = session(
        r#"strip_footing {"points": [[4000, 0], [0, 0], [0, 4000]]}
           wall {"points": [[4000, 0], [0, 0]]}"#,
    );
    let plain_ids = runs(&plain, "stripfooting");
    let overlaid_ids = runs(&overlaid, "stripfooting");
    assert_eq!(plain_ids.len(), 2);
    assert_eq!(overlaid_ids.len(), 2);
    for (plain_id, overlaid_id) in plain_ids.into_iter().zip(overlaid_ids) {
        let expected = outline(&plain, plain_id);
        let actual = outline(&overlaid, overlaid_id);
        assert_eq!(actual.len(), expected.len(), "wall above one footing must not add a spurious arm: {actual:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!(near(*a, e), "the supported footing's neighbouring mitre changed: {actual:?}");
        }
    }
}

#[test]
fn a_flat_ending_on_another_stops_at_its_face() {
    let s = session(
        r#"strip_footing {"points": [[0, 0], [6000, 0]]}
           strip_footing {"points": [[3000, 1000], [3000, 0]]}"#,
    );
    let [through, ending] = runs(&s, "stripfooting")[..] else { panic!() };
    let o = outline(&s, ending);
    // The end stops at the through strip's face: nothing of it over the body it ends on, and its end
    // face right there.
    assert!(o.iter().all(|p| p.1 >= 300.0 - 1e-6), "{o:?}");
    assert_eq!(o.iter().filter(|p| (p.1 - 300.0).abs() < 1e-9).count(), 2, "{o:?}");
    // The through strip is not changed by it: its full rectangle still.
    let t = outline(&s, through);
    assert!(t.iter().all(|p| p.1.abs() <= 300.0 + 1e-9), "{t:?}");
    assert_eq!(t.len(), 4, "{t:?}");
}

#[test]
fn two_flats_end_to_end_meet_with_no_gap_and_no_overlap() {
    let s = session(
        r#"strip_footing {"points": [[0, 0], [4000, 0]]}
           strip_footing {"points": [[4000, 0], [8000, 0]]}"#,
    );
    let [west, east] = runs(&s, "stripfooting")[..] else { panic!() };
    let (a, b) = (outline(&s, west), outline(&s, east));
    assert!((a.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max) - 4000.0).abs() < 1e-9, "{a:?}");
    assert!((b.iter().map(|p| p.0).fold(f64::INFINITY, f64::min) - 4000.0).abs() < 1e-9, "{b:?}");
    // Butt where they meet, 600 wide each way: no step out of plane.
    for o in [&a, &b] {
        assert!(o.iter().any(|p| near(*p, P2(4000.0, 300.0))) && o.iter().any(|p| near(*p, P2(4000.0, -300.0))), "{o:?}");
    }
}

#[test]
fn a_strip_footing_ending_on_a_wall_stops_at_its_face_and_the_wall_is_untouched() {
    let s = session(
        r#"wall {"points": [[0, -4000], [0, 4000]], "type": "Exterior - 300mm"}
           strip_footing {"points": [[-4000, 0], [0, 0]]}"#,
    );
    let wall = *runs(&s, "wall").first().unwrap();
    let footing = *runs(&s, "stripfooting").first().unwrap();
    let o = outline(&s, footing);
    // It stops at the wall's near face: the wall's body is not overlapped, and the node's side of its
    // face is not left square in the air either.
    assert!(o.iter().all(|p| p.0 <= -150.0 + 1e-6), "{o:?}");
    assert_eq!(o.iter().filter(|p| (p.0 + 150.0).abs() < 1e-9).count(), 2, "{o:?}");
    // The wall's own outline is exactly what it is without the footing: runs are no arms of a wall's.
    let w = crate::walls::outline(&s.model, wall);
    assert_eq!(w.len(), 4, "{w:?}");
    assert!(w.iter().all(|p| p.0.abs() - 150.0 < 1e-9), "{w:?}");
}

#[test]
fn flats_mitre_by_the_faces_they_actually_have() {
    let s = session(
        r#"beam {"points": [[4000, 0], [0, 0]], "type": "Beam 200 x 500"}
           beam {"points": [[0, 0], [0, 4000]], "type": "Beam 150 x 400"}"#,
    );
    let ids = runs(&s, "beam");
    assert_eq!(ids.len(), 2);
    // The crossing of the 150 wide beam's faces (x = ±75) with the 200 wide one's (y = ±100): each cap
    // is taken on the other's faces, not on some width they invent.
    for id in ids {
        let o = outline(&s, id);
        assert!(o.iter().any(|p| p.0.abs() - 75.0 < 1e-6 && p.1.abs() - 100.0 < 1e-6), "{id}: {o:?}");
    }
}

#[test]
fn a_pair_untouched_in_height_keeps_its_square_ends() {
    let s = session(
        r#"strip_footing {"points": [[0, 0], [6000, 0]]}
           strip_footing {"points": [[3000, 1000], [3000, 0]], "offset": -1000}"#,
    );
    let [through, ending] = runs(&s, "stripfooting")[..] else { panic!() };
    // A metre under the through strip, its heights reach nowhere near it: no cut — its own end face
    // stands right on the through strip's axis.
    let o = outline(&s, ending);
    assert!(o.iter().any(|p| p.1.abs() < 1e-9), "{o:?}");
    assert_eq!(runs(&s, "stripfooting").len(), 2);
    let _ = through;
}

#[test]
fn refusing_to_join_leaves_both_ends_of_the_pair_square_and_they_stretch_for_no_one() {
    let (mut s, ids) = corner();
    let [a, b] = ids[..] else { panic!() };
    // Off: its own end is its plain rectangle again, and so is its neighbour's — the flag acts on the pair.
    s.execute("modify", &json!({"id": a, "join": false})).unwrap();
    for id in [a, b] {
        let o = outline(&s, id);
        assert!(!o.iter().any(|p| near(*p, P2(-300.0, -300.0))), "{id}: {o:?}");
        assert_eq!(o.len(), 4, "{id}: {o:?}");
    }
    // And neither stretches behind the other: the corner end dragged, its mate left standing.
    s.execute("move", &json!({"ids": [a], "by": [0.0, 1000.0], "end": "end"})).unwrap();
    assert_eq!(run(&s, a).start, P2(4000.0, 0.0), "its own far end stays");
    assert_eq!(run(&s, a).end, P2(0.0, 1000.0), "its gripped end goes where it was told");
    assert_eq!(run(&s, b).start, P2(0.0, 0.0), "its mate stretches for no one");
    // On again, the mitre comes back where the ends meet: put the gripped end back on the corner first.
    s.execute("move", &json!({"ids": [a], "by": [0.0, -1000.0], "end": "end"})).unwrap();
    s.execute("modify", &json!({"id": a, "join": true})).unwrap();
    assert!(outline(&s, a).iter().any(|p| near(*p, P2(-300.0, -300.0))), "the mitre returned");
    assert!(outline(&s, b).iter().any(|p| near(*p, P2(-300.0, -300.0))), "on both sides of the pair");
    // Joined once more, the mate follows the dragged end.
    s.execute("move", &json!({"ids": [a], "by": [0.0, 500.0], "end": "end"})).unwrap();
    assert_eq!(run(&s, b).start, P2(0.0, 500.0), "joined again, the mate follows");
}

#[test]
fn dragging_one_end_of_a_run_takes_the_runs_joined_there_with_it() {
    let mut s = session(
        r#"strip_footing {"points": [[4000, 0], [0, 0]]}
           strip_footing {"points": [[0, 0], [0, 4000]]}
           strip_footing {"points": [[3000, -1000], [3000, 0]]}"#,
    );
    let [corner, mate, on_span] = runs(&s, "stripfooting")[..] else { panic!() };
    // The corner end pulled north: the run whose end was on it goes with it (as a wall's mate does),
    // and the one ending on its span slides along its own line onto the new one.
    s.execute("move", &json!({"ids": [corner], "by": [0.0, 1000.0], "end": "end"})).unwrap();
    assert_eq!((run(&s, corner).start, run(&s, corner).end), (P2(4000.0, 0.0), P2(0.0, 1000.0)));
    assert_eq!(run(&s, mate).start, P2(0.0, 1000.0), "the end on the end went with it");
    assert_eq!(run(&s, on_span).end, P2(3000.0, 250.0), "the end on the span slid to the crossing");
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn moving_a_run_in_any_category_keeps_a_middle_join_on_the_new_intersection() {
    for category in crate::model::RUN_CATEGORIES {
        let command = crate::commands::command_of(category);
        let mut s = session(&format!(
            r#"{command} {{"points": [[0, 0], [6000, 0]]}}
               {command} {{"points": [[3000, 4000], [3000, 0]]}}"#
        ));
        let [taken, joined] = runs(&s, category)[..] else { panic!("{category}: two runs were made") };

        s.execute("move", &json!({"ids": [taken], "by": [0.0, 1000.0]})).unwrap();

        assert_eq!(run(&s, taken).start, P2(0.0, 1000.0), "{category}: the taken run moved");
        assert_eq!(run(&s, taken).end, P2(6000.0, 1000.0), "{category}: the taken run moved");
        assert_eq!(run(&s, joined).start, P2(3000.0, 4000.0), "{category}: the free end stays put");
        assert_eq!(run(&s, joined).end, P2(3000.0, 1000.0), "{category}: its joint follows the new intersection");
    }
}

#[test]
fn a_joined_runs_3d_body_is_cut_where_its_outline_is() {
    let s = session(
        r#"strip_footing {"points": [[0, 0], [6000, 0]]}
           strip_footing {"points": [[3000, 1000], [3000, 0]]}"#,
    );
    let [through, ending] = runs(&s, "stripfooting")[..] else { panic!() };
    let solids = crate::view3d::solids(&s.model);
    let of = |id: u64| solids.iter().filter(|x| x.owner() == id).collect::<Vec<_>>();
    // The cut run's body stands on the very outline the plan fills: no point of it under the face the
    // plan cut it at — plan and 3D can't show different joints.
    let outline = outline(&s, ending);
    let outline_min = outline.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let ring = match of(ending).as_slice() {
        [crate::view3d::Solid::Prism { ring, .. }] => (*ring).clone(),
        other => panic!("the cut run is one prism, not {} solids", other.len()),
    };
    assert!((ring.iter().map(|p| p.1).fold(f64::INFINITY, f64::min) - outline_min).abs() < 1e-9, "{ring:?}");
    assert!(
        ring.len() == outline.len() && ring.iter().all(|p| outline.contains(p)),
        "the body stands on the outline: {ring:?} vs {outline:?}"
    );
    // The through run keeps its plain rectangle: its body is not cut by anything.
    match of(through).as_slice() {
        [crate::view3d::Solid::Prism { ring, .. }] => {
            assert!(ring.iter().any(|p| near(*p, P2(0.0, -300.0))), "{ring:?}");
        }
        other => panic!("the through run is one prism, not {} solids", other.len()),
    }
}

#[test]
fn a_round_runs_body_is_cut_by_the_same_planes_as_its_outline() {
    let s = session(
        r#"pipe {"points": [[0, 0], [6000, 0]]}
           pipe {"points": [[3000, 1000], [3000, 0]]}"#,
    );
    let [through, ending] = runs(&s, "pipe")[..] else { panic!() };
    let solids = crate::view3d::solids(&s.model);
    let of = |id: u64| solids.iter().filter(|x| x.owner() == id).collect::<Vec<_>>();
    let whole = 4 * crate::runs::SIDES;
    let cut = match of(ending).as_slice() {
        [crate::view3d::Solid::Mesh { tris, round, .. }] => {
            assert!(*round, "a pipe's body is a tube");
            assert_ne!(tris.len(), whole, "cut away from the shared tube's {whole} triangles");
            (*tris).clone()
        }
        other => panic!("the cut pipe is one mesh, not {} solids", other.len()),
    };
    // The tube ends in plan where the outline does: the through pipe's face.
    let plan_min = cut.iter().flatten().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let outline_min = outline(&s, ending).iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    assert!((plan_min - outline_min).abs() < 1e-3, "tube {plan_min}, outline {outline_min}");
    // The pipe nothing ends on stays the shared tube to the last triangle: a view still draws one shape
    // for every round run of a material.
    match of(through).as_slice() {
        [crate::view3d::Solid::Mesh { tris, round, .. }] => {
            assert!(*round && tris.len() == whole, "{} triangles", tris.len());
        }
        other => panic!("the through pipe is one mesh, not {} solids", other.len()),
    }
}

#[test]
fn a_runs_ifc_body_is_its_section_swept_along_its_axis_whatever_its_joints() {
    let mut s = session(
        r#"wall {"points": [[0, -4000], [0, 4000]], "type": "Exterior - 300mm"}
           strip_footing {"points": [[-4000, 0], [0, 0]]}"#,
    );
    let file = ifc(&mut s);
    assert_eq!(entities(&file, "IFCFOOTING").len(), 1);
    let solids: Vec<&str> = entities(&file, "IFCEXTRUDEDAREASOLID");
    // The footing keeps the body a reader comes back to: its section, 600 by 300, swept its 4000 along
    // its axis. (The wall is swept up its 8000.)
    assert!(solids.iter().any(|e| e.ends_with(",4000.);")), "{solids:?}");
    let swept_up = solids.iter().any(|e| e.ends_with(",300.);"));
    assert!(!swept_up, "no body swept up its section's depth: {solids:?}");
}

/// The ids of the walls, in id order.
fn walls(s: &Session) -> Vec<u64> {
    let mut ids: Vec<u64> = s.model.elements.iter().filter(|(_, e)| e.category() == "wall").map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids
}

#[test]
fn a_wall_refusing_to_join_leaves_its_corner_square_and_stretches_for_no_one() {
    let mut s = session(
        r#"wall {"points": [[4000, 0], [0, 0]]}
           wall {"points": [[0, 0], [0, 4000]]}"#,
    );
    let ids = walls(&s);
    let [a, b] = ids[..] else { panic!() };
    // Joining, the corner is mitred: each outline reaches a point outside its own rectangle.
    assert!(crate::walls::outline(&s.model, a).iter().any(|p| p.0 < -1e-9), "mitred");
    s.execute("modify", &json!({"id": a, "join": false})).unwrap();
    // Refused: each end stands square in its own rectangle, on both sides of the pair.
    for (id, inside) in [(a, 0), (b, 1)] {
        let o = crate::walls::outline(&s.model, id);
        assert!(o.iter().all(|p| [p.0, p.1][inside] >= -1e-9), "{id}: {o:?}");
    }
    // And neither stretches behind the other's dragged end.
    s.execute("move", &json!({"ids": [a], "by": [0.0, 1000.0], "end": "end"})).unwrap();
    let b_start = s.model.wall(b).map(|w| w.start).unwrap();
    assert_eq!(b_start, P2(0.0, 0.0), "its mate stretches for no one");
    // Taken up again, the corner mitres once more.
    s.execute("modify", &json!({"id": a, "join": true})).unwrap();
    s.execute("move", &json!({"ids": [a], "by": [0.0, -1000.0], "end": "end"})).unwrap();
    assert!(crate::walls::outline(&s.model, a).iter().any(|p| p.0 < -1e-9), "the mitre returned");
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn snaps_take_the_corners_of_a_runs_joined_outline() {
    let (s, ids) = corner();
    let _ = ids;
    // By the mitre corner, where the plain rectangle had nothing: the crossing of the faces, as an
    // endpoint of the outline every view draws.
    let reach = Reach { point: 100.0, line: 60.0 };
    let found = snap(&s.model, 100000, P2(350.0, 350.0), reach, &[], false, &Snaps::default());
    assert_eq!((found.at, found.kind), (P2(300.0, 300.0), Kind::Endpoint), "{found:?}");
    // The axis keeps its line and its own points: a run is still picked and turned by them.
    let end = snap(&s.model, 100000, P2(4040.0, 30.0), reach, &[], false, &Snaps::default());
    assert_eq!((end.at, end.kind), (P2(4000.0, 0.0), Kind::Endpoint), "{end:?}");
}

/// The IFC4 file of the project, and the entities of one kind in it: `#n=NAME(...);`.
fn ifc(s: &mut Session) -> String {
    let path = std::env::temp_dir().join(format!("bimcraft-run-join-{}-{}.ifc", std::process::id(), s.model.uid));
    s.execute("save", &json!({"path": path})).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    text
}

fn entities<'a>(file: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("={name}(");
    file.lines().filter(|l| l.contains(&head)).collect()
}
