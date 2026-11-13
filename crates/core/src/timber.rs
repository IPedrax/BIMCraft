//! A read-only timber layout in a straight wall's selected layer. The wall and its hosted window own all geometry.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom;
use crate::model::{Element, Function, Guid, Id, MAX_COORD, Model, TOL, Wall};
use crate::walls::{self, Axis};

const MAX_STATIONS: usize = 4096;
const EPS: f64 = 1e-6;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    wall: Guid,
    layer: Guid,
    window: Guid,
    spacing: f64,
    thickness: f64,
    depth: f64,
    header_height: f64,
}

#[derive(Serialize)]
struct BoxXZ {
    key: String,
    xz: [f64; 4],
}

#[derive(Default, Serialize)]
struct Layout {
    members: Vec<BoxXZ>,
    bays: Vec<BoxXZ>,
}

impl Layout {
    fn member(&mut self, key: &str, xz: [f64; 4]) {
        self.members.push(BoxXZ { key: key.to_owned(), xz });
    }

    // Fill the clear stretch between two named faces. Grid pieces must fit wholly between those faces.
    // At each station the previous member's right face and this one's left face bound a clear bay.
    fn fill(&mut self, region: &str, left: (&str, f64), right: (&str, f64), z: [f64; 2], rule: &Request) {
        let (mut previous, mut start) = (left.0.to_owned(), left.1);
        let half = rule.thickness / 2.0;
        let first = ((left.1 + half - EPS) / rule.spacing).ceil().max(1.) as usize;
        let last = ((right.1 - half + EPS) / rule.spacing).floor() as usize;
        for i in first..=last {
            let center = i as f64 * rule.spacing;
            let key = format!("{region}/grid/{}/{i}", rule.spacing);
            let lo = (center - half).max(left.1);
            let hi = (center + half).min(right.1);
            self.bay(region, (&previous, start), (&key, lo), z);
            self.member(&key, [lo, hi, z[0], z[1]]);
            previous = key;
            start = hi;
        }
        self.bay(region, (&previous, start), right, z);
    }

    fn bay(&mut self, region: &str, left: (&str, f64), right: (&str, f64), z: [f64; 2]) {
        if right.1 - left.1 > EPS {
            let key = format!("{region}|{}|{}", left.0, right.0);
            self.bays.push(BoxXZ { key, xz: [left.1, right.1, z[0], z[1]] });
        }
    }
}

fn layout(rule: &Request, length: f64, height: f64, cut: [f64; 4], window: Guid) -> Result<Layout, String> {
    let t = rule.thickness;
    if length / rule.spacing > MAX_STATIONS as f64 {
        return Err(format!("layout exceeds {MAX_STATIONS} grid stations"));
    }
    let [left, right, sill, head] = cut;
    let full = [t, height - 2. * t];
    let lower = [t, sill - t];
    let upper = [head + rule.header_height, full[1]];
    if left - 2. * t < t || right + 2. * t > length - t || lower[1] <= lower[0] || upper[1] <= upper[0] {
        return Err("window framing needs room between the end studs and plates".into());
    }

    let mut out = Layout::default();
    out.member("bottom-plate", [0., length, 0., t]);
    out.member("top-plate/1", [0., length, full[1], height - t]);
    out.member("top-plate/2", [0., length, height - t, height]);
    out.member("start-stud", [0., t, full[0], full[1]]);
    out.member("end-stud", [length - t, length, full[0], full[1]]);
    let left_king = format!("{window}/left-king");
    let right_king = format!("{window}/right-king");
    let left_jack = format!("{window}/left-jack");
    let right_jack = format!("{window}/right-jack");
    out.member(&left_king, [left - 2. * t, left - t, full[0], full[1]]);
    out.member(&right_king, [right + t, right + 2. * t, full[0], full[1]]);
    out.member(&left_jack, [left - t, left, full[0], head]);
    out.member(&right_jack, [right, right + t, full[0], head]);
    out.member(&format!("{window}/header"), [left - t, right + t, head, upper[0]]);
    out.member(&format!("{window}/sill"), [left, right, lower[1], sill]);
    out.fill("full", ("start-stud", t), (&left_king, left - 2. * t), full, rule);
    out.fill("full", (&right_king, right + 2. * t), ("end-stud", length - t), full, rule);
    out.fill(&format!("{window}/lower"), (&left_jack, left), (&right_jack, right), lower, rule);
    out.fill(&format!("{window}/upper"), (&left_king, left - t), (&right_king, right + t), upper, rule);
    Ok(out)
}

