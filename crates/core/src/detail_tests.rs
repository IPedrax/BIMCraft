//! Detailing around the detail lines: repeating details and groups, Linework, the files that hold them, and the hostile
//! inputs each cap is for, driven through commands as a front end drives them.

use serde_json::{Value, json};

use crate::curves::Curve;
use crate::engine::Session;
use crate::geom::P2;
use crate::groups::{GroupType, Member, RepeatingDetail};
use crate::ids::Guid;
use crate::lines::DetailLine;
use crate::model::{Element, Id, Model};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A box of walls on Level 1, and a group type `G` of `members` short detail lines, put straight into the model (making
/// 10,000 through commands would take a check each).
fn with_group(members: usize) -> Session {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let line = |i: usize| Member {
        id: 0,
        guid: Guid::default(),
        element: Element::DetailLine(DetailLine {
            view: Guid::default(),
            curve: Curve::line(P2(i as f64, 0.0), P2(i as f64, 5.0)),
            style: "Thin Lines".into(),
            insulation: None,
            centre_mark: false,
            cloud: None,
            revision: None,
        }),
    };
    s.model_mut().detail_groups.insert("G".into(), GroupType { members: (0..members).map(line).collect() });
    s.model_mut().sync_library();
    s
}

/// A repeating detail of `G` in Level 1's plan: `count` of them, `spacing` apart.
fn repeat(count: usize, spacing: f64) -> Value {
    json!({"type": "G", "start": [0, 0], "end": [(count - 1) as f64 * spacing, 0], "spacing": spacing, "view": 100000})
}

#[test]
fn a_repeating_detail_may_place_as_many_members_as_a_view_draws_and_no_more() {
    let max = crate::groups::MAX_PLACED;
    // 100 members to the repeat: `max / 100` repeats are the most.
    let mut s = with_group(100);
    let fit = max / 100;
    assert_eq!(run(&mut s, "repeating_detail", repeat(fit, 10.0))["count"], fit);
    let mut s = with_group(100);
    let e = err(&mut s, "repeating_detail", repeat(fit + 1, 10.0));
    assert!(e.contains("members") && e.contains(&max.to_string()), "{e}");
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::RepeatingDetail(_))).count(), 0, "rolled back");
    // The old hostile case: 2000 repeats of a hundred members.
    let e = err(&mut s, "repeating_detail", repeat(2000, 10.0));
    assert!(e.contains("members"), "{e}");
    // A group type of the most members, repeated: 10,000 x 2 is over.
    let mut s = with_group(crate::groups::MAX_MEMBERS);
    run(&mut s, "repeating_detail", repeat(1, 10.0));
    assert!(err(&mut s, "repeating_detail", repeat(2, 10.0)).contains("members"));
}

#[test]
fn what_a_view_places_is_counted_over_all_its_groups_and_repeats_and_not_over_other_views() {
    let max = crate::groups::MAX_PLACED;
    let mut s = with_group(100);
    run(&mut s, "repeating_detail", repeat(max / 100, 10.0));
    // The view is full: one more group of 100 members in it is over, and in the other level's plan it isn't.
    let e = err(&mut s, "detail_group", json!({"type": "G", "at": [0, 0], "view": 100000}));
    assert!(e.contains("members"), "{e}");
    run(&mut s, "detail_group", json!({"type": "G", "at": [0, 0], "view": 100001}));
    // A repeat made fewer, then more again by `modify`: the rule holds for edits too.
    let mut s = with_group(100);
    let id = id_of(&run(&mut s, "repeating_detail", repeat(10, 100.0)));
    let e = err(&mut s, "modify", json!({"id": id, "spacing": 1.0, "end": [9000, 0]}));
    assert!(e.contains("members") || e.contains("repeat"), "{e}");
    // A file that holds more is refused whole, as the model's check says.
    let mut s = with_group(100);
    let view = s.model.guid(100000);
    let at = RepeatingDetail { view, type_name: "G".into(), start: P2(0.0, 0.0), end: P2(10_000.0, 0.0), spacing: 10.0, mirrored: false };
    s.model_mut().insert(Element::RepeatingDetail(at));
    assert!(s.model.check().is_err_and(|e| e.contains("members")));
}

/// Level 1's plan with a box of walls, and the box's first wall's GUID.
fn walled() -> (Session, Id, Guid) {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}))["ids"][0]
        .as_u64()
        .unwrap_or_default();
    let g = s.model.guid(w);
    (s, w, g)
}

#[test]
fn a_views_linework_is_counted_in_range_and_in_a_line_style_there_is() {
    let (mut s, _, wall) = walled();
    let edge = |i: usize| json!({"element": wall, "from": [i as f64, 0.0], "to": [i as f64 + 1.0, 0.0], "style": "Wide Lines"});
    run(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": [edge(0)]}}));
    let bad = [
        json!({"element": wall, "from": [1e300, 0.0], "to": [0.0, 0.0], "style": "Wide Lines"}),
        json!({"element": wall, "from": [0.0, 0.0], "to": [0.0, -2e9], "style": "Wide Lines"}),
        json!({"element": wall, "from": [0.0, 0.0], "to": [1.0, 0.0], "style": "No Such Style"}),
    ];
    for b in bad {
        let e = err(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": [b]}}));
        assert!(e.contains("linework") || e.contains("Linework"), "{e}");
    }
    // The most, and one over it.
    let most = crate::graphics::MAX_LINEWORK;
    run(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": (0..most).map(edge).collect::<Vec<_>>()}}));
    let e = err(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": (0..=most).map(edge).collect::<Vec<_>>()}}));
    assert!(e.contains(&most.to_string()), "{e}");
}

#[test]
fn linework_remove_takes_off_the_nearest_edge_and_not_every_edge_within_reach() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Partition", "thickness": 100}));
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Partition"}))["ids"][0].as_u64().unwrap_or_default();
    let held = |s: &Session| crate::graphics::graphics_of(&s.model, 100000).map_or(0, |g| g.linework.len());
    // Each face of the 100 mm wall, restyled.
    run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "style": "Wide Lines"}));
    run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -45], "invisible": true}));
    assert_eq!(held(&s), 2);
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "remove": true}));
    assert_eq!((r["removed"].clone(), held(&s)), (json!(1), 1), "{r}");
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -45], "remove": true}));
    assert_eq!((r["removed"].clone(), held(&s)), (json!(1), 0), "{r}");
    // Nothing near: nothing removed.
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "remove": true}));
    assert_eq!(r["removed"], 0);
}

