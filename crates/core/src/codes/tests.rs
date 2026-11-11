use serde_json::{Value, json};

use super::*;
use crate::engine::Session;
use crate::model::Model;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn stair_of(s: &Session, id: u64) -> crate::model::Stair {
    match s.model.elements.get(id) {
        Some(crate::model::Element::Stair(st)) => st.clone(),
        _ => panic!("{id} is no stair"),
    }
}

/// The rules of the findings `stair_check` has for stair `id`.
fn broken(s: &mut Session, id: u64) -> Vec<String> {
    let r = run(s, "stair_check", json!({"ids": [id]}));
    let list = r["stairs"][0]["findings"].as_array().cloned().unwrap_or_default();
    list.iter().filter_map(|f| f["rule"].as_str().map(String::from)).collect()
}

#[test]
fn every_code_reads_names_its_sources_and_keeps_its_own_rules() {
    let all = codes();
    let names: Vec<&str> = all.iter().map(|c| c.code.as_str()).collect();
    assert_eq!(names, ["generic", "nbr", "ibc", "din", "jis"]);
    for c in all {
        c.stairs.check().unwrap_or_else(|e| panic!("{}: {e}", c.code));
        assert!(!c.name.is_empty() && !c.note.is_empty(), "{}", c.code);
        assert!(c.code == GENERIC || !c.references.is_empty(), "{} names no source", c.code);
        let r = &c.stairs;
        // What a new stair starts with is inside the code it starts under.
        assert!((r.riser_min..=r.riser_max).contains(&r.riser), "{}", c.code);
        assert!((r.going_min..=r.going_max).contains(&r.going), "{}", c.code);
        assert!((r.step_min..=r.step_max).contains(&(2.0 * r.riser + r.going)), "{}", c.code);
    }
}

#[test]
fn a_project_has_no_code_until_one_is_chosen_and_then_its_rules() {
    let mut s = Session::new();
    let r = run(&mut s, "building_code", json!({}));
    assert_eq!(r["code"], "generic");
    assert_eq!(r["all"].as_array().map(Vec::len), Some(5));
    assert_eq!(r["stairs"]["riser"], 180.0, "a stair as it always was");
    assert_eq!(r["stairs"]["going"], 280.0);

    let r = run(&mut s, "building_code", json!({"set": "ibc"}));
    assert_eq!(r["code"], "ibc");
    assert_eq!(r["stairs"]["riser_max"], 177.8, "seven inches");
    assert_eq!(r["stairs"]["walk_line"], "narrow_side");
    assert_eq!(r["stairs"]["walk_line_offset"], 304.8);
    assert_eq!(r["changed"], json!([]));
    assert_eq!(s.history(), ["Building Codes"]);

    let e = s.execute("building_code", &json!({"set": "nec"})).unwrap_err();
    assert!(e.contains("generic, nbr, ibc, din, jis"), "{e}");
}

#[test]
fn a_project_changes_its_code_s_rules_and_the_answer_says_which() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "nbr"}));
    let r = run(&mut s, "building_code", json!({"stairs": {"width_min": 1100, "winders": true}}));
    assert_eq!(r["stairs"]["width_min"], 1100.0);
    assert_eq!(r["stairs"]["riser_max"], 180.0, "the rest stays the code's");
    assert_eq!(r["changed"], json!(["width_min", "winders"]));

    // A file keeps the code and what the project changed of it.
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.building_code().code, "nbr");
    assert_eq!(back.stair_rules().width_min, 1100.0);

    // Another code brings its own rules, unless the project's are kept.
    let r = run(&mut s, "building_code", json!({"set": "din", "keep_rules": true}));
    assert_eq!(r["stairs"]["width_min"], 1100.0);
    let r = run(&mut s, "building_code", json!({"set": "din"}));
    assert_eq!(r["stairs"]["width_min"], 1000.0);
    assert_eq!(r["changed"], json!([]));

    run(&mut s, "building_code", json!({"stairs": {"headroom": 2200}}));
    let r = run(&mut s, "building_code", json!({"reset": true}));
    assert_eq!(r["stairs"]["headroom"], 2000.0);
    assert_eq!(s.model.stair_rules, None);
}

