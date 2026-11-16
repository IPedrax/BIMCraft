//! The group commands: Create Group, placing model and detail groups, Ungroup, group types, Exclude and Restore Excluded,
//! Edit Group (Add, Remove, Finish, Cancel) and `resolve_group`, which runs a command that changes groups from inside with
//! what to do with them ([groups](../../../../docs/areas/groups.md)).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::{Instance, Kind, ModelGroup, Placement, Slot, instance, types, types_mut};
use crate::commands::{bad, degrees, element_id, flag, ids_param, level_param, name_param, num, only, point_opt, string, view_param};
use crate::engine::Session;
use crate::geom::P2;
use crate::ids::{Guid, Id};
use crate::library::Kind as LibKind;
use crate::model::{DetailGroup, Element, Model};

type R = Result<Value, String>;

/// What `resolve_group` does with a group a command would change from inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// The group goes; its members stay, as they now are, plain elements.
    Ungroup,
    /// The group becomes a type of its own, as it now stands.
    NewType,
}

/// The group in Edit Group: its instance, the project and the undo steps when the edit began (for Cancel), what joins it at
/// Finish (`added`), what leaves it (`removed`, elements that stay) and the members deleted meanwhile (`deleted`, by member).
#[derive(Clone, Debug)]
pub struct GroupEdit {
    pub group: Guid,
    pub before: Arc<Model>,
    pub undo_len: usize,
    pub added: Vec<Guid>,
    pub removed: Vec<Guid>,
    /// Members whose elements were deleted in the edit, by member GUID: they leave the type at Finish.
    pub deleted: Vec<Guid>,
}

/// The name a new type of `kind` takes: `want`, or `base 1`, `base 2`… the first free.
fn free_name(m: &Model, kind: Kind, base: &str) -> String {
    let have = types(m, kind);
    (1..).map(|k| format!("{base} {k}")).find(|n| !have.contains_key(n)).unwrap_or_default()
}

/// Instance `id` taken away, its members left as plain elements.
pub(crate) fn unplace(m: &mut Model, id: Id) {
    if let Some(g) = m.elements.guid(id) {
        m.pinned.remove(&g);
    }
    m.elements.remove(id);
}

/// Instance `id` made a type of its own, as it now stands: what Create New Type does with a group a command changed.
pub(crate) fn own_type(m: &mut Model, id: Id) -> Result<(), String> {
    let Some(inst) = m.elements.get(id).and_then(instance) else { return Ok(()) };
    let (kind, on, p, old) = (inst.kind, inst.on, inst.placement, inst.type_name.to_string());
    let ids: Vec<Id> = inst.members.iter().filter(|s| !s.excluded).filter_map(|s| m.elements.id(s.element)).collect();
    if ids.is_empty() {
        unplace(m, id);
        return Ok(());
    }
    // Members of its own: a type's parts are its own.
    let (t, levels, slots) = super::type_of(m, &ids, kind, on, p).map_err(|e| format!("{} {id}: {e}", kind.category()))?;
    let name = free_name(m, kind, &old);
    types_mut(m, kind).insert(name.clone(), t);
    match m.elements.get_mut(id) {
        Some(Element::ModelGroup(g)) => (g.type_name, g.members, g.levels) = (name, slots, levels),
        Some(Element::DetailGroup(g)) => (g.type_name, g.members) = (name, slots),
        _ => {}
    }
    Ok(())
}

