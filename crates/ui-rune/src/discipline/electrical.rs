//! Electrical Settings, as Revit's: the project's voltages, distribution systems and wire types, each a table with rows to add and take away,
//! edited as a draft and applied as one undo step through the `voltage`, `distribution_system` and `wire_type` commands (OK and Apply), or dropped
//! (Cancel). egui's `electrical.rs` is the model. Sections are mm², always, as the core keeps them. The numbers are typed in boxes, so a number
//! that is none is refused naming its row; the core's own limits are its words.

use std::collections::BTreeSet;

use bimcraft_core::electrical::{EARTHINGS, MATERIALS, MAX_SIZES, MAX_SYSTEMS, MAX_VOLTAGES, MAX_WIRE_TYPES, Voltage, WireSize, WireType};
use bimcraft_core::model::Model;
use rune::TextEdit;
use serde_json::{Value, json};

use super::{Cell, Choice, Discipline, Foot, Host, Outcome, Section, edits, typed};
use crate::lang::{t, tf};
use crate::renames;

/// What a list says for "no voltage" or "no earthing": the line to leave empty.
const NONE: &str = "\u{2014}";

/// The places of the edits and the lists: a table, a row of it and a column, as one number (`at`).
const TABLE: usize = 100_000;

fn at(table: usize, row: usize, col: usize) -> usize {
    table * TABLE + row * 8 + col
}

/// Which table, row and column a place is.
fn split(i: usize) -> (usize, usize, usize) {
    (i / TABLE, (i % TABLE) / 8, i % 8)
}

/// The list of wire types, and the list of a wire type's material.
const WIRE_LIST: usize = 2 * TABLE + 7;
const MATERIAL_LIST: usize = 2 * TABLE + 6;

/// A voltage as typed: its name, nominal, minimum and maximum; and what it was when the dialog opened (none, a new one).
struct VoltageRow {
    /// What other rows name it by, which its name's being typed over does not change.
    key: u64,
    was: Option<(String, Voltage)>,
    /// The band a new or untouched row starts with, which follows the nominal while it stays.
    start: Voltage,
    edits: Vec<TextEdit>,
}

/// A distribution system as the dialog holds it: the voltages it supplies are rows of the Voltages table, by their keys, so that a voltage
/// typed over is still the one it named.
#[derive(Clone, Debug, PartialEq)]
struct Supply {
    phases: u8,
    wires: u8,
    l_l: Option<u64>,
    l_n: Option<u64>,
    l_g: Option<u64>,
    ground: Option<String>,
}

struct SystemRow {
    key: u64,
    was: Option<(String, Supply)>,
    name: TextEdit,
    supply: Supply,
}

/// A wire type: name, temperature, insulation and note typed, its material chosen, and a table of sizes (section, ampacity, label).
struct WireRow {
    key: u64,
    was: Option<(String, WireType)>,
    edits: Vec<TextEdit>,
    material: String,
    sizes: Vec<Vec<TextEdit>>,
}

/// The Electrical Settings: the three tables as edited and not yet applied.
pub struct Electrical {
    tab: usize,
    voltages: Vec<VoltageRow>,
    systems: Vec<SystemRow>,
    wires: Vec<WireRow>,
    /// The names of the voltages, systems and wire types taken away.
    gone: [Vec<String>; 3],
    /// The wire type shown on its table.
    wire: usize,
    next: u64,
    error: Option<String>,
}

/// A number as the box holds it: the shortest text that reads back as the same number, so a row left alone is a row unchanged.
fn shown(x: f64) -> String {
    format!("{x}")
}

/// A name for a new row that no row of `names` has: `base`, `base 2`...
fn spare(names: &[String], base: &str) -> String {
    std::iter::once(base.to_string()).chain((2..).map(|i| format!("{base} {i}"))).find(|n| !names.contains(n)).unwrap_or_default()
}

fn voltage_row(key: u64, was: Option<(String, Voltage)>, name: &str, v: &Voltage) -> VoltageRow {
    let edits = edits([name.to_string(), shown(v.nominal), shown(v.min), shown(v.max)]);
    VoltageRow { key, was, start: v.clone(), edits }
}

