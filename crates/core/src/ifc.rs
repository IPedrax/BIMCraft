//! IFC4 export (ISO 16739 as an ISO 10303-21 STEP file), the open BIM exchange format every BIM
//! tool reads. Levels become building storeys; walls (their plan outline, mitred where they join,
//! curved or straight), slabs, ceilings (coverings), spaces and columns get swept-solid bodies,
//! roofs, sloped slabs and walls attached to them closed shells of flat faces; each door and window
//! fills an opening element that voids its wall, so readers cut the hole, and a shaft voids each
//! slab, ceiling and roof it passes through with one; compound walls carry their layers as a
//! material layer set. A stair or a ramp is the sum of its parts: its flights, its landings and
//! the railings along it; a railing carries its rail's top line as its axis. What an element holds
//! of the project's parameters, and of its phase and design option, goes in property sets of its own;
//! the project's phases and design option sets in one on the project.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::geom::{self, P2};
use serde_json::Value;

use crate::model::{DEFAULT_HEIGHT, Element, Function, Guid, Id, Layer, Model, ParameterKind, Railing, heading, mix};
use crate::phasing;
use crate::stairs;
use crate::view3d::P3;
use crate::walls::{self, Axis};

// What a GlobalId is for, so each one stays the same from export to export.
const PROJECT: u64 = 1;
const SITE: u64 = 2;
const BUILDING: u64 = 3;
const STOREY: u64 = 4;
const ELEMENT: u64 = 5;
const OPENING: u64 = 6;
const VOIDS: u64 = 7;
const FILLS: u64 = 8;
const AGGREGATES: u64 = 9;
const CONTAINS: u64 = 10;
const SPACES: u64 = 11;
const MATERIALS: u64 = 12;
const SHAFT: u64 = 13;
const SHAFT_VOIDS: u64 = 14;
const FLIGHT: u64 = 15;
const LANDING: u64 = 16;
const PARTS: u64 = 17;
const SLAB_MATERIALS: u64 = 18;
const COMPONENT_MATERIALS: u64 = 19;
const PROPERTIES: u64 = 20;
const DEFINES: u64 = 21;
const PHASED: u64 = 22;
const PHASED_DEFINES: u64 = 23;
const RUN_MATERIALS: u64 = 40;
const RUN_PROPERTIES: u64 = 41;
const RUN_DEFINES: u64 = 42;
const COMMON: u64 = 43;
const COMMON_DEFINES: u64 = 44;
const SYSTEMS: u64 = 45;
const SYSTEM_MEMBERS: u64 = 46;
const LOFTED: u64 = 47;
const LOFTED_DEFINES: u64 = 48;

/// The property set an element's values of the project's parameters go in.
pub const PARAMETERS: &str = "Project Parameters";

/// The property set a run's justification and category go in (its system is an IfcSystem it is assigned to).
pub const RUN: &str = "BIMCraft_Run";

/// The property set a lofted mass records how its footprint and its top outline pair up in, corner for corner (the body is
/// a closed face set that doesn't say): `Loft`, JSON `{bottom, top}`.
pub const LOFT: &str = "BIMCraft_Mass";

/// The property set an element's phase and design option go in; on the project, its phases and design option sets.
pub const PHASING: &str = "BIMCraft_Phasing";

/// What a `Phase Created` says of itself when the element holds none: it was made in the first phase.
pub const IMPLIED: &str = "Implied: no phase is set for it, so it was made in the first phase";

/// What a stair's or a ramp's IFC type is, by its landings: `kind` is STAIR or RAMP.
fn shape_of(landings: &[crate::model::Landing], kind: &str) -> String {
    let turns: Vec<f64> = landings.iter().map(|l| l.turn).collect();
    let shape = match turns.as_slice() {
        [] => "STRAIGHT_RUN",
        [0.0] => "TWO_STRAIGHT_RUN",
        [t] if t.abs() == 90.0 => "QUARTER_TURN",
        [t] if t.abs() == 180.0 => "HALF_TURN",
        [a, b] if a.abs() == 90.0 && a == b => "TWO_QUARTER_TURN",
        _ => return ".NOTDEFINED.".into(),
    };
    format!(".{shape}_{kind}.")
}

/// A STEP string literal: `'` doubled, `\` doubled, anything outside printable ASCII as `\X2\…\X0\`
/// (UCS-2), or past it as `\X4\…\X0\` (UCS-4).
fn s(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str(r"\\"),
            ' '..='~' => out.push(c),
            _ if u32::from(c) > 0xFFFF => {
                let _ = write!(out, r"\X4\{:08X}\X0\", u32::from(c));
            }
            _ => {
                let _ = write!(out, r"\X2\{:04X}\X0\", u32::from(c));
            }
        }
    }
    out.push('\'');
    out
}

/// A STEP real: always with a decimal point.
fn real(x: f64) -> String {
    let x = if x.is_finite() && x != 0.0 { x } else { 0.0 };
    format!("{x:.6}").trim_end_matches('0').to_string()
}

/// A direction's ratio: as a real, to twelve places, so a turn reads back to the millionth of a degree.
fn ratio(x: f64) -> String {
    let x = if x.is_finite() && x != 0.0 { x } else { 0.0 };
    format!("{x:.12}").trim_end_matches('0').to_string()
}

/// The digits of an IFC GlobalId, in order.
const DIGITS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// A 22-character IFC GlobalId, the same for the same model, role and element on every export.
fn guid(uid: u64, role: u64, id: u64) -> String {
    let hi = mix(uid ^ mix(role.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ id));
    let lo = mix(hi ^ id.rotate_left(17) ^ role);
    compress((u128::from(hi) << 64) | u128::from(lo))
}

/// 128 bits as an IFC GlobalId: 22 base-64 digits, the first carrying the top 2 bits. An element's GUID written so is its
/// GlobalId, as IfcOpenShell's `guid.compress` writes a UUID.
pub fn compress(v: u128) -> String {
    let digit = |shift: u32| char::from(DIGITS[((v >> shift) & 63) as usize]);
    std::iter::once(digit(126)).chain((0..21).rev().map(|i| digit(6 * i))).collect()
}

/// The 128 bits an IFC GlobalId holds; none for one that isn't 22 of its digits, the first at most 3.
pub fn expand(text: &str) -> Option<u128> {
    let b = text.as_bytes();
    if b.len() != 22 {
        return None;
    }
    b.iter()
        .try_fold(0u128, |v, c| {
            let d = DIGITS.iter().position(|x| x == c)? as u128;
            (v >> 122 == 0).then_some((v << 6) | d)
        })
        .filter(|_| DIGITS.iter().position(|x| *x == b[0]).is_some_and(|d| d < 4))
}

/// A GUID's 128 bits folded to 64, to seed what an element writes beside itself.
fn fold(g: Guid) -> u64 {
    let v = g.as_u128();
    (v >> 64) as u64 ^ v as u64
}

/// UTC date and time of a Unix time, ISO 8601.
fn utc(secs: u64) -> String {
    // Days to civil date: Howard Hinnant's algorithm.
    let z = (secs / 86_400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60)
}

fn refs(ids: &[u64]) -> String {
    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")
}

struct Step {
    out: String,
    next: u64,
    uid: u64,
    /// Each element's GUID, by its id.
    own: BTreeMap<Id, Guid>,
    /// Shared: the origin placement, +Z, the model's context and its body subcontext.
    world: u64,
    z: u64,
    context: u64,
    body: u64,
    /// Each material written: it, and the surface style its colour shades with.
    materials: BTreeMap<String, (u64, Option<u64>)>,
}

impl Step {
    /// Writes `#n=ENTITY;` and returns n.
    fn add(&mut self, entity: impl AsRef<str>) -> u64 {
        self.next += 1;
        let _ = writeln!(self.out, "#{}={};", self.next, entity.as_ref());
        self.next
    }

    /// The GlobalId of what plays `role` for `id`: an element's or a storey's own is its GUID; what else an element writes
    /// (its openings, its parts, its relations) is made from its GUID too, so a new id changes none of them.
    fn guid(&self, role: u64, id: u64) -> String {
        match self.own.get(&id) {
            Some(g) if role == ELEMENT || role == STOREY => s(&compress(g.as_u128())),
            Some(g) => s(&guid(self.uid, role, fold(*g))),
            None => s(&guid(self.uid, role, id)),
        }
    }

    /// Part `i` of element `id`, for its GlobalId.
    fn part(&self, id: Id, i: usize) -> u64 {
        mix(self.own.get(&id).map_or(id, |g| fold(*g))).wrapping_add(i as u64)
    }

    /// A text property, `note` describing it.
    fn text_property(&mut self, name: &str, note: Option<&str>, value: &str) -> u64 {
        let note = note.map_or_else(|| "$".to_string(), s);
        self.add(format!("IFCPROPERTYSINGLEVALUE({},{note},IFCTEXT({}),$)", s(name), s(value)))
    }

    /// A property set `name` of `props` for `ent`; its GlobalIds from `(set, relation)` and `key`.
    fn define(&mut self, ent: u64, name: &str, props: &[u64], (set, relation): (u64, u64), key: u64) {
        let set = self.add(format!("IFCPROPERTYSET({},$,{},$,({}))", self.guid(set, key), s(name), refs(props)));
        self.add(format!("IFCRELDEFINESBYPROPERTIES({},$,$,$,(#{ent}),#{set})", self.guid(relation, key)));
    }

    /// A material by name, written once; one the library has, with its colour, for viewers to
    /// shade it in.
    fn material(&mut self, m: &Model, name: &str) -> u64 {
        self.material_style(m, name).0
    }

