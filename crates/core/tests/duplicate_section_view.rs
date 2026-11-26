//! Plain section duplication through the command used by the Project Browser.
use bimcraft_core::engine::Session;
use bimcraft_core::geom::P2;
use bimcraft_core::model::Element;
use serde_json::json;

#[test]
fn duplicate_section_preserves_settings_and_can_be_edited_independently() {
    let mut s = Session::new();
    let source = s.execute("section", &json!({"name": "Building cut", "start": [1000, 2000], "end": [9000, 2000], "depth": 6000})).unwrap()
        ["id"]
        .as_u64()
        .unwrap();
    s.script(
        r#"crop {"view": "Building cut", "region": [[500, -200], [6500, 4200]]}
scale {"view": "Building cut", "scale": 50}
detail {"view": "Building cut", "detail": "coarse"}
graphics {"view": "Building cut", "hide": ["wall"]}
underlay {"view": "Building cut", "source": "Level 1", "offset": [250, 500]}"#,
    )
    .unwrap();
    let original = s.model.section(source).unwrap().clone();
    let source_guid = s.model.guid(source);
    let count = s.model.elements.len();

    // The Browser supplies the source id and the plain duplication mode.
    let reply =
        s.execute("duplicate_view", &json!({"view": source, "mode": "duplicate"})).expect("a section should support plain Duplicate View");
    let copy = reply["id"].as_u64().unwrap();
    assert_ne!(copy, source);
    assert_ne!(s.model.guid(copy), source_guid);
    assert_eq!(s.model.elements.len(), count + 1);
    let Some(Element::Section(duplicated)) = s.model.elements.get(copy) else {
        panic!("Duplicate View must create a section");
    };
    assert_eq!(duplicated.name, "Building cut Copy 1");
    assert_eq!(reply["name"], duplicated.name);
    // Compare every section setting, including the cut, crop, graphics and underlay.
    let mut expected = original.clone();
    expected.name = duplicated.name.clone();
    assert_eq!(duplicated, &expected);

    s.execute("modify", &json!({"id": copy, "name": "Detail cut", "depth": 2500})).unwrap();
    s.execute("crop", &json!({"view": copy, "region": [[100, 0], [4100, 3000]]})).unwrap();
    s.execute("scale", &json!({"view": copy, "scale": 100})).unwrap();
    s.execute("graphics", &json!({"view": copy, "show": ["wall"]})).unwrap();
    s.execute("underlay", &json!({"view": copy, "source": null})).unwrap();
    let edited = s.model.section(copy).unwrap();
    assert_eq!(edited.name, "Detail cut");
    assert_eq!(edited.depth, Some(2500.0));
    assert_eq!((edited.start, edited.end), (P2(1600.0, 2000.0), P2(5600.0, 2000.0)));
    assert_eq!(edited.crop, Some([0.0, 3000.0]));
    assert_eq!(edited.graphics.scale, Some(100.0));
    assert_ne!(edited.graphics.categories, original.graphics.categories);
    assert!(edited.underlay.is_none());
    assert_eq!(s.model.section(source), Some(&original), "editing the duplicate must leave the source unchanged");
    assert_eq!(s.model.guid(source), source_guid);
}

#[test]
fn duplicate_section_rejects_unsupported_modes_and_taken_names_without_changes() {
    let mut s = Session::new();
    s.execute("section", &json!({"name": "Cut", "start": [0, 0], "end": [8000, 0]})).unwrap();
    let before = s.model.clone();
    for mode in ["detailing", "dependent"] {
        let error = s.execute("duplicate_view", &json!({"view": "Cut", "mode": mode})).unwrap_err();
        assert!(error.contains("sections support only"), "{error}");
        assert_eq!(s.model, before);
    }
    let error = s.execute("duplicate_view", &json!({"view": "Cut", "name": "Cut"})).unwrap_err();
    assert!(error.contains("already"), "{error}");
    assert_eq!(s.model, before);
}

#[test]
fn duplicate_section_leaves_annotation_behind_and_undo_redo_restores_its_identity() {
    let mut s = Session::new();
    s.execute("section", &json!({"name": "Cut", "start": [0, 0], "end": [8000, 0]})).unwrap();
    s.execute("text", &json!({"view": "Cut", "at": [1000, 1000], "text": "Source only"})).unwrap();
    let before = s.model.clone();
    let reply = s.execute("duplicate_view", &json!({"view": "Cut", "name": "Independent cut"})).unwrap();
    let copy = reply["id"].as_u64().unwrap();
    let guid = s.model.guid(copy);
    assert_eq!(reply["annotations"], 0);
    assert_eq!(s.model.elements.len(), before.elements.len() + 1);
    assert_eq!(s.model.view_name(copy), Some("Independent cut"));
    let notes: Vec<_> = s.model.elements.values().filter(|e| matches!(e, Element::Text(_))).collect();
    let original_notes: Vec<_> = before.elements.values().filter(|e| matches!(e, Element::Text(_))).collect();
    assert_eq!(notes, original_notes);
    let duplicated = s.model.clone();
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model, before);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(s.model, duplicated);
    assert_eq!(s.model.guid(copy), guid);
}