#[test]
fn linework_remove_takes_nothing_off_when_the_edge_nearest_the_pick_has_none() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Partition", "thickness": 100}));
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Partition"}))["ids"][0].as_u64().unwrap_or_default();
    let held = |s: &Session| crate::graphics::graphics_of(&s.model, 100000).map_or(0, |g| g.linework.len());
    // Only the north face restyled; picked at 45 mm from the south face, which has none, 95 mm from the north face's.
    run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "style": "Wide Lines"}));
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -45], "remove": true}));
    assert_eq!((r["removed"].clone(), held(&s)), (json!(0), 1), "the south face had none: {r}");
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "remove": true}));
    assert_eq!((r["removed"].clone(), held(&s)), (json!(1), 0), "{r}");
    // Linework on an edge the wall no longer draws is picked off by the nearest of it.
    run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "style": "Wide Lines"}));
    run(&mut s, "modify", json!({"id": w, "end": [4000, 3000]}));
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [2000, 45], "remove": true}));
    assert_eq!((r["removed"].clone(), held(&s)), (json!(1), 0), "{r}");
}

/// A wall `length` long and `thick` thick on Level 1, its north face styled and its south face made invisible, then a T wall
/// across the middle: the north face is two edges now and the entry that held the whole face matches neither.
fn cut_face(thick: u32, length: u32, south: bool) -> (Session, Id) {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Partition", "thickness": thick}));
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [length, 0]], "type": "Partition"}))["ids"][0].as_u64().unwrap_or_default();
    let half = f64::from(thick) / 2.0;
    run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, half], "style": "Wide Lines"}));
    if south {
        run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -half], "invisible": true}));
    }
    run(&mut s, "wall", json!({"points": [[length / 2, 0], [length / 2, 4000]], "type": "Partition"}));
    (s, w)
}

fn styles(s: &Session) -> Vec<Option<String>> {
    crate::graphics::graphics_of(&s.model, 100000).map_or(Vec::new(), |g| g.linework.iter().map(|l| l.style.clone()).collect())
}

#[test]
fn linework_on_a_face_a_t_wall_has_cut_in_two_is_picked_off_from_either_piece() {
    for at in [[1000, 50], [7000, 50], [4000, 50], [1000, -50]] {
        let (mut s, w) = cut_face(100, 8000, false);
        assert_eq!(styles(&s).len(), 1);
        let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": at, "remove": true}));
        assert_eq!((r["removed"].clone(), styles(&s).len()), (json!(1), 0), "from {at:?}: {r}");
    }
}

#[test]
fn linework_remove_takes_the_stale_entry_at_the_stale_spot_and_the_live_one_at_the_live_edge() {
    // The north entry is stale; the south face, whole, holds a live one. Each pick takes its own, one at a time.
    let (mut s, w) = cut_face(100, 8000, true);
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -50], "remove": true}));
    assert_eq!((r["removed"].clone(), styles(&s)), (json!(1), vec![Some("Wide Lines".to_string())]), "the live south one: {r}");
    let (mut s, w) = cut_face(100, 8000, true);
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 50], "remove": true}));
    assert_eq!((r["removed"].clone(), styles(&s)), (json!(1), vec![None]), "the stale north one, not the live south one: {r}");
    // Picked again over the live face, whose edge holds the one left.
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -50], "remove": true}));
    assert_eq!((r["removed"].clone(), styles(&s).len()), (json!(1), 0), "{r}");
}

#[test]
fn linework_remove_leaves_a_stale_entry_out_of_reach_of_the_pick() {
    // A 600 mm wall: its faces are 600 apart, the reach is 200. The south face has none, the stale north entry is out of reach.
    let (mut s, w) = cut_face(600, 8000, false);
    for at in [[1000, -300], [1000, 5000]] {
        let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": at, "remove": true}));
        assert_eq!((r["removed"].clone(), styles(&s).len()), (json!(0), 1), "from {at:?}: {r}");
    }
}

