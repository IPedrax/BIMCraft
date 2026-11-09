//! Classification: the systems a project's elements are classified by (Uniclass 2015, OmniClass, UniFormat, MasterFormat,
//! NBR 15965, CoClass, CCI, NL-SfB), COBie's fields, the tables a user loads into the project, the parameters that hold each
//! element's or type's code, and the brief and answer through which an AI agent proposes codes.
//!
//! BIMCraft knows how each system looks, which tables it has and which parameters they fill, but it **ships no table**: their
//! publishers own them, and the user loads the edition they hold (`.csv` or `.xlsx`, columns by heading). A loaded table is
//! kept in the project, so it opens without the file. The systems' facts here come from their publishers' public
//! descriptions and are the contributor's: where a table's key or scope is unsure, its note says so.
//!
//! A table feeds parameters through *bindings*: `Uniclass Ss Number` and `Uniclass Ss Title` from the table `Uniclass Ss`,
//! or COBie's `COBie Type Category` from whichever table the user chose, written `code: title`. `classify` writes a code
//! only when the bound table holds it, and the title always comes from the table, never from the caller.
//!
//! Whose value a classification is was decided table by table ([`TableDef::scope`], with its reason in
//! [`TableDef::why`]): a product, a system or an element function is the same for every element of a type, so it's a type
//! parameter; a space's classification and COBie's component fields (a serial number, an installation date) are each
//! element's.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::commands::{bad, flag, only, string};
use crate::engine::Session;
use crate::ids::{Guid, Id};
use crate::library::Kind as LibKind;
use crate::model::{Element, MAX_TEXT, Model, ParameterKind, Scope};
use crate::parameters::{self, Choices, Definition, TYPELESS};

type R = Result<Value, String>;

/// The most tables a project holds.
pub const MAX_TABLES: usize = 64;
/// The most rows of one table.
pub const MAX_ITEMS: usize = 100_000;
/// The longest code.
pub const MAX_CODE: usize = 64;
/// The most rows a brief holds.
pub const MAX_BRIEF: usize = 20_000;
/// The most candidates a brief gives a category of one table.
pub const MAX_CANDIDATES: usize = 400;
/// The most assignments one `classify` makes.
const MAX_ASSIGN: usize = 100_000;
/// The longest reason an answer's row keeps.
pub const MAX_REASON: usize = 400;
/// How many refusals a message names.
const NAMED: usize = 12;

/// Every category that is built: what a classification of what something is can be for.
const BUILT: &[&str] = &[
    "wall",
    "floor",
    "ceiling",
    "roof",
    "door",
    "window",
    "column",
    "component",
    "stair",
    "ramp",
    "railing",
    "structuralcolumn",
    "footing",
    "beam",
    "stripfooting",
    "pipe",
    "duct",
    "conduit",
    "cabletray",
    "wire",
    "downconductor",
    "groundconductor",
    "cable",
];
/// Rooms: what a classification of spaces is for.
const SPACES: &[&str] = &["room"];
/// What COBie's Type and Component sheets list: the assets a facility manager keeps, not the walls and floors.
const ASSETS: &[&str] = &["door", "window", "component"];

/// One table of a classification system.
#[derive(Clone, Copy, Debug)]
pub struct TableDef {
    /// What the table is called within its system: `Ss`, `23`, `3E`. Empty for a system of one table.
    pub key: &'static str,
    pub name: &'static str,
    /// Whose value its code is.
    pub scope: Scope,
    /// The categories it is for by default; a type parameter leaves out what has no type.
    pub categories: &'static [&'static str],
    /// Why it is each element's or each type's.
    pub why: &'static str,
    /// A code of the table as its publisher writes them, for agents and the window; empty where none was checked against
    /// the published table.
    pub example: &'static str,
}

/// A field of COBie's, as a parameter.
#[derive(Clone, Copy, Debug)]
pub struct FieldDef {
    /// The COBie sheet: `Type`, `Component`, `Space`.
    pub sheet: &'static str,
    /// The column: `Manufacturer`, `SerialNumber`.
    pub column: &'static str,
    pub kind: ParameterKind,
    pub scope: Scope,
    pub categories: &'static [&'static str],
    /// Filled from a classification table, written `code: title`.
    pub classified: bool,
    /// The values it takes, when COBie lists them.
    pub picks: &'static [&'static str],
    pub description: &'static str,
}

impl FieldDef {
    /// The parameter it is: `COBie Type Manufacturer`.
    pub fn parameter(&self) -> String {
        format!("COBie {} {}", self.sheet, self.column)
    }
}

/// A classification system.
#[derive(Clone, Copy, Debug)]
pub struct SystemDef {
    /// What commands call it: `uniclass`, `omniclass`, `cobie`.
    pub id: &'static str,
    pub name: &'static str,
    /// What its parameters and tables are named with: `Uniclass`.
    pub short: &'static str,
    pub publisher: &'static str,
    pub region: &'static str,
    /// How it is got, and what to know about it.
    pub note: &'static str,
    pub tables: &'static [TableDef],
    /// COBie's fields; empty for a classification system.
    pub fields: &'static [FieldDef],
}

impl SystemDef {
    /// The table of `key`, in any case.
    pub fn table(&self, key: &str) -> Option<&'static TableDef> {
        self.tables.iter().find(|t| t.key.eq_ignore_ascii_case(key.trim()))
    }

    /// What the project calls its table `t`: `Uniclass Ss`, `UniFormat`.
    pub fn table_name(&self, t: &TableDef) -> String {
        if t.key.is_empty() { self.short.to_string() } else { format!("{} {}", self.short, t.key) }
    }

    /// The parameters table `t` fills: its number's and its title's.
    pub fn parameters_of(&self, t: &TableDef) -> (String, String) {
        let base = self.table_name(t);
        (format!("{base} Number"), format!("{base} Title"))
    }
}

const fn table(
    key: &'static str,
    name: &'static str,
    scope: Scope,
    categories: &'static [&'static str],
    why: &'static str,
    example: &'static str,
) -> TableDef {
    TableDef { key, name, scope, categories, why, example }
}

const WHY_TYPE: &str =
    "what it is (its product, system or function) is the same for every element of a type, so it's the type's: classify a type once";
const WHY_SPACE: &str = "each room is used its own way and rooms have no types here, so it's each room's";
const WHY_WORK_RESULT: &str = "a work result is what a piece of work leaves behind, so it fits a task, a specification or a budget item \
     best; with none of those here it's the type's, as the work its elements leave is, and `scopes` makes it each element's";
const WHY_UNIT: &str = "it classifies a unit (an apartment, a shop), not a room; with no units here it's each room's, and every room \
     of a unit carries the same code";
const WHY_NO_HOME: &str = "what it classifies (materials, properties, phases, services, disciplines, functions, equipment, \
     information) has no category of its own here: load the table, and name the categories it's for to make its parameters";
/// No category: a table whose subject isn't an element here, made only for the categories the user names.
const NONE: &[&str] = &[];

