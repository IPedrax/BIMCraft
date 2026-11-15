//! A family and the pack it names. A table the pack's file does not have narrows nothing, one it has narrows as it did, and the
//! pack's rules judge every graph either way. The packs are the built-in files, with a table taken out in memory where a test
//! needs one that lacks it; no kind of any pack is written here.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::family::{self, Conn, Family};
use super::register_tests::house_family;
use super::{Request, build_family};
use crate::graph_rules::Pack;

/// A built-in pack's file.
fn pack_file(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join("graph-rules").join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("a pack file")).expect("a pack is JSON")
}

/// A built-in pack, with some of its tables taken out.
fn pack_without(name: &str, tables: &[&str]) -> Pack {
    let mut file = pack_file(name);
    for t in tables {
        assert!(file.as_object_mut().expect("an object").remove(*t).is_some(), "the {name} pack has no `{t}` to take out");
    }
    Pack::parse(&file.to_string()).expect("the pack still reads")
}

fn house() -> Value {
    serde_json::from_str(house_family()).expect("the house family is JSON")
}

/// The house family with one link given a type whose means the house pack does not list for that pair of kinds.
fn house_with_an_unlisted_link() -> String {
    let pack = Pack::embedded("house").expect("the house pack");
    let mut file = house();
    let kind = |file: &Value, end: &Value| file["rooms"][end.as_str().expect("a room type")]["kind"].as_str().expect("a kind").to_string();
    let links = file["links"].as_array().expect("links").len();
    for i in 0..links {
        let (a, b) = (kind(&file, &file["links"][i]["from"]), kind(&file, &file["links"][i]["to"]));
        for conn in [Conn::OpenFull, Conn::Single, Conn::Sliding, Conn::Bifold] {
            let listed = pack.connections.iter().chain(&pack.folding_connections).any(|p| {
                ((p.pair[0] == a && p.pair[1] == b) || (p.pair[0] == b && p.pair[1] == a)) && p.means.iter().any(|m| m == conn.means())
            });
            if !listed {
                file["links"][i]["types"] = json!([conn.id()]);
                return file.to_string();
            }
        }
    }
    panic!("the house pack lists every means for every link of the house family");
}

/// The house family with its first room type of a kind, and a role, no pack has.
fn house_with_an_unknown_kind() -> String {
    let mut file = house();
    let key = file["rooms"].as_object().expect("rooms").keys().next().expect("a room type").clone();
    file["rooms"][&key]["kind"] = json!("no_such_kind");
    file["rooms"][&key]["role"] = json!("no_such_role");
    file.to_string()
}

/// A family's text with its `pack_id` set, its rooms left in the file's order.
fn naming(text: &str, pack: &str) -> String {
    let at = text.find("\"pack_id\":").expect("a family has a `pack_id`");
    let end = at + text[at..].find(',').expect("a key follows `pack_id`");
    format!("{}\"pack_id\": \"{pack}\"{}", &text[..at], &text[end..])
}

