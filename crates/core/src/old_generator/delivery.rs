//! `generate --delivery on`: the plan a generator built, again to the delivery drawing rules. The layout is the generator's own and
//! untouched; only what is drawn changes. The script gains the `cad_profile` line, a room is numbered `101` upward in the order of
//! the graph's nodes, and what a delivery drawing may not hold (dimensions) is left out. The graph, the building and the
//! manifest are written again to match. Detail: `docs/areas/graph-generator.md`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;

use super::program::Archetype;
use super::{Built, MANIFEST, Refusal, fnv1a64, pretty, refuse};
use crate::generate::shape_graph;

/// The most rooms a delivery plan numbers: `101` to `999`.
const MAX_NUMBERED: usize = 899;

pub(super) fn text_of<'a>(built: &'a Built, name: &str) -> Result<&'a str, Refusal> {
    built
        .files
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| t.as_str())
        .ok_or_else(|| refuse("generation_failed", format!("the plan has no {name}")))
}

pub(super) fn json_of(built: &Built, name: &str) -> Result<Value, Refusal> {
    serde_json::from_str(text_of(built, name)?).map_err(|e| refuse("generation_failed", format!("{name} is not JSON: {e}")))
}

/// The number a node is drawn with: `101` upward, in node order.
pub(crate) fn number_of(index: usize) -> String {
    (101 + index).to_string()
}

/// The name a `type` line gives.
fn type_name(params: &str) -> Option<String> {
    serde_json::from_str::<Value>(params).ok()?["name"].as_str().map(str::to_string)
}

/// The door types of `script` that are a door by operation only, each with the family that draws that operation (a quarter
/// circle for a leaf's swing, two for a double door, none for a sliding one), its width and height. The delivery drawing's door
/// symbols come from a family, so these go through one.
fn door_types_of(script: &str) -> BTreeMap<String, (&'static str, i64, i64)> {
    let mut out = BTreeMap::new();
    for line in script.lines() {
        let Some(("type", params)) = line.split_once(' ') else { continue };
        let Ok(p) = serde_json::from_str::<Value>(params) else { continue };
        if p["category"] != "door" {
            continue;
        }
        let family = match p["operation"].as_str() {
            Some("swing") => "Single Door",
            Some("double-swing") => "Double Door",
            Some("sliding") => "Sliding Door",
            _ => continue,
        };
        if let (Some(name), Some(w), Some(h)) = (p["name"].as_str(), p["width"].as_i64(), p["height"].as_i64()) {
            out.insert(name.to_string(), (family, w, h));
        }
    }
    out
}

/// The delivery script of `script`: the profile after the drawing property, no dimensions, rooms numbered by their node.
fn rewrite_script(script: &str, node_index: &BTreeMap<String, usize>, seed: u64, note_m2: Option<f64>) -> Result<String, Refusal> {
    let mut out = String::with_capacity(script.len() + 64);
    let mut profile_done = false;
    let (mut doors, mut windows) = (0usize, 0usize);
    let door_types = door_types_of(script);
    let mut families_done = false;
    let mut corner: Option<[i64; 2]> = None;
    for line in script.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let (id, params) = line.split_once(' ').unwrap_or((line, "{}"));
        match id {
            "dimension" => continue,
            // The seed makes the project's GUIDs the seed's, so one script saves the same model every time.
            "new" => {
                let mut p: Value = serde_json::from_str(params)
                    .map_err(|e| refuse("generation_failed", format!("the `new` line of the script is not JSON: {e}")))?;
                if p.get("seed").is_none() {
                    p["seed"] = json!(seed);
                }
                out.push_str(&format!("new {p}\n"));
                continue;
            }
            "cad_profile" => return Err(refuse("generation_failed", "the plan's script already names a CAD profile")),
            "room" => {
                let mut p: Value = serde_json::from_str(params)
                    .map_err(|e| refuse("generation_failed", format!("a room line of the script is not JSON: {e}")))?;
                let old = p["number"].as_str().unwrap_or_default().to_string();
                let index = node_index.get(&old).copied().or_else(|| {
                    // The corridor generators number rooms 101 upward already, in node order.
                    old.parse::<usize>().ok().and_then(|n| n.checked_sub(101)).filter(|i| *i < node_index.len())
                });
                let index = index.ok_or_else(|| refuse("generation_failed", format!("room {old:?} is no node of the graph")))?;
                p["number"] = json!(number_of(index));
                out.push_str(&format!("room {p}\n"));
                continue;
            }
            "type" if door_types.contains_key(type_name(params).as_deref().unwrap_or_default()) => {
                if !families_done {
                    families_done = true;
                    let mut by_family: BTreeMap<&str, serde_json::Map<String, Value>> = BTreeMap::new();
                    for (name, (family, width, height)) in &door_types {
                        by_family.entry(family).or_default().insert(name.clone(), json!({"Width": width, "Height": height}));
                    }
                    for (family, types) in by_family {
                        out.push_str(&format!("family {}\n", json!({"name": family, "types": types})));
                    }
                }
                continue;
            }
            "door" | "window" => {
                // A mark is a letter and a number (`D4`, `W2`): the drawing's door and window tags say nothing else.
                let mut p: Value = serde_json::from_str(params)
                    .map_err(|e| refuse("generation_failed", format!("a {id} line of the script is not JSON: {e}")))?;
                let (letter, count) = if id == "door" { ('D', &mut doors) } else { ('W', &mut windows) };
                *count += 1;
                p["mark"] = json!(format!("{letter}{count}"));
                if id == "door"
                    && p.get("family").is_none()
                    && let Some((family, _, _)) = p["type"].as_str().and_then(|t| door_types.get(t))
                {
                    p["family"] = json!(family);
                }
                out.push_str(&format!("{id} {p}\n"));
                continue;
            }
            _ => {
                if id == "wall"
                    && let Ok(p) = serde_json::from_str::<Value>(params)
                {
                    for pt in p["points"].as_array().into_iter().flatten() {
                        if let (Some(x), Some(y)) = (pt[0].as_i64(), pt[1].as_i64()) {
                            let c = corner.get_or_insert([x, y]);
                            *c = [c[0].min(x), c[1].min(y)];
                        }
                    }
                }
            }
        }
        out.push_str(line);
        out.push('\n');
        if id == "drawing_property" && !profile_done {
            out.push_str("cad_profile {\"name\":\"delivery\"}\n");
            profile_done = true;
        }
    }
    if !profile_done {
        return Err(refuse("generation_failed", "the plan's script has no `drawing_property` line to put the delivery profile after"));
    }
    // A plan with no note (the medical-day archetype's) gets the first one, which says its gross area as the others do: a reader
    // who wants the brief's area finds it there. It stands inside the plan's box, in its lower left corner, because a note below the
    // plan would move the drawing's origin, and the layout must stay where it is.
    if let (Some(m2), Some([x, y])) = (note_m2, corner) {
        let text = format!("Medical day of {} m2 gross, seed {seed}", m2.round());
        out.push_str(&format!("text {}\n", json!({"text": text, "at": [x + 300, y + 300]})));
    }
    Ok(out)
}

