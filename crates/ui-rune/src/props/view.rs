//! The plan's own Graphics rows, for the palette with nothing selected: its scale (from the units' list, or typed as 1 : n), its detail
//! level and its north, the view template it follows, and the buttons of what a dialog sets (Visibility/Graphics, Background Grids). A row
//! the view template controls is shown and not changed here, as in egui (`graphics.rs`, `graphics_properties`). Each is a command of its
//! own (`scale`, `detail`, `graphics`) given the view, not a `modify` (`panel::Via`).

use bimcraft_core::graphics::{Detail, Group as Controls, Orientation};
use bimcraft_core::model::{Id, Model};
use serde_json::{Value, json};

use super::kind::Kind;
use super::panel::{Editor, Pick, Row, Via};
use crate::lang::t;

/// The English name of the group, which what is folded is kept by.
pub const GROUP: &str = "Graphics";

fn pick(label: &str, value: Value) -> Pick {
    Pick { label: label.to_string(), value, also: None }
}

/// The Graphics rows of the plan of `level`.
pub fn rows(m: &Model, level: Id) -> Vec<Row> {
    let units = m.units;
    let own = bimcraft_core::graphics::graphics_of(m, level).cloned().unwrap_or_default();
    let shown = own.resolved(m).into_owned();
    let via = |command: &'static str, key: &'static str| Some(Via { command, key, view: json!(level) });
    let controlled = |g: Controls| own.controlled_by(m, g).is_some();
    let row = |key: &'static str, label: &str, kind: Kind, shown: String, editor: Editor, via: Option<Via>| Row {
        key,
        label: t(label).to_string(),
        kind,
        shown,
        editor,
        via,
    };
    let mut out = Vec::new();

    let n = shown.scale_n(units);
    let scale = units.scale(n);
    if controlled(Controls::Scale) {
        out.push(row("scale", "View Scale", Kind::Shown, scale, Editor::Read, None));
        out.push(row("scale_value", "Scale Value 1:", Kind::Shown, format!("{n}"), Editor::Read, None));
    } else {
        let scales = units.scales();
        let options: Vec<Pick> = scales.iter().map(|s| pick(&units.scale(*s), json!(s))).collect();
        let current = scales.iter().position(|s| (*s - n).abs() < 1e-9);
        out.push(row("scale", "View Scale", Kind::Choice(&[]), scale, Editor::Choice { current, options }, via("scale", "scale")));
        out.push(row("scale_value", "Scale Value 1:", Kind::Scale, format!("{n}"), Editor::Text { hint: "" }, via("scale", "scale")));
    }

    let detail = shown.detail;
    if controlled(Controls::Detail) {
        out.push(row("detail", "Detail Level", Kind::Shown, t(detail.name()).to_string(), Editor::Read, None));
    } else {
        let options: Vec<Pick> = Detail::ALL.iter().map(|d| pick(t(d.name()), json!(d.name()))).collect();
        let current = Detail::ALL.iter().position(|d| *d == detail);
        let editor = Editor::Choice { current, options };
        out.push(row("detail", "Detail Level", Kind::Choice(&[]), t(detail.name()).to_string(), editor, via("detail", "detail")));
    }

    // A plan's north: project north up, or true north, the plan turned on screen by the angle to it.
    let norths = [(Orientation::ProjectNorth, "Project North", "project_north"), (Orientation::TrueNorth, "True North", "true_north")];
    let options: Vec<Pick> = norths.iter().map(|(_, label, value)| pick(t(label), json!(value))).collect();
    let current = norths.iter().position(|(o, _, _)| *o == own.orientation);
    let said = current.and_then(|i| norths.get(i)).map_or("", |(_, label, _)| t(label)).to_string();
    out.push(row(
        "orientation",
        "Orientation",
        Kind::Choice(&[]),
        said,
        Editor::Choice { current, options },
        via("graphics", "orientation"),
    ));

    // Buttons of what a dialog sets: the view template, by its name, and the overrides.
    let template = own.template.clone().unwrap_or_else(|| t("<None>").to_string());
    out.push(row("template", "View Template", Kind::Shown, template, Editor::Seam("View Template"), None));
    out.push(row(
        "overrides",
        "Visibility/Graphics Overrides",
        Kind::Shown,
        t("Edit…").to_string(),
        Editor::Seam("Visibility/Graphics"),
        None,
    ));
    out.push(row("grids", "Background Grids", Kind::Shown, t("Edit…").to_string(), Editor::Seam("Background Grids"), None));
    out
}
