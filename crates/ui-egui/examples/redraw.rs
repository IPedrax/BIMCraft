//! Measure CPU UI frames after the initial view is ready; excludes GPU presentation.
use std::time::Instant;

use bimcraft_core::engine::Session;
use bimcraft_ui_egui::{BimApp, Sheet};
use egui_kittest::Harness;
use serde_json::json;

fn run() -> Result<(), String> {
    let mut s = Session::new();
    s.execute("highrise", &json!({}))?;
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    let mut report = json!({"fixture":"synthetic-highrise", "resolution":[1280,800], "ms":{}});
    for (name, sheet) in [("plan", h.state().sheet), ("3d", Sheet::View3D)] {
        h.state_mut().sheet = sheet;
        h.run();
        for hover in [false, true] {
            let mut frames = Vec::new();
            for i in 0..51 {
                if hover {
                    h.hover_at(egui::pos2(600.0 + i as f32 * 3.0, 400.0));
                }
                h.ctx.request_repaint();
                let start = Instant::now();
                h.step();
                if i > 0 {
                    frames.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            frames.sort_by(f64::total_cmp);
            report["ms"][format!("{name}_{}_median", if hover { "hover" } else { "idle" })] = json!(frames[frames.len() / 2]);
            report["ms"][format!("{name}_{}_p95", if hover { "hover" } else { "idle" })] =
                json!(frames[(frames.len() * 95).div_ceil(100) - 1]);
            report["ms"][format!("{name}_{}_max", if hover { "hover" } else { "idle" })] = json!(frames[frames.len() - 1]);
        }
    }
    println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
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
