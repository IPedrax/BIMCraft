//! Plain elevation duplication through the command used by the Project Browser.
use bimcraft_core::engine::Session;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id};
use bimcraft_core::render::{self, View};
use serde_json::json;

#[cfg(test)]
fn north() -> (Session, Id) {
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0, 0], [8000, 0]], "height": 3000})).unwrap();
    let source = s.execute("elevation", &json!({"facing": "north", "name": "North Elevation"})).unwrap()["id"].as_u64().unwrap();
    (s, source)
}

#[test]
fn duplicate_elevation_preserves_settings_and_edits_independently() {
    let (mut s, source) = north();
    s.script(
        r#"crop {"view": "North Elevation", "region": [[500, -200], [9500, 4200]]}
scale {"view": "North Elevation", "scale": 50}
detail {"view": "North Elevation", "detail": "coarse"}
graphics {"view": "North Elevation", "hide": ["wall"]}
template {"name": "Elevation detail", "from": "North Elevation", "controls": ["detail"]}
graphics {"view": "North Elevation", "template": "Elevation detail"}
underlay {"view": "North Elevation", "source": "Level 1", "offset": [250, 500]}"#,
    )
    .unwrap();
    s.execute("modify", &json!({"id": source, "depth": 12000})).unwrap();
    let original = s.model.section(source).unwrap().clone();
    let reply = s.execute("duplicate_view", &json!({"view": source, "mode": "duplicate"})).unwrap();
    let copy = reply["id"].as_u64().unwrap();
    assert_ne!(copy, source);
    assert_ne!(s.model.guid(copy), s.model.guid(source));
    assert!(matches!(s.model.elements.get(copy), Some(Element::Elevation(_))));
    assert_eq!(reply["name"], "North Elevation Copy 1");
    let mut expected = original.clone();
    expected.name = "North Elevation Copy 1".into();
    assert_eq!(s.model.section(copy), Some(&expected));
    assert_eq!(s.model.section(copy).unwrap().looking(), P2(0.0, -1.0));

    s.execute("modify", &json!({"id": copy, "name": "North detail", "depth": 5000})).unwrap();
    s.execute("crop", &json!({"view": copy, "region": [[100, 0], [4100, 3000]]})).unwrap();
    s.execute("scale", &json!({"view": copy, "scale": 100})).unwrap();
    s.execute("graphics", &json!({"view": copy, "show": ["wall"]})).unwrap();
    s.execute("underlay", &json!({"view": copy, "source": null})).unwrap();
    let edited = s.model.section(copy).unwrap();
    assert_eq!(edited.name, "North detail");
    assert_eq!(edited.depth, Some(5000.0));
    assert_eq!(edited.crop, Some([0.0, 3000.0]));
    assert_eq!(edited.graphics.scale, Some(100.0));
    assert_ne!(edited.graphics.categories, original.graphics.categories);
    assert!(edited.underlay.is_none());
    assert_eq!(edited.looking(), original.looking());
    assert_eq!(s.model.section(source), Some(&original));
}

#[test]
fn duplicate_elevation_names_modes_and_undo_redo() {
    let (mut s, source) = north();
    let before = s.model.clone();
    for mode in ["detailing", "dependent", "twin"] {
        assert!(s.execute("duplicate_view", &json!({"view": source, "mode": mode})).is_err());
        assert_eq!(s.model, before);
    }
    for name in ["North Elevation", "Level 1", ""] {
        assert!(s.execute("duplicate_view", &json!({"view": source, "name": name})).is_err());
        assert_eq!(s.model, before);
    }
    // A name occupied by another kind of view must also be skipped.
    s.execute("duplicate_view", &json!({"view": "Level 1", "name": "North Elevation Copy 1"})).unwrap();
    let reply = s.execute("duplicate_view", &json!({"view": "North Elevation"})).unwrap();
    assert_eq!(reply["name"], "North Elevation Copy 2");
    let before = s.model.clone();
    let reply = s.execute("duplicate_view", &json!({"view": source})).unwrap();
    assert_eq!(reply["name"], "North Elevation Copy 3");
    let copy = reply["id"].as_u64().unwrap();
    let guid = s.model.guid(copy);
    let duplicated = s.model.clone();
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model, before);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(s.model, duplicated);
    assert_eq!(s.model.guid(copy), guid);
}

#[test]
fn duplicate_elevation_leaves_annotation_and_survives_save_reopen() {
    let mut s = Session::new();
    let source = s.execute("elevation", &json!({"name": "Facade", "reference": [[0, 0], [8000, 4000]], "at": [2000, 5000]})).unwrap()["id"]
        .as_u64()
        .unwrap();
    s.execute("crop", &json!({"view": source, "region": [[100, -200], [6000, 4200]]})).unwrap();
    s.execute("text", &json!({"view": source, "at": [1000, 1000], "text": "Source only"})).unwrap();
    s.execute("detail_line", &json!({"view": source, "start": [0, 500], "end": [3000, 500]})).unwrap();
    let before = s.model.clone();
    let reply = s.execute("duplicate_view", &json!({"view": source, "name": "Independent facade"})).unwrap();
    let copy = reply["id"].as_u64().unwrap();
    assert_eq!(reply["annotations"], 0);
    assert_eq!(s.model.elements.len(), before.elements.len() + 1);
    for (id, element) in before.elements.iter() {
        assert_eq!(s.model.elements.get(*id), Some(element));
    }
    let original = s.model.section(source).unwrap().clone();
    let mut expected = original.clone();
    expected.name = "Independent facade".into();
    assert!(expected.reference.is_some() && expected.mark.is_some());
    assert_eq!(s.model.section(copy), Some(&expected));
    s.execute("scale", &json!({"view": copy, "scale": 25})).unwrap();
    expected.graphics.scale = Some(25.0);
    let (source_guid, copy_guid) = (s.model.guid(source), s.model.guid(copy));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("elevations.bimcraft");
    s.execute("save", &json!({"path": path})).unwrap();
    let mut reopened = Session::new();
    let opened = reopened.execute("open", &json!({"path": path})).unwrap();
    assert!(opened.get("left_out").is_none(), "{opened}");
    let copy = reopened.model.elements.id(copy_guid).unwrap();
    assert!(matches!(reopened.model.elements.get(copy), Some(Element::Elevation(_))));
    assert_eq!(reopened.model.view_named("Independent facade"), Some(copy));
    assert_eq!(reopened.model.section(copy), Some(&expected));
    assert_eq!(reopened.model.section(source_guid), Some(&original));
    assert_eq!(reopened.model.elements.len(), s.model.elements.len());
    reopened.execute("scale", &json!({"view": copy, "scale": 50})).unwrap();
    assert_eq!(reopened.model.section(source_guid), Some(&original));
}

#[test]
fn duplicate_elevation_renders_changes_to_the_same_building() {
    let (mut s, source) = north();
    let copy = s.execute("duplicate_view", &json!({"view": source})).unwrap()["id"].as_u64().unwrap();
    let view = View { angle: 0.0, scale: 0.025, cx: 5000.0, cy: 2000.0, width: 256, height: 128 };
    let picture = |s: &Session, id| render::section_picture(&s.model, id, view, &[]).unwrap().rgba;
    let before = picture(&s, source);
    assert_eq!(picture(&s, copy), before);
    let wall = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).unwrap().0;
    s.execute("modify", &json!({"id": wall, "height": 4000})).unwrap();
    let after = picture(&s, source);
    assert!(after != before, "the source must reflect the model edit");
    assert_eq!(picture(&s, copy), after, "the copy sees the same changed wall");
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Wall(_))).count(), 1);
}
