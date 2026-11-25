//! Groups, as Revit's ([groups](../../../docs/areas/groups.md)): a group type holds elements about an origin (`GroupType`), and
//! each group placed is an instance of it whose members are ordinary elements of the project. Model groups
//! (`Model::model_groups`, [`ModelGroup`]) hold model elements and stand on a level; detail groups (`Model::detail_groups`,
//! [`DetailGroup`]) hold what is drawn in one view. A type holds its members in the group's own frame, with what they point
//! at made tokens ([`level_token`], [`view_token`]); [`place`] makes an instance's member from it and [`capture`] a type's
//! member from an instance's. After every command [`keep`] holds each instance to its type. A repeating detail places a
//! detail group's members along a line, drawn as they are placed, never elements.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::ids::{Guid, Id};
use crate::model::{Element, Model, heading};

pub mod commands;
mod keep;
#[cfg(test)]
pub(crate) mod tests;

pub use keep::keep;

/// The most members a group type holds.
pub const MAX_MEMBERS: usize = 10_000;

/// The most instances one placing command makes (one on each level given).
pub const MAX_INSTANCES: usize = 200;

/// The most members a view's repeating details place in all: each repeat places its type's members again, and a view draws
/// every one whenever it is drawn. At 10,000, hatched regions 3 m across take about 0.1 s to draw and 30 m across about 0.5 s
/// (2.8 million primitives) on a machine that is not busy, in the dev build (the core is optimized there too) and in release
/// alike: 0.09 to 0.10 s and 0.43 to 0.51 s in release; a busy machine took 0.13 to 0.29 s and 0.64 to 1.10 s in the dev build.
/// 200,000 took 6 s and 30 million primitives. A member that is a huge circle costs more: it is drawn through 8,192 points,
/// about 0.2 ms to draw and 3 to 5 ms to rasterize in a dev build (8 ms on a busy machine), so a view at the cap of them can
/// take a minute or more to render; the cap stays.
pub const MAX_PLACED: usize = 10_000;

/// What an error refusing a change to a group starts with: front ends ask what to do when they see it ([`is_conflict`]).
pub const CONFLICT: &str = "group conflict";

/// Whether `error` is a command refused because it would change a group from inside ([`CONFLICT`]).
pub fn is_conflict(error: &str) -> bool {
    error.split_once(": ").is_some_and(|(_, rest)| rest.starts_with(CONFLICT)) || error.starts_with(CONFLICT)
}

/// What kind of group an element can be in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Model,
    Detail,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Detail => "detail",
        }
    }

    /// The category its instances are of.
    pub fn category(self) -> &'static str {
        match self {
            Kind::Model => "modelgroup",
            Kind::Detail => "detailgroup",
        }
    }
}

/// The kind of group `e` can be in, or why it can be in none.
pub fn kind_of(e: &Element) -> Result<Kind, String> {
    use Element as E;
    match e {
        E::Wall(_)
        | E::Floor(_)
        | E::Ceiling(_)
        | E::Shaft(_)
        | E::Mass(_)
        | E::Door(_)
        | E::Window(_)
        | E::Room(_)
        | E::RoomSeparator(_)
        | E::PropertyLine(_)
        | E::Column(_)
        | E::StructuralColumn(_)
        | E::Footing(_)
        | E::Beam(_)
        | E::StripFooting(_)
        | E::Pipe(_)
        | E::Duct(_)
        | E::Conduit(_)
        | E::CableTray(_)
        | E::Wire(_)
        | E::DownConductor(_)
        | E::GroundConductor(_)
        | E::Cable(_)
        | E::Rebar(_)
        | E::Stair(_)
        | E::Ramp(_)
        | E::Railing(_)
        | E::Roof(_) => Ok(Kind::Model),
        E::Component(c) if c.adaptive.is_some() => {
            Err("an adaptive component is placed by its points, which a group doesn't move yet; group what it stands among without it"
                .into())
        }
        E::Component(c) if c.host.is_some() => {
            Err("a component on a work plane or a face is placed by what it is on, which a group doesn't carry yet; place one on a level"
                .into())
        }
        E::Component(_) => Ok(Kind::Model),
        E::Text(_) | E::DetailLine(_) | E::FilledRegion(_) | E::Dimension(_) => Ok(Kind::Detail),
        E::ModelGroup(_) | E::DetailGroup(_) => Err("a group can't hold another group yet; ungroup it first".into()),
        E::Level(_) | E::Grid(_) | E::RefPlane(_) => Err("a datum is no group's: groups stand on levels and among grids".into()),
        E::Tag(_) | E::Spot(_) => Err("a tag or a spot reads a model element, which no detail group may hold".into()),
        E::RepeatingDetail(_) => Err("a repeating detail is placed by its own line; ungroup its group or leave it out".into()),
        E::Dimension3D(_) => Err("a 3D view's dimension is that view's".into()),
        E::Section(_)
        | E::Elevation(_)
        | E::Sheet(_)
        | E::Viewport(_)
        | E::Schedule(_)
        | E::View3D(_)
        | E::PlanView(_)
        | E::DetailView(_)
        | E::PanelSchedule(_)
        | E::RebarDetail(_) => Err("a view, a sheet or what places a view is no group's".into()),
        E::Link(_) | E::PointCloud(_) | E::CadImport(_) | E::Image(_) => {
            Err("a link, a scan, a CAD drawing or an image is placed on its own, not in a group".into())
        }
        E::Toposolid(_) => Err("a toposolid is the site's ground, placed on its own, not in a group".into()),
        E::Circuit(_) => Err("a circuit has no place: group its devices".into()),
        E::ProjectInfo(_) => Err("the project's information is no group's".into()),
    }
}

/// The kind of group elements `ids` of `m` can make together: all model elements, or all of one view's annotation.
pub fn kind_of_all(m: &Model, ids: &[Id]) -> Result<Kind, String> {
    let mut kind: Option<(Kind, Id)> = None;
    for &id in ids {
        let e = m.elements.get(id).ok_or_else(|| format!("no element {id}"))?;
        let k = kind_of(e).map_err(|why| format!("{} {id} can't be grouped: {why}", e.category()))?;
        match kind {
            Some((was, other)) if was != k => {
                return Err(format!(
                    "{} {other} is a model element and {} {id} is drawn in a view: a group holds one kind, a model group model \
                     elements and a detail group a view's lines, regions, notes and dimensions",
                    m.elements.get(other).map_or("element", Element::category),
                    e.category()
                ));
            }
            _ => kind = Some((k, id)),
        }
    }
    kind.map(|(k, _)| k).ok_or_else(|| "nothing to group".into())
}

/// What of a selection each kind of group could hold: its model elements, and its detail elements in the view of the first
/// of them; neither holds what is in a group already or what no group holds. What Create Group offers for a selection that is
/// not all of one kind (`create_group {only}`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Split {
    pub model: Vec<Id>,
    pub detail: Vec<Id>,
}

