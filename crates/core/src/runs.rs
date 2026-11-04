//! Runs: the geometry of a straight element between two points, whatever it carries (a beam, a strip footing, a pipe, a
//! conduit, a cable tray, a wire, a conductor, a cable): the ends of its centre line, its footprint in plan, the prism or the
//! tube it makes in 3D. It knows no category but for the table that gives its IFC class and the defaults it takes (may it stand,
//! where its heights are); the plan, the section, the 3D view, the snaps and IFC ask it.

use crate::geom::P2;
use crate::model::{Justify, MAX_SLOPE, Model, Run, RunType, TOL, heading};
use crate::view3d::{P3, add, cross, mul, sub, unit};
use crate::walls::Plane;

mod connections;
mod device_exit;

impl Model {
    /// Origin on the named host face, unit tangent, outward normal. Curved
    /// walls need a different contact representation and cannot use a single plane.
    pub(crate) fn run_mount_frame(&self, mount: &crate::model::RunMount) -> Result<(P3, P3, P3), String> {
        let host = self.elements.get(mount.host).ok_or("mount host is missing")?;
        let slab = match host {
            crate::model::Element::Floor(f) => {
                let top = self.floor_top(f);
                Some((top, self.floor_depth(f), true, &f.boundary))
            }
            crate::model::Element::Ceiling(c) => {
                let under = Plane::flat(self.elevation(c.level) + c.offset);
                let thickness = self.types.ceiling.get(&c.type_name).map_or(0.0, |t| t.thickness);
                Some((under, thickness, false, &c.boundary))
            }
            _ => None,
        };
        if let Some((mut plane, thickness, is_top, boundary)) = slab {
            let sign = match mount.face.as_str() {
                "top" => 1.0,
                "bottom" => -1.0,
                _ => return Err("slab mount face must be top or bottom".into()),
            };
            if is_top && sign < 0.0 {
                plane.z -= thickness;
            }
            if !is_top && sign > 0.0 {
                plane.z += thickness;
            }
            let normal = mul(unit([-plane.g.0, -plane.g.1, 1.0]), sign);
            let at = *boundary.first().ok_or("mount host boundary is empty")?;
            let edge = boundary.iter().map(|p| *p - at).find(|d| d.dot(*d) > 1e-12).ok_or("mount host boundary has no direction")?;
            return Ok(([at.0, at.1, plane.at(at)], unit([edge.0, edge.1, plane.g.dot(edge)]), normal));
        }
        let crate::model::Element::Wall(wall) = host else {
            return Err("mount host must be a straight wall, floor or ceiling".into());
        };
        if wall.arc.abs() > 1e-9 {
            return Err("curved wall mounting is not supported by a planar contact line".into());
        }
        let along = (wall.end - wall.start).unit();
        let left = along.perp();
        let (exterior, interior) = crate::walls::faces(self, wall);
        let (offset, sign) = match mount.face.as_str() {
            "exterior" => (exterior, 1.0),
            "interior" => (interior, -1.0),
            _ => return Err("wall mount face must be exterior or interior".into()),
        };
        let at = wall.start + left * offset;
        Ok(([at.0, at.1, self.base_z(wall)], [along.0, along.1, 0.0], [left.0 * sign, left.1 * sign, 0.0]))
    }

    /// Follow a moving host unless the same command explicitly moved the contact line.
    /// Removing a host detaches at the last physical axis, preserving cable geometry.
    pub(crate) fn follow_run_mounts(&mut self, before: &Self) -> Result<(), String> {
        let mut updates = Vec::new();
        for (id, element) in self.elements.iter() {
            let Some(run) = element.run() else { continue };
            let Some(mount) = &run.mount else { continue };
            let old_element = before.elements.get(*id);
            let old = old_element.and_then(crate::model::Element::run);
            if self.elements.get(mount.host).is_none() {
                if let Some(old_element) = old_element.filter(|_| old.is_some_and(|old| old.mount == run.mount))
                    && before.elements.get(mount.host).is_some()
                    && let Some(geometry) = before.run_geom(old_element)
                {
                    let mut updated = run.clone();
                    updated.mount = None;
                    updated.section_roll = geometry.roll;
                    updated.square_ends = geometry.square_ends;
                    updated.start = P2(geometry.a[0], geometry.a[1]);
                    updated.end = P2(geometry.b[0], geometry.b[1]);
                    updated.offset = geometry.a[2] - self.elevation(run.level);
                    updated.end_offset = Some(geometry.b[2] - self.elevation(run.level));
                    updated.justify = Justify::Centre;
                    updates.push((*id, updated));
                    continue;
                }
                return Err(format!("{id}: mount host is missing"));
            }
            let current = self.run_mount_frame(mount)?;
            if let Some(old) = old.filter(|old| {
                old.mount == run.mount
                    && old.start == run.start
                    && old.end == run.end
                    && old.level == run.level
                    && old.offset == run.offset
                    && old.end_offset == run.end_offset
            }) {
                let previous = before.run_mount_frame(mount)?;
                if previous != current {
                    let mut updated = run.clone();
                    let map = |point: P2, z| {
                        let p = [point.0, point.1, z];
                        let delta = sub(p, previous.0);
                        let old_across = cross(previous.2, previous.1);
                        let new_across = cross(current.2, current.1);
                        add(
                            add(current.0, mul(current.1, crate::view3d::dot(delta, previous.1))),
                            mul(new_across, crate::view3d::dot(delta, old_across)),
                        )
                    };
                    let base = self.elevation(run.level);
                    let old_base = before.elevation(old.level);
                    let a = map(old.start, old_base + old.offset);
                    let b = map(old.end, old_base + old.end_offset.unwrap_or(old.offset));
                    updated.start = P2(a[0], a[1]);
                    updated.end = P2(b[0], b[1]);
                    updated.offset = a[2] - base;
                    updated.end_offset = Some(b[2] - base);
                    updates.push((*id, updated));
                }
            } else {
                // Rigid commands round XY to 0.001 mm independently of the host.
                // Restore only that rounding error to the semantic contact plane.
                let base = self.elevation(run.level);
                let points =
                    [[run.start.0, run.start.1, base + run.offset], [run.end.0, run.end.1, base + run.end_offset.unwrap_or(run.offset)]];
                let distances = points.map(|p| crate::view3d::dot(sub(p, current.0), current.2));
                if distances.iter().all(|d| d.abs() <= 0.002) && distances.iter().any(|d| d.abs() > 1e-8) {
                    let [a, b] = std::array::from_fn::<_, 2, _>(|i| sub(points[i], mul(current.2, distances[i])));
                    let mut updated = run.clone();
                    updated.start = P2(a[0], a[1]);
                    updated.end = P2(b[0], b[1]);
                    updated.offset = a[2] - base;
                    updated.end_offset = Some(b[2] - base);
                    updates.push((*id, updated));
                }
            }
        }
        for (id, updated) in updates {
            if let Some(run) = self.elements.get_mut(id).and_then(crate::model::Element::run_mut) {
                *run = updated;
            }
        }
        Ok(())
    }

