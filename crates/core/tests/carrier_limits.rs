use bimcraft_core::carrier_limits::{Enclosure, Profile, Section, capacity};

fn section(enclosure: Enclosure) -> Section {
    Section { enclosure: Some(enclosure), clear_width: Some(80.0), clear_depth: Some(40.0) }
}

#[test]
fn project_percentage_preserves_legacy_gross_section_calculation() {
    assert_eq!(capacity(Profile::Project, 60.0, 5000.0, None).unwrap(), 3000.0);
}

#[test]
fn russian_box_limits_use_measured_internal_section() {
    for (enclosure, expected) in [(Enclosure::BlindBox, 1120.0), (Enclosure::LiddedBox, 1280.0)] {
        let data = section(enclosure);
        for project_percent in [1.0, 100.0] {
            assert_eq!(capacity(Profile::RuSp76_2016MaxFill, project_percent, 5000.0, Some(&data)).unwrap(), expected);
        }
    }
}

#[test]
fn box_rule_does_not_guess_tray_ladder_or_conduit_limits() {
    for enclosure in [Enclosure::Tray, Enclosure::Ladder, Enclosure::Conduit] {
        let error = capacity(Profile::RuSp76_2016MaxFill, 100.0, 5000.0, Some(&section(enclosure))).unwrap_err();
        assert!(error.contains("not applicable"), "{error}");
    }
}

#[test]
fn unknown_internal_dimensions_or_enclosure_are_not_usable_capacity() {
    let complete = section(Enclosure::LiddedBox);
    for data in [
        None,
        Some(Section { enclosure: None, ..complete.clone() }),
        Some(Section { clear_width: None, ..complete.clone() }),
        Some(Section { clear_depth: None, ..complete }),
    ] {
        let error = capacity(Profile::RuSp76_2016MaxFill, 100.0, 5000.0, data.as_ref()).unwrap_err();
        assert!(error.contains("missing"), "{error}");
    }
}

#[test]
fn invalid_dimensions_and_capacity_inputs_never_produce_a_number() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, f64::MAX] {
        for data in [
            Section { clear_width: Some(bad), ..section(Enclosure::BlindBox) },
            Section { clear_depth: Some(bad), ..section(Enclosure::BlindBox) },
        ] {
            assert!(capacity(Profile::RuSp76_2016MaxFill, 100.0, 5000.0, Some(&data)).is_err());
        }
        assert!(capacity(Profile::Project, bad, 5000.0, None).is_err());
        assert!(capacity(Profile::Project, 100.0, bad, None).is_err());
    }
    assert!(capacity(Profile::Project, 100.01, 5000.0, None).is_err());
    assert!(capacity(Profile::RuSp76_2016MaxFill, 100.0, 1000.0, Some(&section(Enclosure::BlindBox))).is_err());
}

