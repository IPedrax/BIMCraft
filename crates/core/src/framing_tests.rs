//! Wood framing (`framing.rs`) driven through commands: the cabin the user framed by hand is the acceptance test.

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

/// A 2x4 wall: one structure layer of timber, 89 mm.
fn stud_wall_type(s: &mut Session) {
    run(
        s,
        "type",
        json!({"category": "wall", "name": "2x4", "layers": [{"function": "structure", "material": "Timber", "thickness": 89}]}),
    );
}

/// A straight free wall of the 2x4 type, 2448 mm tall, from `a` to `b`.
fn wall(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
    first(&run(s, "wall", json!({"points": [a, b], "type": "2x4", "height": 2448})))
}

/// The cabin's south wall, 7320 mm long, framed at a third of 1220 from its start.
fn cabin(s: &mut Session) -> u64 {
    stud_wall_type(s);
    run(s, "type", json!({"category": "framing", "name": "Cabin", "spacing": 1220.0 / 3.0}));
    let w = wall(s, [0.0, 0.0], [7320.0, 0.0]);
    run(s, "frame", json!({"id": w, "type": "Cabin"}));
    w
}

fn members(s: &mut Session, id: u64) -> Vec<Value> {
    run(s, "framing", json!({"id": id}))["members"].as_array().cloned().unwrap_or_default()
}

fn f(v: &Value, i: usize) -> f64 {
    v[i].as_f64().unwrap_or(f64::NAN)
}

fn centre(m: &Value) -> f64 {
    (f(&m["s"], 0) + f(&m["s"], 1)) / 2.0
}

fn role<'a>(ms: &'a [Value], role: &str) -> Vec<&'a Value> {
    ms.iter().filter(|m| m["role"] == role).collect()
}

fn keyed<'a>(ms: &'a [Value], key: &str) -> &'a Value {
    ms.iter().find(|m| m["key"] == key).unwrap_or_else(|| panic!("no member {key}"))
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 0.02, "{what}: {a} isn't {b}");
}

fn window(s: &mut Session, w: u64, width: f64, offset: f64) -> u64 {
    let name = format!("{width} x 1200");
    run(s, "type", json!({"category": "window", "name": name, "width": width, "height": 1200, "sill": 832}));
    first(&run(s, "window", json!({"wall": w, "offset": offset, "type": name})))
}

#[test]
fn the_cabins_wall_has_studs_from_its_start_every_third_of_1220_and_end_studs_flush() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    let ms = members(&mut s, w);
    let mut studs: Vec<f64> = role(&ms, "stud").into_iter().map(centre).collect();
    studs.sort_by(f64::total_cmp);
    assert_eq!(studs.len(), 19, "two end studs and 17 between: {studs:?}");
    for (i, at) in [19.0, 406.67, 813.33, 1220.0].into_iter().enumerate() {
        close(studs[i], at, "stud");
    }
    close(studs[18], 7301.0, "the last stud");
    for st in role(&ms, "stud") {
        assert_eq!(st["section"], json!([38.0, 89.0]));
        close(f(&st["z"], 0), 38.0, "on one bottom plate");
        close(f(&st["z"], 2), 2448.0 - 76.0, "under two top plates");
        close(st["length"].as_f64().unwrap_or_default(), 2334.0, "cut length");
    }
    // 7320 long, more than the longest stock (6100): each plate in two pieces, end to end, each from stock.
    let (bottom, top) = (role(&ms, "bottom_plate"), role(&ms, "top_plate"));
    assert_eq!((bottom.len(), top.len()), (2, 4));
    for p in bottom.iter().chain(&top) {
        assert_eq!(p["section"], json!([38.0, 89.0]));
        assert!(p["length"].as_f64().unwrap_or_default() <= 6100.0, "{p}");
        assert!(p["stock"].as_f64().is_some(), "{p}");
    }
    let total: f64 = bottom.iter().map(|p| p["length"].as_f64().unwrap_or_default()).sum();
    close(total, 7320.0, "the bottom plate's pieces");
    let r = run(&mut s, "framing", json!({"id": w}));
    let studs_cut = r["cut_list"].as_array().unwrap().iter().find(|c| c["role"] == "stud").cloned().unwrap();
    assert_eq!(
        (studs_cut["count"].as_u64(), studs_cut["length"].as_f64(), studs_cut["stock"].as_f64()),
        (Some(19), Some(2334.0), Some(2440.0))
    );
}

#[test]
fn a_window_gets_kings_jacks_a_header_a_sill_and_cripples_at_the_grid_inside_its_rough_opening() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    let win = window(&mut s, w, 1200.0, 1250.0);
    let g = s.model.guid(win).to_string();
    let ms = members(&mut s, w);
    // The rough opening: 1225 wide, from 637.5 to 1862.5; 1213 high, from 832 to 2045.
    close(centre(keyed(&ms, &format!("{g}/king/left/0"))), 637.5 - 57.0, "left king");
    close(centre(keyed(&ms, &format!("{g}/king/right/0"))), 1862.5 + 57.0, "right king");
    let jack = keyed(&ms, &format!("{g}/jack/left/0"));
    close(centre(jack), 637.5 - 19.0, "left jack");
    close(f(&jack["z"], 2), 2045.0, "the jack under the header");
    close(centre(keyed(&ms, &format!("{g}/jack/right/0"))), 1862.5 + 19.0, "right jack");
    let header = keyed(&ms, &format!("{g}/header"));
    assert_eq!((header["plies"].as_u64(), &header["section"]), (Some(2), &json!([38.0, 184.0])));
    close(f(&header["s"], 0), 637.5 - 38.0, "header from");
    close(f(&header["s"], 1), 1862.5 + 38.0, "header to");
    close(f(&header["z"], 0), 2045.0, "header bottom");
    close(f(&header["z"], 2), 2045.0 + 184.0, "header top");
    let sill = keyed(&ms, &format!("{g}/sill/0"));
    close(f(&sill["z"], 2), 832.0, "sill top");
    close(f(&sill["s"], 0), 637.5, "sill from");
    // The grid's stations 2, 3 and 4 (813.33, 1220, 1626.67) are inside it: cripples over the header and under the sill.
    for k in 2..=4 {
        let top = keyed(&ms, &format!("{g}/cripple/top/{k}"));
        close(f(&top["z"], 0), 2229.0, "upper cripple on the header");
        close(f(&top["z"], 2), 2372.0, "upper cripple under the top plates");
        let under = keyed(&ms, &format!("{g}/cripple/bottom/{k}"));
        close(f(&under["z"], 0), 38.0, "lower cripple on the bottom plate");
        close(f(&under["z"], 2), 832.0 - 38.0, "lower cripple under the sill");
        assert!(!ms.iter().any(|m| m["key"] == format!("stud/{k}")), "no stud {k} through the window");
    }
    assert_eq!(role(&ms, "cripple").len(), 6);
    assert!(ms.iter().any(|m| m["key"] == "stud/1") && ms.iter().any(|m| m["key"] == "stud/5"), "the studs clear of the kings stay");
}

