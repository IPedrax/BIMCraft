//! Roof framing (`framing/roof.rs`) driven through commands: the cabin roof the user framed by hand is the acceptance test.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::framing_roof_hunt_tests::{M, check, first, outputs_agree, read, run};
use crate::library::Kind;

/// The cabin roof, as the user's reference has it: a shed sloping 5 %, its low eave north at `YN`, 9760 along the slope up to
/// its high eave south at `ys()`, from `X0` to `X1` (610 past the walls' faces each side).
pub(crate) const X0: f64 = -661.35;
pub(crate) const X1: f64 = 7878.65;
pub(crate) const YN: f64 = 6658.65;
/// The centre lines of the south and north walls, of the LVL and of the deck beam.
pub(crate) const YSW: f64 = -6.85;
pub(crate) const YNW: f64 = 6004.15;
pub(crate) const YL: f64 = 2998.65;
pub(crate) const YD: f64 = -2421.35;

/// The slope's run over its length.
fn cos() -> f64 {
    1.0 / (1.0f64 + 0.05 * 0.05).sqrt()
}

pub(crate) fn ys() -> f64 {
    YN - 9760.0 * cos()
}

/// The rafters' underside over plan y: 2448 at the north wall's outer face (y 6048.65), rising 5 % south.
pub(crate) fn under(y: f64) -> f64 {
    2448.0 + (6048.65 - y) * 0.05
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 0.02, "{what}: {a} isn't {b}");
}

/// The cabin: its roof (11 mm of sheathing over a 235 mm structure layer, framed a third of 1220 apart, 38 thick, without
/// rims, lapped 230) over framed 2x4 walls whose tops are attached to it, the LVL along `YL` and the deck beam along `YD` with
/// their tops at the rafters' underside. The south and north walls, the LVL and the deck beam run under the whole roof, out
/// under its rakes, so that every line of rafters crosses them; the east and west walls butt into them. The roof, and the
/// walls (south, north, east, west).
pub(crate) fn cabin(s: &mut Session, extra: Value) -> (u64, Vec<u64>) {
    cabin_from(s, extra, [X0, X1])
}

