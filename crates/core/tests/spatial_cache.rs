//! Persistent render work must include every geometry dependency in its identity.
use bimcraft_core::engine::Session;
use bimcraft_core::model::Element;
use bimcraft_core::render::{self, SectionWork};

#[test]
fn section_work_refreshes_after_a_type_edit_without_any_element_mutation() {
    let mut session = Session::new();
    session
        .script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
section {"start": [-1000,3000], "end": [9000,3000]}"#,
        )
        .unwrap();
    let id = session.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Section(_)).then_some(id)).unwrap();
    let mut model = (*session.model).clone();
    let view = render::section_fit(&model, id, 400, 300);
    let work = SectionWork::new(&model);
    let before = render::section_picture_and_over_with(&model, &work, id, view, view, &[]);
    let state = model.elements.state();
    let type_name = model
        .elements
        .iter()
        .find_map(|(_, e)| match e {
            Element::Wall(w) => Some(w.type_name.clone()),
            _ => None,
        })
        .unwrap();
    model.types.wall.get_mut(&type_name).unwrap().thickness += 500.0;
    assert_eq!(model.elements.state(), state, "this fixture must exercise a library-only edit");
    let fresh = render::section_picture_and_over(&model, id, view, view, &[]);
    assert!(render::png(&before.1).unwrap() != render::png(&fresh.1).unwrap(), "the edit must affect the visible cut");
    let reused = render::section_picture_and_over_with(&model, &work, id, view, view, &[]);
    assert!(render::png(&reused.1).unwrap() == render::png(&fresh.1).unwrap(), "cached work missed a library-only type edit");
    let (reused, fresh) = (reused.0.unwrap(), fresh.0.unwrap());
    assert!(reused.rgba == fresh.rgba, "cached pixels differ after a library-only edit");
    for y in 0..fresh.height {
        for x in 0..fresh.width {
            let (x, y) = (f64::from(x), f64::from(y));
            assert_eq!((reused.pick(x, y), reused.face(x, y)), (fresh.pick(x, y), fresh.face(x, y)));
        }
    }
}

#[test]
fn section_framing_refreshes_after_an_in_place_family_edit() {
    let mut session = Session::new();
    session
        .script(
            r#"component {"family":"Desk","type":"1400 x 700","at":[0,0]}
section {"start":[-2000,-2000],"end":[2000,-2000]}"#,
        )
        .unwrap();
    let id = session.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Section(_)).then_some(id)).unwrap();
    let mut model = (*session.model).clone();
    let work = SectionWork::new(&model);
    let before = render::section_fit_with(&model, &work, id, 400, 300);
    let state = model.elements.state();
    let desk = model.families.get_mut("Desk").unwrap();
    desk.parameters.iter_mut().find(|p| p.name == "Height").unwrap().value = 6000_i64.into();
    assert_eq!(state, model.elements.state());
    let fresh = render::section_fit(&model, id, 400, 300);
    assert_ne!(fresh, before, "the family's changed extrema must affect framing");
    assert_eq!(render::section_fit_with(&model, &work, id, 400, 300), fresh);
    let cached = render::section_picture_cancellable(&model, &work, id, fresh, &[], &|| false).unwrap();
    assert!(cached.rgba == render::section_picture(&model, id, fresh, &[]).unwrap().rgba);
}

#[test]
fn section_framing_refreshes_after_cad_data_changes_without_element_edits() {
    use bimcraft_core::cad::{Cad, Colors};
    use bimcraft_core::geom::P2;
    use bimcraft_core::model::CadImport;
    let cad = |span| {
        Cad::new(
            "Envelope probe",
            None,
            format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n{span}\n21\n1000\n0\nENDSEC\n0\nEOF\n").into_bytes(),
        )
        .unwrap()
    };
    let mut session = Session::new();
    session.script(r#"section {"start":[-2000,-2000],"end":[2000,-2000]}"#).unwrap();
    let section = session.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Section(_)).then_some(id)).unwrap();
    let mut m = (*session.model).clone();
    m.cads.insert("Probe".into(), cad(1000));
    m.insert(Element::CadImport(CadImport {
        cad: "Probe".into(),
        level: m.guid(100000),
        at: P2(0.0, 0.0),
        origin: P2(0.0, 0.0),
        scale: 1.0,
        angle: 0.0,
        view: None,
        hidden: Default::default(),
        colors: Colors::Preserve,
    }));
    let work = SectionWork::new(&m);
    let before = render::section_fit_with(&m, &work, section, 400, 300);
    let state = m.elements.state();
    m.cads.insert("Probe".into(), cad(50000));
    assert_eq!(m.elements.state(), state);
    let fresh = render::section_fit(&m, section, 400, 300);
    assert_ne!(fresh, before, "changed CAD bulk bounds must affect the level extent");
    assert_eq!(render::section_fit_with(&m, &work, section, 400, 300), fresh);
}