/// The systems BIMCraft knows. None of their tables is shipped: the user loads the edition they hold.
pub const SYSTEMS: &[SystemDef] = &[
    SystemDef {
        id: "uniclass",
        name: "Uniclass 2015",
        short: "Uniclass",
        publisher: "NBS",
        region: "United Kingdom (ISO 12006-2)",
        note: "Download the tables from NBS (uniclass.thenbs.com) as Excel; each table is a sheet of Code, Group, Sub group, Section, \
               Object and Title. Codes are pairs: Pr_30_59_24_30. Updated each quarter: load the edition the project follows.",
        tables: &[
            table("SL", "Spaces/locations", Scope::Instance, SPACES, WHY_SPACE, "SL_45_10_49"),
            table("EF", "Elements/functions", Scope::Type, BUILT, WHY_TYPE, "EF_25_10"),
            table("Ss", "Systems", Scope::Type, BUILT, WHY_TYPE, "Ss_25_10_30_35"),
            table("Pr", "Products", Scope::Type, BUILT, WHY_TYPE, "Pr_30_59_24_30"),
        ],
        fields: &[],
    },
    SystemDef {
        id: "omniclass",
        name: "OmniClass",
        short: "OmniClass",
        publisher: "CSI (Construction Specifications Institute)",
        region: "North America (ISO 12006-2)",
        note: "Get the tables from CSI (csiresources.org) as Excel; a row is a number and a title. Numbers are the table and pairs of \
               digits: 23-13 35 11 11. Table 23 is what most projects classify their types by.",
        tables: &[
            table("13", "Spaces by Function", Scope::Instance, SPACES, WHY_SPACE, "13-15 11 34 11"),
            table("21", "Elements", Scope::Type, BUILT, WHY_TYPE, "21-02 10 10"),
            table("22", "Work Results", Scope::Type, BUILT, WHY_TYPE, "22-08 14 16"),
            table("23", "Products", Scope::Type, BUILT, WHY_TYPE, "23-17 11 00"),
        ],
        fields: &[],
    },
    SystemDef {
        id: "uniformat",
        name: "UniFormat",
        short: "UniFormat",
        publisher: "CSI and CSC (UniFormat 2010); ASTM E1557 (UNIFORMAT II)",
        region: "North America",
        note: "Elements by their function: B2010 Exterior Walls. Load CSI's UniFormat or ASTM's UNIFORMAT II, whichever the project \
               follows; both are bought from their publishers. It fills what Revit calls an Assembly Code.",
        tables: &[table("", "Elements", Scope::Type, BUILT, WHY_TYPE, "B2010.10")],
        fields: &[],
    },
    SystemDef {
        id: "masterformat",
        name: "MasterFormat",
        short: "MasterFormat",
        publisher: "CSI and CSC",
        region: "North America",
        note: "Work results by specification section: 08 11 13 Hollow Metal Doors and Frames. Bought from CSI; load the edition the \
               specification is written to.",
        tables: &[table("", "Work Results", Scope::Type, BUILT, WHY_TYPE, "08 11 13")],
        fields: &[],
    },
    SystemDef {
        id: "nbr15965",
        name: "NBR 15965",
        short: "NBR 15965",
        publisher: "ABNT",
        region: "Brasil (ISO 12006-2)",
        note: "Sistema de classificação da informação da construção, em sete partes e treze tabelas de dois caracteres (0M, 3E, 3R, \
               4A…). As tabelas são da ABNT, compradas com a norma; carregue a edição que o projeto segue. Códigos em pares: \
               3E 20 02 02 02 00 00; as partes mais antigas imprimem menos pares, e 00 e o fim do código são o mesmo.",
        tables: &[
            table("0M", "Materiais", Scope::Type, NONE, WHY_NO_HOME, ""),
            table("0P", "Propriedades", Scope::Type, NONE, WHY_NO_HOME, ""),
            table("1F", "Fases", Scope::Instance, NONE, WHY_NO_HOME, ""),
            table("1S", "Serviços", Scope::Instance, NONE, WHY_NO_HOME, ""),
            table("1D", "Disciplinas", Scope::Instance, NONE, WHY_NO_HOME, ""),
            table("2N", "Funções", Scope::Type, NONE, WHY_NO_HOME, ""),
            table("2Q", "Equipamentos", Scope::Type, NONE, WHY_NO_HOME, ""),
            table("2C", "Produtos", Scope::Type, BUILT, WHY_TYPE, ""),
            table("3E", "Elementos", Scope::Type, BUILT, WHY_TYPE, "3E 20 02 02 02 00 00"),
            table("3R", "Resultados", Scope::Type, BUILT, WHY_WORK_RESULT, ""),
            table("4A", "Espaços", Scope::Instance, SPACES, WHY_SPACE, "4A 19 00 00 00 00 00"),
            table("4U", "Unidades", Scope::Instance, SPACES, WHY_UNIT, ""),
            table("5I", "Informação", Scope::Instance, NONE, WHY_NO_HOME, ""),
        ],
        fields: &[],
    },
    SystemDef {
        id: "coclass",
        name: "CoClass",
        short: "CoClass",
        publisher: "Svensk Byggtjänst",
        region: "Sverige (ISO 12006-2, IEC 81346)",
        note: "Tables of letter codes, licensed from Svensk Byggtjänst (coclass.byggtjanst.se). Load each table you use as code and \
               title; the table names here are the ISO 12006-2 ones, check them against your licence's.",
        tables: &[
            table("Spaces", "Utrymmen", Scope::Instance, SPACES, WHY_SPACE, "AFA"),
            table("Functional systems", "Funktionella system", Scope::Type, BUILT, WHY_TYPE, "AD"),
            table("Constructive systems", "Konstruktiva system", Scope::Type, BUILT, WHY_TYPE, "GBB"),
            table("Components", "Komponenter", Scope::Type, BUILT, WHY_TYPE, "QQC"),
        ],
        fields: &[],
    },
    SystemDef {
        id: "cci",
        name: "CCI",
        short: "CCI",
        publisher: "Molio (CCI, Construction Classification International)",
        region: "Danmark, Eesti (ISO 12006-2, IEC 81346)",
        note: "Letter codes by IEC 81346, licensed from Molio (ccischeme.org). Load each table you use as code and title.",
        tables: &[
            table("Spaces", "Spaces", Scope::Instance, SPACES, WHY_SPACE, "AA"),
            table("Functional systems", "Functional systems", Scope::Type, BUILT, WHY_TYPE, "A"),
            table("Technical systems", "Technical systems", Scope::Type, BUILT, WHY_TYPE, "AD"),
            table("Components", "Components", Scope::Type, BUILT, WHY_TYPE, "QQA"),
        ],
        fields: &[],
    },
    SystemDef {
        id: "nlsfb",
        name: "NL-SfB",
        short: "NL-SfB",
        publisher: "BIM Loket",
        region: "Nederland",
        note: "Elements by two to four digits: 21.12. Load the edition the project's BIM basis ILS asks for as code and title.",
        tables: &[table("", "Elementen", Scope::Type, BUILT, WHY_TYPE, "21.12")],
        fields: &[],
    },
    SystemDef {
        id: "cobie",
        name: "COBie 2.4",
        short: "COBie",
        publisher: "NIBS (NBIMS-US); BS 1192-4 in the UK",
        region: "International",
        note: "What a facility manager is handed: a type's make and warranty, each component's serial number and dates, each space's \
               use. COBie has no table of its own: its Category fields take the codes of a table you load from another system \
               (OmniClass 23 or Uniclass Pr for types, OmniClass 13 or Uniclass SL for spaces), written code: title.",
        tables: &[],
        fields: COBIE,
    },
];

const fn field(sheet: &'static str, column: &'static str, kind: ParameterKind, description: &'static str) -> FieldDef {
    let (scope, categories) = match sheet.as_bytes() {
        [b'T', ..] => (Scope::Type, ASSETS),
        [b'C', ..] => (Scope::Instance, ASSETS),
        _ => (Scope::Instance, SPACES),
    };
    FieldDef { sheet, column, kind, scope, categories, classified: false, picks: &[], description }
}

const fn classified(f: FieldDef) -> FieldDef {
    FieldDef { classified: true, ..f }
}

const fn picks(f: FieldDef, picks: &'static [&'static str]) -> FieldDef {
    FieldDef { picks, ..f }
}

use ParameterKind::{Length, Number, Text};

/// COBie 2.4's fields that are an element's or a type's. A Type sheet's are the type's (every element of a make and model
/// shares them); a Component sheet's are each element's: its serial number, tag, bar code, installation and warranty start
/// dates. Dates are text, `YYYY-MM-DD`.
pub const COBIE: &[FieldDef] = &[
    classified(field("Type", "Category", Text, "The type's classification, code: title, from the table chosen for it")),
    field("Type", "Description", Text, "What the type is, in words"),
    picks(field("Type", "AssetType", Text, "Whether it is fixed or moveable"), &["Fixed", "Moveable"]),
    field("Type", "Manufacturer", Text, "The maker's contact, as an e-mail address"),
    field("Type", "ModelNumber", Text, "The maker's model number"),
    field("Type", "WarrantyGuarantorParts", Text, "Who guarantees the parts, as an e-mail address"),
    field("Type", "WarrantyDurationParts", Number, "How long the parts are guaranteed, in the warranty's unit"),
    field("Type", "WarrantyGuarantorLabor", Text, "Who guarantees the labour, as an e-mail address"),
    field("Type", "WarrantyDurationLabor", Number, "How long the labour is guaranteed, in the warranty's unit"),
    picks(field("Type", "WarrantyDurationUnit", Text, "The unit of the warranties' durations"), &["Year", "Month", "Week", "Day"]),
    field("Type", "WarrantyDescription", Text, "What the warranties cover"),
    field("Type", "ReplacementCost", Number, "What one costs to replace"),
    field("Type", "ExpectedLife", Number, "How long one lasts, in the duration unit"),
    picks(field("Type", "DurationUnit", Text, "The unit of the expected life"), &["Year", "Month", "Week", "Day"]),
    field("Type", "NominalLength", Length, "Its nominal length"),
    field("Type", "NominalWidth", Length, "Its nominal width"),
    field("Type", "NominalHeight", Length, "Its nominal height"),
    field("Type", "ModelReference", Text, "The maker's name for the model"),
    field("Type", "Shape", Text, "Its shape"),
    field("Type", "Size", Text, "Its size"),
    field("Type", "Color", Text, "Its colour"),
    field("Type", "Finish", Text, "Its finish"),
    field("Type", "Grade", Text, "Its grade"),
    field("Type", "Material", Text, "What it is made of"),
    field("Type", "Constituents", Text, "Its parts"),
    field("Type", "Features", Text, "What else it has"),
    field("Type", "AccessibilityPerformance", Text, "How it serves people with disabilities"),
    field("Type", "CodePerformance", Text, "The codes it meets"),
    field("Type", "SustainabilityPerformance", Text, "How it performs environmentally"),
    field("Component", "Description", Text, "What this one is, in words"),
    field("Component", "SerialNumber", Text, "This one's serial number"),
    field("Component", "InstallationDate", Text, "The day it was installed, YYYY-MM-DD"),
    field("Component", "WarrantyStartDate", Text, "The day its warranty began, YYYY-MM-DD"),
    field("Component", "TagNumber", Text, "The tag on it"),
    field("Component", "BarCode", Text, "Its bar code"),
    field("Component", "AssetIdentifier", Text, "What the owner's asset register calls it"),
    classified(field("Space", "Category", Text, "The space's classification, code: title, from the table chosen for it")),
    field("Space", "Description", Text, "What the space is, in words"),
    field("Space", "RoomTag", Text, "The sign on its door"),
    field("Space", "UsableHeight", Length, "Its usable height"),
];

/// The system `id` or named `id`, in any case.
pub fn system(id: &str) -> Option<&'static SystemDef> {
    let id = id.trim();
    SYSTEMS.iter().find(|s| s.id.eq_ignore_ascii_case(id) || s.name.eq_ignore_ascii_case(id) || s.short.eq_ignore_ascii_case(id))
}

/// One row of a table: a code and its title.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub code: String,
    pub title: String,
    /// The code above it, where the file says; otherwise the longest code of the table it begins with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// A table loaded into the project.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// The system's id: `uniclass`; `custom` for one BIMCraft doesn't know.
    pub system: String,
    /// The table's key within its system: `Ss`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// The edition: `2024-07`, `v1.30`. A label of the user's own.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub edition: String,
    /// The file it was read from, as a name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    pub items: Vec<Item>,
}

impl Table {
    /// Its row of `code`, matched as written, then by its letters and digits alone.
    pub fn find(&self, code: &str) -> Option<&Item> {
        let code = code.trim();
        self.items.iter().find(|i| i.code == code).or_else(|| {
            let k = normal(code);
            (!k.is_empty()).then(|| self.items.iter().find(|i| normal(&i.code) == k)).flatten()
        })
    }

