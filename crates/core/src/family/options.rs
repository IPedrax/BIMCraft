//! A family's options, as Revit's Family Category and Parameters dialog has them: flags kept for what will read them
//! (work plane based, always vertical, cut with voids when loaded, shared), a room calculation point that decides which
//! room a component is in, whether sections cut its forms whatever its category says, and its OmniClass number. Each is a
//! flat field of [`Family`] left out of a file while it holds its default, so an older build reads the file as it was.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{Expr, Family, Kind, MAX_FORMULA, Val, WALL, names, number};
use crate::model::MAX_TEXT;

/// The longest OmniClass number a family keeps: Table 23's longest are a third of it.
pub const MAX_OMNICLASS: usize = 64;

/// The keys of a family's options, as a file and the `family` command's `options` name them.
pub const OPTION_KEYS: [&str; 7] =
    ["work_plane_based", "always_vertical", "cut_with_voids_when_loaded", "shared", "room_calculation_point", "cut_in_views", "omniclass"];

/// A family's OmniClass classification (Table 23, products): its number, digits and dots (`23.30.20.17`), and its title,
/// as the user gives them. No table is bundled, so neither is looked up.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OmniClass {
    pub number: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}

impl OmniClass {
    /// Why it isn't one a file and IFC can carry, if it isn't: a number of digits and dots, at most [`MAX_OMNICLASS`], and a
    /// title of one line, at most [`MAX_TEXT`].
    pub fn check(&self) -> Result<(), String> {
        let n = &self.number;
        let shown: String = n.chars().take(40).collect();
        if n.is_empty() || n.len() > MAX_OMNICLASS || !n.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(format!(
                "`omniclass` `number` `{shown}`: an OmniClass number is digits and dots (`23.30.20.17`), 1 to {MAX_OMNICLASS} of them"
            ));
        }
        if !n.starts_with(|c: char| c.is_ascii_digit()) || !n.ends_with(|c: char| c.is_ascii_digit()) || n.contains("..") {
            return Err(format!("`omniclass` `number` `{shown}`: its digits are in groups between single dots (`23.30.20.17`)"));
        }
        if self.title.chars().count() > MAX_TEXT || self.title.chars().any(char::is_control) {
            return Err(format!("`omniclass` `title`: text of up to {MAX_TEXT} characters on one line"));
        }
        Ok(())
    }
}

/// Revit's default for Always Vertical: on.
pub(super) fn yes() -> bool {
    true
}

/// Whether `b` is Always Vertical's default, which a file leaves out.
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(super) fn is_yes(b: &bool) -> bool {
    *b
}

impl Family {
    /// Whether a section (and the 3D view's section box) cuts its forms, poché where its plane passes through them: what
    /// `cut_in_views` says, else what its category says ([`super::Category::cut`]).
    pub fn cuts_in_views(&self) -> bool {
        self.cut_in_views.unwrap_or_else(|| self.category.cut())
    }

    /// Its options, every one of them, defaults too: what the `family` command replies and takes back as `options`.
    pub fn options_json(&self) -> Value {
        json!({
            "work_plane_based": self.work_plane_based,
            "always_vertical": self.always_vertical,
            "cut_with_voids_when_loaded": self.cut_with_voids_when_loaded,
            "shared": self.shared,
            "room_calculation_point": self.room_calculation_point,
            "cut_in_views": self.cut_in_views,
            "omniclass": self.omniclass,
        })
    }

