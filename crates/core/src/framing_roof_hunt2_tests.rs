//! A second hunt on roof framing (`framing/roof.rs`): hostile but plausible roofs (steep and uneven gables, a flat roof's
//! courtyard, rakes a hair and a lot off the rafters, roofs too small for a rafter, a point along a straight eave, sheds over
//! walls and beams at every turn), each held to the first hunt's checker and to what it found the checker let through.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::framing_roof_hunt_tests::{check, first, outputs_agree, read, roof_of, run};
use crate::geom::{self, P2};

const ROOF: &str = "Hunt2 Rafters";
const FRAMING: &str = "Hunt2 Framing";

fn session(extra: Value) -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "roof", "name": ROOF, "layers": [
            {"function": "substrate", "material": "Timber", "thickness": 11},
            {"function": "structure", "material": "Timber", "thickness": 235}]}),
    );
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Hunt2 Stud", "layers": [{"function": "structure", "material": "Timber", "thickness": 89}]}),
    );
    run(&mut s, "type", json!({"category": "beam", "name": "Hunt2 Beam", "width": 89, "depth": 302}));
    let mut t = json!({"category": "framing", "name": FRAMING, "spacing": 600});
    if let (Some(o), Some(e)) = (t.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    run(&mut s, "type", t);
    s
}

fn roof(s: &mut Session, params: Value) -> u64 {
    let mut p = json!({"type": ROOF, "level": "Level 1", "offset": 2400});
    if let (Some(o), Some(e)) = (p.as_object_mut(), params.as_object()) {
        o.extend(e.clone());
    }
    let id = first(&run(s, "roof", p));
    run(s, "frame", json!({"id": id, "type": FRAMING}));
    id
}

