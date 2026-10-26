//! Phasing and Design Options, as Revit has them: the Phasing dialog (the project's phases in order,
//! the phase filters that say how a view shows each status of what is in its phase, and how those
//! statuses draw where a filter says so), the Design Options dialog with the active option in the
//! status bar, and the Phasing groups of the Properties palette. Each click runs its command
//! (`phase`, `phase_filter`, `option`, `modify` or `graphics`) as it goes, as Project Parameters
//! does; the one thing edited as a draft is a status's graphic overrides, applied as one command.

use bimcraft_i18n::msg;

use std::sync::Arc;

use bimcraft_core::graphics::{Group, Override};
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::phasing::{self, Presentation, Status};
use egui::{Align, Button, ComboBox, Context, Frame, Grid, Id as UiId, Layout, Margin, Modal, RichText, ScrollArea, TextEdit, Ui, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::graphics::{controls, editor, merge_patch};
use crate::lang::{category, t, tf};
use crate::properties::{combo, group_header};
use crate::style::pal;

/// The Phasing dialog's tabs.
const TABS: [&str; 3] = [msg!("Project Phasing"), "Phase Filters", msg!("Graphic Overrides")];

/// How a phase filter shows a status.
const PRESENTATIONS: [Presentation; 3] = [Presentation::ByCategory, Presentation::Overridden, Presentation::NotDisplayed];

/// A command waiting to run after the dialog is drawn: its id and parameters.
type Pending = Option<(&'static str, Value)>;

/// The Phasing dialog: its tab, the phase and the phase filter picked, the names typed, and the
/// graphic overrides of the status shown, as edited and not yet applied.
#[derive(Default)]
pub(crate) struct PhasingDialog {
    tab: usize,
    phase: String,
    name: String,
    filter: String,
    filter_name: String,
    status: usize,
    draft: Option<Override>,
}

/// The Design Options dialog: the set and the option picked, and the name typed.
#[derive(Default)]
pub(crate) struct OptionsDialog {
    set: String,
    option: String,
    name: String,
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::New => "New",
        Status::Existing => "Existing",
        Status::Demolished => msg!("Demolished"),
        Status::Temporary => msg!("Temporary"),
    }
}

fn presentation_name(p: Presentation) -> &'static str {
    match p {
        Presentation::ByCategory => msg!("By Category"),
        Presentation::Overridden => msg!("Overridden"),
        Presentation::NotDisplayed => msg!("Not Displayed"),
    }
}

impl BimApp {
    /// What a command makes is made in the active design option, and in the phase of the view on
    /// screen when that view has one of its own or a phase filter; else in the project's first
    /// phase, which leaves no value.
    pub(crate) fn made_in(&mut self) {
        let m = &self.session.model;
        let own = self.graphics_now();
        let g = own.resolved(m);
        let phase = (g.phase.is_some() || g.phase_filter.is_some()).then(|| g.phase.clone().or_else(|| m.phases.last().cloned())).flatten();
        let option = self.active_option.clone().filter(|o| phasing::option_exists(m, o));
        (self.session.phase, self.session.option) = (phase, option);
    }

    /// Demolish (DW): element `id` comes down in the phase of the view on screen, the project's last unless the view has
    /// its own, or, when it comes down in that phase already, goes back up: one `modify`, one undo step. A door or window
    /// with no phase of its own follows its wall, so only a note says so.
    pub(crate) fn demolish(&mut self, id: Id) {
        let m = Arc::clone(&self.session.model);
        let Some(e) = m.elements.get(id) else { return };
        let what = category(e.category(), false);
        let Some(held) = e.params() else {
            let text = tf("{} holds no parameters, so it has no phase to demolish", &[&what]);
            self.status = crate::Status { text, error: true };
            return;
        };
        let Some(phase) = self.graphics_now().resolved(&m).phase.clone().or_else(|| m.phases.last().cloned()) else { return };
        let own = held.get(phasing::DEMOLISHED).and_then(Value::as_str);
        if own.is_none() && phasing::value(&m, e, phasing::DEMOLISHED) == Some(phase.as_str()) {
            let text = tf("{} follows its wall, which comes down in {}: take the wall back instead", &[&what, &phase]);
            self.status = crate::Status { text, error: false };
            return;
        }
        let down = if own == Some(phase.as_str()) { Value::Null } else { json!(phase) };
        self.run("modify", json!({"id": id, "params": {phasing::DEMOLISHED: down}}));
    }