fn size_row(s: &WireSize) -> Vec<TextEdit> {
    edits([shown(s.section), shown(s.ampacity), s.label.clone().unwrap_or_default()])
}

fn wire_row(key: u64, was: Option<(String, WireType)>, name: &str, w: &WireType) -> WireRow {
    WireRow {
        key,
        was,
        edits: edits([name.to_string(), shown(w.temperature), w.insulation.clone(), w.note.clone()]),
        material: w.material.clone(),
        sizes: w.sizes.iter().map(size_row).collect(),
    }
}

fn number(text: &str, what: &str, row: &str) -> Result<f64, String> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
        .ok_or_else(|| tf("{}: {} is not a number: `{}`", &[row, what, text.trim()]))
}

impl Electrical {
    /// Open on the project's electrical settings.
    pub fn open(m: &Model) -> Electrical {
        let mut d = Electrical {
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
            d.voltages.push(voltage_row(key, Some((n.clone(), v.clone())), n, v));
        }
        for (n, s) in &m.distribution_systems {
            let key = d.key();
            let by =
                |name: &Option<String>| name.as_deref().and_then(|n| d.voltages.iter().find(|r| typed(&r.edits[0]) == n)).map(|r| r.key);
            let supply =
                Supply { phases: s.phases, wires: s.wires, l_l: by(&s.l_l), l_n: by(&s.l_n), l_g: by(&s.l_g), ground: s.ground.clone() };
            d.systems.push(SystemRow { key, was: Some((n.clone(), supply.clone())), name: TextEdit::new(n.clone()), supply });
        }
        for (n, w) in &m.wire_types {
            let key = d.key();
            d.wires.push(wire_row(key, Some((n.clone(), w.clone())), n, w));
        }
        d
    }