fn file(files: &[(&'static str, String)], name: &str) -> Value {
    let text = files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.as_str()).unwrap_or("null");
    serde_json::from_str(text).unwrap_or(Value::Null)
}

#[test]
fn the_built_in_packs_say_which_tables_they_have() {
    let has = |name: &str| {
        let pack = Pack::embedded(name).expect("a built-in pack");
        (pack.has_kinds, pack.has_connections)
    };
    assert_eq!(has("house"), (true, true));
    assert_eq!(has("studio-office"), (true, true));
    assert_eq!(has("office-pattern"), (true, true));
    assert_eq!(has("medical-day"), (true, false));
    assert_eq!(has("hospital"), (false, false));
    assert_eq!(has("clinic"), (false, false));
}

#[test]
fn a_pack_that_has_a_table_is_narrowed_by_it_as_before() {
    let kind = Family::parse(&house_with_an_unknown_kind());
    assert!(kind.is_err_and(|e| e.contains("is not in the `house` pack")), "a kind the pack does not have");
    let link = Family::parse(&house_with_an_unlisted_link());
    assert!(link.is_err_and(|e| e.contains("is not a means the pack lists")), "a link the pack does not list");
    // A pack with one table taken out is still narrowed by the other.
    let no_links = pack_without("house", &["connections", "folding_connections"]);
    assert!(Family::parse_within(&house_with_an_unknown_kind(), &no_links).is_err_and(|e| e.contains("is not in the")));
    let no_kinds = pack_without("house", &["kinds"]);
    assert!(Family::parse_within(&house_with_an_unlisted_link(), &no_kinds).is_err_and(|e| e.contains("is not a means the pack lists")));
}

#[test]
fn a_pack_with_no_connections_table_narrows_no_link() {
    let pack = pack_without("house", &["connections", "folding_connections"]);
    assert!(pack.has_kinds && !pack.has_connections);
    let fam = Family::parse_within(&house_with_an_unlisted_link(), &pack).expect("any link may be listed");
    assert_eq!(fam.pack_id.as_deref(), Some("house"));
    // A table that is there and empty is a table: it lists no link, so it allows none.
    let mut whole = pack_file("house");
    whole.as_object_mut().expect("an object").remove("folding_connections");
    whole["connections"] = json!([]);
    let empty = Pack::parse(&whole.to_string()).expect("the pack reads");
    assert!(empty.has_connections);
    assert!(Family::parse_within(house_family(), &empty).is_err_and(|e| e.contains("is not a means the pack lists")));
}

#[test]
fn a_pack_with_no_kinds_table_narrows_no_kind_role_count_or_area() {
    let no_kinds = pack_without("house", &["kinds"]);
    assert!(!no_kinds.has_kinds && no_kinds.has_connections);
    // The two copies the pack refuses as too wide (`a_family_that_widens_its_pack_or_breaks_its_schema_is_refused`).
    let area = house_family().replace("\"area_m2\": [7.0, 25.0]", "\"area_m2\": [7.0, 250.0]");
    let count = house_family().replace("\"count\": [1, 6]", "\"count\": [1, 60]");
    for (what, text) in [("area", area), ("count", count)] {
        assert_ne!(text, house_family(), "the {what} was changed");
        assert!(Family::parse(&text).is_err(), "the house pack refuses the wider {what}");
        Family::parse_within(&text, &no_kinds).unwrap_or_else(|e| panic!("no table narrows the {what}: {e}"));
    }
    let neither = pack_without("house", &["kinds", "connections", "folding_connections"]);
    let fam = Family::parse_within(&house_with_an_unknown_kind(), &neither).expect("any kind and any role");
    assert!(fam.types.iter().any(|t| t.kind == "no_such_kind" && t.role == "no_such_role"));
}

/// A family with one room type of every kind of a pack, each joined to the first by a door.
fn a_family_of_every_kind(pack_name: &str) -> String {
    let pack = Pack::embedded(pack_name).expect("a built-in pack");
    let spine = pack.kinds.keys().next().expect("the pack has kinds").clone();
    let (mut rooms, mut links) = (serde_json::Map::new(), Vec::new());
    for (kind, k) in &pack.kinds {
        let first = *kind == spine;
        rooms.insert(
            kind.clone(),
            json!({
                "kind": kind, "role": k.role, "place": if first { "spine" } else { "off_spine" }, "count": [1, 2],
                "area_m2": [8.0, 30.0], "max_side_ratio": if first { Value::Null } else { json!(2.5) }, "min_side_mm": 2000,
                "max_links": 40,
            }),
        );
        if !first {
            links.push(json!({"from": spine, "to": kind, "use": "parent", "types": ["single", "open_full", "sliding", "bifold"]}));
        }
    }
    json!({
        "schema_version": 1, "family": "every-kind", "pack_id": pack_name, "building_type": "hospital",
        "review": {"status": "reviewed"}, "entry": null, "rooms": rooms, "links": links,
    })
    .to_string()
}

#[test]
fn a_family_may_list_any_link_under_the_medical_day_pack_and_no_kind_outside_it() {
    let text = a_family_of_every_kind("medical-day");
    let fam = Family::parse(&text).unwrap_or_else(|e| panic!("the pack has no connections table to refuse a link: {e}"));
    assert_eq!(fam.pack_id.as_deref(), Some("medical-day"));
    // Its kinds table is there, and still says which kinds and roles there are.
    let mut file: Value = serde_json::from_str(&text).expect("json");
    let key = file["rooms"].as_object().expect("rooms").keys().next_back().expect("a room type").clone();
    file["rooms"][&key]["role"] = json!("no_such_role");
    assert!(Family::parse(&file.to_string()).is_err_and(|e| e.contains("its role is")));
    file["rooms"][&key]["kind"] = json!("no_such_kind");
    assert!(Family::parse(&file.to_string()).is_err_and(|e| e.contains("is not in the `medical-day` pack")));
}

#[test]
fn a_family_under_a_pack_with_no_tables_gets_a_passing_check_for_each_of_its_rules() {
    for (name, rooms) in [("hospital", 30), ("clinic", 16)] {
        let fam = Family::parse(&naming(family::builtin(name).expect("built in"), name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(fam.pack_id.as_deref(), Some(name));
        let built =
            build_family(&fam, &Request::new(name.into(), 1, rooms, 0, None, false), true).unwrap_or_else(|r| panic!("{name}: {r}"));
        let report = file(&built.files, "report.json");
        assert_eq!(report["rule_set"], format!("{name}-pack"));
        let pack = Pack::embedded(name).expect("its pack");
        assert!(!pack.rules.is_empty());
        let checks = report["checks"].as_array().expect("checks");
        for rule in &pack.rules {
            let found: Vec<&Value> = checks.iter().filter(|c| c["id"] == rule.id.as_str()).collect();
            assert_eq!(found.len(), 1, "{name}: one check for {}", rule.id);
            assert_eq!(found[0]["status"], "PASS", "{name} {}: {}", rule.id, found[0]["detail"]);
        }
        assert_eq!(report["rules"].as_array().map(Vec::len), Some(pack.rules.len()), "{name}");
        assert_eq!(report["summary"]["fail"], 0);
        assert!(checks.iter().any(|c| c["id"] == "SG-003" && c["status"] == "PASS"), "{name}: the built plan is judged too");
    }
}

#[test]
fn a_graph_that_fails_a_rule_of_a_pack_with_no_tables_is_thrown_away() {
    // The clinic family's kinds do not make the hospital pack's minimum program, so no attempt's graph passes it.
    let fam = Family::parse(&naming(family::builtin("clinic").expect("built in"), "hospital")).expect("no table refuses its kinds");
    let (fewest, _) = fam.count_limits();
    let refused = build_family(&fam, &Request::new("clinic".into(), 1, fewest, 0, None, false), true).err().expect("no plan passes");
    assert_eq!(refused.code, "unplaceable_graph");
    assert!(refused.message.contains("fails the hospital rules") && refused.message.contains("PR-001"), "{}", refused.message);
}

#[test]
fn the_manifest_says_whether_the_family_was_reviewed() {
    let req = Request::new("house".into(), 1, 12, 0, None, false);
    let reviewed = Family::parse(house_family()).expect("the house family");
    assert_eq!(reviewed.review.status, "reviewed");
    let built = build_family(&reviewed, &req, false).expect("a plan");
    assert_eq!(file(&built.files, "manifest.json")["family_review"], "reviewed");

    let text = house_family().replace("\"status\": \"reviewed\"", "\"status\": \"unreviewed\"");
    let unreviewed = Family::parse(&text).expect("the copy parses");
    assert!(unreviewed.unreviewed());
    assert_eq!(build_family(&unreviewed, &req, false).err().map(|r| r.code), Some("unreviewed_family"));
    let allowed = build_family(&unreviewed, &req, true).expect("a plan, with `--unreviewed allow`");
    assert_eq!(file(&allowed.files, "manifest.json")["family_review"], "unreviewed");
    // The review changes nothing else: the same plan is written.
    let text = |files: &[(&'static str, String)], name: &str| files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.clone());
    for name in ["topology-graph.json", "script.bimcraft.txt", "report.json"] {
        assert!(text(&built.files, name).is_some(), "{name} is written");
        assert_eq!(text(&allowed.files, name), text(&built.files, name), "{name}");
    }
}
