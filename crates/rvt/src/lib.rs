//! Reads Autodesk Revit files (`.rvt`, `.rfa`), and puts a project's elements into a BIMCraft model ([`import`]).
//!
//! Revit's format has no public specification, so this crate is fenced: its code is Apache-2.0 only (some of it is
//! ported from rvt-rs), `bimcraft-core` never depends on it, and every module and format finding names its source in
//! `docs/rvt-provenance.md`. It reads a user's file and nothing more: it never bundles or extracts assets (families,
//! templates, icons, help text) for the project.

pub mod atom;
mod columns;
mod container;
pub mod curves;
pub mod data;
mod ducts;
pub mod elem_table;
mod error;
#[cfg(any(test, feature = "testing"))]
pub mod fixture;
mod floors;
mod generic;
pub mod gzip;
mod import;
pub mod info;
pub mod inventory;
mod names;
mod openings;
mod pipes;
mod placement;
pub mod preview;
#[cfg(any(test, feature = "testing"))]
pub mod project_fixture;
pub mod records;
mod roofs;
mod rooms;
pub mod schema;
mod sketch;
pub mod types;
mod walls;

pub use container::{BASIC_FILE_INFO, Limits, Revit};
pub use error::{Error, Result};
pub use import::{import, import_path, import_path_with_origins};

#[cfg(test)]
mod tests;
