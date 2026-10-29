//! Bars by rules: `reinforce` arms a beam, a column, a footing or a floor from the cover and the bars, stirrups and meshes it is
//! told, through the commands every front end drives.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn ids(r: &Value) -> Vec<u64> {
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// A beam 200 x 500 and 6 m long with its top at the level: its centre line is at z -250, its section spans z -500 to 0 and y
/// -100 to 100.
fn beam(s: &mut Session) -> u64 {
    ids(&run(s, "beam", json!({"points": [[0, 0], [6000, 0]]})))[0]
}

/// The bars of `host` on `layer`, by id.
fn on(s: &Session, host: u64, layer: &str) -> Vec<Value> {
    let guid = s.model.elements.guid(host).unwrap();
    s.model
        .elements
        .iter()
        .filter(|(_, e)| matches!(e, Element::Rebar(r) if r.host == Some(guid) && r.layer == layer))
        .map(|(id, _)| element_json(&s.model, *id))
        .collect()
}

fn near(a: &Value, b: f64) -> bool {
    a.as_f64().is_some_and(|x| (x - b).abs() < 1e-6)
}

#[test]
fn a_beam_is_armed_with_bars_below_bars_above_and_stirrups_spread_to_their_spacing() {
    let mut s = Session::new();
    let host = beam(&mut s);
    let r = run(
        &mut s,
        "reinforce",
        json!({
            "host": host, "cover": 25,
            "bottom": {"count": 3, "diameter": 16}, "top": {"count": 2, "diameter": 12.5},
            "stirrup": {"diameter": 5, "spacing": 150}, "grade_stirrup": "CA-60"
        }),
    );
    // 3 + 2 bars and stirrups from 27.5 to 5972.5 mm, no further apart than 150: 40 gaps of 148.625, 41 stirrups.
    assert_eq!(ids(&r).len(), 46);
    assert_eq!((r["count"].as_u64(), r["removed"].as_u64()), (Some(46), Some(0)));
    let (bottom, top, stirrups) = (on(&s, host, "bottom"), on(&s, host, "top"), on(&s, host, "stirrup"));
    assert_eq!((bottom.len(), top.len(), stirrups.len()), (3, 2, 41));

    // Bottom bars: 25 mm cover, 5 mm stirrup and half a bar in from the bottom face and the sides, 25 mm short of each end.
    let mut ys: Vec<f64> = bottom.iter().map(|b| b["path"][0][1].as_f64().unwrap()).collect();
    ys.sort_by(f64::total_cmp);
    assert_eq!(ys, [-62.0, 0.0, 62.0]);
    for b in &bottom {
        assert!(near(&b["path"][0][0], 25.0) && near(&b["path"][1][0], 5975.0) && near(&b["path"][0][2], -462.0), "{b}");
        assert_eq!((b["diameter"].as_f64(), b["grade"].as_str(), b["length"].as_f64()), (Some(16.0), Some("CA-50"), Some(5950.0)));
    }
    let mut ys: Vec<f64> = top.iter().map(|b| b["path"][0][1].as_f64().unwrap()).collect();
    ys.sort_by(f64::total_cmp);
    assert_eq!(ys, [-63.75, 63.75]);
    assert!(top.iter().all(|b| near(&b["path"][0][2], -36.25)), "{top:?}");

    // A stirrup is a closed loop of 1180 mm in the section, 25 mm of cover round it, of the grade told for stirrups.
    let first = stirrups.iter().min_by(|a, b| a["path"][0][0].as_f64().unwrap().total_cmp(&b["path"][0][0].as_f64().unwrap())).unwrap();
    assert!(near(&first["path"][0][0], 27.5), "{first}");
    assert_eq!(
        (first["length"].as_f64(), first["grade"].as_str(), first["path"].as_array().map(Vec::len)),
        (Some(1180.0), Some("CA-60"), Some(5))
    );
    let zs: Vec<f64> = first["path"].as_array().unwrap().iter().map(|p| p[2].as_f64().unwrap()).collect();
    assert!(zs.iter().all(|z| near(&json!(*z), -472.5) || near(&json!(*z), -27.5)), "{zs:?}");
    let last = stirrups.iter().map(|b| b["path"][0][0].as_f64().unwrap()).fold(0.0, f64::max);
    assert!((last - 5972.5).abs() < 1e-6);
}

#[test]
fn stirrups_get_closer_near_the_ends_and_skin_bars_fill_the_web() {
    let mut s = Session::new();
    let host = beam(&mut s);
    run(
        &mut s,
        "reinforce",
        json!({
            "host": host, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "top": {"count": 2, "diameter": 12.5},
            "stirrup": {"diameter": 5, "spacing": 150, "ends": {"length": 600, "spacing": 100}},
            "skin": {"diameter": 8, "spacing": 150}
        }),
    );
    // 600 mm each end at no more than 100 (7 each, the 600 mm point shared with the middle), 4745 mm between at no more than 150.
    assert_eq!(on(&s, host, "stirrup").len(), 7 + 31 + 7);
    // The web between the bars' centres, 425.75 mm, in three gaps: two rows a side.
    let skin = on(&s, host, "skin");
    assert_eq!(skin.len(), 4);
    assert!(skin.iter().all(|b| near(&b["path"][0][1], 66.0) || near(&b["path"][0][1], -66.0)), "{skin:?}");
}

#[test]
fn a_hook_adds_its_tails_to_a_stirrup_and_an_anchor_runs_a_bar_past_the_end() {
    let mut s = Session::new();
    let host = beam(&mut s);
    run(
        &mut s,
        "reinforce",
        json!({
            "host": host, "cover": 25, "bottom": {"count": 2, "diameter": 16},
            "stirrup": {"diameter": 5, "spacing": 200, "hook": 50}, "anchor": {"start": 0, "end": 300}
        }),
    );
    let bottom = on(&s, host, "bottom");
    // Flush with the start face, 300 mm past the end face.
    assert!(bottom.iter().all(|b| near(&b["path"][0][0], 0.0) && near(&b["path"][1][0], 6300.0)), "{bottom:?}");
    // A loop and a tail of 50 mm at each end: 7 points, 1180 + 100 mm of bar.
    let st = on(&s, host, "stirrup");
    assert!(st.iter().all(|b| b["path"].as_array().map(Vec::len) == Some(7) && near(&b["length"], 1280.0)), "{:?}", st.first());
}

#[test]
fn the_cover_comes_from_the_hosts_own_parameter_when_none_is_given() {
    let mut s = Session::new();
    let host = beam(&mut s);
    let e = fails(&mut s, "reinforce", json!({"host": host, "bottom": {"count": 2, "diameter": 16}}));
    assert!(e.contains("cover"), "{e}");
    run(&mut s, "parameter", json!({"name": "Cobrimento", "kind": "length", "categories": ["beam"]}));
    run(&mut s, "modify", json!({"id": host, "params": {"Cobrimento": 30}}));
    run(&mut s, "reinforce", json!({"host": host, "bottom": {"count": 2, "diameter": 16}}));
    let bars = on(&s, host, "bottom");
    // 30 cover and half a bar: 250 - 30 - 8 = 212 down from the centre line; 30 short of each end.
    assert!(bars.iter().all(|b| near(&b["path"][0][2], -462.0) && near(&b["path"][0][0], 30.0)), "{bars:?}");
    // A face of its own wins: 40 at the bottom face.
    run(&mut s, "reinforce", json!({"host": host, "cover": {"bottom": 40}, "bottom": {"count": 2, "diameter": 16}}));
    assert!(on(&s, host, "bottom").iter().all(|b| near(&b["path"][0][2], -452.0)));
}

