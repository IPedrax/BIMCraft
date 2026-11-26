//! A framed wall's members changed by hand: one given another section or number of plies, or left out (Revit's framing
//! elements edited one by one). Each is a [`MemberEdit`] in the wall's `framing_edits`, keyed by the member's `key`, with an id
//! and a GUID of its own ([`crate::library::HasPart`]). The members are still worked out from the wall ([`super::wall`]); the
//! edits are laid over them after, so the 3D view, plans, sections, the schedule, the cut list and IFC all take them. An edit
//! whose member is no longer there (a window moved away, the studs laid out again) sleeps, and wakes when it comes back.
//!
//! A member given another size keeps its place: an upright one stays centred where it stood, along the wall and across it; a
//! lying one stays centred across the wall, on what it lay on (a top plate and a sill under what they lay under). Nothing
//! round it moves; one that comes out wider than the structure layer or runs into another member is warned of.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{EPS, Framing, Grain, Member, Role};
use crate::commands::bad;
use crate::engine::Session;
use crate::model::Model;

/// The most edits a wall holds.
pub const MAX_EDITS: usize = 4096;
/// The longest a member's key is, in characters.
pub const MAX_KEY: usize = 200;
/// The most plies a member edited is of.
pub const MAX_PLIES: u32 = 8;
/// The deepest or thickest a section edited is, mm.
pub const MAX_SECTION: f64 = 1000.0;

/// One member of a framed wall, by its key, given another section or number of plies, or left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberEdit {
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: crate::ids::Id,
    #[serde(default, skip_serializing_if = "crate::ids::Guid::is_nil")]
    pub guid: crate::ids::Guid,
    /// The member's key, as `framing` lists it.
    pub key: String,
    /// One ply's section as sawn, mm, in place of the type's: [thickness, depth] as `framing` gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plies: Option<u32>,
    /// Left out: the wall is framed without it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub excluded: bool,
}

impl MemberEdit {
    /// Whether it changes nothing, so it goes.
    fn is_empty(&self) -> bool {
        self.section.is_none() && self.plies.is_none() && !self.excluded
    }
}

impl crate::library::HasPart for MemberEdit {
    fn ident(&mut self) -> (&mut crate::ids::Id, &mut crate::ids::Guid) {
        (&mut self.id, &mut self.guid)
    }
}

/// What a wall's edits must be, which `Model::check` asks: not too many, each of a key, one to a key, a section and plies in
/// range.
pub(crate) fn check(edits: &[MemberEdit]) -> Result<(), String> {
    if edits.len() > MAX_EDITS {
        return Err(format!("{} framing members edited; a wall holds {MAX_EDITS} at most", edits.len()));
    }
    let mut keys = std::collections::BTreeSet::new();
    for e in edits {
        if e.key.is_empty() || e.key.chars().count() > MAX_KEY {
            return Err(format!("a framing member's key is 1 to {MAX_KEY} characters: `{}`", e.key));
        }
        if !keys.insert(e.key.as_str()) {
            return Err(format!("framing member `{}` is edited twice; one edit a member", e.key));
        }
        if let Some(s) = e.section
            && !s.iter().all(|x| x.is_finite() && *x >= 1.0 && *x <= MAX_SECTION)
        {
            return Err(format!("framing member `{}`: its section {s:?} is two sizes from 1 to {MAX_SECTION} mm", e.key));
        }
        if let Some(p) = e.plies
            && !(1..=MAX_PLIES).contains(&p)
        {
            return Err(format!("framing member `{}`: {p} plies; a member is of 1 to {MAX_PLIES}", e.key));
        }
    }
    Ok(())
}

