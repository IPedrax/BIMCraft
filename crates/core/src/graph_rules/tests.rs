//! The packs and the checker: the traced house passes, and the hand-broken copies fail the rule they break.

use serde_json::{Value, json};

use super::*;
use crate::engine::Session;

const KINDS: [(&str, &str, f64); 13] = [
    ("single_bedroom", "bedroom", 9.304),
    ("bedroom_closet", "closet", 1.75),
    ("laundry", "laundry", 0.77),
    ("master_bedroom", "bedroom", 13.4),
    ("master_closet", "closet", 2.91),
    ("bath", "bath", 4.34),
    ("heating", "utility", 0.82),
    ("hall", "hall", 2.92),
    ("living", "living", 14.46),
    ("library", "library", 2.69),
    ("kitchen_dining", "kitchen", 17.25),
    ("entry_closet", "closet", 0.8),
    ("porch", "porch", 6.09),
];

fn door(a: &str, b: &str, op: &str) -> Value {
    json!({"kind": "door", "from": a, "to": b, "operation": op, "width_mm": 800.0})
}

fn open(a: &str, b: &str) -> Value {
    json!({"kind": "open", "from": a, "to": b, "length_mm": 1000.0})
}

/// The traced house as the extractor writes it, with the approved kinds put on its nodes.
fn traced() -> Value {
    json!({
        "entry": {"room": "kitchen_dining", "door_id": 100029},
        "nodes": KINDS.iter().map(|(id, kind, area)| json!({"id": id, "kind": kind, "area_m2": area})).collect::<Vec<_>>(),
        "edges": [
            door("single_bedroom", "hall", "swing"), door("bath", "hall", "swing"), door("master_bedroom", "hall", "swing"),
            door("hall", "living", "swing"), door("single_bedroom", "bedroom_closet", "sliding"),
            door("laundry", "hall", "sliding"), door("heating", "hall", "sliding"),
            door("master_bedroom", "master_closet", "sliding"), door("kitchen_dining", "entry_closet", "sliding"),
            door("living", "porch", "sliding"),
            open("living", "kitchen_dining"), open("living", "library"), open("library", "kitchen_dining"),
            {"kind": "door", "from": "kitchen_dining", "to": "exterior", "exterior": true, "operation": "swing"},
            {"kind": "adjacent", "from": "bath", "to": "heating", "length_mm": 700.0},
        ],
    })
}

fn house(g: &Value) -> Vec<Check> {
    let pack = Pack::embedded("house").unwrap();
    check(pack, &Graph::from_value(g).unwrap()).unwrap()
}

fn failing(checks: &[Check]) -> Vec<&str> {
    checks.iter().filter(|c| c.status == Status::Fail).map(|c| c.id.as_str()).collect()
}

fn edges(g: &mut Value) -> &mut Vec<Value> {
    g["edges"].as_array_mut().unwrap()
}

fn drop_edge(g: &mut Value, a: &str, b: &str) {
    edges(g).retain(|e| !(e["kind"] != "adjacent" && [&e["from"], &e["to"]].iter().all(|x| *x == a || *x == b)));
}

#[test]
fn every_embedded_pack_parses_and_each_rule_has_an_evaluator() {
    for archetype in ["house", "office", "clinic", "hospital"] {
        let pack = Pack::embedded(archetype).unwrap();
        assert_eq!(pack.archetype, archetype);
        let mut graph = traced();
        if archetype != "house" {
            graph = json!({"entry": "a", "nodes": [{"id": "a", "kind": "x", "role": "public", "area_m2": 1.0}], "edges": []});
        }
        let checks = check(pack, &Graph::from_value(&graph).unwrap()).unwrap();
        let said: Vec<&str> = checks.iter().map(|c| c.id.as_str()).collect();
        let mut rules: Vec<&str> = pack.rules.iter().map(|r| r.id.as_str()).collect();
        if archetype == "house" {
            // The swing rules follow the pack's own.
            rules.extend(crate::generate::swing::rules().unwrap().iter().map(|r| r.id.as_str()));
        }
        assert_eq!(said, rules, "{archetype}: one check for each rule, in the pack's order");
    }
    assert!(Pack::embedded("barn").unwrap_err().contains("house"));
}

