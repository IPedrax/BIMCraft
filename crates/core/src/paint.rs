//! Paint: a material laid over one face of an element, over what its type is made of: a wall's
//! exterior, a floor's top. It lives in the element's `params`, under [`KEY`], as `{face: material}`,
//! and shows in the 3D view, in the material's colour and texture.

use serde_json::Value;

use crate::model::{Element, Model};

/// The key an element holds its paint under.
pub const KEY: &str = "Paint";

/// Whether `name` is the one an element holds its paint under: no project or family parameter may take it.
pub fn reserved(name: &str) -> bool {
    name == KEY
}

/// The faces an element of `category` has to paint, by name: a wall's two sides; a floor's, a roof's or a ceiling's top and
/// bottom; and of the other things a model is made of that hold values, all of it.
pub fn faces(category: &str) -> &'static [&'static str] {
    match category {
        "wall" => &["exterior", "interior"],
        "floor" | "roof" | "ceiling" => &["top", "bottom"],
        "column" | "structuralcolumn" | "footing" | "beam" | "stripfooting" | "pipe" | "cablecarrier" | "wire" | "downconductor"
        | "groundconductor" | "cable" | "component" | "door" | "window" | "stair" | "ramp" | "railing" => &["all"],
        _ => &[],
    }
}

/// What an element's paint says, if `key` is where it's held: faces it has, each a material of the library.
pub(crate) fn check_value(m: &Model, who: &str, key: &str, v: &Value, e: &Element) -> Option<Result<(), String>> {
    (key == KEY).then(|| {
        let Some(painted) = v.as_object() else { return Err(format!("{who}: `{KEY}` is {{face: material}}, not {v}")) };
        let faces = faces(e.category());
        for (face, material) in painted {
            if !faces.contains(&face.as_str()) {
                let has = if faces.is_empty() { "none".to_string() } else { faces.join(", ") };
                return Err(format!("{who}: a {} has no face `{face}` to paint; its faces are {has}", e.category()));
            }
            if !material.as_str().is_some_and(|n| m.materials.contains_key(n)) {
                return Err(format!("{who}: its `{face}` is painted a material of the library, not {material}; add one with `material`"));
            }
        }
        Ok(())
    })
}