#[test]
fn a_rough_opening_wider_than_1250_gets_the_deeper_header_and_a_partition_type_a_flat_one() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    let win = window(&mut s, w, 1300.0, 2000.0);
    let g = s.model.guid(win).to_string();
    let ms = members(&mut s, w);
    assert_eq!(keyed(&ms, &format!("{g}/header"))["section"], json!([38.0, 235.0]), "1325 wide: 2-2x10");
    run(&mut s, "type", json!({"category": "framing", "name": "Partition", "headers": [{"depth": 38, "flat": true}]}));
    run(&mut s, "frame", json!({"id": w, "type": "Partition"}));
    let ms = members(&mut s, w);
    let header = keyed(&ms, &format!("{g}/header"));
    assert_eq!((header["plies"].as_u64(), &header["section"]), (Some(1), &json!([38.0, 89.0])));
    close(f(&header["z"], 2) - f(&header["z"], 0), 38.0, "a 2x4 on the flat");
    close(f(&header["across"], 1) - f(&header["across"], 0), 89.0, "as wide as the layer");
}

#[test]
fn a_door_cuts_the_bottom_plate_and_has_no_sill_and_no_lower_cripples() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    let d = first(&run(&mut s, "door", json!({"wall": w, "offset": 3000})));
    let g = s.model.guid(d).to_string();
    let ms = members(&mut s, w);
    // A 900 door: its rough opening 950 wide, from 2525 to 3475, and 2100 + 41 high.
    let mut plates: Vec<[f64; 2]> = role(&ms, "bottom_plate").iter().map(|p| [f(&p["s"], 0), f(&p["s"], 1)]).collect();
    plates.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(plates, [[0.0, 2525.0], [3475.0, 7320.0]]);
    assert!(keyed(&ms, &format!("bottom_plate/0/{g}/0"))["s"] == json!([3475.0, 7320.0]), "named by the door it follows");
    close(f(&keyed(&ms, &format!("{g}/header"))["z"], 0), 2141.0, "header on the jacks");
    assert!(!ms.iter().any(|m| m["role"] == "sill"));
    assert!(!ms.iter().any(|m| m["key"].as_str().is_some_and(|k| k.contains("cripple/bottom"))));
}

/// Whether a member keyed as `prefix` says is among `ms`.
fn any_keyed(ms: &[Value], prefix: &str) -> bool {
    ms.iter().any(|m| m["key"].as_str().is_some_and(|k| k.starts_with(prefix)))
}

#[test]
fn a_gap_too_short_for_a_cripple_over_a_header_or_under_a_sill_is_filled_with_flat_blocking() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    // A 900 x 2100 door: its header's top at 2141 + 184 = 2325, 47 under the top plates (2372); a cripple 47 long splits.
    let d = first(&run(&mut s, "door", json!({"wall": w, "offset": 3000})));
    // A window 150 up: its sill from 112, 74 over the bottom plate; its header's top at 150 + 900 + 13 + 184 = 1247.
    let win = window_at(&mut s, w, 600.0, 900.0, 150.0, 5500.0);
    let (dg, wg) = (s.model.guid(d).to_string(), s.model.guid(win).to_string());
    let ms = members(&mut s, w);
    assert!(!any_keyed(&ms, &format!("{dg}/cripple/")), "no cripple over the door's header");
    let fill = keyed(&ms, &format!("{dg}/blocking/top/0"));
    assert_eq!((&fill["role"], &fill["section"], fill["plies"].as_u64()), (&json!("blocking"), &json!([38.0, 89.0]), Some(1)));
    assert_eq!(fill["z"], json!([2325.0, 2325.0, 2363.0, 2363.0]), "one flat on the header, all that fits");
    assert_eq!(fill["s"], json!([2525.0 - 38.0, 3475.0 + 38.0]), "as long as the header");
    close(f(&fill["across"], 1) - f(&fill["across"], 0), 89.0, "as wide as the layer");
    assert!(!any_keyed(&ms, &format!("{dg}/blocking/top/1")));
    // Under the window's sill: one flat on the bottom plate, between the jacks; over its header the cripples stay.
    assert!(!any_keyed(&ms, &format!("{wg}/cripple/bottom/")), "no cripple under the window's sill");
    let under = keyed(&ms, &format!("{wg}/blocking/bottom/0"));
    assert_eq!(under["z"], json!([38.0, 38.0, 76.0, 76.0]));
    assert_eq!(under["s"], json!([5500.0 - 312.5, 5500.0 + 312.5]));
    assert!(!any_keyed(&ms, &format!("{wg}/blocking/bottom/1")));
    for k in [13, 14] {
        let c = keyed(&ms, &format!("{wg}/cripple/top/{k}"));
        assert_eq!((f(&c["z"], 0), f(&c["z"], 2)), (1247.0, 2372.0), "{c}");
    }
    // A cripple three times as long as it is thick (114) is one: the cabin's window's of 143 stay.
    let win = window(&mut s, w, 1200.0, 1250.0);
    let g = s.model.guid(win).to_string();
    let ms = members(&mut s, w);
    assert_eq!((2..=4).filter(|k| ms.iter().any(|m| m["key"] == format!("{g}/cripple/top/{k}"))).count(), 3);
}

