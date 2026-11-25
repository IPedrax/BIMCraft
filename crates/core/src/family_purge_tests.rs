//! FE-19: what Purge Unused offers in a family, and only that: a parameter or a nested family is offered only where nothing
//! names it (a formula, a form, a label, a dimension, a type's value, a visibility formula, a connector, a nested instance,
//! a reporting or a shared parameter), the project holds no value of it, and the family
//! passes its checks without it.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A family of `category` with type `A` and a length parameter `W`: what is nested.
fn nested(s: &mut Session, name: &str, category: &str) {
    run(
        s,
        "family",
        json!({"name": name, "category": category, "parameters": [{"name": "W", "value": 100}], "types": {"A": {}},
               "forms": [{"profile": [[0, 0], ["W", 0], ["W", 50], [0, 50]], "to": 10}]}),
    );
}

/// A family with a parameter used only by each kind of reference, and `Free` and `Loose`, which nothing uses; nested `Placed`
/// (placed) and `Idle` (not).
fn host(s: &mut Session) {
    for (n, category) in [("Placed", "furniture"), ("Idle", "generic")] {
        nested(s, n, category);
    }
    run(
        s,
        "family",
        json!({
            "name": "Host",
            "category": "generic",
            "parameters": [
                {"name": "Formulaic", "value": 10},
                {"name": "Twice", "formula": "Formulaic * 2"},
                {"name": "Deep", "value": 300},
                {"name": "Labelled", "kind": "text", "value": "x"},
                {"name": "Measured", "value": 400},
                {"name": "Typed", "value": 1},
                {"name": "Shown", "kind": "yesno", "value": true},
                {"name": "Dotted", "kind": "yesno", "value": true},
                {"name": "Volts", "kind": "number", "value": 220},
                {"name": "Passed", "value": 80},
                {"name": "Read", "reporting": true},
                {"name": "Common", "value": 5, "shared": "5a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"},
                {"name": "Handle", "kind": "familytype", "of": "furniture"},
                {"name": "Free", "value": 7},
                {"name": "Loose", "kind": "text", "value": "y"}
            ],
            "types": {"T": {"Typed": 2}},
            "ref_planes": [{"guid": "7e000000-0000-4000-8000-000000000001", "name": "Side", "start": [400, -1000, 0], "end": [400, 1000, 0], "up": [0, 0, 1]}],
            "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": "Deep", "visible": "Shown"}],
            "symbols": [{"text": {"at": [0, 0], "height": 100, "value": "{Labelled}"}}, {"dot": {"centre": [0, 0], "radius": 10}, "visible": "Dotted"}],
            "dimensions": [
                {"refs": [{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}, {"plane": "7e000000-0000-4000-8000-000000000001"}], "view": "plan", "at": [0, -500, 0], "label": "Measured"},
                {"refs": [{"plane": "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3"}, {"plane": "7e000000-0000-4000-8000-000000000001"}], "view": "plan", "at": [0, -700, 0], "label": "Read"}
            ],
            "connectors": [{"kind": "power", "system": "power-balanced", "voltage": "Volts"}],
        }),
    );
    for n in ["Placed", "Idle"] {
        let definition = serde_json::to_value(&s.model.families[n]).unwrap();
        run(s, "family", json!({"name": "Host", "nest": {"definition": definition, "name": n}}));
    }
    run(s, "family", json!({"name": "Host", "nested_instances": [{"family": "Placed", "type": "A", "params": {"W": "Passed"}}]}));
}

#[test]
fn purge_offers_only_what_nothing_uses() {
    let mut s = Session::new();
    host(&mut s);
    let f = &s.model.families["Host"];
    let u = f.unused(&|_| false, &|m| s.model.materials.contains_key(m));
    // Twice is a formula nothing reads, Handle a Family Type parameter nothing reads: unused too, though they read others.
    assert_eq!(u.parameters, ["Twice", "Handle", "Free", "Loose"].map(String::from), "nothing else is unused");
    assert_eq!(u.nested, vec!["Idle".to_string()], "Placed is placed");
}

#[test]
fn a_parameter_a_placed_component_holds_a_value_of_is_not_offered() {
    let mut s = Session::new();
    host(&mut s);
    let f = &s.model.families["Host"];
    let u = f.unused(&|p| p == "Free", &|m| s.model.materials.contains_key(m));
    assert_eq!(u.parameters, ["Twice", "Handle", "Loose"].map(String::from), "Free is held");
}

#[test]
fn what_is_purged_goes_what_it_freed_is_offered_next_and_the_family_still_checks() {
    let mut s = Session::new();
    host(&mut s);
    let has = |m: &str| s.model.materials.contains_key(m);
    let mut f = s.model.families["Host"].clone();
    let first = f.unused(&|_| false, &has);
    f.purge(&first.parameters, &first.nested);
    assert!(!f.parameters.iter().any(|p| first.parameters.contains(&p.name)));
    assert!(!f.nested.iter().any(|n| n.name == "Idle"));
    // What only they read is free now: Formulaic (Twice read it).
    let second = f.unused(&|_| false, &has);
    assert_eq!(second.parameters, ["Formulaic"].map(String::from));
    assert!(second.nested.is_empty());
    f.purge(&second.parameters, &second.nested);
    assert!(f.unused(&|_| false, &has).is_empty(), "nothing left to offer");
    let left: Vec<&str> = f.parameters.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(left, ["Deep", "Labelled", "Measured", "Typed", "Shown", "Dotted", "Volts", "Passed", "Read", "Common"]);
    assert_eq!(f.nested.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Placed"]);
    assert_eq!(f.problems(&has).0, Vec::<String>::new(), "it still checks");
}
