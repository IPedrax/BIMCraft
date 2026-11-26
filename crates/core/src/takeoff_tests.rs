//! Material takeoffs driven through the engine's commands: the layers of compound types by their net areas,
//! their volumes and masses, grouped by material, layer or type, with waste (`takeoff.rs`).

use serde_json::Value;

use crate::engine::Session;

/// The materials the tests' types are made of, with the densities a takeoff weighs them by.
fn materials(s: &mut Session) {
    for (name, density, mass_per_m2) in
        [("OSB-3", 650.0, 0.0), ("Mineral wool", 35.0, 0.0), ("Gypsum board", 900.0, 0.0), ("Membrane", 0.0, 1.0), ("Foam", 0.0, 0.0)]
    {
        s.execute("material", &serde_json::json!({"name": name, "density": density, "mass_per_m2": mass_per_m2})).unwrap();
    }
}

/// A timber frame wall type: OSB on both faces, mineral wool, gypsum board; 231.5 mm thick.
fn frame_type(s: &mut Session, name: &str) {
    s.execute(
        "type",
        &serde_json::json!({"category": "wall", "name": name, "layers": [
            {"function": "substrate", "material": "OSB-3", "thickness": 12.0},
            {"function": "thermal", "material": "Mineral wool", "thickness": 195.0},
            {"function": "finish", "material": "Gypsum board", "thickness": 12.5},
            {"function": "substrate", "material": "OSB-3", "thickness": 12.0},
        ]}),
    )
    .unwrap();
}

fn row<'a>(out: &'a Value, material: &str) -> &'a Value {
    out["rows"].as_array().unwrap().iter().find(|r| r["material"] == material).unwrap_or_else(|| panic!("no row of {material}: {out}"))
}

fn near(v: &Value, want: f64) {
    close(v, want, 0.006);
}

fn close(v: &Value, want: f64, tolerance: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("{v} is not a number"));
    assert!((got - want).abs() < tolerance, "{got} isn't {want}");
}

#[test]
fn a_materials_density_comes_by_its_name_in_any_language() {
    use crate::takeoff::density_of;
    assert_eq!(density_of("Timber"), crate::takeoff::Mass { density: 500.0, per_area: 0.0 });
    assert_eq!(density_of("OSB-3"), crate::takeoff::Mass { density: 650.0, per_area: 0.0 });
    assert_eq!(density_of("Mineral wool 195 mm"), crate::takeoff::Mass { density: 35.0, per_area: 0.0 });
    assert_eq!(density_of("Membrane"), crate::takeoff::Mass { density: 0.0, per_area: 1.0 });
    assert_eq!(density_of("Минвата"), crate::takeoff::Mass { density: 35.0, per_area: 0.0 });
    assert_eq!(density_of("Неизвестный материал"), crate::takeoff::Mass::default());
}

#[test]
fn a_wall_takeoff_counts_each_layer_by_its_net_area_and_weighs_it() {
    let mut s = Session::new();
    materials(&mut s);
    frame_type(&mut s, "Timber frame");
    s.execute("wall", &serde_json::json!({"type": "Timber frame", "points": [[0, 0], [10000, 0]], "height": 3000})).unwrap();
    let out = s.execute("takeoff", &serde_json::json!({"categories": ["walls"], "by": "material", "waste": 10})).unwrap();
    assert_eq!(out["rows"].as_array().unwrap().len(), 3, "{out}");
    // 10 m by 3 m is 30 m² of one face; OSB is on both, so 60 m², 0.72 m³ and 468 kg.
    let osb = row(&out, "OSB-3");
    near(&osb["area_m2"], 60.0);
    near(&osb["volume_m3"], 0.72);
    near(&osb["mass_kg"], 468.0);
    near(&osb["total"], 0.792);
    let wool = row(&out, "Mineral wool");
    near(&wool["volume_m3"], 5.85);
    near(&wool["mass_kg"], 204.8);
    near(&wool["total"], 6.44);
    let board = row(&out, "Gypsum board");
    near(&board["volume_m3"], 0.375);
    near(&board["mass_kg"], 337.5);
    near(&out["totals"]["area_m2"], 120.0);
    near(&out["totals"]["volume_m3"], 6.945);
    assert!(out["warnings"].as_array().is_none_or(|w| w.is_empty()), "{out}");
}

