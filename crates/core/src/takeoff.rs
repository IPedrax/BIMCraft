//! Material takeoff: what the layers of the project's walls, floors, roofs and ceilings come to in each
//! material — its area, its volume and its mass, and that with waste — as a schedule of the project does
//! (`takeoff`). The area is the element's net surface area from `Model::areas` (openings and shafts taken
//! out, a slope measured along it, as TZ-031 has it), never its plan area: a takeoff counts a layer twice
//! when a type has it twice (OSB on both faces), and nothing is deducted a second time.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_i18n::msg;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::category;
use crate::language::Language;
use crate::model::{Element, Function, Guid, Id, Layer, Model};
use crate::schedule::{Column, Kind, Table};

/// The categories a takeoff counts: the compound ones whose types carry layers.
pub const CATEGORIES: [&str; 4] = ["wall", "floor", "roof", "ceiling"];

/// The categories a call may name.
const MAX_CATEGORIES: usize = CATEGORIES.len();

/// The most waste a takeoff may add, percent.
pub const MAX_WASTE: f64 = 1_000.0;
/// The most a material's density may be, kg/m³ (osmium is about 22,600) and its mass per m², kg/m².
pub const MAX_DENSITY: f64 = 30_000.0;
pub const MAX_MASS_PER_M2: f64 = 1_000.0;

/// How a takeoff groups its rows: by material, by layer (material, thickness and function) or by the
/// construction type the layer is of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    /// One row for each material, the whole project summed.
    #[default]
    Material,
    /// One row for each material at a thickness and a function, whatever type it is of.
    Layer,
    /// One row for each layer of each construction type.
    Type,
}

impl By {
    /// Its name in the command's `by`, as [`By::from_name`] reads it.
    pub fn name(self) -> &'static str {
        match self {
            By::Material => "material",
            By::Layer => "layer",
            By::Type => "type",
        }
    }

    /// The grouping `name` names, in any case.
    pub fn from_name(name: &str) -> Option<By> {
        match name.trim().to_ascii_lowercase().as_str() {
            "material" => Some(By::Material),
            "layer" => Some(By::Layer),
            "type" => Some(By::Type),
            _ => None,
        }
    }
}

/// What a takeoff view counts and how it groups it, stored with its schedule (`Schedule::takeoff`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Takeoff {
    /// The categories it counts, as elements name theirs; empty, every one it can ([`CATEGORIES`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default)]
    pub by: By,
    /// Percent added for waste, an allowance on every row; 0, none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub waste: f64,
    /// The level whose elements it counts; none, the whole project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Guid>,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// `Material Takeoff` in the project's language, numbered if a view has that name already: what a takeoff
/// made without a name is called.
pub fn free_name(m: &Model) -> String {
    let base = m.language.word("Material Takeoff").to_string();
    std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base} {n}")))
        .find(|n| !m.elements.keys().any(|id| m.view_name(*id) == Some(n.as_str())))
        .unwrap_or(base)
}

/// A material's mass, kg per m³ of it and, for a sheet (a membrane, a housewrap), kg per m² of it: a sheet
/// may be weighed either way, and one that is said in kg/m² is weighed by its area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mass {
    pub density: f64,
    pub per_area: f64,
}

impl Mass {
    /// Whether it can weigh anything at all.
    pub fn known(&self) -> bool {
        self.density > 0.0 || self.per_area > 0.0
    }

    /// The mass of `area_m2` of a layer `thickness` mm thick, kg: by the area for a sheet, else by the volume.
    pub fn of(&self, area_m2: f64, thickness: f64) -> f64 {
        if self.per_area > 0.0 { area_m2 * self.per_area } else { area_m2 * (thickness / 1000.0) * self.density }
    }
}