fn walls_under(s: &mut Session, roof: u64, lines: &[[[f64; 2]; 2]]) -> Vec<u64> {
    let ws: Vec<u64> = lines.iter().map(|l| first(&run(s, "wall", json!({"points": l, "type": "Hunt2 Stud", "height": 1000})))).collect();
    run(s, "modify", json!({"ids": ws, "attach_top": roof}));
    ws
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn pt(p: [f64; 2]) -> P2 {
    P2(p[0], p[1])
}

fn turn(p: [f64; 2], angle: f64) -> [f64; 2] {
    let (c, s) = (angle.to_radians().cos(), angle.to_radians().sin());
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
}

/// The underside of roof `id`'s structure layer over plan point `p`: of the slope over it.
fn under_at(s: &Session, id: u64, p: [f64; 2]) -> f64 {
    let rf = roof_of(s, id);
    let q = P2(p[0], p[1]);
    rf.faces
        .iter()
        .find(|f| f.loops.first().is_some_and(|l| geom::contains(l, q)))
        .and_then(|f| rf.bands.get(&f.index))
        .map_or(f64::NAN, |(pl, _)| pl.at(q))
}

/// A shed 8000 wide falling south over 6000, turned `angle`, at `slope` degrees, framed; walls under it along each `wall_ys`
/// and beams along each `beam_ys`, east to west, tops at the rafters' underside.
fn shed(extra: Value, angle: f64, slope: f64, wall_ys: &[f64], beam_ys: &[f64], cw: bool) -> (Session, u64, Vec<u64>) {
    let mut s = session(extra);
    let mut ring: Vec<[f64; 2]> = rect(0.0, 0.0, 8000.0, 6000.0).into_iter().map(|p| turn(p, angle)).collect();
    let mut gables = vec![1, 2, 3];
    if cw {
        ring.reverse();
        gables = vec![0, 1, 3];
    }
    let id = roof(&mut s, json!({"boundary": ring, "slope": slope, "gables": gables}));
    let lines: Vec<[[f64; 2]; 2]> = wall_ys.iter().map(|y| [turn([-300.0, *y], angle), turn([8300.0, *y], angle)]).collect();
    let ws = walls_under(&mut s, id, &lines);
    if !ws.is_empty() {
        run(&mut s, "frame", json!({"ids": ws}));
    }
    for y in beam_ys {
        let (a, b) = (turn([-300.0, *y], angle), turn([8300.0, *y], angle));
        let z = under_at(&s, id, turn([4000.0, *y], angle)) - s.model.elevation(s.model.levels()[0].0);
        run(&mut s, "beam", json!({"points": [a, b], "type": "Hunt2 Beam", "offset": z}));
    }
    (s, id, ws)
}

/// The underside and the depth plumb of the structure layer of the slope of roof `id` over `p`.
fn band(s: &Session, id: u64, p: P2) -> (crate::walls::Plane, f64) {
    let rf = roof_of(s, id);
    rf.faces
        .iter()
        .find(|f| f.loops.first().is_some_and(|l| geom::contains(l, p)))
        .and_then(|f| rf.bands.get(&f.index).copied())
        .unwrap_or_else(|| panic!("no slope over {p:?}"))
}

#[test]
fn a_ridge_board_lies_in_the_structure_layer_at_any_slope_and_where_the_slopes_differ() {
    // It was a level box from the underside on the line up the rafters' plumb depth: its top edges then stood half its
    // thickness times the slope over the structure layer's top, through the sheathing and out of the roof from 45° up, and
    // where the slopes differ the deeper one's depth stood over the shallower slope by far more.
    for (slope, slopes) in [(30.0f64, json!([])), (45.0, json!([])), (60.0, json!([])), (75.0, json!([])), (30.0, json!([45, 0, 20, 0]))] {
        // Stock long enough that the 8000 board is one piece.
        let mut s = session(json!({"stock": [9800]}));
        let id = roof(&mut s, json!({"boundary": rect(0.0, 0.0, 8000.0, 5000.0), "slope": slope, "slopes": slopes, "gables": [1, 3]}));
        // Gable walls under its rakes, up to the ridge: the board over their top plates.
        let ws = walls_under(&mut s, id, &[[[500.0, 300.0], [500.0, 4700.0]], [[7500.0, 300.0], [7500.0, 4700.0]]]);
        run(&mut s, "frame", json!({"ids": ws}));
        let r = check(&mut s, id, &ws);
        let ridge = r.role("ridge");
        assert_eq!(ridge.len(), 1, "{slope} {slopes}: {:?}", r.warnings);
        let [b, top] = [ridge[0].z[0], ridge[0].z[2]];
        // Over the line, the layer's underside; at its faces, the lower of the layer's tops.
        let line = ridge[0].middle();
        let (under, _) = band(&s, id, P2(line.0, 1000.0));
        assert!((b - under.at(line)).abs() < 0.02, "{slope} {slopes}: its bottom {b} isn't the underside on the line, {}", under.at(line));
        let tops: Vec<f64> = ridge[0]
            .quad(0.01)
            .iter()
            .map(|p| {
                let (under, dz) = band(&s, id, *p);
                under.at(*p) + dz
            })
            .collect();
        let lowest = tops.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(top <= lowest + 0.02, "{slope} {slopes}: its top {top} over the layer's {lowest}");
        assert!(top >= lowest - 0.03 * (1.0 + slope.to_radians().tan()), "{slope} {slopes}: its top {top} short of the layer's {lowest}");
        // Each rafter's plumb cut against it flush with its top where the slopes are alike.
        let grown = ridge[0].quad(-1.0);
        for m in r.members.iter().filter(|m| m.role == "rafter" || m.key.contains("/edge/")) {
            if slopes == json!([]) && geom::contains(&grown, m.at(m.s[1], (m.across[0] + m.across[1]) / 2.0)) {
                assert!((m.z[3] - top).abs() < 0.02, "{slope}: {} {:?} isn't flush with the ridge board's top {top}", m.key, m.z);
            }
        }
    }
}

#[test]
fn a_flat_roofs_courtyard_is_framed_round_as_an_opening_is() {
    // Its eaves round the courtyard were left to the rafters' ends: nothing along its two sides along the rafters, nothing
    // across the rafters' ends, and without rims not a word of it.
    for extra in [json!({"rim": false}), json!({"rim": true}), json!({"rim": false, "trimmers": 1})] {
        let mut s = session(extra.clone());
        let court = rect(3000.0, 3000.0, 6000.0, 6000.0);
        let id = roof(&mut s, json!({"boundary": rect(0.0, 0.0, 9000.0, 9000.0), "slope": 0, "courtyards": [court]}));
        let r = check(&mut s, id, &[]);
        assert!(!r.warned("round a courtyard") && !r.warned("courtyard 0 isn't"), "{extra}: {:?}", r.warnings);
        assert!(r.members.iter().any(|m| m.key.starts_with("face/0/courtyard/0/trimmer/")), "{extra}");
        assert!(r.members.iter().any(|m| m.key.starts_with("face/0/courtyard/0/header/")), "{extra}");
        // Something just outside the middle of each of its edges, all round.
        for p in [P2(4500.0, 2990.0), P2(6010.0, 4500.0), P2(4500.0, 6010.0), P2(2990.0, 4500.0)] {
            assert!(r.members.iter().any(|m| geom::contains(&m.quad(0.0), p)), "{extra}: nothing along the courtyard at {p:?}");
        }
        outputs_agree(&mut s, id);
    }
    // One too near the roof's edge to trim says so.
    let mut s = session(json!({"rim": false}));
    let id =
        roof(&mut s, json!({"boundary": rect(0.0, 0.0, 9000.0, 9000.0), "slope": 0, "courtyards": [rect(40.0, 3000.0, 6000.0, 6000.0)]}));
    let r = check(&mut s, id, &[]);
    assert!(r.warned("courtyard 0 isn't trimmed"), "{:?}", r.warnings);
}

#[test]
fn a_rake_a_hair_off_the_rafters_keeps_its_edge_rafter_and_a_skewed_one_says_what_it_lacks() {
    // A rake 2 mm off the rafters' way over 5 m was skewed: no edge rafter along it, and without rims not a word.
    for rim in [false, true] {
        for (angle, cw) in [(0.0, false), (37.0, true)] {
            let mut s = session(json!({"rim": rim}));
            let mut ring: Vec<[f64; 2]> = [[0.0, 0.0], [8000.0, 0.0], [8002.0, 5000.0], [0.0, 5000.0]].map(|p| turn(p, angle)).to_vec();
            let mut gables = vec![1, 2, 3];
            if cw {
                ring.reverse();
                gables = vec![0, 1, 3];
            }
            let id = roof(&mut s, json!({"boundary": ring, "slope": 10, "gables": gables}));
            let r = check(&mut s, id, &[]);
            let what = format!("rim {rim}, {angle}°");
            let east = r
                .members
                .iter()
                .filter(|m| m.key.contains("/edge/"))
                .find(|m| (m.middle() - pt(turn([8000.0, 2500.0], angle))).length() < 60.0);
            let east = east.unwrap_or_else(|| panic!("{what}: no edge rafter along the east rake; {:?}", r.warnings));
            assert!(east.s[1] - east.s[0] > 4900.0 - if rim { 76.0 } else { 0.0 }, "{what}: {:?}", east.s);
            assert!(r.warned("runs 0.02° off the rafters"), "{what}: {:?}", r.warnings);
            assert!(!r.warned("runs skewed"), "{what}: {:?}", r.warnings);
        }
    }
    // A trapezoid's rakes, well off the rafters: said, rims or none.
    for rim in [false, true] {
        let mut s = session(json!({"rim": rim}));
        let id = roof(&mut s, json!({"boundary": [[0, 0], [8000, 0], [7800, 5000], [200, 5000]], "slope": 10, "gables": [1, 2, 3]}));
        let r = check(&mut s, id, &[]);
        for e in [1, 3] {
            assert!(r.warned(&format!("its edge {e} runs skewed to the rafters: no rim or edge rafter")), "rim {rim}: {:?}", r.warnings);
        }
    }
}

#[test]
fn a_roof_or_a_floor_too_small_for_a_rafter_or_a_joist_says_so() {
    // A gable 30 mm square framed nothing, and said only that its rims and its ridge board were too short.
    let mut s = session(json!({}));
    let id = roof(&mut s, json!({"boundary": rect(0.0, 0.0, 30.0, 30.0), "slope": 30, "gables": [1, 3]}));
    let r = check(&mut s, id, &[]);
    assert!(r.members.is_empty());
    for f in [0, 1] {
        assert!(r.warned(&format!("slope {f}: no rafter fits on the slope")), "{:?}", r.warnings);
    }
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "Hunt2 Floor", "layers": [{"function": "structure", "material": "Timber", "thickness": 184}]}),
    );
    run(&mut s, "type", json!({"category": "framing", "name": "Hunt2 No Rims", "rim": false}));
    let f = first(&run(&mut s, "floor", json!({"boundary": rect(0.0, 0.0, 30.0, 30.0), "type": "Hunt2 Floor", "level": "Level 1"})));
    run(&mut s, "frame", json!({"id": f, "type": "Hunt2 No Rims", "angle": 90}));
    let r = read(&mut s, f);
    assert!(r.members.is_empty(), "{:?}", r.members);
    assert!(r.warned("no joist fits on the floor"), "{:?}", r.warnings);
}

