//! The built-in medical-day family is data: a file written from its proposal under `fill/`, read with its pack, not yet reviewed.
//! Its kinds come from the pack and the file in memory; none is written here.

use std::path::PathBuf;

use serde_json::Value;

use super::family::{self, Family, Use};
use super::{Request, build};
use crate::graph_rules::Pack;

const NAME: &str = "medical-day";

fn text() -> &'static str {
    family::builtin(NAME).expect("medical-day.json is a built-in family")
}

fn proposal() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/shape-families/fill/medical-day-claude-fable-5-2.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the proposal")).expect("the proposal is JSON")
}

fn file(files: &[(&'static str, String)], name: &str) -> Value {
    let text = files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.as_str()).unwrap_or("null");
    serde_json::from_str(text).unwrap_or(Value::Null)
}

#[test]
fn the_family_names_its_pack_has_no_entry_and_is_not_reviewed() {
    let fam = Family::parse(text()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(fam.pack_id.as_deref(), Some(NAME));
    assert_eq!(fam.building_type, "hospital");
    assert!(fam.entry.is_none(), "a department has no outside door");
    assert!(fam.unreviewed(), "a person reviews it, not its builder");
    let pack = Pack::embedded(NAME).expect("its pack");
    for kind in pack.kinds.keys() {
        assert!(fam.types.iter().any(|t| &t.kind == kind), "no room type of kind {kind}");
    }
    // An isolation room's door to a corridor: the one link that is there whenever both rooms are.
    let required: Vec<_> = fam.links.iter().filter(|l| l.use_ == Use::Required).collect();
    assert_eq!(required.len(), 1);
    assert!(required[0].types.iter().all(|c| !c.is_open()), "a door, not an opening");
}

#[test]
fn the_family_is_the_proposal_with_its_reasons_left_out() {
    let fam: Value = serde_json::from_str(text()).expect("json");
    let mut from = proposal();
    assert_eq!(from["model"], "claude-fable-5-2");
    assert!(fam.get("fill").is_none() && fam.get("model").is_none());
    let sources: Vec<&str> = fam["sources"].as_array().expect("sources").iter().filter_map(Value::as_str).collect();
    assert_eq!(sources.len(), 4, "{sources:?}");
    assert!(sources.iter().any(|s| s.ends_with("shape-families/fill/medical-day-claude-fable-5-2.json")), "{sources:?}");
    for name in ["a112-expected-topology.json", "a112-kinds.json", "a112-room-areas.json"] {
        assert!(sources.contains(&format!("done/fixtures/medical-day/{name}").as_str()), "{sources:?}");
    }
    for key in ["pack_id", "building_type", "entry"] {
        assert_eq!(fam[key], from[key], "{key}");
    }
    // The room types in the proposal's order, which is the order the engine takes them in.
    let keys = |v: &Value| v["rooms"].as_object().expect("rooms").keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&fam), keys(&from));
    for room in from["rooms"].as_object_mut().expect("rooms").values_mut() {
        assert!(room.as_object_mut().expect("a room type").remove("why").is_some());
    }
    for link in from["links"].as_array_mut().expect("links") {
        assert!(link.as_object_mut().expect("a link").remove("why").is_some());
    }
    assert_eq!(fam["rooms"], from["rooms"]);
    assert_eq!(fam["links"], from["links"]);
    for (key, room) in fam["rooms"].as_object().expect("rooms") {
        for field in ["count", "area_m2", "max_side_ratio", "min_side_mm", "max_links"] {
            let s = &room["source"][field];
            let model = s["from"] == "model" && ["low", "medium", "high"].iter().any(|c| s["confidence"] == *c);
            assert!(model || *s == serde_json::json!({"from": "trace"}), "{key}.{field}: {s}");
        }
    }
}

#[test]
fn it_is_refused_until_allowed_and_then_passes_every_rule_of_its_pack() {
    let req = Request::new(NAME.into(), 1, 30, 3, None, false);
    assert_eq!(build(&req, false).err().map(|r| r.code), Some("unreviewed_family"));
    let built = build(&req, true).unwrap_or_else(|r| panic!("{r}"));
    assert!((27..=33).contains(&built.rooms), "{}", built.rooms);
    assert_eq!(file(&built.files, "manifest.json")["family_review"], "unreviewed");
    let graph = file(&built.files, "topology-graph.json");
    assert!(graph["entry"].is_null() || graph["entry"]["room"].is_null(), "{}", graph["entry"]);
    let report = file(&built.files, "report.json");
    let checks = report["checks"].as_array().expect("checks");
    let pack = Pack::embedded(NAME).expect("its pack");
    assert_eq!(pack.rules.len(), 11);
    for rule in &pack.rules {
        let found: Vec<&Value> = checks.iter().filter(|c| c["id"] == rule.id.as_str()).collect();
        assert_eq!(found.len(), 1, "one check for {}", rule.id);
        assert_eq!(found[0]["status"], "PASS", "{}: {}", rule.id, found[0]["detail"]);
    }
    assert_eq!(report["summary"]["fail"], 0);
}
