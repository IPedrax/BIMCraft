//! The shape graph generator: a plan from a fixed catalogue of building outlines and a family file that says which rooms the
//! family has and how they connect. `generate --archetype FAMILY --seed N --rooms R` lays the rooms out along a spine inside one of
//! nine outlines, scaled to hold them, mirrored and turned by the seed. Its code holds no room kind of any family: the kinds, the
//! counts, the sizes and the links come from `data/shape-families/<family>.json`. Detail: `docs/areas/graph-generator.md`.

#[cfg(test)]
mod building_tests;
#[cfg(test)]
mod connection_tests;
mod contents;
mod doors;
mod emit;
pub(crate) mod family;
#[cfg(test)]
mod file_tests;
mod fit;
#[cfg(test)]
mod interior_tests;
mod layout;
mod links;
#[cfg(test)]
mod office_family_tests;
mod plan;
#[cfg(test)]
pub(crate) mod register_tests;
#[cfg(test)]
mod required_tests;
mod tags;
#[cfg(test)]
mod tests;
pub(crate) mod wall_lines;
#[cfg(test)]
mod wall_lines_tests;
#[cfg(test)]
mod window_tests;

use serde_json::{Value, json};

use super::graph::SCHEMA_VERSION;
use super::program::Archetype;
use super::report::{Check, Status};
use super::{Built, FILES, MANIFEST, Refusal, fnv1a64, pretty, refuse};
use family::{Family, Shape};
use fit::{Fail, Fitter, Rnd};

/// Changes when the same arguments would give different files.
pub const GENERATOR_VERSION: &str = "shape-graph/1";
/// Attempts before the engine refuses. It never stops on a clock.
pub const ATTEMPTS: u64 = 400;
/// Attempts each shape gets, when none is pinned, before the next is tried.
const SHAPE_TRIES: u64 = 80;
/// The most the read-back may disagree with the plan before the engine refuses: a disagreement is a bug to fix, not a case to retry.
const MAX_DISAGREEMENTS: usize = 3;
/// The least share of the outline's area the rooms' net areas must make: walls and the spine may take the rest.
const FILL_MIN: f64 = 0.87;
/// The most rooms a plan may ask for.
pub const MAX_ROOMS: i64 = 200;

/// What a call asks for.
#[derive(Clone, Debug)]
pub struct Request {
    pub archetype: String,
    pub seed: u64,
    pub rooms: i64,
    pub tolerance: i64,
    pub shape: Option<String>,
    /// Draw gridlines at the outline's corners.
    pub gridlines: bool,
    /// Whether neighbours facing a spine room line up (`Aligned`), some are set back (`Jogged`), or the seed chooses.
    pub interior: Interior,
    /// Whether one to three rooms push out past the outline or pull in from it.
    pub bumps: bool,
}

/// `--interior`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interior {
    Aligned,
    Jogged,
    Mixed,
}

impl Interior {
    pub fn parse(text: &str) -> Option<Interior> {
        match text {
            "aligned" => Some(Interior::Aligned),
            "jogged" => Some(Interior::Jogged),
            "mixed" => Some(Interior::Mixed),
            _ => None,
        }
    }
}

impl Request {
    /// A request with the options' defaults, which `generate` and `deliver` share: a mixed interior and bumps on.
    pub fn new(archetype: String, seed: u64, rooms: i64, tolerance: i64, shape: Option<String>, gridlines: bool) -> Request {
        Request { archetype, seed, rooms, tolerance, shape, gridlines, interior: Interior::Mixed, bumps: true }
    }
}

/// The `building` of a plan: the family's name and the first 12 hex digits of a hash of its layout (types, families, walls,
/// doors without their marks, separators and rooms), each in sorted order. The title, the seed and the drawing's settings are
/// not in it, so two scripts that draw the same layout have the same building.
pub(crate) fn building_id<S: AsRef<str>>(family: &str, lines: &[(S, Value)]) -> String {
    let mut kept: Vec<String> = Vec::new();
    for (id, params) in lines {
        let id = id.as_ref();
        if !matches!(id, "type" | "family" | "wall" | "door" | "room_separator" | "room") {
            continue;
        }
        let mut params = params.clone();
        if id == "door"
            && let Some(o) = params.as_object_mut()
        {
            o.remove("mark");
        }
        kept.push(format!("{id} {params}"));
    }
    kept.sort();
    let digest = crate::sha256::hex(kept.join("\n").as_bytes());
    format!("{family}-{}", &digest[..12])
}

