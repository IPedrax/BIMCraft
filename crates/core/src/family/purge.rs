//! Purge Unused in a family (FE-19): what nothing in it uses, offered to take out of its draft. A parameter or a nested family
//! is offered only where it is shown to be unused: its name appears nowhere else in the family as a file holds it (a formula,
//! a form's or a symbol's numbers, a label, a dimension, a type's value, a visibility formula, a connector, a nested
//! instance's parameters, the nested families' own definitions), it is no reporting or shared parameter, no placed
//! component holds a value of it, and the family passes its checks without it. What can't be shown unused isn't offered.

use serde_json::Value;

use super::Family;

/// What Purge Unused may take out of a family: its parameters and nested families nothing uses, in its order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unused {
    pub parameters: Vec<String>,
    pub nested: Vec<String>,
}

impl Unused {
    pub fn is_empty(&self) -> bool {
        self.parameters.is_empty() && self.nested.is_empty()
    }
}

/// Whether `name` is in any key or string of `v`: a formula, a label or a type's values naming it.
fn mentions(v: &Value, name: &str) -> bool {
    match v {
        Value::String(s) => s.contains(name),
        Value::Array(a) => a.iter().any(|x| mentions(x, name)),
        Value::Object(o) => o.iter().any(|(k, x)| k.contains(name) || mentions(x, name)),
        _ => false,
    }
}

impl Family {
    /// What nothing uses, as the module says: `held` says which parameters a placed component holds a value of,
    /// `has_material` which materials the project has (for the family's checks).
    pub fn unused(&self, held: &dyn Fn(&str) -> bool, has_material: &dyn Fn(&str) -> bool) -> Unused {
        let Ok(whole) = serde_json::to_value(self) else { return Unused::default() };
        let (errors, _) = self.problems(has_material);
        // The family without what is offered still checks as it did.
        let still = |f: &Family| f.problems(has_material).0 == errors;
        let mut out = Unused::default();
        for p in &self.parameters {
            if p.reporting || p.shared.is_some() || held(&p.name) || p.name.trim().is_empty() {
                continue;
            }
            let mut rest = whole.clone();
            if let Some(list) = rest.get_mut("parameters").and_then(Value::as_array_mut) {
                list.retain(|q| q.get("name").and_then(Value::as_str) != Some(p.name.as_str()));
            }
            if let Some(o) = rest.as_object_mut() {
                o.remove("name");
            }
            if mentions(&rest, &p.name) {
                continue;
            }
            let mut trial = self.clone();
            trial.purge(std::slice::from_ref(&p.name), &[]);
            if still(&trial) {
                out.parameters.push(p.name.clone());
            }
        }
        // A Family Type parameter can't pick a nested family yet; once it can, a nested family one may pick is kept.
        for n in &self.nested {
            if n.name.trim().is_empty() {
                continue;
            }
            let mut rest = whole.clone();
            if let Some(o) = rest.as_object_mut() {
                o.remove("nested");
                o.remove("name");
            }
            if mentions(&rest, &n.name) {
                continue;
            }
            let mut trial = self.clone();
            trial.purge(&[], std::slice::from_ref(&n.name));
            if still(&trial) {
                out.nested.push(n.name.clone());
            }
        }
        out
    }

    /// Parameters `parameters` and nested families `nested` taken out, a parameter's values in each type with it.
    pub fn purge(&mut self, parameters: &[String], nested: &[String]) {
        self.parameters.retain(|p| !parameters.contains(&p.name));
        for values in self.types.values_mut() {
            values.retain(|k, _| !parameters.contains(k));
        }
        self.nested.retain(|n| !nested.contains(&n.name));
    }
}