#[test]
fn reinforcing_again_replaces_its_bars_and_leaves_the_ones_drawn_by_hand() {
    let mut s = Session::new();
    let host = beam(&mut s);
    let hand = ids(&run(&mut s, "rebar", json!({"points": [[50, 0, -400], [5950, 0, -400]], "diameter": 20, "host": host})))[0];
    let spec = json!({"host": host, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}});
    let first = run(&mut s, "reinforce", spec.clone());
    let again = run(&mut s, "reinforce", spec);
    assert_eq!((ids(&again).len(), again["removed"].as_u64()), (44, Some(44)));
    assert!(ids(&first).iter().all(|id| s.model.elements.get(*id).is_none()));
    assert!(s.model.elements.get(hand).is_some());
    let bars = s.model.elements.values().filter(|e| matches!(e, Element::Rebar(_))).count();
    assert_eq!(bars, 1 + 44);
    // Undo puts back the bars it replaced.
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Rebar(_))).count(), 1 + 44);
    // They go with their host.
    run(&mut s, "delete", json!({"ids": [host]}));
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Rebar(_))).count(), 0);
}

#[test]
fn a_beam_that_cannot_hold_what_it_is_told_is_refused_and_says_what_to_change() {
    let mut s = Session::new();
    let host = beam(&mut s);
    let base = json!({"host": host, "cover": 25});
    let with = |extra: Value| -> Value {
        let mut v = base.clone();
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        v
    };
    for (extra, says) in [
        // Six 25 mm bars in 200 mm touch: 0 mm between them, where a bar's own thickness is the least.
        (json!({"bottom": {"count": 6, "diameter": 25}}), "clearance"),
        (json!({"bottom": {"count": 3, "diameter": 16}, "clearance": 60}), "clearance"),
        // Cover and stirrup leave no room.
        (json!({"cover": 100, "bottom": {"count": 1, "diameter": 16}}), "narrow"),
        // Nothing to make.
        (json!({}), "nothing"),
        // Top and bottom bars 24 mm apart centre to centre meet.
        (
            json!({"cover": {"top": 230, "bottom": 230, "sides": 25}, "top": {"count": 2, "diameter": 16}, "bottom": {"count": 2, "diameter": 16}}),
            "meet",
        ),
        // Not a parameter of a beam, and not a parameter of anything.
        (json!({"mesh": {"x": {"diameter": 10, "spacing": 200}}}), "mesh"),
        (json!({"bottom": {"count": 2, "diameter": 16}, "colour": "red"}), "colour"),
        // Counts, diameters and spacings out of range.
        (json!({"bottom": {"count": 0, "diameter": 16}}), "count"),
        (json!({"bottom": {"count": 2, "diameter": 1}}), "diameter"),
        (json!({"bottom": {"count": 2, "diameter": 200}}), "diameter"),
        (json!({"stirrup": {"diameter": 5, "spacing": 0}}), "spacing"),
        (json!({"stirrup": {"diameter": 5, "spacing": 3}}), "spacing"),
        (json!({"stirrup": {"diameter": 5, "spacing": 1e-9}}), "spacing"),
        (json!({"stirrup": {"diameter": 5, "spacing": 150, "ends": {"length": -1, "spacing": 100}}}), "ends"),
        (json!({"stirrup": {"diameter": 5, "spacing": 150, "hook": -3}}), "hook"),
        (json!({"cover": -5, "bottom": {"count": 2, "diameter": 16}}), "cover"),
        (json!({"cover": 1e300, "bottom": {"count": 2, "diameter": 16}}), "cover"),
        (json!({"cover": "thick", "bottom": {"count": 2, "diameter": 16}}), "cover"),
        (json!({"bottom": {"count": 2, "diameter": 16}, "grade": ""}), "grade"),
        // So many stirrups the command would make more bars than it may.
        (json!({"stirrup": {"diameter": 5, "spacing": 5.5}}), "1000"),
    ] {
        let e = fails(&mut s, "reinforce", with(extra.clone()));
        assert!(e.contains(says), "{extra}: {e}");
    }
    for (p, says) in [
        (json!({"cover": 25, "bottom": {"count": 2, "diameter": 16}}), "host"),
        (json!({"host": 999999, "cover": 25, "bottom": {"count": 2, "diameter": 16}}), "no element"),
        (json!({"host": "x", "cover": 25, "bottom": {"count": 2, "diameter": 16}}), "host"),
    ] {
        let e = fails(&mut s, "reinforce", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

/// A structural column 300 x 300 and 3 m tall at 1 m east, 2 m north, on the ground level.
fn column(s: &mut Session, angle: f64) -> u64 {
    run(s, "structural_column", json!({"at": [1000, 2000], "type": "Concrete 300 x 300", "height": 3000, "angle": angle}))["id"]
        .as_u64()
        .unwrap()
}

fn xy(b: &Value, i: usize) -> (f64, f64, f64) {
    let p = &b["path"][i];
    (p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap())
}

#[test]
fn a_column_gets_bars_on_every_face_stirrups_up_its_height_and_ties_across() {
    let mut s = Session::new();
    let host = column(&mut s, 0.0);
    let r = run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 25, "bars": {"diameter": 16, "x": 3, "y": 3}, "stirrup": {"diameter": 5, "spacing": 150}, "ties": {"diameter": 6.3}}),
    );
    // Eight bars (a corner at each, a bar in the middle of each face), 21 stirrups from 27.5 to 2972.5 mm, a tie across the
    // section both ways at each of them.
    let (bars, stirrups, ties) = (on(&s, host, "bars"), on(&s, host, "stirrup"), on(&s, host, "tie"));
    assert_eq!((bars.len(), stirrups.len(), ties.len(), ids(&r).len()), (8, 21, 42, 71));
    // 25 cover, 5 stirrup, half a bar: bars sit 112 mm from the middle; they run from 25 to 2975 up the column.
    let mut at: Vec<(f64, f64)> = bars.iter().map(|b| (xy(b, 0).0, xy(b, 0).1)).collect();
    at.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let want = [
        (888.0, 1888.0),
        (888.0, 2000.0),
        (888.0, 2112.0),
        (1000.0, 1888.0),
        (1000.0, 2112.0),
        (1112.0, 1888.0),
        (1112.0, 2000.0),
        (1112.0, 2112.0),
    ];
    assert_eq!(at, want);
    assert!(bars.iter().all(|b| near(&json!(xy(b, 0).2), 25.0) && near(&json!(xy(b, 1).2), 2975.0)), "{bars:?}");
    assert!(near(&stirrups[0]["length"], 980.0), "{}", stirrups[0]);
    // A tie runs from the outer face of one bar to the outer face of the one across.
    assert!(ties.iter().all(|b| near(&b["length"], 240.0) && b["path"].as_array().map(Vec::len) == Some(2)), "{ties:?}");
}

