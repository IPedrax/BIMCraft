//! The panel schedule on screen: a distribution board's load schedule, a row for each slot, its summary and notes. Its cells are
//! edited in place (a circuit's description, breaker and demand factor, its slot), and the slot picked is marked a spare or a space,
//! or moved to a phase, from the bar above. Everything runs through `panel_schedule`; the sheet's drawing is the core's.

use bimcraft_core::model::{Element, Id};
use bimcraft_core::panel_schedule::{self, Kind, TEMPLATES};
use bimcraft_i18n::msg;
use egui::{Grid, RichText, ScrollArea, Ui, vec2};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

impl BimApp {
    /// The project's panel schedules: their ids and names, by name.
    pub(crate) fn panel_schedules(&self) -> Vec<(Id, &str)> {
        let mut out: Vec<(Id, &str)> = self
            .session
            .model
            .elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::PanelSchedule(ps) => Some((*id, ps.name.as_str())),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(_, name)| name.to_lowercase());
        out
    }

    /// Whether the view on screen is a panel schedule.
    pub(crate) fn showing_panel_schedule(&self) -> Option<Id> {
        match self.sheet {
            Sheet::Table(id) if matches!(self.session.model.elements.get(id), Some(Element::PanelSchedule(_))) => Some(id),
            _ => None,
        }
    }

    /// Electrical > Panel Schedule: the one of the selected distribution board, opened.
    pub(crate) fn make_panel_schedule(&mut self) {
        let m = &self.session.model;
        let boards: Vec<Id> = self.selection.iter().copied().filter(|id| bimcraft_core::electrical::board(m, *id).is_some()).collect();
        let [board] = boards[..] else {
            self.status = Status { text: t("Select one distribution board to make its panel schedule.").into(), error: true };
            return;
        };
        if let Some(id) = self.run("panel_schedule", json!({"panel": board})).and_then(|r| r["id"].as_u64()) {
            self.open_sheet(Sheet::Table(id));
        }
    }

    /// Panel schedule `id` as the middle of the window shows it.
    pub(crate) fn panel_schedule_view(&mut self, ui: &mut Ui, id: Id) {
        let Some(Element::PanelSchedule(ps)) = self.session.model.elements.get(id).cloned() else { return };
        let layout = match panel_schedule::layout(&self.session.model, &ps) {
            Ok(l) => l,
            Err(e) => {
                ui.label(RichText::new(e).color(pal().muted));
                return;
            }
        };
        let board = self.session.model.elements.id(ps.panel);
        // What the bar acts on: the slot picked, and the circuit that has it.
        let slot = self.panel_slot.filter(|s| layout.rows.iter().any(|r| r.slot == *s));
        let circuit = slot.and_then(|s| layout.rows.iter().find(|r| r.slot == s)).and_then(|r| r.circuit);
        let phases = board.and_then(|b| bimcraft_core::electrical::board_phases(&self.session.model, b)).unwrap_or(0);
        let mut change: Option<Value> = None;
        ui.label(RichText::new(&ps.name).heading());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(t("Template"));
            for name in TEMPLATES {
                if ui.selectable_label(ps.template == *name, t(name_of(name))).clicked() && ps.template != *name {
                    change = Some(json!({"template": name}));
                }
            }
            ui.separator();
            let mut flag = |ui: &mut Ui, label: &str, on: bool, key: &str| {
                let mut v = on;
                if ui.checkbox(&mut v, t(label)).changed() {
                    change = Some(json!({ key: v }));
                }
            };
            flag(ui, msg!("Show Spares"), ps.show_spare, "show_spare");
            flag(ui, msg!("Show Spaces"), ps.show_space, "show_space");
            flag(ui, msg!("Phase Columns"), ps.phase_columns, "phase_columns");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(match slot {
                Some(s) => tf("Slot {}", &[&s.to_string()]),
                None => t("Pick a slot").to_string(),
            });
            let spare = ui.add_enabled(slot.is_some(), egui::Button::new(t("Mark as Spare")));
            if spare.clicked() {
                change = slot.map(|s| json!({"slot": s, "mark": "spare"}));
            }
            let space = ui.add_enabled(slot.is_some(), egui::Button::new(t("Mark as Space")));
            if space.clicked() {
                change = slot.map(|s| json!({"slot": s, "mark": "space"}));
            }
            for p in 0..phases {
                let letter = bimcraft_core::electrical::phase_letter(p);
                let go = ui.add_enabled(circuit.is_some(), egui::Button::new(tf("Phase {}", &[&letter.to_string()])));
                if go.clicked() {
                    change = circuit.map(|c| json!({"circuit": c, "phase": letter.to_string()}));
                }
            }
        });
        ui.add_space(6.0);
        ScrollArea::both().show(ui, |ui| {
            for pair in layout.header.chunks(3) {
                ui.horizontal(|ui| {
                    for (label, value) in pair {
                        ui.label(RichText::new(format!("{label}:")).strong());
                        ui.label(value);
                        ui.add_space(12.0);
                    }
                });
            }
            ui.add_space(6.0);
            let selected: Vec<bool> = layout.rows.iter().map(|r| r.circuit.is_some_and(|c| self.selection.contains(&c))).collect();
            let shaded = selected.clone();
            let row_color = move |row: usize, style: &egui::Style| match row.checked_sub(1).and_then(|r| shaded.get(r)) {
                Some(true) => Some(style.visuals.selection.bg_fill.gamma_multiply(0.5)),
                _ => (row % 2 == 1).then_some(style.visuals.faint_bg_color),
            };
            let mut picked: Option<(u32, Option<Id>)> = None;
            let trip = layout.columns.iter().position(|c| *c == t_core(&self.session.model, "Trip"));
            let factor = layout.columns.iter().position(|c| *c == t_core(&self.session.model, "Demand Factor"));
            Grid::new(("panel-schedule", id)).with_row_color(row_color).spacing(vec2(20.0, 5.0)).show(ui, |ui| {
                let labels: Vec<egui::Id> = layout.columns.iter().map(|c| ui.label(RichText::new(c).strong()).id).collect();
                ui.end_row();
                for row in &layout.rows {
                    let grey = matches!(row.kind, Kind::Spare | Kind::Space);
                    for (i, cell) in row.cells.iter().enumerate() {
                        let editable = row.circuit.filter(|_| matches!(row.kind, Kind::Circuit | Kind::Spare));
                        let text =
                            |ui: &mut Ui, s: &str| ui.label(if grey { RichText::new(s).color(pal().muted) } else { RichText::new(s) });
                        match (i, editable) {
                            (0, _) => {
                                let on = slot == Some(row.slot);
                                if ui.selectable_label(on, cell).clicked() {
                                    picked = Some((row.slot, row.circuit));
                                }
                            }
                            (1, Some(c)) => {
                                let shown = if row.kind == Kind::Spare && *cell == t_core(&self.session.model, "Spare") {
                                    String::new()
                                } else {
                                    cell.clone()
                                };
                                if let Some(typed) = self.field(ui, c, "ps-description", shown, labels[i], "") {
                                    change = Some(json!({"circuit": c, "description": typed.trim()}));
                                }
                            }
                            (n, Some(c)) if Some(n) == trip => {
                                if let Some(typed) = self.field(ui, c, "ps-breaker", cell.clone(), labels[i], "A") {
                                    match typed.trim().replace(',', ".").parse::<f64>() {
                                        Ok(a) => change = Some(json!({"circuit": c, "breaker": a})),
                                        Err(_) => {
                                            self.status = Status {
                                                text: tf("{}: `{}` is not a number of amperes", &[&layout.columns[n], &typed]),
                                                error: true,
                                            }
                                        }
                                    }
                                }
                            }
                            (n, Some(c)) if Some(n) == factor => {
                                if let Some(typed) = self.field(ui, c, "ps-demand", cell.clone(), labels[i], "") {
                                    match typed.trim().replace(',', ".").parse::<f64>() {
                                        Ok(f) => change = Some(json!({"circuit": c, "demand_factor": f})),
                                        Err(_) => {
                                            self.status =
                                                Status { text: tf("{}: `{}` is not a number", &[&layout.columns[n], &typed]), error: true }
                                        }
                                    }
                                }
                            }
                            _ => {
                                text(ui, cell);
                            }
                        }
                    }
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            for (title, rows) in &layout.summary_tables {
                ui.label(RichText::new(title).strong());
                Grid::new(("panel-summary", id, title)).striped(true).spacing(vec2(20.0, 4.0)).show(ui, |ui| {
                    for (n, r) in rows.iter().enumerate() {
                        for c in r {
                            ui.label(if n == 0 { RichText::new(c).strong() } else { RichText::new(c) });
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
            }
            let label = ui.label(RichText::new(t("Notes")).strong());
            if let Some(typed) = self.field(ui, id, "ps-notes", ps.notes.clone(), label.id, "") {
                change = Some(json!({"notes": typed}));
            }
            if let Some((s, c)) = picked {
                self.panel_slot = Some(s);
                if let Some(c) = c {
                    self.pick_up(vec![c], ui.input(|i| i.modifiers));
                }
            }
        });
        if let Some(Value::Object(mut o)) = change {
            o.insert("id".into(), json!(id));
            self.run("panel_schedule", Value::Object(o));
        }
    }
}

/// A name of a template as the bar shows it.
fn name_of(template: &str) -> &'static str {
    match template {
        "branch" => msg!("Branch Panel"),
        _ => msg!("List"),
    }
}

/// `english`, a word the core writes, in the project's language.
fn t_core(m: &bimcraft_core::model::Model, english: &str) -> String {
    m.language.word(english).to_string()
}
