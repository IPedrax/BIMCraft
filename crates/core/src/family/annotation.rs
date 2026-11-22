//! The drawings of a two-dimensional annotation family: its symbols laid out in its own millimetres, with line styles,
//! fills, text types and the labels that read its parameters. A sheet or a flat view draws them (`crate::render`), and the
//! family editor shows them as it edits them. A component's symbols never come here: they stay plan marks.

use std::collections::{BTreeMap, HashMap};

use super::{Align, Expr, Family, MAX_FORMULA, MAX_PROFILE, P2, VAlign, Val, lettering, not_a_parameter, number, said_text_or_placeholder};
use crate::model::MAX_TEXT;

/// The most rings one fill may have, and the most points one of them.
pub const MAX_RINGS: usize = 256;

/// A mark an annotation family draws, in its own millimetres and turned by its instance: a polyline or an arc, a solid
/// dot, a label or a fill.
#[derive(Clone, Debug, PartialEq)]
pub enum PaperMark {
    Line { pts: Vec<P2>, style: Option<String> },
    Dot { centre: P2, radius: f64 },
    Label { at: P2, height: f64, text: String, align: Align, text_type: Option<String>, valign: VAlign, wrap: Option<f64> },
    Fill { rings: Vec<Vec<P2>>, type_name: Option<String> },
}

/// The family's symbols worked out for `values`: its fills, lines and arcs, dots and labels, each in the order it is
/// listed. `visible` leaves one out; a detail level is the sheet's or a view's, not the family's.
pub fn marks(family: &Family, values: &BTreeMap<String, Val>) -> Result<Vec<PaperMark>, String> {
    Ok(marks_indexed(family, values)?.into_iter().map(|(_, mark)| mark).collect())
}

/// [`marks`], each with the place its symbol has in the family's list: what an editor picks and drags.
pub fn marks_indexed<'a>(family: &'a Family, values: &BTreeMap<String, Val>) -> Result<Vec<(usize, PaperMark)>, String> {
    let mut read: HashMap<&'a str, Result<super::Node, String>> = HashMap::new();
    let mut out = Vec::new();
    for (i, sym) in family.symbols.iter().enumerate() {
        let said = |e: String| format!("symbol {}: {e}", i + 1);
        let ways = usize::from(!sym.line.is_empty())
            + usize::from(sym.arc.is_some())
            + usize::from(sym.text.is_some())
            + usize::from(sym.dot.is_some())
            + usize::from(sym.fill.is_some());
        if ways != 1 || sym.line.len() > MAX_PROFILE {
            return Err(said(format!(
                "it takes a `line` of up to {MAX_PROFILE} points, an `arc`, a `text`, a `dot` or a `fill`, one of them"
            )));
        }
        if let Some(v) = &sym.visible
            && number(v, values, &mut read).map_err(said)? == 0.0
        {
            continue;
        }
        let mut n = |e: &'a Expr| number(e, values, &mut read).map_err(said);
        if let Some(f) = &sym.fill {
            let mut rings = Vec::with_capacity(f.rings.len());
            for ring in &f.rings {
                let mut pts = Vec::with_capacity(ring.len());
                for [x, y] in ring {
                    pts.push(P2(n(x)?, n(y)?));
                }
                if pts.len() < 3 {
                    return Err(said("a fill ring needs at least three points".into()));
                }
                rings.push(pts);
            }
            if rings.is_empty() {
                return Err(said("a fill needs a ring".into()));
            }
            out.push((i, PaperMark::Fill { rings, type_name: f.type_name.clone() }));
            continue;
        }
        if let Some(c) = &sym.dot {
            let (centre, radius) = (P2(n(&c.centre[0])?, n(&c.centre[1])?), n(&c.radius)?);
            if radius <= 0.0 {
                return Err(said(format!("its dot has radius {radius}; it needs a radius above zero")));
            }
            out.push((i, PaperMark::Dot { centre, radius }));
            continue;
        }
        if let Some(l) = &sym.text {
            let (at, height) = (P2(n(&l.at[0])?, n(&l.at[1])?), n(&l.height)?);
            if height <= 0.0 {
                return Err(said(format!("its text has height {height}; it needs a height above zero")));
            }
            let text = said_text_or_placeholder(&l.value, values, &family.parameters).map_err(said)?;
            let wrap = l.wrap.as_ref().map(&mut n).transpose()?;
            if wrap.is_some_and(|w| w <= 0.0) {
                return Err(said("its text wrap needs a width above zero".into()));
            }
            out.push((i, PaperMark::Label { at, height, text, align: l.align, text_type: l.text_type.clone(), valign: l.valign, wrap }));
            continue;
        }
        let mut pts = Vec::new();
        for [a, b] in &sym.line {
            pts.push(P2(n(a)?, n(b)?));
        }
        if let Some(c) = &sym.arc {
            let (centre, radius, from, to) = (P2(n(&c.centre[0])?, n(&c.centre[1])?), n(&c.radius)?, n(&c.from)?, n(&c.to)?);
            if radius <= 0.0 || (to - from).abs() > 360.0 || to == from {
                return Err(said(format!(
                    "its arc has radius {radius} and runs from {from} to {to} degrees; it needs a radius above zero and up to a full turn"
                )));
            }
            let steps = ((to - from).abs() / super::ARC_STEP).ceil().max(1.0) as usize;
            pts.extend((0..=steps).map(|k| {
                let (s, c) = (from + (to - from) * k as f64 / steps as f64).to_radians().sin_cos();
                P2(centre.0 + radius * c, centre.1 + radius * s)
            }));
        }
        out.push((i, PaperMark::Line { pts, style: sym.style.clone() }));
    }
    Ok(out)
}

