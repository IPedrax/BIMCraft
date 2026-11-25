//! Floor framing (`framing/floor.rs`) driven through commands: the cabin floor the user framed by hand is the acceptance
//! test.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::library::Kind;
use crate::model::Model;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn first(v: &Value) -> u64 {
    v["ids"][0].as_u64().or(v["id"].as_u64()).unwrap_or_else(|| panic!("no id in {v}"))
}

fn f(v: &Value, i: usize) -> f64 {
    v[i].as_f64().unwrap_or(f64::NAN)
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 0.02, "{what}: {a} isn't {b}");
}

/// The cabin's floor edges: x from FW to FE, y from FS to FN.
pub(crate) const FW: f64 = -51.35;
pub(crate) const FE: f64 = 7268.65;
pub(crate) const FS: f64 = -51.35;
pub(crate) const FN: f64 = 6048.65;
/// The middle beam's centre line.
pub(crate) const YL: f64 = 2998.65;

/// The cabin's floor: 18 mm of subfloor over a 184 mm structure layer (2x8 joists), its top at Level 1, framed a third of
/// 1220 apart with its joists running north, over three beams 114 x 235 whose tops are at the joists' underside (-202).
pub(crate) fn cabin(s: &mut Session) -> u64 {
    run(
        s,
        "type",
        json!({"category": "floor", "name": "2x8 Floor", "layers": [
            {"function": "substrate", "material": "Timber", "thickness": 18},
            {"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    run(s, "type", json!({"category": "framing", "name": "Cabin Floor", "spacing": 1220.0 / 3.0}));
    run(s, "type", json!({"category": "beam", "name": "3-2x10", "width": 114, "depth": 235}));
    for y in [5.65, YL, 5991.65] {
        run(s, "beam", json!({"points": [[FW, y], [FE, y]], "type": "3-2x10", "offset": -202}));
    }
    let id = first(&run(s, "floor", json!({"boundary": [[FW, FS], [FE, FS], [FE, FN], [FW, FN]], "type": "2x8 Floor"})));
    let r = run(s, "frame", json!({"id": id, "type": "Cabin Floor", "angle": 90}));
    assert_eq!(r["warnings"], json!([]), "{r}");
    id
}

/// A 2x4 wall type, for partitions standing on the floor.
pub(crate) fn stud_wall_type(s: &mut Session) {
    run(
        s,
        "type",
        json!({"category": "wall", "name": "2x4", "layers": [{"function": "structure", "material": "Timber", "thickness": 89}]}),
    );
}

fn framing(s: &mut Session, id: u64) -> Value {
    run(s, "framing", json!({"id": id}))
}

fn members(r: &Value) -> Vec<Value> {
    r["members"].as_array().cloned().unwrap_or_default()
}

/// A member's box in plan, [x0, y0, x1, y1], and its z [bottom, top], from a `framing` reply: in the host's frame, or its
/// own.
fn world(r: &Value, m: &Value) -> ([f64; 4], [f64; 2]) {
    let (o, x) = match m.get("frame") {
        Some(fr) => (fr["origin"].clone(), fr["x_axis"].clone()),
        None => (r["origin"].clone(), r["x_axis"].clone()),
    };
    let (ox, oy, xx, xy) = (f(&o, 0), f(&o, 1), f(&x, 0), f(&x, 1));
    let at = |s: f64, a: f64| (ox + xx * s - xy * a, oy + xy * s + xx * a);
    let corners = [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(i, j)| at(f(&m["s"], i), f(&m["across"], j)));
    let fold =
        |k: fn(&(f64, f64)) -> f64| corners.iter().map(k).fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| [lo.min(v), hi.max(v)]);
    let ([x0, x1], [y0, y1]) = (fold(|p| p.0), fold(|p| p.1));
    ([x0, y0, x1, y1], [f(&m["z"], 0), f(&m["z"], 2)])
}

fn role<'a>(ms: &'a [Value], role: &str) -> Vec<&'a Value> {
    ms.iter().filter(|m| m["role"] == role).collect()
}

fn keyed<'a>(ms: &'a [Value], key: &str) -> &'a Value {
    ms.iter().find(|m| m["key"] == key).unwrap_or_else(|| panic!("no member {key}: {:?}", ms.iter().map(|m| &m["key"]).collect::<Vec<_>>()))
}

