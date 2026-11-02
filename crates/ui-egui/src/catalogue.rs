//! Desktop polling policy; catalogue ownership, validation and file replacement stay in the core commands.
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

use bimcraft_core::carrier_limits::{Enclosure, Profile, Section};
use bimcraft_core::model::Model;
use egui::Context;
use serde_json::json;

use crate::lang::{t, tf};
use crate::{BimApp, Status};

#[derive(Default)]
pub(crate) struct State {
    next_check: f64,
    model: Weak<Model>,
    path: Option<String>,
    stamp: Option<(SystemTime, u64)>,
    error: Option<String>,
    announced: std::collections::BTreeSet<bimcraft_core::ids::Guid>,
}

struct CompartmentDraft {
    carrier: bimcraft_core::ids::Id,
    name: String,
    width: String,
    depth: String,
    offset: String,
    systems: String,
}

struct SectionDraft {
    name: String,
    section: Section,
    width: String,
    depth: String,
}

pub(crate) struct EquipmentDraft {
    name: String,
    type_guid: Option<bimcraft_core::ids::Guid>,
    system: String,
    quantity: u32,
    import_conflicts: Option<&'static str>,
    carrier_policy: Option<(f64, bool, Profile)>,
    section_draft: Option<SectionDraft>,
    carrier_warnings: Vec<String>,
    compartment_draft: Option<CompartmentDraft>,
}

impl Default for EquipmentDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            type_guid: None,
            system: String::new(),
            quantity: 1,
            import_conflicts: None,
            carrier_policy: None,
            section_draft: None,
            carrier_warnings: Vec::new(),
            compartment_draft: None,
        }
    }
}

