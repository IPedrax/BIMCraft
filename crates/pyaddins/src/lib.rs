//! PyAddins: the user's own tools, in Python. A script is a `.py` file in the user's folder (`store`), never in a project;
//! the Player lists them and builds a form from what each one's `main` asks for (`inputs`); a button on the PyAddins tab
//! runs one with values it keeps. A script runs in the user's own Python (`python`: BIMCraft bundles none, and asks for
//! 3.9 or newer) in a process of its own (`run`), and changes the model only through commands, as an agent does: each
//! checked, refused when it would break the model, and never one that replaces the project.
//!
//! Agents write scripts and buttons too (the MCP server's `pyaddin_*` tools write the folder), and the window shows them
//! as they come. The write-up is [PyAddins](../../docs/areas/pyaddins.md).

pub mod editor;
pub mod inputs;
pub mod python;
pub mod run;
pub mod skill;
pub mod store;

pub use inputs::{Description, Input, Kind};
pub use python::Python;
pub use run::{Event, Outcome, Run};
pub use store::{Button, Store};
