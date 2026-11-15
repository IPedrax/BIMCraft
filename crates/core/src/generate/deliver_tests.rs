//! `deliver` through `Session::execute`, with a fake DWG writer (the DXF text as the file's bytes).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::deliver::{rerun_indices, rooms_for};
use crate::engine::Session;

const GV: &str = "bimcraft-0.0.0+abc1234";

fn dir(test: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-deliver-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn session() -> Session {
    let mut s = Session::new();
    s.dwg_writer = Some(Arc::new(|dxf, _| Ok(dxf.as_bytes().to_vec())));
    s.generator_version = Some(GV.into());
    s
}

fn put(d: &Path, name: &str, bytes: &[u8]) -> String {
    let p = d.join(name);
    std::fs::write(&p, bytes).expect("write");
    p.display().to_string()
}

const RIGHTS: &[u8] = "line one\r\nnon-ascii \u{2014} here, no final newline".as_bytes();

fn brief3(d: &Path) -> String {
    put(
        d,
        "brief.json",
        br#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 4, "rooms": [8, 12], "rooms_tol": 2}]}"#,
    )
}

fn params(d: &Path, brief: &str, id: &str, rights: Option<&str>, root: &Path) -> Value {
    let mut p = json!({"brief": brief, "delivery_id": id, "path": root.display().to_string(), "rerun": 2, "jobs": 2});
    if let Some(r) = rights {
        p["rights"] = json!(r);
    }
    let _ = d;
    p
}

/// Every path under `root` with its size and bytes' length, modified time.
fn tree(root: &Path) -> BTreeMap<String, (bool, u64, std::time::SystemTime, Vec<u8>)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("read_dir").flatten() {
            let p = e.path();
            let md = std::fs::symlink_metadata(&p).expect("md");
            let rel = p.strip_prefix(root).expect("rel").display().to_string();
            if md.is_dir() {
                stack.push(p.clone());
                out.insert(rel, (true, 0, md.modified().expect("mtime"), Vec::new()));
            } else {
                out.insert(rel, (false, md.len(), md.modified().expect("mtime"), std::fs::read(&p).expect("read")));
            }
        }
    }
    out
}

#[test]
fn a_delivery_writes_the_folder_and_nothing_else() {
    let d = dir("layout");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let brief = brief3(&d);
    let mut s = session();
    let reply = s.execute("deliver", &params(&d, &brief, "t1", Some(&rights), &root)).expect("deliver");
    assert_eq!(reply["generator_version"], GV);
    assert_eq!(reply["files"], 4);
    assert!(reply["skipped"].as_array().is_some_and(Vec::is_empty));
    let gv = root.join(GV);
    assert_eq!(std::fs::read_dir(&root).expect("ls").count(), 1);
    let mut inner: Vec<String> = std::fs::read_dir(&gv).expect("ls").flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
    inner.sort();
    assert_eq!(inner, ["t1", "t1-rerun"]);
    for (folder, n) in [("t1", 4), ("t1-rerun", 2)] {
        let f = gv.join(folder);
        let mut names: Vec<String> = std::fs::read_dir(&f).expect("ls").flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
        names.sort();
        assert_eq!(names, ["RIGHTS.txt", "dwg", "manifest.csv"]);
        assert_eq!(std::fs::read(f.join("RIGHTS.txt")).expect("rights"), RIGHTS);
        assert_eq!(std::fs::read_dir(f.join("dwg")).expect("ls").count(), n);
    }
    assert!(reply["folder"].as_str().is_some_and(|f| f.ends_with(&format!("{GV}/t1"))));
    assert!(reply["rerun_folder"].as_str().is_some_and(|f| f.ends_with(&format!("{GV}/t1-rerun"))));
    assert!(reply["sources"]["house"].as_array().is_some_and(|a| !a.is_empty()));
}

