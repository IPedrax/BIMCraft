//! `generate --gridlines` for a plan by `--area`: the plan an archetype built, with its grid left out (`off`) or, for the one
//! archetype that draws none, put in at its outline's corners (`on`). It runs only when the value departs from what the archetype
//! draws, so the default files are never touched. Everything but the grid lines stays as it was: the walls, doors, windows and
//! rooms, and the three graph files. Detail: `docs/areas/graph-generator.md`.

use serde_json::{Value, json};

use crate::engine::Session;

use super::delivery::{json_of, text_of};
use super::script::{outline_grids, ring_corners};
use super::{Built, MANIFEST, Refusal, fnv1a64, pretty, refuse};

/// The script with its `grid` lines taken out, every other line as it was.
fn without_grids(script: &str) -> String {
    let mut out = String::with_capacity(script.len());
    for line in script.lines() {
        if line.starts_with("grid ") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The script with the outline's grid put in after its `level` line; the outline is the `floor` line's boundary.
fn with_outline_grids(script: &str) -> Result<String, Refusal> {
    let params_of = |id: &str| -> Option<Value> {
        script.lines().find_map(|l| l.strip_prefix(id).and_then(|p| p.strip_prefix(' ')).and_then(|p| serde_json::from_str(p).ok()))
    };
    let boundary: Vec<[i64; 2]> = params_of("floor")
        .and_then(|p| p["boundary"].as_array().cloned())
        .map(|pts| pts.iter().filter_map(|p| Some([p[0].as_i64()?, p[1].as_i64()?])).collect())
        .unwrap_or_default();
    let grids = outline_grids(&ring_corners(&boundary));
    if grids.is_empty() {
        return Err(refuse("generation_failed", "the plan's script has no floor outline to put gridlines on"));
    }
    let mut out = String::with_capacity(script.len() + 64 * grids.len());
    let mut done = false;
    for line in script.lines() {
        out.push_str(line);
        out.push('\n');
        if !done && line.starts_with("level ") {
            for g in &grids {
                out.push_str(&format!("grid {g}\n"));
            }
            done = true;
        }
    }
    if !done {
        return Err(refuse("generation_failed", "the plan's script has no `level` line to put gridlines after"));
    }
    Ok(out)
}

/// `built` with its gridlines `on` or `off`, the layout untouched. Only for a value other than the archetype's default.
pub(crate) fn apply(mut built: Built, on: bool) -> Result<Built, Refusal> {
    let script = text_of(&built, "script.bimcraft.txt")?;
    let script = if on { with_outline_grids(script)? } else { without_grids(script) };
    // The edited script must still run: a dimension or a tag may have leaned on a grid line.
    crate::ids::keeping_reproducible(|| Session::new().script(&script)).map_err(|e| {
        refuse("generation_failed", format!("the script does not run with gridlines {}: {e}", if on { "on" } else { "off" }))
    })?;

    let mut report = json_of(&built, "report.json")?;
    let note = if on {
        "Gridlines were put in by `--gridlines on`: a numbered vertical line at each distinct x of the outline's corners and a lettered horizontal line at each distinct y."
    } else {
        "The plan's gridlines were left out by `--gridlines off`; nothing else changed."
    };
    if let Some(notes) = report["capability_notes"].as_array_mut() {
        if on {
            for n in notes.iter_mut() {
                if let Some(s) = n.as_str() {
                    *n = json!(s.replace("No windows, grids, dimensions", "No windows, dimensions"));
                }
            }
        }
        notes.push(json!(note));
    }
    let mut manifest = json_of(&built, MANIFEST)?;
    manifest["gridlines"] = json!(on);

    for (name, text) in &mut built.files {
        match *name {
            "script.bimcraft.txt" => *text = script.clone(),
            "report.json" => *text = pretty(&report)?,
            _ => {}
        }
    }
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
    Ok(built)
}
