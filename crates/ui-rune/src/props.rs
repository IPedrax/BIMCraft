//! The Properties palette's logic, with no window: what egui's palette (`crates/ui-egui/src/properties.rs`) shows and does, as data and
//! pure functions, so it is tested without one and every front end could use it (docs/areas/rune-ui-properties.md has the inventory).
//!
//! - `kind.rs`: the editor kinds, and how a value is shown and typed back, lengths and areas in the project's units, every number and size
//!   checked. `table.rs`: for each category a row for each parameter (egui's table, as data). `names.rs`: the names of the categories.
//! - `panel.rs`: `Panel::of`, the groups and rows a selection has, each with its text and its editor, values shared or blank. `apply.rs`:
//!   what an edit of a row becomes, a `modify` of the shown elements (or a `crop`, or a view's own command), or why it is none.
//! - `types.rs`: the Type Selector's list and the type the elements have. `view.rs`: the plan's own Graphics rows (scale, detail level, north).
//! - `plain.rs`: the first, plain rows of an element's JSON and its editable fields (`Row`, `Kind`, `editable`, `parse`, `rows`), which Type
//!   Properties and the generic command form still read. They are re-exported here; the palette's own editor kinds are `kind::Kind`.

mod plain;
pub use plain::{Kind, Row, editable, parse, rows};

pub mod apply;
pub mod kind;
pub mod names;
pub mod panel;
pub mod table;
pub mod types;
pub mod view;

#[cfg(test)]
mod tests;