/// The cabin with its south and north walls, its LVL and its deck beam from `x0` to `x1`.
pub(crate) fn cabin_from(s: &mut Session, extra: Value, [x0, x1]: [f64; 2]) -> (u64, Vec<u64>) {
    run(
        s,
        "type",
        json!({"category": "roof", "name": "2x10 Roof", "layers": [
            {"function": "substrate", "material": "Timber", "thickness": 11},
            {"function": "structure", "material": "Timber", "thickness": 235}]}),
    );
    run(
        s,
        "type",
        json!({"category": "wall", "name": "2x4", "layers": [{"function": "structure", "material": "Timber", "thickness": 89}]}),
    );
    let mut t = json!({"category": "framing", "name": "Cabin Roof", "spacing": 1220.0 / 3.0, "thickness": 38, "rim": false, "lap": 230});
    if let (Some(o), Some(e)) = (t.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    run(s, "type", t);
    run(s, "type", json!({"category": "framing", "name": "Cabin Walls", "spacing": 1220.0 / 3.0}));
    run(s, "type", json!({"category": "beam", "name": "LVL", "width": 89, "depth": 302}));
    run(s, "type", json!({"category": "beam", "name": "2-2x10", "width": 76, "depth": 235}));
    // A shed: its north edge the eave, the other three upright; its underside at the low eave `under(YN)`.
    let roof = first(&run(
        s,
        "roof",
        json!({"boundary": [[X1, YN], [X0, YN], [X0, ys()], [X1, ys()]], "level": "Level 1", "offset": under(YN),
               "slope": 0.05f64.atan().to_degrees(), "gables": [1, 2, 3], "overhang": 0, "type": "2x10 Roof"}),
    ));
    let lines = [
        [[x0, YSW], [x1, YSW]],
        [[x0, YNW], [x1, YNW]],
        [[7224.15, YSW + 44.5], [7224.15, YNW - 44.5]],
        [[-6.85, YSW + 44.5], [-6.85, YNW - 44.5]],
    ];
    let walls: Vec<u64> = lines.iter().map(|l| first(&run(s, "wall", json!({"points": l, "type": "2x4", "height": 2000})))).collect();
    run(s, "modify", json!({"ids": walls, "attach_top": roof}));
    run(s, "frame", json!({"ids": walls, "type": "Cabin Walls"}));
    for (y, ty) in [(YL, "LVL"), (YD, "2-2x10")] {
        run(s, "beam", json!({"points": [[x0, y], [x1, y]], "type": ty, "offset": under(y)}));
    }
    run(s, "frame", json!({"id": roof, "type": "Cabin Roof"}));
    (roof, walls)
}

/// The members of a line by its key less the piece, in order up the slope.
fn lines(ms: &[M]) -> Vec<(String, Vec<&M>)> {
    let mut out: std::collections::BTreeMap<String, Vec<&M>> = std::collections::BTreeMap::new();
    for m in ms.iter().filter(|m| m.role == "rafter") {
        let line = m.key.rsplit_once('/').map_or(m.key.clone(), |(l, _)| l.to_string());
        out.entry(line).or_default().push(m);
    }
    let mut v: Vec<(String, Vec<&M>)> = out.into_iter().collect();
    for (_, ps) in &mut v {
        ps.sort_by(|a, b| a.s[0].total_cmp(&b.s[0]));
    }
    v.sort_by(|a, b| a.1[0].middle().0.total_cmp(&b.1[0].middle().0));
    v
}

/// A member's ends in plan y and its centre in plan x.
fn ys_of(m: &M) -> [f64; 2] {
    let [a, b] = [m.at(m.s[0], m.across[0]).1, m.at(m.s[1], m.across[0]).1];
    [a.min(b), a.max(b)]
}

#[test]
fn the_cabin_roof_has_22_lines_of_rafters_each_lapped_over_the_south_wall_and_the_lvl() {
    let mut s = Session::new();
    let (roof, walls) = cabin(&mut s, json!({}));
    let r = check(&mut s, roof, &walls);
    assert_eq!(r.warnings, Vec::<String>::new());
    let lines = lines(&r.members);
    assert_eq!(lines.len(), 22, "{:?}", lines.iter().map(|l| &l.0).collect::<Vec<_>>());
    // Edge rafters centred 19 in from each rake, the grid a third of 1220 apart between.
    let xs: Vec<f64> = lines.iter().map(|(_, ps)| ps[0].middle().0).collect();
    close(xs[0], X0 + 19.0, "the west edge rafter");
    close(xs[21], X1 - 19.0, "the east edge rafter");
    for (k, x) in xs.iter().enumerate().take(21).skip(1) {
        close(*x, X0 + k as f64 * 1220.0 / 3.0, "a grid rafter");
    }
    let sec = 1.0 / cos();
    for (line, ps) in &lines {
        // Up the slope from the low eave: to 230 past the LVL; from 230 before it to 230 past the south wall, a thickness
        // over; from 230 before that to the high eave. The north wall and the deck beam split nothing.
        assert_eq!(ps.len(), 3, "{line}: three pieces");
        let want = [[YL - 230.0, YN], [YSW - 230.0, YL + 230.0], [ys(), YSW + 230.0]];
        for (p, [y0, y1]) in ps.iter().zip(want) {
            let [a, b] = ys_of(p);
            close(a, y0, &format!("{} starts", p.key));
            close(b, y1, &format!("{} ends", p.key));
            close(p.length, (y1 - y0) * sec, &format!("{} is as long as along its slope", p.key));
            assert!(p.length <= 6100.0 && p.stock.is_some_and(|st| st >= p.length), "{} from stock: {:?}", p.key, p.stock);
            assert_eq!(p.section, [38.0, 235.0]);
        }
        // The middle piece one thickness over, beside the two it laps.
        close(ps[0].across[0], ps[2].across[0], &format!("{line}: the outer pieces in line"));
        close((ps[1].across[0] - ps[0].across[0]).abs(), 38.0, &format!("{line}: the middle piece beside them"));
    }
    close(lines[11].1[0].length, 3890.0 * sec, "the low piece, the longest, about 3890 along the slope");
    let longest = r.role("rafter").iter().map(|m| m.length).fold(0.0, f64::max);
    close(longest, 3890.0 * sec, "no piece longer");
    // Borne by the south and north walls, the LVL and the deck beam; the span in plan between the south wall and the LVL.
    let supports = r.raw["supports"].as_array().map_or(0, Vec::len);
    assert_eq!(supports, 4, "{}", r.raw["supports"]);
    // The cut list groups them, each row from stock.
    let cuts = r.raw["cut_list"].as_array().cloned().unwrap_or_default();
    let rafters: Vec<&Value> = cuts.iter().filter(|c| c["role"] == "rafter").collect();
    assert_eq!(rafters.iter().map(|c| c["count"].as_u64().unwrap_or(0)).sum::<u64>(), 66);
    assert!(rafters.len() <= 6, "{rafters:?}");
    assert!(rafters.iter().all(|c| c["section"] == json!([38.0, 235.0]) && c["stock"].is_number()));
    outputs_agree(&mut s, roof);
}

#[test]
fn without_a_lap_the_cabins_rafters_butt_on_the_supports_and_long_stock_makes_each_line_one_piece() {
    let mut s = Session::new();
    let (roof, walls) = cabin(&mut s, json!({"lap": 0}));
    let r = check(&mut s, roof, &walls);
    // Butting, the fewest pieces are two, and the one split in two that 6100 holds is over the LVL: 3664.6 along the slope
    // from the low eave, 6095.4 on to the high one.
    for (line, ps) in lines(&r.members) {
        let ends: Vec<[f64; 2]> = ps.iter().map(|p| ys_of(p)).collect();
        assert_eq!(ends.len(), 2, "{line}");
        for (e, [y0, y1]) in ends.iter().zip([[YL, YN], [ys(), YL]]) {
            close(e[0], y0, &line);
            close(e[1], y1, &line);
        }
        close(ps[1].across[0], ps[0].across[0], "butted pieces in line");
        assert!(ps.iter().all(|p| p.length <= 6100.0));
    }
    let mut s = Session::new();
    let (roof, walls) = cabin(&mut s, json!({"stock": [2440, 3050, 3660, 4270, 4880, 6100, 9800]}));
    let r = check(&mut s, roof, &walls);
    let lines = lines(&r.members);
    assert_eq!(lines.len(), 22);
    for (line, ps) in &lines {
        assert_eq!(ps.len(), 1, "{line}");
        close(ps[0].length, 9760.0, &format!("{line}: eave to eave along the slope"));
        assert_eq!(ps[0].stock, Some(9800.0));
    }
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}

#[test]
fn where_no_split_keeps_every_piece_within_the_stock_the_cabins_lines_split_where_their_longest_piece_is_shortest() {
    // Stock no longer than 3050: the shortest the longest piece can be is between the walls and the LVL, 3465.5 in plan with
    // its laps (3470 along the slope), so each line is split over the north wall, the LVL and the south wall, the fewest
    // pieces that keep to that; the deck beam splits nothing, and each of the three pieces longer than the stock is warned of.
    let mut s = Session::new();
    let (roof, walls) = cabin(&mut s, json!({"stock": [2440, 3050]}));
    let r = check(&mut s, roof, &walls);
    let sec = 1.0 / cos();
    let lines = lines(&r.members);
    assert_eq!(lines.len(), 22);
    for (line, ps) in &lines {
        let want = [[YNW - 230.0, YN], [YL - 230.0, YNW + 230.0], [YSW - 230.0, YL + 230.0], [ys(), YSW + 230.0]];
        assert_eq!(ps.len(), 4, "{line}");
        for (p, [y0, y1]) in ps.iter().zip(want) {
            let [a, b] = ys_of(p);
            close(a, y0, &format!("{} starts", p.key));
            close(b, y1, &format!("{} ends", p.key));
        }
        close(ps[1].length, 3465.5 * sec, &format!("{line}: the longest piece"));
        let warned = |len: &str| r.warnings.iter().filter(|w| w.contains(&format!("{line} has a piece {len} mm along its slope"))).count();
        assert_eq!((warned("3470"), warned("3316")), (2, 1), "{line}: {:?}", r.warnings);
        assert!(r.warned("longest stock length, 3050 mm"), "{:?}", r.warnings);
    }
}

#[test]
fn a_line_is_split_into_the_fewest_pieces_the_stock_holds_its_longest_as_short_as_can_be_ties_nearest_the_eave() {
    let split = |over: &[f64], iv: [f64; 2], longest: f64, reach: f64| {
        let mut long = Vec::new();
        let at = crate::framing::spliced(over, iv, longest, 1.0, reach, |l| long.push(l.round()));
        (at, long)
    };
    // Held whole: no split, whatever bears it.
    assert_eq!(split(&[1000.0, 2000.0], [0.0, 6000.0], 6100.0, 0.0), (vec![], vec![]));
    // Two pieces, over the support that makes the longer of them shortest, not the farthest one in reach.
    assert_eq!(split(&[3000.0, 4500.0, 5500.0], [0.0, 9000.0], 6100.0, 0.0), (vec![4500.0], vec![]));
    // Laps count: 200 past 4800 is 5000, past 4000 makes the second piece 6200, too long.
    assert_eq!(split(&[4000.0, 4800.0], [0.0, 10000.0], 6100.0, 200.0), (vec![4800.0], vec![]));
    // Two splits as good: the one nearest the eave.
    assert_eq!(split(&[4000.0, 6000.0], [0.0, 10000.0], 6100.0, 0.0), (vec![4000.0], vec![]));
    // Fewer pieces before a shorter longest one: two pieces of 6000 and 4000, not three of 4000.
    assert_eq!(split(&[4000.0, 6000.0, 8000.0], [0.0, 12000.0], 6100.0, 0.0), (vec![6000.0], vec![]));
    // None keeps to the stock: the split whose longest piece is shortest, with the fewest pieces, each too long one told.
    assert_eq!(split(&[3000.0, 6000.0, 8000.0], [0.0, 9000.0], 2000.0, 0.0), (vec![3000.0, 6000.0], vec![3000.0, 3000.0, 3000.0]));
    assert_eq!(split(&[], [0.0, 9000.0], 6100.0, 0.0), (vec![], vec![9000.0]));
}

#[test]
fn the_cabins_walls_frame_with_bevelled_studs_and_its_rafters_sit_on_their_top_plates() {
    let mut s = Session::new();
    let (roof, walls) = cabin(&mut s, json!({}));
    // `check` holds every rafter clear of every member of the walls; here, the rafters over the south wall touch its top plate.
    let r = check(&mut s, roof, &walls);
    let south = read(&mut s, walls[0]);
    let plate = south.members.iter().filter(|m| m.role == "top_plate").max_by(|a, b| a.z[2].total_cmp(&b.z[2])).expect("a top plate");
    let rafter = r.role("rafter").into_iter().find(|m| ys_of(m)[1] > YSW + 100.0 && ys_of(m)[0] < YSW - 100.0).expect("one over it");
    let p = crate::geom::P2(rafter.middle().0, YSW + 44.5 - 0.1);
    close(rafter.heights(p)[0], plate.heights(p)[1], "the rafter on the plate at its low (north) face");
    // An east wall's studs are bevelled, their tops along the slope.
    let east = read(&mut s, walls[2]);
    let studs: Vec<&M> = east.members.iter().filter(|m| m.role == "stud").collect();
    assert!(studs.len() > 10);
    for st in studs {
        close((st.z[3] - st.z[2]).abs(), 38.0 * 0.05, &format!("{} bevelled", st.key));
    }
}

#[test]
fn where_the_walls_and_beams_stop_at_the_walls_faces_the_lines_over_the_rakes_are_one_piece_each_and_warned_of() {
    // As the reference has it: the south and north walls, the LVL and the deck beam from the west wall's face to the east's,
    // so the two lines over each rake's 610 mm overhang cross none of them.
    let mut s = Session::new();
    let (roof, walls) = cabin_from(&mut s, json!({}), [-51.35, 7268.65]);
    let r = check(&mut s, roof, &walls);
    let lines = lines(&r.members);
    assert_eq!(lines.len(), 22);
    let (alone, lapped): (Vec<_>, Vec<_>) = lines.iter().partition(|(_, ps)| ps.len() == 1);
    assert_eq!(alone.len(), 4, "{:?}", alone.iter().map(|l| &l.0).collect::<Vec<_>>());
    assert!(lapped.iter().all(|(_, ps)| ps.len() == 3));
    for (line, ps) in &alone {
        close(ps[0].length, 9760.0, line);
        assert_eq!(ps[0].stock, None);
        assert!(r.warned(&format!("{line} has a piece 9760 mm along its slope")), "{line}: {:?}", r.warnings);
    }
}

#[test]
fn a_gable_roof_meets_its_ridge_board_a_hipped_one_warns_of_each_hip_and_an_extruded_one_is_refused() {
    let mut s = Session::new();
    let (_, _) = cabin(&mut s, json!({}));
    let gable = first(&run(
        &mut s,
        "roof",
        json!({"boundary": [[20000, 0], [28000, 0], [28000, 5000], [20000, 5000]], "level": "Level 1", "offset": 2400, "slope": 30,
               "gables": [1, 3], "type": "2x10 Roof"}),
    ));
    let corners = [[20500.0, 500.0], [27500.0, 500.0], [27500.0, 4500.0], [20500.0, 4500.0]];
    let walls: Vec<u64> = (0..4)
        .map(|k| first(&run(&mut s, "wall", json!({"points": [corners[k], corners[(k + 1) % 4]], "type": "2x4", "height": 2000}))))
        .collect();
    run(&mut s, "modify", json!({"ids": walls, "attach_top": gable}));
    run(&mut s, "frame", json!({"ids": walls, "type": "Cabin Walls"}));
    let reply = run(&mut s, "frame", json!({"id": gable, "type": "Cabin Roof"}));
    assert_eq!(reply["warnings"], json!([]), "{reply}");
    let r = check(&mut s, gable, &walls);
    // 8000 long, more than the longest stock (6100): spliced in two over a rafter, each piece from stock.
    let ridge = r.role("ridge");
    assert_eq!(ridge.len(), 2, "{:?}", ridge.iter().map(|m| &m.key).collect::<Vec<_>>());
    close(ridge.iter().map(|m| m.length).sum(), 8000.0, "the ridge board end to end");
    let rafters: Vec<f64> = r.role("rafter").iter().map(|m| m.middle().0).collect();
    let mut xs: Vec<[f64; 2]> = ridge.iter().map(|m| [m.at(m.s[0], m.across[0]).0, m.at(m.s[0], m.across[1]).0]).collect();
    xs.iter_mut().for_each(|x| x.sort_by(f64::total_cmp));
    xs.sort_by(|a, b| a[0].total_cmp(&b[0]));
    close(xs[0][1], xs[1][0], "the pieces butt");
    assert!(rafters.iter().any(|x| (x - xs[0][1]).abs() < 0.02), "the splice at {} over a rafter: {rafters:?}", xs[0][1]);
    for m in &ridge {
        assert!(m.length <= 6100.0 && m.stock.is_some_and(|st| st >= m.length), "{} from stock: {:?}", m.key, m.stock);
    }
    assert_eq!(ridge[0].section[0], 38.0);
    // As deep as the rafters' plumb cut less half its thickness times the slope: its top flush with theirs at its faces, its
    // bottom on their underside along the ridge, so it stays in the layer.
    close(ridge[0].section[1], 235.0 / 30f64.to_radians().cos() - 19.0 * 30f64.to_radians().tan(), "in the rafters' plumb cut");
    close(ridge[0].z[0], 2400.0 + 2500.0 * 30f64.to_radians().tan(), "on their underside along the ridge");
    // Both slopes' rafters stop at its faces: every top end 19 from the ridge line.
    for m in r.role("rafter").into_iter().filter(|m| m.key.contains("/rafter/")) {
        let [y0, y1] = ys_of(m);
        assert!((y1 - (2500.0 - 19.0)).abs() < 0.02 || (y0 - (2500.0 + 19.0)).abs() < 0.02 || y1 < 2400.0 || y0 > 2600.0, "{}", m.key);
    }
    // A hipped roof: jacks to the hips, and a warning for each of its four.
    let hip = first(&run(
        &mut s,
        "roof",
        json!({"boundary": [[40000, 0], [48000, 0], [48000, 5000], [40000, 5000]], "level": "Level 1", "offset": 2400, "slope": 30,
               "type": "2x10 Roof"}),
    ));
    let reply = run(&mut s, "frame", json!({"id": hip, "type": "Cabin Roof"}));
    let warnings: Vec<&str> = reply["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    assert_eq!(warnings.iter().filter(|w| w.contains("the hip from")).count(), 4, "{warnings:?}");
    check(&mut s, hip, &[]);
    // A roof by extrusion is refused, saying why.
    let x = first(&run(
        &mut s,
        "roof",
        json!({"extrusion": {"line": [[60000, 0], [68000, 0]], "profile": [[0, 0], [4000, 2000], [8000, 0]], "to": 5000},
               "level": "Level 1", "offset": 2400, "type": "2x10 Roof"}),
    ));
    let e = s.execute("frame", &json!({"id": x, "type": "Cabin Roof"})).unwrap_err();
    assert!(e.contains("only a roof by footprint is framed"), "{e}");
}

#[test]
fn the_cabins_eave_boards_longer_than_the_stock_are_spliced_over_rafters_each_piece_from_stock() {
    let mut s = Session::new();
    let (roof, walls) = cabin(&mut s, json!({"rim": true}));
    let r = check(&mut s, roof, &walls);
    assert!(!r.warned("longer than any stock"), "{:?}", r.warnings);
    // The low eave's board and the high edge's, each 8540 along the roof's width: each in two pieces butting over a rafter.
    let rims = r.role("rim");
    assert_eq!(rims.len(), 4, "{:?}", rims.iter().map(|m| &m.key).collect::<Vec<_>>());
    close(rims.iter().map(|m| m.length).sum(), 2.0 * (X1 - X0), "the two boards end to end");
    let rafters: Vec<f64> = r.role("rafter").iter().map(|m| m.middle().0).collect();
    for m in &rims {
        assert!(m.length <= 6100.0 && m.stock.is_some_and(|st| st >= m.length), "{} from stock: {:?}", m.key, m.stock);
        let [a, b] = [m.at(m.s[0], m.across[0]).0, m.at(m.s[0], m.across[1]).0];
        let inner = if (a.min(b) - X0).abs() < 0.02 { a.max(b) } else { a.min(b) };
        assert!(rafters.iter().any(|x| (x - inner).abs() < 0.02), "{} spliced at {inner}, over no rafter", m.key);
    }
}

#[test]
fn a_skylight_is_trimmed_headed_and_tailed_and_survives_undo_and_redo() {
    let mut s = Session::new();
    let (roof, walls) = cabin(&mut s, json!({}));
    run(&mut s, "modify", json!({"id": roof, "openings": [[[1000, 4000], [2000, 4000], [2000, 5200], [1000, 5200]]]}));
    let r = check(&mut s, roof, &walls);
    assert!(r.role("trimmer").len() >= 4, "{:?}", r.warnings);
    assert_eq!(r.role("header").len(), 4, "{:?}", r.warnings);
    let tails = r.members.iter().filter(|m| m.role == "rafter" && (1000.0..=2000.0).contains(&m.middle().0)).count();
    assert!(tails >= 4, "tails above and below it");
    let framed = read(&mut s, roof).raw;
    run(&mut s, "undo", json!({}));
    assert!(read(&mut s, roof).role("header").is_empty());
    run(&mut s, "redo", json!({}));
    assert_eq!(read(&mut s, roof).raw, framed);
}

#[test]
fn a_framed_roof_is_saved_by_its_framing_types_guid_renamed_with_it_kept_from_deletion_and_brought_in_from_a_file() {
    let mut s = Session::new();
    let (id, _) = cabin(&mut s, json!({}));
    let framed = |s: &Session| match s.model.elements.get(id) {
        Some(crate::model::Element::Roof(r)) => r.framing.clone(),
        _ => None,
    };
    let text = s.model.to_json().unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    let guid = s.model.library.guid(&Kind::Type("framing".into()), "Cabin Roof").unwrap();
    let held = file["elements"].as_object().unwrap().values().find(|e| e["guid"] == json!(s.model.guid(id).to_string())).unwrap();
    assert_eq!(held["framing"], json!(guid.to_string()), "a file holds the type's GUID");
    let (m, left_out) = crate::model::Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(crate::framing::roof::roof(&m, id), crate::framing::roof::roof(&s.model, id));
    run(&mut s, "rename_type", json!({"category": "framing", "type": "Cabin Roof", "name": "Roof 2x10"}));
    assert_eq!(framed(&s).as_deref(), Some("Roof 2x10"));
    assert!(s.execute("delete_type", &json!({"category": "framing", "type": "Roof 2x10"})).unwrap_err().contains("used 1 time"));
    run(&mut s, "modify", json!({"id": id, "framing": null}));
    assert!(s.execute("framing", &json!({"id": id})).unwrap_err().contains("isn't framed"));
    run(&mut s, "modify", json!({"id": id, "framing": "Roof 2x10"}));
    let path = std::env::temp_dir().join(format!("bimcraft-roof-framing-insert-{}.bimc", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut to = Session::new();
    let r = run(&mut to, "insert", json!({"path": path.to_str().unwrap(), "ids": [id]}));
    let _ = std::fs::remove_file(&path);
    let here = r["ids"][id.to_string()].as_u64().expect("the roof came");
    let kind = Kind::Type("framing".into());
    assert_eq!(to.model.library.guid(&kind, "Roof 2x10"), s.model.library.guid(&kind, "Roof 2x10"));
    // Without its walls and beams, nothing bears it there: one piece for each line, longer than the stock, each said.
    let r = read(&mut to, here);
    assert_eq!(r.role("rafter").len(), 22, "{:?}", r.warnings);
    assert!(r.warned("nothing bears its rafters"), "{:?}", r.warnings);
    run(&mut s, "unframe", json!({"id": id}));
    assert_eq!(framed(&s), None);
    run(&mut s, "delete_type", json!({"category": "framing", "type": "Roof 2x10"}));
}

#[test]
fn a_roof_type_with_no_structure_layer_and_an_unknown_framing_type_are_refused_saying_what_to_fix() {
    let mut s = Session::new();
    let (roof, _) = cabin(&mut s, json!({}));
    let e = s.execute("modify", &json!({"id": roof, "framing": "Nope"})).unwrap_err();
    assert!(e.contains("framing type `Nope` doesn't exist"), "{e}");
    run(
        &mut s,
        "type",
        json!({"category": "roof", "name": "Membrane", "layers": [{"function": "finish", "material": "Timber", "thickness": 20}]}),
    );
    let e = s.execute("modify", &json!({"id": roof, "type": "Membrane"})).unwrap_err();
    assert!(e.contains("no structure layer"), "{e}");
    let e = s.execute("modify", &json!({"id": roof, "shape": {"points": [{"at": [0, 0], "z": 10}]}})).unwrap_err();
    assert!(!e.is_empty());
}

/// For the hostile sweep (`tests::hostile_params_never_panic_or_break_the_model`): a framed gable roof over two framed walls,
/// open through a skylight, made in `base`; and payloads for the framing commands on it with their real keys.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let _ = base.execute(
        "type",
        &json!({"category": "roof", "name": "Sweep Rafters", "layers": [{"function": "structure", "material": "Timber", "thickness": 235}]}),
    );
    let id = base
        .execute(
            "roof",
            &json!({"boundary": [[0, 20000], [6000, 20000], [6000, 24000], [0, 24000]], "slope": 30, "gables": [1, 3],
                    "type": "Sweep Rafters", "level": "Level 1", "offset": 2400,
                    "openings": [[[1000, 20800], [2000, 20800], [2000, 21600], [1000, 21600]]]}),
        )
        .ok()
        .and_then(|v| v["id"].as_u64())
        .unwrap_or(0);
    let _ = base.execute("frame", &json!({"id": id}));
    let ty = |v: Value| {
        let mut t = json!({"category": "framing", "name": "2x4 @ 406"});
        t.as_object_mut().into_iter().for_each(|o| o.extend(v.as_object().cloned().unwrap_or_default()));
        ("type", t)
    };
    vec![
        ("frame", json!({"id": id, "angle": 45})),
        ("frame", json!({"ids": [id, 100002]})),
        ("unframe", json!({"ids": [id]})),
        ("framing", json!({"id": id})),
        ty(json!({"ridge": false, "rim": true, "lap": 230})),
        ty(json!({"ridge": 1, "stock": [1e300]})),
        ty(json!({"stock": [], "lap": 2000, "bridging": 300})),
        ("modify", json!({"id": id, "framing": "2x4 @ 406"})),
        ("modify", json!({"id": id, "framing": 7})),
        ("modify", json!({"id": id, "slope": 75, "gables": []})),
        ("modify", json!({"id": id, "slope": 1e-300})),
        ("modify", json!({"id": id, "boundary": [[0, 20000], [9.99e8, 20000], [9.99e8, 24000], [0, 24000]]})),
        ("modify", json!({"id": id, "openings": [[[1, 20001], [5999, 20001], [5999, 23999], [1, 23999]]]})),
    ]
}
