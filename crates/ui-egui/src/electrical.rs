//! Manage > Electrical Settings, as Revit's: the project's voltages, distribution systems and wire types, each a table with rows to
//! add and take away, edited as a draft and applied as one undo step through the `voltage`, `distribution_system` and `wire_type`
//! commands (OK and Apply), or dropped (Cancel). Sections are mm², always, as the core keeps them.

use std::collections::BTreeSet;

use bimcraft_core::electrical::{EARTHINGS, MATERIALS, MAX_SIZES, MAX_SYSTEMS, MAX_VOLTAGES, MAX_WIRE_TYPES, Voltage, WireSize, WireType};
use bimcraft_core::engine::Session;
use bimcraft_core::model::Model;
use bimcraft_i18n::msg;
use egui::{Align, Button, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, RichText, TextEdit, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::renames;
use crate::style::{ERROR_HEIGHT, boxed_text, pal};
use crate::{BimApp, Status};

/// The dialog's tabs.
const TABS: [&str; 3] = [msg!("Voltages"), msg!("Distribution Systems"), msg!("Wire Types")];

/// The conductor materials, as the core names them, and as the window does.
const MATERIAL_LABELS: [(&str, &str); 2] = [("copper", msg!("copper")), ("aluminium", msg!("aluminium"))];

/// What a table's row says for "no voltage": the line to leave empty.
const NONE: &str = "—";

/// A row of a table: what it was when the dialog opened (none, a new one), and what it is.
#[derive(Clone, Debug, PartialEq)]
struct Row<T> {
    /// What other rows name it by, which its name's being typed over does not change.
    key: u64,
    was: Option<(String, T)>,
    name: String,
    value: T,
}

/// A distribution system as the dialog holds it: the voltages it supplies are rows of the Voltages tab, by their keys, so that a
/// voltage typed over is still the one it named.
#[derive(Clone, Debug, PartialEq)]
struct Supply {
    phases: u8,
    wires: u8,
    l_l: Option<u64>,
    l_n: Option<u64>,
    l_g: Option<u64>,
    ground: Option<String>,
}

/// The Electrical Settings dialog: its tab, and the three tables as edited and not yet applied.
pub(crate) struct ElectricalDialog {
    tab: usize,
    voltages: Vec<Row<Voltage>>,
    systems: Vec<Row<Supply>>,
    wires: Vec<Row<WireType>>,
    /// The names of the voltages, systems and wire types taken away.
    gone: [Vec<String>; 3],
    /// The wire type shown on its tab.
    wire: usize,
    next: u64,
    error: Option<String>,
}

impl ElectricalDialog {
    /// Open on the project's electrical settings.
    pub(crate) fn new(m: &Model) -> ElectricalDialog {
        let mut d = ElectricalDialog {
            tab: 0,
            voltages: Vec::new(),
            systems: Vec::new(),
            wires: Vec::new(),
            gone: Default::default(),
            wire: 0,
            next: 0,
            error: None,
        };
        for (n, v) in &m.voltages {
            let key = d.key();
            d.voltages.push(Row { key, was: Some((n.clone(), v.clone())), name: n.clone(), value: v.clone() });
        }
        for (n, s) in &m.distribution_systems {
            let key = d.key();
            let by = |name: &Option<String>| name.as_deref().and_then(|n| d.voltages.iter().find(|r| r.name == n)).map(|r| r.key);
            let value =
                Supply { phases: s.phases, wires: s.wires, l_l: by(&s.l_l), l_n: by(&s.l_n), l_g: by(&s.l_g), ground: s.ground.clone() };
            d.systems.push(Row { key, was: Some((n.clone(), value.clone())), name: n.clone(), value });
        }
        for (n, w) in &m.wire_types {
            let key = d.key();
            d.wires.push(Row { key, was: Some((n.clone(), w.clone())), name: n.clone(), value: w.clone() });
        }
        d
    }

    /// What stopped OK or Apply, or Delete, if anything did.
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn key(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// A name for a new row that no row of `names` has: `base`, `base 2`…
    fn spare(names: &[&str], base: &str) -> String {
        std::iter::once(base.to_string())
            .chain((2..).map(|i| format!("{base} {i}")))
            .find(|n| !names.contains(&n.as_str()))
            .unwrap_or_default()
    }

    fn add_voltage(&mut self) {
        let names: Vec<&str> = self.voltages.iter().map(|r| r.name.as_str()).collect();
        let name = Self::spare(&names, t("New Voltage"));
        let key = self.key();
        self.voltages.push(Row { key, was: None, name, value: Voltage::around(230.0) });
    }

    /// Takes voltage row `i` away, unless a distribution system supplies it.
    fn delete_voltage(&mut self, i: usize) {
        let Some(r) = self.voltages.get(i) else { return };
        if let Some(s) = self.systems.iter().find(|s| [s.value.l_l, s.value.l_n, s.value.l_g].contains(&Some(r.key))) {
            self.error = Some(tf("Voltage {} is supplied by distribution system {}: change that first.", &[&r.name, &s.name]));
            return;
        }
        let r = self.voltages.remove(i);
        if let Some((was, _)) = r.was {
            self.gone[0].push(was);
        }
    }

    fn add_system(&mut self) {
        let names: Vec<&str> = self.systems.iter().map(|r| r.name.as_str()).collect();
        let name = Self::spare(&names, t("New Distribution System"));
        let l_l = self.voltages.first().map(|r| r.key);
        let key = self.key();
        self.systems.push(Row { key, was: None, name, value: Supply { phases: 3, wires: 4, l_l, l_n: None, l_g: None, ground: None } });
    }

    fn delete_system(&mut self, i: usize) {
        if i < self.systems.len()
            && let Some((was, _)) = self.systems.remove(i).was
        {
            self.gone[1].push(was);
        }
    }

    fn add_wire(&mut self) {
        let names: Vec<&str> = self.wires.iter().map(|r| r.name.as_str()).collect();
        let name = Self::spare(&names, t("New Wire Type"));
        let key = self.key();
        let value = WireType {
            material: MATERIALS[0].into(),
            temperature: 70.0,
            insulation: "PVC".into(),
            note: String::new(),
            sizes: vec![WireSize { section: 1.5, ampacity: 15.0, label: None }],
        };
        self.wires.push(Row { key, was: None, name, value });
        self.wire = self.wires.len() - 1;
    }

    fn delete_wire(&mut self) {
        if self.wire < self.wires.len()
            && let Some((was, _)) = self.wires.remove(self.wire).was
        {
            self.gone[2].push(was);
        }
        self.wire = self.wire.min(self.wires.len().saturating_sub(1));
    }

    /// The draft is something the core may be asked for: every row has a name, and no two of a table share one.
    fn check(&self) -> Result<(), String> {
        fn names(one: &str, several: &str, names: Vec<&str>) -> Result<(), String> {
            let mut seen = BTreeSet::new();
            for n in names.into_iter().map(str::trim) {
                if n.is_empty() {
                    return Err(tf("A {} needs a name.", &[t(one)]));
                }
                if !seen.insert(n) {
                    return Err(tf("Two {} are called {}.", &[t(several), n]));
                }
            }
            Ok(())
        }
        names(msg!("voltage"), msg!("voltages"), self.voltages.iter().map(|r| r.name.as_str()).collect())?;
        names(msg!("distribution system"), msg!("distribution systems"), self.systems.iter().map(|r| r.name.as_str()).collect())?;
        names(msg!("wire type"), msg!("wire types"), self.wires.iter().map(|r| r.name.as_str()).collect())
    }

    /// The name of the voltage row `key`, as the core will have it by then.
    fn voltage_name(&self, key: Option<u64>) -> String {
        key.and_then(|k| self.voltages.iter().find(|r| r.key == k)).map(|r| r.name.trim().to_string()).unwrap_or_default()
    }

    fn system_params(&self, name: &str, s: &Supply) -> Value {
        json!({
            "name": name, "phases": s.phases, "wires": s.wires,
            "l_l": self.voltage_name(s.l_l), "l_n": self.voltage_name(s.l_n), "l_g": self.voltage_name(s.l_g),
            "ground": s.ground.clone().unwrap_or_default(),
        })
    }

    /// What the draft asks of the project, in an order the core can do it in: the voltages made, renamed and changed before the
    /// systems that name them, and the ones taken away after the systems that no longer do; the renames of each table by
    /// `renames::plan`, which gets chains, swaps and names freed right.
    fn commands(&self, m: &Model) -> Result<Vec<(&'static str, Value)>, String> {
        self.check()?;
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        let plan = |existing: BTreeSet<String>, rows: Vec<renames::Row>| renames::plan(&existing, &rows);
        fn rows<'a, T>(table: &'a [Row<T>], gone: &'a [String]) -> Vec<renames::Row<'a>> {
            let live =
                table.iter().map(|r| renames::Row { was: r.was.as_ref().map(|(n, _)| n.as_str()), name: r.name.trim(), gone: false });
            live.chain(gone.iter().map(|n| renames::Row { was: Some(n.as_str()), name: n.as_str(), gone: true })).collect()
        }
        let renamed = |command: &'static str, out: &mut Vec<(&'static str, Value)>, renames: &[(String, String)]| {
            out.extend(renames.iter().map(|(from, to)| (command, json!({"name": from, "rename": to}))));
        };
        let deleted = |command: &'static str, out: &mut Vec<(&'static str, Value)>, names: &[String]| {
            out.extend(names.iter().map(|n| (command, json!({"name": n, "delete": true}))));
        };

        let v = plan(m.voltages.keys().cloned().collect(), rows(&self.voltages, &self.gone[0]));
        renamed("voltage", &mut out, &v.renames);
        for r in &self.voltages {
            if r.was.as_ref().is_none_or(|(_, was)| *was != r.value) {
                let (name, x) = (r.name.trim(), &r.value);
                out.push(("voltage", json!({"name": name, "nominal": x.nominal, "min": x.min, "max": x.max})));
            }
        }
        let s = plan(m.distribution_systems.keys().cloned().collect(), rows(&self.systems, &self.gone[1]));
        renamed("distribution_system", &mut out, &s.renames);
        // (A voltage renamed is renamed in the systems that name it by the core, so a system whose draft is as it was needs nothing.)
        for r in &self.systems {
            if r.was.as_ref().is_none_or(|(_, was)| *was != r.value) {
                out.push(("distribution_system", self.system_params(r.name.trim(), &r.value)));
            }
        }
        deleted("distribution_system", &mut out, &s.deletes);
        deleted("voltage", &mut out, &v.deletes);
        let w = plan(m.wire_types.keys().cloned().collect(), rows(&self.wires, &self.gone[2]));
        renamed("wire_type", &mut out, &w.renames);
        for r in &self.wires {
            if r.was.as_ref().is_none_or(|(_, was)| *was != r.value) {
                let (name, x) = (r.name.trim(), &r.value);
                out.push((
                    "wire_type",
                    json!({"name": name, "material": x.material, "temperature": x.temperature, "insulation": x.insulation, "note": x.note, "sizes": x.sizes}),
                ));
            }
        }
        deleted("wire_type", &mut out, &w.deletes);
        Ok(out)
    }
}