/// The densities a project starts with, by the English name the starter library gives them: kg/m³ and, for a
/// sheet, kg/m².
const STARTER: &[(&str, f64, f64)] = &[
    ("Timber", 500.0, 0.0),
    ("Wood", 650.0, 0.0),
    ("Insulation", 35.0, 0.0),
    ("Membrane", 0.0, 1.0),
    ("Gypsum board", 900.0, 0.0),
    ("Concrete", 2400.0, 0.0),
    ("Concrete block", 600.0, 0.0),
    ("Brick", 1800.0, 0.0),
    ("Plaster", 1000.0, 0.0),
    ("Screed", 2000.0, 0.0),
    ("Ceramic tile", 2000.0, 0.0),
    ("Porcelain", 2400.0, 0.0),
    ("Roof tiles", 1900.0, 0.0),
    ("Glass", 2500.0, 0.0),
    ("Metal", 7850.0, 0.0),
    ("PVC", 1400.0, 0.0),
    ("Copper", 8900.0, 0.0),
    ("Fabric", 0.0, 0.2),
    ("Water", 1000.0, 0.0),
];

/// Words a material's name may hold that say what it is, in the project's languages and in the ones a
/// specification is written in: OSB, mineral wool, plywood and the rest a starter library has no entry for.
const KEYWORDS: &[(&str, f64, f64)] = &[
    ("osb", 650.0, 0.0),
    ("osb-3", 650.0, 0.0),
    ("oriented strand", 650.0, 0.0),
    ("plywood", 600.0, 0.0),
    ("mdf", 750.0, 0.0),
    ("particle board", 700.0, 0.0),
    ("chipboard", 700.0, 0.0),
    ("glulam", 500.0, 0.0),
    ("lvl", 550.0, 0.0),
    ("softwood", 500.0, 0.0),
    ("hardwood", 700.0, 0.0),
    ("mineral wool", 35.0, 0.0),
    ("rock wool", 35.0, 0.0),
    ("stone wool", 35.0, 0.0),
    ("glass wool", 25.0, 0.0),
    ("eps", 15.0, 0.0),
    ("xps", 35.0, 0.0),
    ("polyurethane", 40.0, 0.0),
    ("pir", 40.0, 0.0),
    ("gypsum", 900.0, 0.0),
    ("drywall", 900.0, 0.0),
    ("plasterboard", 900.0, 0.0),
    ("membrane", 0.0, 1.0),
    ("vapour barrier", 0.0, 0.15),
    ("vapor barrier", 0.0, 0.15),
    ("housewrap", 0.0, 0.1),
    ("waterproof", 0.0, 1.0),
    ("брус", 500.0, 0.0),
    ("пиломат", 500.0, 0.0),
    ("древес", 500.0, 0.0),
    ("фанера", 600.0, 0.0),
    ("осб", 650.0, 0.0),
    ("осб-3", 650.0, 0.0),
    ("минват", 35.0, 0.0),
    ("минеральн", 35.0, 0.0),
    ("утеплит", 35.0, 0.0),
    ("гипсокартон", 900.0, 0.0),
    ("гипс", 900.0, 0.0),
    ("гкл", 900.0, 0.0),
    ("мембран", 0.0, 1.0),
    ("пароизол", 0.0, 0.15),
    ("гидроизол", 0.0, 1.0),
    ("бетон", 2400.0, 0.0),
    ("кирпич", 1800.0, 0.0),
    ("штукатур", 1000.0, 0.0),
    ("makeira", 500.0, 0.0),
    ("madeira", 500.0, 0.0),
    ("lã mineral", 35.0, 0.0),
    ("lã de rocha", 35.0, 0.0),
    ("gesso acartonado", 900.0, 0.0),
    ("manta", 0.0, 1.0),
    ("madera", 500.0, 0.0),
    ("lana mineral", 35.0, 0.0),
    ("yeso", 900.0, 0.0),
    ("madeira", 500.0, 0.0),
    ("lã mineral", 35.0, 0.0),
    ("gips", 900.0, 0.0),
    ("dämm", 35.0, 0.0),
    ("hout", 500.0, 0.0),
    ("isolatie", 35.0, 0.0),
    ("legno", 500.0, 0.0),
    ("lana di roccia", 35.0, 0.0),
    ("drewno", 500.0, 0.0),
    ("wełna", 35.0, 0.0),
    ("gips", 900.0, 0.0),
    ("trä", 500.0, 0.0),
    ("isolering", 35.0, 0.0),
    ("gips", 900.0, 0.0),
];

