//! The work plane commands (T4, T13): `reference_plane`, a reference plane of the project; `edit_work_plane`, a family's
//! form or reference line, or a project's components, put on another plane; and the `host` a component is placed on.

use super::*;
use crate::family::{Expr, Frame, PlaneRef};
use crate::model::{FaceRole, HostRef, Hosted, ReferencePlane};
use crate::view3d::{P3, dot, sub};

/// `[x, y]` (at `z`) or `[x, y, z]` under `key`, mm within [`MAX_COORD`]: which of the two it was given as, and the point.
fn point2or3(c: &str, p: &Value, key: &str, z: f64) -> Result<Option<(bool, P3)>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => match (point(v), point3(v)) {
            (Some(q), _) => Ok(Some((false, [q.0, q.1, z]))),
            (_, Some(q)) => Ok(Some((true, q))),
            _ => Err(bad(c, format!("`{key}` must be a point [x, y] or [x, y, z] in mm, got {v}"))),
        },
        None => Ok(None),
    }
}

/// A reference plane of the project, drawn as Revit's Reference Plane tool draws one: along a line, upright through a line in
/// plan, or at any angle with `up`.
pub(super) fn reference_plane(s: &mut Session, p: &Value) -> R {
    let c = "reference_plane";
    only(c, p, &["start", "end", "up", "name", "level"])?;
    let m = &s.model;
    let z = m.elevation(level_param(c, m, p, "level")?);
    let start = point2or3(c, p, "start", z)?.ok_or_else(|| bad(c, "`start` [x, y] or [x, y, z] is required"))?.1;
    let end = point2or3(c, p, "end", z)?.ok_or_else(|| bad(c, "`end` [x, y] or [x, y, z] is required"))?.1;
    let up = match p.get("up") {
        v if absent(v) => [0.0, 0.0, 1.0],
        Some(v) => point3(v).ok_or_else(|| bad(c, format!("`up` is a direction [x, y, z], got {v}")))?,
        None => [0.0, 0.0, 1.0],
    };
    let name = string(c, p, "name")?.unwrap_or_default();
    let id = s.model_mut().insert(Element::RefPlane(ReferencePlane { name: name.clone(), start, end, up }));
    Ok(json!({"id": id, "name": name}))
}

/// What a command's `host` names: a level, a reference plane of the project (by name, id or GUID), or a face of a wall, a
/// floor, a ceiling or a roof.
pub(super) enum Given {
    Level(Guid),
    Plane(Guid),
    Face(Guid, FaceRole),
}

