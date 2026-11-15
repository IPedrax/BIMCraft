//! The corridor archetypes' files keep their bytes, and the plan handed to lift, openings, script and report does not read
//! the corridor's fields. The hashes were taken before the house layout was added (Slice 12), and taken again on 2026-11-13
//! when each script gained its `drawing_property` line (`building_type`): every script is its old bytes with that one line
//! after `new`, and every report its old bytes with one more command counted in the two checks that count them. The topology and
//! building hashes are the ones from before Slice 12.

use super::*;

/// (name, archetype, seed, unit, value, fnv1a64 of `script.bimcraft.txt`), as `done/fixtures/house-layout-baseline` lists them.
const SCRIPTS: [(&str, &str, u64, &str, f64, &str); 20] = [
    ("office-sqft-s1", "office", 1, "sqft", 10000.0, "8f37a3989b2b4f6f"),
    ("office-sqft-s3", "office", 3, "sqft", 10000.0, "c2a6a755d3d9dbfd"),
    ("office-sqft-s4", "office", 4, "sqft", 10000.0, "37136a295a30abf9"),
    ("office-sqft-s7", "office", 7, "sqft", 10000.0, "f987631d040da66d"),
    ("office-sqft-s10", "office", 10, "sqft", 10000.0, "27ea55b2f888be36"),
    ("clinic-sqft-s1", "clinic", 1, "sqft", 7000.0, "d2f426d8b1e069c8"),
    ("clinic-sqft-s3", "clinic", 3, "sqft", 7000.0, "28aeac4c6f1fa3be"),
    ("clinic-sqft-s4", "clinic", 4, "sqft", 7000.0, "8026e242f210c066"),
    ("clinic-sqft-s7", "clinic", 7, "sqft", 7000.0, "c292d44f799e84f5"),
    ("clinic-sqft-s10", "clinic", 10, "sqft", 7000.0, "3e78ed6b8555d427"),
    ("hospital-sqft-s1", "hospital", 1, "sqft", 20000.0, "e9bf4481d77618f1"),
    ("hospital-sqft-s3", "hospital", 3, "sqft", 20000.0, "5b660553116c76da"),
    ("hospital-sqft-s4", "hospital", 4, "sqft", 20000.0, "94fa3fad24f69c94"),
    ("hospital-sqft-s7", "hospital", 7, "sqft", 20000.0, "b19a63c13df2c7a7"),
    ("hospital-sqft-s10", "hospital", 10, "sqft", 20000.0, "aa9f50384118c1f5"),
    ("clinic-sqft-s9", "clinic", 9, "sqft", 7000.0, "18eabcf31a5f1c74"),
    ("hospital-sqft-s8", "hospital", 8, "sqft", 20000.0, "ff4e93653f7235ee"),
    ("office-m2-s1", "office", 1, "m2", 900.0, "1408017a878bda60"),
    ("clinic-m2-s3", "clinic", 3, "m2", 650.0, "28aeac4c6f1fa3be"),
    ("hospital-m2-s10", "hospital", 10, "m2", 1800.0, "dc156386e7d877d6"),
];

fn built(archetype: &str, seed: u64, unit: &str, value: f64) -> &'static Built {
    let area = if unit == "sqft" { AreaRequest::sqft(value) } else { AreaRequest::m2(value) };
    super::tests::built_once(Archetype::parse(archetype).unwrap(), seed, area, false)
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    &b.files.iter().find(|(n, _)| *n == name).unwrap().1
}

#[test]
fn legacy_scripts_keep_their_bytes() {
    let mut wrong = Vec::new();
    for (name, archetype, seed, unit, value, hash) in SCRIPTS {
        let got = fnv1a64(file(built(archetype, seed, unit, value), "script.bimcraft.txt"));
        if got != hash {
            wrong.push(format!("(\"{name}\", {got})"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// (archetype, seed, m2, then the hashes of report, topology and building files).
const FILES: [(&str, u64, f64, [&str; 3]); 3] = [
    ("office", 1, 900.0, ["dc93a073d782b78b", "e861b63f8db2b67e", "b1793dbd44590210"]),
    ("clinic", 3, 650.0, ["8c63b56b794bd029", "c7a1c5c1faae85a9", "5b077d2a95d251e9"]),
    ("hospital", 10, 1800.0, ["651ab880a4b15f00", "9acc063d0ce3bad5", "c946d90baf919830"]),
];

#[test]
fn legacy_reports_keep_their_bytes() {
    let mut wrong = Vec::new();
    for (archetype, seed, m2, hashes) in FILES {
        let b = built(archetype, seed, "m2", m2);
        for (name, hash) in ["report.json", "topology-graph.json", "building-ir.json"].into_iter().zip(hashes) {
            let got = fnv1a64(file(b, name));
            if got != hash {
                wrong.push(format!("{archetype} {name} {got}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn corridor_fields_stay_in_layout() {
    let corridor = regex_lite_hits;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/generate");
    let mut hits = Vec::new();
    for name in ["lift.rs", "openings.rs", "script.rs", "report.rs"] {
        let text = std::fs::read_to_string(root.join(name)).unwrap();
        let mut in_tests = false;
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                in_tests = true;
            }
            let code = line.split("//").next().unwrap_or("");
            if !in_tests && corridor(code) {
                hits.push(format!("{name}:{}", i + 1));
            }
        }
    }
    assert!(hits.is_empty(), "corridor fields read outside the layout: {hits:?}");
}

/// `corridor_bottom|corridor_top|\.band\b|\.lobby\b|\.corridor\b`, without a regex crate.
fn regex_lite_hits(code: &str) -> bool {
    if code.contains("corridor_bottom") || code.contains("corridor_top") {
        return true;
    }
    [".band", ".lobby", ".corridor"].iter().any(|word| {
        code.match_indices(word).any(|(i, _)| !code[i + word.len()..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_'))
    })
}
