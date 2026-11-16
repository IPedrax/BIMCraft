//! `deliver` through `Session::execute`, with a fake DWG writer (the DXF text as the file's bytes).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::deliver::{read_brief_for_tests, rerun_indices, rooms_for};
use super::shape_graph;
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

fn props(dwg: &str) -> BTreeMap<String, String> {
    let lines: Vec<&str> = dwg.lines().collect();
    let mut out = BTreeMap::new();
    for w in lines.windows(8) {
        if w[0] == "  9" && w[1] == "$CUSTOMPROPERTYTAG" && w[2] == "  1" && w[4] == "  9" && w[5] == "$CUSTOMPROPERTY" && w[6] == "  1" {
            out.insert(w[3].to_string(), w[7].to_string());
        }
    }
    out
}

fn v2_brief(d: &Path, name: &str, text: &str) -> String {
    put(d, name, text.as_bytes())
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
fn every_delivered_dwg_carries_the_generator_version_as_a_drawing_property() {
    let d = dir("property");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let brief = brief3(&d);
    session().execute("deliver", &params(&d, &brief, "p", Some("emailed"), &root)).expect("deliver");
    let want = format!("  9\n$CUSTOMPROPERTYTAG\n  1\ngenerator_version\n  9\n$CUSTOMPROPERTY\n  1\n{GV}\n");
    let mut n = 0;
    for folder in ["p", "p-rerun"] {
        for e in std::fs::read_dir(root.join(GV).join(folder).join("dwg")).expect("ls").flatten() {
            let dwg = std::fs::read_to_string(e.path()).expect("the fake writer's DXF text");
            assert!(dwg.contains(&want), "{} has no generator_version property", e.path().display());
            n += 1;
        }
    }
    assert_eq!(n, 4 + 2);
}

#[test]
fn rerun_0_makes_no_rerun_folder_and_no_rerun_dwg() {
    let d = dir("rerun0");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let brief = brief3(&d);
    let mut s = session();
    let mut p = params(&d, &brief, "z", Some("emailed"), &root);
    p["rerun"] = json!(0);
    let reply = s.execute("deliver", &p).expect("deliver");
    assert_eq!(reply["files"], 4);
    assert_eq!(reply["rerun_folder"], Value::Null);
    let inner: Vec<String> =
        std::fs::read_dir(root.join(GV)).expect("ls").flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
    assert_eq!(inner, ["z"]);
    assert!(tree(&root).keys().all(|k| !k.contains("rerun")), "{:?}", tree(&root).keys());
    p["rerun"] = json!(-1);
    p["delivery_id"] = json!("z2");
    refused(&mut s, &p, "invalid_params");
}

#[test]
fn v1_rooms_delivery_sha_is_golden() {
    let d = dir("v1-golden");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let brief = brief3(&d);
    let mut p = params(&d, &brief, "gold", Some("emailed"), &root);
    p["rerun"] = json!(0);
    session().execute("deliver", &p).expect("deliver");
    let csv = std::fs::read_to_string(root.join(GV).join("gold").join("manifest.csv")).expect("csv");
    let first = csv.lines().nth(1).expect("row").split(',').collect::<Vec<_>>();
    assert_eq!(first[1], "dwg/house_s1.dwg");
    assert_eq!(first[2], "c785e6e0cccc4dd93d198179c2d1802f93bc485f87fa86724861f2af87f4f0ee");
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
fn v2_stamps_properties_and_manifest_columns_for_every_delivered_dwg() {
    let d = dir("v2-props");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let brief = v2_brief(
        &d,
        "v2.json",
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 8, "building_types": ["office", "hospital"], "gridlines": "on"}]}"#,
    );
    let mut p = params(&d, &brief, "v2", Some("emailed"), &root);
    p["rerun"] = json!(0);
    let reply = session().execute("deliver", &p).expect("deliver");
    assert_eq!(reply["rerun_folder"], Value::Null);
    let folder = root.join(GV).join("v2");
    assert!(!root.join(GV).join("v2-rerun").exists());
    let csv = std::fs::read_to_string(folder.join("manifest.csv")).expect("csv");
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "seed,file,sha256,generator_version,units,wall_layers,door_layers,window_layers,building,floor,drawing,building_type,archetype,shape,rooms,interior,bumps,gridlines"
    );
    assert!(lines.len() > 1, "{reply}");
    for line in &lines[1..] {
        let c: Vec<&str> = line.split(',').collect();
        assert_eq!(c.len(), 18, "{line}");
        let p = props(&std::fs::read_to_string(folder.join(c[1])).expect("dwg text"));
        assert_eq!(p["generator_version"], GV);
        assert_eq!(p["brief_schema"], "2");
        assert_eq!(p["building_type"], c[11]);
        assert_eq!(p["archetype"], c[12]);
        assert_eq!(p["shape"], c[13]);
        assert_eq!(p["rooms"], c[14]);
        assert_eq!(p["interior"], c[15]);
        assert_eq!(p["bumps"], c[16]);
        assert_eq!(p["gridlines"], "on");
        assert_eq!(c[17], "on");
        assert!(matches!(p["building_type"].as_str(), "office" | "hospital"));
    }
}