#[test]
fn the_manifest_names_each_file_by_its_hash_in_seed_order() {
    let d = dir("manifest");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let brief = brief3(&d);
    let mut s = session();
    let reply = s.execute("deliver", &params(&d, &brief, "m1", Some(&rights), &root)).expect("deliver");
    let f = root.join(GV).join("m1");
    let text = std::fs::read_to_string(f.join("manifest.csv")).expect("csv");
    assert!(!text.contains('\r') && text.ends_with('\n'));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "seed,file,sha256,generator_version,units,wall_layers,door_layers,window_layers,building,floor,drawing");
    assert_eq!(lines.len(), 5);
    let mut total = 0;
    let mut buildings = std::collections::BTreeSet::new();
    for (i, line) in lines[1..].iter().enumerate() {
        let c: Vec<&str> = line.split(',').collect();
        assert_eq!(c.len(), 11, "{line}");
        assert_eq!(c[0], (i + 1).to_string());
        assert_eq!(c[1], format!("dwg/house_s{}.dwg", i + 1));
        let bytes = std::fs::read(f.join(c[1])).expect("dwg");
        assert_eq!(c[2], crate::sha256::hex(&bytes));
        total += bytes.len();
        assert_eq!(&c[3..8], [GV, "mm", "A-WALL", "A-DOOR", "A-GLAZ"]);
        assert!(c[8].starts_with("house-") && c[8].len() == 18);
        buildings.insert(c[8].to_string());
        assert_eq!((c[9], c[10]), ("1", ""));
    }
    assert_eq!(reply["buildings"], buildings.len());
    assert_eq!(reply["average_file_bytes"].as_f64().map(|a| a.round() as usize), Some((total as f64 / 4.0).round() as usize));
    // The rerun's rows are the delivery's rows for its seeds.
    let rerun = std::fs::read_to_string(root.join(GV).join("m1-rerun").join("manifest.csv")).expect("rerun csv");
    let rl: Vec<&str> = rerun.lines().collect();
    assert_eq!(rl.len(), 3);
    for l in &rl[1..] {
        assert!(lines.contains(l), "{l}");
    }
    // Seeds 2 and 4 of four: spread through the delivery.
    assert!(rl[1].starts_with("2,") && rl[2].starts_with("4,"), "{rl:?}");
}

#[test]
fn the_tree_does_not_depend_on_the_jobs() {
    let d = dir("jobs");
    let rights = put(&d, "rights.txt", RIGHTS);
    let brief = brief3(&d);
    let mut trees = Vec::new();
    for jobs in [1, 3] {
        let root = d.join(format!("root{jobs}"));
        std::fs::create_dir_all(&root).expect("root");
        let mut p = params(&d, &brief, "j", Some(&rights), &root);
        p["jobs"] = json!(jobs);
        session().execute("deliver", &p).expect("deliver");
        let t: BTreeMap<String, Vec<u8>> = tree(&root).into_iter().filter(|(_, v)| !v.0).map(|(k, v)| (k, v.3)).collect();
        trees.push(t);
    }
    assert!(!trees[0].is_empty());
    assert_eq!(trees[0], trees[1]);
}

#[test]
fn rights_emailed_leaves_no_rights_file() {
    let d = dir("emailed");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let brief = brief3(&d);
    session().execute("deliver", &params(&d, &brief, "e", Some("emailed"), &root)).expect("deliver");
    for f in ["e", "e-rerun"] {
        let p = root.join(GV).join(f);
        assert!(p.join("manifest.csv").is_file() && !p.join("RIGHTS.txt").exists());
    }
}

fn refused(s: &mut Session, p: &Value, code: &str) {
    let e = s.execute("deliver", p).expect_err("refused");
    assert!(e.starts_with(&format!("{code}: ")), "{e}");
}

#[test]
fn refusals_write_nothing_at_all() {
    let d = dir("refusals");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let empty = put(&d, "empty.txt", b"");
    let brief = brief3(&d);
    let overlap = put(
        &d,
        "overlap.json",
        br#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 6, "rooms": [8, 12]},
        {"archetype": "house", "first_seed": 4, "count": 6, "rooms": [8, 12]}]}"#,
    );
    let before = tree(&root);
    let mut s = session();
    refused(&mut s, &params(&d, &brief, "r", None, &root), "missing_rights");
    refused(&mut s, &params(&d, &brief, "r", Some(&d.join("none.txt").display().to_string()), &root), "missing_rights");
    refused(&mut s, &params(&d, &brief, "r", Some(&empty), &root), "missing_rights");
    refused(&mut s, &params(&d, &overlap, "r", Some(&rights), &root), "invalid_brief");
    refused(&mut s, &params(&d, &brief, "../r", Some(&rights), &root), "invalid_params");
    refused(&mut s, &params(&d, &brief, ".hidden", Some(&rights), &root), "invalid_params");
    let mut bare = Session::new();
    refused(&mut bare, &params(&d, &brief, "r", Some(&rights), &root), "invalid_params");
    let mut no_writer = session();
    no_writer.dwg_writer = None;
    refused(&mut no_writer, &params(&d, &brief, "r", Some(&rights), &root), "invalid_params");
    let mut no_version = session();
    no_version.generator_version = None;
    refused(&mut no_version, &params(&d, &brief, "r", Some(&rights), &root), "invalid_params");
    assert_eq!(tree(&root), before);

    // A second delivery of the same id, and a rerun folder alone: refused, and nothing under ROOT changes.
    s.execute("deliver", &params(&d, &brief, "x", Some(&rights), &root)).expect("deliver");
    let before = tree(&root);
    refused(&mut s, &params(&d, &brief, "x", Some(&rights), &root), "delivery_exists");
    assert_eq!(tree(&root), before);
    std::fs::create_dir(root.join(GV).join("y-rerun")).expect("rerun folder");
    let before = tree(&root);
    refused(&mut s, &params(&d, &brief, "y", Some(&rights), &root), "delivery_exists");
    assert_eq!(tree(&root), before);
}