/// The mass of a material named `name`: the starter's, in any of the project's languages, or one a word in
/// it says; none, where nothing does.
pub fn density_of(name: &str) -> Mass {
    let n = name.trim().to_lowercase().replace(['_', '-'], " ");
    for (english, density, per_area) in STARTER {
        let same = n == english.to_lowercase() || Language::ALL.iter().any(|l| l.word(english).to_lowercase() == n);
        if same {
            return Mass { density: *density, per_area: *per_area };
        }
    }
    for (word, density, per_area) in KEYWORDS {
        if n.contains(word) {
            return Mass { density: *density, per_area: *per_area };
        }
    }
    Mass::default()
}

/// A row of a takeoff: one material — its layer's thickness and function when it is grouped by layer or type —
/// over the elements that hold it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub level: String,
    pub category: String,
    pub type_name: String,
    pub material: String,
    /// mm; none where the row's material comes at more than one thickness.
    pub thickness: Option<f64>,
    pub function: String,
    /// The net surface area the layer covers, m²; none where it can't be measured.
    pub area_m2: Option<f64>,
    pub volume_m3: Option<f64>,
    /// kg; none where the material has no density or mass per m².
    pub mass_kg: Option<f64>,
    /// Percent added for waste.
    pub waste: f64,
    /// The measure the material is bought in with its waste: m² for a sheet, m³ for anything else; none where
    /// the row has no quantity.
    pub total: Option<f64>,
    /// `m2` or `m3`: what `total` is.
    pub unit: &'static str,
    /// Why the row has no quantity, when it has none.
    pub note: &'static str,
    /// The elements it counts.
    pub ids: Vec<Id>,
}

impl Row {
    pub fn json(&self) -> Value {
        let places = if self.unit == "m2" { 2 } else { 3 };
        json!({
            "level": self.level,
            "category": self.category,
            "type": self.type_name,
            "material": self.material,
            "thickness": self.thickness,
            "function": self.function,
            "area_m2": self.area_m2.map(|x| round(x, 2)),
            "volume_m3": self.volume_m3.map(|x| round(x, 3)),
            "mass_kg": self.mass_kg.map(|x| round(x, 1)),
            "waste": round(self.waste, 2),
            "total": self.total.map(|x| round(x, places)),
            "unit": self.unit,
            "note": self.note,
            "ids": self.ids,
        })
    }
}

/// A takeoff worked out: its rows and what had to be said about elements it couldn't count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Eval {
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
}

impl Eval {
    /// Its totals: the area, the volume and the mass of every row, and how many elements it counted.
    pub fn total(&self) -> Value {
        let sum = |f: fn(&Row) -> Option<f64>| -> Option<f64> {
            let mut total = 0.0;
            for row in &self.rows {
                total += f(row)?;
            }
            Some(total)
        };
        let ids: BTreeSet<Id> = self.rows.iter().filter(|r| r.volume_m3.is_some()).flat_map(|r| r.ids.iter().copied()).collect();
        json!({
            "area_m2": sum(|r| r.area_m2).map(|x| round(x, 2)),
            "volume_m3": sum(|r| r.volume_m3).map(|x| round(x, 3)),
            "mass_kg": sum(|r| r.mass_kg).map(|x| round(x, 1)),
            "elements": ids.len(),
        })
    }

    /// The rows as the schedule's table has them.
    pub fn table(&self) -> Table {
        // A level is a column only where one was asked for: a takeoff of the whole project has none to show.
        let with_level = self.rows.iter().any(|r| !r.level.is_empty());
        let mut columns = Vec::new();
        if with_level {
            columns.push(column(msg!("Level"), Kind::Text));
        }
        columns.extend([
            column(msg!("Category"), Kind::Text),
            column(msg!("Type"), Kind::Text),
            column(msg!("Material"), Kind::Text),
            column(msg!("Thickness"), Kind::Length),
            column(msg!("Function"), Kind::Text),
            column(msg!("Area"), Kind::Area),
            column(msg!("Volume (m³)"), Kind::Number),
            column(msg!("Mass (kg)"), Kind::Number),
            column(msg!("Waste (%)"), Kind::Number),
            column(msg!("Total"), Kind::Number),
            column(msg!("Note"), Kind::Text),
        ]);
        let cells = |r: &Row| {
            let places = if r.unit == "m2" { 2 } else { 3 };
            let mut cells = Vec::new();
            if with_level {
                cells.push(json!(r.level));
            }
            cells.extend([
                json!(r.category),
                json!(r.type_name),
                json!(r.material),
                json!(r.thickness),
                json!(r.function),
                json!(r.area_m2.map(|x| round(x, 2))),
                json!(r.volume_m3.map(|x| round(x, 3))),
                json!(r.mass_kg.map(|x| round(x, 1))),
                json!(round(r.waste, 2)),
                json!(r.total.map(|x| round(x, places))),
                json!(r.note),
            ]);
            cells
        };
        let rows: Vec<Vec<Value>> = self.rows.iter().map(cells).collect();
        let ids: Vec<Vec<Id>> = self.rows.iter().map(|r| r.ids.clone()).collect();
        let total = self.total();
        let summed = |key: &str| total.get(key).cloned().unwrap_or(Value::Null);
        let mut totals = Vec::new();
        if with_level {
            totals.push(Value::Null);
        }
        totals.extend([
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            summed("area_m2"),
            summed("volume_m3"),
            summed("mass_kg"),
            Value::Null,
            Value::Null,
            Value::Null,
        ]);
        Table { columns, rows, ids, totals: Some(totals) }
    }
}