impl Split {
    /// The elements of `kind`.
    pub fn of(&self, kind: Kind) -> &[Id] {
        match kind {
            Kind::Model => &self.model,
            Kind::Detail => &self.detail,
        }
    }
}

/// [`Split`] of `ids` in `m`, each kept in the order given.
pub fn split(m: &Model, ids: &[Id]) -> Split {
    let mut out = Split::default();
    let mut view = None;
    for &id in ids {
        let Some(e) = m.elements.get(id) else { continue };
        if group_of(m, id).is_some() {
            continue;
        }
        match kind_of(e) {
            Ok(Kind::Model) if !out.model.contains(&id) => out.model.push(id),
            Ok(Kind::Detail) => {
                let here = m.annotation_plan(e);
                if here.is_some() && *view.get_or_insert(here) == here && !out.detail.contains(&id) {
                    out.detail.push(id);
                }
            }
            _ => {}
        }
    }
    out
}

/// What Edit Group lets a front end pick: the group being edited (`group`, of `kind`, on its level or in its view `on`) and the
/// elements that are its now, its members less those removed and with those added or drawn meanwhile. Everything else is
/// greyed and doesn't pick, but for Add, which picks what may join ([`Scope::addable`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub group: Id,
    pub kind: Kind,
    pub on: Guid,
    pub members: BTreeSet<Id>,
}

impl Scope {
    /// The scope of Edit Group `ed` in `m`; none when its group is gone.
    pub fn of(m: &Model, ed: &commands::GroupEdit) -> Option<Scope> {
        let group = m.elements.id(ed.group)?;
        let inst = m.elements.get(group).and_then(instance)?;
        let removed: BTreeSet<Guid> = ed.removed.iter().copied().collect();
        let members = inst
            .members
            .iter()
            .filter(|x| !x.excluded && !removed.contains(&x.element))
            .map(|x| x.element)
            .chain(ed.added.iter().copied())
            .filter_map(|g| m.elements.id(g))
            .collect();
        Some(Scope { group, kind: inst.kind, on: inst.on, members })
    }

    /// Whether element `id` is the group's to change: drawn as it is, picked, moved.
    pub fn editable(&self, id: Id) -> bool {
        self.members.contains(&id)
    }

    /// What of `ids` (in order, each once) Add may take into the group: elements of its kind (in its view, for a detail group)
    /// in no other group and not its own already.
    pub fn addable(&self, m: &Model, ids: &[Id]) -> Vec<Id> {
        let mut out: Vec<Id> = Vec::new();
        for &id in ids {
            if self.editable(id) || id == self.group || out.contains(&id) {
                continue;
            }
            let Some(e) = m.elements.get(id) else { continue };
            if kind_of(e) != Ok(self.kind) || (self.kind == Kind::Detail && m.annotation_plan(e) != Some(self.on)) {
                continue;
            }
            if group_of(m, id).is_some_and(|g| g != self.group) {
                continue;
            }
            out.push(id);
        }
        out
    }
}

/// A storey of a model group type: how many levels above the group's own a level its members name is, and how far above it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Storey {
    pub above: usize,
    pub height: f64,
}

/// A group type: what each one placed holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupType {
    pub members: Vec<Member>,
    /// A model group's: the storeys its level tokens stand for, by token (the first, the group's own level, at 0).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<Storey>,
}

/// An element of a group type, in the group's frame ([`capture`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// Its id and GUID, as an element's: given when it's made, kept while it stays where it is in its list.
    #[serde(default, skip_serializing_if = "crate::ids::unset")]
    pub id: Id,
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub guid: Guid,
    pub element: Element,
}

impl crate::library::HasPart for Member {
    fn ident(&mut self) -> (&mut Id, &mut Guid) {
        (&mut self.id, &mut self.guid)
    }
}

/// One member of an instance: the type's member (by its GUID) and the element that is it here, or that it is excluded here.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub member: Guid,
    /// None while excluded, or before it is placed.
    #[serde(default, skip_serializing_if = "Guid::is_nil")]
    pub element: Guid,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub excluded: bool,
}

/// A detail group placed in a view: type `type_name`'s members, their origin at `at`, turned `angle` degrees
/// counter-clockwise, and first, with `mirrored`, mirrored across their x axis; `members` are the elements that are them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetailGroup {
    pub view: Guid,
    #[serde(rename = "type")]
    pub type_name: String,
    pub at: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Slot>,
}

/// A model group placed on `level`: as a [`DetailGroup`], and the levels its type's level tokens stand for here (`levels`,
/// the first `level`). A `flexible` one lets a member's top follow a level that moved, while the rest of it stays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGroup {
    pub level: Guid,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<Guid>,
    #[serde(rename = "type")]
    pub type_name: String,
    pub at: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Slot>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flexible: bool,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// Where a group stands and which way: what takes its type's frame to the project.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub at: P2,
    pub angle: f64,
    pub mirrored: bool,
}

impl Placement {
    pub const NONE: Placement = Placement { at: P2(0.0, 0.0), angle: 0.0, mirrored: false };

    /// The group's frame to the project: mirrored, turned, then moved, to the picometre ([`nano`]). (Not to the micron: a
    /// note's angle is worked out from two points it places, a metre apart, and a micron of either would turn it by a
    /// hundred-thousandth of a degree.)
    pub fn forward(self) -> impl Fn(P2) -> P2 {
        let f = placing(self.at, self.angle, self.mirrored);
        move |p| nano(f(p))
    }

    /// The project to the group's frame.
    pub fn backward(self) -> impl Fn(P2) -> P2 {
        let (u, v) = (heading(self.angle), heading(self.angle + 90.0));
        move |q: P2| {
            let d = q - self.at;
            let (x, y) = (d.dot(u), d.dot(v));
            nano(P2(x, if self.mirrored { -y } else { y }))
        }
    }
}

/// `p` to the picometre (a millionth of a micron): what a turn leaves in the last digits goes, and a note's angle, worked
/// out from points a metre apart, stays true to the billionth of a degree it is kept to.
fn nano(p: P2) -> P2 {
    P2((p.0 * 1e9).round() / 1e9, (p.1 * 1e9).round() / 1e9)
}

/// An instance of a group, as either kind holds it.
#[derive(Clone, Copy, Debug)]
pub struct Instance<'a> {
    pub kind: Kind,
    pub type_name: &'a str,
    pub placement: Placement,
    /// Its level (a model group's) or its view (a detail group's).
    pub on: Guid,
    pub levels: &'a [Guid],
    pub members: &'a [Slot],
    pub flexible: bool,
}

/// `e` as a group instance, if it is one.
pub fn instance(e: &Element) -> Option<Instance<'_>> {
    match e {
        Element::ModelGroup(g) => Some(Instance {
            kind: Kind::Model,
            type_name: &g.type_name,
            placement: Placement { at: g.at, angle: g.angle, mirrored: g.mirrored },
            on: g.level,
            levels: &g.levels,
            members: &g.members,
            flexible: g.flexible,
        }),
        Element::DetailGroup(g) => Some(Instance {
            kind: Kind::Detail,
            type_name: &g.type_name,
            placement: Placement { at: g.at, angle: g.angle, mirrored: g.mirrored },
            on: g.view,
            levels: &[],
            members: &g.members,
            flexible: false,
        }),
        _ => None,
    }
}

