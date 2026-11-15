//! The house generator through its output: the graphs it samples, the files it writes, and what the topology extractor reads
//! from the project those files build.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::super::*;
use super::place::SHELLS;
use super::sample::sample;
use super::{HouseGraph, Kind, Means};
use crate::graph_rules::{self, Graph as RuleGraph, Pack};

const AREAS: [f64; 5] = [75.0, 95.0, 115.0, 135.0, 160.0];

/// Seeds that, at their area in AREAS, give six shells between them (seed 32 is the corner notch), porches, sliding doors
/// and open edges.
const SHELL_SEEDS: [u64; 6] = [1, 2, 4, 7, 11, 32];

fn house(seed: u64, area: f64) -> Result<Built, Refusal> {
    build(Archetype::House, seed, area)
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    &b.files.iter().find(|(n, _)| *n == name).unwrap().1
}

fn doc(b: &Built, name: &str) -> Value {
    serde_json::from_str(file(b, name)).unwrap()
}

/// The sampled graph as the rule checker reads it, each room with the middle of its kind's area range.
fn rule_graph(g: &HouseGraph) -> Value {
    let pack = Pack::embedded("house").unwrap();
    let nodes: Vec<Value> = g
        .rooms
        .iter()
        .map(|r| {
            let (lo, hi) = pack.kinds[r.kind.name()].area_m2;
            json!({"id": r.id, "kind": r.kind.name(), "area_m2": (lo + hi.unwrap_or(lo * 2.0)) / 2.0})
        })
        .collect();
    let mut edges: Vec<Value> = g
        .links
        .iter()
        .map(|l| {
            let (a, b) = (&g.rooms[l.a].id, &g.rooms[l.b].id);
            match l.means {
                Means::Open => json!({"from": a, "to": b, "kind": "open"}),
                Means::Swing => json!({"from": a, "to": b, "kind": "door", "operation": "swing"}),
                Means::Sliding => json!({"from": a, "to": b, "kind": "door", "operation": "sliding"}),
                Means::Folding => json!({"from": a, "to": b, "kind": "door", "operation": "folding"}),
            }
        })
        .collect();
    edges.push(json!({"from": g.rooms[g.entry].id, "to": "exterior", "kind": "door", "exterior": true, "operation": "swing"}));
    json!({"nodes": nodes, "edges": edges, "entry": {"room": g.rooms[g.entry].id}})
}

#[test]
fn every_sampled_graph_passes_the_house_pack() {
    let pack = Pack::embedded("house").unwrap();
    for seed in 0..500 {
        for area in AREAS {
            let g = sample(seed, area);
            let graph = RuleGraph::from_value(&rule_graph(&g)).unwrap();
            let failed: Vec<String> = graph_rules::check(pack, &graph)
                .unwrap()
                .into_iter()
                .filter(|c| c.status == graph_rules::Status::Fail)
                .map(|c| format!("{} {}", c.id, c.detail))
                .collect();
            assert!(failed.is_empty(), "seed {seed} area {area}: {failed:?}");
            // A porch is closed off by separation lines, which only a house with an open edge has.
            let opens = g.links.iter().filter(|l| l.means == Means::Open).count();
            assert!(g.porch.is_none() || opens > 0, "seed {seed}: a porch and no open edge");
            // One door to outside, into a room that is not the porch's.
            assert!(matches!(g.rooms[g.entry].kind, Kind::Living | Kind::Kitchen), "seed {seed}");
            assert!(g.porch.is_none() || g.entry != g.porch_off, "seed {seed}: the way in is off the porch's room");
        }
    }
}

#[test]
fn sampled_graphs_differ_in_kinds_and_links() {
    let hashes: BTreeSet<String> = (1..=40)
        .map(|seed| {
            let g = sample(seed, AREAS[(seed as usize - 1) % AREAS.len()]);
            let mut kinds: Vec<&str> = g.rooms.iter().map(|r| r.kind.name()).collect();
            kinds.sort_unstable();
            let mut links: Vec<String> = g
                .links
                .iter()
                .map(|l| {
                    let (a, b) = (g.rooms[l.a].kind.name(), g.rooms[l.b].kind.name());
                    format!("{}-{}-{:?}", a.min(b), a.max(b), l.means)
                })
                .collect();
            links.sort_unstable();
            fnv1a64(&format!("{kinds:?}{links:?}"))
        })
        .collect();
    assert!(hashes.len() >= 20, "{} distinct graphs", hashes.len());
}