fn column(heading: &str, kind: Kind) -> Column {
    Column { heading: heading.to_string(), kind }
}

fn round(x: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (x * k).round() / k
}

/// Why `t` can't be counted, said so it can be put right.
pub fn check(m: &Model, t: &Takeoff) -> Result<(), String> {
    if t.categories.len() > MAX_CATEGORIES {
        return Err(format!("it counts {} categories; a takeoff counts up to {MAX_CATEGORIES}", t.categories.len()));
    }
    if let Some(c) = t.categories.iter().find(|c| !CATEGORIES.contains(&category(c).as_str())) {
        return Err(format!("`{c}` isn't a category a takeoff counts; there are: {}", CATEGORIES.join(", ")));
    }
    if !t.waste.is_finite() || !(0.0..=MAX_WASTE).contains(&t.waste) {
        return Err(format!("`waste` is a percent from 0 to {MAX_WASTE}, not {}", t.waste));
    }
    if let Some(level) = t.level
        && m.level(level).is_none()
    {
        return Err(format!("its level {} doesn't exist", m.shown(level)));
    }
    Ok(())
}

/// The layers of a wall, a floor, a roof or a ceiling, and its type's name.
fn layers_of<'a>(m: &'a Model, e: &'a Element) -> Option<(&'static str, &'a str, &'a [Layer])> {
    match e {
        Element::Wall(w) => m.types.wall.get(&w.type_name).map(|t| ("wall", w.type_name.as_str(), &t.layers[..])),
        Element::Floor(f) => m.types.floor.get(&f.type_name).map(|t| ("floor", f.type_name.as_str(), &t.layers[..])),
        Element::Roof(r) => m.types.roof.get(&r.type_name).map(|t| ("roof", r.type_name.as_str(), &t.layers[..])),
        Element::Ceiling(c) => m.types.ceiling.get(&c.type_name).map(|t| ("ceiling", c.type_name.as_str(), &t.layers[..])),
        _ => None,
    }
}

/// The name of what a layer does, as a takeoff shows it.
fn function_name(f: Function) -> &'static str {
    match f {
        Function::Structure => "structure",
        Function::Substrate => "substrate",
        Function::Thermal => "thermal",
        Function::Membrane => "membrane",
        Function::Finish => "finish",
    }
}

/// The key a layer is summed under: which of the material, its thickness, its function and its type a grouping
/// tells apart.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    material: String,
    /// Thousandths of a mm, so it sorts; 0 where the grouping doesn't tell a thickness apart.
    thickness: i64,
    function: String,
    type_name: String,
    category: String,
}

/// What a group of layers comes to, before it is rounded into a row.
#[derive(Clone, Debug, Default)]
struct Sum {
    area: f64,
    /// Whether a layer gave it an area: a framing member is linear, and its row has a volume and no area.
    area_known: bool,
    volume: f64,
    mass: Option<f64>,
    /// Whether some mass couldn't be worked out: the row's mass is then none.
    mass_unknown: bool,
    thicknesses: BTreeSet<i64>,
    functions: BTreeSet<String>,
    types: BTreeSet<String>,
    categories: BTreeSet<String>,
    ids: BTreeSet<Id>,
    note: Option<&'static str>,
}