/// One attempt: the plan laid out, or why not.
fn attempt(fam: &Family, shape: &Shape, requested: &Request, k: u64) -> Result<(Vec<plan::Room>, layout::Laid), Fail> {
    let mut rng = Rnd::new(requested.seed, k);
    let (lo, hi) = fam.count_limits();
    let (from, to) = ((requested.rooms - requested.tolerance).max(lo), (requested.rooms + requested.tolerance).min(hi));
    let count = from + rng.below((to - from + 1).max(1) as usize) as i64;
    let rooms = plan::program(fam, count, &mut rng)?;
    let g = plan::graph(fam, &rooms, &mut rng)?;
    // A mixed interior is chosen by the seed alone, from a source of its own: every attempt of a seed has the same interior (a jogged
    // plan takes more attempts, so a choice made in each would favour aligned plans), and the attempt's own numbers are untouched.
    let jogged = match requested.interior {
        Interior::Aligned => false,
        Interior::Jogged => true,
        Interior::Mixed => Rnd::new(requested.seed, u64::MAX - 1).random() < 0.5,
    };
    let room_ty: Vec<usize> = rooms.iter().map(|r| r.ty).collect();
    let target: Vec<f64> = rooms.iter().map(|r| r.target).collect();
    let fx = Fitter::new(&fam.types, &room_ty, &target);
    let opts = layout::Options { jogged, bumps: requested.bumps };
    let laid = layout::layout(fam, &rooms, &g, &fx, shape, opts, &mut rng)?;
    if laid.verified.net_m2 < FILL_MIN * laid.verified.gross_m2 {
        return Err(Fail("rooms fill too little of the outline"));
    }
    Ok((rooms, laid))
}

/// The files for a call, or why there are none. Touches no file.
pub fn build(req: &Request, allow_unreviewed: bool) -> Result<Built, Refusal> {
    build_styled(req, &wall_lines::Pins::default(), allow_unreviewed)
}

/// `build`, with some of the wall lines pinned.
pub fn build_styled(req: &Request, pins: &wall_lines::Pins, allow_unreviewed: bool) -> Result<Built, Refusal> {
    let fam = builtin_family(&req.archetype)?;
    build_family_styled(&fam, req, pins, allow_unreviewed)
}

/// A built-in family by name.
pub fn builtin_family(name: &str) -> Result<Family, Refusal> {
    // The message lists the families there are and does not repeat the name asked for.
    let text = family::builtin(name)
        .ok_or_else(|| refuse("unknown_archetype", format!("`archetype` is one of {} for a plan by `--rooms`", family::builtin_names())))?;
    Family::parse(text).map_err(|e| refuse("invalid_family", e))
}

#[cfg(test)]
pub fn build_family(fam: &Family, req: &Request, allow_unreviewed: bool) -> Result<Built, Refusal> {
    build_family_styled(fam, req, &wall_lines::Pins::default(), allow_unreviewed)
}

pub fn build_family_styled(fam: &Family, req: &Request, pins: &wall_lines::Pins, allow_unreviewed: bool) -> Result<Built, Refusal> {
    pins.check().map_err(|e| refuse("invalid_params", e))?;
    // From a source of its own: no attempt's numbers are touched, so the layout is the same whatever the style.
    let lines = wall_lines::choose(req.seed, pins);
    if fam.unreviewed() && !allow_unreviewed {
        return Err(refuse(
            "unreviewed_family",
            format!("the `{}` family is marked unreviewed: it may be used with `--unreviewed allow`", fam.name),
        ));
    }
    let shapes = family::shapes().map_err(|e| refuse("generation_failed", e))?;
    let pinned = match &req.shape {
        Some(id) => Some(shapes.iter().find(|s| s.id == *id).ok_or_else(|| {
            refuse(
                "unknown_shape",
                format!(
                    "`shape` is one of {}; got {:?}",
                    shapes.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", "),
                    id.chars().take(40).collect::<String>()
                ),
            )
        })?),
        None => None,
    };
    let (lo, hi) = fam.count_limits();
    if req.rooms + req.tolerance < lo || req.rooms - req.tolerance > hi {
        return Err(refuse(
            "unsatisfiable_rooms",
            format!(
                "no room program of the `{}` family has between {} and {} rooms: the family makes at least {lo} and at most {hi}",
                fam.name,
                req.rooms - req.tolerance,
                req.rooms + req.tolerance
            ),
        ));
    }
    // With no shape pinned, the seed puts the shapes in an order. The first gets SHAPE_TRIES attempts, then the next: every shape
    // is drawn about as often as any other, and one that cannot hold the program is passed over.
    let mut order: Vec<usize> = (0..shapes.len()).collect();
    Rnd::new(req.seed, u64::MAX).shuffle(&mut order);
    let mut disagreements = 0;
    let mut last = String::from("no attempt was made");
    for k in 0..ATTEMPTS {
        let shape = match pinned {
            Some(s) => s,
            None => &shapes[order[(k / SHAPE_TRIES) as usize % order.len()]],
        };
        let (rooms, laid) = match attempt(fam, shape, req, k) {
            Ok(done) => done,
            Err(Fail(why)) => {
                last = why.to_string();
                continue;
            }
        };
        match emit::verify(fam, &rooms, &laid, &shape.id, req.seed, req.gridlines, &lines) {
            Ok(done) => return finish(fam, req, &shape.id, &rooms, &laid, done, &lines),
            Err(emit::Trouble::Readback(e)) => {
                last = e;
                disagreements += 1;
                if disagreements >= MAX_DISAGREEMENTS {
                    break;
                }
            }
            Err(emit::Trouble::Plan(e)) => last = e,
        }
    }
    let pinned_msg = pinned.map_or_else(String::new, |s| format!(" in the `{}` outline", s.id));
    let code = if pinned.is_some() { "unplaceable_shape" } else { "unplaceable_graph" };
    Err(refuse(
        code,
        format!(
            "no layout of {} rooms{pinned_msg} was found for seed {} ({}): {last}. Try another seed{}.",
            req.rooms,
            req.seed,
            if disagreements >= MAX_DISAGREEMENTS { "the read-back disagreed with the plan" } else { "every attempt failed" },
            if pinned.is_some() { ", another outline or fewer rooms" } else { " or fewer rooms" }
        ),
    ))
}