#[test]
fn linework_is_found_again_from_any_other_pick_on_its_edge_whatever_the_ends_of_the_wall() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Partition", "thickness": 100}));
    let mut seed = 17u64;
    let mut next = |lo: f64, hi: f64| {
        seed = crate::model::mix(seed);
        lo + (seed % 1_000_000) as f64 / 1_000_000.0 * (hi - lo)
    };
    let micron = |v: f64| (v * 1000.0).round() / 1000.0;
    let held = |s: &Session| crate::graphics::graphics_of(&s.model, 100000).map_or(0, |g| g.linework.len());
    // 80 walls of a few metres at any angle, each in a cell of its own, and 3 of 0.3, 3 and 30 km, with ends to the micron far
    // from the origin: the ends a pick finds are rounded to a pixel's f32 round that pick, so they differ a little from
    // another pick's, and the more the further they lie.
    let mut walls: Vec<(P2, P2)> = Vec::new();
    for i in 0..83 {
        let (len, angle) = (if i < 80 { next(2000.0, 8000.0) } else { 3.0e5 * 10f64.powi(i - 80) }, next(0.0, 360.0f64).to_radians());
        let (col, row) = (f64::from(i % 10), f64::from(i / 10));
        let a = P2(micron(5000.0 + col * 20000.0 + next(0.0, 900.0)), micron(300000.0 + row * 20000.0 + next(0.0, 900.0)));
        walls.push((a, P2(micron(a.0 + len * angle.cos()), micron(a.1 + len * angle.sin()))));
    }
    for (i, (a, b)) in walls.iter().enumerate() {
        let w =
            run(&mut s, "wall", json!({"points": [[a.0, a.1], [b.0, b.1]], "type": "Partition"}))["ids"][0].as_u64().unwrap_or_default();
        // A point of the wall's north-west face, 50 mm off its centre line, `t` of the way along.
        let n = (*b - *a).unit().perp();
        let at = |t: f64| {
            let p = *a + (*b - *a) * t + n * 50.0;
            json!([micron(p.0), micron(p.1)])
        };
        let before = held(&s);
        run(&mut s, "linework", json!({"view": 100000, "id": w, "at": at(0.3), "style": "Wide Lines"}));
        assert_eq!(held(&s), before + 1, "wall {i}");
        // Styled again from a third pick: the edge takes the newer style, it is not listed twice.
        run(&mut s, "linework", json!({"view": 100000, "id": w, "at": at(0.8), "invisible": true}));
        assert_eq!(held(&s), before + 1, "wall {i} {a:?} {b:?}: styled twice");
        let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": at(0.6), "remove": true}));
        assert_eq!((r["removed"].clone(), held(&s)), (json!(1), before), "wall {i} {a:?} {b:?}: {r}");
    }
}

#[test]
fn an_unknown_field_in_a_group_members_element_is_left_out_as_it_is_in_any_elements() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0]})));
    let b = id_of(&run(&mut s, "detail_line", json!({"start": [0, 500], "end": [1000, 500]})));
    let c = id_of(&run(&mut s, "detail_line", json!({"start": [0, 900], "end": [1000, 900]})));
    run(&mut s, "create_group", json!({"ids": [a, b], "name": "A/B~C"}));
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap_or_default()).unwrap_or_default();
    let key = file["detail_groups"].as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
    file["detail_groups"][&key]["members"][1]["element"]["made_later"] = json!(1);
    file["elements"][c.to_string()]["made_later"] = json!(2);
    let (m, left_out) = Model::read(&file.to_string()).unwrap_or_else(|e| panic!("{e}"));
    let at = format!("/detail_groups/{}/members/1/element/made_later", key.replace('~', "~0").replace('/', "~1"));
    assert!(left_out.contains(&at), "{left_out:?}");
    assert!(left_out.contains(&format!("/elements/{c}/made_later")), "{left_out:?}");
    assert_eq!(m.detail_groups.values().map(|g| g.members.len()).collect::<Vec<_>>(), [2]);
    // A newer field in a member of a member's sketch (a region's line) too.
    let r =
        id_of(&run(&mut s, "filled_region", json!({"sketch": [[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 0]]]})));
    run(&mut s, "create_group", json!({"ids": [r], "name": "R"}));
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap_or_default()).unwrap_or_default();
    let key = file["detail_groups"].as_object().and_then(|o| o.keys().find(|k| *k != &key).cloned()).unwrap_or_default();
    file["detail_groups"][&key]["members"][0]["element"]["sketch"][0]["made_later"] = json!(1);
    let (_, left_out) = Model::read(&file.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.iter().any(|p| p.ends_with("/sketch/0/made_later")), "{left_out:?}");
}

#[test]
fn a_line_style_a_group_member_or_a_region_uses_is_kept_and_the_error_says_which() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    run(&mut s, "line_style", json!({"name": "Thick", "pen": 4}));
    run(&mut s, "line_style", json!({"name": "Edge", "pen": 2}));
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0], "style": "Thick"})));
    run(&mut s, "create_group", json!({"ids": [a], "name": "Section Mark"}));
    let e = err(&mut s, "line_style", json!({"name": "Thick", "delete": true}));
    assert!(e.contains("detail group `Section Mark` has a member drawn in `Thick`"), "{e}");
    // A region of the project, bounded in a style.
    let sq = json!([[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 1000]], [[0, 1000], [0, 0]]]);
    let r = id_of(&run(&mut s, "filled_region", json!({"sketch": sq, "line_style": "Edge"})));
    let e = err(&mut s, "line_style", json!({"name": "Edge", "delete": true}));
    assert!(e.contains(&format!("filled region {r} is bounded in `Edge`")), "{e}");
    // In a group, the region's style is named with the group too.
    run(&mut s, "create_group", json!({"ids": [r], "name": "Boxes"}));
    let e = err(&mut s, "line_style", json!({"name": "Edge", "delete": true}));
    assert!(e.contains("detail group `Boxes` has a member drawn in `Edge`"), "{e}");
    // A style nothing uses is deleted as ever.
    run(&mut s, "line_style", json!({"name": "Overhead Lines", "delete": true}));
}

#[test]
fn a_mirrored_group_turns_its_notes_with_it_and_keeps_them_readable() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let note = id_of(&run(&mut s, "text", json!({"text": "Fix", "at": [0, 0], "angle": 30})));
    run(&mut s, "create_group", json!({"ids": [note], "name": "Note"}));
    let angle = |s: &Session, turn: f64, mirrored: bool| {
        let g =
            crate::groups::DetailGroup { view: s.model.guid(100000), type_name: "Note".into(), at: P2(0.0, 0.0), angle: turn, mirrored };
        match crate::groups::placed(&s.model, &g).first() {
            Some(Element::Text(n)) => n.angle,
            e => panic!("{e:?}"),
        }
    };
    assert_eq!(angle(&s, 0.0, false), 30.0);
    assert_eq!(angle(&s, 40.0, false), 70.0, "turned with the group");
    assert_eq!(angle(&s, 0.0, true), 30.0, "a pure mirror leaves a note reading the way it did");
    assert_eq!(angle(&s, 40.0, true), 70.0, "mirrored and turned, it turns too");
}