/// Create Group (GP): elements `ids`, all model elements or all of one view's annotation, made a group type `name` (Group 1,
/// 2… unless told) and one group of it, their origin the middle of them (or `origin`). The elements stay where they are, and
/// are its members.
pub(crate) fn create_group(s: &mut Session, p: &Value) -> R {
    let c = "create_group";
    only(c, p, &["ids", "name", "origin", "level", "only"])?;
    let m = &s.model;
    let given = ids_param(c, m, p)?;
    if given.is_empty() {
        return Err(bad(c, "`ids`: the elements to group"));
    }
    // `only`: the elements of that kind, the rest of a mixed selection left out (and said).
    let (ids, left_out) = match p.get("only") {
        None | Some(Value::Null) => (given, Vec::new()),
        Some(v) => {
            let kind = match v.as_str() {
                Some("model") => Kind::Model,
                Some("detail") => Kind::Detail,
                _ => {
                    return Err(bad(
                        c,
                        "`only`: \"model\" (the model elements) or \"detail\" (one view's lines, regions, notes and dimensions)",
                    ));
                }
            };
            let keep = super::split(m, &given).of(kind).to_vec();
            if keep.is_empty() {
                return Err(bad(c, format!("there's no {} element among `ids` that a group may hold", kind.name())));
            }
            let rest = given.into_iter().filter(|id| !keep.contains(id)).collect();
            (keep, rest)
        }
    };
    if let Some(id) = ids.iter().find(|id| super::group_of(m, **id).is_some()) {
        return Err(bad(
            c,
            format!("{id} is already in group {}: ungroup that, or exclude it from it, first", super::group_of(m, *id).unwrap_or_default()),
        ));
    }
    let kind = super::kind_of_all(m, &ids).map_err(|e| bad(c, e))?;
    // What a member points at is grouped with it, before anything else is asked of them.
    let view = match kind {
        Kind::Detail => m.elements.get(ids[0]).and_then(|e| m.annotation_plan(e)),
        Kind::Model => None,
    };
    super::outside(m, &ids, kind, view).map_err(|e| bad(c, e))?;
    let origin = match point_opt(c, p, "origin")? {
        Some(o) => o,
        None => super::middle(m, &ids).ok_or_else(|| bad(c, "the elements have no place in plan to make an origin of"))?,
    };
    let on = match kind {
        Kind::Detail => {
            let e = m.elements.get(ids[0]).ok_or_else(|| bad(c, format!("no element {}", ids[0])))?;
            m.annotation_plan(e).ok_or_else(|| bad(c, "the elements are in no view"))?
        }
        Kind::Model if p.get("level").is_some_and(|v| !v.is_null()) => level_param(c, m, p, "level")?,
        Kind::Model => lowest_level(m, &ids).ok_or_else(|| bad(c, "the elements stand on no level"))?,
    };
    let name = match name_param(c, p, "name")? {
        Some(n) if types(m, kind).contains_key(&n) => return Err(bad(c, format!("there's already a {} group `{n}`", kind.name()))),
        Some(n) => n,
        None => free_name(m, kind, if kind == Kind::Model { "Model Group" } else { "Group" }),
    };
    let placement = Placement { at: origin, angle: 0.0, mirrored: false };
    let (t, levels, slots) = super::type_of(m, &ids, kind, on, placement).map_err(|e| bad(c, e))?;
    let element = match kind {
        Kind::Model => Element::ModelGroup(ModelGroup {
            level: on,
            levels,
            type_name: name.clone(),
            at: origin,
            angle: 0.0,
            mirrored: false,
            members: slots,
            flexible: false,
        }),
        Kind::Detail => {
            Element::DetailGroup(DetailGroup { view: on, type_name: name.clone(), at: origin, angle: 0.0, mirrored: false, members: slots })
        }
    };
    let m = s.model_mut();
    types_mut(m, kind).insert(name.clone(), t);
    let id = m.insert(element);
    let mut out = json!({"id": id, "name": name, "kind": kind.name(), "members": ids});
    if !left_out.is_empty() {
        out["left_out"] = json!(left_out);
    }
    Ok(out)
}

/// The lowest level elements `ids` name.
fn lowest_level(m: &Model, ids: &[Id]) -> Option<Guid> {
    let named: BTreeSet<Guid> =
        ids.iter().filter_map(|id| m.elements.get(*id)).flat_map(Element::refs).filter(|g| m.level(*g).is_some()).collect();
    super::storeys(m).into_iter().find(|g| named.contains(g))
}

/// The group `id` of `m` names, by id or GUID: an instance, or an error saying what it is.
fn group_param(c: &str, m: &Model, v: &Value) -> Result<Id, String> {
    let id = element_id(c, m, v)?;
    match m.elements.get(id) {
        Some(e) if instance(e).is_some() => Ok(id),
        Some(e) => Err(bad(c, format!("{} {id} isn't a group", e.category()))),
        None => Err(bad(c, format!("no element {id}"))),
    }
}

/// A placed instance's members, made: its slots filled from its type.
fn fill(c: &str, s: &mut Session, id: Id) -> Result<Vec<Id>, String> {
    let gone = super::regenerate(s.model_mut(), id).map_err(|e| bad(c, e))?;
    let ids: Vec<Id> = gone.iter().filter_map(|g| s.model.elements.id(*g)).collect();
    if !ids.is_empty() {
        crate::commands::delete_ids(s, &ids)?;
    }
    Ok(super::members(&s.model, id))
}

