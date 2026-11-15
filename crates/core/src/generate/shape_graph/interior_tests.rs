//! Interior alignment (jogs) and outline bumps, through the files `generate --rooms` writes.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::family::Family;
use super::{Interior, Request, build, build_family};
use crate::generate::{MANIFEST, fnv1a64};

const TOY: &str = include_str!("testdata/toy-family.json");

fn req(seed: u64, rooms: i64, shape: Option<&str>, interior: Interior, bumps: bool) -> Request {
    Request { archetype: "house".into(), seed, rooms, tolerance: 0, shape: shape.map(String::from), gridlines: false, interior, bumps }
}

fn files(r: &Request) -> BTreeMap<&'static str, String> {
    build(r, false).unwrap_or_else(|e| panic!("{r:?}: {e}")).files.into_iter().collect()
}

fn manifest(f: &BTreeMap<&'static str, String>) -> Value {
    serde_json::from_str(&f[MANIFEST]).expect("the manifest is JSON")
}

/// The hashes of the five files that are not the manifest, for a plan.
fn hashes(r: &Request) -> Vec<String> {
    let built = build(r, false).unwrap_or_else(|e| panic!("{r:?}: {e}"));
    built.files.iter().filter(|(n, _)| *n != MANIFEST).map(|(_, t)| fnv1a64(t)).collect()
}

type P = (f64, f64);
/// A part of one outline outside another: its area, its bounding box and whether it is a rectangle.
type Region = (f64, [f64; 4], bool);

/// The one loop the 250 mm walls of a script make, as corner points (no point between two on one axis line).
fn outline_of(script: &str) -> Vec<P> {
    let mut segs: Vec<([i64; 2], [i64; 2])> = Vec::new();
    for line in script.lines().filter(|l| l.starts_with("wall ")) {
        let v: Value = serde_json::from_str(&line[5..]).unwrap_or(Value::Null);
        if v["type"] != "Exterior - 250mm" {
            continue;
        }
        let mut pts: Vec<[i64; 2]> = v["points"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| [p[0].as_f64().unwrap_or(0.0) as i64, p[1].as_f64().unwrap_or(0.0) as i64])
            .collect();
        if v["closed"] == true && pts.len() > 2 {
            pts.push(pts[0]);
        }
        segs.extend(pts.windows(2).map(|w| (w[0], w[1])));
    }
    let xs: BTreeSet<i64> = segs.iter().flat_map(|s| [s.0[0], s.1[0]]).collect();
    let ys: BTreeSet<i64> = segs.iter().flat_map(|s| [s.0[1], s.1[1]]).collect();
    let mut adj: BTreeMap<[i64; 2], Vec<[i64; 2]>> = BTreeMap::new();
    let mut seen: BTreeSet<([i64; 2], [i64; 2])> = BTreeSet::new();
    let mut edge = |a: [i64; 2], b: [i64; 2]| {
        if seen.insert((a.min(b), a.max(b))) {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
    };
    for (a, b) in &segs {
        if a[0] == b[0] {
            let (lo, hi) = (a[1].min(b[1]), a[1].max(b[1]));
            let v: Vec<i64> = ys.iter().copied().filter(|y| (lo..=hi).contains(y)).collect();
            v.windows(2).for_each(|w| edge([a[0], w[0]], [a[0], w[1]]));
        } else {
            let (lo, hi) = (a[0].min(b[0]), a[0].max(b[0]));
            let v: Vec<i64> = xs.iter().copied().filter(|x| (lo..=hi).contains(x)).collect();
            v.windows(2).for_each(|w| edge([w[0], a[1]], [w[1], a[1]]));
        }
    }
    assert!(adj.values().all(|n| n.len() == 2), "the exterior walls make one loop");
    let start = *adj.keys().next().expect("walls");
    let (mut prev, mut here) = (start, adj[&start][0]);
    let mut walk = vec![start];
    while here != start {
        walk.push(here);
        let next = if adj[&here][0] != prev { adj[&here][0] } else { adj[&here][1] };
        (prev, here) = (here, next);
    }
    let n = walk.len();
    (0..n)
        .filter(|&i| {
            let (a, b, c) = (walk[(i + n - 1) % n], walk[i], walk[(i + 1) % n]);
            (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) != 0
        })
        .map(|i| (walk[i][0] as f64, walk[i][1] as f64))
        .collect()
}

fn inside(poly: &[P], x: f64, y: f64) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            c = !c;
        }
    }
    c
}

