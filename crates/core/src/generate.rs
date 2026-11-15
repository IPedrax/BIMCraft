//! The graph building generator: a brief (an archetype, a gross area and a seed) in, a run directory out. The program, the
//! topology and the placed building are written as JSON, with the BIMCraft script they compile to and a report of the checks
//! made. Everything is whole millimetres and integer seeds, so the same brief gives the same bytes on any machine, and a brief
//! that can't be met writes nothing. Detail: `docs/areas/graph-generator.md`.

pub(crate) mod deliver;
#[cfg(test)]
mod deliver_tests;
pub(crate) mod delivery;
#[cfg(test)]
mod delivery_tests;
#[cfg(test)]
mod family_file_tests;
pub(crate) mod graph;
pub(crate) mod gridlines;
#[cfg(test)]
mod gridlines_tests;
pub(crate) mod house;
mod layout;
#[cfg(test)]
mod legacy_tests;
mod lift;
#[cfg(test)]
mod lift_tests;
pub(crate) mod medical;
mod openings;
pub(crate) mod pattern;
pub(crate) mod program;
pub(crate) mod report;
mod script;
pub(crate) mod shape_graph;
pub(crate) mod studio;
#[cfg(test)]
mod tests;

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::mix;
use graph::{SCHEMA_VERSION, Topology};
use layout::Plan;
use lift::Building;
use program::Archetype;
use report::{Check, Status};

/// Changes when the same brief would give different files.
pub const GENERATOR_VERSION: &str = "graph-generator/1";
/// The largest gross area a brief may ask for, m2: past it the run directory is no longer a plan a person reads.
pub const MAX_AREA_M2: f64 = 5000.0;
const MAX_PATH: usize = 4096;
/// The files of a run directory, but the manifest, which names them.
const FILES: [(&str, &str); 5] = [
    ("program_graph", "program-graph.json"),
    ("topology_graph", "topology-graph.json"),
    ("building_ir", "building-ir.json"),
    ("script", "script.bimcraft.txt"),
    ("report", "report.json"),
];
const MANIFEST: &str = "manifest.json";