#[test]
fn a_point_along_a_straight_eave_leaves_no_seam_one_ridge_board_and_an_even_grid() {
    // The slopes either side of a point along a straight eave lie in one plane: they were framed apart, with a warning of a
    // seam between them to put a beam along, two ridge boards and each slope's rafters counted from its own side.
    let eave = [[0.0, 0.0], [4000.0, 0.0], [8000.0, 0.0]];
    for (what, gables, expect_ridge) in [("gable", vec![2, 4], Some(8000.0)), ("hip", vec![], Some(3000.0)), ("shed", vec![2, 3, 4], None)]
    {
        for angle in [0.0, 37.0] {
            let mut s = session(json!({"layout": "centre"}));
            let far = if what == "shed" { 6000.0 } else { 5000.0 };
            let ring: Vec<[f64; 2]> = eave.iter().copied().chain([[8000.0, far], [0.0, far]]).map(|p| turn(p, angle)).collect();
            let id = roof(&mut s, json!({"boundary": ring, "slope": 30, "gables": gables}));
            let r = check(&mut s, id, &[]);
            let ctx = format!("{what} at {angle}°: {:?}", r.warnings);
            assert!(!r.warned("seam"), "{ctx}");
            assert!(!r.warned("ends inside the footprint"), "{ctx}");
            let ridges = r.role("ridge");
            match expect_ridge {
                Some(len) => {
                    // One board, spliced where it is longer than the stock (`face/<i>/ridge/<edge>/<n>`).
                    let boards: std::collections::BTreeSet<String> =
                        ridges.iter().map(|m| m.key.split('/').take(4).collect::<Vec<_>>().join("/")).collect();
                    assert_eq!(boards.len(), 1, "{ctx}");
                    let whole: f64 = ridges.iter().map(|m| m.length).sum();
                    assert!((whole - len).abs() < 0.02, "{ctx}: {whole}");
                }
                None => assert!(ridges.is_empty(), "{ctx}"),
            }
            let hips = r.warnings.iter().filter(|w| w.contains("hip from")).count();
            assert_eq!(hips, if what == "hip" { 4 } else { 0 }, "{ctx}");
            // The south slope's rafters and edge rafters one grid across the point: none farther apart than the spacing.
            let south = |m: &&crate::framing_roof_hunt_tests::M| {
                let c = m.middle();
                let local = turn([c.0, c.1], -angle);
                m.grain == "along" && local[1] < 1000.0 && local[0] > 2600.0 && local[0] < 5400.0
            };
            let mut xs: Vec<f64> = r.members.iter().filter(south).map(|m| turn([m.middle().0, m.middle().1], -angle)[0]).collect();
            xs.sort_by(f64::total_cmp);
            xs.dedup_by(|a, b| (*a - *b).abs() < 1.0);
            assert!(xs.windows(2).all(|w| w[1] - w[0] <= 600.0 + 0.5), "{ctx}: {xs:?}");
            outputs_agree(&mut s, id);
        }
    }
}

