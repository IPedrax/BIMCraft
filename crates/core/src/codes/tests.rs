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
            .flat_map(|r| r.ring(l.width))
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
    assert!(b.ring(1000.0).iter().any(|q| q.dist(corner) < 1e-6), "{:?}", b.ring(1000.0));
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
