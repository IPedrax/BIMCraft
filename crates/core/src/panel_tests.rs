//! The data of a distribution board: its distribution system, its mains and slots, and the board that supplies it, through the
//! commands every front end drives. (Its totals are sums over its circuits, which come with them.)

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::library::Kind;
use crate::model::Element;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// A distribution board of the starter family, placed.
fn board(s: &mut Session, at: [i32; 2]) -> u64 {
    run(s, "component", json!({"family": "Distribution Board", "type": "12 circuits", "at": at}))["id"].as_u64().unwrap()
}

/// A ceiling luminaire: equipment it is not.
fn luminaire(s: &mut Session) -> u64 {
    run(s, "component", json!({"family": "Ceiling Luminaire", "at": [3000, 3000]}))["id"].as_u64().unwrap()
}

fn panel_of(s: &Session, id: u64) -> Option<crate::electrical::Panel> {
    match s.model.elements.get(id) {
        Some(Element::Component(c)) => c.panel.clone(),
        _ => panic!("{id} is not a component"),
    }
}

#[test]
fn a_board_takes_a_distribution_system_mains_and_slots() {
    let mut s = Session::new();
    let b = board(&mut s, [1000, 1000]);
    // A new board has none of it: the 12, 24 and 36 circuits of its types are geometry, not slots.
    assert_eq!(panel_of(&s, b), None);
    let r = run(&mut s, "panel", json!({"id": b, "distribution": "230/400 V", "mains": 63, "slots": 24}));
    assert_eq!(r["id"], json!(b));
    let p = panel_of(&s, b).expect("it has panel data now");
    assert_eq!((p.distribution.as_deref(), p.mains, p.slots, p.supply_from), (Some("230/400 V"), Some(63.0), Some(24), None));
    // Schedules and agents read it by the parameters' own names.
    let e = element_json(&s.model, b);
    assert_eq!((&e["distribution_system"], &e["mains"], &e["slots"]), (&json!("230/400 V"), &json!(63.0), &json!(24)));
    // What is not given stays.
    run(&mut s, "panel", json!({"id": b, "mains": 80}));
    let p = panel_of(&s, b).unwrap();
    assert_eq!((p.distribution.as_deref(), p.mains, p.slots), (Some("230/400 V"), Some(80.0), Some(24)));
    // The library entry's name is the project's own, with an id and a GUID.
    assert!(s.model.library.entries().any(|(_, e)| e.kind == Kind::DistributionSystem && e.name == "230/400 V"));
    // One undo step, back to what it was.
    run(&mut s, "undo", json!({}));
    assert_eq!(panel_of(&s, b).unwrap().mains, Some(63.0));
}

#[test]
fn a_panel_is_taken_away_by_giving_it_nothing() {
    let mut s = Session::new();
    let b = board(&mut s, [1000, 1000]);
    let up = board(&mut s, [3000, 1000]);
    run(&mut s, "panel", json!({"id": up, "distribution": "230/400 V"}));
    run(&mut s, "panel", json!({"id": b, "distribution": "230/400 V", "mains": 63, "slots": 12, "supply_from": up}));
    run(&mut s, "panel", json!({"id": b, "mains": "", "supply_from": ""}));
    let p = panel_of(&s, b).unwrap();
    assert_eq!((p.mains, p.supply_from, p.slots), (None, None, Some(12)));
    run(&mut s, "panel", json!({"id": b, "distribution": "", "slots": ""}));
    assert_eq!(panel_of(&s, b), None, "a panel with nothing in it is no panel");
}

