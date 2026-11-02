//! Build Shell: the outer walls, the floors at each storey and the roof of the masses selected, by `building_shell`. The buildings are
//! the masses an OpenStreetMap import made (their tags give the storeys and the roof), or any mass.

use egui::{Align, Button, Context, Grid, Id as UiId, Layout, RichText, TextEdit};
use serde_json::{Value, json};

use super::Said;
use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// The Build Shell box: what is to be made, until OK or Cancel.
pub(crate) struct ShellBox {
    /// The masses it is for.
    masses: Vec<u64>,
    /// A storey's height in the project's units; blank is what each building's levels say, else 3 m.
    storey_height: String,
    walls: bool,
    floors: bool,
    roof: bool,
    error: Option<String>,
}

impl BimApp {
    /// Opens the box for the masses selected.
    pub(crate) fn open_shell(&mut self) {
        let masses: Vec<u64> = self
            .selection
            .iter()
            .copied()
            .filter(|id| matches!(self.session.model.elements.get(id), Some(bimcraft_core::model::Element::Mass(_))))
            .collect();
        if masses.is_empty() {
            self.status = crate::Status { text: t("Select the buildings (masses) first.").to_string(), error: true };
            return;
        }
        self.site.shell = Some(ShellBox { masses, storey_height: String::new(), walls: true, floors: true, roof: true, error: None });
    }

    pub(crate) fn shell_window(&mut self, ctx: &Context) {
        let unit = self.session.model.units.name();
        let Some(b) = self.site.shell.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("site-shell"), t("Build Shell")).width(460.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(tf("The outer walls, floors and roof of {} buildings.", &[&b.masses.len().to_string()]));
                ui.add_space(6.0);
                Grid::new("site-shell-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label(t("Storey height"));
                    ui.add_sized([220.0, 20.0], TextEdit::singleline(&mut b.storey_height).hint_text(unit)).labelled_by(l.id);
                    ui.end_row();
                });
                ui.label(RichText::new(t("Blank: from each building's levels, else 3 m.")).color(pal().muted));
                ui.add_space(4.0);
                ui.checkbox(&mut b.walls, t("Outer walls"));
                ui.checkbox(&mut b.floors, t("Floors at each storey"));
                ui.checkbox(&mut b.roof, t("Roof (the shape the OpenStreetMap tags give, else flat)"));
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(b.walls || b.floors || b.roof, Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok {
            self.build_shell();
        } else if close || r.should_close() {
            self.site.shell = None;
        }
    }

    /// Runs `building_shell` for the box. A refusal stays in the box; what it made is said in another.
    fn build_shell(&mut self) {
        let Some(b) = self.site.shell.as_ref() else { return };
        let units = self.session.model.units;
        let mut params = json!({"ids": b.masses, "walls": b.walls, "floors": b.floors, "roof": b.roof});
        if !b.storey_height.trim().is_empty() {
            match units.parse(&b.storey_height) {
                Some(h) => params["storey_height"] = json!(h),
                None => {
                    let e = tf("Storey height: `{}` is not a length", &[b.storey_height.trim()]);
                    if let Some(b) = self.site.shell.as_mut() {
                        b.error = Some(e);
                    }
                    return;
                }
            }
        }
        match self.run("building_shell", params) {
            Some(v) => {
                self.site.shell = None;
                self.say_shell(&v);
            }
            None => {
                let e = std::mem::take(&mut self.status).text;
                if let Some(b) = self.site.shell.as_mut() {
                    b.error = Some(e);
                }
            }
        }
    }

    /// The box that says what the shell made, and what was not as asked.
    fn say_shell(&mut self, v: &Value) {
        let n = |k: &str| v[k].as_u64().unwrap_or(0).to_string();
        let mut lines = vec![tf("{} walls, {} floors and {} roofs", &[&n("walls"), &n("floors"), &n("roofs")])];
        let skipped = v["skipped"].as_array().map_or(0, Vec::len);
        if skipped > 0 {
            lines.push(tf(
                "{} buildings already have walls, floors or a roof made by face: Update to Face makes them follow the mass",
                &[&skipped.to_string()],
            ));
        }
        let notes = v["notes"].as_array().into_iter().flatten().filter_map(Value::as_str);
        lines.extend(notes.clone().take(8).map(str::to_string));
        let more = notes.count().saturating_sub(8);
        if more > 0 {
            lines.push(tf("… and {} more", &[&more.to_string()]));
        }
        self.site.said = Some(Said { title: t("Build Shell").to_string(), lines, credit: None, query: None });
    }
}