#[test]
fn under_a_sloped_top_a_header_gets_flats_only_where_its_whole_gap_is_short_and_no_cripple_is_shorter_than_114() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    // The 5 % shed over the cabin's wall, rising east: the top plates' underside climbs 5 mm in each 100.
    let slope = 0.05f64.atan().to_degrees();
    let roof = first(&run(
        &mut s,
        "roof",
        json!({"boundary": [[-500, -1000], [8000, -1000], [8000, 1000], [-500, 1000]], "level": "Level 1", "offset": 2400,
               "slope": slope, "gables": [0, 1, 2]}),
    ));
    run(&mut s, "modify", json!({"id": w, "attach_top": roof}));
    let doors: Vec<(String, f64)> = [1000.0, 2200.0, 5500.0]
        .into_iter()
        .map(|at| {
            let d = first(&run(&mut s, "door", json!({"wall": w, "offset": at})));
            (s.model.guid(d).to_string(), at)
        })
        .collect();
    let ms = members(&mut s, w);
    // The lower top plates' underside along the wall.
    let under = |at: f64| {
        let p = role(&ms, "top_plate")
            .into_iter()
            .filter(|p| p["key"].as_str().is_some_and(|k| k.starts_with("top_plate/1/")))
            .find(|p| f(&p["s"], 0) <= at && at <= f(&p["s"], 1))
            .cloned()
            .unwrap_or_else(|| panic!("no lower top plate at {at}"));
        f(&p["z"], 0) + (f(&p["z"], 1) - f(&p["z"], 0)) * (at - f(&p["s"], 0)) / (f(&p["s"], 1) - f(&p["s"], 0))
    };
    for (g, at) in &doors {
        // Each header from 475 before its door's centre to 475 after it, its top at 2325.
        let (s0, s1) = (at - 513.0, at + 513.0);
        let (low, high) = (under(s0) - 2325.0, under(s1) - 2325.0);
        let cripples: Vec<&Value> =
            ms.iter().filter(|m| m["key"].as_str().is_some_and(|k| k.starts_with(&format!("{g}/cripple/")))).collect();
        let flats = ms.iter().filter(|m| m["key"].as_str().is_some_and(|k| k.starts_with(&format!("{g}/blocking/top/")))).count();
        if high < 114.0 {
            assert!(cripples.is_empty(), "door at {at}: the whole gap ({low:.0} to {high:.0}) short: {cripples:?}");
            assert_eq!(flats, (low / 38.0).floor() as usize, "door at {at}: as many flats as fit under its low end ({low:.0})");
        } else {
            assert_eq!(flats, 0, "door at {at}: cripples, not flats ({low:.0} to {high:.0})");
            assert!(!cripples.is_empty(), "door at {at}");
            for c in &cripples {
                assert!(c["length"].as_f64().unwrap_or_default() >= 114.0 - 1e-6, "door at {at}: a cripple too short: {c}");
            }
        }
    }
    // Which doors are which: the first's gap all short, the second's crossing 114 (a short cripple left out), the third's long.
    let gap = |at: f64| (under(at - 513.0) - 2325.0, under(at + 513.0) - 2325.0);
    let ((l0, h0), (l1, h1), (l2, _)) = (gap(1000.0), gap(2200.0), gap(5500.0));
    assert!(l0 > 38.0 && h0 < 114.0 && l1 < 114.0 && h1 > 114.0 && l2 > 114.0, "{l0} {h0} {l1} {h1} {l2}");
}

#[test]
fn a_wall_under_a_roof_sloping_5_percent_along_it_has_bevelled_studs_and_sloped_top_plates() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    // A shed roof rising east from its west eave: every edge but that one upright.
    let slope = 0.05f64.atan().to_degrees();
    let roof = first(&run(
        &mut s,
        "roof",
        json!({"boundary": [[-500, -1000], [8000, -1000], [8000, 1000], [-500, 1000]], "level": "Level 1", "offset": 2400,
               "slope": slope, "gables": [0, 1, 2]}),
    ));
    run(&mut s, "modify", json!({"id": w, "attach_top": roof}));
    let ms = members(&mut s, w);
    let mut studs: Vec<&Value> = role(&ms, "stud");
    studs.sort_by(|a, b| centre(a).total_cmp(&centre(b)));
    assert_eq!(studs.len(), 19);
    for pair in studs.windows(2) {
        assert!(pair[1]["length"].as_f64() > pair[0]["length"].as_f64(), "each stud its own length: {} {}", pair[0], pair[1]);
    }
    for st in &studs {
        close(f(&st["z"], 3) - f(&st["z"], 2), 38.0 * 0.05, "a bevelled top");
        close(st["length"].as_f64().unwrap_or_default(), f(&st["z"], 3) - f(&st["z"], 1), "cut to its long edge");
    }
    let tops = role(&ms, "top_plate");
    assert_eq!(tops.len(), 4, "two plates, each spliced in two");
    for p in tops {
        let run_ = f(&p["s"], 1) - f(&p["s"], 0);
        close((f(&p["z"], 3) - f(&p["z"], 2)) / run_, 0.05, "the plate slopes with the roof");
        close(p["length"].as_f64().unwrap_or_default(), run_ * (1.0 + 0.05f64 * 0.05).sqrt(), "its slope length");
    }
}

/// A member's box in the world: [x0, y0, z0, x1, y1, z1], from a `framing` reply.
fn boxes(r: &Value) -> Vec<(String, [f64; 6])> {
    let (o, x, y) = (&r["origin"], &r["x_axis"], &r["depth_axis"]);
    r["members"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let at = |s: f64, a: f64| [f(o, 0) + f(x, 0) * s + f(y, 0) * a, f(o, 1) + f(x, 1) * s + f(y, 1) * a];
            let (p, q) = (at(f(&m["s"], 0), f(&m["across"], 0)), at(f(&m["s"], 1), f(&m["across"], 1)));
            let z = &m["z"];
            let (z0, z1) = (f(z, 0).min(f(z, 1)), f(z, 2).max(f(z, 3)));
            (m["key"].as_str().unwrap_or_default().to_string(), [p[0].min(q[0]), p[1].min(q[1]), z0, p[0].max(q[0]), p[1].max(q[1]), z1])
        })
        .collect()
}

#[test]
fn two_framed_walls_at_a_square_corner_one_running_through_never_overlap_nor_leave_a_gap() {
    let mut s = Session::new();
    stud_wall_type(&mut s);
    let r = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000]], "type": "2x4", "height": 2448}));
    let (a, b) = (r["ids"][0].as_u64().unwrap(), r["ids"][1].as_u64().unwrap());
    run(&mut s, "frame", json!({"ids": [a, b]}));
    assert!(s.model.types.framing.contains_key("2x4 @ 406"), "the default type is made");
    let (ra, rb) = (run(&mut s, "framing", json!({"id": a})), run(&mut s, "framing", json!({"id": b})));
    let (ba, bb) = (boxes(&ra), boxes(&rb));
    for (ka, x) in &ba {
        for (kb, y) in &bb {
            let overlap = (0..3).all(|i| x[i + 3].min(y[i + 3]) - x[i].max(y[i]) > 1e-3);
            assert!(!overlap, "{ka} of one wall and {kb} of the other overlap: {x:?} {y:?}");
        }
    }
    // Round the corner on the bottom plates' height, every point of the two structure layers is in one plate exactly.
    let plates: Vec<[f64; 6]> = ba.iter().chain(&bb).filter(|(k, _)| k.starts_with("bottom_plate")).map(|(_, b)| *b).collect();
    for p in [[4000.0, 0.0], [4040.0, 40.0], [3960.0, -40.0], [4000.0, 60.0], [3940.0, 0.0], [4040.0, -40.0], [3960.0, 40.0]] {
        let n = plates.iter().filter(|b| b[0] < p[0] && p[0] < b[3] && b[1] < p[1] && p[1] < b[4]).count();
        assert_eq!(n, 1, "{p:?} is in {n} plates");
    }
    let through = if s.model.guid(a) < s.model.guid(b) { &ba } else { &bb };
    assert!(through.iter().any(|(k, _)| k.starts_with("corner/")), "a third stud in the wall running through");
}

