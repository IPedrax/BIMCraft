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
    let (bottom, top) = (role(&ms, "bottom_plate"), role(&ms, "top_plate"));
    assert_eq!((bottom.len(), top.len()), (1, 2));
    for p in bottom.iter().chain(&top) {
        assert_eq!(p["section"], json!([38.0, 89.0]));
        close(p["length"].as_f64().unwrap_or_default(), 7320.0, "plate");
        assert_eq!(p["stock"], Value::Null, "longer than any stock length: spliced");
    }
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
    assert!(ms.iter().any(|m| m["key"].as_str().is_some_and(|k| k.starts_with(&format!("{g}/cripple/top/")))));
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
    assert_eq!(tops.len(), 2);
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