/// `host`, `{"level": …}` or `{"plane": …}`, if given.
pub(super) fn host_param(c: &str, m: &Model, p: &Value) -> Result<Option<Given>, String> {
    let v = match p.get("host") {
        v if absent(v) => return Ok(None),
        Some(v) => v,
        None => return Ok(None),
    };
    let wrong = || {
        bad(
            c,
            format!(
                "`host` is {{\"level\": name | id}}, {{\"plane\": name | id | GUID}} (a reference plane) or {{\"face\": {{\"element\": \
                 id | GUID, \"face\": exterior | interior | top | bottom}}}} (a face of a wall, a floor, a ceiling or a roof), not {v}"
            ),
        )
    };
    let Some(o) = v.as_object().filter(|o| o.len() == 1) else { return Err(wrong()) };
    if o.contains_key("level") {
        return Ok(Some(Given::Level(level_param(c, m, v, "level")?)));
    }
    if let Some(f) = o.get("face") {
        let Some(f) = f.as_object().filter(|f| f.len() == 2 && f.contains_key("element") && f.contains_key("face")) else {
            return Err(wrong());
        };
        let id = element_id(c, m, &f["element"])?;
        let face: FaceRole = serde_json::from_value(f["face"].clone())
            .map_err(|_| bad(c, format!("`host` `face` is exterior, interior, top or bottom, not {}", f["face"])))?;
        let e = m.elements.get(id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        if !matches!(e.category(), "wall" | "floor" | "ceiling" | "roof") {
            return Err(bad(
                c,
                format!(
                    "`host`: {} {id} has no faces to host on: a component goes on a face of a wall, a floor, a ceiling or a roof",
                    e.category()
                ),
            ));
        }
        if !face.of(e.category()) {
            let has = if e.category() == "wall" { "exterior and interior" } else { "top and bottom" };
            return Err(bad(c, format!("`host`: {} {id} has no face `{}`; its faces are {has}", e.category(), face.name())));
        }
        return Ok(Some(Given::Face(m.guid(id), face)));
    }
    let Some(plane) = o.get("plane") else { return Err(wrong()) };
    let id = match plane {
        Value::String(n) if Guid::parse(n).is_none() => m
            .rare("refplane")
            .into_iter()
            .find(|id| matches!(m.elements.get(*id), Some(Element::RefPlane(rp)) if rp.name == *n))
            .ok_or_else(|| bad(c, format!("no reference plane named `{n}`")))?,
        v => element_id(c, m, v)?,
    };
    match m.elements.get(id) {
        Some(Element::RefPlane(_)) => Ok(Some(Given::Plane(m.guid(id)))),
        Some(e) => Err(bad(c, format!("`host`: {} {id} isn't a reference plane", e.category()))),
        None => Err(bad(c, format!("no element {id}"))),
    }
}

/// The level the point at height `z` stands on: the highest at or below it, else the lowest.
fn level_under(m: &Model, z: f64) -> Option<Guid> {
    let levels = m.levels();
    let (id, _) = levels.iter().rev().find(|(_, l)| l.elevation <= z + TOL).or(levels.first())?;
    Some(m.guid(*id))
}

/// A component's place on `plane` from a point given: `[x, y]` taken straight down or up onto it (refused for an upright
/// plane), `[x, y, z]` along its normal.
fn on_plane(c: &str, frame: &Frame, given: (bool, P3)) -> Result<[f64; 2], String> {
    let (spatial, w) = given;
    if spatial {
        let d = sub(w, frame.origin);
        return Ok([dot(d, frame.x), dot(d, frame.y)]);
    }
    if frame.z[2].abs() < 1e-9 {
        return Err(bad(c, "the reference plane is upright: give `at` as a point in space, [x, y, z], to put on it"));
    }
    Ok(Model::hosted_uv(frame, w, 0.0))
}

/// A component's place on face `face` of `element` from a point given: on a wall's side `[x, y, z]`, its nearest point
/// there (a plan point alone has no height on it); on a floor's, a ceiling's or a roof's, the point in plan.
fn on_face(c: &str, m: &Model, element: Guid, face: FaceRole, given: (bool, P3)) -> Result<[f64; 2], String> {
    let (spatial, w) = given;
    if !spatial && matches!(face, FaceRole::Exterior | FaceRole::Interior) {
        return Err(bad(c, "a wall's side is upright: give `at` as a point in space, [x, y, z], to put on it"));
    }
    m.face_uv(element, face, w).ok_or_else(|| bad(c, "`host`: no such face"))
}

/// The `component` command: a family's type placed on a level, or on a reference plane (`host`).
pub(super) fn component(s: &mut Session, p: &Value) -> R {
    let c = "component";
    only(c, p, &["family", "type", "at", "level", "angle", "offset", "mirrored", "params", "adaptive", "host"])?;
    let m = &s.model;
    let family = string(c, p, "family")?.ok_or_else(|| bad(c, "`family` is required; `inspect` lists the families"))?;
    let f = m.families.get(&family).ok_or_else(|| {
        let names: Vec<&str> = m.families.keys().map(String::as_str).collect();
        bad(c, format!("family `{family}` doesn't exist (there are: {})", names.join(", ")))
    })?;
    let type_name = match string(c, p, "type")? {
        Some(t) => t,
        None => f.types.keys().next().cloned().ok_or_else(|| bad(c, format!("family `{family}` has no type")))?,
    };
    let params = match p.get("params") {
        Some(Value::Object(o)) => o.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())).collect(),
        v if absent(v) => BTreeMap::new(),
        v => return Err(bad(c, format!("`params` is {{parameter: value}}, not {}", v.unwrap_or(&Value::Null)))),
    };
    let adaptive = p
        .get("adaptive")
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("adaptive: {e}"))))
        .transpose()?;
    let offset = num(c, p, "offset")?;
    let angle = num(c, p, "angle")?.unwrap_or(0.0);
    let mirrored = flag(c, p, "mirrored")?;
    let comp = match host_param(c, m, p)? {
        Some(Given::Plane(g)) => {
            let host = HostRef::Plane(g);
            let frame = m.host_frame(&host).ok_or_else(|| bad(c, "`host`: no such reference plane"))?;
            let given = point2or3(c, p, "at", 0.0)?.ok_or_else(|| bad(c, "`at` [x, y] or [x, y, z] is required"))?;
            let hosted = Hosted { host, uv: on_plane(c, &frame, given)?, offset: offset.unwrap_or(0.0) };
            let w = Model::hosted_point(&frame, hosted.uv, hosted.offset);
            let level = match p.get("level") {
                v if absent(v) => level_under(m, w[2]).ok_or_else(|| bad(c, "the model has no levels"))?,
                _ => level_param(c, m, p, "level")?,
            };
            Component {
                adaptive,
                family,
                type_name,
                level,
                at: P2(w[0], w[1]),
                angle,
                offset: w[2] - m.elevation(level),
                mirrored,
                params,
                host: Some(hosted),
            }
        }
        Some(Given::Face(element, face)) => {
            let given = point2or3(c, p, "at", 0.0)?.ok_or_else(|| bad(c, "`at` [x, y] or [x, y, z] is required"))?;
            let host = HostRef::Face { element, face };
            let hosted = Hosted { host, uv: on_face(c, m, element, face, given)?, offset: offset.unwrap_or(0.0) };
            let w = m.host_at(&hosted).ok_or_else(|| bad(c, "`host`: no such face"))?.lifted(hosted.offset);
            let level = match p.get("level") {
                v if absent(v) => level_under(m, w[2]).ok_or_else(|| bad(c, "the model has no levels"))?,
                _ => level_param(c, m, p, "level")?,
            };
            Component {
                adaptive,
                family,
                type_name,
                level,
                at: P2(w[0], w[1]),
                angle,
                offset: w[2] - m.elevation(level),
                mirrored,
                params,
                host: Some(hosted),
            }
        }
        given => {
            let level = match given {
                Some(Given::Level(l)) => l,
                _ => level_param(c, m, p, "level")?,
            };
            // A family placed on its level, given no height, stands at its Default Elevation; a work plane-based one on its plane,
            // and an adaptive one (`Model::placed_by_points`) where its points put it, as Revit places it by its points alone.
            let at = point_req(c, p, "at")?;
            let mut comp = Component { adaptive, family, type_name, level, at, angle, offset: 0.0, mirrored, params, host: None };
            comp.offset = match offset {
                Some(o) => o,
                None if f.work_plane_based || m.placed_by_points(&comp) => 0.0,
                None => f.default_elevation,
            };
            comp
        }
    };
    let id = s.model_mut().insert(Element::Component(comp));
    Ok(json!({"id": id}))
}