    /// Each row's parent, as an index: its own `parent`, or the longest other code its own begins with.
    pub fn parents(&self) -> Vec<Option<usize>> {
        let keys: Vec<String> = self.items.iter().map(|i| normal(&i.code)).collect();
        let by: HashMap<&str, usize> = keys.iter().enumerate().map(|(n, k)| (k.as_str(), n)).collect();
        self.items
            .iter()
            .enumerate()
            .map(|(n, item)| {
                if let Some(p) = &item.parent {
                    return by.get(normal(p).as_str()).copied().filter(|&q| q != n);
                }
                let k = &keys[n];
                (1..k.len()).rev().filter(|&len| k.is_char_boundary(len)).find_map(|len| by.get(&k[..len]).copied().filter(|&q| q != n))
            })
            .collect()
    }

    /// The table as a `.csv` of code, title and parent, for an agent to read.
    pub fn to_csv(&self) -> String {
        let cell = |t: &str| {
            if t.contains([',', '"', '\n', '\r']) { format!("\"{}\"", t.replace('"', "\"\"")) } else { t.to_string() }
        };
        let parents = self.parents();
        let mut out = String::from("Code,Title,Parent\n");
        for (item, p) in self.items.iter().zip(parents) {
            let parent = p.and_then(|q| self.items.get(q)).map_or("", |i| i.code.as_str());
            out.push_str(&format!("{},{},{}\n", cell(&item.code), cell(&item.title), cell(parent)));
        }
        out
    }

    fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("classification table `{name}`");
        if self.items.len() > MAX_ITEMS {
            return Err(format!("{who} has {} rows; a table holds up to {MAX_ITEMS}", self.items.len()));
        }
        for t in [&self.system, &self.key, &self.edition, &self.source] {
            if t.chars().count() > MAX_TEXT {
                return Err(format!("{who}: its system, key, edition and source hold up to {MAX_TEXT} characters"));
            }
        }
        let mut seen = BTreeSet::new();
        for i in &self.items {
            if i.code.trim().is_empty() || i.code.chars().count() > MAX_CODE {
                return Err(format!("{who}: a code is 1 to {MAX_CODE} characters, not `{}`", i.code));
            }
            if i.title.chars().count() > MAX_TEXT || i.parent.as_ref().is_some_and(|p| p.chars().count() > MAX_CODE) {
                return Err(format!("{who}: `{}`'s title holds up to {MAX_TEXT} characters", i.code));
            }
            if !seen.insert(i.code.as_str()) {
                return Err(format!("{who} has `{}` twice", i.code));
            }
        }
        Ok(())
    }
}

/// How a bound parameter writes a code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// The code alone, with its title in a parameter of its own.
    #[default]
    Code,
    /// `code: title`, as COBie's Category columns are.
    CodeTitle,
}

/// A parameter a table fills.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// The project parameter that holds the code.
    pub parameter: String,
    /// The project parameter that holds its title, written with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The table it takes codes from, by the name the project gives it: `Uniclass Ss`.
    pub table: String,
    #[serde(default, skip_serializing_if = "is_code")]
    pub form: Form,
}

fn is_code(f: &Form) -> bool {
    *f == Form::Code
}

/// The project's classification: its tables and what they fill.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classification {
    /// By the name the project gives each: `Uniclass Ss`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tables: BTreeMap<String, Table>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,
}

impl Classification {
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.bindings.is_empty()
    }

    /// What [`Model::check`] asks of it.
    pub fn check(&self) -> Result<(), String> {
        if self.tables.len() > MAX_TABLES {
            return Err(format!("the project holds {} classification tables; it holds up to {MAX_TABLES}", self.tables.len()));
        }
        for (n, t) in &self.tables {
            if n.trim().is_empty() || n.chars().count() > MAX_TEXT {
                return Err(format!("a classification table's name is 1 to {MAX_TEXT} characters"));
            }
            t.check(n)?;
        }
        let mut seen = BTreeSet::new();
        for b in &self.bindings {
            if !seen.insert(b.parameter.as_str()) {
                return Err(format!("parameter `{}` is bound to two classification tables; it takes codes from one", b.parameter));
            }
        }
        Ok(())
    }

    /// The binding of parameter `name`.
    pub fn binding(&self, name: &str) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.parameter == name)
    }

    /// The names of library entries it holds, to rename or turn into GUIDs.
    pub(crate) fn lib_refs_mut(&mut self, f: &mut dyn FnMut(&LibKind, &mut String)) {
        for b in &mut self.bindings {
            f(&LibKind::Parameter, &mut b.parameter);
            if let Some(t) = b.title.as_mut() {
                f(&LibKind::Parameter, t);
            }
            f(&LibKind::Classification, &mut b.table);
        }
    }
}

/// A code's letters and digits, upper case: what two ways of writing one code share (`Ss_25_10` and `SS 25 10`). Groups
/// of zeros at its end are taken off, so `23-13 00 00` is `2313`.
pub fn normal(code: &str) -> String {
    let groups: Vec<String> = code.split(|c: char| !c.is_alphanumeric()).filter(|g| !g.is_empty()).map(|g| g.to_uppercase()).collect();
    let keep = groups.iter().rposition(|g| !g.chars().all(|c| c == '0')).map_or(0, |p| p + 1);
    groups[..keep.max(1).min(groups.len())].concat()
}

/// What a table's file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadTable {
    pub items: Vec<Item>,
    pub left_out: Vec<String>,
}

/// The headings a table's code is under, in the languages of the systems.
const CODE_HEADINGS: &[&str] =
    &["code", "number", "nummer", "ref", "reference", "codigo", "código", "kod", "kode", "notation", "id", "classification code"];
/// The headings its title is under.
const TITLE_HEADINGS: &[&str] = &[
    "title",
    "description",
    "name",
    "titel",
    "titulo",
    "título",
    "descricao",
    "descrição",
    "nome",
    "benamning",
    "benämning",
    "betegnelse",
    "omschrijving",
    "designation",
    "term",
    "level title",
];
const PARENT_HEADINGS: &[&str] = &["parent", "parent code", "parent number", "pai", "codigo pai", "código pai", "överordnad"];

fn heading(cell: &str) -> String {
    cell.trim().trim_start_matches('\u{feff}').to_lowercase().replace(['_', '-'], " ")
}

/// A table from its rows: columns by heading (code, title, parent, in the systems' languages), or, with none known, code
/// and title as its first two. A row with only a code is passed over; one whose title is in a later column (Uniclass's,
/// after Group, Sub group…) is found by heading. `prefix` keeps only the codes of one table of a file that holds several.
pub fn table_from_rows(rows: &[Vec<String>], prefix: Option<&str>) -> ReadTable {
    let mut out = ReadTable::default();
    let Some(head_at) = rows.iter().position(|r| r.iter().any(|c| !c.trim().is_empty())) else { return out };
    let head: Vec<String> = rows[head_at].iter().map(|c| heading(c)).collect();
    let find = |names: &[&str]| head.iter().position(|h| names.contains(&h.as_str()));
    let (code, title, parent, start) = match (find(CODE_HEADINGS), find(TITLE_HEADINGS)) {
        (Some(c), Some(t)) => (c, t, find(PARENT_HEADINGS), head_at + 1),
        _ => (0, 1, None, head_at),
    };
    let prefix = prefix.map(normal).filter(|p| !p.is_empty());
    let mut seen = BTreeSet::new();
    for (n, row) in rows.iter().enumerate().skip(start) {
        let cell = |i: usize| row.get(i).map_or("", |c| c.trim());
        let (c, t) = (cell(code), cell(title));
        if c.is_empty() && t.is_empty() {
            continue;
        }
        if c.is_empty() || c.chars().count() > MAX_CODE {
            leave_out(&mut out, format!("row {}: `{c}` isn't a code of 1 to {MAX_CODE} characters", n + 1));
            continue;
        }
        if prefix.as_ref().is_some_and(|p| !normal(c).starts_with(p.as_str())) {
            continue;
        }
        if !seen.insert(c.to_string()) {
            leave_out(&mut out, format!("row {}: `{c}` again", n + 1));
            continue;
        }
        if out.items.len() >= MAX_ITEMS {
            leave_out(&mut out, format!("rows after {MAX_ITEMS} codes"));
            break;
        }
        let parent = parent.map(cell).filter(|p| !p.is_empty() && *p != c && p.chars().count() <= MAX_CODE).map(String::from);
        out.items.push(Item { code: c.to_string(), title: t.chars().take(MAX_TEXT).collect(), parent });
    }
    out
}

fn leave_out(r: &mut ReadTable, why: String) {
    if r.left_out.len() < 200 {
        r.left_out.push(why);
    }
}

/// A table's file: `.xlsx` (its first sheet) or `.csv` (comma, semicolon or tab).
pub fn read_table_file(path: &str, prefix: Option<&str>) -> Result<ReadTable, String> {
    let bytes = parameters::table::bytes_of(path)?;
    let lower = path.to_lowercase();
    let rows = if lower.ends_with(".xlsx") || bytes.starts_with(b"PK") {
        crate::xlsx::read(&bytes)?
    } else {
        let text = parameters::table::decode(&bytes);
        if text.lines().next().is_some_and(|l| l.contains('\t') && !l.contains(',') && !l.contains(';')) {
            text.lines().map(|l| l.split('\t').map(|c| c.trim().to_string()).collect()).collect()
        } else {
            parameters::table::read_csv(&text)
        }
    };
    let read = table_from_rows(&rows, prefix);
    if read.items.is_empty() {
        return Err(format!("{path} holds no codes: a table is a column of codes (Code) and one of titles (Title)"));
    }
    Ok(read)
}

/// The GUID of the parameter `name` a system makes: the same in every project, so their schedules and IFC line up.
pub fn parameter_guid(name: &str) -> Guid {
    Guid::seeded(fnv("bimcraft classification parameters"), fnv(name))
}