#[test]
fn v2_seed_draws_cover_options_and_pins_do_not_move_other_draws() {
    let d = dir("v2-seed-control");
    let all = v2_brief(&d, "all.json", r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1000}]}"#);
    let jobs = read_brief_for_tests(&all).expect("brief");
    assert_eq!(jobs.len(), 1000);
    let types: BTreeSet<_> = jobs.iter().filter_map(|j| j.building_type.as_deref()).collect();
    assert_eq!(types, BTreeSet::from(["clinic", "educational", "hospital", "office", "residential"]));
    let families: BTreeSet<_> = jobs.iter().map(|j| j.archetype.as_str()).collect();
    assert_eq!(families, BTreeSet::from(["clinic", "hospital", "house", "medical-day", "office-pattern", "school", "studio-office"]));
    let shapes: BTreeSet<String> = jobs.iter().filter_map(|j| j.shape.clone()).collect();
    let expected_shapes: BTreeSet<String> = shape_graph::family::shapes().expect("shapes").into_iter().map(|s| s.id).collect();
    assert_eq!(shapes, expected_shapes);
    assert!(jobs.iter().any(|j| j.interior == Some(shape_graph::Interior::Aligned)));
    assert!(jobs.iter().any(|j| j.interior == Some(shape_graph::Interior::Jogged)));
    assert!(jobs.iter().any(|j| j.bumps) && jobs.iter().any(|j| !j.bumps));
    assert!(jobs.iter().any(|j| j.gridlines) && jobs.iter().any(|j| !j.gridlines));

    let pinned = v2_brief(
        &d,
        "pinned.json",
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1000, "interior": "jogged", "bumps": "off", "gridlines": "on"}]}"#,
    );
    let pinned = read_brief_for_tests(&pinned).expect("pinned");
    for (a, p) in jobs.iter().zip(&pinned) {
        assert_eq!(p.interior, Some(shape_graph::Interior::Jogged));
        assert!(!p.bumps);
        assert!(p.gridlines);
        assert_eq!(
            (a.seed, &a.building_type, &a.archetype, &a.shape, a.rooms),
            (p.seed, &p.building_type, &p.archetype, &p.shape, p.rooms)
        );
    }
}

