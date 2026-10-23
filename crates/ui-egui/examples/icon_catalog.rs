//! Exports the icons' outlines for review, as `catalog.json`.
//! Run with `cargo run -p bimcraft-ui-egui --example icon_catalog`.

use std::path::PathBuf;

use egui::{Color32, Context, RawInput, Rect, pos2, vec2};
use serde_json::{Value, json};

use bimcraft_ui_egui::icons;

fn contours(icon: icons::Icon, compact: bool) -> Value {
    Value::Array(
        icons::outlines(icon, compact)
            .into_iter()
            .map(|outline| json!({"closed": outline.closed, "points": outline.points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()}))
            .collect(),
    )
}

fn run() -> Result<(), String> {
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../output/icon-catalog"));
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    // Exercise the egui painter the app uses as well as exporting the source contours.
    let context = Context::default();
    let mut frame = context.run_ui(RawInput::default(), |ui| {
        let painter = ui.painter();
        for &icon in icons::Icon::ALL {
            for side in [15.0, 18.0, 19.0, 22.0, 24.0] {
                icons::paint(painter, icon, Rect::from_min_size(pos2(2.0, 2.0), vec2(side, side)), Color32::WHITE);
            }
        }
    });
    frame.textures_delta.clear();
    let drawings: Vec<_> = icons::Icon::ALL
        .iter()
        .map(|&icon| json!({"name": icon.name(), "regular": contours(icon, false), "compact": contours(icon, true)}))
        .collect();
    let catalog = json!({"grid": 24, "stroke": icons::STROKE_WIDTH, "compact_max_size": 19, "icons": drawings});
    let file = directory.join("catalog.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&catalog).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("Exported {} icons to {}", icons::Icon::ALL.len(), file.display());
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
