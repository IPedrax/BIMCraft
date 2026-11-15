//! The seed library through its command and its files: what it writes, what it refuses, and that the data is what the packs say.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Value, json};

use super::*;

fn dir(test: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bimcraft-rulepack-{}-{test}", std::process::id()))
}

fn library() -> Library {
    build().unwrap_or_else(|e| panic!("{e}"))
}

fn doc(lib: &Library, name: &str) -> Value {
    let (_, text) = lib.files.iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("no {name}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The library built from the embedded data with one seed file changed by `edit`.
fn with_seed(archetype: &str, edit: &dyn Fn(&mut Value)) -> Result<Library, Refusal> {
    let emb = embedded();
    let seeds: Vec<(String, String)> = emb
        .seeds
        .iter()
        .map(|(a, text)| {
            if *a == archetype {
                let mut v: Value = serde_json::from_str(text).unwrap();
                edit(&mut v);
                (a.to_string(), v.to_string())
            } else {
                (a.to_string(), text.to_string())
            }
        })
        .collect();
    let seeds: Vec<(&str, &str)> = seeds.iter().map(|(a, t)| (a.as_str(), t.as_str())).collect();
    build_from(&Inputs { seeds: &seeds, proposals: emb.proposals, prompts: emb.prompts })
}

fn first_rule(v: &mut Value) -> &mut Value {
    &mut v["rules"][0]
}

fn refused(r: Result<Library, Refusal>, code: &str, mentions: &str) {
    match r {
        Ok(_) => panic!("expected {code} about {mentions}, got a library"),
        Err(e) => {
            assert_eq!(e.code, code, "{e}");
            assert!(e.message.contains(mentions), "the message names {mentions}: {e}");
        }
    }
}

fn pack_files() -> Vec<String> {
    ARCHETYPES.iter().map(|a| format!("packs/{a}.json")).collect()
}

#[test]
fn the_embedded_seed_data_parses_strictly() {
    let emb = embedded();
    assert_eq!(emb.seeds.len(), 5);
    assert_eq!(emb.proposals.len(), 3);
    assert!(build().is_ok());
    // Corrupted data is an error that says so: it is never read as an empty library.
    refused(with_seed("office", &|v| first_rule(v)["colour"] = json!("red")), "invalid_seed", "office");
    refused(with_seed("office", &|v| v["rules"] = json!("none")), "invalid_seed", "office");
    refused(with_seed("clinic", &|v| first_rule(v)["confidence"] = json!("high")), "invalid_seed", "clinic");
    refused(with_seed("clinic", &|v| first_rule(v)["category"] = json!("vibes")), "invalid_seed", "clinic");
    let broken = [("office", "{not json")];
    let all: Vec<(&str, &str)> = emb.seeds.iter().filter(|(a, _)| *a != "office").chain(broken.iter()).copied().collect();
    refused(build_from(&Inputs { seeds: &all, proposals: emb.proposals, prompts: emb.prompts }), "invalid_seed", "office");
    // A seed for an archetype is needed: five are promised.
    let fewer: Vec<(&str, &str)> = emb.seeds.iter().filter(|(a, _)| *a != "airport").copied().collect();
    refused(build_from(&Inputs { seeds: &fewer, proposals: emb.proposals, prompts: emb.prompts }), "invalid_seed", "airport");
    // The proposals are read as strictly.
    let bad = [("claude-opus", "{\"archetypes\": {\"office\": [{\"key\": \"x\"}]}}"), emb.proposals[1], emb.proposals[2]];
    refused(build_from(&Inputs { seeds: emb.seeds, proposals: &bad, prompts: emb.prompts }), "invalid_seed", "claude-opus");
}

#[test]
fn the_library_meets_the_shape_every_pack_promises() {
    let lib = library();
    let manifest = doc(&lib, "rule-pack-manifest.json");
    assert!(!manifest["schema_version"].is_null());
    assert_eq!(manifest["library_id"], "frontier-model-seed-library");
    assert!(manifest["packs"].as_array().unwrap().len() >= 5);

    let minimum = BTreeMap::from([("office", 8), ("clinic", 8), ("hospital", 10), ("datacenter", 10), ("airport", 10)]);
    let terms: BTreeMap<&str, [&str; 5]> = BTreeMap::from([
        ("office", ["reception", "conference", "private", "restroom", "support"]),
        ("clinic", ["waiting", "exam", "nurse", "clean", "dirty"]),
        ("hospital", ["waiting", "exam", "nurse", "clean", "dirty"]),
        ("datacenter", ["mantrap", "data", "cooling", "electrical", "redund"]),
        ("airport", ["landside", "airside", "security", "baggage", "gate"]),
    ]);
    let categories = ["program", "access", "adjacency", "flow", "separation", "security", "systems", "dimensional", "equipment"];
    let checkability = ["deterministic", "heuristic", "human_review"];
    let mut seen = BTreeSet::new();
    for arch in ARCHETYPES {
        let pack = doc(&lib, &format!("packs/{arch}.json"));
        assert_eq!(pack["archetype"], arch);
        assert_eq!((pack["status"].as_str(), pack["origin"].as_str()), (Some("seed"), Some("frontier_model_seed")));
        assert!(!pack["schema_version"].is_null() && !pack["pack_id"].is_null() && !pack["created_by"].is_null());
        let models: BTreeSet<&str> = pack["models_consulted"].as_array().unwrap().iter().map(|m| m.as_str().unwrap().trim()).collect();
        assert!(models.len() >= 3 && !models.contains(""), "{arch}: {models:?}");
        assert!(pack["prompt_hash"].as_str().unwrap().len() >= 12);
        assert!(pack["conflicts"].is_array() && pack["validation"].is_object());
        let rules = pack["rules"].as_array().unwrap();
        assert!(rules.len() >= minimum[arch], "{arch} has {} rules", rules.len());
        let text = serde_json::to_string(&pack).unwrap().to_lowercase();
        for t in terms[arch] {
            assert!(text.contains(t), "{arch} says nothing of {t}");
        }
        let mut exploratory = 0;
        for r in rules {
            let id = r["id"].as_str().unwrap();
            assert!(id.len() >= 2 && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()), "{id}");
            assert!(id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c)), "{id}");
            assert!(id.starts_with(&format!("{arch}.")), "ids carry their archetype: {id}");
            assert!(seen.insert(id.to_string()), "{id} is in two packs");
            assert!(categories.contains(&r["category"].as_str().unwrap()), "{id}");
            for key in ["subject", "predicate", "object", "rationale"] {
                assert!(r[key].as_str().unwrap().trim().len() >= 3, "{id} {key}");
            }
            let class = r["classification"].as_str().unwrap();
            assert!(class == "exploratory" || class == "soft", "{id} is {class}: a model-seeded rule is never hard");
            exploratory += usize::from(class == "exploratory");
            if class == "soft" {
                assert!(r["deterministic_checker"].as_str().is_some_and(|c| !c.is_empty()), "{id}");
            }
            assert!(checkability.contains(&r["checkability"].as_str().unwrap()), "{id}");
            assert!((0.0..=1.0).contains(&r["confidence"].as_f64().unwrap()), "{id}");
            assert!(r["exceptions"].is_array(), "{id}");
            let p = &r["provenance"];
            assert!(!p["model_refs"].as_array().unwrap().is_empty() && !p["source_proposals"].as_array().unwrap().is_empty(), "{id}");
        }
        // Soft rules are a handful at most; the gate's line is 80% with float margin to spare.
        assert!(exploratory * 100 >= rules.len() * 85, "{arch}: {exploratory} of {} are exploratory", rules.len());
    }
}

