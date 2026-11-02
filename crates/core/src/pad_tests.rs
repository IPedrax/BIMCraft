//! The building pad: ground made level where a building stands, made with `pad`, measured against the terrain, moved, saved, drawn,
//! scheduled and exported.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, FORMAT, Model, Pad};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).expect_err(&format!("{id} {p} should be refused"))
}

fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Value {
    json!([[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

/// Flat ground, 1 m up, over 10 m by 10 m.
fn ground(s: &mut Session) -> u64 {
    let points: Vec<Value> = (0..3).flat_map(|i| (0..3).map(move |j| json!([i * 5000, j * 5000, 1000]))).collect();
    run(s, "toposurface", json!({"points": points, "interval": 0}))["id"].as_u64().unwrap()
}

fn pads(s: &Session) -> Vec<(u64, Pad)> {
    s.model.elements.iter().filter_map(|(id, e)| if let Element::Pad(p) = e { Some((*id, p.clone())) } else { None }).collect()
}

fn listed(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({"category": "pad"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap_or_else(|| panic!("{id} isn't listed"))
}

#[test]
fn a_pad_is_a_flat_outline_on_a_level_with_a_top_and_a_thickness() {
    let mut s = Session::new();
    let r = run(&mut s, "pad", json!({"boundary": rect(3000, 3000, 7000, 7000), "offset": 500, "name": "House"}));
    assert_eq!((r["area_m2"].clone(), r["top"].clone(), r["thickness"].clone()), (json!(16.0), json!(500.0), json!(300.0)), "{r}");
    let id = r["id"].as_u64().unwrap();
    let e = listed(&mut s, id);
    assert_eq!(
        (e["name"].clone(), e["area_m2"].clone(), e["top"].clone(), e["bottom"].clone()),
        (json!("House"), json!(16.0), json!(500.0), json!(200.0))
    );
    // On Level 2 (3000 up) the top is that much higher; the name is the language's when none is given; the ring runs counter-clockwise.
    let two = run(&mut s, "pad", json!({"boundary": [[0, 0], [0, 4000], [4000, 4000], [4000, 0]], "level": "Level 2", "thickness": 150}));
    assert_eq!((two["top"].clone(), two["thickness"].clone()), (json!(3000.0), json!(150.0)));
    assert_eq!(listed(&mut s, two["id"].as_u64().unwrap())["name"], "Pad");
    assert!(crate::geom::area(&pads(&s).into_iter().find(|(i, _)| *i == two["id"]).unwrap().1.footprint()) > 0.0);
}

#[test]
fn its_cut_and_fill_are_measured_against_the_terrain_under_it() {
    let mut s = Session::new();
    let id = run(&mut s, "pad", json!({"boundary": rect(3000, 3000, 7000, 7000), "offset": 500}))["id"].as_u64().unwrap();
    assert!(listed(&mut s, id)["cut_m3"].is_null(), "no ground, no cut or fill");
    ground(&mut s);
    // 1 m of ground, a pad 0.5 m up: half a metre cut over 16 m2.
    let e = listed(&mut s, id);
    assert_eq!((e["cut_m3"].clone(), e["fill_m3"].clone(), e["net_m3"].clone()), (json!(8.0), json!(0.0), json!(-8.0)), "{e}");
    run(&mut s, "modify", json!({"ids": [id], "offset": 1500}));
    let e = listed(&mut s, id);
    assert_eq!((e["cut_m3"].clone(), e["fill_m3"].clone(), e["net_m3"].clone()), (json!(0.0), json!(8.0), json!(8.0)), "{e}");
    // A pad off the ground has none of it.
    let off = run(&mut s, "pad", json!({"boundary": rect(30000, 30000, 34000, 34000)}))["id"].as_u64().unwrap();
    assert_eq!((listed(&mut s, off)["cut_m3"].clone(), listed(&mut s, off)["fill_m3"].clone()), (json!(0.0), json!(0.0)));
}

#[test]
fn what_cannot_be_a_pad_is_refused_with_what_to_fix() {
    let mut s = Session::new();
    let b = rect(0, 0, 4000, 4000);
    for (p, says) in [
        (json!({}), "boundary"),
        (json!({"boundary": [[0, 0], [1, 1]]}), "boundary"),
        (json!({"boundary": [[0, 0], [1000, 0], [2000, 0]]}), "boundary"),
        (json!({"boundary": b, "thickness": 0}), "thickness"),
        (json!({"boundary": b, "thickness": -5}), "thickness"),
        (json!({"boundary": b, "thickness": 20000}), "thickness"),
        (json!({"boundary": b, "offset": "up"}), "offset"),
        (json!({"boundary": b, "offset": 1e300}), "offset"),
        (json!({"boundary": b, "level": "Level 9"}), "level"),
        (json!({"boundary": b, "colour": 1}), "colour"),
    ] {
        assert!(refused(&mut s, "pad", p.clone()).contains(says), "{p}");
    }
    assert!(pads(&s).is_empty());
}

#[test]
fn a_pad_moves_turns_and_mirrors_in_plan_and_keeps_its_height() {
    let mut s = Session::new();
    let id = run(&mut s, "pad", json!({"boundary": rect(0, 0, 4000, 2000), "offset": 250}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [id], "by": [1000, 2000]}));
    let p = &pads(&s)[0].1;
    assert_eq!((p.footprint()[0].0, p.footprint()[0].1, p.offset), (1000.0, 2000.0, 250.0));
    run(&mut s, "rotate", json!({"ids": [id], "about": [1000, 2000], "angle": 90}));
    let q = &pads(&s)[0].1;
    assert!((crate::geom::area(&q.footprint()) - 8e6).abs() < 1.0, "same size");
    assert!(q.footprint().iter().any(|c| (c.0 - 1000.0).abs() < 1e-6 && (c.1 - 2000.0).abs() < 1e-6), "turned about its corner");
    // Mirrored, it is still counter-clockwise.
    run(&mut s, "mirror", json!({"ids": [id], "axis": [[0, 0], [0, 1000]]}));
    assert!(crate::geom::area(&pads(&s)[0].1.footprint()) > 0.0);
}

#[test]
fn it_is_saved_undone_deleted_and_takes_parameters() {
    let mut s = Session::new();
    let id = run(&mut s, "pad", json!({"boundary": rect(0, 0, 4000, 4000)}))["id"].as_u64().unwrap();
    let text = serde_json::to_string(&*s.model).unwrap();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty() && m.format == FORMAT && m.elements.len() == s.model.elements.len());
    let mut newer = serde_json::to_value(&*s.model).unwrap();
    newer["format"] = json!(FORMAT + 1);
    assert!(Model::read(&newer.to_string()).is_err());
    run(&mut s, "parameter", json!({"name": "Use", "kind": "text", "categories": ["pad"]}));
    run(&mut s, "modify", json!({"ids": [id], "params": {"Use": "House"}}));
    run(&mut s, "delete", json!({"ids": [id]}));
    assert!(pads(&s).is_empty());
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(pads(&s).len(), 1);
    // A pad on a level goes when its level does not: the level is refused its deletion while something stands on it, as for any element.
    let copy = run(&mut s, "copy", json!({"ids": [id], "by": [5000, 0]}));
    assert_eq!(pads(&s).len(), 2, "{copy}");
}

/// The line primitives the plan of `level` draws for `id`.
fn plan_lines(s: &Session, id: u64, level: u64) -> usize {
    use crate::render::{self, Prim};
    let d = render::plan(&s.model, level, 800, 600);
    d.prims.iter().zip(&d.owners).filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Line { .. })).count()
}

#[test]
fn plans_draw_a_pad_on_its_own_level_and_a_category_hides_it() {
    let mut s = Session::new();
    let id = run(&mut s, "pad", json!({"boundary": rect(0, 0, 4000, 3000)}))["id"].as_u64().unwrap();
    assert!(plan_lines(&s, id, 100000) >= 1, "Level 1's plan has its outline");
    assert_eq!(plan_lines(&s, id, 100001), 0, "Level 2's does not");
    run(&mut s, "graphics", json!({"view": 100000, "hide": ["pad"]}));
    assert_eq!(plan_lines(&s, id, 100000), 0);
    // A plan of nothing but a pad is fitted to it.
    use crate::render::{self, Prim};
    let mut t = Session::new();
    let far = run(&mut t, "pad", json!({"boundary": rect(500_000, 500_000, 520_000, 510_000)}))["id"].as_u64().unwrap();
    let d = render::plan(&t.model, 100000, 800, 600);
    let mine: Vec<(f32, f32)> = d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(far)).flat_map(|(p, _)| p.points()).collect();
    assert!(!mine.is_empty() && mine.iter().all(|(x, y)| (0.0..=800.0).contains(x) && (0.0..=600.0).contains(y)), "{mine:?}");
    let _ = Prim::Fill { rings: Vec::new(), color: crate::render::Rgb(0, 0, 0) };
}

#[test]
fn the_3d_view_has_a_pad_as_a_slab_under_its_top() {
    use crate::geom::P2;
    let mut s = Session::new();
    let id = run(&mut s, "pad", json!({"boundary": rect(0, 0, 4000, 3000), "offset": 500, "thickness": 200}))["id"].as_u64().unwrap();
    let tops = crate::view3d::tops_at(&s.model, P2(2000.0, 1500.0));
    assert_eq!(tops.get(&id), Some(&500.0), "{tops:?}");
    assert_eq!(crate::view3d::tops_at(&s.model, P2(9000.0, 9000.0)).get(&id), None);
}

#[test]
fn a_schedule_lists_pads_with_their_cut_and_fill() {
    let mut s = Session::new();
    ground(&mut s);
    run(&mut s, "pad", json!({"boundary": rect(3000, 3000, 7000, 7000), "offset": 500, "name": "House"}));
    let r =
        run(&mut s, "schedule", json!({"name": "Pads", "category": "pad", "fields": ["Name", "Area", "Top", "Thickness", "Cut", "Fill"]}));
    let row = &r["rows"][0];
    assert_eq!(row[0], "House");
    assert_eq!((row[1].as_f64(), row[4].as_f64(), row[5].as_f64()), (Some(16.0), Some(8.0), Some(0.0)), "{r}");
}

#[test]
fn ifc_carries_a_pad_as_a_base_slab_on_its_storey_and_reads_it_back() {
    use crate::step::{self, V};
    let mut s = Session::new();
    let id = run(&mut s, "pad", json!({"boundary": rect(1000, 2000, 5000, 6000), "offset": 400, "thickness": 250, "name": "House"}))["id"]
        .as_u64()
        .unwrap();
    let text = crate::ifc::export(&s.model);
    let f = step::parse(text.as_bytes()).unwrap();
    let slabs: Vec<_> = f.entities.values().filter(|e| e.name == "IFCSLAB").collect();
    assert_eq!(slabs.len(), 1);
    assert_eq!(
        (slabs[0].args[2].clone(), slabs[0].args[4].clone(), slabs[0].args[8].clone()),
        (V::Str("House".into()), V::Str("Pad".into()), V::Enum("BASESLAB".into()))
    );
    // Read back: a pad, not a floor.
    let got = crate::import::ifc(text.as_bytes(), "x").unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped);
    let mut t = Session::new();
    t.model = std::sync::Arc::new(got.model);
    assert_eq!(pads(&t).len(), 1);
    assert_eq!(t.model.elements.values().filter(|e| e.category() == "floor").count(), 0, "not read as a floor");
    let (_, pd) = pads(&t).remove(0);
    assert_eq!((pd.name.as_str(), pd.offset, pd.thickness), ("House", 400.0, 250.0));
    let ring = pd.footprint();
    assert!((crate::geom::area(&ring) - 16e6).abs() < 1.0, "{ring:?}");
    let _ = id;
}