    /// A material by name, written once, and the surface style its colour shades with.
    fn material_style(&mut self, m: &Model, name: &str) -> (u64, Option<u64>) {
        if let Some(&written) = self.materials.get(name) {
            return written;
        }
        let id = self.add(format!("IFCMATERIAL({},$,$)", s(name)));
        let style = m.materials.get(name).map(|x| {
            let [r, g, b] = x.color.map(|c| real(f64::from(c) / 255.0));
            let colour = self.add(format!("IFCCOLOURRGB($,{r},{g},{b})"));
            let shading = self.add(format!("IFCSURFACESTYLESHADING(#{colour},$)"));
            let style = self.add(format!("IFCSURFACESTYLE({},.BOTH.,(#{shading}))", s(name)));
            let item = self.add(format!("IFCSTYLEDITEM($,(#{style}),$)"));
            let rep = self.add(format!("IFCSTYLEDREPRESENTATION(#{},'Style','Material',(#{item}))", self.context));
            self.add(format!("IFCMATERIALDEFINITIONREPRESENTATION($,$,(#{rep}),#{id})"));
            style
        });
        self.materials.insert(name.to_string(), (id, style));
        (id, style)
    }

    /// A closed surface of triangles, each corner written once.
    fn faceset(&mut self, tris: &[[P3; 3]]) -> u64 {
        let mut index: BTreeMap<[i64; 3], usize> = BTreeMap::new();
        let mut pts: Vec<P3> = Vec::new();
        let mut faces = Vec::new();
        for t in tris {
            let corners: Vec<usize> = t
                .iter()
                .map(|p| {
                    *index.entry(p.map(|x| (x * 1e3).round() as i64)).or_insert_with(|| {
                        pts.push(*p);
                        pts.len()
                    })
                })
                .collect();
            if let [a, b, c] = corners[..]
                && a != b
                && b != c
                && a != c
            {
                faces.push(format!("({a},{b},{c})"));
            }
        }
        let coords: Vec<String> = pts.iter().map(|p| format!("({},{},{})", real(p[0]), real(p[1]), real(p[2]))).collect();
        let list = self.add(format!("IFCCARTESIANPOINTLIST3D(({}))", coords.join(",")));
        self.add(format!("IFCTRIANGULATEDFACESET(#{list},$,.T.,({}),$)", faces.join(",")))
    }

    /// A body of tessellated `items`, the closed face sets.
    fn tessellated(&mut self, items: &[u64]) -> u64 {
        let rep = self.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','Tessellation',({}))", self.body, refs(items)));
        self.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"))
    }

    fn point(&mut self, p: P2) -> u64 {
        self.add(format!("IFCCARTESIANPOINT(({},{}))", real(p.0), real(p.1)))
    }

    /// Axes at `(x, y, z)` with the x axis along `dir`.
    fn axes(&mut self, (x, y, z): (f64, f64, f64), dir: Option<P2>) -> u64 {
        let o = self.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(x), real(y), real(z)));
        match dir {
            Some(d) => {
                let d = self.add(format!("IFCDIRECTION(({},{},0.))", ratio(d.0), ratio(d.1)));
                self.add(format!("IFCAXIS2PLACEMENT3D(#{o},#{},#{d})", self.z))
            }
            None => self.add(format!("IFCAXIS2PLACEMENT3D(#{o},$,$)")),
        }
    }

    /// A placement at `xyz` with its x axis along `dir`, relative to `to` (none = the world).
    fn place(&mut self, to: Option<u64>, xyz: (f64, f64, f64), dir: Option<P2>) -> u64 {
        let a = self.axes(xyz, dir);
        let to = to.map_or_else(|| "$".to_string(), |t| format!("#{t}"));
        self.add(format!("IFCLOCALPLACEMENT({to},#{a})"))
    }

    /// A closed polyline through `ring`.
    fn polyline(&mut self, ring: &[P2]) -> u64 {
        let mut pts: Vec<u64> = ring.iter().map(|p| self.point(*p)).collect();
        pts.extend(pts.first().copied());
        self.add(format!("IFCPOLYLINE(({}))", refs(&pts)))
    }

    /// A rectangle `size` big centred on `centre`, as a profile.
    fn rectangle(&mut self, centre: P2, size: P2) -> u64 {
        let c = self.point(centre);
        let at = self.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
        self.add(format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,#{at},{},{})", real(size.0), real(size.1)))
    }

    /// A profile around `outer`, less `holes`.
    fn profile(&mut self, outer: &[P2], holes: &[Vec<P2>]) -> u64 {
        let o = self.polyline(outer);
        if holes.is_empty() {
            return self.add(format!("IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{o})"));
        }
        let inner: Vec<u64> = holes.iter().map(|h| self.polyline(h)).collect();
        self.add(format!("IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#{o},({}))", refs(&inner)))
    }

    /// A body: `profile` swept `depth` along the z axis of placement `at`, which aims it anywhere.
    fn body_along(&mut self, profile: u64, at: u64, depth: f64) -> u64 {
        let solid = self.add(format!("IFCEXTRUDEDAREASOLID(#{profile},#{at},#{},{})", self.z, real(depth)));
        let rep = self.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','SweptSolid',(#{solid}))", self.body));
        self.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"))
    }

    /// Axes at the origin with the z axis along `z` and the x axis along `x`, which is square to it.
    fn aim(&mut self, z: P3, x: P3) -> u64 {
        let o = self.add("IFCCARTESIANPOINT((0.,0.,0.))");
        let dir = |w: &mut Step, d: P3| w.add(format!("IFCDIRECTION(({},{},{}))", ratio(d[0]), ratio(d[1]), ratio(d[2])));
        let (z, x) = (dir(self, z), dir(self, x));
        self.add(format!("IFCAXIS2PLACEMENT3D(#{o},#{z},#{x})"))
    }

    /// `Pset_*Common` of something structural: it carries load.
    fn load_bearing(&mut self, ent: u64, pset: &str, id: Id) {
        let load = self.add("IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)");
        self.define(ent, pset, &[load], (COMMON, COMMON_DEFINES), id);
    }

    /// A body: `profile` swept `depth` up from `bottom`, plus any extra representations.
    fn body(&mut self, profile: u64, bottom: f64, depth: f64, extra: &[u64]) -> u64 {
        self.bodies(&[(profile, bottom, depth)], extra)
    }

    /// A body of solids, each a profile swept `depth` up from `bottom`, plus any extra
    /// representations.
    fn bodies(&mut self, solids: &[(u64, f64, f64)], extra: &[u64]) -> u64 {
        let items: Vec<u64> = solids
            .iter()
            .map(|&(profile, bottom, depth)| {
                let at = if bottom == 0.0 { self.world } else { self.axes((0.0, 0.0, bottom), None) };
                self.add(format!("IFCEXTRUDEDAREASOLID(#{profile},#{at},#{},{})", self.z, real(depth)))
            })
            .collect();
        let rep = self.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','SweptSolid',({}))", self.body, refs(&items)));
        // Body first: readers that take the first representation get the solid.
        let reps: Vec<u64> = std::iter::once(rep).chain(extra.iter().copied()).collect();
        self.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({}))", refs(&reps)))
    }

    /// A body of closed shells: around each of the flat counter-clockwise `faces` and its copy
    /// `rise` above; heights from `base`.
    fn breps(&mut self, faces: &[Vec<P3>], base: f64, rise: f64) -> u64 {
        let breps: Vec<u64> = faces.iter().map(|face| self.brep(face, base, rise)).collect();
        self.solid(&breps, &[])
    }

    /// A body of closed shells, plus any extra representations.
    fn solid(&mut self, breps: &[u64], extra: &[u64]) -> u64 {
        let rep = self.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','Brep',({}))", self.body, refs(breps)));
        let reps: Vec<u64> = std::iter::once(rep).chain(extra.iter().copied()).collect();
        self.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({}))", refs(&reps)))
    }

    /// A closed shell around a ring in plan, counter-clockwise, from one plane up to another;
    /// heights from `base`.
    fn prism(&mut self, ring: &[P2], bottom: &walls::Plane, top: &walls::Plane, base: f64) -> u64 {
        let at = |q: &P2, p: &walls::Plane| [q.0, q.1, p.at(*q) - base];
        let (b, t): (Vec<P3>, Vec<P3>) = ring.iter().map(|q| (at(q, bottom), at(q, top))).unzip();
        self.shell(&b, &t)
    }

    /// Landings: each a slab from `base` up to its top, placed in `at`.
    fn landings(&mut self, l: &stairs::Layout, at: u64, base: f64, id: Id) -> Vec<u64> {
        l.pads
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let pat = self.place(Some(at), (0.0, 0.0, 0.0), None);
                let profile = self.profile(&p.ring, &[]);
                let shape = self.body(profile, 0.0, p.z - base, &[]);
                self.add(format!("IFCSLAB({},$,'Landing',$,$,#{pat},#{shape},$,.LANDING.)", self.guid(LANDING, self.part(id, i))))
            })
            .collect()
    }

    /// A railing, placed in `at`: its rail and balusters as closed shells, its rail's top line
    /// as its axis (in the `axis` context); heights from `base`.
    fn railing(&mut self, m: &Model, id: Id, rl: &Railing, at: u64, base: f64, axis: u64) -> u64 {
        let height = m.types.railing.get(&rl.type_name).map_or(0.0, |t| t.height);
        let parts = stairs::railing_parts(m, rl);
        let breps: Vec<u64> = parts.iter().map(|(ring, bottom, top)| self.prism(ring, bottom, top, base)).collect();
        let pts: Vec<u64> = stairs::railing(m, rl)
            .iter()
            .map(|(p, z)| self.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(p.0), real(p.1), real(z + height - base))))
            .collect();
        let line = self.add(format!("IFCPOLYLINE(({}))", refs(&pts)));
        let axis_rep = self.add(format!("IFCSHAPEREPRESENTATION(#{axis},'Axis','Curve3D',(#{line}))"));
        let shape = self.solid(&breps, &[axis_rep]);
        let rat = self.place(Some(at), (0.0, 0.0, 0.0), None);
        let name = s(&rl.type_name);
        self.add(format!("IFCRAILING({},$,{name},$,{name},#{rat},#{shape},'{id}',.GUARDRAIL.)", self.guid(ELEMENT, id)))
    }

    /// A closed shell around the flat counter-clockwise `face` and its copy `rise` above, its
    /// faces turned out; heights from `base`.
    fn brep(&mut self, face: &[[f64; 3]], base: f64, rise: f64) -> u64 {
        let ring = |up: f64| -> Vec<[f64; 3]> { face.iter().map(|p| [p[0], p[1], p[2] - base + up]).collect() };
        self.shell(&ring(0.0), &ring(rise))
    }

    /// A closed shell from the counter-clockwise `bottom` ring up to `top`, the same points above
    /// it: its faces turned out.
    fn shell(&mut self, bottom: &[[f64; 3]], top: &[[f64; 3]]) -> u64 {
        let mut ring = |pts: &[[f64; 3]]| -> Vec<u64> {
            pts.iter().map(|p| self.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(p[0]), real(p[1]), real(p[2])))).collect()
        };
        let (bottom, top) = (ring(bottom), ring(top));
        let mut loops = vec![bottom.iter().rev().copied().collect(), top.clone()];
        let pairs = || bottom.iter().zip(&top);
        loops.extend(pairs().zip(pairs().cycle().skip(1)).map(|((b0, t0), (b1, t1))| vec![*b0, *b1, *t1, *t0]));
        let faces: Vec<u64> = loops
            .iter()
            .map(|l| {
                let lp = self.add(format!("IFCPOLYLOOP(({}))", refs(l)));
                let bound = self.add(format!("IFCFACEOUTERBOUND(#{lp},.T.)"));
                self.add(format!("IFCFACE((#{bound}))"))
            })
            .collect();
        let shell = self.add(format!("IFCCLOSEDSHELL(({}))", refs(&faces)));
        self.add(format!("IFCFACETEDBREP(#{shell})"))
    }
}

