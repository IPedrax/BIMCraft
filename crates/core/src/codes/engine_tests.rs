//! The building codes' engine: the data, layers and amendments, editions, the library, the cache, XML and the site.

use serde_json::{Value, json};

use super::matrix_tests::{breakers_up, method_probe};
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

/// Amendments go in any order the user drags them to, the last one winning; a change made with one of them on is that
/// amendment's (`amend`), not the project's; and a new amendment of the project's own starts empty over the base (`create`).
#[test]
fn amendments_go_in_any_order_and_a_change_goes_into_the_amendment_edited() {
    let mut s = Session::new();
    let a = Discipline::Architectural;
    {
        let codes = &mut *s.model_mut().codes;
        codes.own.push(own("Fire brigade", a, "nbr-9077", "stairs", json!({"width_min": 1200.0, "guard_min": 1100.0})));
        codes.own.push(own("City", a, "Fire brigade", "stairs", json!({"guard_min": 1300.0})));
    }
    run(&mut s, "building_code", json!({"set": "nbr-9077", "amendments": ["City", "Fire brigade"]}));
    assert_eq!(s.model.stair_rules().guard_min, 1100.0, "the last one wins");
    run(&mut s, "building_code", json!({"amendments": ["Fire brigade", "City"]}));
    assert_eq!(s.model.stair_rules().guard_min, 1300.0);

    // A change made in an amendment is the amendment's, not the project's.
    let r = run(&mut s, "building_code", json!({"amend": "City", "rules": {"stairs": {"headroom": 2300}}}));
    assert!(s.model.codes.architectural.changes.is_empty(), "{r}");
    let city = s.model.codes.own.iter().find(|c| c.code == "City").unwrap();
    assert_eq!(city.sections["stairs"]["headroom"], json!(2300.0), "as the section types it");
    assert_eq!(s.model.resolve_code(a, "stairs").from["headroom"], Layer::Amendment("City".into()));
    assert_eq!(r["amendments"][1]["custom"], json!(true));
    // null takes a rule out of it: the layers under it give it again.
    run(&mut s, "building_code", json!({"amend": "City", "rules": {"stairs": {"headroom": null}}}));
    assert_eq!(s.model.resolve_code(a, "stairs").from["headroom"], Layer::Base("nbr-9077".into()));
    // Only an amendment the project has, and what can't be rules is refused as ever.
    assert!(s.execute("building_code", &json!({"amend": "nbr-9077", "rules": {"stairs": {"headroom": 2300}}})).is_err());
    let e = s.execute("building_code", &json!({"amend": "City", "rules": {"stairs": {"riser_min": 250}}})).unwrap_err();
    assert!(e.contains("riser"), "{e}");
    // The project's own changes, made before, go into the amendment when it is edited.
    run(&mut s, "building_code", json!({"rules": {"stairs": {"landing_length": 1300}}}));
    run(&mut s, "building_code", json!({"amend": "City", "rules": {"stairs": {"landing_length": 1300}}}));
    assert!(s.model.codes.architectural.changes.is_empty(), "no longer the project's own");

    // One of BIMCraft's amendments is changed as the project's copy, listed once; reset gives the library's back.
    let p = Discipline::Plumbing;
    run(&mut s, "building_code", json!({"discipline": "plumbing", "set": "nbr", "amendments": ["cbpmesp-it22"]}));
    let before = s.model.resolve_code(p, "hydrants").value["max_speed"].clone();
    run(&mut s, "building_code", json!({"discipline": "plumbing", "amend": "cbpmesp-it22", "rules": {"hydrants": {"max_speed": 4.5}}}));
    let now = s.model.resolve_code(p, "hydrants");
    assert_eq!((now.value["max_speed"].clone(), now.from["max_speed"].clone()), (json!(4.5), Layer::Amendment("cbpmesp-it22".into())));
    assert_eq!(s.model.codes.all(p).filter(|(c, _)| c.code == "cbpmesp-it22").count(), 1, "listed once, as the project's");
    run(&mut s, "building_code", json!({"discipline": "plumbing", "amend": "cbpmesp-it22", "reset": true}));
    assert_eq!(s.model.resolve_code(p, "hydrants").value["max_speed"], before);
    assert!(s.model.codes.own.iter().all(|c| c.code != "cbpmesp-it22"));

    // A new amendment, empty, over the base; chosen with it in one go, and then edited.
    run(&mut s, "building_code", json!({"create": "Our office", "amendments": ["Fire brigade", "City", "Our office"]}));
    let ours = s.model.codes.own.iter().find(|c| c.code == "Our office").unwrap();
    assert_eq!((ours.amends.as_str(), ours.sections.len()), ("nbr-9077", 0));
    run(&mut s, "building_code", json!({"amend": "Our office", "rules": {"doors": {"clear_width_min": 900}}}));
    assert_eq!(s.model.rules::<checks::DoorRules>().clear_width_min, 900.0);
    assert!(s.execute("building_code", &json!({"create": "City"})).unwrap_err().contains("City"), "a name taken");
    // Reset of a section takes it out of an amendment of the project's own.
    run(&mut s, "building_code", json!({"amend": "Our office", "reset": "doors"}));
    assert!(s.model.codes.own.iter().find(|c| c.code == "Our office").unwrap().sections.is_empty());
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

    // An amendment over a code that isn't under it is refused.
    let e = s.execute("building_code", &json!({"amendments": ["Elsewhere"]})).unwrap_err();
    assert!(e.contains("amends `ibc`"), "{e}");
    assert!(s.execute("building_code", &json!({"amendments": ["City"]})).unwrap_err().contains("amends `Fire brigade`"));
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

/// The rules `code_check` finds element `id` outside of.
fn outside(s: &mut Session, id: u64) -> Vec<String> {
    let r = run(s, "code_check", json!({"ids": [id]}));
    let list = r["elements"].get(0).map(|e| e["findings"].clone()).unwrap_or(json!([]));
    list.as_array().into_iter().flatten().filter_map(|f| f["rule"].as_str().map(String::from)).collect()
}

#[test]
fn ramps_doors_and_rooms_are_held_to_the_project_s_code() {
    let mut s = Session::new();
    // A new ramp starts as the code says: the generic one's, then NBR 9050's width.
    let id = run(&mut s, "ramp", json!({"start": [0, 0], "angle": 0, "height": 600}))["id"].as_u64().unwrap();
    let Some(crate::model::Element::Ramp(r)) = s.model.elements.get(id) else { panic!("a ramp") };
    assert_eq!((r.slope, r.width), (100.0 / 12.0, 1200.0));
    assert!(outside(&mut s, id).is_empty(), "the generic code asks nothing of it");
    run(&mut s, "building_code", json!({"set": "nbr-9050"}));
    let id = run(&mut s, "ramp", json!({"start": [0, 5000], "angle": 0, "height": 900}))["id"].as_u64().unwrap();
    let Some(crate::model::Element::Ramp(r)) = s.model.elements.get(id) else { panic!("a ramp") };
    assert_eq!(r.width, 1500.0);
    // 900 mm up at 8.33 %: NBR 9050 allows 6.25 % for a run rising more than 0.80 m.
    assert_eq!(outside(&mut s, id), ["slope_by_rise"]);
    run(&mut s, "modify", json!({"id": id, "slope": 6.0}));
    assert!(outside(&mut s, id).is_empty());
    // The IBC: 30 inches up a run at most, 36 inches wide.
    run(&mut s, "building_code", json!({"set": "ibc"}));
    let id = run(&mut s, "ramp", json!({"start": [0, 10000], "angle": 0, "height": 1000, "width": 800}))["id"].as_u64().unwrap();
    assert_eq!(outside(&mut s, id), ["width_min", "run_rise_max"]);

    // Doors: the IBC's 32 inches clear, and a closet's door left alone.
    run(&mut s, "wall", json!({"points": [[0, 20000], [8000, 20000], [8000, 26000], [0, 26000], [0, 20000]]}));
    run(&mut s, "type", json!({"category": "door", "name": "Narrow 700", "width": 700, "height": 2000}));
    run(&mut s, "type", json!({"category": "door", "name": "Closet 600", "width": 600, "height": 2000}));
    let narrow = run(&mut s, "door", json!({"at": [2000, 20000], "type": "Narrow 700"}))["id"].as_u64().unwrap();
    let closet = run(&mut s, "door", json!({"at": [5000, 20000], "type": "Closet 600"}))["id"].as_u64().unwrap();
    assert_eq!(outside(&mut s, narrow), ["clear_width_min", "clear_height_min"]);
    assert!(outside(&mut s, closet).is_empty(), "exempt by its name");
    run(&mut s, "building_code", json!({"rules": {"doors": {"clear_height_min": 2000}}}));
    assert_eq!(outside(&mut s, narrow), ["clear_width_min"], "the project's own height");

    // Rooms: a bedroom 1.5 by 2 m is short of the IBC's 70 square feet and 7 feet across; a kitchen isn't held to them.
    run(&mut s, "wall", json!({"points": [[0, 22000], [1500, 22000], [1500, 20000]]}));
    let small = run(&mut s, "room", json!({"point": [700, 21000], "name": "Bedroom 2"}))["id"].as_u64().unwrap();
    let big = run(&mut s, "room", json!({"point": [5000, 24000], "name": "Kitchen"}))["id"].as_u64().unwrap();
    assert_eq!(outside(&mut s, small), ["area_min", "side_min", "light_min"], "and it has no window");
    assert!(outside(&mut s, big).is_empty());
    let all = run(&mut s, "code_check", json!({}));
    assert!(all["count"].as_u64().unwrap() >= 5, "{all}");
    // The generic code has no kinds of room: nothing is reported.
    run(&mut s, "building_code", json!({"set": "generic"}));
    assert!(outside(&mut s, small).is_empty());
}

/// A room of a kind with `light_min` has windows of that share of its floor at least: the windows in the walls round it,
/// and only those.
#[test]
fn a_room_is_held_to_the_window_area_its_kind_asks_for() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "ibc"}));
    // A 4 by 3 m bedroom: 12 m², so the IBC's 8 % is 0.96 m² of glass.
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000], [0, 0]]}));
    let room = run(&mut s, "room", json!({"point": [2000, 1500], "name": "Bedroom"}))["id"].as_u64().unwrap();
    assert_eq!(outside(&mut s, room), ["light_min"], "no window at all");
    // A window in the next room's wall is no light of this one.
    run(&mut s, "wall", json!({"points": [[4000, 0], [8000, 0], [8000, 3000], [4000, 3000]]}));
    run(&mut s, "type", json!({"category": "window", "name": "Small 600", "width": 600, "height": 600, "sill": 1200}));
    run(&mut s, "type", json!({"category": "window", "name": "Wide 1200", "width": 1200, "height": 1000, "sill": 1000}));
    run(&mut s, "window", json!({"at": [6000, 0], "type": "Wide 1200"}));
    assert_eq!(outside(&mut s, room), ["light_min"]);
    // 0.36 m² is 3 % of the floor: still short.
    run(&mut s, "window", json!({"at": [2000, 0], "type": "Small 600"}));
    let r = run(&mut s, "code_check", json!({"ids": [room]}));
    let f = &r["elements"][0]["findings"][0];
    assert_eq!((f["rule"].as_str(), f["limit"].as_f64()), (Some("light_min"), Some(0.08)), "{r}");
    assert!((f["value"].as_f64().unwrap() - 0.03).abs() < 1e-9, "{f}");
    // 1.2 m² more is 13 %: inside.
    run(&mut s, "window", json!({"at": [0, 1500], "type": "Wide 1200"}));
    assert!(outside(&mut s, room).is_empty());
    // The project asks for a sixth: 1.56 m² of 12 is 13 %, under 16.7 %.
    let mut kinds = s.model.rules::<crate::codes::checks::RoomRules>().kinds.clone();
    kinds.iter_mut().for_each(|k| k.light_min = if k.kind == "habitable" { 1.0 / 6.0 } else { k.light_min });
    run(&mut s, "building_code", json!({"rules": {"rooms": {"kinds": kinds}}}));
    assert_eq!(outside(&mut s, room), ["light_min"]);
}