#[test]
fn the_house_pack_is_exploratory_from_one_trace_and_keeps_to_its_own_ids() {
    let pack = Pack::embedded("house").unwrap();
    assert_eq!(pack.kinds.len(), 14);
    // HR-001..009 were authored from one plan; Unit 1's trace is a second source that added HR-010..013.
    assert_eq!(pack.reference_plans.len(), 1);
    assert!(
        pack.sources.iter().any(|s| s.ends_with("houses/traced-house")) && pack.sources.iter().any(|s| s.ends_with("houses/unit1-house"))
    );
    assert!(pack.rules.iter().all(|r| r.classification == "exploratory" && r.id.starts_with("HR-")));
    for (name, role) in [("bedroom", "private"), ("closet", "support"), ("hall", "circulation"), ("porch", "public")] {
        assert_eq!(pack.kinds[name].role, role);
    }
}

#[test]
fn the_legacy_packs_are_hard_and_say_who_approved_them() {
    for archetype in ["office", "clinic", "hospital"] {
        let pack = Pack::embedded(archetype).unwrap();
        let ids: Vec<&str> = pack.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["GR-001", "GR-002", "GR-003", "GR-004", "GR-005", "GR-006", "PR-001"]);
        assert!(pack.rules.iter().all(|r| r.classification == "hard" && r.approved_by.is_some()), "{archetype}");
    }
}

#[test]
fn the_traced_house_passes_every_rule() {
    let checks = house(&traced());
    assert_eq!(checks.len(), 13 + crate::generate::swing::rules().unwrap().len());
    assert!(failing(&checks).is_empty(), "{checks:?}");
}

#[test]
fn the_hand_broken_copies_fail_the_rules_they_break() {
    // A closet with two doors.
    let mut g = traced();
    edges(&mut g).push(door("master_closet", "hall", "sliding"));
    assert_eq!(failing(&house(&g)), ["HR-006"]);

    // A bedroom reached only through a bedroom.
    let mut g = traced();
    drop_edge(&mut g, "single_bedroom", "hall");
    edges(&mut g).push(door("single_bedroom", "master_bedroom", "swing"));
    let c = house(&g);
    let f = failing(&c);
    assert!(f.contains(&"HR-008") && !f.contains(&"HR-009"), "{f:?}");

    // No entry.
    let mut g = traced();
    edges(&mut g).retain(|e| e["exterior"] != true);
    g["entry"] = Value::Null;
    let c = house(&g);
    let f = failing(&c);
    assert!(f.contains(&"HR-009") && f.contains(&"HR-008"), "{f:?}");

    // A porch off a closet.
    let mut g = traced();
    drop_edge(&mut g, "porch", "living");
    edges(&mut g).push(door("porch", "master_closet", "sliding"));
    let c = house(&g);
    let f = failing(&c);
    assert!(f.contains(&"HR-006") && f.contains(&"HR-007"), "{f:?}");

    // The entry in a bedroom.
    let mut g = traced();
    g["entry"] = json!({"room": "master_bedroom"});
    assert!(failing(&house(&g)).contains(&"HR-009"));
}

#[test]
fn the_entry_may_be_a_string_an_object_or_nothing() {
    for entry in [json!("kitchen_dining"), json!({"room": "kitchen_dining"}), Value::Null] {
        let mut g = traced();
        g["entry"] = entry.clone();
        let parsed = Graph::from_value(&g).unwrap();
        assert_eq!(parsed.entry.is_some(), !entry.is_null());
    }
    let mut g = traced();
    g.as_object_mut().unwrap().remove("entry");
    assert!(Graph::from_value(&g).unwrap().entry.is_none());
}

#[test]
fn a_space_without_a_kind_or_of_an_unknown_one_fails_hr_001_and_is_named() {
    let mut g = traced();
    g["nodes"][0].as_object_mut().unwrap().remove("kind");
    g["nodes"][1]["kind"] = json!("vault");
    let c = house(&g);
    let hr1 = c.iter().find(|c| c.id == "HR-001").unwrap();
    assert_eq!(hr1.status, Status::Fail);
    assert!(hr1.detail.contains("single_bedroom") && hr1.detail.contains("bedroom_closet"), "{}", hr1.detail);
}

