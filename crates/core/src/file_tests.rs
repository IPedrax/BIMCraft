//! A project file read and written as the window and the agents do: compact JSON in a zip, read on every core, with the read
//! of the whole file as JSON values (the way it always was) as the oracle.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Model;
use crate::progress::Progress;

fn house() -> Session {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    s
}

/// A model read, and what was left out of it.
type Read = (Model, Vec<String>);

/// Both ways of reading `text`: on every core, and whole.
fn both(text: &str) -> (Option<Read>, Result<Read, String>) {
    let budget = crate::memory::Budget::none();
    (Model::read_on_every_core(text, &budget).unwrap(), Model::read_whole(text, &budget))
}

#[test]
fn a_file_read_on_every_core_is_the_model_the_whole_read_makes_of_it() {
    let s = house();
    let text = s.model.to_json().unwrap();
    let (fast, whole) = both(&text);
    let (fast, whole) = (fast.expect("the sample reads on every core"), whole.unwrap());
    assert_eq!(fast.0, whole.0);
    assert_eq!(fast.1, whole.1);
    assert!(fast.1.is_empty(), "{:?}", fast.1);
    // And as the file always was, pretty: the same model.
    let pretty = serde_json::to_string_pretty(&serde_json::from_str::<Value>(&text).unwrap()).unwrap();
    assert_eq!(Model::read(&pretty).unwrap().0, whole.0);
}

#[test]
fn what_a_file_holds_that_this_version_doesnt_know_is_left_out_the_same_way_on_every_core() {
    let s = house();
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    // A field a newer BIMCraft added to two elements and to the project.
    let ids: Vec<String> = file["elements"].as_object().unwrap().keys().take(2).cloned().collect();
    for id in &ids {
        file["elements"][id]["sparkle"] = json!(3);
    }
    file["future"] = json!({"x": 1});
    let text = file.to_string();
    let (fast, whole) = both(&text);
    let (fast, whole) = (fast.unwrap(), whole.unwrap());
    assert_eq!(fast.0, whole.0);
    assert_eq!(fast.1, whole.1);
    for id in &ids {
        assert!(fast.1.contains(&format!("/elements/{id}/sparkle")), "{:?}", fast.1);
    }
    assert!(fast.1.contains(&"/future".to_string()), "{:?}", fast.1);
}

#[test]
fn a_file_the_fast_read_cant_take_goes_the_whole_way_and_says_what_it_always_said() {
    let s = house();
    let text = s.model.to_json().unwrap();
    let mut newer: Value = serde_json::from_str(&text).unwrap();
    newer["format"] = json!(crate::model::FORMAT + 1);
    let mut bad_element: Value = serde_json::from_str(&text).unwrap();
    let id = bad_element["elements"].as_object().unwrap().keys().next().unwrap().clone();
    bad_element["elements"][&id]["category"] = json!("teleporter");
    for text in [newer.to_string(), bad_element.to_string(), "[1, 2]".into(), "{".into(), "".into()] {
        let (fast, whole) = both(&text);
        assert!(fast.is_none(), "{text:.60}");
        assert_eq!(Model::read(&text), whole);
    }
}

#[test]
fn a_project_saves_as_compact_json_or_zipped_when_asked_and_opens_either_way() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = house();
    let json = s.model.to_json().unwrap();
    assert!(!json.contains("\n  "), "compact");
    // Plain by default, as every BIMCraft reads it.
    let plain = dir.path().join("house.bimc");
    assert_eq!(s.execute("save", &json!({"path": plain})).unwrap()["compressed"], json!(false));
    assert_eq!(std::fs::read_to_string(&plain).unwrap(), json);
    // Zipped when asked: several times smaller.
    let zipped = dir.path().join("house-zipped.bimc");
    assert_eq!(s.execute("save", &json!({"path": zipped, "compress": true})).unwrap()["compressed"], json!(true));
    let bytes = std::fs::read(&zipped).unwrap();
    assert!(crate::bimc::is_zip(&bytes));
    assert!(bytes.len() * 4 < json.len(), "{} bytes zipped of {}", bytes.len(), json.len());
    // A .json is always plain; `compress` is true or false.
    let as_json = dir.path().join("house.json");
    assert_eq!(s.execute("save", &json!({"path": as_json, "compress": true})).unwrap()["compressed"], json!(false));
    assert!(s.execute("save", &json!({"path": plain, "compress": "yes"})).is_err());
    // A recovery copy, either way.
    let backup = dir.path().join("house.bimc.bimb");
    assert_eq!(s.execute("autosave", &json!({"path": backup, "compress": true})).unwrap()["compressed"], json!(true));
    for path in [&plain, &zipped, &as_json, &backup] {
        let mut t = Session::new();
        t.execute("open", &json!({"path": path})).unwrap();
        assert_eq!(*t.model, *s.model, "{}", path.display());
    }
}