fn len(m: &Value) -> f64 {
    m["length"].as_f64().unwrap_or(f64::NAN)
}

/// A member's plan rectangle's corners, from a `framing` reply: in the host's frame, or its own.
fn corners(r: &Value, m: &Value) -> [(f64, f64); 4] {
    let (o, x) = match m.get("frame") {
        Some(fr) => (fr["origin"].clone(), fr["x_axis"].clone()),
        None => (r["origin"].clone(), r["x_axis"].clone()),
    };
    let (ox, oy, xx, xy) = (f(&o, 0), f(&o, 1), f(&x, 0), f(&x, 1));
    let at = |s: f64, a: f64| (ox + xx * s - xy * a, oy + xy * s + xx * a);
    [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(i, j)| at(f(&m["s"], i), f(&m["across"], j)))
}

/// No two members of `r` overlap in plan (by more than a micron, on every axis of their sides).
fn none_overlap(r: &Value) {
    let ms = members(r);
    let rects: Vec<(String, [(f64, f64); 4])> = ms.iter().map(|m| (m["key"].as_str().unwrap_or("").to_string(), corners(r, m))).collect();
    let apart = |a: &[(f64, f64); 4], b: &[(f64, f64); 4]| {
        let sides = |c: &[(f64, f64); 4]| [(c[1].0 - c[0].0, c[1].1 - c[0].1), (c[3].0 - c[0].0, c[3].1 - c[0].1)];
        sides(a).into_iter().chain(sides(b)).any(|(dx, dy)| {
            let l = dx.hypot(dy);
            let span = |c: &[(f64, f64); 4]| {
                c.iter().map(|p| (p.0 * dx + p.1 * dy) / l).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
            };
            let ((a0, a1), (b0, b1)) = (span(a), span(b));
            a1.min(b1) - a0.max(b0) <= 1e-3
        })
    };
    for (i, (ka, a)) in rects.iter().enumerate() {
        for (kb, b) in rects.iter().skip(i + 1) {
            assert!(apart(a, b), "{ka} and {kb} overlap: {a:?} {b:?}");
        }
    }
}

/// The x of each joist line's centre, from west to east, each once.
fn joist_lines(r: &Value) -> Vec<f64> {
    let mut xs: Vec<f64> = role(&members(r), "joist")
        .into_iter()
        .map(|m| {
            let b = world(r, m).0;
            ((b[0] + b[2]) / 2.0 * 100.0).round() / 100.0
        })
        .collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    xs
}

