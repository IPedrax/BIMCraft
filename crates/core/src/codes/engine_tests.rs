//! The building codes' engine: the data, layers and amendments, editions, the library, the cache, XML and the site.

use serde_json::{Value, json};

use super::*;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-codes-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn every_file_of_the_folder_is_listed_reads_and_holds_sections_that_can_be() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/codes");
    let mut on_disk = Vec::new();
    for d in Discipline::ALL {
        for f in std::fs::read_dir(root.join(d.key())).unwrap() {
            on_disk.push(format!("{}/{}", d.key(), f.unwrap().file_name().to_string_lossy()));
        }
    }
    on_disk.sort();
    let mut listed: Vec<String> = data::FILES.iter().map(|(p, _)| p.to_string()).collect();
    listed.sort();
    assert_eq!(listed, on_disk, "a code's file is listed in codes/data.rs");
    for (path, text) in data::FILES {
        let c: BuildingCode = serde_json::from_str(text).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(path.starts_with(c.discipline.key()), "{path} is a {} code", c.discipline.key());
        // A code's file is named by it; an edition the library keeps for the projects pinned to it, by it and its edition.
        let file = if c.superseded_by.is_empty() { format!("{}.json", c.code) } else { format!("{}@{}.json", c.code, c.edition) };
        assert_eq!(path.rsplit('/').next(), Some(file.as_str()), "{path}");
        c.check_rules().unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(!c.name.is_empty(), "{path}");
        for (key, v) in &c.sections {
            assert!(section_def(c.discipline, key).is_some(), "{path}: `{key}` is no {} section", c.discipline.key());
            assert!(v.is_object(), "{path}: {key}");
        }
        // A source names a section or a rule the code has, and cites a document it lists.
        for (at, s) in &c.sources {
            let (key, rule) = at.split_once('.').unwrap_or((at, ""));
            let has = c.section(key).is_some_and(|o| rule.is_empty() || o.contains_key(rule));
            assert!(has, "{path}: a source for `{at}`, which it hasn't");
            if let Source::Cited(cite) = s {
                assert!(c.documents.contains_key(&cite.doc), "{path}: `{at}` cites `{}`, which it doesn't list", cite.doc);
            }
        }
        // An amendment amends a code there is.
        if !c.amends.is_empty() {
            assert!(find(c.discipline, &c.amends).is_some(), "{path} amends `{}`, which isn't there", c.amends);
        }
    }
    assert_eq!(codes().len(), data::FILES.len(), "every file reads");
    for d in Discipline::ALL {
        let defaults: Vec<&str> = of(d).filter(|c| c.default).map(|c| c.code.as_str()).collect();
        assert_eq!(defaults.len(), 1, "{}: one default, not {defaults:?}", d.key());
        // One current edition of each code, and no edition twice.
        let mut seen = std::collections::BTreeSet::new();
        for c in codes().iter().filter(|c| c.discipline == d) {
            assert!(seen.insert((c.code.clone(), c.edition.clone())), "{} {} twice", c.code, c.edition);
        }
        for c in of(d) {
            assert_eq!(of(d).filter(|o| o.code == c.code).count(), 1, "{}: one current edition", c.code);
        }
        // Every section a module reads is somewhere in the library, and reads as the module's.
        for def in sections().iter().filter(|s| s.discipline == d && !s.read_by.is_empty()) {
            let lib = library(d, def.key).unwrap_or_else(|| panic!("{} {}: in no code", d.key(), def.key));
            def.check(&lib.sections[def.key]).unwrap_or_else(|e| panic!("{} {}: {e}", lib.code, def.key));
        }
    }
    for p in presets() {
        for (d, pick) in &p.choices {
            let base = find(*d, &pick.code).unwrap_or_else(|| panic!("preset {}: no {} code {}", p.name, d.key(), pick.code));
            assert!(base.amends.is_empty(), "preset {}: {} is an amendment", p.name, pick.code);
            for a in &pick.amendments {
                assert!(find(*d, a).is_some_and(|c| !c.amends.is_empty()), "preset {}: {a}", p.name);
            }
        }
    }
}

/// A code of the project's own, in discipline `d`, amending `amends`, with `section`'s `rules`.
fn own(name: &str, d: Discipline, amends: &str, section: &str, rules: Value) -> BuildingCode {
    let mut c = BuildingCode { code: name.into(), name: name.into(), discipline: d, amends: amends.into(), ..BuildingCode::default() };
    c.sections.insert(section.into(), rules);
    c
}