fn stamp(path: &str) -> Option<(SystemTime, u64)> {
    // shortcut: saves retaining mtime and length are invisible; use content hashing to support such editors.
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

impl BimApp {
    pub(crate) fn catalogue_window(&mut self, ctx: &Context) {
        if !self.show_catalogue {
            self.equipment_draft.carrier_policy = None;
            self.equipment_draft.section_draft = None;
            return;
        }
        if let Some(guid) = self.equipment_draft.type_guid {
            if let Some((_, entry)) = self
                .session
                .model
                .library
                .by_guid(guid)
                .filter(|(_, entry)| entry.kind == bimcraft_core::library::Kind::FamilyType && !entry.placeholder)
            {
                self.equipment_draft.name.clone_from(&entry.name);
            } else {
                self.equipment_draft.type_guid = None;
                self.equipment_draft.name.clear();
            }
        }
        let linked = self.session.model.catalogue.clone();
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("equipment-catalogue")).show(ctx, |ui| {
            ui.set_width(540.0);
            ui.heading(t("Equipment Catalogue"));
            egui::ScrollArea::vertical()
                .id_salt("equipment-catalogue-content")
                .auto_shrink([false, false])
                .max_height((ctx.content_rect().height() - 140.0).max(40.0))
                .show(ui, |ui| {
                    self.carrier_controls(ui);

                    if let Some(path) = linked.as_deref() {
                        ui.add(egui::Label::new(path).truncate()).on_hover_text(path);
                    } else {
                        ui.label(t("Not linked"));
                    }
                    ui.label(t("Changes in the open model take priority over existing ODS rows."));
                    let missing: Vec<_> = self.session.model.library.entries().filter(|(_, entry)| entry.placeholder).collect();
                    if !missing.is_empty() {
                        ui.label(t("Products missing from the catalogue:"));
                        egui::ScrollArea::vertical().id_salt("missing-products").max_height(120.0).show_rows(
                            ui,
                            ui.text_style_height(&egui::TextStyle::Body),
                            missing.len(),
                            |ui, range| {
                                for index in range {
                                    let (id, entry) = missing[index];
                                    ui.add(egui::Label::new(&entry.name).truncate())
                                        .on_hover_text(format!("{}\n{id}\n{}", entry.name, entry.guid));
                                }
                            },
                        );
                    }
                    let pending = bimcraft_core::ods::pending_products(&self.session.model);
                    if !pending.is_empty() {
                        ui.label(t("Unassigned catalogue products:"));
                        egui::ScrollArea::vertical().id_salt("pending-products").max_height(120.0).show_rows(
                            ui,
                            ui.spacing().interact_size.y,
                            pending.len(),
                            |ui, range| {
                                for index in range {
                                    let (id, entry) = pending[index];
                                    let response = if entry.kind == bimcraft_core::library::Kind::FamilyType {
                                        ui.add(
                                            egui::Button::selectable(self.equipment_draft.type_guid == Some(entry.guid), &entry.name)
                                                .truncate(),
                                        )
                                    } else {
                                        ui.add(egui::Label::new(&entry.name).truncate())
                                    };
                                    if response.clicked() && entry.kind == bimcraft_core::library::Kind::FamilyType {
                                        self.equipment_draft.type_guid = Some(entry.guid);
                                        self.equipment_draft.name.clone_from(&entry.name);
                                    }
                                    response.on_hover_text(format!("{}\n{id}\n{}", entry.name, entry.guid));
                                }
                            },
                        );
                    }
                    ui.horizontal(|ui| {
                        for (label, save) in [(bimcraft_i18n::msg!("Link Existing..."), false), (bimcraft_i18n::msg!("Create..."), true)] {
                            if ui.button(t(label)).clicked() {
                                let dialog = rfd::AsyncFileDialog::new().add_filter(t("OpenDocument Spreadsheet"), &["ods"]);
                                let dialog = if save { dialog.set_file_name("equipment.ods") } else { dialog };
                                self.ask_file(ctx, dialog, save, |app, path| {
                                    app.run("catalogue_link", json!({"path":path.to_string_lossy()}));
                                });
                            }
                        }
                    });
                    ui.separator();
                    ui.label(t("Conflicting imported products"));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.equipment_draft.import_conflicts, None, t("Report conflict"));
                        ui.radio_value(&mut self.equipment_draft.import_conflicts, Some("update"), t("Update product"));
                        ui.radio_value(&mut self.equipment_draft.import_conflicts, Some("copy"), t("Create separate product"));
                    });
                    if ui.button(t("Import Catalogue...")).clicked() {
                        let conflicts = self.equipment_draft.import_conflicts;
                        let dialog = rfd::AsyncFileDialog::new().add_filter(t("OpenDocument Spreadsheet"), &["ods"]);
                        self.ask_file(ctx, dialog, false, move |app, path| {
                            let mut params = json!({"path":path.to_string_lossy()});
                            params["conflicts"] = json!(conflicts.unwrap_or("error"));
                            app.run("catalogue_import", params);
                        });
                    }
                    ui.separator();
                    ui.strong(t("Equipment without placement"));
                    if self.equipment_draft.type_guid.is_some() && ui.button(t("New product")).clicked() {
                        self.equipment_draft.type_guid = None;
                        self.equipment_draft.name.clear();
                    }
                    egui::Grid::new("equipment-add").num_columns(2).show(ui, |ui| {
                        let label = ui.label(t("Product name"));
                        ui.add_enabled(
                            self.equipment_draft.type_guid.is_none(),
                            egui::TextEdit::singleline(&mut self.equipment_draft.name).char_limit(bimcraft_core::model::MAX_TEXT),
                        )
                        .labelled_by(label.id);
                        ui.end_row();
                        let label = ui.label(t("System"));
                        ui.add(egui::TextEdit::singleline(&mut self.equipment_draft.system).char_limit(bimcraft_core::model::MAX_TEXT))
                            .labelled_by(label.id);
                        ui.end_row();
                        let label = ui.label(t("Quantity"));
                        ui.add(egui::DragValue::new(&mut self.equipment_draft.quantity).range(1..=1_000_000)).labelled_by(label.id);
                        ui.end_row();
                    });
                    if ui.add_enabled(!self.equipment_draft.name.trim().is_empty(), egui::Button::new(t("Add to specification"))).clicked()
                    {
                        let mut params = json!({"quantity":self.equipment_draft.quantity,
                    "system":self.equipment_draft.system});
                        if let Some(guid) = self.equipment_draft.type_guid {
                            params["type"] = json!(guid);
                        } else {
                            params["name"] = json!(self.equipment_draft.name.trim());
                        }
                        if self.run("equipment", params).is_some() {
                            self.equipment_draft.type_guid = None;
                            self.equipment_draft.name.clear();
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(linked.is_some(), egui::Button::new(t("Synchronize"))).clicked() {
                    self.catalogue.model = Weak::new();
                    self.poll_catalogue(ctx);
                }
                if ui.add_enabled(linked.is_some(), egui::Button::new(t("Unlink"))).clicked() {
                    self.run("catalogue_link", json!({"path":null}));
                }
                close = ui.button(t("Close")).clicked();
            });
        });
        let rect = response.response.rect;
        let layout_changed = ctx.data_mut(|data| {
            let key = egui::Id::new("equipment-catalogue-layout");
            let previous = data.get_temp::<egui::Rect>(key);
            data.insert_temp(key, rect);
            previous != Some(rect)
        });
        if layout_changed {
            ctx.request_repaint();
        }
        if close || response.should_close() {
            self.show_catalogue = false;
        }
    }

    fn carrier_controls(&mut self, ui: &mut egui::Ui) {
        let policy = self.session.model.carrier_settings.as_ref();
        let initial = policy.map_or((100.0, false, Profile::Project), |p| (p.fill_percent, p.auto, p.profile));
        let mut default_type = policy.map_or_else(String::new, |p| p.default_type.clone());
        let types: Vec<_> =
            self.session.model.types.runs.get("cablecarrier").map(|types| types.keys().cloned().collect()).unwrap_or_default();
        let (mut percent, mut auto, mut profile) = *self.equipment_draft.carrier_policy.get_or_insert(initial);
        ui.collapsing(t("Cable carriers"), |ui| {
            ui.radio_value(&mut profile, Profile::Project, t("Project gross-section limit"));
            let previous_type = default_type.clone();
            egui::ComboBox::from_label(t("Project carrier type"))
                .selected_text(if default_type.is_empty() { t("Unspecified") } else { &default_type })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut default_type, String::new(), t("Unspecified"));
                    for name in &types {
                        ui.selectable_value(&mut default_type, name.clone(), name);
                    }
                });
            if default_type != previous_type {
                self.run(
                    "cable_carrier_check",
                    json!({"mode":if auto {"auto"}else{"warn"},"fill_percent":percent,"profile":profile,"configure":true,"default_type":default_type}),
                );
            }
            ui.horizontal(|ui| {
                let label = ui.label(t("Maximum fill (%):"));
                ui.add_enabled(profile == Profile::Project, egui::DragValue::new(&mut percent).range(0.1..=100.0)).labelled_by(label.id);
            });
            ui.radio_value(&mut auto, false, t("Warn and highlight carriers"));
            ui.radio_value(&mut auto, true, t("Automatic carrier sizing"));
            if ui.button(t("Apply carrier settings")).clicked() {
                self.run("cable_carrier_check", json!({"mode":if auto {"auto"}else{"warn"},"fill_percent":percent,"profile":profile,"configure":true}));
            }
            if ui.button(t("Check and highlight carriers")).clicked() {
                self.run("cable_carrier_check", json!({"mode":"warn","fill_percent":percent,"profile":profile}));
            }
            let ids: Vec<_> = self
                .selection
                .iter()
                .copied()
                .filter(|id| self.session.model.elements.get(*id).is_some_and(|e| e.category() == "cablecarrier"))
                .collect();
            ui.horizontal(|ui| {
                for (label, locked) in [
                    (bimcraft_i18n::msg!("Lock selected carrier sizes"), true),
                    (bimcraft_i18n::msg!("Unlock selected carrier sizes"), false),
                ] {
                    if ui.add_enabled(!ids.is_empty(), egui::Button::new(t(label))).clicked() {
                        self.run("modify", json!({"ids":ids,"size_locked":locked}));
                    }
                }
            });
            self.section_controls(ui);
            self.compartment_controls(ui);
            if !self.equipment_draft.carrier_warnings.is_empty() {
                egui::ScrollArea::vertical().id_salt("carrier-warnings").max_height(80.0).show(ui, |ui| {
                    for warning in &self.equipment_draft.carrier_warnings {
                        ui.label(warning);
                    }
                });
            }
        });
        self.equipment_draft.carrier_policy = Some((percent, auto, profile));
    }

    fn section_controls(&mut self, ui: &mut egui::Ui) {
        let name = self.selection.iter().find_map(|id| match self.session.model.elements.get(*id) {
            Some(bimcraft_core::model::Element::CableCarrier(run)) => Some(run.type_name.clone()),
            _ => None,
        });
        let Some(name) = name else { return };
        let units = self.session.model.units;
        let mut draft = self.equipment_draft.section_draft.take().filter(|d| d.name == name).unwrap_or_else(|| {
            let section = self.session.model.carrier_section(&name).cloned().unwrap_or_default();
            SectionDraft {
                name: name.clone(),
                width: section.clear_width.map(|v| units.length(v)).unwrap_or_default(),
                depth: section.clear_depth.map(|v| units.length(v)).unwrap_or_default(),
                section,
            }
        });
        ui.collapsing(t("Measured product section"), |ui| {
            ui.label(&name);
            let choices = [
                (None, bimcraft_i18n::msg!("Unknown")),
                (Some(Enclosure::BlindBox), bimcraft_i18n::msg!("Blind box")),
                (Some(Enclosure::LiddedBox), bimcraft_i18n::msg!("Lidded box")),
                (Some(Enclosure::Tray), bimcraft_i18n::msg!("Tray")),
                (Some(Enclosure::Ladder), bimcraft_i18n::msg!("Ladder")),
                (Some(Enclosure::Conduit), bimcraft_i18n::msg!("Conduit")),
            ];
            let label = choices.iter().find(|(kind, _)| *kind == draft.section.enclosure).map_or("Unknown", |(_, label)| *label);
            egui::ComboBox::from_label(t("Carrier enclosure")).selected_text(t(label)).show_ui(ui, |ui| {
                for (kind, label) in choices {
                    ui.selectable_value(&mut draft.section.enclosure, kind, t(label));
                }
            });
            egui::Grid::new("carrier-clear-section").num_columns(2).show(ui, |ui| {
                for (label, value) in
                    [(bimcraft_i18n::msg!("Clear width:"), &mut draft.width), (bimcraft_i18n::msg!("Clear depth:"), &mut draft.depth)]
                {
                    let label = ui.label(t(label));
                    ui.add(egui::TextEdit::singleline(value).desired_width(180.0)).labelled_by(label.id);
                    ui.end_row();
                }
            });
            if ui.button(t("Apply measured section")).clicked() {
                let parse = |v: &str| if v.trim().is_empty() { Ok(None) } else { units.parse(v).map(Some).ok_or(()) };
                match (parse(&draft.width), parse(&draft.depth)) {
                    (Ok(width), Ok(depth)) => {
                        draft.section.clear_width = width;
                        draft.section.clear_depth = depth;
                        self.run("carrier_section", json!({"type":name,"section":draft.section}));
                    }
                    _ => {
                        self.status =
                            Status { text: t("Enter valid internal lengths or leave unknown dimensions blank.").into(), error: true }
                    }
                }
            }
            if ui.button(t("Clear measured section")).clicked() {
                self.run("carrier_section", json!({"type":name,"section":null}));
                draft.section = Section::default();
                draft.width.clear();
                draft.depth.clear();
            }
        });
        self.equipment_draft.section_draft = Some(draft);
    }

    fn compartment_controls(&mut self, ui: &mut egui::Ui) {
        use bimcraft_core::model::{CarrierCompartment, Element};
        let wire = self.selection.iter().find_map(|id| match self.session.model.elements.get(*id) {
            Some(Element::Wire(w)) => Some((*id, w.clone())),
            _ => None,
        });
        if let Some((id, wire)) = wire {
            let names: Vec<_> = wire
                .carrier
                .and_then(|g| self.session.model.elements.get(g))
                .and_then(|e| e.run())
                .map(|r| r.compartments.keys().cloned().collect())
                .unwrap_or_default();
            if !names.is_empty() {
                ui.collapsing(t("Wire compartment"), |ui| {
                    for name in names {
                        if ui.selectable_label(wire.compartment == name, &name).clicked() {
                            self.run("modify", json!({"id":id,"compartment":name}));
                        }
                    }
                });
            }
        }
        let carrier = self.selection.iter().find_map(|id| match self.session.model.elements.get(*id) {
            Some(e @ Element::CableCarrier(r)) => self.session.model.run_type(e).filter(|t| !t.round).map(|t| (*id, r.clone(), t.clone())),
            _ => None,
        });
        let Some((id, run, section)) = carrier else { return };
        let units = self.session.model.units;
        let mut draft = self.equipment_draft.compartment_draft.take().filter(|d| d.carrier == id).unwrap_or_else(|| CompartmentDraft {
            carrier: id,
            name: String::new(),
            width: units.length(section.width / 2.0),
            depth: units.length(section.depth),
            offset: units.length(0.0),
            systems: String::new(),
        });
        ui.collapsing(t("Carrier compartments"), |ui| {
            ui.label(t("Enter measured usable dimensions and permitted systems. Empty systems allow any system."));
            ui.horizontal_wrapped(|ui| {
                for (name, slot) in &run.compartments {
                    if ui.selectable_label(draft.name == *name, name).clicked() {
                        draft.name.clone_from(name);
                        draft.width = units.length(slot.width);
                        draft.depth = units.length(slot.depth);
                        draft.offset = units.length(slot.offset);
                        draft.systems = slot.systems.join(", ");
                    }
                }
            });
            egui::Grid::new("carrier-compartment-fields").num_columns(2).show(ui, |ui| {
                for (label, value) in [
                    (bimcraft_i18n::msg!("Compartment name:"), &mut draft.name),
                    (bimcraft_i18n::msg!("Compartment width:"), &mut draft.width),
                    (bimcraft_i18n::msg!("Compartment depth:"), &mut draft.depth),
                    (bimcraft_i18n::msg!("Compartment offset:"), &mut draft.offset),
                    (bimcraft_i18n::msg!("Allowed systems (comma separated):"), &mut draft.systems),
                ] {
                    let label = ui.label(t(label));
                    ui.add(egui::TextEdit::singleline(value).desired_width(180.0)).labelled_by(label.id);
                    ui.end_row();
                }
            });
            if ui.button(t("Apply compartment")).clicked() {
                match (units.parse(&draft.width), units.parse(&draft.depth), units.parse(&draft.offset)) {
                    (Some(width), Some(depth), Some(offset)) => {
                        let mut slots = run.compartments.clone();
                        slots.insert(
                            draft.name.trim().into(),
                            CarrierCompartment {
                                width,
                                depth,
                                offset,
                                systems: draft.systems.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
                            },
                        );
                        self.run("modify", json!({"id":id,"compartments":slots}));
                    }
                    _ => self.status = Status { text: t("Enter valid compartment lengths.").into(), error: true },
                }
            }
            if ui.add_enabled(run.compartments.contains_key(draft.name.trim()), egui::Button::new(t("Remove compartment"))).clicked() {
                let mut slots = run.compartments.clone();
                slots.remove(draft.name.trim());
                self.run("modify", json!({"id":id,"compartments":slots}));
            }
            if self.status.error {
                ui.colored_label(ui.visuals().error_fg_color, &self.status.text);
            }
        });
        self.equipment_draft.compartment_draft = Some(draft);
    }

    pub(crate) fn carrier_feedback(&mut self, report: &serde_json::Value) {
        let warnings: Vec<_> = report["warnings"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect();
        if !warnings.is_empty() {
            self.selection = report["elements"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| serde_json::from_value::<bimcraft_core::ids::Guid>(v.clone()).ok())
                .filter_map(|guid| self.session.model.elements.id(guid))
                .collect();
            self.status =
                Status { text: tf("Cable carrier conflicts: {}. Review selected carriers.", &[&warnings.len().to_string()]), error: false };
        }
        self.equipment_draft.carrier_warnings = warnings;
    }

    pub(crate) fn poll_catalogue(&mut self, ctx: &Context) {
        if !self.catalogue.model.ptr_eq(&Arc::downgrade(&self.session.model)) {
            self.notify_pending_catalogue();
        }
        let Some(path) = self.session.model.catalogue.clone() else {
            if self.catalogue.path.is_none() && self.catalogue.model.ptr_eq(&Arc::downgrade(&self.session.model)) {
                return;
            }
            if self.catalogue.error.as_ref() == Some(&self.status.text) {
                self.status = Status::default();
            }
            self.catalogue = State { announced: std::mem::take(&mut self.catalogue.announced), ..State::default() };
            self.missing_catalogue_warning();
            self.catalogue.model = Arc::downgrade(&self.session.model);
            return;
        };
        ctx.request_repaint_after(Duration::from_secs(2));
        let model_changed = !self.catalogue.model.ptr_eq(&Arc::downgrade(&self.session.model));
        let now = ctx.input(|i| i.time);
        if !model_changed && now < self.catalogue.next_check {
            return;
        }
        self.catalogue.next_check = now + 2.0;
        let current = stamp(&path);
        if !model_changed && self.catalogue.path.as_ref() == Some(&path) && current == self.catalogue.stamp {
            return;
        }
        match self.session.execute("catalogue_sync", &json!({})) {
            Ok(reply) => {
                if let Some(report) = reply.get("carrier_check") {
                    self.carrier_feedback(report);
                }
                if self.catalogue.error.as_ref() == Some(&self.status.text) {
                    self.status = Status::default();
                }
                self.catalogue.error = None;
                self.missing_catalogue_warning();
                self.notify_pending_catalogue();
            }
            Err(error) => {
                let text = tf("Equipment catalogue could not be synchronized: {}", &[&error]);
                self.catalogue.error = Some(text.clone());
                self.status = Status { text, error: true };
            }
        }
        self.catalogue.path = Some(path.clone());
        self.catalogue.stamp = stamp(&path);
        self.catalogue.model = Arc::downgrade(&self.session.model);
    }

    fn notify_pending_catalogue(&mut self) {
        for (_, entry) in bimcraft_core::ods::pending_products(&self.session.model) {
            if self.catalogue.announced.insert(entry.guid) {
                self.show_catalogue = true;
            }
        }
    }

    fn missing_catalogue_warning(&mut self) {
        let count = self.session.model.library.entries().filter(|(_, entry)| entry.placeholder).count();
        if count > 0 {
            let text =
                tf("Missing catalogue product types: {}. Geometry is retained; schedules omit these products.", &[&count.to_string()]);
            self.catalogue.error = Some(text.clone());
            self.status = Status { text, error: true };
        }
    }
}
