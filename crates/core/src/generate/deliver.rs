//! The `deliver` command: a brief of batches of seeds to one delivery folder, and a second folder holding a spread of its seeds
//! made again, for the receiver's determinism check. Every plan is built in memory along the path `generate` and `run` take, and
//! its DWG is streamed to its file; the manifest is written once every plan is done. Detail: `docs/areas/graph-generator.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;
use serde_json::{Value, json};

use super::program::Archetype;
use super::shape_graph::{self, family};
use super::{AreaRequest, MAX_AREA_M2, Refusal, refuse};
use crate::engine::Session;
use crate::model::mix;

const MAX_PATH: usize = 4096;
/// The most a brief or a rights file may hold, bytes.
const MAX_FILE: u64 = 4 << 20;
const MAX_BATCHES: usize = 10_000;
/// The most seeds a delivery may ask for.
const MAX_SEEDS: u64 = 1_000_000;
const MAX_RERUN: i64 = 100_000;
const MAX_JOBS: i64 = 64;
const DEFAULT_RERUN: i64 = 50;
const SALT: u64 = 0x5EED_0F0A_C0DE_1234;
/// The area of an `area` batch's seed has a salt of its own, so a brief's rooms and areas never move together.
const AREA_SALT: u64 = 0x5EED_A4EA_0F0A_77B1;
const HEADER: &str = "seed,file,sha256,generator_version,units,wall_layers,door_layers,window_layers,building,floor,drawing";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    /// The root the delivery is written under.
    path: String,
    brief: String,
    delivery_id: String,
    rights: Option<String>,
    rerun: Option<i64>,
    jobs: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Brief {
    schema_version: i64,
    batches: Vec<Batch>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    archetype: String,
    first_seed: u64,
    count: u64,
    rooms: Option<[i64; 2]>,
    area: Option<[f64; 2]>,
    rooms_tol: Option<i64>,
    unreviewed: Option<String>,
}

#[derive(Clone)]
struct Job {
    archetype: String,
    seed: u64,
    rooms: i64,
    /// The gross area, whole m2, of a seed of an `area` batch; `rooms` is then unused.
    area: Option<i64>,
    tol: i64,
    unreviewed: bool,
}

struct Row {
    seed: u64,
    file: String,
    sha256: String,
    building: String,
    bytes: u64,
}