#[test]
fn a_damaged_zip_is_refused_with_a_reason_and_the_project_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = house();
    let path = dir.path().join("house.bimc");
    s.execute("save", &json!({"path": path, "compress": true})).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let half = bytes.len() / 2;
    bytes[half] ^= 0xff;
    let broken = dir.path().join("broken.bimc");
    std::fs::write(&broken, &bytes).unwrap();
    let before = Arc::clone(&s.model);
    let e = s.execute("open", &json!({"path": broken})).unwrap_err();
    assert!(e.contains("broken.bimc"), "{e}");
    assert!(Arc::ptr_eq(&before, &s.model));
}

#[test]
fn an_open_cancelled_stops_and_leaves_the_project_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = house();
    let path = dir.path().join("house.bimc");
    s.execute("save", &json!({"path": path, "compress": true})).unwrap();
    let mut t = Session::new();
    let progress = Arc::new(Progress::new());
    progress.cancel();
    t.progress = Some(Arc::clone(&progress));
    let before = Arc::clone(&t.model);
    let e = t.execute("open", &json!({"path": path})).unwrap_err();
    assert!(e.ends_with(crate::progress::CANCELLED), "{e}");
    assert!(Arc::ptr_eq(&before, &t.model));
    // Not cancelled, it reports its stages and opens.
    let progress = Arc::new(Progress::new());
    t.progress = Some(Arc::clone(&progress));
    t.execute("open", &json!({"path": path})).unwrap();
    assert_eq!(progress.now().0, "Checking the model");
}

/// The check of `m` one element after another, the way it always was.
fn one_by_one(m: &Model) -> Result<(), String> {
    crate::model::ONE_BY_ONE.with(|c| c.set(true));
    let r = m.check();
    crate::model::ONE_BY_ONE.with(|c| c.set(false));
    r
}

#[test]
fn a_check_on_every_core_says_what_the_check_one_by_one_says() {
    let mut s = Session::new();
    s.execute("tower", &json!({"storeys": 3})).unwrap();
    assert!(s.model.elements.len() >= 20_000, "{} elements: too few to be checked on every core", s.model.elements.len());
    assert_eq!(s.model.check(), Ok(()));
    assert_eq!(one_by_one(&s.model), Ok(()));
    let levels: Vec<_> = s.model.levels().into_iter().map(|(id, _)| id).collect();
    // Two levels of one name, far apart in the ids; an element wrong late in the ids; both.
    let mut named = (*s.model).clone();
    let name = named.level(levels[0]).unwrap().name.clone();
    if let Some(crate::model::Element::Level(l)) = named.elements.get_mut(levels[levels.len() - 1]) {
        l.name = name;
    }
    let last = *s.model.elements.keys().rfind(|id| matches!(s.model.elements.get(**id), Some(crate::model::Element::Wall(_)))).unwrap();
    let mut wrong = (*s.model).clone();
    if let Some(crate::model::Element::Wall(w)) = wrong.elements.get_mut(last) {
        w.height = f64::NAN;
    }
    let mut both = wrong.clone();
    both.elements = named.elements.clone();
    if let Some(crate::model::Element::Wall(w)) = both.elements.get_mut(last) {
        w.height = f64::NAN;
    }
    for m in [&named, &wrong, &both] {
        let e = m.check();
        assert!(e.is_err());
        assert_eq!(e, one_by_one(m));
    }
}