/// A small seeded generator (splitmix64 steps): the same seed and salt give the same numbers everywhere.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64, salt: u64) -> Rng {
        Rng(mix(seed ^ mix(salt)))
    }

    pub fn step(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// A number below `n`, or 0 when `n` is.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.step() % n }
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// Why a brief gave no files: a code that names the kind of trouble, and what to change.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { code, message: message.into() }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    path: String,
    archetype: Option<String>,
    /// A family file in place of a built-in family; it needs `rooms`.
    family_file: Option<String>,
    seed: u64,
    area_m2: Option<f64>,
    area_sqft: Option<f64>,
    /// The shape graph engine's options (`--rooms` selects it).
    rooms: Option<i64>,
    rooms_tol: Option<i64>,
    shape: Option<String>,
    interior: Option<String>,
    bumps: Option<String>,
    /// `on` draws the plan to the delivery drawing rules; `--area` plans only.
    delivery: Option<String>,
    /// `on` or `off`: the plan's gridlines. `off` with `rooms`, what the archetype draws today with an area.
    gridlines: Option<String>,
    /// `allow` lets a family file marked unreviewed run through the shape graph engine.
    unreviewed: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AreaRequest {
    value: f64,
    unit: &'static str,
    m2: f64,
}

impl AreaRequest {
    pub(crate) fn m2(value: f64) -> AreaRequest {
        AreaRequest { value, unit: "m2", m2: value }
    }

    fn sqft(value: f64) -> AreaRequest {
        AreaRequest { value, unit: "sqft", m2: value * 0.092_903_04 }
    }
}

/// A run directory's files, in memory and checked, before any of them is written.
pub struct Built {
    pub archetype: Archetype,
    pub seed: u64,
    /// The corridor plan the files were made from; a house has none.
    pub plan: Option<Plan>,
    pub gross_m2: f64,
    pub rooms: usize,
    pub checks: Vec<Check>,
    /// The file name and its text: the manifest last.
    pub files: Vec<(&'static str, String)>,
    pub topology_hash: String,
}

/// 64-bit FNV-1a, as 16 lower-case hex digits.
pub fn fnv1a64(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{hash:016x}")
}

fn pretty(v: &Value) -> Result<String, Refusal> {
    serde_json::to_string_pretty(v)
        .map(|mut t| {
            t.push('\n');
            t
        })
        .map_err(|e| refuse("generation_failed", format!("a document could not be written as JSON: {e}")))
}

/// The least area, in 10 m2 steps, that a brief of this archetype and seed is met at, when there is one.
fn least_area(archetype: Archetype, seed: u64) -> Option<f64> {
    if archetype == Archetype::House {
        return Some(house::least_area());
    }
    if archetype == Archetype::StudioOffice {
        return Some(studio::least_area());
    }
    if archetype == Archetype::OfficePattern {
        return Some(pattern::least_area());
    }
    if archetype == Archetype::MedicalDay {
        return Some(medical::least_area());
    }
    (1..=(MAX_AREA_M2 as u32 / 10)).map(|i| f64::from(i) * 10.0).find(|a| layout::plan(archetype, *a, seed).is_ok())
}

/// The files for a brief, or why there are none. Touches no file.
pub fn build(archetype: Archetype, seed: u64, area_m2: f64) -> Result<Built, Refusal> {
    build_with_area(archetype, seed, AreaRequest::m2(area_m2))
}

/// The files for a brief drawn to the delivery rules: the plan `build` makes, through [`delivery::apply`].
pub(crate) fn build_for_delivery(archetype: Archetype, seed: u64, area: AreaRequest) -> Result<Built, Refusal> {
    delivery::apply(build_with_area(archetype, seed, area)?)
}

fn build_with_area(archetype: Archetype, seed: u64, area: AreaRequest) -> Result<Built, Refusal> {
    if !area.m2.is_finite() || area.m2 <= 0.0 || area.m2 > MAX_AREA_M2 {
        return Err(refuse(
            "invalid_area",
            format!(
                "`area_m2` is the gross area in square metres, above 0 and at most {MAX_AREA_M2}; got {} {} ({} m2)",
                area.value, area.unit, area.m2
            ),
        ));
    }
    if archetype == Archetype::House {
        return house::build(seed, area);
    }
    if archetype == Archetype::StudioOffice {
        return studio::build(seed, area);
    }
    if archetype == Archetype::OfficePattern {
        return pattern::build(seed, area);
    }
    if archetype == Archetype::MedicalDay {
        return medical::build(seed, area);
    }
    let plan = layout::plan(archetype, area.m2, seed).map_err(|why| {
        let least = least_area(archetype, seed)
            .map_or_else(String::new, |a| format!(" The least this {} brief is met at is about {a} m2.", archetype.name()));
        refuse(
            "unsatisfiable_area",
            format!("{:.1} m2 is too small for the {} program: it cannot fit ({why}).{least}", area.m2, archetype.name()),
        )
    })?;
    let topology = Topology::of(&plan);
    let building = Building::lift(&plan, &topology);
    let mut checks = report::graph_checks(&topology, archetype);
    checks.extend(report::lift_checks(&building, &topology));
    let script = script::compile(&plan, &building);
    let mut scratch = Session::new();
    let replies =
        scratch.script(&script.text).map_err(|e| refuse("generation_failed", format!("the generated script does not run: {e}")))?;
    checks.extend(report::run_checks(&plan, &building, &script, &replies, &scratch.model));
    let failed = report::failures(&checks);
    if let Some(first) = failed.first() {
        let more = if failed.len() > 1 { format!(" (and {} more)", failed.len() - 1) } else { String::new() };
        return Err(refuse("rule_violation", format!("{} {}: {}{more}", first.id, first.name, first.detail)));
    }

    let topology_text = pretty(&topology.json(&plan))?;
    let topology_hash = fnv1a64(&topology_text);
    let texts = [
        pretty(&graph::program_json(&plan))?,
        topology_text,
        pretty(&building.json(&topology_hash, topology.nodes.len(), topology.edges.len()))?,
        script.text.clone(),
        pretty(&report::json(&plan, &checks, &script, &replies))?,
    ];
    let named: Vec<Value> =
        FILES.iter().zip(&texts).map(|((key, name), text)| json!({"key": key, "path": name, "hash": fnv1a64(text)})).collect();
    let manifest = pretty(&json!({
        "schema_version": SCHEMA_VERSION,
        "generator_version": GENERATOR_VERSION,
        "archetype": archetype.name(),
        "seed": seed,
        "layout_family": plan.layout_family.name(),
        "layout_strategy": plan.layout_strategy.name(),
        "boundary_shape": plan.boundary_shape.name(),
        "opening_symbol_profile": plan.opening_symbol_profile.name(),
        "opening_symbols": report::opening_symbols(&plan),
        "requested_area": {"value": area.value, "unit": area.unit},
        "requested_area_m2": area.m2,
        "gross_area_m2": plan.gross_m2(),
        "hash_algorithm": "fnv1a64",
        "topology_hash": topology_hash,
        "files": named,
    }))?;
    let mut files: Vec<(&'static str, String)> = FILES.iter().map(|(_, name)| *name).zip(texts).collect();
    files.push((MANIFEST, manifest));
    let (gross_m2, rooms) = (plan.gross_m2(), plan.cells.len() + 1);
    Ok(Built { archetype, seed, plan: Some(plan), gross_m2, rooms, checks, files, topology_hash })
}

/// Writes the files to `dir`, made if need be; a file that can't be written takes the ones before it away again.
fn write(dir: &Path, files: &[(&'static str, String)]) -> Result<(), Refusal> {
    let unwritable = |what: &Path, e: std::io::Error| refuse("output_unwritable", format!("{}: {e}", what.display()));
    std::fs::create_dir_all(dir).map_err(|e| unwritable(dir, e))?;
    let mut written: Vec<PathBuf> = Vec::new();
    for (name, text) in files {
        let at = dir.join(name);
        if let Err(e) = std::fs::write(&at, text) {
            for done in &written {
                let _ = std::fs::remove_file(done);
            }
            return Err(unwritable(&at, e));
        }
        written.push(at);
    }
    Ok(())
}

/// The `generate` command: a brief to a run directory.
pub fn generate(_: &mut Session, p: &Value) -> Result<Value, String> {
    run(p).map_err(|r| r.to_string())
}

fn run(p: &Value) -> Result<Value, Refusal> {
    let given: Params = serde_json::from_value(p.clone()).map_err(|e| {
        refuse(
            "invalid_params",
            format!(
                "{e}; `generate` takes `path`, `archetype` ({}), `seed` (an integer from 0) and one of `area_m2` or `area_sqft`",
                Archetype::names()
            ),
        )
    })?;
    if given.archetype.is_some() == given.family_file.is_some() {
        return Err(refuse(
            "invalid_params",
            "`generate` takes `archetype` (a built-in family) or `family_file` (a family file), one of them",
        ));
    }
    if given.family_file.is_some() && given.rooms.is_none() {
        return Err(refuse("invalid_params", "`family_file` belongs to the plan by `rooms`; give `rooms` too"));
    }
    if given.path.trim().is_empty() || given.path.len() > MAX_PATH || given.path.contains('\0') {
        return Err(refuse("invalid_params", format!("`path` is the run directory to write: not empty, at most {MAX_PATH} bytes")));
    }
    let delivery = match given.delivery.as_deref() {
        None | Some("off") => false,
        Some("on") => true,
        Some(other) => {
            return Err(refuse(
                "invalid_params",
                format!("`delivery` is `on` or `off`; got {:?}", other.chars().take(40).collect::<String>()),
            ));
        }
    };
    let gridlines = match given.gridlines.as_deref() {
        None => None,
        Some("on") => Some(true),
        Some("off") => Some(false),
        Some(other) => {
            return Err(refuse(
                "invalid_params",
                format!("`gridlines` is `on` or `off`; got {:?}", other.chars().take(40).collect::<String>()),
            ));
        }
    };
    if given.delivery.is_some() && given.rooms.is_some() {
        return Err(refuse(
            "invalid_params",
            "`delivery` belongs to the plan by an area (`area_m2` or `area_sqft`); leave it out with `rooms`",
        ));
    }
    // A plan by rooms names a built-in family file. That name need not be an area archetype: a school is not.
    if let Some(rooms) = given.rooms {
        return run_shaped(&given, rooms, gridlines.unwrap_or(false));
    }
    let archetype = match &given.archetype {
        Some(name) => Archetype::parse(name).ok_or_else(|| {
            refuse(
                "unknown_archetype",
                format!("`archetype` is one of {}; got {:?}", Archetype::names(), name.chars().take(40).collect::<String>()),
            )
        })?,
        None => return Err(refuse("invalid_params", "`generate` takes `archetype` or `family_file`")),
    };
    if let Some(option) = [
        ("rooms_tol", given.rooms_tol.is_some()),
        ("shape", given.shape.is_some()),
        ("interior", given.interior.is_some()),
        ("bumps", given.bumps.is_some()),
    ]
    .iter()
    .find(|(_, set)| *set)
    {
        return Err(refuse("invalid_params", format!("`{}` belongs to the plan by `rooms`; give `rooms` too, or leave it out", option.0)));
    }
    let area = match (given.area_m2, given.area_sqft) {
        (Some(_), Some(_)) => return Err(refuse("invalid_params", "`area_m2` and `area_sqft` are mutually exclusive; give one")),
        (Some(m2), None) => AreaRequest::m2(m2),
        (None, Some(sqft)) => AreaRequest::sqft(sqft),
        (None, None) => return Err(refuse("invalid_params", "`generate` needs one of `area_m2` or `area_sqft`")),
    };
    let mut built = build_with_area(archetype, given.seed, area)?;
    // The gridlines are edited first, so the delivery pass runs on the script that ships. A value that is the archetype's
    // default changes nothing, so the default files stay as they were.
    if let Some(on) = gridlines.filter(|on| *on != archetype.draws_grid()) {
        built = gridlines::apply(built, on)?;
    }
    if delivery {
        built = delivery::apply(built)?;
    }
    written(&given.path, archetype.name(), &built)
}

/// Writes a built plan's files and says what was written.
fn written(path: &str, archetype: &str, built: &Built) -> Result<Value, Refusal> {
    let dir = PathBuf::from(path);
    write(&dir, &built.files)?;
    let count = |s: Status| built.checks.iter().filter(|c| c.status == s).count();
    Ok(json!({
        "path": dir,
        "archetype": archetype,
        "seed": built.seed,
        "gross_area_m2": built.gross_m2,
        "rooms": built.rooms,
        "topology_hash": built.topology_hash,
        "checks": {"pass": count(Status::Pass), "warn": count(Status::Warn), "fail": count(Status::Fail)},
        "files": built.files.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
    }))
}

/// A plan by `rooms`: the shape graph engine. Everything but the options is checked before anything is made or written.
fn run_shaped(given: &Params, rooms: i64, gridlines: bool) -> Result<Value, Refusal> {
    if given.area_m2.is_some() || given.area_sqft.is_some() {
        return Err(refuse(
            "invalid_params",
            "`rooms` and an area are mutually exclusive: this engine has no area input, the area is a result",
        ));
    }
    let tol = given.rooms_tol.unwrap_or(0);
    let max = shape_graph::MAX_ROOMS;
    if !(1..=max).contains(&rooms) || !(0..=max).contains(&tol) {
        return Err(refuse("invalid_params", format!("`rooms` is from 1 to {max} and `rooms_tol` from 0 to {max}; got {rooms} and {tol}")));
    }
    if given.interior.as_deref().is_some_and(|i| i != "aligned") {
        return Err(refuse("invalid_params", "`interior` is `aligned`: jogged and mixed interiors come in a later slice"));
    }
    if given.bumps.as_deref().is_some_and(|b| b != "off") {
        return Err(refuse("invalid_params", "`bumps` is `off`: bumps come in a later slice"));
    }
    let allow_unreviewed = match given.unreviewed.as_deref() {
        None => false,
        Some("allow") => true,
        Some(_) => return Err(refuse("invalid_params", "`unreviewed` is `allow`, or left out")),
    };
    let mut request = shape_graph::Request {
        archetype: given.archetype.clone().unwrap_or_default(),
        seed: given.seed,
        rooms,
        tolerance: tol,
        shape: given.shape.clone(),
        gridlines,
    };
    match (&given.archetype, &given.family_file) {
        (Some(name), _) => {
            let built = shape_graph::build(&request, allow_unreviewed)?;
            written(&given.path, name, &built)
        }
        (None, Some(file)) => {
            // Read before anything is made; only the family's name, never the file's path, reaches the files.
            let fam = read_family_file(file)?;
            request.archetype = fam.name.clone();
            let built = shape_graph::build_family(&fam, &request, allow_unreviewed)?;
            written(&given.path, &fam.name, &built)
        }
        (None, None) => Err(refuse("invalid_params", "`generate` takes `archetype` or `family_file`")),
    }
}

/// The most a family file may hold, bytes.
const MAX_FAMILY_FILE: u64 = 1 << 20;

/// A family from a file: read at most a cap (a device that never ends is cut off), then parsed strictly.
fn read_family_file(file: &str) -> Result<shape_graph::family::Family, Refusal> {
    use std::io::Read;
    if file.trim().is_empty() || file.len() > MAX_PATH || file.contains('\0') {
        return Err(refuse("invalid_params", format!("`family_file` is the path of a family file: not empty, at most {MAX_PATH} bytes")));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .and_then(|f| f.take(MAX_FAMILY_FILE + 1).read_to_end(&mut bytes))
        .map_err(|e| refuse("invalid_params", format!("`family_file` {file:?} can't be read: {e}")))?;
    if bytes.len() as u64 > MAX_FAMILY_FILE {
        return Err(refuse("invalid_family", format!("a family file is at most {MAX_FAMILY_FILE} bytes")));
    }
    let text = String::from_utf8(bytes).map_err(|_| refuse("invalid_family", "a family file is UTF-8 text (JSON)"))?;
    shape_graph::family::Family::parse(&text).map_err(|e| refuse("invalid_family", e))
}