/// Italian building code (DM 5/7/1975) and regional amendment for Friuli Venezia Giulia (LR 44/1985 and LR 19/2009):
/// room heights (2.70 m national, 2.50 m FVG; 1.90 m attic), minimum room areas (14 m² double bedroom,
/// 9 m² single bedroom) and natural lighting (1/8 national, 1/10 FVG, 1/20 attic).
#[test]
fn italian_building_code_checks_room_heights_areas_and_natural_lighting() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "it-dm-1975"}));
    assert_eq!(s.model.codes.architectural.code.as_deref(), Some("it-dm-1975"));

    // 1. Single bedroom (Camera singola): 2.5 x 3.2 m = 8.0 m² (< 9 m²) at height 2.60 m (< 2.70 m) with no window.
    run(&mut s, "wall", json!({"points": [[0, 0], [2500, 0], [2500, 3200], [0, 3200], [0, 0]]}));
    let single =
        run(&mut s, "room", json!({"point": [1000, 1000], "name": "Camera singola", "top_level": null, "limit_offset": 2600.0}))["id"]
            .as_u64()
            .unwrap();
    let findings = outside(&mut s, single);
    assert_eq!(findings, ["area_min", "light_min", "height_min"]);

    // 2. Bathroom (Bagno): height 2.50 m (> 2.40 m), light_min is 0.0.
    run(&mut s, "wall", json!({"points": [[3000, 0], [5000, 0], [5000, 2000], [3000, 2000], [3000, 0]]}));
    let bath = run(&mut s, "room", json!({"point": [4000, 1000], "name": "Bagno", "top_level": null, "limit_offset": 2500.0}))["id"]
        .as_u64()
        .unwrap();
    assert!(outside(&mut s, bath).is_empty(), "a bathroom needs only 2.40 m height and no mandatory window in DM 75");

    // 3. Double bedroom (Camera matrimoniale): 4.0 x 3.6 m = 14.4 m² (> 14 m²), height 2.70 m.
    // Window needed: 14.4 / 8 = 1.80 m².
    run(&mut s, "wall", json!({"points": [[6000, 0], [10000, 0], [10000, 3600], [6000, 3600], [6000, 0]]}));
    let double =
        run(&mut s, "room", json!({"point": [8000, 1800], "name": "Camera matrimoniale", "top_level": null, "limit_offset": 2700.0}))["id"]
            .as_u64()
            .unwrap();
    assert_eq!(outside(&mut s, double), ["light_min"]);

    // Add a window of 1.4 x 1.4 m = 1.96 m² (> 1.80 m²):
    run(&mut s, "type", json!({"category": "window", "name": "Finestra 140x140", "width": 1400, "height": 1400, "sill": 1000}));
    run(&mut s, "window", json!({"at": [8000, 0], "type": "Finestra 140x140"}));
    assert!(outside(&mut s, double).is_empty(), "with 1/8 window area it passes all checks");

    // 4. Friuli Venezia Giulia amendment (L.R. 44/1985 and L.R. 19/2009):
    // Habitable height minimum is 2.50 m (art. 2) and natural lighting minimum is 1/10 (art. 7).
    run(&mut s, "building_code", json!({"set": "it-dm-1975", "amendments": ["it-fvg"]}));
    assert_eq!(s.model.codes.architectural.code.as_deref(), Some("it-dm-1975"));
    assert_eq!(s.model.codes.architectural.amendments, ["it-fvg"]);

    // A room 3.0 x 3.2 m = 9.6 m² at height 2.50 m with window 1.0 x 1.0 m = 1.0 m²:
    // Under DM 75 (national): 2.50 m fails height_min (2.70 m), and 1.0 / 9.6 = 10.4% fails light_min (12.5%).
    // Under FVG LR 44/85: 2.50 m passes height_min (2.50 m), and 10.4% passes light_min (10.0%)!
    run(&mut s, "wall", json!({"points": [[12000, 0], [15000, 0], [15000, 3200], [12000, 3200], [12000, 0]]}));
    let fvg_room =
        run(&mut s, "room", json!({"point": [13000, 1500], "name": "Camera singola 2", "top_level": null, "limit_offset": 2500.0}))["id"]
            .as_u64()
            .unwrap();
    run(&mut s, "type", json!({"category": "window", "name": "Finestra 100x100", "width": 1000, "height": 1000, "sill": 1000}));
    run(&mut s, "window", json!({"at": [13500, 0], "type": "Finestra 100x100"}));
    assert!(outside(&mut s, fvg_room).is_empty(), "under FVG LR 44/85, 2.50 m height and 1/10 window area pass");

    // LR 44/85 art. 6, in a seismic zone: a double bedroom needs 12 m², not 14 m². 3.5 x 3.6 m = 12.6 m² at 2.60 m,
    // with a window of 1.96 m² (15.6% of the floor).
    run(&mut s, "wall", json!({"points": [[22000, 0], [25500, 0], [25500, 3600], [22000, 3600], [22000, 0]]}));
    let fvg_double =
        run(&mut s, "room", json!({"point": [23500, 1800], "name": "Camera doppia 2", "top_level": null, "limit_offset": 2600.0}))["id"]
            .as_u64()
            .unwrap();
    run(&mut s, "window", json!({"at": [23500, 0], "type": "Finestra 140x140"}));
    assert!(outside(&mut s, fvg_double).is_empty(), "under FVG LR 44/85 art. 6 a double bedroom of 12.6 m² passes");

    // Switching back to national code reports both height and light outside rules:
    run(&mut s, "building_code", json!({"set": "it-dm-1975", "amendments": []}));
    assert_eq!(outside(&mut s, fvg_room), ["light_min", "height_min"]);
    assert_eq!(outside(&mut s, fvg_double), ["area_min", "height_min"]);

    // 5. Attic recovery under L.R. 19/2009 art. 39:
    // Average height minimum 1.90 m and natural lighting minimum 1/20 (5%).
    run(&mut s, "building_code", json!({"set": "it-dm-1975", "amendments": ["it-fvg"]}));
    run(&mut s, "wall", json!({"points": [[16000, 0], [20000, 0], [20000, 3000], [16000, 3000], [16000, 0]]}));
    let attic = run(&mut s, "room", json!({"point": [17000, 1500], "name": "Mansarda", "top_level": null, "limit_offset": 2000.0}))["id"]
        .as_u64()
        .unwrap();
    run(&mut s, "type", json!({"category": "window", "name": "Velux 80x80", "width": 800, "height": 800, "sill": 800}));
    run(&mut s, "window", json!({"at": [17000, 0], "type": "Velux 80x80"}));
    assert!(outside(&mut s, attic).is_empty(), "attic recovery under FVG LR 19/2009 passes with 2.00 m height and 1/20 light");
}