/// Framing `f` (a wall's, its structure layer `[interior side, exterior side]` across it) with `edits` laid over it: the
/// members left out gone, those resized in their place, and a warning for each that no longer fits.
pub(crate) fn apply(f: &mut Framing, edits: &[MemberEdit], layer: [f64; 2]) {
    if edits.is_empty() {
        return;
    }
    let by_key: std::collections::HashMap<&str, &MemberEdit> = edits.iter().map(|e| (e.key.as_str(), e)).collect();
    f.members.retain(|m| !by_key.get(m.key.as_str()).is_some_and(|e| e.excluded));
    let mut resized = Vec::new();
    for (i, member) in f.members.iter_mut().enumerate() {
        let Some(e) = by_key.get(member.key.as_str()) else { continue };
        if e.section.is_none() && e.plies.is_none() {
            continue;
        }
        resize(member, e.section.unwrap_or(member.section), e.plies.unwrap_or(member.plies));
        resized.push(i);
    }
    for i in resized {
        let Some(member) = f.members.get(i) else { continue };
        if member.across[0] < layer[0] - EPS || member.across[1] > layer[1] + EPS {
            f.warnings.push(format!(
                "{} `{}` is {:.0} mm deep across the wall, deeper than its {:.0} mm structure layer",
                member.role.name(),
                member.key,
                member.across[1] - member.across[0],
                layer[1] - layer[0]
            ));
        }
        if let Some(other) = f.members.iter().enumerate().find(|(j, o)| *j != i && overlap(member, o)).map(|(_, o)| o) {
            f.warnings.push(format!("{} `{}` runs into {} `{}`", member.role.name(), member.key, other.role.name(), other.key));
        }
    }
}

/// How wide `m` is along its run, across the wall and up, as its section and plies make it, for its role: an upright one's plies
/// side by side along the wall; a header's on edge side by side across it, or flat stacked; a plate's, a sill's or blocking's
/// stacked.
fn extents(m: &Member, section: [f64; 2], plies: u32) -> (Option<f64>, f64, Option<f64>) {
    let p = f64::from(plies.max(1));
    match (m.grain, m.role) {
        (Grain::Up, _) => (Some(p * section[0]), section[1], None),
        (_, Role::Header) if flat_header(m) => (None, section[1], Some(p * section[0])),
        (_, Role::Header) => (None, p * section[0], Some(section[1])),
        _ => (None, section[1], Some(p * section[0])),
    }
}

/// Whether header `m` lies flat: as wide across the wall as its section is deep, not its plies side by side.
fn flat_header(m: &Member) -> bool {
    let wide = m.across[1] - m.across[0];
    (wide - m.section[1]).abs() < (wide - f64::from(m.plies) * m.section[0]).abs()
}

/// `m` given `section` and `plies` in its place: see the module's notes.
fn resize(m: &mut Member, section: [f64; 2], plies: u32) {
    let (s_old, across_old, up_old) = extents(m, m.section, m.plies);
    let (s_new, across_new, up_new) = extents(m, section, plies);
    let centred = |r: [f64; 2], old: f64, new: f64| {
        let c = (r[0] + r[1]) / 2.0;
        if old > EPS { [c - new / 2.0, c + new / 2.0] } else { r }
    };
    m.across = centred(m.across, across_old, across_new);
    if let (Some(old), Some(new)) = (s_old, s_new) {
        m.s = centred(m.s, old, new);
    }
    if let (Some(old), Some(new)) = (up_old, up_new)
        && old > EPS
    {
        let k = new / old;
        // A top plate and a sill keep their tops, under what they lie under; the rest their bottoms.
        if matches!(m.role, Role::TopPlate | Role::Sill) {
            m.bottom = [0, 1].map(|i| m.top[i] - (m.top[i] - m.bottom[i]) * k);
        } else {
            m.top = [0, 1].map(|i| m.bottom[i] + (m.top[i] - m.bottom[i]) * k);
        }
    }
    m.section = section;
    m.plies = plies;
}

/// Whether two members of one wall share more than a touch: their boxes along it, across it and up overlap.
fn overlap(a: &Member, b: &Member) -> bool {
    let up = |m: &Member| (m.bottom[0].min(m.bottom[1]), m.top[0].max(m.top[1]));
    let ((a0, a1), (b0, b1)) = (up(a), up(b));
    let apart = |x: [f64; 2], y: [f64; 2]| x[1] <= y[0] + EPS || y[1] <= x[0] + EPS;
    !(apart(a.s, b.s) || apart(a.across, b.across) || a1 <= b0 + EPS || b1 <= a0 + EPS)
}