/// A layer's or a framing member's contribution to its group: what it measures, what it weighs, and where it is of.
struct Fact<'a> {
    area: Option<f64>,
    volume: f64,
    mass: Option<f64>,
    layer: Option<&'a Layer>,
    category: &'a str,
    type_name: &'a str,
    id: Id,
}

impl Sum {
    fn add(&mut self, fact: Fact<'_>) {
        if let Some(area) = fact.area {
            self.area += area;
            self.area_known = true;
        }
        self.volume += fact.volume;
        match fact.mass {
            Some(mass) => self.mass = Some(self.mass.unwrap_or(0.0) + mass),
            None => self.mass_unknown = true,
        }
        if let Some(layer) = fact.layer {
            self.thicknesses.insert((layer.thickness * 1000.0).round() as i64);
            self.functions.insert(function_name(layer.function).to_string());
        }
        self.types.insert(fact.type_name.to_string());
        self.categories.insert(fact.category.to_string());
        self.ids.insert(fact.id);
    }
}

/// The single value of `values`, or nothing where it has none or more than one.
fn one(values: &BTreeSet<i64>) -> Option<f64> {
    if values.len() == 1 { values.iter().next().map(|x| *x as f64 / 1000.0) } else { None }
}

fn only(values: &BTreeSet<String>) -> String {
    if values.len() == 1 { values.iter().next().cloned().unwrap_or_default() } else { String::new() }
}

/// `own` when the grouping tells it apart, else the one value a group holds (`only`).
fn pick(own: &str, values: &BTreeSet<String>) -> String {
    if own.is_empty() { only(values) } else { own.to_string() }
}

