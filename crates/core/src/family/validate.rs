//! What a family's own shape says about itself, as the `family` command reports it: what blocks a
//! change, and what is worth knowing without blocking it. Everything found is reported, not only
//! the first thing, and the geometry is worked out around what doesn't build rather than stopped by
//! it — a family with three faults is told about all three at once.

use std::collections::{BTreeMap, BTreeSet};

use super::{DEFAULT_WALL, Expr, Family, HEIGHT, Kind, Load, MAX_FORMULA, Val, WALL, WIDTH, names, not_a_parameter};
use crate::model::Model;

/// `f`'s errors, which a change may not go ahead with, and its warnings, which don't stop it: its
/// formulas read over its own parameters, [`Family::check`]'s own findings (a family that builds,
/// its materials in the library, its types setting parameters it has), and what its forms' boxes
/// say about a door's or a window's opening and its wall.
pub(crate) fn problems(f: &Family, has_material: &dyn Fn(&str) -> bool) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    formulas(f, &mut errors);
    // A room calculation point's name is the check's to refuse too, in the same words: said once.
    if let Err(e) = f.check(has_material)
        && !errors.contains(&e)
    {
        errors.push(e);
    }
    let mut warnings = Vec::new();
    geometry(f, &mut warnings);
    // A reporting parameter no dimension is labelled with holds what it was given.
    warnings.extend(
        f.parameters
            .iter()
            .filter(|p| p.reporting && !f.dimensions.iter().any(|d| d.label.as_deref() == Some(p.name.as_str())))
            .map(|p| format!("parameter `{}` reports no dimension: it holds its value", p.name)),
    );
    (errors, warnings)
}

/// Every formula the family holds, each with what holds it (`parameter `Width``, `form 2`, …): its parameters', its forms'
/// sizes and `visible`, its symbols' sizes and `visible`, its room calculation point's, its connectors' and its adaptive
/// definition's. A part that holds a formula is listed here as [`Family::rename_parameter`] walks it, so no formula goes
/// unread.
/// Every formula of the family read: the ones that don't read, saying why and where they stop, the ones that say a name it
/// has no parameter of, and the ones that read words. `Wall Thickness`, the wall's own, counts as one of a door's or a
/// window's; no other family's formulas may say it.
fn formulas(f: &Family, out: &mut Vec<String>) {
    for (who, formula) in every_formula(f) {
        // A formula longer than what it may be is `Family::check`'s to count, not read again here.
        if formula.chars().count() > MAX_FORMULA {
            continue;
        }
        match names(formula) {
            Err(e) => out.push(format!("{who}: {e}")),
            Ok(found) => {
                for n in found {
                    if !known(f, &n) {
                        out.push(format!("{who}: formula `{formula}`: {}", not_a_parameter(&n, &f.parameters)));
                    } else if let Some(p) = f.parameters.iter().find(|p| p.name == n && p.kind.text() && p.kind != Kind::Material) {
                        // Words a formula can't read; a material's name is refused where it is worked out, as it always was.
                        out.push(format!("{who}: formula `{formula}` reads `{n}`, which is {}: a formula reads numbers", p.kind.said()));
                    }
                }
            }
        }
    }
}

/// Whether a formula of `f` may say `name`: one of its parameters, or a door's or a window's `Wall Thickness`.
fn known(f: &Family, name: &str) -> bool {
    f.parameters.iter().any(|p| p.name == name) || (f.category.hosted() && name == WALL)
}

/// Each name a formula of `f` says that isn't one of its parameters: the formula, the name, and what holds it.
pub(crate) fn unknown_names(f: &Family) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (who, formula) in every_formula(f) {
        if formula.chars().count() > MAX_FORMULA {
            continue;
        }
        for n in names(formula).unwrap_or_default() {
            if !known(f, &n) {
                out.push((who.clone(), formula.to_string(), n));
            }
        }
    }
    out
}

/// What is wrong with `f`'s formulas' names, a line each, said as [`Family::problems`] says them.
pub(crate) fn unknown_said(f: &Family) -> Vec<String> {
    unknown_names(f)
        .into_iter()
        .map(|(who, formula, n)| format!("{who}: formula `{formula}`: {}", not_a_parameter(&n, &f.parameters)))
        .collect()
}

