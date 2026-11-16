use super::*;

fn unit(kw: Option<f64>, kva: Option<f64>, phases: Option<u8>) -> Unit {
    Unit { installed_kw: kw, demand_kva: kva, phases }
}

fn table(utility: &str, system: &str, use_: &str) -> &'static Table {
    let (u, _) = find(utility).expect("a utility of the file");
    u.table(system, use_).expect("a table of the utility")
}

#[test]
fn the_file_reads_and_every_utility_names_its_source() {
    let all = utilities();
    assert!(all.len() >= 2, "the file did not read: {}", all.len());
    for u in all {
        assert!(!u.source.is_empty() && !u.standard.is_empty() && !u.rule.is_empty(), "{} names no source", u.id);
        assert!(!u.tables.is_empty(), "{} has no table", u.id);
        for t in &u.tables {
            assert!(t.source.contains("page"), "{} {} {}: the source names no page", u.id, t.system, t.use_);
            assert!(!t.rows.is_empty(), "{} {}: no rows", u.id, t.system);
            let same = u.tables.iter().filter(|o| o.system == t.system && o.use_ == t.use_).count();
            assert_eq!(same, 1, "{}: two tables for {} {}", u.id, t.system, t.use_);
        }
    }
}

#[test]
fn ids_are_unique_across_groups_and_members() {
    let mut names: Vec<String> = names().iter().map(|n| n.to_ascii_uppercase()).collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), n, "an id is used twice");
}

#[test]
fn every_row_is_well_formed() {
    for u in utilities() {
        for t in &u.tables {
            let mut seen = Vec::new();
            for r in &t.rows {
                let at = format!("{} {} {} {}", u.id, t.system, t.use_, r.name);
                assert!(!seen.contains(&r.name), "{at}: the name is used twice");
                seen.push(r.name.clone());
                assert!((1..=3).contains(&r.phases), "{at}: phases");
                assert!(!r.breaker_a.is_empty() && r.breaker_a.iter().all(|b| b.is_finite() && *b > 0.0), "{at}: breaker");
                assert!(r.installed_kw.is_some() || r.demand_kva.is_some(), "{at}: no range to pick it by");
                for range in [r.installed_kw, r.demand_kva].into_iter().flatten() {
                    assert!(range.over >= 0.0 && range.over < range.up_to, "{at}: range {range:?}");
                }
                for k in r.columns.keys() {
                    assert!(u.columns.contains_key(k), "{at}: column `{k}` is not explained in the utility's `columns`");
                }
            }
        }
    }
}

#[test]
fn the_ranges_of_a_table_leave_no_gap() {
    for u in utilities() {
        for t in &u.tables {
            // The installed load: from 0, each range starting where one before it ends or inside one.
            let mut kw: Vec<Range> = t.rows.iter().filter_map(|r| r.installed_kw).collect();
            kw.sort_by(|a, b| a.over.total_cmp(&b.over));
            let mut reach = 0.0_f64;
            for r in &kw {
                assert!(r.over <= reach, "{} {}: no row holds an installed load from {reach} to {} kW", u.id, t.system, r.over);
                reach = reach.max(r.up_to);
            }
            // The demand, among rows of the same phases and installed load: contiguous from 0, the breaker never smaller.
            let mut groups: Vec<(u8, Option<(f64, f64)>)> = Vec::new();
            for r in t.rows.iter().filter(|r| r.demand_kva.is_some()) {
                let key = (r.phases, r.installed_kw.map(|k| (k.over, k.up_to)));
                if !groups.contains(&key) {
                    groups.push(key);
                }
            }
            for key in groups {
                let rows: Vec<&Row> = t
                    .rows
                    .iter()
                    .filter(|r| r.demand_kva.is_some() && (r.phases, r.installed_kw.map(|k| (k.over, k.up_to))) == key)
                    .collect();
                let mut end: Option<f64> = None;
                let mut breaker = 0.0_f64;
                for r in rows {
                    let d = r.demand_kva.unwrap_or(Range { over: 0.0, up_to: 0.0 });
                    if let Some(e) = end {
                        assert_eq!(d.over, e, "{} {} {}: a gap or overlap in the demand", u.id, t.system, r.name);
                    }
                    end = Some(d.up_to);
                    let least = r.breaker_a.iter().copied().fold(f64::INFINITY, f64::min);
                    assert!(least >= breaker, "{} {} {}: the breaker shrinks as the demand grows", u.id, t.system, r.name);
                    breaker = least;
                }
            }
        }
    }
}

#[test]
fn members_change_only_categories_that_exist() {
    for u in utilities() {
        for m in &u.members {
            for c in &m.changes {
                let t = u.table(&c.system, &c.use_);
                assert!(t.is_some_and(|t| t.rows.iter().any(|r| r.name == c.category)), "{}: {} {} {}", m.id, c.system, c.use_, c.category);
                assert!(!c.source.is_empty(), "{}: a change with no source", m.id);
            }
        }
    }
}