    fn key(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    fn voltage_names(&self) -> Vec<String> {
        self.voltages.iter().map(|r| typed(&r.edits[0]).trim().to_string()).collect()
    }

    fn add_voltage(&mut self) {
        let name = spare(&self.voltage_names(), t("New Voltage"));
        let key = self.key();
        self.voltages.push(voltage_row(key, None, &name, &Voltage::around(230.0)));
    }

    /// Takes voltage row `i` away, unless a distribution system supplies it.
    fn delete_voltage(&mut self, i: usize) {
        let Some(r) = self.voltages.get(i) else { return };
        if let Some(s) = self.systems.iter().find(|s| [s.supply.l_l, s.supply.l_n, s.supply.l_g].contains(&Some(r.key))) {
            self.error = Some(tf(
                "Voltage {} is supplied by distribution system {}: change that first.",
                &[typed(&r.edits[0]).trim(), typed(&s.name).trim()],
            ));
            return;
        }
        let r = self.voltages.remove(i);
        if let Some((was, _)) = r.was {
            self.gone[0].push(was);
        }
    }

    fn add_system(&mut self) {
        let names: Vec<String> = self.systems.iter().map(|r| typed(&r.name).trim().to_string()).collect();
        let name = spare(&names, t("New Distribution System"));
        let l_l = self.voltages.first().map(|r| r.key);
        let key = self.key();
        let supply = Supply { phases: 3, wires: 4, l_l, l_n: None, l_g: None, ground: None };
        self.systems.push(SystemRow { key, was: None, name: TextEdit::new(name), supply });
    }

    fn delete_system(&mut self, i: usize) {
        if i < self.systems.len()
            && let Some((was, _)) = self.systems.remove(i).was
        {
            self.gone[1].push(was);
        }
    }

    fn add_wire(&mut self) {
        let names: Vec<String> = self.wires.iter().map(|r| typed(&r.edits[0]).trim().to_string()).collect();
        let name = spare(&names, t("New Wire Type"));
        let key = self.key();
        let value = WireType {
            material: MATERIALS[0].into(),
            temperature: 70.0,
            insulation: "PVC".into(),
            note: String::new(),
            sizes: vec![WireSize { section: 1.5, ampacity: 15.0, label: None }],
        };
        self.wires.push(wire_row(key, None, &name, &value));
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

    /// One more size than the last: a larger section, carrying at least as much.
    fn add_size(&mut self) {
        let Some(r) = self.wires.get_mut(self.wire) else { return };
        let last = r.sizes.last().map(|s| (number(&typed(&s[0]), "", "").unwrap_or(1.0), number(&typed(&s[1]), "", "").unwrap_or(15.0)));
        let next = last.map_or(WireSize { section: 1.5, ampacity: 15.0, label: None }, |(section, ampacity)| WireSize {
            section: (section * 1.5 * 10.0).round() / 10.0,
            ampacity,
            label: None,
        });
        r.sizes.push(size_row(&next));
    }

    /// The draft is something the core may be asked for: every row has a name, and no two of a table share one.
    fn check(&self) -> Result<(), String> {
        fn names(one: &str, several: &str, names: Vec<String>) -> Result<(), String> {
            let mut seen = BTreeSet::new();
            for n in names {
                let n = n.trim().to_string();
                if n.is_empty() {
                    return Err(tf("A {} needs a name.", &[t(one)]));
                }
                if !seen.insert(n.clone()) {
                    return Err(tf("Two {} are called {}.", &[t(several), &n]));
                }
            }
            Ok(())
        }
        names("voltage", "voltages", self.voltage_names())?;
        names("distribution system", "distribution systems", self.systems.iter().map(|r| typed(&r.name)).collect())?;
        names("wire type", "wire types", self.wires.iter().map(|r| typed(&r.edits[0])).collect())
    }

    /// Voltage row `i` as typed. A band still the one a new or untouched row started with follows the nominal, so a nominal typed alone is one
    /// the project takes.
    fn voltage_of(&self, i: usize) -> Result<Voltage, String> {
        let r = &self.voltages[i];
        let row = tf("Voltage {}", &[&(i + 1).to_string()]);
        let nominal = number(&typed(&r.edits[1]), t("Nominal"), &row)?;
        let (min, max) = (number(&typed(&r.edits[2]), t("Minimum"), &row)?, number(&typed(&r.edits[3]), t("Maximum"), &row)?);
        let untouched =
            number(&typed(&r.edits[2]), "", "").ok() == Some(r.start.min) && number(&typed(&r.edits[3]), "", "").ok() == Some(r.start.max);
        let was_around = r.start == Voltage::around(r.start.nominal);
        Ok(if untouched && was_around && nominal != r.start.nominal { Voltage::around(nominal) } else { Voltage { nominal, min, max } })
    }

    /// The name of the voltage row `key`, as the core will have it by then.
    fn voltage_name(&self, key: Option<u64>) -> String {
        key.and_then(|k| self.voltages.iter().find(|r| r.key == k)).map(|r| typed(&r.edits[0]).trim().to_string()).unwrap_or_default()
    }

    fn system_params(&self, name: &str, s: &Supply) -> Value {
        json!({
            "name": name, "phases": s.phases, "wires": s.wires,
            "l_l": self.voltage_name(s.l_l), "l_n": self.voltage_name(s.l_n), "l_g": self.voltage_name(s.l_g),
            "ground": s.ground.clone().unwrap_or_default(),
        })
    }

    /// Wire type row `i` as typed.
    fn wire_of(&self, i: usize) -> Result<WireType, String> {
        let r = &self.wires[i];
        let row = tf("Wire type {}", &[typed(&r.edits[0]).trim()]);
        let mut sizes = Vec::new();
        for (n, s) in r.sizes.iter().enumerate() {
            let at = tf("{}: size {}", &[&row, &(n + 1).to_string()]);
            let label = typed(&s[2]).trim().to_string();
            sizes.push(WireSize {
                section: number(&typed(&s[0]), t("Cross-Section"), &at)?,
                ampacity: number(&typed(&s[1]), t("Ampacity"), &at)?,
                label: Some(label).filter(|l| !l.is_empty()),
            });
        }
        Ok(WireType {
            material: r.material.clone(),
            temperature: number(&typed(&r.edits[1]), t("Temperature"), &row)?,
            insulation: typed(&r.edits[2]),
            note: typed(&r.edits[3]),
            sizes,
        })
    }

    /// What the draft asks of the project, in an order the core can do it in: the voltages made, renamed and changed before the systems that
    /// name them, and the ones taken away after the systems that no longer do; the renames of each table by `renames::plan`, which gets
    /// chains, swaps and names freed right.
    pub fn commands(&self, m: &Model) -> Result<Vec<(&'static str, Value)>, String> {
        self.check()?;
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        let plan = |existing: BTreeSet<String>, rows: Vec<renames::Row>| renames::plan(&existing, &rows);
        fn rows<'a>(live: Vec<(Option<&'a str>, String)>, gone: &'a [String]) -> Vec<(Option<&'a str>, String, bool)> {
            live.into_iter().map(|(was, name)| (was, name, false)).chain(gone.iter().map(|n| (Some(n.as_str()), n.clone(), true))).collect()
        }
        fn draft<'a>(rows: &'a [(Option<&'a str>, String, bool)]) -> Vec<renames::Row<'a>> {
            rows.iter().map(|(was, name, gone)| renames::Row { was: *was, name: name.trim(), gone: *gone }).collect()
        }
        let renamed = |command: &'static str, out: &mut Vec<(&'static str, Value)>, renames: &[(String, String)]| {
            out.extend(renames.iter().map(|(from, to)| (command, json!({"name": from, "rename": to}))));
        };
        let deleted = |command: &'static str, out: &mut Vec<(&'static str, Value)>, names: &[String]| {
            out.extend(names.iter().map(|n| (command, json!({"name": n, "delete": true}))));
        };

        let vrows =
            rows(self.voltages.iter().map(|r| (r.was.as_ref().map(|(n, _)| n.as_str()), typed(&r.edits[0]))).collect(), &self.gone[0]);
        let v = plan(m.voltages.keys().cloned().collect(), draft(&vrows));
        renamed("voltage", &mut out, &v.renames);
        for (i, r) in self.voltages.iter().enumerate() {
            let value = self.voltage_of(i)?;
            if r.was.as_ref().is_none_or(|(_, was)| *was != value) {
                out.push((
                    "voltage",
                    json!({"name": typed(&r.edits[0]).trim(), "nominal": value.nominal, "min": value.min, "max": value.max}),
                ));
            }
        }
        let srows = rows(self.systems.iter().map(|r| (r.was.as_ref().map(|(n, _)| n.as_str()), typed(&r.name))).collect(), &self.gone[1]);
        let s = plan(m.distribution_systems.keys().cloned().collect(), draft(&srows));
        renamed("distribution_system", &mut out, &s.renames);
        // (A voltage renamed is renamed in the systems that name it by the core, so a system whose draft is as it was needs nothing.)
        for r in &self.systems {
            if r.was.as_ref().is_none_or(|(_, was)| *was != r.supply) {
                out.push(("distribution_system", self.system_params(typed(&r.name).trim(), &r.supply)));
            }
        }
        deleted("distribution_system", &mut out, &s.deletes);
        deleted("voltage", &mut out, &v.deletes);
        let wrows = rows(self.wires.iter().map(|r| (r.was.as_ref().map(|(n, _)| n.as_str()), typed(&r.edits[0]))).collect(), &self.gone[2]);
        let w = plan(m.wire_types.keys().cloned().collect(), draft(&wrows));
        renamed("wire_type", &mut out, &w.renames);
        for (i, r) in self.wires.iter().enumerate() {
            let x = self.wire_of(i)?;
            if r.was.as_ref().is_none_or(|(_, was)| *was != x) {
                out.push((
                    "wire_type",
                    json!({"name": typed(&r.edits[0]).trim(), "material": x.material, "temperature": x.temperature, "insulation": x.insulation, "note": x.note, "sizes": x.sizes}),
                ));
            }
        }
        deleted("wire_type", &mut out, &w.deletes);
        Ok(out)
    }

