//! The panel schedule: a distribution board's load schedule (quadro de cargas), a view of its own that goes on a sheet and prints
//! with it. It is not a [`crate::schedule::Schedule`]: it has sections (a header, the board's slots, a summary of the load by
//! classification and by phase, notes), and it is edited through the circuits it shows. [`layout`] works out everything it says as
//! data, which the sheet draws and the desktop shows; nothing here is saved but the options below.
//!
//! A board's slots are the numbers of its circuits ([`electrical::slots_of`]): a circuit of number `n` and `p` poles takes the slots
//! `n, n+2, …`, and each slot is on a phase ([`electrical::slot_phase`]). A slot nobody takes is a *space*; a circuit with no
//! device is a *spare*.

use bimcraft_i18n::msg;
use serde::{Deserialize, Serialize};

use crate::electrical::{self, phase_letter, slot_phase};
use crate::ids::{Guid, Id};
use crate::language::Language;
use crate::model::{Element, MAX_TEXT, Model};

/// The templates a panel schedule has: Revit's branch panel (the slots in two columns, odd on the left and even on the right, the
/// phases beside the circuits) and a list (a row for each slot, with the classification, voltage and demand factor).
pub const TEMPLATES: &[&str] = &["branch", "list"];
/// The longest notes a panel schedule holds, characters.
pub const MAX_NOTES: usize = 2_000;
/// How large a panel schedule may be drawn, as a multiple of its natural size.
pub const SCALE: std::ops::RangeInclusive<f64> = 0.25..=4.0;

fn branch() -> String {
    "branch".into()
}
fn yes() -> bool {
    true
}
fn one() -> f64 {
    1.0
}
fn is_branch(t: &String) -> bool {
    t == "branch"
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_one(x: &f64) -> bool {
    *x == 1.0
}

/// A panel schedule: the view of board `panel`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSchedule {
    pub name: String,
    /// The board, by GUID.
    pub panel: Guid,
    #[serde(default = "branch", skip_serializing_if = "is_branch")]
    pub template: String,
    /// Whether a spare (a breaker with no device) is in it.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub show_spare: bool,
    /// Whether a space (a slot no circuit takes) is in it.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub show_space: bool,
    /// Whether it has a column for each phase.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub phase_columns: bool,
    /// How large it comes out on paper: a multiple of its natural size.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl PanelSchedule {
    pub fn new(name: String, panel: Guid) -> PanelSchedule {
        PanelSchedule {
            name,
            panel,
            template: branch(),
            show_spare: true,
            show_space: true,
            phase_columns: true,
            scale: 1.0,
            notes: String::new(),
        }
    }
}

/// What the project must hold of a panel schedule.
pub fn check(m: &Model, who: &str, ps: &PanelSchedule) -> Result<(), String> {
    if ps.name.trim().is_empty() {
        return Err(format!("{who}: a view needs a name"));
    }
    if ps.name.chars().count() > MAX_TEXT {
        return Err(format!("{who}: its name is longer than {MAX_TEXT} characters"));
    }
    if !TEMPLATES.contains(&ps.template.as_str()) {
        return Err(format!("{who}: template `{}` isn't one of: {}", ps.template, TEMPLATES.join(", ")));
    }
    if !(ps.scale.is_finite() && SCALE.contains(&ps.scale)) {
        return Err(format!("{who}: `scale` is {} to {}, not {}", SCALE.start(), SCALE.end(), ps.scale));
    }
    if ps.notes.chars().count() > MAX_NOTES {
        return Err(format!("{who}: its notes are longer than {MAX_NOTES} characters"));
    }
    match m.elements.id(ps.panel) {
        Some(id) if electrical::board(m, id).is_some() => Ok(()),
        Some(id) => Err(format!("{who}: element {id} isn't a distribution board")),
        None => Err(format!("{who}: its board isn't an element of the project")),
    }
}