#[test]
fn a_wall_with_no_structure_layer_or_an_arc_wall_is_refused_framing_saying_why() {
    let mut s = Session::new();
    let generic = first(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]})));
    let e = s.execute("frame", &json!({"id": generic})).unwrap_err();
    assert!(e.contains("structure layer"), "{e}");
    assert!(s.model.wall(generic).is_some_and(|w| w.framing.is_none()));
    stud_wall_type(&mut s);
    let arc = first(&run(&mut s, "wall", json!({"points": [[0, 5000], [4000, 5000]], "arc": 90, "type": "2x4"})));
    let e = s.execute("frame", &json!({"id": arc})).unwrap_err();
    assert!(e.contains("straight"), "{e}");
    // And a framed wall can't become one: the edit is refused.
    let w = wall(&mut s, [0.0, 9000.0], [4000.0, 9000.0]);
    run(&mut s, "frame", json!({"id": w}));
    assert!(s.execute("modify", &json!({"id": w, "arc": 45})).is_err());
    assert!(s.execute("modify", &json!({"id": w, "type": "Generic - 200mm"})).is_err());
    assert!(s.execute("modify", &json!({"id": w, "framing": "Nope"})).is_err());
    assert!(s.execute("frame", &json!({"id": w, "type": "Nope"})).is_err());
    let door = first(&run(&mut s, "door", json!({"wall": w, "offset": 2000})));
    assert!(s.execute("frame", &json!({"ids": [w, door]})).unwrap_err().contains("isn't a wall"));
    assert!(s.execute("type", &json!({"category": "framing", "name": "Tight", "spacing": 30})).is_err(), "closer than thick");
    assert!(s.execute("type", &json!({"category": "framing", "name": "Tight", "spacng": 400})).is_err(), "a misspelled key");
    assert!(s.execute("type", &json!({"category": "framing", "name": "2x4 @ 406", "headers": [{"depth": 184, "plies": 3}]})).is_err());
}

#[test]
fn framing_undoes_and_redoes_and_modify_framing_null_unframes() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    let framed = |s: &Session| s.model.wall(w).and_then(|x| x.framing.clone());
    run(&mut s, "undo", json!({}));
    assert_eq!(framed(&s), None);
    run(&mut s, "redo", json!({}));
    assert_eq!(framed(&s).as_deref(), Some("Cabin"));
    run(&mut s, "modify", json!({"id": w, "framing": null}));
    assert_eq!(framed(&s), None);
    run(&mut s, "modify", json!({"id": w, "framing": "Cabin"}));
    run(&mut s, "unframe", json!({"ids": [w]}));
    assert_eq!(framed(&s), None);
    assert!(s.execute("framing", &json!({"id": w})).unwrap_err().contains("isn't framed"));
}

#[test]
fn a_framed_wall_is_saved_by_its_types_guid_and_reopens_framed_and_a_file_of_format_20_opens_unchanged() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    let text = s.model.to_json().unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    let guid = s.model.library.guid(&Kind::Type("framing".into()), "Cabin").unwrap();
    let held = file["elements"].as_object().unwrap().values().find(|e| e["guid"] == json!(s.model.guid(w).to_string())).unwrap();
    assert_eq!(held["framing"], json!(guid.to_string()), "a file holds the type's GUID");
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.wall(w).and_then(|x| x.framing.clone()).as_deref(), Some("Cabin"));
    assert_eq!(m.types.framing, s.model.types.framing);
    assert_eq!(crate::framing::wall(&m, w), crate::framing::wall(&s.model, w));
    // A project of 20, framed nowhere, reads as it was.
    let mut plain = Session::new();
    stud_wall_type(&mut plain);
    wall(&mut plain, [0.0, 0.0], [4000.0, 0.0]);
    let mut v: Value = serde_json::from_str(&plain.model.to_json().unwrap()).unwrap();
    v["format"] = json!(20);
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty() && m.format == crate::model::FORMAT, "{left_out:?}");
    assert_eq!(m.to_json().unwrap(), plain.model.to_json().unwrap());
}

#[test]
fn moving_a_window_keeps_the_keys_and_places_of_the_studs_far_from_it() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    let win = window(&mut s, w, 1200.0, 1250.0);
    let far = |ms: &[Value]| -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = ms
            .iter()
            .filter(|m| m["role"] == "stud" && centre(m) > 3000.0)
            .map(|m| (m["key"].as_str().unwrap_or("").into(), centre(m)))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    let before = members(&mut s, w);
    run(&mut s, "modify", json!({"id": win, "offset": 1400}));
    let after = members(&mut s, w);
    assert_ne!(before, after, "the window's members moved");
    assert_eq!(far(&before), far(&after));
    assert!(far(&before).iter().any(|(k, _)| k == "stud/17"), "grid studs by their station: {:?}", far(&before));
}

#[test]
fn a_framing_type_renames_on_its_walls_is_kept_while_used_and_comes_in_with_a_wall_from_another_file() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    run(&mut s, "rename_type", json!({"category": "framing", "type": "Cabin", "name": "Cabin 16"}));
    assert_eq!(s.model.wall(w).and_then(|x| x.framing.clone()).as_deref(), Some("Cabin 16"));
    let e = s.execute("delete_type", &json!({"category": "framing", "type": "Cabin 16"})).unwrap_err();
    assert!(e.contains("used 1 time"), "{e}");
    // Insert from File brings the wall with its framing type, its GUID kept.
    let path = std::env::temp_dir().join(format!("bimcraft-framing-insert-{}.bimc", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut to = Session::new();
    let r = run(&mut to, "insert", json!({"path": path.to_str().unwrap(), "ids": [w]}));
    let here = r["ids"][w.to_string()].as_u64().expect("the wall came");
    let _ = std::fs::remove_file(&path);
    assert_eq!(to.model.wall(here).and_then(|x| x.framing.clone()).as_deref(), Some("Cabin 16"));
    let kind = Kind::Type("framing".into());
    assert_eq!(to.model.library.guid(&kind, "Cabin 16"), s.model.library.guid(&kind, "Cabin 16"));
    assert_eq!(members(&mut to, here).len(), members(&mut s, w).len());
    // Unused, the last framing type goes: a project needs none.
    run(&mut s, "unframe", json!({"id": w}));
    run(&mut s, "delete_type", json!({"category": "framing", "type": "Cabin 16"}));
    assert!(s.model.types.framing.is_empty());
}

#[test]
fn blocking_runs_between_full_studs_and_tee_studs_back_a_wall_butting_into_the_side() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    run(&mut s, "type", json!({"category": "framing", "name": "Cabin", "blocking": [1200]}));
    let partition = wall(&mut s, [3608.65, 0.0], [3608.65, 3000.0]);
    let pg = s.model.guid(partition).to_string();
    let ms = members(&mut s, w);
    close(centre(keyed(&ms, &format!("tee/{pg}/0"))), 3608.65 - 44.5 - 19.0, "backing west of it");
    close(centre(keyed(&ms, &format!("tee/{pg}/1"))), 3608.65 + 44.5 + 19.0, "backing east of it");
    let blocks = role(&ms, "blocking");
    assert!(!blocks.is_empty());
    for b in &blocks {
        close(f(&b["z"], 0), 1200.0 - 19.0, "centred on its row");
    }
    // Between each pair of neighbouring full-height studs, end to end: the blocks and the studs fill the wall.
    let studs = role(&ms, "stud").len();
    assert_eq!(blocks.len(), studs - 1);
}