#[test]
fn a_batch_no_family_can_meet_is_skipped_with_its_code() {
    let d = dir("skipped");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let brief = put(
        &d,
        "brief.json",
        br#"{"schema_version": 1, "batches": [
        {"archetype": "house", "first_seed": 1, "count": 3, "rooms": [8, 12], "rooms_tol": 2},
        {"archetype": "house", "first_seed": 101, "count": 2, "rooms": [2, 2], "rooms_tol": 0}]}"#,
    );
    let reply = s_deliver(&d, &brief, &rights, &root);
    assert_eq!(reply["files"], 3);
    let skipped = reply["skipped"].as_array().expect("skipped");
    assert_eq!(skipped.len(), 2);
    for (k, s) in skipped.iter().enumerate() {
        assert_eq!(s["seed"], 101 + k);
        assert_eq!(s["error"], "unsatisfiable_rooms");
    }
    let csv = std::fs::read_to_string(root.join(GV).join("sk").join("manifest.csv")).expect("csv");
    assert_eq!(csv.lines().count(), 4);
    assert!(!root.join(GV).join("sk").join("dwg").join("house_s101.dwg").exists());
}

fn s_deliver(d: &Path, brief: &str, rights: &str, root: &Path) -> Value {
    session().execute("deliver", &params(d, brief, "sk", Some(rights), root)).expect("deliver")
}

#[test]
fn briefs_the_schema_does_not_allow_are_invalid() {
    let d = dir("briefs");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let mut s = session();
    for (i, text) in [
        r#"{"schema_version": 2, "batches": []}"#,
        r#"{"schema_version": 1, "batches": []}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "barn", "first_seed": 1, "count": 1, "rooms": [8, 12]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1, "rooms": [8, 12], "area": [100, 200]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1, "area": [200, 100]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1, "area": [100.2, 100.8]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1, "area": [0, 100]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1, "area": [100, 5001]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "barn", "first_seed": 1, "count": 1, "area": [100, 200]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 18446744073709551615, "count": 5, "rooms": [8, 12]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 0, "rooms": [8, 12]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1, "rooms": [12, 8]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 5000000, "rooms": [8, 12]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 1, "rooms": [8, 12], "extra": 1}]}"#,
        "not json",
    ]
    .iter()
    .enumerate()
    {
        let brief = put(&d, &format!("b{i}.json"), text.as_bytes());
        refused(&mut s, &params(&d, &brief, "b", Some(&rights), &root), "invalid_brief");
    }
    assert_eq!(std::fs::read_dir(&root).expect("ls").count(), 0);
    refused(&mut s, &params(&d, "/dev/zero", "b", Some(&rights), &root), "invalid_brief");
    refused(&mut s, &params(&d, "/nonexistent/b.json", "b", Some(&rights), &root), "invalid_brief");
}

#[test]
fn the_rerun_is_spread_through_the_delivery() {
    for (n, k) in [(200usize, 50usize), (190, 50), (4, 2), (3, 50), (1, 1), (7, 7)] {
        let k = k.min(n);
        let at = rerun_indices(n, k);
        assert_eq!(at.len(), k);
        assert!(at.windows(2).all(|w| w[0] < w[1]) && at.iter().all(|i| *i < n));
        let step = n.div_ceil(k);
        if k < n {
            assert!(at[0] <= step && *at.last().unwrap() + 1 + step >= n, "{n} {k} {at:?}");
            assert!(at.windows(2).all(|w| w[1] - w[0] <= step + 1));
        } else {
            assert_eq!(at, (0..n).collect::<Vec<_>>());
        }
    }
    assert_eq!(&rerun_indices(200, 50)[..3], [2, 6, 10]);
    assert_eq!(rerun_indices(4, 2), [1, 3]);
}

#[test]
fn a_seeds_room_count_depends_on_the_seed_alone_and_stays_pinned() {
    for seed in 0..500 {
        let r = rooms_for(seed, 5, 22);
        assert!((5..=22).contains(&r));
        assert_eq!(r, rooms_for(seed, 5, 22));
    }
    assert_eq!(rooms_for(9, 7, 7), 7);
    let first: Vec<i64> = (1..=5).map(|s| rooms_for(s, 5, 22)).collect();
    assert_eq!(first, PINNED, "past deliveries depend on these never changing");
}