#[test]
fn profile_and_section_have_strict_versioned_file_representation() {
    assert_eq!(serde_json::to_value(Profile::RuSp76_2016MaxFill).unwrap(), "ru_sp76_2016_max_fill");
    assert!(serde_json::from_str::<Profile>("\"ru_sp76\"").is_err());
    assert!(serde_json::from_str::<Section>(r#"{"clear_wdth":80}"#).is_err());
    let data = section(Enclosure::LiddedBox);
    assert_eq!(serde_json::from_value::<Section>(serde_json::to_value(&data).unwrap()).unwrap(), data);
}

#[test]
fn measured_product_section_is_command_owned_and_survives_save_undo_redo() {
    use bimcraft_core::engine::Session;
    use serde_json::json;
    let mut s = Session::new();
    s.execute("type", &json!({"category":"cablecarrier","name":"Measured box","width":100,"depth":50})).unwrap();
    let before = s.model.to_json().unwrap();
    let data = json!({"enclosure":"lidded_box","clear_width":80.0,"clear_depth":40.0});
    s.execute("carrier_section", &json!({"type":"Measured box","section":data})).unwrap();
    let after = s.model.to_json().unwrap();
    let loaded = bimcraft_core::model::Model::read(&after).unwrap();
    let entry = loaded.0.library.entries().find(|(_, e)| e.name == "Measured box").unwrap().1;
    assert_eq!(serde_json::to_value(entry.product.as_ref().unwrap()).unwrap()["carrier"], data);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.to_json().unwrap(), before);
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(s.model.to_json().unwrap(), after);
    s.execute("carrier_section", &json!({"type":"Measured box","section":null})).unwrap();
    assert!(s.model.library.entries().find(|(_, e)| e.name == "Measured box").unwrap().1.product.is_none());
}

#[test]
fn bad_measured_section_rolls_back_the_entire_command() {
    use bimcraft_core::engine::Session;
    use serde_json::json;
    let mut s = Session::new();
    s.execute("type", &json!({"category":"cablecarrier","name":"Measured box","width":100,"depth":50})).unwrap();
    let before = s.model.to_json().unwrap();
    for data in [
        json!({"clear_width":101}),
        json!({"clear_depth":51}),
        json!({"clear_width":0}),
        json!({"clear_depth":-1}),
        json!({"clear_width":1e20}),
        json!({"clear_wdth":80}),
    ] {
        assert!(s.execute("carrier_section", &json!({"type":"Measured box","section":data})).is_err());
        assert_eq!(s.model.to_json().unwrap(), before);
    }
}

#[test]
fn ods_exports_and_restores_measured_carrier_product_data() {
    use bimcraft_core::{engine::Session, ods};
    use serde_json::json;
    let mut s = Session::new();
    s.execute("type", &json!({"category":"cablecarrier","name":"Measured box","width":100,"depth":50})).unwrap();
    s.execute("carrier_section", &json!({"type":"Measured box","section":section(Enclosure::LiddedBox)})).unwrap();
    let rows = ods::catalogue_rows(&s.model);
    assert_eq!(&rows[0][13..], ["CarrierEnclosure", "ClearWidth_mm", "ClearDepth_mm"]);
    let row = rows.iter().find(|row| row.get(2).is_some_and(|name| name == "Measured box")).unwrap();
    assert_eq!(&row[13..], ["lidded_box", "80", "40"]);
    let entry = |s: &Session| s.model.library.entries().find(|(_, e)| e.name == "Measured box").unwrap().1.clone();
    let before = entry(&s);
    s.execute("carrier_section", &json!({"type":"Measured box","section":null})).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    assert_eq!(entry(&s), before);
    let mut equivalent = rows;
    let row = equivalent.iter_mut().find(|row| row.get(2).is_some_and(|name| name == "Measured box")).unwrap();
    row[0].clear();
    row[14] = "80.000".into();
    row[15] = "40.0".into();
    std::fs::write(&path, ods::write(&equivalent).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    assert_eq!(entry(&s), before, "equivalent numeric dimensions must reuse the existing identity");
}

#[test]
fn legacy_ods_headers_preserve_measurements_but_new_explicit_blanks_clear_them() {
    use bimcraft_core::{engine::Session, ods};
    use serde_json::json;
    let mut s = Session::new();
    s.execute("type", &json!({"category":"cablecarrier","name":"Measured box","width":100,"depth":50})).unwrap();
    s.execute("carrier_section", &json!({"type":"Measured box","section":section(Enclosure::LiddedBox)})).unwrap();
    let full = ods::catalogue_rows(&s.model);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalogue.ods");
    for width in [7, 12, 13] {
        let mut rows = full.clone();
        for row in &mut rows {
            row.truncate(width);
        }
        std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
        s.execute("catalogue_import", &json!({"path":path})).unwrap();
        let entry = s.model.library.entries().find(|(_, e)| e.name == "Measured box").unwrap().1;
        assert_eq!(
            serde_json::to_value(entry.product.as_ref().unwrap()).unwrap()["carrier"],
            serde_json::to_value(section(Enclosure::LiddedBox)).unwrap()
        );
    }
    let mut rows = full;
    for row in rows.iter_mut().skip(1) {
        row.resize(16, String::new());
        row[13..].fill(String::new());
    }
    rows[0].resize(16, String::new());
    rows[0][13..].clone_from_slice(&["CarrierEnclosure".into(), "ClearWidth_mm".into(), "ClearDepth_mm".into()]);
    std::fs::write(&path, ods::write(&rows).unwrap()).unwrap();
    s.execute("catalogue_import", &json!({"path":path})).unwrap();
    assert!(s.model.library.entries().find(|(_, e)| e.name == "Measured box").unwrap().1.product.is_none());
}

#[test]
fn ods_uses_stored_floats_for_measured_sections_in_localized_sheets() {
    use std::io::{Cursor, Write};
    let xml = r#"<table xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0">
      <table-row><table-cell><p>ClearWidth_mm</p></table-cell><table-cell><p>ClearDepth_mm</p></table-cell></table-row>
      <table-row><table-cell office:value-type="float" office:value="80.5"><p>80,5</p></table-cell>
      <table-cell office:value-type="float" office:value="40.25"><p>40,25</p></table-cell></table-row></table>"#;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
    archive.write_all(xml.as_bytes()).unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    assert_eq!(bimcraft_core::ods::read(&bytes).unwrap()[1], ["80.5", "40.25"]);
}

#[test]
fn configured_box_profile_controls_warning_persistence_and_unknown_data() {
    use bimcraft_core::{engine::Session, model::Model};
    use serde_json::json;
    let mut s = Session::new();
    s.execute("type", &json!({"category":"cablecarrier","name":"Norm box","width":100,"depth":50,"round":false})).unwrap();
    s.execute("carrier_section", &json!({"type":"Norm box","section":{"enclosure":"blind_box","clear_width":80,"clear_depth":40}}))
        .unwrap();
    s.execute("type", &json!({"category":"wire","name":"Norm cable","width":38,"depth":38,"round":true})).unwrap();
    s.execute("cable_carrier", &json!({"type":"Norm box","points":[[0,0],[3000,0]]})).unwrap();
    let host = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    s.execute("wire", &json!({"type":"Norm cable","points":[[0,0],[3000,0]],"carrier":host})).unwrap();
    assert!(s.model.carrier_warnings().is_empty());
    let reply = s
        .execute("cable_carrier_check", &json!({"mode":"warn","fill_percent":100,"configure":true,"profile":"ru_sp76_2016_max_fill"}))
        .unwrap();
    assert!(!reply["warnings"].as_array().unwrap().is_empty());
    assert!(!reply["elements"].as_array().unwrap().is_empty());
    assert!(s.model.carrier_warnings().iter().any(|w| w.contains("exceeds")));
    let (loaded, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(loaded.carrier_settings, s.model.carrier_settings);
    assert_eq!(loaded.carrier_warnings(), s.model.carrier_warnings());
    s.execute("undo", &json!({})).unwrap();
    assert!(s.model.carrier_settings.is_none());
    s.execute("redo", &json!({})).unwrap();
    s.execute("carrier_section", &json!({"type":"Norm box","section":{"enclosure":"lidded_box","clear_width":80,"clear_depth":40}}))
        .unwrap();
    assert!(s.model.carrier_warnings().is_empty(), "40% of 3200 fits 38mm cable");
    let reply = s.execute("carrier_section", &json!({"type":"Norm box","section":null})).unwrap();
    assert!(reply["carrier_check"]["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("missing"))));
    assert!(s.model.carrier_warnings().iter().any(|w| w.contains("missing")));
}

#[test]
fn normative_autosize_requires_known_capacity_same_maker_and_enclosure() {
    use bimcraft_core::{
        engine::Session,
        library::{Kind, Product},
    };
    use serde_json::json;
    let mut s = Session::new();
    for (name, width, depth, maker, enclosure, clear_width, clear_depth) in [
        ("Original", 100, 50, "Maker A", Some("blind_box"), 80, 40),
        ("Unknown", 110, 60, "Maker A", None, 0, 0),
        ("Thin interior", 150, 75, "Maker A", Some("blind_box"), 40, 40),
        ("Wrong enclosure", 110, 60, "Maker A", Some("lidded_box"), 90, 50),
        ("Wrong maker", 110, 60, "Maker B", Some("blind_box"), 90, 50),
        ("Narrow interior", 150, 100, "Maker A", Some("blind_box"), 50, 80),
        ("Sufficient", 200, 100, "Maker A", Some("blind_box"), 80, 80),
    ] {
        s.execute("type", &json!({"category":"cablecarrier","name":name,"width":width,"depth":depth,"round":false})).unwrap();
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: maker.into(), ..Default::default() }))
            .unwrap();
        if let Some(enclosure) = enclosure {
            s.execute(
                "carrier_section",
                &json!({"type":name,"section":{"enclosure":enclosure,"clear_width":clear_width,"clear_depth":clear_depth}}),
            )
            .unwrap();
        }
    }
    for start in [0, 3000, 6000] {
        s.execute("cable_carrier", &json!({"type":"Original","points":[[start,0],[start+3000,0]]})).unwrap();
    }
    let hosts: Vec<_> = s.model.elements.iter().filter(|(_, e)| e.category() == "cablecarrier").map(|(id, _)| *id).collect();
    s.execute("modify", &json!({"id":hosts[2],"size_locked":true})).unwrap();
    s.execute("type", &json!({"category":"wire","name":"Norm cable","width":38,"depth":38,"round":true})).unwrap();
    s.execute("wire", &json!({"type":"Norm cable","points":[[0,0],[3000,0]],"carrier":hosts[0]})).unwrap();
    s.execute("modify", &json!({"id":hosts[0],"compartments":{"Main":{"width":60,"depth":40,"offset":0,"systems":[]}}})).unwrap();
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":wire,"compartment":"Main"})).unwrap();
    let before = s.model.elements.clone();
    let reply = s
        .execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100,"configure":true,"profile":"ru_sp76_2016_max_fill"}))
        .unwrap();
    assert_eq!(reply["changed"].as_array().unwrap().len(), 2);
    for host in &hosts[..2] {
        assert_eq!(s.model.elements.get(*host).unwrap().run().unwrap().type_name, "Sufficient");
    }
    assert_eq!(s.model.elements.get(hosts[2]).unwrap().run().unwrap().type_name, "Original");
    assert!(s.model.carrier_warnings().is_empty());
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements, before);
    s.execute("redo", &json!({})).unwrap();
    assert!(s.model.carrier_warnings().is_empty());
}