#[test]
fn a_revision_cloud_stays_within_the_points_a_curve_draws_and_never_cuts_a_corner() {
    let mut s = Session::new();
    // A circle half a million metres across, scalloped by arcs 1.5 mm long.
    let id = id_of(&run(&mut s, "detail_line", json!({"centre": [0, 0], "radius": 5e8, "cloud": 1.5})));
    let Some(Element::DetailLine(l)) = s.model.elements.get(id) else { panic!("no line") };
    let n = l.drawn().len();
    assert!(n <= crate::curves::MAX_DRAWN, "{n} points");
    // A rectangle whose sides are no whole number of scallops: none of its scallops dips inside it.
    let id =
        id_of(&run(&mut s, "detail_line", json!({"points": [[0, 0], [1000, 0], [1000, 700], [0, 700]], "closed": true, "cloud": 300})));
    let Some(Element::DetailLine(l)) = s.model.elements.get(id) else { panic!("no line") };
    let inside = l.drawn().iter().filter(|p| p.0 > 1.0 && p.0 < 999.0 && p.1 > 1.0 && p.1 < 699.0).count();
    assert_eq!(inside, 0, "{inside} points inside the loop");
    // A cloud and an insulation are two drawings of one line: one is asked for.
    let e = err(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0], "cloud": 100, "insulation": 100}));
    assert!(e.contains("cloud") && e.contains("insulation"), "{e}");
}

/// What `MAX_PLACED` (10,000) hatched regions cost a plan to draw: `cargo test -p bimcraft-core --lib probe_ -- --ignored
/// --nocapture` (and with `--release`), whose numbers `MAX_PLACED` and the docs quote.
#[test]
#[ignore = "a probe: cargo test -p bimcraft-core --lib probe_ -- --ignored --nocapture"]
fn probe_the_cost_of_the_most_hatched_regions_a_view_may_place() {
    for (side, of) in [(300.0, "Diagonal Up"), (3000.0, "Diagonal Up"), (30000.0, "Diagonal Up"), (3000.0, "Crosshatch"), (3000.0, "Brick")]
    {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        // Five regions to a group, repeated 2,000 times (the most a repeat takes): 10,000 placed.
        let regions: Vec<Id> = (0..5)
            .map(|k| {
                let at = f64::from(k) * 50.0;
                let sketch = json!([
                    [[at, 0], [at + side, 0]],
                    [[at + side, 0], [at + side, side]],
                    [[at + side, side], [at, side]],
                    [[at, side], [at, 0]]
                ]);
                id_of(&run(&mut s, "filled_region", json!({"sketch": sketch, "type": of, "line_style": null})))
            })
            .collect();
        run(&mut s, "create_group", json!({"ids": regions, "name": "Hatched"}));
        let r = run(&mut s, "repeating_detail", json!({"type": "Hatched", "start": [0, 0], "end": [199_800, 0], "spacing": 100}));
        let at = std::time::Instant::now();
        let d = crate::render::plan(&s.model, 100000, 1600, 1000);
        eprintln!("{} placed, {of} regions {side} mm across: {:?}, {} prims", r["count"], at.elapsed(), d.prims.len());
    }
}

/// What placed members that are huge circles cost a plan to draw and to rasterize, each: a curve is drawn through up to
/// `MAX_DRAWN` points wherever it is, so `MAX_PLACED` of them take a long while to render, a figure `MAX_PLACED` and the docs
/// quote (`cargo test -p bimcraft-core --lib probe_ -- --ignored --nocapture`).
#[test]
#[ignore = "a probe: cargo test -p bimcraft-core --lib probe_ -- --ignored --nocapture"]
fn probe_the_cost_of_placed_members_that_are_huge_circles() {
    for count in [100, 400, 800] {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        let circle = id_of(&run(&mut s, "detail_line", json!({"centre": [0, 0], "radius": 5e8})));
        run(&mut s, "create_group", json!({"ids": [circle], "name": "Huge"}));
        let r = run(&mut s, "repeating_detail", json!({"type": "Huge", "start": [0, 0], "end": [count - 1, 0], "spacing": 1}));
        let at = std::time::Instant::now();
        let d = crate::render::plan(&s.model, 100000, 1600, 1000);
        let drawn = at.elapsed();
        let _ = crate::render::png(&d);
        let took = at.elapsed();
        eprintln!(
            "{} placed huge circles: drawn in {drawn:?}, and rasterized {took:?}, {:?} each, {} prims",
            r["count"],
            took / count,
            d.prims.len()
        );
    }
}

#[test]
fn a_cloud_round_a_huge_circle_is_drawn_without_listing_its_scallops_first() {
    // A span asks for the scallops that fit it, and never more than the most a curve may be cut into (plus the one that
    // says it is too many), whatever its length: the count sizes an allocation.
    let most = crate::curves::MAX_DRAWN / 8;
    assert_eq!(crate::lines::scallops(1000.0, 300.0, most), 3);
    assert_eq!(crate::lines::scallops(1.0, 300.0, most), 1);
    assert_eq!(crate::lines::scallops(1e9, 1.5, most), most + 1);
    assert_eq!(crate::lines::scallops(f64::MAX, 1.5, most), most + 1);
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    // Arcs 1.5 mm long round these circles are 84 million, 419 million and 2 billion scallops: listing their cuts took 670 MB,
    // 3.4 GB and 16.8 GB, and the plan that drew them aborted.
    for radius in [2e7, 1e8, 5e8] {
        let id = id_of(&run(&mut s, "detail_line", json!({"centre": [0, 0], "radius": radius, "cloud": 1.5})));
        let Some(Element::DetailLine(l)) = s.model.elements.get(id) else { panic!("no line") };
        let n = l.drawn().len();
        assert!(n > 8 && n <= crate::curves::MAX_DRAWN, "{radius}: {n} points");
    }
    // An open line from -5e8 to 5e8 is one span, as long as the circles' way round.
    run(&mut s, "detail_line", json!({"start": [-5e8, 0], "end": [5e8, 0], "cloud": 1.5}));
    let png = run(&mut s, "render", json!({"view": 100000, "width": 64, "height": 64}));
    assert!(png["pngBase64"].as_str().is_some_and(|p| !p.is_empty()), "{png}");
}