/// `edit_work_plane`: a family's form or reference line put on another of its planes, or the project's components put on a
/// level or a reference plane.
pub(super) fn edit_work_plane(s: &mut Session, p: &Value) -> R {
    if p.get("family").is_some_and(|v| !v.is_null()) { family_work_plane(s, p) } else { components_work_plane(s, p) }
}

fn components_work_plane(s: &mut Session, p: &Value) -> R {
    let c = "edit_work_plane";
    only(c, p, &["ids", "id", "host"])?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    let given = host_param(c, m, p)?.ok_or_else(|| bad(c, "`host` {\"level\": …}, {\"plane\": …} or {\"face\": …} is required"))?;
    let mut changed = Vec::new();
    for id in ids {
        let Some(Element::Component(old)) = m.elements.get(id) else {
            let what = m.elements.get(id).map_or("nothing", Element::category);
            return Err(bad(c, format!("{what} {id} isn't a component: only a component's work plane changes here")));
        };
        let mut cp = old.clone();
        let w = [cp.at.0, cp.at.1, m.elevation(cp.level) + cp.offset];
        match given {
            // On a level, at its plan point, on the level.
            Given::Level(l) => (cp.host, cp.level, cp.offset) = (None, l, 0.0),
            Given::Plane(g) => {
                let host = HostRef::Plane(g);
                let frame = m.host_frame(&host).ok_or_else(|| bad(c, "`host`: no such reference plane"))?;
                // From a plane, its Offset from Host kept; from a level, on the plane.
                let offset = old.host.map_or(0.0, |h| h.offset);
                let upright = frame.z[2].abs() < 1e-9;
                let uv = Model::hosted_uv(&frame, if upright { w } else { [w[0], w[1], 0.0] }, if upright { offset } else { 0.0 });
                let hosted = Hosted { host, uv, offset };
                let at = Model::hosted_point(&frame, uv, offset);
                (cp.at, cp.offset, cp.host) = (P2(at[0], at[1]), at[2] - m.elevation(cp.level), Some(hosted));
            }
            Given::Face(element, face) => {
                // Taken onto the face (on a wall's side, its nearest point), its Offset from Host kept from a plane or a face.
                let offset = old.host.map_or(0.0, |h| h.offset);
                let probe = Hosted { host: HostRef::Face { element, face }, uv: [0.0, 0.0], offset };
                let uv = m.host_uv(&probe, w).ok_or_else(|| bad(c, "`host`: no such face"))?;
                let hosted = Hosted { uv, ..probe };
                let at = m.host_at(&hosted).ok_or_else(|| bad(c, "`host`: no such face"))?.lifted(offset);
                (cp.at, cp.offset, cp.host) = (P2(at[0], at[1]), at[2] - m.elevation(cp.level), Some(hosted));
            }
        }
        changed.push((id, cp));
    }
    let model = s.model_mut();
    for (id, cp) in &changed {
        if let Some(e) = model.elements.get_mut(*id) {
            *e = Element::Component(cp.clone());
        }
    }
    Ok(json!({"ids": changed.iter().map(|(id, _)| *id).collect::<Vec<_>>()}))
}