#[test]
fn counts_areas_and_connections_are_judged() {
    let mut g = traced();
    g["nodes"].as_array_mut().unwrap().retain(|n| n["kind"] != "bath");
    edges(&mut g).retain(|e| e["from"] != "bath" && e["to"] != "bath");
    assert!(failing(&house(&g)).contains(&"HR-003"));

    let mut g = traced();
    g["nodes"][0]["area_m2"] = json!(1.0);
    assert_eq!(failing(&house(&g)), ["HR-004"]);
    g["nodes"][0].as_object_mut().unwrap().remove("area_m2");
    assert_eq!(failing(&house(&g)), ["HR-004"]);

    let mut g = traced();
    edges(&mut g).push(door("bath", "porch", "swing"));
    assert!(failing(&house(&g)).contains(&"HR-005"));

    // A folding door between a bedroom and a hall is no means the pack allows, and no closet's or laundry's.
    let mut g = traced();
    g["edges"][0]["operation"] = json!("folding");
    assert_eq!(failing(&house(&g)), ["HR-005", "HR-012"]);
    g["edges"][0]["operation"] = json!("double-swing");
    assert!(failing(&house(&g)).is_empty());

    // An edge to something that is no space.
    let mut g = traced();
    edges(&mut g).push(door("bath", "nowhere", "swing"));
    assert!(failing(&house(&g)).contains(&"HR-002"));
}

#[test]
fn the_same_graph_gives_the_same_checks() {
    assert_eq!(format!("{:?}", house(&traced())), format!("{:?}", house(&traced())));
}

#[test]
fn a_graph_that_cannot_be_read_says_why() {
    assert!(Graph::from_value(&json!(5)).is_err());
    assert!(Graph::from_value(&json!({"nodes": 5, "edges": []})).is_err());
    assert!(Graph::from_value(&json!({"nodes": [{"kind": "x"}], "edges": []})).is_err());
    assert!(Graph::from_value(&json!({"nodes": [{"id": "a", "area_m2": "big"}], "edges": []})).is_err());
    assert!(Graph::from_value(&json!({"nodes": [], "edges": [{"from": "a"}]})).is_err());
    assert!(Graph::from_value(&json!({"nodes": [], "edges": [], "entry": 7})).is_err());
    let many: Vec<Value> = (0..=MAX_ITEMS).map(|i| json!({"id": format!("n{i}")})).collect();
    assert!(Graph::from_value(&json!({"nodes": many, "edges": []})).is_err());
}

// ---- the Unit 1 trace

const UNIT1: [(&str, &str, f64); 17] = [
    ("living", "living", 25.194),
    ("kitchen", "kitchen", 13.465),
    ("dining", "dining", 20.999),
    ("entry", "hall", 10.487),
    ("hall", "hall", 5.235),
    ("bed_1", "bedroom", 15.415),
    ("ens", "bath", 4.805),
    ("wir", "closet", 4.22),
    ("bed_2", "bedroom", 12.819),
    ("robe_2", "closet", 1.413),
    ("bed_3", "bedroom", 11.593),
    ("robe_3", "closet", 1.37),
    ("linen", "closet", 1.222),
    ("bath", "bath", 8.483),
    ("laundry", "laundry", 1.915),
    ("garage", "garage", 37.083),
    ("deck", "deck", 20.422),
];

/// A swing door as the extractor writes it, with the room it opens into and its jamb.
fn swung(a: &str, b: &str, into: &str, hinge: &str) -> Value {
    json!({"kind": "door", "from": a, "to": b, "operation": "swing", "width_mm": 820.0, "opens_into": into, "hinge": hinge})
}