#[test]
fn a_column_turned_to_an_angle_has_its_bars_turned_with_it_and_anchors_run_past_its_ends() {
    let mut s = Session::new();
    let host = column(&mut s, 90.0);
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": {"top": 25, "bottom": 50, "sides": 25}, "bars": {"diameter": 16, "x": 2, "y": 3}, "anchor": {"below": 400, "above": 600}}),
    );
    // Width along north: four corners and one bar in the middle of each face along the depth, six bars.
    let bars = on(&s, host, "bars");
    assert_eq!(bars.len(), 6);
    // No stirrup, so 25 cover and half a bar: 117 mm from the middle; north-west of the middle is a corner too.
    let corner = bars.iter().find(|b| (xy(b, 0).0 - 883.0).abs() < 1e-6 && (xy(b, 0).1 - 2117.0).abs() < 1e-6);
    assert!(corner.is_some(), "{bars:?}");
    // 400 below the base, 600 above the top.
    assert!(bars.iter().all(|b| near(&json!(xy(b, 0).2), -400.0) && near(&json!(xy(b, 1).2), 3600.0)), "{bars:?}");
    // The cover at the base is the bottom's, 50, when the bars stop short of it.
    run(&mut s, "reinforce", json!({"host": host, "cover": {"top": 25, "bottom": 50, "sides": 25}, "bars": {"diameter": 16}}));
    let bars = on(&s, host, "bars");
    assert_eq!(bars.len(), 4);
    assert!(bars.iter().all(|b| near(&json!(xy(b, 0).2), 50.0) && near(&json!(xy(b, 1).2), 2975.0)), "{bars:?}");
}

#[test]
fn a_column_that_cannot_be_armed_as_asked_is_refused() {
    let mut s = Session::new();
    let host = column(&mut s, 0.0);
    let round = run(&mut s, "structural_column", json!({"at": [5000, 0], "type": "Concrete Round 400"}))["id"].as_u64().unwrap();
    let base = json!({"host": host, "cover": 25});
    let with = |extra: Value| -> Value {
        let mut v = base.clone();
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        v
    };
    for (extra, says) in [
        (json!({"bars": {"diameter": 16, "x": 1}}), "bars"),
        (json!({"bars": {"diameter": 16, "x": 2000}}), "bars"),
        (json!({"bars": {"diameter": 16, "x": 9, "y": 9}}), "clearance"),
        (json!({"bars": {"diameter": 16}, "ties": {"diameter": 6.3}}), "tie"),
        (json!({"bars": {"diameter": 16}, "mesh": {}}), "mesh"),
        (json!({"bars": {"diameter": 16}, "bottom": {"count": 2, "diameter": 16}}), "bottom"),
        (json!({"stirrup": {"diameter": 5, "spacing": 150}, "anchor": {"below": -5}}), "anchor"),
        (json!({"cover": 200, "bars": {"diameter": 16}}), "narrow"),
        (json!({}), "nothing"),
    ] {
        let e = fails(&mut s, "reinforce", with(extra.clone()));
        assert!(e.contains(says), "{extra}: {e}");
    }
    let e = fails(&mut s, "reinforce", json!({"host": round, "cover": 25, "bars": {"diameter": 16}}));
    assert!(e.contains("round"), "{e}");
}

/// A footing 1200 x 1200 and 400 thick with its top at the level, at 3 m east, 3 m north.
fn footing(s: &mut Session) -> u64 {
    run(s, "footing", json!({"at": [3000, 3000], "type": "Footing 1200 x 1200"}))["id"].as_u64().unwrap()
}

#[test]
fn a_footing_gets_two_layers_of_mesh_with_their_ends_turned_up() {
    let mut s = Session::new();
    let host = footing(&mut s);
    let r = run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 50, "mesh": {"x": {"diameter": 10, "spacing": 150}, "y": {"diameter": 10, "spacing": 150}, "hook": 100}}),
    );
    // 1090 mm between the outer bars' centres at no more than 150: eight gaps, nine bars each way.
    let (mx, my) = (on(&s, host, "mesh x"), on(&s, host, "mesh y"));
    assert_eq!((mx.len(), my.len(), ids(&r).len()), (9, 9, 18));
    // The x bars lie on the cover and run 50 short of each end, 1100 mm, with a leg of 100 turned up at both: 1300 mm.
    let mut ys: Vec<f64> = mx.iter().map(|b| xy(b, 1).1).collect();
    ys.sort_by(f64::total_cmp);
    assert!((ys[0] - (3000.0 - 545.0)).abs() < 1e-6 && (ys[8] - (3000.0 + 545.0)).abs() < 1e-6, "{ys:?}");
    for b in &mx {
        // The footing's bottom is at z -400; a 10 mm bar on 50 of cover is centred 55 above it, the second layer 65.
        assert!(near(&b["length"], 1300.0) && b["path"].as_array().map(Vec::len) == Some(4), "{b}");
        assert!(near(&json!(xy(b, 1).2), -345.0) && near(&json!(xy(b, 0).2), -245.0), "{b}");
        assert!((xy(b, 1).0 - 2450.0).abs() < 1e-6 && (xy(b, 2).0 - 3550.0).abs() < 1e-6, "{b}");
    }
    assert!(my.iter().all(|b| near(&json!(xy(b, 1).2), -335.0)), "{my:?}");
}

#[test]
fn a_footing_refuses_a_mesh_that_does_not_fit_its_depth() {
    let mut s = Session::new();
    let host = footing(&mut s);
    let m = json!({"x": {"diameter": 10, "spacing": 150}});
    for (extra, says) in [
        (json!({"mesh": {"x": {"diameter": 10, "spacing": 150}, "hook": 400}}), "hook"),
        (json!({"cover": 200, "mesh": m.clone()}), "thickness"),
        (json!({"mesh": {}}), "nothing"),
        (json!({"mesh": {"z": {"diameter": 10, "spacing": 150}}}), "z"),
        (json!({"bottom": {"count": 2, "diameter": 10}}), "bottom"),
        (json!({"mesh": {"x": {"diameter": 10, "spacing": 10}}}), "spacing"),
    ] {
        let mut p = json!({"host": host, "cover": 50});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let e = fails(&mut s, "reinforce", p);
        assert!(e.contains(says), "{extra}: {e}");
    }
}

/// A floor 4 m by 3 m, 200 thick, its top at the level.
fn slab(s: &mut Session, extra: Value) -> u64 {
    let mut p = json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]});
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    run(s, "floor", p)["id"].as_u64().unwrap()
}

#[test]
fn a_floor_gets_a_mesh_at_the_bottom_and_one_at_the_top() {
    let mut s = Session::new();
    let host = slab(&mut s, json!({}));
    let r = run(
        &mut s,
        "reinforce",
        json!({
            "host": host, "cover": 20,
            "bottom": {"x": {"diameter": 8, "spacing": 150}, "y": {"diameter": 8, "spacing": 200}},
            "top": {"x": {"diameter": 6.3, "spacing": 200}}
        }),
    );
    let (bx, by, tx) = (on(&s, host, "bottom x"), on(&s, host, "bottom y"), on(&s, host, "top x"));
    // 2952 mm of room across the 3 m at no more than 150: 20 gaps, 21 bars; 3952 along the 4 m at 200: 21; the top x bars at 200
    // across 2955.7: 15 gaps, 16 bars.
    assert_eq!((bx.len(), by.len(), tx.len(), ids(&r).len()), (21, 21, 16, 58));
    // The x bars run 20 in from each edge: 3960. The bottom is at z -200: 20 cover and half a bar up, then the y layer on them.
    assert!(bx.iter().all(|b| near(&b["length"], 3960.0) && near(&json!(xy(b, 0).2), -176.0)), "{bx:?}");
    assert!(by.iter().all(|b| near(&b["length"], 2960.0) && near(&json!(xy(b, 0).2), -168.0)), "{by:?}");
    // The top mesh hangs from the top face, 20 cover and half a bar down.
    assert!(tx.iter().all(|b| near(&json!(xy(b, 0).2), -23.15)), "{tx:?}");
}

