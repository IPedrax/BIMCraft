//! After every command: each group the command touched is held to its type ([groups](../../../../docs/areas/groups.md)). One
//! placed anew (moved, turned, mirrored, copied, given another level or type) has its members placed again; one changed from
//! inside is a conflict, refused unless `resolve_group` said what to do with it.

use std::collections::BTreeSet;

use super::commands::Fix;
use super::{CONFLICT, Kind, instance, instances};
use crate::engine::Session;
use crate::ids::{Changed, Id};
use crate::model::Model;

/// The most of a conflict's changes its error lists.
const SAID: usize = 6;

/// The groups command `before` → `s.model` touched, placed again or held to their types; `first_new` the first id it made.
/// Whether it changed anything.
pub fn keep(s: &mut Session, before: &Model, first_new: Id) -> Result<bool, String> {
    if !super::any(&s.model) && !super::any(before) {
        return Ok(false);
    }
    let was = std::sync::Arc::clone(&s.model);
    adopt(s, first_new);
    forget_deleted(s);
    let editing = s.group_edit.as_ref().map(|e| e.group);
    let candidates = touched(&s.model, before);
    let mut conflicts: Vec<(Id, Vec<String>)> = Vec::new();
    let mut gone = Vec::new();
    for id in candidates {
        let m = &s.model;
        let Some(now) = m.elements.get(id) else { continue };
        if instance(now).is_none() || Some(m.guid(id)) == editing {
            continue;
        }
        // Placed anew (moved, turned, copied, given another level or type): placed again from its type, unless its members
        // already stand as it places them.
        if before.elements.get(id) != Some(now) {
            if super::unplaced(m, id) || !super::conflicts(m, id).is_empty() {
                gone.extend(super::regenerate(s.model_mut(), id)?);
            }
            continue;
        }
        let found = super::conflicts(m, id);
        if !found.is_empty() {
            conflicts.push((id, found));
        }
    }
    if !gone.is_empty() {
        let ids: Vec<Id> = gone.iter().filter_map(|g| s.model.elements.id(*g)).collect();
        if !ids.is_empty() {
            crate::commands::delete_ids(s, &ids)?;
        }
    }
    if conflicts.is_empty() {
        return Ok(!std::sync::Arc::ptr_eq(&was, &s.model));
    }
    match s.group_fix {
        Some(Fix::Ungroup) => {
            let m = s.model_mut();
            for (id, _) in &conflicts {
                super::commands::unplace(m, *id);
            }
            Ok(true)
        }
        Some(Fix::NewType) => {
            for (id, _) in &conflicts {
                super::commands::own_type(s.model_mut(), *id)?;
            }
            Ok(true)
        }
        None => Err(said(&s.model, &conflicts)),
    }
}

/// The groups `before` → `m` may have touched: those changed, those whose members or levels changed, and those of types that
/// changed; every one when what changed can't be told.
fn touched(m: &Model, before: &Model) -> Vec<Id> {
    if m.model_groups != before.model_groups || m.detail_groups != before.detail_groups {
        return instances(m);
    }
    let Changed::Ids(changed) = m.changed_since(before) else { return instances(m) };
    let mut out: BTreeSet<Id> = BTreeSet::new();
    let (now, was) = (m.lookup(), super::any(before).then(|| before.lookup()));
    for &id in &changed {
        if m.elements.get(id).and_then(instance).is_some() {
            out.insert(id);
            continue;
        }
        if let Some(g) = m.elements.guid(id) {
            out.extend(now.referrers(&g).filter(|r| m.elements.get(*r).and_then(instance).is_some()));
        }
        if let (Some(look), Some(g)) = (&was, before.elements.guid(id)) {
            out.extend(look.referrers(&g).filter(|r| before.elements.get(*r).and_then(instance).is_some()));
        }
    }
    out.into_iter().collect()
}

/// In Edit Group, what the command made of the group's kind (in its view, for a detail group) joins it at Finish.
fn adopt(s: &mut Session, first_new: Id) {
    let Some(ed) = &s.group_edit else { return };
    let m = &s.model;
    let Some(inst) = m.elements.id(ed.group).and_then(|i| m.elements.get(i)).and_then(instance) else { return };
    let made: Vec<_> = m
        .elements
        .iter()
        .filter(|(id, _)| **id >= first_new)
        .filter(|(_, e)| super::kind_of(e).is_ok_and(|k| k == inst.kind))
        .filter(|(_, e)| inst.kind == Kind::Model || m.annotation_plan(e) == Some(inst.on))
        .map(|(id, _)| m.guid(*id))
        .collect();
    if let Some(ed) = &mut s.group_edit {
        for g in made {
            if !ed.added.contains(&g) {
                ed.added.push(g);
            }
        }
    }
}

/// In Edit Group, a member deleted: excluded until Finish, which takes it out of the type.
fn forget_deleted(s: &mut Session) {
    let Some(ed) = &s.group_edit else { return };
    let m = &s.model;
    let Some(id) = m.elements.id(ed.group) else { return };
    let went: Vec<crate::ids::Guid> = m
        .elements
        .get(id)
        .and_then(instance)
        .map(|i| i.members.iter().filter(|x| !x.excluded && m.elements.id(x.element).is_none()).map(|x| x.member).collect())
        .unwrap_or_default();
    if went.is_empty() {
        return;
    }
    if let Some(slots) = s.model_mut().elements.get_mut(id).and_then(super::slots_mut) {
        for slot in slots.iter_mut().filter(|x| went.contains(&x.member)) {
            (slot.element, slot.excluded) = (crate::ids::Guid::default(), true);
        }
    }
    if let Some(ed) = &mut s.group_edit {
        ed.deleted.extend(went);
    }
}

/// The error a conflict refuses a command with.
fn said(m: &Model, conflicts: &[(Id, Vec<String>)]) -> String {
    let groups: Vec<String> = conflicts
        .iter()
        .map(|(id, what)| {
            let (kind, name) = m.elements.get(*id).and_then(instance).map_or(("", ""), |i| (i.kind.name(), i.type_name));
            let mut listed: Vec<String> = what.iter().take(SAID).cloned().collect();
            if what.len() > SAID {
                listed.push(format!("{} more", what.len() - SAID));
            }
            format!("{kind} group `{name}` ({id}): {}", listed.join("; "))
        })
        .collect();
    format!(
        "{CONFLICT}: this changes {} from inside: {}. A group changes as a whole: move, turn or mirror the group, or change its \
         members in Edit Group (`edit_group`). To go on, run the same command with `resolve_group {{fix: \"ungroup\" | \"new_type\", \
         command, params}}`: `ungroup` makes the changed groups plain elements first, `new_type` makes each a group type of its own",
        if conflicts.len() == 1 { "a group" } else { "groups" },
        groups.join(" | ")
    )
}