/// A group instance's slots, to change.
pub fn slots_mut(e: &mut Element) -> Option<&mut Vec<Slot>> {
    match e {
        Element::ModelGroup(g) => Some(&mut g.members),
        Element::DetailGroup(g) => Some(&mut g.members),
        _ => None,
    }
}

/// The types of groups of `kind`.
pub fn types(m: &Model, kind: Kind) -> &BTreeMap<String, GroupType> {
    match kind {
        Kind::Model => &m.model_groups,
        Kind::Detail => &m.detail_groups,
    }
}

pub fn types_mut(m: &mut Model, kind: Kind) -> &mut BTreeMap<String, GroupType> {
    match kind {
        Kind::Model => &mut m.model_groups,
        Kind::Detail => &mut m.detail_groups,
    }
}

/// Whether `m` has any group types, and so may have instances.
pub fn any(m: &Model) -> bool {
    !m.model_groups.is_empty() || !m.detail_groups.is_empty()
}

/// Every group instance of `m`, by id.
pub fn instances(m: &Model) -> Vec<Id> {
    if !any(m) {
        return Vec::new();
    }
    let look = m.lookup();
    let mut ids: Vec<Id> = look.rare("modelgroup").chain(look.rare("detailgroup")).collect();
    ids.sort_unstable();
    ids
}

/// The instance element `id` of `m` is a member of, if any.
pub fn group_of(m: &Model, id: Id) -> Option<Id> {
    if !any(m) {
        return None;
    }
    let g = m.elements.guid(id)?;
    m.lookup().referrers(&g).find(|r| m.elements.get(*r).and_then(instance).is_some_and(|i| i.members.iter().any(|s| s.element == g)))
}

/// The group holding a member among `ids` to its type: one not in Edit Group (`editing`, its scope). Of such members a front end
/// changes only their values of the parameters that vary by group instance ([`varying`]); the rest change in Edit Group.
pub fn held(m: &Model, ids: &[Id], editing: Option<&Scope>) -> Option<Id> {
    if !any(m) {
        return None;
    }
    ids.iter().filter(|id| editing.is_none_or(|s| !s.editable(**id))).find_map(|id| group_of(m, *id))
}

/// The elements of instance `id` of `m`, those it has here (none excluded).
pub fn members(m: &Model, id: Id) -> Vec<Id> {
    m.elements
        .get(id)
        .and_then(instance)
        .map(|i| i.members.iter().filter(|s| !s.excluded).filter_map(|s| m.elements.id(s.element)).collect())
        .unwrap_or_default()
}

/// `ids` and the members of any instance among them, each once, in order.
pub fn with_members(m: &Model, ids: &[Id]) -> Vec<Id> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        if seen.insert(id) {
            out.push(id);
        }
        for member in members(m, id) {
            if seen.insert(member) {
                out.push(member);
            }
        }
    }
    out
}