#[test]
fn a_floors_bars_stop_at_its_outline_and_round_its_openings() {
    let mut s = Session::new();
    // An L: 4 m wide below 1.4 m, 1.5 m wide above it, up to 3 m.
    let host = slab(&mut s, json!({"boundary": [[0, 0], [4000, 0], [4000, 1400], [1500, 1400], [1500, 3000], [0, 3000]]}));
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 150}, "y": {"diameter": 8, "spacing": 200}}}),
    );
    let (bx, by) = (on(&s, host, "bottom x"), on(&s, host, "bottom y"));
    // Bars across y at 24 + 147.6 k: ten below 1400 run the whole 4 m (3960), eleven above run 1500 less the covers (1460).
    assert_eq!(
        (bx.iter().filter(|b| near(&b["length"], 3960.0)).count(), bx.iter().filter(|b| near(&b["length"], 1460.0)).count()),
        (10, 11)
    );
    // Bars along y at 24 + 197.6 k: eight in the tall arm (2960), thirteen in the low one, 1400 less the covers (1360).
    assert_eq!(
        (by.iter().filter(|b| near(&b["length"], 2960.0)).count(), by.iter().filter(|b| near(&b["length"], 1360.0)).count()),
        (8, 13)
    );

    // A hole is gone round: the bars it crosses are in two pieces, each a cover short of it.
    let host = slab(&mut s, json!({"openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}));
    run(&mut s, "reinforce", json!({"host": host, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 150}}}));
    let bx = on(&s, host, "bottom x");
    // y = 24 + 147.6 k is inside the hole for k = 7 to 13: seven bars of two pieces, 14 of 3960 less nothing: 14 whole, 7 * 2.
    assert_eq!(bx.len(), 14 + 14);
    assert_eq!(bx.iter().filter(|b| near(&b["length"], 960.0)).count(), 7);
    assert_eq!(bx.iter().filter(|b| near(&b["length"], 1960.0)).count(), 7);
}

#[test]
fn a_floor_that_cannot_be_armed_is_refused() {
    let mut s = Session::new();
    let host = slab(&mut s, json!({}));
    let bottom = json!({"x": {"diameter": 8, "spacing": 150}});
    for (extra, says) in [
        // 90 of cover at both faces in 200 of slab: the meshes meet.
        (json!({"cover": 90, "bottom": bottom.clone(), "top": bottom.clone()}), "meet"),
        (json!({"bottom": {}}), "nothing"),
        (json!({"stirrup": {"diameter": 5, "spacing": 150}, "bottom": bottom.clone()}), "stirrup"),
    ] {
        let mut p = json!({"host": host, "cover": 20});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let e = fails(&mut s, "reinforce", p);
        assert!(e.contains(says), "{extra}: {e}");
    }
}

#[test]
fn a_floor_too_big_for_one_command_is_refused_not_cut_short() {
    let mut s = Session::new();
    let host = slab(&mut s, json!({"boundary": [[0, 0], [40000, 0], [40000, 30000], [0, 30000]]}));
    let e = fails(&mut s, "reinforce", json!({"host": host, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 28}}}));
    assert!(e.contains("1000"), "{e}");
}

#[test]
fn hostile_parameters_for_every_kind_of_host_are_refused_or_made_never_a_crash() {
    let mut s = Session::new();
    s.model_mut().types.footing.insert("Round 1000".into(), crate::model::ColumnType { width: 1000.0, depth: 1000.0, round: true });
    let round_column =
        run(&mut s, "structural_column", json!({"at": [9000, 0], "type": "Concrete Round 400", "height": 3000}))["id"].as_u64().unwrap();
    let round_footing = run(&mut s, "footing", json!({"at": [9000, 5000], "type": "Round 1000"}))["id"].as_u64().unwrap();
    let sloped = slab(&mut s, json!({"slope": 7, "arrow": [[0, 0], [4000, 3000]]}));
    let hosts = [beam(&mut s), column(&mut s, 33.0), footing(&mut s), slab(&mut s, json!({})), round_column, round_footing, sloped];
    let row = |k: Value, d: Value| json!({"count": k, "diameter": d});
    let pitch = |d: Value, sp: Value| json!({"diameter": d, "spacing": sp});
    let mut hostile = Vec::new();
    for n in [json!(0), json!(-1), json!(1e300), json!(5e-324), json!(2.5), json!("x"), json!(null), json!([]), json!({}), json!(1e9)] {
        hostile.push(json!({"cover": n.clone(), "bottom": row(json!(2), json!(16)), "bars": {"diameter": 16}}));
        hostile.push(json!({"cover": 25, "bottom": row(n.clone(), json!(16)), "top": row(json!(2), n.clone())}));
        hostile.push(
            json!({"cover": 25, "stirrup": pitch(json!(5), n.clone()), "bars": {"diameter": n.clone(), "x": n.clone(), "y": n.clone()}}),
        );
        hostile
            .push(json!({"cover": 25, "mesh": {"x": pitch(n.clone(), json!(150)), "y": pitch(json!(10), n.clone()), "hook": n.clone()}}));
        hostile.push(json!({"cover": 25, "bottom": {"x": pitch(n.clone(), n.clone()), "y": pitch(json!(8), json!(100))}, "top": {"y": pitch(json!(8), n.clone())}}));
        hostile.push(json!({"cover": {"top": n.clone(), "bottom": n.clone(), "sides": n.clone()}, "stirrup": pitch(json!(5), json!(150)), "bars": {"diameter": 16}}));
        hostile.push(json!({"cover": 25, "stirrup": {"diameter": 5, "spacing": 150, "ends": {"length": n.clone(), "spacing": n.clone()}, "hook": n.clone()}, "ties": {"diameter": n.clone()}}));
        hostile.push(json!({"cover": 25, "bottom": row(json!(2), json!(16)), "top": {"count": 2, "diameter": 16, "hook": {"start": n.clone()}}, "bars": {"diameter": 16, "hook": {"below": n.clone(), "above": n.clone()}}}));
        hostile.push(
            json!({"cover": 25, "bottom": {"count": 2, "diameter": 16, "hook": n.clone()}, "bars": {"diameter": 16, "hook": n.clone()}}),
        );
        hostile.push(
            json!({"cover": 25, "bars": {"diameter": 16, "count": n.clone(), "hook": n.clone()}, "stirrup": pitch(json!(5), json!(150)), "ties": {"diameter": 5, "hook": n.clone()}}),
        );
        hostile.push(
            json!({"cover": 25, "bottom": {"x": pitch(json!(8), json!(150)), "hook": n.clone()}, "top": {"y": pitch(json!(8), json!(150)), "hook": n.clone()}}),
        );
        hostile.push(json!({"cover": 25, "bars": {"diameter": 16}, "anchor": {"start": n.clone(), "end": n.clone(), "below": n.clone(), "above": n.clone()}, "skin": pitch(json!(8), n.clone()), "clearance": n}));
    }
    for host in hosts {
        for p in &hostile {
            let mut p = p.clone();
            if let Some(o) = p.as_object_mut() {
                o.insert("host".into(), json!(host));
            }
            let before = s.model.clone();
            match s.execute("reinforce", &p) {
                Ok(_) => assert!(s.model.check().is_ok(), "{p}"),
                Err(e) => {
                    assert!(!e.contains("internal error"), "{p}: {e}");
                    assert_eq!(*s.model, *before, "{p}");
                }
            }
        }
    }
}

