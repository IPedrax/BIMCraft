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
use crate::ids::{Guid, Key};
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

impl Model {
    /// The locked or equal dimensions that hold element `k`, by id: a dimension kept equal that measures to it, or one with
    /// a locked segment that ends at it.
    pub fn held_by(&self, k: impl Key) -> Vec<Id> {
        let Some(g) = self.elements.guid(k) else { return Vec::new() };
        let holds = |d: &Dimension| {
            let at = |i: usize| d.equal || d.locked.iter().any(|l| l.segment == i || l.segment + 1 == i);
            self.stations(d).1.iter().enumerate().any(|(i, (_, _, r))| r.id == g && at(i))
        };
        // A dimension that measures to it points at it: those are its referrers, found in the lookup, in the order of their ids.
        let lookup = self.lookup();
        let held: Vec<Id> = lookup
            .referrers(&g)
            .filter(|id| matches!(self.elements.get(*id), Some(Element::Dimension(d)) if constrained(d) && holds(d)))
            .collect();
        #[cfg(test)]
        assert_eq!(
            held,
            self.elements
                .iter()
                .filter(|(_, e)| matches!(e, Element::Dimension(d) if constrained(d) && holds(d)))
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            "the dimensions holding an element, from the lookup, aren't every one that does"
        );
        held
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::model::Id;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
    }

    /// Two walls running north, apart and not joined: the west one's centre at x = 0, the east one's at `x`; and a dimension
    /// between their centre lines, south of them.
    fn two_walls(s: &mut Session, x: f64) -> (Id, Id, Id) {
        let one = |s: &mut Session, p: Value| run(s, "wall", p)["ids"][0].as_u64().unwrap_or_default();
        let a = one(s, json!({"points": [[0, 0], [0, 4000]]}));
        let b = one(s, json!({"points": [[x, 0], [x, 4000]]}));
        let d = run(s, "dimension", json!({"refs": [{"id": a}, {"id": b}], "at": [x / 2.0, -1000]}))["id"].as_u64().unwrap_or_default();
        (a, b, d)
    }

    fn start_x(s: &Session, id: Id) -> f64 {
        s.model.wall(id).map_or(f64::NAN, |w| w.start.0)
    }