#[test]
fn rules_that_cannot_be_are_refused() {
    let mut s = Session::new();
    for p in [
        json!({"stairs": {"riser_max": -1}}),
        json!({"stairs": {"riser_min": 300}}),
        json!({"stairs": {"going_max": 1e300}}),
        json!({"stairs": {"flight_risers_max": 0}}),
        json!({"stairs": {"flight_risers_max": 2.5}}),
        json!({"stairs": {"landing_rule": "long"}}),
        json!({"stairs": {"walk_line": "left"}}),
        json!({"stairs": {"colour": 1}}),
        json!({"stairs": {"winders": "yes"}}),
        json!({"stairs": []}),
        json!({"stairs": {"riser": null}}),
        json!({"set": 5}),
        json!({"reset": "yes"}),
        json!({"reset": true, "stairs": {"headroom": 2100}}),
        json!({"colour": 1}),
    ] {
        assert!(s.execute("building_code", &p).is_err(), "{p}");
    }
    assert_eq!(s.model.stair_rules, None);
    assert_eq!(s.model.building_code, None);
}

#[test]
fn a_new_stair_starts_with_its_code_s_riser_and_going() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "ibc"}));
    let id = run(&mut s, "stair", json!({"start": [0, 0], "angle": 0}))["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    assert_eq!((st.riser, st.tread), (177.8, 279.4));
    assert_eq!(broken(&mut s, id), ["width_min"], "a metre wide, where the IBC asks 44 inches");
}

#[test]
fn a_stair_outside_its_code_is_built_and_reported() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "nbr"}));
    // 3000 up in 15 risers of 200, goings of 240: too steep and too short, though its step, 640, is inside the rule.
    let id = run(&mut s, "stair", json!({"start": [0, 0], "angle": 0, "riser": 200, "tread": 240, "width": 1200}))["id"].as_u64().unwrap();
    assert_eq!(broken(&mut s, id), ["riser_max", "going_min"]);
    let r = run(&mut s, "stair_check", json!({}));
    assert_eq!(r["code"], "nbr");
    assert_eq!(r["count"], 2);
    let f = &r["stairs"][0]["findings"][0];
    assert_eq!((f["value"].as_f64(), f["limit"].as_f64()), (Some(200.0), Some(180.0)));
    assert!(f["message"].as_str().is_some_and(|t| t.contains("200") && t.contains("180")), "{f}");
    assert_eq!(r["stairs"][0]["step"], 640.0);

    // Changing the rule is the project's right, and then the stair is inside it.
    run(&mut s, "building_code", json!({"stairs": {"riser_max": 200, "going_min": 240}}));
    assert_eq!(broken(&mut s, id), Vec::<String>::new());

    // The generic rules ask little.
    run(&mut s, "building_code", json!({"set": "generic"}));
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
    assert!(s.execute("stair_check", &json!({"ids": [100000]})).unwrap_err().contains("stair"));
}

#[test]
fn a_flight_too_high_is_due_a_landing_which_auto_gives_it() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "nbr"}));
    let up = json!({"start": [0, 0], "angle": 0, "height": 5400, "width": 1200});
    let id = run(&mut s, "stair", up.clone())["id"].as_u64().unwrap();
    assert_eq!(broken(&mut s, id), ["flight_rise_max"], "5.4 m in one flight, where a landing is due every 3.2 m");

    let mut auto = up;
    auto["landings"] = json!("auto");
    let id = run(&mut s, "stair", auto.clone())["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    assert_eq!(st.landings.iter().map(|l| (l.at, l.turn)).collect::<Vec<_>>(), [(15.0, 0.0)], "30 risers of 180 in two flights");
    assert_eq!(broken(&mut s, id), Vec::<String>::new());

    // DIN counts risers: a landing after 18 at most.
    run(&mut s, "building_code", json!({"set": "din"}));
    let id = run(&mut s, "stair", auto)["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    assert!(st.landings.len() == 1 && st.landings[0].at <= 18.0, "{:?}", st.landings);
    // Its landing is as long as DIN asks, a going and a step, 270 + 630 or so, not as the stair is wide, 1200.
    assert!((st.landings[0].length - 900.0).abs() < 15.0, "{:?}", st.landings);
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
    run(&mut s, "modify", json!({"id": id, "width": 800}));
    assert_eq!(broken(&mut s, id), ["width_min"], "the landing keeps its own length");
    run(&mut s, "modify", json!({"id": id, "landings": [{"at": st.landings[0].at, "length": 600}]}));
    assert_eq!(broken(&mut s, id), ["width_min", "landing_length"]);
}

#[test]
fn a_stair_stands_by_its_location_line_which_stays_when_it_widens() {
    let mut s = Session::new();
    // Climbing north from the origin, the line its left edge: the stair lies east of it.
    let id = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1000, "line": "left"}))["id"].as_u64().unwrap();
    let left = |s: &Session| {
        let l = crate::stairs::stair(&s.model, &stair_of(s, id));
        l.runs[0].ring(l.width).iter().map(|p| p.0).fold(f64::INFINITY, f64::min)
    };
    assert!(left(&s).abs() < 1e-6);
    run(&mut s, "modify", json!({"id": id, "width": 1400}));
    assert!(left(&s).abs() < 1e-6, "its left edge stays on the line");
    run(&mut s, "modify", json!({"id": id, "offset": 200}));
    assert!((left(&s) + 200.0).abs() < 1e-6, "200 to the left of it");
    run(&mut s, "modify", json!({"id": id, "line": "center", "offset": 0}));
    assert!((left(&s) + 700.0).abs() < 1e-6);
    assert!(s.execute("stair", &json!({"start": [0, 0], "angle": 90, "line": "middle"})).is_err());
    assert!(s.execute("stair", &json!({"start": [0, 0], "angle": 90, "offset": 1e300})).is_err());
}