#[test]
fn every_rule_is_distilled_from_proposals_that_exist() {
    let lib = library();
    let emb = embedded();
    let proposals: BTreeMap<String, f64> = emb
        .proposals
        .iter()
        .flat_map(|(label, text)| {
            let v: Value = serde_json::from_str(text).unwrap();
            v["archetypes"]
                .as_object()
                .unwrap()
                .iter()
                .flat_map(|(arch, list)| {
                    list.as_array()
                        .unwrap()
                        .iter()
                        .map(|p| (format!("{label}#{arch}/{}", p["key"].as_str().unwrap()), p["confidence"].as_f64().unwrap()))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(proposals.len() > 200, "the three models proposed {}", proposals.len());
    for arch in ARCHETYPES {
        let pack = doc(&lib, &format!("packs/{arch}.json"));
        let labels: Vec<&str> = pack["models_consulted"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
        assert_eq!(
            labels,
            ["claude-haiku", "claude-opus", "claude-sonnet"],
            "models_consulted are the files proposals came from, in order"
        );
        for r in pack["rules"].as_array().unwrap() {
            let id = r["id"].as_str().unwrap();
            let refs: Vec<&str> = r["provenance"]["source_proposals"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
            let top = refs
                .iter()
                .map(|s| *proposals.get(*s).unwrap_or_else(|| panic!("{id} cites {s}, which no model proposed")))
                .fold(0.0, f64::max);
            assert!(r["confidence"].as_f64().unwrap() <= top + 1e-9, "{id} is surer than every proposal it came from");
            for s in &refs {
                assert!(s.contains(&format!("#{arch}/")), "{id} cites another archetype's proposal {s}");
            }
            let from: BTreeSet<&str> = refs.iter().map(|s| s.split('#').next().unwrap()).collect();
            let model_refs: BTreeSet<&str> =
                r["provenance"]["model_refs"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
            assert_eq!(from, model_refs, "{id}");
        }
        for c in pack["conflicts"].as_array().unwrap() {
            assert!(c["positions"].as_array().unwrap().len() >= 2, "a conflict has two sides: {c}");
            for pos in c["positions"].as_array().unwrap() {
                assert!(proposals.contains_key(pos["proposal"].as_str().unwrap()), "{pos}");
            }
        }
    }
    // The packs with a disagreement among the models keep it, and none is invented where the models agree.
    let conflicts = |arch: &str| doc(&lib, &format!("packs/{arch}.json"))["conflicts"].as_array().unwrap().len();
    assert!(conflicts("hospital") >= 1 && conflicts("airport") >= 1);
}

#[test]
fn writing_the_library_twice_gives_the_same_bytes() {
    let (a, b) = (dir("twice-a"), dir("twice-b"));
    let mut s = Session::new();
    s.execute("rule_pack", &json!({"path": a})).unwrap();
    s.execute("rule_pack", &json!({"path": b})).unwrap();
    let tree = |root: &PathBuf| -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.insert(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read(&p).unwrap());
                }
            }
        }
        out
    };
    let (ta, tb) = (tree(&a), tree(&b));
    let names: Vec<&str> = ta.keys().map(String::as_str).collect();
    let mut want = vec!["rule-pack-manifest.json", "validation-report.json"];
    let packs = pack_files();
    want.extend(packs.iter().map(String::as_str));
    want.sort_unstable();
    assert_eq!(names, want);
    assert_eq!(ta, tb);
    for bytes in ta.values() {
        let text = std::str::from_utf8(bytes).unwrap();
        assert!(text.ends_with("}\n"), "a file ends with a newline");
        assert!(!text.contains(&a.to_string_lossy().to_string()), "no path in a file");
    }
    // The same files as the library holds in memory, and what the command's own build gives again.
    let lib = library();
    for (name, text) in &lib.files {
        assert_eq!(ta[*name], text.as_bytes(), "{name}");
    }
    std::fs::remove_dir_all(&a).unwrap();
    std::fs::remove_dir_all(&b).unwrap();
}

#[test]
fn the_prompt_hash_follows_the_prompt_and_the_archetype() {
    let emb = embedded();
    let hash = |lib: &Library, arch: &str| doc(lib, &format!("packs/{arch}.json"))["prompt_hash"].as_str().unwrap().to_string();
    let lib = library();
    assert_ne!(hash(&lib, "office"), hash(&lib, "clinic"));
    for p in 0..2 {
        let mut prompts = emb.prompts;
        let changed = format!("{} and more", prompts[p]);
        prompts[p] = &changed;
        let other = build_from(&Inputs { seeds: emb.seeds, proposals: emb.proposals, prompts }).unwrap();
        assert_ne!(hash(&lib, "office"), hash(&other, "office"), "prompt {p}");
    }
    assert!(emb.prompts.iter().all(|p| p.len() > 100), "the prompts are the real files");
}

#[test]
fn the_validation_report_holds_only_passes_and_warnings_and_rp_ids() {
    let lib = library();
    let report = doc(&lib, "validation-report.json");
    let checks = report["checks"].as_array().unwrap();
    assert!(checks.len() >= 8);
    assert!(checks.iter().all(|c| c["id"].as_str().unwrap().starts_with("RP-")));
    assert!(checks.iter().all(|c| matches!(c["status"].as_str(), Some("PASS" | "WARN")) && !c["detail"].as_str().unwrap().is_empty()));
    let ids: BTreeSet<&str> = checks.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), checks.len(), "ids are stable and unique");
    assert_eq!(report["summary"]["fail"], 0);
    assert!(!report["capability_notes"].as_array().unwrap().is_empty(), "the report says what the library cannot vouch for");
    let notes = report["capability_notes"].to_string();
    assert!(notes.contains("Anthropic"), "the report is frank that all three models are one provider's: {notes}");
}

#[test]
fn the_library_refuses_what_its_checks_forbid() {
    refused(with_seed("office", &|v| first_rule(v)["classification"] = json!("hard")), "rule_violation", "office.reception-at-entry");
    // Soft needs a checker; and one the generator runs.
    refused(
        with_seed("office", &|v| {
            first_rule(v)["classification"] = json!("soft");
        }),
        "rule_violation",
        "office.reception-at-entry",
    );
    refused(
        with_seed("office", &|v| {
            first_rule(v)["classification"] = json!("soft");
            first_rule(v)["deterministic_checker"] = json!("GR-999");
        }),
        "rule_violation",
        "GR-999",
    );
    refused(
        with_seed("office", &|v| first_rule(v)["deterministic_checker"] = json!("GR-002")),
        "rule_violation",
        "office.reception-at-entry",
    );
    // Ids are stable machine form, and not shared between packs.
    refused(with_seed("office", &|v| first_rule(v)["id"] = json!("Office Reception")), "rule_violation", "Office Reception");
    refused(with_seed("office", &|v| first_rule(v)["id"] = json!("x")), "rule_violation", "\"x\"");
    refused(
        with_seed("office", &|v| first_rule(v)["id"] = json!("clinic.public-entry-to-waiting")),
        "rule_violation",
        "clinic.public-entry-to-waiting",
    );
    refused(
        with_seed("office", &|v| {
            let dup = v["rules"][1].clone();
            v["rules"][0]["id"] = dup["id"].clone();
        }),
        "rule_violation",
        "twice",
    );
    // Text and numbers.
    refused(with_seed("office", &|v| first_rule(v)["rationale"] = json!("  ")), "rule_violation", "rationale");
    refused(with_seed("office", &|v| first_rule(v)["confidence"] = json!(1.5)), "rule_violation", "confidence");
    refused(with_seed("office", &|v| first_rule(v)["confidence"] = json!(0.99)), "rule_violation", "surer");
    // Provenance has to point at a proposal that was made, for this archetype.
    refused(
        with_seed("office", &|v| first_rule(v)["source_proposals"] = json!(["claude-opus#office/nothing-of-the-kind"])),
        "rule_violation",
        "nothing-of-the-kind",
    );
    refused(with_seed("office", &|v| first_rule(v)["source_proposals"] = json!([])), "rule_violation", "office.reception-at-entry");
    refused(
        with_seed("office", &|v| first_rule(v)["source_proposals"] = json!(["claude-opus#clinic/reception-at-entry"])),
        "rule_violation",
        "claude-opus#clinic/reception-at-entry",
    );
    // A conflict names rules and proposals that exist.
    refused(
        with_seed("hospital", &|v| v["conflicts"][0]["rule_ids"] = json!(["hospital.no-such-rule"])),
        "rule_violation",
        "hospital.no-such-rule",
    );
    refused(
        with_seed("hospital", &|v| v["conflicts"][0]["positions"][0]["proposal"] = json!("claude-opus#hospital/none")),
        "rule_violation",
        "claude-opus#hospital/none",
    );
    // Too few rules, and too few topics.
    refused(with_seed("office", &|v| v["rules"] = json!([v["rules"][0].clone()])), "rule_violation", "office");
    refused(
        with_seed("airport", &|v| {
            let text = v.to_string().replace("baggage", "luggage");
            *v = serde_json::from_str(&text).unwrap();
        }),
        "rule_violation",
        "baggage",
    );
    // Three models are needed to call it a three-model seed.
    let emb = embedded();
    let two = [emb.proposals[0], emb.proposals[1]];
    refused(build_from(&Inputs { seeds: emb.seeds, proposals: &two, prompts: emb.prompts }), "rule_violation", "three");
    // The ratio: too many soft rules and the rest are no longer the default.
    refused(
        with_seed("clinic", &|v| {
            for r in v["rules"].as_array_mut().unwrap().iter_mut().take(4) {
                r["classification"] = json!("soft");
                r["deterministic_checker"] = json!("GR-002");
            }
        }),
        "rule_violation",
        "exploratory",
    );
}

#[test]
fn a_directory_that_cannot_be_written_is_refused_and_leaves_nothing_behind() {
    let base = dir("blocked");
    std::fs::create_dir_all(&base).unwrap();
    let file = base.join("in-the-way");
    std::fs::write(&file, "x").unwrap();
    let mut s = Session::new();
    for path in [file.clone(), file.join("run")] {
        let e = s.execute("rule_pack", &json!({"path": path})).unwrap_err();
        assert!(e.starts_with("output_unwritable: "), "{e}");
    }
    // The last file can't be made: a directory stands where it goes, and everything before it is taken away again.
    let run = base.join("run");
    std::fs::create_dir_all(run.join("rule-pack-manifest.json")).unwrap();
    let e = s.execute("rule_pack", &json!({"path": run})).unwrap_err();
    assert!(e.starts_with("output_unwritable: "), "{e}");
    let mut left: Vec<String> = std::fs::read_dir(&run).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["rule-pack-manifest.json"]);
    // One in the packs directory.
    std::fs::remove_dir_all(&run).unwrap();
    std::fs::create_dir_all(run.join("packs").join("clinic.json")).unwrap();
    let e = s.execute("rule_pack", &json!({"path": run})).unwrap_err();
    assert!(e.starts_with("output_unwritable: "), "{e}");
    let files = |d: &std::path::Path| -> Vec<String> {
        std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
    };
    assert_eq!(files(&run), ["packs"]);
    assert_eq!(files(&run.join("packs")), ["clinic.json"]);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn hostile_params_are_refused_before_any_file_is_touched() {
    let base = dir("hostile");
    let out = base.join("out");
    let mut s = Session::new();
    let before = s.model.clone();
    let long = "x".repeat(5000);
    let cases: Vec<(Value, &str)> = vec![
        (Value::Null, "invalid_params"),
        (json!([]), "invalid_params"),
        (json!("x"), "invalid_params"),
        (json!({}), "invalid_params"),
        (json!({"path": 5}), "invalid_params"),
        (json!({"path": ""}), "invalid_params"),
        (json!({"path": "   "}), "invalid_params"),
        (json!({"path": long}), "invalid_params"),
        (json!({"path": "a\0b"}), "invalid_params"),
        (json!({"path": out, "archetype": "office"}), "invalid_params"),
    ];
    for (p, code) in &cases {
        let e = s.execute("rule_pack", p).unwrap_err();
        assert!(e.starts_with(&format!("{code}: ")), "{p}: {e}");
        assert!(e.chars().count() < 1000, "a refusal stays short: {} characters", e.chars().count());
    }
    assert!(!out.exists(), "nothing was made");
    // A request that is fine is the control: it is the table that refuses, not the command.
    let r = s.execute("rule_pack", &json!({"path": out})).unwrap();
    assert_eq!(r["packs"].as_array().unwrap().len(), 5);
    assert!(out.join("rule-pack-manifest.json").is_file());
    std::fs::remove_dir_all(&base).unwrap();
    assert!(s.model.check().is_ok());
    assert!(std::sync::Arc::ptr_eq(&s.model, &before), "the command does not touch the session's project");
}

#[test]
fn the_catalog_lists_the_command_with_a_params_doc() {
    let c = crate::engine::find("rule_pack").unwrap();
    for key in ["path", "rule-pack-manifest.json", "validation-report.json", "packs", "office", "airport", "deterministic"] {
        assert!(c.params.contains(key), "the params doc names {key}");
    }
}