/// The parts of `a` outside `b`: each as (area, bounding box x0, y0, x1, y1, is a rectangle).
fn regions(a: &[P], b: &[P]) -> Vec<(f64, [f64; 4], bool)> {
    let grid = |k: usize| {
        let mut v: Vec<f64> = a.iter().chain(b).map(|p| if k == 0 { p.0 } else { p.1 }).collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    };
    let (xs, ys) = (grid(0), grid(1));
    let mut cells: BTreeSet<(usize, usize)> = BTreeSet::new();
    for i in 0..xs.len() - 1 {
        for j in 0..ys.len() - 1 {
            let (cx, cy) = ((xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0);
            if inside(a, cx, cy) && !inside(b, cx, cy) {
                cells.insert((i, j));
            }
        }
    }
    let mut out = Vec::new();
    let mut done: BTreeSet<(usize, usize)> = BTreeSet::new();
    for &c in &cells {
        if done.contains(&c) {
            continue;
        }
        let (mut comp, mut todo) = (vec![c], vec![c]);
        done.insert(c);
        while let Some((i, j)) = todo.pop() {
            for d in [(i + 1, j), (i.wrapping_sub(1), j), (i, j + 1), (i, j.wrapping_sub(1))] {
                if cells.contains(&d) && done.insert(d) {
                    comp.push(d);
                    todo.push(d);
                }
            }
        }
        let area: f64 = comp.iter().map(|&(i, j)| (xs[i + 1] - xs[i]) * (ys[j + 1] - ys[j])).sum();
        let x0 = xs[comp.iter().map(|c| c.0).min().unwrap_or(0)];
        let x1 = xs[comp.iter().map(|c| c.0).max().unwrap_or(0) + 1];
        let y0 = ys[comp.iter().map(|c| c.1).min().unwrap_or(0)];
        let y1 = ys[comp.iter().map(|c| c.1).max().unwrap_or(0) + 1];
        out.push((area, [x0, y0, x1, y1], (area - (x1 - x0) * (y1 - y0)).abs() < 1.0));
    }
    out
}

fn base_of(m: &Value) -> Vec<P> {
    m["base_outline"]
        .as_array()
        .expect("base_outline")
        .iter()
        .map(|p| (p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)))
        .collect()
}

#[test]
fn aligned_off_plans_are_unchanged() {
    // Recorded on the commit before jogs and bumps: the aligned path draws no extra random number.
    let cases = [
        (
            req(3, 16, Some("court"), Interior::Aligned, false),
            ["c36b3c33d64dff7b", "05ea4bc8a6cea27e", "1187c8a0a9d8d525", "6e5fc0d9167f6cbe", "c03f9a83f0400961"],
        ),
        (
            req(5, 12, Some("t_shape"), Interior::Aligned, false),
            ["aa6796588871ca0b", "a46555c0b80c65fe", "1c480414afee010b", "82e1e538b623a22f", "c7f9a5ba99aff147"],
        ),
        (
            req(7, 8, None, Interior::Aligned, false),
            ["6afef852bced675f", "706e0145a7492e95", "b931c960257077ca", "f373253228428faf", "230af99a51e8c526"],
        ),
        (
            req(2, 5, Some("bar"), Interior::Aligned, false),
            ["df3ac3dd197662dc", "18bda057fbdaf7fa", "1545ba332882d5da", "a269548fe13cdc38", "874abb5809d413c0"],
        ),
    ];
    for (r, want) in &cases {
        assert_eq!(&hashes(r), want, "{r:?}");
    }
}