#[test]
fn a_file_with_rules_that_cannot_be_is_refused_and_an_unknown_code_is_generic() {
    let mut m = Model::new("x");
    m.building_code = Some("mars".into());
    assert_eq!(m.building_code().code, GENERIC);
    m.check().unwrap();
    let mut rules = code("ibc").unwrap().stairs.clone();
    rules.headroom = f64::NAN;
    m.stair_rules = Some(rules);
    assert!(m.check().unwrap_err().contains("headroom"));
}

#[test]
fn a_turn_winds_and_a_code_without_winders_says_so_and_still_builds_it() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "ibc"}));
    // 3000 up in 17 risers: 6 up, 3 winders round a turn to the left (2 more risers), 9 more.
    let p = json!({"start": [0, 0], "angle": 90, "width": 1200, "landings": [{"at": 6, "turn": 90, "winders": 3}]});
    let id = run(&mut s, "stair", p.clone())["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    let l = crate::stairs::stair(&s.model, &st);
    assert_eq!((l.runs.len(), l.pads.len(), l.winders), (2, 3, 3));
    assert_eq!((l.runs[0].risers, l.runs[1].risers), (6, 17 - 6 - 2));
    let each = 3000.0 / 17.0;
    for (k, pad) in l.pads.iter().enumerate() {
        assert!((pad.z - (6.0 + k as f64) * each).abs() < 1e-6, "{k}: {}", pad.z);
    }
    assert!((l.runs[1].z.0 - 8.0 * each).abs() < 1e-6 && (l.runs[1].z.1 - 3000.0).abs() < 1e-6);
    // The winders fill the landing's square, and all meet at the inside corner of the turn.
    let area: f64 = l.pads.iter().map(|p| crate::geom::area(&p.ring)).sum();
    assert!((area - 1200.0 * 1200.0).abs() < 1e-3, "{area}");
    let corner = crate::geom::P2(-600.0, 5.0 * 279.4);
    assert!(l.pads.iter().all(|p| p.ring.iter().any(|q| q.dist(corner) < 1e-6)), "{:?}", l.pads);
    // On the IBC's walk line, 12 inches from the narrow side: 304.8 × 90° / 3 = 159.6, under 11 inches; and a point.
    assert_eq!(broken(&mut s, id), ["winder_going_min", "winder_narrow_min"]);

    // NBR has no winders: built all the same, and said.
    run(&mut s, "building_code", json!({"set": "nbr"}));
    assert!(broken(&mut s, id).contains(&"winders".to_string()));

    // Back beside the run, six winders through 180.
    let mut back = p;
    back["landings"] = json!([{"at": 5, "turn": -180, "winders": 6}]);
    let id = run(&mut s, "stair", back)["id"].as_u64().unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    let area: f64 = l.pads.iter().map(|p| crate::geom::area(&p.ring)).sum();
    assert!((area - 2.0 * 1200.0 * 1200.0).abs() < 1e-3, "{area}");

    for bad in [
        json!([{"at": 6, "turn": 0, "winders": 3}]),
        json!([{"at": 6, "turn": 90, "winders": 31}]),
        json!([{"at": 14, "turn": 90, "winders": 4}]),
        json!([{"at": 6, "turn": 90, "winders": -1}]),
    ] {
        let p = json!({"start": [0, 0], "angle": 90, "landings": bad});
        assert!(s.execute("stair", &p).is_err(), "{p}");
    }
    assert!(s.execute("ramp", &json!({"start": [0, 0], "angle": 0, "landings": [{"at": 500, "turn": 90, "winders": 3}]})).is_err());
}
