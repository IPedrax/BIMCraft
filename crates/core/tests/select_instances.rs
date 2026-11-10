#![allow(clippy::unwrap_used)]

use bimcraft_core::engine::Session;
use serde_json::json;

#[test]
fn select_instances_matches_types_with_optional_candidate_scope_without_editing() {
    let mut s = Session::new();
    let walls = s.execute("wall", &json!({"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true})).unwrap()["ids"]
        .as_array()
        .unwrap()
        .clone();
    s.execute("type", &json!({"category":"wall", "name":"Thick", "thickness":300})).unwrap();
    s.execute("modify", &json!({"ids":[walls[1]], "type":"Thick"})).unwrap();
    let before = s.model.clone();
    let history = s.history().len();
    let all = s.execute("select_instances", &json!({"ids":[walls[0]]})).unwrap();
    assert_eq!(all["ids"], json!([walls[0], walls[2], walls[3]]));
    let limited =
        s.execute("select_instances", &json!({"category":"wall", "type":"Generic - 200mm", "within":[walls[3], walls[1]]})).unwrap();
    assert_eq!(limited["ids"], json!([walls[3]]));
    let empty = s.execute("select_instances", &json!({"ids":[walls[0]], "within":[]})).unwrap();
    assert_eq!(empty["ids"], json!([]));
    let both = s.execute("select_instances", &json!({"ids":[walls[0],walls[1]]})).unwrap();
    assert_eq!(both["ids"], json!(walls));
    assert_eq!(s.model, before);
    assert_eq!(s.history().len(), history);
}

#[test]
fn select_instances_keeps_same_named_types_in_other_families_separate_and_accepts_guids() {
    let mut s = Session::new();
    let mut placed = Vec::new();
    for family in ["First", "Second"] {
        s.execute("family", &json!({"name":family,"types":{"A":{}},"forms":[{"profile":[[0,0],[100,0],[100,100],[0,100]],"to":100}]}))
            .unwrap();
        for x in [0, 200] {
            placed.push(s.execute("component", &json!({"family":family,"type":"A","at":[x,0]})).unwrap()["id"].as_u64().unwrap());
        }
    }
    let guid = s.model.guid(placed[0]);
    let all = s.execute("select_instances", &json!({"ids":[guid]})).unwrap();
    assert_eq!(all["ids"], json!(&placed[..2]));
    let browser = s.execute("select_instances", &json!({"family":"Second","type":"A"})).unwrap();
    assert_eq!(browser["ids"], json!(&placed[2..]));
    assert!(s.execute("select_instances", &json!({"type":"A"})).is_err());
    assert!(s.execute("select_instances", &json!({"ids":[]})).is_err());
    assert!(s.execute("select_instances", &json!({"family":"First","type":"missing"})).is_err());
}
