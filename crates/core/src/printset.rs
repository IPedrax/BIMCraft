//! Print sets, as Revit's View/Sheet Sets: a named list of the views and sheets printed or exported together, any of them
//! and any mix. A set is a library entry ([`crate::library::Kind::PrintSet`]): its name is unique, and it has an id and a
//! GUID as every entry has. It holds its views by GUID, in the order they print; a view deleted leaves every set it was in
//! ([`crate::model::Model::prune_print_sets`]).

use serde::{Deserialize, Serialize};

use crate::export::{self, Kind};
use crate::ids::Guid;
use crate::model::Model;

/// The most sets a project holds.
pub const MAX_SETS: usize = 1000;
/// The most views one set holds.
pub const MAX_VIEWS: usize = 10_000;
/// The longest name, in characters.
pub const MAX_NAME: usize = 128;

/// A named set of views and sheets.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrintSet {
    /// The views and sheets in it, in the order they print.
    pub views: Vec<Guid>,
}

/// Whether the element `g` is something a set may hold: a plan, a section, an elevation, a 3D view, a sheet or a schedule.
pub fn printable(m: &Model, g: Guid) -> bool {
    m.elements.id(g).is_some_and(|id| matches!(export::kind_of(m, Some(id)), Some(k) if k != Kind::Project))
}

impl PrintSet {
    /// What is wrong with set `name`, if anything.
    pub fn check(&self, name: &str) -> Result<(), String> {
        if name.trim().is_empty() || name.chars().count() > MAX_NAME {
            return Err(format!("a print set's name is 1 to {MAX_NAME} characters, not `{name}`"));
        }
        if self.views.len() > MAX_VIEWS {
            return Err(format!("print set `{name}` holds {} views; a set holds up to {MAX_VIEWS}", self.views.len()));
        }
        let mut seen = std::collections::BTreeSet::new();
        for g in &self.views {
            if !seen.insert(*g) {
                return Err(format!("print set `{name}` holds {g} twice"));
            }
        }
        Ok(())
    }
}

impl Model {
    /// Whether a set holds a view the project no longer has.
    pub fn print_sets_dangle(&self) -> bool {
        self.print_sets.values().flat_map(|s| &s.views).any(|g| !self.elements.contains_key(*g))
    }

    /// Takes out of every set the views the project no longer has.
    pub fn prune_print_sets(&mut self) {
        let Model { print_sets, elements, .. } = self;
        for s in print_sets.values_mut() {
            s.views.retain(|g| elements.contains_key(*g));
        }
    }
}
