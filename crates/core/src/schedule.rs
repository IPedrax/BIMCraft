//! Schedules: one row per element of a category, as a schedule view lists them. Its fields are its
//! columns: its category's own, the project parameters for it and, for components, their families'
//! parameters; or calculated, worked out from the others by a formula, as a family's are. Its
//! filters keep the elements every one of them holds for; its rows sort by its sort fields in turn
//! (a phase by its place in the project's order, not its name) and, grouped, come together into one
//! row for each set of their values.

use bimcraft_i18n::{msg, ui_msg};

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{category, element_json};
use crate::family::{self, MAX_FORMULA};
use crate::language::Language;
use crate::model::{Element, Id, MAX_COORD, MAX_TEXT, Model, ParameterKind, Schedule};
use crate::phasing;
use crate::units::Units;

/// What a field holds, and so how a schedule shows it and a formula reads it: text, a number, a
/// length (mm), an area (m²), or yes or no (1 or 0 to a formula).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    #[default]
    Number,
    Length,
    Area,
    YesNo,
}

/// A schedule's column: one of its category's fields, or, with `formula`, a calculated one of its
/// own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    /// A formula over the row's other fields, as a family's are: lengths in mm, areas in m².
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// What a calculated field comes to: a number unless it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// Its column's heading, when it isn't its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    /// Summed in a totals row under the rows, and over each group.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub total: bool,
}

/// How a filter holds a field's value up to its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Op {
    Equals,
    NotEquals,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    Contains,
    NotContains,
    BeginsWith,
    NotBeginsWith,
    EndsWith,
    NotEndsWith,
    HasValue,
    HasNoValue,
}

/// A schedule keeps the elements whose `field` holds `op` to `value` (text in any case).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub field: String,
    pub op: Op,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sort {
    pub field: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub descending: bool,
}

/// The most fields, filters or sort fields a schedule has.
pub const MAX_FIELDS: usize = 64;

/// A column of a table: its heading, and what its cells hold.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub heading: String,
    pub kind: Kind,
}

pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    /// The elements each row lists: one, or a group's.
    pub ids: Vec<Vec<Id>>,
    /// Its total fields' sums, each under its column, `null` under the rest; none without one.
    pub totals: Option<Vec<Value>>,
}

/// The schedules every project has, as views: each one's category, and its name. Each is the
/// project's own once it's placed on a sheet or changed.
pub const VIEWS: [(&str, &str); 6] = [
    ("room", msg!("Room Schedule")),
    ("door", msg!("Door Schedule")),
    ("window", msg!("Window Schedule")),
    ("wall", msg!("Wall Schedule")),
    ("floor", msg!("Floor Schedule")),
    ("level", msg!("Level Schedule")),
];

/// One of the schedules every project has, by its name (in any of the languages, in any case) or,
/// with no name, its category: its category, and its English name.
pub fn builtin(name: Option<&str>, cat: Option<&str>) -> Option<(&'static str, &'static str)> {
    match (name, cat) {
        (Some(n), _) => {
            let n = n.trim().to_lowercase();
            VIEWS.iter().find(|(_, en)| Language::ALL.iter().any(|l| l.word(en).to_lowercase() == n)).copied()
        }
        (None, Some(cat)) => VIEWS.iter().find(|(c, _)| *c == cat).copied(),
        (None, None) => None,
    }
}

/// The project's schedule named `name`: one every project has, whatever language or case names it;
/// else the one of that name.
pub fn named(m: &Model, name: &str) -> Option<Id> {
    let twin = builtin(Some(name), None);
    m.elements.iter().find_map(|(id, e)| match e {
        Element::Schedule(sc) if twin.is_some() && builtin(Some(&sc.name), None) == twin => Some(*id),
        Element::Schedule(sc) if twin.is_none() && sc.name == name => Some(*id),
        _ => None,
    })
}

/// The schedule of category `cat` every project has: the project's own once it's made it so, else
/// as every project has it, named in its language.
pub fn standard(m: &Model, cat: &str) -> (Option<Id>, Schedule) {
    let name = VIEWS.iter().find(|(c, _)| *c == cat).map_or("", |(_, n)| *n);
    match named(m, name).and_then(|id| Some((id, m.elements.get(id)?))) {
        Some((id, Element::Schedule(sc))) => (Some(id), sc.clone()),
        _ => (None, Schedule { name: m.language.word(name).to_string(), of: cat.to_string(), ..Schedule::default() }),
    }
}