    #[test]
    fn a_length_typed_into_a_dimension_moves_its_second_reference_and_a_locked_segment_is_locked_at_it() {
        let mut s = Session::new();
        let (a, b, d) = two_walls(&mut s, 2000.0);
        let out = run(&mut s, "dimension_value", json!({"id": d, "value": 3000}));
        assert_eq!((out["moved"].as_u64(), start_x(&s, a), start_x(&s, b)), (Some(b), 0.0, 3000.0), "the second wall moved");
        assert_eq!(
            s.model.dimension_values(match s.model.elements.get(d) {
                Some(crate::model::Element::Dimension(d)) => d,
                other => panic!("{other:?}"),
            }),
            vec![3000.0]
        );
        // Locked, typing changes the lock: the first wall moved, the second keeps 2500 from it.
        run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
        run(&mut s, "dimension_value", json!({"id": d, "value": 2500}));
        run(&mut s, "move", json!({"ids": [a], "by": [-400, 0]}));
        assert_eq!((start_x(&s, a), start_x(&s, b)), (-400.0, 2100.0));
        // Refused: no such segment, nothing long, not a dimension.
        for p in [json!({"id": d, "value": 1000, "segment": 1}), json!({"id": d, "value": 0}), json!({"id": a, "value": 1000})] {
            assert!(s.execute("dimension_value", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn align_with_its_lock_keeps_the_two_lines_on_one_line_when_the_first_wall_moves() {
        let mut s = Session::new();
        let one = |s: &mut Session, p: Value| run(s, "wall", p)["ids"][0].as_u64().unwrap_or_default();
        let a = one(&mut s, json!({"points": [[0, 0], [0, 4000]]}));
        let b = one(&mut s, json!({"points": [[500, 6000], [500, 9000]]}));
        let out =
            run(&mut s, "align", json!({"id": b, "reference": [[0, 0], [0, 4000]], "line": [[500, 6000], [500, 9000]], "lock": true}));
        assert_eq!((start_x(&s, b), out["line"].clone()), (0.0, json!([[0.0, 6000.0], [0.0, 9000.0]])));
        let lock = out["lock"].as_u64().unwrap_or_default();
        assert!(matches!(s.model.elements.get(lock), Some(crate::model::Element::Dimension(d)) if d.alignment && d.refs.len() == 2));
        assert_eq!(s.model.held_by(b), vec![lock]);
        // The first wall moved, then turned back: the second follows it, each time.
        run(&mut s, "move", json!({"ids": [a], "by": [300, 0]}));
        assert_eq!((start_x(&s, a), start_x(&s, b)), (300.0, 300.0));
        run(&mut s, "move", json!({"ids": [a], "by": [-1300, 0]}));
        assert_eq!(start_x(&s, b), -1000.0);
        // It isn't drawn: a plan shows nothing of it.
        let level = s.model.levels()[0].0;
        let view = crate::render::View { angle: 0.0, scale: 0.1, cx: 0.0, cy: 4000.0, width: 800, height: 1200 };
        assert!(!crate::render::draw(&s.model, level, view).owners.contains(&Some(lock)), "a lock is a front end's to show");
        // Unlocked (deleted), the walls are free again.
        run(&mut s, "delete", json!({"ids": [lock]}));
        run(&mut s, "move", json!({"ids": [a], "by": [500, 0]}));
        assert_eq!((start_x(&s, a), start_x(&s, b)), (-500.0, -1000.0));
    }

    #[test]
    fn a_lock_that_cant_hold_with_the_others_is_refused_and_the_project_is_as_it_was() {
        let mut s = Session::new();
        // The second wall north of the first, so aligned they lie end to end.
        let one = |s: &mut Session, p: Value| run(s, "wall", p)["ids"][0].as_u64().unwrap_or_default();
        let a = one(&mut s, json!({"points": [[0, 0], [0, 4000]]}));
        let b = one(&mut s, json!({"points": [[2000, 6000], [2000, 9000]]}));
        let d = run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": b}], "at": [1000, -1000]}))["id"].as_u64().unwrap_or_default();
        run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
        let before = s.model.elements.len();
        // Locked 2000 apart, and locked on one line: they can't be both.
        let e = s
            .execute("align", &json!({"id": b, "reference": [[0, 0], [0, 4000]], "line": [[2000, 6000], [2000, 9000]], "lock": true}))
            .unwrap_err();
        assert!(e.contains("constraints"), "{e}");
        assert_eq!((s.model.elements.len(), start_x(&s, a), start_x(&s, b)), (before, 0.0, 2000.0), "as it was");
        // Without the lock it aligns, and the locked dimension takes the first wall along.
        run(&mut s, "align", json!({"id": b, "reference": [[0, 0], [0, 4000]], "line": [[2000, 6000], [2000, 9000]]}));
        assert_eq!((start_x(&s, a), start_x(&s, b)), (-2000.0, 0.0));
    }

    #[test]
    fn an_element_is_held_by_the_dimensions_whose_locked_or_equal_segments_end_at_it() {
        let mut s = Session::new();
        let w = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        let w: Vec<Id> = w["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        let g = run(&mut s, "grid", json!({"start": [2000, -2000], "end": [2000, 8000]}))["id"].as_u64().unwrap_or_default();
        // West wall to grid, then grid to east wall: the first segment locked.
        let d = run(&mut s, "dimension", json!({"refs": [{"id": w[3]}, {"id": g}, {"id": w[1]}], "at": [0, -1000]}))["id"].as_u64();
        let d = d.unwrap_or_default();
        run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
        let held = |s: &Session, id: Id| s.model.held_by(id);
        let none: Vec<Id> = Vec::new();
        assert_eq!((held(&s, w[3]), held(&s, g)), (vec![d], vec![d]));
        assert_eq!(
            (held(&s, w[1]), held(&s, w[0])),
            (none.clone(), none.clone()),
            "the east wall's segment is free; the south wall isn't measured"
        );
        // Equal, it holds all it measures to; with no constraint, nothing.
        run(&mut s, "dimension_constraint", json!({"id": d, "locked": [], "equal": true}));
        assert_eq!(held(&s, w[1]), vec![d]);
        run(&mut s, "dimension_constraint", json!({"id": d, "equal": false}));
        assert_eq!(held(&s, g), none);
    }
}