#[test]
fn closed_ods_reopen_uses_persisted_internal_box_limit() {
    use bimcraft_core::{engine::Session, ods};
    use serde_json::json;
    let mut s = Session::new();
    s.execute("type", &json!({"category":"cablecarrier","name":"Norm box","width":100,"depth":50,"round":false})).unwrap();
    s.execute("carrier_section", &json!({"type":"Norm box","section":{"enclosure":"blind_box","clear_width":80,"clear_depth":40}}))
        .unwrap();
    s.execute("type", &json!({"category":"wire","name":"Norm cable","width":28,"depth":28,"round":true})).unwrap();
    s.execute("cable_carrier", &json!({"type":"Norm box","points":[[0,0],[3000,0]]})).unwrap();
    let host = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    s.execute("wire", &json!({"type":"Norm cable","points":[[0,0],[3000,0]],"carrier":host})).unwrap();
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":wire,"start":[0,-16],"end":[3000,-16]})).unwrap();
    s.execute("wire", &json!({"type":"Norm cable","points":[[0,16],[3000,16]],"carrier":host})).unwrap();
    s.execute("carrier_section", &json!({"type":"Norm box","section":{"enclosure":"lidded_box","clear_width":80,"clear_depth":40}}))
        .unwrap();
    s.execute("cable_carrier_check", &json!({"mode":"warn","fill_percent":100,"configure":true,"profile":"ru_sp76_2016_max_fill"}))
        .unwrap();
    assert!(s.model.carrier_warnings().is_empty());
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("norm.ods");
    let project = temp.path().join("norm.bimcraft");
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("save", &json!({"path":project})).unwrap();
    let elements = s.model.elements.clone();
    drop(s);
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|r| r[2] == "Norm cable").unwrap();
    row[3] = "30".into();
    row[4] = "30".into();
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let mut opened = Session::new();
    let reply = opened.execute("open", &json!({"path":project})).unwrap();
    assert_eq!(opened.model.elements, elements);
    assert_eq!(reply["carrier_check"]["elements"].as_array().unwrap().len(), 1);
    assert!(opened.model.carrier_warnings().iter().any(|w| w.contains("exceeds allowed section 1280.0")));
    assert!(!opened.model.carrier_warnings().iter().any(|w| w.contains("geometric conflict")));
}