/// For the hostile sweep (`tests::hostile_params_never_panic_or_break_the_model`): a framed wall with a window and a
/// partition butting into it, made in `base`; and payloads for the framing commands with their real keys.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    stud_wall_type(base);
    let _ = base.execute("type", &json!({"category": "framing", "name": "Sweep", "blocking": [1200]}));
    let w = wall(base, [0.0, -3000.0], [6000.0, -3000.0]);
    let p = wall(base, [3000.0, -3000.0], [3000.0, -5000.0]);
    let win = window(base, w, 1200.0, 1500.0);
    let _ = base.execute("frame", &json!({"ids": [w, p], "type": "Sweep"}));
    let ty = |v: Value| {
        let mut t = json!({"category": "framing", "name": "Sweep"});
        t.as_object_mut().into_iter().for_each(|o| o.extend(v.as_object().cloned().unwrap_or_default()));
        ("type", t)
    };
    vec![
        ("frame", json!({"ids": [w, 100002], "type": "Sweep"})),
        ("frame", json!({"id": w, "type": "Nope"})),
        ("frame", json!({"id": w, "type": 5, "extra": 1})),
        ("frame", json!({"ids": [w, win]})),
        ("frame", json!({"ids": [p]})),
        ("unframe", json!({"ids": [w, win]})),
        ("unframe", json!({"id": "x"})),
        ("framing", json!({"id": w})),
        ("framing", json!({"id": win})),
        ("framing", json!({"ids": [w, p]})),
        ty(json!({"spacing": 1e300})),
        ty(json!({"spacing": 20, "thickness": 38})),
        ty(json!({"thickness": -1, "spacing": f64::MAX})),
        ty(json!({"headers": [{"depth": 184, "plies": 3}]})),
        ty(json!({"headers": [], "stock": [3000, 2000]})),
        ty(json!({"headers": [{"depth": 1e300, "up_to": -1}], "stock": vec![3000; 100]})),
        ty(json!({"bottom_plates": 1e9, "kings": -1})),
        ty(json!({"top_plates": 4, "bottom_plates": 4, "sill_plates": 4, "kings": 3, "jacks": 3})),
        ty(json!({"blocking": vec![1000; 100], "material": "Nope"})),
        ty(json!({"blocking": [0, 99999, 1e300]})),
        ty(json!({"layout": "sideways", "corner": "four_stud", "tee": 5})),
        ty(json!({"layout": "centre", "corner": "two_stud", "tee": "none", "material": "Concrete"})),
        ty(json!({"window_rough": [1e300, 0], "door_rough": [-1]})),
        ty(json!({"window_rough": [500, 500], "door_rough": [500, 500], "spacing": 39})),
        ty(json!({"headers": [{"depth": 1000, "plies": 1, "flat": true}, {"depth": 38}]})),
        ty(json!({"nope": 1})),
        ("modify", json!({"id": w, "framing": "Nope"})),
        ("modify", json!({"id": w, "framing": 5})),
        ("modify", json!({"id": w, "arc": 90})),
        ("modify", json!({"id": w, "end": [9.99e8, -3000]})),
        ("modify", json!({"id": w, "height": 60})),
        ("modify", json!({"id": w, "height": 1e9})),
        ("modify", json!({"id": win, "offset": 1e8})),
        ("modify", json!({"id": win, "offset": 30, "sill": -500})),
        ("modify", json!({"id": p, "start": [3000, -3020]})),
        ("type", json!({"category": "wall", "name": "2x4", "layers": [{"function": "finish", "material": "Timber", "thickness": 89}]})),
        ("type", json!({"category": "wall", "name": "2x4", "layers": [{"function": "structure", "material": "Timber", "thickness": 50}]})),
        ("delete_type", json!({"category": "framing", "type": "Sweep"})),
        ("rename_type", json!({"category": "framing", "type": "Sweep", "name": "Swept"})),
        ("rename_type", json!({"category": "framing", "type": "Sweep", "name": ""})),
    ]
}

/// A window type `width` x `height` with its sill at `sill`, placed `offset` along wall `w`.
fn window_at(s: &mut Session, w: u64, width: f64, height: f64, sill: f64, offset: f64) -> u64 {
    let name = format!("{width} x {height} @ {sill}");
    run(s, "type", json!({"category": "window", "name": name, "width": width, "height": height, "sill": sill}));
    first(&run(s, "window", json!({"wall": w, "offset": offset, "type": name})))
}

#[test]
fn a_window_too_tall_for_its_header_is_still_framed_with_a_shallower_header_and_a_warning() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    // Rough top 900 + 1300 + 13 = 2213: 159 mm under the top plates (2372), less than the 2x8's 184.
    let win = window_at(&mut s, w, 1200.0, 1300.0, 900.0, 1250.0);
    let g = s.model.guid(win).to_string();
    let r = run(&mut s, "framing", json!({"id": w}));
    let ms = r["members"].as_array().cloned().unwrap_or_default();
    for side in ["left", "right"] {
        keyed(&ms, &format!("{g}/king/{side}/0"));
        keyed(&ms, &format!("{g}/jack/{side}/0"));
    }
    keyed(&ms, &format!("{g}/sill/0"));
    let header = keyed(&ms, &format!("{g}/header"));
    close(f(&header["z"], 0), 2213.0, "header on the jacks");
    close(f(&header["z"], 2), 2372.0, "header up to the top plates");
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|t| t.contains("159") && t.contains("184"))), "{warnings:?}");
}