/// The placement `p` gives: `at`, `angle` and `mirrored`.
fn placement(c: &str, p: &Value) -> Result<Placement, String> {
    let at = point_opt(c, p, "at")?.ok_or_else(|| bad(c, "`at` [x, y], where the group's origin goes, is required"))?;
    Ok(Placement { at, angle: degrees(num(c, p, "angle")?.unwrap_or(0.0)).rem_euclid(360.0), mirrored: flag(c, p, "mirrored")? })
}

/// Place Detail Group: one of type `type` in a view, its members made there.
pub(crate) fn detail_group(s: &mut Session, p: &Value) -> R {
    let c = "detail_group";
    only(c, p, &["type", "at", "angle", "mirrored", "view", "level"])?;
    let m = &s.model;
    let type_name = name_param(c, p, "type")?.ok_or_else(|| bad(c, "`type`, the detail group to place, is required"))?;
    if !m.detail_groups.contains_key(&type_name) {
        let there: Vec<&str> = m.detail_groups.keys().map(String::as_str).collect();
        return Err(bad(c, format!("there's no detail group `{type_name}` (there are: {})", there.join(", "))));
    }
    let pl = placement(c, p)?;
    let view = m.guid(view_param(c, m, p, true)?);
    let g = DetailGroup { view, type_name, at: pl.at, angle: pl.angle, mirrored: pl.mirrored, members: Vec::new() };
    let id = s.model_mut().insert(Element::DetailGroup(g));
    let members = fill(c, s, id)?;
    Ok(json!({"id": id, "members": members}))
}

/// Place Model Group: one of type `type` on each level of `levels` (else on `level`, else the type's own lowest), as Revit's
/// Paste Aligned to Selected Levels places copies.
pub(crate) fn model_group(s: &mut Session, p: &Value) -> R {
    let c = "model_group";
    only(c, p, &["type", "at", "angle", "mirrored", "level", "levels", "flexible"])?;
    let m = &s.model;
    let type_name = name_param(c, p, "type")?.ok_or_else(|| bad(c, "`type`, the model group to place, is required"))?;
    if !m.model_groups.contains_key(&type_name) {
        let there: Vec<&str> = m.model_groups.keys().map(String::as_str).collect();
        return Err(bad(c, format!("there's no model group `{type_name}` (there are: {})", there.join(", "))));
    }
    let pl = placement(c, p)?;
    let levels: Vec<Guid> = match p.get("levels") {
        None | Some(Value::Null) => vec![level_param(c, m, p, "level")?],
        Some(Value::Array(a)) if a.is_empty() => return Err(bad(c, "`levels` names at least one level")),
        Some(Value::Array(a)) if a.len() > super::MAX_INSTANCES => {
            return Err(bad(c, format!("`levels` names {} levels; one command places at most {}", a.len(), super::MAX_INSTANCES)));
        }
        Some(Value::Array(a)) => {
            let mut out = Vec::new();
            for v in a {
                let g = level_param(c, m, &json!({"level": v}), "level")?;
                if !out.contains(&g) {
                    out.push(g);
                }
            }
            out
        }
        Some(v) => return Err(bad(c, format!("`levels` is a list of levels by name, id or GUID, not {v}"))),
    };
    let flexible = flag(c, p, "flexible")?;
    let mut ids = Vec::new();
    let mut all = Vec::new();
    for level in levels {
        let g = ModelGroup {
            level,
            levels: Vec::new(),
            type_name: type_name.clone(),
            at: pl.at,
            angle: pl.angle,
            mirrored: pl.mirrored,
            members: Vec::new(),
            flexible,
        };
        let id = s.model_mut().insert(Element::ModelGroup(g));
        all.extend(fill(c, s, id)?);
        ids.push(id);
    }
    if let [one] = ids.as_slice() {
        return Ok(json!({"id": one, "members": all}));
    }
    Ok(json!({"ids": ids, "members": all}))
}

/// Ungroup (UG): each group's members, plain elements where they are; the group goes, its type stays.
pub(crate) fn ungroup(s: &mut Session, p: &Value) -> R {
    let c = "ungroup";
    only(c, p, &["id", "ids"])?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let mut out = Vec::new();
    for &id in &ids {
        group_param(c, m, &json!(id))?;
        out.extend(super::members(m, id));
    }
    let m = s.model_mut();
    for id in ids {
        unplace(m, id);
    }
    Ok(json!({"ids": out}))
}

