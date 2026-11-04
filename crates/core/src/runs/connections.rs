//! Free equipment contacts: semantic wall plane, allowed envelope sides and physical leads.

use super::{Geom, device_exit};
use crate::geom::P2;
use crate::model::{Component, ConnectionSurface, DeviceSide, Element, MAX_COORD, Model, RunMount, TOL, WireConnection};
use crate::view3d::{P3, add, cross, dot, mul, sub};

// Commands round XY to 0.001 mm; retain that precision without inventing gaps.
const CONTACT_TOL: f64 = 0.002;

pub(super) struct Solved {
    pub device: P3,
    pub elbow: P3,
    pub target: P3,
    pub normal: P3,
    pub point: P3,
    pub surface: ConnectionSurface,
}

fn reference(model: &Model, c: &Component) -> P3 {
    [c.at.0, c.at.1, model.elevation(c.level) + c.offset]
}

fn rotate(c: &Component, mut p: P3) -> P3 {
    if c.mirrored {
        p[1] = -p[1];
    }
    let (sin, cos) = c.angle.to_radians().sin_cos();
    [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos, p[2]]
}

fn unrotate(c: &Component, p: P3) -> P3 {
    let (sin, cos) = c.angle.to_radians().sin_cos();
    [p[0] * cos + p[1] * sin, (p[1] * cos - p[0] * sin) * if c.mirrored { -1.0 } else { 1.0 }, p[2]]
}

