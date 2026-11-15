//! Wall lines: the seed's choice and the pins, through the script, the DXF and the PNG they make.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::wall_lines::{Pins, choose};
use super::{Interior, Request, build_styled};
use crate::engine::Session;

#[test]
fn the_seed_chooses_inside_the_ranges_with_the_exterior_never_thinner() {
    for seed in 0..200 {
        let w = choose(seed, &Pins::default());
        assert!((8..=14).contains(&w.exterior.pen) && (3..=9).contains(&w.interior.pen), "{seed}: {w:?}");
        assert!((0..=45).contains(&w.exterior.lightness) && (0..=65).contains(&w.interior.lightness), "{seed}: {w:?}");
        assert!(w.exterior.pen >= w.interior.pen, "{seed}: {w:?}");
        assert!(w.exterior.dxf_pair() != w.interior.dxf_pair(), "{seed}: the DXF could not tell them apart: {w:?}");
        assert_eq!(choose(seed, &Pins::default()), w, "the same seed, the same lines");
    }
}

#[test]
fn forty_seeds_show_the_spread() {
    let all: Vec<_> = (0..40).map(|s| choose(s, &Pins::default())).collect();
    let n = |f: &dyn Fn(&super::wall_lines::WallLines) -> i64| all.iter().map(f).collect::<BTreeSet<_>>().len();
    assert!(n(&|w| w.exterior.pen) >= 5);
    assert!(n(&|w| w.interior.pen) >= 5);
    assert!(n(&|w| w.exterior.lightness) >= 8);
    assert!(n(&|w| w.interior.lightness) >= 8);
    let fills: BTreeSet<_> = all.iter().flat_map(|w| [w.exterior.fill, w.interior.fill]).collect();
    assert_eq!(fills.len(), 2, "both fills");
}

#[test]
fn one_pinned_pen_leaves_the_order_to_the_other() {
    for seed in 0..100 {
        for pen in 8..=14 {
            let w = choose(seed, &Pins { ext_pen: Some(pen), ..Pins::default() });
            assert_eq!(w.exterior.pen, pen);
            assert!(w.interior.pen <= pen && w.interior.pen >= 3, "{seed} {pen}: {w:?}");
        }
        for pen in 3..=9 {
            let w = choose(seed, &Pins { int_pen: Some(pen), ..Pins::default() });
            assert_eq!(w.interior.pen, pen);
            assert!(w.exterior.pen >= pen && w.exterior.pen >= 8, "{seed} {pen}: {w:?}");
        }
    }
}

#[test]
fn all_four_pinned_are_drawn_exactly() {
    let pins = Pins { ext_pen: Some(12), ext_light: Some(20), int_pen: Some(5), int_light: Some(60) };
    for seed in 0..20 {
        let m = choose(seed, &pins).manifest_json();
        assert_eq!((m["exterior"]["pen"].clone(), m["exterior"]["lineweight_mm"].clone()), (json!(12), json!(0.5)));
        assert_eq!((m["exterior"]["lightness"].clone(), m["exterior"]["color"].clone()), (json!(20), json!("#333333")));
        assert_eq!((m["interior"]["pen"].clone(), m["interior"]["lineweight_mm"].clone()), (json!(5), json!(0.2)));
        assert_eq!((m["interior"]["lightness"].clone(), m["interior"]["color"].clone()), (json!(60), json!("#999999")));
    }
}

#[test]
fn lightness_is_the_grey_rounded() {
    for (l, hex) in [(0, "#000000"), (10, "#1a1a1a"), (20, "#333333"), (45, "#737373"), (60, "#999999"), (65, "#a6a6a6")] {
        let w = choose(1, &Pins { ext_light: Some(l.min(45)), int_light: Some(l), ..Pins::default() });
        assert_eq!(w.interior.color(), hex);
    }
}