const PINNED: [i64; 5] = [5, 19, 5, 17, 16];

#[test]
fn a_seeds_area_depends_on_the_seed_alone_and_stays_pinned() {
    use super::deliver::area_for;
    for seed in 0..500 {
        let a = area_for(seed, 150, 400);
        assert!((150..=400).contains(&a));
        assert_eq!(a, area_for(seed, 150, 400));
    }
    assert_eq!(area_for(9, 300, 300), 300);
    // Its own salt: the areas do not follow the rooms.
    assert_ne!((1..=20).map(|s| area_for(s, 5, 22)).collect::<Vec<_>>(), (1..=20).map(|s| rooms_for(s, 5, 22)).collect::<Vec<_>>());
    let first: Vec<i64> = (1..=5).map(|s| area_for(s, 150, 400)).collect();
    assert_eq!(first, PINNED_AREAS, "past deliveries depend on these never changing");
}

const PINNED_AREAS: [i64; 5] = [263, 211, 366, 178, 385];

const MIXED: &[u8] = br#"{"schema_version": 1, "batches": [
    {"archetype": "house", "first_seed": 1, "count": 2, "rooms": [8, 12]},
    {"archetype": "clinic", "first_seed": 11, "count": 2, "area": [600, 700]},
    {"archetype": "medical-day", "first_seed": 21, "count": 1, "area": [800, 900]},
    {"archetype": "house", "first_seed": 31, "count": 2, "area": [12, 12]}]}"#;

#[test]
fn an_area_batch_delivers_named_files_and_skips_what_its_archetype_refuses() {
    let d = dir("area");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let brief = put(&d, "brief.json", MIXED);
    let reply = s_deliver(&d, &brief, &rights, &root);
    assert_eq!(reply["files"], 5, "{reply}");
    let folder = root.join(GV).join("sk");
    for name in ["house_s1", "house_s2", "clinic_s11", "clinic_s12", "medical-day_s21"] {
        assert!(folder.join("dwg").join(format!("{name}.dwg")).exists(), "{name}");
    }
    let skipped = reply["skipped"].as_array().expect("skipped");
    assert_eq!(skipped.iter().map(|s| s["seed"].as_u64().unwrap()).collect::<Vec<_>>(), [31, 32]);
    let want = super::build_for_delivery(super::program::Archetype::House, 31, super::AreaRequest::m2(12.0)).err().expect("refused").code;
    assert!(skipped.iter().all(|s| s["error"] == want), "{skipped:?} {want}");
    // The sources are the house family's alone: the area batches have none.
    let sources = reply["sources"].as_object().expect("sources");
    assert_eq!(sources.keys().collect::<Vec<_>>(), ["house"]);
    let csv = std::fs::read_to_string(folder.join("manifest.csv")).expect("csv");
    assert_eq!(csv.lines().count(), 6);
    assert!(csv.lines().skip(1).filter(|l| l.contains("clinic_s1")).all(|l| l.contains(",clinic-")), "{csv}");
}

#[test]
fn an_area_seeds_file_is_what_its_delivery_script_saves() {
    use super::program::Archetype;
    let d = dir("area-bytes");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let brief = put(
        &d,
        "brief.json",
        br#"{"schema_version": 1, "batches": [{"archetype": "hospital", "first_seed": 3, "count": 1, "area": [1800, 2000]}]}"#,
    );
    s_deliver(&d, &brief, &rights, &root);
    let got = std::fs::read(root.join(GV).join("sk").join("dwg").join("hospital_s3.dwg")).expect("dwg");
    let area = super::deliver::area_for(3, 1800, 2000);
    let built = super::build_for_delivery(Archetype::Hospital, 3, super::AreaRequest::m2(area as f64)).expect("built");
    let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").expect("script").1.clone();
    let at = d.join("again.dwg");
    crate::ids::keeping_reproducible(|| {
        let mut s = session();
        s.script(&script).expect("script");
        s.execute("save", &json!({"path": at.display().to_string()})).expect("save");
    });
    assert_eq!(got, std::fs::read(&at).expect("again"));
}