/// The electrical commands follow the project's electrical code: NBR 5410's sections, a utility's amendment for a building's
/// demand and its categories, and what the project or an amendment changed of them.
#[test]
fn the_electrical_commands_follow_the_project_s_code_and_what_it_changed() {
    let mut s = Session::new();
    let e = Discipline::Electrical;
    let room = json!({"rooms": [{"name": "Sala", "area": 20, "perimeter": 18000, "points": 2}]});
    assert_eq!(run(&mut s, "load_points", room.clone())["rooms"][0]["tug_minimum"], json!(4), "the library's NBR 5410");
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410"}));
    // A living room's outlet every 3 m of its perimeter, not 5: the project's own.
    let mut rooms = s.model.resolve_code(e, "load_points").value["rooms"].clone();
    for r in rooms.as_array_mut().unwrap().iter_mut().filter(|r| r["kind"] == "dry") {
        r["per_perimeter_m"] = json!(3.0);
    }
    run(&mut s, "building_code", json!({"discipline": "electrical", "rules": {"load_points": {"rooms": rooms}}}));
    assert_eq!(run(&mut s, "load_points", room.clone())["rooms"][0]["tug_minimum"], json!(6));
    // A residual-current device of 10 mA at most.
    run(&mut s, "building_code", json!({"discipline": "electrical", "rules": {"rcd": {"max_residual_ma": 10}}}));
    let r = run(&mut s, "dr_check", json!({"circuits": [{"name": "1", "points": [{"place": "bath", "kind": "outlet"}]}]}));
    assert!(r.to_string().contains("10"), "{r}");

    // A building's demand: the utility is an amendment over NBR 5410; with none, a utility is named.
    let flats = json!({"dwellings": [{"count": 3, "area": 60}]});
    assert!(s.execute("building_demand", &flats).unwrap_err().contains("Building Codes"));
    run(&mut s, "building_code", json!({"discipline": "electrical", "amendments": ["cemig"]}));
    let e2 = s.execute("building_demand", &flats).unwrap_err();
    assert!(e2.contains("CEMIG ND-5.2 covers 4 dwellings or more"), "{e2}");
    // The project's copy of Cemig's standard takes three.
    run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "cemig", "rules": {"building_demand": {"min_dwellings": 3}}}));
    let r = run(&mut s, "building_demand", flats.clone());
    assert_eq!(r["standard"], "CEMIG ND-5.2");
    // Named, BIMCraft's: the project's copy is the one by that name.
    assert!(s.execute("building_demand", &json!({"utility": "CEMIG", "dwellings": [{"count": 3, "area": 60}]})).is_ok());
    assert!(s.execute("building_demand", &json!({"utility": "CELESC", "dwellings": [{"count": 3, "area": 60}]})).is_ok());

    // Service categories: the utility's amendment, changed, is what `demand` reads for it.
    let lib = crate::entrance::find("COPEL").unwrap().0;
    let rows = serde_json::to_string(&s.model.codes).unwrap();
    assert!(!rows.contains("NTC 901100"), "not the project's yet");
    assert!(!lib.tables.is_empty());
    run(&mut s, "building_code", json!({"discipline": "electrical", "amendments": ["copel"]}));
    let (held, own) = s.model.section_as::<crate::entrance::Utility>(e, "entrance").unwrap();
    assert!(own && held.id == "COPEL");
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "amend": "copel", "rules": {"entrance": {"rule": "the project's own"}}}),
    );
    assert_eq!(s.model.section_as::<crate::entrance::Utility>(e, "entrance").unwrap().0.rule, "the project's own");
}

/// Every element's check costs what is near it, never what is in the building: a room or a door of a 20-storey tower is
/// checked in about the time one of a 4-storey tower is.
#[test]
fn a_room_or_a_door_is_checked_in_the_same_time_however_tall_the_building() {
    let mut each = Vec::new();
    for storeys in [4, 20] {
        let mut s = Session::new();
        *s.model_mut() = crate::sample::highrise(storeys).unwrap();
        run(&mut s, "building_code", json!({"set": "ibc"}));
        let levels: Vec<u64> = s.model.levels().iter().map(|(id, _)| *id).collect();
        let mut held = Vec::new();
        for level in levels.iter().take(storeys as usize) {
            held.push(run(&mut s, "room", json!({"point": [6000, 6000], "level": level, "name": "Bedroom"}))["id"].as_u64().unwrap());
            // A door where the façade's windows leave room for one.
            for x in [500, 1500, 2500, 3500, 4500, 5500] {
                if let Ok(d) = s.execute("door", &json!({"at": [x, 0], "level": level})) {
                    held.push(d["id"].as_u64().unwrap());
                    break;
                }
            }
        }
        // Warm: what the model keeps (the lookup, the rooms) is made once, as it is in the window.
        let m = (*s.model).clone();
        let check = |id: u64| m.elements.get(id).map_or(0, |e| crate::codes::findings(&m, id, e).len());
        held.iter().for_each(|id| {
            check(*id);
        });
        let t = std::time::Instant::now();
        let rounds = 5;
        for _ in 0..rounds {
            for id in held.iter().take(8) {
                check(*id);
            }
        }
        each.push(t.elapsed().as_secs_f64() / f64::from(rounds * 8));
    }
    // Generous, for a debug build on a busy machine: what matters is that it doesn't grow with the building.
    assert!(each[1] < each[0] * 3.0 + 0.002, "4 storeys {:.3} ms, 20 storeys {:.3} ms an element", each[0] * 1e3, each[1] * 1e3);
}

#[test]
fn the_electrical_settings_are_rules_of_the_code_both_ways() {
    let mut s = Session::new();
    let e = Discipline::Electrical;
    // A code chosen brings its voltages, systems and wire types into the project.
    s.model_mut().voltages.remove("277 V");
    s.model_mut().voltages.remove("480 V");
    s.model_mut().distribution_systems.remove("277/480 V");
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nec"}));
    assert!(s.model.distribution_systems.contains_key("277/480 V") && s.model.voltages.contains_key("480 V"));
    assert!(s.model.wire_types.contains_key("Cu THHN 90 °C"));
    // One the project adds or changes is the project's change of its code; one it takes away, of the code's, is null there.
    run(&mut s, "voltage", json!({"name": "240 V", "nominal": 240}));
    let r = s.model.resolve_code(e, "voltages");
    assert_eq!(r.from.get("240 V"), Some(&Layer::Project));
    run(&mut s, "voltage", json!({"name": "277 V", "max": 300}));
    assert_eq!(s.model.resolve_code(e, "voltages").value["277 V"]["max"], json!(300.0));
    run(&mut s, "wire_type", json!({"name": "Cu THW 75 °C", "delete": true}));
    assert_eq!(s.model.codes.electrical.changes["wire_types"]["Cu THW 75 °C"], Value::Null);
    // Set back as the code has it, it is no change.
    run(&mut s, "voltage", json!({"name": "277 V", "max": 304.7}));
    assert!(!s.model.codes.electrical.changes["voltages"].contains_key("277 V"));
    // A change made in an amendment is the amendment's, and the project takes it; saved, a code carries them all.
    run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Site", "amendments": ["Site"]}));
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "amend": "Site", "rules": {"voltages": {"600 V": {"nominal": 600, "min": 540, "max": 660}}}}),
    );
    assert_eq!(s.model.voltages.get("600 V").map(|v| v.nominal), Some(600.0));
    let site = s.model.codes.own.iter().find(|c| c.code == "Site").unwrap();
    assert!(site.sections["voltages"].get("600 V").is_some());
    run(&mut s, "building_code", json!({"discipline": "electrical", "save": "Ours"}));
    let ours = s.model.codes.own.iter().find(|c| c.code == "Ours").unwrap();
    assert!(["240 V", "277 V", "600 V"].iter().all(|v| ours.sections["voltages"].get(*v).is_some()), "{:?}", ours.sections["voltages"]);
    assert!(ours.sections["distribution_systems"].get("277/480 V").is_some());
    // A setting that isn't one is refused in a code as in the project.
    let bad = json!({"discipline": "electrical", "rules": {"voltages": {"1 V": {"nominal": -5, "min": 0, "max": 1}}}});
    assert!(s.execute("building_code", &bad).is_err());
}

#[test]
fn nbr_5410_and_the_electrical_settings_go_out_as_xml_and_come_back_whole() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410"}));
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "rules": {"voltages": {"My 500 V": {"nominal": 500, "min": 450, "max": 550}}}}),
    );
    let d = dir("nbr5410");
    let path = d.join("nbr5410.xml").to_string_lossy().to_string();
    run(&mut s, "building_code_export", json!({"path": path, "discipline": "electrical", "name": "Obra X"}));
    let mut other = Session::new();
    // A rule written as text that reads like a number ("7", the default method) comes back as text.
    let r = run(&mut other, "building_code_import", json!({"path": path, "set": true}));
    assert!(r["left_out"].as_array().is_none_or(|a| a.is_empty()), "nothing left out: {r}");
    let e = Discipline::Electrical;
    assert_eq!(other.model.resolve_code(e, "circuits").value, s.model.resolve_code(e, "circuits").value);
    // The settings are named by the user, so any name comes back.
    assert!(other.model.resolve_code(e, "voltages").value.contains_key("My 500 V"));
    assert_eq!(other.model.resolve_code(e, "voltages").value, s.model.resolve_code(e, "voltages").value);
}

#[test]
fn two_utilities_layered_each_answer_by_their_own_method() {
    let flats = |u: &str| json!({"utility": u, "dwellings": [{"count": 20, "area": 100}, {"count": 16, "area": 80}]});
    let mut lib = Session::new();
    let fecoergs = run(&mut lib, "building_demand", flats("FECOERGS"));
    let hidropan = run(&mut lib, "building_demand", flats("HIDROPAN"));
    let mut s = Session::new();
    // Cemig's or Celesc's rules never fill in what Fecoergs's or Hidropan's method leaves out.
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410", "amendments": ["cemig", "fecoergs"]}));
    assert_eq!(run(&mut s, "building_demand", flats("FECOERGS")), fecoergs);
    run(&mut s, "building_code", json!({"discipline": "electrical", "amendments": ["celesc", "hidropan"]}));
    assert_eq!(run(&mut s, "building_demand", flats("HIDROPAN")), hidropan);
}