fn fnv(t: &str) -> u64 {
    t.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn type_categories(cats: &[&str]) -> Vec<String> {
    cats.iter().filter(|c| !TYPELESS.contains(c)).map(|c| c.to_string()).collect()
}

/// The parameters a system makes, as a dictionary's definitions, and the bindings that tie them to tables. `tables` picks
/// the tables (all, when empty), `scopes` and `categories` override a table's by its key, `fields` takes COBie's in and
/// `cobie` names the tables its Category fields take their codes from, by sheet.
pub struct Plan {
    pub definitions: Vec<Definition>,
    pub bindings: Vec<Binding>,
    pub notes: Vec<String>,
}

/// What [`plan`] is asked for.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    pub system: String,
    #[serde(default)]
    pub tables: Vec<String>,
    #[serde(default)]
    pub scopes: BTreeMap<String, Scope>,
    #[serde(default)]
    pub categories: BTreeMap<String, Vec<String>>,
    /// COBie: its Category fields' tables, by sheet (`Type`, `Space`).
    #[serde(default)]
    pub cobie: BTreeMap<String, String>,
    /// COBie: which sheets to make (`Type`, `Component`, `Space`); all when empty.
    #[serde(default)]
    pub sheets: Vec<String>,
}

/// The parameters and bindings `ask` makes.
pub fn plan(ask: &Ask) -> Result<Plan, String> {
    let c = "classification_parameters";
    let sys = system(&ask.system).ok_or_else(|| bad(c, format!("there's no system `{}`; there are {}", ask.system, ids())))?;
    let mut out = Plan { definitions: Vec::new(), bindings: Vec::new(), notes: Vec::new() };
    let wanted: Vec<&TableDef> = if ask.tables.is_empty() {
        sys.tables.iter().collect()
    } else {
        ask.tables
            .iter()
            .map(|k| sys.table(k).ok_or_else(|| bad(c, format!("{} has no table `{k}`; it has {}", sys.name, keys(sys)))))
            .collect::<Result<_, _>>()?
    };
    let categories_for = |key: &str, scope: Scope, default: &[&str]| -> Result<Vec<String>, String> {
        let cats: Vec<String> = match ask.categories.get(key) {
            Some(list) => list.iter().map(|k| crate::commands::category(k)).collect(),
            None => default.iter().map(|s| s.to_string()).collect(),
        };
        if let Some(k) = cats.iter().find(|k| !crate::model::PARAMETER_CATEGORIES.contains(&k.as_str())) {
            return Err(bad(c, format!("`{k}` isn't a category it can be for")));
        }
        let cats: Vec<String> = if scope == Scope::Type {
            let refs: Vec<&str> = cats.iter().map(String::as_str).collect();
            type_categories(&refs)
        } else {
            cats
        };
        if cats.is_empty() {
            return Err(bad(c, format!("`{key}` is for no category: name the categories it's for in `categories`")));
        }
        Ok(cats)
    };
    for t in wanted {
        if ask.tables.is_empty() && t.categories.is_empty() && !ask.categories.contains_key(t.key) {
            out.notes.push(format!(
                "{} ({}) is for no category here: name its categories in `categories` to make its parameters.",
                sys.table_name(t),
                t.name
            ));
            continue;
        }
        let scope = ask.scopes.get(t.key).copied().unwrap_or(t.scope);
        let cats = categories_for(t.key, scope, t.categories)?;
        let note = "Stairs and ramps have no types here, so the tables classified by type leave them out.";
        if scope == Scope::Type && cats.len() < t.categories.len() && !out.notes.iter().any(|n| n == note) {
            out.notes.push(note.to_string());
        }
        let (number, title) = sys.parameters_of(t);
        let group = format!("{} {}", sys.name, t.name);
        for (n, what) in [(&number, "number"), (&title, "title")] {
            out.definitions.push(Definition {
                guid: Some(parameter_guid(n)),
                name: n.clone(),
                kind: ParameterKind::Text,
                description: format!("The {what} of its {} {} classification", sys.name, t.name),
                group: group.clone(),
                read_only: Some(false),
                scope: Some(scope),
                categories: cats.clone(),
            });
        }
        out.bindings.push(Binding { parameter: number, title: Some(title), table: sys.table_name(t), form: Form::Code });
    }
    let sheets: Vec<String> = ask.sheets.iter().map(|s| s.trim().to_lowercase()).collect();
    for f in sys.fields.iter().filter(|f| sheets.is_empty() || sheets.contains(&f.sheet.to_lowercase())) {
        let n = f.parameter();
        let scope = ask.scopes.get(f.sheet).copied().unwrap_or(f.scope);
        let cats = categories_for(f.sheet, scope, f.categories)?;
        out.definitions.push(Definition {
            guid: Some(parameter_guid(&n)),
            name: n.clone(),
            kind: f.kind,
            description: f.description.to_string(),
            group: format!("COBie {}", f.sheet),
            read_only: Some(false),
            scope: Some(scope),
            categories: cats,
        });
        if f.classified {
            match ask.cobie.get(f.sheet).or_else(|| ask.cobie.get(&f.sheet.to_lowercase())) {
                Some(t) => out.bindings.push(Binding { parameter: n, title: None, table: t.trim().to_string(), form: Form::CodeTitle }),
                None => out.notes.push(format!("{n}: no table named for it in `cobie`; it's a plain parameter until one is")),
            }
        }
    }
    if let Some(k) = ask.cobie.keys().find(|k| !sys.fields.iter().any(|f| f.classified && f.sheet.eq_ignore_ascii_case(k))) {
        return Err(bad(c, format!("`cobie` names the tables of COBie's Type and Space Category, not `{k}`")));
    }
    Ok(out)
}

/// Whether table `t`'s codes begin with its key (`Ss_25`, `3E 20`), so a file of all the system's tables is cut by it: its
/// example says so, and a table with no example is taken to (a prefix that leaves nothing is taken back).
fn prefixed(t: &TableDef) -> bool {
    !t.key.is_empty() && !t.key.contains(' ') && (t.example.is_empty() || t.example.contains(t.key))
}

fn ids() -> String {
    SYSTEMS.iter().map(|s| s.id).collect::<Vec<_>>().join(", ")
}

fn keys(s: &SystemDef) -> String {
    s.tables.iter().map(|t| if t.key.is_empty() { "\"\"" } else { t.key }).collect::<Vec<_>>().join(", ")
}

fn system_json(s: &SystemDef) -> Value {
    json!({
        "id": s.id, "name": s.name, "publisher": s.publisher, "region": s.region, "note": s.note,
        "tables": s.tables.iter().map(|t| {
            let (number, title) = s.parameters_of(t);
            json!({"key": t.key, "name": t.name, "table": s.table_name(t), "scope": t.scope, "categories": type_or_all(t),
                   "why": t.why, "example": t.example, "parameters": [number, title]})
        }).collect::<Vec<_>>(),
        "fields": s.fields.iter().map(|f| json!({
            "parameter": f.parameter(), "sheet": f.sheet, "column": f.column, "kind": f.kind, "scope": f.scope,
            "categories": f.categories, "classified": f.classified, "picks": f.picks, "description": f.description,
        })).collect::<Vec<_>>(),
    })
}

fn type_or_all(t: &TableDef) -> Vec<String> {
    if t.scope == Scope::Type { type_categories(t.categories) } else { t.categories.iter().map(|s| s.to_string()).collect() }
}

/// The `classification_systems` command: the systems BIMCraft knows, their tables and parameters, and what the project holds.
pub(crate) fn systems(s: &mut Session, p: &Value) -> R {
    let c = "classification_systems";
    only(c, p, &["system"])?;
    let list: Vec<Value> = match string(c, p, "system")? {
        Some(id) => vec![system_json(system(&id).ok_or_else(|| bad(c, format!("there's no system `{id}`; there are {}", ids())))?)],
        None => SYSTEMS.iter().map(system_json).collect(),
    };
    let m = &s.model;
    let tables: Vec<Value> = m
        .classification
        .tables
        .iter()
        .map(|(n, t)| json!({"table": n, "system": t.system, "key": t.key, "edition": t.edition, "source": t.source, "codes": t.items.len()}))
        .collect();
    Ok(json!({"systems": list, "loaded": tables, "bindings": m.classification.bindings}))
}

/// What a newer table changes of the one loaded.
fn changes(was: Option<&Table>, now: &[Item]) -> Value {
    let Some(was) = was else { return json!({"added": now.len(), "removed": 0, "retitled": 0}) };
    let old: HashMap<&str, &str> = was.items.iter().map(|i| (i.code.as_str(), i.title.as_str())).collect();
    let new: HashMap<&str, &str> = now.iter().map(|i| (i.code.as_str(), i.title.as_str())).collect();
    let removed: Vec<&str> = was.items.iter().map(|i| i.code.as_str()).filter(|c| !new.contains_key(c)).collect();
    let retitled = now.iter().filter(|i| old.get(i.code.as_str()).is_some_and(|t| *t != i.title)).count();
    let added = now.iter().filter(|i| !old.contains_key(i.code.as_str())).count();
    json!({"added": added, "removed": removed.len(), "retitled": retitled, "removed_codes": removed.iter().take(50).collect::<Vec<_>>()})
}

