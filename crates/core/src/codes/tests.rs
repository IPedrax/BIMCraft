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
        l.runs[0].ring().iter().map(|p| p.0).fold(f64::INFINITY, f64::min)
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

#[test]
fn a_project_saves_its_rules_as_a_code_of_its_own_and_hands_it_on_as_xml() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "nbr", "stairs": {"width_min": 1100, "winders": true}}));
    let r = run(&mut s, "building_code", json!({"save": "São Paulo IT 11"}));
    assert_eq!((r["code"].as_str(), r["custom"].as_bool(), r["based_on"].as_str()), (Some("São Paulo IT 11"), Some(true), Some("nbr")));
    assert_eq!(r["changed"], json!([]), "they are the code's own rules now");
    assert_eq!(r["stairs"]["width_min"], 1100.0);
    assert_eq!(r["all"].as_array().map(Vec::len), Some(6));
    assert_eq!(s.model.stair_rules, None);

    // Another code and back; a file keeps it.
    run(&mut s, "building_code", json!({"set": "din"}));
    let r = run(&mut s, "building_code", json!({"set": "São Paulo IT 11"}));
    assert_eq!(r["stairs"]["width_min"], 1100.0);
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.stair_rules().width_min, 1100.0);

    // Out as XML, and into another project.
    let dir = std::env::temp_dir().join(format!("bimcraft-codes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("it11.xml").to_string_lossy().to_string();
    run(&mut s, "building_code_export", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("<rule name=\"width_min\" value=\"1100.0\"/>") && text.contains("name=\"São Paulo IT 11\""), "{text}");
    let mut other = Session::new();
    let r = run(&mut other, "building_code_import", json!({"path": path, "set": true}));
    assert_eq!((r["code"].as_str(), r["left_out"].as_array().map(Vec::len)), (Some("São Paulo IT 11"), Some(0)));
    assert_eq!(other.model.stair_rules(), s.model.stair_rules());
    assert_eq!(other.model.building_code().based_on, "nbr");

    // One written by hand: what it leaves out is the generic code's, what it has that we don't know is said.
    let hand = dir.join("hand.xml").to_string_lossy().to_string();
    std::fs::write(&hand, "<building-code name='A &amp; B'><stairs><rule name='riser_max' value='170'/><rule name='colour' value='1'/></stairs><extra/></building-code>").unwrap();
    let r = run(&mut other, "building_code_import", json!({"path": hand}));
    assert_eq!(r["code"], "A & B");
    assert_eq!(r["left_out"], json!(["rule `colour`", "<extra>"]));
    let own = other.model.code("A & B").unwrap();
    assert_eq!((own.stairs.riser_max, own.stairs.going), (170.0, 280.0));
    assert_eq!(other.model.building_code().code, "São Paulo IT 11", "not set: the project stays as it was");

    // A code BIMCraft has, exported as it is, needs a name of the project's own to come back.
    run(&mut other, "building_code_export", json!({"path": hand, "code": "ibc"}));
    assert!(other.execute("building_code_import", &json!({"path": hand})).unwrap_err().contains("name"));
    run(&mut other, "building_code_import", json!({"path": hand, "name": "IBC, ours"}));
    assert_eq!(other.model.code("IBC, ours").unwrap().stairs.riser_max, 177.8);

    // Taken away while the project is held to it: the rules stay, as its own, under the code it was based on.
    let r = run(&mut s, "building_code", json!({"delete": "São Paulo IT 11"}));
    assert_eq!((r["code"].as_str(), r["stairs"]["width_min"].as_f64()), (Some("nbr"), Some(1100.0)));
    assert_eq!(r["changed"], json!(["width_min", "winders"]));
    for p in [json!({"save": "nbr"}), json!({"save": ""}), json!({"save": "x".repeat(65)}), json!({"delete": "nbr"}), json!({"save": 5})] {
        assert!(s.execute("building_code", &p).is_err(), "{p}");
    }
    for bad in
        ["", "<building-code", "<stairs/>", "<building-code name='x'><stairs><rule name='riser_min' value='900'/></stairs></building-code>"]
    {
        std::fs::write(&hand, bad).unwrap();
        assert!(s.execute("building_code_import", &json!({"path": hand})).is_err(), "{bad}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_stair_flipped_climbs_the_other_way_over_the_same_ground() {
    let mut s = Session::new();
    let p = json!({"start": [0, 0], "angle": 90, "width": 1000, "line": "left", "offset": 100,
        "landings": [{"at": 5, "turn": 90, "winders": 3}, {"at": 11, "turn": -180, "length": 1400}]});
    let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    let ground = |s: &Session| {
        let l = crate::stairs::stair(&s.model, &stair_of(s, id));
        let mut pts: Vec<(i64, i64)> = l
            .runs
            .iter()
            .flat_map(|r| r.ring())
            .chain(l.pads.iter().flat_map(|p| p.ring.clone()))
            .map(|p| ((p.0 * 10.0).round() as i64, (p.1 * 10.0).round() as i64))
            .collect();
        pts.sort_unstable();
        pts.dedup();
        (pts, l.runs.iter().map(|r| r.risers).collect::<Vec<_>>())
    };
    let (before, risers) = ground(&s);
    run(&mut s, "flip", json!({"ids": [id]}));
    let (after, back) = ground(&s);
    assert_eq!(after, before, "the same runs, landings and winders");
    assert_eq!(back, risers.iter().rev().copied().collect::<Vec<_>>());
    let st = stair_of(&s, id);
    assert_eq!(st.landings.iter().map(|l| (l.at, l.turn, l.winders)).collect::<Vec<_>>(), [(6.0, 180.0, 0), (10.0, -90.0, 3)]);
    assert_eq!(st.line, crate::model::StairLine::Right);
    run(&mut s, "flip", json!({"ids": [id]}));
    assert_eq!(ground(&s).0, before);
}

#[test]
fn a_stair_s_grips_move_it_turn_it_widen_it_and_shift_its_landings() {
    use crate::geom::P2;
    use crate::model::Side;
    use crate::stairs::{Grip, dragged, grips};
    let mut s = Session::new();
    let p = json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 6, "turn": 90}, {"at": 12, "turn": 0}]});
    let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    let m = &s.model;
    let at = |g: Grip| grips(m, &st).into_iter().find(|(k, _)| *k == g).map(|(_, p)| p).unwrap();
    let near = |a: P2, b: P2| a.dist(b) < 1e-6;
    assert!(near(at(Grip::Foot), P2(0.0, 0.0)));
    assert!(near(at(Grip::Turn(0)), P2(0.0, 5.0 * 280.0)));
    assert!(near(at(Grip::Landing(0)), P2(0.0, 5.0 * 280.0 + 1000.0)));
    assert!((at(Grip::Width(Side::Right)).0 - 500.0).abs() < 1e-6);

    assert_eq!(dragged(m, &st, Grip::Foot, P2(250.0, -40.0)).start, P2(250.0, -40.0));
    // The first run two treads longer: its landing two risers on, and the next landing with it.
    let longer = dragged(m, &st, Grip::Turn(0), P2(30.0, 7.0 * 280.0 + 20.0));
    assert_eq!(longer.landings.iter().map(|l| l.at).collect::<Vec<_>>(), [8.0, 14.0]);
    assert_eq!(dragged(m, &st, Grip::Turn(0), P2(0.0, -5000.0)).landings[0].at, 2.0, "a run keeps two risers");
    assert_eq!(dragged(m, &st, Grip::Width(Side::Right), P2(700.0, 300.0)).width, 1400.0);
    assert_eq!(dragged(m, &st, Grip::Landing(1), at(Grip::Landing(1)) + P2(-400.0, 0.0)).landings[1].length, 1400.0, "it runs west there");
    // The top dragged a quarter turn round the foot.
    let top = at(Grip::Top);
    let turned = dragged(m, &st, Grip::Top, P2(top.1, -top.0));
    assert!((turned.angle - 0.0).abs() < 1e-6, "{}", turned.angle);
    // From a side that is its location line, the width is from that line.
    let mut left = st.clone();
    left.line = crate::model::StairLine::Left;
    assert_eq!(grips(m, &left).iter().filter(|(g, _)| matches!(g, Grip::Width(_))).count(), 1);
    assert_eq!(dragged(m, &left, Grip::Width(Side::Right), P2(1300.0, 0.0)).width, 1300.0);
}

