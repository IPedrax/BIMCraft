//! BIMCraft core: the building model, the command engine every front end drives, plan views
//! rendered to SVG and PNG, sections, elevations, the 3D view, sheets printed to PDF, schedules
//! and IFC4 export. Nothing here knows about a UI.

pub mod align;
#[cfg(test)]
mod align_tests;
pub mod alignment;
pub mod autosave;
#[cfg(test)]
mod autosave_tests;
pub mod background_grid;
pub mod bounds;
pub mod bridge;
pub mod cad;
#[cfg(test)]
mod cad_tests;
pub mod cloud;
#[cfg(test)]
mod cloud_tests;
mod commands;
mod component_geometry;
pub mod conduit;
mod constraints;
pub mod crosscheck;
mod csg;
pub mod curves;
pub mod demand;
pub mod derived;
pub mod dims;
pub mod dims3d;
pub mod dps;
pub mod dr;
pub mod drainage;
pub mod draw;
pub mod dxf;
#[cfg(test)]
mod eberick_tests;
pub mod electrical;
#[cfg(test)]
mod electrical_tests;
pub mod engine;
pub mod falls;
pub mod family;
#[cfg(test)]
mod family_section_tests;
pub mod favorites;
pub mod fills;
mod fixtures;
mod font;
pub mod geom;
#[cfg(test)]
mod georef_tests;
pub mod gltf;
pub mod graphics;
pub mod groups;
pub mod hydrant;
pub mod ids;
pub mod ifc;
#[cfg(test)]
mod ifc_colour_tests;
#[cfg(test)]
mod ifc_generic_tests;
#[cfg(test)]
mod ifc_geometry_tests;
#[cfg(test)]
mod ifc_member_tests;
pub mod import;
pub mod inquiry;
pub mod insert;
pub mod language;
pub mod lenient;
pub mod library;
pub mod lines;
pub mod link;
pub mod loads;
pub mod massing;
pub mod model;
pub mod nbr5410;
#[cfg(test)]
mod openings_past_top_tests;
#[cfg(test)]
mod oracle_tests;
pub mod paint;
pub mod pat;
pub mod pdf;
pub mod pen;
pub mod phasing;
#[cfg(test)]
mod read_only_tests;
pub mod rebar;
#[cfg(test)]
mod rebar_tests;
pub mod reinforce;
#[cfg(test)]
mod reinforce_tests;
pub mod render;
pub mod reservoir;
#[cfg(test)]
mod roof_slopes_tests;
#[cfg(test)]
mod room_area_tests;
#[cfg(test)]
mod room_bounds_tests;
#[cfg(test)]
mod room_freeze_tests;
#[cfg(test)]
mod room_height_tests;
pub mod rooms;
pub mod runs;
pub mod sample;
pub mod schedule;
#[cfg(test)]
mod section_box_tests;
pub mod selection;
#[cfg(test)]
mod shape_tests;
pub mod shared;
pub mod sheet;
pub mod skeleton;
pub mod sketch;
pub mod snap;
pub mod stairs;
pub mod standards;
pub mod step;
pub mod storm;
pub mod styles;
pub mod symbol_set;
pub mod text;
pub mod texture;
pub mod triad;
pub mod triangulation;
pub mod units;
pub mod view3d;
#[cfg(test)]
mod wall_top_offset_tests;
pub mod walls;
pub mod water;
pub mod workplane;
#[cfg(test)]
mod workplane_tests;
pub mod xlsx;

pub use commands::{command_of, element_json, floor_outline, opening_of, roof_footprint, run_category_of, runs_of, stood_of};

#[cfg(test)]
mod detail_tests;
#[cfg(test)]
mod dimension_tests;
#[cfg(test)]
mod line_tests;
#[cfg(test)]
mod mep_tests;
#[cfg(test)]
mod plan_view_tests;
#[cfg(test)]
mod purge_tests;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;
