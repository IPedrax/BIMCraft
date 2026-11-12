//! The volumes of NBR 5410:2004 item 9.1 around a tub and a shower, and what may stand in each, worked by hand.

use super::*;

/// A tub of 1700 by 700 mm, its rim 450 mm up, on a floor at 0.
fn tub() -> Bath {
    Bath { fixture: Fixture::Outline(vec![P2(0.0, 0.0), P2(1700.0, 0.0), P2(1700.0, 700.0), P2(0.0, 700.0)]), floor: 0.0, rim: 450.0 }
}

fn shower() -> Bath {
    Bath { fixture: Fixture::Shower(P2(0.0, 0.0)), floor: 100.0, rim: 0.0 }
}

fn set() -> &'static RuleSet {
    rule_set(DEFAULT).unwrap_or_else(|| panic!("no {DEFAULT}"))
}

fn volume(b: &Bath, at: [f64; 3]) -> Option<&'static str> {
    set().volume(b, at).map(|v| v.name.as_str())
}

fn thing(kind: &str, at: [f64; 3]) -> Thing {
    Thing { at, kind: kind.into(), ip: Some(7), class: None, supply: "mains".into(), voltage: None, rcd: None, serving: false }
}

#[test]
fn a_tub_s_volumes_go_out_and_up_from_its_edge() {
    let b = tub();
    assert_eq!(volume(&b, [800.0, 300.0, 200.0]), Some("0"), "in the tub, under its rim");
    assert_eq!(volume(&b, [800.0, 300.0, 1000.0]), Some("1"), "over the tub");
    assert_eq!(volume(&b, [800.0, 300.0, 2250.0]), Some("1"));
    assert_eq!(volume(&b, [800.0, 300.0, 2500.0]), Some("2"), "over volume 1, up to 3 m");
    assert_eq!(volume(&b, [800.0, 300.0, 3100.0]), None);
    assert_eq!(volume(&b, [800.0, 1000.0, 1000.0]), Some("2"), "300 mm out");
    assert_eq!(volume(&b, [800.0, 1300.0, 2900.0]), Some("2"), "600 mm out, under 3 m");
    assert_eq!(volume(&b, [800.0, 1301.0, 1000.0]), Some("3"));
    assert_eq!(volume(&b, [800.0, 1400.0, 2400.0]), None, "volume 3 stops at 2.25 m");
    assert_eq!(volume(&b, [800.0, 3700.0, 1000.0]), Some("3"), "3 m out");
    assert_eq!(volume(&b, [800.0, 3701.0, 1000.0]), None);
    assert_eq!(volume(&b, [800.0, 1000.0, -10.0]), None, "under the floor");
}

#[test]
fn a_shower_with_no_tray_has_no_volume_0_and_reaches_from_its_outlet() {
    let b = shower();
    assert_eq!(volume(&b, [0.0, 0.0, 100.0]), Some("1"), "the floor under it is volume 1");
    assert_eq!(volume(&b, [600.0, 0.0, 2350.0]), Some("1"), "0.6 m round, 2.25 m over the floor at 100");
    assert_eq!(volume(&b, [0.0, 1200.0, 3100.0]), Some("2"));
    assert_eq!(volume(&b, [0.0, 1201.0, 1000.0]), Some("3"));
    assert_eq!(volume(&b, [0.0, 3600.0, 1000.0]), Some("3"));
    assert_eq!(volume(&b, [0.0, 3601.0, 1000.0]), None);
}

#[test]
fn an_outlet_only_in_volume_3_and_protected() {
    let b = tub();
    let mut o = thing("outlet", [800.0, 2000.0, 1100.0]);
    let v = set().check(&b, &o, false);
    assert!(!v.allowed && v.volume.as_deref() == Some("3"), "{v:?}");
    o.rcd = Some(30.0);
    assert!(set().check(&b, &o, false).allowed);
    o.rcd = Some(300.0);
    assert!(!set().check(&b, &o, false).allowed);
    o.rcd = None;
    o.supply = "separation".into();
    assert!(set().check(&b, &o, false).allowed);
    o.at = [800.0, 1000.0, 1100.0];
    let v = set().check(&b, &o, false);
    assert!(!v.allowed && v.why.iter().any(|w| w.contains("no outlet")), "{v:?}");
    o.at = [800.0, 5000.0, 1100.0];
    let v = set().check(&b, &o, false);
    assert!(v.allowed && v.volume.is_none(), "outside every volume");
}

#[test]
fn luminaires_and_heaters_by_their_class() {
    let b = tub();
    let mut l = thing("luminaire", [800.0, 1000.0, 2400.0]);
    l.class = Some("II".into());
    assert!(set().check(&b, &l, false).allowed, "class II in volume 2");
    l.class = Some("I".into());
    let v = set().check(&b, &l, false);
    assert!(!v.allowed && v.why.iter().any(|w| w.contains("class II")), "{v:?}");
    l.at = [800.0, 300.0, 2000.0];
    l.class = Some("II".into());
    assert!(!set().check(&b, &l, false).allowed, "no mains luminaire in volume 1");
    l.supply = "selv".into();
    assert!(set().check(&b, &l, false).allowed, "at SELV it is left out of the rule");
    let mut h = thing("heater", [800.0, 300.0, 2000.0]);
    assert!(!set().check(&b, &h, false).allowed, "a class is needed to say");
    h.class = Some("I".into());
    assert!(set().check(&b, &h, false).allowed);
}