/// What a pick of `picks` (nearest first) takes, as Revit's groups pick: a member's group before the member, so a click takes
/// the group and Tab steps on to the element in it. In Edit Group (`editing`), only what is the group's picks, as itself.
pub fn group_picks(m: &Model, picks: &[Id], editing: Option<&Scope>) -> Vec<Id> {
    if let Some(scope) = editing {
        let mut out: Vec<Id> = Vec::with_capacity(picks.len());
        for &id in picks {
            if scope.editable(id) && !out.contains(&id) {
                out.push(id);
            }
        }
        return out;
    }
    if !any(m) {
        return picks.to_vec();
    }
    let mut out: Vec<Id> = Vec::with_capacity(picks.len() + 1);
    for &id in picks {
        if let Some(g) = group_of(m, id)
            && !out.contains(&g)
        {
            out.push(g);
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// What a box selection of `ids` takes: a member's group in its place (once); in Edit Group (`editing`), only what is the
/// group's, as itself.
pub fn as_picked(m: &Model, ids: &[Id], editing: Option<&Scope>) -> Vec<Id> {
    if editing.is_some() {
        return group_picks(m, ids, editing);
    }
    if !any(m) {
        return ids.to_vec();
    }
    let mut out: Vec<Id> = Vec::with_capacity(ids.len());
    for &id in ids {
        let one = group_of(m, id).unwrap_or(id);
        if !out.contains(&one) {
            out.push(one);
        }
    }
    out
}

/// What a group of type `name` of `kind` placed at `p` on `on` (a level or a view) would hold, for a front end to draw
/// before it is placed: none where it can't stand there.
pub fn preview(m: &Model, kind: Kind, name: &str, on: Guid, p: Placement) -> Vec<Element> {
    let Some(t) = types(m, kind).get(name) else { return Vec::new() };
    let levels = match kind {
        Kind::Model => match levels_for(m, name, t, on) {
            Ok(l) => l,
            Err(_) => return Vec::new(),
        },
        Kind::Detail => Vec::new(),
    };
    // Its members point at one another by stand-ins that are no elements yet: what draws on its own still draws.
    let elements: BTreeMap<Guid, Guid> = t.members.iter().map(|x| (x.guid, Guid::new())).collect();
    let inst = Instance { kind, type_name: name, placement: p, on, levels: &levels, members: &[], flexible: true };
    let f = frame_of(m, &inst, &elements);
    t.members.iter().filter_map(|x| place(m, t, x, &f).ok()).collect()
}

const TOKEN_SEED: u64 = 0x6772_6f75_7073;

/// What a type's member names in place of its detail group's view.
pub fn view_token() -> Guid {
    Guid::seeded(TOKEN_SEED, 0)
}

/// What a type's member names in place of the level of its detail group's view (a dimension's level).
pub fn view_level_token() -> Guid {
    Guid::seeded(TOKEN_SEED, 1)
}

/// What a model group type's member names in place of its `k`th level ([`GroupType::levels`]).
pub fn level_token(k: usize) -> Guid {
    Guid::seeded(TOKEN_SEED, 2 + k as u64)
}

/// The project's levels, lowest first (by elevation, then id): what "storeys above" counts.
pub fn storeys(m: &Model) -> Vec<Guid> {
    let mut v: Vec<(f64, Id)> = m.levels().into_iter().map(|(id, l)| (l.elevation, id)).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|(_, id)| m.guid(id)).collect()
}

/// The levels a model group of type `t` on level `base` names, by token: the level as many storeys up as each storey says.
pub fn levels_for(m: &Model, name: &str, t: &GroupType, base: Guid) -> Result<Vec<Guid>, String> {
    let all = storeys(m);
    let at = all.iter().position(|g| *g == base).ok_or_else(|| format!("{} isn't a level", m.shown(base)))?;
    t.levels
        .iter()
        .map(|s| {
            all.get(at + s.above).copied().ok_or_else(|| {
                format!(
                    "model group `{name}` reaches {} level{} above its own, and {} has {} above it: add the levels, or place it lower",
                    s.above,
                    if s.above == 1 { "" } else { "s" },
                    level_name(m, base),
                    all.len() - at - 1
                )
            })
        })
        .collect()
}

fn level_name(m: &Model, g: Guid) -> String {
    m.level(g).map_or_else(|| m.shown(g), |l| format!("`{}`", l.name))
}

/// The tokens of type `t` some member stands on (its base level): what even a flexible group keeps at its storey's height.
fn base_tokens(t: &GroupType) -> BTreeSet<usize> {
    let tokens: BTreeMap<Guid, usize> = (0..t.levels.len()).map(|k| (level_token(k), k)).collect();
    t.members
        .iter()
        .filter_map(|x| {
            let mut first = None;
            x.element.clone().refs_mut(&mut |g| {
                if first.is_none()
                    && let Some(k) = tokens.get(g)
                {
                    first = Some(*k);
                }
            });
            first
        })
        .collect()
}

/// How far the levels `levels` of an instance of `t` are from what its storeys say: each level whose height above the first
/// isn't the type's, with that height, when every one counts (`all`) or only the ones some member stands on.
pub fn storeys_off(m: &Model, t: &GroupType, levels: &[Guid], all: bool) -> Vec<(usize, f64)> {
    let Some(&first) = levels.first() else { return Vec::new() };
    let base = m.elevation(first);
    let kept = if all { (0..t.levels.len()).collect() } else { base_tokens(t) };
    t.levels
        .iter()
        .zip(levels)
        .enumerate()
        .filter(|(k, _)| kept.contains(k))
        .filter_map(|(k, (s, g))| {
            let h = m.elevation(*g) - base;
            ((h - s.height).abs() > TOL).then_some((k, h))
        })
        .collect()
}

/// How near two numbers of a member must be to be the same, mm or degrees.
const TOL: f64 = 0.01;

/// The fields a member holds of its own in each instance, never a change to the group: a door's or a window's mark, a room's
/// number. Its values of the parameters that vary by group instance are its own too ([`varying`]).
pub const OWN: &[&str] = &["mark", "number"];

/// What tokens and members of a placed instance stand for: the elements and levels (or view) they are here.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub kind: Option<Kind>,
    pub placement: Option<Placement>,
    /// Token or member GUID → what it is here.
    pub map: BTreeMap<Guid, Guid>,
    /// A detail group's view, and whether it is a level's own plan.
    pub view: Option<(Guid, bool)>,
}

impl Frame {
    fn new(kind: Kind, placement: Placement) -> Frame {
        Frame { kind: Some(kind), placement: Some(placement), ..Frame::default() }
    }
}

/// The level a detail group's view looks at: a plan's level, else the lowest.
fn view_level(m: &Model, view: Guid) -> Guid {
    match m.plan_level(view) {
        Some(l) => m.guid(l),
        None => storeys(m).first().copied().unwrap_or_default(),
    }
}

/// The frame of instance `inst` of type `t`, its members' elements as `elements` (member GUID → element GUID).
fn frame_of(m: &Model, inst: &Instance, elements: &BTreeMap<Guid, Guid>) -> Frame {
    let mut f = Frame::new(inst.kind, inst.placement);
    f.map.extend(elements.iter().map(|(a, b)| (*a, *b)));
    match inst.kind {
        Kind::Model => f.map.extend(inst.levels.iter().enumerate().map(|(k, g)| (level_token(k), *g))),
        Kind::Detail => {
            f.view = Some((inst.on, matches!(m.elements.get(inst.on), Some(Element::Level(_)))));
            f.map.insert(view_token(), inst.on);
            // A type made before view tokens held its members in no view.
            f.map.insert(Guid::default(), inst.on);
            f.map.insert(view_level_token(), view_level(m, inst.on));
        }
    }
    f
}

/// Element `e` taken by `f`, mirrored if `mirror`, as Move, Rotate and Mirror take elements; a door or a window has no plan
/// place of its own (its offset along its wall flips with a mirror, `host_len` its wall's length), nor does a railing on a
/// stair.
fn moved(
    m: &Model,
    e: Element,
    f: &dyn Fn(P2) -> P2,
    (mirror, turn): (bool, f64),
    host_len: &dyn Fn(Guid) -> Option<f64>,
) -> Result<Element, String> {
    match e {
        Element::Door(o) if mirror => Ok(Element::Door(flipped(o, host_len))),
        Element::Window(o) if mirror => Ok(Element::Window(flipped(o, host_len))),
        e @ (Element::Door(_) | Element::Window(_)) => Ok(e),
        Element::Railing(mut r) if r.host.is_some() => {
            if mirror {
                r.side = r.side.map(|s| match s {
                    crate::model::Side::Left => crate::model::Side::Right,
                    crate::model::Side::Right => crate::model::Side::Left,
                });
            }
            Ok(Element::Railing(r))
        }
        Element::Text(n) if mirror => {
            let was = n.angle;
            let mo = crate::commands::Motion { map: f, slide: None, mirror };
            let mut e = crate::commands::transform("group", m, Element::Text(n), 0, &mo, &BTreeSet::new())?;
            // A mirror leaves a note reading the way it did (`transform`); the group's turn still turns it.
            if let Element::Text(n) = &mut e {
                n.angle = (was + turn).rem_euclid(360.0);
            }
            Ok(e)
        }
        e => {
            let mo = crate::commands::Motion { map: f, slide: None, mirror };
            crate::commands::transform("group", m, e, 0, &mo, &BTreeSet::new())
        }
    }
}

/// A door or a window of a mirrored wall, which runs back the other way: as far from its other end, on the other jamb.
fn flipped(mut o: crate::model::Opening, host_len: &dyn Fn(Guid) -> Option<f64>) -> crate::model::Opening {
    if let Some(len) = host_len(o.host) {
        o.offset = len - o.offset;
    }
    o.flip_hand = !o.flip_hand;
    o
}

/// Element `e` with every GUID it points at swapped by `map` (left as it is where `map` has none).
pub(crate) fn swapped(mut e: Element, map: &BTreeMap<Guid, Guid>) -> Element {
    e.refs_mut(&mut |g| {
        if let Some(to) = map.get(g) {
            *g = *to;
        }
    });
    e
}

/// A dimension of a detail group, as its type holds it: in the group's view (`view_token`), at its level.
fn dimension_to_type(mut e: Element) -> Element {
    if let Element::Dimension(d) = &mut e {
        d.view = Some(view_token());
        d.level = view_level_token();
    }
    e
}

/// A dimension placed in `view`: a level's plan names only its level, another view itself and its level.
fn dimension_placed(mut e: Element, f: &Frame) -> Element {
    if let (Element::Dimension(d), Some((view, plan))) = (&mut e, f.view) {
        if plan {
            (d.view, d.level) = (None, view);
        } else {
            d.view = Some(view);
        }
    }
    e
}

/// Member `x` of type `t`, as instance frame `f` has it: its tokens and members swapped for what they are there, then moved,
/// turned and mirrored into place.
pub fn place(m: &Model, t: &GroupType, x: &Member, f: &Frame) -> Result<Element, String> {
    let p = f.placement.unwrap_or(Placement::NONE);
    // A door's wall in the type: its length is its length placed.
    let host_len = |g: Guid| -> Option<f64> {
        t.members.iter().find(|y| y.guid == g).and_then(|y| match &y.element {
            Element::Wall(w) => Some(crate::walls::Axis::of(w).length()),
            _ => None,
        })
    };
    // Its own hosts in the type are named by member GUIDs: look at them before they're swapped.
    let host = match &x.element {
        Element::Door(o) | Element::Window(o) => Some(o.host),
        _ => None,
    };
    let len = host.and_then(host_len);
    let e = swapped(x.element.clone(), &f.map);
    let e = dimension_placed(e, f);
    let fwd = p.forward();
    let e = moved(m, e, &fwd, (p.mirrored, p.angle), &|_| len)?;
    Ok(e)
}

/// Element `e` of an instance at `p`, as its type would hold it: what it names swapped by `map` (elements to members, levels
/// or the view to tokens), and taken back to the group's frame.
pub fn capture(m: &Model, e: &Element, p: Placement, map: &BTreeMap<Guid, Guid>, kind: Kind) -> Result<Element, String> {
    let len = match e {
        Element::Door(o) | Element::Window(o) => m.wall(o.host).map(|w| crate::walls::Axis::of(w).length()),
        _ => None,
    };
    let back = p.backward();
    let mut c = moved(m, e.clone(), &back, (p.mirrored, -p.angle), &|_| len)?;
    // A mirrored note was turned by the group's angle as well as kept reading the right way: take the turn back.
    if let (Element::Text(n), Element::Text(was), true) = (&mut c, e, p.mirrored) {
        n.angle = (was.angle - p.angle).rem_euclid(360.0);
    }
    let mut c = swapped(c, map);
    if kind == Kind::Detail {
        c = dimension_to_type(c);
    }
    // Its parts get ids of their own again when it is placed.
    if let Some(lines) = c.sketch_mut() {
        lines.iter_mut().for_each(|l| (l.id, l.guid) = (0, Guid::default()));
    }
    if let Some(sh) = c.shape_mut() {
        sh.points.iter_mut().for_each(|x| (x.id, x.guid) = (0, Guid::default()));
        sh.lines.iter_mut().for_each(|x| (x.id, x.guid) = (0, Guid::default()));
    }
    // No mass's floor or face: a group's member is its own.
    if let Some(params) = c.params_mut() {
        crate::massing::detach(params);
    }
    Ok(c)
}

/// What elements `ids` point at that a group of them couldn't hold: what isn't among them, nor a level (a model group's), nor
/// their view and its level (a detail group's).
pub(crate) fn outside(m: &Model, ids: &[Id], kind: Kind, view: Option<Guid>) -> Result<(), String> {
    let inside: BTreeSet<Guid> = ids.iter().map(|id| m.guid(*id)).collect();
    let view_level = view.map(|v| view_level(m, v));
    for &id in ids {
        let Some(e) = m.elements.get(id) else { continue };
        for g in e.refs() {
            if inside.contains(&g) {
                continue;
            }
            let ok = match kind {
                Kind::Model => m.level(g).is_some(),
                Kind::Detail => Some(g) == view || Some(g) == view_level,
            };
            if ok {
                continue;
            }
            let what =
                m.elements.id(g).and_then(|o| m.elements.get(o).map(|x| format!("{} {o}", x.category()))).unwrap_or_else(|| m.shown(g));
            return Err(match e {
                Element::Dimension(_) => format!(
                    "dimension {id} measures {what}, which isn't in the group: a dimension is grouped only with the lines and regions \
                     it measures; select them too, or leave the dimension out"
                ),
                _ => format!(
                    "{} {id} points at {what}, which isn't in the group: select it too, or leave {} {id} out",
                    e.category(),
                    e.category()
                ),
            });
        }
    }
    Ok(())
}

/// The plan points that place element `e`: its ends, its corners, its point. (Not what Move would take through its map,
/// which probes a curve's directions a kilometre out.)
pub fn points_of(m: &Model, e: &Element) -> Vec<P2> {
    match e {
        Element::Wall(w) => vec![w.start, w.end],
        Element::Room(r) => vec![r.point],
        Element::Dimension(d) => vec![d.at],
        Element::Text(n) => vec![n.at],
        Element::Door(_) | Element::Window(_) => Vec::new(),
        Element::Rebar(rb) => rb.path.iter().map(|q| P2(q[0], q[1])).collect(),
        Element::Component(c) => vec![c.at],
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => vec![c.at],
        Element::Stair(st) => vec![st.start],
        Element::Ramp(r) => vec![r.start],
        e => crate::align::element_lines(m, e).into_iter().flat_map(|(a, b)| [a, b]).collect(),
    }
}

/// The middle of the plan points of elements `ids`: a new group's origin. None when they have no point.
pub fn middle(m: &Model, ids: &[Id]) -> Option<P2> {
    let pts: Vec<P2> = ids.iter().filter_map(|id| m.elements.get(*id)).flat_map(|e| points_of(m, e)).filter(|p| p.is_finite()).collect();
    if pts.is_empty() {
        return None;
    }
    let (lo, hi) = pts.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
        (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
    });
    Some(crate::commands::micron((lo + hi) * 0.5))
}

