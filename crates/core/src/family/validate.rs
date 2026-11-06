//! What a family's own shape says about itself, as the `family` command reports it: what blocks a
//! change, and what is worth knowing without blocking it. Everything found is reported, not only
//! the first thing, and the geometry is worked out around what doesn't build rather than stopped by
//! it — a family with three faults is told about all three at once.

use std::collections::{BTreeMap, BTreeSet};

use super::{DEFAULT_WALL, Expr, Family, HEIGHT, MAX_FORMULA, Val, WALL, WIDTH, names};

/// `f`'s errors, which a change may not go ahead with, and its warnings, which don't stop it: its
/// formulas read over its own parameters, [`Family::check`]'s own findings (a family that builds,
/// its materials in the library, its types setting parameters it has), and what its forms' boxes
/// say about a door's or a window's opening and its wall.
pub(crate) fn problems(f: &Family, has_material: &dyn Fn(&str) -> bool) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    formulas(f, &mut errors);
    if let Err(e) = f.check(has_material) {
        errors.push(e);
    }
    let mut warnings = Vec::new();
    geometry(f, &mut warnings);
    (errors, warnings)
}

/// Every formula of the family read: the ones that don't read, saying why and where they stop, and
/// the ones that say a name it has no parameter of. `Wall Thickness`, the wall's own, counts as one
/// of a door's or a window's; no other family's formulas may say it.
fn formulas(f: &Family, out: &mut Vec<String>) {
    let given: BTreeSet<&str> = f.parameters.iter().map(|p| p.name.as_str()).collect();
    let read = |who: &str, formula: &str, out: &mut Vec<String>| {
        // A formula longer than what it may be is `Family::check`'s to count, not read again here.
        if formula.chars().count() > MAX_FORMULA {
            return;
        }
        match names(formula) {
            Err(e) => out.push(format!("{who}: {e}")),
            Ok(found) => out.extend(
                found
                    .into_iter()
                    .filter(|n| !given.contains(n.as_str()) && !(f.category.hosted() && n == WALL))
                    .map(|n| format!("{who}: formula `{formula}` reads `{n}`, which isn't one of its parameters")),
            ),
        }
    };
    for p in &f.parameters {
        if let Some(formula) = &p.formula {
            read(&format!("parameter `{}`", p.name), formula, out);
        }
    }
    for (i, form) in f.forms.iter().enumerate() {
        let circle = form.circle.iter().flat_map(|c| c.centre.iter().chain([&c.radius]));
        let exprs = form
            .profile
            .iter()
            .flatten()
            .chain([&form.from, &form.to])
            .chain(&form.visible)
            .chain(circle)
            .chain(form.axis.iter().flatten().flatten());
        for e in exprs {
            if let Expr::Formula(formula) = e {
                read(&format!("form {}", i + 1), formula, out);
            }
        }
    }
    for (i, sym) in f.symbols.iter().enumerate() {
        let exprs = sym
            .line
            .iter()
            .flatten()
            .chain(&sym.visible)
            .chain(sym.arc.iter().flat_map(|c| c.centre.iter().chain([&c.radius, &c.from, &c.to])));
        for e in exprs {
            if let Expr::Formula(formula) = e {
                read(&format!("symbol {}", i + 1), formula, out);
            }
        }
    }
}

/// What the forms' boxes say, over every type: a void that reaches no solid before it, a door's or
/// a window's form wholly outside the opening it stands in, one thicker than the wall it's in, and
/// a plan with nothing drawn on it. A fault every type has is one warning naming all of them.
fn geometry(f: &Family, out: &mut Vec<String>) {
    if f.category.hosted() && f.symbols.is_empty() {
        out.push("its plan draws no symbols: a door's or a window's plan shows its swing or its sill, and this one shows nothing".into());
    }
    let number = |values: &BTreeMap<String, Val>, name: &str| match values.get(name) {
        Some(Val::Number(x)) => Some(*x),
        _ => None,
    };
    let mut faults: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for ty in f.types.keys() {
        // A type whose formulas don't work out has `Family::check`'s error to say so; its geometry is beyond telling.
        let Ok(values) = f.values_in(ty, &BTreeMap::new(), DEFAULT_WALL) else {
            continue;
        };
        let shapes = f.shapes(&values);
        for void in shapes.iter().filter(|s| s.void) {
            let cut = shapes.iter().any(|solid| !solid.void && solid.index < void.index && void.over(solid));
            if !cut {
                faults
                    .entry(format!("form {}: its void passes through no solid form listed before it; it cuts nothing", void.index + 1))
                    .or_default()
                    .insert(ty.clone());
            }
        }
        if !f.category.hosted() {
            continue;
        }
        let (width, height) = (number(&values, WIDTH), number(&values, HEIGHT));
        let wall = number(&values, WALL).unwrap_or(DEFAULT_WALL);
        for solid in shapes.iter().filter(|s| !s.void) {
            if let (Some(w), Some(h)) = (width, height)
                && w > 0.0
                && h > 0.0
                && solid.outside(w / 2.0, 0.0, h)
            {
                faults
                    .entry(format!(
                        "form {}: it lies wholly outside the opening (x {}…{} mm, z {}…{} mm)",
                        solid.index + 1,
                        whole(solid.min[0]),
                        whole(solid.max[0]),
                        whole(solid.min[2]),
                        whole(solid.max[2])
                    ))
                    .or_default()
                    .insert(ty.clone());
            }
            let across = solid.max[1] - solid.min[1];
            if across > wall + 1e-6 {
                faults
                    .entry(format!(
                        "form {}: it is {} mm thick across the wall where the wall is {} mm",
                        solid.index + 1,
                        whole(across),
                        whole(wall)
                    ))
                    .or_default()
                    .insert(ty.clone());
            }
        }
    }
    for (what, types) in faults {
        let types: Vec<String> = types.iter().map(|t| format!("`{t}`")).collect();
        out.push(format!("{what} (in type {})", types.join(", ")));
    }
}

/// A length in mm, said as whole tenths of a millimetre: a warning tells a size, not a sum of
/// floating point noise, and never `-0`.
fn whole(mm: f64) -> String {
    let mm = (mm * 10.0).round() / 10.0 + 0.0;
    format!("{mm}")
}
