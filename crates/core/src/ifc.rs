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

use bimcraft_i18n::ui_msg;

pub mod classes;

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::family::ConnectorKind;
use crate::geom::{self, P2};
use serde_json::Value;

use crate::model::{Element, Function, Guid, Id, Layer, Model, Operation, ParameterKind, Railing, RoomArea, heading, mix};
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
const QUANTITIES: u64 = 49;
const QUANTITY_DEFINES: u64 = 50;
const TYPES: u64 = 51;
const TYPE_DEFINES: u64 = 52;
const DEVICE: u64 = 53;
const DEVICE_DEFINES: u64 = 54;
const CONNECTOR: u64 = 55;
const CONNECTOR_DEFINES: u64 = 56;
const TYPE_MATERIALS: u64 = 57;
const USAGE_MATERIALS: u64 = 58;
const PANEL_PROPERTIES: u64 = 59;
const PANEL_DEFINES: u64 = 60;
const WALLED: u64 = 61;
const WALLED_DEFINES: u64 = 62;
const DISTRIBUTION_SERVES: u64 = 63;
const DISTRIBUTION: u64 = 71;
const DISTRIBUTION_MEMBERS: u64 = 72;

/// The property set an element's values of the project's parameters go in.
pub const PARAMETERS: &str = "Project Parameters";

/// The property set a run's justification and category go in (its system is an IfcSystem it is assigned to).
pub const RUN: &str = "BIMCraft_Run";

/// The property set a distribution board's data goes in: `Mains` (amperes), `Slots`, `DistributionSystem` (its name) and `SupplyFrom`
/// (the GlobalId of the board that supplies it). The system is also an IfcDistributionSystem the board is assigned to, which is
/// what another tool reads it from.
pub const PANEL: &str = "BIMCraft_Panel";

/// The property set a lofted mass records how its footprint and its top outline pair up in, corner for corner (the body is
/// a closed face set that doesn't say): `Loft`, JSON `{bottom, top}`.
pub const LOFT: &str = "BIMCraft_Mass";

/// A wall's authored location line (`Location`), and the GlobalIds of the roof or floor its top and base follow
/// (`AttachTop`, `AttachBase`). Its Axis representation lies on that location line.
pub const WALL: &str = "BIMCraft_Wall";

/// The object type a room separation line's virtual element carries, which the import reads it back by.
pub const SEPARATION: &str = "RoomSeparation";

/// The property set what a placed device's connector says that IFC's `Pset_ElectricalDeviceCommon` has no property for goes
/// in: its load in VA (in all, and `LoadVAPhase1`… for each phase), its system and its classification.
pub const CONNECTOR_SET: &str = "BIMCraft_Connector";

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
    /// What goes in each element's `Pset_*Common`: its entity, the set's name and its properties, written once at the end.
    common: BTreeMap<Id, (u64, &'static str, Vec<u64>)>,
    /// The occurrences of each type object, by its key (see `TypeKey`). Written once at the end.
    typed: BTreeMap<TypeKey, TypeBuild>,
    /// The profile definition of each section written, shared by the material profile set of the type that has it and the bodies of
    /// its occurrences.
    sections: BTreeMap<Section, u64>,
}

/// A member's section as its type gives it: `width` and `depth` in mm (a round section is `width` across, so its depth is
/// its width), kept as bits so a section can key a map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Section {
    width: u64,
    depth: u64,
    round: bool,
}

impl Section {
    fn new(width: f64, depth: f64, round: bool) -> Section {
        Section { width: width.to_bits(), depth: if round { width } else { depth }.to_bits(), round }
    }

    fn width(self) -> f64 {
        f64::from_bits(self.width)
    }

    fn depth(self) -> f64 {
        f64::from_bits(self.depth)
    }
}

/// What a type object's material profile set holds: its section, and the name of its material (empty for none: a column's type
/// has no material).
type Member = (Section, String);

/// A type object's key: its IFC class, what comes ahead of and after its predefined type (each with its comma), its name and, for
/// a beam's, column's or footing's, its member (two types of one name and two sections are two type objects).
type TypeKey = (String, String, String, String, Option<Member>);

/// A type object as the export builds it: the predefined type its occurrences share, their ids, and which of them use its
/// material profile set by each cardinal point.
struct TypeBuild {
    predefined: String,
    occurrences: Vec<u64>,
    usages: BTreeMap<u8, Vec<u64>>,
}

