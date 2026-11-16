//! Print Sets (File > Export > Print Set…, the Publisher, and the Export dialog's Print Sets… button), as Revit's View/Sheet Set: one window
//! where a set is picked by name, its views and sheets checked in a list of every one the project has (Show: Sheets, Views),
//! and saved, saved as a new set, reverted, renamed or deleted. Every change is the `print_set` command; a set's name is
//! unique, and it has an id and a GUID as every library entry has.
//!
//! The Project Browser makes one too: views picked there with Ctrl and Shift, then Create Print Set… on its right-click menu,
//! which asks for the name ([`NameSet`]).

use std::collections::BTreeSet;

use bimcraft_core::model::Id;
use egui::{Align, Context, Layout, RichText, Ui};
use serde_json::json;

use crate::find::Found;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Sheet};

/// How tall the list of views is.
const LIST: f32 = 300.0;

/// What a name being typed is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Naming {
    /// Save As: a new set of what is checked.
    New,
    Rename,
}

/// The Print Sets window, while it is open.
pub(crate) struct Dialog {
    /// The set shown, by name; none, what is checked is no set yet.
    pub(crate) current: Option<String>,
    /// What is checked, by id.
    pub(crate) checked: BTreeSet<Id>,
    pub(crate) sheets: bool,
    pub(crate) views: bool,
    find: String,
    naming: Option<(Naming, String)>,
    error: Option<String>,
}

/// The name asked for a print set made of the views picked in the Project Browser.
pub(crate) struct NameSet {
    pub(crate) name: String,
    pub(crate) views: Vec<Id>,
    /// Why the last name wasn't taken.
    pub(crate) error: Option<String>,
}

/// The element a view of the browser is, as a print set holds it: none for what isn't an element yet (a standard schedule
/// not made, the default 3D view before it is changed).
pub(crate) fn element_of(found: &Found) -> Option<Id> {
    match found {
        Found::View(Sheet::Plan(id) | Sheet::Ceiling(id) | Sheet::Section(id) | Sheet::Paper(id) | Sheet::Table(id)) => Some(*id),
        Found::View3D(id) => *id,
        _ => None,
    }
}

impl BimApp {
    /// Every view and sheet a print set can hold, in the Project Browser's order, each once: its id, title and whether it
    /// is a sheet.
    pub(crate) fn printables(&self) -> Vec<(Id, String, bool)> {
        let m = &self.session.model;
        let mut seen = BTreeSet::new();
        self.openable()
            .into_iter()
            .filter_map(|(title, found)| {
                let id = element_of(&found)?;
                let g = m.elements.guid(id)?;
                (bimcraft_core::printset::printable(m, g) && seen.insert(id)).then(|| (id, title, m.sheet(id).is_some()))
            })
            .collect()
    }

    /// Opens Print Sets on set `name`, or the first, or none.
    pub(crate) fn open_print_sets(&mut self, name: Option<String>) {
        let m = &self.session.model;
        let current = name.filter(|n| m.print_sets.contains_key(n)).or_else(|| m.print_sets.keys().next().cloned());
        let mut d =
            Dialog { current: None, checked: BTreeSet::new(), sheets: true, views: true, find: String::new(), naming: None, error: None };
        d.show_set(m, current);
        self.print_sets = Some(d);
    }

    /// Opens Print Sets on a set not made yet: nothing checked, its name asked for (the Publisher's New Print Set…).
    pub(crate) fn open_new_print_set(&mut self) {
        self.print_sets = Some(Dialog {
            current: None,
            checked: BTreeSet::new(),
            sheets: true,
            views: true,
            find: String::new(),
            naming: Some((Naming::New, String::new())),
            error: None,
        });
    }