/// The `classification_table` command: a table read into the project from its file, or taken out.
pub(crate) fn load_table(s: &mut Session, p: &Value) -> R {
    let c = "classification_table";
    only(c, p, &["path", "system", "table", "name", "edition", "prefix", "remove", "dry_run"])?;
    if flag(c, p, "remove")? {
        let name = string(c, p, "name")?.ok_or_else(|| bad(c, "`name` is the table to take out"))?;
        let m = s.model_mut();
        if m.classification.tables.remove(&name).is_none() {
            return Err(bad(c, format!("there's no table `{name}`")));
        }
        return Ok(json!({"removed": name}));
    }
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path` is the table's .csv or .xlsx"))?;
    let sys_id = string(c, p, "system")?.unwrap_or_else(|| "custom".to_string());
    let sys = system(&sys_id);
    if sys.is_none() && !sys_id.eq_ignore_ascii_case("custom") {
        return Err(bad(c, format!("there's no system `{sys_id}`; there are {}, or custom for a table of your own", ids())));
    }
    let key = string(c, p, "table")?.unwrap_or_default();
    let def = match sys {
        Some(sys) if !sys.tables.is_empty() => {
            Some(sys.table(&key).ok_or_else(|| bad(c, format!("{} has no table `{key}`; it has {}", sys.name, keys(sys))))?)
        }
        Some(sys) => {
            return Err(bad(c, format!("{} has no tables of its own: load another system's table for its Category fields", sys.name)));
        }
        None => None,
    };
    let name = match (string(c, p, "name")?, sys, def) {
        (Some(n), ..) => n.trim().to_string(),
        (None, Some(sys), Some(t)) => sys.table_name(t),
        _ => return Err(bad(c, "`name` is required for a custom table")),
    };
    if name.is_empty() {
        return Err(bad(c, "`name` is required"));
    }
    // A file that holds every table of a system (Uniclass's in one sheet) is cut down to the one asked for.
    let prefix = match string(c, p, "prefix")? {
        Some(x) => Some(x),
        None => def.filter(|t| prefixed(t)).map(|t| t.key.to_string()),
    };
    let mut read = read_table_file(&path, prefix.as_deref()).map_err(|e| bad(c, e))?;
    // A prefix guessed from the table's key that leaves nothing is taken back: the file's codes are written another way.
    if read.items.is_empty() && string(c, p, "prefix")?.is_none() {
        read = read_table_file(&path, None).map_err(|e| bad(c, e))?;
    }
    let was = s.model.classification.tables.get(&name);
    let changed = changes(was, &read.items);
    let report = json!({"table": name, "codes": read.items.len(), "left_out": read.left_out, "changes": changed,
                        "first": read.items.iter().take(5).collect::<Vec<_>>()});
    if flag(c, p, "dry_run")? {
        return Ok(report);
    }
    if was.is_none() && s.model.classification.tables.len() >= MAX_TABLES {
        return Err(bad(c, format!("a project holds up to {MAX_TABLES} tables")));
    }
    let edition = string(c, p, "edition")?.unwrap_or_default();
    let source = std::path::Path::new(&path).file_name().and_then(|f| f.to_str()).unwrap_or_default().chars().take(MAX_TEXT).collect();
    let table = Table {
        system: sys.map_or("custom", |s| s.id).to_string(),
        key: def.map_or(key, |t| t.key.to_string()),
        edition,
        source,
        items: read.items,
    };
    // Its id and GUID come with the library's next sync.
    s.model_mut().classification.tables.insert(name, table);
    Ok(report)
}

/// The `classification_parameters` command: a system's parameters made in the project (with the same GUIDs in every
/// project) and bound to its tables.
pub(crate) fn make_parameters(s: &mut Session, p: &Value) -> R {
    let c = "classification_parameters";
    let mut q = p.clone();
    let dry_run = flag(c, p, "dry_run")?;
    if let Some(o) = q.as_object_mut() {
        o.remove("dry_run");
    }
    let ask: Ask = serde_json::from_value(q).map_err(|e| bad(c, e))?;
    let made = plan(&ask)?;
    let choices =
        Choices { scopes: made.definitions.iter().filter_map(|d| Some((d.name.clone(), d.scope?))).collect(), ..Choices::default() };
    let rows = parameters::plan(&s.model, &made.definitions, &choices);
    let summary: Vec<Value> = rows
        .iter()
        .map(|r| json!({"name": r.here, "status": r.status, "scope": r.scope, "categories": r.categories, "notes": r.notes}))
        .collect();
    if dry_run {
        return Ok(json!({"parameters": summary, "bindings": made.bindings, "notes": made.notes}));
    }
    if let Some(r) = rows.iter().find(|r| r.status == "conflict") {
        return Err(bad(c, format!("`{}`: {}", r.name, r.notes.join("; "))));
    }
    parameters::apply(s, &rows, None, "").map_err(|e| e.replace("parameter_import", c))?;
    let m = s.model_mut();
    for b in made.bindings.iter() {
        m.classification.bindings.retain(|x| x.parameter != b.parameter);
        m.classification.bindings.push(b.clone());
    }
    Ok(json!({"parameters": summary, "bindings": made.bindings, "notes": made.notes}))
}

/// Whose value a binding's parameter is, and what it is for.
fn param_of<'a>(m: &'a Model, b: &Binding) -> Option<&'a crate::model::ProjectParameter> {
    m.parameters.get(&b.parameter)
}

/// What a target names: an element, or a type (its library entry).
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Element(Id),
    Type(Guid),
}

/// The target `key` names: an element's id or GUID, or a type's GUID.
pub fn target(m: &Model, key: &str) -> Option<Target> {
    let g = Guid::parse(key)?;
    if let Some(id) = g.pending() {
        return m.elements.contains_key(id).then_some(Target::Element(id));
    }
    if let Some(id) = m.elements.id(g) {
        return Some(Target::Element(id));
    }
    let (_, e) = m.library.by_guid(g)?;
    matches!(e.kind, LibKind::Type(_) | LibKind::FamilyType).then_some(Target::Type(g))
}

/// The category of parameters a type's entry is for.
fn type_category(m: &Model, g: Guid) -> Option<String> {
    let (_, e) = m.library.by_guid(g)?;
    parameters::category_of_type(m, &e.kind, &e.name)
}

/// A type's name as people read it: `Basic Wall 200` or `Door: 900 x 2100`.
pub fn type_label(m: &Model, g: Guid) -> String {
    m.library.by_guid(g).map_or_else(|| g.to_string(), |(_, e)| e.name.clone())
}

/// What one assignment writes: the parameter, whose (an element's or a type's), and the value.
#[derive(Clone, Debug)]
struct Write {
    parameter: String,
    owner: Target,
    value: Option<Value>,
}

/// One assignment as `classify` takes it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assign {
    /// An element's id or GUID, or a type's GUID.
    pub target: String,
    /// The bound parameter, or the table whose parameters to fill.
    #[serde(default)]
    pub parameter: Option<String>,
    #[serde(default)]
    pub table: Option<String>,
    /// The code; null takes it away.
    #[serde(default)]
    pub code: Option<String>,
}

/// The writes `a` makes, or why it can't.
fn writes(m: &Model, a: &Assign) -> Result<(Vec<Write>, Value), String> {
    let t = target(m, &a.target).ok_or_else(|| format!("`{}` is no element or type", a.target))?;
    let bindings: Vec<&Binding> = match (&a.parameter, &a.table) {
        (Some(p), _) => vec![m.classification.binding(p).ok_or_else(|| format!("`{p}` takes no codes from a table"))?],
        (None, Some(tb)) => {
            let found: Vec<&Binding> = m.classification.bindings.iter().filter(|b| b.table == *tb).collect();
            if found.is_empty() {
                return Err(format!("no parameter takes codes from `{tb}`: load the system's parameters first"));
            }
            found
        }
        (None, None) => return Err("say the `table` or the `parameter`".into()),
    };
    let mut out = Vec::new();
    let mut shown = json!(null);
    for b in bindings {
        let table = m.classification.tables.get(&b.table).ok_or_else(|| format!("table `{}` isn't loaded", b.table))?;
        let item = match &a.code {
            Some(code) => Some(table.find(code).ok_or_else(|| format!("`{code}` isn't in {}", b.table))?),
            None => None,
        };
        let q = param_of(m, b).ok_or_else(|| format!("there's no parameter `{}`", b.parameter))?;
        let (owner, cat) = match (&t, q.scope) {
            (Target::Element(id), Scope::Instance) => (Target::Element(*id), m.elements.get(*id).map(|e| e.category().to_string())),
            (Target::Element(id), Scope::Type) => {
                let e = m.elements.get(*id).ok_or("no element")?;
                let g = parameters::type_guid(m, e).ok_or_else(|| format!("element {id} has no type for `{}`", b.parameter))?;
                (Target::Type(g), Some(e.category().to_string()))
            }
            (Target::Type(g), Scope::Type) => (Target::Type(*g), type_category(m, *g)),
            (Target::Type(_), Scope::Instance) => {
                return Err(format!("`{}` is each element's: name an element, not a type", b.parameter));
            }
        };
        let cat = cat.unwrap_or_default();
        if !q.categories.contains(&cat) {
            return Err(format!("`{}` isn't for {cat}s", b.parameter));
        }
        let value = item.map(|i| match b.form {
            Form::Code => Value::String(i.code.clone()),
            Form::CodeTitle => Value::String(format!("{}: {}", i.code, i.title).chars().take(MAX_TEXT).collect()),
        });
        out.push(Write { parameter: b.parameter.clone(), owner: owner.clone(), value });
        if let Some(tp) = &b.title {
            out.push(Write { parameter: tp.clone(), owner: owner.clone(), value: item.map(|i| Value::String(i.title.clone())) });
        }
        shown = json!({"code": item.map(|i| &i.code), "title": item.map(|i| &i.title)});
    }
    Ok((out, shown))
}

fn owner_json(m: &Model, o: &Target) -> Value {
    match o {
        Target::Element(id) => json!({"element": id}),
        Target::Type(g) => json!({"type": type_label(m, *g), "guid": g}),
    }
}

/// The `classify` command: codes written to elements or their types, from the tables their parameters are bound to.
pub(crate) fn classify(s: &mut Session, p: &Value) -> R {
    let c = "classify";
    only(c, p, &["assign", "dry_run"])?;
    let list: Vec<Assign> = p
        .get("assign")
        .cloned()
        .ok_or_else(|| bad(c, "`assign` is a list of {target, table or parameter, code}"))
        .and_then(|v| serde_json::from_value(v).map_err(|e| bad(c, format!("`assign`: {e}"))))?;
    if list.len() > MAX_ASSIGN {
        return Err(bad(c, format!("up to {MAX_ASSIGN} at once")));
    }
    let mut synced = (*s.model).clone();
    synced.sync_library();
    let mut all = Vec::new();
    let mut rows = Vec::new();
    let mut refused = Vec::new();
    for (n, a) in list.iter().enumerate() {
        match writes(&synced, a) {
            Ok((w, shown)) => {
                rows.push(json!({"target": a.target, "owner": w.first().map(|x| owner_json(&synced, &x.owner)), "set": shown}));
                all.extend(w);
            }
            Err(e) => refused.push(format!("{}: {e}", n + 1)),
        }
    }
    if flag(c, p, "dry_run")? {
        return Ok(json!({"rows": rows, "refused": refused}));
    }
    if !refused.is_empty() {
        let named: Vec<&String> = refused.iter().take(NAMED).collect();
        return Err(bad(
            c,
            format!("{} can't be written: {}", refused.len(), named.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" | ")),
        ));
    }
    *s.model_mut() = synced;
    let m = s.model_mut();
    for w in all {
        match w.owner {
            Target::Element(id) => {
                if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                    match w.value {
                        Some(v) => params.insert(w.parameter, v),
                        None => params.remove(&w.parameter),
                    };
                }
            }
            Target::Type(g) => {
                if let Some(q) = m.parameters.get_mut(&w.parameter) {
                    match w.value {
                        Some(v) => q.type_values.insert(g, v),
                        None => q.type_values.remove(&g),
                    };
                }
            }
        }
    }
    Ok(json!({"rows": rows}))
}

/// What an element or a type holds of a binding's parameter.
pub fn current<'a>(m: &'a Model, b: &Binding, t: &Target) -> Option<&'a str> {
    let q = m.parameters.get(&b.parameter)?;
    let v = match (t, q.scope) {
        (Target::Element(id), Scope::Instance) => m.elements.get(*id)?.params()?.get(&b.parameter),
        (Target::Element(id), Scope::Type) => q.type_values.get(&parameters::type_guid(m, m.elements.get(*id)?)?),
        (Target::Type(g), Scope::Type) => q.type_values.get(g),
        (Target::Type(_), Scope::Instance) => None,
    }?;
    let text = v.as_str()?;
    Some(match b.form {
        Form::Code => text,
        Form::CodeTitle => text.split_once(':').map_or(text, |(c, _)| c.trim()),
    })
}

/// The elements a binding's parameter is for: what classifying it covers.
fn elements_for<'a>(m: &'a Model, b: &Binding) -> Vec<(Id, &'a Element)> {
    let Some(q) = m.parameters.get(&b.parameter) else { return Vec::new() };
    m.elements
        .iter()
        .filter(|(_, e)| q.categories.iter().any(|c| c == e.category()))
        .filter(|(_, e)| q.scope == Scope::Instance || parameters::type_guid(m, e).is_some())
        .map(|(id, e)| (*id, e))
        .collect()
}

/// One row of a brief: an element, or a type with what its elements share.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// The GUID of the element or the type: what an answer names it by.
    pub key: String,
    pub category: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_name: String,
    /// A room's name and number, or another element's mark.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<String>,
    /// What hosts it: `wall: Basic Wall 200`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// Its sizes, in mm: `thickness 200`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<String>,
    /// How many elements it stands for.
    pub count: usize,
    /// The tables it is asked for, and what it holds of each now.
    pub tables: BTreeMap<String, Option<String>>,
    /// The elements it stands for, their ids: for the window to select.
    #[serde(skip)]
    pub ids: Vec<Id>,
}

/// A brief: what an agent is given to classify.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Brief {
    pub tables: Vec<Value>,
    pub rows: Vec<Row>,
    /// For each table, for each category: the codes most like what the rows are.
    pub candidates: BTreeMap<String, BTreeMap<String, Vec<(String, String)>>>,
}

/// What a brief is made of.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BriefAsk {
    /// The tables to classify by (each by its name); all bound and loaded, when empty.
    #[serde(default)]
    pub tables: Vec<String>,
    /// Only what has no code yet (the default), or `all`.
    #[serde(default)]
    pub rows: Option<String>,
    /// Only these elements (ids or GUIDs).
    #[serde(default)]
    pub elements: Vec<String>,
    /// How many candidates each category gets of each table.
    #[serde(default)]
    pub candidates: Option<usize>,
}

fn level_name(m: &Model, e: &Element) -> Option<String> {
    m.level_of(e).and_then(|g| m.level(g)).map(|l| l.name.clone())
}

fn host_of(m: &Model, e: &Element) -> Option<String> {
    let g = match e {
        Element::Door(o) | Element::Window(o) => Some(o.host),
        Element::Rebar(r) => r.host,
        Element::Railing(r) => r.host,
        _ => None,
    }?;
    let h = m.elements.get(g)?;
    let ty = parameters::type_entry(h).map(|(_, n)| n).unwrap_or_default();
    Some(if ty.is_empty() { h.category().to_string() } else { format!("{}: {ty}", h.category()) })
}

fn materials_of(m: &Model, e: &Element) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some((_, name)) = parameters::type_entry(e) {
        for l in m.type_layers(e.category(), &name) {
            if !l.material.is_empty() && !out.contains(&l.material) {
                out.push(l.material.clone());
            }
        }
    }
    if let Some(r) = m.run_type(e).filter(|r| !r.material.is_empty()) {
        out.push(r.material.clone());
    }
    out
}

fn sizes_of(m: &Model, e: &Element) -> Vec<String> {
    let mut out = Vec::new();
    if let Some((_, name)) = parameters::type_entry(e)
        && let Some((.., thick, _)) = m.layered_types().find(|(c, n, ..)| *c == e.category() && *n == name)
    {
        out.push(format!("thickness {thick:.0}"));
    }
    if let Ok(o) = m.opening_dims(e) {
        out.push(format!("width {:.0}", o.width));
        out.push(format!("height {:.0}", o.height));
    }
    out
}

fn element_name(e: &Element) -> String {
    match e {
        Element::Room(r) => format!("{} {}", r.number, r.name).trim().to_string(),
        _ => e.params().and_then(|p| p.get("Mark")).and_then(Value::as_str).unwrap_or_default().to_string(),
    }
}

fn push_unique(list: &mut Vec<String>, v: Option<String>) {
    if let Some(v) = v.filter(|v| !v.is_empty())
        && !list.contains(&v)
        && list.len() < 12
    {
        list.push(v);
    }
}

/// The model with every type in the library, so each has its GUID: itself when it is (after any command it is), else a copy.
fn synced(m: &Model) -> std::borrow::Cow<'_, Model> {
    if m.library_in_step() {
        std::borrow::Cow::Borrowed(m)
    } else {
        let mut c = m.clone();
        c.sync_library();
        std::borrow::Cow::Owned(c)
    }
}

/// The brief `ask` makes of the model.
pub fn brief(m: &Model, ask: &BriefAsk) -> Result<Brief, String> {
    let m2 = synced(m);
    let m = m2.as_ref();
    let bindings: Vec<&Binding> = m
        .classification
        .bindings
        .iter()
        .filter(|b| m.classification.tables.contains_key(&b.table) && m.parameters.contains_key(&b.parameter))
        .filter(|b| ask.tables.is_empty() || ask.tables.contains(&b.table))
        .collect();
    if bindings.is_empty() {
        return Err("no table is both loaded and bound to a parameter: load a system's table and its parameters first".into());
    }
    // One binding a table: a table may feed several parameters (Uniclass Pr and COBie's Type Category), and is asked once.
    let mut by_table: BTreeMap<&str, &Binding> = BTreeMap::new();
    for b in &bindings {
        by_table.entry(b.table.as_str()).or_insert(b);
    }
    let all = ask.rows.as_deref().is_some_and(|r| r.eq_ignore_ascii_case("all"));
    let only: Option<BTreeSet<Id>> = (!ask.elements.is_empty()).then(|| {
        ask.elements
            .iter()
            .filter_map(|k| match target(m, k) {
                Some(Target::Element(id)) => Some(id),
                _ => None,
            })
            .collect()
    });
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for (tname, b) in &by_table {
        let Some(q) = m.parameters.get(&b.parameter) else { continue };
        for (id, e) in elements_for(m, b) {
            if only.as_ref().is_some_and(|o| !o.contains(&id)) {
                continue;
            }
            let t = Target::Element(id);
            let now = current(m, b, &t).map(String::from);
            if now.is_some() && !all {
                continue;
            }
            let key = match q.scope {
                Scope::Type => parameters::type_guid(m, e).map(|g| g.to_string()),
                Scope::Instance => m.elements.guid(id).map(|g| g.to_string()),
            };
            let Some(key) = key else { continue };
            let row = rows.entry(key.clone()).or_insert_with(|| {
                let (family, ty) = match e.family() {
                    Some((f, t)) => (f.to_string(), t.to_string()),
                    None => (String::new(), parameters::type_entry(e).map(|(_, n)| n).unwrap_or_default()),
                };
                Row {
                    key,
                    category: e.category().to_string(),
                    family,
                    type_name: ty,
                    sizes: sizes_of(m, e),
                    materials: materials_of(m, e),
                    ..Row::default()
                }
            });
            if !row.ids.contains(&id) {
                row.ids.push(id);
                row.count += 1;
                push_unique(&mut row.levels, level_name(m, e));
                push_unique(&mut row.hosts, host_of(m, e));
                if q.scope == Scope::Instance {
                    row.name = element_name(e);
                }
            }
            row.tables.insert(tname.to_string(), now);
        }
    }
    if rows.len() > MAX_BRIEF {
        return Err(format!("{} rows; a brief holds up to {MAX_BRIEF}: classify a selection, or by table", rows.len()));
    }
    let rows: Vec<Row> = rows.into_values().collect();
    let n = ask.candidates.unwrap_or(60).clamp(1, MAX_CANDIDATES);
    let mut candidates: BTreeMap<String, BTreeMap<String, Vec<(String, String)>>> = BTreeMap::new();
    for tname in by_table.keys() {
        let Some(table) = m.classification.tables.get(*tname) else { continue };
        let mut cats: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
        for r in rows.iter().filter(|r| r.tables.contains_key(*tname)) {
            cats.entry(r.category.as_str()).or_default().push(r);
        }
        for (cat, rs) in cats {
            let found = candidates_for(table, cat, &rs, n);
            candidates.entry(tname.to_string()).or_default().insert(cat.to_string(), found);
        }
    }
    let tables: Vec<Value> = by_table
        .iter()
        .filter_map(|(n, b)| {
            let t = m.classification.tables.get(*n)?;
            let sys = system(&t.system);
            let q = m.parameters.get(&b.parameter)?;
            Some(json!({"table": n, "system": sys.map_or(t.system.as_str(), |s| s.name), "edition": t.edition, "codes": t.items.len(),
                        "scope": q.scope, "example": sys.and_then(|s| s.table(&t.key)).map(|d| d.example).filter(|e| !e.is_empty()),
                        "file": format!("{}.csv", file_stem(n))}))
        })
        .collect();
    Ok(Brief { tables, rows, candidates })
}

/// What a table's name is as a file's: letters, digits and dashes.
pub fn file_stem(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect()
}

/// Words that say what a category is, in the languages of the systems: what a table's titles are searched for.
fn category_words(cat: &str) -> &'static [&'static str] {
    match cat {
        "wall" => &["wall", "walling", "partition", "parede", "vägg", "væg", "wand", "muur", "alvenaria"],
        "floor" => &["floor", "slab", "laje", "piso", "bjälklag", "golv", "dæk", "gulv", "vloer"],
        "ceiling" => &["ceiling", "forro", "teto", "innertak", "loft", "plafond"],
        "roof" => &["roof", "roofing", "cobertura", "telhado", "tak", "dak"],
        "door" => &["door", "doorset", "porta", "dörr", "dør", "deur"],
        "window" => &["window", "glazing", "janela", "fönster", "vindue", "raam"],
        "column" | "structuralcolumn" => &["column", "pillar", "pilar", "coluna", "pelare", "søjle", "kolom"],
        "beam" => &["beam", "viga", "balk", "bjælke"],
        "footing" | "stripfooting" => &["foundation", "footing", "fundação", "sapata", "grundläggning", "fundament", "fundering"],
        "stair" => &["stair", "stairs", "escada", "trappa", "trappe", "trap"],
        "ramp" => &["ramp", "rampa", "rampe"],
        "railing" => &["railing", "balustrade", "handrail", "guarda", "corrimão", "räcke", "rækværk", "leuning"],
        "pipe" => &["pipe", "pipework", "piping", "tubo", "tubulação", "rör", "rør", "leiding"],
        "duct" => &["duct", "ductwork", "duto", "kanal", "luftkanal", "kanaal"],
        "conduit" => &["conduit", "eletroduto", "elrör", "installationsrør"],
        "cabletray" => &["cable tray", "tray", "eletrocalha", "kabelstege", "kabelbakke", "kabelgoot"],
        "wire" | "cable" => &["cable", "wire", "cabo", "fio", "kabel"],
        "downconductor" | "groundconductor" => &["lightning", "earthing", "conductor", "para-raios", "aterramento", "åskledare", "jording"],
        "room" => &["space", "room", "espaço", "ambiente", "sala", "rum", "utrymme", "ruimte"],
        "component" => {
            &["equipment", "fixture", "fitting", "furniture", "appliance", "equipamento", "mobiliário", "utrustning", "inventarier"]
        }
        "mass" => &["building", "edifício", "byggnad", "bygning", "gebouw"],
        _ => &[],
    }
}

fn fold(t: &str) -> String {
    t.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'æ' => 'a',
            c => c,
        })
        .collect()
}

/// A text's words of three letters or more, folded: what titles and rows are matched on.
fn words(t: &str) -> Vec<String> {
    fold(t)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 && !w.chars().all(|c| c.is_ascii_digit()))
        .map(String::from)
        .collect()
}

/// Whether `a` and `b` are one word, the plural or a longer form of the other (`door`, `doors`, `doorsets`).
fn alike(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short.len() >= 4 && long.starts_with(short) || a == b
}

/// The `n` codes of `table` most like a category's rows: their titles (and their parents') scored by the words of the
/// category, the rows' types, families, materials and hosts, the more specific code first among equals.
pub fn candidates_for(table: &Table, cat: &str, rows: &[&Row], n: usize) -> Vec<(String, String)> {
    let mut want: Vec<(String, f64)> = Vec::new();
    for w in category_words(cat) {
        for x in words(w) {
            want.push((x, 3.0));
        }
    }
    for r in rows {
        let text = format!("{} {} {} {}", r.family, r.type_name, r.materials.join(" "), r.name);
        for w in words(&text) {
            want.push((w, 1.0));
        }
    }
    let parents = table.parents();
    let titles: Vec<Vec<String>> = table.items.iter().map(|i| words(&i.title)).collect();
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(Some(p)) = parents.get(i) {
            d += 1;
            i = *p;
            if d > 32 {
                break;
            }
        }
        d
    };
    let mut scored: Vec<(f64, usize)> = table
        .items
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let mut s = 0.0;
            for (w, weight) in &want {
                if titles[i].iter().any(|t| alike(t, w)) {
                    s += weight;
                } else if let Some(Some(p)) = parents.get(i)
                    && titles[*p].iter().any(|t| alike(t, w))
                {
                    s += weight * 0.3;
                }
            }
            (s + depth(i) as f64 * 0.01, i)
        })
        .filter(|(s, _)| *s >= 1.0)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(n).map(|(_, i)| (table.items[i].code.clone(), table.items[i].title.clone())).collect()
}

/// The JSON Schema of an answer, for the agents that hold their answer to one.
pub fn answer_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "rows": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "key": {"type": "string"},
                        "table": {"type": "string"},
                        "code": {"type": ["string", "null"]},
                        "confidence": {"type": "number"},
                        "reason": {"type": "string"}
                    },
                    "required": ["key", "table", "code", "confidence", "reason"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["rows"],
        "additionalProperties": false
    })
}

/// What an agent is told: the task, the files beside it, and the answer's shape. `files` says whether it can read the
/// tables' files; an agent that can't gets the candidates only.
pub fn instructions(brief: &Brief, files: bool) -> String {
    let mut t = String::from(
        "# Classify building elements\n\n\
         You are classifying the elements of a building model by construction classification tables. Each row of the brief is \
         an element type (or, for spaces and other per-element tables, one element), with what the model knows of it: its \
         category, family, type name, materials, the levels it is on, what hosts it, its sizes and how many elements it stands \
         for. Pick, for each row and each table it lists, the code of that table that best says what the row is.\n\n\
         ## Rules\n\n\
         - Use only codes of the table. Never make one up, and never shorten or reformat one: copy it as the table writes it.\n\
         - Prefer the most specific code that is surely right; when only a broader one is, use the broader one.\n\
         - When no code fits, answer null for that row and table.\n\
         - `confidence` is from 0 to 1: how sure you are the code is the one an experienced information manager would pick.\n\
         - `reason` is one short sentence (under 140 characters) saying what decided it.\n\
         - Answer every row for every table in its `tables`, keyed by the row's `key` and the table's name.\n\
         - Answer only with the JSON object described below: no prose before or after it.\n\n",
    );
    t.push_str("## Tables\n\n");
    for x in &brief.tables {
        let name = x.get("table").and_then(Value::as_str).unwrap_or_default();
        let sys = x.get("system").and_then(Value::as_str).unwrap_or_default();
        let ed = x.get("edition").and_then(Value::as_str).unwrap_or_default();
        let ex = x.get("example").and_then(Value::as_str).unwrap_or_default();
        let file = x.get("file").and_then(Value::as_str).unwrap_or_default();
        t.push_str(&format!("- `{name}`: {sys} {ed}"));
        if !ex.is_empty() {
            t.push_str(&format!(", codes written like `{ex}`"));
        }
        if files {
            t.push_str(&format!(". The whole table is `{file}` (Code, Title, Parent)"));
        }
        t.push('\n');
    }
    if files {
        t.push_str(
            "\n`brief.json` holds the rows and, for each table and category, candidates: the codes whose titles are most like \
             the rows. Start from the candidates, and search the table's file when none fits.\n",
        );
    } else {
        t.push_str("\nThe brief below holds the rows and, for each table and category, the candidate codes. Pick only from those.\n");
    }
    t.push_str(
        "\n## Answer\n\nOne JSON object: `{\"rows\": [{\"key\": \"…\", \"table\": \"…\", \"code\": \"…\" or null, \"confidence\": 0.0, \
         \"reason\": \"…\"}]}`\n",
    );
    t
}

