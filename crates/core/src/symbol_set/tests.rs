//! The AREI symbol set: every family loads through the real command, in each language, and draws where its data says.

use serde_json::{Value, json};

use super::*;
use crate::engine::Session;
use crate::family::{MAX_PROFILE, MAX_SYMBOLS};
use crate::geom::P2;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn names(n: Names) -> Vec<String> {
    families("arei", n).into_iter().map(|(name, _)| name).collect()
}

#[test]
fn the_set_has_seventy_six_families_each_a_flat_symbol_of_one_type() {
    let all = families("arei", Names::English);
    assert_eq!(all.len(), 76);
    for (name, f) in &all {
        assert!(f.forms.is_empty(), "{name}: a flat plan symbol");
        assert_eq!(f.types.keys().collect::<Vec<_>>(), ["Standard"], "{name}");
        assert!(!f.symbols.is_empty() && f.symbols.len() <= MAX_SYMBOLS, "{name}");
        assert!(f.symbols.iter().all(|s| s.line.len() <= MAX_PROFILE), "{name}");
    }
    assert!(families("nothing", Names::English).is_empty());
}

#[test]
fn every_family_loads_through_the_family_command_and_the_model_still_checks() {
    for n in [Names::English, Names::Dutch, Names::French] {
        let mut s = Session::new();
        let before = s.model.families.len();
        let r = run(
            &mut s,
            "symbol_set",
            json!({"set": "arei", "names": match n { Names::English => "en", Names::Dutch => "nl", Names::French => "fr" }}),
        );
        assert_eq!(r["loaded"].as_array().map(Vec::len), Some(76), "{n:?}");
        assert_eq!(s.model.families.len(), before + 76, "{n:?}");
        assert_eq!(s.model.check(), Ok(()), "{n:?}");
        // The same through the plain `family` command, one at a time, as a user's file would load.
        let mut t = Session::new();
        for (name, f) in families("arei", n) {
            let mut v = serde_json::to_value(&f).unwrap();
            v["name"] = json!(name);
            run(&mut t, "family", v);
        }
        assert_eq!(t.model.check(), Ok(()), "{n:?}");
    }
}

#[test]
fn names_exist_in_each_language_and_are_unique_in_it_and_against_the_starter_families() {
    use crate::language::Language;
    for n in [Names::English, Names::Dutch, Names::French] {
        let list = names(n);
        let unique: std::collections::BTreeSet<&String> = list.iter().collect();
        assert_eq!(unique.len(), 76, "{n:?}");
        assert!(list.iter().all(|x| !x.trim().is_empty()), "{n:?}");
    }
    assert_ne!(names(Names::English), names(Names::Dutch));
    assert_ne!(names(Names::English), names(Names::French));
    // None collides with a family a new project has, so loading never skips one.
    for language in Language::ALL {
        let m = crate::model::Model::new_in("x", language);
        for n in [Names::English, Names::of(language)] {
            for name in names(n) {
                assert!(!m.families.contains_key(&name), "{language:?}: {name}");
            }
        }
    }
}

#[test]
fn loading_twice_changes_nothing_and_never_overwrites_an_edited_family() {
    let mut s = Session::new();
    run(&mut s, "symbol_set", json!({"set": "arei"}));
    let once = serde_json::to_string(&s.model.families).unwrap();
    run(&mut s, "family", json!({"name": "Bell", "symbols": [{"dot": {"centre": [0, 0], "radius": 10}}]}));
    let edited = serde_json::to_string(&s.model.families).unwrap();
    let r = run(&mut s, "symbol_set", json!({"set": "arei"}));
    assert_eq!(r["loaded"], json!([]));
    assert_eq!(r["kept"].as_array().map(Vec::len), Some(76));
    assert_eq!(serde_json::to_string(&s.model.families).unwrap(), edited);
    assert_ne!(once, edited);
    let mut t = Session::new();
    run(&mut t, "symbol_set", json!({"set": "arei"}));
    let first = serde_json::to_string(&t.model.to_json()).unwrap();
    run(&mut t, "symbol_set", json!({"set": "arei"}));
    assert_eq!(serde_json::to_string(&t.model.to_json()).unwrap(), first, "ids and GUIDs too");
}

#[test]
fn a_french_project_gets_french_names_and_a_dutch_one_dutch() {
    let mut s = Session::new();
    run(&mut s, "language", json!({"code": "fr"}));
    run(&mut s, "symbol_set", json!({"set": "arei"}));
    assert!(s.model.families.contains_key("Interrupteur unipolaire"));
    assert!(!s.model.families.contains_key("Switch, single-pole"));
    let mut d = Session::new();
    run(&mut d, "language", json!({"code": "nl"}));
    run(&mut d, "symbol_set", json!({"set": "arei"}));
    assert!(d.model.families.contains_key("Eenpolige schakelaar"));
    assert_eq!(d.model.families["Eenpolige schakelaar"].category, Category::Switch);
    assert!(!d.model.families.contains_key("Switch, single-pole"));
    // Asked for by name, Dutch comes in a project of any language.
    let mut e = Session::new();
    run(&mut e, "symbol_set", json!({"set": "arei", "names": "nl"}));
    assert!(e.model.families.contains_key("Eenpolige schakelaar"));
    // Another language falls back to English.
    let mut p = Session::new();
    run(&mut p, "language", json!({"code": "pt"}));
    run(&mut p, "symbol_set", json!({"set": "arei"}));
    assert!(p.model.families.contains_key("Switch, single-pole"));
}

