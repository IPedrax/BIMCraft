//! Connectors with a place: where a pipe, a data cable or an earthing conductor meets a family, which way it faces and how
//! big it is, worked out for a component where it stands. What a network makes of them is a later piece; these are the
//! connectors themselves, through the commands every front end drives.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::language::Language;
use crate::model::Model;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn made(s: &mut Session, p: Value) -> u64 {
    run(s, "component", p)["id"].as_u64().unwrap()
}

fn connectors(s: &Session, id: u64) -> Vec<Value> {
    element_json(&s.model, id)["connectors"].as_array().cloned().unwrap_or_default()
}

fn near(a: &Value, b: [f64; 3]) -> bool {
    a.as_array().is_some_and(|v| v.len() == 3 && v.iter().zip(b).all(|(x, y)| x.as_f64().is_some_and(|x| (x - y).abs() < 1e-6)))
}

/// A trap with its waste out through the floor and an inlet from the wall, sized by its type.
fn trap(s: &mut Session) {
    run(
        s,
        "family",
        json!({
            "name": "Trap", "category": "plumbing", "kind": "sink",
            "parameters": [{"name": "DN", "kind": "length", "value": 50}],
            "types": {"50": {}, "75": {"DN": 75}},
            "connectors": [
                {"name": "Waste", "kind": "pipe", "domain": "waste", "flow": "out", "at": [100, -200, 0], "direction": [0, 0, -3], "size": "DN"},
                {"kind": "pipe", "domain": "cold", "flow": "in", "at": [0, 0, "DN * 10"], "direction": [0, 1, 0], "size": 20}
            ]
        }),
    );
}

#[test]
fn a_pipe_connector_stands_where_its_component_does_and_faces_with_it() {
    let mut s = Session::new();
    trap(&mut s);
    let plain = made(&mut s, json!({"family": "Trap", "at": [1000, 2000]}));
    let c = connectors(&s, plain);
    assert_eq!(c.len(), 2, "{c:?}");
    assert_eq!((c[0]["name"].as_str(), c[0]["kind"].as_str(), c[0]["domain"].as_str()), (Some("Waste"), Some("pipe"), Some("waste")));
    assert_eq!((c[0]["flow"].as_str(), c[0]["size"].as_f64()), (Some("out"), Some(50.0)));
    assert!(near(&c[0]["at"], [1100.0, 1800.0, 0.0]), "{}", c[0]);
    assert!(near(&c[0]["direction"], [0.0, 0.0, -1.0]), "a direction is a unit vector: {}", c[0]);
    assert!(c[0].get("load_va").is_none() && c[0].get("system").is_none(), "a pipe connector has no load: {}", c[0]);
    assert!(c[0]["guid"].is_string(), "a connector has its GUID: {}", c[0]);
    // The type sets what its formulas read.
    let big = made(&mut s, json!({"family": "Trap", "type": "75", "at": [0, 0]}));
    let c = connectors(&s, big);
    assert_eq!(c[0]["size"].as_f64(), Some(75.0));
    assert!(near(&c[1]["at"], [0.0, 0.0, 750.0]), "{}", c[1]);
    // Turned a quarter, raised and mirrored, its connectors turn, rise and mirror with it.
    let turned = made(&mut s, json!({"family": "Trap", "at": [1000, 2000], "angle": 90, "offset": 50}));
    let c = connectors(&s, turned);
    assert!(near(&c[0]["at"], [1200.0, 2100.0, 50.0]), "{}", c[0]);
    assert!(near(&c[1]["direction"], [-1.0, 0.0, 0.0]), "{}", c[1]);
    let mirrored = made(&mut s, json!({"family": "Trap", "at": [1000, 2000], "mirrored": true}));
    let c = connectors(&s, mirrored);
    assert!(near(&c[0]["at"], [1100.0, 2200.0, 0.0]), "{}", c[0]);
    assert!(near(&c[1]["direction"], [0.0, -1.0, 0.0]), "{}", c[1]);
}

