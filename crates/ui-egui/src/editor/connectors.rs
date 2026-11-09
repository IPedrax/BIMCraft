//! A family's connectors in the Family Editor, as Revit's: placed from Create > Connectors, listed in the Project Browser as
//! its forms are, and, selected, shown in Properties, where their numbers are typed.

use bimcraft_core::family::{Category, Connector, ConnectorKind, ConnectorSystem, Domain, Expr, Flow, Load};
use bimcraft_i18n::msg;
use egui::{ComboBox, TextEdit, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use super::{FamilyEditor, document, expr_text, field, split_top, text_expr};
use crate::lang::t;

/// A connector as Properties types it: the family's own, its numbers and formulas as text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ConnectorText {
    /// The connector as the family has it: its id, GUID, kind and the rest typed here written over it.
    base: Connector,
    /// Where it is, in the family's axes; all three blank, nowhere (an electrical device's may have no place).
    at: [String; 3],
    /// The way it faces, out of the family; all three blank, none.
    direction: [String; 3],
    size: String,
    voltage: String,
    /// Its load in VA, or one for each pole, by commas.
    load: String,
    power_factor: String,
}

/// The kinds Create > Connectors places, each with its button's name and tip.
pub(crate) const KINDS: [(ConnectorKind, &str, &str); 5] = [
    (ConnectorKind::Pipe, msg!("Pipe Connector"), msg!("Pipe Connector: where a pipe meets the family, its domain, flow and size")),
    (ConnectorKind::Power, msg!("Power Connector"), msg!("Power Connector: what an electrical device draws, its voltage and load")),
    (ConnectorKind::Switch, msg!("Switch Connector"), msg!("Switch Connector: what a switch controls")),
    (ConnectorKind::Data, msg!("Data Connector"), msg!("Data Connector: where a data cable meets the family")),
    (ConnectorKind::Earthing, msg!("Earthing Connector"), msg!("Earthing Connector: where an earthing conductor meets the family")),
];

impl ConnectorText {
    pub(crate) fn of(c: &Connector) -> ConnectorText {
        let xyz = |p: &Option<[Expr; 3]>| p.as_ref().map(|p| p.each_ref().map(expr_text)).unwrap_or_default();
        let load = match &c.load_va {
            Load::One(e) => expr_text(e),
            Load::PerPhase(v) => v.iter().map(expr_text).collect::<Vec<_>>().join(", "),
        };
        ConnectorText {
            base: c.clone(),
            at: xyz(&c.at),
            direction: xyz(&c.direction),
            size: c.size.as_ref().map(expr_text).unwrap_or_default(),
            voltage: expr_text(&c.voltage),
            load,
            power_factor: expr_text(&c.power_factor),
        }
    }

    /// A new connector of `kind` for a family of `category`, placed at the family's origin with what its kind needs to be
    /// whole: a pipe's facing down, 15 mm, cold water (a fitting's or accessory's takes its pipe's); a device's balanced.
    pub(crate) fn blank(kind: ConnectorKind, category: Category) -> ConnectorText {
        let fitting = matches!(category, Category::PipeFitting | Category::PipeAccessory);
        let base = Connector {
            kind,
            system: match kind {
                ConnectorKind::Power => Some(ConnectorSystem::PowerBalanced),
                ConnectorKind::Switch => Some(ConnectorSystem::Switch),
                _ => None,
            },
            domain: (kind == ConnectorKind::Pipe && !fitting).then_some(Domain::Cold),
            at: Some([Expr::Number(0.0), Expr::Number(0.0), Expr::Number(0.0)]),
            direction: (kind == ConnectorKind::Pipe).then_some([Expr::Number(0.0), Expr::Number(0.0), Expr::Number(-1.0)]),
            size: (kind == ConnectorKind::Pipe).then_some(Expr::Number(15.0)),
            poles: 1,
            voltage: Expr::Number(if kind.electrical() { 220.0 } else { 0.0 }),
            power_factor: Expr::Number(1.0),
            ..connector_default()
        };
        ConnectorText::of(&base)
    }

    /// The connector as typed.
    pub(crate) fn connector(&self) -> Connector {
        let xyz = |p: &[String; 3]| (!p.iter().all(|s| s.trim().is_empty())).then(|| p.each_ref().map(|s| text_expr(s)));
        let electrical = self.base.kind.electrical();
        let load = if self.base.kind == ConnectorKind::Power {
            let parts = split_top(&self.load);
            if parts.len() > 1 { Load::PerPhase(parts.iter().map(|s| text_expr(s)).collect()) } else { Load::One(text_expr(&self.load)) }
        } else {
            Load::One(Expr::Number(0.0))
        };
        Connector {
            at: xyz(&self.at),
            direction: xyz(&self.direction),
            size: (!electrical && !self.size.trim().is_empty()).then(|| text_expr(&self.size)),
            voltage: text_expr(&self.voltage),
            load_va: load,
            power_factor: if self.power_factor.trim().is_empty() { Expr::Number(1.0) } else { text_expr(&self.power_factor) },
            ..self.base.clone()
        }
    }