#[test]
fn layers_go_base_then_amendments_in_order_then_the_project_and_say_where_each_rule_is_from() {
    let mut s = Session::new();
    let a = Discipline::Architectural;
    {
        let codes = &mut *s.model_mut().codes;
        codes.own.push(own("Fire brigade", a, "nbr-9077", "stairs", json!({"width_min": 1200.0, "guard_min": 1100.0})));
        codes.own.push(own("City", a, "Fire brigade", "stairs", json!({"guard_min": 1300.0})));
        codes.own.push(own("Elsewhere", a, "ibc", "stairs", json!({"headroom": 2500.0})));
    }
    let r = run(&mut s, "building_code", json!({"set": "nbr-9077", "amendments": ["Fire brigade", "City"], "stairs": {"riser": 172}}));
    assert_eq!(r["amendments"].as_array().map(Vec::len), Some(2));
    let st = s.model.stair_rules();
    assert_eq!((st.width_min, st.guard_min, st.riser, st.going), (1200.0, 1300.0, 172.0, 295.0));
    let res = s.model.resolve_code(a, "stairs");
    assert_eq!(res.from["going"], Layer::Base("nbr-9077".into()));
    assert_eq!(res.from["width_min"], Layer::Amendment("Fire brigade".into()));
    assert_eq!(res.from["guard_min"], Layer::Amendment("City".into()), "a member over its group");
    assert_eq!(res.from["riser"], Layer::Project);
    assert_eq!(res.kinds["riser"], Kind::Assumption);
    assert!(res.sources["going"].contains("NBR 9077"), "{:?}", res.sources["going"]);

    // An amendment over a code that isn't under it is refused, and so is one before what it amends.
    let e = s.execute("building_code", &json!({"amendments": ["Elsewhere"]})).unwrap_err();
    assert!(e.contains("amends `ibc`"), "{e}");
    assert!(s.execute("building_code", &json!({"amendments": ["City", "Fire brigade"]})).is_err());
    assert!(s.execute("building_code", &json!({"set": "Fire brigade"})).unwrap_err().contains("amendments"));
    // Another base takes the amendments away with the project's changes.
    run(&mut s, "building_code", json!({"set": "ibc"}));
    assert!(s.model.codes.architectural.amendments.is_empty() && s.model.codes.architectural.changes.is_empty());
}

#[test]
fn a_project_changes_any_section_and_a_number_that_is_the_code_s_is_no_change() {
    let mut s = Session::new();
    let r = run(&mut s, "building_code", json!({"rules": {"stairs": {"headroom": 2200, "riser": 180}}}));
    assert_eq!(r["changes"]["stairs"], json!(["headroom"]), "180 is the code's own riser");
    assert_eq!(s.model.codes.architectural.changes["stairs"]["headroom"], json!(2200.0), "as the section types it");
    // A section of reference takes rules of its own; one a module reads takes only the rules it has.
    run(&mut s, "building_code", json!({"rules": {"egress": {"corridor_width_min": 1200}}}));
    assert_eq!(s.model.resolve_code(Discipline::Architectural, "egress").value["corridor_width_min"], 1200);
    for p in [
        json!({"rules": {"stairs": {"colour": 1}}}),
        json!({"rules": {"plumbing": {}}}),
        json!({"rules": {"stairs": 5}}),
        json!({"rules": []}),
        json!({"discipline": "garden"}),
        json!({"reset": "nothing"}),
        json!({"amendments": "x"}),
        json!({"amendments": ["nowhere"]}),
    ] {
        assert!(s.execute("building_code", &p).is_err(), "{p}");
    }
    let r = run(&mut s, "building_code", json!({"reset": "stairs"}));
    assert_eq!(r["changes"], json!({"egress": ["corridor_width_min"]}), "only the stairs' are taken away");
    run(&mut s, "building_code", json!({"reset": true}));
    assert!(s.model.codes.is_empty());
}

#[test]
fn a_section_none_of_the_layers_has_comes_from_the_library_and_says_so() {
    let m = Model::new("x");
    let lib = library(Discipline::Architectural, "stairs").unwrap();
    assert!(lib.default, "the default code has the stairs");
    // A discipline's default with no section of it: the library's is read, and named.
    for def in sections().iter().filter(|s| !s.read_by.is_empty()) {
        let r = m.resolve_code(def.discipline, def.key);
        assert!(!r.value.is_empty(), "{}", def.key);
        let base = m.codes.base(def.discipline);
        if !base.sections.contains_key(def.key) {
            assert!(r.from.values().all(|l| matches!(l, Layer::Library(_))), "{}: {:?}", def.key, r.from);
        }
    }
}