/// One proposal of an answer, checked against the model.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Proposal {
    pub key: String,
    pub table: String,
    pub code: Option<String>,
    pub title: Option<String>,
    pub confidence: f64,
    pub reason: String,
    /// What it holds now.
    pub current: Option<String>,
}

/// An answer, checked: what can be applied, and what can't with why.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Answer {
    pub proposals: Vec<Proposal>,
    pub refused: Vec<String>,
}

/// The JSON object in an agent's text: all of it, or within a fence, or from its first `{` to its last `}`.
pub fn json_in(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Some(v);
    }
    if let Some(start) = t.find("```") {
        let rest = &t[start + 3..];
        let rest = rest.find('\n').map_or(rest, |n| &rest[n + 1..]);
        if let Some(end) = rest.find("```")
            && let Ok(v) = serde_json::from_str::<Value>(rest[..end].trim())
        {
            return Some(v);
        }
    }
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    (a < b).then(|| serde_json::from_str(&t[a..=b]).ok()).flatten()
}

/// An agent's answer checked against the model: each row's key a known element or type, its table loaded and bound, its
/// code in the table. What fails is listed and left out, never applied.
pub fn read_answer(m: &Model, answer: &Value) -> Answer {
    let m2 = synced(m);
    let m = m2.as_ref();
    let mut out = Answer::default();
    let rows = answer.get("rows").and_then(Value::as_array).or_else(|| answer.as_array());
    let Some(rows) = rows else {
        out.refused.push("the answer holds no `rows`".into());
        return out;
    };
    let mut seen = BTreeSet::new();
    for (n, r) in rows.iter().enumerate().take(MAX_BRIEF * 8) {
        let field = |k: &str| r.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
        let refuse = |out: &mut Answer, why: String| {
            if out.refused.len() < 500 {
                out.refused.push(format!("row {}: {why}", n + 1));
            }
        };
        let (Some(key), Some(table)) = (field("key"), field("table")) else {
            refuse(&mut out, "it has no `key` or `table`".into());
            continue;
        };
        if key.chars().count() > MAX_TEXT || table.chars().count() > MAX_TEXT {
            refuse(&mut out, "its key or table is too long".into());
            continue;
        }
        let Some(t) = target(m, key) else {
            refuse(&mut out, format!("`{key}` is no element or type of the project"));
            continue;
        };
        let Some(b) = m.classification.bindings.iter().find(|b| b.table == table && m.parameters.contains_key(&b.parameter)) else {
            refuse(&mut out, format!("no parameter takes codes from `{table}`"));
            continue;
        };
        let Some(tb) = m.classification.tables.get(table) else {
            refuse(&mut out, format!("`{table}` isn't loaded"));
            continue;
        };
        if !seen.insert((key.to_string(), table.to_string())) {
            refuse(&mut out, format!("`{key}` and `{table}` again"));
            continue;
        }
        let code = field("code").filter(|c| !c.eq_ignore_ascii_case("null"));
        let item = match code {
            Some(c) => match tb.find(c) {
                Some(i) => Some(i),
                None => {
                    refuse(&mut out, format!("`{}` isn't in {table}", c.chars().take(MAX_CODE).collect::<String>()));
                    continue;
                }
            },
            None => None,
        };
        let assign =
            Assign { target: key.to_string(), parameter: Some(b.parameter.clone()), table: None, code: item.map(|i| i.code.clone()) };
        if let Err(e) = writes(m, &assign) {
            refuse(&mut out, e);
            continue;
        }
        let confidence = r.get("confidence").and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0);
        let reason: String = field("reason").unwrap_or_default().chars().take(MAX_REASON).collect();
        out.proposals.push(Proposal {
            key: key.to_string(),
            table: table.to_string(),
            code: item.map(|i| i.code.clone()),
            title: item.map(|i| i.title.clone()),
            confidence,
            reason,
            current: current(m, b, &t).map(String::from),
        });
    }
    out
}