/// A material as the window shows it.
fn material_label(material: &str) -> &str {
    MATERIAL_LABELS.iter().find(|(m, _)| *m == material).map_or(material, |(_, label)| t(label))
}

/// A text field of `width`, named `name` for screen readers.
fn text_field(ui: &mut Ui, text: &mut String, width: f32, name: String) {
    let r = ui.add_sized([width, 22.0], TextEdit::singleline(text));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, name.clone()));
}

/// A number field.
fn number_field(ui: &mut Ui, x: &mut f64, range: std::ops::RangeInclusive<f64>, suffix: &str, name: String) {
    let r = ui.add(DragValue::new(x).range(range).speed(0.5).max_decimals(2).suffix(suffix));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, name.clone()));
}

/// A delete button for a row.
fn delete_button(ui: &mut Ui, name: String) -> bool {
    let r = ui.add(Button::new(t("Delete")).small());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    r.clicked()
}

/// The heads of a table's columns, the last of which may be none.
fn heads(ui: &mut Ui, heads: &[&str]) {
    for h in heads {
        ui.label(RichText::new(if h.is_empty() { "" } else { t(h) }).strong());
    }
    ui.end_row();
}

/// The voltages: a name and the band each may read within.
fn voltages_tab(ui: &mut Ui, d: &mut ElectricalDialog) {
    ui.label(t("The voltages of the project, in volts, and the band a device may read within. A distribution system names them."));
    ui.add_space(4.0);
    let mut delete = None;
    Grid::new("electrical-voltages").striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
        heads(ui, &[msg!("Name"), msg!("Nominal"), msg!("Minimum"), msg!("Maximum"), ""]);
        for (i, r) in d.voltages.iter_mut().enumerate() {
            let n = (i + 1).to_string();
            text_field(ui, &mut r.name, 180.0, tf("Voltage {} name", &[&n]));
            number_field(ui, &mut r.value.nominal, 1.0..=100_000.0, " V", tf("Voltage {} nominal", &[&n]));
            number_field(ui, &mut r.value.min, 1.0..=100_000.0, " V", tf("Voltage {} minimum", &[&n]));
            number_field(ui, &mut r.value.max, 1.0..=100_000.0, " V", tf("Voltage {} maximum", &[&n]));
            if delete_button(ui, tf("Delete voltage {}", &[&n])) {
                delete = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = delete {
        d.delete_voltage(i);
    }
    ui.add_space(6.0);
    if ui.add_enabled(d.voltages.len() < MAX_VOLTAGES, Button::new(t("Add Voltage"))).clicked() {
        d.add_voltage();
    }
}

/// A combo box choosing one of the voltages (or none) for a system's line.
fn voltage_combo(ui: &mut Ui, salt: (&str, u64), name: String, value: &mut Option<u64>, voltages: &[Row<Voltage>]) {
    let shown = value.and_then(|k| voltages.iter().find(|r| r.key == k)).map_or_else(|| NONE.to_string(), |r| r.name.clone());
    let c = ComboBox::from_id_salt(salt).selected_text(shown).width(120.0).show_ui(ui, |ui| {
        ui.selectable_value(value, None, NONE);
        for r in voltages {
            ui.selectable_value(value, Some(r.key), &r.name);
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
}

/// The distribution systems: phases and wires, and the voltages each supplies.
fn systems_tab(ui: &mut Ui, d: &mut ElectricalDialog) {
    ui.label(t(
        "How many phases and wires each system has (the line conductors and the neutral), the voltages it supplies, and how it is earthed.",
    ));
    ui.add_space(4.0);
    let mut delete = None;
    let ElectricalDialog { voltages, systems, .. } = &mut *d;
    Grid::new("electrical-systems").striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
        heads(
            ui,
            &[
                msg!("Name"),
                msg!("Phases"),
                msg!("Wires"),
                msg!("Line to line"),
                msg!("Line to neutral"),
                msg!("Line to earth"),
                msg!("Earthing"),
                "",
            ],
        );
        for (i, r) in systems.iter_mut().enumerate() {
            let n = (i + 1).to_string();
            text_field(ui, &mut r.name, 150.0, tf("System {} name", &[&n]));
            let c =
                ComboBox::from_id_salt(("system-phases", r.key)).selected_text(r.value.phases.to_string()).width(48.0).show_ui(ui, |ui| {
                    for p in 1..=3u8 {
                        ui.selectable_value(&mut r.value.phases, p, p.to_string());
                    }
                });
            c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, tf("System {} phases", &[&n])));
            let wires = ui.add(DragValue::new(&mut r.value.wires).range(2..=4).speed(0.1));
            wires.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, tf("System {} wires", &[&n])));
            voltage_combo(ui, ("system-ll", r.key), tf("System {} line to line", &[&n]), &mut r.value.l_l, voltages);
            voltage_combo(ui, ("system-ln", r.key), tf("System {} line to neutral", &[&n]), &mut r.value.l_n, voltages);
            voltage_combo(ui, ("system-lg", r.key), tf("System {} line to earth", &[&n]), &mut r.value.l_g, voltages);
            let shown = r.value.ground.clone().unwrap_or_else(|| NONE.to_string());
            let c = ComboBox::from_id_salt(("system-ground", r.key)).selected_text(shown).width(80.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut r.value.ground, None, NONE);
                for g in EARTHINGS {
                    ui.selectable_value(&mut r.value.ground, Some(g.to_string()), g);
                }
            });
            c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, tf("System {} earthing", &[&n])));
            if delete_button(ui, tf("Delete system {}", &[&n])) {
                delete = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = delete {
        d.delete_system(i);
    }
    ui.add_space(6.0);
    if ui.add_enabled(d.systems.len() < MAX_SYSTEMS, Button::new(t("Add Distribution System"))).clicked() {
        d.add_system();
    }
}