/// The box round what the family draws for `values`, in its own millimetres: `[lo, hi]`, none when it draws nothing.
pub fn bounds(family: &Family, values: &BTreeMap<String, Val>) -> Result<Option<[P2; 2]>, String> {
    let mut lo = P2(f64::INFINITY, f64::INFINITY);
    let mut hi = P2(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut any = false;
    let mut at = |p: P2| {
        any = true;
        lo = P2(lo.0.min(p.0), lo.1.min(p.1));
        hi = P2(hi.0.max(p.0), hi.1.max(p.1));
    };
    for mark in marks(family, values)? {
        match mark {
            PaperMark::Line { pts, .. } => pts.into_iter().for_each(&mut at),
            PaperMark::Dot { centre, radius } => {
                at(P2(centre.0 - radius, centre.1 - radius));
                at(P2(centre.0 + radius, centre.1 + radius));
            }
            PaperMark::Label { at: p, height, text, wrap, .. } => {
                let wide = wrap
                    .unwrap_or_else(|| lettering(&text, P2(0.0, 0.0), height, Align::Left).iter().flatten().fold(0.0, |w, q| w.max(q.0)));
                at(p);
                at(P2(p.0 + wide, p.1 + height));
            }
            PaperMark::Fill { rings, .. } => rings.into_iter().flatten().for_each(&mut at),
        }
    }
    Ok(any.then_some([lo, hi]))
}

/// Why its annotation content isn't sound, if it isn't: a fill has rings of three points or more, its formulas are within
/// reach, and its line styles and text types are named shortly. A name of a style or a type is the project's, so only its
/// shape is checked here; a view that lacks it draws the default and says so.
pub fn check(family: &Family) -> Result<(), String> {
    let long = |f: &str| f.chars().count() > MAX_FORMULA;
    for (i, sym) in family.symbols.iter().enumerate() {
        let said = |e: String| format!("symbol {}: {e}", i + 1);
        let ways = usize::from(!sym.line.is_empty())
            + usize::from(sym.arc.is_some())
            + usize::from(sym.text.is_some())
            + usize::from(sym.dot.is_some())
            + usize::from(sym.fill.is_some());
        if ways != 1 {
            return Err(said("it takes a `line` of up to 64 points, an `arc`, a `text`, a `dot` or a `fill`, one of them".into()));
        }
        if let Some(f) = &sym.fill {
            if f.rings.is_empty() || f.rings.len() > MAX_RINGS {
                return Err(said(format!("a fill takes 1 to {MAX_RINGS} rings")));
            }
            for ring in &f.rings {
                if ring.len() < 3 || ring.len() > MAX_PROFILE {
                    return Err(said(format!("a fill ring takes 3 to {MAX_PROFILE} points")));
                }
                if ring.iter().flatten().any(|e| matches!(e, Expr::Formula(f) if long(f))) {
                    return Err(said(format!("a formula of it is longer than {MAX_FORMULA} characters")));
                }
            }
            if let Some(t) = &f.type_name
                && (t.trim().is_empty() || t.chars().count() > MAX_TEXT || t.chars().any(char::is_control))
            {
                return Err(said("its fill type is a name of up to 1024 characters on one line".into()));
            }
        }
        if let Some(s) = &sym.style
            && (s.trim().is_empty() || s.chars().count() > MAX_TEXT || s.chars().any(char::is_control))
        {
            return Err(said("its line style is a name of up to 1024 characters on one line".into()));
        }
        if let Some(l) = &sym.text {
            if let Some(t) = &l.text_type
                && (t.trim().is_empty() || t.chars().count() > MAX_TEXT || t.chars().any(char::is_control))
            {
                return Err(said("its text type is a name of up to 1024 characters on one line".into()));
            }
            for name in super::fields(&l.value).map_err(said)? {
                if !family.parameters.iter().any(|p| p.name == name) {
                    return Err(said(format!("its text `{{{name}}}`: {}", not_a_parameter(name, &family.parameters))));
                }
            }
            if l.wrap.as_ref().is_some_and(|w| matches!(w, Expr::Formula(f) if long(f))) {
                return Err(said(format!("its wrap formula is longer than {MAX_FORMULA} characters")));
            }
        }
    }
    Ok(())
}