/// The `classification_brief` command: the brief an agent is handed, its instructions and the answer's schema.
pub(crate) fn brief_command(s: &mut Session, p: &Value) -> R {
    let c = "classification_brief";
    only(c, p, &["tables", "rows", "elements", "candidates", "files"])?;
    let files = p.get("files").and_then(Value::as_bool).unwrap_or(false);
    let mut q = p.clone();
    if let Some(o) = q.as_object_mut() {
        o.remove("files");
    }
    let ask: BriefAsk = serde_json::from_value(q).map_err(|e| bad(c, e))?;
    if let Some(r) = ask.rows.as_deref().filter(|r| !r.eq_ignore_ascii_case("all") && !r.eq_ignore_ascii_case("unclassified")) {
        return Err(bad(c, format!("`rows` is `unclassified` or `all`, not `{r}`")));
    }
    let b = brief(&s.model, &ask).map_err(|e| bad(c, e))?;
    Ok(json!({"instructions": instructions(&b, files), "brief": b, "schema": answer_schema()}))
}

/// The `classification_answer` command: an agent's answer checked against the project, nothing written.
pub(crate) fn answer_command(s: &mut Session, p: &Value) -> R {
    let c = "classification_answer";
    only(c, p, &["answer"])?;
    let v = match p.get("answer") {
        Some(Value::String(t)) => json_in(t).ok_or_else(|| bad(c, "the answer holds no JSON object"))?,
        Some(v) if !v.is_null() => v.clone(),
        _ => return Err(bad(c, "`answer` is the agent's answer, as JSON or its text")),
    };
    serde_json::to_value(read_answer(&s.model, &v)).map_err(|e| bad(c, e))
}