#[test]
fn a_utility_s_method_goes_out_as_xml_and_comes_back_as_it_was() {
    // Cemig's method has no common areas' factors (null): it must not come back with another utility's.
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410", "amendments": ["cemig", "equatorial"]}));
    let e = Discipline::Electrical;
    let before = s.model.resolve_code(e, "building_demand").value;
    let d = dir("cemig");
    let path = d.join("cemig.xml").to_string_lossy().to_string();
    run(&mut s, "building_code_export", json!({"path": path, "discipline": "electrical", "name": "W1"}));
    let mut other = Session::new();
    run(&mut other, "building_code_import", json!({"path": path, "set": true}));
    assert_eq!(other.model.resolve_code(e, "building_demand").value, before);
}

#[test]
fn setting_the_base_in_force_again_keeps_its_amendments_for_every_base() {
    let e = Discipline::Electrical;
    for base in of(e).filter(|c| c.amends.is_empty()) {
        // The default, chosen by default: an own amendment over it, changed, stays when it is named.
        let mut s = Session::new();
        let mut started = run(&mut s, "building_code", json!({"discipline": "electrical", "set": base.code}));
        if base.code == default_of(e).code {
            s = Session::new();
            started = run(&mut s, "building_code", json!({"discipline": "electrical"}));
        }
        assert_eq!(started["code"], json!(base.code), "{}", base.code);
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Site", "amendments": ["Site"]}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Site", "rules": {"surge": {"follow_current": 50}}}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": base.code}));
        assert_eq!(s.model.codes.electrical.amendments, vec!["Site".to_string()], "{}", base.code);
        assert_eq!(s.model.resolve_code(e, "surge").value["follow_current"], json!(50), "{}", base.code);
        // Each of BIMCraft's amendments over it, as well.
        for a in of(e).filter(|c| c.amends == base.code) {
            let mut s = Session::new();
            run(&mut s, "building_code", json!({"discipline": "electrical", "set": base.code, "amendments": [a.code]}));
            run(&mut s, "building_code", json!({"discipline": "electrical", "set": base.code}));
            assert_eq!(s.model.codes.electrical.amendments, vec![a.code.clone()], "{} over {}", a.code, base.code);
        }
    }
}

/// The project's voltages, systems and wire types, by name.
fn settings(s: &Session) -> [Vec<String>; 3] {
    let m = &s.model;
    [m.voltages.keys().cloned().collect(), m.distribution_systems.keys().cloned().collect(), m.wire_types.keys().cloned().collect()]
}

#[test]
fn every_setting_the_code_takes_away_leaves_the_project_and_stays_away() {
    use crate::electrical;
    let e = Discipline::Electrical;
    let lists = [
        ("voltages", electrical::voltages().into_keys().collect::<Vec<_>>()),
        ("distribution_systems", electrical::distribution_systems().into_keys().collect()),
        ("wire_types", electrical::wire_types().into_keys().collect()),
    ];
    for (i, (key, names)) in lists.iter().enumerate() {
        for name in names {
            let mut s = Session::new();
            let r = s.execute("building_code", &json!({"discipline": "electrical", "rules": {*key: {name.as_str(): null}}}));
            let supplied = *key == "voltages" && s.model.distribution_systems.values().any(|d| d.voltage_names().any(|v| v == name));
            if supplied {
                // A voltage a system supplies is refused, in the code as in the dialog.
                assert!(r.is_err(), "{key} {name}: {r:?}");
                assert!(s.model.voltages.contains_key(name));
                continue;
            }
            r.unwrap_or_else(|e| panic!("{key} {name}: {e}"));
            assert!(!settings(&s)[i].contains(name), "{key} {name} taken away by the code is out of the project");
            // Out, into a new project: it doesn't come back.
            let path = dir("away").join(format!("{i}-{}.xml", name.replace(['/', ' ', '°'], "_"))).to_string_lossy().to_string();
            run(&mut s, "building_code_export", json!({"path": path, "discipline": "electrical", "name": "Away"}));
            let mut other = Session::new();
            run(&mut other, "building_code_import", json!({"path": path, "set": true}));
            assert_eq!(settings(&other), settings(&s), "{key} {name} after export and import");
            assert_eq!(other.model.resolve_code(e, key).value, s.model.resolve_code(e, key).value);
        }
    }
    // Taken away by the dialog's commands where no code names it: the same.
    for name in electrical::wire_types().into_keys() {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nec"}));
        run(&mut s, "wire_type", json!({"name": name, "delete": true}));
        let path = dir("deleted").join(format!("{}.xml", name.replace(['/', ' ', '°'], "_"))).to_string_lossy().to_string();
        run(&mut s, "building_code_export", json!({"path": path, "discipline": "electrical", "name": "Deleted"}));
        let mut other = Session::new();
        run(&mut other, "building_code_import", json!({"path": path, "set": true}));
        assert!(!other.model.wire_types.contains_key(&name), "{name} deleted comes back on import");
    }
}

#[test]
fn a_setting_no_code_names_any_more_goes_back_to_what_a_new_project_has() {
    let e = Discipline::Electrical;
    for base in of(e).filter(|c| c.amends.is_empty()) {
        let mut s = Session::new();
        // One of the project's, then another base: the code no longer names it, so the project no longer has it.
        run(&mut s, "voltage", json!({"name": "690 V", "nominal": 690}));
        run(
            &mut s,
            "wire_type",
            json!({"name": "Al X", "material": "aluminium", "temperature": 90, "insulation": "XLPE",
            "sizes": [{"section": 16, "ampacity": 60}]}),
        );
        run(&mut s, "voltage", json!({"name": "230 V", "nominal": 231}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": base.code}));
        assert!(!s.model.voltages.contains_key("690 V"), "{}", base.code);
        assert!(!s.model.wire_types.contains_key("Al X"), "{}", base.code);
        let named = s.model.resolve_code(e, "voltages").value;
        let want = match named.get("230 V") {
            Some(v) => serde_json::from_value(v.clone()).unwrap(),
            None => crate::electrical::voltages()["230 V"].clone(),
        };
        assert_eq!(s.model.voltages["230 V"], want, "{}: 230 V as the code or a new project has it", base.code);
    }
}

#[test]
fn a_voltage_renamed_is_renamed_in_the_code_s_systems_too() {
    let e = Discipline::Electrical;
    for (name, supplied) in crate::electrical::voltages().into_keys().map(|v| {
        let s = Session::new();
        let by: Vec<String> =
            s.model.distribution_systems.iter().filter(|(_, d)| d.voltage_names().any(|x| *x == v)).map(|(k, _)| k.clone()).collect();
        (v, by)
    }) {
        let mut s = Session::new();
        let to = format!("{name} new");
        run(&mut s, "voltage", json!({"name": name, "rename": to}));
        let code = s.model.resolve_code(e, "distribution_systems").value;
        for system in &supplied {
            let names: Vec<String> = s.model.distribution_systems[system].voltage_names().cloned().collect();
            assert!(names.contains(&to), "{system} in the project names {to}");
            assert_eq!(code.get(system), Some(&json!(s.model.distribution_systems[system])), "{system} in the code as in the project");
        }
        // And another base after it leaves no system naming a voltage the project hasn't.
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410"}));
        for (k, d) in &s.model.distribution_systems {
            assert!(d.voltage_names().all(|v| s.model.voltages.contains_key(v)), "{k} after renaming {name}");
        }
    }
}

#[test]
fn a_system_naming_a_voltage_nobody_has_is_refused() {
    let mut s = Session::new();
    let bad = json!({"discipline": "electrical", "rules": {"distribution_systems": {"48 DC": {"phases": 1, "wires": 2, "l_n": "48 V"}}}});
    assert!(s.execute("building_code", &bad).is_err());
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "rules": {"voltages": {"48 V": {"nominal": 48, "min": 43, "max": 53}}}}),
    );
    run(&mut s, "building_code", bad);
    assert!(s.model.distribution_systems.contains_key("48 DC"));
    // Its voltage taken away under it: refused.
    let away = json!({"discipline": "electrical", "rules": {"voltages": {"48 V": null}}});
    assert!(s.execute("building_code", &away).is_err());
}

#[test]
fn a_code_of_the_project_s_holding_a_utility_s_method_is_not_taken_away_while_the_project_is_held_to_it() {
    let e = Discipline::Electrical;
    let whole: Vec<&str> = WHOLE.iter().filter(|(d, _)| *d == e).map(|(_, k)| *k).collect();
    for a in of(e).filter(|c| !c.amends.is_empty() && whole.iter().any(|k| c.sections.contains_key(*k))) {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": a.amends, "amendments": [a.code]}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "save": "Ours"}));
        let before: Vec<Value> = whole.iter().map(|k| json!(s.model.resolve_code(e, k).value)).collect();
        let r = s.execute("building_code", &json!({"discipline": "electrical", "delete": "Ours"}));
        assert!(r.is_err(), "{}: {r:?}", a.code);
        let after: Vec<Value> = whole.iter().map(|k| json!(s.model.resolve_code(e, k).value)).collect();
        assert_eq!(after, before, "{}: refused, nothing moved", a.code);
        // Held to another, it goes.
        run(&mut s, "building_code", json!({"discipline": "electrical", "save": "Ours 2"}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "delete": "Ours"}));
        assert!(s.model.codes.own.iter().all(|c| c.code != "Ours"), "{}", a.code);
    }
}

#[test]
fn an_amendment_s_few_settings_go_over_the_library_s_on_every_base() {
    let e = Discipline::Electrical;
    let lib = |key: &str| library(e, key).and_then(|c| c.section(key)).cloned().unwrap_or_default();
    for base in of(e).filter(|c| c.amends.is_empty()) {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": base.code}));
        let before = s.model.resolve_code(e, "wire_types").value;
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Site", "amendments": ["Site"]}));
        let al = json!({"material": "aluminium", "temperature": 90, "insulation": "XLPE", "sizes": [{"section": 16, "ampacity": 60}]});
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Site", "rules": {"wire_types": {"Al X": al}}}));
        let now = s.model.resolve_code(e, "wire_types").value;
        assert!(now.contains_key("Al X"), "{}", base.code);
        for k in before.keys().chain(if base.section("wire_types").is_none() { lib("wire_types") } else { Default::default() }.keys()) {
            assert!(now.contains_key(k), "{}: {k} is still in force", base.code);
        }
    }
}