// Restrict this first layout to an isolated wall. The spatial lookup supplies candidates; test their
// actual plan outlines against the host footprint so sharing a tile alone never rejects a wall.
fn touching_wall(m: &Model, id: Id, w: &Wall) -> Option<Id> {
    let axis = Axis::of(w);
    let dir = axis.tangent(0.);
    let (ext, int) = walls::faces(m, w);
    let reach = walls::reach(m, w)?;
    let max_width = m.types.wall.values().map(|t| t.thickness).fold(0., f64::max);
    let (lo, hi) = walls::heights(m, w);
    m.lookup().walls_near_at(&[(reach, lo, hi)], max_width + TOL).into_iter().find(|&other| {
        if other == id || !m.wall(other).is_some_and(|n| walls::stacked(m, w, n)) {
            return false;
        }
        let ring = walls::outline(m, other);
        let ring = geom::clip(&ring, |p| (p - w.start).dot(dir) + TOL);
        let ring = geom::clip(&ring, |p| axis.length() - (p - w.start).dot(dir) + TOL);
        let ring = geom::clip(&ring, |p| (p - w.start).dot(dir.perp()) - int + TOL);
        !geom::clip(&ring, |p| ext - (p - w.start).dot(dir.perp()) + TOL).is_empty()
    })
}

pub(crate) fn query(s: &mut Session, params: &Value) -> Result<Value, String> {
    let rule: Request = serde_json::from_value(params.clone()).map_err(|e| format!("timber_framing: {e}"))?;
    if [rule.spacing, rule.thickness, rule.depth, rule.header_height].iter().any(|&v| !v.is_finite() || v <= EPS || v > MAX_COORD)
        || rule.spacing <= rule.thickness
    {
        return Err("supply positive framing dimensions in mm and spacing greater than stud thickness".into());
    }
    let m = &s.model;
    let id = m.elements.id(rule.wall).ok_or("unknown framing wall")?;
    let w = m.wall(id).ok_or("select a wall for timber framing")?;
    if w.arc != 0. || w.attach_top.is_some() || w.attach_base.is_some() {
        return Err("timber framing requires a straight wall with flat, unattached top and base".into());
    }
    let ty = m.types.wall.get(&w.type_name).ok_or("unknown wall type")?;
    let (index, layer) = ty
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| l.guid == rule.layer || Some(l.id) == rule.layer.pending())
        .ok_or("select a layer belonging to this wall type")?;
    if layer.function != Function::Structure || (layer.thickness - rule.depth).abs() > EPS {
        return Err("select a structural layer matching the framing depth".into());
    }
    let wall = m.guid(id);
    let e = m.elements.get(rule.window).ok_or("unknown framing window")?;
    let Element::Window(o) = e else { return Err("select a rectangular hosted window".into()) };
    if o.host != wall {
        return Err("window belongs to another wall".into());
    }
    let window = m.elements.guid(rule.window).ok_or("unknown framing window")?;
    if m.lookup()
        .referrers(&wall)
        .any(|k| m.guid(k) != window && matches!(m.elements.get(k), Some(Element::Door(n)|Element::Window(n)) if n.host==wall))
    {
        return Err("timber framing supports one hosted window and no other openings".into());
    }
    if let Some(other) = touching_wall(m, id, w) {
        return Err(format!("timber framing requires an isolated wall; wall {other} meets it"));
    }
    let dims = m.opening_dims(e)?;
    let (middle, sill) = m.opening_middle(o);
    let cut = [middle - dims.width / 2., middle + dims.width / 2., sill, sill + dims.height];
    let axis = Axis::of(w);
    let (base, top) = walls::heights(m, w);
    let result = layout(&rule, axis.length(), top - base, cut, window)?;
    let interior = walls::boundaries(m, w)[index + 1];
    let origin = axis.at(0., interior);
    let dir = axis.tangent(0.);
    Ok(json!({"units":"mm","wall":wall,"layer":layer.guid,"window":window,"opening_xz":cut,
        "origin":[origin.0,origin.1,base],"x_axis":dir,"depth_axis":dir.perp(),"depth":layer.thickness,
        "members":result.members,"bays":result.bays}))
}