    /// OK and Apply: what changed, as one undo step, all or nothing; a refusal (a name blank or used twice, a number that is none, the core's
    /// reasons) keeps the dialog open to fix it. Whether it was done.
    fn apply(&mut self, apply: bool, host: &mut dyn Host) -> bool {
        self.error = None;
        let done = self.commands(host.model()).and_then(|commands| {
            if commands.is_empty() {
                return Ok(());
            }
            host.batch("Electrical Settings", &commands).map(|_| ())
        });
        if let Err(why) = done {
            self.error = Some(why);
            return false;
        }
        if apply {
            let (tab, wire) = (self.tab, self.wire);
            *self = Electrical::open(host.model());
            (self.tab, self.wire) = (tab, wire.min(self.wires.len().saturating_sub(1)));
        }
        true
    }

    /// The header row of a table: its columns' names, each as wide as the cells under it.
    fn heads(heads: &[(&str, f32)]) -> Vec<Cell> {
        heads.iter().map(|(h, w)| if h.is_empty() { Cell::text("", *w) } else { Cell::head(t(h), *w) }).collect()
    }

    fn voltages_table(&self) -> Vec<Section> {
        let mut rows = vec![Self::heads(&[("Name", 180.0), ("Nominal", 90.0), ("Minimum", 90.0), ("Maximum", 90.0), ("", 70.0)])];
        for (i, _) in self.voltages.iter().enumerate() {
            let n = (i + 1).to_string();
            let name = |what: &str| tf("Voltage {} {}", &[&n, what]);
            rows.push(vec![
                Cell::field(at(0, i, 0), tf("Voltage {} name", &[&n]), "", 180.0),
                Cell::field(at(0, i, 1), name("nominal"), "V", 90.0),
                Cell::field(at(0, i, 2), name("minimum"), "V", 90.0),
                Cell::field(at(0, i, 3), name("maximum"), "V", 90.0),
                Cell::button("delete_voltage", i, t("Delete"), true),
            ]);
        }
        rows.push(vec![Cell::button("add_voltage", 0, t("Add Voltage"), self.voltages.len() < MAX_VOLTAGES)]);
        vec![Section::new(
            t("The voltages of the project, in volts, and the band a device may read within. A distribution system names them."),
            rows,
        )]
    }