/// A corridor generator's graph names its lobby as the entry, while its drawing puts the main entry door on the corridor (the
/// lobby is a room off it). The delivery graph must say what the drawing says, so its entry becomes the room the saved model's entry
/// is. This is not a rewrite to whatever comes out: the extractor's entry is the room the widest swing (or double-swing) exterior
/// door opens into, which the tests check is the main entry door, never a service exit.
fn set_entry_as_drawn(topology: &mut Value, script: &str, ids: &[String]) -> Result<(), Refusal> {
    let extracted = crate::ids::keeping_reproducible(|| {
        let mut scratch = Session::new();
        scratch
            .script(script)
            .map(|_| crate::topology::extract(&scratch.model))
            .map_err(|e| refuse("generation_failed", format!("the delivery script does not run: {e}")))
    })?;
    let Some(number) = extracted["entry"]["room"].as_str() else { return Ok(()) };
    let Some(id) = number.parse::<usize>().ok().and_then(|n| n.checked_sub(101)).and_then(|i| ids.get(i)) else { return Ok(()) };
    match &mut topology["entry"] {
        Value::String(s) => *s = id.clone(),
        Value::Object(o) => {
            o.insert("room".into(), json!(id));
        }
        _ => {}
    }
    Ok(())
}

/// `built` to the delivery drawing rules: the same layout, a delivery script, graph and manifest.
pub(crate) fn apply(mut built: Built) -> Result<Built, Refusal> {
    let mut topology = json_of(&built, "topology-graph.json")?;
    let ids: Vec<String> = topology["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().map(|n| n["id"].as_str().unwrap_or_default().to_string()).collect())
        .unwrap_or_default();
    if ids.len() > MAX_NUMBERED {
        return Err(refuse("generation_failed", format!("a delivery plan numbers its rooms 101 to 999, and this one has {}", ids.len())));
    }
    let node_index: BTreeMap<String, usize> = ids.iter().enumerate().map(|(i, id)| (id.clone(), i)).collect();
    let note = (built.archetype == Archetype::MedicalDay).then_some(built.gross_m2);
    let script = rewrite_script(text_of(&built, "script.bimcraft.txt")?, &node_index, built.seed, note)?;
    if let Some(nodes) = topology["nodes"].as_array_mut() {
        for (i, n) in nodes.iter_mut().enumerate() {
            n["number"] = json!(number_of(i));
        }
    }
    set_entry_as_drawn(&mut topology, &script, &ids)?;
    let topology_text = pretty(&topology)?;
    let topology_hash = fnv1a64(&topology_text);

    let mut building = json_of(&built, "building-ir.json")?;
    building["source_2d"]["topology_hash"] = json!(topology_hash);
    let mut manifest = json_of(&built, MANIFEST)?;
    let lines: Vec<(String, Value)> = script
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (id, params) = l.split_once(' ').unwrap_or((l, "{}"));
            (id.to_string(), serde_json::from_str(params).unwrap_or(Value::Null))
        })
        .collect();
    manifest["delivery"] = json!(true);
    manifest["building"] = json!(shape_graph::building_id(built.archetype.name(), &lines));
    manifest["topology_hash"] = json!(topology_hash);

    let mut new_texts: BTreeMap<&'static str, String> = BTreeMap::new();
    new_texts.insert("topology-graph.json", topology_text);
    new_texts.insert("building-ir.json", pretty(&building)?);
    new_texts.insert("script.bimcraft.txt", script);
    for (name, text) in &mut built.files {
        if let Some(t) = new_texts.remove(name) {
            *text = t;
        }
    }
    // The manifest names every file by its hash.
    if let Some(files) = manifest["files"].as_array_mut() {
        for f in files {
            let path = f["path"].as_str().unwrap_or_default().to_string();
            if let Some((_, text)) = built.files.iter().find(|(n, _)| *n == path) {
                f["hash"] = json!(fnv1a64(text));
            }
        }
    }
    let manifest_text = pretty(&manifest)?;
    for (name, text) in &mut built.files {
        if *name == MANIFEST {
            *text = manifest_text.clone();
        }
    }
    built.topology_hash = topology_hash;
    Ok(built)
}