#[test]
fn legacy_carrier_policy_migrates_to_explicit_project_profile() {
    use bimcraft_core::{
        engine::Session,
        model::{FORMAT, Model},
    };
    use serde_json::json;
    let mut s = Session::new();
    s.execute("cable_carrier_check", &json!({"configure":true,"fill_percent":60,"mode":"warn"})).unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(12);
    file["carrier_settings"].as_object_mut().unwrap().remove("profile");
    let (loaded, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty());
    let policy = loaded.carrier_settings.as_ref().unwrap();
    assert_eq!(policy.profile, Profile::Project);
    assert_eq!(policy.fill_percent, 60.0);
    assert_eq!(loaded.format, FORMAT);
    const { assert!(FORMAT >= 13, "older readers must refuse a selected normative profile") };
}

#[test]
fn measured_compartment_conflict_warns_and_autosizes_even_when_cable_fits() {
    use bimcraft_core::{
        engine::Session,
        library::{Kind, Product},
        model::Model,
    };
    use serde_json::json;
    for (with_cable, width, depth, offset) in [(false, 60, 40, 0), (true, 60, 40, 0), (true, 40, 40, 10), (true, 40, 45, 0)] {
        let mut s = Session::new();
        for (name, clear_width, clear_depth) in [("Narrow cavity", 50, 40), ("Wide cavity", 80, 48)] {
            s.execute("type", &json!({"category":"cablecarrier","name":name,"width":100,"depth":50,"round":false})).unwrap();
            s.model_mut()
                .library
                .set_product(
                    &Kind::Type("cablecarrier".into()),
                    name,
                    Some(Product { manufacturer: "Maker A".into(), ..Default::default() }),
                )
                .unwrap();
            s.execute(
                "carrier_section",
                &json!({"type":name,"section":{"enclosure":"blind_box","clear_width":clear_width,"clear_depth":clear_depth}}),
            )
            .unwrap();
        }
        s.execute("cable_carrier", &json!({"type":"Narrow cavity","points":[[0,0],[3000,0]]})).unwrap();
        let host = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
        s.execute("modify", &json!({"id":host,"compartments":{"Main":{"width":width,"depth":depth,"offset":offset,"systems":[]}}}))
            .unwrap();
        if with_cable {
            s.execute("type", &json!({"category":"wire","name":"Small cable","width":10,"depth":10,"round":true})).unwrap();
            s.execute("wire", &json!({"type":"Small cable","points":[[0,0],[3000,0]],"carrier":host,"compartment":"Main"})).unwrap();
        }
        let reply = s
            .execute("cable_carrier_check", &json!({"mode":"warn","fill_percent":100,"configure":true,"profile":"ru_sp76_2016_max_fill"}))
            .unwrap();
        assert!(
            !reply["warnings"].as_array().unwrap().is_empty(),
            "measured compartment cannot fit the cavity even with no cables or a small cable"
        );
        assert!(!reply["elements"].as_array().unwrap().is_empty());
        assert!(s.model.carrier_warnings().iter().any(|w| w.contains("compartment") && w.contains("geometric conflict")));
        let (loaded, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
        assert_eq!(loaded.carrier_warnings(), s.model.carrier_warnings());
        let before = s.model.elements.clone();
        let reply = s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
        assert_eq!(reply["changed"].as_array().unwrap().len(), 1);
        assert_eq!(s.model.elements.get(host).unwrap().run().unwrap().type_name, "Wide cavity");
        assert!(s.model.carrier_warnings().is_empty());
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.model.elements, before);
        s.execute("redo", &json!({})).unwrap();
        assert!(s.model.carrier_warnings().is_empty());
    }
}