#[test]
fn the_reference_wall_comes_to_the_hand_calculation() {
    // R-004: 84.76 m² of net wall, OSB 12 on both faces, mineral wool 195, gypsum board 12.5.
    let mut s = Session::new();
    materials(&mut s);
    frame_type(&mut s, "Timber frame");
    s.execute("wall", &serde_json::json!({"type": "Timber frame", "points": [[0, 0], [28253.3333333333, 0]], "height": 3000})).unwrap();
    let out = s.execute("takeoff", &serde_json::json!({"categories": ["walls"]})).unwrap();
    let osb = row(&out, "OSB-3");
    near(&osb["area_m2"], 169.52);
    near(&osb["volume_m3"], 2.034);
    close(&osb["mass_kg"], 1322.26, 0.06);
    let wool = row(&out, "Mineral wool");
    near(&wool["area_m2"], 84.76);
    near(&wool["volume_m3"], 16.528);
    close(&wool["mass_kg"], 578.49, 0.06);
    let board = row(&out, "Gypsum board");
    near(&board["volume_m3"], 1.06);
    close(&board["mass_kg"], 953.55, 0.06);
    near(&out["totals"]["volume_m3"], 19.622);
}

#[test]
fn a_door_takes_its_area_from_the_takeoff_once() {
    let mut s = Session::new();
    materials(&mut s);
    frame_type(&mut s, "Timber frame");
    let wall =
        s.execute("wall", &serde_json::json!({"type": "Timber frame", "points": [[0, 0], [10000, 0]], "height": 3000})).unwrap()["id"]
            .as_u64()
            .unwrap();
    s.execute("door", &serde_json::json!({"at": [5000, 0], "width": 900, "height": 2100})).unwrap();
    // The net area already has the door out (TZ-031): the takeoff takes it and nothing twice.
    let net = s.model.areas(wall).unwrap().net / 1e6;
    assert!((net - 28.11).abs() < 1e-6, "{net}");
    let out = s.execute("takeoff", &serde_json::json!({"categories": ["walls"], "by": "material"})).unwrap();
    near(&row(&out, "OSB-3")["area_m2"], net * 2.0);
    near(&row(&out, "Mineral wool")["area_m2"], net);
}

