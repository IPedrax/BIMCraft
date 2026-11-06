//! The pipe network (`network`), through the command every front end drives: nodes where pipes' ends and connectors meet,
//! the edges between them, the networks they make, the open ends and what looks wrong.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn made(s: &mut Session, id: &str, p: Value) -> Vec<u64> {
    let r = run(s, id, p);
    match r["ids"].as_array() {
        Some(v) => v.iter().filter_map(Value::as_u64).collect(),
        None => r["id"].as_u64().into_iter().collect(),
    }
}

fn pipe(s: &mut Session, points: Value, extra: Value) -> Vec<u64> {
    let mut p = json!({"points": points});
    if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    made(s, "pipe", p)
}

fn net(s: &mut Session, p: Value) -> Value {
    run(s, "network", p)
}

fn kinds(n: &Value) -> Vec<String> {
    n["warnings"].as_array().unwrap().iter().map(|w| w["kind"].as_str().unwrap().to_string()).collect()
}

#[test]
fn pipes_end_to_end_are_one_network_with_two_open_ends() {
    let mut s = Session::new();
    let ids = pipe(&mut s, json!([[0, 0], [3000, 0], [3000, 2000]]), json!({"offset": 500, "system": "Água fria"}));
    assert_eq!(ids.len(), 2);
    let n = net(&mut s, json!({}));
    assert_eq!(n["nodes"].as_array().unwrap().len(), 3, "{n}");
    assert_eq!(n["edges"].as_array().unwrap().len(), 2);
    assert_eq!(n["groups"].as_array().unwrap().len(), 1);
    assert_eq!(n["groups"][0]["systems"], json!(["Água fria"]));
    assert_eq!(n["open"].as_array().unwrap().len(), 2, "the first pipe's start and the last's end");
    assert_eq!(n["edges"][0]["length"], 3000.0);
    assert_eq!(kinds(&n), Vec::<String>::new());
    // Nothing is stored: the model is as it was, and the command is no step to undo.
    assert_eq!(crate::engine::find("network").map(|c| c.undoable), Some(false));
}

#[test]
fn a_fixtures_connector_is_a_node_and_one_with_no_pipe_is_open() {
    let mut s = Session::new();
    let wc = made(&mut s, "component", json!({"family": "WC", "at": [0, 0]}))[0];
    // Its waste outlet is 300 mm out from the wall, through the floor.
    let drain = pipe(&mut s, json!([[0, -300], [2000, -300]]), json!({"offset": 0, "end_offset": -40, "system": "Esgoto"}))[0];
    let n = net(&mut s, json!({}));
    let groups = n["groups"].as_array().unwrap();
    let with_wc: Vec<&Value> = groups.iter().filter(|g| g["elements"].as_array().unwrap().contains(&json!(wc))).collect();
    let joined = with_wc.iter().find(|g| g["elements"].as_array().unwrap().contains(&json!(drain))).unwrap();
    assert_eq!(joined["domains"], json!(["waste"]), "{n}");
    // Open: the drain's far end and the WC's cold water inlet.
    let open: Vec<&Value> = n["open"].as_array().unwrap().iter().map(|i| &n["nodes"][i.as_u64().unwrap() as usize]).collect();
    assert_eq!(open.len(), 2, "{n}");
    assert!(open.iter().any(|x| x["joins"][0]["domain"] == "cold"), "{open:?}");
    assert!(open.iter().any(|x| x["joins"][0]["pipe"] == drain && x["joins"][0]["end"] == "end"), "{open:?}");
}