/// A plane of family `f` by name (a datum's too), as the Work Plane dialog lists it (`"Reference Line 1 : Vertical"`), or
/// as `PlaneRef` takes it.
fn family_host(c: &str, f: &crate::family::Family, v: &Value) -> Result<PlaneRef, String> {
    if let Value::String(n) = v {
        return f.plane_set().by_name(n).ok_or_else(|| {
            let names: Vec<String> = f.plane_set().listed().into_iter().map(|(_, l)| l).collect();
            bad(c, format!("the family has no work plane named `{n}`; by name they are: {}", names.join(", ")))
        });
    }
    if let Some(Value::String(n)) = v.get("plane")
        && Guid::parse(n).is_none()
    {
        let plane = f.planes.iter().find(|pl| pl.name == *n).ok_or_else(|| bad(c, format!("the family has no plane named `{n}`")))?;
        return Ok(PlaneRef::Plane(plane.guid));
    }
    let mut host: PlaneRef = serde_json::from_value(v.clone()).map_err(|e| bad(c, format!("`host`: {e}")))?;
    // An id, as the family's planes and lines hold them.
    let as_id = |g: Guid| g.pending();
    if let Some(id) = as_id(host.guid()) {
        let found = match host {
            PlaneRef::Plane(_) => f.planes.iter().find(|pl| pl.id == id).map(|pl| pl.guid),
            PlaneRef::Line(..) => f.lines.iter().find(|l| l.id == id).map(|l| l.guid),
        };
        let g = found.ok_or_else(|| bad(c, format!("`host`: the family has no plane or line {id}")))?;
        host = match host {
            PlaneRef::Plane(_) => PlaneRef::Plane(g),
            PlaneRef::Line(_, face) => PlaneRef::Line(g, face),
        };
    }
    Ok(host)
}

/// `a·u + b·v + k` of two coordinates of a profile, a number where both are, else a formula.
fn affine(a: f64, u: &Expr, b: f64, v: &Expr, k: f64) -> Expr {
    let snap = |x: f64| {
        let r = x.round();
        if (x - r).abs() < 1e-12 { r } else { x }
    };
    let (a, b, k) = (snap(a), snap(b), snap(k));
    if let (Expr::Number(x), Expr::Number(y)) = (u, v) {
        return Expr::Number(a * x + b * y + k + 0.0);
    }
    let mut terms: Vec<String> = Vec::new();
    let mut constant = k;
    for (coef, e) in [(a, u), (b, v)] {
        if coef == 0.0 {
            continue;
        }
        match e {
            Expr::Number(x) => constant += coef * x,
            Expr::Formula(f) if coef == 1.0 => terms.push(format!("({f})")),
            Expr::Formula(f) if coef == -1.0 => terms.push(format!("-({f})")),
            Expr::Formula(f) => terms.push(format!("{coef} * ({f})")),
        }
    }
    if constant != 0.0 {
        terms.push(format!("{constant}"));
    }
    Expr::Formula(terms.join(" + "))
}

/// The map of a profile drawn on `from` to the same place drawn on `to`, two parallel planes: each new coordinate as
/// `[a, b, k]` of the old two, the normal's sign and the planes' distance along the new one's normal.
fn reexpress(from: &Frame, to: &Frame) -> Option<([[f64; 3]; 2], f64, f64)> {
    let s = dot(from.z, to.z);
    if (s.abs() - 1.0).abs() > 1e-9 {
        return None;
    }
    let o = sub(from.origin, to.origin);
    let row = |axis: P3| [dot(from.x, axis), dot(from.y, axis), dot(o, axis)];
    Some(([row(to.x), row(to.y)], s.signum(), dot(o, to.z)))
}