    pub(crate) fn check_run_mounts(&self) -> Result<(), String> {
        let mounted: Vec<_> =
            self.elements.iter().filter_map(|(id, e)| e.run().filter(|r| r.mount.is_some()).map(|r| (*id, e, r))).collect();
        if mounted.is_empty() {
            return Ok(());
        }
        for (id, element, run) in &mounted {
            if !self.run_type(element).is_some_and(|t| t.round || element.category() == "cablecarrier")
                || run.carrier.is_some()
                || run.feeder.is_some()
                || run.justify != Justify::Centre
                || !matches!(element.category(), "wire" | "pipe" | "conduit" | "cablecarrier" | "cable")
            {
                return Err(format!("{id}: mount requires an independent round MEP run or cable carrier with centre justification"));
            }
        }
        let scene = crate::view3d::Scene::new(self);
        for (id, element, run) in mounted {
            let Some(mount) = &run.mount else { continue };
            let (origin, _, normal) = self.run_mount_frame(mount)?;
            let base = self.elevation(run.level);
            let a = [run.start.0, run.start.1, base + run.offset];
            let b = [run.end.0, run.end.1, base + run.end_offset.unwrap_or(run.offset)];
            if [a, b].iter().any(|p| crate::view3d::dot(sub(*p, origin), normal).abs() > 1e-5)
                || !scene.supports_segment(self.elements.id(mount.host).ok_or("missing mount host")?, &mount.face, a, b, normal)?
            {
                return Err(format!(
                    "{id}: contact line leaves its host face or crosses an opening; detach or move it onto supported surface"
                ));
            }
            if let Some(g) = self.run_geom(element).filter(|g| !g.round) {
                let (_, across, _) = g.frame();
                let half = mul(across, g.width / 2.0);
                let corners = [sub(a, half), sub(b, half), add(b, half), add(a, half)];
                let hit = crate::view3d::SurfaceHit {
                    host: self.elements.id(mount.host).ok_or("missing mount host")?,
                    face: match mount.face.as_str() {
                        "exterior" => "exterior",
                        "interior" => "interior",
                        "top" => "top",
                        "bottom" => "bottom",
                        _ => return Err("unsupported mount face".into()),
                    },
                    point: a,
                    normal,
                    distance: 0.0,
                };
                if !scene.supports_patch(&hit, corners)? {
                    return Err(format!("{id}: mounting back leaves its host face or overlaps an opening"));
                }
            }
        }
        Ok(())
    }

    /// Unlinked whole wire segments contained in this carrier's actual 3D section and span.
    /// This query never confirms a link, even when there is just one candidate.
    pub fn carrier_wire_candidates(&self, carrier: crate::model::Id) -> Result<Vec<crate::model::Id>, String> {
        use crate::model::Element;
        use crate::view3d::dot;
        let Some(element @ Element::CableCarrier(_)) = self.elements.get(carrier) else {
            return Err("select an existing cable carrier".into());
        };
        let host = self.run_geom(element).ok_or("carrier has no valid physical axis")?;
        let frame = |g: &Geom| g.frame();
        let (axis, x, y) = frame(&host);
        let mut candidates = Vec::new();
        for (id, element) in self.elements.iter() {
            let Element::Wire(run) = element else { continue };
            if run.carrier.is_some() || run.feeder.is_some() {
                continue;
            }
            let Some(wire) = self.run_geom(element) else { continue };
            let (along, cx, cy) = frame(&wire);
            if dot(axis, along).abs() < 1.0 - 1e-8 {
                continue;
            }
            let support = |normal| {
                if wire.round { wire.width / 2.0 } else { (wire.width * dot(cx, normal).abs() + wire.depth * dot(cy, normal).abs()) / 2.0 }
            };
            if [wire.a, wire.b].into_iter().all(|point| {
                let delta = sub(point, host.a);
                let span = dot(delta, axis);
                let (px, py) = (dot(delta, x).abs(), dot(delta, y).abs());
                span >= -TOL
                    && span <= host.length() + TOL
                    && if host.round {
                        // Bounding the corners is conservative for rectangular cables in a round carrier.
                        let radius = if wire.round { px.hypot(py) + wire.width / 2.0 } else { (px + support(x)).hypot(py + support(y)) };
                        radius <= host.width / 2.0 + TOL
                    } else {
                        px + support(x) <= host.width / 2.0 + TOL && py + support(y) <= host.depth / 2.0 + TOL
                    }
            }) {
                candidates.push(*id);
            }
        }
        Ok(candidates)
    }