    /// The Print Sets window, while it is open.
    pub(crate) fn print_sets_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.print_sets.take() else { return };
        let all = self.printables();
        let model = std::sync::Arc::clone(&self.session.model);
        let saved = |d: &Dialog| -> BTreeSet<Id> {
            d.current
                .as_ref()
                .and_then(|n| model.print_sets.get(n))
                .map(|s| s.views.iter().filter_map(|g| model.elements.id(*g)).collect())
                .unwrap_or_default()
        };
        let changed = d.current.is_some() && saved(&d) != d.checked;
        // The checked views in the browser's order.
        let ordered = |d: &Dialog| -> Vec<Id> { all.iter().map(|(id, ..)| *id).filter(|id| d.checked.contains(id)).collect() };
        let mut close = false;
        let mut run: Vec<serde_json::Value> = Vec::new();
        let mut then: Option<Option<String>> = None;
        let r = Modal::new(egui::Id::new("print-sets"), t("Print Sets")).width(620.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t("Name"));
                    let shown = match &d.current {
                        Some(n) if changed => format!("{n} *"),
                        Some(n) => n.clone(),
                        None => t("<in-session>").to_string(),
                    };
                    egui::ComboBox::from_id_salt("print-set-name").width(260.0).selected_text(shown).show_ui(ui, |ui| {
                        for name in model.print_sets.keys() {
                            if ui.selectable_label(d.current.as_ref() == Some(name), name).clicked() {
                                then = Some(Some(name.clone()));
                            }
                        }
                    });
                });
                let (now, current) = (ordered(&d), d.current.clone());
                let mut drop_naming = false;
                if let Some((naming, name)) = &mut d.naming {
                    ui.horizontal(|ui| {
                        let label = if *naming == Naming::New { t("New name") } else { t("Rename to") };
                        let l = ui.label(label);
                        let edit = egui::TextEdit::singleline(name).char_limit(bimcraft_core::printset::MAX_NAME).desired_width(240.0);
                        let r = ui.add(edit).labelled_by(l.id);
                        r.request_focus();
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let go = if *naming == Naming::New { t("Create") } else { t("Rename") };
                        if ui.button(go).clicked() || enter {
                            let name = name.trim().to_string();
                            match (*naming, &current) {
                                (Naming::Rename, Some(from)) => run.push(json!({"name": from, "rename": name})),
                                _ => run.push(json!({"name": name, "views": now})),
                            }
                            then = Some(Some(name));
                        }
                        drop_naming = ui.button(t("Cancel")).clicked();
                    });
                }
                if drop_naming {
                    d.naming = None;
                }
                if let Some(e) = &d.error {
                    ui.label(RichText::new(e.as_str()).color(pal().error));
                }
                ui.add_space(4.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(440.0);
                        ui.add(egui::TextEdit::singleline(&mut d.find).hint_text(t("Search…")).desired_width(440.0));
                        egui::Frame::new().stroke(egui::Stroke::new(1.0, pal().border)).inner_margin(4.0).show(ui, |ui| {
                            egui::ScrollArea::vertical().id_salt("print-set-list").min_scrolled_height(LIST).max_height(LIST).show(
                                ui,
                                |ui| {
                                    ui.set_width(426.0);
                                    let words = crate::browser::words(&d.find);
                                    for (id, title, sheet) in &all {
                                        if !(if *sheet { d.sheets } else { d.views }) {
                                            continue;
                                        }
                                        if !words.iter().all(|w| crate::browser::spans(title, w).is_some()) {
                                            continue;
                                        }
                                        let mut on = d.checked.contains(id);
                                        if ui.checkbox(&mut on, title.as_str()).changed() {
                                            if on {
                                                d.checked.insert(*id);
                                            } else {
                                                d.checked.remove(id);
                                            }
                                        }
                                    }
                                },
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(t("Show"));
                            ui.checkbox(&mut d.sheets, t("Sheets"));
                            ui.checkbox(&mut d.views, t("Views"));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let shown: Vec<Id> =
                                    all.iter().filter(|(_, _, s)| if *s { d.sheets } else { d.views }).map(|(id, ..)| *id).collect();
                                if ui.button(t("Check None")).clicked() {
                                    shown.iter().for_each(|id| {
                                        d.checked.remove(id);
                                    });
                                }
                                if ui.button(t("Check All")).clicked() {
                                    d.checked.extend(shown);
                                }
                            });
                        });
                        ui.label(RichText::new(tf("{} checked", &[&d.checked.len().to_string()])).color(pal().muted));
                    });
                    // Revit's buttons, beside the list.
                    ui.vertical(|ui| {
                        let wide = |ui: &mut Ui, text: &str, enabled: bool| {
                            ui.add_enabled(enabled, egui::Button::new(t(text)).min_size(egui::vec2(120.0, 0.0))).clicked()
                        };
                        if wide(ui, "Save", changed)
                            && let Some(name) = &d.current
                        {
                            run.push(json!({"name": name, "views": ordered(&d)}));
                        }
                        if wide(ui, "Save As…", true) {
                            d.naming = Some((Naming::New, String::new()));
                        }
                        if wide(ui, "Revert", changed) {
                            then = Some(d.current.clone());
                        }
                        if wide(ui, "Rename…", d.current.is_some()) {
                            d.naming = Some((Naming::Rename, d.current.clone().unwrap_or_default()));
                        }
                        if wide(ui, "Delete", d.current.is_some())
                            && let Some(name) = &d.current
                        {
                            run.push(json!({"name": name, "delete": true}));
                            then = Some(None);
                        }
                    });
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.button(t("Cancel")).clicked() {
                        close = true;
                    }
                    if ui.button(t("OK")).clicked() {
                        if changed && let Some(name) = &d.current {
                            run.push(json!({"name": name, "views": ordered(&d)}));
                        }
                        close = true;
                    }
                });
            });
        });
        let mut failed = None;
        for params in run {
            if self.run("print_set", params).is_none() {
                failed = Some(self.status.text.clone());
                break;
            }
        }
        match failed {
            Some(e) => {
                d.error = Some(e);
                close = false;
            }
            None => {
                if let Some(name) = then {
                    let first = self.session.model.print_sets.keys().next().cloned();
                    d.naming = None;
                    d.error = None;
                    d.show_set(&self.session.model, name.or(first));
                }
            }
        }
        if !(close || r.should_close()) {
            self.print_sets = Some(d);
        }
    }

    /// The name asked for a print set of the views picked in the Project Browser.
    pub(crate) fn name_set_window(&mut self, ctx: &Context) {
        let Some(mut n) = self.name_set.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let r = Modal::new(egui::Id::new("name-print-set"), t("Create Print Set")).width(360.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(tf("A print set of the {} views and sheets picked.", &[&n.views.len().to_string()]));
                ui.horizontal(|ui| {
                    let l = ui.label(t("Name"));
                    let edit = egui::TextEdit::singleline(&mut n.name).char_limit(bimcraft_core::printset::MAX_NAME).desired_width(240.0);
                    let r = ui.add(edit).labelled_by(l.id);
                    r.request_focus();
                    ok = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                });
                if let Some(e) = &n.error {
                    ui.label(RichText::new(e.as_str()).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok |= ui.add_enabled(!n.name.trim().is_empty(), egui::Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok && !n.name.trim().is_empty() {
            let name = n.name.trim().to_string();
            if self.run("print_set", json!({"name": name, "views": n.views})).is_some() {
                self.status = crate::Status { text: tf("Print set {} saved.", &[&name]), error: false };
                return;
            }
            n.error = Some(std::mem::take(&mut self.status).text);
        }
        if !(cancel || r.should_close()) {
            self.name_set = Some(n);
        }
    }
}

impl Dialog {
    /// Shows set `name` as it is saved; none, nothing checked.
    fn show_set(&mut self, m: &bimcraft_core::model::Model, name: Option<String>) {
        self.checked = name
            .as_ref()
            .and_then(|n| m.print_sets.get(n))
            .map(|s| s.views.iter().filter_map(|g| m.elements.id(*g)).collect())
            .unwrap_or_default();
        self.current = name;
    }
}