#[test]
fn the_rules_in_force_are_kept_until_the_codes_change() {
    let mut s = Session::new();
    let a = s.model.stair_rules();
    let b = s.model.stair_rules();
    assert!(std::sync::Arc::ptr_eq(&a, &b), "read once");
    run(&mut s, "building_code", json!({"stairs": {"width_min": 1500}}));
    let c = s.model.stair_rules();
    assert_eq!(c.width_min, 1500.0);
    assert!(!std::sync::Arc::ptr_eq(&a, &c));
    // A copy of the model reads its own.
    let copy = (*s.model).clone();
    assert_eq!(copy.stair_rules().width_min, 1500.0);
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.stair_rules().width_min, a.width_min, "undone, the code's again");
}

#[test]
fn a_project_pins_the_editions_it_chose_and_says_where_the_library_has_moved_on() {
    let mut s = Session::new();
    let r = run(&mut s, "building_code", json!({"set": "ibc"}));
    assert_eq!(r["editions"], json!({"ibc": "2021"}));
    assert_eq!(r["editions_gone"], json!([]));
    // A file pinned to an edition the library no longer keeps: held to the one there is, and told.
    let mut m = (*s.model).clone();
    m.codes.architectural.editions.insert("ibc".into(), "2018".into());
    assert_eq!(m.codes.editions_gone(Discipline::Architectural), [("ibc".to_string(), "2018".to_string(), "2021".to_string())]);
    assert_eq!(m.building_code().edition, "2021");
    *s.model_mut() = m;
    let r = run(&mut s, "building_code", json!({}));
    assert_eq!(r["editions_gone"], json!([{"code": "ibc", "pinned": "2018", "now": "2021"}]));
    // Chosen again, it is pinned to the edition in force.
    let r = run(&mut s, "building_code", json!({"set": "ibc"}));
    assert_eq!((r["editions"].clone(), r["editions_gone"].clone()), (json!({"ibc": "2021"}), json!([])));
}

#[test]
fn a_code_goes_out_as_xml_with_its_documents_and_tables_and_comes_back_the_same() {
    let mut code = own(
        "Office rules",
        Discipline::Architectural,
        "",
        "egress",
        json!({"exits_by_load": [[500, 2], [1000, 3]], "corridor_width_min": 1118.0, "sprinklers": true, "method": "IBC"}),
    );
    code.region = "Somewhere & co".into();
    code.edition = "3".into();
    code.documents.insert(
        "ibc".into(),
        Document {
            title: "International Building Code".into(),
            edition: "2024".into(),
            url: "https://example.org/ibc?a=1&b=2".into(),
            accessed: "2026-11-16".into(),
            ..Document::default()
        },
    );
    code.sources.insert(
        "egress.corridor_width_min".into(),
        Source::Cited(Citation { doc: "ibc".into(), item: "1020.3".into(), page: "10-62".into(), ..Citation::default() }),
    );
    code.sources.insert("egress".into(), Source::Text("Chapter 10 <means of egress>".into()));
    code.kinds.insert("egress.method".into(), Kind::Practice);
    code.note = "Two lines\nof note".into();
    let text = code.to_xml();
    assert!(text.contains("<rule name=\"exits_by_load\" json=\"[[500,2],[1000,3]]\""), "{text}");
    assert!(text.contains("source-doc=\"ibc\" source-item=\"1020.3\" source-page=\"10-62\""), "{text}");
    let (back, left) = BuildingCode::from_xml(&text, None).unwrap();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(back, code);
    assert_eq!(back.source("egress", "corridor_width_min").as_deref(), Some("International Building Code (2024), 1020.3, p. 10-62"));
    assert_eq!(back.source("egress", "sprinklers").as_deref(), Some("Chapter 10 <means of egress>"), "the section's");

    // Unknown sections and rules are listed, never refused.
    let odd = "<building-code format='2' discipline='plumbing' name='P'><section name='garden'><rule name='x' value='1'/></section><extra/></building-code>";
    let (p, left) = BuildingCode::from_xml(odd, None).unwrap();
    assert_eq!(p.discipline, Discipline::Plumbing);
    assert_eq!(left, ["section `garden`", "<extra>"]);
    assert!(BuildingCode::from_xml("<building-code discipline='garden' name='G'/>", None).is_err(), "no code it can be");
}