/// The Unit 1 house trace, its doors and open edges as `topology --out` reads them from its script.
fn unit1() -> Value {
    json!({
        "entry": {"room": "entry", "door_id": 100035},
        "nodes": UNIT1.iter().map(|(id, kind, area)| json!({"id": id, "kind": kind, "area_m2": area})).collect::<Vec<_>>(),
        "edges": [
            swung("bed_1", "ens", "ens", "end"), door("bed_2", "robe_2", "folding"), door("bed_3", "robe_3", "folding"),
            door("dining", "deck", "sliding"), door("living", "deck", "sliding"),
            swung("entry", "bed_1", "bed_1", "end"), swung("entry", "garage", "garage", "end"),
            {"kind": "door", "from": "entry", "to": "exterior", "exterior": true, "operation": "swing", "opens_into": "entry", "hinge": "end"},
            {"kind": "door", "from": "garage", "to": "exterior", "exterior": true, "operation": "swing", "opens_into": "garage", "hinge": "end"},
            {"kind": "door", "from": "garage", "to": "exterior", "exterior": true, "operation": "folding"},
            swung("hall", "bath", "bath", "end"), swung("hall", "bed_2", "bed_2", "start"), swung("hall", "bed_3", "bed_3", "end"),
            door("hall", "laundry", "folding"), door("hall", "linen", "folding"),
            open("bed_1", "wir"), open("entry", "hall"), open("kitchen", "dining"), open("kitchen", "entry"),
            open("living", "dining"), open("living", "kitchen"),
            {"kind": "adjacent", "from": "bath", "to": "ens", "length_mm": 700.0},
        ],
    })
}

/// Moves the one door or open edge `a`-`b` so that its end `old` is `new`.
fn rehome(g: &mut Value, a: &str, b: &str, old: &str, new: &str) {
    let hit: Vec<&mut Value> =
        edges(g).iter_mut().filter(|e| e["kind"] != "adjacent" && [&e["from"], &e["to"]].iter().all(|x| *x == a || *x == b)).collect();
    assert_eq!(hit.len(), 1, "{a}-{b}");
    for e in hit {
        for key in ["from", "to", "opens_into"] {
            if e[key] == old {
                e[key] = json!(new);
            }
        }
    }
}

#[test]
fn the_unit1_house_passes_every_rule() {
    let checks = house(&unit1());
    assert!(failing(&checks).is_empty(), "{checks:?}");
}

#[test]
fn the_pack_knows_the_garage_and_the_deck() {
    let pack = Pack::embedded("house").unwrap();
    for (name, role) in [("garage", "service"), ("deck", "public")] {
        let k = &pack.kinds[name];
        assert_eq!(k.role, role);
        assert_eq!(k.count.0, 0.0, "{name}: a house without one still passes");
        assert!(k.count.1.is_some_and(|hi| hi >= 1.0));
    }
    assert!((15.0..=37.0).contains(&pack.kinds["garage"].area_m2.0) && pack.kinds["garage"].area_m2.1.unwrap() >= 37.1);
    assert!(pack.kinds["deck"].area_m2.0 <= 20.4 && pack.kinds["deck"].area_m2.1.unwrap() >= 20.4);
}

#[test]
fn hand_broken_unit1_graphs_fail_the_named_pack_rule_and_nothing_else() {
    let ids: Vec<&str> = Pack::embedded("house").unwrap().rules.iter().map(|r| r.id.as_str()).collect();
    let mut cases: Vec<(&str, Value, &str)> = Vec::new();

    // The garage only through a bedroom.
    let mut g = unit1();
    rehome(&mut g, "entry", "garage", "entry", "bed_1");
    cases.push(("garage through a bedroom", g, "HR-010"));

    // The deck only off a bedroom.
    let mut g = unit1();
    drop_edge(&mut g, "dining", "deck");
    rehome(&mut g, "living", "deck", "living", "bed_1");
    cases.push(("deck off a bedroom", g, "HR-011"));

    // A folding robe door off the living room.
    let mut g = unit1();
    rehome(&mut g, "bed_3", "robe_3", "bed_3", "living");
    cases.push(("folding robe off the living room", g, "HR-012"));

    // No way from the entry to the hall.
    let mut g = unit1();
    drop_edge(&mut g, "entry", "hall");
    cases.push(("no path to the hall", g, "HR-013"));

    for (what, g, rule) in cases {
        let checks = house(&g);
        let f = failing(&checks);
        assert!(f.contains(&rule), "{what}: {f:?}");
        assert!(f.iter().all(|id| ids.contains(id)), "{what}: fails a rule that is not the pack's: {f:?}");
    }
}