#[test]
fn only_electrical_equipment_has_panel_data_and_what_is_given_must_be_sound() {
    let mut s = Session::new();
    let b = board(&mut s, [1000, 1000]);
    let l = luminaire(&mut s);
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}))["ids"][0].as_u64().unwrap();
    for (p, says) in [
        (json!({"mains": 16}), "id"),
        (json!({"id": l, "mains": 16}), "equipment"),
        (json!({"id": wall, "mains": 16}), "equipment"),
        (json!({"id": 999_999, "mains": 16}), "999999"),
        (json!({"id": b, "distribution": "nope"}), "nope"),
        (json!({"id": b, "distribution": 5}), "distribution"),
        (json!({"id": b, "mains": 0}), "mains"),
        (json!({"id": b, "mains": -16}), "mains"),
        (json!({"id": b, "mains": 1e300}), "mains"),
        (json!({"id": b, "mains": "high"}), "mains"),
        (json!({"id": b, "slots": 0}), "slots"),
        (json!({"id": b, "slots": 1.5}), "slots"),
        (json!({"id": b, "slots": 1e9}), "slots"),
        (json!({"id": b, "slots": -3}), "slots"),
        (json!({"id": b, "supply_from": b}), "itself"),
        (json!({"id": b, "supply_from": l}), "equipment"),
        (json!({"id": b, "supply_from": 999_999}), "999999"),
        (json!({"id": b, "volts": 230}), "volts"),
    ] {
        let e = fails(&mut s, "panel", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    // The refusal for a system the project lacks lists the ones it has.
    let e = fails(&mut s, "panel", json!({"id": b, "distribution": "nope"}));
    assert!(e.contains("230/400 V"), "{e}");
}

#[test]
fn supply_from_builds_a_tree_and_a_cycle_is_refused() {
    let mut s = Session::new();
    let (main, sub1, sub2, leaf) = (board(&mut s, [0, 0]), board(&mut s, [2000, 0]), board(&mut s, [4000, 0]), board(&mut s, [6000, 0]));
    for (b, up) in [(sub1, main), (sub2, main), (leaf, sub1)] {
        run(&mut s, "panel", json!({"id": b, "supply_from": up}));
    }
    // Each says which board feeds it, and the main which it feeds.
    let e = element_json(&s.model, leaf);
    assert_eq!(e["supply_from"], json!(s.model.guid(sub1)));
    let r = run(&mut s, "panel", json!({}));
    let by = |id: u64| r["panels"].as_array().unwrap().iter().find(|p| p["id"] == json!(id)).cloned().unwrap();
    assert_eq!(by(leaf)["supply_from"], json!(sub1));
    assert_eq!(by(main)["feeds"], json!([sub1, sub2]));
    assert_eq!(by(sub1)["feeds"], json!([leaf]));
    assert_eq!(by(main)["supply_from"], Value::Null);
    // A board cannot be supplied from one it feeds, however far down.
    for (b, up) in [(main, leaf), (main, sub1), (sub1, leaf), (sub1, sub1)] {
        let e = fails(&mut s, "panel", json!({"id": b, "supply_from": up}));
        assert!(e.contains("itself") || e.contains("cycle") || e.contains("loop"), "{e}");
    }
    // The chain is named, so the one to change is clear.
    let e = fails(&mut s, "panel", json!({"id": main, "supply_from": leaf}));
    assert!(e.contains(&main.to_string()) && e.contains(&leaf.to_string()), "{e}");
    // A tree changed is a tree: leaf moves under the other sub-board.
    run(&mut s, "panel", json!({"id": leaf, "supply_from": sub2}));
    assert_eq!(panel_of(&s, leaf).unwrap().supply_from, Some(s.model.guid(sub2)));
}

#[test]
fn a_board_that_goes_leaves_those_it_fed_without_a_supply() {
    let mut s = Session::new();
    let (main, sub) = (board(&mut s, [0, 0]), board(&mut s, [2000, 0]));
    run(&mut s, "panel", json!({"id": sub, "distribution": "230/400 V", "mains": 40, "supply_from": main}));
    run(&mut s, "delete", json!({"ids": [main]}));
    let p = panel_of(&s, sub).expect("it is still a panel");
    assert_eq!((p.supply_from, p.mains, p.distribution.as_deref()), (None, Some(40.0), Some("230/400 V")));
    assert_eq!(s.model.check(), Ok(()));
    // And undone, the supply is there again.
    run(&mut s, "undo", json!({}));
    assert!(panel_of(&s, sub).unwrap().supply_from.is_some());
}

#[test]
fn a_distribution_system_a_board_is_in_stays_and_follows_its_rename() {
    let mut s = Session::new();
    let b = board(&mut s, [1000, 1000]);
    run(&mut s, "panel", json!({"id": b, "distribution": "230/400 V"}));
    let e = fails(&mut s, "distribution_system", json!({"name": "230/400 V", "delete": true}));
    assert!(e.contains("230/400 V") && e.contains(&b.to_string()), "the refusal names the board: {e}");
    run(&mut s, "distribution_system", json!({"name": "230/400 V", "rename": "EU 3F+N"}));
    assert_eq!(panel_of(&s, b).unwrap().distribution.as_deref(), Some("EU 3F+N"));
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn the_panel_data_is_saved_by_guid_and_opened_again() {
    let mut s = Session::new();
    let (main, sub) = (board(&mut s, [0, 0]), board(&mut s, [2000, 0]));
    run(&mut s, "panel", json!({"id": main, "distribution": "230/400 V", "mains": 125, "slots": 36}));
    run(&mut s, "panel", json!({"id": sub, "distribution": "230/400 V", "mains": 40, "slots": 12, "supply_from": main}));
    let path = std::env::temp_dir().join(format!("bimcraft-panel-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    let (a, b) = (panel_of(&t, main).unwrap(), panel_of(&t, sub).unwrap());
    assert_eq!((a.distribution.as_deref(), a.mains, a.slots), (Some("230/400 V"), Some(125.0), Some(36)));
    assert_eq!((b.mains, b.slots, b.supply_from), (Some(40.0), Some(12), Some(t.model.guid(main))));
    assert_eq!(t.model.check(), Ok(()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_copy_of_a_board_is_a_new_board_with_the_same_data() {
    let mut s = Session::new();
    let (main, sub) = (board(&mut s, [0, 0]), board(&mut s, [2000, 0]));
    run(&mut s, "panel", json!({"id": sub, "distribution": "230/400 V", "mains": 40, "slots": 12, "supply_from": main}));
    let r = run(&mut s, "copy", json!({"ids": [sub], "by": [0, 3000]}));
    let copy = r["ids"].as_array().unwrap()[0].as_u64().unwrap();
    assert_ne!(s.model.guid(copy), s.model.guid(sub));
    let p = panel_of(&s, copy).unwrap();
    assert_eq!((p.mains, p.slots, p.supply_from), (Some(40.0), Some(12), Some(s.model.guid(main))));
}

#[test]
fn boards_come_in_from_another_project_with_their_supply() {
    let mut from = Session::new();
    let (main, sub) = (board(&mut from, [0, 0]), board(&mut from, [2000, 0]));
    run(&mut from, "panel", json!({"id": main, "distribution": "230/400 V", "slots": 36}));
    run(&mut from, "panel", json!({"id": sub, "distribution": "230/400 V", "slots": 12, "supply_from": main}));
    let path = std::env::temp_dir().join(format!("bimcraft-panel-insert-{}.bimcraft", std::process::id()));
    run(&mut from, "save", json!({"path": path.to_str().unwrap()}));
    // Only the sub-board is asked for: the board that supplies it comes with it.
    let mut to = Session::new();
    let r = run(&mut to, "insert", json!({"path": path.to_str().unwrap(), "ids": [sub]}));
    let made: Vec<u64> = r["ids"].as_object().unwrap().values().filter_map(Value::as_u64).collect();
    assert_eq!(made.len(), 2, "{r}");
    let sub_here = r["ids"][sub.to_string()].as_u64().unwrap();
    let up = panel_of(&to, sub_here).unwrap().supply_from.expect("its supply came with it");
    assert_eq!(panel_of(&to, to.model.elements.id(up).unwrap()).unwrap().slots, Some(36));
    assert_eq!(to.model.check(), Ok(()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_schedule_of_components_shows_the_panel_data_by_the_parameters_names() {
    let mut s = Session::new();
    let (main, sub) = (board(&mut s, [0, 0]), board(&mut s, [2000, 0]));
    run(&mut s, "panel", json!({"id": main, "distribution": "230/400 V", "mains": 125, "slots": 36}));
    run(&mut s, "panel", json!({"id": sub, "distribution": "230/400 V", "mains": 40, "slots": 12, "supply_from": main}));
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Boards", "category": "components", "fields": ["Distribution System", "Mains", "Max #1 Pole Breakers", "Supply From"], "sort": ["Mains"]}),
    );
    assert_eq!(r["rows"], json!([["230/400 V", 40.0, 12.0, main as f64], ["230/400 V", 125.0, 36.0, 0.0]]), "{r}");
}

#[test]
fn every_panel_command_payload_is_safe() {
    let mut s = Session::new();
    let b = board(&mut s, [0, 0]);
    for p in [
        json!(null),
        json!([]),
        json!({"id": b, "mains": f64::MAX, "slots": u64::MAX}),
        json!({"id": b, "distribution": ["x"], "supply_from": {"a": 1}}),
        json!({"id": u64::MAX, "supply_from": u64::MAX}),
        json!({"id": b, "supply_from": b, "mains": "", "slots": "", "distribution": ""}),
    ] {
        let _ = s.execute("panel", &p);
        assert_eq!(s.model.check(), Ok(()), "{p}");
    }
}

// ---------- IFC ----------

fn ifc_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-panel-{tag}-{}.ifc", std::process::id()))
}

/// The IFC file the project is written as.
fn written(s: &mut Session, tag: &str) -> String {
    let path = ifc_path(tag);
    run(s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    text
}

/// The lines of `file` that are entity `name`.
fn entities<'a>(file: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("={name}(");
    file.lines().filter(|l| l.contains(&head)).collect()
}

/// `file` opened as a project, and what it left out.
fn read(text: &str, tag: &str) -> (Session, Value) {
    let path = ifc_path(tag);
    std::fs::write(&path, text).unwrap();
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    (t, r)
}

/// Two boards on a system, one feeding the other.
fn two_boards() -> (Session, u64, u64) {
    let mut s = Session::new();
    let (main, sub) = (board(&mut s, [0, 0]), board(&mut s, [2000, 0]));
    run(&mut s, "panel", json!({"id": main, "distribution": "230/400 V", "mains": 125, "slots": 36}));
    run(&mut s, "panel", json!({"id": sub, "distribution": "230/400 V", "mains": 40, "slots": 12, "supply_from": main}));
    (s, main, sub)
}

/// The GlobalId a board is written with.
fn global_id(file: &str, nth: usize) -> String {
    let line = entities(file, "IFCELECTRICDISTRIBUTIONBOARD")[nth];
    line.split('\'').nth(1).unwrap().to_string()
}

#[test]
fn a_boards_distribution_system_is_an_electrical_ifc_system_it_is_assigned_to() {
    let (mut s, main, sub) = two_boards();
    let file = written(&mut s, "export");
    // One IfcDistributionSystem for the system, whatever the number of boards on it, and both boards assigned to it.
    let systems = entities(&file, "IFCDISTRIBUTIONSYSTEM");
    assert_eq!(systems.len(), 1, "{systems:?}");
    assert!(systems[0].contains("'230/400 V'") && systems[0].ends_with(".ELECTRICAL.);"), "{}", systems[0]);
    let assigns = entities(&file, "IFCRELASSIGNSTOGROUP");
    assert_eq!(assigns.len(), 1, "{assigns:?}");
    assert_eq!(assigns[0].matches('#').count(), 4, "two boards and the system: {}", assigns[0]);
    // The rest, in the property set BIMCraft reads it back from: the mains, the slots and the board that supplies it, by GlobalId.
    let sets: Vec<&str> = entities(&file, "IFCPROPERTYSET").into_iter().filter(|l| l.contains("'BIMCraft_Panel'")).collect();
    assert_eq!(sets.len(), 2, "{sets:?}");
    assert!(file.contains("IFCELECTRICCURRENTMEASURE(125.)") && file.contains("IFCINTEGER(36)"), "{file}");
    let up = global_id(&file, 0);
    let (first, second) = (global_id(&file, 0), global_id(&file, 1));
    assert_eq!(s.model.elements.guid(main).map(|g| crate::ifc::compress(g.as_u128())), Some(first));
    assert_eq!(s.model.elements.guid(sub).map(|g| crate::ifc::compress(g.as_u128())), Some(second));
    assert_eq!(file.matches(&format!("IFCTEXT('{up}')")).count(), 1, "only the sub-board names its supply");
}

#[test]
fn a_board_without_panel_data_adds_nothing_to_the_file() {
    let mut s = Session::new();
    board(&mut s, [0, 0]);
    let file = written(&mut s, "plain");
    assert!(entities(&file, "IFCDISTRIBUTIONSYSTEM").is_empty() && !file.contains("BIMCraft_Panel"));
}

#[test]
fn the_distribution_system_and_the_supply_chain_come_back_from_ifc() {
    let (mut s, main, sub) = two_boards();
    let file = written(&mut s, "round");
    let (t, r) = read(&file, "round");
    assert_eq!(r["skipped"], json!([]), "{r}");
    // A GlobalId is the GUID, so a board is the one it was, and what it names it names.
    let (a, b) = (t.model.elements.id(s.model.guid(main)).unwrap(), t.model.elements.id(s.model.guid(sub)).unwrap());
    let (pa, pb) = (panel_of(&t, a).unwrap(), panel_of(&t, b).unwrap());
    assert_eq!((pa.distribution.as_deref(), pa.mains, pa.slots, pa.supply_from), (Some("230/400 V"), Some(125.0), Some(36), None));
    assert_eq!((pb.distribution.as_deref(), pb.mains, pb.slots), (Some("230/400 V"), Some(40.0), Some(12)));
    assert_eq!(pb.supply_from, Some(t.model.guid(a)));
    assert_eq!(t.model.check(), Ok(()));
}

#[test]
fn a_board_another_tool_put_in_a_system_takes_its_name_as_its_distribution_system() {
    let (mut s, main, _) = two_boards();
    // Another tool's file: the group, and none of BIMCraft's property set.
    let file = written(&mut s, "other").replace("'BIMCraft_Panel'", "'Pset_Other'");
    let (t, r) = read(&file, "other");
    assert_eq!(r["skipped"], json!([]), "{r}");
    let a = t.model.elements.id(s.model.guid(main)).unwrap();
    assert_eq!(panel_of(&t, a).unwrap().distribution.as_deref(), Some("230/400 V"));
}

#[test]
fn a_system_the_project_lacks_is_left_out_with_why_and_the_board_stays() {
    let (mut s, main, _) = two_boards();
    let file = written(&mut s, "unknown").replace("230/400 V", "Mystery 5 phases");
    let (t, r) = read(&file, "unknown");
    let a = t.model.elements.id(s.model.guid(main)).expect("the board stays");
    let p = panel_of(&t, a).unwrap();
    assert_eq!((p.distribution, p.mains, p.slots), (None, Some(125.0), Some(36)));
    assert!(r["skipped"].to_string().contains("Mystery 5 phases"), "{r}");
    assert_eq!(t.model.check(), Ok(()));
}

#[test]
fn a_supply_loop_in_a_file_is_broken_with_why_and_every_board_stays() {
    let mut s = Session::new();
    let (main, sub, third) = (board(&mut s, [0, 0]), board(&mut s, [2000, 0]), board(&mut s, [4000, 0]));
    run(&mut s, "panel", json!({"id": sub, "mains": 40, "supply_from": main}));
    run(&mut s, "panel", json!({"id": third, "mains": 16, "supply_from": sub}));
    let file = written(&mut s, "loop");
    let (g_main, g_sub, g_third) = (global_id(&file, 0), global_id(&file, 1), global_id(&file, 2));
    // The sub-board is supplied from the third, which it supplies.
    let looped = file.replace(&format!("IFCTEXT('{g_main}')"), &format!("IFCTEXT('{g_third}')"));
    assert!(looped != file, "the edit found the supply to change");
    let (t, r) = read(&looped, "loop");
    let at = |id: u64| t.model.elements.id(s.model.guid(id)).expect("every board stays");
    let _ = at(main);
    assert_eq!(panel_of(&t, at(sub)).unwrap().supply_from, None, "the first of the loop loses its supply");
    assert_eq!(panel_of(&t, at(third)).unwrap().supply_from, Some(t.model.guid(at(sub))), "the rest stays");
    assert!(r["skipped"].to_string().contains("supply"), "{r}");
    assert_eq!(t.model.check(), Ok(()));
    // A board that names itself.
    let selfish = file.replace(&format!("IFCTEXT('{g_main}')"), &format!("IFCTEXT('{g_sub}')"));
    let (t, r) = read(&selfish, "self");
    assert_eq!(panel_of(&t, t.model.elements.id(s.model.guid(sub)).unwrap()).unwrap().supply_from, None);
    assert!(r["skipped"].to_string().contains("supply"), "{r}");
    // A board that names one the file does not have.
    let missing = file.replace(&format!("IFCTEXT('{g_main}')"), "IFCTEXT('0000000000000000000000')");
    let (t, r) = read(&missing, "missing");
    assert_eq!(panel_of(&t, t.model.elements.id(s.model.guid(sub)).unwrap()).unwrap().supply_from, None);
    assert!(r["skipped"].to_string().contains("supply"), "{r}");
    assert_eq!(t.model.check(), Ok(()));
}

/// Each GlobalId role is a number of its own: two relationships of one element given the same role would be written with the
/// same GlobalId. (Two branches once numbered theirs from 51; a merge keeps both, and the compiler says nothing.)
#[test]
fn every_ifc_globalid_role_is_a_number_of_its_own() {
    let source = include_str!("ifc.rs");
    let mut roles: Vec<(&str, u64)> = Vec::new();
    for line in source.lines() {
        let Some(rest) = line.strip_prefix("const ") else { continue };
        let Some((name, value)) = rest.split_once(": u64 = ") else { continue };
        if let Ok(n) = value.trim_end_matches(';').trim().parse::<u64>() {
            roles.push((name, n));
        }
    }
    assert!(roles.len() > 40, "the roles were found: {}", roles.len());
    for (i, (a, n)) in roles.iter().enumerate() {
        if let Some((b, _)) = roles[..i].iter().find(|(_, m)| m == n) {
            panic!("`{a}` and `{b}` are both role {n}");
        }
    }
}