#[test]
fn pins_out_of_range_or_out_of_order_are_refused_with_what_to_fix() {
    for (pins, word) in [
        (Pins { ext_pen: Some(20), ..Pins::default() }, "ext_pen"),
        (Pins { ext_pen: Some(7), ..Pins::default() }, "ext_pen"),
        (Pins { int_light: Some(90), ..Pins::default() }, "int_light"),
        (Pins { int_light: Some(-1), ..Pins::default() }, "int_light"),
        (Pins { ext_light: Some(46), ..Pins::default() }, "ext_light"),
        (Pins { int_pen: Some(10), ..Pins::default() }, "int_pen"),
        (Pins { ext_pen: Some(8), int_pen: Some(9), ..Pins::default() }, "thinner"),
    ] {
        let e = pins.check().expect_err("refused");
        assert!(e.contains(word), "{e}");
    }
    assert!(Pins { ext_pen: Some(9), int_pen: Some(9), ..Pins::default() }.check().is_ok());
}

fn request(seed: u64) -> Request {
    Request {
        archetype: "house".into(),
        seed,
        rooms: 8,
        tolerance: 0,
        shape: None,
        gridlines: false,
        interior: Interior::Jogged,
        bumps: false,
    }
}

fn files(seed: u64, pins: &Pins) -> Vec<(&'static str, String)> {
    build_styled(&request(seed), pins, false).expect("builds").files
}