    /// Whether the active design option is one the view on screen doesn't show (it shows another option of its set):
    /// what is made in it would not be seen.
    pub(crate) fn active_option_unseen(&self) -> bool {
        let m = &self.session.model;
        let Some((set, name)) = self.active_option.as_deref().and_then(|o| o.split_once(": ")) else { return false };
        if self.graphic_view().is_none() {
            return false;
        }
        let own = self.graphics_now();
        let g = own.resolved(m);
        let shown = g.options.get(set).or_else(|| m.option_sets.get(set).map(|s| &s.primary));
        shown.map(String::as_str) != Some(name)
    }

    /// Make `option` (`set: option`; none is the main model) what new elements are made in, and show
    /// it in the view on screen, which would otherwise leave out what is made.
    pub(crate) fn activate(&mut self, option: Option<String>) {
        if self.run("option", json!({"now": option})).is_none() {
            return;
        }
        if self.active_option_unseen()
            && let Some((set, name)) = option.as_deref().and_then(|o| o.split_once(": "))
            && let Some(view) = self.graphic_view()
        {
            self.run("graphics", json!({"view": view, "options": {set: name}}));
        }
    }

    /// The active design option, in the status bar: what new elements are made in. Nothing while the
    /// project has no sets of options.
    pub(crate) fn option_selector(&mut self, ui: &mut Ui) {
        let m = Arc::clone(&self.session.model);
        if m.option_sets.is_empty() {
            return;
        }
        let active = self.active_option.clone().filter(|o| phasing::option_exists(&m, o));
        let main = t("Main Model").to_string();
        let mut picked = None;
        // Right to left: the box first, its label to its left. Red, when this view shows another option of its set.
        let unseen = self.active_option_unseen();
        let shown = RichText::new(active.clone().unwrap_or_else(|| main.clone()));
        let shown = if unseen { shown.color(pal().error) } else { shown };
        let c = ComboBox::from_id_salt("active-option").selected_text(shown).width(190.0).show_ui(ui, |ui| {
            if ui.selectable_label(active.is_none(), &main).clicked() {
                picked = Some(None);
            }
            for (set, s) in &m.option_sets {
                for o in &s.options {
                    let name = format!("{set}: {o}");
                    if ui.selectable_label(active.as_ref() == Some(&name), &name).clicked() {
                        picked = Some(Some(name));
                    }
                }
            }
        });
        let l = ui.label(t("Active design option:"));
        let tip = if unseen {
            t("This view shows another option of that set: pick it again to show it here")
        } else {
            t("New elements are made in this option; the view on screen shows it")
        };
        c.response.labelled_by(l.id).on_hover_text(tip);
        if let Some(option) = picked {
            self.activate(option);
        }
    }