#[test]
fn winders_balanced_into_the_flights_have_depth_at_the_narrow_end() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "din"}));
    // 3000 up in 17 risers of 176.5, goings of 270: 6 up, 3 winders round a turn, and 3 treads of each flight with them.
    let p = json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 7, "turn": 90, "winders": 3, "balance": 3}]});
    let id = run(&mut s, "stair", p.clone())["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    let l = crate::stairs::stair(&s.model, &st);
    assert_eq!((l.runs[0].risers, l.runs[1].risers, l.winders), (4, 17 - 7 - 2 - 3, 9));
    // The nine steps cover the corner and three treads each side, with no overlap.
    let area: f64 = l.pads.iter().map(|p| crate::geom::area(&p.ring)).sum();
    assert!((area - (1000.0 * 1000.0 + 2.0 * 3.0 * 270.0 * 1000.0)).abs() < 1.0, "{area}");
    let each = 3000.0 / 17.0;
    for (k, pad) in l.pads.iter().enumerate() {
        assert!((pad.z - (4.0 + k as f64) * each).abs() < 1e-6, "{k}: {}", pad.z);
        assert!(crate::geom::area(&pad.ring) > 1e4, "{k}: {:?}", pad.ring);
    }
    assert!((l.runs[1].z.0 - 12.0 * each).abs() < 1e-6 && (l.runs[1].z.1 - 3000.0).abs() < 1e-6);
    // Every step is 2 × 3 × 270 / 9 = 180 deep on the inside string, and the middle one of the nine goes round the corner.
    let corner = crate::geom::P2(-500.0, 3.0 * 270.0 + 3.0 * 270.0);
    assert_eq!(
        l.pads.iter().filter(|p| p.ring.iter().any(|q| q.dist(corner) < 1e-6)).count(),
        1,
        "one step holds the corner, none comes to a point there"
    );
    // DIN asks 100 at the narrow end: 180 holds it, where a point did not.
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
    let mut point = p.clone();
    point["landings"][0]["balance"] = json!(0);
    let other = run(&mut s, "stair", point)["id"].as_u64().unwrap();
    assert!(broken(&mut s, other).contains(&"winder_narrow_min".to_string()));

    // Back beside the run, and flipped over the same ground.
    let mut back = p;
    back["landings"] = json!([{"at": 8, "turn": -180, "winders": 6, "balance": 2}]);
    let id = run(&mut s, "stair", back)["id"].as_u64().unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    let area: f64 = l.pads.iter().map(|p| crate::geom::area(&p.ring)).sum();
    assert!((area - (2.0 * 1000.0 * 1000.0 + 2.0 * 2.0 * 270.0 * 1000.0)).abs() < 1.0, "{area}");
    assert_eq!(l.winders, 10);

    for bad in [
        json!([{"at": 7, "turn": 90, "balance": 2}]),
        json!([{"at": 7, "turn": 90, "winders": 3, "balance": 31}]),
        json!([{"at": 3, "turn": 90, "winders": 3, "balance": 3}]),
        json!([{"at": 12, "turn": 90, "winders": 3, "balance": 3}]),
    ] {
        let p = json!({"start": [0, 0], "angle": 90, "landings": bad});
        assert!(s.execute("stair", &p).is_err(), "{p}");
    }
}