#[test]
fn sheds_over_walls_and_beams_at_every_turn_hold_with_short_stock_close_supports_and_one_support() {
    let cases: Vec<(Value, f64, Vec<f64>, Vec<f64>)> = vec![
        (json!({"lap": 230, "rim": false}), 2.8624, vec![300.0, 5700.0], vec![3000.0]),
        (json!({"lap": 230, "stock": [2440]}), 20.0, vec![300.0, 5700.0], vec![3000.0]),
        (
            json!({"lap": 230, "stock": [2440, 3050]}),
            30.0,
            vec![300.0],
            vec![700.0, 1000.0, 1300.0, 2200.0, 2500.0, 4100.0, 4300.0, 5600.0],
        ),
        (json!({"lap": 230}), 30.0, vec![300.0], vec![]),
        (json!({"lap": 0, "rim": true, "stock": [3050]}), 45.0, vec![300.0, 5700.0], vec![2000.0, 4000.0]),
        (json!({"lap": 300, "bridging": 800}), 75.0, vec![300.0, 5700.0], vec![3000.0]),
    ];
    for (extra, slope, wy, by) in cases {
        for (angle, cw) in [(0.0, false), (90.0, true), (37.0, false)] {
            eprintln!("case: {extra} at {slope}°, turned {angle}, clockwise {cw}");
            let (mut s, id, ws) = shed(extra.clone(), angle, slope, &wy, &by, cw);
            let r = check(&mut s, id, &ws);
            assert!(!r.role("rafter").is_empty());
            assert!(r.raw["supports"].as_array().is_some_and(|a| a.len() == wy.len() + by.len()), "{extra}: {}", r.raw["supports"]);
        }
    }
}
