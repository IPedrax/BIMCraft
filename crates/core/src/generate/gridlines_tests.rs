//! `generate --gridlines`: the grid by `--rooms` at the outline's corners, and by `--area` what the archetype draws, left out or put in.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};

use serde_json::{Value, json};

use super::*;

const AREA_ARCHETYPES: [(&str, f64); 6] =
    [("house", 115.0), ("office", 900.0), ("clinic", 650.0), ("hospital", 1800.0), ("studio-office", 180.0), ("office-pattern", 200.0)];
const FILES6: [&str; 6] =
    ["program-graph.json", "topology-graph.json", "building-ir.json", "script.bimcraft.txt", "report.json", "manifest.json"];

fn dir(test: &str, n: usize) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-gridlines-{}-{test}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// The six files of a call, by name, in a fresh directory that is removed again.
fn call(test: &str, n: usize, mut params: Value) -> Result<BTreeMap<String, String>, Refusal> {
    let d = dir(test, n);
    params["path"] = json!(d);
    let r = run(&params);
    let out = r.map(|_| FILES6.iter().map(|f| (f.to_string(), std::fs::read_to_string(d.join(f)).unwrap_or_default())).collect());
    let _ = std::fs::remove_dir_all(&d);
    out
}

/// The files of a brief, made once for all the tests that read them: a brief with other params is another call.
fn files(test: &str, n: usize, params: Value) -> &'static BTreeMap<String, String> {
    type Files = BTreeMap<String, String>;
    static MADE: LazyLock<Mutex<HashMap<String, &'static OnceLock<Files>>>> = LazyLock::new(Mutex::default);
    let brief = params.to_string();
    let place = *MADE.lock().unwrap().entry(brief.clone()).or_insert_with(|| Box::leak(Box::default()));
    // The directory is the brief's own, so tests that run side by side do not share one.
    place.get_or_init(|| call(&format!("{test}-{}", fnv1a64(&brief)), n, params).unwrap_or_else(|e| panic!("{test}: {e}")))
}

/// The commands of a script: its id and params.
fn commands(script: &str) -> Vec<(String, Value)> {
    script
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (id, p) = l.split_once(' ').unwrap_or((l, "{}"));
            (id.to_string(), serde_json::from_str(p).unwrap_or(Value::Null))
        })
        .collect()
}

fn grids(script: &str) -> Vec<Value> {
    commands(script).into_iter().filter(|(c, _)| c == "grid").map(|(_, p)| p).collect()
}

fn pt(v: &Value) -> (i64, i64) {
    (v[0].as_i64().unwrap_or(i64::MIN), v[1].as_i64().unwrap_or(i64::MIN))
}