#[test]
fn a_regions_or_floors_pattern_origin_and_angle_show_in_elements_and_align_moves_the_origin() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "closed": true}));
    run(&mut s, "filled_region_type", json!({"name": "Tiles", "foreground": "Tile 300"}));
    let sq = json!([[[0, 0], [3000, 0]], [[3000, 0], [3000, 3000]], [[3000, 3000], [0, 3000]], [[0, 3000], [0, 0]]]);
    let region = id_of(&run(&mut s, "filled_region", json!({"sketch": sq, "type": "Tiles", "line_style": null})));
    let floor = id_of(&run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]})));
    for id in [region, floor] {
        let e = crate::element_json(&s.model, id);
        assert_eq!((e["pattern_origin"].clone(), e["pattern_angle"].clone()), (json!([0.0, 0.0]), json!(0.0)), "{e}");
    }
    // The tile line nearest x = 630 lies on x = 600; a reference at x = 650 asks for the origin 50 mm east.
    let near = |s: &Session| crate::render::pattern_line_at(&s.model, 100000, region, P2(640.0, 1650.0), 100.0);
    let line = near(&s).unwrap_or_else(|| panic!("no pattern line there"));
    assert!((line.0.0 - 600.0).abs() < 0.5 && (line.1.0 - 600.0).abs() < 0.5, "{line:?}");
    let reference = (P2(650.0, 0.0), P2(650.0, 3000.0));
    let to = s.model.align_pattern(region, reference, line).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(to, P2(50.0, 0.0));
    run(&mut s, "modify", json!({"id": region, "pattern_origin": [to.0, to.1]}));
    let after = near(&s).unwrap_or_else(|| panic!("no pattern line there"));
    assert!((after.0.0 - 650.0).abs() < 0.5, "{after:?}");
    // Aligned again from there it stays; a floor takes it the same way; and what can't be aligned says why.
    assert_eq!(s.model.align_pattern(region, reference, after), Ok(to));
    assert_eq!(s.model.align_pattern(floor, reference, line), Ok(P2(50.0, 0.0)));
    let e = s.model.align_pattern(region, (P2(0.0, 0.0), P2(1000.0, 0.0)), line).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.model.align_pattern(region, (P2(5.0, 5.0), P2(5.0, 5.0)), line).unwrap_err();
    assert!(e.contains("no length"), "{e}");
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(i, _)| *i).unwrap_or_default();
    assert!(s.model.align_pattern(wall, reference, line).unwrap_err().contains("pattern"));
    assert!(s.model.align_pattern(u64::MAX, reference, line).is_err());
    // A turned pattern aligns across its own lines' way: a reference at 45 degrees, the lines turned to match.
    run(&mut s, "modify", json!({"id": region, "pattern_origin": [0, 0], "pattern_angle": 45}));
    let diag = crate::render::pattern_line_at(&s.model, 100000, region, P2(1500.0, 1560.0), 100.0).unwrap_or_else(|| panic!("no line"));
    let r45 = (P2(0.0, 100.0), P2(1000.0, 1100.0));
    let moved = s.model.align_pattern(region, r45, diag).unwrap_or_else(|e| panic!("{e}"));
    run(&mut s, "modify", json!({"id": region, "pattern_origin": [moved.0, moved.1]}));
    let again = crate::render::pattern_line_at(&s.model, 100000, region, P2(1500.0, 1640.0), 100.0).unwrap_or_else(|| panic!("no line"));
    let off = (again.0 - r45.0).dot((r45.1 - r45.0).unit().perp()).abs();
    assert!(off < 0.5, "{off} mm off the reference");
}