#[test]
fn a_batch_with_both_rooms_and_area_or_neither_is_invalid() {
    let d = dir("both");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let rights = put(&d, "rights.txt", RIGHTS);
    let mut s = session();
    for (i, text) in [
        r#"{"schema_version": 1, "batches": [{"archetype": "clinic", "first_seed": 1, "count": 1, "rooms": [8, 12], "area": [600, 700]}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "clinic", "first_seed": 1, "count": 1}]}"#,
        r#"{"schema_version": 1, "batches": [{"archetype": "office", "first_seed": 1, "count": 1, "rooms": [8, 12]}]}"#,
    ]
    .iter()
    .enumerate()
    {
        let brief = put(&d, &format!("b{i}.json"), text.as_bytes());
        refused(&mut s, &params(&d, &brief, "b", Some(&rights), &root), "invalid_brief");
    }
    assert_eq!(std::fs::read_dir(&root).expect("ls").count(), 0);
}

#[test]
fn a_rooms_batch_is_accepted_for_every_family_file_and_refused_for_none() {
    let d = dir("family-files");
    let rights = put(&d, "rights.txt", RIGHTS);
    let names = super::shape_graph::register_tests::family_file_names();
    let batch = |archetype: &str, first_seed: usize| json!({"archetype": archetype, "first_seed": first_seed, "count": 1, "rooms": [12, 12], "rooms_tol": 2, "unreviewed": "allow"});
    let all: Vec<Value> = names.iter().enumerate().map(|(i, n)| batch(n, 100 * (i + 1))).collect();
    let brief = put(&d, "all.json", json!({"schema_version": 1, "batches": all}).to_string().as_bytes());
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let out = session().execute("deliver", &params(&d, &brief, "all", Some(&rights), &root));
    assert!(out.is_ok(), "{out:?}");
    let none = put(&d, "none.json", json!({"schema_version": 1, "batches": [batch("no-such-family", 1)]}).to_string().as_bytes());
    let root2 = d.join("root2");
    std::fs::create_dir_all(&root2).expect("root");
    refused(&mut session(), &params(&d, &none, "none", Some(&rights), &root2), "invalid_brief");
}

/// A reviewed family is delivered with or without `"unreviewed": "allow"`, which changes nothing for it. Every built-in family is
/// reviewed, so the refusal of an unreviewed one is tested on the build (`pack_tables_tests`), not through a brief.
#[test]
fn a_reviewed_family_is_delivered_with_or_without_unreviewed_allow() {
    let d = dir("unreviewed");
    let rights = put(&d, "rights.txt", RIGHTS);
    let batch = |allow: bool| {
        let mut b = json!({"archetype": "medical-day", "first_seed": 701, "count": 3, "rooms": [24, 30], "rooms_tol": 3});
        if allow {
            b["unreviewed"] = json!("allow");
        }
        b
    };
    let brief = |name: &str, batches: Value| put(&d, name, json!({"schema_version": 1, "batches": batches}).to_string().as_bytes());
    let root = |name: &str| {
        let r = d.join(name);
        std::fs::create_dir_all(&r).expect("root");
        r
    };
    let mut trees = Vec::new();
    for allow in [false, true] {
        let r = root(if allow { "allowed" } else { "plain" });
        let reply = s_deliver(&d, &brief(&format!("{allow}.json"), json!([batch(allow)])), &rights, &r);
        assert_eq!(reply["files"], 3, "{reply}");
        assert_eq!(reply["skipped"], json!([]));
        let dwg = r.join(GV).join("sk").join("dwg");
        assert!(dwg.join("medical-day_s701.dwg").exists());
        trees.push(tree(&dwg).into_iter().map(|(name, (.., bytes))| (name, bytes)).collect::<BTreeMap<_, _>>());
    }
    assert_eq!(trees[0], trees[1], "the same drawings either way");
}

/// When every seed of a brief is skipped there is nothing to deliver: the refusal says how many seeds were skipped and with which
/// code, gives the first one's own message, and writes nothing.
#[test]
fn a_brief_whose_every_seed_is_skipped_is_refused_with_its_codes() {
    let d = dir("empty");
    let rights = put(&d, "rights.txt", RIGHTS);
    let alone = d.join("alone");
    std::fs::create_dir_all(&alone).expect("root");
    let brief = put(
        &d,
        "alone.json",
        br#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 101, "count": 2, "rooms": [2, 2], "rooms_tol": 0}]}"#,
    );
    let e = session().execute("deliver", &params(&d, &brief, "sk", Some(&rights), &alone)).expect_err("refused");
    assert!(e.starts_with("empty_delivery: "), "{e}");
    assert!(e.contains("all 2 seeds of the brief were skipped (2 with `unsatisfiable_rooms`)"), "{e}");
    assert!(e.contains("seed 101"), "the first skip's own message: {e}");
    assert_eq!(std::fs::read_dir(&alone).expect("ls").count(), 0);
}