/// `framing_member`: members of a framed wall given another section or plies, left out, taken back in, or reset.
pub(crate) fn command(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "framing_member";
    crate::commands::only(c, p, &["id", "key", "keys", "section", "plies", "exclude", "reset"])?;
    let m = &s.model;
    let ids = crate::commands::ids_param(c, m, p)?;
    let [id] = ids[..] else { return Err(bad(c, "`id`, one framed wall, is required")) };
    let w = m.wall(id).ok_or_else(|| bad(c, format!("{id} isn't a wall: only a wall's framing members are edited one by one")))?;
    if w.framing.is_none() {
        return Err(bad(c, format!("wall {id} isn't framed: `frame` it first")));
    }
    let mut keys: Vec<String> = match (p.get("key"), p.get("keys")) {
        (Some(k), None) => vec![k.as_str().ok_or_else(|| bad(c, "`key` is a member's key, as `framing` lists it"))?.to_string()],
        (None, Some(Value::Array(ks))) => ks
            .iter()
            .map(|k| k.as_str().map(String::from).ok_or_else(|| bad(c, "`keys` are members' keys, as `framing` lists them")))
            .collect::<Result<_, _>>()?,
        _ => return Err(bad(c, "give `key`, one member's key, or `keys`, a list of them, as `framing` lists them")),
    };
    keys.dedup();
    if keys.is_empty() || keys.len() > MAX_EDITS {
        return Err(bad(c, format!("1 to {MAX_EDITS} keys")));
    }
    let section = match p.get("section") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let a: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            match a[..] {
                [t, d] if [t, d].iter().all(|x| x.is_finite() && *x >= 1.0 && *x <= MAX_SECTION) => Some([t, d]),
                _ => return Err(bad(c, format!("`section` is [thickness, depth], each 1 to {MAX_SECTION} mm"))),
            }
        }
    };
    let plies = match p.get("plies") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| (1..=MAX_PLIES).contains(n))
                .ok_or_else(|| bad(c, format!("`plies` is a whole number from 1 to {MAX_PLIES}")))?,
        ),
    };
    let flag = |k: &str| match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(c, format!("`{k}` is true or false"))),
    };
    let (exclude, reset) = (flag("exclude")?, flag("reset")?.unwrap_or(false));
    if !reset && section.is_none() && plies.is_none() && exclude.is_none() {
        return Err(bad(c, "give `section`, `plies`, `exclude` or `reset`"));
    }
    // Every key must name a member the wall has, as it is framed without its edits; a reset may name an edit that sleeps.
    let all = super::unedited(m, id).map_err(|e| bad(c, format!("wall {id}: {e}")))?.map(|f| f.members).unwrap_or_default();
    let edited = |k: &String| reset && w.framing_edits.iter().any(|e| &e.key == k);
    if let Some(k) = keys.iter().find(|k| !all.iter().any(|x| &x.key == *k) && !edited(k)) {
        return Err(bad(c, format!("wall {id} has no framing member `{k}`; `framing {{id: {id}}}` lists its members' keys")));
    }
    let Some(crate::model::Element::Wall(w)) = s.model_mut().elements.get_mut(id) else { return Err(bad(c, "no such wall")) };
    for k in &keys {
        let at = w.framing_edits.iter().position(|e| &e.key == k);
        let mut e = at.map(|i| w.framing_edits[i].clone()).unwrap_or_else(|| MemberEdit { key: k.clone(), ..MemberEdit::default() });
        if reset {
            e = MemberEdit { id: e.id, guid: e.guid, key: k.clone(), ..MemberEdit::default() };
        }
        if section.is_some() {
            e.section = section;
        }
        if plies.is_some() {
            e.plies = plies;
        }
        if let Some(x) = exclude {
            e.excluded = x;
        }
        match (at, e.is_empty()) {
            (Some(i), true) => {
                w.framing_edits.remove(i);
            }
            (Some(i), false) => w.framing_edits[i] = e,
            (None, false) => w.framing_edits.push(e),
            (None, true) => {}
        }
    }
    let m = &s.model;
    let f = super::wall(m, id).map_err(|e| bad(c, format!("wall {id}: {e}")))?;
    let members: Vec<Value> = f
        .as_ref()
        .map(|f| f.members.iter().filter(|x| keys.contains(&x.key)).map(|x| super::member_json(f, x)).collect())
        .unwrap_or_default();
    let edits = m.wall(id).map(|w| serde_json::to_value(&w.framing_edits).unwrap_or(Value::Null)).unwrap_or(Value::Null);
    Ok(json!({"id": id, "members": members, "edits": edits, "warnings": f.map(|f| f.warnings.clone()).unwrap_or_default()}))
}