/// A new type of `kind` made of elements `ids` of `m`, as an instance at `p` on `on` (a level or a view) holds them: the type,
/// the levels its tokens stand for (a model group's) and the slots that make `ids` its members.
pub fn type_of(m: &Model, ids: &[Id], kind: Kind, on: Guid, p: Placement) -> Result<(GroupType, Vec<Guid>, Vec<Slot>), String> {
    type_keeping(m, ids, kind, on, p, &BTreeMap::new())
}

/// [`type_of`], the members of elements already in `keep` (element → member GUID) keeping their member GUIDs.
pub fn type_keeping(
    m: &Model,
    ids: &[Id],
    kind: Kind,
    on: Guid,
    p: Placement,
    keep: &BTreeMap<Guid, Guid>,
) -> Result<(GroupType, Vec<Guid>, Vec<Slot>), String> {
    if ids.is_empty() {
        return Err("a group holds at least one element".into());
    }
    if ids.len() > MAX_MEMBERS {
        return Err(format!("{} elements; a group holds at most {MAX_MEMBERS}", ids.len()));
    }
    if kind_of_all(m, ids)? != kind {
        return Err(format!("these elements make a {} group, not a {} one", kind_of_all(m, ids)?.name(), kind.name()));
    }
    let view = (kind == Kind::Detail).then_some(on);
    if kind == Kind::Detail {
        for &id in ids {
            let e = m.elements.get(id).ok_or_else(|| format!("no element {id}"))?;
            if m.annotation_plan(e) != Some(on) {
                return Err(format!("{} {id} is in another view: a detail group's members are all in one", e.category()));
            }
        }
    }
    outside(m, ids, kind, view)?;
    let mut map: BTreeMap<Guid, Guid> = BTreeMap::new();
    let mut slots = Vec::with_capacity(ids.len());
    for &id in ids {
        let g = m.guid(id);
        let member = keep.get(&g).copied().unwrap_or_else(Guid::new);
        map.insert(g, member);
        slots.push(Slot { member, element: g, excluded: false });
    }
    let (levels, storeys) = match kind {
        Kind::Model => {
            let all = storeys(m);
            let base_at = all.iter().position(|g| *g == on).ok_or_else(|| format!("{} isn't a level", m.shown(on)))?;
            let mut used: BTreeSet<usize> = BTreeSet::new();
            used.insert(base_at);
            for &id in ids {
                if let Some(e) = m.elements.get(id) {
                    for g in e.refs() {
                        if let Some(k) = all.iter().position(|x| *x == g) {
                            if k < base_at {
                                return Err(format!(
                                    "{} {id} names {}, below the group's own level {}: stand the group on the lowest level its members name",
                                    e.category(),
                                    level_name(m, g),
                                    level_name(m, on)
                                ));
                            }
                            used.insert(k);
                        }
                    }
                }
            }
            let base = m.elevation(on);
            let levels: Vec<Guid> = used.iter().map(|k| all[*k]).collect();
            let storeys: Vec<Storey> = used.iter().map(|k| Storey { above: k - base_at, height: m.elevation(all[*k]) - base }).collect();
            (levels, storeys)
        }
        Kind::Detail => (Vec::new(), Vec::new()),
    };
    match kind {
        Kind::Model => map.extend(levels.iter().enumerate().map(|(k, g)| (*g, level_token(k)))),
        Kind::Detail => {
            map.insert(on, view_token());
            map.insert(view_level(m, on), view_level_token());
        }
    }
    let mut members = Vec::with_capacity(ids.len());
    for (&id, slot) in ids.iter().zip(&slots) {
        let e = m.elements.get(id).ok_or_else(|| format!("no element {id}"))?;
        let element = capture(m, e, p, &map, kind)?;
        members.push(Member { id: 0, guid: slot.member, element });
    }
    Ok((GroupType { members, levels: storeys }, levels, slots))
}