/// What a row of a panel schedule is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The first slot of a circuit with devices.
    Circuit,
    /// A later slot of a circuit of more poles.
    Continues,
    /// A breaker with no device on it (reserva).
    Spare,
    /// A slot no circuit takes (espaço).
    Space,
}

/// A slot's row: its cells, one for each of [`Layout::columns`].
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub slot: u32,
    pub kind: Kind,
    /// The circuit that has the slot; none for a space.
    pub circuit: Option<Id>,
    pub cells: Vec<String>,
}

/// A classification's loads in the summary.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassLoad {
    pub class: String,
    pub installed_va: f64,
    /// The factor of the project's table; 1 when it has none for the class.
    pub factor: f64,
    pub demand_va: f64,
}

/// A phase's loads in the summary: the board's phases add up to its total.
#[derive(Clone, Debug, PartialEq)]
pub struct PhaseLoad {
    pub phase: char,
    pub installed_va: f64,
    pub demand_va: f64,
    /// The demand over the phase voltage (line to neutral), amperes.
    pub current_a: Option<f64>,
}

/// The load summary, as numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub classes: Vec<ClassLoad>,
    pub phases: Vec<PhaseLoad>,
    pub installed_va: f64,
    pub demand_va: f64,
    /// The estimated current of the whole demand, amperes.
    pub current_a: Option<f64>,
}

/// Everything a panel schedule says.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub title: String,
    /// Label and value pairs: the board, its system, voltage, phases, mains and location.
    pub header: Vec<(String, String)>,
    /// The columns of a side of the slots' table.
    pub columns: Vec<String>,
    /// One side (a list) or two (the branch panel's odd and even slots).
    pub sides: usize,
    pub rows: Vec<Row>,
    /// The summary's tables, each with its title and rows (the headings first).
    pub summary_tables: Vec<(String, Vec<Vec<String>>)>,
    pub summary: Summary,
    pub notes: String,
}

fn thousandth(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0 + 0.0
}

/// A number the way a schedule says it: to a tenth, with no trailing zero.
pub fn number(x: f64) -> String {
    let r = (x * 10.0).round() / 10.0 + 0.0;
    if r == r.trunc() { format!("{r:.0}") } else { format!("{r:.1}") }
}

/// The voltage between a phase and the neutral in volts: the system's line-to-neutral one, else the line-to-line over √3 (or itself
/// for a single phase).
fn phase_voltage(m: &Model, board: Id) -> Option<f64> {
    let name = electrical::board(m, board)?.1.panel.as_ref()?.distribution.as_deref()?;
    let sys = m.distribution_systems.get(name)?;
    let nominal = |n: &Option<String>| n.as_ref().and_then(|n| m.voltages.get(n)).map(|v| v.nominal);
    match (nominal(&sys.l_n), nominal(&sys.l_l)) {
        (Some(v), _) => Some(v),
        (None, Some(v)) if sys.phases > 1 => Some(v / 3f64.sqrt()),
        (None, Some(v)) => Some(v),
        _ => nominal(&sys.l_g),
    }
}

/// The load summary of board `id`: by classification, by phase and all of it.
pub fn summary(m: &Model, id: Id) -> Summary {
    let mut classes: Vec<ClassLoad> = Vec::new();
    for cid in electrical::circuits_of(m, id) {
        let Some(Element::Circuit(c)) = m.elements.get(cid) else { continue };
        // A spare has no load to class.
        if c.devices.is_empty() {
            continue;
        }
        let Some(t) = electrical::circuit_totals(m, c) else { continue };
        let at = match classes.iter().position(|k| k.class == c.classification) {
            Some(at) => at,
            None => {
                classes.push(ClassLoad {
                    class: c.classification.clone(),
                    installed_va: 0.0,
                    factor: electrical::class_factor(m, &c.classification),
                    demand_va: 0.0,
                });
                classes.len() - 1
            }
        };
        classes[at].installed_va += t.installed_va;
        classes[at].demand_va += t.demand_va * classes[at].factor;
    }
    classes.sort_by(|a, b| a.class.cmp(&b.class));
    for k in &mut classes {
        k.installed_va = thousandth(k.installed_va);
        k.demand_va = thousandth(k.demand_va);
    }
    let volts = phase_voltage(m, id);
    let phases = electrical::phase_loads(m, id)
        .map(|l| {
            l.installed
                .iter()
                .zip(&l.demand)
                .enumerate()
                .map(|(i, (installed, demand))| PhaseLoad {
                    phase: phase_letter(u8::try_from(i).unwrap_or(0)),
                    installed_va: *installed,
                    demand_va: *demand,
                    current_a: volts.filter(|v| *v > 0.0).map(|v| thousandth(demand / v)),
                })
                .collect()
        })
        .unwrap_or_default();
    let totals = electrical::board_totals(m, id);
    Summary {
        classes,
        phases,
        installed_va: totals.as_ref().map_or(0.0, |t| t.installed_va),
        demand_va: totals.as_ref().map_or(0.0, |t| t.demand_va),
        current_a: totals.and_then(|t| t.current_a),
    }
}