#[test]
fn an_unsound_connector_is_refused_and_says_what_to_fix() {
    let mut s = Session::new();
    let pipe = json!({"kind": "pipe", "domain": "waste", "at": [0, 0, 0], "direction": [0, 0, -1], "size": 40});
    let with = |k: &str, v: Value| {
        let mut c = pipe.clone();
        if v.is_null() {
            c.as_object_mut().unwrap().remove(k);
        } else {
            c[k] = v;
        }
        json!({"name": "Bad", "category": "plumbing", "types": {"A": {}}, "connectors": [c]})
    };
    for (k, v, says) in [
        ("domain", Value::Null, "domain"),
        ("at", Value::Null, "`at`"),
        ("direction", Value::Null, "direction"),
        ("size", Value::Null, "size"),
        ("size", json!(0), "size"),
        ("size", json!(5000), "size"),
        ("size", json!("Nothing"), "size"),
        ("direction", json!([0, 0, 0]), "no length"),
        ("at", json!([0, 0, 1e300]), "place"),
        ("system", json!("power-balanced"), "system"),
        ("load_va", json!(100), "load"),
        ("domain", json!("steam"), "`cold`"),
        ("flow", json!("sideways"), "`both`"),
    ] {
        let e = fails(&mut s, "family", with(k, v.clone()));
        assert!(e.contains(says), "{k} = {v}: {e}");
    }
    // An electrical device's connector takes no domain and no size; it may have a place.
    let power = json!({"kind": "power", "system": "power-balanced", "voltage": 127, "load_va": 10});
    for (k, v) in [("domain", json!("cold")), ("size", json!(20))] {
        let mut c = power.clone();
        c[k] = v;
        let e = fails(&mut s, "family", json!({"name": "Bad", "category": "outlet", "types": {"A": {}}, "connectors": [c]}));
        assert!(e.contains(k), "{e}");
    }
    let mut placed = power.clone();
    placed["at"] = json!([0, -20, 300]);
    run(&mut s, "family", json!({"name": "Box", "category": "outlet", "types": {"A": {}}, "connectors": [placed]}));
    let id = made(&mut s, json!({"family": "Box", "at": [500, 0]}));
    let c = &connectors(&s, id)[0];
    assert!(near(&c["at"], [500.0, -20.0, 300.0]) && c["load_va"].as_f64() == Some(10.0), "{c}");
    // A power or a switch connector still needs its system.
    let e = fails(&mut s, "family", json!({"name": "Bad", "types": {"A": {}}, "connectors": [{"kind": "power", "voltage": 127}]}));
    assert!(e.contains("system"), "{e}");
    // A data or an earthing connector needs its place, and nothing more.
    let e = fails(&mut s, "family", json!({"name": "Bad", "types": {"A": {}}, "connectors": [{"kind": "data"}]}));
    assert!(e.contains("`at`"), "{e}");
    run(&mut s, "family", json!({"name": "Jack", "types": {"A": {}}, "connectors": [{"kind": "data", "at": [0, 0, 300]}]}));
}

/// A family's connectors, as (domain, flow, DN).
type Want = &'static [(&'static str, &'static str, f64)];

#[test]
fn the_starter_fixtures_carry_their_waste_and_water_in_every_language() {
    // (family, its connectors as (domain, flow, DN))
    let cases: [(&str, Want); 6] = [
        ("WC", &[("waste", "out", 100.0), ("cold", "in", 20.0)]),
        ("Washbasin", &[("waste", "out", 40.0), ("cold", "in", 20.0)]),
        ("Bathtub", &[("waste", "out", 40.0), ("cold", "in", 20.0), ("hot", "in", 20.0)]),
        ("Kitchen Sink", &[("waste", "out", 50.0), ("cold", "in", 20.0)]),
        ("Shower", &[("waste", "out", 40.0), ("cold", "in", 20.0), ("hot", "in", 20.0)]),
        ("Floor Drain", &[("waste", "out", 40.0)]),
    ];
    for l in Language::ALL {
        let m = Model::new_in("x", l);
        assert_eq!(m.check(), Ok(()), "{l:?}");
        for (family, want) in cases {
            let f = m.families.get(l.word(family)).unwrap_or_else(|| panic!("{l:?}: no {family}"));
            for t in f.types.keys() {
                let got: Vec<(String, String, f64)> = f
                    .connector_values(t, &Default::default())
                    .unwrap()
                    .iter()
                    .map(|c| {
                        let domain = serde_json::to_value(c.domain).unwrap().as_str().unwrap_or_default().to_string();
                        let flow = serde_json::to_value(c.flow).unwrap().as_str().unwrap_or_default().to_string();
                        assert!(c.at.is_some() && c.direction.is_some(), "{l:?} {family} {t}");
                        (domain, flow, c.size.unwrap_or_default())
                    })
                    .collect();
                let want: Vec<(String, String, f64)> = want.iter().map(|(d, f, n)| (d.to_string(), f.to_string(), *n)).collect();
                assert_eq!(got, want, "{l:?} {family} {t}");
            }
        }
    }
}

#[test]
fn a_waste_outlet_through_the_floor_is_under_the_fixture_and_faces_down() {
    let mut s = Session::new();
    let wc = made(&mut s, json!({"family": "WC", "at": [2000, 1000], "angle": 180}));
    let c = connectors(&s, wc);
    let waste = c.iter().find(|c| c["domain"] == "waste").unwrap();
    // 300 mm out from the wall it stands against, which a turn of 180° puts to its north.
    assert!(near(&waste["at"], [2000.0, 1300.0, 0.0]), "{waste}");
    assert!(near(&waste["direction"], [0.0, 0.0, -1.0]), "{waste}");
}

#[test]
fn a_plumbing_fixture_with_pipe_connectors_is_no_electrical_device_in_ifc() {
    let mut s = Session::new();
    made(&mut s, json!({"family": "WC", "at": [0, 0]}));
    made(&mut s, json!({"family": "Outlet", "at": [1000, 0]}));
    let f = crate::ifc::export(&s.model);
    assert_eq!(f.matches("'Pset_ElectricalDeviceCommon'").count(), 1, "the outlet's only");
}

#[test]
fn a_family_with_pipe_connectors_saves_and_opens_as_it_was() {
    let mut s = Session::new();
    trap(&mut s);
    let before = s.model.families["Trap"].clone();
    let back = Model::from_json(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(back.families["Trap"], before);
    // A file's connectors of before keep their system, and read as they did.
    let outlet = &back.families[Language::English.word("Outlet")].connectors[0];
    assert_eq!(serde_json::to_value(outlet.system).unwrap(), json!("power-balanced"));
}