/// What a command must leave true of the families it made or changed, which [`Family::check`] doesn't ask of every family
/// because a file from before it may break it and still open: no formula says a name that isn't one of its family's
/// parameters. A family changed that held one before passes while the change adds no new one, so a command that only
/// touches it in passing (a material renamed) isn't refused for what it found; the `family` command, which edits it, asks
/// all of them ([`Family::problems`]). `before` is the project the command began with.
pub(crate) fn check_family_edits(now: &Model, before: &Model) -> Result<(), String> {
    if now.families.same_allocation(&before.families) {
        return Ok(());
    }
    for (name, f) in now.families.iter() {
        let was = before.families.get(name);
        if was == Some(f) {
            continue;
        }
        let old: BTreeSet<(String, String)> = was.map(unknown_names).unwrap_or_default().into_iter().map(|(_, x, n)| (x, n)).collect();
        let new: Vec<String> = unknown_names(f)
            .into_iter()
            .filter(|(_, x, n)| !old.contains(&(x.clone(), n.clone())))
            .map(|(who, formula, n)| format!("family `{name}`: {who}: formula `{formula}`: {}", not_a_parameter(&n, &f.parameters)))
            .collect();
        if !new.is_empty() {
            return Err(new.join("\n"));
        }
    }
    Ok(())
}

pub(crate) fn every_formula(f: &Family) -> Vec<(String, &str)> {
    fn add<'a>(out: &mut Vec<(String, &'a str)>, who: &dyn Fn() -> String, e: &'a Expr) {
        if let Expr::Formula(formula) = e {
            out.push((who(), formula.as_str()));
        }
    }
    let mut out: Vec<(String, &str)> = Vec::new();
    for p in &f.parameters {
        if let Some(formula) = &p.formula {
            out.push((format!("parameter `{}`", p.name), formula.as_str()));
        }
    }
    for (i, form) in f.forms.iter().enumerate() {
        let exprs = form.sketch_exprs().chain([&form.from, &form.to]).chain(&form.visible);
        for e in exprs {
            add(&mut out, &|| format!("form {}", i + 1), e);
        }
    }
    for (i, sym) in f.symbols.iter().enumerate() {
        let label = sym.text.iter().flat_map(|l| l.at.iter().chain([&l.height]));
        let dot = sym.dot.iter().flat_map(|c| c.centre.iter().chain([&c.radius]));
        let exprs = sym
            .line
            .iter()
            .flatten()
            .chain(&sym.visible)
            .chain(sym.arc.iter().flat_map(|c| c.centre.iter().chain([&c.radius, &c.from, &c.to])))
            .chain(label)
            .chain(dot);
        for e in exprs {
            add(&mut out, &|| format!("symbol {}", i + 1), e);
        }
    }
    for e in f.room_calculation_point.iter().flatten() {
        add(&mut out, &|| "`room_calculation_point`".to_string(), e);
    }
    for (i, c) in f.connectors.iter().enumerate() {
        let loads: &[Expr] = match &c.load_va {
            Load::One(e) => std::slice::from_ref(e),
            Load::PerPhase(v) => v,
        };
        for e in [&c.voltage, &c.power_factor].into_iter().chain(loads) {
            add(&mut out, &|| format!("connector {}", i + 1), e);
        }
    }
    if let Some(d) = &f.adaptive {
        for (id, e) in d.exprs() {
            add(&mut out, &|| format!("adaptive element `{id}`"), e);
        }
    }
    // A nested instance's place and the parameters it sets but its words, which name a host parameter or are words.
    for (i, inst) in f.nested_instances.iter().enumerate() {
        for e in inst.place_exprs() {
            add(&mut out, &|| format!("nested instance {}", i + 1), e);
        }
        for (k, e) in f.instance_exprs(inst) {
            add(&mut out, &|| format!("nested instance {}'s `{k}`", i + 1), e);
        }
    }
    out
}

/// What the forms' boxes say, over every type: a void that reaches no solid before it (but a door's or a window's, which
/// cuts its wall), a door's or
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
        let wall = number(&values, WALL).unwrap_or(DEFAULT_WALL);
        for void in shapes.iter().filter(|s| s.void) {
            let cuts = f.forms.get(void.index).map(super::Form::cuts).unwrap_or_default();
            let cut = cuts.forms() && shapes.iter().any(|solid| !solid.void && solid.index < void.index && void.over(solid));
            // A door's or a window's cuts its wall ([`Family::host_voids`]), where it reaches it, unless it cuts the forms alone.
            let host = f.category.hosted() && cuts.host();
            // A work plane-based family's cuts the face it is placed on ([`Family::face_voids`]), where it reaches it.
            if !cut && !(f.work_plane_based && cuts.host()) && (!host || void.max[1] <= -wall / 2.0 || void.min[1] >= wall / 2.0) {
                let what = if host { "no solid form listed before it, nor its wall" } else { "no solid form listed before it" };
                faults
                    .entry(format!("form {}: its void passes through {what}; it cuts nothing", void.index + 1))
                    .or_default()
                    .insert(ty.clone());
            }
        }
        if !f.category.hosted() {
            continue;
        }
        let (width, height) = (number(&values, WIDTH), number(&values, HEIGHT));
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