/// Payloads for the hostile sweep, each with the command it is written for.
pub(crate) type Payloads = Vec<(&'static str, Value)>;

/// The square of side `w` with its corner at `(x, y)`, as the lines of a sketch.
fn square(x: f64, y: f64, w: f64) -> Value {
    json!([[[x, y], [x + w, y]], [[x + w, y], [x + w, y + w]], [[x + w, y + w], [x, y + w]], [[x, y + w], [x, y]]])
}

/// What the hostile sweep (`tests::hostile_params_never_panic_or_break_the_model`) needs of detailing. `s` is the sweep's
/// base project, a box of walls (ids 100002 to 100005) and two masses: this gives it one of each thing the detailing
/// commands act on (detail lines of several shapes, a revision cloud, filled regions, a text note, a detail group, a
/// repeating detail of it, a floor drawn from a sketch, a plan with Linework), so that they reach a target, and returns
/// the payloads that name them, each with the command it is written for and that command's real keys, and values wrong in
/// the ways its checks look for: `.0` are run against every command, `.1` against the one they are for.
pub(crate) fn sweep_payloads(s: &mut Session) -> (Payloads, Payloads) {
    let line = |s: &mut Session, p: Value| id_of(&run(s, "detail_line", p));
    let a = line(s, json!({"start": [0, 0], "end": [2000, 0]}));
    let b = line(s, json!({"start": [0, 500], "end": [2000, 500]}));
    let poly = line(s, json!({"points": [[0, 1000], [1000, 1000], [1000, 2000]]}));
    let spline = line(s, json!({"fit": [[0, 3000], [500, 3500], [1500, 2500], [2000, 3000]]}));
    let circle = line(s, json!({"centre": [3000, 3000], "radius": 500}));
    let crossing = line(s, json!({"start": [500, -500], "end": [500, 400]}));
    let tail = line(s, json!({"start": [2000, 0], "end": [3000, 500]}));
    line(s, json!({"points": [[3000, 0], [4000, 0], [4000, 700], [3000, 700]], "closed": true, "cloud": 300, "revision": "Rev 1"}));
    let region = id_of(&run(s, "filled_region", json!({"sketch": square(5000.0, 0.0, 800.0), "type": "Brick"})));
    let note = id_of(&run(s, "text", json!({"text": "Fix", "at": [6000, 100]})));
    let member = line(s, json!({"start": [6000, 500], "end": [6500, 500]}));
    let boxed = id_of(&run(s, "filled_region", json!({"sketch": square(6000.0, 1000.0, 300.0)})));
    let group = id_of(&run(s, "create_group", json!({"ids": [note, member, boxed], "name": "Grp"})));
    let repeat = id_of(&run(s, "repeating_detail", json!({"type": "Grp", "start": [0, 4000], "end": [4000, 4000], "spacing": 1000})));
    let floor = id_of(&run(s, "floor", json!({"sketch": square(0.0, 0.0, 8000.0)})));
    run(s, "linework", json!({"view": 100000, "id": 100002, "at": [4000, 95], "style": "Wide Lines"}));
    // Two lines of their own (the others are joined, filleted and trimmed) and a locked dimension between them: what is typed
    // into it, and Align's lock against it.
    let (low, high) =
        (line(s, json!({"start": [0, -2000], "end": [2000, -2000]})), line(s, json!({"start": [0, -1500], "end": [2000, -1500]})));
    let dim = id_of(&run(s, "dimension", json!({"refs": [{"id": low}, {"id": high}], "at": [1000, -1750]})));
    run(s, "dimension_constraint", json!({"id": dim, "locked": [0]}));
    let bow = json!([[[0, 0], [4000, 4000]], [[4000, 4000], [4000, 0]], [[4000, 0], [0, 2000]], [[0, 2000], [0, 0]]]);
    let open = json!([[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]]]);
    let far = json!([[[0, 0], [1e300, 0]], [[1e300, 0], [0, 1e300]], [[0, 1e300], [0, 0]]]);
    let overlap = json!([[[0, 0], [1000, 0]], [[1000, 0], [0, 0]]]);
    let cross = vec![
        // The sketch of a floor, a ceiling, a roof, a shaft, a region; sketch itself; edit_boundary.
        ("sketch", json!({"lines": bow})),
        ("sketch", json!({"lines": open})),
        ("sketch", json!({"lines": far})),
        ("sketch", json!({"lines": overlap})),
        ("sketch", json!({"lines": "x"})),
        ("floor", json!({"sketch": bow})),
        ("ceiling", json!({"sketch": open, "level": 100000})),
        ("shaft", json!({"sketch": far})),
        ("filled_region", json!({"sketch": overlap, "type": "Masking"})),
        ("roof", json!({"sketch": "x"})),
        ("edit_boundary", json!({"id": floor, "sketch": bow})),
        ("edit_boundary", json!({"id": floor, "sketch": open})),
        ("edit_boundary", json!({"id": floor})),
        ("edit_boundary", json!({"id": region, "sketch": far})),
        ("edit_boundary", json!({"id": region, "sketch": square(0.0, 0.0, 1000.0)})),
        // Detail lines of every shape, with the wrong numbers.
        ("detail_line", json!({"start": [0, 0], "end": [1000, 0], "arc": 359.9999999999})),
        ("detail_line", json!({"start": [0, 0], "end": [1000, 0], "through": [500, 1e300]})),
        ("detail_line", json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "arcs": [0, 1e300], "closed": true})),
        ("detail_line", json!({"points": [[0, 0], [1000, 0]], "arcs": [0, 90, 180]})),
        ("detail_line", json!({"centre": [0, 0], "radius": 1e9, "cloud": 1.5})),
        ("detail_line", json!({"centre": [1e9, 1e9], "radius": 1e9, "style": "No Such Style"})),
        ("detail_line", json!({"centre": [0, 0], "radii": [1e300, 1], "angle": 1e300, "span": [0, 1e300]})),
        ("detail_line", json!({"control": [[0, 0], [1000, 1000], [2000, 0]], "knots": [0, 0, 0.5, 1, 1]})),
        ("detail_line", json!({"control": [[0, 0], [1, 1]], "knots": [0, 0, 1]})),
        ("detail_line", json!({"fit": [[0, 0], [0, 0], [0, 0]], "start_tangent": [0, 0], "end_tangent": [1e300, 0]})),
        ("detail_line", json!({"start": [0, 0], "end": [1000, 0], "cloud": 100, "insulation": 100, "revision": 5, "centre_mark": "yes"})),
        // Typed lengths, Align's lock, and a 3D dimension to an edge of no size.
        ("dimension_value", json!({"id": dim, "value": 1e300})),
        ("dimension_value", json!({"id": dim, "value": 800, "segment": 1e300})),
        ("dimension_value", json!({"id": dim, "value": 800})),
        ("align", json!({"id": high, "reference": [[0, -2000], [2000, -2000]], "line": [[0, -1500], [2000, -1500]], "lock": true})),
        (
            "align",
            json!({"id": low, "reference": [[0, -1500], [2000, -1500]], "line": [[0, -2000], [2000, -2000]], "lock": "x", "to": u64::MAX}),
        ),
        ("dimension_3d", json!({"refs": [{"at": [0, 0, 0]}, {"at": [1000, 0, 0], "edge": [[0, 0, 0], [0, 0, 0]]}], "at": [0, -500, 0]})),
        // Edits of them.
        ("split", json!({"id": a, "at": [1e300, 0]})),
        ("vertex", json!({"id": poly, "at": [1000, 1000], "delete": true})),
        ("vertex", json!({"id": spline, "at": [1e300, 0], "delete": true})),
        ("fillet", json!({"ids": [a, b], "radius": 100})),
        ("fillet", json!({"ids": [a, poly], "radius": 1e300})),
        ("offset", json!({"id": a, "distance": 1e300, "toward": [0, 1], "copy": false})),
        ("trim_extend", json!({"boundary": a, "ids": [poly, b], "keep": [[100, 0], [5, 5]]})),
        ("create_group", json!({"ids": [a, region, group]})),
        // Model patterns, Linework, groups.
        ("modify", json!({"id": region, "pattern_origin": [1e300, 0], "pattern_angle": 1e300})),
        ("modify", json!({"id": floor, "pattern_origin": "x", "pattern_angle": [1]})),
        (
            "modify",
            json!({"id": 100000, "graphics": {"linework": [{"element": "x", "from": [1e300, 0], "to": [0, 0], "style": "No Such Style"}]}}),
        ),
        ("detail_group", json!({"type": "Grp", "at": [1e300, 0], "angle": 1e300, "mirrored": 5})),
        // Text: its types, its runs, its formatting.
        ("modify", json!({"id": note, "runs": [{"from": 0, "to": 1e300, "bold": true}], "override_format": true})),
        ("modify", json!({"id": note, "runs": [{"from": 2, "to": 1}], "type": "Nope"})),
        ("modify", json!({"id": note, "text": "Fixed ő Ж 漢", "size": 3})),
        ("text", json!({"text": "x".repeat(5000), "at": [0, 0], "type": 5})),
        ("text", json!({"text": "\u{0}\t\n", "at": [0, 0], "runs": [{"from": 0, "to": 1, "italic": "yes"}]})),
    ];
    let targeted = vec![
        // Each command once as it is meant to be used, so that the base has what it needs to do something.
        ("sketch", json!({"lines": square(0.0, 0.0, 1000.0)})),
        ("edit_boundary", json!({"id": floor, "sketch": square(0.0, 0.0, 6000.0)})),
        ("detail_line", json!({"start": [100, 100], "end": [900, 100]})),
        ("trim_extend", json!({"boundary": a, "ids": [crossing], "keep": [[500, 400]]})),
        ("fillet", json!({"ids": [a, crossing], "radius": 100})),
        ("offset", json!({"id": a, "distance": 100, "toward": [0, 1]})),
        ("split", json!({"id": a, "at": [1000, 0]})),
        ("join_polyline", json!({"ids": [a, tail]})),
        (
            "line_pattern",
            json!({"name": "Graphic", "segments": [
                {"offset": {"length": 4, "count": 9, "distance": 0.5}},
                {"wave": {"length": 6, "frequency": 2, "amplitude": 1}},
                {"zigzag": {"length": 6, "frequency": 3, "amplitude": 1}},
                {"text": {"length": 12, "size": 2, "text": "Gas"}}, {"dot_length": 1}
            ]}),
        ),
        ("line_pattern", json!({"name": "Fence", "segments": [{"dash": 4}, {"space": 1}]})),
        ("line_style", json!({"name": "Fence Line", "pen": 3})),
        ("fill_pattern", json!({"name": "Hatch", "lines": [{"angle": 45, "spacing": 10}]})),
        ("filled_region_type", json!({"name": "Hatched", "foreground": "Sand"})),
        ("text_type", json!({"name": "Title", "like": "5mm", "font": "IBM Plex Mono", "bold": true, "border": true, "border_pen": 3})),
        (
            "text",
            json!({"text": "Bold\nlines", "at": [100, 100], "type": "3.5mm", "override_format": true, "runs": [{"from": 0, "to": 4, "bold": true}]}),
        ),
        ("text_type", json!({"name": "X", "font": "Comic", "size": 1e300, "width_factor": -1, "border_offset": 1e9, "border_pen": 99})),
        ("text_type", json!({"name": "2.5mm", "delete": true, "rename": "x", "color": "#zz"})),
        ("text_type", json!({"name": "3.5mm", "rename": "2.5mm"})),
        ("filled_region", json!({"sketch": square(0.0, 0.0, 500.0), "type": "Sand"})),
        ("export_fill_patterns", json!({"names": ["Sand"]})),
        ("import_fill_patterns", json!({"text": "*Hatch\n45,0,0,0,10\n"})),
        ("create_group", json!({"ids": [a, b]})),
        ("detail_group", json!({"type": "Grp", "at": [100, 100], "angle": 30, "mirrored": true})),
        ("repeating_detail", json!({"type": "Grp", "start": [0, 0], "end": [3000, 0], "spacing": 1000})),
        ("duplicate_view", json!({"view": 100000, "mode": "detailing", "name": "Detailed"})),
        // And with what is wrong in each.
        ("duplicate_view", json!({"view": 100000, "mode": "dependent", "name": ""})),
        ("trim_extend", json!({"boundary": 100002, "ids": [a], "keep": [[1e300, 0]]})),
        ("trim_extend", json!({"boundary": a, "ids": [a], "keep": [[0, 0]]})),
        ("trim_extend", json!({"boundary": 5, "ids": [circle, spline], "keep": "x"})),
        ("vertex", json!({"id": poly, "at": [500, 1000]})),
        ("vertex", json!({"id": a, "at": [0, 0], "delete": true})),
        ("convert_spline", json!({"id": spline})),
        ("convert_spline", json!({"id": circle})),
        ("fillet", json!({"ids": [100002, 100003], "radius": 1e300})),
        ("fillet", json!({"ids": [a, a], "radius": 5})),
        ("fillet", json!({"ids": [circle, spline], "radius": 5})),
        ("offset", json!({"id": circle, "distance": -500, "toward": [3000, 3000]})),
        ("offset", json!({"id": spline, "distance": 1e9, "toward": [0, 0], "copy": "x"})),
        ("split", json!({"id": poly, "at": [1e300, 1e300]})),
        ("split", json!({"id": circle, "at": [3500, 3000]})),
        ("join_polyline", json!({"ids": [a, b]})),
        ("join_polyline", json!({"ids": [poly]})),
        ("join_polyline", json!({"ids": [a, 100002]})),
        ("explode", json!({"id": poly})),
        ("explode", json!({"id": a})),
        ("line_pattern", json!({"name": "X", "segments": [{"dash": 1e300}, {"space": -1}, "dot", 5]})),
        ("line_pattern", json!({"name": "Dash", "delete": true, "rename": "Dot"})),
        ("line_style", json!({"name": "X", "pen": 99, "color": "#zz", "pattern": 5})),
        ("line_style", json!({"name": "Thin Lines", "delete": true})),
        ("line_style", json!({"name": "Wide Lines", "delete": true})),
        ("fill_pattern", json!({"name": "P", "lines": [{"angle": 1e300, "spacing": 0}], "target": "x"})),
        ("fill_pattern", json!({"name": "P", "lines": [{"angle": 0, "spacing": 10, "dashes": [-1]}]})),
        ("fill_pattern", json!({"name": "P", "lines": [{"angle": 0, "spacing": 0.05, "dashes": [-0.05]}]})),
        ("fill_pattern", json!({"name": "Brick", "delete": true})),
        ("filled_region_type", json!({"name": "T", "foreground": "nope", "background": 5, "masking": "x", "foreground_color": [999]})),
        ("filled_region_type", json!({"name": "Brick", "delete": true})),
        ("filled_region", json!({"sketch": bow, "type": "Masking", "view": 100002})),
        ("filled_region", json!({"sketch": square(0.0, 0.0, 1000.0), "line_style": 5})),
        ("linework", json!({"view": 100000, "id": 100002, "at": [1e300, 0], "style": "nope", "invisible": true, "remove": true})),
        ("linework", json!({"view": 100000, "id": 100002, "at": [4000, 95], "remove": true})),
        ("linework", json!({"view": 100000, "id": a, "at": [1000, 0], "invisible": true})),
        ("import_fill_patterns", json!({"text": "*a\n0,0,0,0,10,-1\n*b\n1e999,0,0,0,1\n*c\n0,0,0,0,0\n", "replace": true})),
        ("import_fill_patterns", json!({"text": "*Sand\n0,0,0,0,10\n", "replace": false})),
        ("import_fill_patterns", json!({"text": 5, "path": "/nonexistent/x.pat"})),
        ("import_fill_patterns", json!({"path": "/nonexistent/x.pat"})),
        ("export_fill_patterns", json!({"path": "/nonexistent/dir/out.pat", "names": ["nope"]})),
        ("export_fill_patterns", json!({"names": "x"})),
        ("create_group", json!({"ids": [100002]})),
        ("create_group", json!({"ids": [group]})),
        ("create_group", json!({"ids": [a, b], "name": "Grp"})),
        ("detail_group", json!({"type": "Grp", "at": [0, 0], "view": 100002})),
        ("detail_group", json!({"type": "Nope", "at": [0, 0], "angle": 90})),
        ("repeating_detail", json!({"type": "Grp", "start": [0, 0], "end": [100000, 0], "spacing": 1})),
        ("repeating_detail", json!({"type": "Grp", "start": [0, 0], "end": [0, 0], "spacing": 100})),
        ("ungroup", json!({"id": group})),
        ("ungroup", json!({"id": repeat})),
        ("group_type", json!({"name": "Grp", "rename": "Grp 2"})),
        ("group_type", json!({"name": "Grp", "delete": true})),
        ("modify", json!({"id": repeat, "spacing": 1.0, "end": [4000, 4000]})),
    ];
    (cross, targeted)
}