#[test]
fn a_setting_taken_away_in_an_amendment_leaves_the_project_whoever_had_it() {
    use crate::electrical;
    let e = Discipline::Electrical;
    let site = |s: &mut Session| {
        run(s, "building_code", json!({"discipline": "electrical", "set": "nbr-5410"}));
        run(s, "building_code", json!({"discipline": "electrical", "create": "Site", "amendments": ["Site"]}));
    };
    // What a new project has, whether the code names it or not.
    for name in electrical::wire_types().into_keys() {
        let mut s = Session::new();
        site(&mut s);
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Site", "rules": {"wire_types": {name.as_str(): null}}}));
        assert!(!s.model.wire_types.contains_key(&name), "{name}");
        assert_eq!(s.model.resolve_code(e, "wire_types").value.get(&name), Some(&Value::Null), "{name}");
    }
    // The project's own.
    let mut s = Session::new();
    site(&mut s);
    run(&mut s, "voltage", json!({"name": "690 V", "nominal": 690}));
    run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Site", "rules": {"voltages": {"690 V": null}}}));
    assert!(!s.model.voltages.contains_key("690 V"));
    assert!(s.model.resolve_code(e, "voltages").value.get("690 V").is_none_or(Value::is_null));
}

#[test]
fn a_setting_an_element_names_is_not_taken_away_by_a_code() {
    for system in crate::electrical::distribution_systems().into_keys() {
        let mut s = Session::new();
        let b = run(&mut s, "component", json!({"family": "Distribution Board", "type": "12 circuits", "at": [0, 0]}))["id"].clone();
        run(&mut s, "panel", json!({"id": b, "distribution": system}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Site", "amendments": ["Site"]}));
        for p in [
            json!({"discipline": "electrical", "rules": {"distribution_systems": {system.as_str(): null}}}),
            json!({"discipline": "electrical", "amend": "Site", "rules": {"distribution_systems": {system.as_str(): null}}}),
        ] {
            let codes = s.model.codes.clone();
            assert!(s.execute("building_code", &p).is_err(), "{p}");
            assert_eq!(s.model.codes, codes, "{p}: refused, nothing changed");
            assert!(s.model.distribution_systems.contains_key(&system));
        }
    }
}

#[test]
fn a_system_naming_a_voltage_an_older_project_took_away_is_refused() {
    let e = Discipline::Electrical;
    let mut s = Session::new();
    run(&mut s, "distribution_system", json!({"name": "127/220 V", "delete": true}));
    run(&mut s, "voltage", json!({"name": "127 V", "delete": true}));
    // Saved before a starter's entry taken away was written down: the project lacks it, and its code says nothing.
    let mut codes = (*s.model.codes).clone();
    for key in crate::electrical::SECTIONS {
        if let Some(c) = codes.choice_mut(e).changes.get_mut(key) {
            c.retain(|_, v| !v.is_null());
        }
    }
    codes.choice_mut(e).changes.retain(|_, c| !c.is_empty());
    s.model_mut().codes = crate::shared::Shared::new(codes);
    let system =
        json!({"discipline": "electrical", "rules": {"distribution_systems": {"127 DC": {"phases": 1, "wires": 2, "l_n": "127 V"}}}});
    assert!(s.execute("building_code", &system).is_err());
    assert!(!s.model.distribution_systems.contains_key("127 DC"));
}

#[test]
fn a_method_a_project_changed_before_amendments_held_it_still_answers_as_changed() {
    // Before a utility's method was changed in its amendment, a project kept the change among its own (`changes`), and
    // `demand` read it there. Such a file answers as it did: the same as the change made in the amendment today.
    let mut checked = 0;
    for code in of(Discipline::Electrical) {
        for key in ["unit_demand", "building_demand", "entrance"] {
            let Some(original) = code.sections.get(key) else { continue };
            let mut changed = original.clone();
            if key == "building_demand" {
                changed["factor"] = json!(2.0);
            } else {
                breakers_up(&mut changed);
            }
            let held = |s: &mut Session| {
                let p = if code.amends.is_empty() {
                    json!({"discipline": "electrical", "set": code.code})
                } else {
                    json!({"discipline": "electrical", "set": code.amends, "amendments": [code.code]})
                };
                run(s, "building_code", p);
            };
            let (cmd, probe) = method_probe(code, key, &changed);
            // Today's way: the change in its amendment; a country's method, held by the base, in the project's copy of it.
            let mut today = Session::new();
            held(&mut today);
            if code.amends.is_empty() {
                let mut copy = code.clone();
                copy.code = format!("{} (project)", code.code);
                copy.based_on = code.code.clone();
                copy.sections.insert(key.into(), changed.clone());
                let codes = &mut *today.model_mut().codes;
                codes.own.push(copy.clone());
                codes.choice_mut(Discipline::Electrical).code = Some(copy.code.clone());
            } else {
                run(&mut today, "building_code", json!({"discipline": "electrical", "amend": code.code, "rules": {key: changed}}));
            }
            let expected = run(&mut today, cmd, probe.clone());
            // The older file: the change among the project's own, read back from its text.
            let mut older = Session::new();
            held(&mut older);
            older
                .model_mut()
                .codes
                .choice_mut(Discipline::Electrical)
                .changes
                .insert(key.into(), changed.as_object().cloned().unwrap_or_default());
            let text = serde_json::to_string(&*older.model).unwrap();
            let (m, left_out) = crate::model::Model::read(&text).unwrap();
            assert!(left_out.is_empty(), "{} {key}: {left_out:?}", code.code);
            *older.model_mut() = m;
            older.model.check().unwrap_or_else(|e| panic!("{} {key}: the read file is a valid project: {e}", code.code));
            // A country's method, held by the base, too: into an amendment of the project's own over it.
            let got = run(&mut older, cmd, probe.clone());
            let answer = |r: &Value| if key == "building_demand" { r["demand"].clone() } else { r["category"].clone() };
            assert_eq!(answer(&got), answer(&expected), "{} {key}: the older file's change, asked {probe}", code.code);
            assert!(
                !older.model.codes.choice(Discipline::Electrical).changes.contains_key(key),
                "{} {key}: moved into its layer",
                code.code
            );
            if !code.amends.is_empty() {
                // It is the amendment's now: a change made today wins, and a reset takes both away.
                let mut again = original.clone();
                if key == "building_demand" {
                    again["factor"] = json!(3.0);
                } else {
                    breakers_up(&mut again);
                    breakers_up(&mut again);
                }
                let mut edited = Session::new();
                *edited.model_mut() = (*older.model).clone();
                run(&mut edited, "building_code", json!({"discipline": "electrical", "amend": code.code, "rules": {key: again}}));
                let mut fresh = Session::new();
                held(&mut fresh);
                run(&mut fresh, "building_code", json!({"discipline": "electrical", "amend": code.code, "rules": {key: again}}));
                assert_eq!(
                    answer(&run(&mut edited, cmd, probe.clone())),
                    answer(&run(&mut fresh, cmd, probe.clone())),
                    "{} {key}: edited after",
                    code.code
                );
                let mut reset = Session::new();
                *reset.model_mut() = (*older.model).clone();
                run(&mut reset, "building_code", json!({"discipline": "electrical", "amend": code.code, "reset": key}));
                let mut library = Session::new();
                held(&mut library);
                assert_eq!(
                    answer(&run(&mut reset, cmd, probe.clone())),
                    answer(&run(&mut library, cmd, probe.clone())),
                    "{} {key}: reset",
                    code.code
                );
                // Another utility chosen in its place answers by its own method, not by the change.
                if let Some(other) =
                    of(Discipline::Electrical).find(|o| o.code != code.code && o.amends == code.amends && o.sections.contains_key(key))
                {
                    let (ocmd, oprobe) = method_probe(other, key, &other.sections[key]);
                    let mut swapped = Session::new();
                    *swapped.model_mut() = (*older.model).clone();
                    run(&mut swapped, "building_code", json!({"discipline": "electrical", "amendments": [other.code]}));
                    let mut plain = Session::new();
                    run(&mut plain, "building_code", json!({"discipline": "electrical", "set": other.amends, "amendments": [other.code]}));
                    assert_eq!(
                        answer(&run(&mut swapped, ocmd, oprobe.clone())),
                        answer(&run(&mut plain, ocmd, oprobe.clone())),
                        "{} {key}: {} chosen instead",
                        code.code,
                        other.code
                    );
                }
            }
            checked += 1;
        }
    }
    assert!(checked >= 8, "every utility's method was asked ({checked})");
}

#[test]
fn an_older_files_method_change_goes_to_the_utility_it_names_or_else_to_the_one_in_force() {
    // Two utilities held, one over the other. A whole method that names the lower one goes into the lower one; a change
    // that names none (a rule or two, as an older dialog kept it) into the one in force when the file was saved, the upper.
    let mut checked = 0;
    let all: Vec<&BuildingCode> = of(Discipline::Electrical).collect();
    for code in all.iter().copied().filter(|c| !c.amends.is_empty()) {
        for key in ["unit_demand", "building_demand", "entrance"] {
            let Some(original) = code.sections.get(key) else { continue };
            let Some(over) = all.iter().find(|o| o.code != code.code && o.amends == code.amends && o.sections.contains_key(key)) else {
                continue;
            };
            let answer = |r: &Value| if key == "building_demand" { r["demand"].clone() } else { r["category"].clone() };
            let held = json!({"discipline": "electrical", "set": code.amends, "amendments": [code.code, over.code]});
            let older = |changed: Value| {
                let mut s = Session::new();
                run(&mut s, "building_code", held.clone());
                s.model_mut()
                    .codes
                    .choice_mut(Discipline::Electrical)
                    .changes
                    .insert(key.into(), changed.as_object().cloned().unwrap_or_default());
                let (m, _) = crate::model::Model::read(&serde_json::to_string(&*s.model).unwrap()).unwrap();
                *s.model_mut() = m;
                s
            };
            let today = |target: &str, rules: Value| -> Result<Session, String> {
                let mut s = Session::new();
                run(&mut s, "building_code", held.clone());
                s.execute("building_code", &json!({"discipline": "electrical", "amend": target, "rules": {key: rules}}))?;
                Ok(s)
            };
            // The lower one's whole method, named.
            let mut changed = original.clone();
            if key == "building_demand" {
                changed["factor"] = json!(2.0);
            } else {
                breakers_up(&mut changed);
            }
            let Ok(mut expected) = today(&code.code, changed.clone()) else { continue }; // refused under another (G9)
            let mut got = older(changed);
            assert_eq!(
                got.model.resolve_code(Discipline::Electrical, key).value,
                expected.model.resolve_code(Discipline::Electrical, key).value,
                "{} named, under {}: the rules in force",
                code.code,
                over.code
            );
            for c in [code, *over] {
                let (cmd, probe) = method_probe(c, key, &c.sections[key]);
                assert_eq!(
                    answer(&run(&mut got, cmd, probe.clone())),
                    answer(&run(&mut expected, cmd, probe.clone())),
                    "{} named, under {}: asked {probe}",
                    code.code,
                    over.code
                );
            }
            // A change that names no utility: the one in force, the upper.
            let field = if key == "building_demand" { "factor" } else { "source" };
            let partial = json!({field: if key == "building_demand" { json!(2.5) } else { json!("changed") }});
            if let Ok(mut expected) = today(&over.code, partial.clone()) {
                let mut got = older(partial);
                for c in [code, *over] {
                    let (cmd, probe) = method_probe(c, key, &c.sections[key]);
                    let a = run(&mut got, cmd, probe.clone());
                    let b = run(&mut expected, cmd, probe.clone());
                    assert_eq!(
                        (answer(&a), &a["source"]),
                        (answer(&b), &b["source"]),
                        "unnamed change, {} under {}: asked {probe}",
                        code.code,
                        over.code
                    );
                }
            }
            checked += 1;
        }
    }
    assert!(checked >= 4, "pairs asked ({checked})");
}

#[test]
fn a_setting_the_project_added_and_an_amendment_takes_away_leaves_the_project_whichever_amendment() {
    // The dialog moves the project's settings into the amendment edited, whichever it is, and sends a setting the user
    // cleared as null there: the project's own change goes with it, under another amendment as on top.
    let e = Discipline::Electrical;
    for (i, key) in ["voltages", "distribution_systems", "wire_types"].into_iter().enumerate() {
        for target in ["Lower", "Upper"] {
            let mut s = Session::new();
            run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Lower", "amendments": ["Lower"]}));
            run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Upper", "amendments": ["Lower", "Upper"]}));
            let some = s.model.resolve_code(e, key).value.values().next().cloned().unwrap();
            run(&mut s, "building_code", json!({"discipline": "electrical", "rules": {key: {"Mine": some}}}));
            assert!(settings(&s)[i].iter().any(|n| n == "Mine"), "{key}: the project's own");
            assert!(s.model.codes.choice(e).changes.get(key).is_some_and(|c| c.contains_key("Mine")));
            let before = settings(&s);
            run(&mut s, "building_code", json!({"discipline": "electrical", "amend": target, "rules": {key: {"Mine": null}}}));
            let at = format!("{key} in {target}");
            assert!(!settings(&s)[i].iter().any(|n| n == "Mine"), "{at}: out of the project");
            assert!(!s.model.codes.choice(e).changes.get(key).is_some_and(|c| c.contains_key("Mine")), "{at}: no change left");
            assert!(s.model.resolve_code(e, key).value.get("Mine").is_none_or(Value::is_null), "{at}: not in force");
            // The null is in the amendment edited, and the other doesn't name it.
            let other = if target == "Lower" { "Upper" } else { "Lower" };
            let held = |code: &str| {
                let c = s.model.codes.own.iter().find(|c| c.code == code && c.discipline == e);
                c.and_then(|c| c.sections.get(key)).and_then(Value::as_object).and_then(|o| o.get("Mine")).cloned()
            };
            assert_eq!(held(target), Some(Value::Null), "{at}: a null in {target}");
            assert_eq!(held(other), None, "{at}: nothing in {other}");
            // Out, into a new project: it doesn't come back.
            let path = dir("amended away").join(format!("{key}-{target}.xml")).to_string_lossy().to_string();
            run(&mut s, "building_code_export", json!({"path": path, "discipline": "electrical", "name": "Away"}));
            let mut other = Session::new();
            run(&mut other, "building_code_import", json!({"path": path, "set": true}));
            assert_eq!(settings(&other), settings(&s), "{at}: after export and import");
            // Undone, the project has it again, as its own change.
            run(&mut s, "undo", json!({}));
            assert_eq!(settings(&s), before, "{at}: undone");
            assert!(s.model.codes.choice(e).changes.get(key).is_some_and(|c| c.contains_key("Mine")), "{at}: undone, its own");
        }
    }
}

#[test]
fn an_amendment_takes_away_a_setting_it_added_by_leaving_it_out_and_one_from_under_it_by_a_null() {
    // A setting only the amendment itself added leaves its section; one a layer under it gives stays away as a null there,
    // so the layer under keeps it and the amendment hides it.
    let e = Discipline::Electrical;
    let held = |s: &Session, code: &str, key: &str| -> Option<Value> {
        let c = s.model.codes.own.iter().find(|c| c.code == code && c.discipline == e)?;
        c.sections.get(key).and_then(Value::as_object).and_then(|o| o.get("Mine")).cloned()
    };
    for (i, key) in ["voltages", "distribution_systems", "wire_types"].into_iter().enumerate() {
        for adder in ["Lower", "Upper"] {
            let mut s = Session::new();
            run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Lower", "amendments": ["Lower"]}));
            run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Upper", "amendments": ["Lower", "Upper"]}));
            let some = s.model.resolve_code(e, key).value.values().next().cloned().unwrap();
            run(&mut s, "building_code", json!({"discipline": "electrical", "amend": adder, "rules": {key: {"Mine": some.clone()}}}));
            assert!(settings(&s)[i].iter().any(|n| n == "Mine"), "{key} added in {adder}");
            run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Upper", "rules": {key: {"Mine": null}}}));
            let at = format!("{key} added in {adder}, taken away in Upper");
            assert!(!settings(&s)[i].iter().any(|n| n == "Mine"), "{at}: out of the project");
            assert!(s.model.resolve_code(e, key).value.get("Mine").is_none_or(Value::is_null), "{at}: not in force");
            if adder == "Upper" {
                assert_eq!(held(&s, "Upper", key), None, "{at}: left out of Upper");
            } else {
                assert_eq!(held(&s, "Upper", key), Some(Value::Null), "{at}: a null in Upper");
                assert!(held(&s, "Lower", key).is_some_and(|v| !v.is_null()), "{at}: Lower keeps it");
            }
        }
    }
}

#[test]
fn an_amendment_takes_away_a_setting_a_layer_under_it_also_gives_even_when_it_gives_it_too() {
    // Both amendments give `Mine`, each its own: taken away in the upper one, it stays away, as when only the lower one
    // gives it; the lower one keeps its own.
    let e = Discipline::Electrical;
    let held = |s: &Session, code: &str, key: &str| -> Option<Value> {
        let c = s.model.codes.own.iter().find(|c| c.code == code && c.discipline == e)?;
        c.sections.get(key).and_then(Value::as_object).and_then(|o| o.get("Mine")).cloned()
    };
    for (i, key) in ["voltages", "distribution_systems", "wire_types"].into_iter().enumerate() {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Lower", "amendments": ["Lower"]}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Upper", "amendments": ["Lower", "Upper"]}));
        let mut values = s.model.resolve_code(e, key).value.into_values();
        let low = values.next().unwrap();
        // Another entry's rules where the section has one, so each layer's own can be told apart.
        let up = values.next().unwrap_or_else(|| low.clone());
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Lower", "rules": {key: {"Mine": low.clone()}}}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Upper", "rules": {key: {"Mine": up.clone()}}}));
        assert_eq!(s.model.resolve_code(e, key).value.get("Mine"), Some(&up), "{key}: the upper one's in force");
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Upper", "rules": {key: {"Mine": null}}}));
        assert!(!settings(&s)[i].iter().any(|n| n == "Mine"), "{key}: out of the project");
        assert!(s.model.resolve_code(e, key).value.get("Mine").is_none_or(Value::is_null), "{key}: not in force");
        assert_eq!(held(&s, "Upper", key), Some(Value::Null), "{key}: a null in Upper");
        assert_eq!(held(&s, "Lower", key), Some(low.clone()), "{key}: Lower keeps its own");
        // Out, into a new project: it doesn't come back.
        let path = dir("both away").join(format!("{key}.xml")).to_string_lossy().to_string();
        run(&mut s, "building_code_export", json!({"path": path, "discipline": "electrical", "name": "Away"}));
        let mut other = Session::new();
        run(&mut other, "building_code_import", json!({"path": path, "set": true}));
        assert_eq!(settings(&other), settings(&s), "{key}: after export and import");
        // Undone, the upper one's is in force again.
        run(&mut s, "undo", json!({}));
        assert_eq!(s.model.resolve_code(e, key).value.get("Mine"), Some(&up), "{key}: undone");
        assert_eq!(held(&s, "Upper", key), Some(up.clone()), "{key}: undone, Upper's own");
    }
}