#[test]
fn legal_edits_of_the_unit1_house_still_pass() {
    let mut g = unit1();
    rehome(&mut g, "entry", "garage", "entry", "hall");
    assert!(failing(&house(&g)).is_empty(), "garage off the hall");

    let mut g = unit1();
    drop_edge(&mut g, "living", "deck");
    assert!(failing(&house(&g)).is_empty(), "deck off the dining only");

    let mut g = unit1();
    rehome(&mut g, "bed_3", "robe_3", "bed_3", "hall");
    assert!(failing(&house(&g)).is_empty(), "robe folding off the hall");
}

#[test]
fn a_garage_with_no_door_to_a_hall_or_the_entry_fails() {
    let mut g = unit1();
    rehome(&mut g, "entry", "garage", "entry", "kitchen");
    let c = house(&g);
    assert!(failing(&c).contains(&"HR-010"), "{:?}", failing(&c));
}

#[test]
fn the_judged_swing_rules_leave_a_side_alone_where_the_two_traces_disagree() {
    use crate::generate::swing;
    for id in ["SW-001", "SW-002"] {
        let r = swing::judged_rules().unwrap().iter().find(|r| r.id == id).unwrap();
        assert_eq!(r.hinge, "either", "{id}: the traced house hangs these from the start, Unit 1's from both");
        assert_eq!(r.opens_into, swing::rules().unwrap().iter().find(|r| r.id == id).unwrap().opens_into, "{id}");
    }
    for id in ["SW-003", "SW-004"] {
        let (judged, traced) = (swing::judged_rules().unwrap(), swing::rules().unwrap());
        let (j, t) = (judged.iter().find(|r| r.id == id).unwrap(), traced.iter().find(|r| r.id == id).unwrap());
        assert_eq!((&j.opens_into, &j.hinge), (&t.opens_into, &t.hinge), "{id}");
    }
}

// ---- the command

#[test]
fn the_command_reads_a_graph_given_inline_or_by_path_and_never_touches_the_project() {
    let mut s = Session::new();
    let before = s.model.clone();
    let r = s.execute("rule_check", &json!({"archetype": "house", "graph": traced()})).unwrap();
    assert_eq!(r["pass"], true);
    assert_eq!(r["archetype"], "house");
    assert_eq!(r["pack_id"], "house.graph-rules/1");
    let n = 13 + crate::generate::swing::rules().unwrap().len();
    assert_eq!(r["checks"].as_array().unwrap().len(), n);
    assert_eq!(r["summary"], json!({"pass": n, "fail": 0}));

    let path = std::env::temp_dir().join(format!("bimcraft-rule-check-{}.json", std::process::id()));
    let mut g = traced();
    edges(&mut g).push(door("master_closet", "hall", "sliding"));
    std::fs::write(&path, g.to_string()).unwrap();
    let r = s.execute("rule_check", &json!({"archetype": "house", "graph": path})).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(r["pass"], false);
    assert_eq!(r["checks"][5]["id"], "HR-006");
    assert_eq!(r["checks"][5]["status"], "FAIL");
    assert!(s.model == before);
}

/// The traced graph with fields put on the first door edge between `a` and `b`.
fn with_swing(a: &str, b: &str, fields: Value) -> Value {
    let mut g = traced();
    for e in edges(&mut g) {
        if e["kind"] == "door" && [&e["from"], &e["to"]].iter().all(|x| *x == a || *x == b || *x == "exterior") && e["operation"] == "swing"
        {
            for (k, v) in fields.as_object().unwrap() {
                e[k] = v.clone();
            }
            break;
        }
    }
    g
}

fn status_of(checks: &[Check], id: &str) -> Status {
    checks.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no {id}")).status
}