#[test]
fn the_cabin_floor_has_rims_north_and_south_and_19_lines_of_joists_each_in_two_pieces_butting_on_the_middle_beam() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let r = framing(&mut s, id);
    let ms = members(&r);
    assert_eq!(r["warnings"], json!([]));
    // The rims: along the south and north edges, the floor's whole width, a joist thick and deep, 19 in; 7320 long, more than
    // the longest stock (6100), each spliced in two over the ninth grid joist, 3660 along, each piece from 3660.
    let rims = role(&ms, "rim");
    assert_eq!(rims.len(), 4, "{rims:?}");
    let mut ys: Vec<f64> = rims.iter().map(|m| (world(&r, m).0[1] + world(&r, m).0[3]) / 2.0).collect();
    ys.sort_by(f64::total_cmp);
    close(ys[0], FS + 19.0, "the south rim");
    close(ys[3], FN - 19.0, "the north rim");
    for edge in [0, 2] {
        let mut pieces: Vec<[f64; 2]> = (0..2)
            .map(|n| {
                let rim = keyed(&ms, &format!("rim/{edge}/{n}"));
                close(len(rim), 3660.0, "a rim's piece");
                assert_eq!((&rim["section"], &rim["stock"]), (&json!([38.0, 184.0]), &json!(3660.0)));
                let ([x0, _, x1, _], [z0, z1]) = world(&r, rim);
                close(z0, -202.0, "on the beams");
                close(z1, -18.0, "under the subfloor");
                [x0, x1]
            })
            .collect();
        pieces.sort_by(|a, b| a[0].total_cmp(&b[0]));
        for ([x0, x1], [from, to]) in pieces.into_iter().zip([[FW, FW + 3660.0], [FW + 3660.0, FE]]) {
            close(x0, from, "a rim's piece from");
            close(x1, to, "to");
        }
    }
    // The joists: flush with the east and west edges, and 17 between, a third of 1220 apart.
    let xs = joist_lines(&r);
    assert_eq!(xs.len(), 19, "{xs:?}");
    close(xs[0], FW + 19.0, "the west edge joist");
    close(xs[18], FE - 19.0, "the east edge joist");
    for (k, x) in xs.iter().enumerate().take(18).skip(1) {
        close(*x, FW + k as f64 * 1220.0 / 3.0, "a grid joist");
    }
    // Each line in two pieces that butt on the middle beam's centre line, the beams at its ends splitting nothing.
    let joists = role(&ms, "joist");
    assert_eq!(joists.len(), 38);
    for j in &joists {
        close(len(j), 3012.0, "a joist's length");
        assert_eq!(j["section"], json!([38.0, 184.0]));
        let [_, y0, _, y1] = world(&r, j).0;
        assert!(
            (y0 - (FS + 38.0)).abs() < 0.02 && (y1 - YL).abs() < 0.02 || (y0 - YL).abs() < 0.02 && (y1 - (FN - 38.0)).abs() < 0.02,
            "{j}"
        );
    }
    // The beams bear it; the longest clear span is between the middle one's face and an end one's.
    let beams: Vec<u64> = s.model.elements.iter().filter(|(_, e)| e.category() == "beam").map(|(id, _)| *id).collect();
    let mut supports: Vec<u64> = r["supports"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    supports.sort_unstable();
    assert_eq!(supports, beams);
    close(f(&json!([r["span"]]), 0), 2879.0, "the clear span");
    close(f(&json!([r["angle"]]), 0), 90.0, "the joists run north");
    none_overlap(&r);
}

#[test]
fn the_cut_list_and_the_schedule_list_38_joists_from_3050_stock_and_two_rims_each_spliced_from_3660_stock() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let r = framing(&mut s, id);
    let cuts = r["cut_list"].as_array().unwrap();
    let joists = cuts.iter().find(|c| c["role"] == "joist").unwrap();
    assert_eq!(
        (joists["count"].as_u64(), joists["length"].as_f64(), joists["stock"].as_f64(), &joists["section"]),
        (Some(38), Some(3012.0), Some(3050.0), &json!([38.0, 184.0]))
    );
    let rims = cuts.iter().find(|c| c["role"] == "rim").unwrap();
    assert_eq!((rims["count"].as_u64(), rims["length"].as_f64(), rims["stock"].as_f64()), (Some(4), Some(3660.0), Some(3660.0)));
    assert_eq!(cuts.len(), 2, "{cuts:?}");
    // The schedule: each member's host is the floor.
    let t = run(
        &mut s,
        "schedule",
        json!({"name": "Floor Cut List", "category": "framingmembers", "fields": ["Host", "Host Type", "Role", "Section", "Length",
               "Stock Length", "Count"], "sort": ["Role", "Section", "Length"], "grouped": true}),
    );
    let rows: Vec<Vec<Value>> = t["rows"].as_array().unwrap().iter().map(|r| r.as_array().unwrap().clone()).collect();
    assert_eq!(
        rows,
        [
            vec![json!(id as f64), json!("2x8 Floor"), json!("joist"), json!("38 x 184"), json!(3012.0), json!(3050.0), json!(38.0)],
            vec![json!(id as f64), json!("2x8 Floor"), json!("rim"), json!("38 x 184"), json!(3660.0), json!(3660.0), json!(4.0)],
        ]
    );
}