#[test]
fn a_fitting_joins_the_pipes_at_its_ends_through_it() {
    let mut s = Session::new();
    // A tee of DN 50: its ends 105 mm out from its middle along x, and one toward +y.
    let tee = made(&mut s, "component", json!({"family": "Tee", "type": "50", "at": [0, 0], "offset": 500}))[0];
    for pts in [json!([[-1000, 0], [-105, 0]]), json!([[105, 0], [1000, 0]]), json!([[0, 105], [0, 1000]])] {
        pipe(&mut s, pts, json!({"offset": 500, "system": "Água fria"}));
    }
    let n = net(&mut s, json!({}));
    assert_eq!(n["groups"].as_array().unwrap().len(), 1, "{n}");
    let through: Vec<&Value> = n["edges"].as_array().unwrap().iter().filter(|e| e["through"] == tee).collect();
    assert_eq!(through.len(), 2, "from its first end to the other two: {n}");
    assert_eq!(n["open"].as_array().unwrap().len(), 3, "the three pipes' far ends");
    assert_eq!(kinds(&n), Vec::<String>::new(), "a fitting takes its pipes' domain, so nothing is mixed");
}

#[test]
fn a_fixtures_waste_and_water_are_not_joined_through_it() {
    let mut s = Session::new();
    made(&mut s, "component", json!({"family": "WC", "at": [0, 0]}));
    let n = net(&mut s, json!({}));
    assert_eq!(n["groups"].as_array().unwrap().len(), 2, "its waste and its cold water: {n}");
    assert_eq!(n["edges"].as_array().unwrap().len(), 0);
}

#[test]
fn two_systems_in_one_network_are_warned_of_not_refused() {
    let mut s = Session::new();
    pipe(&mut s, json!([[0, 0], [2000, 0]]), json!({"offset": 500, "system": "Esgoto"}));
    pipe(&mut s, json!([[2000, 0], [4000, 0]]), json!({"offset": 500, "system": "Água fria"}));
    let n = net(&mut s, json!({}));
    assert_eq!(kinds(&n), vec!["systems_meet"], "{n}");
    assert!(n["warnings"][0]["message"].as_str().unwrap().contains("Esgoto"), "{n}");
    assert_eq!(s.model.check(), Ok(()), "a warning is no error");
    // Asked for one system, the network its pipes are in, whole.
    let only = net(&mut s, json!({"system": "Esgoto"}));
    assert_eq!(only["groups"].as_array().unwrap().len(), 1);
    assert_eq!(net(&mut s, json!({"system": "Gás"}))["groups"], json!([]));
}

#[test]
fn a_waste_and_a_cold_water_connector_in_one_network_are_warned_of() {
    let mut s = Session::new();
    made(&mut s, "component", json!({"family": "WC", "at": [0, 0]}));
    // From the WC's waste outlet straight to its cistern's inlet.
    pipe(&mut s, json!([[0, -300], [150, 0]]), json!({"offset": 0, "end_offset": 200}));
    let n = net(&mut s, json!({}));
    assert!(kinds(&n).contains(&"domains_meet".to_string()), "{n}");
}

#[test]
fn a_pipe_met_along_its_length_is_split_there_and_warned_of() {
    let mut s = Session::new();
    let main = pipe(&mut s, json!([[0, 0], [4000, 0]]), json!({"offset": 500}))[0];
    pipe(&mut s, json!([[1000, 0], [1000, 2000]]), json!({"offset": 500}));
    let n = net(&mut s, json!({}));
    assert_eq!(n["groups"].as_array().unwrap().len(), 1, "{n}");
    let pieces: Vec<f64> =
        n["edges"].as_array().unwrap().iter().filter(|e| e["pipe"] == main).map(|e| e["length"].as_f64().unwrap()).collect();
    assert_eq!(pieces, vec![1000.0, 3000.0], "{n}");
    assert_eq!(kinds(&n), vec!["joined_mid_run"]);
    assert_eq!(n["warnings"][0]["at"], json!([1000.0, 0.0, 500.0]));
    let pipes = s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Pipe(_))).count();
    assert_eq!(pipes, 2, "the pipe stays whole");
}