/// The member of framing `f` a ray from `from` toward `to` meets first, of those `keep` keeps: its key and how far along the ray,
/// as a share of `from` to `to`. What Tab picks in a 3D view, a section or an elevation.
pub fn on_ray(f: &Framing, from: [f64; 3], to: [f64; 3], keep: &dyn Fn(&Member) -> bool) -> Vec<(String, f64)> {
    let local = |p: [f64; 3]| {
        let (u, v) = (f.x_axis, f.depth_axis());
        let d = crate::geom::P2(p[0], p[1]) - f.origin;
        [d.dot(u), d.dot(v), p[2]]
    };
    let (a, b) = (local(from), local(to));
    let mut hits: Vec<(String, f64)> = f
        .members
        .iter()
        .filter(|m| m.frame.is_none() && keep(m))
        .filter_map(|m| {
            let lo = [m.s[0], m.across[0], m.bottom[0].min(m.bottom[1])];
            let hi = [m.s[1], m.across[1], m.top[0].max(m.top[1])];
            let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
            for k in 0..3 {
                let d = b[k] - a[k];
                if d.abs() < 1e-12 {
                    if a[k] < lo[k] || a[k] > hi[k] {
                        return None;
                    }
                    continue;
                }
                let (u, v) = ((lo[k] - a[k]) / d, (hi[k] - a[k]) / d);
                t0 = t0.max(u.min(v));
                t1 = t1.min(u.max(v));
            }
            (t1 >= t0 && t1 >= 0.0).then(|| (m.key.clone(), t0.max(0.0)))
        })
        .collect();
    hits.sort_by(|x, y| x.1.total_cmp(&y.1));
    hits
}

/// The members of framing `f` a plan cutting at height `z` shows within `reach` mm of point `q`: their keys, nearest first. What
/// Tab picks in a plan.
pub fn in_plan(f: &Framing, q: crate::geom::P2, z: f64, reach: f64) -> Vec<String> {
    let d = q - f.origin;
    let (s, a) = (d.dot(f.x_axis), d.dot(f.depth_axis()));
    let mut near: Vec<(f64, String)> = f
        .members
        .iter()
        .filter(|m| m.frame.is_none())
        .filter_map(|m| {
            let [s0, s1] = m.cut_at(z)?;
            let gap = |x: f64, r: [f64; 2]| (r[0] - x).max(x - r[1]).max(0.0);
            let off = gap(s, [s0.min(s1), s0.max(s1)]).hypot(gap(a, m.across));
            (off <= reach).then(|| (off + (m.across[1] - m.across[0]).min(m.s[1] - m.s[0]) * 1e-6, m.key.clone()))
        })
        .collect();
    near.sort_by(|x, y| x.0.total_cmp(&y.0));
    near.into_iter().map(|(_, k)| k).collect()
}

/// A member of framing `f` as its eight corners in the model, bottom four then top four: what a view outlines when it is picked.
pub fn corners(f: &Framing, m: &Member) -> [[f64; 3]; 8] {
    let at = |s: f64, a: f64, z: f64| {
        let p = f.at(s, a);
        [p.0, p.1, z]
    };
    let [s0, s1] = m.s;
    let [a0, a1] = m.across;
    [
        at(s0, a0, m.bottom[0]),
        at(s1, a0, m.bottom[1]),
        at(s1, a1, m.bottom[1]),
        at(s0, a1, m.bottom[0]),
        at(s0, a0, m.top[0]),
        at(s1, a0, m.top[1]),
        at(s1, a1, m.top[1]),
        at(s0, a1, m.top[0]),
    ]
}

/// The edit of wall `id`'s member `key`, if it has one.
pub fn of<'a>(m: &'a Model, id: crate::model::Id, key: &str) -> Option<&'a MemberEdit> {
    m.wall(id)?.framing_edits.iter().find(|e| e.key == key)
}