#[test]
fn several_hosts_are_armed_by_one_command_or_none_is() {
    let mut s = Session::new();
    let a = beam(&mut s);
    let b = ids(&run(&mut s, "beam", json!({"points": [[0, 1000], [4000, 1000]]})))[0];
    let spec = json!({"hosts": [a, b], "cover": 25, "bottom": {"count": 2, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}});
    let r = run(&mut s, "reinforce", spec.clone());
    // 6 m: 2 bars and 41 stirrups; 4 m: 2 and 28.
    assert_eq!((ids(&r).len(), r["hosts"].as_u64(), r["removed"].as_u64()), (73, Some(2), Some(0)));
    assert_eq!((on(&s, a, "stirrup").len(), on(&s, b, "stirrup").len(), on(&s, b, "bottom").len()), (41, 28, 2));
    // Again: each host's bars are replaced, and a host given twice is armed once.
    let mut again = spec.clone();
    again["hosts"] = json!([a, b, a]);
    let r = run(&mut s, "reinforce", again);
    assert_eq!((ids(&r).len(), r["removed"].as_u64()), (73, Some(73)));
    // A beam that cannot take it refuses the lot, and says which: four 16 mm bars do not go in 150 mm.
    let narrow = ids(&run(&mut s, "beam", json!({"points": [[0, 3000], [4000, 3000]], "type": "Beam 150 x 400"})))[0];
    let mut four = spec;
    four["hosts"] = json!([a, narrow]);
    four["bottom"] = json!({"count": 4, "diameter": 16});
    let e = fails(&mut s, "reinforce", four);
    assert!(e.contains(&narrow.to_string()) && e.contains("beam") && e.contains("clearance"), "{e}");
    assert_eq!(on(&s, a, "bottom").len(), 2, "the first host keeps its bars");
}

#[test]
fn hosts_and_host_are_one_or_the_other_and_a_list_has_a_size() {
    let mut s = Session::new();
    let a = beam(&mut s);
    let one = json!({"cover": 25, "bottom": {"count": 2, "diameter": 16}});
    let with = |k: &str, v: Value| -> Value {
        let mut p = one.clone();
        p[k] = v;
        p
    };
    for (p, says) in [
        (with("hosts", json!([])), "hosts"),
        (with("hosts", json!("x")), "hosts"),
        (with("hosts", json!([999999])), "no element"),
        (with("hosts", json!(vec![a; 1001])), "hosts"),
    ] {
        let e = fails(&mut s, "reinforce", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    let mut both = with("hosts", json!([a]));
    both["host"] = json!(a);
    assert!(fails(&mut s, "reinforce", both).contains("not both"));
    // A list of one is armed as `host` arms it.
    assert_eq!(ids(&run(&mut s, "reinforce", with("hosts", json!([a])))).len(), 2);
}

#[test]
fn more_bars_than_a_command_may_make_across_its_hosts_are_refused() {
    let mut s = Session::new();
    // Each floor takes 965 bars, a little under what one host may have; twenty-one are past what a command makes.
    let floors: Vec<u64> = (0..21)
        .map(|_| run(&mut s, "floor", json!({"boundary": [[0, 0], [27000, 0], [27000, 1000], [0, 1000]]}))["id"].as_u64().unwrap())
        .collect();
    let e = fails(&mut s, "reinforce", json!({"hosts": floors, "cover": 20, "bottom": {"y": {"diameter": 8, "spacing": 28}}}));
    assert!(e.contains("20000"), "{e}");
}

#[test]
fn a_beams_bars_end_in_hooks_up_at_the_bottom_and_down_at_the_top() {
    let mut s = Session::new();
    let host = beam(&mut s);
    run(
        &mut s,
        "reinforce",
        json!({
            "host": host, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150},
            "bottom": {"count": 2, "diameter": 16, "hook": 200}, "top": {"count": 2, "diameter": 12.5, "hook": {"start": 100}}
        }),
    );
    // Bottom bars: a leg of 200 turned up at each end, from 462 below the top of the beam to 262: 5950 + 400.
    for b in on(&s, host, "bottom") {
        assert_eq!(b["path"].as_array().map(Vec::len), Some(4), "{b}");
        assert!(near(&b["length"], 6350.0), "{b}");
        assert!((xy(&b, 0).2 + 262.0).abs() < 1e-6 && (xy(&b, 1).2 + 462.0).abs() < 1e-6 && (xy(&b, 3).2 + 262.0).abs() < 1e-6, "{b}");
        assert!((xy(&b, 0).0 - 25.0).abs() < 1e-6 && (xy(&b, 3).0 - 5975.0).abs() < 1e-6, "{b}");
    }
    // Top bars: a leg of 100 turned down at the start only.
    for b in on(&s, host, "top") {
        assert_eq!(b["path"].as_array().map(Vec::len), Some(3), "{b}");
        assert!(near(&b["length"], 6050.0), "{b}");
        assert!((xy(&b, 0).2 + 136.25).abs() < 1e-6 && (xy(&b, 1).2 + 36.25).abs() < 1e-6, "{b}");
    }
    // A hook that does not go inside the stirrup, or is not a length, is refused.
    for (hook, says) in
        [(json!(500), "hook"), (json!(-5), "hook"), (json!({"start": 1e300}), "hook"), (json!("x"), "hook"), (json!({"middle": 5}), "hook")]
    {
        let e = fails(
            &mut s,
            "reinforce",
            json!({"host": host, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150}, "bottom": {"count": 2, "diameter": 16, "hook": hook}}),
        );
        assert!(e.contains(says), "{e}");
    }
}