#[test]
fn a_setting_only_the_amendment_gives_leaves_it_even_over_a_null_under_it() {
    // Only the upper amendment gives `Mine` (the lower one names none, or takes it away as a null): taken away in the
    // upper one, it leaves its section rather than stay there as a null.
    let e = Discipline::Electrical;
    for key in ["voltages", "distribution_systems", "wire_types"] {
        for lower_null in [false, true] {
            let mut s = Session::new();
            run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Lower", "amendments": ["Lower"]}));
            run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Upper", "amendments": ["Lower", "Upper"]}));
            let some = s.model.resolve_code(e, key).value.values().next().cloned().unwrap();
            if lower_null {
                // The project's own, taken away in the lower amendment: a null there.
                run(&mut s, "building_code", json!({"discipline": "electrical", "rules": {key: {"Mine": some.clone()}}}));
                run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Lower", "rules": {key: {"Mine": null}}}));
                let lower = s.model.codes.own.iter().find(|c| c.code == "Lower" && c.discipline == e).unwrap();
                let held = lower.sections.get(key).and_then(Value::as_object).and_then(|o| o.get("Mine"));
                assert_eq!(held, Some(&Value::Null), "{key}: a null in Lower");
            }
            run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Upper", "rules": {key: {"Mine": some.clone()}}}));
            run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Upper", "rules": {key: {"Mine": null}}}));
            let upper = s.model.codes.own.iter().find(|c| c.code == "Upper" && c.discipline == e).unwrap();
            let held = upper.sections.get(key).and_then(Value::as_object).and_then(|o| o.get("Mine"));
            assert_eq!(held, None, "{key}, a null under it {lower_null}: left out of Upper");
            assert!(s.model.resolve_code(e, key).value.get("Mine").is_none_or(Value::is_null), "{key}: not in force");
        }
    }
}

