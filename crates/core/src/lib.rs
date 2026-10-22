//! BIMCraft core: the building model, the command engine every front end drives, plan views
//! rendered to SVG and PNG, sections, elevations, the 3D view, sheets printed to PDF, schedules
//! and IFC4 export. Nothing here knows about a UI.

pub mod autosave;
#[cfg(test)]
mod autosave_tests;
mod commands;
mod csg;
pub mod engine;
pub mod family;
pub mod favorites;
mod fixtures;
mod font;
pub mod geom;
pub mod graphics;
pub mod ids;
pub mod ifc;
pub mod import;
pub mod inquiry;
pub mod insert;
pub mod language;
pub mod lenient;
pub mod library;
pub mod massing;
pub mod model;
pub mod paint;
pub mod pdf;
pub mod phasing;
pub mod render;
pub mod runs;
pub mod sample;
pub mod schedule;
pub mod sheet;
pub mod skeleton;
pub mod snap;
pub mod stairs;
pub mod step;
pub mod texture;
pub mod units;
pub mod view3d;
pub mod walls;

pub use commands::{command_of, element_json, floor_outline, opening_of, roof_footprint, run_category_of, runs_of, stood_of};

#[cfg(test)]
mod mep_tests;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod tests;