#[test]
fn a_columns_bars_end_in_hooks_turned_into_the_section() {
    let mut s = Session::new();
    let host = column(&mut s, 0.0);
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150}, "bars": {"diameter": 16, "x": 3, "y": 3, "hook": {"below": 150}}}),
    );
    let bars = on(&s, host, "bars");
    assert_eq!(bars.len(), 8);
    // Every bar has its leg at the foot, toward the middle of the section, and none at the top.
    assert!(bars.iter().all(|b| b["path"].as_array().map(Vec::len) == Some(3) && near(&b["length"], 2950.0 + 150.0)), "{bars:?}");
    // The corner at 888, 2112: its leg runs south from y 2112 to 1962 at the foot.
    let corner = bars.iter().find(|b| (xy(b, 1).0 - 888.0).abs() < 1e-6 && (xy(b, 1).1 - 2112.0).abs() < 1e-6).expect("corner");
    assert!(
        (xy(corner, 0).1 - 1962.0).abs() < 1e-6 && (xy(corner, 0).0 - 888.0).abs() < 1e-6 && (xy(corner, 0).2 - 25.0).abs() < 1e-6,
        "{corner}"
    );
    // The bar in the middle of a side runs across: 112 in from the west face it goes east.
    let side = bars.iter().find(|b| (xy(b, 1).0 - 888.0).abs() < 1e-6 && (xy(b, 1).1 - 2000.0).abs() < 1e-6).expect("side");
    assert!((xy(side, 0).0 - 1038.0).abs() < 1e-6, "{side}");
    // Past the middle of the section is no hook.
    let e = fails(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150}, "bars": {"diameter": 16, "hook": 300}}),
    );
    assert!(e.contains("hook"), "{e}");
    run(&mut s, "reinforce", json!({"host": host, "cover": 25, "bars": {"diameter": 16, "hook": {"above": 100}}}));
    assert!(on(&s, host, "bars").iter().all(|b| b["path"].as_array().map(Vec::len) == Some(3) && (xy(b, 2).2 - 2975.0).abs() < 1e-6));
}

use crate::render::{self, Drawing, Prim, View};

fn plan_view() -> View {
    View { scale: 0.1, cx: 3000.0, cy: 2000.0, width: 1400, height: 800 }
}

/// What a drawing draws of element `id`.
fn drawn(d: &Drawing, id: u64) -> Vec<&Prim> {
    d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)).map(|(p, _)| p).collect()
}

#[test]
fn a_plan_draws_the_bars_under_its_cut_plane_solid_the_ones_over_it_dashed_and_a_standing_bar_as_a_dot() {
    let mut s = Session::new();
    let slab = slab(&mut s, json!({}));
    run(&mut s, "reinforce", json!({"host": slab, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 150}}}));
    // A beam over the cut plane, 1200 up, and a column the plane cuts.
    let over = ids(&run(&mut s, "beam", json!({"points": [[0, 5000], [6000, 5000]], "offset": 2800})))[0];
    run(
        &mut s,
        "reinforce",
        json!({"host": over, "cover": 25, "bottom": {"count": 2, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}}),
    );
    let col =
        run(&mut s, "structural_column", json!({"at": [8000, 1000], "type": "Concrete 300 x 300", "height": 3000}))["id"].as_u64().unwrap();
    run(
        &mut s,
        "reinforce",
        json!({"host": col, "cover": 25, "bars": {"diameter": 16, "x": 3, "y": 3}, "stirrup": {"diameter": 5, "spacing": 150}}),
    );
    let d = render::draw(&s.model, 100000, plan_view());
    let of_host = |host: u64, layer: &str| -> Vec<u64> { on(&s, host, layer).iter().map(|b| b["id"].as_u64().unwrap()).collect() };

    // Under the cut plane: a thin solid line along each bar.
    let low = of_host(slab, "bottom x");
    assert_eq!(low.len(), 21);
    for id in &low {
        let prims = drawn(&d, *id);
        assert!(
            matches!(prims.as_slice(), [Prim::Line { dash: None, closed: false, pts, .. }] if pts.len() == 2),
            "{id}: {} prims",
            prims.len()
        );
    }
    // Over it, and under the view range's top: dashed.
    for id in of_host(over, "bottom") {
        assert!(matches!(drawn(&d, id).as_slice(), [Prim::Line { dash: Some(_), .. }]), "{id}");
    }
    // Stirrups over it too, but those at one place are drawn once: along the beam they are 41 places, each a tick across it.
    let ticks: usize = of_host(over, "stirrup").iter().map(|id| drawn(&d, *id).len()).sum();
    assert_eq!(ticks, 41, "a stirrup is seen from above as a line across the beam");
    // A column's bars stand: a dot each, and its stirrups, all at one place, are one square.
    let dots: Vec<u64> = of_host(col, "bars");
    assert_eq!(dots.len(), 8);
    assert!(dots.iter().all(|id| matches!(drawn(&d, *id).as_slice(), [Prim::Fill { .. }])), "dots");
    let squares: usize = of_host(col, "stirrup").iter().map(|id| drawn(&d, *id).len()).sum();
    assert_eq!(squares, 1);
    // The plan above sees none of the bars under it, nor the ones of its own level's neighbours.
    let up = render::draw(&s.model, 100001, plan_view());
    assert!(low.iter().all(|id| drawn(&up, *id).is_empty()));
    // Hidden by category, they are gone.
    s.execute("graphics", &json!({"view": "Level 1", "hide": ["rebar"]})).unwrap();
    let d = render::draw(&s.model, 100000, plan_view());
    assert!(low.iter().all(|id| drawn(&d, *id).is_empty()));
}