    pub(crate) fn kind(&self) -> ConnectorKind {
        self.base.kind
    }
}

/// A connector made of nothing but its defaults, as a file without its fields reads one: one pole, a power factor of one.
fn connector_default() -> Connector {
    Connector {
        id: Default::default(),
        guid: Default::default(),
        name: String::new(),
        kind: ConnectorKind::Power,
        system: None,
        domain: None,
        flow: Flow::Both,
        at: None,
        direction: None,
        size: None,
        poles: 1,
        voltage: Expr::Number(0.0),
        load_va: Load::One(Expr::Number(0.0)),
        power_factor: Expr::Number(1.0),
        classification: String::new(),
    }
}

/// A word of the file's for an enum: `pipe`, `waste`, `power-balanced`.
fn word(v: Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_default()
}

/// A connector in a line, as the Project Browser lists it: what it is, what it carries, and where it is and faces in the
/// family's axes.
pub(crate) fn connector_text(c: &Connector) -> String {
    let xyz = |p: &[Expr; 3]| format!("({}, {}, {})", expr_text(&p[0]), expr_text(&p[1]), expr_text(&p[2]));
    let mut parts = vec![word(json!(c.kind))];
    if !c.name.is_empty() {
        parts.insert(0, c.name.clone());
    }
    if let Some(s) = c.system {
        parts.push(word(json!(s)));
    }
    if let Some(d) = c.domain {
        parts.push(word(json!(d)));
    }
    if c.flow != Flow::Both {
        parts.push(word(json!(c.flow)));
    }
    if let Some(e) = &c.size {
        parts.push(format!("{} {}", t("Size"), expr_text(e)));
    }
    if let Some(p) = &c.at {
        parts.push(format!("{} {}", t("at"), xyz(p)));
    }
    if let Some(p) = &c.direction {
        parts.push(format!("{} {}", t("facing"), xyz(p)));
    }
    parts.join(" · ")
}

/// What a connector's kind is called, as Properties' selector names it.
pub(crate) fn kind_name(kind: ConnectorKind) -> &'static str {
    KINDS.iter().find(|(k, ..)| *k == kind).map_or(msg!("Connector"), |(_, name, _)| name)
}

impl FamilyEditor {
    /// The connector selected, while it is there.
    pub(crate) fn selected_connector(&self) -> Option<usize> {
        self.connector.filter(|i| *i < self.connectors.len())
    }

    /// Connector `i` alone selected, as the Project Browser selects it: whatever else was selected let go.
    pub(crate) fn select_connector(&mut self, i: usize) {
        self.let_everything_go();
        self.connector = Some(i);
    }

    /// A connector of `kind` placed, as Create > Connectors places one, and selected so Properties show its rows.
    pub(crate) fn add_connector(&mut self, kind: ConnectorKind) {
        self.connectors.push(ConnectorText::blank(kind, self.category));
        self.select_connector(self.connectors.len() - 1);
    }

    /// Connector `i` taken away.
    pub(crate) fn remove_connector(&mut self, i: usize) {
        if i < self.connectors.len() {
            self.connectors.remove(i);
        }
        self.connector = self.connector.and_then(|c| match c.cmp(&i) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(c - 1),
            std::cmp::Ordering::Less => Some(c),
        });
    }

    /// Connector `i`'s name, as the Project Browser lists it and Properties' selector names it.
    pub(crate) fn connector_name(&self, i: usize) -> String {
        let line = self.connectors.get(i).map(|c| connector_text(&c.connector())).unwrap_or_default();
        format!("{}. {line}", i + 1)
    }
}

/// A drop-down of the file's words for `options`, named `label`: what was picked, if it was.
fn choose<T: Copy + PartialEq>(ui: &mut Ui, label: String, now: T, options: &[T], name: impl Fn(T) -> String) -> Option<T> {
    let mut picked = None;
    let c = ComboBox::from_id_salt(&label).selected_text(name(now)).width(150.0).show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(*o == now, name(*o)).clicked() {
                picked = Some(*o);
            }
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.as_str()));
    picked
}