/// The wire types: one at a time, with its table of sections.
fn wires_tab(ui: &mut Ui, d: &mut ElectricalDialog) {
    ui.label(t("A conductor's material, insulation and working temperature, and the ampacity of each section, in mm²."));
    ui.add_space(4.0);
    d.wire = d.wire.min(d.wires.len().saturating_sub(1));
    ui.horizontal(|ui| {
        let l = ui.label(t("Wire type:"));
        let shown = d.wires.get(d.wire).map(|r| r.name.clone()).unwrap_or_default();
        let wires: Vec<(usize, String)> = d.wires.iter().enumerate().map(|(i, r)| (i, r.name.clone())).collect();
        let c = ComboBox::from_id_salt("electrical-wire").selected_text(shown).width(220.0).show_ui(ui, |ui| {
            for (i, n) in &wires {
                ui.selectable_value(&mut d.wire, *i, n);
            }
        });
        c.response.labelled_by(l.id);
        if ui.add_enabled(d.wires.len() < MAX_WIRE_TYPES, Button::new(t("Add Wire Type"))).clicked() {
            d.add_wire();
        }
        if ui.add_enabled(!d.wires.is_empty(), Button::new(t("Delete Wire Type"))).clicked() {
            d.delete_wire();
        }
    });
    let Some(r) = d.wires.get_mut(d.wire) else { return };
    ui.add_space(6.0);
    Grid::new("electrical-wire-fields").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(t("Name:"));
        text_field(ui, &mut r.name, 260.0, t("Wire type name").to_string());
        ui.end_row();
        ui.label(t("Material:"));
        let c = ComboBox::from_id_salt("electrical-wire-material").selected_text(material_label(&r.value.material)).width(140.0).show_ui(
            ui,
            |ui| {
                for (m, label) in MATERIAL_LABELS {
                    ui.selectable_value(&mut r.value.material, m.to_string(), t(label));
                }
            },
        );
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Wire material")));
        ui.end_row();
        ui.label(t("Temperature:"));
        number_field(ui, &mut r.value.temperature, 1.0..=250.0, " °C", t("Wire temperature").to_string());
        ui.end_row();
        ui.label(t("Insulation:"));
        text_field(ui, &mut r.value.insulation, 260.0, t("Wire insulation").to_string());
        ui.end_row();
        ui.label(t("Note:"));
        text_field(ui, &mut r.value.note, 520.0, t("Wire note").to_string());
        ui.end_row();
    });
    ui.add_space(8.0);
    let mut delete = None;
    Grid::new("electrical-wire-sizes").striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
        heads(ui, &[msg!("Section"), msg!("Ampacity"), msg!("Label"), ""]);
        for (i, s) in r.value.sizes.iter_mut().enumerate() {
            let n = (i + 1).to_string();
            number_field(ui, &mut s.section, 0.01..=10_000.0, " mm²", tf("Size {} section", &[&n]));
            number_field(ui, &mut s.ampacity, 0.1..=100_000.0, " A", tf("Size {} ampacity", &[&n]));
            let mut label = s.label.clone().unwrap_or_default();
            text_field(ui, &mut label, 120.0, tf("Size {} label", &[&n]));
            s.label = Some(label.trim().to_string()).filter(|l| !l.is_empty());
            if delete_button(ui, tf("Delete size {}", &[&n])) {
                delete = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = delete {
        r.value.sizes.remove(i);
    }
    ui.add_space(6.0);
    if ui.add_enabled(r.value.sizes.len() < MAX_SIZES, Button::new(t("Add Size"))).clicked() {
        // One more than the last: a larger section, carrying at least as much.
        let next = r.value.sizes.last().map_or(WireSize { section: 1.5, ampacity: 15.0, label: None }, |l| WireSize {
            section: (l.section * 1.5 * 10.0).round() / 10.0,
            ampacity: l.ampacity,
            label: None,
        });
        r.value.sizes.push(next);
    }
}

impl BimApp {
    /// The Electrical Settings dialog, while it's open.
    pub(crate) fn electrical_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.electrical_dialog.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(UiId::new("electrical-settings"), t("Electrical Settings")).width(940.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (i, label) in TABS.iter().enumerate() {
                        if ui.selectable_label(d.tab == i, t(label)).clicked() {
                            d.tab = i;
                        }
                    }
                });
                ui.separator();
                match d.tab {
                    0 => voltages_tab(ui, &mut d),
                    1 => systems_tab(ui, &mut d),
                    _ => wires_tab(ui, &mut d),
                }
                if let Some(error) = &d.error {
                    ui.add_space(6.0);
                    boxed_text(ui, "electrical-error", pal().error, error, ERROR_HEIGHT);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        let failed = (ok || apply) && !self.apply_electrical(&mut d, apply);
        if failed || !(ok || cancel || r.should_close()) {
            self.electrical_dialog = Some(d);
        }
    }

    /// OK and Apply: what changed, as one undo step, all or nothing; a refusal (a name blank or used twice, the core's reasons)
    /// keeps the dialog open to fix it, saying why in itself. Whether it was done.
    fn apply_electrical(&mut self, d: &mut ElectricalDialog, apply: bool) -> bool {
        d.error = None;
        let done = d.commands(&self.session.model).and_then(|commands| {
            if commands.is_empty() {
                return Ok(());
            }
            self.session
                .transaction("Electrical Settings", |s: &mut Session| commands.iter().try_for_each(|(c, p)| s.execute(c, p).map(|_| ())))
        });
        if let Err(e) = done {
            d.error = Some(e.clone());
            self.status = Status { text: e, error: true };
            return false;
        }
        self.status = Status::default();
        if apply {
            let (tab, wire) = (d.tab, d.wire);
            *d = ElectricalDialog::new(&self.session.model);
            (d.tab, d.wire) = (tab, wire.min(d.wires.len().saturating_sub(1)));
        }
        true
    }
}