#[test]
fn an_area_too_small_for_a_house_is_refused_and_names_what_to_change() {
    let e = house(1, 12.0).err().unwrap();
    assert_eq!(e.code, "unsatisfiable_area");
    assert!(e.message.contains("too small") && e.message.contains("m2"), "{e}");
    assert_eq!(house(1, f64::NAN).err().unwrap().code, "invalid_area");
}

/// The corners of the extracted outline and how much of its bounding box it fills.
fn outline(extracted: &Value) -> (usize, f64) {
    let f = &extracted["footprint"];
    let ring: Vec<[f64; 2]> = f["outline"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]).collect();
    (ring.len(), f["area_over_bbox"].as_f64().unwrap())
}

/// The shape of an outline: its turns, left or right, from the one that makes the least word, either way round.
fn turns(extracted: &Value) -> String {
    let ring: Vec<[f64; 2]> =
        extracted["footprint"]["outline"].as_array().unwrap().iter().map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]).collect();
    let n = ring.len();
    let word: Vec<char> = (0..n)
        .map(|i| {
            let (a, b, c) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            if (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) > 0.0 { 'L' } else { 'R' }
        })
        .collect();
    let spin = |w: &[char]| (0..w.len()).map(|i| w.iter().cycle().skip(i).take(w.len()).collect::<String>()).min().unwrap_or_default();
    let back: Vec<char> = word.iter().rev().copied().collect();
    spin(&word).min(spin(&back))
}

/// Builds a house's script in a project of its own and reads it back, as the done check does.
fn read_back(b: &Built) -> (Session, Value) {
    let mut s = Session::new();
    s.script(file(b, "script.bimcraft.txt")).unwrap();
    let extracted = crate::topology::extract(&s.model);
    (s, extracted)
}