pub fn export(m: &Model) -> String {
    let own = m.elements.with_guids().map(|(id, g, _)| (id, g)).collect();
    let mut w = Step { out: String::new(), next: 0, uid: m.uid, own, world: 0, z: 0, context: 0, body: 0, materials: BTreeMap::new() };
    let origin = w.add("IFCCARTESIANPOINT((0.,0.,0.))");
    w.z = w.add("IFCDIRECTION((0.,0.,1.))");
    w.world = w.add(format!("IFCAXIS2PLACEMENT3D(#{origin},$,$)"));
    let ctx = w.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#{},$)", w.world));
    w.context = ctx;
    w.body = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{ctx},$,.MODEL_VIEW.,$)"));
    let axis = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Axis','Model',*,*,*,*,#{ctx},$,.GRAPH_VIEW.,$)"));
    let units = [
        "IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)",
        "IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)",
        "IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)",
        "IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)",
    ]
    .map(|u| w.add(u));
    let units = w.add(format!("IFCUNITASSIGNMENT(({}))", refs(&units)));
    let project = w.add(format!("IFCPROJECT({},$,{},$,$,$,$,(#{ctx}),#{units})", w.guid(PROJECT, 0), s(&m.name)));
    let site_at = w.place(None, (0.0, 0.0, 0.0), None);
    let site = w.add(format!("IFCSITE({},$,'Site',$,$,#{site_at},$,$,.ELEMENT.,$,$,$,$,$)", w.guid(SITE, 0)));
    let building_at = w.place(Some(site_at), (0.0, 0.0, 0.0), None);
    let building = w.add(format!("IFCBUILDING({},$,{},$,$,#{building_at},$,$,.ELEMENT.,$,$,$)", w.guid(BUILDING, 0), s(&m.name)));
    w.add(format!("IFCRELAGGREGATES({},$,$,$,#{project},(#{site}))", w.guid(AGGREGATES, PROJECT)));
    w.add(format!("IFCRELAGGREGATES({},$,$,$,#{site},(#{building}))", w.guid(AGGREGATES, SITE)));

    // Storeys: (entity, placement) by level.
    let mut storeys: BTreeMap<Guid, (u64, u64)> = BTreeMap::new();
    for (id, l) in m.levels() {
        let at = w.place(Some(building_at), (0.0, 0.0, l.elevation), None);
        let st = w.add(format!("IFCBUILDINGSTOREY({},$,{},$,$,#{at},$,$,.ELEMENT.,{})", w.guid(STOREY, id), s(&l.name), real(l.elevation)));
        storeys.insert(m.guid(id), (st, at));
    }
    let all: Vec<u64> = storeys.values().map(|(st, _)| *st).collect();
    if !all.is_empty() {
        w.add(format!("IFCRELAGGREGATES({},$,$,$,#{building},({}))", w.guid(AGGREGATES, BUILDING), refs(&all)));
    }

    let mut contained: BTreeMap<Guid, Vec<u64>> = BTreeMap::new();
    let mut spaces: BTreeMap<Guid, Vec<u64>> = BTreeMap::new();
    // Each element's own entity, for what it holds of the project's parameters.
    let mut products: BTreeMap<Id, u64> = BTreeMap::new();
    // Walls as (entity, placement), for the openings in them.
    let mut placed: BTreeMap<Id, (u64, u64)> = BTreeMap::new();
    // Floors, ceilings and roofs as (entity, placement, its height), for the shafts through them.
    let mut hosts: BTreeMap<Id, (u64, u64, f64)> = BTreeMap::new();
    // Stairs and ramps as (entity, placement, its height, its parts), for the railings along them.
    let mut wholes: BTreeMap<Id, (u64, u64, f64, Vec<u64>)> = BTreeMap::new();
    // The runs in each system, by its name, for the IfcSystem they are assigned to.
    let mut systems: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for (&id, e) in &m.elements {
        match e {
            Element::Wall(wl) => {
                let Some(&(_, storey)) = storeys.get(&wl.level) else { continue };
                let (path, base) = (Axis::of(wl), m.own_base(wl));
                // Placed at its start, along the way it sets off: its axis, outline and openings in that frame.
                let u = path.tangent(0.0);
                let local = |p: P2| P2((p - wl.start).dot(u), (p - wl.start).dot(u.perp()));
                let at = w.place(Some(storey), (wl.start.0, wl.start.1, wl.base_offset), Some(u));
                let pts: Vec<u64> = path.points(0.0, path.length(), 0.0).into_iter().map(|p| w.point(local(p))).collect();
                let line = w.add(format!("IFCPOLYLINE(({}))", refs(&pts)));
                let axis_rep = w.add(format!("IFCSHAPEREPRESENTATION(#{axis},'Axis','Curve2D',(#{line}))"));
                // Its outline swept up, mitred where it joins others as the plan's is; attached to a
                // roof or a floor, a closed shell for each piece under (or over) a plane of it.
                let shape = if wl.attach_top.is_none() && wl.attach_base.is_none() {
                    let outline: Vec<P2> = walls::outline(m, id).into_iter().map(local).collect();
                    let profile = w.profile(&outline, &[]);
                    w.body(profile, 0.0, m.own_top(wl) - base, &[axis_rep])
                } else {
                    let breps: Vec<u64> = walls::prisms(m, id, false)
                        .iter()
                        .map(|p| {
                            let mut ring = p.ring.clone();
                            if geom::area(&ring) < 0.0 {
                                ring.reverse();
                            }
                            let up = |q: &P2, plane: &walls::Plane| {
                                let l = local(*q);
                                [l.0, l.1, plane.at(*q) - base]
                            };
                            let (bottom, top): (Vec<_>, Vec<_>) = ring.iter().map(|q| (up(q, &p.bottom), up(q, &p.top))).unzip();
                            w.shell(&bottom, &top)
                        })
                        .collect();
                    let rep = w.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','Brep',({}))", w.body, refs(&breps)));
                    w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep},#{axis_rep}))"))
                };
                let ent = w.add(format!(
                    "IFCWALL({},$,{},$,{},#{at},#{shape},'{id}',.STANDARD.)",
                    w.guid(ELEMENT, id),
                    s(&wl.type_name),
                    s(&wl.type_name)
                ));
                contained.entry(wl.level).or_default().push(ent);
                products.insert(id, ent);
                placed.insert(id, (ent, at));
            }
            Element::Floor(f) => {
                let Some(&(_, storey)) = storeys.get(&f.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                // Level, its outline less its openings swept up to its top; sloped, a shell round each
                // piece of it.
                let depth = m.floor_depth(f);
                let shape = if m.floor_top(f).g == P2(0.0, 0.0) {
                    let profile = w.profile(&f.boundary, &f.openings);
                    w.body(profile, f.offset - depth, depth, &[])
                } else {
                    let faces: Vec<Vec<P3>> = m.floor_slabs(f).into_iter().map(|(face, _)| face).collect();
                    w.breps(&faces, m.elevation(f.level), depth)
                };
                let ent = w.add(format!(
                    "IFCSLAB({},$,{},$,{},#{at},#{shape},'{id}',.FLOOR.)",
                    w.guid(ELEMENT, id),
                    s(&f.type_name),
                    s(&f.type_name)
                ));
                contained.entry(f.level).or_default().push(ent);
                products.insert(id, ent);
                hosts.insert(id, (ent, at, m.elevation(f.level)));
                if f.structural {
                    w.load_bearing(ent, "Pset_SlabCommon", id);
                }
            }
            Element::Ceiling(c) => {
                let Some(&(_, storey)) = storeys.get(&c.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                // Its outline less its openings swept up through its thickness.
                let profile = w.profile(&c.boundary, &c.openings);
                let shape = w.body(profile, c.offset, m.types.ceiling.get(&c.type_name).map_or(0.0, |t| t.thickness), &[]);
                let ent = w.add(format!(
                    "IFCCOVERING({},$,{},$,{},#{at},#{shape},'{id}',.CEILING.)",
                    w.guid(ELEMENT, id),
                    s(&c.type_name),
                    s(&c.type_name)
                ));
                contained.entry(c.level).or_default().push(ent);
                products.insert(id, ent);
                hosts.insert(id, (ent, at, m.elevation(c.level)));
            }
            Element::Room(r) => {
                let Some(&(_, storey)) = storeys.get(&r.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                // A room that isn't enclosed has no shape to give.
                let shape = match m.room_region(r) {
                    Some(region) => {
                        let height = m.level_above(r.level).map_or(DEFAULT_HEIGHT, |up| m.elevation(up) - m.elevation(r.level));
                        let profile = w.profile(&region.outer, &region.holes);
                        format!("#{}", w.body(profile, 0.0, height, &[]))
                    }
                    None => "$".into(),
                };
                let ent = w.add(format!(
                    "IFCSPACE({},$,{},$,$,#{at},{shape},{},.ELEMENT.,.SPACE.,$)",
                    w.guid(ELEMENT, id),
                    s(&r.number),
                    s(&r.name)
                ));
                spaces.entry(r.level).or_default().push(ent);
                products.insert(id, ent);
            }
            // A mass: a proxy of object type `Mass`, as Revit writes them, its footprint swept up from its base to its top; lofted,
            // a closed face set, with a property set to say which corner pairs with which.
            Element::Mass(ms) => {
                let Some(&(_, storey)) = storeys.get(&ms.level) else { continue };
                let (bottom, top) = m.span(ms.level, ms.base_offset, ms.top_level, ms.height);
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let shape = if ms.lofted() {
                    let skin = crate::massing::tris(&ms.footprint(), &ms.top_ring(), (ms.base_offset, ms.base_offset + top - bottom));
                    let set = w.faceset(&skin);
                    w.tessellated(&[set])
                } else {
                    let profile = w.profile(&ms.footprint(), &[]);
                    w.body(profile, ms.base_offset, top - bottom, &[])
                };
                let ent = w.add(format!(
                    "IFCBUILDINGELEMENTPROXY({},$,{},$,'Mass',#{at},#{shape},'{id}',.USERDEFINED.)",
                    w.guid(ELEMENT, id),
                    s(&ms.name)
                ));
                if ms.lofted() {
                    let loft = serde_json::json!({"bottom": ms.footprint(), "top": ms.top_ring()}).to_string();
                    let prop = w.text_property("Loft", None, &loft);
                    w.define(ent, LOFT, &[prop], (LOFTED, LOFTED_DEFINES), id);
                }
                contained.entry(ms.level).or_default().push(ent);
                products.insert(id, ent);
            }
            // A structural column and an isolated footing are written as a column is, a footing as an IfcFooting; what is
            // structural says it carries load.
            Element::Column(col) | Element::StructuralColumn(col) | Element::Footing(col) => {
                let (Some(&(_, storey)), Some(t)) = (storeys.get(&col.level), m.column_type(e)) else { continue };
                let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
                let at = w.place(Some(storey), (col.at.0, col.at.1, col.base_offset), Some(heading(col.angle)));
                let profile = if t.round {
                    let c = w.point(P2(0.0, 0.0));
                    let centre = w.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
                    w.add(format!("IFCCIRCLEPROFILEDEF(.AREA.,$,#{centre},{})", real(t.width / 2.0)))
                } else {
                    w.rectangle(P2(0.0, 0.0), P2(t.width, t.depth))
                };
                let shape = w.body(profile, 0.0, top - bottom, &[]);
                let (class, kind) =
                    if matches!(e, Element::Footing(_)) { ("IFCFOOTING", ".PAD_FOOTING.") } else { ("IFCCOLUMN", ".COLUMN.") };
                let ent = w.add(format!(
                    "{class}({},$,{},$,{},#{at},#{shape},'{id}',{kind})",
                    w.guid(ELEMENT, id),
                    s(&col.type_name),
                    s(&col.type_name)
                ));
                contained.entry(col.level).or_default().push(ent);
                products.insert(id, ent);
                match e {
                    Element::StructuralColumn(_) => w.load_bearing(ent, "Pset_ColumnCommon", id),
                    Element::Footing(_) => w.load_bearing(ent, "Pset_FootingCommon", id),
                    _ => {}
                }
            }
            // A run: its section swept along its axis, in its material, a member of the system it is in.
            crate::model::run_arm!(r) => {
                let (Some(&(_, storey)), Some(t), Some(g), Some((class, kind))) =
                    (storeys.get(&r.level), m.run_type(e), m.run_geom(e), crate::runs::ifc(e.category()))
                else {
                    continue;
                };
                let axis = crate::view3d::unit(crate::view3d::sub(g.b, g.a));
                // Placed at the start of its centre line; its section's x across it, level, its y up.
                let at = w.place(Some(storey), (g.a[0], g.a[1], g.a[2] - m.elevation(r.level)), None);
                let across = if g.vertical() { [1.0, 0.0, 0.0] } else { crate::view3d::unit([-axis[1], axis[0], 0.0]) };
                let profile = if g.round {
                    let c = w.point(P2(0.0, 0.0));
                    let centre = w.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
                    w.add(format!("IFCCIRCLEPROFILEDEF(.AREA.,$,#{centre},{})", real(g.width / 2.0)))
                } else {
                    w.rectangle(P2(0.0, 0.0), P2(g.width, g.depth))
                };
                let along = w.aim(axis, across);
                let shape = w.body_along(profile, along, g.length());
                let ent = w.add(format!(
                    "{class}({},$,{},$,{},#{at},#{shape},'{id}',{kind})",
                    w.guid(ELEMENT, id),
                    s(&r.type_name),
                    s(&r.type_name)
                ));
                contained.entry(r.level).or_default().push(ent);
                products.insert(id, ent);
                match e {
                    Element::Beam(_) => w.load_bearing(ent, "Pset_BeamCommon", id),
                    Element::StripFooting(_) => w.load_bearing(ent, "Pset_FootingCommon", id),
                    _ => {}
                }
                if !t.material.is_empty() {
                    let material = w.material(m, &t.material);
                    w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,(#{ent}),#{material})", w.guid(RUN_MATERIALS, id)));
                }
                if !r.system.is_empty() {
                    systems.entry(&r.system).or_default().push(ent);
                }
                // Where its heights are on its section, and which category it is, for a reader that finds only a class.
                let props = [w.text_property("Justification", None, r.justify.word()), w.text_property("Category", None, e.category())];
                w.define(ent, RUN, &props, (RUN_PROPERTIES, RUN_DEFINES), id);
            }
            // A component: its category's element, named by its family and type as Revit names them,
            // each form a closed face set styled in its material's colour.
            Element::Component(cp) => {
                let (Some(&(_, storey)), Some(f)) = (storeys.get(&cp.level), m.families.get(&cp.family)) else { continue };
                let pieces = m.component_local_pieces(cp);
                if pieces.is_empty() {
                    continue;
                }
                let at = w.place(Some(storey), (cp.at.0, cp.at.1, cp.offset), Some(heading(cp.angle)));
                let mut items = Vec::new();
                let mut made_of: Vec<u64> = Vec::new();
                for piece in &pieces {
                    let set = w.faceset(&piece.tris);
                    if !piece.material.trim().is_empty() {
                        let (material, style) = w.material_style(m, piece.material.trim());
                        if let Some(style) = style {
                            w.add(format!("IFCSTYLEDITEM(#{set},(#{style}),$)"));
                        }
                        if !made_of.contains(&material) {
                            made_of.push(material);
                        }
                    }
                    items.push(set);
                }
                let shape = w.tessellated(&items);
                // A door's or a window's family is placed in a wall, never as a component.
                let (entity, kind, object_type) = f.category.ifc();
                let name = s(&format!("{}:{}", cp.family, cp.type_name));
                let object = object_type.map_or_else(|| name.clone(), s);
                let ent = w.add(format!("{entity}({},$,{name},$,{object},#{at},#{shape},'{id}',{kind})", w.guid(ELEMENT, id)));
                contained.entry(cp.level).or_default().push(ent);
                products.insert(id, ent);
                // What it's made of: its material, or the set of them.
                let relating = match made_of.as_slice() {
                    [] => None,
                    [one] => Some(*one),
                    many => {
                        let parts: Vec<u64> = many.iter().map(|x| w.add(format!("IFCMATERIALCONSTITUENT($,$,#{x},$,$)"))).collect();
                        Some(w.add(format!("IFCMATERIALCONSTITUENTSET({},$,({}))", name, refs(&parts))))
                    }
                };
                if let Some(r) = relating {
                    w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,(#{ent}),#{r})", w.guid(COMPONENT_MATERIALS, id)));
                }
            }
            Element::Stair(st) => {
                let Some(&(_, storey)) = storeys.get(&st.level) else { continue };
                let (_, each, _) = m.flight(st);
                let (base, l) = (m.elevation(st.level), stairs::stair(m, st));
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let ent =
                    w.add(format!("IFCSTAIR({},$,'Stair',$,$,#{at},$,'{id}',{})", w.guid(ELEMENT, id), shape_of(&st.landings, "STAIR")));
                contained.entry(st.level).or_default().push(ent);
                products.insert(id, ent);
                // Its flights, each tread a block from the floor up to it, as in 3D; its landings.
                let mut parts = Vec::new();
                for (i, run) in l.runs.iter().enumerate() {
                    let fat = w.place(Some(at), (run.start.0, run.start.1, run.z.0 - base), Some(run.u));
                    let steps: Vec<(u64, f64, f64)> = (1..run.risers)
                        .map(|k| {
                            let k = f64::from(k);
                            (w.rectangle(P2((k - 0.5) * st.tread, 0.0), P2(st.tread, st.width)), base - run.z.0, run.z.0 - base + k * each)
                        })
                        .collect();
                    let shape = w.bodies(&steps, &[]);
                    parts.push(w.add(format!(
                        "IFCSTAIRFLIGHT({},$,'Flight',$,$,#{fat},#{shape},$,{},{},{},{},.STRAIGHT.)",
                        w.guid(FLIGHT, w.part(id, i)),
                        run.risers,
                        run.risers.saturating_sub(1),
                        real(each),
                        real(st.tread)
                    )));
                }
                parts.extend(w.landings(&l, at, base, id));
                wholes.insert(id, (ent, at, base, parts));
            }
            Element::Ramp(r) => {
                let Some(&(_, storey)) = storeys.get(&r.level) else { continue };
                let (base, l) = (m.elevation(r.level), stairs::ramp(m, r));
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let ent = w.add(format!("IFCRAMP({},$,'Ramp',$,$,#{at},$,'{id}',{})", w.guid(ELEMENT, id), shape_of(&r.landings, "RAMP")));
                contained.entry(r.level).or_default().push(ent);
                products.insert(id, ent);
                // Its flights, each solid from the floor up to its slope, as in 3D; its landings.
                let mut parts = Vec::new();
                for (i, run) in l.runs.iter().enumerate() {
                    let g = run.u * ((run.z.1 - run.z.0) / run.length.max(1e-9));
                    let top = walls::Plane { z: run.z.0 - g.dot(run.start), g };
                    let brep = w.prism(&run.ring(r.width), &walls::Plane::flat(base), &top, base);
                    let shape = w.solid(&[brep], &[]);
                    let fat = w.place(Some(at), (0.0, 0.0, 0.0), None);
                    parts.push(
                        w.add(format!("IFCRAMPFLIGHT({},$,'Flight',$,$,#{fat},#{shape},$,.STRAIGHT.)", w.guid(FLIGHT, w.part(id, i)))),
                    );
                }
                parts.extend(w.landings(&l, at, base, id));
                wholes.insert(id, (ent, at, base, parts));
            }
            // On a stair or a ramp, it's one of its parts; see below.
            Element::Railing(rl) if rl.host.is_none() => {
                let Some(&(_, storey)) = storeys.get(&rl.level) else { continue };
                let ent = w.railing(m, id, rl, storey, m.elevation(rl.level), axis);
                contained.entry(rl.level).or_default().push(ent);
                products.insert(id, ent);
            }
            Element::Roof(r) => {
                let (Some(&(_, storey)), pieces) = (storeys.get(&r.level), m.roof_slabs(r)) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let breps: Vec<u64> = pieces.iter().map(|(face, rise)| w.brep(face, m.elevation(r.level), *rise)).collect();
                let shape = w.solid(&breps, &[]);
                let kind = match (&r.extrusion, r.slope == 0.0, r.gable || !r.gables.is_empty()) {
                    (Some(x), ..) => [".SHED_ROOF.", ".GABLE_ROOF."].get(x.profile.len().saturating_sub(2)).unwrap_or(&".FREEFORM."),
                    (None, true, _) => ".FLAT_ROOF.",
                    (None, false, true) => ".GABLE_ROOF.",
                    (None, false, false) => ".HIP_ROOF.",
                };
                let ent = w.add(format!(
                    "IFCROOF({},$,{},$,{},#{at},#{shape},'{id}',{kind})",
                    w.guid(ELEMENT, id),
                    s(&r.type_name),
                    s(&r.type_name)
                ));
                contained.entry(r.level).or_default().push(ent);
                products.insert(id, ent);
                hosts.insert(id, (ent, at, m.elevation(r.level)));
            }
            _ => {}
        }
    }
    // Railings along stairs and ramps join their parts; each stair and ramp is the sum of its parts.
    for (&id, e) in &m.elements {
        if let Element::Railing(rl) = e
            && let Some((_, at, base, _)) = rl.host.and_then(|h| wholes.get(&m.elements.id(h)?)).cloned()
        {
            let ent = w.railing(m, id, rl, at, base, axis);
            products.insert(id, ent);
            if let Some((_, _, _, parts)) = rl.host.and_then(|h| wholes.get_mut(&m.elements.id(h)?)) {
                parts.push(ent);
            }
        }
    }
    for (id, (ent, _, _, parts)) in &wholes {
        w.add(format!("IFCRELAGGREGATES({},$,$,$,#{ent},({}))", w.guid(PARTS, *id), refs(parts)));
    }
    // A shaft voids each floor, ceiling and roof it passes through: an opening in each, its
    // outline swept up the shaft's whole height.
    for (&host, &(ent, at, z)) in &hosts {
        let slabs = match m.elements.get(host) {
            Some(Element::Floor(f)) => m.floor_slabs(f),
            Some(Element::Ceiling(c)) => m.ceiling_slabs(c),
            Some(Element::Roof(r)) => m.roof_slabs(r),
            _ => Vec::new(),
        };
        let mut shafts: Vec<Id> = slabs.iter().flat_map(|(face, rise)| m.less_shafts(face, *rise).1).collect();
        shafts.sort_unstable();
        shafts.dedup();
        for id in shafts {
            let Some(Element::Shaft(sh)) = m.elements.get(id) else { continue };
            let (bottom, top) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
            let key = mix(id) ^ host;
            let o_at = w.place(Some(at), (0.0, 0.0, bottom - z), None);
            let profile = w.profile(&sh.boundary, &[]);
            let shape = w.body(profile, 0.0, top - bottom, &[]);
            let opening = w.add(format!("IFCOPENINGELEMENT({},$,'Shaft',$,$,#{o_at},#{shape},'{id}',.OPENING.)", w.guid(SHAFT, key)));
            w.add(format!("IFCRELVOIDSELEMENT({},$,$,$,#{ent},#{opening})", w.guid(SHAFT_VOIDS, key)));
        }
    }
    for (&id, e) in &m.elements {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        let (Some(t), Some(wl), Some(&(wall, wall_at))) =
            (m.opening_type(e), m.wall(o.host), m.elements.id(o.host).and_then(|h| placed.get(&h)))
        else {
            continue;
        };
        // From its jamb toward the wall's start, square to the wall at its middle, in the wall's frame.
        let axis = Axis::of(wl);
        let (u, way) = (axis.tangent(0.0), axis.tangent(o.offset));
        let jamb = axis.at(o.offset, walls::centre(m, wl)) - way * (t.width / 2.0) - wl.start;
        let at = w.place(Some(wall_at), (jamb.dot(u), jamb.dot(u.perp()), o.sill), Some(P2(way.dot(u), way.dot(u.perp()))));
        // The hole reaches 10 mm past both faces so it cuts cleanly; through a curved wall, past its bow too.
        let bow = axis.radius().map_or(0.0, |r| {
            let inner = r - m.thickness(wl) / 2.0 - walls::centre(m, wl).abs();
            if inner > t.width / 2.0 { inner - (inner * inner - t.width * t.width / 4.0).sqrt() } else { t.width }
        });
        let profile = w.rectangle(P2(t.width / 2.0, 0.0), P2(t.width, m.thickness(wl) + 20.0 + 2.0 * bow));
        let shape = w.body(profile, 0.0, t.height, &[]);
        let opening = w.add(format!("IFCOPENINGELEMENT({},$,'Opening',$,$,#{at},#{shape},$,.OPENING.)", w.guid(OPENING, id)));
        w.add(format!("IFCRELVOIDSELEMENT({},$,$,$,#{wall},#{opening})", w.guid(VOIDS, id)));
        // The door or window itself: a 50 mm panel centred in the wall; of a family, its forms as a component's are,
        // closed face sets in their materials' colours, standing at the middle of the opening.
        let mut made_of: Vec<u64> = Vec::new();
        let (fill_at, shape) = match m.opening_local(e).filter(|_| o.family.is_some()) {
            Some((pieces, _)) => {
                let mut items = Vec::new();
                for piece in &pieces {
                    let set = w.faceset(&piece.tris);
                    if !piece.material.trim().is_empty() {
                        let (material, style) = w.material_style(m, piece.material.trim());
                        if let Some(style) = style {
                            w.add(format!("IFCSTYLEDITEM(#{set},(#{style}),$)"));
                        }
                        if !made_of.contains(&material) {
                            made_of.push(material);
                        }
                    }
                    items.push(set);
                }
                let rep = w.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','Tessellation',({}))", w.body, refs(&items)));
                (w.place(Some(at), (t.width / 2.0, 0.0, 0.0), None), w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))")))
            }
            None => {
                let profile = w.rectangle(P2(t.width / 2.0, 0.0), P2(t.width, 50.0));
                (w.place(Some(at), (0.0, 0.0, 0.0), None), w.body(profile, 0.0, t.height, &[]))
            }
        };
        let (h, wd) = (real(t.height), real(t.width));
        // Named `Family:Type` as Revit names them; a project's own type, by its name.
        let name = s(&o.family.as_ref().map_or_else(|| o.type_name.clone(), |f| format!("{f}:{}", o.type_name)));
        let ent = if matches!(e, Element::Door(_)) {
            let swing = if o.flip_hand { ".SINGLE_SWING_RIGHT." } else { ".SINGLE_SWING_LEFT." };
            w.add(format!(
                "IFCDOOR({},$,{name},$,{name},#{fill_at},#{shape},{},{h},{wd},.DOOR.,{swing},$)",
                w.guid(ELEMENT, id),
                s(&o.mark)
            ))
        } else {
            w.add(format!(
                "IFCWINDOW({},$,{name},$,{name},#{fill_at},#{shape},{},{h},{wd},.WINDOW.,.SINGLE_PANEL.,$)",
                w.guid(ELEMENT, id),
                s(&o.mark)
            ))
        };
        w.add(format!("IFCRELFILLSELEMENT({},$,$,$,#{opening},#{ent})", w.guid(FILLS, id)));
        // What a family's door or window is made of: its material, or the set of them.
        let relating = match made_of.as_slice() {
            [] => None,
            [one] => Some(*one),
            many => {
                let parts: Vec<u64> = many.iter().map(|x| w.add(format!("IFCMATERIALCONSTITUENT($,$,#{x},$,$)"))).collect();
                Some(w.add(format!("IFCMATERIALCONSTITUENTSET({},$,({}))", s(&o.type_name), refs(&parts))))
            }
        };
        if let Some(r) = relating {
            w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,(#{ent}),#{r})", w.guid(COMPONENT_MATERIALS, id)));
        }
        contained.entry(wl.level).or_default().push(ent);
        products.insert(id, ent);
    }
    // Compound types' layers: a layer set for each type used, each layer's material once, with its
    // colour for viewers to shade it in; for each wall type and line its walls stand on, the set's
    // use, from their exterior faces in.
    let layer_set = |w: &mut Step, name: &str, layers: &[Layer]| -> u64 {
        let mut ids = Vec::new();
        for l in layers {
            let material = match l.material.trim() {
                "" => "$".to_string(),
                named => format!("#{}", w.material(m, named)),
            };
            let category = match l.function {
                Function::Structure => "'LoadBearing'",
                Function::Thermal => "'Insulation'",
                Function::Finish => "'Finish'",
                Function::Substrate => "'Substrate'",
                Function::Membrane => "'Membrane'",
            };
            ids.push(w.add(format!("IFCMATERIALLAYER({material},{},$,$,$,{category},$)", real(l.thickness))));
        }
        w.add(format!("IFCMATERIALLAYERSET(({}),{},$)", refs(&ids), s(name)))
    };
    let mut sets: BTreeMap<&str, u64> = BTreeMap::new();
    let used: std::collections::BTreeSet<&str> = placed.keys().filter_map(|id| m.wall(*id)).map(|wl| wl.type_name.as_str()).collect();
    for (name, t) in m.types.wall.iter().filter(|(name, t)| !t.layers.is_empty() && used.contains(name.as_str())) {
        sets.insert(name, layer_set(&mut w, name, &t.layers));
    }
    // A compound floor's set, from its top down, for each type and how high its top is over its
    // level; a roof's or a ceiling's set as it is.
    let mut slab_uses: BTreeMap<(&str, &str, String), Vec<u64>> = BTreeMap::new();
    for (&id, &(ent, ..)) in &hosts {
        let (cat, name, offset) = match m.elements.get(id) {
            Some(Element::Floor(f)) => (
                "floor",
                f.type_name.as_str(),
                real(m.floor_top(f).at(f.boundary.first().copied().unwrap_or_default()) - m.elevation(f.level)),
            ),
            Some(Element::Roof(r)) => ("roof", r.type_name.as_str(), String::new()),
            Some(Element::Ceiling(c)) => ("ceiling", c.type_name.as_str(), String::new()),
            _ => continue,
        };
        if !m.type_layers(cat, name).is_empty() {
            slab_uses.entry((cat, name, offset)).or_default().push(ent);
        }
    }
    let mut slab_sets: BTreeMap<(&str, &str), u64> = BTreeMap::new();
    for (k, ((cat, name, offset), ents)) in slab_uses.into_iter().enumerate() {
        let set = match slab_sets.get(&(cat, name)) {
            Some(set) => *set,
            None => {
                let set = layer_set(&mut w, name, m.type_layers(cat, name));
                slab_sets.insert((cat, name), set);
                set
            }
        };
        let relating = if cat == "floor" { w.add(format!("IFCMATERIALLAYERSETUSAGE(#{set},.AXIS3.,.NEGATIVE.,{offset},$)")) } else { set };
        w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,({}),#{relating})", w.guid(SLAB_MATERIALS, k as u64), refs(&ents)));
    }
    let mut uses: BTreeMap<(&str, String), Vec<u64>> = BTreeMap::new();
    for (id, (ent, _)) in &placed {
        if let Some(wl) = m.wall(*id).filter(|wl| sets.contains_key(wl.type_name.as_str())) {
            uses.entry((wl.type_name.as_str(), real(walls::faces(m, wl).0))).or_default().push(*ent);
        }
    }
    for (k, ((name, offset), ents)) in uses.into_iter().enumerate() {
        let Some(set) = sets.get(name) else { continue };
        let usage = w.add(format!("IFCMATERIALLAYERSETUSAGE(#{set},.AXIS2.,.NEGATIVE.,{offset},$)"));
        w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,({}),#{usage})", w.guid(MATERIALS, k as u64), refs(&ents)));
    }
    // A system is an IfcSystem its runs are assigned to; its GlobalIds follow its name, so they stay when another is added.
    for (name, members) in &systems {
        let key = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
        let system = w.add(format!("IFCSYSTEM({},$,{},$,$)", w.guid(SYSTEMS, key), s(name)));
        w.add(format!("IFCRELASSIGNSTOGROUP({},$,$,$,({}),$,#{system})", w.guid(SYSTEM_MEMBERS, key), refs(members)));
    }
    for (level, (storey, _)) in &storeys {
        if let Some(items) = contained.get(level) {
            w.add(format!(
                "IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,({}),#{storey})",
                w.guid(CONTAINS, m.elements.id(*level).unwrap_or_default()),
                refs(items)
            ));
        }
        if let Some(items) = spaces.get(level) {
            w.add(format!(
                "IFCRELAGGREGATES({},$,$,$,#{storey},({}))",
                w.guid(SPACES, m.elements.id(*level).unwrap_or_default()),
                refs(items)
            ));
        }
    }
    // Each element's values of the project's parameters: a property set of its own, `Project
    // Parameters`, lengths in the file's millimetres.
    for (&id, &ent) in &products {
        let Some(e) = m.elements.get(id) else { continue };
        let Some(params) = e.params() else { continue };
        let props: Vec<u64> = params
            .iter()
            .filter_map(|(name, v)| {
                // The project's parameters for its category: a component's family's values are its family's.
                let p = m.parameters.get(name).filter(|p| p.categories.iter().any(|c| c == e.category()))?;
                let value = match (p.kind, v) {
                    (ParameterKind::Text, Value::String(t)) => format!("IFCTEXT({})", s(t)),
                    (ParameterKind::Number, v) => format!("IFCREAL({})", real(v.as_f64()?)),
                    (ParameterKind::Length, v) => format!("IFCLENGTHMEASURE({})", real(v.as_f64()?)),
                    (ParameterKind::YesNo, Value::Bool(b)) => format!("IFCBOOLEAN({})", if *b { ".T." } else { ".F." }),
                    _ => return None,
                };
                Some(w.add(format!("IFCPROPERTYSINGLEVALUE({},$,{value},$)", s(name))))
            })
            .collect();
        if !props.is_empty() {
            w.define(ent, PARAMETERS, &props, (PROPERTIES, DEFINES), id);
        }
    }
    // Each element's phase and design option, in a property set of its own. The first phase is no value in a
    // model; here an element holding another value names it as its `Phase Created` (said to be implied), for
    // another tool to read. A door or a window holds only what it says: one with no phase follows its wall.
    for (&id, &ent) in &products {
        let Some(e) = m.elements.get(id) else { continue };
        let Some(params) = e.params() else { continue };
        let held = |key: &str| params.get(key).and_then(Value::as_str);
        let [created, demolished, option] = [phasing::CREATED, phasing::DEMOLISHED, phasing::OPTION].map(held);
        if created.is_none() && demolished.is_none() && option.is_none() {
            continue;
        }
        let follows = matches!(e, Element::Door(_) | Element::Window(_));
        let first = m.phases.first().map(String::as_str).filter(|_| !follows);
        let props: Vec<u64> = [
            (phasing::CREATED, created.or(first), created.is_none().then_some(IMPLIED)),
            (phasing::DEMOLISHED, demolished, None),
            (phasing::OPTION, option, None),
        ]
        .into_iter()
        .filter_map(|(name, value, note)| Some(w.text_property(name, note, value?)))
        .collect();
        w.define(ent, PHASING, &props, (PHASED, PHASED_DEFINES), id);
    }
    // The project's phases, in order, when it uses any of it, with the phase filters, overrides and design option
    // sets that differ from what a project read from IFC starts with, as the JSON a project file holds them in. The
    // phases and filters a project in another language starts with are nothing it uses.
    // ponytail: JSON inside an IfcText, which only BIMCraft reads; IfcComplexProperty records if another tool needs them.
    let changed = [
        ("Phase Filters", !phasing::is_default_filters(&m.phase_filters), serde_json::to_string(&m.phase_filters)),
        ("Phase Overrides", !phasing::is_default_overrides(&m.phase_overrides), serde_json::to_string(&m.phase_overrides)),
        ("Design Option Sets", !m.option_sets.is_empty(), serde_json::to_string(&m.option_sets)),
    ];
    let own = (phasing::phases(m.language), phasing::filters(m.language));
    let used = phasing::any(m)
        || m.phases != own.0
        || m.phase_filters != own.1
        || !phasing::is_default_overrides(&m.phase_overrides)
        || !m.option_sets.is_empty();
    if used {
        let phases: Vec<String> = m.phases.iter().map(|p| format!("IFCTEXT({})", s(p))).collect();
        let mut props = vec![w.add(format!("IFCPROPERTYLISTVALUE('Phases',$,({}),$)", phases.join(",")))];
        for (name, _, json) in changed.into_iter().filter(|(_, changed, _)| *changed) {
            if let Ok(json) = json {
                props.push(w.text_property(name, None, &json));
            }
        }
        w.define(project, PHASING, &props, (PHASED, PHASED_DEFINES), 0);
    }

    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let app = s(&format!("BIMCraft {}", env!("CARGO_PKG_VERSION")));
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [DesignTransferView]'),'2;1');\n\
         FILE_NAME({},'{}',(''),(''),{app},{app},'');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
        s(&format!("{}.ifc", m.name)),
        utc(now),
        w.out
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn step_strings_escape_quotes_backslashes_and_other_scripts() {
        assert_eq!(s("it's"), "'it''s'");
        assert_eq!(s(r"a\b"), r"'a\\b'");
        assert_eq!(s("Área"), r"'\X2\00C1\X0\rea'");
        // Past the Basic Multilingual Plane, UCS-4: as IFC readers expect, not UTF-16's surrogates.
        assert_eq!(s("Tile 😀"), r"'Tile \X4\0001F600\X0\'");
        assert_eq!(s("x');#9=IFCWALL("), "'x'');#9=IFCWALL('", "a name can't end the string early");
    }

    #[test]
    fn reals_always_carry_a_decimal_point() {
        assert_eq!([real(1500.0), real(0.00001), real(-0.0), real(-200.5), real(f64::NAN)], ["1500.", "0.00001", "0.", "-200.5", "0."]);
    }

    #[test]
    fn global_ids_are_22_base64_characters_and_stable() {
        let a = guid(7, 5, 12);
        assert_eq!(a.len(), 22);
        assert!(matches!(a.as_bytes()[0], b'0'..=b'3'), "{a}");
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'$'), "{a}");
        assert_eq!(a, guid(7, 5, 12));
        let all: BTreeSet<String> = (0..2000).flat_map(|id| [guid(7, 5, id), guid(7, 6, id), guid(8, 5, id)]).collect();
        assert_eq!(all.len(), 6000);
    }

    #[test]
    fn timestamps_are_utc_iso_dates() {
        assert_eq!(utc(0), "1970-01-01T00:00:00");
        assert_eq!(utc(951_868_800), "2000-03-01T00:00:00");
        assert_eq!(utc(1_000_000_000), "2001-09-09T01:46:40");
    }

    fn sample_ifc() -> String {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        export(&s.model)
    }

    #[test]
    fn the_export_has_the_spatial_tree_and_every_element() {
        let f = sample_ifc();
        assert!(f.starts_with("ISO-10303-21;") && f.trim_end().ends_with("END-ISO-10303-21;"));
        assert!(f.contains("FILE_SCHEMA(('IFC4'));"));
        let n = |e: &str| f.matches(&format!("={e}(")).count();
        assert_eq!([n("IFCPROJECT"), n("IFCSITE"), n("IFCBUILDING"), n("IFCBUILDINGSTOREY")], [1, 1, 1, 2]);
        assert_eq!([n("IFCWALL"), n("IFCDOOR"), n("IFCWINDOW"), n("IFCSLAB"), n("IFCSPACE")], [7, 4, 5, 1, 4]);
        assert_eq!([n("IFCOPENINGELEMENT"), n("IFCRELVOIDSELEMENT"), n("IFCRELFILLSELEMENT")], [9, 9, 9]);
        assert!(f.contains("'Kitchen'"), "rooms keep their names");
    }

    #[test]
    fn components_export_as_their_categorys_elements_with_their_materials() {
        let mut s = Session::new();
        s.script(
            r#"component {"family": "Table", "type": "1600 x 900", "at": [2000, 3000], "angle": 30}
               component {"family": "WC", "at": [500, 0], "mirrored": true}"#,
        )
        .unwrap();
        let f = export(&s.model);
        let n = |e: &str| f.matches(&format!("={e}(")).count();
        assert_eq!([n("IFCFURNITURE"), n("IFCSANITARYTERMINAL")], [1, 1]);
        assert!(f.contains("'Table:1600 x 900'"), "named by family and type, as Revit names them");
        // Each form a closed face set: the table's top and four legs, the WC's three parts.
        assert_eq!(n("IFCTRIANGULATEDFACESET"), 8);
        // Each material once, its colour styling the forms made of it.
        assert_eq!(n("IFCMATERIAL"), 2);
        assert!(f.contains("IFCMATERIAL('Timber'") && f.contains("IFCMATERIAL('Porcelain'"));
        assert_eq!([n("IFCSTYLEDITEM"), n("IFCRELASSOCIATESMATERIAL")], [2 + 8, 2]);
        // In their storey.
        let table = f.lines().find(|l| l.contains("=IFCFURNITURE(")).and_then(|l| l.split('=').next()).unwrap().to_string();
        assert!(
            f.lines().any(|l| l.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE(") && l.contains(&format!("{table},"))
                || l.contains(&format!("{table})")))
        );
    }

    #[test]
    fn family_doors_and_windows_export_their_forms_as_face_sets_in_their_materials() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
               door {"at": [2000, 0], "family": "Single Door", "type": "800 x 2100"}
               window {"at": [8000, 3000], "family": "Casement Window"}
               door {"at": [5000, 0]}"#,
        )
        .unwrap();
        let f = export(&s.model);
        dangling(&f);
        let n = |e: &str| f.matches(&format!("={e}(")).count();
        assert_eq!([n("IFCDOOR"), n("IFCWINDOW"), n("IFCOPENINGELEMENT")], [2, 1, 3]);
        assert!(f.contains("'Single Door:800 x 2100'") && f.contains("'Casement Window:1200 x 1500'"), "named by family and type");
        assert!(f.contains("'Single 900 x 2100'"), "a project's own type, by its name");
        // The door's forms: its jambs, head, leaf and two handles; the window's: its frame's four, and two sashes (cut
        // round their panes) with a pane each.
        assert_eq!(n("IFCTRIANGULATEDFACESET"), 6 + 4 + 4);
        // Each material they're made of written once, with the families' door and window sharing Timber; each of the two made of its set.
        for material in ["Timber", "Metal", "Glass"] {
            assert!(f.contains(&format!("IFCMATERIAL('{material}'")), "{material}");
        }
        assert_eq!(n("IFCRELASSOCIATESMATERIAL"), 2, "the family door and the family window; the plain door has none");
    }

    #[test]
    fn compound_curved_and_attached_walls_export_their_layers_and_shapes() {
        let mut s = Session::new();
        s.script(
            r#"type {"category": "wall", "name": "Cavity", "layers": [{"function": "finish", "material": "Brick", "thickness": 100}, {"function": "structure", "material": "Concrete block", "thickness": 200}]}
               wall {"points": [[0, 0], [0, 6000], [8000, 6000], [8000, 0]], "type": "Cavity"}
               wall {"points": [[8000, 0], [0, 0]], "arc": -60, "type": "Cavity", "location": "interior-finish"}
               window {"at": [4000, -1100]}
               roof {"gable": true, "boundary": [[-600, -1800], [8600, -1800], [8600, 6600], [-600, 6600]]}
               modify {"ids": [100002, 100004], "attach_top": 100007}
               door {"at": [4000, 6000]}"#,
        )
        .unwrap();
        let f = export(&s.model);
        let n = |e: &str| f.matches(&format!("={e}(")).count();
        assert_eq!([n("IFCMATERIAL"), n("IFCMATERIALLAYER"), n("IFCMATERIALLAYERSET")], [2, 2, 1]);
        // One use for the walls on their centre lines, one for the curved one on its interior face.
        assert_eq!([n("IFCMATERIALLAYERSETUSAGE"), n("IFCRELASSOCIATESMATERIAL")], [2, 2]);
        assert!(
            f.contains("IFCMATERIALLAYERSETUSAGE(") && f.contains(".AXIS2.,.NEGATIVE.,150.,$)") && f.contains(".AXIS2.,.NEGATIVE.,300.,$)")
        );
        assert!(f.contains("IFCMATERIALLAYER(#") && f.contains("'LoadBearing'") && f.contains("'Brick'"));
        // The walls attached to the roof, and the roof, as closed shells; the others swept up.
        assert_eq!(f.matches("'Body','Brep'").count(), 3);
        assert_eq!([n("IFCWALL"), n("IFCOPENINGELEMENT"), n("IFCWINDOW")], [4, 2, 1]);
        // Every hole, through a straight wall or a curved one, has a size.
        for line in f.lines().filter(|l| l.contains("=IFCRECTANGLEPROFILEDEF(")) {
            assert!(!line.contains(",0.)") && !line.contains(",0.,"), "{line}");
        }
        dangling(&f);
    }

    /// Every reference in `f` points at an entity written once.
    fn dangling(f: &str) {
        let mut defined = BTreeSet::new();
        let mut referenced = BTreeSet::new();
        for line in f.lines().filter(|l| l.starts_with('#')) {
            let (head, body) = line.split_once('=').unwrap();
            assert!(defined.insert(head[1..].parse::<u64>().unwrap()), "{head} written twice");
            for r in body.split('#').skip(1) {
                referenced.insert(r.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap());
            }
        }
        let dangling: Vec<_> = referenced.difference(&defined).collect();
        assert!(dangling.is_empty(), "dangling references: {dangling:?}");
    }

    #[test]
    fn every_reference_points_at_an_entity_written_once() {
        let f = sample_ifc();
        let mut defined = BTreeSet::new();
        let mut referenced = BTreeSet::new();
        for line in f.lines().filter(|l| l.starts_with('#')) {
            let (head, body) = line.split_once('=').unwrap();
            assert!(defined.insert(head[1..].parse::<u64>().unwrap()), "{head} written twice");
            for r in body.split('#').skip(1) {
                referenced.insert(r.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap());
            }
        }
        let dangling: Vec<_> = referenced.difference(&defined).collect();
        assert!(dangling.is_empty(), "dangling references: {dangling:?}");
    }

    /// The numbers of the entities of kind `kind` in `f`, in the order they're written.
    fn of_kind(f: &str, kind: &str) -> Vec<u64> {
        f.lines().filter(|l| l.contains(&format!("={kind}("))).filter_map(|l| l[1..].split('=').next()?.parse().ok()).collect()
    }

    /// The properties of the `BIMCraft_Phasing` set of entity `#ent`, as written, in the set's order; none when it has no such set.
    fn phasing_of(f: &str, ent: u64) -> Vec<String> {
        let body = |n: &str| -> String {
            let line = f.lines().find(|l| l.starts_with(&format!("#{n}="))).unwrap_or_else(|| panic!("no #{n}"));
            line.split_once('=').unwrap().1.trim_end_matches(';').to_string()
        };
        for rel in f.lines().filter(|l| l.contains("=IFCRELDEFINESBYPROPERTIES(")) {
            // `#7=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#12,#13),#40);`
            let (related, set) = rel.split_once("$,$,$,(").unwrap().1.split_once("),#").unwrap();
            if !related.split(',').any(|r| r == format!("#{ent}")) {
                continue;
            }
            let set = body(set.trim_end_matches(");").trim_end_matches(';').trim_end_matches(')'));
            if !set.contains(&format!("'{PHASING}'")) {
                continue;
            }
            let props = set.rsplit_once(",(").unwrap().1.trim_end_matches(')');
            return props.split(',').map(|p| body(p.trim_start_matches('#'))).collect();
        }
        Vec::new()
    }

    /// A text property as BIMCraft writes one: `note` is its description, `$` for none.
    fn text_property(name: &str, note: &str, value: &str) -> String {
        format!("IFCPROPERTYSINGLEVALUE('{name}',{note},IFCTEXT('{value}'),$)")
    }

    #[test]
    fn phases_and_design_options_go_out_in_a_property_set_of_each_element_that_holds_one() {
        use crate::graphics::testing::{house, run};
        let implied = s(IMPLIED);
        let mut s = Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        for (id, held) in [
            (walls[1], json!({"Phase Created": "New Construction"})),
            (walls[2], json!({"Phase Demolished": "Demolition", "Design Option": "Facade: Glass"})),
            (walls[3], json!({"Phase Created": "Existing"})),
            (door, json!({"Phase Demolished": "Demolition"})),
        ] {
            run(&mut s, "modify", json!({"id": id, "params": held}));
        }
        let f = export(&s.model);
        let wall = of_kind(&f, "IFCWALL");
        assert_eq!(phasing_of(&f, wall[0]), Vec::<String>::new(), "made in the first phase and in no option: nothing to say");
        assert_eq!(phasing_of(&f, wall[1]), [text_property("Phase Created", "$", "New Construction")]);
        assert_eq!(
            phasing_of(&f, wall[2]),
            [
                text_property("Phase Created", &implied, "Existing"),
                text_property("Phase Demolished", "$", "Demolition"),
                text_property("Design Option", "$", "Facade: Glass"),
            ],
            "the first phase is named, and said to be implied"
        );
        assert_eq!(phasing_of(&f, wall[3]), [text_property("Phase Created", "$", "Existing")], "held, so not implied");
        // A door says only what it holds: naming a phase it holds none of would stop it following its wall.
        assert_eq!(phasing_of(&f, of_kind(&f, "IFCDOOR")[0]), [text_property("Phase Demolished", "$", "Demolition")]);
        // The project's phases in order, for another tool to read the elements' by, and its sets.
        let project = phasing_of(&f, of_kind(&f, "IFCPROJECT")[0]);
        assert_eq!(
            project,
            [
                "IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existing'),IFCTEXT('Demolition'),IFCTEXT('New Construction')),$)".to_string(),
                text_property("Design Option Sets", "$", r#"{"Facade":{"options":["Brick","Glass"],"primary":"Brick"}}"#),
            ],
            "its filters and overrides are the usual ones, left out"
        );
        assert_eq!(f.matches(&format!("'{PHASING}'")).count(), 3 + 1 + 1, "three walls and the door hold a value; and the project");
        dangling(&f);
    }

    #[test]
    fn a_projects_own_phase_filters_and_overrides_go_out_as_text_on_the_project() {
        use crate::graphics::testing::run;
        let filter = json!({"name": "Renovation", "new": "overridden", "existing": "by-category", "demolished": "not-displayed", "temporary": "not-displayed"});
        let overrides = json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}});
        // No element holds a value: the project's own filters, its own overrides, or both, are what it says.
        for (filters, own, says) in [
            (false, true, vec!["Phases", "Phase Overrides"]),
            (true, false, vec!["Phases", "Phase Filters"]),
            (true, true, vec!["Phases", "Phase Filters", "Phase Overrides"]),
        ] {
            let mut s = Session::new();
            if filters {
                run(&mut s, "phase_filter", filter.clone());
            }
            if own {
                run(&mut s, "phase", overrides.clone());
            }
            let f = export(&s.model);
            let project = phasing_of(&f, of_kind(&f, "IFCPROJECT")[0]);
            let names: Vec<&str> = project.iter().map(|p| p.split('\'').nth(1).unwrap()).collect();
            assert_eq!(names, says);
            if let Some(p) = project.iter().find(|p| p.contains("'Phase Filters'")) {
                assert!(p.contains(
                    r#""Renovation":{"new":"overridden","existing":"by-category","demolished":"not-displayed","temporary":"not-displayed"}"#
                ));
                assert!(p.contains(r#""Show All":"#), "all of them, the usual ones too");
            }
            if let Some(p) = project.iter().find(|p| p.contains("'Phase Overrides'")) {
                assert!(p.contains(r#""new":{"cut":{"color":[0,0,200]}}"#), "{p}");
            }
            dangling(&f);
        }
    }

    #[test]
    fn the_phases_are_listed_when_an_element_holds_one_or_the_project_has_phases_of_its_own() {
        use crate::graphics::testing::{house, run};
        let on_project = |s: &Session| {
            let f = export(&s.model);
            phasing_of(&f, of_kind(&f, "IFCPROJECT")[0])
        };
        // Only its phases: its filters, overrides and sets are the usual ones, or none.
        let mut s = Session::new();
        let walls = house(&mut s);
        assert_eq!(on_project(&s), Vec::<String>::new());
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": "New Construction"}}));
        assert_eq!(on_project(&s), ["IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existing'),IFCTEXT('New Construction')),$)"]);
        // A phase of its own, with nothing in it yet.
        let mut t = Session::new();
        run(&mut t, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        assert_eq!(
            on_project(&t),
            ["IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existing'),IFCTEXT('Demolition'),IFCTEXT('New Construction')),$)"]
        );
    }

    #[test]
    fn a_project_in_another_language_that_uses_no_phasing_says_nothing_of_it() {
        use crate::graphics::testing::{house, run};
        let mut s = Session::new();
        run(&mut s, "new", json!({"language": "pt"}));
        let walls = house(&mut s);
        assert!(!export(&s.model).contains(PHASING), "its phases and filters are its language's, as a new project has them");
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": "Nova construção"}}));
        let f = export(&s.model);
        let project = phasing_of(&f, of_kind(&f, "IFCPROJECT")[0]);
        let names: Vec<&str> = project.iter().map(|p| p.split('\'').nth(1).unwrap()).collect();
        // What a project read from IFC starts with is in English, so its filters go too, or they'd come back in English.
        assert_eq!(names, ["Phases", "Phase Filters"]);
        assert_eq!(
            project[0],
            r"IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existente'),IFCTEXT('Nova constru\X2\00E7\X0\\X2\00E3\X0\o')),$)"
        );
    }

    #[test]
    fn a_project_that_uses_no_phasing_says_nothing_of_it() {
        use crate::graphics::testing::{house, run};
        assert!(!sample_ifc().contains(PHASING), "the sample doesn't use phases, so its file has no set for them");
        // A phase named, which is no new value, or a view that shows one, doesn't count as using them.
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing", "phase_filter": "Show New"}));
        assert!(!export(&s.model).contains(PHASING));
        // An option set does.
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        assert_eq!(export(&s.model).matches(&format!("'{PHASING}'")).count(), 1);
    }
}