#[test]
fn the_walk_line_goes_round_a_winding_turn_at_the_stair_s_middle() {
    let mut s = Session::new();
    let p = json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 7, "turn": 90, "winders": 3, "balance": 2}]});
    let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    // The inside corner of the turn: every point of the arc is half the width from it, and the line only climbs.
    let corner = crate::geom::P2(-500.0, 4.0 * 280.0 + 2.0 * 280.0);
    let round: Vec<_> = l.walk.iter().filter(|(q, _)| (q.dist(corner) - 500.0).abs() < 1e-6).collect();
    assert!(round.len() >= 7, "{:?}", l.walk);
    assert!(l.walk.windows(2).all(|w| w[1].1 >= w[0].1 - 1e-9), "{:?}", l.walk);
    let (first, last) = (l.walk.first().unwrap(), l.walk.last().unwrap());
    assert_eq!((first.1, (last.1 - 3000.0).abs() < 1e-6), (0.0, true));
    // A railing down its side goes round with it.
    let r = run(&mut s, "railing", json!({"host": id, "side": "left"}));
    assert_eq!(r["ids"].as_array().map(Vec::len), Some(1), "{r}");
}

#[test]
fn a_stair_turns_by_any_angle_on_a_landing_or_on_winders() {
    use crate::geom::P2;
    let mut s = Session::new();
    // North, then 45 to the left on a landing.
    let id = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 6, "turn": 45}]}))["id"]
        .as_u64()
        .unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    let (a, b) = (l.runs[0], l.runs[1]);
    assert!((b.u.0 + 0.5f64.sqrt()).abs() < 1e-9 && (b.u.1 - 0.5f64.sqrt()).abs() < 1e-9, "north-west: {:?}", b.u);
    // The runs share the inside corner of the turn, the left end of the first run's top.
    let corner = a.end() + P2(-500.0, 0.0);
    assert!(b.ring().iter().any(|q| q.dist(corner) < 1e-6), "{:?}", b.ring());
    // The landing: a kite of w × w·tan(22.5°).
    let kite = 1000.0 * 1000.0 * (22.5f64).to_radians().tan();
    assert!((crate::geom::area(&l.pads[0].ring) - kite).abs() < 1e-3, "{}", crate::geom::area(&l.pads[0].ring));

    // The same turn on winders, plain and balanced: the same ground and the flights' treads, no gaps.
    for (winders, balance, ground) in [(2, 0, kite), (2, 2, kite + 2.0 * 2.0 * 280.0 * 1000.0)] {
        let p = json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 6, "turn": -45, "winders": winders, "balance": balance}]});
        let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
        let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
        let area: f64 = l.pads.iter().map(|p| crate::geom::area(&p.ring)).sum();
        assert!((area - ground).abs() < 1.0, "{winders} {balance}: {area} against {ground}");
        assert_eq!(l.winders as u32, winders + 2 * balance);
        assert!((l.walk.last().unwrap().1 - 3000.0).abs() < 1e-6);
    }
    // And flipped, over the same ground.
    let before: f64 = crate::stairs::stair(&s.model, &stair_of(&s, id)).pads.iter().map(|p| crate::geom::area(&p.ring)).sum();
    run(&mut s, "flip", json!({"ids": [id]}));
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    assert!((l.pads.iter().map(|p| crate::geom::area(&p.ring)).sum::<f64>() - before).abs() < 1.0);

    for turn in [3, 140, 179, -200] {
        assert!(s.execute("stair", &json!({"start": [0, 0], "angle": 90, "landings": [{"at": 6, "turn": turn}]})).is_err(), "{turn}");
    }
}