    pub(crate) fn check_wire_connections(&self) -> Result<(), String> {
        use crate::model::{Element, MAX_COORD, MAX_POINTS};
        let mut owned = Vec::new();
        for (id, element) in self.elements.iter() {
            let Some(run) = element.run() else { continue };
            if run.connections.len() > MAX_POINTS || (!run.connections.is_empty() && (element.category() != "wire" || run.feeder.is_some()))
            {
                return Err(format!("{id}: only a main wire may have at most {MAX_POINTS} connections"));
            }
            let mut ends = Vec::new();
            for connection in &run.connections {
                let key = (connection.device, connection.end);
                if !connection.point.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD) || connection.end > 1 || ends.contains(&key) {
                    return Err(format!("{id}: connection points must be finite and bounded; connect each device/end (0 or 1) once"));
                }
                ends.push(key);
                if connection.landing.is_some_and(|point| !point.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)) {
                    return Err(format!("{id}: the last route landing must be finite and within coordinate limits"));
                }
                if connection
                    .carrier
                    .is_some_and(|guid| !matches!(self.elements.get(guid), Some(Element::CableCarrier(r)) if r.feeder.is_none()))
                {
                    return Err(format!("{id}: a connection route must name an existing main cable carrier"));
                }
                if !matches!(self.elements.get(connection.device), Some(Element::Component(c)) if c.is_placed()) {
                    return Err(format!("{id}: a connection must name placed equipment"));
                }
                if connection.surface.is_some() {
                    let Some(Element::Component(component)) = self.elements.get(connection.device) else {
                        return Err("connection equipment is missing".into());
                    };
                    let main = self.run_geom(element).ok_or("wire has no physical geometry")?;
                    let route = connection.carrier.and_then(|g| self.elements.get(g)).and_then(|e| self.run_geom(e)).unwrap_or(main);
                    self.free_connection(component, connection, &route, &main)?;
                }
                if connection.in_carrier && connection.carrier.or(run.carrier).is_none() {
                    return Err(format!("{id}: select a carrier before connecting equipment in a carrier"));
                }
            }
            if let Some(owner) = &run.feeder {
                let Some(Element::Wire(parent)) = self.elements.get(owner.wire) else {
                    return Err(format!("{id}: its feeder owner must name a main wire"));
                };
                let key = (owner.wire, owner.device, owner.end, owner.leg, element.category());
                if parent.feeder.is_some()
                    || owner.leg > 1
                    || !parent.connections.iter().any(|c| c.device == owner.device && c.end == owner.end)
                    || !matches!(element.category(), "wire" | "cablecarrier")
                    || owned.contains(&key)
                {
                    return Err(format!("{id}: invalid or duplicate physical feeder ownership"));
                }
                owned.push(key);
            }
        }
        Ok(())
    }

    /// Regenerate owned legs in place; surviving roles keep their element ids and GUIDs.
    pub(crate) fn refresh_wire_connections(&mut self, before: &Self) -> Result<(), String> {
        use crate::model::{Element, Feeder};
        // Deleting equipment disconnects its owned legs. An arbitrary missing reference is still an error.
        let removed: Vec<_> =
            before.elements.iter().filter(|(id, _)| self.elements.get(**id).is_none()).map(|(id, _)| before.guid(*id)).collect();
        for (_, element) in self.elements.iter_mut() {
            if let Element::Wire(run) = element {
                run.connections.retain(|c| !removed.contains(&c.device));
            }
        }
        let stale: Vec<_> = self
            .elements
            .iter()
            .filter_map(|(id, element)| {
                let owner = element.run()?.feeder.as_ref()?;
                (removed.contains(&owner.wire) || removed.contains(&owner.device)).then_some(*id)
            })
            .collect();
        for id in stale {
            self.elements.remove(id);
        }
        self.follow_connection_surfaces(before)?;
        self.check_wire_connections()?;
        let parents: Vec<_> = self
            .elements
            .iter()
            .filter_map(|(id, element)| match element {
                Element::Wire(run) if !run.connections.is_empty() => Some((*id, run.clone())),
                _ => None,
            })
            .collect();
        let mut keep = Vec::new();
        for (id, parent) in parents {
            let parent_guid = self.guid(id);
            let element = Element::Wire(parent.clone());
            let main_geometry = self.run_geom(&element).ok_or_else(|| format!("{id}: wire has no physical axis"))?;
            for (index, connection) in parent.connections.iter().enumerate() {
                let route_geometry = connection.carrier.and_then(|guid| self.elements.get(guid)).and_then(|e| self.run_geom(e));
                let geometry = route_geometry.as_ref().unwrap_or(&main_geometry);
                let Some(Element::Component(component)) = self.elements.get(connection.device) else {
                    return Err(format!("{id}: connection equipment is missing"));
                };
                let (device, target, elbow, free_normal) = if connection.surface.is_some() {
                    let solved = self.free_connection(component, connection, geometry, &main_geometry)?;
                    if let Some(Element::Wire(run)) = self.elements.get_mut(id)
                        && let Some(updated) = run.connections.get_mut(index)
                    {
                        updated.point = solved.point;
                        updated.surface = Some(solved.surface);
                        if connection.carrier.is_some() {
                            updated.landing = Some(solved.target);
                        }
                    }
                    (solved.device, solved.target, solved.target, Some(solved.normal))
                } else {
                    let [x, mut y, z] = connection.point;
                    if component.mirrored {
                        y = -y;
                    }
                    let (sin, cos) = component.angle.to_radians().sin_cos();
                    let device = [
                        component.at.0 + x * cos - y * sin,
                        component.at.1 + x * sin + y * cos,
                        self.elevation(component.level) + component.offset + z,
                    ];
                    let dx = geometry.b[0] - geometry.a[0];
                    let dy = geometry.b[1] - geometry.a[1];
                    let flat = dx * dx + dy * dy;
                    let t = if flat > TOL * TOL {
                        (((device[0] - geometry.a[0]) * dx + (device[1] - geometry.a[1]) * dy) / flat).clamp(0.0, 1.0)
                    } else {
                        let dz = geometry.b[2] - geometry.a[2];
                        if dz.abs() > TOL { ((device[2] - geometry.a[2]) / dz).clamp(0.0, 1.0) } else { 0.0 }
                    };
                    let projected = add(geometry.a, mul(sub(geometry.b, geometry.a), t));
                    let target = if connection.carrier.is_some() { projected } else { connection.landing.unwrap_or(projected) };
                    if connection.carrier.is_some()
                        && let Some(Element::Wire(run)) = self.elements.get_mut(id)
                        && let Some(updated) = run.connections.get_mut(index)
                    {
                        updated.landing = Some(target);
                    }
                    (device, target, [device[0], device[1], target[2]], None)
                };
                for (leg, a, b) in [(0, device, elbow), (1, elbow, target)] {
                    if sub(b, a).iter().map(|v| v * v).sum::<f64>().sqrt() <= TOL {
                        continue;
                    }
                    let owner = Feeder { wire: parent_guid, device: connection.device, end: connection.end, leg };
                    let mut wire = parent.clone();
                    // Generated legs use the solved physical axis, not the parent's contact line.
                    wire.mount = None;
                    wire.connections.clear();
                    wire.feeder = Some(owner.clone());
                    wire.start = P2(a[0], a[1]);
                    wire.end = P2(b[0], b[1]);
                    wire.offset = a[2] - self.elevation(parent.level);
                    wire.end_offset = Some(b[2] - self.elevation(parent.level));
                    wire.justify = Justify::Centre;
                    wire.source = None;
                    wire.devices.clear();
                    wire.carrier = None;
                    wire.compartment.clear();
                    wire.local_offset = 0.0;
                    if connection.in_carrier {
                        let Some(Element::CableCarrier(carrier)) = connection.carrier.or(parent.carrier).and_then(|g| self.elements.get(g))
                        else {
                            return Err(format!("{id}: connection requires a cable carrier"));
                        };
                        let mut branch = wire.clone();
                        branch.type_name = carrier.type_name.clone();
                        branch.reserve = 0.0;
                        branch.cable.clear();
                        branch.compartments.clear();
                        branch.size_locked = false;
                        if let Some(normal) = free_normal {
                            let element = Element::CableCarrier(branch.clone());
                            let geom = self.run_geom(&element).ok_or("connection branch type has no geometry")?;
                            let depth = if geom.round { geom.width } else { geom.depth };
                            let shift = mul(normal, depth / 2.0 - main_geometry.width / 2.0);
                            branch.start = P2(a[0] + shift[0], a[1] + shift[1]);
                            branch.end = P2(b[0] + shift[0], b[1] + shift[1]);
                            branch.offset += shift[2];
                            branch.end_offset = Some(b[2] + shift[2] - self.elevation(parent.level));
                            let (_, x, y) = Geom { roll: 0.0, ..geom }.frame();
                            branch.section_roll = (-crate::view3d::dot(normal, x)).atan2(crate::view3d::dot(normal, y)).to_degrees();
                            branch.square_ends = !geom.round;
                            if main_geometry.width > depth + TOL || main_geometry.width > geom.width + TOL {
                                return Err("connection branch is too small for the cable diameter".into());
                            }
                        }
                        let branch_id = self.upsert_feeder(Element::CableCarrier(branch), &owner);
                        keep.push(branch_id);
                        wire.carrier = Some(self.guid(branch_id));
                    }
                    keep.push(self.upsert_feeder(Element::Wire(wire), &owner));
                }
            }
        }
        let stale: Vec<_> = self
            .elements
            .iter()
            .filter_map(|(id, e)| (e.run().is_some_and(|r| r.feeder.is_some()) && !keep.contains(id)).then_some(*id))
            .collect();
        for id in stale {
            self.elements.remove(id);
        }
        Ok(())
    }

    fn upsert_feeder(&mut self, element: crate::model::Element, owner: &crate::model::Feeder) -> crate::model::Id {
        let existing = self.elements.iter().find_map(|(id, e)| {
            (e.category() == element.category() && e.run().and_then(|r| r.feeder.as_ref()) == Some(owner)).then_some(*id)
        });
        if let Some(id) = existing {
            self.elements.insert(id, element);
            id
        } else {
            self.insert(element)
        }
    }
}