/// Takeoff `t` worked out on `m`: a row for each material, layer or type, its layers' net areas and volumes,
/// their mass, and the waste added to the measure it is bought in.
pub fn eval(m: &Model, t: &Takeoff) -> Result<Eval, String> {
    check(m, t)?;
    let categories: BTreeSet<String> = if t.categories.is_empty() {
        CATEGORIES.into_iter().map(String::from).collect()
    } else {
        t.categories.iter().map(|c| category(c)).collect()
    };
    let level = t.level.and_then(|g| m.elements.id(g));
    let level_name = level.and_then(|id| m.level(id).map(|l| l.name.clone())).unwrap_or_default();
    let mut warnings = BTreeSet::new();
    let mut sums: BTreeMap<Key, Sum> = BTreeMap::new();
    for (id, e) in m.elements.iter() {
        if !categories.contains(&category(e.category())) {
            continue;
        }
        if let Some(level) = t.level
            && m.level_of(e) != Some(level)
        {
            continue;
        }
        let Some(area) = m.areas(*id) else {
            warnings.insert(format!("{id}: its area can't be worked out (no type, no thickness, or a roof with no faces)"));
            let key = Key { material: String::new(), ..Key::default() };
            let sum = sums.entry(key).or_default();
            sum.note = Some("area undefined");
            sum.ids.insert(*id);
            continue;
        };
        let Some((cat, type_name, layers)) = layers_of(m, e) else {
            warnings.insert(format!("{id}: it has no type this takeoff can count"));
            let key = Key { material: String::new(), ..Key::default() };
            let sum = sums.entry(key).or_default();
            sum.note = Some("no type");
            sum.ids.insert(*id);
            continue;
        };
        if layers.is_empty() {
            warnings.insert(format!("{id}: {cat} type `{type_name}` has no layers"));
            let key = Key { type_name: type_name.to_string(), category: cat.to_string(), ..Key::default() };
            let sum = sums.entry(key).or_default();
            sum.note = Some("no layers");
            sum.ids.insert(*id);
        }
        let area_m2 = area.net / 1e6;
        for layer in layers {
            if layer.material.trim().is_empty() {
                warnings.insert(format!("{id}: a layer of {cat} type `{type_name}` has no material (`layer_without_material`)"));
                continue;
            }
            let mass = m.materials.get(&layer.material).map(|mat| Mass { density: mat.density, per_area: mat.mass_per_m2 });
            let volume_m3 = area_m2 * (layer.thickness / 1000.0);
            let kg = match mass.filter(Mass::known) {
                Some(mass) => Some(mass.of(area_m2, layer.thickness)),
                None => {
                    warnings.insert(format!("material `{}` has no density: its mass is not counted", layer.material));
                    None
                }
            };
            let thickness = match t.by {
                By::Material => 0,
                _ => (layer.thickness * 1000.0).round() as i64,
            };
            let function = match t.by {
                By::Material => String::new(),
                _ => function_name(layer.function).to_string(),
            };
            let type_key = match t.by {
                By::Type => type_name.to_string(),
                _ => String::new(),
            };
            let category_key = match t.by {
                By::Material => String::new(),
                _ => cat.to_string(),
            };
            let key = Key { material: layer.material.clone(), thickness, function, type_name: type_key, category: category_key };
            let sum = sums.entry(key).or_default();
            sum.add(Fact { area: Some(area_m2), volume: volume_m3, mass: kg, layer: Some(layer), category: cat, type_name, id: *id });
        }
        // Its framing, where it has some: its studs, joists or rafters as the framing type lays them out (`framing::of`),
        // linear timber with a section and a length, and so a volume and a mass but no area. Their material may be the
        // framing type's own, else the structure layer's.
        if let Ok(Some(framing)) = crate::framing::of(m, *id) {
            for w in &framing.warnings {
                warnings.insert(format!("{id}: {w}"));
            }
            let material = framing.material.trim().to_string();
            if material.is_empty() {
                warnings.insert(format!("{id}: its framing names no material (`framing_without_material`)"));
            } else {
                let volume: f64 = framing.members.iter().map(|x| x.section[0] * x.section[1] * x.length * f64::from(x.plies) / 1e9).sum();
                let density = m.materials.get(&material).map_or(0.0, |mat| mat.density);
                let kg = if density > 0.0 {
                    Some(volume * density)
                } else {
                    warnings.insert(format!("material `{material}` has no density: its mass is not counted"));
                    None
                };
                let (thickness, function, type_key, category_key) = match t.by {
                    By::Material => (0, String::new(), String::new(), String::new()),
                    By::Layer => (0, "framing".to_string(), String::new(), cat.to_string()),
                    By::Type => (0, "framing".to_string(), framing.type_name.clone(), cat.to_string()),
                };
                let key = Key { material, thickness, function, type_name: type_key, category: category_key };
                let sum = sums.entry(key).or_default();
                sum.add(Fact { area: None, volume, mass: kg, layer: None, category: cat, type_name, id: *id });
            }
        }
    }
    let rows: Vec<Row> = sums
        .into_iter()
        .map(|(key, sum)| {
            let waste = t.waste;
            let mass_kg = (!sum.mass_unknown).then_some(sum.mass.unwrap_or(0.0));
            let mass = m.materials.get(&key.material);
            let sheet = sum.area_known && mass.is_some_and(|mat| mat.mass_per_m2 > 0.0);
            let unit = if sheet { "m2" } else { "m3" };
            let base = if sheet { sum.area } else { sum.volume };
            let measured = sum.note.is_none();
            let (area_m2, volume_m3, total) = if measured {
                (sum.area_known.then_some(sum.area), Some(sum.volume), Some(base * (1.0 + waste / 100.0)))
            } else {
                // A row that says why nothing is counted holds no quantity, not a zero.
                (None, None, None)
            };
            let mass_kg = if measured { mass_kg } else { None };
            Row {
                level: level_name.clone(),
                category: pick(&key.category, &sum.categories),
                type_name: pick(&key.type_name, &sum.types),
                material: key.material,
                thickness: if key.thickness != 0 { Some(key.thickness as f64 / 1000.0) } else { one(&sum.thicknesses) },
                function: if key.function.is_empty() { only(&sum.functions) } else { key.function },
                area_m2,
                volume_m3,
                mass_kg,
                waste,
                total,
                unit,
                note: sum.note.unwrap_or(""),
                ids: sum.ids.into_iter().collect(),
            }
        })
        .collect();
    Ok(Eval { rows, warnings: warnings.into_iter().collect() })
}

/// Takeoff `t`'s table, as its schedule draws it.
pub fn table(m: &Model, t: &Takeoff) -> Result<Table, String> {
    Ok(eval(m, t)?.table())
}