#[test]
fn a_joist_longer_than_any_stock_between_what_bears_it_is_warned_of_and_a_rim_that_cant_be_spliced_too() {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "2x8 Floor", "layers": [{"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    // 2000 wide, its joists running 7000 north with nothing under them: each 6924 between the rims, more than 6100.
    let id = first(&run(&mut s, "floor", json!({"boundary": [[0, 0], [2000, 0], [2000, 7000], [0, 7000]], "type": "2x8 Floor"})));
    let r = run(&mut s, "frame", json!({"id": id, "angle": 90}));
    let warnings: Vec<String> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(String::from)).collect();
    for key in ["edge/1/0", "joist/1/0", "edge/3/0"] {
        assert!(
            warnings.iter().any(|w| w.contains(&format!("{key} is 6924 mm long")) && w.contains("longer than any stock")),
            "{key}: {warnings:?}"
        );
    }
    // Studs of stock 1000 and joists 1220 apart: no splice over a joist keeps the 2000 mm rims within it.
    run(&mut s, "type", json!({"category": "framing", "name": "2x4 @ 406", "spacing": 1220, "stock": [1000]}));
    let r = framing(&mut s, id);
    let warnings: Vec<String> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(String::from)).collect();
    for key in ["rim/0", "rim/2"] {
        assert!(warnings.iter().any(|w| w.contains(&format!("{key} has a piece")) && w.contains("longer than any stock")), "{warnings:?}");
    }
}

#[test]
fn a_partition_along_the_joists_gets_a_pair_under_it_in_place_of_the_grid_joist() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    stud_wall_type(&mut s);
    let w = first(&run(&mut s, "wall", json!({"points": [[3608.65, FS + 89.0], [3608.65, YL]], "type": "2x4", "height": 2448})));
    run(&mut s, "frame", json!({"id": w}));
    let g = s.model.guid(w).to_string();
    let r = framing(&mut s, id);
    assert_eq!(r["warnings"], json!([]));
    let ms = members(&r);
    // Ply 0 on the right of the joists' way (north): east.
    for (ply, x) in [(0, 3608.65 + 19.0), (1, 3608.65 - 19.0)] {
        let pieces: Vec<&Value> = ms.iter().filter(|m| m["key"].as_str().unwrap_or("").starts_with(&format!("under/{g}/{ply}/"))).collect();
        assert_eq!(pieces.len(), 2, "the whole floor long, cut over the middle beam");
        for p in pieces {
            let b = world(&r, p).0;
            close((b[0] + b[2]) / 2.0, x, "a joist of the pair");
        }
    }
    let xs = joist_lines(&r);
    assert_eq!(xs.len(), 20, "the grid joist under it gone, the pair in its place: {xs:?}");
    assert!(!xs.iter().any(|x| (x - 3608.65).abs() < 1.0), "{xs:?}");
    none_overlap(&r);
    // A wall across the joists needs nothing.
    run(&mut s, "wall", json!({"points": [[FW + 89.0, 1500], [3000, 1500]], "type": "2x4", "height": 2448}));
    assert_eq!(members(&framing(&mut s, id)).len(), ms.len());
}

/// The cabin floor open through a rectangle `x0`..`x1` by `y0`..`y1`.
fn open(s: &mut Session, id: u64, [x0, x1]: [f64; 2], [y0, y1]: [f64; 2]) -> Value {
    run(s, "modify", json!({"id": id, "openings": [[[x0, y0], [x1, y0], [x1, y1], [x0, y1]]]}));
    framing(s, id)
}