#[test]
fn swing_rules_judge_the_side_and_the_hinge_of_a_swing_door() {
    let ok = house(&with_swing("single_bedroom", "hall", json!({"opens_into": "single_bedroom", "hinge": "start"})));
    assert!(failing(&ok).is_empty(), "{:?}", failing(&ok));
    assert_eq!(status_of(&ok, "SW-002"), Status::Pass);

    let into_hall = house(&with_swing("single_bedroom", "hall", json!({"opens_into": "hall", "hinge": "start"})));
    assert_eq!(failing(&into_hall), vec!["SW-002"]);
    // The two traces hang bedroom doors from both jambs, so that half of SW-002 is not enforced; SW-004's still is.
    let either_hand = house(&with_swing("single_bedroom", "hall", json!({"opens_into": "single_bedroom", "hinge": "end"})));
    assert!(failing(&either_hand).is_empty(), "{:?}", failing(&either_hand));
    let wrong_hand = house(&with_swing("hall", "living", json!({"opens_into": "hall", "hinge": "start"})));
    assert_eq!(failing(&wrong_hand), vec!["SW-004"]);

    // Not judged without the fields.
    assert!(failing(&house(&traced())).is_empty());
}

#[test]
fn the_exterior_door_is_judged_by_the_exterior_rule() {
    let ok = house(&with_swing("kitchen_dining", "exterior", json!({"opens_into": "kitchen_dining", "hinge": "end"})));
    assert!(failing(&ok).is_empty(), "{:?}", failing(&ok));
    let out = house(&with_swing("kitchen_dining", "exterior", json!({"opens_into": "exterior", "hinge": "end"})));
    assert_eq!(failing(&out), vec!["SW-003"]);
    // Whatever room it is in.
    let mut g = with_swing("kitchen_dining", "exterior", json!({"opens_into": "kitchen_dining", "hinge": "start"}));
    for n in g["nodes"].as_array_mut().unwrap() {
        if n["id"] == "kitchen_dining" {
            n["kind"] = json!("living");
        }
    }
    assert!(failing(&house(&g)).contains(&"SW-003"));
}

#[test]
fn a_pair_with_no_swing_rule_is_not_judged() {
    let g = with_swing("master_bedroom", "hall", json!({"opens_into": "hall", "hinge": "end"}));
    // bedroom-hall has a rule, so this fails; a bath-bedroom swing door has none.
    assert_eq!(failing(&house(&g)), vec!["SW-002"]);
    let mut g = traced();
    edges(&mut g)
        .push(json!({"kind": "door", "from": "bath", "to": "single_bedroom", "operation": "swing", "opens_into": "bath", "hinge": "end"}));
    assert!(!failing(&house(&g)).iter().any(|i| i.starts_with("SW-")));
}

#[test]
fn a_hostile_swing_field_is_refused_with_a_message() {
    let mut g = traced();
    edges(&mut g)[0]["opens_into"] = json!(5);
    let e = Graph::from_value(&g).unwrap_err();
    assert!(e.contains("opens_into"), "{e}");
    let mut g = traced();
    edges(&mut g)[0]["hinge"] = json!({"a": 1});
    assert!(Graph::from_value(&g).unwrap_err().contains("hinge"));
}

#[test]
fn the_command_names_what_is_wrong_with_its_params() {
    let mut s = Session::new();
    for (p, word) in [
        (json!({"archetype": "barn", "graph": {}}), "house"),
        (json!({"graph": {}}), "archetype"),
        (json!({"archetype": "house"}), "graph"),
        (json!({"archetype": "house", "graph": "/nonexistent/g.json"}), "/nonexistent/g.json"),
        (json!({"archetype": "house", "graph": 5}), "graph"),
        (json!({"archetype": "house", "graph": ""}), "graph"),
        (json!({"archetype": "house", "graph": {}, "extra": 1}), "extra"),
        (json!({"archetype": "office", "graph": {"nodes": [], "edges": []}}), "entry"),
    ] {
        let e = s.execute("rule_check", &p).unwrap_err();
        assert!(e.contains(word), "{p}: {e}");
    }
}
