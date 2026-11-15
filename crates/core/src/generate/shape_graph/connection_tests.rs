//! The nine connection types: each is drawn, read back as its row of the table, and a door stands where it likes along its wall.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::family::{Conn, Family, HOUSE_FAMILY, shapes};
use super::{Request, build, build_family};
use crate::engine::Session;
use crate::topology;

const TOY: &str = include_str!("testdata/toy-family.json");
const NINE: [&str; 9] = ["open_full", "cased", "single", "double", "unequal_double", "sliding", "double_sliding", "pocket", "bifold"];

fn graph(files: &[(&'static str, String)]) -> Value {
    let text = files.iter().find(|(n, _)| *n == "topology-graph.json").map(|(_, t)| t.as_str()).unwrap_or("null");
    serde_json::from_str(text).unwrap_or(Value::Null)
}

fn script(files: &[(&'static str, String)]) -> String {
    files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).unwrap_or_default()
}

fn count(g: &Value, into: &mut BTreeMap<String, usize>) {
    for e in g["edges"].as_array().into_iter().flatten() {
        if let Some(c) = e["connection"].as_str() {
            *into.entry(c.to_string()).or_default() += 1;
        }
    }
}

fn house(seed: u64, rooms: i64) -> crate::generate::Built {
    let req = Request { archetype: "house".into(), seed, rooms, tolerance: 0, shape: None, gridlines: false };
    build(&req, false).unwrap_or_else(|r| panic!("house {seed}/{rooms}: {r}"))
}

#[test]
fn a_family_naming_all_nine_types_draws_each_of_them() {
    let ids: Vec<String> = shapes().expect("the catalogue reads").into_iter().map(|s| s.id).collect();
    let mut v: Value = serde_json::from_str(TOY).expect("the toy family is JSON");
    for l in v["links"].as_array_mut().into_iter().flatten() {
        l["types"] = json!(NINE);
    }
    let fam = Family::parse(&v.to_string()).expect("a family may name every type");
    let mut seen = BTreeMap::new();
    for k in 0..12u64 {
        let req = Request {
            archetype: String::new(),
            seed: k + 1,
            rooms: 7 + (k % 12) as i64,
            tolerance: 0,
            shape: Some(ids[(k as usize) % ids.len()].clone()),
            gridlines: false,
        };
        let built = build_family(&fam, &req, false).unwrap_or_else(|r| panic!("brief {k}: {r}"));
        count(&graph(&built.files), &mut seen);
    }
    for t in NINE {
        assert!(seen.get(t).is_some_and(|n| *n > 0), "{t} never drawn: {seen:?}");
    }
}

#[test]
fn the_entry_must_be_a_door_type() {
    let bad = HOUSE_FAMILY.replace("\"doors\": [1, 1], \"types\": [\"single\"]", "\"doors\": [1, 1], \"types\": [\"cased\"]");
    assert!(Family::parse(&bad).is_err_and(|e| e.contains("entry.types")));
    let ok = HOUSE_FAMILY.replace("\"doors\": [1, 1], \"types\": [\"single\"]", "\"doors\": [1, 1], \"types\": [\"double\"]");
    assert!(Family::parse(&ok).is_ok());
}

#[test]
fn the_new_types_have_their_door_rows() {
    assert_eq!(Conn::UnequalDouble.door(), Some(("Unequal Double Door", "double-swing")));
    assert_eq!(Conn::DoubleSliding.door(), Some(("Double Sliding Door", "double-sliding")));
    assert_eq!(Conn::Pocket.door(), Some(("Pocket Door", "sliding")));
    assert_eq!(Conn::Cased.door(), None);
}

#[test]
fn house_plans_draw_the_new_types_and_read_them_back() {
    let mut seen = BTreeMap::new();
    for seed in 1..=20u64 {
        let built = house(seed, 8 + (seed as i64 - 1) % 15);
        count(&graph(&built.files), &mut seen);
        // The script runs, and the extractor finds each door as the table says.
        let mut s = Session::new();
        let replies = s.script(&script(&built.files)).expect("the script runs");
        assert!(!replies.is_empty());
        let ext = topology::extract(&s.model);
        for e in ext["edges"].as_array().into_iter().flatten() {
            let (op, fam) = (e["operation"].as_str(), e["family"].as_str());
            match fam {
                Some("Unequal Double Door") => assert_eq!(op, Some("double-swing")),
                Some("Double Sliding Door") => assert_eq!(op, Some("double-sliding")),
                Some("Pocket Door") => assert_eq!(op, Some("sliding")),
                _ => {}
            }
        }
    }
    for t in ["cased", "double_sliding", "pocket"] {
        assert!(seen.get(t).is_some_and(|n| *n > 0), "{t} never drawn in 20 houses: {seen:?}");
    }
    assert!(!seen.contains_key("unequal_double"), "the house family does not use unequal_double: {seen:?}");
}

#[test]
fn a_cased_opening_is_a_separation_line_across_a_gap_in_the_wall() {
    for seed in 1..=30u64 {
        let built = house(seed, 12);
        let g = graph(&built.files);
        let cased = g["edges"].as_array().into_iter().flatten().filter(|e| e["connection"] == "cased").count();
        if cased == 0 {
            continue;
        }
        let text = script(&built.files);
        let seps: Vec<f64> = text
            .lines()
            .filter_map(|l| l.strip_prefix("room_separator "))
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .map(|v| {
                let p = |i: usize, k: usize| v["points"][i][k].as_f64().unwrap_or(0.0);
                (p(1, 0) - p(0, 0)).abs() + (p(1, 1) - p(0, 1)).abs()
            })
            .collect();
        assert!(seps.iter().any(|l| (900.0..=2400.0).contains(l)), "seed {seed}: separators {seps:?}");
        return;
    }
    panic!("no cased link in thirty houses");
}

#[test]
fn doors_do_not_all_stand_in_the_middle() {
    let built = house(5, 14);
    let mut mids = 0;
    let mut off = 0;
    let text = script(&built.files);
    let walls: Vec<([f64; 2], [f64; 2])> = text
        .lines()
        .filter_map(|l| l.strip_prefix("wall "))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| {
            let p = |i: usize, k: usize| v["points"][i][k].as_f64().unwrap_or(0.0);
            ([p(0, 0), p(0, 1)], [p(1, 0), p(1, 1)])
        })
        .collect();
    for l in text.lines().filter_map(|l| l.strip_prefix("door ")) {
        let v: Value = serde_json::from_str(l).expect("a door line is JSON");
        let at = [v["at"][0].as_f64().unwrap_or(0.0), v["at"][1].as_f64().unwrap_or(0.0)];
        for (a, b) in &walls {
            let on = (a[1] == b[1] && at[1] == a[1] && at[0] >= a[0].min(b[0]) && at[0] <= a[0].max(b[0]))
                || (a[0] == b[0] && at[0] == a[0] && at[1] >= a[1].min(b[1]) && at[1] <= a[1].max(b[1]));
            if on {
                let (mid, u) = if a[1] == b[1] { ((a[0] + b[0]) / 2.0, at[0]) } else { ((a[1] + b[1]) / 2.0, at[1]) };
                if (u - mid).abs() < 1.0 {
                    mids += 1;
                } else {
                    off += 1;
                }
                break;
            }
        }
    }
    assert!(off > 0, "{mids} doors in the middle, {off} off it");
}
