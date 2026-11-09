//! A default Butt for a pair of finish/structure/finish wall types at a square corner.
use super::{Arm, WallEnd};
use crate::geom::{self, P2};
use crate::model::{Element, Function, Layer, Location, Model, Wall};

pub(super) struct Butt {
    // Exterior to interior. The long lines between layers end at these turns.
    pub(super) turns: [P2; 4],
    // The actual end of the structural band, also exterior to interior.
    pub(super) core: [P2; 2],
    pub(super) draw_seam: bool,
}

fn corner_layers<'a>(m: &'a Model, a: &Arm, reach: f64) -> Option<&'a [Layer; 3]> {
    let w = m.wall(a.id)?;
    let layers: &[Layer; 3] = m.types.wall.get(&w.type_name)?.layers.as_slice().try_into().ok()?;
    if layers.each_ref().map(|l| l.function) != [Function::Finish, Function::Structure, Function::Finish]
        || layers.iter().any(|l| l.material.is_empty() || l.thickness <= 0.0 || l.variable)
        || w.arc != 0.0
        || w.location != Location::Centre
        || w.attach_base.is_some()
        || w.attach_top.is_some()
        || w.start.dist(w.end) <= 2.0 * reach
    {
        return None;
    }
    // An opening within the join can cut away a returning band. Leave it to the ordinary cap.
    let guid = m.guid(a.id);
    let occupied = m.lookup().referrers(&guid).any(|id| {
        let Some(e @ (Element::Door(o) | Element::Window(o))) = m.elements.get(id) else { return false };
        let station = if a.at_end { w.start.dist(w.end) - o.offset } else { o.offset };
        m.opening_type(e).is_some_and(|t| station - t.width / 2.0 <= reach)
    });
    (!occupied).then_some(layers)
}

pub(super) fn corner(m: &Model, me: &Arm, other: &Arm) -> Option<WallEnd> {
    if me.way.dot(other.way).abs() > 1e-9 {
        return None;
    }
    let (wall, neighbour) = (m.wall(me.id)?, m.wall(other.id)?);
    let node = |a: &Arm, w: &Wall| if a.at_end { w.end } else { w.start };
    if node(me, wall).dist(node(other, neighbour)) > 1e-6
        || (m.own_base(wall) - m.own_base(neighbour)).abs() > 1e-6
        || (m.own_top(wall) - m.own_top(neighbour)).abs() > 1e-6
    {
        return None;
    }
    let reach = me.thick.max(other.thick);
    let (mine, theirs) = (corner_layers(m, me, reach)?, corner_layers(m, other, reach)?);
    // Walk each arm clockwise to counterclockwise. Opposing faces meet in pairs.
    let sides = |arm: &Arm| if arm.at_end { [0, 2] } else { [2, 0] };
    let [cw_finish, ccw_finish] = sides(me);
    let [other_cw, other_ccw] = sides(other);
    if mine[cw_finish].material != theirs[other_ccw].material || mine[ccw_finish].material != theirs[other_cw].material {
        return None;
    }
    let cross = |p, q| geom::line_intersection(p, p + me.way, q, q + other.way);
    let normal = me.way.perp();
    let other_normal = other.way.perp();
    let cw = cross(me.cw, other.ccw)?;
    let ccw = cross(me.ccw, other.cw)?;
    let core_cw = cross(me.cw + normal * mine[cw_finish].thickness, other.ccw - other_normal * theirs[other_ccw].thickness)?;
    let core_ccw = cross(me.ccw - normal * mine[ccw_finish].thickness, other.cw + other_normal * theirs[other_cw].thickness)?;
    let cw_is_outer = (core_cw - core_ccw).dot(me.way) < 0.0;
    let (outer, inner) = if cw_is_outer { (core_cw, core_ccw) } else { (core_ccw, core_cw) };
    // The wall of the lower GUID carries its structure to the return's far core face, and the return
    // stops at the near core face of the through wall. An id is given anew when the corner comes in
    // from another file; a GUID never changes, so the same core runs through wherever the corner goes.
    let through = m.guid(me.id) < m.guid(other.id);
    let (through_way, return_way) = if through { (me.way, other.way) } else { (other.way, me.way) };
    let elbow = geom::line_intersection(outer, outer + return_way, inner, inner + through_way)?;
    let mut core = if through == cw_is_outer { [core_cw, elbow] } else { [elbow, core_ccw] };
    let mut turns = [cw, core_cw, core_ccw, ccw];
    if !me.at_end {
        turns.reverse();
        core.reverse();
    }
    Some(WallEnd {
        outline: vec![cw, core_cw, elbow, core_ccw, ccw],
        butt: Some(Butt { turns, core, draw_seam: !through && mine[1].material != theirs[1].material }),
    })
}