/// Connector `i`'s rows in Properties: its name, what it carries, its place and, a device's, its electrical data.
pub(crate) fn connector_properties(ed: &mut FamilyEditor, ui: &mut Ui, i: usize) {
    let Some(c) = ed.connectors.get_mut(i) else { return };
    let n = i + 1;
    let w = (ui.available_width() / 2.0 - 16.0).max(60.0);
    let kind = c.base.kind;
    document::group(ui, msg!("Identity Data"), |ui| {
        ui.label(t("Name"));
        let r = ui.add_sized([w, ui.spacing().interact_size.y], TextEdit::singleline(&mut c.base.name).id_salt(("fe-connector-name", i)));
        r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{} {n}", t("Connector Name"))));
        ui.end_row();
    });
    // What it carries, as Revit's connector shows it: a pipe's Mechanical rows, another's its system.
    let carries = if kind == ConnectorKind::Pipe { msg!("Mechanical") } else { msg!("System") };
    document::group(ui, carries, |ui| {
        ui.label(t("Kind"));
        ui.label(t(kind_name(kind)));
        ui.end_row();
        match kind {
            ConnectorKind::Power => {
                ui.label(t("System"));
                let all = [ConnectorSystem::PowerBalanced, ConnectorSystem::PowerUnbalanced];
                if let Some(s) =
                    choose(ui, format!("{} {n}", t("Connector System")), c.base.system.unwrap_or_default(), &all, |s| word(json!(s)))
                {
                    c.base.system = Some(s);
                }
                ui.end_row();
            }
            ConnectorKind::Pipe => {
                ui.label(t("Domain"));
                // A fitting's or an accessory's may have none: it takes its pipe's.
                let all = [
                    None,
                    Some(Domain::Cold),
                    Some(Domain::Hot),
                    Some(Domain::Waste),
                    Some(Domain::Vent),
                    Some(Domain::Storm),
                    Some(Domain::Gas),
                ];
                let label = format!("{} {n}", t("Connector Domain"));
                let shown = c.base.domain.map_or_else(|| t("(none)").to_string(), |d| word(json!(d)));
                let r = ComboBox::from_id_salt(&label).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                    for d in all {
                        let name = d.map_or_else(|| t("(none)").to_string(), |d| word(json!(d)));
                        if ui.selectable_label(c.base.domain == d, name).clicked() {
                            c.base.domain = d;
                        }
                    }
                });
                r.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.as_str()));
                ui.end_row();
            }
            _ => {}
        }
        if !kind.electrical() {
            ui.label(t("Flow"));
            if let Some(f) =
                choose(ui, format!("{} {n}", t("Connector Flow")), c.base.flow, &[Flow::In, Flow::Out, Flow::Both], |f| word(json!(f)))
            {
                c.base.flow = f;
            }
            ui.end_row();
            ui.label(t("Size"));
            field(ui, format!("{} {n}", t("Connector Size")), &mut c.size, w);
            ui.end_row();
        }
    });
    // Its place and the way it faces, in the family's axes: x along it, y to its left, z up from its base.
    document::group(ui, msg!("Constraints"), |ui| {
        for (k, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            ui.label(format!("{} {axis}", t("Location")));
            field(ui, format!("{} {axis} {n}", t("Connector Location")), &mut c.at[k], w);
            ui.end_row();
        }
        for (k, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            ui.label(format!("{} {axis}", t("Direction")));
            field(ui, format!("{} {axis} {n}", t("Connector Direction")), &mut c.direction[k], w);
            ui.end_row();
        }
    });
    if kind.electrical() {
        document::group(ui, msg!("Electrical"), |ui| {
            ui.label(t("Number of Poles"));
            let mut poles = u32::from(c.base.poles);
            let r = ui.add(egui::DragValue::new(&mut poles).range(1..=3));
            r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, format!("{} {n}", t("Connector Poles"))));
            c.base.poles = u8::try_from(poles).unwrap_or(1);
            ui.end_row();
            ui.label(t("Voltage"));
            field(ui, format!("{} {n}", t("Connector Voltage")), &mut c.voltage, w);
            ui.end_row();
            if kind == ConnectorKind::Power {
                ui.label(t("Apparent Load (VA)"));
                field(ui, format!("{} {n}", t("Connector Load")), &mut c.load, w);
                ui.end_row();
                ui.label(t("Power Factor"));
                field(ui, format!("{} {n}", t("Connector Power Factor")), &mut c.power_factor, w);
                ui.end_row();
            }
            ui.label(t("Classification"));
            field(ui, format!("{} {n}", t("Connector Classification")), &mut c.base.classification, w);
            ui.end_row();
        });
    }
}