#[test]
fn houses_build_and_the_extractor_reads_the_generated_graph_back() {
    let mut shells = BTreeMap::new();
    let (mut shapes, mut graphs, mut rooms) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let (mut ops, mut opens, mut porches) = (BTreeSet::new(), 0, 0);
    let mut failed = Vec::new();
    for seed in SHELL_SEEDS {
        let area = AREAS[(seed as usize - 1) % AREAS.len()];
        let b = match house(seed, area) {
            Ok(b) => b,
            Err(e) => {
                failed.push(format!("seed {seed} area {area}: {e}"));
                continue;
            }
        };
        let generated = doc(&b, "topology-graph.json");
        let (_, extracted) = read_back(&b);
        let ids = |v: &Value| -> BTreeSet<String> {
            v["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(ids(&generated), ids(&extracted), "seed {seed}");
        assert!(extracted["diagnostics"].as_array().unwrap().is_empty(), "seed {seed}: {}", extracted["diagnostics"]);
        // Every door is one family door, and the script has a room for each space and separators only for open edges.
        let script = file(&b, "script.bimcraft.txt");
        let count = |word: &str| script.lines().filter(|l| l.starts_with(word)).count();
        let doors = generated["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == "door").count();
        assert_eq!(count("door "), doors, "seed {seed}");
        assert_eq!(script.lines().filter(|l| l.starts_with("door ") && !l.contains("\"family\"")).count(), 0);
        assert_eq!(count("room "), generated["nodes"].as_array().unwrap().len(), "seed {seed}");
        let open_edges = generated["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == "open").count();
        assert_eq!(count("room_separator ") > 0, open_edges > 0, "seed {seed}");
        // The gross area is within 10% of the request.
        let gross = doc(&b, "manifest.json")["gross_area_m2"].as_f64().unwrap();
        assert!((gross - area).abs() <= area * 0.1, "seed {seed}: {gross} m2 for {area}");
        let shell = doc(&b, "manifest.json")["shell"].as_str().unwrap().to_string();
        let (corners, ratio) = outline(&extracted);
        if shell != "bar" {
            assert!(corners >= 6 && ratio <= 0.98, "seed {seed} {shell}: {corners} corners, {ratio}");
        }
        *shells.entry(shell).or_insert(0) += 1;
        shapes.insert(turns(&extracted));
        rooms.insert(generated["nodes"].as_array().unwrap().len());
        let kinds: BTreeMap<&str, &str> =
            generated["nodes"].as_array().unwrap().iter().map(|n| (n["id"].as_str().unwrap(), n["kind"].as_str().unwrap())).collect();
        let mut shape: Vec<String> = generated["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                ops.insert(e["operation"].as_str().unwrap_or("").to_string());
                let (a, b) = (
                    kinds.get(e["from"].as_str().unwrap()).copied().unwrap_or("exterior"),
                    kinds.get(e["to"].as_str().unwrap()).copied().unwrap_or("exterior"),
                );
                format!("{}-{}-{}", a.min(b), a.max(b), e["kind"])
            })
            .collect();
        shape.sort_unstable();
        let mut by_kind: Vec<&&str> = kinds.values().collect();
        by_kind.sort_unstable();
        graphs.insert(fnv1a64(&format!("{shape:?}{by_kind:?}")));
        opens += open_edges;
        porches += usize::from(kinds.values().any(|k| *k == "porch"));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(shells.len() >= 6, "{shells:?}");
    assert!(shapes.len() >= 3, "{shapes:?}");
    assert_eq!(graphs.len(), SHELL_SEEDS.len(), "every seed its own graph");
    assert!(rooms.len() >= 3, "{rooms:?}");
    assert!(ops.contains("swing") && ops.contains("sliding"), "{ops:?}");
    assert!(opens > 0 && porches > 0);
    assert!(SHELLS.len() >= 6);
}

#[test]
fn the_same_brief_gives_the_same_bytes() {
    let (a, b) = (house(7, 115.0).unwrap(), house(7, 115.0).unwrap());
    assert_eq!(a.files, b.files);
    assert_ne!(file(&a, "topology-graph.json"), file(&house(8, 115.0).unwrap(), "topology-graph.json"));
}

#[test]
fn sliding_doors_are_of_a_sliding_family_and_every_door_has_a_family() {
    for seed in 1..=10_u64 {
        let b = house(seed, 115.0).unwrap();
        let script = file(&b, "script.bimcraft.txt");
        let g = doc(&b, "topology-graph.json");
        let sliding = g["edges"].as_array().unwrap().iter().filter(|e| e["operation"] == "sliding").count();
        let slid = script.lines().filter(|l| l.starts_with("door ") && l.contains("Sliding Door")).count();
        assert_eq!(sliding, slid, "seed {seed}");
    }
}

#[test]
fn a_door_tag_a_neighbours_leaf_would_hide_under_is_moved() {
    // Seed 3 at 115 m2: D4's tag lay on D1's leaf, whose mask clipped it.
    let b = house(3, 115.0).unwrap();
    let script = file(&b, "script.bimcraft.txt");
    assert!(script.lines().any(|l| l.starts_with("tag {\"ids\"")), "{script}");
    assert!(script.lines().last().is_some_and(|l| !l.starts_with("tag")) || script.contains("tag {}"));
}

#[test]
fn swing_doors_carry_the_fields_the_extractor_reads_and_sliding_doors_none() {
    let (mut enforced, mut flipped) = (0, 0);
    for seed in SHELL_SEEDS {
        let area = AREAS[(seed as usize - 1) % AREAS.len()];
        let Ok(b) = house(seed, area) else { continue };
        let generated = doc(&b, "topology-graph.json");
        let (_, extracted) = read_back(&b);
        let key = |e: &Value| {
            let (a, b) = (e["from"].as_str().unwrap().to_string(), e["to"].as_str().unwrap().to_string());
            let width = e["width_mm"].as_f64().unwrap().round() as i64;
            (
                a.clone().min(b.clone()),
                a.max(b),
                e["operation"].as_str().unwrap().to_string(),
                width,
                e["opens_into"].clone(),
                e["hinge"].clone(),
            )
        };
        let doors = |g: &Value| {
            let mut v: Vec<_> = g["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == "door").map(key).collect();
            v.sort_by_key(|k| format!("{k:?}"));
            v
        };
        assert_eq!(doors(&generated), doors(&extracted), "seed {seed}");
        for e in generated["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == "door") {
            if e["operation"] == "swing" {
                assert!(e["opens_into"].is_string() && e["hinge"].is_string(), "seed {seed}: {e}");
            } else {
                assert!(e.get("opens_into").is_none() && e.get("hinge").is_none(), "seed {seed}: {e}");
            }
        }
        let script = file(&b, "script.bimcraft.txt");
        flipped += script.lines().filter(|l| l.starts_with("door ") && l.contains("flip_")).count();
        enforced += generated["edges"].as_array().unwrap().iter().filter(|e| e["opens_into"].is_string()).count();
    }
    assert!(enforced > 10 && flipped > 2, "{enforced} swing doors, {flipped} flipped");
}

/// The areas the Unit 1 done check generates at, and the seed's among them.
const SWEEP_AREAS: [f64; 6] = [90.0, 124.0, 158.0, 192.0, 226.0, 260.0];

#[test]
fn large_sampled_graphs_pass_the_house_pack() {
    let pack = Pack::embedded("house").unwrap();
    for seed in 0..300 {
        for area in [161.0, 192.0, 226.0, 260.0] {
            let g = sample(seed, area);
            let graph = RuleGraph::from_value(&rule_graph(&g)).unwrap();
            let failed: Vec<String> = graph_rules::check(pack, &graph)
                .unwrap()
                .into_iter()
                .filter(|c| c.status == graph_rules::Status::Fail)
                .map(|c| format!("{} {}", c.id, c.detail))
                .collect();
            assert!(failed.is_empty(), "seed {seed} area {area}: {failed:?}");
            // The way in is an entry hall, that the hall of the bedroom wing opens into.
            assert_eq!(g.rooms[g.entry].kind, Kind::Hall, "seed {seed}");
            assert_ne!(g.entry, g.hall, "seed {seed}");
            assert!(g.links.iter().any(|l| l.means == Means::Open && [l.a, l.b] == [g.hall, g.entry]), "seed {seed}");
            // A deck is off living and dining, or living alone, by sliding doors; a garage is a swing door off the entry.
            if let Some(d) = g.deck {
                let off: Vec<usize> = g.links.iter().filter(|l| l.b == d).map(|l| l.a).collect();
                assert_eq!(off, g.deck_off, "seed {seed}");
                assert!(g.links.iter().filter(|l| l.b == d).all(|l| l.means == Means::Sliding), "seed {seed}");
            }
            if let Some(gr) = g.rooms.iter().position(|r| r.kind == Kind::Garage) {
                let l = g.links.iter().find(|l| l.a == gr || l.b == gr).unwrap();
                assert!(l.means == Means::Swing && (l.a == g.entry || l.b == g.entry), "seed {seed}");
            }
        }
    }
}

#[test]
fn houses_of_the_smaller_family_have_no_garage_deck_or_folding_door() {
    for seed in 0..200 {
        for area in AREAS {
            let g = sample(seed, area);
            assert!(g.rooms.iter().all(|r| r.kind != Kind::Garage) && g.deck.is_none() && g.deck_off.is_empty(), "seed {seed} area {area}");
            assert!(g.links.iter().all(|l| l.means != Means::Folding), "seed {seed} area {area}");
        }
    }
}

/// What the done check reads off a built house's extracted graph: the Unit 1 layout is a public open band that the entry hall
/// opens into, an entry hall and a hall of the bedroom wing joined, and that hall with two or more bedrooms off it.
fn unit1_like(graph: &Value) -> bool {
    let kind: BTreeMap<&str, &str> =
        graph["nodes"].as_array().unwrap().iter().map(|n| (n["id"].as_str().unwrap(), n["kind"].as_str().unwrap())).collect();
    let entry = graph["entry"]["room"].as_str().unwrap();
    let links: Vec<(&str, &str, &str)> = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["exterior"] != true && (e["kind"] == "door" || e["kind"] == "open"))
        .map(|e| (e["from"].as_str().unwrap(), e["to"].as_str().unwrap(), e["kind"].as_str().unwrap()))
        .collect();
    let shared = |k: &str| matches!(k, "living" | "kitchen" | "dining");
    let next = |id: &str| -> Vec<&str> {
        links
            .iter()
            .filter_map(|(a, b, _)| {
                if *a == id {
                    Some(*b)
                } else if *b == id {
                    Some(*a)
                } else {
                    None
                }
            })
            .collect()
    };
    let other_halls: Vec<&str> = kind.iter().filter(|(id, k)| **k == "hall" && **id != entry).map(|(id, _)| *id).collect();
    let split = kind[entry] == "hall" && other_halls.iter().any(|h| next(entry).contains(h));
    let open_shared = links.iter().filter(|(a, b, t)| *t == "open" && shared(kind[a]) && shared(kind[b])).count();
    let band = links.iter().any(|(a, b, t)| *t == "open" && (*a == entry || *b == entry) && (shared(kind[a]) || shared(kind[b])));
    let wing = other_halls.iter().any(|h| next(h).iter().filter(|x| kind[*x] == "bedroom").count() >= 2);
    split && open_shared >= 2 && band && wing
}

#[test]
fn large_houses_build_and_read_back_and_the_sweep_has_each_unit1_concept() {
    let (mut garages, mut decks, mut folding, mut layouts) = (0, 0, 0, 0);
    // Seed 4 at 192 m2 has a garage, a deck, a folding door and the Unit 1 layout; seed 5 at 226 m2 is a second large house.
    for seed in [4_u64, 5] {
        let area = SWEEP_AREAS[(seed as usize - 1) % 6];
        let b = house(seed, area).unwrap_or_else(|e| panic!("seed {seed} area {area}: {e}"));
        let generated = doc(&b, "topology-graph.json");
        let (_, extracted) = read_back(&b);
        let ids = |v: &Value| -> BTreeSet<String> {
            v["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(ids(&generated), ids(&extracted), "seed {seed}");
        assert!(extracted["diagnostics"].as_array().unwrap().is_empty(), "seed {seed}: {}", extracted["diagnostics"]);
        // The doors and open edges extracted are the generated ones, by their kind of link.
        let links = |g: &Value| -> Vec<String> {
            let mut v: Vec<String> = g["edges"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "door" || e["kind"] == "open")
                .map(|e| {
                    let (a, b) = (e["from"].as_str().unwrap(), e["to"].as_str().unwrap());
                    format!("{}-{}-{}-{}", a.min(b), a.max(b), e["kind"], e["operation"])
                })
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(links(&generated), links(&extracted), "seed {seed}");
        let kinds: BTreeSet<&str> = generated["nodes"].as_array().unwrap().iter().map(|n| n["kind"].as_str().unwrap()).collect();
        garages += usize::from(kinds.contains("garage"));
        decks += usize::from(kinds.contains("deck"));
        let fold = generated["edges"].as_array().unwrap().iter().filter(|e| e["operation"] == "folding").collect::<Vec<_>>();
        for e in &fold {
            assert!(e["family"].as_str().unwrap().contains("Fold") && e.get("opens_into").is_none() && e.get("hinge").is_none(), "{e}");
        }
        folding += usize::from(!fold.is_empty());
        let mut built = extracted.clone();
        let kind_of: BTreeMap<String, String> = generated["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| (n["id"].as_str().unwrap().to_string(), n["kind"].as_str().unwrap().to_string()))
            .collect();
        for n in built["nodes"].as_array_mut().unwrap() {
            let k = kind_of[n["id"].as_str().unwrap()].clone();
            n["kind"] = json!(k);
        }
        layouts += usize::from(unit1_like(&built));
    }
    assert!(
        garages > 0 && decks > 0 && folding > 0 && layouts > 0,
        "garages {garages}, decks {decks}, folding {folding}, layouts {layouts}"
    );
}

/// A house of the smaller family with one closet's door made folding, as far as it builds.
#[test]
fn a_folding_closet_door_builds_and_the_extractor_tells_it_from_swing_and_sliding() {
    for seed in 1..=40_u64 {
        let mut g = sample(seed, 115.0);
        let Some(link) = g.links.iter().position(|l| g.rooms[l.b].kind == Kind::Closet && g.rooms[l.a].kind == Kind::Bedroom) else {
            continue;
        };
        g.links[link].means = Means::Folding;
        let area = AreaRequest::m2(115.0);
        for attempt in super::place::attempts(&g, seed, 115.0) {
            let Ok(mut layout) = attempt else { continue };
            if super::swing::orient(&g, &mut layout).is_err() {
                continue;
            }
            // A try that the rules or the room bounds refuse is the next's; the first that builds says what it says.
            let Ok(done) = super::emit::verify(&g, &layout, seed, area) else { continue };
            let graph = &done.graph;
            let door = graph["edges"].as_array().unwrap().iter().find(|e| e["operation"] == "folding").unwrap();
            assert!(door["family"].as_str().unwrap().contains("Fold"), "{door}");
            assert!(door.get("opens_into").is_none() && door.get("hinge").is_none(), "{door}");
            assert!(done.script.text.contains("\"operation\":\"folding\""), "the folding family is defined in the script");
            return;
        }
    }
    panic!("no house with a bedroom's closet built with it folding");
}
