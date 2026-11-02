//! Desktop polling policy; catalogue ownership, validation and file replacement stay in the core commands.
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

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

pub(crate) struct EquipmentDraft {
    name: String,
    type_guid: Option<bimcraft_core::ids::Guid>,
    system: String,
    quantity: u32,
    import_conflicts: Option<&'static str>,
}

impl Default for EquipmentDraft {
    fn default() -> Self {
        Self { name: String::new(), type_guid: None, system: String::new(), quantity: 1, import_conflicts: None }
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
            Ok(_) => {
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