/// The members of element `e` whose values are its own in each instance, from `was`: its numbers ([`OWN`]) and its values of
/// the parameters `vary` names, those that vary by group instance ([`varying`]), had or not.
fn keep_own(vary: &BTreeSet<String>, e: &mut Element, was: &Element) {
    match (&mut *e, was) {
        (Element::Door(o) | Element::Window(o), Element::Door(w) | Element::Window(w)) => o.mark.clone_from(&w.mark),
        (Element::Room(r), Element::Room(w)) => r.number.clone_from(&w.number),
        _ => {}
    }
    let had = was.params();
    if let Some(params) = e.params_mut().filter(|_| !vary.is_empty()) {
        for n in vary {
            match had.and_then(|p| p.get(n)) {
                Some(v) => params.insert(n.clone(), v.clone()),
                None => params.remove(n),
            };
        }
    }
}

/// The names of `m`'s parameters whose values vary by group instance (Revit's Values can vary by group instance): a member's
/// value of one is its own in each instance, never a change to its group. The rest are aligned per group type.
pub fn varying(m: &Model) -> BTreeSet<String> {
    m.parameters.iter().filter(|(_, p)| p.vary_by_group).map(|(n, _)| n.clone()).collect()
}

/// Every group type's members, of both kinds, as elements to change: what changes every element's values of a parameter
/// (renamed, taken away) changes these too, so groups stay as their types have them.
pub fn member_elements_mut(m: &mut Model) -> impl Iterator<Item = &mut Element> {
    m.model_groups.values_mut().chain(m.detail_groups.values_mut()).flat_map(|t| t.members.iter_mut().map(|x| &mut x.element))
}

/// Parameter `name` aligned per group type: each instance's members take their type's value of it, or have none where it has
/// none. How many values that changed.
pub fn align(m: &mut Model, name: &str) -> usize {
    let mut set: Vec<(Id, Option<serde_json::Value>)> = Vec::new();
    for id in instances(m) {
        let Some(inst) = m.elements.get(id).and_then(instance) else { continue };
        let Some(t) = types(m, inst.kind).get(inst.type_name) else { continue };
        for x in &t.members {
            let Some(e) = inst.members.iter().find(|s| s.member == x.guid && !s.excluded).and_then(|s| m.elements.id(s.element)) else {
                continue;
            };
            let want = x.element.params().and_then(|p| p.get(name)).cloned();
            let have = m.elements.get(e).and_then(Element::params).and_then(|p| p.get(name)).cloned();
            if want != have && m.elements.get(e).and_then(Element::params).is_some() {
                set.push((e, want));
            }
        }
    }
    for (id, v) in &set {
        if let Some(params) = m.elements.get_mut(*id).and_then(Element::params_mut) {
            match v {
                Some(v) => params.insert(name.to_string(), v.clone()),
                None => params.remove(name),
            };
        }
    }
    set.len()
}

/// Element `e`, as it is to be held to its member: as JSON with its parts' ids and GUIDs and its own fields ([`OWN`]) left out.
fn comparable(e: &Element) -> serde_json::Value {
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(o) => {
                o.retain(|k, x| !(k == "guid" || (k == "id" && x.is_number())));
                o.values_mut().for_each(strip);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v = serde_json::to_value(e).unwrap_or_default();
    if let serde_json::Value::Object(o) = &mut v {
        for k in OWN {
            o.remove(*k);
        }
    }
    strip(&mut v);
    v
}

/// Whether two values are the same within [`TOL`], angles the same turn.
fn alike(a: &serde_json::Value, b: &serde_json::Value, key: &str) -> bool {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) if key.ends_with("angle") || key == "turn" => {
                let d = (x - y).rem_euclid(360.0);
                d.min(360.0 - d) <= TOL
            }
            (Some(x), Some(y)) => (x - y).abs() <= TOL,
            _ => x == y,
        },
        (V::Array(x), V::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| alike(a, b, key)),
        (V::Object(x), V::Object(y)) => x.len() == y.len() && x.iter().all(|(k, a)| y.get(k).is_some_and(|b| alike(a, b, k))),
        _ => a == b,
    }
}

/// Whether element `e` is what member `x` places as `want` (both as the project has them).
pub fn same(e: &Element, want: &Element) -> bool {
    alike(&comparable(e), &comparable(want), "")
}