#[test]
fn an_opening_in_one_bay_is_trimmed_and_headed_in_two_plies_and_the_joists_it_cuts_are_tails() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let r = open(&mut s, id, [1300.0, 2300.0], [1000.0, 2200.0]);
    assert_eq!(r["warnings"], json!([]), "a rectangle square to the joists, inside the rims");
    let ms = members(&r);
    // Two trimmers each side (right and left of the joists' way, north: east and west), the joists' whole length (cut over
    // the middle beam), the plies side by side.
    for (side, xs) in [("right", [2300.0 + 19.0, 2300.0 + 57.0]), ("left", [1300.0 - 19.0, 1300.0 - 57.0])] {
        for (ply, x) in xs.into_iter().enumerate() {
            let pieces: Vec<&Value> =
                ms.iter().filter(|m| m["key"].as_str().unwrap_or("").starts_with(&format!("opening/0/trimmer/{side}/{ply}/"))).collect();
            assert_eq!(pieces.len(), 2, "{side} {ply}");
            for p in pieces {
                assert_eq!(p["role"], "trimmer");
                let b = world(&r, p).0;
                close((b[0] + b[2]) / 2.0, x, "a trimmer");
            }
        }
    }
    // Two headers at each end, between the trimmers.
    for (end, ys) in [("start", [1000.0 - 19.0, 1000.0 - 57.0]), ("end", [2200.0 + 19.0, 2200.0 + 57.0])] {
        for (ply, y) in ys.into_iter().enumerate() {
            let h = keyed(&ms, &format!("opening/0/header/{end}/{ply}"));
            assert_eq!(h["role"], "header");
            let b = world(&r, h).0;
            close((b[1] + b[3]) / 2.0, y, "a header");
            close(b[0], 1300.0, "from the right trimmer");
            close(b[2], 2300.0, "to the left one");
            close(len(h), 1000.0, "cut across the opening");
        }
    }
    // The joists it cuts are tails: from the rim to the header, from the header to the middle beam, and on.
    for x in [FW + 4.0 * 1220.0 / 3.0, FW + 5.0 * 1220.0 / 3.0] {
        let mut tails: Vec<[f64; 2]> = role(&ms, "joist")
            .into_iter()
            .map(|m| world(&r, m).0)
            .filter(|b| ((b[0] + b[2]) / 2.0 - x).abs() < 0.05)
            .map(|b| [b[1], b[3]])
            .collect();
        tails.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert_eq!(tails.len(), 3, "{x}: {tails:?}");
        close(tails[0][0], FS + 38.0, "a tail from the rim");
        close(tails[0][1], 1000.0 - 76.0, "to the header");
        close(tails[1][0], 2200.0 + 76.0, "a tail from the far header");
        close(tails[1][1], YL, "to the middle beam");
    }
    none_overlap(&r);
    // Moved onto the floor's edge: not trimmed, with a warning naming it.
    let r = open(&mut s, id, [FW + 1.5, FW + 1001.5], [1000.0, 2200.0]);
    let warnings: Vec<&str> = r["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(warnings.iter().any(|w| w.contains("opening 0") && w.contains("isn't trimmed")), "{warnings:?}");
    assert!(!members(&r).iter().any(|m| m["role"] == "trimmer"));
    let done = run(&mut s, "frame", json!({"id": id}));
    assert!(
        done["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.starts_with(&format!("floor {id}: opening 0"))))
    );
}

#[test]
fn a_shaft_through_the_floor_is_trimmed_by_its_guid_and_a_round_opening_by_the_box_round_it_with_a_warning() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let sh = first(&run(
        &mut s,
        "shaft",
        json!({"boundary": [[4500, 3500], [5200, 3500], [5200, 4300], [4500, 4300]], "level": "Level 1", "base_offset": -1000, "height": 4000}),
    ));
    let g = s.model.guid(sh).to_string();
    let r = framing(&mut s, id);
    let ms = members(&r);
    assert!(ms.iter().any(|m| m["key"] == format!("opening/{g}/header/start/0")), "{:?}", r["warnings"]);
    assert_eq!(r["warnings"], json!([]));
    none_overlap(&r);
    // An eight-sided opening: framed by its box, said so.
    let ring: Vec<[f64; 2]> = (0..8)
        .map(|k| (k as f64 * std::f64::consts::FRAC_PI_4).sin_cos())
        .map(|(sn, c)| [1800.0 + 400.0 * c, 1500.0 + 400.0 * sn])
        .collect();
    run(&mut s, "modify", json!({"id": id, "openings": [ring]}));
    let r = framing(&mut s, id);
    let warnings: Vec<&str> = r["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(warnings.iter().any(|w| w.contains("opening 0") && w.contains("box round it")), "{warnings:?}");
    assert!(members(&r).iter().any(|m| m["key"] == "opening/0/header/end/1"));
}

#[test]
fn lapped_joists_run_300_past_the_middle_beam_every_other_piece_a_thickness_over_and_nothing_overlaps() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    stud_wall_type(&mut s);
    run(&mut s, "wall", json!({"points": [[3608.65, FS + 89.0], [3608.65, YL]], "type": "2x4", "height": 2448}));
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin Floor", "lap": 300}));
    let r = framing(&mut s, id);
    assert_eq!(r["warnings"], json!([]));
    let ms = members(&r);
    let grid: Vec<&Value> = role(&ms, "joist").into_iter().filter(|m| m["key"].as_str().unwrap_or("").starts_with("joist/")).collect();
    assert_eq!(grid.len(), 16 * 2, "16 grid lines, each in two pieces");
    for k in [1, 5, 17] {
        let (a, b) = (keyed(&ms, &format!("joist/{k}/0")), keyed(&ms, &format!("joist/{k}/1")));
        let (ba, bb) = (world(&r, a).0, world(&r, b).0);
        close(ba[3], YL + 300.0, "the first piece runs 300 past the beam's centre");
        close(bb[1], YL - 300.0, "the second from 300 before it");
        close((ba[0] - bb[0]).abs(), 38.0, "beside it, a thickness over");
    }
    // The edge joists and the pair under the partition butt on it.
    for m in role(&ms, "joist").into_iter().filter(|m| !m["key"].as_str().unwrap_or("").starts_with("joist/")) {
        close(len(m), 3012.0, "butting");
    }
    none_overlap(&r);
}