impl Model {
    pub(super) fn free_connection(&self, c: &Component, connection: &WireConnection, route: &Geom, wire: &Geom) -> Result<Solved, String> {
        let surface = connection.surface.as_ref().ok_or("free connection requires a surface")?;
        if !wire.round || wire.width <= 0.0 || !wire.width.is_finite() {
            return Err("free connection requires a round cable with a positive outside diameter".into());
        }
        if !surface.origin.iter().chain(surface.normal.iter()).all(|v| v.is_finite() && v.abs() <= MAX_COORD)
            || (dot(surface.normal, surface.normal) - 1.0).abs() > 1e-8
            || !matches!(surface.face.as_str(), "interior" | "exterior")
        {
            return Err("free connection requires a finite local plane with a unit normal and an interior/exterior face".into());
        }
        let at = reference(self, c);
        let (origin, normal) = if let Some(host) = surface.host {
            if !matches!(self.elements.get(host), Some(Element::Wall(_))) {
                return Err("free equipment connections require an existing straight wall".into());
            }
            let (origin, _, normal) = self.run_mount_frame(&RunMount { host, face: surface.face.clone() })?;
            (origin, normal)
        } else {
            (add(at, rotate(c, surface.origin)), rotate(c, surface.normal))
        };
        let local_right = rotate(c, [1.0, 0.0, 0.0]);
        let up = [0.0, 0.0, 1.0];
        if normal[2].abs() > 1e-8 || dot(local_right, normal).abs() > 1e-6 {
            return Err("rotate the equipment so its local left/right sides lie in the mounting wall plane".into());
        }
        // Use the actual host tangent: independent XY rounding can make the
        // equipment yaw and the wall direction differ by a fraction of a micron.
        let tangent = cross(up, normal);
        let right = mul(tangent, if dot(tangent, local_right) < 0.0 { -1.0 } else { 1.0 });
        let base = sub(at, mul(normal, dot(sub(at, origin), normal)));
        let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        let mut nearest = f64::INFINITY;
        for piece in self.component_local_pieces(c) {
            bounds[0][1] = bounds[0][1].min(piece.bottom);
            bounds[1][1] = bounds[1][1].max(piece.top);
            // Local pieces already include the component's Y reflection.
            let (sin, cos) = c.angle.to_radians().sin_cos();
            for p in piece.outline {
                bounds[0][0] = bounds[0][0].min(p.0);
                bounds[1][0] = bounds[1][0].max(p.0);
                let world = add(at, [p.0 * cos - p.1 * sin, p.0 * sin + p.1 * cos, 0.0]);
                nearest = nearest.min(dot(sub(world, origin), normal));
            }
        }
        if nearest.abs() > CONTACT_TOL {
            return Err("place the equipment back against the mounting wall without penetrating it".into());
        }
        let frozen = connection.carrier.is_none() && connection.landing.is_some();
        if !frozen && dot(sub(route.b, route.a), normal).abs() > CONTACT_TOL {
            return Err("the connection route must run parallel to the equipment's mounting wall".into());
        }
        let project = |p| {
            let d = sub(p, base);
            [dot(d, right), dot(d, up)]
        };
        let route_points = if connection.carrier.is_none() {
            if let Some(landing) = connection.landing { [project(landing); 2] } else { [project(route.a), project(route.b)] }
        } else {
            [project(route.a), project(route.b)]
        };
        let mut allowed = [c.connection_sides.is_empty(); 4];
        for side in &c.connection_sides {
            allowed[match side {
                DeviceSide::Bottom => 0,
                DeviceSide::Top => 1,
                DeviceSide::Right => 2,
                DeviceSide::Left => 3,
            }] = true;
        }
        let exit = device_exit::solve(bounds, route_points, allowed)?;
        let world = |p: [f64; 2]| add(base, add(mul(right, p[0]), mul(up, p[1])));
        let contact = world(exit.contact);
        let landing = world(exit.landing);
        // Exit outwards first, so the turn cannot send the lead through the device.
        let corner = if exit.side < 2 { [exit.contact[0], exit.landing[1]] } else { [exit.landing[0], exit.contact[1]] };
        let elbow = world(corner);
        let radius = wire.width / 2.0;
        let device = add(contact, mul(normal, radius));
        let target = add(landing, mul(normal, radius));
        if let Some(minimum) = connection.min_bend_radius {
            if !minimum.is_finite() || minimum <= 0.0 || minimum > MAX_COORD {
                return Err("invalid minimum cable bend radius".into());
            }
            let first = sub(elbow, contact);
            let second = sub(landing, elbow);
            let first_length = dot(first, first).sqrt();
            let second_length = dot(second, second).sqrt();
            let has_turn = first_length > TOL && second_length > TOL;
            if has_turn && !bend_fits(minimum, first_length, second_length, 0.0) {
                return Err(format!("the vertical transition does not have room for a {minimum} mm cable bend"));
            }
            if !frozen {
                let incoming = if second_length > TOL { second } else { first };
                let length = dot(incoming, incoming).sqrt();
                let fits = length <= TOL
                    || [route.a, route.b].into_iter().any(|endpoint| {
                        let outgoing = sub(world(project(endpoint)), landing);
                        let available = dot(outgoing, outgoing).sqrt();
                        let remaining = if has_turn { length - minimum } else { length };
                        available > TOL && bend_fits(minimum, remaining, available, dot(incoming, outgoing) / (length * available))
                    });
                if !fits {
                    return Err(format!("the lead and route do not have room for a {minimum} mm cable bend"));
                }
            }
        }
        if connection.carrier.is_some() {
            let (_, x, y) = route.frame();
            let d = sub(target, route.a);
            let (px, py) = (dot(d, x).abs(), dot(d, y).abs());
            let fits = if route.round {
                px.hypot(py) + radius <= route.width / 2.0 + CONTACT_TOL
            } else {
                px + radius <= route.width / 2.0 + CONTACT_TOL && py + radius <= route.depth / 2.0 + CONTACT_TOL
            };
            if !fits {
                return Err(
                    "the wall-tangent cable does not fit the selected carrier section; mount the carrier against the same wall".into()
                );
            }
        } else if (dot(sub(connection.landing.unwrap_or(route.a), origin), normal) - radius).abs() > CONTACT_TOL {
            return Err("mount the main cable tangent to the wall or select a carrier for this connection".into());
        }
        if let Some(host) = surface.host {
            let host = self.elements.id(host).ok_or("mounting wall is missing")?;
            let scene = crate::view3d::Scene::new(self);
            for (a, b) in [(contact, elbow), (elbow, landing)] {
                if dot(sub(b, a), sub(b, a)) > TOL * TOL && !scene.supports_segment(host, &surface.face, a, b, normal)? {
                    return Err("the equipment lead crosses an opening or leaves the mounting wall face".into());
                }
            }
        }
        Ok(Solved {
            device,
            elbow: add(elbow, mul(normal, radius)),
            target,
            normal,
            point: unrotate(c, sub(contact, at)),
            surface: ConnectionSurface {
                host: surface.host,
                face: surface.face.clone(),
                origin: unrotate(c, sub(base, at)),
                normal: unrotate(c, normal),
            },
        })
    }

