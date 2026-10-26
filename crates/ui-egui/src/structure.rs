//! The Structure tab: Revit's Structure ribbon tab on the engine's beams, structural columns, isolated and wall footings and
//! structural floors. A tool that makes a run takes its category (`Tool::Run`), and one that makes a column or a footing
//! takes its (`Tool::Stand`), so a tab that draws pipes or conduits adds a button and a row to the tables here, not a tool.

use bimcraft_i18n::msg;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Justify, Model, TOL};
use bimcraft_core::{runs_of, stood_of};
use egui::{ComboBox, TextEdit, Ui};
use serde_json::{Value, json};

use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::{BimApp, Status, Tool, big_button, panel, top_params};

/// The command that makes elements of `category`.
pub(crate) fn command(category: &str) -> &'static str {
    bimcraft_core::command_of(category)
}

/// The title of the contextual tab while the tool is on.
pub(crate) fn title(tool: &Tool) -> Option<&'static str> {
    match tool {
        Tool::Run("beam", _) => Some(msg!("Place Beam")),
        Tool::Run("pipe", _) => Some(msg!("Place Pipe")),
        Tool::Run("conduit", _) => Some(msg!("Place Conduit")),
        Tool::Run("cabletray", _) => Some(msg!("Place Cable Tray")),
        Tool::Run("wire", _) => Some(msg!("Place Wire")),
        Tool::Run("downconductor", _) => Some(msg!("Place Down Conductor")),
        Tool::Run("groundconductor", _) => Some(msg!("Place Earthing Conductor")),
        Tool::Run("cable", _) => Some(msg!("Place Cabling")),
        Tool::Run(_, _) => Some(msg!("Place Wall Footing")),
        Tool::Stand("structuralcolumn") => Some(msg!("Place Structural Column")),
        Tool::Stand(_) => Some(msg!("Place Isolated Footing")),
        _ => None,
    }
}

/// What the status bar says to do next with a structure tool on.
pub(crate) fn hint(tool: &Tool) -> Option<&'static str> {
    match tool {
        Tool::Run(_, None) => Some(msg!("Click the start of the run: a chain of clicks makes one for each step, Esc ends it.")),
        Tool::Run(_, Some(_)) => Some(msg!("Click the end of the run. Esc stops the chain.")),
        Tool::Stand("structuralcolumn") => Some(msg!("Click to place a structural column, as high as the Options Bar says.")),
        Tool::Stand(_) => Some(msg!("Click to place an isolated footing: its top at the Options Bar's offset, as thick as it says.")),
        _ => None,
    }
}