#[test]
fn blocking_rows_divide_each_span_evenly_between_neighbouring_joists_and_none_crosses_an_opening() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin Floor", "bridging": 2000}));
    let r = open(&mut s, id, [1300.0, 2300.0], [1000.0, 2200.0]);
    let ms = members(&r);
    let blocks = role(&ms, "blocking");
    // Each span of 3012 between the rim and the middle beam is cut by one row in its middle; 18 bays a row, two rows: less
    // those the opening and its trimmers take.
    let ys: std::collections::BTreeSet<i64> =
        blocks.iter().map(|b| ((world(&r, b).0[1] + world(&r, b).0[3]) / 2.0).round() as i64).collect();
    assert!(ys.len() >= 2, "{ys:?}");
    for b in &blocks {
        let w = world(&r, b).0;
        assert!(!(w[2] > 1300.0 && w[0] < 2300.0 && w[3] > 1000.0 - 76.0 && w[1] < 2200.0 + 76.0), "a block in the opening: {b}");
        close(len(b), w[2] - w[0], "cut between the joists");
    }
    assert!(blocks.len() > 30, "{}", blocks.len());
    none_overlap(&r);
}

#[test]
fn a_floor_with_nothing_under_it_is_framed_with_a_warning_of_its_span() {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "2x8", "layers": [{"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    let id = first(&run(&mut s, "floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "type": "2x8"})));
    let r = run(&mut s, "frame", json!({"id": id}));
    let w = r["warnings"].as_array().unwrap();
    assert!(w.iter().any(|w| w.as_str().is_some_and(|w| w.contains("nothing bears its joists") && w.contains("3924"))), "{w:?}");
    // Square to its longest edge, its joists span the short way.
    let r = framing(&mut s, id);
    close(f(&json!([r["angle"]]), 0), 90.0, "north");
    assert_eq!(r["supports"], json!([]));
}

#[test]
fn a_sloped_floor_a_shaped_one_and_one_with_no_structure_layer_are_refused_framing_saying_why() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let e = s.execute("modify", &json!({"id": id, "slope": 5, "arrow": [[0, 0], [0, 1000]]})).unwrap_err();
    assert!(e.contains("only a flat floor is framed"), "{e}");
    let e = s
        .execute(
            "type",
            &json!({"category": "floor", "name": "2x8 Floor", "layers": [{"function": "finish", "material": "Timber", "thickness": 202}]}),
        )
        .unwrap_err();
    assert!(e.contains("structure layer"), "{e}");
    let plain = first(&run(
        &mut s,
        "floor",
        json!({"boundary": [[0, 9000], [3000, 9000], [3000, 12000], [0, 12000]], "slope": 5, "arrow": [[0, 9000], [0, 10000]]}),
    ));
    let e = s.execute("frame", &json!({"id": plain})).unwrap_err();
    assert!(e.contains("only a flat floor is framed"), "{e}");
    assert!(s.model.elements.get(plain).is_some_and(|e| matches!(e, crate::model::Element::Floor(f) if f.framing.is_none())));
    let generic = first(&run(&mut s, "floor", json!({"boundary": [[0, 20000], [3000, 20000], [3000, 23000], [0, 23000]]})));
    let e = s.execute("frame", &json!({"id": generic})).unwrap_err();
    assert!(e.contains("structure layer") || e.contains("only a flat floor"), "{e}");
    assert!(s.execute("frame", &json!({"id": id, "angle": "north"})).is_err());
    assert!(s.execute("frame", &json!({"id": id, "angle": 1e300})).is_err());
    assert!(s.execute("modify", &json!({"id": id, "framing_angle": 1e300})).is_err());
    let beam = s.model.elements.iter().find(|(_, e)| e.category() == "beam").map(|(id, _)| *id).unwrap();
    assert!(s.execute("frame", &json!({"ids": [id, beam]})).unwrap_err().contains("isn't a wall, a floor or a roof"));
}