/// The element GUIDs of instance `inst`'s members, by member GUID: those it has.
fn elements_of(inst: &Instance) -> BTreeMap<Guid, Guid> {
    inst.members.iter().filter(|s| !s.element.is_nil()).map(|s| (s.member, s.element)).collect()
}

/// What is wrong with instance `id` of `m` against its type: each member changed or gone, and the storeys a non-flexible one
/// no longer keeps (or a flexible one's members stand on). Empty when it is as its type places it.
pub fn conflicts(m: &Model, id: Id) -> Vec<String> {
    let Some(e) = m.elements.get(id) else { return Vec::new() };
    let Some(inst) = instance(e) else { return Vec::new() };
    let Some(t) = types(m, inst.kind).get(inst.type_name) else { return Vec::new() };
    let mut out = Vec::new();
    let elements = elements_of(&inst);
    let f = frame_of(m, &inst, &elements);
    let vary = varying(m);
    // Each member's first slot, found at once: a type of thousands is looked through once, not once for each member.
    let slots: BTreeMap<Guid, &Slot> = inst.members.iter().rev().map(|s| (s.member, s)).collect();
    for x in &t.members {
        let Some(slot) = slots.get(&x.guid) else { continue };
        if slot.excluded {
            continue;
        }
        let Some(have) = m.elements.get(slot.element) else {
            out.push(format!("its {} {} was deleted", x.element.category(), m.shown(slot.element)));
            continue;
        };
        let Ok(mut want) = place(m, t, x, &f) else { continue };
        keep_own(&vary, &mut want, have);
        if !same(have, &want) {
            let id = m.elements.id(slot.element).unwrap_or_default();
            out.push(format!("its {} {id} changed", have.category()));
        }
    }
    if inst.kind == Kind::Model {
        for (k, h) in storeys_off(m, t, inst.levels, !inst.flexible) {
            let (Some(g), Some(s)) = (inst.levels.get(k), t.levels.get(k)) else { continue };
            out.push(format!(
                "{} is {h:.0} mm above {}, where its type has {:.0} mm{}",
                level_name(m, *g),
                level_name(m, inst.on),
                s.height,
                if inst.flexible { ", and a member stands on it" } else { "" }
            ));
        }
    }
    out
}

/// Whether instance `id` of `m` must be placed from its type whatever its members are: a member yet to be made, or levels
/// that aren't those of the level it stands on (it was given another).
pub fn unplaced(m: &Model, id: Id) -> bool {
    let Some(inst) = m.elements.get(id).and_then(instance) else { return false };
    let Some(t) = types(m, inst.kind).get(inst.type_name) else { return false };
    inst.members.iter().any(|s| !s.excluded && s.element.is_nil())
        || inst.members.len() != t.members.len()
        || (inst.kind == Kind::Model && (inst.levels.len() != t.levels.len() || inst.levels.first() != Some(&inst.on)))
}

/// Instance `id` of `m` placed again from its type: each member it has written over (keeping its id, its GUID and its own
/// fields), each it lacks made, those its type no longer has given back to be deleted; its levels worked out again when it
/// stands on another level than they start at. What the caller deletes: the elements of members gone from its type.
pub fn regenerate(m: &mut Model, id: Id) -> Result<Vec<Guid>, String> {
    let Some(e) = m.elements.get(id) else { return Ok(Vec::new()) };
    let Some(inst) = instance(e) else { return Ok(Vec::new()) };
    let who = format!("{} {id}", e.category());
    let (kind, type_name) = (inst.kind, inst.type_name.to_string());
    let t = types(m, kind).get(&type_name).cloned().ok_or_else(|| format!("{who}: there's no {} group `{type_name}`", kind.name()))?;
    let mut slots: Vec<Slot> = inst.members.to_vec();
    let mut levels: Vec<Guid> = inst.levels.to_vec();
    let on = inst.on;
    let flexible = inst.flexible;
    if kind == Kind::Model && (levels.len() != t.levels.len() || levels.first() != Some(&on)) {
        levels = levels_for(m, &type_name, &t, on).map_err(|e| format!("{who}: {e}"))?;
        let off = storeys_off(m, &t, &levels, !flexible);
        if let Some((k, h)) = off.first() {
            let s = t.levels.get(*k).map_or(0.0, |s| s.height);
            return Err(format!(
                "{who}: model group `{type_name}` needs {} {:.0} mm above {}, and it is {h:.0} mm: {}",
                levels.get(*k).map_or_else(String::new, |g| level_name(m, *g)),
                s,
                level_name(m, on),
                if flexible {
                    "a flexible group's members still stand where they stood"
                } else {
                    "place it where the storeys are its type's, or make it flexible (`flexible: true`), so its members' tops follow the levels there"
                }
            ));
        }
    }
    // Each member's slot: the one it has, else a new one; slots of members the type has no more go.
    let wanted: BTreeSet<Guid> = t.members.iter().map(|x| x.guid).collect();
    let gone: Vec<Guid> = slots.iter().filter(|s| !wanted.contains(&s.member) && !s.element.is_nil()).map(|s| s.element).collect();
    slots.retain(|s| wanted.contains(&s.member));
    let mut had: BTreeSet<Guid> = slots.iter().map(|s| s.member).collect();
    for x in &t.members {
        if had.insert(x.guid) {
            slots.push(Slot { member: x.guid, element: Guid::default(), excluded: false });
        }
    }
    // In the type's order (each member's first place in it).
    let order: BTreeMap<Guid, usize> = t.members.iter().enumerate().rev().map(|(k, x)| (x.guid, k)).collect();
    slots.sort_by_key(|s| order.get(&s.member).copied());
    for s in &mut slots {
        if !s.excluded && (s.element.is_nil() || m.elements.id(s.element).is_none()) {
            s.element = Guid::new();
        }
        if s.excluded {
            s.element = Guid::default();
        }
    }
    let elements: BTreeMap<Guid, Guid> = slots.iter().filter(|s| !s.excluded).map(|s| (s.member, s.element)).collect();
    let placed_inst = Instance { levels: &levels, members: &slots, ..inst };
    let f = frame_of(m, &placed_inst, &elements);
    let mut made: Vec<(Guid, Element, bool)> = Vec::new();
    let vary = varying(m);
    for x in &t.members {
        let Some(&g) = elements.get(&x.guid) else { continue };
        let mut e = place(m, &t, x, &f).map_err(|e| format!("{who}: {e}"))?;
        let had = m.elements.get(g);
        if let Some(was) = had {
            keep_own(&vary, &mut e, was);
            keep_parts(&mut e, was);
        }
        made.push((g, e, had.is_some()));
    }
    // Marks and numbers a new member takes anew, as a copy's do.
    for (_, e, had) in &mut made {
        if *had {
            continue;
        }
        match e {
            Element::Door(o) => o.mark = crate::commands::next_mark(m, true),
            Element::Window(o) => o.mark = crate::commands::next_mark(m, false),
            Element::Room(r) => r.number = crate::commands::next_room_number(m),
            _ => {}
        }
    }
    for (g, e, had) in made {
        if had {
            if let Some(i) = m.elements.id(g) {
                m.elements.insert(i, e);
            }
        } else {
            let i = m.fresh_id();
            m.elements.insert_as(i, g, e)?;
        }
    }
    match m.elements.get_mut(id) {
        Some(Element::ModelGroup(g)) => (g.members, g.levels) = (slots, levels),
        Some(Element::DetailGroup(g)) => g.members = slots,
        _ => {}
    }
    Ok(gone)
}