    /// The Phasing dialog, while it's open.
    pub(crate) fn phasing_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.phasing_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        if !model.phases.contains(&d.phase) {
            d.phase = model.phases.last().cloned().unwrap_or_default();
        }
        if !model.phase_filters.contains_key(&d.filter) {
            d.filter = model.phase_filters.keys().next().cloned().unwrap_or_default();
        }
        let (mut close, mut run, mut phase, mut filter) = (false, None, None::<String>, None::<String>);
        let r = Modal::new(UiId::new("phasing")).show(ctx, |ui| {
            // As large on every tab, so the dialog doesn't move about as the tabs change.
            ui.set_width(640.0);
            ui.set_min_height(380.0);
            ui.label(RichText::new(t("Phasing")).heading());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, label) in TABS.iter().enumerate() {
                    if ui.selectable_label(d.tab == i, t(label)).clicked() {
                        d.tab = i;
                        d.draft = None;
                    }
                }
            });
            ui.separator();
            match d.tab {
                0 => project_phasing(ui, &mut d, &model, &mut run, &mut phase),
                1 => phase_filters(ui, &mut d, &model, &mut run, &mut filter),
                _ => graphic_overrides(ui, &mut d, &model, &mut run),
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        // A refusal says why and leaves the dialog as it was.
        let ran = run.map(|(id, p)| self.run(id, p).is_some());
        if ran == Some(true) {
            d.draft = None;
            if let Some(n) = phase.take() {
                (d.phase, d.name) = (n, String::new());
            }
            if let Some(n) = filter.take() {
                (d.filter, d.filter_name) = (n, String::new());
            }
        }
        // Picking from a list isn't a command.
        if ran.is_none() {
            if let Some(n) = phase {
                d.phase = n;
            }
            if let Some(n) = filter {
                d.filter = n;
            }
        }
        if !(close || r.should_close()) {
            self.phasing_dialog = Some(d);
        }
    }

    /// The Design Options dialog, while it's open.
    pub(crate) fn options_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.options_dialog.take() else { return };
        let model = Arc::clone(&self.session.model);
        if !model.option_sets.contains_key(&d.set) {
            d.set = model.option_sets.keys().next().cloned().unwrap_or_default();
        }
        let set = model.option_sets.get(&d.set).cloned();
        if !set.as_ref().is_some_and(|s| s.options.contains(&d.option)) {
            d.option = set.as_ref().map(|s| s.primary.clone()).unwrap_or_default();
        }
        let (mut close, mut run, mut pick, mut edit) = (false, None::<Value>, None::<(String, String)>, None::<Option<String>>);
        let r = Modal::new(UiId::new("design-options")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.set_min_height(300.0);
            ui.label(RichText::new(t("Design Options")).heading());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let l = ui.label(t("Option set:"));
                let c = ComboBox::from_id_salt("option-set").selected_text(&d.set).width(220.0).show_ui(ui, |ui| {
                    for n in model.option_sets.keys() {
                        if ui.selectable_label(*n == d.set, n).clicked() {
                            pick = Some((n.clone(), String::new()));
                        }
                    }
                });
                c.response.labelled_by(l.id);
            });
            match &set {
                Some(s) => {
                    ui.add_space(4.0);
                    ui.label(t("Options:"));
                    for o in &s.options {
                        let text = if *o == s.primary { tf("{} (primary)", &[o]) } else { o.clone() };
                        if ui.selectable_label(d.option == *o, text).clicked() {
                            pick = Some((d.set.clone(), o.clone()));
                        }
                    }
                }
                None => {
                    ui.add_space(4.0);
                    ui.label(RichText::new(t("No design option sets yet: New Set makes one.")).color(pal().muted));
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let l = ui.label(t("Name:"));
                ui.add(TextEdit::singleline(&mut d.name).desired_width(220.0)).labelled_by(l.id);
            });
            let name = d.name.trim().to_string();
            let typed = !name.is_empty();
            let options: Vec<String> = set.as_ref().map(|s| s.options.clone()).unwrap_or_default();
            let (has_set, has_option) = (set.is_some(), set.is_some() && options.contains(&d.option));
            ui.horizontal(|ui| {
                if ui.add_enabled(typed && !model.option_sets.contains_key(&name), Button::new(t("New Set"))).clicked() {
                    run = Some(json!({"set": name, "options": [format!("{} 1", t("Option"))]}));
                    pick = Some((name.clone(), String::new()));
                }
                if ui.add_enabled(typed && has_set && !model.option_sets.contains_key(&name), Button::new(t("Rename Set"))).clicked() {
                    run = Some(json!({"set": d.set, "rename": name}));
                    pick = Some((name.clone(), d.option.clone()));
                }
                let tip = t("Delete Set: the set goes, and every element in its options");
                if ui.add_enabled(has_set, Button::new(t("Delete Set"))).on_hover_text(tip).clicked() {
                    run = Some(json!({"set": d.set, "delete": true}));
                }
                let tip = t("Accept Primary: the primary option's elements stay in the model; the others' elements and the set go");
                if ui.add_enabled(has_set, Button::new(t("Accept Primary"))).on_hover_text(tip).clicked() {
                    run = Some(json!({"set": d.set, "accept": true}));
                }
            });
            ui.horizontal(|ui| {
                let add = typed && has_set && !options.contains(&name);
                if ui.add_enabled(add, Button::new(t("Add Option"))).clicked() {
                    let list: Vec<&String> = options.iter().chain(std::iter::once(&name)).collect();
                    run = Some(json!({"set": d.set, "options": list}));
                    pick = Some((d.set.clone(), name.clone()));
                }
                if ui.add_enabled(add && has_option, Button::new(t("Rename Option"))).clicked() {
                    run = Some(json!({"set": d.set, "option": d.option, "rename": name}));
                    pick = Some((d.set.clone(), name.clone()));
                }
                if ui.add_enabled(has_option, Button::new(t("Make Primary"))).clicked() {
                    run = Some(json!({"set": d.set, "primary": d.option}));
                }
                if ui.add_enabled(has_option, Button::new(t("Delete Option"))).clicked() {
                    let list: Vec<&String> = options.iter().filter(|o| **o != d.option).collect();
                    run = Some(json!({"set": d.set, "options": list}));
                }
                let tip = t("Edit Option: new elements are made in this option, and the view on screen shows it");
                if ui.add_enabled(has_option, Button::new(t("Edit Option"))).on_hover_text(tip).clicked() {
                    edit = Some(Some(format!("{}: {}", d.set, d.option)));
                }
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Close")).clicked());
        });
        let ran = run.map(|p| self.run("option", p).is_some());
        if ran == Some(true) {
            d.name.clear();
        }
        if ran != Some(false)
            && let Some((set, option)) = pick
        {
            (d.set, d.option) = (set, option);
        }
        if let Some(option) = edit {
            self.activate(option);
        }
        if !(close || r.should_close()) {
            self.options_dialog = Some(d);
        }
    }

    /// Properties' Phasing group for a view: the phase it shows, and the phase filter that says how
    /// each status of what's in it shows.
    pub(crate) fn phasing_properties(&mut self, ui: &mut Ui) {
        let Some(view) = self.graphic_view() else { return };
        let model = Arc::clone(&self.session.model);
        let own = self.graphics_now();
        let g = own.resolved(&model).into_owned();
        let follows = controls(&model, own.template.as_ref()).contains(&Group::Phase);
        let group = "Phasing";
        let open = !self.collapsed.contains(group);
        if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let mut set = None::<Value>;
        Frame::new().inner_margin(Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new("phasing-properties").num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(ui, |ui| {
                let label = ui.label(t("Phase Filter"));
                let none = t("<None>").to_string();
                let now = g.phase_filter.clone().unwrap_or_else(|| none.clone());
                let options: Vec<String> = std::iter::once(none.clone()).chain(model.phase_filters.keys().cloned()).collect();
                ui.add_enabled_ui(!follows, |ui| {
                    if let Some(picked) = combo(ui, (0, "phase-filter"), label.id, &now, &options) {
                        set = Some(json!({"view": view, "phase_filter": if picked == none { Value::Null } else { json!(picked) }}));
                    }
                });
                ui.end_row();
                // The phase it shows now is the last unless it says; any phase picked, that one too, is its own.
                let label = ui.label(t("Phase"));
                let shown = g.phase.clone().or_else(|| model.phases.last().cloned()).unwrap_or_default();
                ui.add_enabled_ui(!follows, |ui| {
                    let c = ComboBox::from_id_salt((0, "phase")).selected_text(&shown).width(ui.available_width()).show_ui(ui, |ui| {
                        for p in &model.phases {
                            if ui.selectable_label(*p == shown, p).clicked() && g.phase.as_ref() != Some(p) {
                                set = Some(json!({"view": view, "phase": p}));
                            }
                        }
                    });
                    c.response.labelled_by(label.id);
                });
                ui.end_row();
            });
        });
        if let Some(p) = set {
            self.run("graphics", p);
        }
    }

    /// Properties' Phasing group for elements: the phase each was made in and the one it comes down
    /// in, and its design option while the project has any. A value the selection doesn't share is
    /// left blank.
    pub(crate) fn element_phasing(&mut self, ui: &mut Ui, model: &Model, ids: &[Id]) {
        let held: Vec<&Element> = ids.iter().filter_map(|id| model.elements.get(id)).collect();
        // Annotation holds no parameters: it belongs to its view, not to a phase.
        let (Some(&id), false) = (ids.first(), held.is_empty() || held.iter().any(|e| e.params().is_none())) else { return };
        let one = |key: &str| -> Option<Option<String>> {
            let of = |e: &&Element| phasing::value(model, e, key).map(String::from);
            let first = held.first().and_then(of);
            held.iter().all(|e| of(e) == first).then_some(first)
        };
        let (first, none, main) = (model.phases.first().cloned().unwrap_or_default(), t("<None>").to_string(), t("Main Model").to_string());
        let said = |v: Option<Option<String>>, default: &String| v.map(|v| v.unwrap_or_else(|| default.clone())).unwrap_or_default();
        let created = said(one(phasing::CREATED), &first);
        let demolished = said(one(phasing::DEMOLISHED), &none);
        let option = said(one(phasing::OPTION), &main);
        let group = "Phasing";
        let open = !self.collapsed.contains(group);
        if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let mut change: Option<(&str, Value)> = None;
        Frame::new().inner_margin(Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new(("phasing-parameters", id)).num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(ui, |ui| {
                let label = ui.label(t("Phase Created"));
                // Written out even for the first phase: a door with none follows its wall.
                if let Some(p) = combo(ui, (id, "phase-created"), label.id, &created, &model.phases) {
                    change = Some((phasing::CREATED, json!(p)));
                }
                ui.end_row();
                let label = ui.label(t("Phase Demolished"));
                let phases: Vec<String> = std::iter::once(none.clone()).chain(model.phases.iter().cloned()).collect();
                if let Some(p) = combo(ui, (id, "phase-demolished"), label.id, &demolished, &phases) {
                    change = Some((phasing::DEMOLISHED, if p == none { Value::Null } else { json!(p) }));
                }
                ui.end_row();
                if !model.option_sets.is_empty() {
                    let label = ui.label(t("Design Option"));
                    let all = model.option_sets.iter().flat_map(|(set, s)| s.options.iter().map(move |o| format!("{set}: {o}")));
                    let options: Vec<String> = std::iter::once(main.clone()).chain(all).collect();
                    if let Some(p) = combo(ui, (id, "design-option"), label.id, &option, &options) {
                        change = Some((phasing::OPTION, if p == main { Value::Null } else { json!(p) }));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((key, v)) = change {
            self.run("modify", json!({"ids": ids, "params": {key: v}}));
        }
    }
}

/// Project Phasing: the phases in order, one picked, and what the name typed does with it.
fn project_phasing(ui: &mut Ui, d: &mut PhasingDialog, m: &Model, run: &mut Pending, pick: &mut Option<String>) {
    ui.label(t("Phases, in order:"));
    ScrollArea::vertical().max_height(170.0).auto_shrink([false, true]).id_salt("phasing-phases").show(ui, |ui| {
        for p in &m.phases {
            if ui.selectable_label(d.phase == *p, p).clicked() {
                *pick = Some(p.clone());
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let l = ui.label(t("Name:"));
        ui.add(TextEdit::singleline(&mut d.name).desired_width(220.0)).labelled_by(l.id);
    });
    let name = d.name.trim().to_string();
    let (typed, at) = (!name.is_empty() && !m.phases.contains(&name), m.phases.iter().position(|p| *p == d.phase));
    ui.horizontal(|ui| {
        if ui.add_enabled(typed, Button::new(t("Add at End"))).clicked() {
            *run = Some(("phase", json!({"name": name})));
            *pick = Some(name.clone());
        }
        if ui.add_enabled(typed && at.is_some(), Button::new(t("Insert Before"))).clicked() {
            *run = Some(("phase", json!({"name": name, "before": d.phase})));
            *pick = Some(name.clone());
        }
        if ui.add_enabled(typed && at.is_some(), Button::new(t("Insert After"))).clicked() {
            *run = Some(("phase", json!({"name": name, "after": d.phase})));
            *pick = Some(name.clone());
        }
        if ui.add_enabled(typed && at.is_some(), Button::new(t("Rename"))).clicked() {
            *run = Some(("phase", json!({"name": d.phase, "rename": name})));
            *pick = Some(name.clone());
        }
    });
    // Combining takes the phase out: what was made, taken down and shown in it is the neighbour's.
    let before = at.and_then(|i| i.checked_sub(1)).and_then(|i| m.phases.get(i));
    let after = at.and_then(|i| m.phases.get(i + 1));
    ui.horizontal(|ui| {
        let tip = t("Combine: what was made, taken down and shown in this phase goes to the neighbouring one, and this one goes");
        for (label, into) in [(msg!("Combine with Previous"), before), (msg!("Combine with Next"), after)] {
            if ui.add_enabled(into.is_some(), Button::new(t(label))).on_hover_text(tip).clicked()
                && let Some(into) = into
            {
                *run = Some(("phase", json!({"name": d.phase, "delete": true, "into": into})));
                *pick = Some(into.clone());
            }
        }
    });
}

/// Phase Filters: the filters, one picked, and how it shows each status.
fn phase_filters(ui: &mut Ui, d: &mut PhasingDialog, m: &Model, run: &mut Pending, pick: &mut Option<String>) {
    ui.horizontal(|ui| {
        let l = ui.label(t("Phase filter:"));
        let c = ComboBox::from_id_salt("phase-filter-name").selected_text(&d.filter).width(260.0).show_ui(ui, |ui| {
            for n in m.phase_filters.keys() {
                if ui.selectable_label(*n == d.filter, n).clicked() {
                    *pick = Some(n.clone());
                }
            }
        });
        c.response.labelled_by(l.id);
        if ui.add_enabled(m.phase_filters.contains_key(&d.filter), Button::new(t("Delete"))).clicked() {
            *run = Some(("phase_filter", json!({"name": d.filter, "delete": true})));
            *pick = m.phase_filters.keys().find(|n| **n != d.filter).cloned();
        }
    });
    ui.horizontal(|ui| {
        let l = ui.label(t("New filter:"));
        ui.add(TextEdit::singleline(&mut d.filter_name).desired_width(220.0)).labelled_by(l.id);
        let name = d.filter_name.trim().to_string();
        if ui.add_enabled(!name.is_empty() && !m.phase_filters.contains_key(&name), Button::new(t("New Filter"))).clicked() {
            *run = Some(("phase_filter", json!({"name": name})));
            *pick = Some(name);
        }
    });
    let Some(f) = m.phase_filters.get(&d.filter) else { return };
    ui.add_space(8.0);
    ui.label(RichText::new(t("How it shows what is, in the view's phase:")).strong());
    Grid::new("phase-filter-statuses").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
        for s in Status::ALL {
            let l = ui.label(t(status_name(s)));
            let now = f.of(s);
            let c = ComboBox::from_id_salt(("phase-filter-status", s.name())).selected_text(t(presentation_name(now))).width(200.0);
            let c = c.show_ui(ui, |ui| {
                for p in PRESENTATIONS {
                    if ui.selectable_label(p == now, t(presentation_name(p))).clicked() && p != now {
                        *run = Some(("phase_filter", json!({"name": d.filter, s.name(): p})));
                    }
                }
            });
            c.response.labelled_by(l.id);
            ui.end_row();
        }
    });
}

/// Graphic Overrides: how each status draws where a phase filter says Overridden. The status shown
/// is edited as a draft that Apply Overrides sends as one command.
fn graphic_overrides(ui: &mut Ui, d: &mut PhasingDialog, m: &Model, run: &mut Pending) {
    ui.horizontal(|ui| {
        for (i, s) in Status::ALL.into_iter().enumerate() {
            if ui.selectable_label(d.status == i, t(status_name(s))).clicked() {
                (d.status, d.draft) = (i, None);
            }
        }
    });
    let status = Status::ALL[d.status.min(Status::ALL.len() - 1)];
    let own = m.phase_overrides.get(&status).cloned().unwrap_or_default();
    ui.label(RichText::new(t("A phase filter draws a status this way where it says Overridden.")).color(pal().muted));
    ui.add_space(4.0);
    let draft = d.draft.get_or_insert_with(|| own.clone());
    // Phase filters hide what they don't display; overrides don't see through either.
    editor(ui, draft, true, false);
    let differs = *draft != own;
    if ui.add_enabled(differs, Button::new(t("Apply Overrides"))).clicked() {
        let patch = merge_patch(&json!(own), &json!(draft));
        *run = Some(("phase", json!({"overrides": {status.name(): patch}})));
    }
}