#[test]
fn a_drain_that_narrows_downstream_is_warned_of() {
    let mut s = Session::new();
    made(&mut s, "component", json!({"family": "WC", "at": [0, 0]}));
    // The WC's waste down a 110 to a node, then on down a 50.
    pipe(&mut s, json!([[0, -300], [2000, -300]]), json!({"offset": 0, "end_offset": -40, "type": "PVC 110"}));
    pipe(&mut s, json!([[2000, -300], [4000, -300]]), json!({"offset": -40, "end_offset": -80, "type": "PVC 50"}));
    let n = net(&mut s, json!({}));
    assert_eq!(kinds(&n), vec!["smaller_downstream"], "{n}");
    // The other way round, widening as it goes down, is right.
    let mut s = Session::new();
    made(&mut s, "component", json!({"family": "WC", "at": [0, 0]}));
    pipe(&mut s, json!([[0, -300], [2000, -300]]), json!({"offset": 0, "end_offset": -40, "type": "PVC 50"}));
    pipe(&mut s, json!([[2000, -300], [4000, -300]]), json!({"offset": -40, "end_offset": -80, "type": "PVC 110"}));
    assert_eq!(kinds(&net(&mut s, json!({}))), Vec::<String>::new());
}

#[test]
fn the_tolerance_says_how_close_is_one_node_and_a_level_keeps_its_own() {
    let mut s = Session::new();
    pipe(&mut s, json!([[0, 0], [2000, 0]]), json!({"offset": 500}));
    pipe(&mut s, json!([[2003, 0], [4000, 0]]), json!({"offset": 500}));
    assert_eq!(net(&mut s, json!({}))["groups"].as_array().unwrap().len(), 2, "3 mm apart, past 1 mm");
    assert_eq!(net(&mut s, json!({"tolerance": 5}))["groups"].as_array().unwrap().len(), 1);
    pipe(&mut s, json!([[0, 0], [2000, 0]]), json!({"offset": 500, "level": "Level 2"}));
    assert_eq!(net(&mut s, json!({"level": "Level 2"}))["groups"].as_array().unwrap().len(), 1);
    assert_eq!(net(&mut s, json!({}))["groups"].as_array().unwrap().len(), 3);
}

#[test]
fn what_it_cant_use_is_refused_and_changes_nothing() {
    let mut s = Session::new();
    for p in [
        json!([]),
        json!({"tolerance": -1}),
        json!({"tolerance": 1000}),
        json!({"tolerance": "x"}),
        json!({"level": "nowhere"}),
        json!({"system": 5}),
        json!({"domain": "waste"}),
    ] {
        let before = s.model.clone();
        assert!(s.execute("network", &p).is_err(), "{p}");
        assert_eq!(*s.model, *before);
    }
}

#[test]
fn a_stack_standing_upright_joins_the_drain_at_its_foot() {
    let mut s = Session::new();
    // A stack from 2500 down to the floor (the same point twice, two heights), then a drain on from its foot.
    let stack = pipe(&mut s, json!([[0, 0], [0, 0]]), json!({"offset": 2500, "end_offset": 0, "type": "PVC 110"}))[0];
    let drain = pipe(&mut s, json!([[0, 0], [3000, 0]]), json!({"offset": 0, "end_offset": -60, "type": "PVC 110"}))[0];
    let n = net(&mut s, json!({}));
    assert_eq!(n["groups"].as_array().unwrap().len(), 1, "{n}");
    let foot = n["nodes"].as_array().unwrap().iter().find(|x| x["joins"].as_array().unwrap().len() == 2).unwrap();
    assert_eq!(foot["at"], json!([0.0, 0.0, 0.0]));
    let pipes: Vec<&Value> = foot["joins"].as_array().unwrap().iter().map(|j| &j["pipe"]).collect();
    assert!(pipes.contains(&&json!(stack)) && pipes.contains(&&json!(drain)), "{foot}");
    assert_eq!(n["edges"][0]["length"], 2500.0, "a standing pipe's length is its height");
}
