//! Locked and equal dimensions kept, as Revit keeps them: after each command, what a constrained dimension measures to
//! moves along the way it measures so that its locked segments keep their lengths and an equal one's segments stay
//! equal. Each dimension is solved on its own line: its stations (where its references cross it) are positions; a locked
//! segment ties two together, and equality ties every segment to one length. What the command moved stays where it put
//! it, and the rest follows; an equal string with nothing locked keeps its two ends, its inner references spread evenly.
//! An element is moved as the `move` command moves it (joined walls stretch, doors slide along their walls), and a
//! dimension solved may move what another measures, so they are solved again, a few times, until all hold. Where they
//! can't, the command is refused and the project is as it was.

use std::collections::BTreeMap;

use crate::engine::Session;
use crate::ids::Guid;
use crate::model::{DimKind, Dimension, Element, Id, Model};

/// How far a station may be from where its constraint puts it, mm.
const SLACK: f64 = 0.01;
/// How many times the dimensions are solved, one after another, before what still doesn't hold is refused.
const PASSES: usize = 8;

/// Whether dimension `d` holds constraints.
fn constrained(d: &Dimension) -> bool {
    matches!(d.kind, DimKind::Aligned | DimKind::Linear) && (d.equal || !d.locked.is_empty())
}

/// Where each station of `d` should be, given those of elements in `fixed` stay: none where it holds already. An error
/// when its constraints contradict each other, or what stays.
fn targets(m: &Model, d: &Dimension, fixed: &dyn Fn(Guid) -> bool) -> Result<Vec<(Guid, f64)>, String> {
    let (_, st) = m.stations(d);
    let k = st.len().saturating_sub(1);
    if k == 0 {
        return Ok(Vec::new());
    }
    let s: Vec<f64> = st.iter().map(|x| x.0).collect();
    // Each segment's length, where one is set: a lock's, or with equality, all of them one.
    let mut gap: Vec<Option<f64>> = vec![None; k];
    for l in d.locked.iter().filter(|l| l.segment < k) {
        if let Some(g) = gap.get_mut(l.segment) {
            *g = Some(l.length);
        }
    }
    let mut want = s.clone();
    if d.equal {
        let set: Vec<f64> = gap.iter().flatten().copied().collect();
        match set.first() {
            Some(&g) if set.iter().all(|x| (x - g).abs() <= SLACK) => gap = vec![Some(g); k],
            Some(_) => return Err("its locked segments differ in length, so they can't be equal".into()),
            // Nothing locked: its two ends stay as they are, and what lies between goes back to its even place.
            None => {
                let g = (s[k] - s[0]) / k as f64;
                for (t, w) in want.iter_mut().enumerate() {
                    *w = s[0] + g * t as f64;
                }
                return Ok(st
                    .iter()
                    .zip(want)
                    .filter(|((at, _, _), w)| (w - at).abs() > SLACK)
                    .map(|((at, _, r), w)| (r.id, w - at))
                    .collect());
            }
        }
    }
    // Stations tied by set lengths make rigid runs; each run stays where a station of it that is fixed is, else where
    // its first is.
    let mut i = 0;
    while i <= k {
        let mut j = i;
        while j < k && gap.get(j).copied().flatten().is_some() {
            j += 1;
        }
        // Stations i..=j, each from the run's first by the set lengths.
        let mut offset = vec![0.0; j - i + 1];
        for t in i..j {
            offset[t - i + 1] = offset[t - i] + gap.get(t).copied().flatten().unwrap_or(0.0);
        }
        let anchors: Vec<f64> = (i..=j).filter(|&t| fixed(st[t].2.id)).map(|t| s[t] - offset[t - i]).collect();
        let start = match anchors.first() {
            Some(&a) if anchors.iter().all(|x| (x - a).abs() <= SLACK) => a,
            Some(_) => return Err("what the command moved pulls its locked segments apart".into()),
            None => s[i],
        };
        for t in i..=j {
            want[t] = start + offset[t - i];
        }
        i = j + 1;
    }
    Ok(st.iter().zip(want).filter(|((at, _, _), w)| (w - at).abs() > SLACK).map(|((at, _, r), w)| (r.id, w - at)).collect())
}

/// The elements whose stations move how far, each once: an error when two of one element's would move it apart.
fn moves(m: &Model, d: &Dimension, fixed: &dyn Fn(Guid) -> bool) -> Result<BTreeMap<Guid, f64>, String> {
    let mut by: BTreeMap<Guid, f64> = BTreeMap::new();
    for (id, delta) in targets(m, d, fixed)? {
        match by.get(&id) {
            Some(was) if (was - delta).abs() > SLACK => {
                return Err(format!(
                    "{} {} would have to move two ways",
                    m.elements.get(id).map_or("element", |e| e.category()),
                    m.shown(id)
                ));
            }
            _ => {
                by.insert(id, delta);
            }
        }
    }
    Ok(by)
}

/// The constrained dimensions kept after a command changed `before` into the session's model: what they measure to moved
/// as they say. An error, saying which and why, when they can't be.
pub(crate) fn keep(s: &mut Session, before: &Model) -> Result<(), String> {
    let lookup = s.model.lookup();
    let dims: Vec<Id> =
        lookup.dimensions().filter(|id| matches!(s.model.elements.get(*id), Some(Element::Dimension(d)) if constrained(d))).collect();
    if dims.is_empty() {
        return Ok(());
    }
    // What the command changed stays; so does what keeping the constraints has moved already.
    let mut stays: Vec<Guid> = match s.model.changed_since(before) {
        crate::ids::Changed::Ids(ids) => ids
            .into_iter()
            .filter(|id| s.model.elements.get(*id).is_some_and(|e| before.elements.get(*id) != Some(e)))
            .filter_map(|id| s.model.elements.guid(id))
            .collect(),
        crate::ids::Changed::All => {
            s.model.elements.with_guids().filter(|(id, _, e)| before.elements.get(*id) != Some(*e)).map(|(_, g, _)| g).collect()
        }
    };
    for _ in 0..PASSES {
        let mut moved = false;
        for &id in &dims {
            let Some(Element::Dimension(d)) = s.model.elements.get(id).cloned() else { continue };
            let held = stays.clone();
            let fixed = |g: Guid| held.contains(&g);
            let by = moves(&s.model, &d, &fixed).map_err(|e| format!("dimension {id}'s constraints can't be kept: {e}"))?;
            let (n, _) = s.model.stations(&d);
            for (g, delta) in by {
                let Some(e) = s.model.elements.id(g) else { continue };
                let v = n * delta;
                crate::commands::move_by(s, e, v).map_err(|err| format!("dimension {id}'s constraints can't be kept: {err}"))?;
                stays.push(g);
                moved = true;
            }
        }
        if !moved {
            return Ok(());
        }
    }
    // Still moving after every pass: they pull against each other.
    let unkept = dims.iter().find(|id| {
        matches!(s.model.elements.get(**id), Some(Element::Dimension(d)) if targets(&s.model, d, &|_| false).map_or(true, |t| !t.is_empty()))
    });
    match unkept {
        Some(id) => Err(format!("dimension {id}'s constraints can't be kept with the others: unlock one, or take its equality off")),
        None => Ok(()),
    }
}