fn presets(s: &Session) -> Value {
    s.model
        .reinforcement
        .iter()
        .map(|(n, p)| (n.clone(), json!({"kind": p.kind, "params": p.params})))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

#[test]
fn a_preset_keeps_a_kind_of_hosts_parameters_and_arms_by_its_name() {
    let mut s = Session::new();
    let (a, b) = (beam(&mut s), ids(&run(&mut s, "beam", json!({"points": [[0, 1000], [4000, 1000]]})))[0]);
    let r = run(
        &mut s,
        "reinforcement_preset",
        json!({"name": "V 20 x 50", "kind": "beam", "params": {"cover": 25, "bottom": {"count": 3, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}}}),
    );
    assert_eq!(r["name"], json!("V 20 x 50"));
    assert_eq!(presets(&s)["V 20 x 50"]["kind"], json!("beam"));
    // Without a name, they are listed.
    assert_eq!(run(&mut s, "reinforcement_preset", json!({}))["presets"]["V 20 x 50"]["params"]["cover"], json!(25));
    // By its name: both beams armed alike.
    let r = run(&mut s, "reinforce", json!({"hosts": [a, b], "preset": "V 20 x 50"}));
    assert_eq!((ids(&r).len(), on(&s, a, "bottom").len(), on(&s, b, "stirrup").len()), (44 + 31, 3, 28));
    // What is given with it wins: stirrups every 100.
    run(&mut s, "reinforce", json!({"host": a, "preset": "V 20 x 50", "stirrup": {"diameter": 5, "spacing": 100}}));
    assert_eq!(on(&s, a, "stirrup").len(), 61);
    // It belongs to a kind of host, and a name it does not have is no preset.
    let column = column(&mut s, 0.0);
    let e = fails(&mut s, "reinforce", json!({"host": column, "preset": "V 20 x 50"}));
    assert!(e.contains("beam") && e.contains("column"), "{e}");
    let e = fails(&mut s, "reinforce", json!({"host": a, "preset": "nope"}));
    assert!(e.contains("nope"), "{e}");
    // The project keeps it, as an entry of its library with an id and a GUID of its own, and gives it again on opening.
    let path = std::env::temp_dir().join(format!("bimcraft-preset-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    assert_eq!(presets(&t), presets(&s));
    let _ = std::fs::remove_file(&path);
    assert!(s.model.library.entries().any(|(_, e)| e.name == "V 20 x 50" && e.kind == crate::library::Kind::ReinforcementPreset));
    // Renamed, and deleted.
    run(&mut s, "reinforcement_preset", json!({"name": "V 20 x 50", "rename": "Viga padrão"}));
    assert!(presets(&s).get("Viga padrão").is_some() && presets(&s).get("V 20 x 50").is_none());
    run(&mut s, "reinforce", json!({"host": a, "preset": "Viga padrão"}));
    run(&mut s, "reinforcement_preset", json!({"name": "Viga padrão", "delete": true}));
    assert!(presets(&s).as_object().is_some_and(serde_json::Map::is_empty));
}

#[test]
fn a_preset_that_is_not_one_of_a_kind_of_host_is_refused() {
    let mut s = Session::new();
    let ok = json!({"cover": 25, "bottom": {"count": 3, "diameter": 16}});
    for (p, says) in [
        (json!({"name": "A", "params": ok}), "kind"),
        (json!({"name": "A", "kind": "wall", "params": ok}), "kind"),
        (json!({"name": "A", "kind": "beam"}), "params"),
        (json!({"name": "A", "kind": "beam", "params": "x"}), "params"),
        (json!({"name": "A", "kind": "beam", "params": {"mesh": {}}}), "mesh"),
        (json!({"name": "A", "kind": "beam", "params": {"host": 100002}}), "host"),
        (json!({"name": "A", "kind": "beam", "params": {"preset": "B"}}), "preset"),
        (json!({"name": "  ", "kind": "beam", "params": ok}), ""),
        (json!({"name": "A", "kind": "beam", "params": ok, "colour": 1}), "colour"),
        (json!({"name": "x".repeat(100_000), "kind": "beam", "params": ok}), "name"),
        (json!({"name": "A", "kind": "beam", "params": {"cover": "y".repeat(100_000)}}), "params"),
        (json!({"name": "A", "delete": true}), "no reinforcement preset"),
        (json!({"name": "A", "rename": "B"}), "kind"),
    ] {
        let before = s.model.clone();
        match s.execute("reinforcement_preset", &p) {
            Err(e) => assert!(e.contains(says), "{p}: {e}"),
            Ok(_) if p["name"].as_str().is_some_and(|n| n.trim().is_empty()) => {}
            Ok(r) => panic!("{p} made {r}"),
        }
        assert!(s.model.check().is_ok());
        let _ = before;
    }
    // A preset is not checked against any host until it is used: this one fits no beam, and says so then.
    run(
        &mut s,
        "reinforcement_preset",
        json!({"name": "wide", "kind": "beam", "params": {"cover": 25, "bottom": {"count": 9, "diameter": 25}}}),
    );
    let host = beam(&mut s);
    let e = fails(&mut s, "reinforce", json!({"host": host, "preset": "wide"}));
    assert!(e.contains("clearance"), "{e}");
    // The most presets a project holds.
    for i in 0..crate::reinforce::MAX_PRESETS - 1 {
        run(&mut s, "reinforcement_preset", json!({"name": format!("p{i}"), "kind": "beam", "params": {"cover": 25}}));
    }
    let e = fails(&mut s, "reinforcement_preset", json!({"name": "one too many", "kind": "beam", "params": {"cover": 25}}));
    assert!(e.contains(&crate::reinforce::MAX_PRESETS.to_string()), "{e}");
}

fn distance_from(b: &Value, i: usize, x: f64, y: f64) -> f64 {
    let (px, py, _) = xy(b, i);
    (px - x).hypot(py - y)
}

#[test]
fn a_round_column_gets_bars_round_a_circle_and_round_hoops() {
    let mut s = Session::new();
    let host =
        run(&mut s, "structural_column", json!({"at": [1000, 2000], "type": "Concrete Round 400", "height": 3000}))["id"].as_u64().unwrap();
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150, "hook": 50}, "bars": {"diameter": 16, "count": 8, "hook": {"below": 100}}}),
    );
    // Eight bars on a circle 162 mm from the middle (200 less 25 of cover, 5 of hoop, 8 for half the bar), a leg at the foot.
    let bars = on(&s, host, "bars");
    assert_eq!(bars.len(), 8);
    for b in &bars {
        assert_eq!(b["path"].as_array().map(Vec::len), Some(3), "{b}");
        assert!((distance_from(b, 1, 1000.0, 2000.0) - 162.0).abs() < 1e-6, "{b}");
        // The leg runs 100 toward the middle: 62 from it.
        assert!((distance_from(b, 0, 1000.0, 2000.0) - 62.0).abs() < 1e-6, "{b}");
        assert!(near(&b["length"], 2950.0 + 100.0), "{b}");
    }
    assert!(bars.iter().any(|b| (xy(b, 1).0 - 1162.0).abs() < 1e-6 && (xy(b, 1).1 - 2000.0).abs() < 1e-6));
    // Hoops of 24 sides 172.5 from the middle, a tail of 50 at each end: 27 points, the perimeter and the tails.
    let hoops = on(&s, host, "stirrup");
    assert_eq!(hoops.len(), 21);
    let side = 2.0 * 172.5 * (std::f64::consts::PI / 24.0).sin();
    assert!(
        hoops
            .iter()
            .all(|b| b["path"].as_array().map(Vec::len) == Some(27) && near(&b["length"], ((24.0 * side + 100.0) * 100.0).round() / 100.0)),
        "{:?}",
        hoops.first()
    );
}

#[test]
fn a_round_column_takes_a_count_and_a_rectangular_one_takes_x_and_y() {
    let mut s = Session::new();
    let round =
        run(&mut s, "structural_column", json!({"at": [5000, 0], "type": "Concrete Round 400", "height": 3000}))["id"].as_u64().unwrap();
    let rect = column(&mut s, 0.0);
    for (host, extra, says) in [
        (round, json!({"bars": {"diameter": 16, "x": 3}}), "round"),
        (round, json!({"bars": {"diameter": 16}}), "count"),
        (round, json!({"bars": {"diameter": 16, "count": 2}}), "count"),
        (round, json!({"bars": {"diameter": 16, "count": 60}}), "clearance"),
        (round, json!({"bars": {"diameter": 16, "count": 8}, "stirrup": {"diameter": 5, "spacing": 150}, "ties": {"diameter": 5}}), "tie"),
        (round, json!({"bars": {"diameter": 16, "count": 8, "hook": 500}}), "hook"),
        (round, json!({"cover": 195, "bars": {"diameter": 16, "count": 8}}), "narrow"),
        (rect, json!({"bars": {"diameter": 16, "count": 8}}), "rectangular"),
    ] {
        let mut p = json!({"host": host, "cover": 25});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let e = fails(&mut s, "reinforce", p);
        assert!(e.contains(says), "{extra}: {e}");
    }
}