/// The `classification_check` command: for each bound parameter, how much is classified, what isn't, and what holds a
/// code its table doesn't.
pub(crate) fn check_command(s: &mut Session, p: &Value) -> R {
    let c = "classification_check";
    only(c, p, &["table"])?;
    let only_table = string(c, p, "table")?;
    let m2 = synced(&s.model);
    let m = m2.as_ref();
    let mut out = Vec::new();
    for b in m.classification.bindings.iter().filter(|b| only_table.as_ref().is_none_or(|t| *t == b.table)) {
        let Some(q) = m.parameters.get(&b.parameter) else {
            out.push(json!({"parameter": b.parameter, "table": b.table, "problem": "the parameter is gone"}));
            continue;
        };
        let table = m.classification.tables.get(&b.table);
        let mut owners: BTreeMap<String, (Target, Vec<Id>)> = BTreeMap::new();
        for (id, e) in elements_for(m, b) {
            let (k, t) = match q.scope {
                Scope::Type => match parameters::type_guid(m, e) {
                    Some(g) => (g.to_string(), Target::Type(g)),
                    None => continue,
                },
                Scope::Instance => (id.to_string(), Target::Element(id)),
            };
            owners.entry(k).or_insert((t, Vec::new())).1.push(id);
        }
        let (mut done, mut missing, mut unknown) = (0usize, Vec::new(), Vec::new());
        for (t, ids) in owners.values() {
            match current(m, b, t) {
                Some(code) if table.is_none_or(|tb| tb.find(code).is_some()) => done += 1,
                Some(code) => unknown.push(json!({"owner": owner_json(m, t), "code": code, "elements": ids.len()})),
                None => missing.push(json!({"owner": owner_json(m, t), "elements": ids.len()})),
            }
        }
        out.push(json!({
            "parameter": b.parameter, "table": b.table, "loaded": table.is_some(), "scope": q.scope,
            "classified": done, "of": owners.len(), "unclassified": missing.iter().take(200).collect::<Vec<_>>(),
            "unclassified_count": missing.len(), "not_in_table": unknown.iter().take(200).collect::<Vec<_>>(),
        }));
    }
    Ok(json!({"bindings": out}))
}

impl Brief {
    /// The brief cut into briefs of up to `n` rows, each with the candidates of the categories its rows are.
    pub fn chunks(&self, n: usize) -> Vec<Brief> {
        let n = n.max(1);
        self.rows
            .chunks(n)
            .map(|rows| {
                let cats: BTreeSet<&str> = rows.iter().map(|r| r.category.as_str()).collect();
                let tables: BTreeSet<&String> = rows.iter().flat_map(|r| r.tables.keys()).collect();
                let candidates = self
                    .candidates
                    .iter()
                    .filter(|(t, _)| tables.contains(t))
                    .map(|(t, by)| {
                        (t.clone(), by.iter().filter(|(c, _)| cats.contains(c.as_str())).map(|(c, l)| (c.clone(), l.clone())).collect())
                    })
                    .collect();
                let tables = self
                    .tables
                    .iter()
                    .filter(|x| x.get("table").and_then(Value::as_str).is_some_and(|t| tables.iter().any(|n| n.as_str() == t)))
                    .cloned()
                    .collect();
                Brief { tables, rows: rows.to_vec(), candidates }
            })
            .collect()
    }
}

/// What an agent is handed for a brief: its instructions, the files beside them (the brief and each table, as `.csv`),
/// and, for an agent that reads no files, the brief as text.
pub struct Handover {
    pub instructions: String,
    pub files: Vec<(String, String)>,
    pub inline: String,
}

/// The handover of `brief` from the tables of `m`.
pub fn handover(m: &Model, brief: &Brief, files: bool) -> Handover {
    let text = serde_json::to_string_pretty(brief).unwrap_or_default();
    let mut out = vec![("brief.json".to_string(), text.clone())];
    if files {
        for x in &brief.tables {
            let Some(name) = x.get("table").and_then(Value::as_str) else { continue };
            if let Some(t) = m.classification.tables.get(name) {
                out.push((format!("{}.csv", file_stem(name)), t.to_csv()));
            }
        }
    }
    let inline = if files { String::new() } else { serde_json::to_string(brief).unwrap_or_default() };
    Handover { instructions: instructions(brief, files), files: out, inline }
}

/// How many of what a table's parameter is for have a code from it: types, or elements for a table of each element's.
pub fn coverage(m: &Model, table: &str) -> Option<(usize, usize)> {
    let b = m.classification.bindings.iter().find(|b| b.table == table && m.parameters.contains_key(&b.parameter))?;
    let q = m.parameters.get(&b.parameter)?;
    let mut owners: BTreeMap<String, Target> = BTreeMap::new();
    for (id, e) in elements_for(m, b) {
        match q.scope {
            Scope::Type => {
                if let Some(g) = parameters::type_guid(m, e) {
                    owners.entry(g.to_string()).or_insert(Target::Type(g));
                }
            }
            Scope::Instance => {
                owners.insert(id.to_string(), Target::Element(id));
            }
        }
    }
    let done = owners.values().filter(|t| current(m, b, t).is_some()).count();
    Some((done, owners.len()))
}

/// The system an IFC file's `IfcClassification` name is of, when it is one of `SYSTEMS` with tables: by its id, name or short
/// name, or a name beginning with one (`Uniclass 2015 v1.30`). Its codes come in as its own parameters; another system's,
/// as the parameter the import names for it.
pub fn known_system(name: &str) -> Option<&'static SystemDef> {
    let n = name.trim();
    system(n)
        .or_else(|| {
            let lower = n.to_lowercase();
            SYSTEMS
                .iter()
                .find(|s| !s.tables.is_empty() && (lower.starts_with(&s.short.to_lowercase()) || lower.starts_with(&s.name.to_lowercase())))
        })
        .filter(|s| !s.tables.is_empty())
}

/// The parameters an IFC file's code of the classification `name` comes in as: a known system's own (the table its code
/// begins with, `Uniclass Ss Number` and `Uniclass Ss Title`), so a project written and read back is itself; another's,
/// `<name> Number` and `<name> Title`.
pub fn imported_parameters(name: &str, code: &str) -> (String, String) {
    let n = name.trim();
    if let Some(sys) = known_system(n) {
        let k = normal(code);
        let by_key = sys
            .tables
            .iter()
            .filter(|t| !t.key.is_empty() && t.key.chars().all(char::is_alphanumeric))
            .find(|t| k.starts_with(&normal(t.key)));
        if let Some(t) = by_key.or_else(|| (sys.tables.len() == 1).then(|| &sys.tables[0])) {
            return sys.parameters_of(t);
        }
        return (format!("{} Number", sys.short), format!("{} Title", sys.short));
    }
    let base: String = n.chars().take(MAX_TEXT / 2).collect();
    let base = if base.is_empty() { "Classification".to_string() } else { base };
    (format!("{base} Number"), format!("{base} Title"))
}