#[test]
fn a_window_whose_head_reaches_the_top_plates_keeps_its_kings_jacks_and_sill_with_no_header_and_a_warning() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    // The demo's 1200 x 1500 at 900: rough top 2413, past the top plates' underside at 2372.
    let win = window_at(&mut s, w, 1200.0, 1500.0, 900.0, 1250.0);
    let g = s.model.guid(win).to_string();
    let r = run(&mut s, "framing", json!({"id": w}));
    let ms = r["members"].as_array().cloned().unwrap_or_default();
    for side in ["left", "right"] {
        keyed(&ms, &format!("{g}/king/{side}/0"));
        let jack = keyed(&ms, &format!("{g}/jack/{side}/0"));
        close(f(&jack["z"], 2), 2372.0, "the jack up to the top plates");
    }
    keyed(&ms, &format!("{g}/sill/0"));
    assert!(!ms.iter().any(|m| m["key"] == format!("{g}/header")), "no room for a header");
    assert!(!ms.iter().any(|m| m["key"].as_str().is_some_and(|k| k.starts_with(&format!("{g}/cripple/top")))));
    assert_eq!(role(&ms, "cripple").len(), 3, "the lower cripples stay");
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|t| t.contains("top plates"))), "{warnings:?}");
    // `frame` says so too.
    let done = run(&mut s, "frame", json!({"id": w, "type": "Cabin"}));
    assert!(!done["warnings"].as_array().cloned().unwrap_or_default().is_empty(), "{done}");
}

#[test]
fn an_opening_that_cant_be_framed_is_never_left_without_a_warning() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    // So near the start that its kings would run past the wall's end.
    window_at(&mut s, w, 600.0, 900.0, 1000.0, 330.0);
    let r = run(&mut s, "framing", json!({"id": w}));
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|t| t.contains("isn't framed"))), "{warnings:?}");
}

/// Where pieces of one plate (each `[from, to]` along the wall) meet end to end: its splices.
fn splices(pieces: &[&Value]) -> Vec<f64> {
    let mut spans: Vec<[f64; 2]> = pieces.iter().map(|p| [f(&p["s"], 0), f(&p["s"], 1)]).collect();
    spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
    spans.windows(2).filter(|w| (w[0][1] - w[1][0]).abs() < 1e-6).map(|w| w[0][1]).collect()
}

#[test]
fn plates_longer_than_the_longest_stock_are_spliced_over_full_studs_the_top_plates_splices_610_apart() {
    let mut s = Session::new();
    stud_wall_type(&mut s);
    // The user's cabin: a wall 6101 long and 2440 high framed by the default type (6100 its longest stock), a window in it.
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [6101, 0]], "type": "2x4", "height": 2440})));
    run(&mut s, "frame", json!({"id": w}));
    let win = window(&mut s, w, 1200.0, 3000.0);
    let r = run(&mut s, "framing", json!({"id": w}));
    assert_eq!(r["warnings"], json!([]), "{r}");
    let ms = r["members"].as_array().cloned().unwrap_or_default();
    for p in role(&ms, "bottom_plate").into_iter().chain(role(&ms, "top_plate")) {
        assert!(p["length"].as_f64().unwrap_or_default() <= 6100.0 && p["stock"].as_f64().is_some(), "{p}");
    }
    for c in r["cut_list"].as_array().unwrap() {
        assert!(c["stock"].as_f64().is_some(), "{c}");
    }
    // Each splice over a full-height stud (none over the window, between its kings), the two top plates' 610 or more apart.
    let posts: Vec<f64> = ms.iter().filter(|m| m["role"] == "stud" || m["role"] == "king").map(centre).collect();
    let g = s.model.guid(win).to_string();
    let kings = [centre(keyed(&ms, &format!("{g}/king/left/0"))), centre(keyed(&ms, &format!("{g}/king/right/0")))];
    let ply = |prefix: &str| -> Vec<f64> {
        splices(&ms.iter().filter(|m| m["key"].as_str().is_some_and(|k| k.starts_with(prefix))).collect::<Vec<_>>())
    };
    let (bottom, upper, lower) = (ply("bottom_plate/"), ply("top_plate/0/"), ply("top_plate/1/"));
    assert_eq!((bottom.len(), upper.len(), lower.len()), (1, 1, 1), "{bottom:?} {upper:?} {lower:?}");
    for at in bottom.iter().chain(&upper).chain(&lower) {
        assert!(posts.iter().any(|c| (c - at).abs() < 1e-6), "a splice at {at}, over no stud: {posts:?}");
        assert!(*at <= kings[0] + 1e-6 || *at >= kings[1] - 1e-6, "a splice at {at} over the window");
    }
    assert!((upper[0] - lower[0]).abs() >= 610.0 - 1e-6, "{upper:?} {lower:?}");
}

#[test]
fn a_plate_that_cant_be_spliced_within_the_stock_over_a_stud_is_warned_of() {
    let mut s = Session::new();
    stud_wall_type(&mut s);
    // Studs 1220 apart, stock of 1000: no piece between two studs comes from it.
    run(&mut s, "type", json!({"category": "framing", "name": "Short", "spacing": 1220, "stock": [1000]}));
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]], "type": "2x4", "height": 2440})));
    run(&mut s, "frame", json!({"id": w, "type": "Short"}));
    let r = run(&mut s, "framing", json!({"id": w}));
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    for plate in ["bottom_plate/0/start/0", "top_plate/0/0", "top_plate/1/0"] {
        assert!(
            warnings.iter().any(|w| w.as_str().is_some_and(|t| t.contains(plate) && t.contains("longer than any stock"))),
            "{plate}: {warnings:?}"
        );
    }
}

/// Every member of walls `ids` against every other, by their boxes in the world: none may overlap another by more than a
/// hundredth of a mm in all three directions. For walls square to the axes with flat tops, where a box is the member.
fn no_overlaps(s: &mut Session, ids: &[u64]) {
    let all: Vec<(u64, String, [f64; 6])> =
        ids.iter().flat_map(|id| boxes(&run(s, "framing", json!({"id": id}))).into_iter().map(move |(k, b)| (*id, k, b))).collect();
    for (i, (wa, ka, x)) in all.iter().enumerate() {
        for (wb, kb, y) in &all[i + 1..] {
            let overlap = (0..3).all(|j| x[j + 3].min(y[j + 3]) - x[j].max(y[j]) > 1e-2);
            assert!(!overlap, "{ka} of wall {wa} and {kb} of wall {wb} overlap: {x:?} {y:?}");
        }
    }
}