/// The corners of the loop of the thickest walls, found independently of the generator: walk the loop, keep the turns.
fn outline_corners(script: &str) -> Vec<(i64, i64)> {
    let cmds = commands(script);
    let mut thick: BTreeMap<String, i64> = BTreeMap::new();
    for (c, p) in &cmds {
        if c == "type" && p["category"] == "wall" {
            let t = p["layers"].as_array().map_or(0, |ls| ls.iter().map(|l| l["thickness"].as_i64().unwrap_or(0)).sum());
            thick.insert(p["name"].as_str().unwrap_or_default().to_string(), t);
        }
    }
    let walls: Vec<(&str, &Value)> =
        cmds.iter().filter(|(c, _)| c == "wall").map(|(_, p)| (p["type"].as_str().unwrap_or_default(), p)).collect();
    let tmax = walls.iter().map(|(t, _)| thick.get(*t).copied().unwrap_or(0)).max().expect("walls");
    let mut adj: BTreeMap<(i64, i64), Vec<(i64, i64)>> = BTreeMap::new();
    for (t, p) in &walls {
        if thick.get(*t).copied().unwrap_or(0) != tmax {
            continue;
        }
        let (a, b) = (pt(&p["points"][0]), pt(&p["points"][1]));
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    assert!(adj.values().all(|n| n.len() == 2), "the exterior walls close one loop");
    let start = *adj.keys().next().expect("a wall");
    let (mut loop_, mut prev, mut cur) = (vec![start], start, adj[&start][0]);
    while cur != start {
        loop_.push(cur);
        let next = adj[&cur].iter().copied().find(|p| *p != prev).expect("a next point");
        prev = cur;
        cur = next;
    }
    let n = loop_.len();
    (0..n)
        .filter(|&k| {
            let (p, q, r) = (loop_[(k + n - 1) % n], loop_[k], loop_[(k + 1) % n]);
            (q.0 - p.0) * (r.1 - q.1) - (q.1 - p.1) * (r.0 - q.0) != 0
        })
        .map(|k| loop_[k])
        .collect()
}

fn rooms_brief(shape: &str, seed: u64) -> Value {
    json!({"archetype": "house", "seed": seed, "rooms": 10, "shape": shape, "interior": "aligned", "bumps": "off"})
}

fn with(mut brief: Value, key: &str, value: &str) -> Value {
    brief[key] = json!(value);
    brief
}

fn shape_ids() -> Vec<String> {
    shape_graph::family::shapes().expect("the catalogue reads").into_iter().map(|s| s.id).collect()
}

fn names(gs: &[Value]) -> Vec<String> {
    gs.iter().map(|g| g["name"].as_str().unwrap_or_default().to_string()).collect()
}

#[test]
fn gridlines_other_than_on_or_off_is_refused_and_writes_nothing() {
    for (n, brief) in [json!({"archetype": "house", "seed": 1, "rooms": 10}), json!({"archetype": "house", "seed": 1, "area_m2": 115.0})]
        .into_iter()
        .enumerate()
    {
        let d = dir("maybe", n);
        let mut p = with(brief, "gridlines", "maybe");
        p["path"] = json!(d);
        let err = run(&p).unwrap_err();
        assert_eq!(err.code, "invalid_params");
        assert!(!d.exists());
    }
}

#[test]
fn rooms_default_is_off_and_has_no_grid() {
    for shape in ["court", "rectangle"].iter().filter(|s| shape_ids().iter().any(|i| i == **s)) {
        let default = files("rd", 0, rooms_brief(shape, 1));
        let off = files("rd", 1, with(rooms_brief(shape, 1), "gridlines", "off"));
        assert_eq!(default, off, "{shape}");
        assert!(grids(&default["script.bimcraft.txt"]).is_empty());
        let m: Value = serde_json::from_str(&default["manifest.json"]).expect("json");
        assert_eq!(m["gridlines"], json!(false));
    }
}

#[test]
fn rooms_on_draws_a_line_at_each_corner_x_and_y_for_every_shape() {
    let mut most = 0;
    for (i, shape) in shape_ids().iter().enumerate() {
        let on = files("ro", i, with(rooms_brief(shape, 1), "gridlines", "on"));
        let script = &on["script.bimcraft.txt"];
        let corners = outline_corners(script);
        let xs: BTreeSet<i64> = corners.iter().map(|c| c.0).collect();
        let ys: BTreeSet<i64> = corners.iter().map(|c| c.1).collect();
        let (x0, x1, y0, y1) = (*xs.first().unwrap(), *xs.last().unwrap(), *ys.first().unwrap(), *ys.last().unwrap());
        let gs = grids(script);
        let (mut v, mut h): (Vec<_>, Vec<_>) = gs.iter().partition(|g| pt(&g["start"]).0 == pt(&g["end"]).0);
        v.sort_by_key(|g| pt(&g["start"]).0);
        h.sort_by_key(|g| pt(&g["start"]).1);
        assert_eq!(v.iter().map(|g| pt(&g["start"]).0).collect::<Vec<_>>(), xs.iter().copied().collect::<Vec<_>>(), "{shape}");
        assert_eq!(h.iter().map(|g| pt(&g["start"]).1).collect::<Vec<_>>(), ys.iter().copied().collect::<Vec<_>>(), "{shape}");
        for g in &v {
            assert_eq!((pt(&g["start"]).1, pt(&g["end"]).1), (y0 - 1500, y1 + 1500), "{shape}");
        }
        for g in &h {
            assert_eq!((pt(&g["start"]).0, pt(&g["end"]).0), (x0 - 1500, x1 + 1500), "{shape}");
        }
        assert_eq!(names(&v.iter().map(|g| (*g).clone()).collect::<Vec<_>>()), (1..=v.len()).map(|n| n.to_string()).collect::<Vec<_>>());
        let letters: Vec<String> = (0..h.len()).map(|k| ((b'A' + k as u8) as char).to_string()).collect();
        assert_eq!(names(&h.iter().map(|g| (*g).clone()).collect::<Vec<_>>()), letters, "{shape}");
        let m: Value = serde_json::from_str(&on["manifest.json"]).expect("json");
        assert_eq!(m["gridlines"], json!(true));
        most = most.max(v.len().min(h.len()));
    }
    assert!(most >= 3, "some plan has three lines each way");
}

#[test]
fn rooms_on_and_off_are_the_same_plan() {
    for (i, shape) in shape_ids().iter().enumerate() {
        let on = files("rs", 2 * i, with(rooms_brief(shape, 1), "gridlines", "on"));
        let off = files("rs", 2 * i + 1, with(rooms_brief(shape, 1), "gridlines", "off"));
        assert_eq!(on["topology-graph.json"], off["topology-graph.json"], "{shape}");
        let key = |f: &BTreeMap<String, String>| {
            commands(&f["script.bimcraft.txt"])
                .into_iter()
                .filter(|(c, _)| matches!(c.as_str(), "wall" | "door" | "room_separator" | "room"))
                .map(|(c, mut p)| {
                    if let Some(o) = p.as_object_mut() {
                        o.remove("tag");
                    }
                    format!("{c} {p}")
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(key(on), key(off), "{shape}");
        let (mon, moff): (Value, Value) =
            (serde_json::from_str(&on["manifest.json"]).unwrap(), serde_json::from_str(&off["manifest.json"]).unwrap());
        assert_eq!(mon["topology_hash"], moff["topology_hash"], "{shape}");
        assert_eq!(mon["building"], moff["building"], "{shape}");
    }
}

/// By area the default is the grid on, byte for byte, and off leaves only the grid out. One archetype a test.
fn area_on_is_the_default_and_off_leaves_only_the_grid_out(name: &str) {
    let area = AREA_ARCHETYPES.iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("{name}")).1;
    let base = json!({"archetype": name, "seed": 1, "area_m2": area});
    for delivery in [false, true] {
        let mut b = base.clone();
        if delivery {
            b["delivery"] = json!("on");
        }
        let default = files("ao", 0, b.clone());
        assert!(!grids(&default["script.bimcraft.txt"]).is_empty(), "{name} draws a grid by default");
        assert_eq!(default, files("ao", 1, with(b.clone(), "gridlines", "on")), "{name} delivery {delivery}");
        let off = files("ao", 2, with(b, "gridlines", "off"));
        assert!(grids(&off["script.bimcraft.txt"]).is_empty(), "{name}");
        let rest = |f: &BTreeMap<String, String>| {
            commands(&f["script.bimcraft.txt"])
                .into_iter()
                .filter(|(c, _)| c != "grid")
                .map(|(c, p)| format!("{c} {p}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(rest(default), rest(off), "{name} delivery {delivery}");
        for f in ["program-graph.json", "topology-graph.json", "building-ir.json"] {
            assert_eq!(default[f], off[f], "{name} {f}");
        }
    }
}

#[test]
fn area_on_is_the_default_byte_for_byte_and_off_leaves_only_the_grid_out_house() {
    area_on_is_the_default_and_off_leaves_only_the_grid_out("house");
}

#[test]
fn area_on_is_the_default_byte_for_byte_and_off_leaves_only_the_grid_out_office() {
    area_on_is_the_default_and_off_leaves_only_the_grid_out("office");
}

#[test]
fn area_on_is_the_default_byte_for_byte_and_off_leaves_only_the_grid_out_clinic() {
    area_on_is_the_default_and_off_leaves_only_the_grid_out("clinic");
}

#[test]
fn area_on_is_the_default_byte_for_byte_and_off_leaves_only_the_grid_out_hospital() {
    area_on_is_the_default_and_off_leaves_only_the_grid_out("hospital");
}

#[test]
fn area_on_is_the_default_byte_for_byte_and_off_leaves_only_the_grid_out_studio_office() {
    area_on_is_the_default_and_off_leaves_only_the_grid_out("studio-office");
}

#[test]
fn area_on_is_the_default_byte_for_byte_and_off_leaves_only_the_grid_out_office_pattern() {
    area_on_is_the_default_and_off_leaves_only_the_grid_out("office-pattern");
}

// Every archetype in the table has its test above: one more in the table needs one more here.
const _: () = assert!(AREA_ARCHETYPES.len() == 6);

#[test]
fn medical_day_default_is_off_and_on_adds_the_outline_grid_only() {
    let base = json!({"archetype": "medical-day", "seed": 1, "area_m2": 500.0});
    let default = files("md", 0, base.clone());
    assert!(grids(&default["script.bimcraft.txt"]).is_empty());
    assert_eq!(default, files("md", 1, with(base.clone(), "gridlines", "off")));
    let on = files("md", 2, with(base, "gridlines", "on"));
    let gs = grids(&on["script.bimcraft.txt"]);
    let v = gs.iter().filter(|g| pt(&g["start"]).0 == pt(&g["end"]).0).count();
    assert!(v >= 2 && gs.len() - v >= 2, "{} lines", gs.len());
    let rest = |f: &BTreeMap<String, String>| {
        commands(&f["script.bimcraft.txt"]).into_iter().filter(|(c, _)| c != "grid").map(|(c, p)| format!("{c} {p}")).collect::<Vec<_>>()
    };
    assert_eq!(rest(default), rest(on));
    for f in ["program-graph.json", "topology-graph.json", "building-ir.json"] {
        assert_eq!(default[f], on[f], "{f}");
    }
}

#[test]
fn ring_and_segment_corners_agree_on_an_l() {
    let ring = [[0, 0], [4000, 0], [4000, 2000], [2000, 2000], [2000, 4000], [0, 4000], [0, 2000]];
    let mut a = grid::ring_corners(&ring);
    let segs: Vec<([i64; 2], [i64; 2])> = (0..ring.len()).map(|i| (ring[i], ring[(i + 1) % ring.len()])).collect();
    let mut b = grid::segment_corners(&segs);
    a.sort();
    b.sort();
    assert_eq!(a, b);
    assert_eq!(a.len(), 6);
}

/// The coordinates of what a DXF draws on `layer`, entity by entity.
fn layer_coordinates(dxf: &str, layer: &str) -> Vec<Vec<String>> {
    let lines: Vec<&str> = dxf.lines().collect();
    let mut out: Vec<Vec<String>> = Vec::new();
    let (mut entity, mut on): (Vec<String>, bool) = (Vec::new(), false);
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0].trim(), pair.get(1).map_or("", |v| v.trim()));
        if code == "0" {
            if on && !entity.is_empty() {
                out.push(std::mem::take(&mut entity));
            }
            (entity, on) = (Vec::new(), false);
        } else if code == "8" {
            on = value == layer;
        } else if ["10", "20", "11", "21"].contains(&code) {
            entity.push(value.to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn the_cad_export_writes_the_walls_in_the_same_digits_with_or_without_the_grid() {
    let on = files("cad", 0, with(rooms_brief("l_shape", 1), "gridlines", "on"));
    let off = files("cad", 1, with(rooms_brief("l_shape", 1), "gridlines", "off"));
    let dxf = |f: &BTreeMap<String, String>| {
        let mut s = crate::engine::Session::new();
        s.script(&f["script.bimcraft.txt"]).expect("the script runs");
        let level = s.model.levels().first().map(|(id, _)| *id).expect("a level");
        crate::render::dxf(&crate::render::plan_for_cad(&s.model, level, 1600, 1000), &s.model)
    };
    let (a, b) = (dxf(on), dxf(off));
    let walls = layer_coordinates(&b, "A-WALL");
    assert!(!walls.is_empty());
    assert_eq!(layer_coordinates(&a, "A-WALL"), walls);
    assert!(!layer_coordinates(&a, "A-GRID").is_empty());
    assert!(layer_coordinates(&b, "A-GRID").is_empty());
}