/// The instances of type `name` of `kind`, and the repeating details that repeat it.
fn placed_of(m: &Model, kind: Kind, name: &str) -> Vec<Id> {
    m.elements
        .iter()
        .filter(|(_, e)| match e {
            Element::RepeatingDetail(r) => kind == Kind::Detail && r.type_name == name,
            e => instance(e).is_some_and(|i| i.kind == kind && i.type_name == name),
        })
        .map(|(id, _)| *id)
        .collect()
}

/// Groups (the Project Browser's): with no `name`, every type of both kinds; with one, `rename` renames it (its groups follow)
/// and `delete` takes it away once nothing is of it.
pub(crate) fn group_type(s: &mut Session, p: &Value) -> R {
    let c = "group_type";
    only(c, p, &["name", "kind", "rename", "delete"])?;
    let kind = match string(c, p, "kind")?.as_deref() {
        None | Some("detail") => None,
        Some("model") => Some(Kind::Model),
        Some(k) => return Err(bad(c, format!("`kind` is model or detail, not `{k}`"))),
    };
    let Some(name) = name_param(c, p, "name")? else {
        let m = &s.model;
        let list = |kind: Kind| -> Map<String, Value> {
            types(m, kind)
                .iter()
                .map(|(n, g)| (n.clone(), json!({"members": g.members.len(), "placed": placed_of(m, kind, n).len()})))
                .collect()
        };
        return Ok(json!({"groups": list(Kind::Detail), "model_groups": list(Kind::Model)}));
    };
    // A name of one kind only needs no `kind`.
    let kind = kind.unwrap_or(if !s.model.detail_groups.contains_key(&name) && s.model.model_groups.contains_key(&name) {
        Kind::Model
    } else {
        Kind::Detail
    });
    if !types(&s.model, kind).contains_key(&name) {
        return Err(bad(c, format!("there's no {} group `{name}`", kind.name())));
    }
    if flag(c, p, "delete")? {
        if let Some(id) = placed_of(&s.model, kind, &name).first() {
            return Err(bad(c, format!("{id} is of {} group `{name}`; delete or ungroup it first", kind.name())));
        }
        types_mut(s.model_mut(), kind).remove(&name);
        return Ok(json!({"deleted": name}));
    }
    let name = match name_param(c, p, "rename")? {
        Some(to) if to != name => {
            let m = s.model_mut();
            if types(m, kind).contains_key(&to) {
                return Err(bad(c, format!("there's already a {} group `{to}`", kind.name())));
            }
            if let Some(g) = types_mut(m, kind).remove(&name) {
                types_mut(m, kind).insert(to.clone(), g);
            }
            let lib = if kind == Kind::Model { LibKind::ModelGroup } else { LibKind::DetailGroup };
            crate::commands::rename_entry(m, &lib, &name, &to);
            to
        }
        _ => name,
    };
    Ok(json!({"name": name, "kind": kind.name()}))
}

/// Exclude: members `ids` taken out of their groups here, as Revit's Exclude does: each goes, and its group remembers it
/// (Restore Excluded puts it back). A wall takes its doors and windows with it.
pub(crate) fn exclude_member(s: &mut Session, p: &Value) -> R {
    let c = "exclude_member";
    only(c, p, &["id", "ids"])?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let mut groups: BTreeSet<Id> = BTreeSet::new();
    for &id in &ids {
        let g = super::group_of(m, id)
            .ok_or_else(|| bad(c, format!("{} {id} isn't in a group", m.elements.get(id).map_or("element", Element::category))))?;
        if Some(m.guid(g)) == s.editing_group() {
            return Err(bad(c, format!("group {g} is in Edit Group: remove {id} from it there, or finish first")));
        }
        groups.insert(g);
    }
    let before = Arc::clone(&s.model);
    crate::commands::delete_ids(s, &ids)?;
    // Whatever of the groups' members went, with what it hosted, is excluded.
    let mut excluded = Vec::new();
    for g in groups {
        let went: Vec<Guid> = s
            .model
            .elements
            .get(g)
            .and_then(instance)
            .map(|i| i.members.iter().filter(|x| !x.excluded && s.model.elements.id(x.element).is_none()).map(|x| x.element).collect())
            .unwrap_or_default();
        excluded.extend(went.iter().filter_map(|e| before.elements.id(*e)));
        if let Some(slots) = s.model_mut().elements.get_mut(g).and_then(super::slots_mut) {
            for slot in slots.iter_mut().filter(|x| went.contains(&x.element)) {
                (slot.element, slot.excluded) = (Guid::default(), true);
            }
        }
    }
    Ok(json!({"excluded": excluded}))
}