#[test]
fn a_setting_taken_away_in_a_lower_amendment_stays_away_under_an_upper_one_that_takes_it_away_too() {
    // The project's own base gives it, the upper amendment takes it away, then the lower one does: with the upper one out,
    // the lower one's null still hides the base's.
    let e = Discipline::Electrical;
    for key in ["voltages", "distribution_systems", "wire_types"] {
        let mut s = Session::new();
        let some = s.model.resolve_code(e, key).value.values().find(|v| !v.is_null()).cloned().unwrap();
        let name = "On the base";
        run(&mut s, "building_code", json!({"discipline": "electrical", "rules": {key: {name: some}}, "save": "Own base"}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Lower", "amendments": ["Lower"]}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Upper", "amendments": ["Lower", "Upper"]}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Upper", "rules": {key: {name: null}}}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Lower", "rules": {key: {name: null}}}));
        let lower = s.model.codes.own.iter().find(|c| c.code == "Lower" && c.discipline == e).unwrap();
        let held = lower.sections.get(key).and_then(Value::as_object).and_then(|o| o.get(name)).cloned();
        assert_eq!(held, Some(Value::Null), "{key}: a null in Lower");
        run(&mut s, "building_code", json!({"discipline": "electrical", "amendments": ["Lower"]}));
        assert!(s.model.resolve_code(e, key).value.get(name).is_none_or(Value::is_null), "{key}: Upper out, still away");
    }
}

#[test]
fn a_setting_only_an_amendment_gives_and_an_element_names_is_not_taken_away_by_it() {
    // Taken away where only the amendment gives it, the setting would leave the amendment's section rather than stay there
    // as a null: it is refused all the same while a board names it, with nothing changed.
    let e = Discipline::Electrical;
    for lower in [false, true] {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Site", "amendments": ["Site"]}));
        if lower {
            run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Over", "amendments": ["Site", "Over"]}));
        }
        let some = s.model.resolve_code(e, "distribution_systems").value.values().find(|v| !v.is_null()).cloned().unwrap();
        let rules = json!({"distribution_systems": {"Exclusive": some}});
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Site", "rules": rules}));
        let b = run(&mut s, "component", json!({"family": "Distribution Board", "type": "12 circuits", "at": [0, 0]}))["id"].clone();
        run(&mut s, "panel", json!({"id": b, "distribution": "Exclusive"}));
        let codes = s.model.codes.clone();
        let p = json!({"discipline": "electrical", "amend": "Site", "rules": {"distribution_systems": {"Exclusive": null}}});
        let r = s.execute("building_code", &p);
        assert!(r.as_ref().is_err_and(|e| e.contains("Exclusive")), "under another {lower}: taken");
        assert_eq!(s.model.codes, codes, "under another {lower}: refused, nothing changed");
        assert!(s.model.distribution_systems.contains_key("Exclusive"));
        assert!(s.model.resolve_code(e, "distribution_systems").value.get("Exclusive").is_some_and(|v| !v.is_null()));
    }
}

#[test]
fn taking_away_a_setting_an_element_names_never_hands_it_another_layer_s_rules() {
    // Both amendments give `Shared`, each its own; the project may give its own too. Taking it away in either amendment
    // while a board names it would bring another layer's rules in force for the board: refused, with nothing changed.
    let e = Discipline::Electrical;
    let key = "distribution_systems";
    for (target, project) in [("Upper", false), ("Lower", true)] {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Lower", "amendments": ["Lower"]}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": "Upper", "amendments": ["Lower", "Upper"]}));
        let low = s.model.resolve_code(e, key).value.into_values().find(|v| !v.is_null()).unwrap();
        // The same system earthed another way, so each layer's own can be told apart.
        let mut up = low.clone();
        up["ground"] = json!(if low["ground"] == json!("TT") { "TN-S" } else { "TT" });
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Lower", "rules": {key: {"Shared": low}}}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "amend": "Upper", "rules": {key: {"Shared": up.clone()}}}));
        if project {
            run(&mut s, "building_code", json!({"discipline": "electrical", "rules": {key: {"Shared": low.clone()}}}));
        }
        let b = run(&mut s, "component", json!({"family": "Distribution Board", "type": "12 circuits", "at": [0, 0]}))["id"].clone();
        run(&mut s, "panel", json!({"id": b, "distribution": "Shared"}));
        let (codes, system) = (s.model.codes.clone(), s.model.distribution_systems.get("Shared").cloned());
        let p = json!({"discipline": "electrical", "amend": target, "rules": {key: {"Shared": null}}});
        assert!(s.execute("building_code", &p).is_err(), "{target}, the project's own {project}: taken");
        assert_eq!(s.model.codes, codes, "{target}: refused, nothing changed");
        assert_eq!(s.model.distribution_systems.get("Shared").cloned(), system, "{target}: the board's system as it was");
    }
}

#[test]
fn a_broken_rule_is_refused_on_the_layer_it_is_given_even_under_another_that_hides_it() {
    // A door's clear width below zero, given in the lower amendment while the upper one gives its own: hidden in force, it
    // would answer the day the upper one goes. Refused on its layer, with nothing changed, as alone; a good one is taken.
    for upper in [false, true] {
        let mut s = Session::new();
        run(&mut s, "building_code", json!({"create": "Lower", "amendments": ["Lower"]}));
        run(&mut s, "building_code", json!({"create": "Upper", "amendments": ["Lower", "Upper"]}));
        if upper {
            run(&mut s, "building_code", json!({"amend": "Upper", "rules": {"doors": {"clear_width_min": 900}}}));
        }
        let codes = s.model.codes.clone();
        let r = s.execute("building_code", &json!({"amend": "Lower", "rules": {"doors": {"clear_width_min": -5}}}));
        assert!(r.is_err(), "hidden by the upper one {upper}: a broken rule was taken");
        assert_eq!(s.model.codes, codes, "hidden by the upper one {upper}: refused, nothing changed");
        run(&mut s, "building_code", json!({"amend": "Lower", "rules": {"doors": {"clear_width_min": 850}}}));
    }
}

#[test]
fn a_broken_rule_is_refused_on_its_layer_as_the_layers_stand_and_a_split_or_empty_ask_is_judged_whole() {
    // The layers as they stand, in the order the project chose (`Child` under `Parent`, though made over it): a broken rule
    // in `Child`, hidden by `Parent`'s over it, is refused; and `Child`'s rules are under `Target` when it stands under it.
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"create": "Parent", "amendments": ["Parent"]}));
    run(&mut s, "building_code", json!({"create": "Child", "over": "Parent", "amendments": ["Child", "Parent"]}));
    run(&mut s, "building_code", json!({"amend": "Parent", "rules": {"doors": {"clear_width_min": 900}}}));
    let codes = s.model.codes.clone();
    let r = s.execute("building_code", &json!({"amend": "Child", "rules": {"doors": {"clear_width_min": -5}}}));
    assert!(r.is_err(), "over its parent, chosen before it: a broken rule was taken");
    assert_eq!(s.model.codes, codes, "refused, nothing changed");
    // Under `Target` as they stand, `Child`'s rules are under it though what `Child` was made over stands above.
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"create": "Parent", "amendments": ["Parent"]}));
    run(&mut s, "building_code", json!({"create": "Child", "over": "Parent", "amendments": ["Child", "Parent"]}));
    run(&mut s, "building_code", json!({"create": "Target", "amendments": ["Child", "Target", "Parent"]}));
    run(&mut s, "building_code", json!({"amend": "Child", "rules": {"stairs": {"riser_max": 300}}}));
    run(&mut s, "building_code", json!({"amend": "Target", "rules": {"stairs": {"riser_min": 250}}}));
    // One ask in two parts (`rules.stairs` and `stairs`) is judged as it ends: 200 to 250 is a riser's range.
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"create": "Lower", "amendments": ["Lower"]}));
    run(&mut s, "building_code", json!({"amend": "Lower", "rules": {"stairs": {"riser_max": 180}}}));
    run(&mut s, "building_code", json!({"amend": "Lower", "rules": {"stairs": {"riser_min": 200}}, "stairs": {"riser_max": 250}}));
    // A section asked with no rule changes nothing: taken, even over a broken rule an older file holds.
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"create": "Lower", "amendments": ["Lower"]}));
    run(&mut s, "building_code", json!({"create": "Upper", "amendments": ["Lower", "Upper"]}));
    run(&mut s, "building_code", json!({"amend": "Upper", "rules": {"doors": {"clear_width_min": 900}}}));
    let mut codes = (*s.model.codes).clone();
    let lower = codes.own.iter_mut().find(|c| c.code == "Lower").unwrap();
    lower.sections.insert("doors".into(), json!({"clear_width_min": -5}));
    s.model_mut().codes = crate::shared::Shared::new(codes);
    run(&mut s, "building_code", json!({"amend": "Lower", "rules": {"doors": {}}}));
}