    fn systems_table(&self) -> Vec<Section> {
        let heads = [
            ("Name", 150.0),
            ("Phases", 60.0),
            ("Wires", 60.0),
            ("Line to line", 120.0),
            ("Line to neutral", 120.0),
            ("Line to earth", 120.0),
            ("Earthing", 80.0),
            ("", 70.0),
        ];
        let mut rows = vec![Self::heads(&heads)];
        for (i, _) in self.systems.iter().enumerate() {
            let n = (i + 1).to_string();
            rows.push(vec![
                Cell::field(at(1, i, 0), tf("System {} name", &[&n]), "", 150.0),
                Cell::Pick { pick: at(1, i, 1), width: 60.0 },
                Cell::Pick { pick: at(1, i, 2), width: 60.0 },
                Cell::Pick { pick: at(1, i, 3), width: 120.0 },
                Cell::Pick { pick: at(1, i, 4), width: 120.0 },
                Cell::Pick { pick: at(1, i, 5), width: 120.0 },
                Cell::Pick { pick: at(1, i, 6), width: 80.0 },
                Cell::button("delete_system", i, t("Delete"), true),
            ]);
        }
        rows.push(vec![Cell::button("add_system", 0, t("Add Distribution System"), self.systems.len() < MAX_SYSTEMS)]);
        vec![Section::new(
            t(
                "How many phases and wires each system has (the line conductors and the neutral), the voltages it supplies, and how it is earthed.",
            ),
            rows,
        )]
    }

