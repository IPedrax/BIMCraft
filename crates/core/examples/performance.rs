//! Release-mode performance probe. Private inputs and outputs should stay in test_model/.
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

use bimcraft_core::{engine::Session, model::Element, render, view3d};
use serde_json::{Value, json};

fn timed<T>(report: &mut Value, name: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    eprintln!("measuring {name}");
    let start = Instant::now();
    let result = f();
    report["ms"][name] = json!(start.elapsed().as_secs_f64() * 1000.0);
    result
}

/// The process's resident memory and its peak, MB (Linux; none elsewhere).
fn memory() -> Option<(f64, f64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let field =
        |name: &str| status.lines().find(|l| l.starts_with(name))?.split_whitespace().nth(1)?.parse::<f64>().ok().map(|kb| kb / 1024.0);
    Some((field("VmRSS:")?, field("VmHWM:")?))
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut report_path = None;
    let mut imported = None;
    let mut storeys = None;
    let mut repeats = 5usize;
    let mut fixture = "highrise".to_string();
    let mut skip: Vec<String> = Vec::new();
    let mut room_sample = 200usize;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--input" => input = Some(value),
            "--output" => output = Some(value),
            "--report" => report_path = Some(value),
            "--storeys" => storeys = Some(value.parse::<u32>().map_err(|e| format!("storeys: {e}"))?),
            "--fixture" => fixture = value.clone(),
            // Steps to leave out, by name (inspect, rooms, plan, scene, ifc, reopen): what doesn't finish at a size.
            "--skip" => skip = value.split(',').map(str::to_string).collect(),
            "--rooms" => room_sample = value.parse().map_err(|e| format!("rooms: {e}"))?,
            "--repeat" => repeats = value.parse().map_err(|e| format!("repeat: {e}"))?,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    if !(1..=50).contains(&repeats) {
        return Err("repeat must be 1–50".into());
    }
    if !["highrise", "tower"].contains(&fixture.as_str()) {
        return Err(format!("--fixture is highrise or tower, not {fixture}"));
    }
    let skipped = |step: &str| skip.iter().any(|s| s == step);
    let fixture_name = if input.is_some() { "private".to_string() } else { format!("synthetic-{fixture}") };
    let mut report = json!({"version": env!("CARGO_PKG_VERSION"), "fixture": fixture_name, "skipped": skip,
        "logical_cpus": std::thread::available_parallelism().map_or(1, usize::from), "resolution": [1280, 720], "ms": {}});
    let mut s = Session::new();
    let result = (|| {
        timed(&mut report, "load", || {
            if let Some(path) = input {
                imported = Some(s.execute("open", &json!({"path": path}))?);
            } else {
                let storeys = storeys.unwrap_or(if fixture == "tower" { 100 } else { 20 });
                s.execute(&fixture, &json!({"storeys": storeys}))?;
            }
            Ok(())
        })?;
        if let Some(reply) = imported {
            report["import_skipped_notices"] = json!(reply["skipped"].as_array().map_or(0, Vec::len));
        }
        let mut counts = BTreeMap::new();
        for e in s.model.elements.values() {
            *counts.entry(e.category()).or_insert(0usize) += 1;
        }
        report["counts"] = json!(counts);
        report["elements"] = json!(s.model.elements.len());
        report["levels"] = json!(s.model.levels().len());
        if let Some((now, peak)) = memory() {
            report["memory_mb"] = json!({"after_load": now, "peak": peak});
        }
        timed(&mut report, "check", || s.model.check())?;
        if !skipped("rooms") {
            // Each room's boundary as the model finds it: a sample of the rooms, spread over the storeys, and what every
            // room would take at that rate (what `inspect`, schedules and IFC export ask for).
            let rooms: Vec<_> = s
                .model
                .elements
                .values()
                .filter_map(|e| match e {
                    Element::Room(r) => Some(r),
                    _ => None,
                })
                .collect();
            let step = (rooms.len() / room_sample.max(1)).max(1);
            let sample: Vec<_> = rooms.iter().step_by(step).take(room_sample).collect();
            let start = Instant::now();
            let enclosed = sample.iter().filter(|r| black_box(s.model.room_region(r)).is_some()).count();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let each = ms / sample.len().max(1) as f64;
            report["rooms"] = json!({"count": rooms.len(), "sampled": sample.len(), "enclosed": enclosed});
            report["ms"]["room_boundary_each"] = json!(each);
            report["ms"]["room_boundaries_all_estimated"] = json!(each * rooms.len() as f64);
        }
        if !skipped("inspect") {
            timed(&mut report, "inspect_command", || s.execute("inspect", &json!({})))?;
        }
        let editable = s.model.elements.iter().find(|(_, e)| e.column().is_some() || matches!(e, Element::Wall(_))).map(|(id, _)| *id);
        if let Some(id) = editable {
            let mut edits = Vec::new();
            for _ in 0..repeats {
                let start = Instant::now();
                s.execute("move", &json!({"ids": [id], "by": [10, 0]}))?;
                edits.push(start.elapsed().as_secs_f64() * 1000.0);
                s.execute("undo", &json!({}))?;
            }
            edits.sort_by(f64::total_cmp);
            report["ms"]["edit_median"] = json!(edits[edits.len() / 2]);
            report["edit_samples_ms"] = json!(edits);
        }
        let text = timed(&mut report, "native_serialize", || s.model.to_json())?;
        report["native_bytes"] = json!(text.len());
        if !skipped("reopen") {
            let reopened = timed(&mut report, "native_read", || bimcraft_core::model::Model::from_json(&text))?;
            if reopened.elements.len() != s.model.elements.len() {
                return Err("the project read back has another number of elements".into());
            }
        }
        let level = s.model.levels().first().map(|(id, _)| *id).ok_or("no levels")?;
        if skipped("plan") {
            return finish(&mut report, &s, output, text, None, None);
        }
        let d = timed(&mut report, "plan_fit_draw", || Ok(render::plan(&s.model, level, 1280, 720)))?;
        report["plan_primitives"] = json!(d.prims.len());
        timed(&mut report, "plan_redraw", || Ok(black_box(render::draw(&s.model, level, d.view))))?;
        let png = timed(&mut report, "plan_png", || render::png(&d))?;
        if skipped("scene") {
            return finish(&mut report, &s, output, text, Some(png), None);
        }
        let scene = timed(&mut report, "scene_build", || Ok(view3d::Scene::new(&s.model)))?;
        let cam = scene.fit(view3d::AZIMUTH, view3d::ELEVATION, 1280, 720);
        let mut frames = Vec::new();
        let mut last = None;
        for i in 0..repeats + 1 {
            let start = Instant::now();
            let image = black_box(scene.render(&cam.orbit(i as f64, 0.0), &[]));
            if i > 0 {
                frames.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            last = Some(image);
        }
        frames.sort_by(f64::total_cmp);
        report["ms"]["frame_median"] = json!(frames[frames.len() / 2]);
        report["frame_samples_ms"] = json!(frames);
        let picture = last.map(|image| image.png()).transpose()?;
        finish(&mut report, &s, output, text, Some(png), picture)
    })();
    if let Some((_, peak)) = memory() {
        report["memory_mb"]["peak"] = json!(peak);
    }
    // Keep partial reports on import failures. Errors can include private names;
    // keep reports from private inputs inside the ignored fixture directory.
    if let Err(e) = &result {
        report["error"] = json!(e);
    }
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    if let Some(path) = report_path {
        std::fs::write(path, &text).map_err(|e| e.to_string())?;
    }
    println!("{text}");
    result
}

/// The files, with `--output`: the project, its IFC (unless skipped), the plan and the 3D picture.
fn finish(
    report: &mut Value,
    s: &Session,
    output: Option<&String>,
    text: String,
    plan: Option<Vec<u8>>,
    picture: Option<Vec<u8>>,
) -> Result<(), String> {
    let Some(dir) = output else { return Ok(()) };
    let dir = std::path::Path::new(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = report["fixture"].as_str().unwrap_or("model").trim_start_matches("synthetic-").to_string();
    std::fs::write(dir.join(format!("{name}.bimcraft")), text).map_err(|e| e.to_string())?;
    let skipped = report["skipped"].as_array().is_some_and(|a| a.iter().any(|s| s == "ifc"));
    if !skipped {
        let ifc = timed(report, "ifc_export", || Ok(bimcraft_core::ifc::export(&s.model)))?;
        std::fs::write(dir.join(format!("{name}.ifc")), ifc).map_err(|e| e.to_string())?;
    }
    if let Some(png) = plan {
        std::fs::write(dir.join("plan.png"), png).map_err(|e| e.to_string())?;
    }
    if let Some(png) = picture {
        std::fs::write(dir.join("3d.png"), png).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