#[test]
fn an_interior_or_bumps_value_out_of_range_is_refused_and_writes_nothing() {
    let dir = std::env::temp_dir().join("shape-graph-interior-refused");
    let _ = std::fs::remove_dir_all(&dir);
    for extra in [json!({"interior": "zigzag"}), json!({"bumps": "sideways"})] {
        let mut p = json!({"path": dir.display().to_string(), "archetype": "house", "seed": 1, "rooms": 12});
        p.as_object_mut().expect("object").extend(extra.as_object().expect("object").clone());
        let err = crate::generate::run(&p).expect_err("refused");
        assert_eq!(err.code, "invalid_params", "{p}");
        assert!(!dir.exists(), "nothing is written");
    }
    for extra in
        [json!({"interior": "jogged"}), json!({"interior": "mixed", "bumps": "on"}), json!({"interior": "aligned", "bumps": "off"})]
    {
        let mut p = json!({"path": dir.display().to_string(), "archetype": "house", "seed": 1, "rooms": 12});
        p.as_object_mut().expect("object").extend(extra.as_object().expect("object").clone());
        crate::generate::run(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn an_aligned_plan_without_bumps_has_the_catalogue_outline_as_its_base() {
    for (seed, shape) in [(3, "court"), (4, "l_shape"), (9, "bar")] {
        let f = files(&req(seed, 14, Some(shape), Interior::Aligned, false));
        let m = manifest(&f);
        assert_eq!(m["interior"], "aligned");
        assert_eq!(m["bumps"], json!([]));
        let out = outline_of(&f["script.bimcraft.txt"]);
        let base = base_of(&m);
        assert_eq!(out.len(), base.len(), "{shape}");
        assert!(regions(&out, &base).is_empty() && regions(&base, &out).is_empty(), "{shape}: base_outline is the outline");
    }
}

#[test]
fn a_jogged_plan_without_bumps_keeps_the_outline_and_changes_the_inside() {
    for rooms in [8, 12, 20] {
        for seed in 1..=4 {
            let jogged = files(&req(seed, rooms, None, Interior::Jogged, false));
            let m = manifest(&jogged);
            assert_eq!(m["interior"], "jogged");
            assert_eq!(m["bumps"], json!([]));
            let out = outline_of(&jogged["script.bimcraft.txt"]);
            let base = base_of(&m);
            assert!(regions(&out, &base).is_empty() && regions(&base, &out).is_empty(), "{rooms} rooms, seed {seed}");
            assert_eq!(out.len(), base.len());
        }
    }
}

#[test]
fn aligned_bumps_differ_from_the_base_by_exactly_the_listed_bumps() {
    let (mut outward, mut inward) = (0, 0);
    for seed in 1..=12 {
        let f = files(&req(seed, 12 + (seed as i64 % 5) * 2, None, Interior::Aligned, true));
        let m = manifest(&f);
        let bumps = m["bumps"].as_array().expect("bumps");
        assert!((1..=3).contains(&bumps.len()), "seed {seed}: {} bumps", bumps.len());
        let out = outline_of(&f["script.bimcraft.txt"]);
        let base = base_of(&m);
        let mut parts: Vec<(i32, Region)> = regions(&out, &base).into_iter().map(|r| (1, r)).collect();
        parts.extend(regions(&base, &out).into_iter().map(|r| (-1, r)));
        assert_eq!(parts.len(), bumps.len(), "seed {seed}");
        let mut want: Vec<(i32, i64)> = Vec::new();
        for b in bumps {
            let off = b["offset_mm"].as_i64().expect("offset");
            assert!((300..=1200).contains(&off.abs()), "{off}");
            assert!(b["side"].as_str().is_some_and(|s| ["N", "E", "S", "W"].contains(&s)), "{b}");
            assert!(b["room"].as_str().is_some_and(|s| !s.is_empty()));
            want.push((off.signum() as i32, off.abs()));
            if off > 0 { outward += 1 } else { inward += 1 }
        }
        let mut got: Vec<(i32, i64)> = Vec::new();
        for (sign, (_, bb, rect)) in &parts {
            assert!(rect, "seed {seed}: a bump is one rectangle");
            let depth = ((bb[2] - bb[0]).min(bb[3] - bb[1])) as i64;
            got.push((*sign, depth));
        }
        want.sort();
        got.sort();
        assert_eq!(got, want, "seed {seed}");
        assert_eq!(
            base.len(),
            super::family::shapes().expect("shapes").iter().find(|s| s.id == m["shape"].as_str().unwrap_or("")).map_or(0, |s| s.corners)
        );
    }
    assert!(outward > 0 && inward > 0, "both an outward and an inward bump occur: {outward} out, {inward} in");
}

#[test]
fn the_defaults_are_mixed_and_on() {
    let default = Request::new("house".into(), 8, 12, 0, None, false);
    assert_eq!((default.interior, default.bumps), (Interior::Mixed, true));
    let (mut aligned, mut jogged) = (0, 0);
    for seed in 1..=10 {
        let a = files(&Request::new("house".into(), seed, 12, 0, None, false));
        let b = files(&Request { interior: Interior::Mixed, bumps: true, ..Request::new("house".into(), seed, 12, 0, None, false) });
        assert_eq!(a, b, "seed {seed}");
        let m = manifest(&a);
        match m["interior"].as_str() {
            Some("aligned") => aligned += 1,
            Some("jogged") => jogged += 1,
            other => panic!("interior {other:?}"),
        }
        assert!(!m["bumps"].as_array().expect("bumps").is_empty(), "seed {seed} has a bump");
    }
    assert!(aligned > 0 && jogged > 0, "{aligned} aligned, {jogged} jogged");
}

#[test]
fn the_toy_family_takes_jogs_and_bumps() {
    let fam = Family::parse(TOY).expect("toy");
    for seed in 1..=3 {
        let r = Request { archetype: String::new(), ..req(seed, 12, None, Interior::Jogged, true) };
        let built = build_family(&fam, &r, false).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(built.rooms, 12);
    }
}