/// Sides a round run's tube is drawn with.
pub const SIDES: usize = 24;

/// Whether a run of `category` may stand upright (a riser, a down conductor) rather than run along: not a beam or a
/// strip footing, which are columns when they stand.
pub fn may_stand(category: &str) -> bool {
    !matches!(category, "beam" | "stripfooting")
}

/// How each run category is written in IFC: its class and predefined type (all of them take a product's nine attributes). Import
/// reads the same table back, a class and a type to the first category that has them.
pub const IFC: &[(&str, &str, &str)] = &[
    ("beam", "IFCBEAM", ".BEAM."),
    ("stripfooting", "IFCFOOTING", ".STRIP_FOOTING."),
    ("pipe", "IFCPIPESEGMENT", ".RIGIDSEGMENT."),
    ("cablecarrier", "IFCCABLECARRIERSEGMENT", ".CONDUITSEGMENT."),
    ("cablecarrier", "IFCCABLECARRIERSEGMENT", ".CABLETRAYSEGMENT."),
    // A ladder is a tray to BIMCraft, written as a tray: only a file of another tool's says it.
    ("cablecarrier", "IFCCABLECARRIERSEGMENT", ".CABLELADDERSEGMENT."),
    ("wire", "IFCCABLESEGMENT", ".CABLESEGMENT."),
    // IFC has no lightning protection: a down conductor is a conductor segment, as is an earthing conductor (BIMCraft's own
    // `Category` tells them apart).
    ("downconductor", "IFCCABLESEGMENT", ".CONDUCTORSEGMENT."),
    ("groundconductor", "IFCCABLESEGMENT", ".CONDUCTORSEGMENT."),
    // A data cable is a cable segment, as a wire is (BIMCraft's own `Category` tells them apart).
    ("cable", "IFCCABLESEGMENT", ".CABLESEGMENT."),
];