    /// Its options, each of `given` merged over what it holds: a `null` takes an option back to its default (none, for a
    /// point, an override or a classification). A key that isn't an option, or a value of the wrong kind, is refused.
    pub fn merge_options(&mut self, given: &Value) -> Result<(), String> {
        let Value::Object(o) = given else {
            return Err(format!("`options` is an object of a family's options ({}), not {given}", OPTION_KEYS.join(", ")));
        };
        let flag = |k: &str, v: &Value, default: bool| match v {
            Value::Null => Ok(default),
            Value::Bool(b) => Ok(*b),
            _ => Err(format!("`options` `{k}` is true or false (null for its default), not {v}")),
        };
        for (k, v) in o {
            match k.as_str() {
                "work_plane_based" => self.work_plane_based = flag(k, v, false)?,
                "always_vertical" => self.always_vertical = flag(k, v, true)?,
                "cut_with_voids_when_loaded" => self.cut_with_voids_when_loaded = flag(k, v, false)?,
                "shared" => self.shared = flag(k, v, false)?,
                "cut_in_views" => {
                    self.cut_in_views = match v {
                        Value::Null => None,
                        Value::Bool(b) => Some(*b),
                        _ => return Err(format!("`options` `cut_in_views` is true or false (null for its category's rule), not {v}")),
                    }
                }
                "room_calculation_point" => {
                    self.room_calculation_point = match v {
                        Value::Null => None,
                        v => Some(serde_json::from_value(v.clone()).map_err(|e| {
                            format!("`options` `room_calculation_point` is [x, y, z] in the family's axes, mm, numbers or formulas: {e}")
                        })?),
                    }
                }
                "omniclass" => {
                    self.omniclass = match v {
                        Value::Null => None,
                        v => Some(
                            serde_json::from_value(v.clone()).map_err(|e| format!("`options` `omniclass` is {{number, title?}}: {e}"))?,
                        ),
                    }
                }
                other => {
                    let shown: String = other.chars().take(40).collect();
                    return Err(format!("`options` has no `{shown}`: a family's options are {}", OPTION_KEYS.join(", ")));
                }
            }
        }
        Ok(())
    }

    /// The names of its options that differ from `was`'s, for what a dry run says changed.
    pub fn options_changed(&self, was: Option<&Family>) -> Vec<&'static str> {
        let now = self.options_json();
        let before = was.map_or_else(|| Family::default().options_json(), Family::options_json);
        OPTION_KEYS.into_iter().filter(|k| now.get(*k) != before.get(*k)).collect()
    }

    /// Its options as a file holds them, taken out of `fields` (a family's flat fields) into an object the `family` command
    /// takes as `options`; none, when it holds none.
    pub fn options_from_fields(fields: &mut Map<String, Value>) -> Option<Value> {
        let options: Map<String, Value> = OPTION_KEYS.iter().filter_map(|k| fields.remove(*k).map(|v| ((*k).to_string(), v))).collect();
        (!options.is_empty()).then_some(Value::Object(options))
    }

    /// Whether its options are sound: an OmniClass number and title it can carry, and a room calculation point whose
    /// formulas read only its length parameters (a door's or a window's `Wall Thickness` too) and that works out, for every
    /// type, finite and within reach.
    pub(super) fn check_options(&self) -> Result<(), String> {
        if let Some(o) = &self.omniclass {
            o.check()?;
        }
        let Some(point) = &self.room_calculation_point else { return Ok(()) };
        let who = "`room_calculation_point`";
        for e in point {
            match e {
                Expr::Number(x) if !x.is_finite() || x.abs() > crate::model::MAX_COORD => {
                    return Err(format!("{who}: {x} is not a usable number; give mm within ±{}", crate::model::MAX_COORD));
                }
                Expr::Number(_) => {}
                Expr::Formula(f) if f.chars().count() > MAX_FORMULA => {
                    return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
                }
                Expr::Formula(f) => {
                    for n in names(f).map_err(|e| format!("{who}: {e}"))? {
                        let length = self.parameters.iter().any(|p| p.name == n && p.kind == Kind::Length);
                        if !length && !(self.category.hosted() && n == WALL) {
                            return Err(format!(
                                "{who}: formula `{f}` reads `{n}`, which isn't one of its length parameters: the point reads lengths only"
                            ));
                        }
                    }
                }
            }
        }
        for t in self.types.keys() {
            self.values(t, &BTreeMap::new()).and_then(|v| self.room_point(&v)).map_err(|e| format!("type `{t}`: {who}: {e}"))?;
        }
        Ok(())
    }

    /// Its room calculation point worked out for `values` (a type's, a component's), in its own axes, mm; none when it has
    /// none.
    pub fn room_point(&self, values: &BTreeMap<String, Val>) -> Result<Option<[f64; 3]>, String> {
        let Some([x, y, z]) = &self.room_calculation_point else { return Ok(None) };
        let mut read = HashMap::new();
        Ok(Some([number(x, values, &mut read)?, number(y, values, &mut read)?, number(z, values, &mut read)?]))
    }
}