#[test]
fn section_framing_refreshes_after_cloud_data_changes_without_element_edits() {
    use bimcraft_core::cloud::{Cloud, Scan};
    use bimcraft_core::geom::P2;
    use bimcraft_core::model::PointCloud;
    let cloud = |span| {
        Cloud::new("Envelope probe", Scan { origin: [0.0; 3], points: vec![[0.0; 3], [span, 1000.0, 0.0]], colours: None, read: 2 })
            .unwrap()
    };
    let mut session = Session::new();
    session.script(r#"section {"start":[-2000,-2000],"end":[2000,-2000]}"#).unwrap();
    let section = session.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Section(_)).then_some(id)).unwrap();
    let mut m = (*session.model).clone();
    m.clouds.insert("Probe".into(), cloud(1000.0));
    m.insert(Element::PointCloud(PointCloud { cloud: "Probe".into(), level: m.guid(100000), at: P2(0.0, 0.0), offset: 0.0, angle: 0.0 }));
    let work = SectionWork::new(&m);
    let before = render::section_fit_with(&m, &work, section, 400, 300);
    let state = m.elements.state();
    m.clouds.insert("Probe".into(), cloud(50000.0));
    assert_eq!(m.elements.state(), state);
    let fresh = render::section_fit(&m, section, 400, 300);
    assert_ne!(fresh, before, "changed cloud bounds must affect the level extent");
    assert_eq!(render::section_fit_with(&m, &work, section, 400, 300), fresh);
}

#[test]
fn definition_snapshots_share_until_edited_and_keep_the_existing_file_shape() {
    let model = bimcraft_core::model::Model::new("Shared definitions");
    let mut edited = model.clone();
    assert!(model.types.same_allocation(&edited.types));
    assert!(model.materials.same_allocation(&edited.materials));
    assert!(model.families.same_allocation(&edited.families));
    let name = model.types.wall.keys().next().unwrap().clone();
    let before = model.types.wall[&name].thickness;
    edited.types.wall.get_mut(&name).unwrap().thickness += 100.0;
    assert_eq!(model.types.wall[&name].thickness, before);
    assert!(!model.types.same_allocation(&edited.types));
    assert!(model.families.same_allocation(&edited.families));
    assert_eq!(serde_json::to_value(&model.types).unwrap(), serde_json::to_value(&*model.types).unwrap());
    assert_eq!(serde_json::to_value(&model.materials).unwrap(), serde_json::to_value(&*model.materials).unwrap());
    assert_eq!(serde_json::to_value(&model.families).unwrap(), serde_json::to_value(&*model.families).unwrap());
    let text = model.to_json().unwrap();
    let loaded = bimcraft_core::model::Model::from_json(&text).unwrap();
    loaded.check().unwrap();
    assert_eq!(text, loaded.to_json().unwrap());
}

#[test]
fn type_edits_undo_redo_and_branches_keep_independent_definition_identities() {
    let mut session = Session::new();
    session
        .script(
            r#"wall {"points":[[0,0],[8000,0]]}
section {"start":[-1000,3000],"end":[9000,3000]}"#,
        )
        .unwrap();
    let before = session.model.clone();
    let name = before.elements.iter().find_map(|(_, e)| if let Element::Wall(w) = e { Some(w.type_name.clone()) } else { None }).unwrap();
    session.script(&format!("type {{\"category\":\"wall\",\"name\":{name:?},\"thickness\":700}}")).unwrap();
    let changed = session.model.clone();
    assert!(!before.types.same_allocation(&changed.types));
    session.script("undo {}").unwrap();
    assert!(before.types.same_allocation(&session.model.types));
    session.script("redo {}").unwrap();
    assert!(changed.types.same_allocation(&session.model.types));
    session.script("undo {}").unwrap();
    session.script(&format!("type {{\"category\":\"wall\",\"name\":{name:?},\"thickness\":900}}")).unwrap();
    assert_eq!(session.model.types.wall[&name].thickness, 900.0);
    assert_eq!(changed.types.wall[&name].thickness, 700.0);
    assert!(!changed.types.same_allocation(&session.model.types));
}

#[test]
fn a_local_annotation_edit_keeps_all_geometry_definitions_shared() {
    let mut session = Session::new();
    session
        .script(
            r#"wall {"points":[[0,0],[8000,0]]}
section {"start":[-1000,3000],"end":[9000,3000]}"#,
        )
        .unwrap();
    let id = session.model.elements.iter().find_map(|(&id, e)| matches!(e, Element::Section(_)).then_some(id)).unwrap();
    let before = session.model.clone();
    session.script(&format!("text {{\"view\":{id},\"at\":[2000,2000],\"text\":\"Note\"}}")).unwrap();
    assert!(before.types.same_allocation(&session.model.types), "annotation copied type definitions");
    assert!(before.materials.same_allocation(&session.model.materials), "annotation copied material definitions");
    assert!(before.families.same_allocation(&session.model.families), "annotation copied family definitions");
}