/// Restore Excluded: every excluded member of groups `ids` put back as its type has it.
pub(crate) fn restore_excluded(s: &mut Session, p: &Value) -> R {
    let c = "restore_excluded";
    only(c, p, &["id", "ids"])?;
    let ids = ids_param(c, &s.model, p)?;
    let mut restored = 0;
    for &id in &ids {
        group_param(c, &s.model, &json!(id))?;
        if let Some(slots) = s.model_mut().elements.get_mut(id).and_then(super::slots_mut) {
            for slot in slots.iter_mut().filter(|s| s.excluded) {
                (slot.excluded, slot.element) = (false, Guid::default());
                restored += 1;
            }
        }
        fill(c, s, id)?;
    }
    Ok(json!({"restored": restored, "ids": ids}))
}

/// Edit Group: group `id`'s members change freely until `finish_group` or `cancel_group`; what is drawn meanwhile of its kind
/// joins it at Finish.
pub(crate) fn edit_group(s: &mut Session, p: &Value) -> R {
    let c = "edit_group";
    only(c, p, &["id"])?;
    let id = group_param(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    if let Some(g) = s.editing_group() {
        return Err(bad(c, format!("group {} is in Edit Group: finish or cancel that first", s.model.shown(g))));
    }
    let group = s.model.guid(id);
    s.group_edit = Some(Box::new(GroupEdit {
        group,
        before: Arc::clone(&s.model),
        undo_len: s.undo_len(),
        added: Vec::new(),
        removed: Vec::new(),
        deleted: Vec::new(),
    }));
    Ok(json!({"editing": id, "members": super::members(&s.model, id)}))
}

/// The group in Edit Group, as an instance id, or an error.
fn editing(c: &str, s: &Session) -> Result<Id, String> {
    let g = s.editing_group().ok_or_else(|| bad(c, "no group is in Edit Group: start one with `edit_group {id}`"))?;
    s.model.elements.id(g).ok_or_else(|| bad(c, "the group in Edit Group is gone: cancel the edit"))
}

/// Add (Edit Group): elements `ids` join the group being edited at Finish.
pub(crate) fn add_to_group(s: &mut Session, p: &Value) -> R {
    let c = "add_to_group";
    only(c, p, &["id", "ids"])?;
    let group = editing(c, s)?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let Some(inst) = m.elements.get(group).and_then(instance) else { return Err(bad(c, "the group in Edit Group is gone")) };
    for &id in &ids {
        let e = m.elements.get(id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        let k = super::kind_of(e).map_err(|why| bad(c, format!("{} {id} can't be grouped: {why}", e.category())))?;
        if k != inst.kind {
            return Err(bad(c, format!("{} {id} can't join a {} group", e.category(), inst.kind.name())));
        }
        if k == Kind::Detail && m.annotation_plan(e) != Some(inst.on) {
            return Err(bad(c, format!("{} {id} is in another view than the group", e.category())));
        }
        let removed = s.group_edit.as_ref().is_some_and(|ed| ed.removed.contains(&m.guid(id)));
        match super::group_of(m, id) {
            // A member removed in this edit comes back.
            Some(g) if g == group && removed => {}
            Some(g) if g == group => return Err(bad(c, format!("{} {id} is a member of this group already", e.category()))),
            Some(other) => return Err(bad(c, format!("{} {id} is in group {other} already", e.category()))),
            None => {}
        }
    }
    let guids: Vec<Guid> = ids.iter().map(|id| m.guid(*id)).collect();
    if let Some(ed) = &mut s.group_edit {
        for g in guids {
            // One removed is a member again; anything else joins at Finish.
            if ed.removed.contains(&g) {
                ed.removed.retain(|x| *x != g);
            } else if !ed.added.contains(&g) {
                ed.added.push(g);
            }
        }
    }
    Ok(json!({"adding": ids}))
}

/// Remove (Edit Group): members `ids` leave the group being edited at Finish, staying as plain elements; leaving it, they go
/// from every other group of its type too.
pub(crate) fn remove_from_group(s: &mut Session, p: &Value) -> R {
    let c = "remove_from_group";
    only(c, p, &["id", "ids"])?;
    let group = editing(c, s)?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let Some(inst) = m.elements.get(group).and_then(instance) else { return Err(bad(c, "the group in Edit Group is gone")) };
    let mut guids = Vec::new();
    for &id in &ids {
        let g = m.guid(id);
        let added = s.group_edit.as_ref().is_some_and(|ed| ed.added.contains(&g));
        if !added && !inst.members.iter().any(|s| s.element == g) {
            return Err(bad(c, format!("{id} isn't a member of the group in Edit Group")));
        }
        guids.push(g);
    }
    if let Some(ed) = &mut s.group_edit {
        for g in guids {
            ed.added.retain(|x| *x != g);
            if !ed.removed.contains(&g) {
                ed.removed.push(g);
            }
        }
    }
    Ok(json!({"removing": ids}))
}

/// Finish (Edit Group): the group's type made what the group now is, and every other group of it placed again.
pub(crate) fn finish_group(s: &mut Session, p: &Value) -> R {
    let c = "finish_group";
    only(c, p, &[])?;
    let group = editing(c, s)?;
    let Some(ed) = s.group_edit.as_deref().cloned() else { return Err(bad(c, "no group is in Edit Group")) };
    let m = &s.model;
    let Some(inst) = m.elements.get(group).and_then(instance) else { return Err(bad(c, "the group in Edit Group is gone")) };
    let (kind, on, pl, name) = (inst.kind, inst.on, inst.placement, inst.type_name.to_string());
    let old = types(m, kind).get(&name).cloned().unwrap_or_default();
    // What it holds now: its members still there and not removed, and what was added.
    let mut keep: BTreeMap<Guid, Guid> = BTreeMap::new();
    let mut ids: Vec<Id> = Vec::new();
    for slot in inst.members.iter().filter(|s| !s.excluded && !ed.removed.contains(&s.element)) {
        if let Some(id) = m.elements.id(slot.element) {
            keep.insert(slot.element, slot.member);
            ids.push(id);
        }
    }
    for g in &ed.added {
        if let Some(id) = m.elements.id(*g)
            && !ids.contains(&id)
            && super::group_of(m, id).is_none_or(|o| o == group)
        {
            ids.push(id);
        }
    }
    // Members excluded here stay in the type, as they were: those whose element went in the edit leave it.
    let excluded: Vec<Slot> = inst.members.iter().filter(|s| s.excluded && !ed.deleted.contains(&s.member)).copied().collect();
    if ids.is_empty() && excluded.is_empty() {
        return Err(bad(c, "the group would hold nothing: ungroup it, or cancel the edit"));
    }
    let (mut t, mut levels, mut slots) = if ids.is_empty() {
        (super::GroupType { members: Vec::new(), levels: old.levels.clone() }, inst.levels.to_vec(), Vec::new())
    } else {
        super::type_keeping(m, &ids, kind, on, pl, &keep).map_err(|e| bad(c, e))?
    };
    for slot in &excluded {
        let Some(x) = old.members.iter().find(|x| x.guid == slot.member) else { continue };
        let mut x = x.clone();
        // Its level tokens, the old type's, made the new one's.
        if kind == Kind::Model {
            let mut swap: BTreeMap<Guid, Guid> = BTreeMap::new();
            for (k, g) in inst.levels.iter().enumerate() {
                let at = match levels.iter().position(|l| l == g) {
                    Some(at) => at,
                    None => {
                        levels.push(*g);
                        t.levels.push(old.levels.get(k).copied().unwrap_or(super::Storey { above: 0, height: 0.0 }));
                        levels.len() - 1
                    }
                };
                swap.insert(super::level_token(k), super::level_token(at));
            }
            x.element = super::swapped(x.element, &swap);
        }
        t.members.push(x);
        slots.push(*slot);
    }
    let removed: Vec<Id> = ed.removed.iter().filter_map(|g| m.elements.id(*g)).collect();
    let others: Vec<Id> =
        placed_of(m, kind, &name).into_iter().filter(|id| *id != group && m.elements.get(*id).and_then(instance).is_some()).collect();
    s.group_edit = None;
    let mm = s.model_mut();
    types_mut(mm, kind).insert(name.clone(), t);
    match mm.elements.get_mut(group) {
        Some(Element::ModelGroup(g)) => (g.members, g.levels) = (slots, levels),
        Some(Element::DetailGroup(g)) => g.members = slots,
        _ => {}
    }
    for id in &others {
        fill(c, s, *id)?;
    }
    Ok(json!({"finished": group, "members": super::members(&s.model, group), "removed": removed, "updated": others}))
}

/// Cancel (Edit Group): the project as it was when the edit began, the steps since forgotten.
pub(crate) fn cancel_group(s: &mut Session, p: &Value) -> R {
    let c = "cancel_group";
    only(c, p, &[])?;
    let ed = s.group_edit.take().ok_or_else(|| bad(c, "no group is in Edit Group"))?;
    s.rewind(ed.before, ed.undo_len);
    Ok(json!({"cancelled": true}))
}

/// Runs `command` with `params` as [`Session::execute`] does, doing `fix` with any group it would change from inside.
pub(crate) fn resolve_group(s: &mut Session, p: &Value) -> R {
    let c = "resolve_group";
    only(c, p, &["fix", "command", "params"])?;
    let fix = match string(c, p, "fix")?.as_deref() {
        Some("ungroup") => Fix::Ungroup,
        Some("new_type") => Fix::NewType,
        Some(f) => return Err(bad(c, format!("`fix` is ungroup or new_type, not `{f}`"))),
        None => return Err(bad(c, "`fix`, ungroup or new_type, is required")),
    };
    let command = string(c, p, "command")?.ok_or_else(|| bad(c, "`command`, the command to run, is required"))?;
    if matches!(command.as_str(), "resolve_group" | "undo" | "redo" | "script") {
        return Err(bad(c, format!("`{command}` isn't run through it")));
    }
    let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
    let was = s.group_fix.replace(fix);
    let result = s.execute(&command, &params);
    s.group_fix = was;
    result
}

/// The instances among `ids` (by id), for the commands that take a group with its members.
pub(crate) fn instances_in(m: &Model, ids: &[Id]) -> Vec<Id> {
    ids.iter().copied().filter(|id| m.elements.get(*id).and_then(instance).is_some()).collect()
}

/// A placed instance that is a copy: its members left for the copy to place anew.
pub(crate) fn as_copy(e: &mut Element) {
    if let Some(slots) = super::slots_mut(e) {
        for s in slots.iter_mut().filter(|s| !s.excluded) {
            s.element = Guid::default();
        }
    }
}

/// Instance `to`, just made a copy of `from`: its members placed now rather than by `groups::keep`, each taking its original's
/// values of the parameters that vary by group instance, as a copy of one of Revit's groups does. Nothing to do when none
/// varies.
pub(crate) fn copy_own(m: &mut Model, from: Guid, to: Guid) -> Result<(), String> {
    let vary = super::varying(m);
    let (Some(a), Some(b)) = (m.elements.id(from), m.elements.id(to)) else { return Ok(()) };
    if vary.is_empty() {
        return Ok(());
    }
    super::regenerate(m, b)?;
    let slots = |id: Id| -> BTreeMap<Guid, Guid> {
        m.elements
            .get(id)
            .and_then(instance)
            .map(|i| i.members.iter().filter(|s| !s.excluded).map(|s| (s.member, s.element)).collect())
            .unwrap_or_default()
    };
    let (theirs, ours) = (slots(a), slots(b));
    // Each copy's member, and its original's values (none where it had none).
    type Values = Vec<(String, Option<Value>)>;
    let mut set: Vec<(Guid, Values)> = Vec::new();
    for (member, was) in theirs {
        let (Some(&now), Some(e)) = (ours.get(&member), m.elements.id(was).and_then(|i| m.elements.get(i))) else { continue };
        let values = vary.iter().map(|n| (n.clone(), e.params().and_then(|p| p.get(n)).cloned())).collect();
        set.push((now, values));
    }
    for (g, values) in set {
        let Some(params) = m.elements.id(g).and_then(|i| m.elements.get_mut(i)).and_then(Element::params_mut) else { continue };
        for (n, v) in values {
            match v {
                Some(v) => params.insert(n, v),
                None => params.remove(&n),
            };
        }
    }
    Ok(())
}

/// The instance `inst`'s origin, for what shows and snaps to it.
pub fn origin(inst: &Instance) -> P2 {
    inst.placement.at
}