#[test]
fn volume_0_takes_selv_up_to_12_v_only() {
    let b = tub();
    let mut l = thing("luminaire", [800.0, 300.0, 100.0]);
    l.supply = "selv".into();
    l.voltage = Some(12.0);
    assert!(set().check(&b, &l, false).allowed);
    l.voltage = Some(24.0);
    assert!(!set().check(&b, &l, false).allowed);
    let mut x = thing("box", [800.0, 300.0, 100.0]);
    x.serving = true;
    assert!(!set().check(&b, &x, false).allowed, "a box in volume 0 at mains voltage");
}

#[test]
fn the_ip_code_by_volume_and_in_a_public_bathroom() {
    let b = tub();
    let mut s = thing("switch", [800.0, 2000.0, 1100.0]);
    s.ip = Some(1);
    let v = set().check(&b, &s, false);
    assert!(v.allowed && v.ip_ok == Some(true) && v.ip_min == Some(1), "{v:?}");
    let v = set().check(&b, &s, true);
    assert_eq!((v.ip_min, v.ip_ok), (Some(5), Some(false)), "IPX5 in a public bathroom");
    s.ip = None;
    let v = set().check(&b, &s, false);
    assert!(v.ip_ok.is_none() && v.why.iter().any(|w| w.contains("give `ip`")), "{v:?}");
    let mut l = thing("luminaire", [800.0, 1000.0, 2400.0]);
    l.class = Some("II".into());
    l.ip = Some(2);
    assert_eq!(set().check(&b, &l, false).ip_ok, Some(false), "volume 2 asks IPX3");
}

#[test]
fn every_volume_of_every_standard_names_its_source() {
    let sets = rule_sets();
    assert!(!sets.is_empty(), "data/bath-volumes.json did not load");
    for s in sets {
        let n = &s.standard;
        assert!(!n.trim().is_empty() && !s.source.trim().is_empty() && !s.not_covered.trim().is_empty(), "{n}: no source");
        assert!(s.max_rcd_ma.is_finite() && s.max_rcd_ma > 0.0, "{n}");
        assert!(!s.volumes.is_empty(), "{n}: no volumes");
        for v in &s.volumes {
            assert!(!v.name.trim().is_empty() && !v.source.trim().is_empty(), "{n} {}: no source", v.name);
            assert!(v.to_rim || v.top_mm.is_some_and(|t| t.is_finite() && t > 0.0), "{n} {}: no top", v.name);
            for r in std::iter::once(v.reach_mm).chain(v.shower_reach_mm) {
                assert!(r.is_finite() && r >= 0.0, "{n} {}: reach {r}", v.name);
            }
            assert!(v.ip <= 8 && v.ip_public <= 8 && v.ip_public >= v.ip, "{n} {}: ip", v.name);
            for a in &v.allows {
                assert!(!a.source.trim().is_empty(), "{n} {}: a row names no source", v.name);
                assert!(!a.kinds.is_empty() && a.kinds.iter().all(|k| KINDS.contains(&k.as_str())), "{n} {}: {:?}", v.name, a.kinds);
                assert!(a.classes.iter().all(|c| CLASSES.contains(&c.as_str())), "{n} {}: {:?}", v.name, a.classes);
                assert!(a.any_of.iter().all(|p| ["selv", "separation", "rcd"].contains(&p.as_str())), "{n} {}: {:?}", v.name, a.any_of);
            }
        }
    }
    assert!(rule_set(DEFAULT).is_some(), "the default `{DEFAULT}` is not in the data");
}