/// The IFC class and predefined type a run of `category` is written as.
pub fn ifc(category: &str) -> Option<(&'static str, &'static str)> {
    IFC.iter().find(|(c, ..)| *c == category).map(|(_, class, kind)| (*class, *kind))
}

/// The IFC class and predefined type a run of `category` is written as for its shape: a cable carrier's
/// round section writes as a conduit segment, a rectangular one as a cable tray segment.
pub fn ifc_shape(category: &str, round: bool) -> Option<(&'static str, &'static str)> {
    if category != "cablecarrier" {
        return ifc(category);
    }
    let kind = if round { ".CONDUITSEGMENT." } else { ".CABLETRAYSEGMENT." };
    IFC.iter().find(|(c, _, t)| *c == category && *t == kind).map(|(_, class, kind)| (*class, *kind))
}

/// The category of a run an IFC product is, from its `class`, its `kind` (the predefined type) and, when a file of BIMCraft's says
/// so, the `declared` category: that one if the class has it, else the one whose type it is, else the class's first.
pub fn from_ifc(class: &str, kind: Option<&str>, declared: Option<&str>) -> Option<&'static str> {
    let class = if class == "IFCBEAMSTANDARDCASE" { "IFCBEAM" } else { class };
    let rows = || IFC.iter().filter(move |(_, c, _)| *c == class);
    let by_kind = |k: &str| rows().find(|(_, _, t)| t.trim_matches('.') == k);
    let by_name = |d: &str| rows().find(|(c, ..)| *c == d);
    declared.and_then(by_name).or_else(|| kind.and_then(by_kind)).or_else(|| rows().next()).map(|(c, ..)| *c)
}

/// The line of its section a run of `category` has its heights at unless told: the top of a beam or a footing, as engineers
/// think of them (top of beam at the top of slab), the middle of anything that carries something (a pipe, a cable).
pub fn default_justify(category: &str) -> Justify {
    if may_stand(category) { Justify::Centre } else { Justify::Top }
}

/// Why `g`, a run of `category`, isn't one the model takes, if it isn't.
pub fn check(who: &str, category: &str, g: &Geom) -> Result<(), String> {
    if g.length() <= TOL {
        return Err(format!("{who}: too short ({} mm)", g.length()));
    }
    if !may_stand(category) {
        if g.vertical() {
            return Err(format!("{who}: its ends are one over the other; a {category} runs along, and what stands is a column"));
        }
        let tilt = (g.b[2] - g.a[2]).abs().atan2(g.plan_length()).to_degrees();
        if tilt > MAX_SLOPE {
            return Err(format!("{who}: it slopes {tilt:.1}°, more than the {MAX_SLOPE}° a {category} may; what stands is a column"));
        }
    }
    Ok(())
}