    fn wires_table(&self) -> Vec<Section> {
        let mut out = vec![Section::new(
            t("A conductor's material, insulation and working temperature, and the ampacity of each section, in mm²."),
            vec![vec![
                Cell::text(t("Wire type:"), 80.0),
                Cell::Pick { pick: WIRE_LIST, width: 220.0 },
                Cell::button("add_wire", 0, t("Add Wire Type"), self.wires.len() < MAX_WIRE_TYPES),
                Cell::button("delete_wire", 0, t("Delete Wire Type"), !self.wires.is_empty()),
            ]],
        )];
        let Some(r) = self.wires.get(self.wire) else { return out };
        let label = |text: &str| Cell::text(t(text), 100.0);
        out.push(Section::new(
            "",
            vec![
                vec![label("Name:"), Cell::field(at(2, 0, 0), t("Wire type name"), "", 260.0)],
                vec![label("Material:"), Cell::Pick { pick: MATERIAL_LIST, width: 140.0 }],
                vec![label("Temperature:"), Cell::field(at(2, 0, 1), t("Wire temperature"), "°C", 90.0)],
                vec![label("Insulation:"), Cell::field(at(2, 0, 2), t("Wire insulation"), "", 260.0)],
                vec![label("Note:"), Cell::field(at(2, 0, 3), t("Wire note"), "", 520.0)],
            ],
        ));
        let mut rows = vec![Self::heads(&[("Cross-Section", 110.0), ("Ampacity", 110.0), ("Label", 140.0), ("", 70.0)])];
        for (i, _) in r.sizes.iter().enumerate() {
            let n = (i + 1).to_string();
            rows.push(vec![
                Cell::field(at(2, i + 1, 0), tf("Size {} section", &[&n]), "mm²", 110.0),
                Cell::field(at(2, i + 1, 1), tf("Size {} ampacity", &[&n]), "A", 110.0),
                Cell::field(at(2, i + 1, 2), tf("Size {} label", &[&n]), "", 140.0),
                Cell::button("delete_size", i, t("Delete"), true),
            ]);
        }
        rows.push(vec![Cell::button("add_size", 0, t("Add Size"), r.sizes.len() < MAX_SIZES)]);
        out.push(Section::new("", rows));
        out
    }
}

impl Discipline for Electrical {
    fn title(&self) -> String {
        t("Electrical Settings").to_string()
    }

    fn width(&self) -> f32 {
        960.0
    }

    fn tabs(&self) -> Vec<String> {
        ["Voltages", "Distribution Systems", "Wire Types"].iter().map(|s| t(s).to_string()).collect()
    }

    fn tab(&self) -> usize {
        self.tab
    }

    fn sections(&self) -> Vec<Section> {
        match self.tab {
            0 => self.voltages_table(),
            1 => self.systems_table(),
            _ => self.wires_table(),
        }
    }

    fn edit(&self, i: usize) -> Option<&TextEdit> {
        match split(i) {
            (0, row, col) => self.voltages.get(row)?.edits.get(col),
            (1, row, 0) => self.systems.get(row).map(|r| &r.name),
            (2, 0, col) => self.wires.get(self.wire)?.edits.get(col),
            (2, row, col) => self.wires.get(self.wire)?.sizes.get(row - 1)?.get(col),
            _ => None,
        }
    }

    fn edit_mut(&mut self, i: usize) -> Option<&mut TextEdit> {
        match split(i) {
            (0, row, col) => self.voltages.get_mut(row)?.edits.get_mut(col),
            (1, row, 0) => self.systems.get_mut(row).map(|r| &mut r.name),
            (2, 0, col) => self.wires.get_mut(self.wire)?.edits.get_mut(col),
            (2, row, col) => self.wires.get_mut(self.wire)?.sizes.get_mut(row - 1)?.get_mut(col),
            _ => None,
        }
    }