#[test]
fn under_a_roof_sloping_across_the_wall_no_member_rises_above_the_walls_top_at_either_face() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"elevation": 0, "name": "L1"}));
    run(&mut s, "level", json!({"elevation": 2440, "name": "L2"}));
    stud_wall_type(&mut s);
    let r =
        run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "closed": true, "type": "2x4", "level": "L1"}));
    let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    let roof = first(&run(&mut s, "roof", json!({"level": "L2", "slope": 30, "gable": true, "overhang": 300})));
    run(&mut s, "modify", json!({"ids": ids, "attach_top": roof}));
    run(&mut s, "frame", json!({"ids": ids}));
    let mut worst = f64::NEG_INFINITY;
    for &id in &ids {
        let f = crate::framing::wall(&s.model, id).unwrap().unwrap();
        let w = s.model.wall(id).unwrap();
        for m in &f.members {
            let top = |at: f64| m.top[0] + (m.top[1] - m.top[0]) * (at - m.s[0]) / (m.s[1] - m.s[0]);
            for off in m.across {
                for p in crate::walls::profile(&s.model, w, true, off, m.s[0], m.s[1]) {
                    for (at, z) in [(p[0], p[2]), (p[1], p[3])] {
                        worst = worst.max(top(at) - z);
                        assert!(top(at) <= z + 1e-3, "wall {id}'s {} rises {:.1} mm over its top at across {off}", m.key, top(at) - z);
                    }
                }
            }
        }
    }
    assert!(worst > -1e-3, "the top plates reach the wall's top somewhere: {worst}");
}

#[test]
fn a_window_whose_rough_opening_starts_in_the_top_plates_is_left_unframed_with_a_warning() {
    let mut s = Session::new();
    stud_wall_type(&mut s);
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "2x4", "height": 2440})));
    let win = window_at(&mut s, w, 1200.0, 400.0, 2400.0, 2000.0);
    let g = s.model.guid(win).to_string();
    run(&mut s, "frame", json!({"id": w}));
    let r = run(&mut s, "framing", json!({"id": w}));
    let ms = r["members"].as_array().cloned().unwrap_or_default();
    assert!(!ms.iter().any(|m| m["key"].as_str().is_some_and(|k| k.starts_with(&g))), "none of its members");
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w.as_str().is_some_and(|t| t.contains("isn't framed") && t.contains("in the top plates"))),
        "{warnings:?}"
    );
    no_overlaps(&mut s, &[w]);
}

/// A 2x6 wall: 140 mm of timber between 13 mm of gypsum board each side.
fn clad_2x6(s: &mut Session) {
    let board = json!({"function": "finish", "material": "Gypsum board", "thickness": 13});
    let timber = json!({"function": "structure", "material": "Timber", "thickness": 140});
    run(s, "type", json!({"category": "wall", "name": "2x6", "layers": [board, timber, board]}));
}

#[test]
fn a_wall_butting_into_a_clad_wall_is_framed_to_its_structure_not_its_finish() {
    let mut s = Session::new();
    clad_2x6(&mut s);
    stud_wall_type(&mut s);
    let r = run(
        &mut s,
        "wall",
        json!({"points": [[0, 0], [0, 3000], [5000, 3000], [5000, 0]], "closed": true, "type": "2x6", "height": 2440,
               "location": "exterior-finish"}),
    );
    let outer: Vec<u64> = r["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    let p = first(&run(&mut s, "wall", json!({"points": [[2500, 3000], [2500, 0]], "type": "2x4", "height": 2440})));
    run(&mut s, "type", json!({"category": "framing", "name": "c", "layout": "centre", "blocking": [1200]}));
    run(&mut s, "frame", json!({"ids": outer, "type": "c"}));
    run(&mut s, "frame", json!({"id": p, "type": "c"}));
    // The host's structure is 13 to 153 mm in from its exterior face, on the location line: the partition's plates and end
    // studs go from 153 to 2847 along it.
    let ms = members(&mut s, p);
    let plate = role(&ms, "bottom_plate")[0];
    close(f(&plate["s"], 0), 153.0, "from the north wall's structure");
    close(f(&plate["s"], 1), 2847.0, "to the south wall's");
    close(f(&keyed(&ms, "stud/start")["s"], 0), 153.0, "the start stud against it");
    close(f(&keyed(&ms, "stud/end")["s"], 1), 2847.0, "the end stud against it");
    let mut ids = outer.clone();
    ids.push(p);
    no_overlaps(&mut s, &ids);
}

/// The members of walls `ids` whose boxes hold the point (x, y, z).
fn at_point(s: &mut Session, ids: &[u64], q: [f64; 3]) -> Vec<(u64, String)> {
    ids.iter()
        .flat_map(|id| {
            boxes(&run(s, "framing", json!({"id": id})))
                .into_iter()
                .filter(|(_, b)| (0..3).all(|i| b[i] < q[i] && q[i] < b[i + 3]))
                .map(|(k, _)| (*id, k))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn the_upper_top_plates_lap_at_corners_and_tees_with_no_two_members_overlapping() {
    let mut s = Session::new();
    stud_wall_type(&mut s);
    let r =
        run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true, "type": "2x4", "height": 2440}));
    let mut ids: Vec<u64> = r["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    let p = first(&run(&mut s, "wall", json!({"points": [[2000, 0], [2000, 3000]], "type": "2x4", "height": 2440})));
    ids.push(p);
    run(&mut s, "frame", json!({"ids": ids}));
    no_overlaps(&mut s, &ids);
    let (upper, lower) = (2440.0 - 19.0, 2440.0 - 57.0);
    // At each corner, in the square both structure layers share: one wall's upper plate and the other's lower one.
    for [x, y] in [[0.0, 0.0], [4000.0, 0.0], [4000.0, 3000.0], [0.0, 3000.0]] {
        let (u, l) = (at_point(&mut s, &ids, [x + 10.0, y + 10.0, upper]), at_point(&mut s, &ids, [x + 10.0, y + 10.0, lower]));
        assert_eq!((u.len(), l.len()), (1, 1), "at ({x}, {y}): {u:?} {l:?}");
        assert!(u[0].1.starts_with("top_plate/0/") && l[0].1.starts_with("top_plate/1/"), "{u:?} {l:?}");
        assert_ne!(u[0].0, l[0].0, "at ({x}, {y}) the upper plate laps the other wall's lower one: {u:?} {l:?}");
    }
    // Where the partition meets each side, its upper plate runs over the side's lower one, the side's upper one cut round it.
    for y in [10.0, 3000.0 - 10.0] {
        let u = at_point(&mut s, &ids, [2000.0, y, upper]);
        assert_eq!(u.len(), 1, "{u:?}");
        assert_eq!(u[0].0, p, "the partition's upper plate over the wall it meets: {u:?}");
        let l = at_point(&mut s, &ids, [2000.0, y, lower]);
        assert!(l.len() == 1 && l[0].0 != p, "{l:?}");
        // And either side of it, the side's own upper plate goes on.
        for x in [2000.0 - 60.0, 2000.0 + 60.0] {
            let u = at_point(&mut s, &ids, [x, y, upper]);
            assert!(u.len() == 1 && u[0].0 != p, "{u:?}");
        }
    }
    // The partition's other members stop at the side's structure face.
    let ms = members(&mut s, p);
    close(f(&role(&ms, "bottom_plate")[0]["s"], 0), 44.5, "bottom plate from the south wall's face");
    close(f(&keyed(&ms, "top_plate/1/0")["s"], 0), 44.5, "the lower top plate too");
    close(f(&keyed(&ms, "top_plate/0/0")["s"], 0), -44.5, "the upper one over the south wall");
}

#[test]
fn a_three_stud_corners_extra_stud_backs_the_inside_corner_of_a_deeper_wall() {
    let mut s = Session::new();
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "2x6", "layers": [{"function": "structure", "material": "Timber", "thickness": 140}]}),
    );
    let r = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000]], "type": "2x6", "height": 2440}));
    let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    run(&mut s, "frame", json!({"ids": ids}));
    no_overlaps(&mut s, &ids);
    // The inside corner of the two structure layers is at (3930, 70): the extra stud's face is on it.
    let all: Vec<(String, [f64; 6])> = ids.iter().flat_map(|id| boxes(&run(&mut s, "framing", json!({"id": id})))).collect();
    let corner: Vec<&[f64; 6]> = all.iter().filter(|(k, _)| k.starts_with("corner/")).map(|(_, b)| b).collect();
    assert_eq!(corner.len(), 1, "{all:?}");
    let b = corner[0];
    let on = |lo: f64, hi: f64, v: f64| lo - 1e-2 <= v && v <= hi + 1e-2;
    assert!(on(b[0], b[3], 3930.0) && on(b[1], b[4], 70.0), "the corner stud {b:?} backs (3930, 70)");
    assert!((b[0] - 3930.0).abs() < 1e-2 || (b[4] - 70.0).abs() < 1e-2, "flush with the inside face: {b:?}");
}