/// The heading of each column of a side, and whether it is for a phase: the template's, with a column for each of `phases` when
/// the schedule has them.
fn columns(ps: &PanelSchedule, phases: u8, language: Language) -> Vec<String> {
    let w = |s: &str| language.word(s).to_string();
    let mut cols = if ps.template == "list" {
        vec![
            w(msg!("Slot")),
            w(msg!("Description")),
            w(msg!("Classification")),
            w(msg!("Poles")),
            w(msg!("Voltage")),
            w(msg!("Trip")),
            w(msg!("VA")),
            w(msg!("Demand Factor")),
        ]
    } else {
        vec![w(msg!("Slot")), w(msg!("Description")), w(msg!("Classification")), w(msg!("Trip")), w(msg!("Poles"))]
    };
    if ps.phase_columns {
        cols.extend((0..phases).map(|p| phase_letter(p).to_string()));
    }
    cols
}

/// Panel schedule `ps` worked out: its header, the rows of its slots, its summary and its notes.
pub fn layout(m: &Model, ps: &PanelSchedule) -> Result<Layout, String> {
    check(m, "panel schedule", ps)?;
    let id = m.elements.id(ps.panel).ok_or("its board isn't an element of the project")?;
    let (_, comp) = electrical::board(m, id).ok_or("its board isn't a distribution board")?;
    let lang = m.language;
    let w = |s: &str| lang.word(s).to_string();
    let data = comp.panel.clone().unwrap_or_default();
    let system = data.distribution.as_ref().and_then(|n| m.distribution_systems.get(n));
    let phases = system.map_or(0, |s| s.phases);
    let columns = columns(ps, phases, lang);

    // Which circuit has each slot, and in which pole of it.
    let by_slot = electrical::occupied(m, id);
    let count = electrical::slot_count(m, id);
    let mut rows = Vec::new();
    for slot in 1..=count {
        let Some(&cid) = by_slot.get(&slot) else {
            if ps.show_space {
                let mut cells = vec![String::new(); columns.len()];
                cells[0] = slot.to_string();
                cells[1] = w(msg!("Space"));
                rows.push(Row { slot, kind: Kind::Space, circuit: None, cells });
            }
            continue;
        };
        let Some(Element::Circuit(c)) = m.elements.get(cid) else { continue };
        let totals = electrical::circuit_totals(m, c);
        let first = c.number == slot;
        let pole = electrical::slots_of(c.number, c.poles).position(|s| s == slot).unwrap_or(0);
        let spare = c.devices.is_empty();
        if spare && !ps.show_spare {
            continue;
        }
        let kind = if !first {
            Kind::Continues
        } else if spare {
            Kind::Spare
        } else {
            Kind::Circuit
        };
        let pole_va = totals.as_ref().and_then(|t| t.pole_va.get(pole).copied()).unwrap_or(0.0);
        let volts = electrical::circuit_voltage(m, c).map(|(_, v)| number(v.nominal)).unwrap_or_default();
        let (name, class) = if first {
            (if spare && c.name.is_empty() { w(msg!("Spare")) } else { c.name.clone() }, c.classification.clone())
        } else {
            Default::default()
        };
        let mut cells = vec![slot.to_string(), name, class];
        if ps.template == "list" {
            cells.push(if first { c.poles.to_string() } else { String::new() });
            cells.push(if first { volts } else { String::new() });
            cells.push(if first { number(c.rating_a) } else { String::new() });
            cells.push(if first { totals.as_ref().map_or_else(String::new, |t| number(t.installed_va)) } else { String::new() });
            cells.push(if first { number(c.demand_factor) } else { String::new() });
        } else {
            cells.push(if first { number(c.rating_a) } else { String::new() });
            cells.push(if first { c.poles.to_string() } else { String::new() });
        }
        if ps.phase_columns {
            let on = slot_phase(slot, phases);
            cells.extend((0..phases).map(|p| if p == on && pole_va > 0.0 { number(pole_va) } else { String::new() }));
        }
        rows.push(Row { slot, kind, circuit: Some(cid), cells });
    }

    let header = vec![
        (w(msg!("Panel")), comp.type_name.clone()),
        (w(msg!("Distribution System")), data.distribution.clone().unwrap_or_default()),
        (
            w(msg!("Voltage")),
            system
                .map(|s| {
                    [&s.l_l, &s.l_n]
                        .into_iter()
                        .filter_map(|n| n.as_ref().and_then(|n| m.voltages.get(n)))
                        .map(|v| number(v.nominal))
                        .collect::<Vec<_>>()
                        .join("/")
                        + " V"
                })
                .unwrap_or_default(),
        ),
        (w(msg!("Phases")), if phases > 0 { phases.to_string() } else { String::new() }),
        (w(msg!("Mains")), data.mains.map_or_else(String::new, |a| format!("{} A", number(a)))),
        (
            w(msg!("Location")),
            match m.elements.get(id).and_then(|e| m.level_of(e)) {
                Some(g) => m.elements.id(g).and_then(|l| m.view_name(l)).unwrap_or_default().to_string(),
                None => String::new(),
            },
        ),
    ];

    let s = summary(m, id);
    let mut class_rows =
        vec![vec![w(msg!("Classification")), w(msg!("Connected Load (VA)")), w(msg!("Demand Factor")), w(msg!("Estimated Demand (VA)"))]];
    for k in &s.classes {
        class_rows.push(vec![
            if k.class.is_empty() { w(msg!("Unclassified")) } else { k.class.clone() },
            number(k.installed_va),
            number(k.factor),
            number(k.demand_va),
        ]);
    }
    class_rows.push(vec![w("Total"), number(s.installed_va), String::new(), number(s.demand_va)]);
    let mut phase_rows =
        vec![vec![w(msg!("Phase")), w(msg!("Connected Load (VA)")), w(msg!("Estimated Demand (VA)")), w(msg!("Estimated Current (A)"))]];
    for p in &s.phases {
        phase_rows.push(vec![
            p.phase.to_string(),
            number(p.installed_va),
            number(p.demand_va),
            p.current_a.map_or_else(String::new, number),
        ]);
    }
    phase_rows.push(vec![w("Total"), number(s.installed_va), number(s.demand_va), s.current_a.map_or_else(String::new, number)]);
    Ok(Layout {
        title: ps.name.clone(),
        header,
        columns,
        sides: if ps.template == "list" { 1 } else { 2 },
        rows,
        summary_tables: vec![(w(msg!("Load Summary by Classification")), class_rows), (w(msg!("Load Summary by Phase")), phase_rows)],
        summary: s,
        notes: ps.notes.clone(),
    })
}

/// The panel schedules of board `board`, by id.
pub fn of_board(m: &Model, board: Id) -> Vec<Id> {
    let g = m.guid(board);
    m.elements.iter().filter(|(_, e)| matches!(e, Element::PanelSchedule(p) if p.panel == g)).map(|(i, _)| *i).collect()
}