#[test]
fn a_spiral_stair_winds_about_its_centre() {
    use crate::geom::P2;
    let mut s = Session::new();
    // Starting east of its centre and climbing north, counter-clockwise: 3000 up in 17 risers, a quarter turn in all.
    let p = json!({"start": [1500, 0], "angle": 90, "width": 1000, "spiral": {"radius": 1500, "sweep": 90}});
    let id = run(&mut s, "stair", p.clone())["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    let l = crate::stairs::stair(&s.model, &st);
    assert_eq!((l.runs.len(), l.pads.len(), l.winders), (0, 16, 16));
    // A quarter of the ring between 1000 and 2000 from the centre.
    let area: f64 = l.pads.iter().map(|p| crate::geom::area(&p.ring)).sum();
    let ring = std::f64::consts::PI * (2000.0f64.powi(2) - 1000.0f64.powi(2)) / 4.0;
    assert!((area - ring).abs() / ring < 0.002, "{area} against {ring}");
    assert!(l.pads.iter().flat_map(|p| &p.ring).all(|q| (999.0..=2001.0).contains(&q.dist(P2(0.0, 0.0)))));
    let (top, z) = *l.walk.last().unwrap();
    assert!(top.dist(P2(0.0, 1500.0)) < 1e-6 && (z - 3000.0).abs() < 1e-6, "{top:?} {z}");
    // The generic rules: 1500 × 90° / 16 = 147 on the middle line, under any going, and 98 at the narrow end.
    assert_eq!(broken(&mut s, id), ["winder_going_min", "winder_narrow_min"]);

    // By its tread on the middle line, it turns as far as that makes it; clockwise with the centre to its right.
    let id = run(&mut s, "stair", json!({"start": [0, 0], "angle": 90, "width": 900, "tread": 280, "spiral": {"radius": -1100}}))["id"]
        .as_u64()
        .unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    let centre = P2(1100.0, 0.0);
    assert!(l.walk.iter().all(|(q, _)| (q.dist(centre) - 1100.0).abs() < 1e-6));
    assert!(l.walk[1].0.1 > 0.0, "north first");
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
    // Flipped, over the same ground.
    let ground =
        |s: &Session| crate::stairs::stair(&s.model, &stair_of(s, id)).pads.iter().map(|p| crate::geom::area(&p.ring)).sum::<f64>();
    let (before, top) = (ground(&s), l.walk.last().unwrap().0);
    run(&mut s, "flip", json!({"ids": [id]}));
    assert!((ground(&s) - before).abs() < 1.0);
    assert!(crate::stairs::stair(&s.model, &stair_of(&s, id)).walk[0].0.dist(top) < 0.01);
    // NBR has no winders at all.
    run(&mut s, "building_code", json!({"set": "nbr"}));
    assert!(broken(&mut s, id).contains(&"winders".to_string()));

    for bad in [json!({"radius": 100}), json!({"radius": 1500, "sweep": 2}), json!({"radius": 1e300}), json!({"sweep": 90}), json!(5)] {
        assert!(s.execute("stair", &json!({"start": [0, 0], "angle": 90, "spiral": bad})).is_err(), "{bad}");
    }
    let mut landed = p;
    landed["landings"] = json!([{"at": 8, "turn": 0}]);
    assert!(s.execute("stair", &landed).unwrap_err().contains("landings"));
}

#[test]
fn a_stair_is_built_as_its_type_says() {
    let mut s = Session::new();
    let names = s.model.type_names("stair").join(", ");
    assert!(names.contains("Monolithic 150mm") && names.contains("Cantilevered Treads"), "{names}");
    let p = json!({"start": [0, 0], "angle": 0, "width": 1000, "landings": [{"at": 8, "turn": 90}]});
    let solid = run(&mut s, "stair", p.clone())["id"].as_u64().unwrap();
    assert!(crate::stairs::parts(&s.model, &stair_of(&s, solid)).is_none(), "of no type: solid to its level");

    let of = |s: &mut Session, name: &str| {
        let mut p = p.clone();
        p["type"] = json!(name);
        let id = run(s, "stair", p)["id"].as_u64().unwrap();
        crate::stairs::parts(&s.model, &stair_of(s, id)).unwrap()
    };
    // Monolithic: nothing of it under the floor, and under the landing nothing lower than the slab.
    let mono = of(&mut s, "Monolithic 150mm");
    assert!(mono.iter().all(|(ring, bottom, top)| ring.iter().all(|q| bottom.at(*q) >= -1e-6 && top.at(*q) > bottom.at(*q) - 1e-6)));
    let each = 3000.0 / 17.0;
    let landing = mono.iter().find(|(_, _, top)| (top.z - 8.0 * each).abs() < 1e-6 && top.g == crate::geom::P2(0.0, 0.0)).unwrap();
    assert!((landing.1.z - (8.0 * each - 150.0)).abs() < 1e-6, "the landing is the slab's thickness");
    // Halfway up the first run, the underside is the slab's thickness below the steps' inner corners, square to the slope.
    let at = crate::geom::P2(4.0 * 280.0 + 1.0, 0.0);
    let under = mono.iter().filter(|(ring, _, _)| crate::geom::contains(ring, at)).map(|(_, b, _)| b.at(at)).fold(f64::INFINITY, f64::min);
    let slope = each / 280.0;
    let want = at.0 * slope - 150.0 * (1.0 + slope * slope).sqrt();
    assert!((under - want).abs() < 1e-6, "{under} against {want}");

    // Open treads: each 50 thick, 7 in the first run and 8 in the second, a landing, and no risers; with stringers, two a run.
    let free = of(&mut s, "Cantilevered Treads");
    assert_eq!(free.len(), 7 + 8 + 1);
    assert!(free.iter().all(|(_, b, t)| (t.z - b.z - 80.0).abs() < 1e-6));
    let strung = of(&mut s, "Stringers - Open Risers");
    assert!(strung.len() >= 7 + 8 + 1 + 4, "{}", strung.len());
    let closed = of(&mut s, "Stringers - Closed Risers");
    assert_eq!(closed.len(), strung.len() + 8 + 9, "a riser for each riser");

    // A type of the project's own, and one that cannot be.
    run(&mut s, "type", json!({"category": "stair", "name": "Slab 200", "waist": 200}));
    assert_eq!(s.model.types.stair["Slab 200"].waist, 200.0);
    run(&mut s, "modify", json!({"id": solid, "type": "Slab 200"}));
    assert!(crate::stairs::parts(&s.model, &stair_of(&s, solid)).is_some());
    assert!(s.execute("type", &json!({"category": "stair", "name": "Nothing"})).is_err());
    assert!(s.execute("type", &json!({"category": "stair", "name": "Deep", "waist": 1e9})).is_err());
    assert!(s.execute("modify", &json!({"id": solid, "type": "No such type"})).unwrap_err().contains("Monolithic"));
    // A file keeps the type, by its GUID.
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.types.stair["Slab 200"].waist, 200.0);
}

#[test]
fn the_solver_proposes_the_nearest_stairs_that_fit_the_code() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "nbr"}));
    // 3000 up in 15 risers of 200 with treads of 240, a metre wide: too steep, too short a tread, too narrow.
    let id = run(&mut s, "stair", json!({"start": [0, 0], "angle": 0, "riser": 200, "tread": 240}))["id"].as_u64().unwrap();
    assert_eq!(broken(&mut s, id), ["riser_max", "going_min", "width_min"]);
    let r = run(&mut s, "stair_solve", json!({"id": id}));
    assert_eq!(r["inside"], false);
    let all = r["proposals"].as_array().unwrap();
    assert!(!all.is_empty() && all.iter().all(|p| p["findings"] == json!([])), "{r}");
    // The nearest: 17 risers of 176.5 (the fewest under 180), the least tread the step rule allows, the least width.
    let first = &all[0];
    assert_eq!((first["risers"].as_u64(), first["width"].as_f64()), (Some(17), Some(1200.0)));
    let (riser, tread) = (first["riser"].as_f64().unwrap(), first["tread"].as_f64().unwrap());
    assert!((630.0..=650.0).contains(&(2.0 * riser + tread)) && (280.0..=320.0).contains(&tread), "{first}");
    assert!(first["changes"].as_array().unwrap().iter().any(|c| c.as_str().unwrap().contains("17 risers")), "{first}");

    // Locked, the width stays, and the answers say what they still break.
    let r = run(&mut s, "stair_solve", json!({"id": id, "lock": ["width"]}));
    let locked = r["proposals"].as_array().unwrap();
    let narrow = |p: &Value| p["findings"].as_array().unwrap().iter().any(|f| f["rule"] == "width_min");
    assert!(locked.iter().all(|p| p["width"] == 1000.0 && narrow(p)), "{r}");
    assert_eq!(locked[0]["findings"].as_array().map(Vec::len), Some(1), "the nearest breaks the width alone");

    // Applied, the stair is inside the rules, in one step; and then there is nothing to propose.
    let steps = s.history().len();
    run(&mut s, "stair_solve", json!({"id": id, "apply": 1}));
    assert_eq!(s.history().len(), steps + 1);
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
    let r = run(&mut s, "stair_solve", json!({"id": id}));
    assert_eq!((r["inside"].as_bool(), r["proposals"].as_array().map(Vec::len)), (Some(true), Some(0)));
    assert!(s.execute("stair_solve", &json!({"id": id, "apply": 1})).is_err());

    // A flight too high gets its landing, and a short landing its length.
    let tall = run(&mut s, "stair", json!({"start": [0, 5000], "angle": 0, "height": 5400, "width": 1200}))["id"].as_u64().unwrap();
    assert_eq!(broken(&mut s, tall), ["flight_rise_max"]);
    let r = run(&mut s, "stair_solve", json!({"id": tall}));
    assert!(r["proposals"][0]["changes"][0].as_str().unwrap().contains("landing"), "{r}");
    run(&mut s, "stair_solve", json!({"id": tall, "apply": 1}));
    assert_eq!(broken(&mut s, tall), Vec::<String>::new());
    assert_eq!(stair_of(&s, tall).landings.len(), 1);
    assert!(s.execute("stair_solve", &json!({"id": tall, "lock": ["colour"]})).is_err());
}

