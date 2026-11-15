//! `generate` with `family_file`: the family read from a file in place of a built-in one.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::shape_graph::family::Family;
use super::shape_graph::register_tests::house_family;
use super::*;
use crate::engine::Session;
use crate::graph_rules::Pack;

const TOY: &str = include_str!("testdata/shape-graph/toy-family.json");
const BAD_RANGE: &str = include_str!("testdata/shape-graph/toy-family-bad-range.json");

fn dir(test: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bimcraft-family-file-{}-{test}", std::process::id()))
}

fn put(test: &str, name: &str, text: &str) -> PathBuf {
    let d = dir(test);
    std::fs::create_dir_all(&d).expect("temp dir");
    let at = d.join(name);
    std::fs::write(&at, text).expect("temp file");
    at
}

fn run_json(p: Value) -> Result<Value, String> {
    generate(&mut Session::new(), &p)
}

fn files(out: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(out)
        .map(|r| {
            r.flatten()
                .map(|e| (e.file_name().to_string_lossy().to_string(), std::fs::read_to_string(e.path()).unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn a_house_by_file_is_the_house_by_name_byte_for_byte() {
    let fam = put("house", "house.json", house_family());
    for (seed, rooms, tol, shape) in [(1u64, 9i64, 0i64, Some("court")), (2, 12, 0, None), (5, 14, 2, None), (7, 8, 0, Some("narrow_bar"))]
    {
        let (a, b) = (dir(&format!("house-a{seed}")), dir(&format!("house-b{seed}")));
        let mut p = json!({"seed": seed, "rooms": rooms, "rooms_tol": tol});
        if let Some(s) = shape {
            p["shape"] = json!(s);
        }
        let mut by_name = p.clone();
        by_name["path"] = json!(a);
        by_name["archetype"] = json!("house");
        let mut by_file = p;
        by_file["path"] = json!(b);
        by_file["family_file"] = json!(fam);
        let (x, y) = (run_json(by_name), run_json(by_file));
        assert_eq!(x.is_ok(), y.is_ok(), "{seed}: {x:?} {y:?}");
        if x.is_ok() {
            let (fa, fb) = (files(&a), files(&b));
            assert_eq!(fa.len(), 6);
            assert_eq!(fa, fb, "seed {seed}");
        }
    }
}

#[test]
fn a_family_the_code_has_never_seen_generates_from_its_file() {
    let fam = put("toy", "toy.json", TOY);
    let out = dir("toy-out");
    let r = run_json(json!({"path": out, "family_file": fam, "seed": 4, "rooms": 12})).expect("generates");
    assert_eq!(r["archetype"], "workshop");
    let written = files(&out);
    assert_eq!(written.len(), 6);
    let get = |n: &str| written.iter().find(|(f, _)| f == n).map(|(_, t)| t.clone()).unwrap_or_default();
    let manifest: Value = serde_json::from_str(&get("manifest.json")).expect("manifest");
    assert_eq!(manifest["family"], "workshop");
    assert!(get("script.bimcraft.txt").contains("building_type"));
    assert!(get("script.bimcraft.txt").contains("Bench Room 1"));
    let report: Value = serde_json::from_str(&get("report.json")).expect("report");
    assert_eq!(report["archetype"], "workshop");
    for (name, text) in &written {
        assert!(!text.contains("toy.json"), "{name} holds the file's name");
        assert!(!text.contains(&dir("toy").to_string_lossy().to_string()), "{name} holds the file's path");
    }
}

fn refused(p: Value, code: &str) {
    let out = p["path"].as_str().map(PathBuf::from);
    let e = run_json(p).expect_err("refused");
    assert!(e.contains(code), "{code}: {e}");
    if let Some(out) = out {
        assert!(!out.exists(), "wrote {}", out.display());
    }
}

#[test]
fn a_family_file_that_is_wrong_is_refused_and_nothing_is_written() {
    let bad = put("bad", "bad.json", BAD_RANGE);
    refused(json!({"path": dir("bad-out"), "family_file": bad, "seed": 1, "rooms": 9}), "invalid_family");

    // The house with a room type's area wider than its pack's.
    let pack = Pack::embedded("house").expect("pack");
    let mut v: Value = serde_json::from_str(house_family()).expect("json");
    let fam = Family::parse(house_family()).expect("family");
    let mut widened = false;
    for t in &fam.types {
        if let Some((_, Some(hi))) = pack.kinds.get(&t.kind).map(|k| k.area_m2) {
            let key = v["rooms"].as_object().and_then(|m| m.iter().find(|(_, r)| r["kind"] == t.kind.as_str()).map(|(k, _)| k.clone()));
            if let Some(key) = key {
                v["rooms"][&key]["area_m2"][1] = json!(hi + 5.0);
                widened = true;
                break;
            }
        }
    }
    assert!(widened);
    let wide = put("wide", "wide.json", &v.to_string());
    refused(json!({"path": dir("wide-out"), "family_file": wide, "seed": 1, "rooms": 9}), "invalid_family");
    let same = put("same", "same.json", house_family());
    assert!(run_json(json!({"path": dir("same-out"), "family_file": same, "seed": 1, "rooms": 9})).is_ok());
}

#[test]
fn archetype_and_family_file_are_asked_for_once() {
    let fam = put("once", "house.json", house_family());
    refused(json!({"path": dir("both"), "archetype": "house", "family_file": fam, "seed": 1, "rooms": 9}), "invalid_params");
    refused(json!({"path": dir("neither"), "seed": 1, "rooms": 9}), "invalid_params");
    refused(json!({"path": dir("norooms"), "family_file": fam, "seed": 1, "area_m2": 300.0}), "invalid_params");
    refused(json!({"path": dir("five"), "family_file": 5, "seed": 1, "rooms": 9}), "invalid_params");
}

#[test]
fn a_family_file_that_cannot_be_read_is_refused() {
    let d = dir("unreadable");
    std::fs::create_dir_all(&d).expect("dir");
    for (i, f) in ["/nonexistent/f.json", "/dev/null"].iter().enumerate() {
        refused(json!({"path": dir(&format!("unread-out{i}")), "family_file": f, "seed": 1, "rooms": 9}), "invalid_");
    }
    refused(json!({"path": dir("unread-dir"), "family_file": d, "seed": 1, "rooms": 9}), "invalid_params");
    refused(json!({"path": dir("unread-nul"), "family_file": "a\0b", "seed": 1, "rooms": 9}), "invalid_params");
    refused(json!({"path": dir("unread-empty"), "family_file": "", "seed": 1, "rooms": 9}), "invalid_params");
}

#[test]
fn a_renamed_family_makes_the_same_plan_under_its_new_names() {
    let mut renamed = TOY.to_string();
    for (from, to) in [
        ("foyer", "wicket"),
        ("lane", "arcade"),
        ("bench_room", "atelier"),
        ("tool_store", "cupboard"),
        ("wash", "basin"),
        ("assembly", "zenith"),
    ] {
        renamed = renamed.replace(&format!("\"{from}\""), &format!("\"{to}\""));
    }
    let (a, b) = (put("ren-a", "a.json", TOY), put("ren-b", "b.json", &renamed));
    for seed in 1..6u64 {
        let (oa, ob) = (dir(&format!("ren-oa{seed}")), dir(&format!("ren-ob{seed}")));
        let ra = run_json(json!({"path": oa, "family_file": a, "seed": seed, "rooms": 10 + seed}));
        let rb = run_json(json!({"path": ob, "family_file": b, "seed": seed, "rooms": 10 + seed}));
        let (ra, rb) = (ra.expect("a"), rb.expect("b"));
        assert_eq!(ra["gross_area_m2"], rb["gross_area_m2"], "seed {seed}");
        let m = |o: &Path| {
            let text = std::fs::read_to_string(o.join("manifest.json")).unwrap_or_default();
            serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)
        };
        let (ma, mb) = (m(&oa), m(&ob));
        for k in ["shape", "mirror", "quarter_turns", "rooms", "net_area_m2", "gross_area_m2"] {
            assert_eq!(ma[k], mb[k], "seed {seed} {k}");
        }
    }
}
