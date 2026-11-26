//! Eligibility for moving an existing ordinary opening to a different straight wall.
//! Placement and fit still use `modify` and the model's checks; this module limits the tool's scope.

use crate::model::{Element, Id, Model, Opening, Wall};

fn wall(m: &Model, id: Id) -> Result<&Wall, String> {
    let w = m.wall(id).ok_or_else(|| format!("Pick New Host: {id} is not a wall"))?;
    if w.arc != 0.0 || w.panel_of.is_some() || crate::curtain::pattern(m, w).is_some() {
        return Err("Pick New Host supports ordinary straight walls, not curved walls or curtain panels".into());
    }
    if w.slant != 0.0 || !w.profile.is_empty() {
        return Err("Pick New Host does not support slanted or profiled walls; choose an ordinary upright wall".into());
    }
    if crate::groups::group_of(m, id).is_some() {
        return Err("Pick New Host does not support grouped walls; choose an ungrouped wall".into());
    }
    Ok(w)
}

/// The opening a front end may offer Pick New Host for. Edit Group is checked separately on the session.
pub fn opening(m: &Model, id: Id) -> Result<&Opening, String> {
    let Some(Element::Door(o) | Element::Window(o)) = m.elements.get(id) else {
        return Err("Pick New Host: select one ordinary door or window".into());
    };
    if m.is_pinned(id) {
        return Err(format!("Pick New Host: opening {id} is pinned; unpin it first"));
    }
    if crate::groups::group_of(m, id).is_some() {
        return Err("Pick New Host does not support grouped openings".into());
    }
    let host = m.elements.id(o.host).ok_or_else(|| "Pick New Host: the source wall is missing".to_string())?;
    wall(m, host)?;
    Ok(o)
}

/// Validate the two hosts before changing anything. Sill and flips stay relative to the host's base and faces.
pub(crate) fn target(m: &Model, id: Id, new_host: Id) -> Result<(), String> {
    let o = opening(m, id)?;
    if m.same(o.host, new_host) {
        return Err("Pick New Host: choose another wall; use Move to reposition on the current host".into());
    }
    let old = m.wall(o.host).ok_or_else(|| "Pick New Host: the source wall is missing".to_string())?;
    let new = wall(m, new_host)?;
    if old.level != new.level || (m.own_base(old) - m.own_base(new)).abs() > 1e-6 {
        return Err("Pick New Host currently requires walls on the same level and at the same base elevation".into());
    }
    Ok(())
}