#[test]
fn v2_matches_generate_for_values_read_back_from_the_dwg() {
    let d = dir("v2-matches-generate");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let brief = v2_brief(
        &d,
        "brief.json",
        r#"{"schema_version": 2, "batches": [{"first_seed": 11, "count": 3, "building_types": ["office"], "gridlines": "off"}]}"#,
    );
    let jobs = read_brief_for_tests(&brief).expect("jobs");
    let mut p = params(&d, &brief, "match", Some("emailed"), &root);
    p["rerun"] = json!(0);
    session().execute("deliver", &p).expect("deliver");
    let folder = root.join(GV).join("match");
    let csv = std::fs::read_to_string(folder.join("manifest.csv")).expect("csv");
    for line in csv.lines().skip(1) {
        let c: Vec<&str> = line.split(',').collect();
        let seed: u64 = c[0].parse().unwrap();
        let prop = props(&std::fs::read_to_string(folder.join(c[1])).expect("dwg text"));
        let job = jobs.iter().find(|j| j.seed == seed).expect("job");
        let out = d.join(format!("generate-{seed}"));
        let generated = session()
            .execute(
                "generate",
                &json!({
                    "path": out.display().to_string(),
                    "archetype": prop["archetype"],
                    "seed": seed,
                    "rooms": prop["rooms"].parse::<i64>().unwrap(),
                    "rooms_tol": job.tol,
                    "shape": prop["shape"],
                    "interior": prop["interior"],
                    "bumps": prop["bumps"],
                    "gridlines": prop["gridlines"]
                }),
            )
            .expect("generate");
        let manifest: Value = serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).expect("manifest")).expect("json");
        let mut request = shape_graph::Request::new(
            prop["archetype"].clone(),
            seed,
            prop["rooms"].parse::<i64>().unwrap(),
            job.tol,
            Some(prop["shape"].clone()),
            prop["gridlines"] == "on",
        );
        request.interior = shape_graph::Interior::parse(&prop["interior"]).unwrap();
        request.bumps = prop["bumps"] == "on";
        let built = shape_graph::build(&request, false).expect("shape graph build");
        assert_eq!(manifest["building"], c[8]);
        assert_eq!(manifest["topology_hash"], generated["topology_hash"]);
        assert_eq!(built.topology_hash, generated["topology_hash"]);
        assert_eq!(manifest["shape"], prop["shape"]);
        assert_eq!(manifest["rooms"]["actual"].to_string(), prop["rooms"]);
        assert_eq!(manifest["interior"], prop["interior"]);
    }
}

#[test]
fn v2_refuses_forbidden_keys_bad_options_and_overlapping_seeds() {
    let d = dir("v2-refusals");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let mut s = session();
    for (i, text) in [
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "archetype": "house"}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "rooms": [8, 12]}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "rooms_tol": 2}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "area": [100, 200]}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "unreviewed": "allow"}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "shape": "bar"}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "building_types": []}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "building_types": ["office", "office"]}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "building_types": ["airport"]}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "interior": "mixed"}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "bumps": "maybe"}]}"#,
        r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 3}, {"first_seed": 3, "count": 2}]}"#,
    ]
    .iter()
    .enumerate()
    {
        let brief = v2_brief(&d, &format!("bad-{i}.json"), text);
        refused(&mut s, &params(&d, &brief, "bad", Some("emailed"), &root), "invalid_brief");
    }
    assert_eq!(std::fs::read_dir(&root).expect("ls").count(), 0);
}

#[test]
fn shared_option_errors_match_generate_word_for_word() {
    let d = dir("shared-options");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let generate = session()
        .execute(
            "generate",
            &json!({"path": d.join("g").display().to_string(), "archetype": "house", "seed": 1, "rooms": 8, "bumps": "maybe"}),
        )
        .expect_err("generate refuses");
    let brief = v2_brief(&d, "bad.json", r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 1, "bumps": "maybe"}]}"#);
    let deliver = session().execute("deliver", &params(&d, &brief, "d", Some("emailed"), &root)).expect_err("deliver refuses");
    assert_eq!(generate.strip_prefix("invalid_params: "), deliver.strip_prefix("invalid_brief: batch 0: "));
}

#[test]
fn v2_jobs_do_not_change_the_bytes() {
    let d = dir("v2-jobs");
    let brief = v2_brief(&d, "brief.json", r#"{"schema_version": 2, "batches": [{"first_seed": 21, "count": 6, "gridlines": "off"}]}"#);
    let mut trees = Vec::new();
    for jobs in [1, 8] {
        let root = d.join(format!("root{jobs}"));
        std::fs::create_dir_all(&root).expect("root");
        let mut p = params(&d, &brief, "j", Some("emailed"), &root);
        p["rerun"] = json!(0);
        p["jobs"] = json!(jobs);
        session().execute("deliver", &p).expect("deliver");
        trees.push(tree(&root).into_iter().filter(|(_, v)| !v.0).map(|(k, v)| (k, v.3)).collect::<BTreeMap<_, _>>());
    }
    assert_eq!(trees[0], trees[1]);
}

#[test]
fn v2_hundred_thousand_seed_brief_validates_without_building() {
    let d = dir("v2-scale-validate");
    let brief = v2_brief(&d, "brief.json", r#"{"schema_version": 2, "batches": [{"first_seed": 1, "count": 100000}]}"#);
    assert_eq!(read_brief_for_tests(&brief).expect("brief").len(), 100_000);
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