/// A run worked out: its centre line from `a` to `b` (mm: x east, y north, z up), and its section, `width` across by `depth`
/// deep, square to that line; round, `width` is the diameter and `depth` is as much.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geom {
    pub square_ends: bool,
    pub roll: f64,
    pub a: P3,
    pub b: P3,
    pub width: f64,
    pub depth: f64,
    pub round: bool,
}

impl Geom {
    /// `r` of type `t`: its heights are those of its `justify` line, so its centre line is half the section's depth (square
    /// to the line) under it for a top, over it for a bottom.
    pub fn of(m: &Model, r: &Run, t: &RunType) -> Geom {
        let base = m.elevation(r.level);
        let (z0, z1) = (base + r.offset, base + r.end_offset.unwrap_or(r.offset));
        let depth = if t.round { t.width } else { t.depth };
        let g = Geom {
            square_ends: r.square_ends,
            roll: r.section_roll,
            a: [r.start.0, r.start.1, z0],
            b: [r.end.0, r.end.1, z1],
            width: t.width,
            depth,
            round: t.round,
        };
        if let Some(mount) = &r.mount
            && let Ok((_, _, normal)) = m.run_mount_frame(mount)
        {
            let (_, x, y) = Geom { roll: 0.0, ..g }.frame();
            let roll =
                if t.round { r.section_roll } else { (-crate::view3d::dot(normal, x)).atan2(crate::view3d::dot(normal, y)).to_degrees() };
            let shift = mul(normal, depth / 2.0);
            return Geom { a: add(g.a, shift), b: add(g.b, shift), roll, square_ends: !t.round || g.square_ends, ..g };
        }
        // Upright, it has no top or bottom to be the line.
        let shift = match r.justify {
            Justify::Top => -g.half(),
            Justify::Centre => 0.0,
            Justify::Bottom => g.half(),
        };
        Geom { a: [g.a[0], g.a[1], z0 + shift], b: [g.b[0], g.b[1], z1 + shift], ..g }
    }

    /// Along the centre line: its true length.
    pub fn length(&self) -> f64 {
        self.plan_length().hypot(self.b[2] - self.a[2])
    }

    /// Physical longitudinal, width and depth axes shared by drawing, export and containment.
    pub fn frame(&self) -> (P3, P3, P3) {
        let axis = unit(sub(self.b, self.a));
        let x = if self.vertical() { [1.0, 0.0, 0.0] } else { unit([-axis[1], axis[0], 0.0]) };
        let y = cross(axis, x);
        let (sin, cos) = self.roll.to_radians().sin_cos();
        (axis, add(mul(x, cos), mul(y, sin)), sub(mul(y, cos), mul(x, sin)))
    }