#[test]
fn a_method_s_last_rule_taken_away_is_not_checked_as_an_ordinary_section() {
    // A utility's method whole with its last rule taken away is no ordinary section: what is under it (here a broken
    // method an older file holds, hidden by another utility's on top) is not checked in its place.
    use crate::shared::Shared;
    let e = Discipline::Electrical;
    let mut s = Session::new();
    let p = json!({"discipline": "electrical", "set": "nbr-5410", "create": "Middle", "amendments": ["cemig", "Middle", "fecoergs"]});
    run(&mut s, "building_code", p);
    let mut codes = (*s.model.codes).clone();
    let mut lower = codes.find(e, "cemig").unwrap().clone();
    lower.based_on = "cemig".into();
    lower.sections.get_mut("building_demand").unwrap()["min_dwellings"] = json!("broken");
    let middle = codes.own.iter_mut().find(|c| c.code == "Middle").unwrap();
    middle.based_on = "cemig".into();
    middle.sections.insert("building_demand".into(), json!({"min_dwellings": 3}));
    codes.own.push(lower);
    codes.check().unwrap();
    s.model_mut().codes = Shared::new(codes);
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "amend": "Middle", "rules": {"building_demand": {"min_dwellings": null}}}),
    );
}

#[test]
fn a_voltage_only_a_kept_system_named_leaves_when_nothing_holds_it() {
    // 230 V renamed 231 V (the system that supplies it names 231 V), a board on that system: switched to NEC, the system
    // stays for its board with 231 V; switched again to REBT, whose 230/400 V names 230 V, nothing names 231 V any more.
    let mut s = Session::new();
    run(&mut s, "voltage", json!({"name": "230 V", "rename": "231 V"}));
    let b = run(&mut s, "component", json!({"family": "Distribution Board", "type": "12 circuits", "at": [0, 0]}))["id"].clone();
    run(&mut s, "panel", json!({"id": b, "distribution": "230/400 V"}));
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nec"}));
    let supplies = |s: &Session, v: &str| s.model.distribution_systems["230/400 V"].voltage_names().any(|n| n == v);
    assert!(s.model.voltages.contains_key("231 V") && supplies(&s, "231 V"), "the kept system still names it");
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "rebt"}));
    assert!(supplies(&s, "230 V") && !supplies(&s, "231 V"), "REBT's own 230/400 V now");
    let named = s.model.distribution_systems.values().any(|d| d.voltage_names().any(|v| v == "231 V"));
    assert!(named || !s.model.voltages.contains_key("231 V"), "231 V stays with nothing naming it");
    // One a new project has stays, whether a code names it or not.
    for v in crate::electrical::voltages().into_keys().filter(|v| v != "230 V") {
        assert!(s.model.voltages.contains_key(&v), "{v}, a new project's, is kept");
    }
}

#[test]
fn a_voltage_no_system_ever_named_is_left_as_it_is_when_the_codes_change() {
    // An older file's voltage no code and no system names: a change of the codes, or a code imported again as it is, leaves
    // it in the project.
    let at = dir("legacy voltage").join("generic.xml").to_string_lossy().to_string();
    let mut s = Session::new();
    run(&mut s, "building_code_export", json!({"discipline": "architectural", "path": at, "name": "Again"}));
    let old: crate::electrical::Voltage = serde_json::from_value(json!({"nominal": 123, "min": 110, "max": 135})).unwrap();
    s.model_mut().voltages.insert("123 V".into(), old);
    run(&mut s, "building_code_import", json!({"path": at, "set": false}));
    assert!(s.model.voltages.contains_key("123 V"), "imported");
    run(&mut s, "building_code_import", json!({"path": at, "set": false}));
    assert!(s.model.voltages.contains_key("123 V"), "imported again, the same");
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nec"}));
    assert!(s.model.voltages.contains_key("123 V"), "the codes switched");
}

#[test]
fn a_voltage_a_new_project_has_stays_when_the_system_that_named_it_is_taken_away() {
    // Under NEC, taking away 230/400 V, the only system naming 230 V, which NEC doesn't name either: a new project has 230 V,
    // so it stays.
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "nec", "rules": {"distribution_systems": {"230/400 V": null}}}));
    assert!(!s.model.distribution_systems.values().any(|d| d.voltage_names().any(|v| v == "230 V")), "nothing names it");
    assert!(s.model.voltages.contains_key("230 V"), "a new project's voltage stays");
}

#[test]
fn deleting_the_code_a_project_is_held_to_keeps_what_was_in_force_and_brings_in_nothing_more() {
    // The project is held to its own saved base `S1` and has chosen `F1`, an amendment made over `generic` that doesn't
    // stand over `S1`. Deleting `S1` holds the project to `generic` again, where `F1` would stand and bring in what wasn't in
    // force: it is no longer chosen, and nothing in force changes.
    let e = Discipline::Electrical;
    let f1 = dir("held base").join("f1.xml").to_string_lossy().to_string();
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"discipline": "electrical", "create": "P1", "amendments": ["P1"]}));
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "amend": "P1", "rules": {"voltages": {"48 V": {"nominal": 48, "min": 44, "max": 52}}}}),
    );
    run(&mut s, "building_code_export", json!({"discipline": "electrical", "code": "P1", "name": "F1", "path": f1}));
    run(&mut s, "building_code", json!({"discipline": "electrical", "amendments": []}));
    run(&mut s, "building_code", json!({"discipline": "electrical", "save": "S1"}));
    run(&mut s, "building_code_import", json!({"path": f1, "set": true}));
    let voltages = |s: &Session| s.model.resolve_code(e, "voltages").value;
    let (before, model) = (voltages(&s), s.model.voltages.clone());
    assert!(before.get("48 V").is_none_or(Value::is_null), "F1 doesn't stand over S1");
    run(&mut s, "building_code", json!({"discipline": "electrical", "delete": "S1"}));
    let after = voltages(&s);
    for (k, v) in &after {
        assert!(before.get(k).is_some_and(|b| b == v) || (v.is_null() && before.get(k).is_none()), "{k}: {v} came into force");
    }
    assert!(after.get("48 V").is_none_or(Value::is_null));
    assert_eq!(s.model.voltages, model, "the project's settings as they were");
    assert!(!s.model.codes.choice(e).amendments.contains(&"F1".to_string()), "F1 no longer chosen");
    assert!(s.model.codes.own.iter().any(|c| c.code == "F1"), "F1 still the project's, to choose again");
}

#[test]
fn deleting_a_held_code_keeps_chosen_the_amendments_that_stood_or_stand_on_neither() {
    // Of two amendments standing, deleting one keeps the other chosen; an amendment chosen over another base, standing
    // neither before nor after, stays chosen too.
    let e = Discipline::Electrical;
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"discipline": "electrical", "create": "A", "amendments": ["A"]}));
    run(&mut s, "building_code", json!({"discipline": "electrical", "create": "B", "amendments": ["A", "B"]}));
    run(
        &mut s,
        "building_code",
        json!({"discipline": "electrical", "amend": "A", "rules": {"voltages": {"48 V": {"nominal": 48, "min": 44, "max": 52}}}}),
    );
    run(&mut s, "building_code", json!({"discipline": "electrical", "delete": "A"}));
    assert_eq!(s.model.codes.choice(e).amendments, ["B"], "B stood and stands");
    // Two amendments imported while the project is held to `S1`: `F1` made over `generic`, `G1` over `nec`.
    let at = |n: &str| dir("held base").join(format!("{n}.xml")).to_string_lossy().to_string();
    let mut s = Session::new();
    for (code, base) in [("F1", "generic"), ("G1", "nec")] {
        run(&mut s, "building_code", json!({"discipline": "electrical", "set": base}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "create": code, "amendments": [code]}));
        run(&mut s, "building_code_export", json!({"discipline": "electrical", "code": code, "name": code, "path": at(code)}));
        run(&mut s, "building_code", json!({"discipline": "electrical", "amendments": [], "delete": code}));
    }
    run(&mut s, "building_code", json!({"discipline": "electrical", "set": "generic"}));
    run(&mut s, "building_code", json!({"discipline": "electrical", "save": "S1"}));
    for code in ["F1", "G1"] {
        run(&mut s, "building_code_import", json!({"path": at(code), "set": true}));
    }
    let chosen = s.model.codes.choice(e).amendments.clone();
    assert!(chosen.contains(&"F1".to_string()) && chosen.contains(&"G1".to_string()), "{chosen:?}");
    run(&mut s, "building_code", json!({"discipline": "electrical", "delete": "S1"}));
    let now = s.model.codes.choice(e).amendments.clone();
    assert!(!now.contains(&"F1".to_string()), "F1 would stand over generic");
    assert!(now.contains(&"G1".to_string()), "G1 stands on neither, and stays chosen");
}
