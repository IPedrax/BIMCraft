//! BIMCraft core: the building model, the command engine every front end drives, plan views
//! rendered to SVG and PNG, sections, elevations, the 3D view, sheets printed to PDF, schedules
//! and IFC4 export. Nothing here knows about a UI.

pub mod adaptive;
pub mod align;
#[cfg(test)]
mod align_tests;
pub mod alignment;
pub mod autosave;
#[cfg(test)]
mod autosave_tests;
pub mod background_grid;
pub mod bath;
pub mod bounds;
pub mod bridge;
pub mod cad;
#[cfg(test)]
mod cad_tests;
#[cfg(test)]
mod circuit_tests;
pub mod cloud;
#[cfg(test)]
mod cloud_tests;
mod commands;
mod component_geometry;
pub mod conduit;
mod constraints;
pub mod coordinates;
#[cfg(test)]
mod coordinates_io_tests;
#[cfg(test)]
mod coordinates_tests;
pub mod cowork;
#[cfg(test)]
mod cowork_tests;
pub mod crosscheck;
pub mod crs;
mod csg;
pub mod curves;
pub mod demand;
pub mod derived;
pub mod dims;
pub mod dims3d;
mod doors;
pub mod dps;
pub mod dr;
pub mod drainage;
pub mod draw;
#[cfg(test)]
mod dry_run_tests;
pub mod dxf;
#[cfg(test)]
mod eberick_tests;
pub mod electrical;
#[cfg(test)]
mod electrical_tests;
pub mod elevation;
pub mod engine;
pub mod export;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod face_host_tests;
#[cfg(test)]
mod face_void_tests;
pub mod falls;
pub mod family;
#[cfg(test)]
mod family_angle_tests;
#[cfg(test)]
mod family_blend_tests;
#[cfg(test)]
mod family_constraint_tests;
#[cfg(test)]
mod family_nested_tests;
#[cfg(test)]
mod family_options_tests;
#[cfg(test)]
mod family_parameter_tests;
#[cfg(test)]
mod family_pivot_tests;
#[cfg(test)]
mod family_plane_tests;
#[cfg(test)]
mod family_section_tests;
#[cfg(test)]
mod family_sketch_lock_tests;
#[cfg(test)]
mod family_sketch_tests;
#[cfg(test)]
mod family_visibility_tests;
pub mod favorites;
#[cfg(test)]
mod file_extension_tests;
pub mod files;
pub mod fills;
mod fixtures;
mod font;
#[cfg(test)]
mod formula_visibility_tests;
pub mod geom;
#[cfg(test)]
mod georef_tests;
pub mod gltf;
pub mod glue;
#[cfg(test)]
mod glue_tests;
pub mod graphics;
pub mod groups;
pub mod handles;
#[cfg(test)]
mod host_void_tests;
pub mod hosting;
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
pub mod memory;
pub mod mesh_in;
#[cfg(test)]
mod mesh_in_tests;
pub mod model;
pub mod nbr5410;
pub mod network;
pub mod ods;
#[cfg(test)]
mod opening_subcategory_tests;
#[cfg(test)]
mod openings_past_top_tests;
#[cfg(test)]
mod oracle_tests;
pub mod paint;
pub mod panel_schedule;
#[cfg(test)]
mod parameter_tests;
pub mod parameters;
pub mod pat;
pub mod pdf;
pub mod pen;
pub mod phasing;
pub mod places;
pub mod plumbing;
pub mod printset;
pub mod project_info;
#[cfg(test)]
mod project_info_tests;
#[cfg(test)]
mod pset_tests;
pub mod publish;
#[cfg(test)]
mod publish_tests;
#[cfg(test)]
mod raised_wall_room_tests;
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
mod room_finish_tests;
#[cfg(test)]
mod room_freeze_tests;
#[cfg(test)]
mod room_height_tests;
#[cfg(test)]
mod room_of_tests;
pub mod rooms;
#[cfg(test)]
mod run_join_tests;
pub mod runs;
pub mod sample;
pub mod schedule;
#[cfg(test)]
mod section_box_tests;
pub mod selection;
#[cfg(test)]
mod shape_tests;
pub mod shared;
#[cfg(test)]
mod shared_parameter_tests;
pub mod shared_parameters;
pub mod sheet;
pub mod site;
pub mod skeleton;
pub mod sketch;
pub mod snap;
#[cfg(test)]
mod spot_coordinate_tests;
pub mod spots;
pub mod stairs;
pub mod standards;
pub mod step;
pub mod storm;
pub mod styles;
pub mod symbol_set;
pub mod text;
pub mod texture;
mod timber;
pub mod titleblock;
#[cfg(test)]
mod titleblock_tests;
pub mod triad;
pub mod triangulation;
pub mod units;
pub mod view3d;
#[cfg(test)]
mod wall_top_offset_tests;
pub mod walls;
pub mod water;
#[cfg(test)]
mod work_plane_dialog_tests;
#[cfg(test)]
mod work_plane_tests;
pub mod workplane;
#[cfg(test)]
mod workplane_tests;
pub mod world;
pub mod xlsx;

pub use commands::{
    command_of, element_json, element_json_with, floor_outline, opening_of, roof_footprint, run_category_of, runs_of, stood_of,
};

#[cfg(test)]
mod connector_tests;
#[cfg(test)]
mod detail_tests;
#[cfg(test)]
mod dimension_edit_tests;
#[cfg(test)]
mod dimension_tests;
#[cfg(test)]
mod line_tests;
#[cfg(test)]
mod mep_tests;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod panel_schedule_tests;
#[cfg(test)]
mod panel_tests;
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

#[cfg(test)]
mod profile_tests;
pub mod profiles;