#[test]
fn a_round_footing_gets_a_mesh_whose_bars_stop_at_its_circle() {
    let mut s = Session::new();
    s.model_mut().types.footing.insert("Round 1000".into(), crate::model::ColumnType { width: 1000.0, depth: 1000.0, round: true });
    let host = run(&mut s, "footing", json!({"at": [3000, 3000], "type": "Round 1000"}))["id"].as_u64().unwrap();
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 50, "mesh": {"x": {"diameter": 10, "spacing": 150}, "y": {"diameter": 10, "spacing": 150}, "hook": 100}}),
    );
    let (mx, my) = (on(&s, host, "mesh x"), on(&s, host, "mesh y"));
    // 890 mm between the outer bars at no more than 150: six gaps, seven bars each way.
    assert_eq!((mx.len(), my.len()), (7, 7));
    for b in mx.iter().chain(&my) {
        // Each bar runs from the circle 450 from the middle (500 less the cover) to the circle, a leg of 100 turned up at each end.
        assert_eq!(b["path"].as_array().map(Vec::len), Some(4), "{b}");
        for i in [1, 2] {
            assert!((distance_from(b, i, 3000.0, 3000.0) - 450.0).abs() < 1e-6, "{b}");
        }
    }
    // The middle bar crosses the whole circle: 900, and its legs 200.
    assert!(mx.iter().any(|b| near(&b["length"], 1100.0)), "{:?}", mx.iter().map(|b| b["length"].clone()).collect::<Vec<_>>());
    let e = fails(&mut s, "reinforce", json!({"host": host, "cover": 500, "mesh": {"x": {"diameter": 10, "spacing": 150}}}));
    assert!(e.contains("fit") || e.contains("small") || e.contains("inside"), "{e}");
}

#[test]
fn a_sloped_floors_bars_follow_its_top_a_perpendicular_cover_below_it() {
    let mut s = Session::new();
    let host = slab(&mut s, json!({"slope": 5, "arrow": [[0, 0], [4000, 0]]}));
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 150}, "y": {"diameter": 8, "spacing": 200}}}),
    );
    let (tan, sec) = (5f64.to_radians().tan(), 1.0 / 5f64.to_radians().cos());
    let (bx, by) = (on(&s, host, "bottom x"), on(&s, host, "bottom y"));
    assert_eq!((bx.len(), by.len()), (21, 21));
    // The top rises with x from z 0 at x 0; the bars lie 20 + 4 mm from the underside, 176 from the top, square to it, which is
    // 176 / cos 5 plumb. They run from x 20 to 3980 along the slope.
    for b in &bx {
        let (z0, z1) = (xy(b, 0).2, xy(b, 1).2);
        assert!((z0 - (20.0 * tan - 176.0 * sec)).abs() < 1e-6 && (z1 - (3980.0 * tan - 176.0 * sec)).abs() < 1e-6, "{b}");
        assert!(near(&b["length"], ((3960.0 * (1.0 + tan * tan).sqrt()) * 100.0).round() / 100.0), "{b}");
    }
    // Across the slope the bars are level, on the layer above: 168 from the top.
    for b in &by {
        let x = xy(b, 0).0;
        assert!((xy(b, 0).2 - (x * tan - 168.0 * sec)).abs() < 1e-6 && (xy(b, 1).2 - xy(b, 0).2).abs() < 1e-9, "{b}");
    }
    // A mesh that does not fit the thickness is refused as on a level floor.
    let e = fails(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 90, "bottom": {"x": {"diameter": 8, "spacing": 150}}, "top": {"x": {"diameter": 8, "spacing": 200}}}),
    );
    assert!(e.contains("meet"), "{e}");
}

#[test]
fn a_floors_bars_end_in_hooks_up_at_the_bottom_and_down_at_the_top() {
    let mut s = Session::new();
    let host = slab(&mut s, json!({}));
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 150}, "hook": 100}, "top": {"x": {"diameter": 6.3, "spacing": 200}, "hook": 60}}),
    );
    for b in on(&s, host, "bottom x") {
        assert_eq!(b["path"].as_array().map(Vec::len), Some(4), "{b}");
        assert!(near(&b["length"], 3960.0 + 200.0), "{b}");
        assert!((xy(&b, 0).2 + 76.0).abs() < 1e-6 && (xy(&b, 1).2 + 176.0).abs() < 1e-6, "{b}");
    }
    for b in on(&s, host, "top x") {
        assert!(near(&b["length"], 3960.0 + 120.0), "{b}");
        assert!((xy(&b, 0).2 - (-23.15 - 60.0)).abs() < 1e-6, "{b}");
    }
    // A leg past the cover on the other face, or no length, is refused.
    for hook in [json!(160), json!(-1), json!(1e300)] {
        let e =
            fails(&mut s, "reinforce", json!({"host": host, "cover": 20, "bottom": {"x": {"diameter": 8, "spacing": 150}, "hook": hook}}));
        assert!(e.contains("hook"), "{e}");
    }
}

#[test]
fn a_tie_ends_in_tails_when_it_is_told_to() {
    let mut s = Session::new();
    let host = column(&mut s, 0.0);
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150}, "bars": {"diameter": 16, "x": 3, "y": 3}, "ties": {"diameter": 6.3, "hook": 50}}),
    );
    let ties = on(&s, host, "tie");
    assert_eq!(ties.len(), 42);
    // Across 240 between the bars' outer faces, and a tail of 50 at each end.
    assert!(ties.iter().all(|b| b["path"].as_array().map(Vec::len) == Some(4) && near(&b["length"], 240.0 + 100.0)), "{:?}", ties.first());
    let e = fails(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150}, "bars": {"diameter": 16, "x": 3, "y": 3}, "ties": {"diameter": 6.3, "hook": -1}}),
    );
    assert!(e.contains("hook"), "{e}");
}

#[test]
fn a_bar_lying_in_a_sections_plane_is_drawn_along_it_and_one_it_crosses_as_a_dot() {
    let mut s = Session::new();
    let host = beam(&mut s);
    run(
        &mut s,
        "reinforce",
        json!({"host": host, "cover": 25, "bottom": {"count": 1, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 1000}}),
    );
    let along = run(&mut s, "section", json!({"start": [-500, 0], "end": [6500, 0]}))["id"].as_u64().unwrap();
    let view = render::section_fit(&s.model, along, 1400, 600);
    let d = render::section(&s.model, along, view);
    // The one bar below, in the plane y 0 of the beam's axis: a strip from one end to the other, 25 in from each.
    let bar = on(&s, host, "bottom")[0]["id"].as_u64().unwrap();
    let fills: Vec<(f64, f64)> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(bar))
        .filter_map(|(p, _)| if let Prim::Fill { rings, .. } = p { Some(rings) } else { None })
        .map(|rings| {
            let xs: Vec<f64> = rings.iter().flatten().map(|q| d.view.mm(*q).0).collect();
            (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max))
        })
        .collect();
    assert!(fills.iter().any(|(lo, hi)| hi - lo > 5900.0 && hi - lo < 6000.0), "{fills:?}");
    // The section across the beam cuts the same bar once, a dot of about its thickness.
    let across = run(&mut s, "section", json!({"start": [3000, -1000], "end": [3000, 1000]}))["id"].as_u64().unwrap();
    let d = render::section(&s.model, across, render::section_fit(&s.model, across, 1400, 600));
    let dots: Vec<f64> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(bar))
        .filter_map(|(p, _)| if let Prim::Fill { rings, .. } = p { Some(rings) } else { None })
        .map(|rings| {
            let xs: Vec<f64> = rings.iter().flatten().map(|q| d.view.mm(*q).0).collect();
            xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min)
        })
        .collect();
    assert!(dots.iter().any(|w| (10.0..30.0).contains(w)), "{dots:?}");
}