#[test]
fn a_multistorey_stair_stands_on_each_level_with_the_risers_its_storey_asks() {
    let mut s = Session::new();
    // Level 2 is 3000 up; a third 3600 over it, and a fourth 3000 over that.
    let l3 = run(&mut s, "level", json!({"name": "Level 3", "elevation": 6600}))["id"].as_u64().unwrap();
    let l4 = run(&mut s, "level", json!({"name": "Level 4", "elevation": 9600}))["id"].as_u64().unwrap();
    let p =
        json!({"start": [0, 0], "angle": 90, "width": 1200, "top_level": l4, "multistorey": true, "landings": [{"at": 8, "turn": 180}]});
    let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    // It is the stair of its first storey: 17 risers of 176.5.
    let (n, each, _) = s.model.flight(&st);
    assert_eq!(n, 17);
    assert!((each - 3000.0 / 17.0).abs() < 1e-9);
    let all = crate::stairs::storeys(&s.model, &st);
    assert_eq!(all.len(), 3);
    assert_eq!(all[2].0, s.model.guid(l3), "by the level each stands on");
    // The middle storey is 3600: 20 risers of 180, its landing after the same share of them (8 of 17: 9 of 20).
    let counts: Vec<Vec<u32>> = all.iter().map(|(_, l)| l.runs.iter().map(|r| r.risers).collect()).collect();
    assert_eq!(counts, [vec![8, 9], vec![9, 11], vec![8, 9]]);
    let tops: Vec<f64> = all.iter().map(|(_, l)| l.walk.last().unwrap().1).collect();
    assert!(tops.iter().zip([3000.0, 6600.0, 9600.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{tops:?}");
    // Every storey starts where the first does.
    assert!(all.iter().all(|(_, l)| l.runs[0].start == all[0].1.runs[0].start));
    let whole = crate::stairs::stair(&s.model, &st);
    assert_eq!((whole.runs.len(), whole.pads.len()), (6, 3));

    // Its storeys' risers are 176.5 and 180: 3.5 apart, inside the generic rules' 10 and outside NBR's 5? No: inside both.
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
    // A storey of 2800 has 16 risers of 175 under a riser of 180; of 2500, 14 of 178.6. Tighten the rule to see it said.
    run(&mut s, "building_code", json!({"stairs": {"uniformity": 2}}));
    let found = run(&mut s, "stair_check", json!({"ids": [id]}));
    let f = &found["stairs"][0]["findings"][0];
    assert_eq!(f["rule"], "uniformity");
    assert!((f["value"].as_f64().unwrap() - 3.53).abs() < 0.01, "{f}");

    // The plan of the middle level shows its own storey, going up.
    let png = std::env::temp_dir().join(format!("bimcraft-storeys-{}.png", std::process::id())).to_string_lossy().to_string();
    let r = run(&mut s, "render", json!({"view": "Level 3", "path": png}));
    assert_eq!(r["kind"], "plan");
    std::fs::remove_file(&png).ok();
    // Each storey is built in the 3D scene.
    run(&mut s, "modify", json!({"id": id, "type": "Monolithic 150mm"}));
    let parts = crate::stairs::parts(&s.model, &stair_of(&s, id)).unwrap();
    let high = parts.iter().map(|(ring, _, top)| ring.iter().map(|q| top.at(*q)).fold(0.0, f64::max)).fold(0.0, f64::max);
    assert!((high - (9600.0 - 3000.0 / 17.0)).abs() < 1e-6, "the top tread, a riser under the top floor: {high}");

    // Not multistorey, the same stair is one flight to the top; and a multistorey stair needs a top level.
    run(&mut s, "modify", json!({"id": id, "multistorey": false}));
    assert_eq!(crate::stairs::storeys(&s.model, &stair_of(&s, id)).len(), 1);
    assert!(s.execute("stair", &json!({"start": [0, 0], "angle": 0, "height": 6000, "multistorey": true})).is_err());
    let _ = l4;
}

#[test]
fn a_floor_over_a_stair_is_reported_and_cut_open_where_the_headroom_is_short() {
    let mut s = Session::new();
    // A floor 300 thick on Level 2, over all of a stair that climbs to it: 3000 up in 17 risers.
    run(&mut s, "type", json!({"category": "floor", "name": "Slab 300", "thickness": 300}));
    let floor = run(
        &mut s,
        "floor",
        json!({"level": "Level 2", "type": "Slab 300", "boundary": [[-3000, -3000], [9000, -3000], [9000, 3000], [-3000, 3000]]}),
    )["id"]
        .as_u64()
        .unwrap();
    let id = run(&mut s, "stair", json!({"start": [0, 0], "angle": 0, "width": 1000}))["id"].as_u64().unwrap();
    // The underside is 2700 up: a step 880 up or more has under 2000 over it (the generic rule's headroom).
    let found = run(&mut s, "stair_check", json!({"ids": [id]}));
    let f = &found["stairs"][0]["findings"][0];
    assert_eq!(f["rule"], "headroom");
    assert_eq!(f["value"], 0.0, "its top tread, 2824 up, is inside the floor's thickness");
    assert!(f["message"].as_str().unwrap().contains(&floor.to_string()), "{f}");

    // Cut open: one shaft through that floor alone, round the steps short of headroom: from the 4th tread to the top.
    let r = run(&mut s, "stair_opening", json!({"id": id}));
    assert_eq!((r["floors"].clone(), r["shafts"].as_array().map(Vec::len)), (json!([floor]), Some(1)));
    let shaft = r["shafts"][0].as_u64().unwrap();
    let Some(crate::model::Element::Shaft(sh)) = s.model.elements.get(shaft) else { panic!("no shaft") };
    assert_eq!(sh.boundary.len(), 4, "a rectangle: {:?}", sh.boundary);
    let xs: Vec<f64> = sh.boundary.iter().map(|q| q.0).collect();
    let (lo, hi) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(0.0, f64::max));
    // Steps 700 up or less keep 2000: the 4th tread is 705.9 up, and starts 840 along.
    assert!((lo - 840.0).abs() < 1e-6 && (hi - 16.0 * 280.0).abs() < 1e-6, "{lo} to {hi}");
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
    let again = run(&mut s, "stair_opening", json!({"id": id}));
    assert_eq!(again["shafts"], json!([]), "it has its headroom");

    // An L of treads is cut round as an L, not as its hull.
    let l = run(&mut s, "stair", json!({"start": [0, -2000], "angle": 0, "width": 1000, "landings": [{"at": 12, "turn": -90}]}))["id"]
        .as_u64()
        .unwrap();
    let r = run(&mut s, "stair_opening", json!({"id": l}));
    let shaft = r["shafts"][0].as_u64().unwrap();
    let Some(crate::model::Element::Shaft(sh)) = s.model.elements.get(shaft) else { panic!("no shaft") };
    assert_eq!(sh.boundary.len(), 6, "{:?}", sh.boundary);
    assert_eq!(broken(&mut s, l), Vec::<String>::new());
    assert!(s.execute("stair_opening", &json!({"id": floor})).is_err());
}

#[test]
fn a_stair_is_drawn_by_hand_riser_by_riser_and_a_made_one_becomes_one() {
    use crate::geom::P2;
    use crate::stairs::{Grip, dragged, grips};
    let mut s = Session::new();
    // Six risers fanning out: 900 wide at the foot, 1500 at the top, 300 apart on the middle line.
    let steps: Vec<Value> = (0..6)
        .map(|i| {
            let (y, half) = (300.0 * f64::from(i), 450.0 + 60.0 * f64::from(i));
            json!({"riser": [[-half, y], [half, y]]})
        })
        .collect();
    let id = run(&mut s, "stair", json!({"sketch": steps, "height": 1050}))["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    assert_eq!((st.start, st.angle), (P2(0.0, 0.0), 90.0));
    let (n, each, length) = s.model.flight(&st);
    assert_eq!((n, each, length), (6, 175.0, 1500.0));
    let l = crate::stairs::stair(&s.model, &st);
    assert_eq!((l.runs.len(), l.pads.len()), (0, 5));
    assert!((l.pads[0].z - 175.0).abs() < 1e-9 && (l.pads[4].z - 875.0).abs() < 1e-9);
    assert!((crate::geom::area(&l.pads[0].ring) - 300.0 * (900.0 + 1020.0) / 2.0).abs() < 1e-6);
    assert_eq!(l.walk.last().unwrap().1, 1050.0);
    // Measured as drawn: 300 on the middle line and 900 at its narrowest, under NBR's 1200.
    run(&mut s, "building_code", json!({"set": "nbr"}));
    assert_eq!(broken(&mut s, id), ["width_min"]);
    run(&mut s, "building_code", json!({"set": "generic"}));
    assert_eq!(broken(&mut s, id), Vec::<String>::new());

    // An end of a riser dragged, the treads on both sides of it follow; its foot dragged, all of it.
    let m = &s.model;
    assert_eq!(grips(m, &st).len(), 1 + 12);
    let wide = dragged(m, &st, Grip::Riser(2, true), P2(900.0, 600.0));
    assert_eq!(wide.sketch[2].riser[1], P2(900.0, 600.0));
    let moved = dragged(m, &st, Grip::Foot, P2(1000.0, 500.0));
    assert_eq!((moved.start, moved.sketch[5].riser[0]), (P2(1000.0, 500.0), P2(1000.0 - 750.0, 2000.0)));
    // Moved and flipped as any stair is.
    run(&mut s, "move", json!({"ids": [id], "by": [100, 0]}));
    assert_eq!(stair_of(&s, id).sketch[0].riser[0], P2(-350.0, 0.0));
    run(&mut s, "flip", json!({"ids": [id]}));
    let back = stair_of(&s, id);
    assert_eq!((back.sketch[0].riser, back.angle), ([P2(850.0, 1500.0), P2(-650.0, 1500.0)], -90.0));

    // A stair made by runs, a landing and winders becomes one drawn by hand, and stands as it did.
    let p = json!({"start": [5000, 0], "angle": 90, "width": 1000, "landings": [{"at": 5, "turn": 90, "winders": 3, "balance": 1}, {"at": 13, "turn": -90}]});
    let made = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    let ground = |s: &Session| {
        let l = crate::stairs::stair(&s.model, &stair_of(s, made));
        let mut all: Vec<(i64, i64)> = l
            .runs
            .iter()
            .flat_map(|r| {
                let n = r.u.perp() * 500.0;
                (0..r.risers).flat_map(move |k| {
                    let at = r.start + r.u * (f64::from(k) * 280.0);
                    [at + n, at - n]
                })
            })
            .chain(l.pads.iter().flat_map(|p| p.ring.clone()))
            .map(|q| ((q.0 * 10.0).round() as i64, (q.1 * 10.0).round() as i64))
            .collect();
        all.sort_unstable();
        all.dedup();
        (all, l.walk.last().unwrap().1)
    };
    let before = ground(&s);
    let r = run(&mut s, "stair_sketch", json!({"id": made}));
    assert_eq!(r["risers"], 17);
    let after = stair_of(&s, made);
    assert!(after.landings.is_empty() && after.sketch.len() == 17);
    assert_eq!(ground(&s), before, "every riser and every corner where it was");
    assert!(s.execute("stair_sketch", &json!({"id": made})).unwrap_err().contains("already"));

    for bad in [
        json!([{"riser": [[0, 0], [1000, 0]]}]),
        json!([{"riser": [[0, 0], [0, 0]]}, {"riser": [[0, 300], [1000, 300]]}]),
        json!([{"riser": [[0, 0], [1000, 0]], "outline": [[0, 0], [1, 1]]}, {"riser": [[0, 300], [1000, 300]]}]),
        json!([{"riser": [[0, 0], [1e300, 0]]}, {"riser": [[0, 300], [1000, 300]]}]),
        json!("x"),
    ] {
        assert!(s.execute("stair", &json!({"sketch": bad})).is_err(), "{bad}");
    }
    assert!(s.execute("stair", &json!({"sketch": steps_of(2), "landings": [{"at": 1, "turn": 0}]})).is_err());
}

fn steps_of(n: u32) -> Value {
    json!((0..n).map(|i| json!({"riser": [[0, 300 * i], [1000, 300 * i]]})).collect::<Vec<_>>())
}

#[test]
fn the_solver_balances_winders_that_come_to_a_point() {
    let mut s = Session::new();
    run(&mut s, "building_code", json!({"set": "din"}));
    let p = json!({"start": [0, 0], "angle": 90, "width": 1000, "landings": [{"at": 7, "turn": 90, "winders": 3}]});
    let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    assert!(broken(&mut s, id).contains(&"winder_narrow_min".to_string()));
    let r = run(&mut s, "stair_solve", json!({"id": id}));
    let first = &r["proposals"][0];
    assert_eq!(first["findings"], json!([]), "{r}");
    // DIN asks 100 at the narrow end: one tread of each flight gives 2 × 270 / 5 = 108.
    assert_eq!(first["landings"][0]["balance"], 1);
    assert!(first["changes"].as_array().unwrap().iter().any(|c| c.as_str().unwrap().contains("balanced over 1 tread")), "{first}");
    run(&mut s, "stair_solve", json!({"id": id, "apply": 1}));
    assert_eq!(broken(&mut s, id), Vec::<String>::new());
}

#[test]
fn a_run_after_a_landing_takes_a_width_of_its_own() {
    use crate::geom::P2;
    let mut s = Session::new();
    // North 1200 wide; then left, 900 wide; then back beside that run, 1500 wide.
    let p = json!({"start": [0, 0], "angle": 90, "width": 1200,
        "landings": [{"at": 6, "turn": 90, "width": 900}, {"at": 11, "turn": 180, "width": 1500}]});
    let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    let l = crate::stairs::stair(&s.model, &st);
    assert_eq!(l.runs.iter().map(|r| r.width).collect::<Vec<_>>(), [1200.0, 900.0, 1500.0]);
    // The first landing is as wide as the first run and as deep as the second is wide; the second run leaves its far end.
    let top = 5.0 * 280.0;
    let near = |a: P2, b: P2| a.dist(b) < 1e-6;
    assert!((crate::geom::area(&l.pads[0].ring) - 1200.0 * 900.0).abs() < 1e-3);
    assert!(near(l.runs[1].start, P2(-600.0, top + 450.0)), "{:?}", l.runs[1].start);
    // The landing back is across both runs, 900 + 1500, and as deep as the run onto it is wide.
    assert!((crate::geom::area(&l.pads[1].ring) - 2400.0 * 900.0).abs() < 1e-3, "{}", crate::geom::area(&l.pads[1].ring));
    let end = l.runs[1].end();
    assert!(
        near(l.runs[2].start, end + P2(0.0, -1200.0)),
        "beside it to its left, their middles 1200 apart: {:?} from {:?}",
        l.runs[2].start,
        end
    );
    // Its narrowest run is held to the least width; flipped, the widths are met the other way round.
    run(&mut s, "building_code", json!({"stairs": {"width_min": 1000}}));
    assert_eq!(broken(&mut s, id), ["width_min"]);
    run(&mut s, "flip", json!({"ids": [id]}));
    let back = stair_of(&s, id);
    assert_eq!((back.width, back.landings.iter().map(|l| l.width).collect::<Vec<_>>()), (1500.0, vec![900.0, 1200.0]));
    let widths: Vec<f64> = crate::stairs::stair(&s.model, &back).runs.iter().map(|r| r.width).collect();
    assert_eq!(widths, [1500.0, 900.0, 1200.0]);

    for bad in [
        json!([{"at": 6, "turn": 90, "winders": 3, "width": 900}]),
        json!([{"at": 6, "turn": 45, "width": 900}]),
        json!([{"at": 6, "turn": 90, "width": 50}]),
        json!([{"at": 6, "turn": 90, "width": -900}]),
    ] {
        assert!(s.execute("stair", &json!({"start": [0, 0], "angle": 90, "landings": bad})).is_err(), "{bad}");
    }
}

#[test]
fn a_spiral_s_centre_dragged_sets_its_radius_and_keeps_its_first_riser() {
    use crate::geom::P2;
    use crate::stairs::{Grip, dragged, grips};
    let mut s = Session::new();
    let id =
        run(&mut s, "stair", json!({"start": [1500, 0], "angle": 90, "width": 1000, "spiral": {"radius": 1500}}))["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    let m = &s.model;
    let all = grips(m, &st);
    assert_eq!(all.len(), 2);
    assert!(all[1].1.dist(P2(0.0, 0.0)) < 1e-6, "its centre, to its left: {:?}", all[1]);
    // The centre dragged to the south of its foot, 2000 away: it starts where it did, going west now, still turning left.
    let wider = dragged(m, &st, Grip::Centre, P2(1500.0, -2000.0));
    assert_eq!((wider.start, wider.spiral.map(|sp| sp.radius)), (P2(1500.0, 0.0), Some(2000.0)));
    assert!((wider.angle - 180.0).abs() < 1e-6 || (wider.angle + 180.0).abs() < 1e-6, "west, the centre to its left: {}", wider.angle);
    // No nearer than half its width.
    assert_eq!(dragged(m, &st, Grip::Centre, P2(1400.0, 0.0)).spiral.map(|sp| sp.radius), Some(500.0));
}

#[test]
fn a_stair_is_drawn_along_a_path_of_flights_and_landings() {
    use crate::geom::P2;
    let mut s = Session::new();
    // North 5 treads, a landing east 2500 long, then south to the top: two flights 2500 apart, a U with a well.
    let p = json!({"start": [0, 0], "width": 1000, "path": [
        {"to": [0, 1900]},
        {"to": [2500, 1900], "landing": true},
        {"to": [2500, -5000]}
    ]});
    let id = run(&mut s, "stair", p)["id"].as_u64().unwrap();
    let st = stair_of(&s, id);
    assert_eq!(st.angle, 90.0);
    let l = crate::stairs::stair(&s.model, &st);
    // The first flight stops half the width short of the corner: 1400 of it, 5 treads, 6 risers; the second has the 11 left.
    assert_eq!(l.runs.iter().map(|r| r.risers).collect::<Vec<_>>(), [6, 11]);
    let near = |a: P2, b: P2| a.dist(b) < 1e-6;
    assert!(near(l.runs[0].end(), P2(0.0, 1400.0)) && near(l.runs[1].start, P2(2500.0, 1400.0)), "{:?}", l.runs);
    assert!(near(l.runs[1].u, P2(0.0, -1.0)));
    // One landing between them, round both corners: 3500 long at the stair's width, 1000.
    assert_eq!((l.pads.len(), l.turns.clone()), (1, vec![1]));
    assert!((crate::geom::area(&l.pads[0].ring) - (3500.0 * 1000.0)).abs() < 1.0, "{}", crate::geom::area(&l.pads[0].ring));
    let each = 3000.0 / 17.0;
    assert!((l.pads[0].z - 6.0 * each).abs() < 1e-6 && (l.walk.last().unwrap().1 - 3000.0).abs() < 1e-6);
    assert!(!crate::stairs::path_full(&s.model, &st) || l.runs[1].length <= 6400.0);
    assert_eq!(broken(&mut s, id), Vec::<String>::new());

    // Two flights meeting at a corner with no landing drawn: the corner is the landing; with winders asked, winders.
    let corner = json!({"start": [10000, 0], "width": 1000, "path": [{"to": [10000, 2000]}, {"to": [16000, 2000]}]});
    let id = run(&mut s, "stair", corner.clone())["id"].as_u64().unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    assert_eq!((l.runs[0].risers, l.pads.len(), l.pads[0].tread), (6, 1, false));
    assert!(
        (crate::geom::area(&l.pads[0].ring) - (1000.0 * 1000.0 + 100.0 * 1000.0)).abs() < 1.0,
        "the corner and the 100 the flight left: {}",
        crate::geom::area(&l.pads[0].ring)
    );
    let mut wound = corner;
    wound["path"][1]["winders"] = json!(3);
    let id = run(&mut s, "stair", wound)["id"].as_u64().unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    assert_eq!((l.winders, l.runs.iter().map(|r| r.risers).sum::<u32>()), (3, 17 - 2));

    // Drawn past its top, the path is cut where the risers run out; a straight path of two clicks is one flight.
    let long = json!({"start": [20000, 0], "path": [{"to": [20000, 3000]}, {"to": [23000, 3000], "landing": true}, {"to": [23000, 9000]}, {"to": [30000, 9000]}]});
    let id = run(&mut s, "stair", long)["id"].as_u64().unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    assert_eq!(l.runs.iter().map(|r| r.risers).sum::<u32>(), 17);
    assert!(crate::stairs::path_full(&s.model, &stair_of(&s, id)));
    let id =
        run(&mut s, "stair", json!({"start": [30000, 0], "path": [{"to": [30000, 1000]}, {"to": [30000, 2000]}]}))["id"].as_u64().unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    assert_eq!((l.runs.len(), l.runs[0].risers, l.pads.len()), (1, 17, 0));

    // Round an arc: a quarter turn to the left of radius 2000, ring-shaped treads; then flipped and moved as any stair.
    let id = run(&mut s, "stair", json!({"start": [40000, 0], "width": 1000, "path": [{"to": [38000, 2000], "arc": 90}]}))["id"]
        .as_u64()
        .unwrap();
    let l = crate::stairs::stair(&s.model, &stair_of(&s, id));
    assert_eq!((l.runs.len(), l.pads.len()), (0, 16));
    let centre = P2(38000.0, 0.0);
    assert!(
        l.pads.iter().flat_map(|p| &p.ring).all(|q| (1499.0..=2501.0).contains(&q.dist(centre))),
        "between 1500 and 2500 from its centre"
    );
    run(&mut s, "move", json!({"ids": [id], "by": [0, 500]}));
    assert_eq!(stair_of(&s, id).path[0].to, P2(38000.0, 2500.0));
    run(&mut s, "flip", json!({"ids": [id]}));
    let back = stair_of(&s, id);
    assert_eq!((back.start, back.path[0].to, back.path[0].arc), (P2(38000.0, 2500.0), P2(40000.0, 500.0), -90.0));

    for bad in [
        json!([{"to": [0, 0]}]),
        json!([{"to": [0, 1000], "landing": true}]),
        json!([{"to": [0, 1000], "arc": 350}]),
        json!([{"to": [0, 1000], "landing": true, "winders": 3}, {"to": [0, 2000]}]),
        json!([{"to": [0, 1e300]}]),
        json!("x"),
    ] {
        assert!(s.execute("stair", &json!({"start": [0, 0], "path": bad})).is_err(), "{bad}");
    }
    assert!(s.execute("stair", &json!({"start": [0, 0], "path": [{"to": [0, 2000]}], "landings": [{"at": 4, "turn": 0}]})).is_err());
}