#[test]
fn a_framed_floor_undoes_redoes_and_reopens_by_its_framing_types_guid() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let framed = |s: &Session| match s.model.elements.get(id) {
        Some(crate::model::Element::Floor(f)) => (f.framing.clone(), f.framing_angle),
        _ => (None, None),
    };
    assert_eq!(framed(&s), (Some("Cabin Floor".into()), Some(90.0)));
    run(&mut s, "undo", json!({}));
    assert_eq!(framed(&s), (None, None));
    run(&mut s, "redo", json!({}));
    assert_eq!(framed(&s), (Some("Cabin Floor".into()), Some(90.0)));
    let text = s.model.to_json().unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    let guid = s.model.library.guid(&Kind::Type("framing".into()), "Cabin Floor").unwrap();
    let held = file["elements"].as_object().unwrap().values().find(|e| e["guid"] == json!(s.model.guid(id).to_string())).unwrap();
    assert_eq!(held["framing"], json!(guid.to_string()), "a file holds the type's GUID");
    assert_eq!(held["framing_angle"], json!(90.0));
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(crate::framing::floor::floor(&m, id), crate::framing::floor::floor(&s.model, id));
    // Renamed with its type; refused deletion while used; unframed, it goes.
    run(&mut s, "rename_type", json!({"category": "framing", "type": "Cabin Floor", "name": "Floor 16"}));
    assert_eq!(framed(&s).0.as_deref(), Some("Floor 16"));
    assert!(s.execute("delete_type", &json!({"category": "framing", "type": "Floor 16"})).unwrap_err().contains("used 1 time"));
    run(&mut s, "modify", json!({"id": id, "framing": null}));
    assert!(s.execute("framing", &json!({"id": id})).unwrap_err().contains("isn't framed"));
    run(&mut s, "modify", json!({"id": id, "framing": "Floor 16", "framing_angle": 0}));
    run(&mut s, "unframe", json!({"id": id}));
    assert_eq!(framed(&s).0, None);
    run(&mut s, "delete_type", json!({"category": "framing", "type": "Floor 16"}));
}

