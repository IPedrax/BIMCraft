//! Project values and nested parameter associations follow a shared parameter's GUID when its name changes.
use super::{Family, Parameter};
use std::collections::{BTreeMap, BTreeSet};

impl Family {
    fn matching_parameter(&self, other: &Parameter) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.shared == other.shared && (p.shared.is_some() || p.name == other.name))
    }

    /// Which parameter of this definition can supply the incoming parameter's value on reload.
    pub(crate) fn reload_source(&self, incoming: &Parameter) -> Option<&Parameter> {
        self.matching_parameter(incoming).filter(|old| compatible_values(old, incoming))
    }
}

fn compatible_values(old: &Parameter, incoming: &Parameter) -> bool {
    old.kind == incoming.kind
        && old.instance == incoming.instance
        && old.of == incoming.of
        && !old.reporting
        && !incoming.reporting
        && old.formula.is_none()
        && incoming.formula.is_none()
}

/// A parameter can keep its identity even when its new definition (a formula, for example) can't take an old value.
#[derive(Default)]
pub(crate) struct ReloadNames {
    pub renamed: BTreeMap<String, String>,
    pub replaced: BTreeSet<String>,
    value_names: BTreeMap<String, String>,
    incompatible: BTreeSet<String>,
}

impl ReloadNames {
    pub fn between(old: &Family, incoming: &Family) -> Self {
        let mut names = Self::default();
        if !old.parameters.iter().chain(&incoming.parameters).any(|p| p.shared.is_some()) {
            return names;
        }
        for p in &old.parameters {
            if let Some(q) = incoming.matching_parameter(p) {
                if p.shared.is_none() {
                    continue;
                }
                if p.name != q.name {
                    names.renamed.insert(p.name.clone(), q.name.clone());
                    if compatible_values(p, q) {
                        names.value_names.insert(p.name.clone(), q.name.clone());
                    }
                }
                if !compatible_values(p, q) && !p.reporting && p.formula.is_none() {
                    names.incompatible.insert(p.name.clone());
                }
            } else if incoming.parameters.iter().any(|q| q.name == p.name) {
                // The old name now belongs to a different parameter. Its project identity and overrides can't transfer.
                names.replaced.insert(p.name.clone());
                names.incompatible.insert(p.name.clone());
            }
        }
        names
    }

    pub fn checks_values(&self) -> bool {
        !self.value_names.is_empty() || !self.incompatible.is_empty()
    }

    /// Read all the old keys before writing any new ones, including when two parameters exchange names.
    pub fn move_values<T>(&self, values: &mut BTreeMap<String, T>) -> Result<(), String> {
        if let Some(old) = self.incompatible.iter().find(|old| values.contains_key(*old)) {
            return Err(format!("the value of parameter `{old}` cannot be kept by the incoming definition"));
        }
        if let Some(to) = self.value_names.values().find(|to| values.contains_key(*to) && !self.value_names.contains_key(*to)) {
            return Err(format!("parameter `{to}` already holds a value under the incoming name"));
        }
        let moved: Vec<_> = self.value_names.iter().filter_map(|(from, to)| values.remove(from).map(|v| (to.clone(), v))).collect();
        values.extend(moved);
        Ok(())
    }
}