#[test]
fn fecoergs_picks_by_installed_load_then_by_demand() {
    let t = table("FECOERGS", "127/220", "individual");
    let p = pick(t, None, unit(Some(8.0), None, None));
    assert_eq!(p.category.map(|r| r.name), Some("A1".to_string()));
    let p = pick(t, None, unit(Some(12.0), None, None));
    assert_eq!(p.category.map(|r| r.name), Some("B1".to_string()));
    // Past 15 kW the category is the demand's: without it, every C row is named as needing it.
    let p = pick(t, None, unit(Some(20.0), None, None));
    assert!(p.category.is_none());
    assert_eq!(p.needs.len(), 9);
    assert!(p.needs.iter().all(|(_, what)| *what == "demand_kva"));
    let p = pick(t, None, unit(Some(20.0), Some(25.0), None));
    let c = p.category.expect("C4");
    assert_eq!((c.name.as_str(), c.breaker_a.as_slice()), ("C4", &[63.0, 70.0][..]));
    assert_eq!(c.columns.get("connection").and_then(|v| v.as_str()), Some("Q-25"));
    // A range's top belongs to it, its bottom to the row before.
    let p = pick(t, None, unit(Some(20.0), Some(27.0), None));
    assert_eq!(p.category.map(|r| r.name), Some("C4".to_string()));
    let p = pick(t, None, unit(Some(20.0), Some(27.01), None));
    assert_eq!(p.category.map(|r| r.name), Some("C5".to_string()));
    // Past the table, nothing.
    let p = pick(t, None, unit(Some(80.0), None, None));
    assert!(p.category.is_none() && p.needs.is_empty());
}

#[test]
fn coprel_and_cerfox_change_c18_by_their_own_documents() {
    let (u, m) = find("coprel").expect("Coprel");
    assert_eq!(u.id, "FECOERGS");
    let t = u.table("220/380", "individual").expect("220/380");
    let unit = unit(Some(70.0), Some(70.0), None);
    let fecoergs = pick(t, None, unit);
    assert_eq!(fecoergs.category.as_ref().and_then(|r| r.measurement.clone()).as_deref(), Some("indirect"));
    assert!(fecoergs.changed_by.is_empty());
    let coprel = pick(t, m, unit);
    let c = coprel.category.expect("C18");
    assert_eq!((c.name.as_str(), c.measurement.as_deref()), ("C18", Some("direct or indirect")));
    assert_eq!(coprel.changed_by.len(), 1);
    assert!(coprel.changed_by[0].contains("OTD 006.01"));
    // Another member keeps the RIC as printed.
    let (_, certel) = find("CERTEL").expect("Certel");
    assert_eq!(pick(t, certel, unit).category.and_then(|r| r.measurement).as_deref(), Some("indirect"));
    // Cerfox keeps it indirect and cites the exception its own documents allow.
    let (_, cerfox) = find("Cerfox").expect("Cerfox");
    let p = pick(t, cerfox, unit);
    assert_eq!(p.category.and_then(|r| r.measurement).as_deref(), Some("indirect"));
    assert!(p.changed_by.len() == 1 && p.changed_by[0].contains("OT_NIC_019"), "{:?}", p.changed_by);
}

#[test]
fn hidropan_names_both_categories_where_both_hold_the_load() {
    let t = table("HIDROPAN", "220/380", "individual");
    let p = pick(t, None, unit(Some(10.0), Some(10.0), None));
    assert_eq!(p.category.map(|r| r.name), Some("A".to_string()));
    assert_eq!(p.candidates, vec!["A".to_string(), "C1".to_string()]);
    let p = pick(t, None, unit(Some(10.0), Some(10.0), Some(3)));
    assert_eq!(p.category.map(|r| (r.name, r.breaker_a)), Some(("C1".to_string(), vec![30.0])));
    let p = pick(t, None, unit(Some(40.0), Some(40.0), None));
    assert_eq!(p.category.map(|r| (r.name, r.breaker_a)), Some(("C4".to_string(), vec![70.0])));
}

#[test]
fn the_metering_centre_goes_by_the_demand_alone() {
    let t = table("FECOERGS", "220/380", "metering_centre");
    let p = pick(t, None, unit(None, Some(100.0), None));
    let c = p.category.expect("C23");
    assert_eq!((c.name.as_str(), c.breaker_a.as_slice()), ("C23", &[175.0][..]));
}

#[test]
fn energisa_picks_single_and_two_phase_by_load_and_three_phase_by_demand() {
    let t = table("ENERGISA", "127/220", "individual");
    let p = pick(t, None, unit(Some(5.0), None, Some(1)));
    assert_eq!(p.category.map(|r| r.name), Some("M2".to_string()));
    let p = pick(t, None, unit(Some(11.0), None, Some(2)));
    let c = p.category.expect("B2");
    assert_eq!((c.name.as_str(), c.breaker_a.as_slice()), ("B2", &[60.0, 63.0][..]));
    // Three-phase rows go by the demand alone.
    let p = pick(t, None, unit(Some(30.0), None, Some(3)));
    assert!(p.category.is_none());
    assert_eq!(p.needs.len(), 6);
    let p = pick(t, None, unit(None, Some(40.0), Some(3)));
    assert_eq!(p.category.map(|r| (r.name, r.breaker_a)), Some(("T5".to_string(), vec![150.0])));
    let t = table("ENERGISA", "220/380", "individual");
    let p = pick(t, None, unit(None, Some(70.0), Some(3)));
    assert_eq!(p.category.map(|r| (r.name, r.breaker_a)), Some(("T5".to_string(), vec![125.0])));
    // A company of the group is found by its own name.
    assert_eq!(find("emt").map(|(u, m)| (u.id.as_str(), m.map(|m| m.seat.as_str()))), Some(("ENERGISA", Some("Cuiabá"))));
}

