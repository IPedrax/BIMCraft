//! Quantities seen by a consumer after commands, native save/reopen and IFC export.
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bimcraft_core::engine::Session;
    use bimcraft_core::step::{self, V};
    use serde_json::json;

    fn exported(script: &str) -> step::File {
        let mut session = Session::new();
        session.script(script).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("quantities.bimcraft");
        let ifc = dir.path().join("quantities.ifc");
        session.execute("save", &json!({"path":native})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path":native})).unwrap();
        reopened.execute("save", &json!({"path":ifc})).unwrap();
        step::parse(&std::fs::read(ifc).unwrap()).unwrap()
    }

    fn quantities(f: &step::File, class: &str, name: &str) -> BTreeMap<String, f64> {
        let product = f.entities.iter().find(|(_, e)| e.name == class).unwrap().0;
        let set = f.entities.values().find_map(|r| {
            if r.name != "IFCRELDEFINESBYPROPERTIES" || r.args[4] != V::List(vec![V::Ref(*product)]) {
                return None;
            }
            let V::Ref(id) = r.args[5] else { return None };
            let set = &f.entities[&id];
            (set.name == "IFCELEMENTQUANTITY" && set.args[2] == V::Str(name.into())).then_some(set)
        });
        let set = set.unwrap_or_else(|| panic!("{class} is missing {name}"));
        let V::List(qs) = &set.args[5] else { panic!("quantity list") };
        qs.iter()
            .map(|q| {
                let V::Ref(id) = q else { panic!("quantity reference") };
                let q = &f.entities[id];
                let V::Str(name) = &q.args[0] else { panic!("quantity name") };
                let value = match q.args[3] {
                    V::Real(v) => v,
                    V::Int(v) => v as f64,
                    _ => panic!("quantity value"),
                };
                (name.clone(), value)
            })
            .collect()
    }

    fn near(q: &BTreeMap<String, f64>, name: &str, expected: f64) {
        let actual = q.get(name).unwrap_or_else(|| panic!("missing {name}: {q:?}"));
        // STEP's existing real-number writer rounds to six decimal places.
        assert!((actual - expected).abs() < 0.000_000_51, "{name}: {actual}, expected {expected}");
    }

    #[test]
    fn ceiling_areas_subtract_openings_and_only_intersecting_shafts() {
        let f = exported(
            r#"
type {"category":"ceiling","name":"Test ceiling","thickness":20}
ceiling {"type":"Test ceiling","boundary":[[0,0],[4000,0],[4000,3000],[0,3000]],"openings":[[[500,500],[1500,500],[1500,1500],[500,1500]]]}
shaft {"boundary":[[2000,500],[3000,500],[3000,1500],[2000,1500]],"height":5000}
shaft {"boundary":[[2000,2000],[3000,2000],[3000,2500],[2000,2500]],"height":1000}
"#,
        );
        let q = quantities(&f, "IFCCOVERING", "Qto_CoveringBaseQuantities");
        near(&q, "Width", 20.0);
        near(&q, "GrossArea", 12.0);
        near(&q, "NetArea", 10.0);
    }

    #[test]
    fn roof_areas_follow_the_pitch_and_subtract_openings_and_shafts() {
        let f = exported(
            r#"
roof {"level":100001,"boundary":[[0,0],[4000,0],[4000,3000],[0,3000]],"slope":30,"overhang":0,"openings":[[[500,500],[1500,500],[1500,1500],[500,1500]]]}
shaft {"boundary":[[2500,500],[3500,500],[3500,1500],[2500,1500]],"height":10000}
"#,
        );
        let q = quantities(&f, "IFCROOF", "Qto_RoofBaseQuantities");
        near(&q, "GrossArea", 12.0 / 30_f64.to_radians().cos());
        near(&q, "NetArea", 10.0 / 30_f64.to_radians().cos());
        near(&q, "ProjectedArea", 12.0);
    }

    #[test]
    fn shaped_variable_layer_roof_areas_follow_the_top_planes() {
        let f = exported(
            r#"
type {"category":"roof","name":"Variable roof","layers":[{"function":"substrate","thickness":200,"variable":true}]}
roof {"level":100001,"type":"Variable roof","boundary":[[0,0],[4000,0],[4000,4000],[0,4000]],"slope":0,"overhang":0}
edit_shape {"id":100002,"points":[{"at":[2000,2000],"z":-100}]}
"#,
        );
        let q = quantities(&f, "IFCROOF", "Qto_RoofBaseQuantities");
        let area = 16.0 * (1.0_f64 + 0.05 * 0.05).sqrt();
        near(&q, "GrossArea", area);
        near(&q, "NetArea", area);
        near(&q, "ProjectedArea", 16.0);
    }

    #[test]
    fn roof_courtyard_is_absent_from_gross_and_projected_areas() {
        let f = exported(
            r#"
roof {"boundary":[[0,0],[4000,0],[4000,3000],[0,3000]],"slope":0,"overhang":0,"courtyards":[[[1000,1000],[2000,1000],[2000,2000],[1000,2000]]]}
"#,
        );
        let q = quantities(&f, "IFCROOF", "Qto_RoofBaseQuantities");
        for n in ["GrossArea", "NetArea", "ProjectedArea"] {
            near(&q, n, 11.0);
        }
    }

    #[test]
    fn pad_footing_dimensions_and_volume_match_its_rectangular_solid() {
        let f = exported(
            r#"
type {"category":"footing","name":"Test pad","width":1200,"depth":1000}
footing {"type":"Test pad","at":[0,0],"thickness":400,"offset":-200,"angle":30}
"#,
        );
        let q = quantities(&f, "IFCFOOTING", "Qto_FootingBaseQuantities");
        near(&q, "Length", 1200.0);
        near(&q, "Width", 1000.0);
        near(&q, "Height", 400.0);
        near(&q, "GrossVolume", 0.48);
        near(&q, "NetVolume", 0.48);
    }

    #[test]
    fn round_pad_uses_the_circle_area_and_strip_uses_its_run_length() {
        let f = exported(
            r#"
type {"category":"footing","name":"Round pad","width":1000,"round":true}
footing {"type":"Round pad","at":[0,0],"thickness":400}
"#,
        );
        let q = quantities(&f, "IFCFOOTING", "Qto_FootingBaseQuantities");
        near(&q, "Width", 1000.0);
        near(&q, "GrossVolume", std::f64::consts::PI * 0.1);
        near(&q, "NetVolume", std::f64::consts::PI * 0.1);
        let f = exported(
            r#"
type {"category":"stripfooting","name":"Test strip","width":600,"depth":300}
strip_footing {"type":"Test strip","points":[[0,0],[4000,0]]}
"#,
        );
        let q = quantities(&f, "IFCFOOTING", "Qto_FootingBaseQuantities");
        near(&q, "Length", 4000.0);
        near(&q, "Width", 600.0);
        near(&q, "Height", 300.0);
        near(&q, "CrossSectionArea", 0.18);
        near(&q, "GrossVolume", 0.72);
        near(&q, "NetVolume", 0.72);
    }

    #[test]
    fn extruded_roof_projected_area_comes_from_its_profile_instead_of_its_boundary() {
        let f = exported(
            r#"
roof {"level":100001,"extrusion":{"line":[[0,0],[4000,0]],"profile":[[0,0],[2000,1000],[4000,0]],"from":0,"to":3000}}
"#,
        );
        let q = quantities(&f, "IFCROOF", "Qto_RoofBaseQuantities");
        near(&q, "ProjectedArea", 12.0);
        near(&q, "GrossArea", 12.0 * 1.25_f64.sqrt());
        near(&q, "NetArea", 12.0 * 1.25_f64.sqrt());
    }

    #[test]
    fn wall_base_attached_to_a_slope_subtracts_only_the_door_above_its_base() {
        let f = exported(
            r#"
type {"category":"wall","name":"Test wall","thickness":200}
type {"category":"door","name":"Test door","width":1000,"height":2000}
floor {"boundary":[[-500,-500],[4500,-500],[4500,500],[-500,500]],"slope":14.036243467926479,"arrow":[[0,0],[4000,0]]}
wall {"type":"Test wall","points":[[0,0],[4000,0]],"height":6000,"attach_base":100002}
door {"type":"Test door","wall":100003,"offset":1000}
"#,
        );
        let q = quantities(&f, "IFCWALL", "Qto_WallBaseQuantities");
        near(&q, "GrossSideArea", 22.0);
        near(&q, "NetSideArea", 20.25);
        near(&q, "GrossVolume", 4.4);
        near(&q, "NetVolume", 4.05);
        assert!(!q.contains_key("Height"));
    }

    #[test]
    fn wall_attached_across_a_roof_ridge_measures_both_planes() {
        let f = exported(
            r#"
type {"category":"wall","name":"Test wall","thickness":200}
type {"category":"door","name":"Test door","width":1000,"height":2000}
roof {"level":100001,"extrusion":{"line":[[0,0],[4000,0]],"profile":[[0,0],[2000,1000],[4000,0]],"from":0,"to":1000}}
wall {"type":"Test wall","points":[[0,500],[4000,500]],"attach_top":100002}
door {"type":"Test door","wall":100003,"offset":1000}
"#,
        );
        let q = quantities(&f, "IFCWALL", "Qto_WallBaseQuantities");
        near(&q, "GrossSideArea", 14.0);
        near(&q, "NetSideArea", 12.0);
        near(&q, "GrossVolume", 2.8);
        near(&q, "NetVolume", 2.4);
        assert!(!q.contains_key("Height"));
    }

    #[test]
    fn attached_wall_quantities_integrate_the_sloping_top_and_clipped_door() {
        let f = exported(
            r#"
type {"category":"wall","name":"Test wall","thickness":200}
type {"category":"floor","name":"Test floor","thickness":200}
type {"category":"door","name":"Tall door","width":1000,"height":5000}
floor {"type":"Test floor","boundary":[[-500,-500],[4500,-500],[4500,500],[-500,500]],"offset":3000,"slope":14.036243467926479,"arrow":[[0,0],[4000,0]]}
wall {"type":"Test wall","points":[[0,0],[4000,0]],"attach_top":100002}
door {"type":"Tall door","wall":100003,"offset":1000}
"#,
        );
        let q = quantities(&f, "IFCWALL", "Qto_WallBaseQuantities");
        // The floor's 200 mm thickness is normal to its slope, so its underside is 200/cos(slope) below its top.
        let underside = 3000.0 - 200.0 * (1.0_f64 + 0.25 * 0.25).sqrt();
        let gross = 4.0 * (underside + 500.0) / 1000.0;
        let cut = (underside + 250.0) / 1000.0;
        near(&q, "GrossSideArea", gross);
        near(&q, "NetSideArea", gross - cut);
        near(&q, "GrossVolume", gross * 0.2);
        near(&q, "NetVolume", (gross - cut) * 0.2);
        assert!(!q.contains_key("Height"), "a varying height must not be reported as a single height");
    }
}
