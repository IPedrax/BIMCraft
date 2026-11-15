//! The area-driven generator (`graph-generator/1`): a brief of an archetype, a gross area and a seed in, a run directory's
//! files out. The office, clinic and hospital are laid out along a corridor (`layout`, `lift`, `openings`, `script` and
//! `report`); the house, studio office, office pattern and medical day department are graph first (`house`, `studio`,
//! `pattern` and `medical`). The `generate` command in `crate::generate` sends a brief by an area here; what both engines
//! use (the files, `Built`, `Refusal`, the graph types, the archetypes, the checks, the gridlines, the door swings) is there.
//! Detail: `docs/areas/graph-generator.md`.

pub(crate) mod delivery;
pub(crate) mod graph;
pub(crate) mod gridlines;
pub(crate) mod house;
pub(crate) mod layout;
mod lift;
pub(crate) mod medical;
mod openings;
pub(crate) mod pattern;
pub(crate) mod program;
pub(crate) mod report;
mod script;
pub(crate) mod studio;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::generate::{Built, FILES, MANIFEST, Refusal, Rng, fnv1a64, pretty, refuse};
use graph::{SCHEMA_VERSION, Topology};
use lift::Building;
use program::Archetype;

/// Changes when the same brief would give different files.
pub const GENERATOR_VERSION: &str = "graph-generator/1";
/// The largest gross area a brief may ask for, m2: past it the run directory is no longer a plan a person reads.
pub const MAX_AREA_M2: f64 = 5000.0;
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

/// A brief by an area, as the `generate` command takes it: one of `area_m2` or `area_sqft`, then the gridlines when they are
/// not the archetype's default, then the delivery drawing rules.
pub(crate) fn area_brief(
    archetype: Archetype,
    seed: u64,
    area_m2: Option<f64>,
    area_sqft: Option<f64>,
    gridlines: Option<bool>,
    delivery: bool,
) -> Result<Built, Refusal> {
    let area = match (area_m2, area_sqft) {
        (Some(_), Some(_)) => return Err(refuse("invalid_params", "`area_m2` and `area_sqft` are mutually exclusive; give one")),
        (Some(m2), None) => AreaRequest::m2(m2),
        (None, Some(sqft)) => AreaRequest::sqft(sqft),
        (None, None) => return Err(refuse("invalid_params", "`generate` needs one of `area_m2` or `area_sqft`")),
    };
    let mut built = build_with_area(archetype, seed, area)?;
    // The gridlines are edited first, so the delivery pass runs on the script that ships. A value that is the archetype's
    // default changes nothing, so the default files stay as they were.
    if let Some(on) = gridlines.filter(|on| *on != archetype.draws_grid()) {
        built = gridlines::apply(built, on)?;
    }
    if delivery {
        built = delivery::apply(built)?;
    }
    Ok(built)
}