#[test]
fn equatorial_and_copel_pick_by_their_own_measure() {
    // Equatorial: the installed load, a printed '5,1' read from 5.
    let t = table("EQUATORIAL", "220/380", "individual");
    let p = pick(t, None, unit(Some(5.05), None, Some(1)));
    assert_eq!(p.category.map(|r| r.name), Some("MONO 40 A".to_string()));
    let t = table("EQUATORIAL", "127/220", "individual");
    let p = pick(t, None, unit(Some(55.0), None, Some(3)));
    assert_eq!(p.category.map(|r| (r.name, r.breaker_a)), Some(("TRI 175 A".to_string(), vec![175.0])));
    // Copel: the demand, urban and rural apart.
    let t = table("COPEL", "127/220", "individual");
    let p = pick(t, None, unit(None, Some(25.0), Some(3)));
    assert_eq!(p.category.map(|r| (r.name, r.breaker_a)), Some(("38".to_string(), vec![80.0, 70.0])));
    let t = table("COPEL", "127/254", "individual");
    let p = pick(t, None, unit(None, Some(12.0), None));
    assert_eq!(p.category.map(|r| r.name), Some("22".to_string()));
}

mod command {
    use crate::engine::Session;
    use serde_json::{Value, json};

    fn run(s: &mut Session, p: Value) -> Value {
        match s.execute("demand", &p) {
            Ok(r) => r,
            Err(e) => panic!("{p}: {e}"),
        }
    }

    #[test]
    fn demand_frames_a_cooperative_unit_from_the_data() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "FECOERGS", "installed_kw": 20, "demand_kva": 25}));
        assert_eq!(r["system"], "127/220");
        assert_eq!(r["category"]["name"], "C4");
        assert_eq!(r["category"]["breaker_a"], json!([63.0, 70.0]));
        assert_eq!(r["category"]["columns"]["entry_mm2"], 25);
        assert!(r["columns"]["entry_mm2"].as_str().is_some_and(|t| t.contains("copper")), "{r}");
        assert!(r["source"].as_str().is_some_and(|t| t.contains("REGD 035.01.06")), "{r}");
        assert!(r["table"].as_str().is_some_and(|t| t.contains("page 68")), "{r}");
        let r = run(&mut s, json!({"utility": "FECOERGS", "installed_kw": 20}));
        assert_eq!(r["category"], Value::Null);
        assert!(r["category_note"].as_str().is_some_and(|t| t.contains("C1") && t.contains("demand_kva")), "{r}");
    }

    #[test]
    fn a_member_is_named_by_itself_and_cites_its_own_change() {
        let mut s = Session::new();
        let r = run(&mut s, json!({"utility": "Coprel", "system": "220/380", "installed_kw": 70, "demand_kva": 70}));
        assert_eq!((r["utility"].as_str(), r["group"].as_str()), (Some("COPREL"), Some("FECOERGS")));
        assert_eq!(r["category"]["name"], "C18");
        assert_eq!(r["category"]["measurement"], "direct or indirect");
        assert!(r["changed_by"][0].as_str().is_some_and(|t| t.contains("OTD 006.01")), "{r}");
        let r = run(&mut s, json!({"utility": "FECOERGS", "system": "220/380", "installed_kw": 70, "demand_kva": 70}));
        assert_eq!(r["category"]["measurement"], "indirect");
        let r = run(&mut s, json!({"utility": "FECOERGS", "entrance": "metering_centre", "demand_kva": 90}));
        assert_eq!((r["system"].as_str(), r["category"]["name"].as_str()), (Some("220/380"), Some("C22")));
    }

    #[test]
    fn what_a_data_utility_does_not_take_is_refused() {
        let mut s = Session::new();
        for p in [
            json!({"utility": "FECOERGS"}),
            json!({"utility": "FECOERGS", "installed_kw": -1}),
            json!({"utility": "FECOERGS", "installed_kw": f64::MAX}),
            json!({"utility": "FECOERGS", "installed_kw": 5, "system": "127/254"}),
            json!({"utility": "FECOERGS", "installed_kw": 5, "entrance": "collective"}),
            json!({"utility": "FECOERGS", "installed_kw": 5, "phases": 4}),
            json!({"utility": "FECOERGS", "installed_kw": 5, "area": 40}),
            json!({"utility": "HIDROPAN", "installed_kw": 5, "system": "127/220"}),
            json!({"utility": "CPFL", "entrance": "individual"}),
        ] {
            assert!(s.execute("demand", &p).is_err(), "{p}");
        }
    }
}
