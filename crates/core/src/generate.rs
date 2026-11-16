//! The `generate` command: a brief in, a run directory out. A brief by `rooms` (a built-in family or a family file) goes to the
//! shape graph engine (`shape_graph`); a brief by an area goes to the area-driven generator in `crate::old_generator`. The
//! program, the topology and the placed building are written as JSON, with the BIMCraft script they compile to and a report
//! of the checks made. Everything is whole millimetres and integer seeds, so the same brief gives the same bytes on any
//! machine, and a brief that can't be met writes nothing. Detail: `docs/areas/graph-generator.md`.
//!
//! What both engines use is here: the files a run directory holds, `Built`, `Refusal`, `Rng`, the graph types, the
//! archetypes, the checks, the gridlines and the door swings. Nothing here or in `shape_graph` reads the old generator, but
//! for the area branch of the command, `deliver`'s area batches and `Built::plan`.

pub(crate) mod deliver;
#[cfg(test)]
mod deliver_tests;
#[cfg(test)]
mod family_file_tests;
pub(crate) mod graph;
pub(crate) mod grid;
#[cfg(test)]
mod gridlines_tests;
pub(crate) mod program;
pub(crate) mod report;
pub(crate) mod shape_graph;
pub(crate) mod swing;

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::mix;
use crate::old_generator::layout::Plan;
use program::Archetype;
use report::{Check, Status};

const MAX_PATH: usize = 4096;
/// The files of a run directory, but the manifest, which names them.
pub(crate) const FILES: [(&str, &str); 5] = [
    ("program_graph", "program-graph.json"),
    ("topology_graph", "topology-graph.json"),
    ("building_ir", "building-ir.json"),
    ("script", "script.bimcraft.txt"),
    ("report", "report.json"),
];
pub(crate) const MANIFEST: &str = "manifest.json";

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

pub(crate) fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
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
    /// Pin the wall lines of a plan by `rooms`: the exterior pen (8 to 14), its lightness (0 to 45), the interior pen (3 to 9) and
    /// its lightness (0 to 65).
    ext_pen: Option<i64>,
    ext_light: Option<i64>,
    int_pen: Option<i64>,
    int_light: Option<i64>,
    /// `allow` lets a family file marked unreviewed run through the shape graph engine.
    unreviewed: Option<String>,
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

pub(crate) fn pretty(v: &Value) -> Result<String, Refusal> {
    serde_json::to_string_pretty(v)
        .map(|mut t| {
            t.push('\n');
            t
        })
        .map_err(|e| refuse("generation_failed", format!("a document could not be written as JSON: {e}")))
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

pub(crate) fn run(p: &Value) -> Result<Value, Refusal> {
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
    let shaped = ShapedOptions::parse(given.interior.as_deref(), given.bumps.as_deref(), given.gridlines.as_deref())
        .map_err(|e| refuse("invalid_params", e))?;
    if given.delivery.is_some() && given.rooms.is_some() {
        return Err(refuse(
            "invalid_params",
            "`delivery` belongs to the plan by an area (`area_m2` or `area_sqft`); leave it out with `rooms`",
        ));
    }
    // A plan by rooms names a built-in family file. That name need not be an area archetype: a school is not.
    if let Some(rooms) = given.rooms {
        return run_shaped(&given, rooms, &shaped);
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
        ("ext_pen", given.ext_pen.is_some()),
        ("ext_light", given.ext_light.is_some()),
        ("int_pen", given.int_pen.is_some()),
        ("int_light", given.int_light.is_some()),
    ]
    .iter()
    .find(|(_, set)| *set)
    {
        return Err(refuse("invalid_params", format!("`{}` belongs to the plan by `rooms`; give `rooms` too, or leave it out", option.0)));
    }
    let built = crate::old_generator::area_brief(archetype, given.seed, given.area_m2, given.area_sqft, shaped.gridlines, delivery)?;
    written(&given.path, archetype.name(), &built)
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ShapedOptions {
    pub interior: Option<shape_graph::Interior>,
    pub bumps: Option<bool>,
    pub gridlines: Option<bool>,
}

impl ShapedOptions {
    pub(crate) fn parse(interior: Option<&str>, bumps: Option<&str>, gridlines: Option<&str>) -> Result<ShapedOptions, String> {
        let interior = match interior {
            None => None,
            Some(text) => Some(shape_graph::Interior::parse(text).ok_or_else(|| {
                format!("`interior` is `aligned`, `jogged` or `mixed`; got {:?}", text.chars().take(40).collect::<String>())
            })?),
        };
        let bumps = match bumps {
            None => None,
            Some("on") => Some(true),
            Some("off") => Some(false),
            Some(text) => return Err(format!("`bumps` is `on` or `off`; got {:?}", text.chars().take(40).collect::<String>())),
        };
        let gridlines = match gridlines {
            None => None,
            Some("on") => Some(true),
            Some("off") => Some(false),
            Some(text) => return Err(format!("`gridlines` is `on` or `off`; got {:?}", text.chars().take(40).collect::<String>())),
        };
        Ok(ShapedOptions { interior, bumps, gridlines })
    }
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
fn run_shaped(given: &Params, rooms: i64, shaped: &ShapedOptions) -> Result<Value, Refusal> {
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
    let allow_unreviewed = match given.unreviewed.as_deref() {
        None => false,
        Some("allow") => true,
        Some(_) => return Err(refuse("invalid_params", "`unreviewed` is `allow`, or left out")),
    };
    let pins = shape_graph::wall_lines::Pins {
        ext_pen: given.ext_pen,
        ext_light: given.ext_light,
        int_pen: given.int_pen,
        int_light: given.int_light,
    };
    pins.check().map_err(|e| refuse("invalid_params", e))?;
    let mut request = shape_graph::Request::new(
        given.archetype.clone().unwrap_or_default(),
        given.seed,
        rooms,
        tol,
        given.shape.clone(),
        shaped.gridlines.unwrap_or(false),
    );
    request.interior = shaped.interior.unwrap_or(request.interior);
    request.bumps = shaped.bumps.unwrap_or(request.bumps);
    match (&given.archetype, &given.family_file) {
        (Some(name), _) => {
            let built = shape_graph::build_styled(&request, &pins, allow_unreviewed)?;
            written(&given.path, name, &built)
        }
        (None, Some(file)) => {
            // Read before anything is made; only the family's name, never the file's path, reaches the files.
            let fam = read_family_file(file)?;
            request.archetype = fam.name.clone();
            let built = shape_graph::build_family_styled(&fam, &request, &pins, allow_unreviewed)?;
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