    fn box_rings(&self) -> [[P3; 4]; 2] {
        let (_, x, y) = self.frame();
        [self.a, self.b].map(|at| {
            [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)].map(|(w, d)| add(at, add(mul(x, w * self.width), mul(y, d * self.depth))))
        })
    }

    pub fn box_triangles(&self) -> Vec<[P3; 3]> {
        let [a, b] = self.box_rings();
        let mut tris = vec![[a[0], a[2], a[1]], [a[0], a[3], a[2]], [b[0], b[1], b[2]], [b[0], b[2], b[3]]];
        for i in 0..4 {
            let j = (i + 1) % 4;
            tris.extend([[a[i], a[j], b[j]], [a[i], b[j], b[i]]]);
        }
        tris
    }

    /// Its length in plan.
    pub fn plan_length(&self) -> f64 {
        P2(self.b[0] - self.a[0], self.b[1] - self.a[1]).dist(P2(0.0, 0.0))
    }

    /// Whether it stands upright: its ends one over the other in plan.
    pub fn vertical(&self) -> bool {
        self.plan_length() <= TOL
    }

    /// Half the section's depth, taken straight up: more than half of it, square to the line, where the line slopes.
    pub fn half(&self) -> f64 {
        if !self.round && (self.square_ends || self.roll.abs() > 1e-9) {
            let (_, x, y) = self.frame();
            return (x[2].abs() * self.width + y[2].abs() * self.depth) / 2.0;
        }
        let plan = self.plan_length();
        if plan > TOL { self.depth / 2.0 * self.length() / plan } else { 0.0 }
    }

    /// Along it in plan: east when upright.
    fn along(&self) -> P2 {
        if self.vertical() { P2(1.0, 0.0) } else { P2(self.b[0] - self.a[0], self.b[1] - self.a[1]).unit() }
    }

    /// Its footprint in plan, counter-clockwise: a strip `width` wide from end to end, or, upright, its section, `width`
    /// along east and `depth` along north, or round.
    pub fn footprint(&self) -> Vec<P2> {
        if !self.round && (self.square_ends || self.roll.abs() > 1e-9) {
            return crate::geom::hull(self.box_rings().into_iter().flatten().map(|p| P2(p[0], p[1])).collect());
        }
        let (a, b) = (P2(self.a[0], self.a[1]), P2(self.b[0], self.b[1]));
        if !self.vertical() {
            let n = self.along().perp() * (self.width / 2.0);
            return vec![a - n, b - n, b + n, a + n];
        }
        if self.round {
            return (0..SIDES).map(|i| a + heading(360.0 * i as f64 / SIDES as f64) * (self.width / 2.0)).collect();
        }
        let (w, d) = (self.width / 2.0, self.depth / 2.0);
        [(-w, -d), (w, -d), (w, d), (-w, d)].into_iter().map(|(x, y)| a + P2(x, y)).collect()
    }

    /// The lowest and the highest it reaches.
    pub fn z_span(&self) -> (f64, f64) {
        if !self.round && (self.square_ends || self.roll.abs() > 1e-9) {
            let (_, x, y) = self.frame();
            let h = (x[2].abs() * self.width + y[2].abs() * self.depth) / 2.0;
            return (self.a[2].min(self.b[2]) - h, self.a[2].max(self.b[2]) + h);
        }
        let h = self.half();
        (self.a[2].min(self.b[2]) - h, self.a[2].max(self.b[2]) + h)
    }

    /// As a prism: its footprint between a bottom plane and a top one, a section a rectangle (round, the same square to it).
    pub fn prism(&self) -> (Vec<P2>, Plane, Plane) {
        let ring = self.footprint();
        if self.vertical() {
            let (lo, hi) = self.z_span();
            return (ring, Plane::flat(lo), Plane::flat(hi));
        }
        let a = P2(self.a[0], self.a[1]);
        let g = self.along() * ((self.b[2] - self.a[2]) / self.plan_length());
        let z = self.a[2] - g.dot(a);
        (ring, Plane { z: z - self.half(), g }, Plane { z: z + self.half(), g })
    }

    /// A round run as a tube of triangles turned out: its two caps' first, and how many they are.
    pub fn tube(&self) -> (Vec<[P3; 3]>, usize) {
        let (d, r) = (unit(sub(self.b, self.a)), self.width / 2.0);
        let e1 = if self.vertical() { [1.0, 0.0, 0.0] } else { unit([-d[1], d[0], 0.0]) };
        let e2 = cross(d, e1);
        let ring = |c: P3| -> Vec<P3> {
            (0..SIDES)
                .map(|k| {
                    let t = std::f64::consts::TAU * k as f64 / SIDES as f64;
                    add(c, add(mul(e1, r * t.cos()), mul(e2, r * t.sin())))
                })
                .collect()
        };
        let (ra, rb) = (ring(self.a), ring(self.b));
        let next = |k: usize| (k + 1) % SIDES;
        let mut tris = Vec::with_capacity(4 * SIDES);
        for k in 0..SIDES {
            tris.push([self.a, ra[next(k)], ra[k]]);
            tris.push([self.b, rb[k], rb[next(k)]]);
        }
        for k in 0..SIDES {
            tris.push([ra[k], ra[next(k)], rb[k]]);
            tris.push([ra[next(k)], rb[next(k)], rb[k]]);
        }
        (tris, 2 * SIDES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> (Model, crate::model::Guid) {
        let m = Model::new("t");
        let level = m.guid(m.levels()[0].0);
        (m, level)
    }

    fn run(level: crate::model::Guid, a: P2, b: P2, offset: f64, end_offset: Option<f64>, justify: Justify) -> Run {
        Run {
            square_ends: false,
            section_roll: 0.0,
            mount: None,
            connections: Vec::new(),
            feeder: None,
            type_name: "t".into(),
            level,
            start: a,
            end: b,
            offset,
            end_offset,
            justify,
            system: String::new(),
            source: None,
            devices: Vec::new(),
            cable: String::new(),
            reserve: 0.0,
            along_walls: false,
            carrier: None,
            local_offset: 0.0,
            size_locked: false,
            compartments: Default::default(),
            compartment: String::new(),
            params: Default::default(),
        }
    }

    fn rect(width: f64, depth: f64) -> RunType {
        RunType { width, depth, round: false, material: String::new() }
    }

    fn volume(tris: &[[P3; 3]]) -> f64 {
        // The divergence theorem: the sum of the signed volumes of the tetrahedra to the origin; positive if turned out.
        tris.iter().map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c)) / 6.0).sum()
    }

    #[test]
    fn a_runs_heights_are_those_of_its_justify_line() {
        let (m, level) = model();
        let t = rect(200.0, 500.0);
        let at = |j| Geom::of(&m, &run(level, P2(0.0, 0.0), P2(6000.0, 0.0), 2800.0, None, j), &t);
        // The top of the section is at 2800 for a top, the bottom for a bottom, the middle for a centre.
        assert_eq!(at(Justify::Top).z_span(), (2300.0, 2800.0));
        assert_eq!(at(Justify::Bottom).z_span(), (2800.0, 3300.0));
        assert_eq!(at(Justify::Centre).z_span(), (2550.0, 3050.0));
        assert_eq!((at(Justify::Top).a, at(Justify::Top).b), ([0.0, 0.0, 2550.0], [6000.0, 0.0, 2550.0]));
        assert_eq!(at(Justify::Top).length(), 6000.0);
    }

    #[test]
    fn a_sloped_run_keeps_its_section_square_to_its_axis() {
        let (m, level) = model();
        // 3000 along, 4000 up the ends' difference ... a 3-4-5: 3000 in plan, 4000 up, 5000 along.
        let g = Geom::of(&m, &run(level, P2(0.0, 0.0), P2(3000.0, 0.0), 0.0, Some(4000.0), Justify::Centre), &rect(100.0, 200.0));
        assert_eq!(g.length(), 5000.0);
        // Vertically its section is 200 / cos θ = 200 / 0.6 deep, so the prism's thickness is that, on each side of the line.
        let (ring, bottom, top) = g.prism();
        let q = ring[0];
        assert!((top.at(q) - bottom.at(q) - 200.0 / 0.6).abs() < 1e-9, "{} {}", top.at(q), bottom.at(q));
        // Its volume is the section's area times its length, whatever its slope (its ends are cut upright).
        let plan = crate::geom::area(&ring);
        let v = plan * (top.at(P2(1500.0, 0.0)) - bottom.at(P2(1500.0, 0.0)));
        assert!((v - 100.0 * 200.0 * 5000.0).abs() < 1.0, "{v}");
        assert!(plan > 0.0, "counter-clockwise");
        // The planes are over the line wherever it starts: 500 up at its start, 4500 at its end, 200 / 0.6 / 2 each way.
        let g =
            Geom::of(&m, &run(level, P2(1000.0, 2000.0), P2(4000.0, 2000.0), 500.0, Some(4500.0), Justify::Centre), &rect(100.0, 200.0));
        let (_, bottom, top) = g.prism();
        let half = 100.0 / 0.6;
        assert!(
            (bottom.at(P2(1000.0, 2000.0)) - (500.0 - half)).abs() < 1e-9 && (top.at(P2(4000.0, 2000.0)) - (4500.0 + half)).abs() < 1e-9
        );
    }

    #[test]
    fn a_run_a_millimetre_long_in_plan_stands_and_a_shorter_one_is_too_short() {
        let (m, level) = model();
        let at = |to: P2, end_offset| Geom::of(&m, &run(level, P2(0.0, 0.0), to, 0.0, end_offset, Justify::Centre), &rect(100.0, 200.0));
        assert!(at(P2(TOL, 0.0), Some(500.0)).vertical() && !at(P2(TOL + 0.01, 0.0), Some(500.0)).vertical());
        assert!(check("a beam 1", "beam", &at(P2(TOL, 0.0), None)).unwrap_err().contains("too short"), "a millimetre is too short");
        assert!(check("a beam 2", "beam", &at(P2(0.5, 0.0), None)).unwrap_err().contains("too short"));
        assert!(check("a beam 3", "beam", &at(P2(2.0, 0.0), None)).is_ok());
    }

    #[test]
    fn a_footprint_is_a_strip_or_when_upright_its_section() {
        let (m, level) = model();
        let strip = Geom::of(&m, &run(level, P2(0.0, 0.0), P2(4000.0, 0.0), 0.0, None, Justify::Top), &rect(300.0, 500.0)).footprint();
        assert_eq!(strip, vec![P2(0.0, -150.0), P2(4000.0, -150.0), P2(4000.0, 150.0), P2(0.0, 150.0)]);
        // Upright: its section, width along east and depth along north, round a point; its heights from end to end.
        let up = Geom::of(&m, &run(level, P2(1000.0, 1000.0), P2(1000.0, 1000.0), 0.0, Some(3000.0), Justify::Centre), &rect(300.0, 500.0));
        assert!(up.vertical());
        assert_eq!(up.length(), 3000.0);
        assert_eq!(up.footprint(), vec![P2(850.0, 750.0), P2(1150.0, 750.0), P2(1150.0, 1250.0), P2(850.0, 1250.0)]);
        assert_eq!(up.z_span(), (0.0, 3000.0));
        let (ring, bottom, top) = up.prism();
        assert_eq!((ring.len(), bottom.at(ring[0]), top.at(ring[0])), (4, 0.0, 3000.0));
    }

    #[test]
    fn a_round_run_is_a_closed_tube_turned_out() {
        let (m, level) = model();
        let round = RunType { width: 200.0, depth: 200.0, round: true, material: String::new() };
        // From a point off the origin, where a cap turned the wrong way doesn't cancel in the volume.
        let from = P2(1000.0, -2000.0);
        for (by, end_offset) in
            [(P2(5000.0, 0.0), None), (P2(3000.0, 4000.0), Some(1000.0)), (P2(0.0, 0.0), Some(2000.0)), (P2(-3000.0, 1000.0), Some(-700.0))]
        {
            let g = Geom::of(&m, &run(level, from, from + by, 500.0, end_offset, Justify::Centre), &round);
            let (tris, caps) = g.tube();
            assert_eq!((tris.len(), caps), (4 * SIDES, 2 * SIDES));
            // The area of a regular polygon of SIDES sides in a circle of radius 100.
            let area = SIDES as f64 / 2.0 * 100.0 * 100.0 * (std::f64::consts::TAU / SIDES as f64).sin();
            assert!((volume(&tris) - area * g.length()).abs() < 1e-3 * area * g.length(), "{} {}", volume(&tris), area * g.length());
        }
    }

    #[test]
    fn a_runs_reach_covers_its_section_square_to_its_axis() {
        let (m, level) = model();
        let g = Geom::of(&m, &run(level, P2(0.0, 0.0), P2(3000.0, 0.0), 0.0, Some(4000.0), Justify::Centre), &rect(100.0, 200.0));
        let (lo, hi) = g.z_span();
        assert!((lo - -100.0 / 0.6).abs() < 1e-9 && (hi - (4000.0 + 100.0 / 0.6)).abs() < 1e-9, "{lo} {hi}");
    }
}