#[test]
fn all_five_disciplines_go_out_in_one_file_and_set_come_back_as_the_project_s() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "ibc", "stairs": {"width_min": 1200}}));
    run(&mut s, "building_code", json!({"discipline": "mechanical", "rules": {"ventilation": {"note": "ours"}}}));
    let d = dir("all");
    let path = d.join("all.xml").to_string_lossy().to_string();
    let r = run(&mut s, "building_code_export", json!({"path": path, "all": true, "name": "Our office"}));
    assert_eq!(r["codes"], 5);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("<?xml") && text.contains("<building-codes format=\"2\">"), "{text}");
    let mut other = Session::new();
    let r = run(&mut other, "building_code_import", json!({"path": path, "set": true}));
    assert_eq!(r["codes"].as_array().map(Vec::len), Some(5), "one name, five disciplines");
    assert_eq!(other.model.stair_rules(), s.model.stair_rules());
    assert_eq!(other.model.resolve_code(Discipline::Mechanical, "ventilation").value["note"], "ours");
    assert_eq!(other.model.codes.own.len(), 5);
    assert!(Discipline::ALL.iter().all(|d| other.model.codes.base(*d).code == "Our office"));
    // One discipline alone leaves the others as they are.
    let p = d.join("plumbing.xml").to_string_lossy().to_string();
    run(&mut s, "building_code_export", json!({"path": p, "discipline": "plumbing", "name": "Plumbing, ours"}));
    let mut third = Session::new();
    run(&mut third, "building_code", json!({"set": "din"}));
    run(&mut third, "building_code_import", json!({"path": p, "set": true}));
    assert_eq!(
        (third.model.building_code().code.as_str(), third.model.codes.base(Discipline::Plumbing).code.as_str()),
        ("din", "Plumbing, ours")
    );
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn the_site_s_numbers_are_kept_with_their_sources_and_refused_out_of_range() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "site_climate",
        json!({"rain_intensity": 172, "thunderstorms": "AQ2", "sources": {"rain_intensity": "NBR 10844 Tabela 5, São Paulo"}}),
    );
    assert_eq!((r["rain_intensity"].as_f64(), r["thunderstorms"].as_str()), (Some(172.0), Some("AQ2")));
    for bad in [
        json!({"rain_intensity": -1}),
        json!({"thunderstorms": "AQ9"}),
        json!({"wind_speed": "fast"}),
        json!({"sources": {"wind_speed": "a map"}}),
        json!({"colour": 1}),
        json!({"climate_zone": "x".repeat(65)}),
    ] {
        assert!(s.execute("site_climate", &bad).is_err(), "{bad}");
    }
    let r = run(&mut s, "site_climate", json!({"rain_intensity": null}));
    assert!(r.get("rain_intensity").is_none() && r.get("sources").is_none(), "its source goes with it: {r}");
    let (back, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(back.codes.site.thunderstorms.as_deref(), Some("AQ2"));
}

#[test]
fn a_project_s_codes_are_held_to_a_size() {
    let mut s = Session::new();
    let big: Vec<Value> = (0..super::MAX_VALUES).map(|i| json!(i)).collect();
    let e = s.execute("building_code", &json!({"rules": {"egress": {"table": big}}})).unwrap_err();
    assert!(e.contains("more than"), "{e}");
    assert!(s.model.codes.is_empty());
}

#[test]
fn code_rules_says_each_rule_s_value_layer_source_and_kind() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "nbr-9050"}));
    let r = run(&mut s, "code_rules", json!({"section": "stairs"}));
    let rules = r["sections"][0]["rules"].as_array().unwrap();
    let headroom = rules.iter().find(|x| x["rule"] == "headroom").unwrap();
    assert_eq!(headroom["from"], json!({"layer": "base", "code": "nbr-9050"}));
    assert_eq!(headroom["kind"], "forgiving");
    let r = run(&mut s, "code_rules", json!({"discipline": "mechanical"}));
    assert!(r["sections"].as_array().unwrap().iter().all(|x| x["reference"] == true));
    assert!(s.execute("code_rules", &json!({"section": "nothing"})).is_err());
}