/// Where a category's own field comes from: its element's JSON (`element_json`) under a key (a
/// number missing there is 0), its level's name, metres there in mm, a railing's path's length, or
/// 1, to count it.
#[derive(Clone, Copy)]
enum From {
    Key(&'static str),
    /// The id of the element it points at under a key (by GUID in its JSON).
    Ref(&'static str),
    Level,
    Metres(&'static str),
    PathLength,
    Count,
    /// A yes or no in the element's JSON under a key (no where it has none).
    Flag(&'static str),
    /// A value among the element's parameters, under the field's own name (its phases, its design option).
    Param,
    /// How a door or a window opens, in the project's language.
    Operation,
}

type Own = (&'static str, Kind, From);

const ID: Own = ("Id", Kind::Number, From::Key("id"));
const TYPE: Own = ("Type", Kind::Text, From::Key("type"));
const FAMILY: Own = (msg!("Family"), Kind::Text, From::Key("family"));
const LEVEL: Own = ("Level", Kind::Text, From::Level);
const AREA: Own = (msg!("Area"), Kind::Area, From::Key("area_m2"));
const WIDTH: Own = ("Width", Kind::Length, From::Key("width"));
const HEIGHT: Own = ("Height", Kind::Length, From::Key("height"));
const LENGTH: Own = ("Length", Kind::Length, From::Key("length"));
const THICKNESS: Own = ("Thickness", Kind::Length, From::Key("thickness"));
const DEPTH: Own = ("Depth", Kind::Length, From::Key("depth"));
const SYSTEM: Own = (ui_msg!("System"), Kind::Text, From::Key("system"));
const OFFSET: Own = (msg!("Offset"), Kind::Length, From::Key("offset"));
const MARK: Own = ("Mark", Kind::Text, From::Key("mark"));
const OPERATION: Own = (msg!("Operation"), Kind::Text, From::Operation);
const NAME: Own = ("Name", Kind::Text, From::Key("name"));
const HOST: Own = ("Wall", Kind::Number, From::Ref("host"));
const COUNT: Own = (msg!("Count"), Kind::Number, From::Count);
const PHASE_CREATED: Own = ("Phase Created", Kind::Text, From::Param);
const PHASE_DEMOLISHED: Own = ("Phase Demolished", Kind::Text, From::Param);
const DESIGN_OPTION: Own = ("Design Option", Kind::Text, From::Param);
const VOLUME: Own = (msg!("Volume"), Kind::Number, From::Key("volume_m3"));

/// Each category's own fields, and how many of them, from the first, its schedule shows unless it
/// chooses.
const OWN: &[(&str, usize, &[Own])] = &[
    (
        "room",
        5,
        &[
            (msg!("Number"), Kind::Text, From::Key("number")),
            NAME,
            LEVEL,
            AREA,
            (msg!("Perimeter"), Kind::Length, From::Metres("perimeter_m")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("door", 6, &[MARK, TYPE, WIDTH, HEIGHT, LEVEL, HOST, FAMILY, OPERATION, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "window",
        7,
        &[
            MARK,
            TYPE,
            WIDTH,
            HEIGHT,
            (msg!("Sill"), Kind::Length, From::Key("sill")),
            LEVEL,
            HOST,
            FAMILY,
            OPERATION,
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("wall", 6, &[ID, TYPE, LEVEL, LENGTH, HEIGHT, THICKNESS, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "floor",
        5,
        &[
            ID,
            TYPE,
            LEVEL,
            AREA,
            THICKNESS,
            OFFSET,
            COUNT,
            (ui_msg!("Structural"), Kind::YesNo, From::Flag("structural")),
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("ceiling", 5, &[ID, TYPE, LEVEL, OFFSET, AREA, THICKNESS, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "roof",
        4,
        &[
            ID,
            TYPE,
            LEVEL,
            (msg!("Slope"), Kind::Number, From::Key("slope")),
            (msg!("Plan Area"), Kind::Area, From::Key("area_m2")),
            OFFSET,
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    (
        "column",
        6,
        &[
            ID,
            TYPE,
            LEVEL,
            WIDTH,
            ("Depth", Kind::Length, From::Key("depth")),
            HEIGHT,
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("structuralcolumn", 6, &[ID, TYPE, LEVEL, WIDTH, DEPTH, HEIGHT, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "footing",
        6,
        &[
            ID,
            TYPE,
            LEVEL,
            WIDTH,
            DEPTH,
            ("Thickness", Kind::Length, From::Key("height")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("beam", 6, &[ID, TYPE, LEVEL, LENGTH, WIDTH, DEPTH, SYSTEM, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "rebar",
        7,
        &[
            ID,
            LEVEL,
            ("Grade", Kind::Text, From::Key("grade")),
            ("Diameter", Kind::Length, From::Key("diameter")),
            LENGTH,
            COUNT,
            (msg!("Weight"), Kind::Number, From::Key("weight_kg")),
            ("Host", Kind::Number, From::Ref("host")),
            ("Layer", Kind::Text, From::Key("layer")),
        ],
    ),
    ("stripfooting", 6, &[ID, TYPE, LEVEL, LENGTH, WIDTH, DEPTH, SYSTEM, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "pipe",
        6,
        &[
            ID,
            TYPE,
            LEVEL,
            SYSTEM,
            LENGTH,
            ("Diameter", Kind::Length, From::Key("width")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    (
        "conduit",
        6,
        &[
            ID,
            TYPE,
            LEVEL,
            SYSTEM,
            LENGTH,
            ("Diameter", Kind::Length, From::Key("width")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("cabletray", 7, &[ID, TYPE, LEVEL, SYSTEM, LENGTH, WIDTH, DEPTH, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "cable",
        6,
        &[
            ID,
            TYPE,
            LEVEL,
            SYSTEM,
            LENGTH,
            ("Diameter", Kind::Length, From::Key("width")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("downconductor", 7, &[ID, TYPE, LEVEL, SYSTEM, LENGTH, WIDTH, DEPTH, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    ("groundconductor", 7, &[ID, TYPE, LEVEL, SYSTEM, LENGTH, WIDTH, DEPTH, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "wire",
        6,
        &[
            ID,
            TYPE,
            LEVEL,
            SYSTEM,
            LENGTH,
            ("Diameter", Kind::Length, From::Key("width")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    ("component", 4, &[FAMILY, TYPE, LEVEL, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION]),
    (
        "stair",
        7,
        &[
            ID,
            LEVEL,
            WIDTH,
            HEIGHT,
            (msg!("Risers"), Kind::Number, From::Key("risers")),
            (msg!("Riser Height"), Kind::Length, From::Key("riser_height")),
            (msg!("Tread"), Kind::Length, From::Key("tread")),
            LENGTH,
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    (
        "ramp",
        6,
        &[
            ID,
            LEVEL,
            WIDTH,
            HEIGHT,
            LENGTH,
            ("Slope", Kind::Number, From::Key("slope")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    (
        "railing",
        5,
        &[ID, TYPE, LEVEL, ("Length", Kind::Length, From::PathLength), HEIGHT, COUNT, PHASE_CREATED, PHASE_DEMOLISHED, DESIGN_OPTION],
    ),
    (
        "mass",
        5,
        &[
            NAME,
            LEVEL,
            HEIGHT,
            AREA,
            VOLUME,
            (msg!("Gross Floor Area"), Kind::Area, From::Key("gross_floor_area_m2")),
            COUNT,
            PHASE_CREATED,
            PHASE_DEMOLISHED,
            DESIGN_OPTION,
        ],
    ),
    // A row for each level a mass crosses (`sources`): its floor area there, and the volume from there up to the next.
    (
        "massfloor",
        5,
        &[("Mass", Kind::Text, From::Key("mass")), LEVEL, AREA, ("Perimeter", Kind::Length, From::Metres("perimeter_m")), VOLUME, COUNT],
    ),
    ("level", 2, &[("Elevation", Kind::Length, From::Key("elevation")), NAME, COUNT]),
];

/// The fields of what schedules aren't of: annotation, views and sheets, what Filter on a selection goes by
/// (`selection::common`) and a tag may show. Each one's count is 0: no schedule shows them.
const MORE: &[(&str, usize, &[Own])] = &[
    ("line", 0, &[("Line Style", Kind::Text, From::Key("style")), LENGTH]),
    ("region", 0, &[TYPE, ("Line Style", Kind::Text, From::Key("line_style"))]),
    ("detailgroup", 0, &[TYPE]),
    ("repeatingdetail", 0, &[TYPE, LENGTH]),
    ("text", 0, &[("Text", Kind::Text, From::Key("text"))]),
    ("tag", 0, &[("Text", Kind::Text, From::Key("text"))]),
    ("dimension", 0, &[("Kind", Kind::Text, From::Key("kind"))]),
    ("grid", 0, &[NAME]),
    ("section", 0, &[NAME]),
    ("elevation", 0, &[NAME]),
    ("sheet", 0, &[("Number", Kind::Text, From::Key("number")), NAME]),
    ("viewport", 0, &[(ui_msg!("Title"), Kind::Text, From::Key("title"))]),
];

/// The categories schedules are of, as `OWN` lists them.
pub fn categories() -> Vec<&'static str> {
    OWN.iter().map(|(c, _, _)| *c).collect()
}

fn own(cat: &str) -> (usize, &'static [Own]) {
    OWN.iter().chain(MORE).find(|(c, _, _)| *c == cat).map_or((0, &[][..]), |(_, n, f)| (*n, *f))
}

/// Whether `name` is one of category `cat`'s own fields.
pub fn own_field(cat: &str, name: &str) -> bool {
    own(cat).1.iter().any(|(n, _, _)| *n == name)
}

/// Category `cat`'s fields, each once, with what it holds: its own, then the project parameters
/// for it and, for components, the parameters of the model's families.
pub fn fields(m: &Model, cat: &str) -> Vec<(String, Kind)> {
    let mut out: Vec<(String, Kind)> = own(cat).1.iter().map(|(n, k, _)| (n.to_string(), *k)).collect();
    let mut seen: BTreeSet<String> = out.iter().map(|(n, _)| n.clone()).collect();
    let mut add = |n: &str, k: Kind| {
        if seen.insert(n.to_string()) {
            out.push((n.to_string(), k));
        }
    };
    for (n, p) in m.parameters.iter().filter(|(_, p)| p.categories.iter().any(|c| c == cat)) {
        add(
            n,
            match p.kind {
                ParameterKind::Text => Kind::Text,
                ParameterKind::Number => Kind::Number,
                ParameterKind::Length => Kind::Length,
                ParameterKind::YesNo => Kind::YesNo,
            },
        );
    }
    if cat == "component" {
        for p in m.families.values().flat_map(|f| &f.parameters) {
            add(
                &p.name,
                match p.kind {
                    family::Kind::Length => Kind::Length,
                    family::Kind::Number | family::Kind::Angle => Kind::Number,
                    family::Kind::YesNo => Kind::YesNo,
                    family::Kind::Material => Kind::Text,
                },
            );
        }
    }
    out
}

/// Field `name` of element `id` as its category's schedule shows it (a length or an area in the project's units, a yes or no as
/// `Yes` or `No`): what a tag shows. None where the element has no such field, or no value for it.
pub fn field_text(m: &Model, id: impl crate::ids::Key, name: &str) -> Option<String> {
    let id = m.elements.id(id)?;
    let e = m.elements.get(id)?;
    let cat = e.category();
    let kind = fields(m, cat).into_iter().find(|(n, _)| n == name)?.1;
    let formulas = BTreeMap::new();
    let mut row = Row { m, e, cat, json: element_json(m, id), formulas: &formulas, done: BTreeMap::new() };
    match row.get(name) {
        Value::Null => None,
        v => Some(shown(m.units, kind, &v)),
    }
}

/// The fields a schedule of category `cat` shows unless it chooses.
pub fn defaults(cat: &str) -> Vec<Field> {
    let (n, own) = own(cat);
    own.iter().take(n).map(|(name, _, _)| Field { name: name.to_string(), ..Field::default() }).collect()
}

/// Its fields: those it chose, or its category's.
fn chosen(sc: &Schedule, cat: &str) -> Vec<Field> {
    if sc.fields.is_empty() { defaults(cat) } else { sc.fields.clone() }
}

/// What `what` holds, said: `text`, `a number`…
fn holding(k: Kind) -> &'static str {
    match k {
        Kind::Text => "text",
        Kind::Number => "a number",
        Kind::Length => "a length",
        Kind::Area => "an area",
        Kind::YesNo => "yes or no",
    }
}

/// Whether schedule `sc` is sound: of a category there are schedules of; its fields its category's
/// or calculated from them (each once, formulas reading numbers, none coming back to itself), its
/// totals of numbers; filtering and sorting by its fields, filters comparing what the field holds.
pub fn check(m: &Model, sc: &Schedule) -> Result<(), String> {
    let cat = category(&sc.of);
    if own(&cat).1.is_empty() {
        return Err(format!("there's no schedule of `{}`; there are schedules of: {}", sc.of, categories().join(", ")));
    }
    if let Some((of, name)) = builtin(Some(&sc.name), None).filter(|(of, _)| *of != cat) {
        return Err(format!("`{}` is the {of}s' {name} every project has; call a schedule of {cat}s something else", sc.name));
    }
    for (what, n) in [("fields", sc.fields.len()), ("filters", sc.filters.len()), ("sort fields", sc.sort.len())] {
        if n > MAX_FIELDS {
            return Err(format!("it has {n} {what}; a schedule has up to {MAX_FIELDS}"));
        }
    }
    let available = fields(m, &cat);
    let mut kinds: BTreeMap<&str, Kind> = available.iter().map(|(n, k)| (n.as_str(), *k)).collect();
    let unknown = |n: &str| {
        let names: Vec<&str> = available.iter().map(|(n, _)| n.as_str()).collect();
        format!("`{n}` isn't a field of {cat}s; there are: {}", names.join(", "))
    };
    let fields = chosen(sc, &cat);
    let mut names = BTreeSet::new();
    for f in &fields {
        if !names.insert(f.name.as_str()) {
            return Err(format!("field `{}` is in it twice", f.name));
        }
        if f.heading.as_ref().is_some_and(|h| h.chars().count() > MAX_TEXT) {
            return Err(format!("field `{}`: a heading is up to {MAX_TEXT} characters", f.name));
        }
        match &f.formula {
            None if !kinds.contains_key(f.name.as_str()) => return Err(unknown(&f.name)),
            None if f.kind.is_some() => {
                return Err(format!("field `{}`: only a calculated field says what it holds; give it a formula", f.name));
            }
            None => {}
            Some(formula) => {
                if !family::nameable(&f.name) {
                    return Err(format!(
                        "calculated field `{}`: a name starts with a letter and holds letters, digits, `_` and spaces, up to {MAX_TEXT}",
                        f.name
                    ));
                }
                if kinds.contains_key(f.name.as_str()) {
                    return Err(format!("calculated field `{}`: {cat}s have a field of that name; call it something else", f.name));
                }
                if f.kind == Some(Kind::Text) {
                    return Err(format!("calculated field `{}`: a formula comes to a number, a length, an area or yes or no", f.name));
                }
                if formula.chars().count() > MAX_FORMULA {
                    return Err(format!("calculated field `{}`: a formula is up to {MAX_FORMULA} characters", f.name));
                }
            }
        }
    }
    let formulas: BTreeMap<&str, &str> = fields.iter().filter_map(|f| Some((f.name.as_str(), f.formula.as_deref()?))).collect();
    for f in fields.iter().filter(|f| f.formula.is_some()) {
        kinds.insert(&f.name, f.kind.unwrap_or_default());
    }
    let mut depths = BTreeMap::new();
    for name in formulas.keys() {
        follow(name, &formulas, &kinds, &unknown, &mut Vec::new(), &mut depths)?;
    }
    if let Some(f) =
        fields.iter().find(|f| f.total && !matches!(kinds.get(f.name.as_str()), Some(Kind::Number | Kind::Length | Kind::Area)))
    {
        let k = kinds.get(f.name.as_str()).copied().unwrap_or_default();
        return Err(format!("field `{}` holds {}; only numbers, lengths and areas total", f.name, holding(k)));
    }
    for flt in &sc.filters {
        check_filter(flt, &kinds, &unknown)?;
    }
    if let Some(s) = sc.sort.iter().find(|s| !kinds.contains_key(s.field.as_str())) {
        return Err(format!("sorting: {}", unknown(&s.field)));
    }
    Ok(())
}

/// Whether filter `flt` is sound for fields of these `kinds`: a field there is, compared as what it
/// holds can be, with a value of that kind.
fn check_filter(flt: &Filter, kinds: &BTreeMap<&str, Kind>, unknown: &dyn Fn(&str) -> String) -> Result<(), String> {
    let k = *kinds.get(flt.field.as_str()).ok_or_else(|| format!("filter: {}", unknown(&flt.field)))?;
    let who = format!("filter on `{}`", flt.field);
    let text_op = matches!(flt.op, Op::Contains | Op::NotContains | Op::BeginsWith | Op::NotBeginsWith | Op::EndsWith | Op::NotEndsWith);
    if text_op && k != Kind::Text {
        return Err(format!("{who}: it holds {}, and only text contains, begins or ends", holding(k)));
    }
    if k == Kind::YesNo && !matches!(flt.op, Op::Equals | Op::NotEquals | Op::HasValue | Op::HasNoValue) {
        return Err(format!("{who}: yes or no only equals or doesn't"));
    }
    let fits = match (flt.op, k, &flt.value) {
        // Read by none, it's still small.
        (Op::HasValue | Op::HasNoValue, _, v) => match v {
            Value::Null | Value::Bool(_) => true,
            Value::String(s) => s.chars().count() <= MAX_TEXT,
            v => v.as_f64().is_some_and(|x| x.is_finite() && x.abs() <= MAX_COORD),
        },
        (_, Kind::Text, Value::String(s)) => s.chars().count() <= MAX_TEXT,
        (_, Kind::YesNo, Value::Bool(_)) => true,
        (_, Kind::Number | Kind::Length | Kind::Area, v) => v.as_f64().is_some_and(|x| x.is_finite() && x.abs() <= MAX_COORD),
        _ => false,
    };
    if !fits {
        let unit = match k {
            Kind::Length => " (mm)",
            Kind::Area => " (m²)",
            _ => "",
        };
        return Err(format!("{who}: it compares {}{unit}, not {}", holding(k), flt.value));
    }
    Ok(())
}

/// Whether `rules` are sound for the elements of each category in `cats`: filters on fields every
/// one of them has.
pub fn check_rules(m: &Model, cats: &[String], rules: &[Filter]) -> Result<(), String> {
    if rules.len() > MAX_FIELDS {
        return Err(format!("it has {} rules; a filter has up to {MAX_FIELDS}", rules.len()));
    }
    for cat in cats {
        let available = fields(m, cat);
        let kinds: BTreeMap<&str, Kind> = available.iter().map(|(n, k)| (n.as_str(), *k)).collect();
        let unknown = |n: &str| {
            let names: Vec<&str> = available.iter().map(|(n, _)| n.as_str()).collect();
            format!("`{n}` isn't a field of {cat}s; there are: {}", names.join(", "))
        };
        for flt in rules {
            check_filter(flt, &kinds, &unknown)?;
        }
    }
    Ok(())
}

/// The elements of categories `cats` that hold every one of `rules`.
pub fn picks(m: &Model, cats: &[String], rules: &[Filter]) -> BTreeSet<Id> {
    let kinds: BTreeMap<&str, BTreeMap<String, Kind>> = cats.iter().map(|c| (c.as_str(), fields(m, c).into_iter().collect())).collect();
    let formulas = BTreeMap::new();
    let mut out = BTreeSet::new();
    for (&id, e) in &m.elements {
        let Some(of) = kinds.get(e.category()) else { continue };
        let mut row = Row { m, e, cat: e.category(), json: element_json(m, id), formulas: &formulas, done: BTreeMap::new() };
        if rules.iter().all(|f| keeps(f, &row.get(&f.field), of.get(&f.field).copied(), m.units)) {
            out.insert(id);
        }
    }
    out
}

/// Calculated field `name`'s formula, and those of the calculated fields it reads, each reading
/// fields there are and holding numbers, none coming back to itself, nor in a row of more than
/// `MAX_CHAIN`; `path`, the formulas being followed; `depths`, each one followed's longest row of
/// formulas from it (itself among them), whichever order they come in.
fn follow<'a>(
    name: &str,
    formulas: &BTreeMap<&'a str, &'a str>,
    kinds: &BTreeMap<&str, Kind>,
    unknown: &dyn Fn(&str) -> String,
    path: &mut Vec<&'a str>,
    depths: &mut BTreeMap<&'a str, usize>,
) -> Result<usize, String> {
    let Some((&name, formula)) = formulas.get_key_value(name) else { return Ok(0) };
    let too_deep = || format!("calculated field `{name}` reads formulas reading more than {} others in a row", family::MAX_CHAIN);
    if let Some(&d) = depths.get(name) {
        return if path.len() + d > family::MAX_CHAIN { Err(too_deep()) } else { Ok(d) };
    }
    if path.contains(&name) {
        return Err(format!("calculated field `{name}`'s formula comes back to itself"));
    }
    if path.len() >= family::MAX_CHAIN {
        return Err(too_deep());
    }
    let who = format!("calculated field `{name}`");
    path.push(name);
    let mut below = 0;
    for n in family::names(formula).map_err(|e| format!("{who}: {e}"))? {
        match kinds.get(n.as_str()) {
            None => return Err(format!("{who}: {}", unknown(&n))),
            Some(Kind::Text) => return Err(format!("{who}: `{n}` is text, and a formula reads numbers")),
            Some(_) => below = below.max(follow(&n, formulas, kinds, unknown, path, depths)?),
        }
    }
    path.pop();
    if path.len() + below + 1 > family::MAX_CHAIN {
        return Err(too_deep());
    }
    depths.insert(name, below + 1);
    Ok(below + 1)
}

/// An element's values, as a row of a schedule works them out: its category's fields' from its
/// JSON, and calculated ones' from their formulas.
struct Row<'a> {
    m: &'a Model,
    e: &'a Element,
    /// The category of the schedule it is a row of: `e`'s, but for a mass floor, which is a row of a mass.
    cat: &'a str,
    json: Value,
    formulas: &'a BTreeMap<&'a str, (&'a str, Kind)>,
    done: BTreeMap<String, Value>,
}

impl Row<'_> {
    /// Field `name`'s value; `null` where it has none.
    fn get(&mut self, name: &str) -> Value {
        self.value(name, &mut Vec::new())
    }

    fn value(&mut self, name: &str, path: &mut Vec<String>) -> Value {
        if let Some(v) = self.done.get(name) {
            return v.clone();
        }
        let formulas = self.formulas;
        let v = match formulas.get(name) {
            // Read at check; here, a field with no value leaves it with none.
            Some(_) if path.iter().any(|p| p == name) || path.len() >= family::MAX_CHAIN => Value::Null,
            Some(&(formula, kind)) => {
                path.push(name.to_string());
                let x = family::eval(formula, &mut |n: &str| match self.value(n, path) {
                    Value::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
                    v => v.as_f64().ok_or_else(|| format!("`{n}` has no number")),
                });
                path.pop();
                match (x, kind) {
                    (Ok(x), Kind::YesNo) => Value::Bool(x != 0.0),
                    (Ok(x), _) => serde_json::json!(fine(x)),
                    (Err(_), _) => Value::Null,
                }
            }
            None => self.field(name),
        };
        self.done.insert(name.to_string(), v.clone());
        v
    }

    /// One of its category's fields: its own, a project parameter's, or its family's.
    fn field(&self, name: &str) -> Value {
        let (m, j) = (self.m, &self.json);
        if let Some((_, kind, from)) = own(self.cat).1.iter().find(|(n, _, _)| *n == name) {
            let number = |v: Option<&Value>| v.and_then(Value::as_f64).unwrap_or(0.0);
            return match *from {
                From::Key(k) if matches!(kind, Kind::Text) => match j.get(k) {
                    Some(Value::String(s)) if s.is_empty() => Value::Null,
                    Some(v) => v.clone(),
                    None => Value::Null,
                },
                From::Key(k) => serde_json::json!(number(j.get(k))),
                From::Ref(k) => {
                    let to = j.get(k).and_then(|g| serde_json::from_value::<crate::model::Guid>(g.clone()).ok());
                    serde_json::json!(to.and_then(|g| m.elements.id(g)).map_or(0.0, |id| id as f64))
                }
                From::Level => j
                    .get("level")
                    .and_then(|l| serde_json::from_value::<crate::model::Guid>(l.clone()).ok())
                    .and_then(|l| m.level(l))
                    .map_or(Value::Null, |l| Value::String(l.name.clone())),
                From::Metres(k) => serde_json::json!(fine(number(j.get(k)) * 1000.0)),
                From::PathLength => {
                    let path: Vec<crate::geom::P2> = j.get("path").and_then(|p| serde_json::from_value(p.clone()).ok()).unwrap_or_default();
                    serde_json::json!(fine(path.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()))
                }
                From::Count => serde_json::json!(1.0),
                From::Flag(k) => Value::Bool(j.get(k).and_then(Value::as_bool).unwrap_or(false)),
                From::Operation => j
                    .get("operation")
                    .and_then(Value::as_str)
                    .and_then(crate::model::Operation::parse)
                    .map_or(Value::Null, |op| Value::String(m.language.word(op.label()).to_string())),
                // Its parameter of that name, or its wall's for a door or window with none; a phase it holds none of
                // is the project's first.
                From::Param => match phasing::value(m, self.e, name) {
                    Some(v) if !v.is_empty() => Value::String(v.to_string()),
                    _ if name == phasing::CREATED => m.phases.first().map_or(Value::Null, |p| Value::String(p.clone())),
                    _ => Value::Null,
                },
            };
        }
        let cat = self.cat;
        if let Some(p) = m.parameters.get(name).filter(|p| p.categories.iter().any(|c| c == cat)) {
            return match self.e.params().and_then(|ps| ps.get(name)) {
                Some(v) => v.clone(),
                // Unset, a yes or no is no, as the palette shows it.
                None if p.kind == ParameterKind::YesNo => Value::Bool(false),
                None => Value::Null,
            };
        }
        if let Some((family, _)) = self.e.family()
            && let Some(p) = m.families.get(family).and_then(|f| f.parameters.iter().find(|p| p.name == name))
        {
            let v = j.get("values").and_then(|v| v.get(name)).cloned().unwrap_or(Value::Null);
            return match (p.kind, v.as_f64()) {
                (family::Kind::YesNo, Some(x)) => Value::Bool(x != 0.0),
                (family::Kind::Material, _) if v.as_str() == Some("") => Value::Null,
                _ => v,
            };
        }
        Value::Null
    }
}

/// `x` to the millionth, so sums and formulas come out as written.
fn fine(x: f64) -> f64 {
    let r = (x * 1e6).round() / 1e6 + 0.0;
    if r.is_finite() { r } else { x }
}

/// What a schedule of category `cat` has a row for, each with its JSON: an element of the category. Of mass floors, a mass
/// at each level it crosses, with its floor area there, its perimeter and the volume up to the next such level or its top.
fn sources<'a>(m: &'a Model, cat: &str) -> Vec<(Id, &'a Element, Value)> {
    if cat != "massfloor" {
        return m.elements.iter().filter(|(_, e)| e.category() == cat).map(|(id, e)| (*id, e, element_json(m, *id))).collect();
    }
    let mut out = Vec::new();
    for (&id, e) in &m.elements {
        let Element::Mass(ms) = e else { continue };
        for (level, z, up) in m.mass_slices(ms) {
            let outline = m.mass_outline(ms, z);
            let (area, perimeter) = (crate::geom::area(&outline).abs(), crate::geom::perimeter(&outline));
            out.push((
                id,
                e,
                serde_json::json!({
                    "id": id, "mass": ms.name, "level": level, "area_m2": crate::commands::m2(area),
                    "perimeter_m": (perimeter / 10.0).round() / 100.0, "volume_m3": (m.mass_volume(ms, z, z + up) / 1e7).round() / 100.0,
                }),
            ));
        }
    }
    out
}

/// Schedule `sc`'s table: a column for each of its fields, a row for each element its filters keep,
/// sorted by its sort fields (by its first column unless it says; a phase field in the project's
/// order of phases) or, grouped, a row for each set of their values; its totals under them.
pub fn table(m: &Model, sc: &Schedule) -> Result<Table, String> {
    let cat = category(&sc.of);
    if own(&cat).1.is_empty() {
        return Err(format!("no schedule of `{}`; there are schedules of: {}", sc.of, categories().join(", ")));
    }
    let available = fields(m, &cat);
    let fields = chosen(sc, &cat);
    let formulas: BTreeMap<&str, (&str, Kind)> =
        fields.iter().filter_map(|f| Some((f.name.as_str(), (f.formula.as_deref()?, f.kind.unwrap_or_default())))).collect();
    let columns: Vec<Column> = fields
        .iter()
        .map(|f| Column {
            heading: f.heading.clone().unwrap_or_else(|| f.name.clone()),
            kind: match f.formula {
                Some(_) => f.kind.unwrap_or_default(),
                None => available.iter().find(|(n, _)| *n == f.name).map_or(Kind::Text, |(_, k)| *k),
            },
        })
        .collect();
    let kind_of = |n: &str| formulas.get(n).map(|(_, k)| *k).or_else(|| available.iter().find(|(a, _)| a == n).map(|(_, k)| *k));
    let sort: Vec<(&str, bool)> = match (sc.sort.is_empty(), sc.grouped, fields.first()) {
        (true, false, Some(first)) => vec![(first.name.as_str(), false)],
        _ => sc.sort.iter().map(|s| (s.field.as_str(), s.descending)).collect(),
    };
    // Each element its filters keep: its cells, its sort fields' values, and itself.
    let mut rows: Vec<(Vec<Value>, Vec<Value>, Id)> = Vec::new();
    for (id, e, json) in sources(m, &cat) {
        let mut row = Row { m, e, cat: &cat, json, formulas: &formulas, done: BTreeMap::new() };
        if sc.filters.iter().all(|f| keeps(f, &row.get(&f.field), kind_of(&f.field), m.units)) {
            let cells = fields.iter().map(|f| row.get(&f.name)).collect();
            rows.push((cells, sort.iter().map(|(n, _)| sorts_as(m, &cat, n, row.get(n))).collect(), id));
        }
    }
    rows.sort_by(|(_, a, _), (_, b, _)| {
        for ((_, down), (x, y)) in sort.iter().zip(a.iter().zip(b)) {
            let o = natural(x, y);
            if o != Ordering::Equal {
                return if *down { o.reverse() } else { o };
            }
        }
        Ordering::Equal
    });
    let summed = |f: &Field| f.total || (f.formula.is_none() && f.name == "Count");
    let totals = fields.iter().any(|f| f.total).then(|| {
        fields.iter().enumerate().map(|(i, f)| if f.total { sum(rows.iter().map(|(cells, ..)| &cells[i])) } else { Value::Null }).collect()
    });
    let (rows, ids) = if sc.grouped {
        let mut groups: Vec<(Vec<Vec<Value>>, Vec<Id>)> = Vec::new();
        let mut last: Option<&Vec<Value>> = None;
        for (cells, keys, id) in &rows {
            match (groups.last_mut(), last) {
                (Some((g, ids)), Some(k)) if k.iter().zip(keys).all(|(a, b)| natural(a, b) == Ordering::Equal) => {
                    g.push(cells.clone());
                    ids.push(*id);
                }
                _ => groups.push((vec![cells.clone()], vec![*id])),
            }
            last = Some(keys);
        }
        let ids = groups.iter().map(|(_, ids)| ids.clone()).collect();
        let rows = groups
            .into_iter()
            .map(|(g, _)| {
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let column = g.iter().filter_map(|cells| cells.get(i));
                        if summed(f) {
                            sum(column)
                        } else {
                            let mut column = column;
                            let first = column.next().cloned().unwrap_or(Value::Null);
                            if column.all(|v| natural(v, &first) == Ordering::Equal) { first } else { Value::Null }
                        }
                    })
                    .collect()
            })
            .collect();
        (rows, ids)
    } else {
        rows.into_iter().map(|(cells, _, id)| (cells, vec![id])).unzip()
    };
    Ok(Table { columns, rows, ids, totals })
}

/// What field `name` of category `cat` sorts and groups by, where a row's value for it is `v`: a phase, its place
/// in the project's order (a phase never taken down comes after every phase), not its name; the rest, itself.
fn sorts_as(m: &Model, cat: &str, name: &str, v: Value) -> Value {
    if !matches!(name, phasing::CREATED | phasing::DEMOLISHED) || !own_field(cat, name) {
        return v;
    }
    serde_json::json!(v.as_str().and_then(|p| phasing::index(m, p)).unwrap_or(m.phases.len()))
}

/// The numbers among `values`, summed.
fn sum<'a>(values: impl Iterator<Item = &'a Value>) -> Value {
    serde_json::json!(fine(values.filter_map(Value::as_f64).sum::<f64>()))
}

/// Whether filter `f` keeps a value `v` of a field holding `kind`: text in any case; numbers as the
/// schedule shows them in `units` (two that show the same are equal); no value only for
/// `has-no-value` and the filters saying "not".
fn keeps(f: &Filter, v: &Value, kind: Option<Kind>, units: Units) -> bool {
    let empty = v.is_null() || v.as_str() == Some("");
    let text = |v: &Value| cell(v).to_lowercase();
    let (a, b) = (text(v), text(&f.value));
    let numbers = (v.as_f64(), f.value.as_f64(), kind.filter(|k| *k != Kind::Text));
    let order = || match numbers {
        (Some(x), Some(y), Some(k)) if shown(units, k, &serde_json::json!(x)) == shown(units, k, &serde_json::json!(y)) => Ordering::Equal,
        (Some(x), Some(y), Some(_)) => x.total_cmp(&y),
        _ => natural(&Value::String(a.clone()), &Value::String(b.clone())),
    };
    let same = || match numbers {
        (Some(_), Some(_), Some(_)) => order() == Ordering::Equal,
        _ => a == b,
    };
    match f.op {
        Op::HasValue => !empty,
        Op::HasNoValue => empty,
        Op::NotEquals => empty || !same(),
        Op::NotContains => empty || !a.contains(&b),
        Op::NotBeginsWith => empty || !a.starts_with(&b),
        Op::NotEndsWith => empty || !a.ends_with(&b),
        _ if empty => false,
        Op::Equals => same(),
        Op::Contains => a.contains(&b),
        Op::BeginsWith => a.starts_with(&b),
        Op::EndsWith => a.ends_with(&b),
        Op::Greater => order() == Ordering::Greater,
        Op::GreaterOrEqual => order() != Ordering::Less,
        Op::Less => order() == Ordering::Less,
        Op::LessOrEqual => order() != Ordering::Greater,
    }
}

/// A column's heading as a schedule shows it in `units`: an area's says m² or ft².
pub fn heading(units: Units, column: &Column) -> String {
    match column.kind {
        Kind::Area => format!(ui_msg!("{} ({})"), column.heading, units.area(0.0).1),
        _ => column.heading.clone(),
    }
}

/// A cell as a schedule shows it in `units`: lengths as drawings show them; areas to two places,
/// so the column lines up; other numbers to the hundredth; yes or no as `Yes` or `No` (English,
/// for the caller to say in its language); text as it is.
pub fn shown(units: Units, kind: Kind, v: &Value) -> String {
    match (v, kind) {
        (Value::Bool(b), _) => (if *b { msg!("Yes") } else { msg!("No") }).into(),
        (v, Kind::Length) if v.is_number() => units.length(v.as_f64().unwrap_or(0.0)),
        (v, Kind::Area) if v.is_number() => format!("{:.2}", units.area(v.as_f64().unwrap_or(0.0) * 1e6).0),
        (Value::Number(x), _) => format!("{}", (x.as_f64().unwrap_or(0.0) * 100.0).round() / 100.0 + 0.0),
        (v, _) => cell(v),
    }
}

/// Two texts in the order `natural` puts them.
pub fn natural_text(a: &str, b: &str) -> Ordering {
    natural(&Value::String(a.into()), &Value::String(b.into()))
}

/// Numbers (and numeric text such as marks "2" and "10") by value first, then other text, in any
/// case first.
fn natural(a: &Value, b: &Value) -> Ordering {
    let key = |v: &Value| {
        let s = cell(v);
        (s.trim().parse::<f64>().ok(), s.to_lowercase(), s)
    };
    match (key(a), key(b)) {
        ((Some(x), _, _), (Some(y), _, _)) => x.total_cmp(&y),
        ((Some(_), _, _), (None, _, _)) => Ordering::Less,
        ((None, _, _), (Some(_), _, _)) => Ordering::Greater,
        ((None, x, s), (None, y, t)) => x.cmp(&y).then(s.cmp(&t)),
    }
}

fn cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// The table as CSV: its headings (in metric), its rows, then its totals. Text a spreadsheet would
/// take for a formula (`=`, `+`, `-`, `@` first) starts with a `'`.
pub fn to_csv(t: &Table) -> String {
    let quote = |s: String| if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s };
    let text = |s: String| if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s };
    let mut out = t.columns.iter().map(|c| quote(text(heading(Units::Millimetres, c)))).collect::<Vec<_>>().join(",");
    out.push('\n');
    for row in t.rows.iter().chain(&t.totals) {
        let cells: Vec<String> = row.iter().map(|v| quote(if v.is_string() { text(cell(v)) } else { cell(v) })).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// The table as a workbook of one sheet named `name`: its headings (in metric) in bold, its rows, then its totals. Numbers
/// go in as numbers, so the spreadsheet sums them; text as text, never a formula.
pub fn to_xlsx(t: &Table, name: &str) -> Vec<u8> {
    use crate::xlsx::Cell;
    let headings: Vec<String> = t.columns.iter().map(|c| heading(Units::Millimetres, c)).collect();
    let rows: Vec<Vec<Cell>> = t
        .rows
        .iter()
        .chain(&t.totals)
        .map(|row| {
            row.iter()
                .map(|v| match v {
                    Value::Number(x) => x.as_f64().map_or(Cell::Empty, Cell::Number),
                    Value::Null => Cell::Empty,
                    v => Cell::Text(cell(v)),
                })
                .collect()
        })
        .collect();
    crate::xlsx::workbook(name, &headings, &rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::json;

    fn house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               wall {"points": [[5000,0],[5000,6000]], "type": "Interior - 100mm"}
               room {"point": [6500, 3000], "name": "Kitchen"}
               room {"point": [2000, 3000], "name": "Living", "number": "10"}
               door {"at": [6000, 0], "mark": "D2"}
               door {"at": [2000, 0]}"#,
        )
        .unwrap();
        s
    }

    fn of(cat: &str) -> Schedule {
        Schedule { name: String::new(), of: cat.into(), ..Schedule::default() }
    }

    fn headings(t: &Table) -> Vec<String> {
        t.columns.iter().map(|c| heading(Units::Millimetres, c)).collect()
    }

    #[test]
    fn room_schedules_list_number_name_level_area_and_perimeter_in_number_order() {
        let t = table(&house().model, &of("rooms")).unwrap();
        assert_eq!(headings(&t), ["Number", "Name", "Level", "Area (m²)", "Perimeter"]);
        assert_eq!(
            t.rows,
            [
                vec![json!("1"), json!("Kitchen"), json!("Level 1"), json!(18.0), json!(18000.0)],
                vec![json!("10"), json!("Living"), json!("Level 1"), json!(30.0), json!(22000.0)],
            ]
        );
        assert!(t.totals.is_none());
    }

    #[test]
    fn door_schedules_read_sizes_from_the_type() {
        let t = table(&house().model, &of("Door")).unwrap();
        assert_eq!(headings(&t), ["Mark", "Type", "Width", "Height", "Level", "Wall"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][0], "1", "numbers before other marks");
        assert_eq!((&t.rows[1][0], &t.rows[1][2], &t.rows[1][3]), (&json!("D2"), &json!(900.0), &json!(2100.0)));
    }

    #[test]
    fn unknown_categories_name_the_schedules_there_are() {
        let e = table(&house().model, &of("clouds")).err().unwrap();
        assert!(e.contains("room") && e.contains("stair"), "{e}");
    }

    #[test]
    fn schedules_show_lengths_and_areas_in_the_project_units() {
        let (ft, m) = (Units::FeetInches, Units::Metres);
        let area = Column { heading: "Area".into(), kind: Kind::Area };
        assert_eq!((heading(ft, &area), shown(ft, Kind::Area, &json!(18.0))), ("Area (ft²)".into(), "193.75".into()));
        assert_eq!((heading(m, &area), shown(m, Kind::Area, &json!(18.0))), ("Area (m²)".into(), "18.00".into()));
        assert_eq!([shown(m, Kind::Length, &json!(900.0)), shown(ft, Kind::Length, &json!(900.0))], ["0.90", "2'-11 7/16\""]);
        assert_eq!([shown(ft, Kind::Text, &json!("D2")), shown(ft, Kind::Number, &json!(4))], ["D2", "4"]);
        assert_eq!([shown(m, Kind::YesNo, &json!(true)), shown(m, Kind::Number, &json!(2.0 / 3.0))], ["Yes", "0.67"]);
    }

    #[test]
    fn filters_compare_values_as_the_schedule_shows_them() {
        let (ft, feet) = (Units::FeetInches, Units::Feet);
        let filter = |op, value| Filter { field: String::new(), op, value };
        // 2'-11 7/16" typed is 900.1125 mm: a 900 mm door shows the same.
        assert!(keeps(&filter(Op::Equals, json!(900.1125)), &json!(900.0), Some(Kind::Length), ft));
        assert!(!keeps(&filter(Op::Equals, json!(910.0)), &json!(900.0), Some(Kind::Length), ft));
        // 18 m² shows 193.75 ft²; 193.75 typed is a hair under it.
        assert!(keeps(&filter(Op::LessOrEqual, json!(193.75 * 0.09290304)), &json!(18.0), Some(Kind::Area), feet));
        assert!(!keeps(&filter(Op::Less, json!(193.75 * 0.09290304)), &json!(18.0), Some(Kind::Area), feet));
    }

    #[test]
    fn csv_leaves_no_text_a_spreadsheet_would_take_for_a_formula() {
        let t = Table {
            columns: vec![Column { heading: "Name".into(), kind: Kind::Text }, Column { heading: "Offset".into(), kind: Kind::Length }],
            rows: vec![vec![json!("=1+1"), json!(-5.0)], vec![json!("@SUM(A1)"), json!(5.0)]],
            ids: Vec::new(),
            totals: None,
        };
        assert_eq!(to_csv(&t), "Name,Offset\n'=1+1,-5.0\n'@SUM(A1),5.0\n");
    }

    #[test]
    fn a_schedule_goes_to_a_workbook_with_its_numbers_as_numbers() {
        let t = table(&house().model, &of("rooms")).unwrap();
        let parts = crate::xlsx::unzip(&to_xlsx(&t, "Room Schedule"));
        assert!(parts["xl/workbook.xml"].contains(r#"name="Room Schedule""#));
        let sheet = &parts["xl/worksheets/sheet1.xml"];
        assert!(sheet.contains(r#"<t xml:space="preserve">Area (m²)</t>"#), "{sheet}");
        assert!(sheet.contains(r#"<t xml:space="preserve">Kitchen</t></is></c><c r="C2" t="inlineStr"><is><t xml:space="preserve">Level 1</t></is></c><c r="D2"><v>18</v></c><c r="E2"><v>18000</v></c>"#), "{sheet}");
    }

    #[test]
    fn csv_quotes_the_cells_that_need_it_and_ends_with_the_totals() {
        let column = |h: &str, kind| Column { heading: h.into(), kind };
        let t = Table {
            columns: vec![column("Name", Kind::Text), column("Area", Kind::Area)],
            rows: vec![vec![json!("A, B"), json!(1.5)], vec![json!("say \"hi\""), Value::Null]],
            ids: Vec::new(),
            totals: Some(vec![Value::Null, json!(1.5)]),
        };
        assert_eq!(to_csv(&t), "Name,Area (m²)\n\"A, B\",1.5\n\"say \"\"hi\"\"\",\n,1.5\n");
    }
}