#[test]
fn a_roofs_area_is_its_surface_not_its_plan() {
    let mut s = Session::new();
    materials(&mut s);
    s.execute(
        "type",
        &serde_json::json!({"category": "roof", "name": "Pitched", "layers": [{"function": "structure", "material": "OSB-3", "thickness": 18.0}]}),
    )
    .unwrap();
    let roof = s
        .execute("roof", &serde_json::json!({"type": "Pitched", "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "slope": 30}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let net = s.model.areas(roof).unwrap().net / 1e6;
    let plan = s.model.plan_area(roof).unwrap() / 1e6;
    assert!(net > plan, "a pitched roof's surface is more than its plan: {net} against {plan}");
    let out = s.execute("takeoff", &serde_json::json!({"categories": ["roofs"]})).unwrap();
    near(&row(&out, "OSB-3")["area_m2"], net);
    assert!(row(&out, "OSB-3")["area_m2"].as_f64().unwrap() > plan, "the takeoff uses the surface, not the plan: {out}");
}

#[test]
fn by_material_layer_and_type_sum_the_same() {
    let mut s = Session::new();
    materials(&mut s);
    frame_type(&mut s, "Timber frame");
    s.execute("wall", &serde_json::json!({"type": "Timber frame", "points": [[0, 0], [10000, 0]], "height": 3000})).unwrap();
    s.execute(
        "type",
        &serde_json::json!({"category": "floor", "name": "Pitched", "layers": [{"function": "structure", "material": "OSB-3", "thickness": 18.0}]}),
    )
    .unwrap();
    s.execute("floor", &serde_json::json!({"type": "Pitched", "boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]})).unwrap();
    let mut volumes = Vec::new();
    for by in ["material", "layer", "type"] {
        let out = s.execute("takeoff", &serde_json::json!({"categories": ["walls", "floors"], "by": by})).unwrap();
        volumes.push(out["totals"]["volume_m3"].as_f64().unwrap());
        assert!(!out["rows"].as_array().unwrap().is_empty(), "{by}: {out}");
    }
    assert!(volumes.iter().all(|v| (v - volumes[0]).abs() < 1e-9), "{volumes:?}");
}

#[test]
fn a_takeoff_is_worked_out_from_the_model_as_it_is() {
    let mut s = Session::new();
    materials(&mut s);
    frame_type(&mut s, "Timber frame");
    s.execute("wall", &serde_json::json!({"type": "Timber frame", "points": [[0, 0], [10000, 0]], "height": 3000})).unwrap();
    let made = s.execute("takeoff", &serde_json::json!({"name": "Materials"})).unwrap();
    assert!(made["rows"].as_array().unwrap().iter().any(|r| r["material"] == "Mineral wool"));
    // The type loses the wool: the next look at the view has no wool either, without the view being edited.
    s.execute(
        "type",
        &serde_json::json!({"category": "wall", "name": "Timber frame", "layers": [
            {"function": "substrate", "material": "OSB-3", "thickness": 12.0},
            {"function": "substrate", "material": "OSB-3", "thickness": 12.0},
        ]}),
    )
    .unwrap();
    let out = s.execute("refresh_takeoff", &serde_json::json!({"name": "Materials"})).unwrap();
    assert!(!out["rows"].as_array().unwrap().iter().any(|r| r["material"] == "Mineral wool"), "{out}");
    near(&row(&out, "OSB-3")["volume_m3"], 0.72);
    // A door added after the view was made takes its area from the next look at it, once (TZ-031).
    s.execute("door", &serde_json::json!({"at": [5000, 0], "width": 900, "height": 2100})).unwrap();
    let out = s.execute("refresh_takeoff", &serde_json::json!({"name": "Materials"})).unwrap();
    near(&row(&out, "OSB-3")["area_m2"], (30.0 - 1.89) * 2.0);
}

#[test]
fn a_framed_wall_counts_its_studs_as_timber_by_their_sections() {
    let mut s = Session::new();
    materials(&mut s);
    s.execute(
        "type",
        &serde_json::json!({"category": "wall", "name": "Framed", "layers": [{"function": "structure", "material": "Concrete", "thickness": 200.0}]}),
    )
    .unwrap();
    s.execute("type", &serde_json::json!({"category": "framing", "name": "Test framing", "material": "Timber"})).unwrap();
    let wall = s.execute("wall", &serde_json::json!({"type": "Framed", "points": [[0, 0], [10000, 0]], "height": 3000})).unwrap()["id"]
        .as_u64()
        .unwrap();
    s.execute("frame", &serde_json::json!({"id": wall, "type": "Test framing"})).unwrap();
    let framing = crate::framing::of(&s.model, wall).unwrap().unwrap();
    let volume: f64 = framing.members.iter().map(|x| x.section[0] * x.section[1] * x.length * f64::from(x.plies) / 1e9).sum();
    let out = s.execute("takeoff", &serde_json::json!({"categories": ["walls"]})).unwrap();
    let timber = row(&out, "Timber");
    close(&timber["volume_m3"], volume, 0.0015);
    assert!(timber["area_m2"].is_null(), "framing is linear, so its own row has no area: {out}");
    close(&timber["mass_kg"], volume * 500.0, 0.06);
}

#[test]
fn a_layer_without_a_material_and_a_material_without_a_density_are_warnings() {
    let mut s = Session::new();
    materials(&mut s);
    s.execute(
        "type",
        &serde_json::json!({"category": "wall", "name": "Odd", "layers": [
            {"function": "thermal", "material": "Foam", "thickness": 50.0},
            {"function": "finish", "material": "", "thickness": 10.0},
        ]}),
    )
    .unwrap();
    s.execute("wall", &serde_json::json!({"type": "Odd", "points": [[0, 0], [10000, 0]], "height": 3000})).unwrap();
    let out = s.execute("takeoff", &serde_json::json!({"categories": ["walls"]})).unwrap();
    let foam = row(&out, "Foam");
    assert_eq!(foam["mass_kg"], Value::Null, "a material with no density has no mass, not 0: {out}");
    let warnings = out["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("layer_without_material"))), "{out}");
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("no density"))), "{out}");
    assert_eq!(out["rows"].as_array().unwrap().len(), 1, "the empty layer makes no row: {out}");
}

#[test]
fn a_takeoff_is_saved_with_the_project_and_placed_on_a_sheet() {
    let mut s = Session::new();
    materials(&mut s);
    frame_type(&mut s, "Timber frame");
    s.execute("wall", &serde_json::json!({"type": "Timber frame", "points": [[0, 0], [10000, 0]], "height": 3000})).unwrap();
    s.execute("takeoff", &serde_json::json!({"name": "Materials"})).unwrap();
    s.execute("sheet", &serde_json::json!({})).unwrap();
    s.execute("place", &serde_json::json!({"sheet": "A101", "view": "Materials"})).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("takeoff.bimc");
    s.execute("save", &serde_json::json!({"path": path.to_str().unwrap()})).unwrap();
    let mut back = Session::new();
    back.execute("open", &serde_json::json!({"path": path.to_str().unwrap()})).unwrap();
    let out = back.execute("refresh_takeoff", &serde_json::json!({"name": "Materials"})).unwrap();
    assert_eq!(row(&out, "OSB-3")["area_m2"], serde_json::json!(60.0), "{out}");
}