#[test]
fn the_plumbing_commands_size_by_the_project_s_code_and_say_where_the_rain_is_from() {
    let mut s = Session::new();
    // As BIMCraft ships it: the library's numbers.
    let r = run(&mut s, "water_size", json!({"flow": 1.5}));
    let dn = r["dn"].as_f64().unwrap();
    // A project that allows only 2 m/s: a larger pipe for the same flow.
    run(&mut s, "building_code", json!({"discipline": "plumbing", "rules": {"cold_water": {"max_speed": 2.0}}}));
    let r = run(&mut s, "water_size", json!({"flow": 1.5}));
    assert!(r["dn"].as_f64().unwrap() > dn, "{r}");
    assert!(r["speed_ms"].as_f64().unwrap() <= 2.0);
    // A fixture of the project's own.
    let mut fixtures = crate::plumbing::fixtures().fixtures.clone();
    let mut bidet = fixtures[1].clone();
    bidet.kind = "bidet".into();
    bidet.name = "Bidet".into();
    fixtures.push(bidet);
    let e = s.execute("water_size", &json!({"fixtures": {"bidet": 1}})).unwrap_err();
    assert!(e.contains("no fixture `bidet`"), "{e}");
    run(&mut s, "building_code", json!({"discipline": "plumbing", "rules": {"fixtures": {"fixtures": fixtures}}}));
    run(&mut s, "water_size", json!({"fixtures": {"bidet": 1}}));
    run(&mut s, "drain_size", json!({"fixtures": {"bidet": 2}}));
    // A building drain may be smaller where the project says so.
    assert_eq!(run(&mut s, "drain_size", json!({"units": 10, "role": "main"}))["dn"], 100.0);
    run(&mut s, "building_code", json!({"discipline": "plumbing", "rules": {"drainage": {"main_min_dn": 150.0}}}));
    assert_eq!(run(&mut s, "drain_size", json!({"units": 10, "role": "main"}))["dn"], 150.0);
    // A table that can't be is refused, and the project stays as it was.
    let bad = json!({"discipline": "plumbing", "rules": {"drainage": {"main_slopes": [0.01, 0.005]}}});
    assert!(s.execute("building_code", &bad).is_err());
    // The rain: the default for a small roof, the site's where it has one, given where given.
    let r = run(&mut s, "storm_size", json!({"kind": "horizontal", "area": 80, "slope": 0.01}));
    assert_eq!((r["rainfall_from"].as_str(), r["flow_lpm"].as_f64()), (Some("default"), Some(200.0)));
    assert!(s.execute("storm_size", &json!({"kind": "horizontal", "area": 400, "slope": 0.01})).unwrap_err().contains("site_climate"));
    run(&mut s, "site_climate", json!({"rain_intensity": 180}));
    let r = run(&mut s, "storm_size", json!({"kind": "horizontal", "area": 400, "slope": 0.01}));
    assert_eq!((r["rainfall_from"].as_str(), r["flow_lpm"].as_f64()), (Some("site"), Some(1200.0)));
    let r = run(&mut s, "storm_size", json!({"kind": "horizontal", "area": 400, "slope": 0.01, "rainfall": 120}));
    assert_eq!(r["rainfall_from"], "given");
    // Hydrants: the amendment's reserve, changed by the project.
    assert_eq!(run(&mut s, "hydrant_size", json!({"column": 1, "area": 100}))["reserve_m3"], 5.0);
    let mut reserve = crate::codes::library_rules::<crate::hydrant::HydrantRules>().reserve.clone();
    reserve[0][0].1 = 6.0;
    run(&mut s, "building_code", json!({"discipline": "plumbing", "rules": {"hydrants": {"reserve": reserve}}}));
    assert_eq!(run(&mut s, "hydrant_size", json!({"column": 1, "area": 100}))["reserve_m3"], 6.0);
    // The answers name where the hydrants' numbers come from: the library's IT 22, as no layer of the project has them.
    let r = run(&mut s, "code_rules", json!({"discipline": "plumbing", "section": "hydrants"}));
    let types = r["sections"][0]["rules"].as_array().unwrap().iter().find(|x| x["rule"] == "types").cloned().unwrap();
    assert_eq!(types["from"], json!({"layer": "library", "code": "cbpmesp-it22"}));
    assert!(types["source"].as_str().unwrap().contains("Tabela 2"));
}
