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

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut report_path = None;
    let mut imported = None;
    let mut storeys = 20u32;
    let mut repeats = 5usize;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--input" => input = Some(value),
            "--output" => output = Some(value),
            "--report" => report_path = Some(value),
            "--storeys" => storeys = value.parse().map_err(|e| format!("storeys: {e}"))?,
            "--repeat" => repeats = value.parse().map_err(|e| format!("repeat: {e}"))?,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    if !(1..=50).contains(&repeats) {
        return Err("repeat must be 1–50".into());
    }
    let mut report = json!({"version": env!("CARGO_PKG_VERSION"), "fixture": if input.is_some() {"private"} else {"synthetic-highrise"},
        "logical_cpus": std::thread::available_parallelism().map_or(1, usize::from), "resolution": [1280, 720], "ms": {}});
    let mut s = Session::new();
    let result = (|| {
        timed(&mut report, "load", || {
            if let Some(path) = input {
                imported = Some(s.execute("open", &json!({"path": path}))?);
            } else {
                s.execute("highrise", &json!({"storeys": storeys}))?;
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
        timed(&mut report, "check", || s.model.check())?;
        timed(&mut report, "inspect_command", || s.execute("inspect", &json!({})))?;
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
        let level = s.model.levels().first().map(|(id, _)| *id).ok_or("no levels")?;
        let d = timed(&mut report, "plan_fit_draw", || Ok(render::plan(&s.model, level, 1280, 720)))?;
        report["plan_primitives"] = json!(d.prims.len());
        timed(&mut report, "plan_redraw", || Ok(black_box(render::draw(&s.model, level, d.view))))?;
        let png = timed(&mut report, "plan_png", || render::png(&d))?;
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
        if let Some(dir) = output {
            let dir = std::path::Path::new(dir);
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("highrise.bimcraft"), text).map_err(|e| e.to_string())?;
            let ifc = timed(&mut report, "ifc_export", || Ok(bimcraft_core::ifc::export(&s.model)))?;
            std::fs::write(dir.join("highrise.ifc"), ifc).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("plan.png"), png).map_err(|e| e.to_string())?;
            if let Some(image) = last {
                std::fs::write(dir.join("3d.png"), image.png()?).map_err(|e| e.to_string())?;
            }
        }
        Ok::<_, String>(())
    })();
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

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