fn family_work_plane(s: &mut Session, p: &Value) -> R {
    let c = "edit_work_plane";
    only(c, p, &["family", "form", "ref_line", "host", "keep", "dry_run"])?;
    let name = string(c, p, "family")?.ok_or_else(|| bad(c, "`family` is required"))?;
    let f = s.model.families.get(&name).ok_or_else(|| bad(c, format!("family `{name}` doesn't exist")))?;
    let host_v =
        p.get("host").filter(|v| !v.is_null()).ok_or_else(|| bad(c, "`host` {\"plane\": …} or {\"line\": …, \"face\": …} is required"))?;
    let host = family_host(c, f, host_v)?;
    let to = f.frame(&host).map_err(|e| bad(c, format!("`host`: {e}")))?;
    let keep = flag(c, p, "keep")?;
    let part = |key: &str, ids: Vec<(Id, Guid)>| -> Result<Option<usize>, String> {
        let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
        let g: Guid = serde_json::from_value(v.clone()).map_err(|_| bad(c, format!("`{key}` is an id or a GUID, not {v}")))?;
        let at = ids.iter().position(|(id, guid)| *guid == g || g.pending() == Some(*id));
        at.map(Some).ok_or_else(|| bad(c, format!("family `{name}` has no {key} {v}")))
    };
    let form = part("form", f.forms.iter().map(|x| (x.id, x.guid)).collect())?;
    let line = part("ref_line", f.lines.iter().map(|x| (x.id, x.guid)).collect())?;
    let patch = match (form, line) {
        (Some(i), None) => {
            let mut fm = f.forms[i].clone();
            if !fm.mesh.is_empty() {
                return Err(bad(c, "the form is a mesh, its triangles in the family's own axes: it has no work plane to change"));
            }
            let from = f.frame(&fm.host).map_err(|e| bad(c, e))?;
            if keep {
                let Some(([rx, ry], sign, d)) = reexpress(&from, &to) else {
                    return Err(bad(c, "`keep`: the new plane isn't parallel to the form's, so the form can't stay where it is on it"));
                };
                let map = |q: &[Expr; 2]| [affine(rx[0], &q[0], rx[1], &q[1], rx[2]), affine(ry[0], &q[0], ry[1], &q[1], ry[2])];
                // A plane that mirrors the profile turns its arcs the other way.
                let flip = rx[0] * ry[1] - rx[1] * ry[0] < 0.0;
                fm.sketch.iter_mut().for_each(|l| l.map_points(map, flip));
                if !fm.revolves() {
                    let depth = |t: &Expr| affine(sign, t, 0.0, &Expr::Number(0.0), d);
                    (fm.from, fm.to) = if sign > 0.0 { (depth(&fm.from), depth(&fm.to)) } else { (depth(&fm.to), depth(&fm.from)) };
                }
            }
            fm.host = host;
            json!({"form": fm})
        }
        (None, Some(i)) => {
            let mut l = f.lines[i].clone();
            if !keep {
                let from = f.frame(&l.host).map_err(|e| bad(c, e))?;
                let local = |q: P3| {
                    let d = sub(q, from.origin);
                    [dot(d, from.x), dot(d, from.y), dot(d, from.z)]
                };
                let (a, b) = (local(l.start), local(l.end));
                (l.start, l.end) = (to.point(P2(a[0], a[1]), a[2]), to.point(P2(b[0], b[1]), b[2]));
            }
            l.host = host;
            json!({"ref_line": l})
        }
        (None, None) => return Err(bad(c, "give the `form` or the `ref_line` whose work plane changes")),
        (Some(_), Some(_)) => return Err(bad(c, "give one of `form` and `ref_line`")),
    };
    let mut call = patch;
    // Its host whole: merged over the old one, a plane's would keep a line's `face`.
    let host_json = match host {
        PlaneRef::Plane(g) => json!({"plane": g, "line": null, "face": null}),
        PlaneRef::Line(g, face) => json!({"plane": null, "line": g, "face": face}),
    };
    for key in ["form", "ref_line"] {
        if let Some(part) = call.get_mut(key) {
            part["host"] = host_json.clone();
        }
    }
    call["name"] = json!(name);
    if let Some(d) = p.get("dry_run") {
        call["dry_run"] = d.clone();
    }
    family(s, &call).map_err(|e| e.replacen("family: ", "edit_work_plane: ", 1))
}