/// The cardinal point (IFC4's `IfcCardinalPointReference`) a beam is placed by: its `justify` line, the top (8), the middle (5)
/// or the bottom (2) of its section, which an upright one, with no top or bottom, has in the middle.
fn cardinal_point(justify: crate::model::Justify, upright: bool) -> u8 {
    use crate::model::Justify;
    match justify {
        _ if upright => 5,
        Justify::Top => 8,
        Justify::Centre => 5,
        Justify::Bottom => 2,
    }
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

    /// Says `ent` is an occurrence of the type object `class` (with its `predefined` type) named `name`.
    fn typed(&mut self, class: &str, predefined: &str, name: &str, ent: u64) {
        self.typed_with(class, "", predefined, "", name, ent);
    }

    /// As `typed`, for a type with attributes beside its PredefinedType: `before` it and `after` it, each with its comma (a
    /// furniture type's assembly place comes before; a door's operation after). Occurrences whose predefined types differ make
    /// their type's NOTDEFINED: a roof type is flat only when every roof of it is. An empty `predefined` is a class that has none.
    fn typed_with(&mut self, class: &str, before: &str, predefined: &str, after: &str, name: &str, ent: u64) {
        let key = (class.to_string(), before.to_string(), after.to_string(), name.to_string(), None);
        self.build(key, predefined).occurrences.push(ent);
    }

    /// As `typed`, for a beam, a column or a footing: its type holds `member` (its section and material) as a material profile
    /// set, and `ent` uses that set, placed by `cardinal` (an `IfcCardinalPointReference`).
    fn typed_member(&mut self, class: &str, predefined: &str, name: &str, member: Member, cardinal: u8, ent: u64) {
        let key = (class.to_string(), String::new(), String::new(), name.to_string(), Some(member));
        let build = self.build(key, predefined);
        build.occurrences.push(ent);
        build.usages.entry(cardinal).or_default().push(ent);
    }

    /// The type object `key`'s build, new with `predefined`; occurrences whose predefined types differ make it NOTDEFINED.
    fn build(&mut self, key: TypeKey, predefined: &str) -> &mut TypeBuild {
        let build = self.typed.entry(key).or_insert_with(|| TypeBuild {
            predefined: predefined.to_string(),
            occurrences: Vec::new(),
            usages: BTreeMap::new(),
        });
        if build.predefined != predefined {
            build.predefined = ".NOTDEFINED.".into();
        }
        build
    }

    /// Writes each type object once, with its `IfcRelDefinesByType` to its occurrences and, for a beam's, column's or footing's,
    /// its material profile set (one `IfcMaterialProfile`: its material and its section) with the `IfcMaterialProfileSetUsage`
    /// its occurrences use, one for each cardinal point. All the GlobalIds come from the class, the name and the member, so they
    /// are the same on every export.
    fn write_types(&mut self, m: &Model) {
        for ((class, before, after, name, member), TypeBuild { predefined, occurrences, usages }) in std::mem::take(&mut self.typed) {
            let member_text = member.as_ref().map(|x| format!("{x:?}")).unwrap_or_default();
            let key = name
                .bytes()
                .chain(class.bytes())
                .chain(member_text.bytes())
                .fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3));
            // Six attributes after the name (description to element type), then `before`, the predefined type and `after`.
            let predefined = if predefined.is_empty() { predefined } else { format!(",{predefined}") };
            let kind = self.add(format!("{class}({},$,{},$,$,$,$,$,${before}{predefined}{after})", self.guid(TYPES, key), s(&name)));
            self.add(format!("IFCRELDEFINESBYTYPE({},$,$,$,({}),#{kind})", self.guid(TYPE_DEFINES, key), refs(&occurrences)));
            let Some((section, material)) = member else { continue };
            let profile = self.section_profile(section);
            let material = if material.is_empty() { "$".to_string() } else { format!("#{}", self.material(m, &material)) };
            let entry = self.add(format!("IFCMATERIALPROFILE({},$,{material},#{profile},$,$)", s(&name)));
            let set = self.add(format!("IFCMATERIALPROFILESET({},$,(#{entry}),$)", s(&name)));
            self.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,(#{kind}),#{set})", self.guid(TYPE_MATERIALS, key)));
            for (cardinal, users) in usages {
                let usage = self.add(format!("IFCMATERIALPROFILESETUSAGE(#{set},{cardinal},$)"));
                let relation = self.guid(USAGE_MATERIALS, key.wrapping_add(u64::from(cardinal)));
                self.add(format!("IFCRELASSOCIATESMATERIAL({relation},$,$,$,({}),#{usage})", refs(&users)));
            }
        }
    }

    /// Quantity set `name` of `(name, kind, value)` for `ent`: kind `LENGTH` in mm, `AREA` in m², `VOLUME` in m³.
    fn quantities(&mut self, ent: u64, name: &str, values: &[(&str, &str, f64)], key: u64) {
        let qs: Vec<u64> = values.iter().map(|(n, kind, v)| self.add(format!("IFCQUANTITY{kind}({},$,$,{},$)", s(n), real(*v)))).collect();
        let set = self.add(format!("IFCELEMENTQUANTITY({},$,{},$,$,({}))", self.guid(QUANTITIES, key), s(name), refs(&qs)));
        self.add(format!("IFCRELDEFINESBYPROPERTIES({},$,$,$,(#{ent}),#{set})", self.guid(QUANTITY_DEFINES, key)));
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
            // How see-through the material is, 0 for opaque to 1 for clear; none for an opaque one.
            let clear = if x.transparency == 0 { "$".to_string() } else { real(f64::from(x.transparency.min(100)) / 100.0) };
            let style = self.surface_style(&s(name), x.color, &clear);
            let item = self.add(format!("IFCSTYLEDITEM($,(#{style}),$)"));
            let rep = self.add(format!("IFCSTYLEDREPRESENTATION(#{},'Style','Material',(#{item}))", self.context));
            self.add(format!("IFCMATERIALDEFINITIONREPRESENTATION($,$,(#{rep}),#{id})"));
            style
        });
        self.materials.insert(name.to_string(), (id, style));
        (id, style)
    }

    /// A surface style named `name` (a STEP string, or `$` for none) that shades in `rgb`, `clear` how see-through (`$`
    /// for opaque).
    fn surface_style(&mut self, name: &str, rgb: [u8; 3], clear: &str) -> u64 {
        let [r, g, b] = rgb.map(|c| real(f64::from(c) / 255.0));
        let colour = self.add(format!("IFCCOLOURRGB($,{r},{g},{b})"));
        let shading = self.add(format!("IFCSURFACESTYLESHADING(#{colour},{clear})"));
        self.add(format!("IFCSURFACESTYLE({name},.BOTH.,(#{shading}))"))
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

    /// The profile definition of `section` (a circle of half its width, or a rectangle, centred on the origin), written once: the
    /// material profile set of a type and the bodies of its occurrences share it.
    fn section_profile(&mut self, section: Section) -> u64 {
        if let Some(&written) = self.sections.get(&section) {
            return written;
        }
        let profile = if section.round {
            let c = self.point(P2(0.0, 0.0));
            let centre = self.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
            self.add(format!("IFCCIRCLEPROFILEDEF(.AREA.,$,#{centre},{})", real(section.width() / 2.0)))
        } else {
            self.rectangle(P2(0.0, 0.0), P2(section.width(), section.depth()))
        };
        self.sections.insert(section, profile);
        profile
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
    fn load_bearing(&mut self, ent: u64, pset: &'static str, id: Id) {
        let load = self.add("IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)");
        self.common.entry(id).or_insert((ent, pset, Vec::new())).2.push(load);
    }

    /// A body: `profile` swept `depth` up from `bottom`, plus any extra representations.
    fn body(&mut self, profile: u64, bottom: f64, depth: f64, extra: &[u64]) -> u64 {
        self.bodies(&[(profile, bottom, depth)], extra, None)
    }

    /// A body of solids, each a profile swept `depth` up from `bottom` and styled in `style` when there's one, plus any
    /// extra representations.
    fn bodies(&mut self, solids: &[(u64, f64, f64)], extra: &[u64], style: Option<u64>) -> u64 {
        let items: Vec<u64> = solids
            .iter()
            .map(|&(profile, bottom, depth)| {
                let at = if bottom == 0.0 { self.world } else { self.axes((0.0, 0.0, bottom), None) };
                let solid = self.add(format!("IFCEXTRUDEDAREASOLID(#{profile},#{at},#{},{})", self.z, real(depth)));
                if let Some(style) = style {
                    self.add(format!("IFCSTYLEDITEM(#{solid},(#{style}),$)"));
                }
                solid
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

    /// A closed solid's shell (`model::Polyhedron`): each corner written once, so its faces share their edges; heights
    /// from `base`.
    fn polyhedron(&mut self, (corners, faces): &crate::model::Polyhedron, base: f64) -> u64 {
        let pts: Vec<u64> =
            corners.iter().map(|p| self.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(p[0]), real(p[1]), real(p[2] - base)))).collect();
        let faces: Vec<u64> = faces
            .iter()
            .map(|loops| {
                let bounds: Vec<u64> = loops
                    .iter()
                    .enumerate()
                    .map(|(k, l)| {
                        let refs_: Vec<u64> = l.iter().filter_map(|&i| pts.get(i).copied()).collect();
                        let lp = self.add(format!("IFCPOLYLOOP(({}))", refs(&refs_)));
                        let kind = if k == 0 { "IFCFACEOUTERBOUND" } else { "IFCFACEBOUND" };
                        self.add(format!("{kind}(#{lp},.T.)"))
                    })
                    .collect();
                self.add(format!("IFCFACE(({}))", refs(&bounds)))
            })
            .collect();
        let shell = self.add(format!("IFCCLOSEDSHELL(({}))", refs(&faces)));
        self.add(format!("IFCFACETEDBREP(#{shell})"))
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

/// The `Pset_*Common` an element's IFC entity carries; none for what has none BIMCraft writes.
fn common_pset(e: &Element) -> Option<&'static str> {
    Some(match e {
        Element::Wall(_) => "Pset_WallCommon",
        Element::Floor(_) => "Pset_SlabCommon",
        Element::Roof(_) => "Pset_RoofCommon",
        Element::Ceiling(_) => "Pset_CoveringCommon",
        Element::Door(_) => "Pset_DoorCommon",
        Element::Window(_) => "Pset_WindowCommon",
        Element::Column(_) | Element::StructuralColumn(_) => "Pset_ColumnCommon",
        Element::Beam(_) => "Pset_BeamCommon",
        Element::Footing(_) | Element::StripFooting(_) => "Pset_FootingCommon",
        Element::Stair(_) => "Pset_StairCommon",
        Element::Ramp(_) => "Pset_RampCommon",
        Element::Railing(_) => "Pset_RailingCommon",
        _ => return None,
    })
}

/// What the work does to an element, in IFC's words for `Pset_*Common.Status`.
fn ifc_status(m: &Model, e: &Element) -> Option<&'static str> {
    e.params()?;
    let at = |key| phasing::value(m, e, key).and_then(|n| phasing::index(m, n));
    let created = at(phasing::CREATED).unwrap_or(0);
    Some(match at(phasing::DEMOLISHED) {
        Some(d) if d == created => "TEMPORARY",
        Some(_) => "DEMOLISH",
        None if created == 0 => "EXISTING",
        None => "NEW",
    })
}

/// The length round a closed ring, mm.
fn perimeter(ring: &[P2]) -> f64 {
    ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| a.dist(*b)).sum()
}

/// What a beam or a column measures, for its `Qto_BeamBaseQuantities` or `Qto_ColumnBaseQuantities`: its length (a column's
/// height), the area of its section (m²), its outer surface (m²) and its volume (m³). The outer surface is the sides only,
/// perimeter by length, without the two ends, as IFC 4.3 defines it for these sets. IFC gives the gross surface (with the
/// ends) only when the outer surface and the section can't be given apart, so it isn't written: a reader adds two sections.
/// Nothing is subtracted from it, so it has no net values: nothing is taken off where a beam meets a column or a slab yet.
fn member_quantities(width: f64, depth: f64, round: bool, length: f64) -> Vec<(&'static str, &'static str, f64)> {
    let (area, perimeter) = if round {
        (std::f64::consts::PI * width * width / 4.0, std::f64::consts::PI * width)
    } else {
        (width * depth, 2.0 * (width + depth))
    };
    vec![
        ("Length", "LENGTH", length),
        ("CrossSectionArea", "AREA", area / 1e6),
        ("OuterSurfaceArea", "AREA", perimeter * length / 1e6),
        ("GrossVolume", "VOLUME", area * length / 1e9),
    ]
}

/// What a wall measures, for its `Qto_WallBaseQuantities`: its length along its axis, its thickness and height, its side
/// (length by height, m²) and that less the doors and windows in it (`holes`, as (width, height, sill), each cut to the
/// wall's height), and both through its thickness (m³), its joins aside. A wall attached to a roof or a floor has no single
/// height, so only its length and thickness.
fn wall_quantities(m: &Model, wl: &crate::model::Wall, holes: &[(f64, f64, f64)]) -> Vec<(&'static str, &'static str, f64)> {
    let (length, width) = (Axis::of(wl).length(), m.thickness(wl));
    let mut values = vec![("Length", "LENGTH", length), ("Width", "LENGTH", width)];
    if wl.attach_top.is_none() && wl.attach_base.is_none() {
        let height = m.top_z(wl) - m.base_z(wl);
        let gross = length * height;
        let cut: f64 = holes.iter().map(|&(wd, h, sill)| wd.min(length) * h.min(height - sill).max(0.0)).sum();
        let net = (gross - cut).max(0.0);
        values.extend([
            ("Height", "LENGTH", height),
            ("GrossSideArea", "AREA", gross / 1e6),
            ("NetSideArea", "AREA", net / 1e6),
            ("GrossVolume", "VOLUME", gross * width / 1e9),
            ("NetVolume", "VOLUME", net * width / 1e9),
        ]);
    }
    values
}

pub fn export(m: &Model) -> String {
    let rooms = crate::rooms::Rooms::new(m);
    let own = m.elements.with_guids().map(|(id, g, _)| (id, g)).collect();
    let mut w = Step {
        out: String::new(),
        next: 0,
        uid: m.uid,
        own,
        world: 0,
        z: 0,
        context: 0,
        body: 0,
        materials: BTreeMap::new(),
        common: BTreeMap::new(),
        typed: BTreeMap::new(),
        sections: BTreeMap::new(),
    };
    let origin = w.add("IFCCARTESIANPOINT((0.,0.,0.))");
    w.z = w.add("IFCDIRECTION((0.,0.,1.))");
    w.world = w.add(format!("IFCAXIS2PLACEMENT3D(#{origin},$,$)"));
    let ctx = w.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#{},$)", w.world));
    w.context = ctx;
    w.body = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{ctx},$,.MODEL_VIEW.,$)"));
    let axis = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Axis','Model',*,*,*,*,#{ctx},$,.GRAPH_VIEW.,$)"));
    // Where it lies on a map: a projected CRS in metres, and the model context's map conversion to it, its scale taking the
    // project's millimetres to them.
    if let Some(g) = &m.georeference {
        let metre = w.add("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)");
        let or_none = |t: &Option<String>| t.as_deref().map_or_else(|| "$".to_string(), s);
        let crs = w.add(format!(
            "IFCPROJECTEDCRS({},{},{},{},{},{},#{metre})",
            s(&g.crs),
            or_none(&g.description),
            or_none(&g.geodetic_datum),
            or_none(&g.vertical_datum),
            or_none(&g.map_projection),
            or_none(&g.map_zone)
        ));
        let (sin, cos) = g.rotation.to_radians().sin_cos();
        w.add(format!(
            "IFCMAPCONVERSION(#{ctx},#{crs},{},{},{},{},{},{})",
            real(g.eastings / 1000.0),
            real(g.northings / 1000.0),
            real(g.height / 1000.0),
            ratio(cos),
            ratio(sin),
            ratio(g.scale / 1000.0)
        ));
    }
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
    // Reinforcing bars as (level, host, entity): aggregated to their host, else in their level.
    let mut bars: Vec<(Guid, Option<Id>, u64)> = Vec::new();
    // The runs in each system, by its name, for the IfcSystem they are assigned to.
    let mut systems: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    // The boards on each distribution system, by its name, for the IfcDistributionSystem they are assigned to.
    let mut distribution: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    // The doors and windows in each wall as (width, height, sill), for what its sides measure less them.
    let mut holes: BTreeMap<Id, Vec<(f64, f64, f64)>> = BTreeMap::new();
    for e in m.elements.values() {
        if let Element::Door(o) | Element::Window(o) = e
            && let (Some(t), Some(host)) = (m.opening_type(e), m.elements.id(o.host))
        {
            holes.entry(host).or_default().push((t.width, t.height, o.sill));
        }
    }
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
                let mut props = vec![w.text_property("Location", None, wl.location.name())];
                for (name, to) in [("AttachTop", wl.attach_top), ("AttachBase", wl.attach_base)] {
                    if let Some(to) = to.and_then(|g| m.elements.id(g)) {
                        let global = w.guid(ELEMENT, to);
                        props.push(w.add(format!("IFCPROPERTYSINGLEVALUE({},$,IFCIDENTIFIER({global}),$)", s(name))));
                    }
                }
                w.define(ent, WALL, &props, (WALLED, WALLED_DEFINES), id);
                products.insert(id, ent);
                placed.insert(id, (ent, at));
                w.typed("IFCWALLTYPE", ".STANDARD.", &wl.type_name, ent);
                let values = wall_quantities(m, wl, holes.get(&id).map_or(&[][..], |h| h.as_slice()));
                w.quantities(ent, "Qto_WallBaseQuantities", &values, id);
            }
            Element::Floor(f) => {
                let Some(&(_, storey)) = storeys.get(&f.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                // Level, its outline less its openings swept up to its top; sloped or shaped, a shell round each
                // piece of it.
                let depth = m.floor_depth(f);
                let level = m.floor_top(f).g == P2(0.0, 0.0) && f.shape.is_empty();
                let solid = m.shape_solid(f);
                let shape = if level {
                    let profile = w.profile(&f.boundary, &f.openings);
                    w.body(profile, f.offset - depth, depth, &[])
                } else if let Some(solid) = &solid {
                    let brep = w.polyhedron(solid, m.elevation(f.level));
                    w.solid(&[brep], &[])
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
                w.typed("IFCSLABTYPE", ".FLOOR.", &f.type_name, ent);
                if f.structural {
                    w.load_bearing(ent, "Pset_SlabCommon", id);
                }
                // A level slab measures as its outline less its openings, through its depth; a sloped or shaped one has no
                // one plan area to give, so its depth, and its volumes: its plan through its depth plumb, as its underside
                // follows its top.
                let gross = geom::area(&f.boundary).abs();
                let net = gross - f.openings.iter().map(|o| geom::area(o).abs()).sum::<f64>();
                let values = if level {
                    vec![
                        ("Width", "LENGTH", depth),
                        ("Perimeter", "LENGTH", perimeter(&f.boundary)),
                        ("GrossArea", "AREA", geom::area(&f.boundary).abs() / 1e6),
                        ("NetArea", "AREA", net / 1e6),
                        ("GrossVolume", "VOLUME", geom::area(&f.boundary).abs() * depth / 1e9),
                        ("NetVolume", "VOLUME", net * depth / 1e9),
                    ]
                } else {
                    // A shaped one's, its solid's: under a variable layer its underside is level, so its depth isn't one.
                    let (gross, net) = match &solid {
                        Some(solid) => {
                            (m.shape_solid_over(f, &[]).map_or(gross * depth, |s| crate::model::volume(&s)), crate::model::volume(solid))
                        }
                        None => (gross * depth, net * depth),
                    };
                    vec![("Width", "LENGTH", depth), ("GrossVolume", "VOLUME", gross / 1e9), ("NetVolume", "VOLUME", net / 1e9)]
                };
                w.quantities(ent, "Qto_SlabBaseQuantities", &values, id);
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
                w.typed("IFCCOVERINGTYPE", ".CEILING.", &c.type_name, ent);
                hosts.insert(id, (ent, at, m.elevation(c.level)));
            }
            Element::Room(r) => {
                let Some(&(_, storey)) = storeys.get(&r.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                // A room that isn't enclosed has no shape to give.
                // As tall as its upper limit and offsets make it, from its base offset; in its colour on plans, if it has one.
                let height = m.room_height(r);
                let shape = match rooms.region(r) {
                    Some(region) => {
                        let profile = w.profile(&region.outer, &region.holes);
                        let style = r.colour.map(|c| w.surface_style("$", c, "$"));
                        format!("#{}", w.bodies(&[(profile, r.base_offset, height)], &[], style))
                    }
                    None => "$".into(),
                };
                let ent = w.add(format!(
                    "IFCSPACE({},$,{},$,$,#{at},{shape},{},.ELEMENT.,.SPACE.,$)",
                    w.guid(ELEMENT, id),
                    s(&r.number),
                    s(&r.name)
                ));
                // Its areas both ways: gross at the walls' centre lines, net at their faces.
                if let (Some(gross), Some(net)) = (rooms.region_at(r, RoomArea::Centre), rooms.region_at(r, RoomArea::Finish)) {
                    let values = [
                        ("Height", "LENGTH", height),
                        ("GrossPerimeter", "LENGTH", gross.perimeter()),
                        ("NetPerimeter", "LENGTH", net.perimeter()),
                        ("GrossFloorArea", "AREA", gross.area() / 1e6),
                        ("NetFloorArea", "AREA", net.area() / 1e6),
                        ("GrossVolume", "VOLUME", gross.area() * height / 1e9),
                        ("NetVolume", "VOLUME", net.area() * height / 1e9),
                    ];
                    w.quantities(ent, "Qto_SpaceBaseQuantities", &values, id);
                }
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
                // Its section is its type's, which the type's material profile set holds too.
                let section = Section::new(t.width, t.depth, t.round);
                let profile = w.section_profile(section);
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
                // Placed by the middle of its section: the mid-depth centre, 5.
                if matches!(e, Element::Footing(_)) {
                    w.typed_member("IFCFOOTINGTYPE", ".PAD_FOOTING.", &col.type_name, (section, String::new()), 5, ent);
                } else {
                    w.typed_member("IFCCOLUMNTYPE", ".COLUMN.", &col.type_name, (section, String::new()), 5, ent);
                    let values = member_quantities(t.width, t.depth, t.round, top - bottom);
                    w.quantities(ent, "Qto_ColumnBaseQuantities", &values, id);
                }
            }
            // A run: its section swept along its axis, in its material, a member of the system it is in.
            // See the plan and the 3D view for its joints: the file keeps the line and the section a reader
            // comes back to, whatever the mitres show on screen.
            crate::model::run_arm!(r) => {
                let (Some(&(_, storey)), Some(t), Some(g), Some((class, kind))) =
                    (storeys.get(&r.level), m.run_type(e), m.run_geom(e), crate::runs::ifc(e.category()))
                else {
                    continue;
                };
                // Placed at the start of its centre line; its section's x across it, level, its y up.
                let at = w.place(Some(storey), (g.a[0], g.a[1], g.a[2] - m.elevation(r.level)), None);
                // A beam's section is its type's, which the type's material profile set holds too.
                let section = Section::new(g.width, g.depth, g.round);
                let profile = if matches!(e, Element::Beam(_)) {
                    w.section_profile(section)
                } else if g.round {
                    let c = w.point(P2(0.0, 0.0));
                    let centre = w.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
                    w.add(format!("IFCCIRCLEPROFILEDEF(.AREA.,$,#{centre},{})", real(g.width / 2.0)))
                } else {
                    w.rectangle(P2(0.0, 0.0), P2(g.width, g.depth))
                };
                let axis = crate::view3d::unit(crate::view3d::sub(g.b, g.a));
                let across = if g.vertical() { [1.0, 0.0, 0.0] } else { crate::view3d::unit([-axis[1], axis[0], 0.0]) };
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
                    Element::Beam(_) => {
                        w.load_bearing(ent, "Pset_BeamCommon", id);
                        // Placed by the line its heights are on, the top, the middle or the bottom of its section.
                        let cardinal = cardinal_point(r.justify, g.vertical());
                        w.typed_member("IFCBEAMTYPE", ".BEAM.", &r.type_name, (section, t.material.clone()), cardinal, ent);
                        let values = member_quantities(g.width, g.depth, g.round, g.length());
                        w.quantities(ent, "Qto_BeamBaseQuantities", &values, id);
                    }
                    Element::StripFooting(_) => w.load_bearing(ent, "Pset_FootingCommon", id),
                    // A pipe, duct, conduit, tray, wire or cable: its type is of the segment class it is written as.
                    _ => w.typed(&format!("{class}TYPE"), kind, &r.type_name, ent),
                }
                // A beam's material is in its type's material profile set; the others' is the material itself.
                if !t.material.is_empty() && !matches!(e, Element::Beam(_)) {
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
                let (entity, kind, object_type) = f.ifc();
                let label = format!("{}:{}", cp.family, cp.type_name);
                let name = s(&label);
                let object = object_type.map_or_else(|| name.clone(), |o| s(&o));
                // What follows its tag, as IFC4 has its class: its predefined type, and what comes round it.
                let class = classes::class(&entity);
                let tail = class.map_or_else(|| format!(",{kind}"), |c| classes::fill(c.1, &kind));
                let ent = w.add(format!("{entity}({},$,{name},$,{object},#{at},#{shape},'{id}'{tail})", w.guid(ELEMENT, id)));
                contained.entry(cp.level).or_default().push(ent);
                products.insert(id, ent);
                // Its type object, of the class it is written as, where IFC4 has one: with its family's kind's predefined type,
                // or the one it remembers but `USERDEFINED` (which a type object would have to say), else none defined.
                let says = f.kind.is_some() || f.remembered().is_some();
                let type_kind = if kind.is_empty() || says && kind != ".USERDEFINED." { &kind } else { ".NOTDEFINED." };
                if let Some(attributes) = class.and_then(|c| c.2) {
                    let (before, after) = classes::around(attributes);
                    w.typed_with(&format!("{entity}TYPE"), &before, type_kind, &after, &label, ent);
                }
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
                // A board's own data: in a property set of BIMCraft's, and its system an IfcDistributionSystem it is assigned to.
                if let Some(panel) = &cp.panel {
                    let mut props = Vec::new();
                    if let Some(amps) = panel.mains {
                        props.push(w.add(format!("IFCPROPERTYSINGLEVALUE('Mains',$,IFCELECTRICCURRENTMEASURE({}),$)", real(amps))));
                    }
                    if let Some(n) = panel.slots {
                        props.push(w.add(format!("IFCPROPERTYSINGLEVALUE('Slots',$,IFCINTEGER({n}),$)")));
                    }
                    if let Some(name) = &panel.distribution {
                        props.push(w.text_property("DistributionSystem", None, name));
                        distribution.entry(name).or_default().push(ent);
                    }
                    if let Some(up) = panel.supply_from {
                        props.push(w.text_property(
                            "SupplyFrom",
                            Some("The GlobalId of the board that supplies this one"),
                            &compress(up.as_u128()),
                        ));
                    }
                    if !props.is_empty() {
                        w.define(ent, PANEL, &props, (PANEL_PROPERTIES, PANEL_DEFINES), id);
                    }
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
                    let shape = w.bodies(&steps, &[], None);
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
            // A room separation line: a virtual element along its line, which bounds spaces and is nothing else.
            Element::RoomSeparator(rs) => {
                let Some(&(_, storey)) = storeys.get(&rs.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let (a, b) = (w.point(rs.start), w.point(rs.end));
                let line = w.add(format!("IFCPOLYLINE((#{a},#{b}))"));
                let rep = w.add(format!("IFCSHAPEREPRESENTATION(#{axis},'Axis','Curve2D',(#{line}))"));
                let shape = w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"));
                let ent = w.add(format!(
                    "IFCVIRTUALELEMENT({},$,'Room Separation Line',$,'{SEPARATION}',#{at},#{shape},'{id}')",
                    w.guid(ELEMENT, id)
                ));
                contained.entry(rs.level).or_default().push(ent);
                products.insert(id, ent);
            }
            // A reinforcing bar: a disk swept along each straight piece of its path (a sweep round a sharp corner is no solid
            // IfcOpenShell builds), in the steel it is of.
            Element::Rebar(rb) => {
                let Some(&(_, storey)) = storeys.get(&rb.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let pts: Vec<u64> =
                    rb.path.iter().map(|q| w.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(q[0]), real(q[1]), real(q[2])))).collect();
                let radius = real(rb.diameter / 2.0);
                let mut disks = Vec::new();
                for (piece, ends) in pts.windows(2).zip(rb.path.windows(2)) {
                    if let ([a, b], [p, q]) = (piece, ends)
                        && crate::rebar::length(&[*p, *q]) > crate::model::TOL
                    {
                        let line = w.add(format!("IFCPOLYLINE((#{a},#{b}))"));
                        disks.push(w.add(format!("IFCSWEPTDISKSOLID(#{line},{radius},$,$,$)")));
                    }
                }
                let rep = w.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','AdvancedSweptSolid',({}))", w.body, refs(&disks)));
                let shape = w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"));
                let area = std::f64::consts::PI * rb.diameter * rb.diameter / 4.0;
                let ent = w.add(format!(
                    "IFCREINFORCINGBAR({},$,{},$,$,#{at},#{shape},'{id}',{},{},{},{},.NOTDEFINED.,.PLAIN.)",
                    w.guid(ELEMENT, id),
                    s(&format!("Bar {}", rb.diameter)),
                    s(&rb.grade),
                    real(rb.diameter),
                    // In the file's square metres, to twelve places: a bar's section is a ten-thousandth of one.
                    ratio(area / 1e6),
                    real(crate::rebar::length(&rb.path))
                ));
                bars.push((rb.level, rb.host.and_then(|h| m.elements.id(h)), ent));
                products.insert(id, ent);
            }
            Element::Railing(rl) if rl.host.is_none() => {
                let Some(&(_, storey)) = storeys.get(&rl.level) else { continue };
                let ent = w.railing(m, id, rl, storey, m.elevation(rl.level), axis);
                contained.entry(rl.level).or_default().push(ent);
                products.insert(id, ent);
            }
            Element::Roof(r) => {
                let (Some(&(_, storey)), pieces) = (storeys.get(&r.level), m.roof_slabs(r)) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                // A shaped one is one closed faceted Brep, as a shaped floor is; any other a shell round each piece.
                let breps: Vec<u64> = match m.roof_solid(r) {
                    Some(solid) => vec![w.polyhedron(&solid, m.elevation(r.level))],
                    None => pieces.iter().map(|(face, rise)| w.brep(face, m.elevation(r.level), *rise)).collect(),
                };
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
                // Its type is flat only where the roof is (the test its own kind takes above), else NOTDEFINED.
                let flat = r.extrusion.is_none() && r.slope == 0.0;
                w.typed("IFCROOFTYPE", if flat { ".FLAT_ROOF." } else { ".NOTDEFINED." }, &r.type_name, ent);
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
    // A bar is a part of the element it reinforces; one with none, or whose host isn't written, is in its level.
    let mut reinforced: BTreeMap<Id, Vec<u64>> = BTreeMap::new();
    for (level, host, ent) in bars {
        match host.filter(|h| products.contains_key(h)) {
            Some(h) => reinforced.entry(h).or_default().push(ent),
            None => contained.entry(level).or_default().push(ent),
        }
    }
    for (host, parts) in &reinforced {
        if let Some(ent) = products.get(host) {
            w.add(format!("IFCRELAGGREGATES({},$,$,$,#{ent},({}))", w.guid(PARTS, *host), refs(parts)));
        }
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
        let label = o.family.as_ref().map_or_else(|| o.type_name.clone(), |f| format!("{f}:{}", o.type_name));
        let name = s(&label);
        // How it opens, as its family or its type says; a door that says nothing swings.
        let ent = if matches!(e, Element::Door(_)) {
            let operation = Operation::ifc_door(t.operation, o.flip_hand);
            w.add(format!(
                "IFCDOOR({},$,{name},$,{name},#{fill_at},#{shape},{},{h},{wd},.DOOR.,{operation},$)",
                w.guid(ELEMENT, id),
                s(&o.mark)
            ))
        } else {
            let partitioning = Operation::ifc_window_partitioning(t.operation);
            w.add(format!(
                "IFCWINDOW({},$,{name},$,{name},#{fill_at},#{shape},{},{h},{wd},.WINDOW.,{partitioning},$)",
                w.guid(ELEMENT, id),
                s(&o.mark)
            ))
        };
        // Its type object: after its PredefinedType, the operation (a door's) or partitioning (a window's) its type has, that
        // the occurrence's own sizes take precedence over the type's, and no user-defined operation.
        let (class, predefined, how) = if matches!(e, Element::Door(_)) {
            ("IFCDOORTYPE", ".DOOR.", Operation::ifc_door(t.operation, false))
        } else {
            ("IFCWINDOWTYPE", ".WINDOW.", Operation::ifc_window_partitioning(t.operation))
        };
        w.typed_with(class, "", predefined, &format!(",{how},.T.,$"), &label, ent);
        w.add(format!("IFCRELFILLSELEMENT({},$,$,$,#{opening},#{ent})", w.guid(FILLS, id)));
        let qto = if matches!(e, Element::Door(_)) { "Qto_DoorBaseQuantities" } else { "Qto_WindowBaseQuantities" };
        let values = [
            ("Width", "LENGTH", t.width),
            ("Height", "LENGTH", t.height),
            ("Perimeter", "LENGTH", 2.0 * (t.width + t.height)),
            ("Area", "AREA", t.width * t.height / 1e6),
        ];
        w.quantities(ent, qto, &values, id);
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
            // The variable layer (MOD-16) says so in its description: IFC has no word of its own for a layer that tapers.
            let description = if l.variable { "'Variable'" } else { "$" };
            ids.push(w.add(format!("IFCMATERIALLAYER({material},{},$,$,{description},{category},$)", real(l.thickness))));
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
    // A distribution system is an IfcDistributionSystem, electrical, its boards assigned to it and the building it serves.
    for (name, members) in &distribution {
        let key = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
        let system = w.add(format!("IFCDISTRIBUTIONSYSTEM({},$,{},$,$,$,.ELECTRICAL.)", w.guid(DISTRIBUTION, key), s(name)));
        w.add(format!("IFCRELASSIGNSTOGROUP({},$,$,$,({}),$,#{system})", w.guid(DISTRIBUTION_MEMBERS, key), refs(members)));
        w.add(format!("IFCRELSERVICESBUILDINGS({},$,$,$,#{system},(#{building}))", w.guid(DISTRIBUTION_SERVES, key)));
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
    // A placed device's connector (the power one, else the first electrical one; a pipe's waits for IFC's ports): `Pset_ElectricalDeviceCommon`, with the poles, and for a power
    // connector the power factor and the voltage (IFC4's types: a ratio, and a bounded value with its set point), and `BIMCraft_Connector` with what IFC has no property for.
    for (&id, &ent) in &products {
        let Some(Element::Component(cp)) = m.elements.get(id) else { continue };
        let Some(connectors) = m.families.get(&cp.family).and_then(|f| f.connector_values(&cp.type_name, &cp.params).ok()) else {
            continue;
        };
        let Some(c) = connectors.iter().find(|c| c.kind == ConnectorKind::Power).or(connectors.iter().find(|c| c.kind.electrical())) else {
            continue;
        };
        let power = c.kind == ConnectorKind::Power;
        let mut device = vec![w.add(format!("IFCPROPERTYSINGLEVALUE('NumberOfPoles',$,IFCINTEGER({}),$)", c.poles))];
        if power {
            device.push(w.add(format!("IFCPROPERTYSINGLEVALUE('PowerFactor',$,IFCNORMALISEDRATIOMEASURE({}),$)", real(c.power_factor))));
            device.push(w.add(format!("IFCPROPERTYBOUNDEDVALUE('RatedVoltage',$,$,$,$,IFCELECTRICVOLTAGEMEASURE({}))", real(c.voltage))));
        }
        w.define(ent, "Pset_ElectricalDeviceCommon", &device, (DEVICE, DEVICE_DEFINES), id);
        let system = serde_json::to_value(c.system).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let mut own = vec![w.text_property("System", None, &system)];
        if !c.classification.is_empty() {
            own.push(w.text_property("Classification", None, &c.classification));
        }
        if power {
            own.push(w.add(format!("IFCPROPERTYSINGLEVALUE('LoadVA',$,IFCREAL({}),$)", real(c.load_va))));
            for (k, va) in c.load_va_by_phase.iter().enumerate() {
                own.push(w.add(format!("IFCPROPERTYSINGLEVALUE('LoadVAPhase{}',$,IFCREAL({}),$)", k + 1, real(*va))));
            }
        }
        w.define(ent, CONNECTOR_SET, &own, (CONNECTOR, CONNECTOR_DEFINES), id);
    }
    // A phased project's elements say what the work does to them, as IFC's `Status` (PEnum_ElementStatus) in their `Pset_*Common`: taken down in
    // any phase, DEMOLISH (TEMPORARY when made in that phase too); made in the first phase, EXISTING; else NEW. A door or a
    // window with no phase of its own follows its wall. A project with no phases says nothing: all of it is new work.
    if phasing::any(m) {
        for (&id, &ent) in &products {
            let Some(e) = m.elements.get(id) else { continue };
            let Some(pset) = common_pset(e) else { continue };
            let Some(status) = ifc_status(m, e) else { continue };
            let p = w.add(format!("IFCPROPERTYENUMERATEDVALUE('Status',$,(IFCLABEL('{status}')),$)"));
            w.common.entry(id).or_insert((ent, pset, Vec::new())).2.push(p);
        }
    }
    for (id, (ent, pset, props)) in std::mem::take(&mut w.common) {
        w.define(ent, pset, &props, (COMMON, COMMON_DEFINES), id);
    }
    w.write_types(m);
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
        (ui_msg!("Phase Filters"), !phasing::is_default_filters(&m.phase_filters), serde_json::to_string(&m.phase_filters)),
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

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
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

    #[test]
    fn a_rooms_colour_styles_the_solid_of_its_spaces_body() {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
               room {"point": [4000, 3000]}"#,
        )
        .unwrap();
        let styled = |f: &str| f.matches("=IFCSTYLEDITEM(").count();
        let plain = styled(&export(&s.model));
        let room = s.model.elements.iter().find(|(_, e)| e.category() == "room").map(|(id, _)| *id).unwrap();
        s.execute("modify", &json!({"id": room, "colour": [214, 229, 210]})).unwrap();
        let f = export(&s.model);
        dangling(&f);
        assert_eq!(styled(&f), plain + 1, "one style, for the one room with a colour");
        // `#n=...` of reference `#n`, and the first reference in it after `after`.
        let line = |n: &str| f.lines().find(|l| l.starts_with(&format!("{n}="))).unwrap_or_else(|| panic!("no {n}")).to_string();
        let after = |l: &str, after: &str| {
            format!("#{}", l.split_once(after).unwrap().1.chars().take_while(char::is_ascii_digit).collect::<String>())
        };
        // The space's body, its shape's representation's solid: the item its style is on.
        let space = f.lines().find(|l| l.contains("=IFCSPACE(")).unwrap();
        let shape = space.split(',').nth(6).unwrap();
        let solid = after(&line(&after(&line(shape), "(#")), ",(#");
        assert!(line(&solid).contains("=IFCEXTRUDEDAREASOLID("), "{}", line(&solid));
        let item = f.lines().find(|l| l.contains(&format!("=IFCSTYLEDITEM({solid},"))).unwrap();
        // Its surface style shades it in the room's colour.
        let shading = after(&line(&after(item, ",(#")), ",(#");
        let colour = line(&after(&line(&shading), "(#"));
        let [r, g, b] = [214.0, 229.0, 210.0].map(|c: f64| real(c / 255.0));
        assert!(colour.ends_with(&format!("=IFCCOLOURRGB($,{r},{g},{b});")), "{colour}");
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
        set_of(f, ent, PHASING)
    }

    /// The properties of set `name` of entity `#ent`, as written, in the set's order; none when it has no such set.
    fn set_of(f: &str, ent: u64, name: &str) -> Vec<String> {
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
            if !set.contains(&format!("'{name}'")) {
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
    fn a_phased_projects_elements_say_what_the_work_does_to_them_in_their_common_set() {
        use crate::graphics::testing::{house, run};
        let status = |v: &str| format!("IFCPROPERTYENUMERATEDVALUE('Status',$,(IFCLABEL('{v}')),$)");
        let mut s = Session::new();
        let walls = house(&mut s);
        run(&mut s, "door", json!({"at": [2000, 0]}));
        let window = run(&mut s, "window", json!({"at": [0, 3000]}))["id"].as_u64().unwrap();
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        for (id, held) in [
            (walls[0], json!({"Phase Demolished": "Demolition"})),
            (walls[1], json!({"Phase Created": "New Construction"})),
            (walls[2], json!({"Phase Created": "Demolition", "Phase Demolished": "Demolition"})),
            (window, json!({"Phase Created": "New Construction"})),
        ] {
            run(&mut s, "modify", json!({"id": id, "params": held}));
        }
        let f = export(&s.model);
        let wall = of_kind(&f, "IFCWALL");
        let common = |ent| set_of(&f, ent, "Pset_WallCommon");
        assert_eq!(common(wall[0]), [status("DEMOLISH")]);
        assert_eq!(common(wall[1]), [status("NEW")]);
        assert_eq!(common(wall[2]), [status("TEMPORARY")], "made and taken down in the same phase");
        assert_eq!(common(wall[3]), [status("EXISTING")], "no value is the first phase");
        // A door with no phase of its own follows its wall; a window holding one says its own.
        assert_eq!(set_of(&f, of_kind(&f, "IFCDOOR")[0], "Pset_DoorCommon"), [status("DEMOLISH")]);
        assert_eq!(set_of(&f, of_kind(&f, "IFCWINDOW")[0], "Pset_WindowCommon"), [status("NEW")]);
        dangling(&f);
    }

    /// The occurrences an `IfcRelDefinesByType` of the type object `#kind` lists.
    fn occurrences_of(f: &str, kind: u64) -> Vec<u64> {
        let rel =
            f.lines().find(|l| l.contains("=IFCRELDEFINESBYTYPE(") && l.trim_end_matches(';').ends_with(&format!(",#{kind})"))).unwrap();
        let list = rel.split_once("$,$,$,(").unwrap().1.split_once("),#").unwrap().0;
        list.split(',').map(|r| r.trim_start_matches('#').parse().unwrap()).collect()
    }

    #[test]
    fn each_type_is_written_once_and_related_to_its_occurrences() {
        use crate::graphics::testing::{house, run};
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "type", json!({"category": "beam", "name": "B 150x600", "width": 150, "depth": 600}));
        for y in [0, 4000] {
            run(&mut s, "beam", json!({"points": [[0, y], [5000, y]], "type": "B 150x600", "offset": 3000}));
        }
        run(&mut s, "structural_column", json!({"at": [0, 0], "width": 300, "depth": 300}));
        run(&mut s, "floor", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}));
        let f = export(&s.model);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
        let beams = of_kind(&f, "IFCBEAM");
        assert_eq!(beams.len(), 2);
        let kinds = of_kind(&f, "IFCBEAMTYPE");
        assert_eq!(kinds.len(), 1, "one type for the two beams");
        assert!(f.contains("IFCBEAMTYPE(") && f.contains("'B 150x600',$,$,$,$,$,$,.BEAM.)"));
        assert_eq!(occurrences_of(&f, kinds[0]), beams);
        let q = quantities_of(&f, beams[0], "Qto_BeamBaseQuantities");
        assert!(near(q["Length"], 5000.0) && near(q["CrossSectionArea"], 0.09) && !q.contains_key("NetVolume"), "{q:?}");
        assert!(near(q["GrossVolume"], 0.45) && near(q["OuterSurfaceArea"], 2.0 * (0.15 + 0.6) * 5.0), "the sides only: {q:?}");
        assert!(!q.contains_key("GrossSurfaceArea"), "outer surface and section are given apart: {q:?}");
        let columns = of_kind(&f, "IFCCOLUMN");
        let types = of_kind(&f, "IFCCOLUMNTYPE");
        assert_eq!((columns.len(), types.len()), (1, 1));
        assert_eq!(occurrences_of(&f, types[0]), columns);
        let q = quantities_of(&f, columns[0], "Qto_ColumnBaseQuantities");
        assert!(near(q["CrossSectionArea"], 0.09) && near(q["GrossVolume"], 0.09 * q["Length"] / 1e3), "{q:?}");
        assert!(near(q["OuterSurfaceArea"], 4.0 * 0.3 * q["Length"] / 1e3) && !q.contains_key("GrossSurfaceArea"), "{q:?}");
        // Walls and slabs have theirs, one for each type used.
        let walls: BTreeSet<String> =
            s.model.elements.iter().filter_map(|(_, e)| if let Element::Wall(w) = e { Some(w.type_name.clone()) } else { None }).collect();
        assert_eq!(of_kind(&f, "IFCWALLTYPE").len(), walls.len());
        assert_eq!(of_kind(&f, "IFCSLABTYPE").len(), 1, "one type for the floor");
        // The same on every export, so a diff of two shows only what changed.
        assert_eq!(f, export(&s.model));
        dangling(&f);
    }

    /// The entity lines of kind `class` in `f`, as written.
    fn entities(f: &str, class: &str) -> Vec<String> {
        f.lines().filter(|l| l.contains(&format!("={class}("))).map(str::to_string).collect()
    }

    /// The attributes of an entity line, as the STEP text spells them (strings with their quotes), split at the commas
    /// that are outside strings.
    fn attributes(line: &str) -> Vec<String> {
        let body = line.split_once('(').unwrap().1.strip_suffix(");").unwrap();
        let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
        for c in body.chars() {
            match c {
                '\'' => {
                    quoted = !quoted;
                    cur.push(c);
                }
                ',' if !quoted => out.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            }
        }
        out.push(cur);
        out
    }

    /// The attributes of the type object of `class` named `name` in `f`; there is one.
    fn type_object(f: &str, class: &str, name: &str) -> Vec<String> {
        let named: Vec<Vec<String>> =
            entities(f, class).iter().map(|l| attributes(l.as_str())).filter(|a| a[2] == format!("'{name}'")).collect();
        assert_eq!(named.len(), 1, "one {class} named {name}");
        named.into_iter().next().unwrap()
    }

    /// The type name of the first element of `category` in the model: a door's, window's, ceiling's or roof's.
    fn type_name_of(s: &Session, category: &str) -> String {
        s.model
            .elements
            .values()
            .filter(|e| e.category() == category)
            .find_map(|e| match e {
                Element::Door(o) | Element::Window(o) => Some(o.type_name.clone()),
                Element::Ceiling(c) => Some(c.type_name.clone()),
                Element::Roof(r) => Some(r.type_name.clone()),
                _ => None,
            })
            .unwrap()
    }

    /// The type objects of `type_class` in `f`, once every `class` occurrence in `f` is related to one of them and to no
    /// other: each occurrence listed by exactly one `IfcRelDefinesByType`.
    fn typed_once(f: &str, class: &str, type_class: &str) -> Vec<u64> {
        let types = of_kind(f, type_class);
        let mut related: Vec<u64> = types.iter().flat_map(|t| occurrences_of(f, *t)).collect();
        related.sort_unstable();
        let mut all = of_kind(f, class);
        all.sort_unstable();
        assert_eq!(related, all, "each {class} related to exactly one {type_class}");
        types
    }

    #[test]
    fn doors_windows_roofs_and_ceilings_get_type_objects_each_occurrence_related_once() {
        use crate::graphics::testing::{house, run};
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "door", json!({"at": [2000, 0]}));
        run(&mut s, "door", json!({"at": [5000, 0]}));
        run(&mut s, "window", json!({"at": [8000, 3000]}));
        run(&mut s, "ceiling", json!({"point": [4000, 3000]}));
        run(&mut s, "roof", json!({}));
        let f = export(&s.model);
        dangling(&f);
        // A door's type carries, after its PredefinedType, its OperationType, whether the occurrence's own parameters take
        // precedence (its sizes are its own: yes) and a user-defined operation (none).
        assert_eq!(typed_once(&f, "IFCDOOR", "IFCDOORTYPE").len(), 1, "one type for both doors");
        let door = type_object(&f, "IFCDOORTYPE", &type_name_of(&s, "door"));
        assert_eq!(door.len(), 13, "IfcDoorType's attributes");
        assert_eq!(door[9..].join(","), ".DOOR.,.SINGLE_SWING_LEFT.,.T.,$");
        // A window's type carries its PartitioningType the same way.
        assert_eq!(typed_once(&f, "IFCWINDOW", "IFCWINDOWTYPE").len(), 1);
        let window = type_object(&f, "IFCWINDOWTYPE", &type_name_of(&s, "window"));
        assert_eq!(window.len(), 13, "IfcWindowType's attributes");
        assert_eq!(window[9..].join(","), ".WINDOW.,.SINGLE_PANEL.,.T.,$");
        assert_eq!(typed_once(&f, "IFCCOVERING", "IFCCOVERINGTYPE").len(), 1);
        let ceiling = type_object(&f, "IFCCOVERINGTYPE", &type_name_of(&s, "ceiling"));
        assert_eq!((ceiling.len(), ceiling[9].as_str()), (10, ".CEILING."));
        assert_eq!(typed_once(&f, "IFCROOF", "IFCROOFTYPE").len(), 1);
        let roof = type_object(&f, "IFCROOFTYPE", &type_name_of(&s, "roof"));
        assert_eq!((roof.len(), roof[9].as_str()), (10, ".NOTDEFINED."), "a roof sloping 30 degrees, the default");
    }

    #[test]
    fn a_roof_type_is_flat_only_when_every_roof_of_it_is_flat() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        run(&mut s, "roof", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "slope": 0}));
        let f = export(&s.model);
        dangling(&f);
        assert_eq!(typed_once(&f, "IFCROOF", "IFCROOFTYPE").len(), 1);
        assert_eq!(type_object(&f, "IFCROOFTYPE", &type_name_of(&s, "roof"))[9], ".FLAT_ROOF.");
        // A sloped roof of the same type: the type is no longer flat, and both roofs are of it.
        run(&mut s, "roof", json!({"boundary": [[10000, 0], [14000, 0], [14000, 4000], [10000, 4000]]}));
        let f = export(&s.model);
        dangling(&f);
        assert_eq!(typed_once(&f, "IFCROOF", "IFCROOFTYPE").len(), 1, "one type for both roofs");
        assert_eq!(type_object(&f, "IFCROOFTYPE", &type_name_of(&s, "roof"))[9], ".NOTDEFINED.");
    }

    #[test]
    fn components_get_type_objects_of_the_class_each_is_written_as_with_their_attribute_counts() {
        use crate::graphics::testing::run;
        // A starter family of each class a component is written as.
        let families = [
            ("Table", "IFCFURNITURE"),
            ("Kitchen Sink", "IFCSANITARYTERMINAL"),
            ("Floor Drain", "IFCWASTETERMINAL"),
            ("Elbow 90", "IFCPIPEFITTING"),
            ("Gate Valve", "IFCVALVE"),
            ("Distribution Board", "IFCELECTRICDISTRIBUTIONBOARD"),
            ("Outlet", "IFCOUTLET"),
            ("Switch", "IFCSWITCHINGDEVICE"),
            ("Ceiling Luminaire", "IFCLIGHTFIXTURE"),
            ("Air Terminal", "IFCBUILDINGELEMENTPROXY"),
        ];
        let mut s = Session::new();
        for (i, (family, _)) in families.iter().enumerate() {
            run(&mut s, "component", json!({"family": family, "at": [i as f64 * 2000.0, 0.0]}));
        }
        let f = export(&s.model);
        dangling(&f);
        for (family, class) in families {
            let type_class = format!("{class}TYPE");
            typed_once(&f, class, &type_class);
            let type_name = s
                .model
                .elements
                .values()
                .find_map(|e| match e {
                    Element::Component(cp) if cp.family == family => Some(cp.type_name.clone()),
                    _ => None,
                })
                .unwrap();
            let attrs = type_object(&f, &type_class, &format!("{family}:{type_name}"));
            // Furniture's type has its AssemblyPlace (mandatory) ahead of its PredefinedType; the others have ten attributes.
            let count = if class == "IFCFURNITURE" { 11 } else { 10 };
            // Its predefined type is its family's kind's (a kitchen sink's .SINK.), or not defined.
            let fam = s.model.families.get(family).unwrap();
            let predefined = if fam.kind.is_some() { fam.ifc().1 } else { ".NOTDEFINED.".into() };
            assert_eq!((attrs.len(), attrs[count - 1].as_str()), (count, predefined.as_ref()), "{type_class}");
            if class == "IFCFURNITURE" {
                assert_eq!(attrs[9], ".NOTDEFINED.", "its assembly place");
            }
        }
    }

    #[test]
    fn discipline_runs_get_the_type_objects_of_their_segment_classes() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        run(&mut s, "pipe", json!({"points": [[0, 6000], [6000, 6000]], "offset": 500}));
        run(&mut s, "duct", json!({"points": [[0, 7000], [6000, 7000]], "offset": 2500}));
        run(&mut s, "conduit", json!({"points": [[0, 8000], [6000, 8000]], "offset": 2500}));
        run(&mut s, "cable_tray", json!({"points": [[0, 9000], [6000, 9000]], "offset": 2700}));
        run(&mut s, "wire", json!({"points": [[0, 10000], [6000, 10000]], "offset": 2500}));
        run(&mut s, "cabling", json!({"points": [[0, 11000], [6000, 11000]], "offset": 2500}));
        let f = export(&s.model);
        dangling(&f);
        // The PredefinedType of each type is the one its runs are written with (the last attribute of each type object).
        let kinds = |class: &str| -> Vec<String> { entities(&f, class).iter().map(|l| attributes(l).pop().unwrap()).collect() };
        typed_once(&f, "IFCPIPESEGMENT", "IFCPIPESEGMENTTYPE");
        assert_eq!(kinds("IFCPIPESEGMENTTYPE"), [".RIGIDSEGMENT."]);
        typed_once(&f, "IFCDUCTSEGMENT", "IFCDUCTSEGMENTTYPE");
        assert_eq!(kinds("IFCDUCTSEGMENTTYPE"), [".RIGIDSEGMENT."]);
        // A conduit and a cable tray are both cable carrier segments, each its own type.
        assert_eq!(typed_once(&f, "IFCCABLECARRIERSEGMENT", "IFCCABLECARRIERSEGMENTTYPE").len(), 2);
        let mut carriers = kinds("IFCCABLECARRIERSEGMENTTYPE");
        carriers.sort();
        assert_eq!(carriers, [".CABLETRAYSEGMENT.", ".CONDUITSEGMENT."]);
        // A wire and structured cabling are both cable segments.
        typed_once(&f, "IFCCABLESEGMENT", "IFCCABLESEGMENTTYPE");
        let cables = kinds("IFCCABLESEGMENTTYPE");
        assert!(!cables.is_empty() && cables.iter().all(|k| k == ".CABLESEGMENT."), "{cables:?}");
    }

    /// The whole line of entity `#n` in `f`, as written.
    fn line(f: &str, n: u64) -> String {
        let head = format!("#{n}=");
        f.lines().find(|l| l.starts_with(&head)).unwrap_or_else(|| panic!("no #{n}")).to_string()
    }

    /// The number a STEP reference names: `#12` is 12.
    fn number(reference: &str) -> u64 {
        reference.trim_start_matches('#').parse().unwrap_or_else(|_| panic!("not a reference: {reference}"))
    }

    /// What `IfcRelAssociatesMaterial` gives entity `#ent`: the number of the material, set or usage. There is exactly one.
    fn material_of(f: &str, ent: u64) -> u64 {
        let given: Vec<u64> = f
            .lines()
            .filter(|l| l.contains("=IFCRELASSOCIATESMATERIAL("))
            .filter_map(|l| {
                let (list, material) = l.split_once("$,$,$,(")?.1.split_once("),#")?;
                list.split(',').any(|r| r == format!("#{ent}")).then(|| number(material.trim_end_matches(");")))
            })
            .collect();
        assert_eq!(given.len(), 1, "#{ent} is associated with one material, set or usage");
        given[0]
    }

    /// The one profile of material profile set `#set`: its material (none when `$`) and its profile definition, by number.
    fn the_profile(f: &str, set: u64) -> (Option<u64>, u64) {
        let set = attributes(&line(f, set));
        assert_eq!(set.len(), 4, "IfcMaterialProfileSet's attributes");
        let profiles = set[2].trim_matches(|c| c == '(' || c == ')');
        assert!(!profiles.contains(','), "one profile: {profiles}");
        let profile = attributes(&line(f, number(profiles)));
        assert_eq!(profile.len(), 6, "IfcMaterialProfile's attributes");
        ((profile[2] != "$").then(|| number(&profile[2])), number(&profile[3]))
    }

    /// The set and the cardinal point of the usage entity `#ent` is associated with.
    fn usage_of(f: &str, ent: u64) -> (u64, String) {
        let usage = line(f, material_of(f, ent));
        assert!(usage.contains("=IFCMATERIALPROFILESETUSAGE("), "{usage}");
        let a = attributes(&usage);
        assert_eq!((a.len(), a[2].as_str()), (3, "$"), "IfcMaterialProfileSetUsage's attributes: {usage}");
        (number(&a[0]), a[1].clone())
    }

    /// The set of the type object `#kind`, its profile definition's line and its material's line (empty when it has none).
    fn section_of(f: &str, kind: u64) -> (u64, String, String) {
        let set = material_of(f, kind);
        assert!(line(f, set).contains("=IFCMATERIALPROFILESET("), "{}", line(f, set));
        let (material, profile) = the_profile(f, set);
        (set, line(f, profile), material.map(|m| line(f, m)).unwrap_or_default())
    }

    #[test]
    fn a_beam_type_holds_its_section_in_a_material_profile_set_that_each_beam_uses() {
        use crate::graphics::testing::{house, run};
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "type", json!({"category": "beam", "name": "B 150x600", "width": 150, "depth": 600, "material": "Concrete"}));
        run(&mut s, "beam", json!({"points": [[0, 0], [5000, 0]], "type": "B 150x600", "offset": 3000}));
        run(&mut s, "beam", json!({"points": [[0, 4000], [5000, 4000]], "type": "B 150x600", "offset": 3000, "justify": "bottom"}));
        let f = export(&s.model);
        dangling(&f);
        // The type holds one set: one profile, of the type's material, a 150 by 600 rectangle.
        let kind = of_kind(&f, "IFCBEAMTYPE");
        assert_eq!(kind.len(), 1);
        let (set, profile, material) = section_of(&f, kind[0]);
        assert_eq!(of_kind(&f, "IFCMATERIALPROFILESET"), [set], "one set for the type");
        assert!(line(&f, set).contains("=IFCMATERIALPROFILESET('B 150x600',$,(#"), "{}", line(&f, set));
        assert!(material.contains("=IFCMATERIAL('Concrete'"), "{material}");
        assert!(profile.contains("=IFCRECTANGLEPROFILEDEF(.AREA."), "{profile}");
        let rectangle = attributes(&profile);
        assert_eq!((rectangle[3].as_str(), rectangle[4].as_str()), ("150.", "600."), "{profile}");
        // Both beams use the set, each by a usage with the cardinal point it is placed by: the top of its section (a beam's
        // default) is 8, its bottom 2.
        let beams = of_kind(&f, "IFCBEAM");
        assert_eq!(beams.len(), 2);
        let usages: Vec<(u64, String)> = beams.iter().map(|b| usage_of(&f, *b)).collect();
        assert!(usages.iter().all(|(of, _)| *of == set), "{usages:?}");
        let mut points: Vec<&str> = usages.iter().map(|(_, c)| c.as_str()).collect();
        points.sort_unstable();
        assert_eq!(points, ["2", "8"]);
        // The profile is written once, and both bodies sweep it.
        assert_eq!(of_kind(&f, "IFCRECTANGLEPROFILEDEF").len(), 1);
        let shared = the_profile(&f, set).1;
        let swept = entities(&f, "IFCEXTRUDEDAREASOLID").iter().filter(|l| attributes(l)[0] == format!("#{shared}")).count();
        assert_eq!(swept, 2, "both bodies sweep the type's profile");
        // The material is in the set, so a beam has no other: one for the type, one for each cardinal point's usage.
        assert_eq!(of_kind(&f, "IFCRELASSOCIATESMATERIAL").len(), 3);
        assert_eq!(f, export(&s.model), "the same on every export");
    }

    #[test]
    fn a_beam_type_with_no_material_gives_its_profile_none_and_a_round_one_a_circle() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        run(&mut s, "type", json!({"category": "beam", "name": "Plain", "width": 120, "depth": 300}));
        run(&mut s, "beam", json!({"points": [[0, 0], [4000, 0]], "type": "Plain", "offset": 3000, "justify": "centre"}));
        run(&mut s, "type", json!({"category": "beam", "name": "Tube 160", "width": 160, "round": true, "material": "Metal"}));
        run(&mut s, "beam", json!({"points": [[0, 2000], [4000, 2000]], "type": "Tube 160", "offset": 3000}));
        let f = export(&s.model);
        dangling(&f);
        let kinds = of_kind(&f, "IFCBEAMTYPE");
        assert_eq!(kinds.len(), 2);
        let by_name = |name: &str| *kinds.iter().find(|k| line(&f, **k).contains(&format!("'{name}'"))).unwrap();
        let (_, profile, material) = section_of(&f, by_name("Plain"));
        assert_eq!(material, "", "no material, no IfcMaterial");
        assert!(attributes(&profile)[3] == "120." && attributes(&profile)[4] == "300.", "{profile}");
        // A round section is a circle of half its diameter, as its body is.
        let (_, tube, material) = section_of(&f, by_name("Tube 160"));
        assert!(tube.contains("=IFCCIRCLEPROFILEDEF(.AREA."), "{tube}");
        assert_eq!(attributes(&tube)[3], "80.", "{tube}");
        assert!(material.contains("=IFCMATERIAL('Metal'"), "{material}");
        // Centred on the line it was drawn at is 5.
        let plain = of_kind(&f, "IFCBEAM").into_iter().find(|b| occurrences_of(&f, by_name("Plain")).contains(b)).unwrap();
        assert_eq!(usage_of(&f, plain).1, "5");
    }

    #[test]
    fn a_columns_type_holds_a_circle_of_its_diameter_or_a_rectangle_and_its_columns_use_it() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        run(&mut s, "structural_column", json!({"at": [0, 0], "type": "Concrete Round 400"}));
        run(&mut s, "structural_column", json!({"at": [3000, 0], "type": "Concrete Round 400"}));
        run(&mut s, "structural_column", json!({"at": [6000, 0], "type": "Concrete 200 x 500"}));
        run(&mut s, "column", json!({"at": [9000, 0]}));
        let f = export(&s.model);
        dangling(&f);
        let types = of_kind(&f, "IFCCOLUMNTYPE");
        assert_eq!(types.len(), 3, "the round, the 200 by 500 and the default column");
        let mut circles = 0;
        for kind in &types {
            let (set, profile, material) = section_of(&f, *kind);
            assert_eq!(material, "", "a column's type has no material");
            // Every column of the type uses its set, centred: the mid-depth centre, 5.
            for column in occurrences_of(&f, *kind) {
                assert_eq!(usage_of(&f, column), (set, "5".to_string()));
            }
            if profile.contains("=IFCCIRCLEPROFILEDEF(") {
                circles += 1;
                // 400 across: a radius of 200, for both columns of that type.
                assert_eq!(attributes(&profile)[3], "200.", "{profile}");
                assert_eq!(occurrences_of(&f, *kind).len(), 2);
            }
        }
        assert_eq!(circles, 1);
        assert!(entities(&f, "IFCRECTANGLEPROFILEDEF").iter().any(|r| attributes(r)[3] == "200." && attributes(r)[4] == "500."));
        // Three usages: the two round columns share one, all five columns were related once.
        assert_eq!(of_kind(&f, "IFCMATERIALPROFILESETUSAGE").len(), 3);
    }

    #[test]
    fn a_pad_footings_type_has_a_profile_set_and_a_strip_footing_or_a_pipe_has_none() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        run(&mut s, "footing", json!({"at": [0, 0]}));
        run(&mut s, "strip_footing", json!({"points": [[0, 3000], [4000, 3000]]}));
        run(&mut s, "pipe", json!({"points": [[0, 6000], [4000, 6000]], "offset": 500}));
        let f = export(&s.model);
        dangling(&f);
        let kind = of_kind(&f, "IFCFOOTINGTYPE");
        assert_eq!(kind.len(), 1, "the pad's type; the strip footing has none");
        let (set, profile, _) = section_of(&f, kind[0]);
        assert_eq!(of_kind(&f, "IFCMATERIALPROFILESET"), [set]);
        let pad = s.model.types.footing.values().next().unwrap();
        assert!(profile.contains("=IFCRECTANGLEPROFILEDEF("), "{profile}");
        assert_eq!(attributes(&profile)[3..5], [real(pad.width), real(pad.depth)]);
        let footing = occurrences_of(&f, kind[0]);
        assert_eq!(usage_of(&f, footing[0]), (set, "5".to_string()));
        // A strip footing and a pipe keep the plain material they have.
        for class in ["IFCFOOTING", "IFCPIPESEGMENT"] {
            let plain: Vec<String> =
                of_kind(&f, class).into_iter().filter(|e| !footing.contains(e)).map(|e| line(&f, material_of(&f, e))).collect();
            assert_eq!(plain.len(), 1, "{class}");
            assert!(plain[0].contains("=IFCMATERIAL("), "{}", plain[0]);
        }
    }

    #[test]
    fn types_of_one_name_and_two_sections_stay_two_types_each_with_its_own_set_and_global_ids() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        // The architectural and the structural column categories each have their own types; both may call one "C 300".
        run(&mut s, "type", json!({"category": "column", "name": "C 300", "width": 300, "depth": 300}));
        run(&mut s, "type", json!({"category": "structuralcolumn", "name": "C 300", "width": 250, "depth": 600}));
        run(&mut s, "column", json!({"at": [0, 0], "type": "C 300"}));
        run(&mut s, "structural_column", json!({"at": [3000, 0], "type": "C 300"}));
        let f = export(&s.model);
        dangling(&f);
        let kinds = of_kind(&f, "IFCCOLUMNTYPE");
        assert_eq!(kinds.len(), 2, "two sections, two types");
        let mut sections: Vec<(String, String)> = kinds
            .iter()
            .map(|k| {
                let (_, profile, _) = section_of(&f, *k);
                (attributes(&profile)[3].clone(), attributes(&profile)[4].clone())
            })
            .collect();
        sections.sort();
        assert_eq!(sections, [("250.".to_string(), "600.".to_string()), ("300.".to_string(), "300.".to_string())]);
        // No two entities of the file share a GlobalId.
        let mut ids = BTreeSet::new();
        for l in f.lines().filter(|l| l.contains("=IFCREL") || l.contains("TYPE(") || l.contains("=IFCCOLUMN(")) {
            let first = attributes(l).swap_remove(0);
            if first.len() == 24 && first.starts_with('\'') {
                assert!(ids.insert(first.clone()), "GlobalId {first} is written twice: {l}");
            }
        }
    }

    #[test]
    fn every_type_object_of_the_sample_house_relates_occurrences_of_its_class_once() {
        let f = sample_ifc();
        dangling(&f);
        let type_classes: BTreeSet<String> = f
            .lines()
            .filter_map(|l| {
                let class = l.split_once('=')?.1.split_once('(')?.0;
                (class.starts_with("IFC") && !class.starts_with("IFCREL") && class.ends_with("TYPE")).then(|| class.to_string())
            })
            .collect();
        let expected = ["IFCCOVERINGTYPE", "IFCDOORTYPE", "IFCFURNITURETYPE", "IFCROOFTYPE", "IFCSLABTYPE", "IFCWALLTYPE", "IFCWINDOWTYPE"];
        assert_eq!(type_classes, expected.map(str::to_string).into_iter().collect::<BTreeSet<_>>());
        let mut listed = BTreeSet::new();
        for class in &type_classes {
            let occurrence = class.strip_suffix("TYPE").unwrap();
            let of_class = of_kind(&f, occurrence);
            for t in of_kind(&f, class) {
                for o in occurrences_of(&f, t) {
                    assert!(of_class.contains(&o), "#{o} is listed by {class} but is not a {occurrence}");
                    assert!(listed.insert(o), "#{o} is related to more than one type");
                }
            }
            // Attributes IFC4 gives the type: thirteen for a door's or a window's, eleven for a furniture's, else ten.
            let count = match class.as_str() {
                "IFCDOORTYPE" | "IFCWINDOWTYPE" => 13,
                "IFCFURNITURETYPE" => 11,
                _ => 10,
            };
            for line in entities(&f, class) {
                assert_eq!(attributes(&line).len(), count, "{line}");
            }
        }
    }

    #[test]
    fn a_structural_elements_common_set_holds_both_what_it_carries_and_its_status() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        let col = run(&mut s, "structural_column", json!({"at": [0, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "modify", json!({"id": col, "params": {"Phase Created": "New Construction"}}));
        let f = export(&s.model);
        let ent = of_kind(&f, "IFCCOLUMN")[0];
        assert_eq!(
            set_of(&f, ent, "Pset_ColumnCommon"),
            ["IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)", "IFCPROPERTYENUMERATEDVALUE('Status',$,(IFCLABEL('NEW')),$)"],
            "one set, not two of the same name"
        );
        assert_eq!(f.matches("'Pset_ColumnCommon'").count(), 1);
        dangling(&f);
    }

    #[test]
    fn a_project_with_no_phasing_gives_no_status() {
        use crate::graphics::testing::{house, run};
        let mut s = Session::new();
        house(&mut s);
        run(&mut s, "structural_column", json!({"at": [0, 0]}));
        let f = export(&s.model);
        assert!(!f.contains("'Status'"), "all of it is new work: nothing to say");
        assert!(!f.contains("'Pset_WallCommon'"));
        assert_eq!(
            set_of(&f, of_kind(&f, "IFCCOLUMN")[0], "Pset_ColumnCommon"),
            ["IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)"]
        );
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

    /// The quantities of set `name` of entity `#ent`, by their names; none when it has no such set.
    fn quantities_of(f: &str, ent: u64, name: &str) -> BTreeMap<String, f64> {
        let body = |n: &str| -> String {
            let line = f.lines().find(|l| l.starts_with(&format!("#{n}="))).unwrap_or_else(|| panic!("no #{n}"));
            line.split_once('=').unwrap().1.trim_end_matches(';').to_string()
        };
        for rel in f.lines().filter(|l| l.contains("=IFCRELDEFINESBYPROPERTIES(")) {
            let (related, set) = rel.split_once("$,$,$,(").unwrap().1.split_once("),#").unwrap();
            if !related.split(',').any(|r| r == format!("#{ent}")) {
                continue;
            }
            let set = body(set.trim_end_matches(';').trim_end_matches(')'));
            if !set.starts_with("IFCELEMENTQUANTITY(") || !set.contains(&format!("'{name}'")) {
                continue;
            }
            let qs = set.rsplit_once(",(").unwrap().1.trim_end_matches(')');
            return qs
                .split(',')
                .map(|q| {
                    // `IFCQUANTITYAREA('NetSideArea',$,$,12.5,$)`
                    let q = body(q.trim_start_matches('#'));
                    let fields: Vec<&str> = q.split_once('(').unwrap().1.split(',').collect();
                    (fields[0].trim_matches('\'').to_string(), fields[3].parse().unwrap())
                })
                .collect();
        }
        BTreeMap::new()
    }

    #[test]
    fn walls_slabs_doors_and_windows_carry_their_base_quantities() {
        use crate::graphics::testing::{house, run};
        let mut s = Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        let window = run(&mut s, "window", json!({"at": [4000, 0]}))["id"].as_u64().unwrap();
        run(
            &mut s,
            "floor",
            json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}),
        );
        let f = export(&s.model);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
        let (m, wall) = (&s.model, walls[0]);
        let Some(Element::Wall(wl)) = m.elements.get(wall) else { panic!("no wall") };
        let (height, thick) = (m.top_z(wl) - m.base_z(wl), m.thickness(wl));
        let size = |id: u64| m.opening_type(m.elements.get(id).unwrap()).unwrap();
        let (d, wn) = (size(door), size(window));
        let sill = match m.elements.get(window) {
            Some(Element::Window(o)) => o.sill,
            _ => panic!("no window"),
        };
        let q = quantities_of(&f, of_kind(&f, "IFCWALL")[0], "Qto_WallBaseQuantities");
        let gross = 8000.0 * height / 1e6;
        let cut = (d.width * d.height + wn.width * wn.height.min(height - sill)) / 1e6;
        assert!(near(q["Length"], 8000.0) && near(q["Width"], thick) && near(q["Height"], height), "{q:?}");
        assert!(near(q["GrossSideArea"], gross), "{q:?}");
        assert!(near(q["NetSideArea"], gross - cut), "less the door and the window: {q:?}");
        assert!(near(q["NetVolume"], (gross - cut) * thick / 1e3), "{q:?}");
        // A wall with nothing in it: net is gross.
        let q = quantities_of(&f, of_kind(&f, "IFCWALL")[1], "Qto_WallBaseQuantities");
        assert!(near(q["NetSideArea"], q["GrossSideArea"]), "{q:?}");
        let q = quantities_of(&f, of_kind(&f, "IFCDOOR")[0], "Qto_DoorBaseQuantities");
        assert!(near(q["Width"], d.width) && near(q["Height"], d.height) && near(q["Area"], d.width * d.height / 1e6), "{q:?}");
        assert!(near(q["Perimeter"], 2.0 * (d.width + d.height)), "{q:?}");
        let q = quantities_of(&f, of_kind(&f, "IFCWINDOW")[0], "Qto_WindowBaseQuantities");
        assert!(near(q["Area"], wn.width * wn.height / 1e6), "{q:?}");
        let slab = *of_kind(&f, "IFCSLAB").last().unwrap();
        let q = quantities_of(&f, slab, "Qto_SlabBaseQuantities");
        assert!(near(q["GrossArea"], 48.0) && near(q["NetArea"], 47.0) && near(q["Perimeter"], 28000.0), "{q:?}");
        assert!(near(q["NetVolume"], 47.0 * q["Width"] / 1e3), "{q:?}");
        dangling(&f);
        // Attached to a roof or a floor, a wall has no single height: only its length and thickness.
        let mut attached = wl.clone();
        attached.attach_top = Some(m.guid(walls[1]));
        let names: Vec<&str> = wall_quantities(m, &attached, &[]).iter().map(|(n, _, _)| *n).collect();
        assert_eq!(names, ["Length", "Width"]);
    }

    /// Every closed shell in `f` is closed: each edge of its faces, from one point to the next round a bound, is run once
    /// each way, so the shell has no gap and its faces turn one way.
    fn closed(f: &str) {
        let bodies: BTreeMap<u64, &str> = f
            .lines()
            .filter(|l| l.starts_with('#'))
            .filter_map(|l| {
                let (head, body) = l.split_once('=')?;
                Some((head[1..].parse().ok()?, body.trim_end_matches(';')))
            })
            .collect();
        let list = |body: &str| -> Vec<u64> {
            let inner = body.split_once("((").map_or("", |(_, r)| r.split("))").next().unwrap_or(""));
            inner.split(',').filter_map(|r| r.trim().trim_start_matches('#').parse().ok()).collect()
        };
        let shells: Vec<&str> = bodies.values().copied().filter(|b| b.starts_with("IFCCLOSEDSHELL(")).collect();
        assert!(!shells.is_empty(), "no closed shell");
        for shell in shells {
            let mut edges: BTreeMap<(u64, u64), usize> = BTreeMap::new();
            for face in list(shell) {
                for bound in list(bodies[&face]) {
                    let b = bodies[&bound];
                    assert!(b.ends_with(",.T.)"), "{b}");
                    let lp: u64 = b.split_once("(#").unwrap().1.split(',').next().unwrap().parse().unwrap();
                    let pts = list(bodies[&lp]);
                    for (a, z) in pts.iter().zip(pts.iter().cycle().skip(1)) {
                        *edges.entry((*a, *z)).or_default() += 1;
                    }
                }
            }
            for (&(a, z), &n) in &edges {
                assert_eq!(n, 1, "edge #{a} to #{z} run {n} times");
                assert!(edges.contains_key(&(z, a)), "edge #{a} to #{z} has no face the other side: {shell}");
            }
        }
    }

    #[test]
    fn a_shaped_slab_is_one_closed_brep_and_sloped_and_shaped_slabs_give_their_volumes() {
        use crate::graphics::testing::run;
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
        let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"boundary": boundary, "openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}))
            ["id"]
            .as_u64()
            .unwrap();
        let shape = json!({"id": f, "points": [{"at": [5000, 3000], "z": -30}], "lines": [{"a": [5000, 3000], "b": [8000, 6000]}]});
        run(&mut s, "edit_shape", shape);
        let text = export(&s.model);
        // One solid: its top's planes, the same planes under them, and its sides round its outline and its opening.
        assert_eq!(of_kind(&text, "IFCFACETEDBREP").len(), 1, "one solid for the slab");
        closed(&text);
        dangling(&text);
        let q = quantities_of(&text, *of_kind(&text, "IFCSLAB").last().unwrap(), "Qto_SlabBaseQuantities");
        let depth = q["Width"];
        assert!(depth > 0.0 && near(q["NetVolume"], 47.0 * depth / 1e3) && near(q["GrossVolume"], 48.0 * depth / 1e3), "{q:?}");
        // A sloped slab: its plan area through its depth plumb.
        let mut s = Session::new();
        run(&mut s, "floor", json!({"boundary": boundary, "slope": 10, "arrow": [[0, 0], [8000, 0]]}));
        let text = export(&s.model);
        let q = quantities_of(&text, *of_kind(&text, "IFCSLAB").last().unwrap(), "Qto_SlabBaseQuantities");
        let plumb = q["Width"];
        assert!(near(q["NetVolume"], 48.0 * plumb / 1e3) && near(q["GrossVolume"], 48.0 * plumb / 1e3), "{q:?}");
    }

    #[test]
    fn a_shaped_slab_over_a_variable_layer_has_a_level_underside_in_ifc() {
        use crate::graphics::testing::run;
        let mut s = Session::new();
        let layers = json!([{"function": "finish", "thickness": 10}, {"function": "substrate", "thickness": 60, "variable": true}, {"thickness": 200}]);
        run(&mut s, "type", json!({"category": "floor", "name": "Terrace", "layers": layers}));
        let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
        let f = run(&mut s, "floor", json!({"boundary": boundary, "type": "Terrace"}))["id"].as_u64().unwrap();
        run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
        let text = export(&s.model);
        closed(&text);
        // 48 m² of 270 mm less the pyramid the drain takes out of its top: 48 m² by 30 mm over 3.
        let q = quantities_of(&text, *of_kind(&text, "IFCSLAB").last().unwrap(), "Qto_SlabBaseQuantities");
        let want = 48.0 * 0.270 - 48.0 * 0.030 / 3.0;
        assert!((q["NetVolume"] - want).abs() < 1e-6 && (q["GrossVolume"] - want).abs() < 1e-6, "{q:?}");
        // Every corner of its underside is at -270 mm.
        assert!(text.contains("IFCCARTESIANPOINT((0.,0.,-270.))") && !text.contains("IFCCARTESIANPOINT((4000.,3000.,-300.))"));
    }
}
