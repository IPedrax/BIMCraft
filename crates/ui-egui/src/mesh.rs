//! Insert > Import 3D Model: an OBJ or glTF file's objects brought into the project as Generic Model families, each placed
//! where the file has it on the level of the plan on screen (`import_mesh`), and selected. An OBJ is taken in metres with y up,
//! as most programs write one; agents and scripts give `units` and `up` for others.

use std::path::PathBuf;

use egui::Context;
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};

impl BimApp {
    /// Insert > Import 3D Model: the file the user picks, brought in.
    pub(crate) fn import_mesh_dialog(&mut self, ctx: &Context) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("3D models (OBJ, glTF)"), &["obj", "gltf", "glb"]);
        self.ask_files(ctx, dialog, |app, paths| {
            if let Some(path) = paths.into_iter().next() {
                app.import_mesh_path(path);
            }
        });
    }

    /// Brings in the objects of the file at `path`, on the level of the plan on screen, and selects them; says on the status
    /// bar how many came in and what was left out.
    pub(crate) fn import_mesh_path(&mut self, path: PathBuf) {
        let mut p = json!({"path": path.to_string_lossy()});
        if let Some(level) = self.plan_level() {
            p["level"] = json!(level);
        }
        let Some(v) = self.run("import_mesh", p) else { return };
        self.selection = v["components"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        let left: Vec<String> =
            v["left_out"].as_object().into_iter().flatten().map(|(what, n)| format!("{} {what}", n.as_u64().unwrap_or(0))).collect();
        let mut text = tf("Brought in {} objects as Generic Model families.", &[&self.selection.len().to_string()]);
        if !left.is_empty() {
            text.push(' ');
            text.push_str(&tf("Left out: {}.", &[&left.join(", ")]));
        }
        self.status = crate::Status { text, error: false };
    }
}