#[test]
fn no_grid_stud_stands_between_a_tees_backing_studs_and_no_blocking_is_shorter_than_a_stud_is_thick() {
    let mut s = Session::new();
    stud_wall_type(&mut s);
    run(&mut s, "type", json!({"category": "framing", "name": "Blocked", "blocking": [1200]}));
    let w = wall(&mut s, [0.0, 0.0], [4000.0, 0.0]);
    // The partition centred on the grid's fifth station, 4 x 406.4 along.
    let p = wall(&mut s, [1625.6, 0.0], [1625.6, 2000.0]);
    run(&mut s, "frame", json!({"ids": [w, p], "type": "Blocked"}));
    let ms = members(&mut s, w);
    let pg = s.model.guid(p).to_string();
    keyed(&ms, &format!("tee/{pg}/0"));
    keyed(&ms, &format!("tee/{pg}/1"));
    assert!(!ms.iter().any(|m| m["key"] == "stud/4"), "no grid stud between the backing studs");
    for b in role(&ms, "blocking") {
        assert!(b["length"].as_f64().unwrap_or_default() >= 38.0 - 1e-6, "a sliver of blocking: {b}");
    }
    no_overlaps(&mut s, &[w, p]);
}

#[test]
fn a_members_stock_length_is_chosen_from_its_length_unrounded_in_the_reply_the_cut_list_and_the_schedule() {
    let mut s = Session::new();
    stud_wall_type(&mut s);
    // Studs 2554.3 - 38 - 76 = 2440.3 long: more than 2440, so from 3050.
    let w = first(&run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]], "type": "2x4", "height": 2554.3})));
    run(&mut s, "frame", json!({"id": w}));
    let r = run(&mut s, "framing", json!({"id": w}));
    let stud = r["members"].as_array().unwrap().iter().find(|m| m["role"] == "stud").cloned().unwrap();
    assert_eq!(stud["stock"], json!(3050.0), "{stud}");
    let cut = r["cut_list"].as_array().unwrap().iter().find(|c| c["role"] == "stud").cloned().unwrap();
    assert_eq!((cut["length"].as_f64(), cut["stock"].as_f64()), (Some(2440.0), Some(3050.0)), "{cut}");
    let t = run(&mut s, "schedule", json!({"name": "FM", "category": "framingmembers", "fields": ["Role", "Length", "Stock Length"]}));
    let rows = t["rows"].as_array().cloned().unwrap_or_default();
    let studs: Vec<&Value> = rows.iter().filter(|r| r[0] == "stud").collect();
    assert!(!studs.is_empty() && studs.iter().all(|r| r[2] == json!(3050.0)), "{rows:?}");
}

#[test]
fn a_shortened_flat_header_is_reported_at_its_real_height_and_a_shortened_header_is_scheduled_to_the_mm() {
    let mut s = Session::new();
    let w = cabin(&mut s);
    // Rough top 1098.5433 + 1200 + 13 = 2311.5433, 60.4567 under the top plates (2372): one flat 2x of 38 fits, not two.
    run(
        &mut s,
        "type",
        json!({"category": "framing", "name": "Flat", "spacing": 1220.0 / 3.0, "headers": [{"depth": 38, "plies": 2, "flat": true}]}),
    );
    let win = window_at(&mut s, w, 1200.0, 1200.0, 1098.5433, 1250.0);
    run(&mut s, "frame", json!({"id": w, "type": "Flat"}));
    let r = run(&mut s, "framing", json!({"id": w}));
    let g = s.model.guid(win).to_string();
    let ms = r["members"].as_array().cloned().unwrap_or_default();
    let header = keyed(&ms, &format!("{g}/header"));
    close(f(&header["z"], 2) - f(&header["z"], 0), 38.0, "one ply");
    let warnings = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|t| t.contains("cut to 38 mm"))), "{warnings:?}");
    // Two windows whose 2x8 headers are cut to 153.46 and 153.2: one row of the schedule, 2 x 38 x 153.
    run(&mut s, "frame", json!({"id": w, "type": "Cabin"}));
    run(&mut s, "delete", json!({"ids": [win]}));
    window_at(&mut s, w, 1200.0, 1200.0, 1005.5433, 1250.0);
    window_at(&mut s, w, 1200.0, 1200.0, 1005.8, 4500.0);
    let t = run(
        &mut s,
        "schedule",
        json!({"name": "Headers", "category": "framingmembers", "fields": ["Role", "Section", "Length", "Count"],
               "sort": ["Role", "Section", "Length"], "grouped": true,
               "filters": [{"field": "Role", "op": "equals", "value": "header"}]}),
    );
    let rows = t["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0][1], json!("2 x 38 x 153"), "{rows:?}");
    assert_eq!(rows[0][3], json!(2.0), "{rows:?}");
}