#[test]
fn the_sweeps_detailing_payloads_use_real_keys_and_the_base_holds_something_for_each_to_reach() {
    let mut base = Session::new();
    run(&mut base, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let (cross, targeted) = sweep_payloads(&mut base);
    // One of each kind of thing the detailing commands act on.
    for category in ["line", "region", "detailgroup", "repeatingdetail", "floor"] {
        let n = run(&mut base, "elements", json!({"category": category}))["count"].clone();
        assert!(n.as_u64().is_some_and(|n| n > 0), "no {category} in the base: {n}");
    }
    assert_eq!(crate::graphics::graphics_of(&base.model, 100000).map(|g| g.linework.len()), Some(1));
    assert!(base.model.elements.values().any(|e| matches!(e, Element::Floor(f) if !f.sketch.is_empty())), "a floor with a sketch");
    // Each payload names keys its command has: an error may be about the values, never about a key it doesn't take.
    let mut accepted = std::collections::BTreeSet::new();
    for (c, p) in cross.iter().chain(&targeted) {
        match base.clone().execute(c, p) {
            Ok(_) => {
                accepted.insert(*c);
            }
            Err(e) => assert!(!e.contains("isn't one of its parameters") && !e.contains("unknown field"), "{c} {p}: {e}"),
        }
    }
    // Every command the detailing stack added has a payload that it takes: the base has the target it needs.
    let added = [
        "convert_spline",
        "create_group",
        "detail_group",
        "detail_line",
        "edit_boundary",
        "explode",
        "export_fill_patterns",
        "fill_pattern",
        "filled_region",
        "filled_region_type",
        "fillet",
        "group_type",
        "import_fill_patterns",
        "join_polyline",
        "line_pattern",
        "line_style",
        "linework",
        "offset",
        "repeating_detail",
        "sketch",
        "split",
        "trim_extend",
        "ungroup",
        "vertex",
    ];
    let missing: Vec<&str> = added.into_iter().filter(|c| !accepted.contains(c)).collect();
    assert!(missing.is_empty(), "no payload is accepted by {missing:?}");
}