#[test]
fn a_framed_floor_comes_in_from_another_file_with_its_framing_type() {
    let mut s = Session::new();
    let id = cabin(&mut s);
    let path = std::env::temp_dir().join(format!("bimcraft-floor-framing-insert-{}.bimc", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut to = Session::new();
    let r = run(&mut to, "insert", json!({"path": path.to_str().unwrap(), "ids": [id]}));
    let _ = std::fs::remove_file(&path);
    let here = r["ids"][id.to_string()].as_u64().expect("the floor came");
    let kind = Kind::Type("framing".into());
    assert_eq!(to.model.library.guid(&kind, "Cabin Floor"), s.model.library.guid(&kind, "Cabin Floor"));
    // Without the beams, nothing bears it there.
    let r = framing(&mut to, here);
    assert_eq!(members(&r).len(), 4 + 19, "two rims each spliced in two, and 19 joists");
}

#[test]
fn a_floor_framed_along_its_long_way_and_an_l_shaped_one_cut_by_a_skewed_edge_frame_without_overlaps() {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "2x8", "layers": [{"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    // An L, with its corner cut on the skew.
    let id = first(&run(
        &mut s,
        "floor",
        json!({"boundary": [[0, 0], [6000, 0], [6000, 2500], [5000, 3500], [3000, 3500], [3000, 6000], [0, 6000]], "type": "2x8"}),
    ));
    run(&mut s, "frame", json!({"id": id, "angle": 0}));
    let r = framing(&mut s, id);
    let ms = members(&r);
    none_overlap(&r);
    // The skewed edge has a rim of its own frame; the joists end square short of it.
    let skew = ms.iter().find(|m| m["role"] == "rim" && m.get("frame").is_some()).expect("a rim along the skewed edge");
    close(len(skew), (2.0f64).sqrt() * (1000.0 - 38.0), "the skewed edge, from the east rim's inner face, which runs through");
    let rims = role(&ms, "rim");
    assert_eq!(rims.len(), 4, "east, the skewed one, the inner east and west: {:?}", rims.iter().map(|m| &m["key"]).collect::<Vec<_>>());
    // Every joist lies inside the floor.
    for j in role(&ms, "joist") {
        let b = world(&r, j).0;
        let inside = |x: f64, y: f64| {
            crate::geom::contains(
                &s.model
                    .elements
                    .get(id)
                    .map(|e| match e {
                        crate::model::Element::Floor(f) => f.boundary.clone(),
                        _ => Vec::new(),
                    })
                    .unwrap_or_default(),
                crate::geom::P2(x, y),
            )
        };
        assert!(inside(b[0] + 0.01, b[1] + 0.01) && inside(b[2] - 0.01, b[3] - 0.01), "{j}");
    }
}

/// For the hostile sweep (`tests::hostile_params_never_panic_or_break_the_model`): a framed floor with an opening and a
/// beam under it, made in `base`; and payloads for the framing commands on floors.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let _ = base.execute(
        "type",
        &json!({"category": "floor", "name": "Sweep Joists", "layers": [{"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    let _ = base.execute("type", &json!({"category": "beam", "name": "Sweep Beam", "width": 114, "depth": 235}));
    let _ = base.execute("beam", &json!({"points": [[0, -9000], [5000, -9000]], "type": "Sweep Beam", "offset": -184}));
    let id = base
        .execute(
            "floor",
            &json!({"boundary": [[0, -12000], [5000, -12000], [5000, -7000], [0, -7000]], "type": "Sweep Joists",
                    "openings": [[[1000, -11000], [2000, -11000], [2000, -10000], [1000, -10000]]]}),
        )
        .ok()
        .and_then(|v| v["id"].as_u64())
        .unwrap_or(0);
    let _ = base.execute("frame", &json!({"id": id, "angle": 90}));
    let ty = |v: Value| {
        let mut t = json!({"category": "framing", "name": "2x4 @ 406"});
        t.as_object_mut().into_iter().for_each(|o| o.extend(v.as_object().cloned().unwrap_or_default()));
        ("type", t)
    };
    vec![
        ("frame", json!({"id": id, "angle": 45})),
        ("frame", json!({"id": id, "angle": -1e300})),
        ("frame", json!({"id": id, "angle": f64::MAX})),
        ("frame", json!({"id": id, "angle": [1, 2]})),
        ("frame", json!({"ids": [id, 100002], "angle": 0})),
        ("unframe", json!({"ids": [id]})),
        ("framing", json!({"id": id})),
        ty(json!({"lap": 300, "bridging": 1200, "trimmers": 3})),
        ty(json!({"lap": -1, "bridging": 1})),
        ty(json!({"lap": 1e300, "bridging": f64::MAX, "trimmers": 0})),
        ty(json!({"trimmers": 4, "rim": "yes", "under_partitions": 2})),
        ty(json!({"rim": false, "under_partitions": false, "spacing": 39, "lap": 2000})),
        ty(json!({"bridging": 300, "spacing": 50})),
        ("modify", json!({"id": id, "framing_angle": 1e300})),
        ("modify", json!({"id": id, "framing_angle": 33.3})),
        ("modify", json!({"id": id, "framing_angle": "x"})),
        ("modify", json!({"id": id, "slope": 3, "arrow": [[0, -12000], [0, -11000]]})),
        ("modify", json!({"id": id, "boundary": [[0, -12000], [9.99e8, -12000], [9.99e8, -7000], [0, -7000]]})),
        ("modify", json!({"id": id, "openings": [[[1, -11999], [4999, -11999], [4999, -7001], [1, -7001]]]})),
        ("modify", json!({"id": id, "type": "Generic 150mm"})),
        (
            "type",
            json!({"category": "floor", "name": "Sweep Joists", "layers": [{"function": "finish", "material": "Timber", "thickness": 184}]}),
        ),
    ]
}