#[test]
fn the_same_check_takes_another_standard_s_volumes() {
    // Made-up volumes, not any standard's: two zones, outlets anywhere in the second.
    let s: RuleSet = serde_json::from_str(
        r#"{"standard": "Example", "source": "a test", "max_rcd_ma": 10, "not_covered": "t",
            "volumes": [
              {"name": "A", "reach_mm": 0, "shower_reach_mm": 1000, "top_mm": 2000, "ip": 4, "ip_public": 4, "allows": [], "source": "t"},
              {"name": "B", "reach_mm": 500, "shower_reach_mm": 1500, "top_mm": 2000, "ip": 2, "ip_public": 2, "source": "t",
               "allows": [{"kinds": ["outlet"], "any_of": ["rcd"], "source": "tb"}]}
            ]}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let b = shower();
    assert_eq!(s.volume(&b, [900.0, 0.0, 1000.0]).map(|v| v.name.as_str()), Some("A"));
    let mut o = thing("outlet", [1400.0, 0.0, 1000.0]);
    o.rcd = Some(30.0);
    assert!(!s.check(&b, &o, false).allowed, "above its 10 mA");
    o.rcd = Some(10.0);
    let v = s.check(&b, &o, false);
    assert!(v.allowed && v.sources.contains(&"tb".to_string()), "{v:?}");
    assert!(s.volume(&b, [800.0, 0.0, 2200.0]).is_none(), "nothing over 2 m here");
}

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        s.execute("bath_volumes", &p).unwrap_or_else(|e| panic!("bath_volumes {p}: {e}"))
    }

    #[test]
    fn things_come_back_with_their_volume_and_verdict() {
        let mut s = Session::new();
        let r = run(
            &mut s,
            json!({"fixture": {"outline": [[0, 0], [900, 0], [900, 900], [0, 900]]},
            "things": [
              {"name": "TUG lavatory", "kind": "outlet", "at": [450, 1700, 1100], "ip": 1, "rcd": 30},
              {"name": "Switch", "kind": "switch", "at": [450, 1200, 1100], "ip": 1},
              {"name": "Ceiling light", "kind": "luminaire", "at": [450, 450, 2500], "ip": 4, "class": "II"},
              {"name": "Hall outlet", "kind": "outlet", "at": [450, 5000, 300]}
            ]}),
        );
        let t = &r["things"];
        assert_eq!((&t[0]["volume"], &t[0]["ok"]), (&json!("3"), &json!(true)), "{r}");
        assert_eq!((&t[1]["volume"], &t[1]["ok"]), (&json!("2"), &json!(false)), "{r}");
        assert_eq!((&t[2]["volume"], &t[2]["ok"]), (&json!("2"), &json!(true)), "{r}");
        assert_eq!((&t[3]["volume"], &t[3]["ok"]), (&json!(null), &json!(true)), "{r}");
        assert_eq!(r["failing"], json!(["Switch"]));
        assert_eq!(r["standard"], "NBR 5410:2004");
        assert!(r["standard_source"].as_str().is_some_and(|x| x.contains("9.1") && x.contains("IEC 60364-7-701")), "{r}");
        assert!(t[1]["sources"].as_array().is_some_and(|a| !a.is_empty()), "{r}");
        assert!(r["not_covered"].as_str().is_some_and(|n| n.contains("partitions")), "{r}");
        assert_eq!(crate::engine::find("bath_volumes").map(|c| c.undoable), Some(false));
    }

    #[test]
    fn a_shower_and_a_public_bathroom() {
        let mut s = Session::new();
        let r = run(
            &mut s,
            json!({"fixture": {"shower": [0, 0], "floor": 0}, "public": true,
                   "things": [{"name": "L", "kind": "luminaire", "at": [1000, 0, 2600], "ip": 4, "class": "II"}]}),
        );
        assert_eq!(r["things"][0]["volume"], "2");
        assert_eq!(r["things"][0]["ip_min"], 5);
        assert_eq!(r["things"][0]["ok"], false);
    }

    #[test]
    fn hostile_input_is_refused() {
        let mut s = Session::new();
        let fx = json!({"shower": [0, 0]});
        let ok = json!({"kind": "switch", "at": [0, 0, 0]});
        for p in [
            json!([]),
            json!({}),
            json!({"fixture": fx.clone()}),
            json!({"fixture": fx.clone(), "things": []}),
            json!({"fixture": {}, "things": [ok.clone()]}),
            json!({"fixture": {"shower": [0, 0], "outline": [[0, 0], [1, 0], [1, 1]]}, "things": [ok.clone()]}),
            json!({"fixture": {"outline": [[0, 0], [1, 0]]}, "things": [ok.clone()]}),
            json!({"fixture": {"shower": [1e300, 0]}, "things": [ok.clone()]}),
            json!({"fixture": {"shower": [0, 0], "rim": -1}, "things": [ok.clone()]}),
            json!({"fixture": {"shower": [0, 0], "colour": 1}, "things": [ok.clone()]}),
            json!({"fixture": fx.clone(), "things": [5]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "toaster", "at": [0, 0, 0]}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch"}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch", "at": [0, 0]}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch", "at": [0, 0, 0], "ip": 9}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch", "at": [0, 0, 0], "class": "IV"}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch", "at": [0, 0, 0], "supply": "battery"}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch", "at": [0, 0, 0], "rcd": -1}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch", "at": [0, 0, 0], "voltage": "x"}]}),
            json!({"fixture": fx.clone(), "things": [{"kind": "switch", "at": [0, 0, 0], "colour": 1}]}),
            json!({"fixture": fx.clone(), "things": [ok.clone()], "public": "yes"}),
            json!({"fixture": fx.clone(), "things": [ok.clone()], "standard": "BS 7671"}),
            json!({"fixture": fx.clone(), "things": [ok.clone()], "units": 1}),
        ] {
            assert!(s.execute("bath_volumes", &p).is_err(), "{p}");
        }
    }
}