/// The ids and GUIDs of `was`'s parts kept on `e`, place for place: a member placed again keeps its sketch lines and its shape.
fn keep_parts(e: &mut Element, was: &Element) {
    let mut was = was.clone();
    if let (Some(new), Some(old)) = (e.sketch_mut(), was.sketch_mut()) {
        crate::library::keep_parts(old, new);
    }
    if let (Some(new), Some(old)) = (e.shape_mut(), was.shape_mut()) {
        crate::library::keep_parts(&mut old.points, &mut new.points);
        crate::library::keep_parts(&mut old.lines, &mut new.lines);
    }
}

/// Where a group at `at`, turned `angle` and perhaps `mirrored`, puts a point of its type: mirrored, turned, then moved.
pub fn placing(at: P2, angle: f64, mirrored: bool) -> impl Fn(P2) -> P2 {
    let (u, v) = (heading(angle), heading(angle + 90.0));
    move |p: P2| {
        let y = if mirrored { -p.1 } else { p.1 };
        at + u * p.0 + v * y
    }
}

/// The members of detail group type `type_name` placed at `at`, turned `angle` and perhaps `mirrored`, each an element of
/// view `view`: what a repeating detail draws for each repeat.
pub fn placed_type(m: &Model, type_name: &str, view: Guid, p: Placement) -> Vec<Element> {
    let Some(t) = m.detail_groups.get(type_name) else { return Vec::new() };
    let inst = Instance { kind: Kind::Detail, type_name, placement: p, on: view, levels: &[], members: &[], flexible: false };
    let f = frame_of(m, &inst, &BTreeMap::new());
    t.members.iter().filter_map(|x| place(m, t, x, &f).ok()).collect()
}

/// `e`, a member, put in view `view` (none for a group type's own).
pub fn set_view(e: &mut Element, view: Guid) {
    match e {
        Element::DetailLine(l) => l.view = view,
        Element::FilledRegion(r) => r.view = view,
        Element::Text(n) => n.view = view,
        _ => {}
    }
}

/// The most times a repeating detail repeats.
pub const MAX_REPEATS: usize = 2000;

/// A repeating detail, as Revit's Repeating Detail Component: detail group `type_name` placed along the line from `start`
/// to `end`, every `spacing` mm from `start`, each turned along the line (and, with `mirrored`, mirrored across it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatingDetail {
    pub view: Guid,
    #[serde(rename = "type")]
    pub type_name: String,
    pub start: P2,
    pub end: P2,
    pub spacing: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
}

impl RepeatingDetail {
    /// Where its groups go: each one's origin and angle. None past [`MAX_REPEATS`].
    pub fn places(&self) -> Option<Vec<(P2, f64)>> {
        let along = self.end - self.start;
        let len = along.length();
        if !(self.spacing.is_finite() && self.spacing > 0.0 && len.is_finite()) {
            return None;
        }
        let n = (len / self.spacing + 1e-9).floor() + 1.0;
        if n > MAX_REPEATS as f64 {
            return None;
        }
        let angle = along.1.atan2(along.0).to_degrees();
        let u = if len > 0.0 { along * (1.0 / len) } else { P2(1.0, 0.0) };
        Some((0..n as usize).map(|k| (self.start + u * (k as f64 * self.spacing), angle)).collect())
    }

    /// How many times it repeats: none, past [`MAX_REPEATS`].
    pub fn count(&self) -> usize {
        self.places().map_or(0, |p| p.len())
    }

    /// Each repeat's members, placed.
    pub fn members(&self, m: &Model) -> Vec<Element> {
        self.places()
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(at, angle)| placed_type(m, &self.type_name, self.view, Placement { at, angle, mirrored: self.mirrored }))
            .collect()
    }
}

/// What [`Model::check`] holds group instance `e` (element `id`, `who`) to: its type's, each member once, each slot one of
/// the type's members holding an element of its kind (none while excluded), its levels as many as its type's storeys from
/// its own, and no element a member of two groups (`grouped`, member → group).
pub(crate) fn check_instance(m: &Model, id: Id, who: &str, e: &Element, grouped: &mut BTreeMap<Guid, Id>) -> Result<(), String> {
    let Some(inst) = instance(e) else { return Ok(()) };
    let Some(t) = types(m, inst.kind).get(inst.type_name) else {
        let there: Vec<&str> = types(m, inst.kind).keys().map(String::as_str).collect();
        return Err(format!("{who}: there's no {} group `{}` (there are: {})", inst.kind.name(), inst.type_name, there.join(", ")));
    };
    if inst.members.len() != t.members.len() {
        return Err(format!(
            "{who}: it has {} members and its type `{}` {}: each of the type's members has its place in it",
            inst.members.len(),
            inst.type_name,
            t.members.len()
        ));
    }
    let mut seen = BTreeSet::new();
    let theirs: BTreeSet<Guid> = t.members.iter().map(|x| x.guid).collect();
    for s in inst.members {
        if !theirs.contains(&s.member) {
            return Err(format!("{who}: member {} isn't one of its type `{}`'s", s.member, inst.type_name));
        }
        if !seen.insert(s.member) {
            return Err(format!("{who}: member {} has two places in it", s.member));
        }
        match (s.excluded, s.element.is_nil()) {
            (true, false) => return Err(format!("{who}: an excluded member holds no element, and {} does", m.shown(s.element))),
            (false, true) => return Err(format!("{who}: member {} holds no element and isn't excluded", s.member)),
            (true, true) => continue,
            (false, false) => {}
        }
        let Some(el) = m.elements.get(s.element) else { continue };
        if !kind_of(el).is_ok_and(|k| k == inst.kind) {
            return Err(format!("{who}: {} {} can't be in a {} group", el.category(), m.shown(s.element), inst.kind.name()));
        }
        if let Some(other) = grouped.insert(s.element, id).filter(|o| *o != id) {
            return Err(format!(
                "{who}: {} {} is a member of group {other} too; an element is in one group",
                el.category(),
                m.shown(s.element)
            ));
        }
    }
    if inst.kind == Kind::Model {
        if !inst.levels.is_empty() && inst.levels.len() != t.levels.len() {
            return Err(format!("{who}: it names {} levels for its type's {} storeys", inst.levels.len(), t.levels.len()));
        }
        if inst.levels.first().is_some_and(|l| *l != inst.on) {
            return Err(format!("{who}: its first level is the level it stands on"));
        }
    }
    Ok(())
}