fn finish(
    fam: &Family,
    req: &Request,
    shape: &str,
    rooms: &[plan::Room],
    laid: &layout::Laid,
    done: emit::Done,
    lines: &wall_lines::WallLines,
) -> Result<Built, Refusal> {
    let topology_text = pretty(&done.graph)?;
    let topology_hash = fnv1a64(&topology_text);
    let edges = done.graph["edges"].as_array().map_or(0, Vec::len);
    let label = emit::names(fam, rooms);
    let measured: Vec<Value> = done
        .script
        .rooms
        .iter()
        .filter_map(|(at, room)| {
            Some(json!({"space": rooms[*room].id, "number": label[*room].1, "net_area_m2": done.replies.get(*at)?.get("area_m2")?}))
        })
        .collect();
    let net: f64 = measured.iter().filter_map(|m| m["net_area_m2"].as_f64()).sum();
    let gross = laid.verified.gross_m2;
    let count = |s: Status| done.checks.iter().filter(|c| c.status == s).count();
    let report = json!({
        "schema_version": SCHEMA_VERSION,
        "engine": "shape-graph",
        "archetype": fam.name,
        "seed": req.seed,
        "shell": shape,
        "rule_set": fam.pack_id.as_ref().map(|p| format!("{p}-pack")),
        "summary": {"pass": count(Status::Pass), "warn": count(Status::Warn), "fail": count(Status::Fail)},
        "checks": done.checks.iter().map(|c| json!({"id": c.id, "name": c.name, "status": match c.status { Status::Pass => "PASS", Status::Warn => "WARN", Status::Fail => "FAIL" }, "detail": c.detail}))
            .chain(done.rules.iter().cloned()).collect::<Vec<_>>(),
        "rules": done.rules,
        "capability_notes": [
            "The room program and the relationships come from the family file; the layout puts the rooms along a spine inside one of the catalogue's outlines, then mirrors and turns the plan. The topology extractor, run on the built project, gives the generated graph.",
            "A room's area is measured to the wall finish faces; the building file's rectangles are to wall centre lines.",
        ],
        "scores": {"room_count": rooms.len(), "net_to_gross": if gross > 0.0 { (net / gross * 1000.0).round() / 1000.0 } else { 0.0 }},
        "measured_rooms": measured,
    });
    let texts = [
        pretty(&emit::program_json(fam, rooms, laid, req.seed, req.rooms, req.tolerance))?,
        topology_text,
        pretty(&emit::building_json(fam, rooms, laid, &done.drawn, shape, req.seed, &topology_hash, edges))?,
        done.script.text.clone(),
        pretty(&report)?,
    ];
    let named: Vec<Value> =
        FILES.iter().zip(&texts).map(|((key, name), text)| json!({"key": key, "path": name, "hash": fnv1a64(text)})).collect();
    let round2 = |v: f64| (v * 100.0).round() / 100.0;
    let manifest = pretty(&json!({
        "schema_version": SCHEMA_VERSION,
        "engine": "shape-graph",
        "generator_version": GENERATOR_VERSION,
        "family": fam.name,
        "building": building_id(&fam.name, &done.script.lines),
        "seed": req.seed,
        "rooms": {"requested": req.rooms, "tolerance": req.tolerance, "actual": rooms.len()},
        "shape": shape,
        "gridlines": req.gridlines,
        "wall_lines": lines.manifest_json(),
        "mirror": laid.mirror,
        "quarter_turns": laid.quarter_turns,
        "interior": if laid.jogged { "jogged" } else { "aligned" },
        "bumps": laid.bumps.iter().map(|b| json!({"room": rooms[b.room].id, "side": b.side, "offset_mm": b.offset})).collect::<Vec<_>>(),
        "base_outline": laid.base_outline.iter().map(|(x, y)| json!([x, y])).collect::<Vec<_>>(),
        "net_area_m2": round2(net),
        "gross_area_m2": gross,
        "hash_algorithm": "fnv1a64",
        "topology_hash": topology_hash,
        "files": named,
    }))?;
    let mut files: Vec<(&'static str, String)> = FILES.iter().map(|(_, name)| *name).zip(texts).collect();
    files.push((MANIFEST, manifest));
    let checks: Vec<Check> = done.checks;
    Ok(Built {
        archetype: Archetype::parse(&fam.name).unwrap_or(Archetype::House),
        seed: req.seed,
        plan: None,
        gross_m2: gross,
        rooms: rooms.len(),
        checks,
        files,
        topology_hash,
    })
}