    fn choice(&self, pick: usize) -> Option<Choice> {
        let (table, row, col) = split(pick);
        let voltages = |now: Option<u64>| {
            let names = self.voltage_names();
            let options: Vec<String> = std::iter::once(NONE.to_string()).chain(names).collect();
            let current = now.and_then(|k| self.voltages.iter().position(|r| r.key == k)).map_or(0, |p| p + 1);
            (options, Some(current))
        };
        match (table, col) {
            _ if pick == WIRE_LIST => {
                let names: Vec<String> = self.wires.iter().map(|r| typed(&r.edits[0]).trim().to_string()).collect();
                Some(Choice {
                    name: t("Wire type:").to_string(),
                    options: names,
                    current: Some(self.wire).filter(|w| *w < self.wires.len()),
                })
            }
            _ if pick == MATERIAL_LIST => {
                let now = self.wires.get(self.wire)?.material.as_str();
                let options = MATERIALS.iter().map(|m| t(m).to_string()).collect();
                Some(Choice { name: t("Wire material").to_string(), options, current: MATERIALS.iter().position(|m| *m == now) })
            }
            (1, 1) => Some(Choice {
                name: tf("System {} phases", &[&(row + 1).to_string()]),
                options: ["1", "2", "3"].map(String::from).to_vec(),
                current: self.systems.get(row).and_then(|r| usize::from(r.supply.phases).checked_sub(1)),
            }),
            (1, 2) => Some(Choice {
                name: tf("System {} wires", &[&(row + 1).to_string()]),
                options: ["2", "3", "4"].map(String::from).to_vec(),
                current: self.systems.get(row).and_then(|r| usize::from(r.supply.wires).checked_sub(2)),
            }),
            (1, 3..=5) => {
                let r = self.systems.get(row)?;
                let (what, now) = match col {
                    3 => ("line to line", r.supply.l_l),
                    4 => ("line to neutral", r.supply.l_n),
                    _ => ("line to earth", r.supply.l_g),
                };
                let (options, current) = voltages(now);
                Some(Choice { name: tf("System {} {}", &[&(row + 1).to_string(), what]), options, current })
            }
            (1, 6) => {
                let r = self.systems.get(row)?;
                let options: Vec<String> = std::iter::once(NONE.to_string()).chain(EARTHINGS.iter().map(|g| g.to_string())).collect();
                let current = r.supply.ground.as_ref().and_then(|g| EARTHINGS.iter().position(|e| e == g)).map_or(0, |p| p + 1);
                Some(Choice { name: tf("System {} earthing", &[&(row + 1).to_string()]), options, current: Some(current) })
            }
            _ => None,
        }
    }

    fn choose(&mut self, pick: usize, option: usize, _host: &mut dyn Host) {
        let (table, row, col) = split(pick);
        if pick == WIRE_LIST {
            self.wire = option.min(self.wires.len().saturating_sub(1));
        } else if pick == MATERIAL_LIST {
            if let (Some(r), Some(m)) = (self.wires.get_mut(self.wire), MATERIALS.get(option)) {
                r.material = m.to_string();
            }
        } else if table == 1 {
            let keys: Vec<u64> = self.voltages.iter().map(|r| r.key).collect();
            let key = option.checked_sub(1).and_then(|p| keys.get(p)).copied();
            let Some(r) = self.systems.get_mut(row) else { return };
            match col {
                1 => r.supply.phases = u8::try_from(option + 1).unwrap_or(3),
                2 => r.supply.wires = u8::try_from(option + 2).unwrap_or(4),
                3 => r.supply.l_l = key,
                4 => r.supply.l_n = key,
                5 => r.supply.l_g = key,
                6 => r.supply.ground = option.checked_sub(1).and_then(|p| EARTHINGS.get(p)).map(|g| g.to_string()),
                _ => {}
            }
        }
    }

    fn press(&mut self, action: &str, arg: usize, host: &mut dyn Host) -> Outcome {
        match action {
            "tab" => {
                self.tab = arg.min(2);
                self.error = None;
            }
            "add_voltage" => self.add_voltage(),
            "delete_voltage" => {
                self.error = None;
                self.delete_voltage(arg);
            }
            "add_system" => self.add_system(),
            "delete_system" => self.delete_system(arg),
            "add_wire" => self.add_wire(),
            "delete_wire" => self.delete_wire(),
            "add_size" => self.add_size(),
            "delete_size" => {
                if let Some(r) = self.wires.get_mut(self.wire).filter(|r| arg < r.sizes.len()) {
                    r.sizes.remove(arg);
                }
            }
            "apply" => {
                self.apply(true, host);
            }
            "ok" => {
                if self.apply(false, host) {
                    return Outcome::Close;
                }
            }
            "cancel" => return Outcome::Close,
            _ => {}
        }
        Outcome::Keep
    }

    fn foot(&self) -> Vec<Foot> {
        vec![Foot::new("cancel", t("Cancel"), false), Foot::new("apply", t("Apply"), false), Foot::new("ok", t("OK"), true)]
    }

    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests;