#[test]
fn bad_asks_are_refused_saying_what_to_ask() {
    let mut s = Session::new();
    for p in
        [json!({}), json!({"set": "nope"}), json!({"set": "arei", "names": "xx"}), json!({"set": 5}), json!({"set": "arei", "names": []})]
    {
        let e = s.execute("symbol_set", &p).unwrap_err();
        assert!(e.contains("symbol_set"), "{p}: {e}");
    }
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn the_categories_follow_the_kind_of_device() {
    let all: std::collections::BTreeMap<String, Family> = families("arei", Names::English).into_iter().collect();
    let cat = |n: &str| all[n].category;
    assert_eq!(cat("Switch, single-pole"), Category::Switch);
    assert_eq!(cat("Push button"), Category::Switch);
    assert_eq!(cat("Socket, protective contact"), Category::Outlet);
    assert_eq!(cat("Data socket"), Category::DataOutlet);
    assert_eq!(cat("Light point"), Category::Lighting);
    assert_eq!(cat("Cooker"), Category::Generic);
    assert_eq!(cat("Circuit breaker, 20 A curve C"), Category::Generic);
}

/// What component `id` draws in the plan of Level 1: open lines and filled rings, in model mm.
fn ink(s: &Session, id: u64) -> (Vec<Vec<P2>>, Vec<Vec<P2>>) {
    let view = crate::render::View { scale: 0.5, cx: 1000.0, cy: 1000.0, width: 1000, height: 800 };
    let d = crate::render::draw(&s.model, 100000, view);
    let (mut lines, mut fills) = (Vec::new(), Vec::new());
    for (p, _) in d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)) {
        match p {
            crate::render::Prim::Line { pts, .. } => lines.push(pts.iter().map(|q| view.mm(*q)).collect()),
            crate::render::Prim::Fill { rings, .. } => fills.extend(rings.iter().map(|r| r.iter().map(|q| view.mm(*q)).collect())),
            _ => {}
        }
    }
    (lines, fills)
}

fn place(s: &mut Session, family: &str, at: [f64; 2]) -> u64 {
    run(s, "component", json!({"family": family, "at": at}))["id"].as_u64().unwrap()
}

#[test]
fn a_placed_cooker_draws_its_lead_its_box_and_its_three_rings_where_the_data_says() {
    let mut s = Session::new();
    run(&mut s, "symbol_set", json!({"set": "arei"}));
    let id = place(&mut s, "Cooker", [1000.0, 1000.0]);
    let (lines, fills) = ink(&s, id);
    let near = |a: P2, x: f64, y: f64| a.dist(P2(x, y)) < 2.0;
    assert!(lines.iter().any(|l| l.len() == 2 && near(l[0], 1000.0 - 526.5, 1000.0) && near(l[1], 1000.0, 1000.0)), "the lead");
    assert!(
        lines.iter().any(|l| l.len() == 5 && l.iter().any(|p| near(*p, 1432.9, 1214.5)) && l.iter().any(|p| near(*p, 1000.0, 785.5))),
        "the box"
    );
    assert_eq!(fills.len(), 3, "three solid dots");
    let centres: Vec<P2> = fills
        .iter()
        .map(|r| P2(r.iter().map(|p| p.0).sum::<f64>() / r.len() as f64, r.iter().map(|p| p.1).sum::<f64>() / r.len() as f64))
        .collect();
    for (x, y) in [(1101.4, 1113.1), (1308.1, 1113.1), (1308.1, 894.7)] {
        assert!(centres.iter().any(|c| near(*c, x, y)), "a dot at {x}, {y}: {centres:?}");
    }
}

#[test]
fn a_circuit_breaker_draws_its_blades_dot_and_label() {
    let mut s = Session::new();
    run(&mut s, "symbol_set", json!({"set": "arei"}));
    let id = place(&mut s, "Circuit breaker, 20 A curve C", [1000.0, 1000.0]);
    let (lines, fills) = ink(&s, id);
    assert_eq!(fills.len(), 1);
    // "C20" is capitals 117 tall, started 85.8 right of the origin, centred on its height.
    let label: Vec<&P2> = lines.iter().filter(|l| l.iter().all(|p| p.0 > 1080.0)).flatten().collect();
    assert!(!label.is_empty(), "the label's strokes");
    assert!(label.iter().all(|p| (941.0..=1060.0).contains(&p.1)), "{label:?}");
    assert!(label.iter().any(|p| p.0 > 1150.0), "three characters wide");
}

#[test]
fn every_symbol_leaves_ink_on_the_plan_inside_a_metre_of_its_origin() {
    let mut s = Session::new();
    run(&mut s, "symbol_set", json!({"set": "arei"}));
    for (name, _) in families("arei", Names::English) {
        let id = place(&mut s, &name, [1000.0, 1000.0]);
        let (lines, fills) = ink(&s, id);
        let pts: Vec<&P2> = lines.iter().chain(fills.iter()).flatten().collect();
        assert!(!pts.is_empty(), "{name}: no ink");
        assert!(pts.iter().all(|p| p.dist(P2(1000.0, 1000.0)) < 1000.0), "{name}: strays");
    }
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn loading_the_set_adds_no_warnings() {
    let mut s = Session::new();
    let before = run(&mut s, "inspect", json!({}))["warnings"].clone();
    run(&mut s, "symbol_set", json!({"set": "arei"}));
    for (name, _) in families("arei", Names::English) {
        place(&mut s, &name, [1000.0, 1000.0]);
    }
    assert_eq!(run(&mut s, "inspect", json!({}))["warnings"], before);
}
