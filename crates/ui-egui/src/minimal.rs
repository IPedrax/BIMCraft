//! Independent minimal workspace: object tools beside the canvas and all commands on demand.
//! Reuses the existing tool handlers, contextual actions and Options Bar; no second model state.

use bimcraft_i18n::msg;
use egui::{Panel, ScrollArea, Ui, vec2};

use crate::lang::t;
use crate::{BimApp, Tab, Tool, mep, style};

impl BimApp {
    pub(crate) fn minimal_tools(&mut self, ui: &mut Ui) {
        Panel::right("minimal-tools").exact_size(156.0).frame(style::card([false, true, false, false])).show_separator_line(false).show(
            ui,
            |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    ui.set_min_width(140.0);
                    ui.heading(t(msg!("Tools")));
                    if self.minimal_search(ui) {
                        return;
                    }
                    ui.add_space(8.0);
                    for (label, tool) in [
                        ("Modify", Tool::Modify),
                        ("Wall", Tool::Wall(Vec::new())),
                        ("Wire", Tool::Run("wire", None)),
                        ("Cable Carrier", Tool::Run("cablecarrier", None)),
                    ] {
                        let active = crate::structure::same_tool(&self.tool, &tool);
                        if ui.add_sized(vec2(ui.available_width(), 30.0), egui::Button::new(t(label)).selected(active)).clicked() {
                            self.set_tool(tool);
                        }
                    }
                    ui.menu_button(t(msg!("Equipment")), |ui| {
                        for fixture in mep::FIXTURES {
                            if ui.button(t(fixture.label)).clicked() {
                                self.set_tool(Tool::Fixture(fixture.key));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    ui.menu_button(t(msg!("All Tools")), |ui| {
                        for (tab, label) in [
                            (Tab::Architecture, "Architecture"),
                            (Tab::Structure, "Structure"),
                            (Tab::Plumbing, "Plumbing"),
                            (Tab::Electrical, "Electrical"),
                            (Tab::Lightning, "Lightning Protection"),
                            (Tab::Data, "Data"),
                            (Tab::LowVoltage, "Low Voltage"),
                            (Tab::Insert, "Insert"),
                            (Tab::Annotate, "Annotate"),
                            (Tab::Massing, "Massing"),
                            (Tab::View, "View"),
                            (Tab::Manage, "Manage"),
                            (Tab::Modify, "Modify"),
                        ] {
                            ui.menu_button(t(label), |ui| {
                                ui.set_min_width(640.0_f32.min(ui.ctx().content_rect().width() - 200.0));
                                let previous = self.tab;
                                let tool = self.tool.clone();
                                self.tab = tab;
                                self.ribbon_panels(ui);
                                if self.tab == tab {
                                    self.tab = previous;
                                }
                                if self.tool != tool {
                                    ui.close();
                                }
                            });
                        }
                    });
                    ui.separator();
                    ui.menu_button(t(msg!("Properties")), |ui| {
                        ui.set_min_width(260.0);
                        ScrollArea::vertical().max_height(420.0).show(ui, |ui| self.properties(ui));
                    });
                    if let Some(name) = self.context() {
                        ui.separator();
                        ui.menu_button(name, |ui| {
                            let previous = self.tab;
                            self.tab = Tab::Context;
                            self.ribbon_panels(ui);
                            if self.tab == Tab::Context {
                                self.tab = previous;
                            }
                        });
                    }
                });
            },
        );
    }

    fn minimal_search(&mut self, ui: &mut Ui) -> bool {
        let id = ui.make_persistent_id("tool-search");
        let mut query = ui.data_mut(|data| data.get_temp::<String>(id).unwrap_or_default());
        let label = ui.label(t(msg!("Search tools")));
        ui.add(egui::TextEdit::singleline(&mut query).desired_width(ui.available_width()).char_limit(128)).labelled_by(label.id);
        ui.data_mut(|data| data.insert_temp(id, query.clone()));
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return false;
        }
        let mut matched = false;
        let mut chosen = false;
        for &action in crate::shortcuts::ACTIONS {
            if action.to_lowercase().contains(&needle) || t(action).to_lowercase().contains(&needle) {
                matched = true;
                if ui.button(t(action)).clicked() {
                    self.execute_shortcut_action(action);
                    chosen = true;
                }
            }
        }
        for fixture in mep::FIXTURES {
            if crate::shortcuts::ACTIONS.contains(&fixture.label) {
                continue;
            }
            if fixture.label.to_lowercase().contains(&needle) || t(fixture.label).to_lowercase().contains(&needle) {
                matched = true;
                if ui.button(t(fixture.label)).clicked() {
                    self.set_tool(Tool::Fixture(fixture.key));
                    chosen = true;
                }
            }
        }
        if !matched {
            ui.label(t(msg!("No matching tools")));
        }
        if chosen {
            ui.data_mut(|data| data.insert_temp(id, String::new()));
        }
        true
    }
}