fn named<'a>(files: &'a [(&'static str, String)], name: &str) -> &'a str {
    files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.as_str()).unwrap_or("")
}

const PIN: Pins = Pins { ext_pen: Some(12), ext_light: Some(20), int_pen: Some(5), int_light: Some(60) };

#[test]
fn a_style_option_changes_the_script_only_at_its_end_and_nothing_else() {
    let (a, b) = (files(4, &Pins::default()), files(4, &PIN));
    assert_eq!(named(&a, "topology-graph.json"), named(&b, "topology-graph.json"));
    assert_eq!(named(&a, "program-graph.json"), named(&b, "program-graph.json"));
    let (ma, mb): (Value, Value) =
        (serde_json::from_str(named(&a, "manifest.json")).expect("json"), serde_json::from_str(named(&b, "manifest.json")).expect("json"));
    assert_eq!(ma["building"], mb["building"]);
    assert_eq!(mb["wall_lines"]["exterior"]["color"], "#333333");
    let (sa, sb): (Vec<_>, Vec<_>) =
        (named(&a, "script.bimcraft.txt").lines().collect(), named(&b, "script.bimcraft.txt").lines().collect());
    assert_eq!(sa.len(), sb.len());
    let diff: Vec<usize> = (0..sa.len()).filter(|i| sa[*i] != sb[*i]).collect();
    assert!(!diff.is_empty() && diff.iter().all(|i| *i >= sa.len() - 3), "{diff:?} of {}", sa.len());
    assert!(sb[sb.len() - 3].starts_with("filter ") && sb[sb.len() - 1].starts_with("graphics "));
}

fn scratch(test: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-wall-lines-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

fn session_of(files: &[(&'static str, String)]) -> Session {
    let mut s = Session::new();
    s.script(named(files, "script.bimcraft.txt")).expect("the script runs");
    s
}

#[test]
fn pinned_lines_reach_the_dxf_on_the_right_walls() {
    let f = files(4, &PIN);
    let mut s = session_of(&f);
    let d = scratch("dxf");
    let path = d.join("plan.dxf");
    s.execute("save", &json!({"path": path})).expect("saved");
    let text = std::fs::read_to_string(&path).expect("dxf");
    let _ = std::fs::remove_dir_all(&d);
    // Each A-WALL LWPOLYLINE: layer 8, lineweight 370, true colour 420.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut pairs = BTreeSet::new();
    let mut at = 0;
    while at + 1 < lines.len() {
        if lines[at] == "0" && lines[at + 1] == "LWPOLYLINE" {
            let end = (at + 2..lines.len().saturating_sub(1)).find(|i| lines[*i] == "0").unwrap_or(lines.len() - 1);
            let group = |code: &str| (at..end).step_by(1).find(|i| lines[*i] == code && (i - at) % 2 == 0).map(|i| lines[i + 1]);
            if group("8") == Some("A-WALL") {
                pairs.insert((group("370").unwrap_or("?").to_string(), group("420").unwrap_or("?").to_string()));
            }
            at = end;
        } else {
            at += 1;
        }
    }
    let want: BTreeSet<(String, String)> =
        [("50".to_string(), 0x333333.to_string()), ("20".to_string(), 0x999999.to_string())].into_iter().collect();
    assert_eq!(pairs, want);
}

/// Pixels darker than quarter grey. `walls_only` leaves out the plan's doors, windows, contents and room tags, which are black
/// whatever the wall lines are.
fn dark(f: &[(&'static str, String)], test: &str, walls_only: bool) -> usize {
    let mut s = if walls_only {
        let text: String = named(f, "script.bimcraft.txt")
            .lines()
            .filter(|l| !["door ", "window ", "component ", "room "].iter().any(|p| l.starts_with(p)))
            .map(|l| format!("{l}\n"))
            .collect();
        let mut s = Session::new();
        s.script(&text).expect("the script runs");
        s
    } else {
        session_of(f)
    };
    let d = scratch(test);
    let path = d.join("plan.png");
    s.execute("save", &json!({"path": path})).expect("saved");
    let bytes = std::fs::read(&path).expect("png");
    let _ = std::fs::remove_dir_all(&d);
    let img = tiny_skia::Pixmap::decode_png(&bytes).expect("decodes");
    img.pixels().iter().filter(|p| (299 * u32::from(p.red()) + 587 * u32::from(p.green()) + 114 * u32::from(p.blue())) / 1000 < 64).count()
}

#[test]
fn the_png_shows_the_pen_and_the_lightness() {
    let heavy = Pins { ext_pen: Some(14), int_pen: Some(9), ext_light: Some(0), int_light: Some(0) };
    let light = Pins { ext_pen: Some(8), int_pen: Some(8), ext_light: Some(0), int_light: Some(0) };
    let faded = Pins { ext_pen: Some(14), int_pen: Some(9), ext_light: Some(45), int_light: Some(45) };
    let (h, l) = (dark(&files(4, &heavy), "h", false), dark(&files(4, &light), "l", false));
    assert!(h >= 2 * l, "heavy {h}, light {l}");
    // The room tags, doors and furniture stay black at any lightness, and are about a tenth of a heavy plan's dark pixels
    // themselves, so the faded plan is measured on its walls.
    let (h, f) = (dark(&files(4, &heavy), "hw", true), dark(&files(4, &faded), "fw", true));
    assert!(f * 10 < h, "faded {f}, heavy {h}");
}

fn refused(params: Value) -> String {
    let d = scratch("refuse");
    let mut p = json!({"archetype": "house", "seed": 4, "rooms": 8, "path": d.join("out")});
    for (k, v) in params.as_object().into_iter().flatten() {
        p[k] = v.clone();
    }
    let r = Session::new().execute("generate", &p);
    let wrote = d.join("out").exists();
    let _ = std::fs::remove_dir_all(&d);
    assert!(!wrote, "a refusal writes nothing");
    r.expect_err("refused")
}

#[test]
fn generate_refuses_bad_pins_before_writing_anything() {
    for p in [json!({"ext_pen": 20}), json!({"int_light": 90}), json!({"ext_pen": 8, "int_pen": 9}), json!({"int_pen": -2})] {
        let e = refused(p.clone());
        assert!(e.contains("invalid_params"), "{p}: {e}");
    }
}

#[test]
fn style_options_belong_to_the_plan_by_rooms() {
    let d = scratch("area");
    let r =
        Session::new().execute("generate", &json!({"archetype": "house", "seed": 1, "area_m2": 115.0, "ext_pen": 9, "path": d.join("o")}));
    let _ = std::fs::remove_dir_all(&d);
    assert!(r.expect_err("refused").contains("ext_pen"));
}