enum Outcome {
    Row(Row),
    Skipped { seed: u64, code: &'static str, message: String },
}

/// The rooms a seed is asked for: between `lo` and `hi`, by the seed alone. Past deliveries depend on this never changing.
pub(super) fn rooms_for(seed: u64, lo: i64, hi: i64) -> i64 {
    let span = (hi - lo + 1).max(1) as u64;
    lo + (mix(seed ^ SALT) % span) as i64
}

/// The gross area, whole square metres, a seed is asked for: between `lo` and `hi`, by the seed alone. Past deliveries depend on
/// this never changing.
pub(super) fn area_for(seed: u64, lo: i64, hi: i64) -> i64 {
    let span = (hi - lo + 1).max(1) as u64;
    lo + (mix(seed ^ AREA_SALT) % span) as i64
}

/// Which `k` of `n` rows the rerun repeats: spread evenly through them, the first and last stretch included.
pub(super) fn rerun_indices(n: usize, k: usize) -> Vec<usize> {
    let k = k.min(n);
    (0..k).map(|i| ((2 * i + 1) * n) / (2 * k)).collect()
}

fn bad(message: impl Into<String>) -> Refusal {
    refuse("invalid_brief", message)
}

fn params_error(message: impl Into<String>) -> Refusal {
    refuse("invalid_params", message)
}

fn read_capped(path: &str) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The brief's seeds as jobs, in seed order, or what is wrong with it.
fn read_brief(path: &str) -> Result<Vec<Job>, Refusal> {
    if path.trim().is_empty() || path.len() > MAX_PATH || path.contains('\0') {
        return Err(params_error("`brief` is the path of a brief JSON file"));
    }
    let bytes = read_capped(path).map_err(|e| bad(format!("the brief {path:?} can't be read: {e}")))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(bad(format!("a brief is at most {MAX_FILE} bytes")));
    }
    let brief: Brief = serde_json::from_slice(&bytes).map_err(|e| bad(format!("the brief: {e}")))?;
    if brief.schema_version != 1 {
        return Err(bad(format!("`schema_version` is 1, not {}", brief.schema_version)));
    }
    if brief.batches.is_empty() || brief.batches.len() > MAX_BATCHES {
        return Err(bad(format!("`batches` holds 1 to {MAX_BATCHES} batches")));
    }
    let max = shape_graph::MAX_ROOMS;
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let mut total = 0u64;
    for (i, b) in brief.batches.iter().enumerate() {
        let at = |what: &str| bad(format!("batch {i}: {what}"));
        match (&b.rooms, &b.area) {
            (Some(_), Some(_)) | (None, None) => return Err(at("give `rooms` or `area`, one of them")),
            (None, Some([lo, hi])) => {
                if Archetype::parse(&b.archetype).is_none() {
                    return Err(at(&format!(
                        "`archetype` is one of {} for a batch by `area`; got {:?}",
                        Archetype::names(),
                        b.archetype.chars().take(40).collect::<String>()
                    )));
                }
                if !(lo.is_finite() && hi.is_finite() && *lo > 0.0 && lo <= hi && *hi <= MAX_AREA_M2 && lo.ceil() <= hi.floor()) {
                    return Err(at(&format!(
                        "`area` is two square-metre areas, least first, above 0 and at most {MAX_AREA_M2}, with a whole number between them"
                    )));
                }
                if b.rooms_tol.is_some() {
                    return Err(at("`rooms_tol` belongs to a batch by `rooms`"));
                }
            }
            (Some([lo, hi]), None) => {
                if family::builtin(&b.archetype).is_none() {
                    return Err(at(&format!(
                        "`archetype` is one of {} for a batch by `rooms`; got {:?}",
                        family::builtin_names(),
                        b.archetype.chars().take(40).collect::<String>()
                    )));
                }
                if *lo < 1 || lo > hi || *hi > max {
                    return Err(at(&format!("`rooms` is two whole numbers, fewest first, from 1 to {max}")));
                }
            }
        }
        if b.rooms_tol.is_some_and(|t| !(0..=max).contains(&t)) {
            return Err(at(&format!("`rooms_tol` is from 0 to {max}")));
        }
        if b.unreviewed.as_deref().is_some_and(|u| u != "allow") {
            return Err(at("`unreviewed` is `allow`, or left out"));
        }
        if b.count < 1 {
            return Err(at("`count` is at least 1"));
        }
        let end = b.first_seed.checked_add(b.count).ok_or_else(|| at("`first_seed` + `count` is past the largest seed"))?;
        total = total.saturating_add(b.count);
        if total > MAX_SEEDS {
            return Err(bad(format!("a delivery is at most {MAX_SEEDS} seeds")));
        }
        ranges.push((b.first_seed, end));
    }
    ranges.sort_unstable();
    if let Some(w) = ranges.windows(2).find(|w| w[1].0 < w[0].1) {
        return Err(bad(format!("two batches share seeds: one ends at {} and the next starts at {}", w[0].1 - 1, w[1].0)));
    }
    let mut jobs = Vec::with_capacity(total as usize);
    for b in &brief.batches {
        let [lo, hi] = b.rooms.unwrap_or([1, 1]);
        let [alo, ahi] = b.area.unwrap_or([1.0, 1.0]);
        for seed in b.first_seed..b.first_seed + b.count {
            jobs.push(Job {
                archetype: b.archetype.clone(),
                seed,
                rooms: rooms_for(seed, lo, hi),
                area: b.area.map(|_| area_for(seed, alo.ceil() as i64, ahi.floor() as i64)),
                tol: b.rooms_tol.unwrap_or(0),
                unreviewed: b.unreviewed.is_some(),
            });
        }
    }
    jobs.sort_by_key(|j| j.seed);
    Ok(jobs)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_version(v: &str) -> bool {
    v.contains('+') && valid_id(&v.replace('+', "-"))
}

/// One seed: built in memory, run in a scratch session as `run --script` runs it, saved as DWG at `dwg`, read back for its hash.
fn make_one(job: &Job, writer: &crate::cad::DwgWriter, dir: &Path) -> Result<Outcome, String> {
    let built = match job.area {
        // A plan by area: the generator's own, drawn to the delivery rules, as `generate --delivery on` makes it.
        Some(area) => match Archetype::parse(&job.archetype)
            .ok_or_else(|| Refusal { code: "unknown_archetype", message: job.archetype.clone() })
            .and_then(|a| super::build_for_delivery(a, job.seed, AreaRequest::m2(area as f64)))
        {
            Ok(b) => b,
            Err(Refusal { code, message }) => return Ok(Outcome::Skipped { seed: job.seed, code, message }),
        },
        None => {
            let request = shape_graph::Request {
                archetype: job.archetype.clone(),
                seed: job.seed,
                rooms: job.rooms,
                tolerance: job.tol,
                shape: None,
                gridlines: false,
            };
            match shape_graph::build(&request, job.unreviewed) {
                Ok(b) => b,
                Err(Refusal { code, message }) => return Ok(Outcome::Skipped { seed: job.seed, code, message }),
            }
        }
    };
    let text = |name: &str| built.files.iter().find(|(n, _)| *n == name).map(|(_, t)| t.as_str());
    let script = text("script.bimcraft.txt").ok_or("generation_failed: the plan has no script")?;
    let manifest: Value = serde_json::from_str(text("manifest.json").unwrap_or("null")).unwrap_or(Value::Null);
    let building = manifest["building"].as_str().ok_or("generation_failed: the plan's manifest names no building")?.to_string();
    let name = format!("{}_s{}.dwg", job.archetype, job.seed);
    let at = dir.join("dwg").join(&name);
    crate::ids::keeping_reproducible(|| -> Result<(), String> {
        let mut scratch = Session::new();
        scratch.dwg_writer = Some(writer.clone());
        scratch.script(script).map_err(|e| format!("generation_failed: the plan's script does not run: {e}"))?;
        scratch.execute("save", &json!({"path": at.display().to_string()})).map_err(|e| format!("output_unwritable: {e}"))?;
        Ok(())
    })?;
    let bytes = std::fs::read(&at).map_err(|e| format!("output_unwritable: {}: {e}", at.display()))?;
    Ok(Outcome::Row(Row {
        seed: job.seed,
        file: format!("dwg/{name}"),
        sha256: crate::sha256::hex(&bytes),
        building,
        bytes: bytes.len() as u64,
    }))
}

/// Every job on `threads` plain threads, each pulling the next index, results by position. The reproducible-GUID state is
/// thread-local, so a seed's job never shares a thread's state with another's while it runs.
fn run_all(jobs: &[Job], dir: &Path, writer: &crate::cad::DwgWriter, threads: usize) -> Result<Vec<Outcome>, String> {
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let slots: Mutex<Vec<Option<Result<Outcome, String>>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads.min(jobs.len()).max(1) {
            scope.spawn(|| {
                loop {
                    if failed.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { return };
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| make_one(job, writer, dir)))
                        .unwrap_or_else(|_| Err(format!("generation_failed: seed {} stopped the generator", job.seed)));
                    if done.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[i] = Some(done);
                }
            });
        }
    });
    let slots = slots.into_inner().unwrap_or_else(PoisonError::into_inner);
    let mut out = Vec::with_capacity(slots.len());
    let mut first_error = None;
    for slot in slots {
        match slot {
            Some(Ok(o)) => out.push(o),
            Some(Err(e)) => {
                first_error.get_or_insert(e);
            }
            None => {}
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

fn csv_row(r: &Row, gv: &str) -> String {
    format!("{},{},{},{gv},mm,A-WALL,A-DOOR,A-GLAZ,{},1,", r.seed, r.file, r.sha256, r.building)
}

fn manifest(rows: &[&Row], gv: &str) -> String {
    let mut text = String::from(HEADER);
    text.push('\n');
    for r in rows {
        text.push_str(&csv_row(r, gv));
        text.push('\n');
    }
    text
}

/// What this run made, so a failure takes exactly that away again.
struct Made {
    folders: Vec<PathBuf>,
    version_dir: Option<PathBuf>,
}

impl Made {
    fn undo(&self) {
        for f in &self.folders {
            let _ = std::fs::remove_dir_all(f);
        }
        if let Some(v) = &self.version_dir {
            let _ = std::fs::remove_dir(v);
        }
    }
}

/// The `deliver` command.
pub fn deliver(s: &mut Session, p: &Value) -> Result<Value, String> {
    run(s, p).map_err(|r| r.to_string())
}

fn run(s: &Session, p: &Value) -> Result<Value, Refusal> {
    // Everything is checked before the disk is touched, in this order.
    let given: Params = serde_json::from_value(p.clone()).map_err(|e| {
        params_error(format!(
            "{e}; `deliver` takes `brief`, `delivery_id`, `rights` (a file, or `emailed`), `path` (the root to write under), `rerun` and `jobs`"
        ))
    })?;
    if given.path.trim().is_empty() || given.path.len() > MAX_PATH || given.path.contains('\0') {
        return Err(params_error(format!("`path` is the root folder to write under: not empty, at most {MAX_PATH} bytes")));
    }
    if !valid_id(&given.delivery_id) {
        return Err(params_error("`delivery_id` is up to 100 letters, digits, `.`, `_` and `-`, and does not start with `.`"));
    }
    let rerun = given.rerun.unwrap_or(DEFAULT_RERUN);
    if !(1..=MAX_RERUN).contains(&rerun) {
        return Err(params_error(format!("`rerun` is from 1 to {MAX_RERUN}")));
    }
    let threads = match given.jobs {
        Some(j) if (1..=MAX_JOBS).contains(&j) => j as usize,
        Some(_) => return Err(params_error(format!("`jobs` is from 1 to {MAX_JOBS}"))),
        None => std::thread::available_parallelism().map_or(1, usize::from).clamp(1, MAX_JOBS as usize),
    };
    let rights: Option<Vec<u8>> = match given.rights.as_deref() {
        None => return Err(refuse("missing_rights", "give `rights`: the path of the signed rights statement, or `emailed`")),
        Some("emailed") => None,
        Some(file) => {
            let missing =
                |why: &str| refuse("missing_rights", format!("`rights` {file:?} {why}: give the signed statement's file, or `emailed`"));
            if file.is_empty() || file.len() > MAX_PATH || file.contains('\0') {
                return Err(missing("is not a file path"));
            }
            let bytes = read_capped(file).map_err(|e| missing(&format!("can't be read ({e})")))?;
            if bytes.is_empty() {
                return Err(missing("is empty"));
            }
            if bytes.len() as u64 > MAX_FILE {
                return Err(params_error(format!("a rights file is at most {MAX_FILE} bytes")));
            }
            Some(bytes)
        }
    };
    let jobs = read_brief(&given.brief)?;
    let gv = s
        .generator_version
        .as_deref()
        .filter(|v| valid_version(v))
        .ok_or_else(|| params_error("this build has no generator version (`bimcraft-<release>+<commit>`), so it can't deliver"))?;
    let writer = s.dwg_writer.clone().ok_or_else(|| params_error("this build can't write DWG, so it can't deliver"))?;
    let version_dir = Path::new(&given.path).join(gv);
    let (dfold, rfold) = (version_dir.join(&given.delivery_id), version_dir.join(format!("{}-rerun", given.delivery_id)));
    for f in [&dfold, &rfold] {
        if std::fs::symlink_metadata(f).is_ok() {
            return Err(refuse("delivery_exists", format!("{} is already there; a fix is a new `delivery_id`", f.display())));
        }
    }

    let made_version = std::fs::symlink_metadata(&version_dir).is_err();
    let mut made = Made { folders: Vec::new(), version_dir: made_version.then(|| version_dir.clone()) };
    let result = write_delivery(&given, &jobs, rights.as_deref(), gv, &writer, threads, rerun as usize, (&dfold, &rfold), &mut made);
    if result.is_err() {
        made.undo();
    }
    result
}

fn io(path: &Path, e: std::io::Error) -> Refusal {
    refuse("output_unwritable", format!("{}: {e}", path.display()))
}

fn from_message(e: String) -> Refusal {
    match e.split_once(": ") {
        Some((code, message)) if ["generation_failed", "output_unwritable"].contains(&code) => Refusal {
            code: if code == "generation_failed" { "generation_failed" } else { "output_unwritable" },
            message: message.to_string(),
        },
        _ => refuse("generation_failed", e),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_delivery(
    given: &Params,
    jobs: &[Job],
    rights: Option<&[u8]>,
    gv: &str,
    writer: &crate::cad::DwgWriter,
    threads: usize,
    rerun: usize,
    (dfold, rfold): (&Path, &Path),
    made: &mut Made,
) -> Result<Value, Refusal> {
    let version_dir = dfold.parent().ok_or_else(|| params_error("`path` has no parent"))?;
    std::fs::create_dir_all(version_dir).map_err(|e| io(version_dir, e))?;
    for f in [dfold, rfold] {
        std::fs::create_dir(f).map_err(|e| io(f, e))?;
        made.folders.push(f.to_path_buf());
        std::fs::create_dir(f.join("dwg")).map_err(|e| io(f, e))?;
    }
    let outcomes = run_all(jobs, dfold, writer, threads).map_err(from_message)?;
    let mut rows: Vec<&Row> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    for o in &outcomes {
        match o {
            Outcome::Row(r) => rows.push(r),
            Outcome::Skipped { seed, code, message } => skipped.push(json!({"seed": seed, "error": code, "message": message})),
        }
    }
    if rows.is_empty() {
        return Err(refuse(
            "empty_delivery",
            "every seed of the brief was skipped, so there is nothing to deliver (see the summary's `skipped` in the first error)",
        ));
    }
    // The rerun: a spread of the delivered seeds, made again from the brief.
    let by_seed: BTreeMap<u64, &Job> = jobs.iter().map(|j| (j.seed, j)).collect();
    let picked: Vec<Job> = rerun_indices(rows.len(), rerun.min(rows.len()))
        .into_iter()
        .filter_map(|i| by_seed.get(&rows[i].seed).map(|j| (*j).clone()))
        .collect();
    let again = run_all(&picked, rfold, writer, threads).map_err(from_message)?;
    let mut rerun_rows: Vec<&Row> = Vec::new();
    for o in &again {
        match o {
            Outcome::Row(r) => rerun_rows.push(r),
            Outcome::Skipped { seed, .. } => {
                return Err(refuse("nondeterministic", format!("seed {seed} was delivered, and refused the second time")));
            }
        }
    }
    for r in &rerun_rows {
        let first = rows.iter().find(|d| d.seed == r.seed).map(|d| csv_row(d, gv));
        if first.as_deref() != Some(csv_row(r, gv).as_str()) {
            return Err(refuse("nondeterministic", format!("seed {} made a different file the second time", r.seed)));
        }
    }
    for (folder, list) in [(dfold, &rows), (rfold, &rerun_rows)] {
        let at = folder.join("manifest.csv");
        std::fs::write(&at, manifest(list, gv)).map_err(|e| io(&at, e))?;
        if let Some(bytes) = rights {
            let at = folder.join("RIGHTS.txt");
            std::fs::write(&at, bytes).map_err(|e| io(&at, e))?;
        }
    }
    let files = rows.len();
    let total: u64 = rows.iter().map(|r| r.bytes).sum();
    let buildings = rows.iter().map(|r| r.building.as_str()).collect::<BTreeSet<_>>().len();
    let mut sources = serde_json::Map::new();
    for job in jobs.iter().filter(|j| j.area.is_none()) {
        if !sources.contains_key(&job.archetype) {
            let fam = shape_graph::builtin_family(&job.archetype)?;
            sources.insert(job.archetype.clone(), json!(fam.sources));
        }
    }
    let _ = &given.brief;
    Ok(json!({
        "folder": dfold.display().to_string(),
        "rerun_folder": rfold.display().to_string(),
        "generator_version": gv,
        "files": files,
        "buildings": buildings,
        "average_file_bytes": total as f64 / files as f64,
        "skipped": skipped,
        "sources": sources,
    }))
}
