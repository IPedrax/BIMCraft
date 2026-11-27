//! The Type Selector: the types a category's elements can be, and the one the selected elements are. A door or a window may be of a
//! family (`Family: Type`) as well as of one of the project's own types; a component is always of a family's. Picking one is a
//! `modify` of the shown elements (`apply::set_type`), so it is checked, rolled back when it breaks a rule and undone like any change.

use std::collections::BTreeSet;

use bimcraft_core::family::Category;
use bimcraft_core::model::{Id, Model};
use serde_json::Value;

use super::names::category as category_name;
use crate::options::TypePick;

/// The most types a list offers.
const MAX_TYPES: usize = 500;

/// One type of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    /// What the list says: the type's name, or `Family: Type`.
    pub label: String,
    /// The type's own name, which `modify` takes as `type`.
    pub name: String,
    /// The family of a type of a family.
    pub family: Option<String>,
    /// The heading it is listed under (a wall's list sets curtain walls apart).
    pub heading: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    /// The card's first line: the category, or the family the elements are of.
    pub heading: String,
    /// The card's second line: the type the shown elements share, as the list names it.
    pub current: Option<String>,
    pub types: Vec<Type>,
    /// A door or a window: one of the project's own types takes it off the family it was of.
    pub hosted: bool,
    /// Whether the list opens: not when the category has no types, nor for a floor or a ceiling that follows a room (its type is the
    /// room type's).
    pub enabled: bool,
}

/// Every type of `category` a list offers: the project's own (a wall's curtain walls under a heading of their own), then each
/// family's, for a door, a window (those families') or a component (every family not hosted).
fn listed(m: &Model, category: &str) -> (Vec<Type>, Option<Category>, bool) {
    let hosted = match category {
        "door" => Some(Category::Door),
        "window" => Some(Category::Window),
        _ => None,
    };
    let component = category == "component";
    let pairs: Vec<(String, String)> = m
        .families
        .iter()
        .filter(|(_, x)| if component { !x.category.hosted() } else { hosted == Some(x.category) })
        .flat_map(|(f, x)| x.types.keys().map(move |ty| (f.clone(), ty.clone())))
        .take(MAX_TYPES)
        .collect();
    let own: Vec<&str> = if component { Vec::new() } else { m.type_names(category).into_iter().take(MAX_TYPES).collect() };
    // A wall's curtain walls are listed under a heading of their own, and only then is there one on the rest.
    let curtain = |n: &str| category == "wall" && m.types.wall.get(n).is_some_and(|w| w.curtain.is_some());
    let curtains = own.iter().any(|n| curtain(n));
    let (curtains_of, plain): (Vec<&str>, Vec<&str>) = own.iter().copied().partition(|n| curtain(n));
    let mut types: Vec<Type> = Vec::new();
    for (names, heading) in [(plain, "Walls"), (curtains_of, "Curtain Walls")] {
        let heading = curtains.then_some(heading);
        types.extend(names.into_iter().map(|n| Type { label: n.to_string(), name: n.to_string(), family: None, heading }));
    }
    types.extend(pairs.into_iter().map(|(f, ty)| Type { label: format!("{f}: {ty}"), name: ty, family: Some(f), heading: None }));
    (types, hosted, component)
}

impl Selector {
    /// The selector for the elements `ids`, which are all of `category` and whose JSON is `all`.
    pub fn of(m: &Model, category: &str, ids: &[Id], all: &[Value]) -> Selector {
        let (types, hosted, component) = listed(m, category);
        // The family and type the elements share, if they are of one.
        let of: BTreeSet<Option<(String, String)>> =
            ids.iter().map(|id| m.elements.get(*id).and_then(|e| e.family()).map(|(f, ty)| (f.to_string(), ty.to_string()))).collect();
        let pair = if of.len() == 1 { of.into_iter().next().flatten() } else { None };
        let named: BTreeSet<&str> = all.iter().filter_map(|j| j["type"].as_str()).collect();
        let current = match (&pair, named.len()) {
            (Some((f, ty)), _) => Some(format!("{f}: {ty}")),
            (None, 1) if !component => named.first().map(|n| n.to_string()),
            _ => None,
        };
        let followed = ids.iter().any(|id| m.finish_of(*id).is_some());
        Selector {
            heading: pair.map_or_else(|| category_name(category, false), |(f, _)| f),
            current,
            enabled: !types.is_empty() && !followed,
            types,
            hosted: hosted.is_some(),
        }
    }

    /// The selector of the tool in hand, for a category: the type it places is the one `pick`ed, else the project's usual one (a door's or
    /// a window's of a family first), and none for a component until one is picked.
    pub fn for_tool(m: &Model, category: &str, pick: Option<&TypePick>) -> Selector {
        let (types, hosted, component) = listed(m, category);
        let usual: Option<(Option<String>, String)> = match pick {
            Some(p) => Some((p.family.clone(), p.name.clone())),
            None if component => None,
            None if hosted.is_some() => match m.default_family(category) {
                Some((f, ty)) => Some((Some(f.to_string()), ty.to_string())),
                None => m.default_type(category).map(|n| (None, n.to_string())),
            },
            None => m.default_type(category).map(|n| (None, n.to_string())),
        };
        let heading = match &usual {
            Some((Some(f), _)) => f.clone(),
            _ => category_name(category, false),
        };
        let current = usual.map(|(f, n)| match f {
            Some(f) => format!("{f}: {n}"),
            None => n,
        });
        Selector { heading, current, enabled: !types.is_empty(), types, hosted: hosted.is_some() }
    }

    /// Whether the selected elements are of a type Edit Type can open.
    pub fn editable(&self) -> bool {
        self.current.is_some()
    }
}