    pub(super) fn follow_connection_surfaces(&mut self, before: &Self) -> Result<(), String> {
        let mut mounts = std::collections::BTreeMap::new();
        for element in self.elements.values() {
            let Some(run) = element.run() else { continue };
            for connection in &run.connections {
                let Some(surface) = &connection.surface else { continue };
                let Some(host) = surface.host else { continue };
                let key = (host, surface.face.clone());
                if mounts.insert(connection.device, key.clone()).is_some_and(|previous| previous != key) {
                    return Err("all free connections of one equipment must use the same mounting wall face".into());
                }
            }
        }
        for (device, (host, face)) in mounts {
            if self.elements.get(host).is_none() && before.elements.get(host).is_some() {
                for element in self.elements.values_mut() {
                    if let Some(run) = element.run_mut() {
                        for connection in &mut run.connections {
                            if connection.device == device
                                && let Some(surface) = &mut connection.surface
                            {
                                surface.host = None;
                            }
                        }
                    }
                }
                continue;
            }
            let mount = RunMount { host, face };
            let Some(Element::Component(old)) = before.elements.get(device) else { continue };
            let Some(Element::Component(current)) = self.elements.get(device) else { continue };
            if current != old || before.elements.get(host).is_none() {
                continue;
            }
            let previous = before.run_mount_frame(&mount)?;
            let next = self.run_mount_frame(&mount)?;
            if previous == next {
                continue;
            }
            let d = sub(reference(before, old), previous.0);
            let old_up = cross(previous.2, previous.1);
            let new_up = cross(next.2, next.1);
            let at = add(next.0, add(mul(next.1, dot(d, previous.1)), add(mul(new_up, dot(d, old_up)), mul(next.2, dot(d, previous.2)))));
            let turn = next.1[1].atan2(next.1[0]).to_degrees() - previous.1[1].atan2(previous.1[0]).to_degrees();
            let base = self.elevation(current.level);
            if let Some(Element::Component(c)) = self.elements.get_mut(device) {
                c.at = P2(at[0], at[1]);
                c.offset = at[2] - base;
                c.angle += turn;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn right_angle_bend_needs_radius_on_both_legs() {
    assert!(bend_fits(100.0, 100.0, 100.0, 0.0));
    assert!(!bend_fits(100.0, 99.0, 150.0, 0.0));
    assert!(!bend_fits(100.0, 150.0, 99.0, 0.0));
    assert!(bend_fits(100.0, 1.0, 1.0, 1.0));
    assert!(!bend_fits(100.0, 1000.0, 1000.0, -1.0));
}

// For unit directions, tan(turn / 2) = sqrt((1 - cos(turn)) / (1 + cos(turn))).
pub(crate) fn bend_fits(radius: f64, incoming: f64, outgoing: f64, cosine: f64) -> bool {
    let cosine = cosine.clamp(-1.0, 1.0);
    let trim = radius * ((1.0 - cosine) / (1.0 + cosine)).sqrt();
    trim.is_finite() && trim <= incoming.min(outgoing) + 1e-8
}