/// Whether two tools are the same button's: the same kind, and of runs and standing things, the same category.
pub(crate) fn same_tool(a: &Tool, b: &Tool) -> bool {
    match (a, b) {
        (Tool::Run(x, _), Tool::Run(y, _)) | (Tool::Stand(x), Tool::Stand(y)) | (Tool::Fixture(x), Tool::Fixture(y)) => x == y,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

impl BimApp {
    /// Switching to the run tool of `category`: the justification and the offset on the Options Bar are the last that tool had,
    /// or its usual.
    pub(crate) fn keep_run_options(&mut self, category: &'static str) {
        if self.run_loaded == category {
            return;
        }
        let was = (self.run_justify, std::mem::take(&mut self.beam_offset));
        self.run_kept.insert(self.run_loaded, was);
        let zero = self.session.model.units.length(0.0);
        (self.run_justify, self.beam_offset) =
            self.run_kept.remove(category).unwrap_or_else(|| (bimcraft_core::runs::default_justify(category), zero));
        self.run_loaded = category;
    }

    /// The Structure tab: Revit's Structure panel (a column, a beam, a structural floor) and its Foundation panel.
    pub(crate) fn structure_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, "Structure", |ui| {
            let tools: [(Icon, &str, &str, Tool); 2] = [
                (Icon::Column, "Column", msg!("Structural Column (CL): click where it stands"), Tool::Stand("structuralcolumn")),
                (
                    Icon::Beam,
                    "Beam",
                    msg!("Beam (BM): click its start and its end; a chain of clicks makes a beam for each step"),
                    Tool::Run("beam", None),
                ),
            ];
            for (icon, label, tip, tool) in tools {
                if big_button(ui, icon, label, same_tool(&self.tool, &tool), tip).clicked() {
                    self.set_tool(tool);
                }
            }
            let tip = msg!("Floor: Structural (SB): click inside the walls to floor them with a slab that carries load");
            if big_button(ui, Icon::Floor, "Floor: Structural", self.tool == Tool::Floor && self.floor_structural, tip).clicked() {
                self.structural_floor();
            }
        });
        panel(ui, msg!("Foundation"), |ui| {
            let tools: [(Icon, &str, &str, Tool); 2] = [
                (Icon::Footing, "Isolated Footing", msg!("Isolated Footing (FI): click where the footing goes"), Tool::Stand("footing")),
                (
                    Icon::WallFooting,
                    "Wall Footing",
                    msg!("Wall Footing (FW): click its start and its end, or a chain of clicks, under a wall"),
                    Tool::Run("stripfooting", None),
                ),
            ];
            for (icon, label, tip, tool) in tools {
                if big_button(ui, icon, label, same_tool(&self.tool, &tool), tip).clicked() {
                    self.set_tool(tool);
                }
            }
        });
    }

    /// The Floor tool, making structural floors (SB).
    pub(crate) fn structural_floor(&mut self) {
        self.set_tool(Tool::Floor);
        self.floor_structural = true;
    }

    /// The Options Bar of a run's tool: its justification and its height over the level, a wall footing's top; Chain.
    pub(crate) fn run_options(&mut self, ui: &mut Ui, category: &'static str) {
        let hint = self.session.model.units.name();
        if category != "stripfooting" {
            let l = ui.label(t("Justification:"));
            let now = self.run_justify;
            let c = ComboBox::from_id_salt("run-justify").selected_text(t(justify_name(now))).width(90.0).show_ui(ui, |ui| {
                for j in [Justify::Top, Justify::Centre, Justify::Bottom] {
                    if ui.selectable_label(j == now, t(justify_name(j))).clicked() {
                        self.run_justify = j;
                    }
                }
            });
            c.response.labelled_by(l.id);
            let l = ui.label(t("Offset:"));
            ui.add(TextEdit::singleline(&mut self.beam_offset).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
        } else {
            let l = ui.label(t("Top Offset:"));
            ui.add(TextEdit::singleline(&mut self.footing_top).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
        }
        ui.checkbox(&mut self.chain, t("Chain"));
    }

    /// The Options Bar of a standing tool: a structural column's height; a footing's thickness and where its top is.
    pub(crate) fn stand_options(&mut self, ui: &mut Ui, category: &'static str) {
        if category == "structuralcolumn" {
            if let Some(level) = self.plan_level() {
                crate::height_options(
                    ui,
                    &self.session.model,
                    level,
                    &mut self.column_top,
                    &mut self.column_height,
                    "structural-column-top",
                );
            }
            return;
        }
        let hint = self.session.model.units.name();
        let l = ui.label(t("Thickness:"));
        ui.add(TextEdit::singleline(&mut self.footing_thickness).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
        let l = ui.label(t("Top Offset:"));
        ui.add(TextEdit::singleline(&mut self.footing_top).desired_width(64.0).hint_text(hint)).labelled_by(l.id);
    }

    /// The parameters a run made between `a` and `b` on `level` has, from the Options Bar: its heights as typed, and the type
    /// picked for the tool.
    pub(crate) fn run_params(&self, m: &Model, category: &str, level: Id, a: P2, b: P2) -> Result<Value, String> {
        let command = command(category);
        let height = |text: &str| m.units.parse(text).ok_or_else(|| tf("{}: `{}` is not a height", &[command, text]));
        let mut p = json!({"points": [[a.0, a.1], [b.0, b.1]], "level": level});
        if category != "stripfooting" {
            p["justify"] = json!(self.run_justify);
            p["offset"] = json!(height(&self.beam_offset)?);
        } else {
            p["offset"] = json!(height(&self.footing_top)?);
        }
        Ok(self.typed(category, p))
    }

    /// The parameters a standing thing made at `at` on `level` has, from the Options Bar.
    pub(crate) fn stand_params(&self, m: &Model, category: &str, level: Id, at: P2) -> Result<Value, String> {
        let command = command(category);
        let mut p = if category == "structuralcolumn" {
            top_params(m, level, self.column_top, &self.column_height).map_err(|e| tf("{}: {}", &[command, &e]))?
        } else {
            let height = |text: &str| m.units.parse(text).ok_or_else(|| tf("{}: `{}` is not a height", &[command, text]));
            json!({"thickness": height(&self.footing_thickness)?, "offset": height(&self.footing_top)?})
        };
        p["at"] = json!([at.0, at.1]);
        p["level"] = json!(level);
        Ok(self.typed(category, p))
    }

    /// A click of a run's tool at `p`: the start, then the end, which makes it, and with Chain starts the next there.
    pub(crate) fn place_run(&mut self, level: Id, p: P2) {
        let Tool::Run(category, start) = self.tool.clone() else { return };
        let Some(a) = start else {
            self.tool = Tool::Run(category, Some(p));
            return;
        };
        if a.dist(p) <= TOL {
            return;
        }
        let m = std::sync::Arc::clone(&self.session.model);
        match self.run_params(&m, category, level, a, p) {
            Ok(params) => {
                if self.run(command(category), params).is_some() {
                    self.tool = Tool::Run(category, self.chain.then_some(p));
                }
            }
            Err(e) => self.status = Status { text: e, error: true },
        }
    }

    /// A click of a standing tool at `at`.
    pub(crate) fn place_stand(&mut self, level: Id, at: P2, category: &'static str) {
        let m = std::sync::Arc::clone(&self.session.model);
        match self.stand_params(&m, category, level, at) {
            Ok(params) => {
                self.run(command(category), params);
            }
            Err(e) => self.status = Status { text: e, error: true },
        }
    }

    /// What a click would make with the run's tool, its start picked, the end at `b`; none while the Options Bar says what
    /// isn't a height.
    pub(crate) fn run_sketch(&self, m: &Model, category: &str, level: Id, a: P2, b: P2) -> Vec<Element> {
        self.run_params(m, category, level, a, b).ok().and_then(|p| runs_of(m, &p, category).ok()).unwrap_or_default()
    }

    /// What a click would make with a standing tool at `at`.
    pub(crate) fn stand_sketch(&self, m: &Model, category: &str, level: Id, at: P2) -> Option<Element> {
        self.stand_params(m, category, level, at).ok().and_then(|p| stood_of(m, &p, category).ok())
    }
}

fn justify_name(j: Justify) -> &'static str {
    match j {
        Justify::Top => "Top",
        Justify::Centre => "Centre",
        Justify::Bottom => "Bottom",
    }
}
