//! Copy to Clipboard, Cut and Paste, as Revit's Clipboard panel has them ([copy and paste](../../../docs/areas/copy-paste.md)).
//!
//! What's copied is text: a small project of its own, holding the elements copied and what they point at (their levels, the
//! wall a door is in, the view a note is drawn in), with where it was copied from. Text goes on the system clipboard, so
//! another BIMCraft window pastes it as this one does. A paste takes the copied elements of it (never what they only
//! pointed at): onto the levels and into the views the mode says, moved as it says, and brings them in through
//! [`crate::insert`], Insert from File's engine, which brings the types, families and materials they use that the project
//! hasn't and gives a copy of an element the project has its own id and GUID.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::geom::P2;
use crate::lines::DetailLine;
use crate::model::{Dimension, Element, Finish, Guid, HostRef, Id, Model, Note, Spot, TOL, Tag};
use crate::walls::Axis;

/// What a clipboard's text says it is.
pub const KIND: &str = "bimcraft clipboard";
/// The clipboard's own format: one a newer BIMCraft wrote with a larger number is refused.
pub const VERSION: u32 = 1;
/// The longest clipboard text read: what a whole project of that size would take.
pub const MAX_CLIPBOARD: usize = 512 << 20;
/// How close a level's elevation must be to stand in for another's, mm.
const SAME_HEIGHT: f64 = 0.5;

/// The frame of a view: what the points of what's drawn in it mean.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Frame {
    /// A plan of `level`: the model's x and y.
    Plan { level: Guid, elevation: f64 },
    /// A section or an elevation: along its line from `start`, and up (the model's z).
    Section { start: P2, end: P2 },
    /// A sheet: paper millimetres.
    Sheet,
    /// A 3D view.
    ThreeD,
}

impl Frame {
    /// The frame of view `key` of `m`: a level's plan, a plan view, a section, an elevation, a sheet or a 3D view.
    pub fn of(m: &Model, key: impl crate::model::Key) -> Option<Frame> {
        Some(match m.elements.get(key)? {
            Element::Level(l) => Frame::Plan { level: m.elements.guid(key)?, elevation: l.elevation },
            Element::PlanView(v) => Frame::Plan { level: v.level, elevation: m.elevation(v.level) },
            Element::Section(v) | Element::Elevation(v) => Frame::Section { start: v.start, end: v.end },
            Element::Sheet(_) => Frame::Sheet,
            Element::View3D(_) => Frame::ThreeD,
            _ => return None,
        })
    }

    /// Its name, as the paste's notes say it.
    fn name(&self) -> &'static str {
        match self {
            Frame::Plan { .. } => "a plan",
            Frame::Section { .. } => "a section or an elevation",
            Frame::Sheet => "a sheet",
            Frame::ThreeD => "a 3D view",
        }
    }

    /// A section's direction along its line.
    fn along(&self) -> Option<P2> {
        match self {
            Frame::Section { start, end } if start.dist(*end) > TOL => Some((*end - *start).unit()),
            _ => None,
        }
    }

    /// How a point of a view of frame `self` moves to be at the same place in a view of frame `to`: where the two look the
    /// same way (two plans; two sections along parallel lines, looking the same way), by the model's coordinates; where they
    /// don't, by the view's own (its origin to the other's): Revit's view orientation.
    pub fn to(&self, to: &Frame) -> P2 {
        match (self, to) {
            (Frame::Section { start: a, .. }, Frame::Section { start: b, .. }) => match (self.along(), to.along()) {
                (Some(u), Some(v)) if u.dot(v) > 1.0 - 1e-9 => P2((*a - *b).dot(u), 0.0),
                _ => P2(0.0, 0.0),
            },
            _ => P2(0.0, 0.0),
        }
    }

    /// Whether the two look at the model the same way, so a paste keeps the model's coordinates between them.
    pub fn same_orientation(&self, to: &Frame) -> bool {
        match (self, to) {
            (Frame::Plan { .. }, Frame::Plan { .. }) => true,
            (Frame::Section { .. }, Frame::Section { .. }) => {
                matches!((self.along(), to.along()), (Some(u), Some(v)) if u.dot(v) > 1.0 - 1e-9)
            }
            _ => false,
        }
    }
}

/// Where a clipboard's elements were copied from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The project's uid, which tells a paste into the same project from one into another.
    pub project: u64,
    /// The project's name.
    #[serde(default)]
    pub name: String,
    /// The view the copy was made in, and its frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Guid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Frame>,
    /// The level the model elements copied stand on: the plan's, else the lowest of theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Guid>,
    /// The frame of each view the view-specific elements copied are drawn in.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub views: BTreeMap<Guid, Frame>,
    /// The elements copied, by their ids in the clipboard's project.
    pub ids: Vec<Id>,
}

/// A clipboard read: where it came from, and the project that holds what was copied.
pub struct Clip {
    pub source: Source,
    pub model: Model,
}

/// What a copy made: the clipboard's text, how many elements it holds, and what was left out of it, and why.
#[derive(Debug)]
pub struct Copied {
    pub text: String,
    pub count: usize,
    pub left_out: Vec<String>,
}

/// What kind of element a clipboard takes something as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort {
    /// Of the model: it stands on a level, at a place in plan.
    Model,
    /// View-specific: drawn in one view, in its terms.
    Annotation,
}

/// How an element is taken by the clipboard, or why it can't be.
fn sort(e: &Element) -> Result<Sort, &'static str> {
    Ok(match e {
        Element::Level(_) => return Err("a level is added in a section or an elevation, not pasted"),
        Element::Sheet(_) | Element::Viewport(_) => return Err("a sheet and its views are duplicated in the Project Browser"),
        Element::Schedule(_) | Element::PanelSchedule(_) | Element::RebarDetail(_) | Element::PlanView(_) => {
            return Err("a view is duplicated in the Project Browser");
        }
        Element::View3D(v) if v.camera.is_none() => return Err("a 3D view is duplicated in the Project Browser"),
        Element::ProjectInfo(_) => return Err("a project has one Project Information"),
        Element::Circuit(_) => return Err("a circuit is made anew of the pasted devices with `circuit`"),
        Element::Text(_)
        | Element::Spot(_)
        | Element::DetailLine(_)
        | Element::FilledRegion(_)
        | Element::DetailGroup(_)
        | Element::RepeatingDetail(_)
        | Element::Dimension(_)
        | Element::Tag(_)
        | Element::Dimension3D(_) => Sort::Annotation,
        Element::CadImport(ci) if ci.view.is_some() => Sort::Annotation,
        _ => Sort::Model,
    })
}

/// The view a view-specific element is drawn in.
fn view_of(m: &Model, e: &Element) -> Option<Guid> {
    match e {
        Element::Dimension3D(d) => Some(d.view),
        Element::CadImport(ci) => ci.view,
        Element::Tag(t) => t.view.or_else(|| m.level_of(e)),
        _ => m.annotation_plan(e),
    }
}

/// Where a view-specific element may be pasted: the frames it has a meaning in.
fn fits(e: &Element, to: &Frame) -> bool {
    match e {
        // A plan's: measured, named or drawn over a plan.
        Element::Dimension(_) | Element::Tag(_) | Element::CadImport(_) => matches!(to, Frame::Plan { .. }),
        Element::Dimension3D(_) => matches!(to, Frame::ThreeD),
        Element::Spot(_) => matches!(to, Frame::Plan { .. } | Frame::Section { .. }),
        _ => !matches!(to, Frame::ThreeD),
    }
}

/// Element `e`, a view-specific one, drawn in view `view` of frame `frame` instead.
fn into_view(e: &mut Element, view: Guid, frame: &Frame) {
    let plan = |frame: &Frame| match frame {
        Frame::Plan { level, .. } => Some(*level),
        _ => None,
    };
    match e {
        Element::Dimension(Dimension { level, view: v, .. }) => {
            if let Some(l) = plan(frame) {
                *level = l;
                *v = (l != view).then_some(view);
            }
        }
        Element::Tag(Tag { view: v, .. }) => *v = plan(frame).filter(|l| *l != view).map(|_| view),
        Element::Dimension3D(d) => d.view = view,
        Element::CadImport(ci) => {
            ci.view = Some(view);
            if let Some(l) = plan(frame) {
                ci.level = l;
            }
        }
        Element::Text(Note { view: v, .. })
        | Element::Spot(Spot { view: v, .. })
        | Element::DetailLine(DetailLine { view: v, .. })
        | Element::FilledRegion(crate::fills::FilledRegion { view: v, .. })
        | Element::DetailGroup(crate::groups::DetailGroup { view: v, .. })
        | Element::RepeatingDetail(crate::groups::RepeatingDetail { view: v, .. }) => *v = view,
        _ => {}
    }
}

/// The elements a copy of `ids` takes: those, with what is hosted in them (a wall's doors and windows, a stair's railings,
/// what's on a floor's face), as Revit copies a host with what it hosts; and what can't be copied, and why.
fn taken(m: &Model, ids: &[Id]) -> Result<(BTreeSet<Id>, Vec<String>), String> {
    let mut take = BTreeSet::new();
    let mut left_out = Vec::new();
    for &id in ids {
        let e = m.elements.get(id).ok_or_else(|| format!("no element {id}"))?;
        match sort(e) {
            Ok(_) => {
                take.insert(id);
            }
            Err(why) => left_out.push(format!("{} {id}: {why}", e.category())),
        }
    }
    // A group's members go with it: its copy places them anew (`groups::keep`), as Copy's does.
    let members: BTreeSet<Id> = take.iter().flat_map(|id| crate::groups::members(m, *id)).collect();
    take.retain(|id| !members.contains(id));
    // A curtain wall brings the walls in its cells, and those their doors, as Copy does.
    let curtains: BTreeSet<Guid> = take.iter().map(|id| m.guid(*id)).collect();
    take.extend(m.elements.iter().filter_map(|(id, e)| match e {
        Element::Wall(w) if w.panel_of.is_some_and(|q| curtains.contains(&q.host)) => Some(*id),
        _ => None,
    }));
    let hosts: BTreeSet<Guid> = take.iter().map(|id| m.guid(*id)).collect();
    for (&id, e) in &m.elements {
        let hosted = match e {
            Element::Door(o) | Element::Window(o) => hosts.contains(&o.host),
            Element::Railing(r) => r.host.is_some_and(|h| hosts.contains(&h)),
            Element::Component(c) => c.host.is_some_and(|h| matches!(h.host, HostRef::Face { .. }) && hosts.contains(&h.host.guid())),
            _ => false,
        };
        if hosted {
            take.insert(id);
        }
    }
    if take.is_empty() {
        return Err(match left_out.first() {
            Some(why) => format!("nothing to copy: {why}"),
            None => "nothing to copy: give `ids`".into(),
        });
    }
    Ok((take, left_out))
}

/// Elements `ids` of `m` to the clipboard, copied in view `view` (or none: a plan of their lowest level).
pub fn copy(m: &Model, ids: &[Id], view: Option<Id>) -> Result<Copied, String> {
    let (take, left_out) = taken(m, ids)?;
    // What they point at goes too, so a paste knows where a door was in its wall, and the levels, so it knows their heights.
    let mut keep: BTreeSet<Id> = take.clone();
    for id in &take {
        for g in m.elements.get(*id).map(Element::refs).unwrap_or_default() {
            if let Some(r) = m.elements.id(g) {
                keep.insert(r);
            }
        }
    }
    keep.extend(m.levels().into_iter().map(|(id, _)| id));
    let mut views = BTreeMap::new();
    for id in &take {
        let Some(e) = m.elements.get(*id) else { continue };
        if sort(e) == Ok(Sort::Annotation)
            && let Some(v) = view_of(m, e)
            && let Some(f) = Frame::of(m, v)
        {
            views.insert(v, f);
        }
    }
    let lowest = take
        .iter()
        .filter_map(|id| m.elements.get(*id))
        .filter(|e| sort(e) == Ok(Sort::Model))
        .filter_map(|e| m.level_of(e))
        .min_by(|a, b| m.elevation(*a).total_cmp(&m.elevation(*b)));
    let frame = view.and_then(|v| Frame::of(m, v));
    let level = match &frame {
        Some(Frame::Plan { level, .. }) if lowest.is_some() => Some(*level),
        _ => lowest,
    };
    let source = Source {
        project: m.uid,
        name: m.name.clone(),
        view: view.and_then(|v| m.elements.guid(v)),
        frame,
        level,
        views,
        ids: take.iter().copied().collect(),
    };
    let mut clip = m.clone();
    clip.elements.retain(|id, _| keep.contains(id));
    clip.pinned.clear();
    let project = clip.to_json()?;
    let head = serde_json::to_string(&source).map_err(|e| format!("the clipboard couldn't be written: {e}"))?;
    let text = format!("{{\"bimcraft\":\"{KIND}\",\"version\":{VERSION},\"source\":{head},\"project\":{project}}}");
    Ok(Copied { text, count: take.len(), left_out })
}

#[derive(Deserialize)]
struct Raw<'a> {
    bimcraft: String,
    version: u32,
    source: Source,
    #[serde(borrow)]
    project: &'a RawValue,
}

/// Whether `text` looks like a clipboard of BIMCraft's, without reading it: what a window checks before offering Paste.
pub fn is_clipboard(text: &str) -> bool {
    marked(text, KIND)
}

/// Whether `text` is a JSON object that says early on it is a clipboard of `kind`: a project's ([`KIND`]) or a family's
/// ([`crate::family::clipboard::KIND`]).
pub(crate) fn marked(text: &str, kind: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('{') && t.get(..64).unwrap_or(t).contains(kind)
}

/// A clipboard's text read.
pub fn read(text: &str) -> Result<Clip, String> {
    if text.len() > MAX_CLIPBOARD {
        return Err(format!("the clipboard is too large ({} bytes; a paste takes one under {MAX_CLIPBOARD})", text.len()));
    }
    let raw: Raw = serde_json::from_str(text).map_err(|e| format!("the clipboard holds no BIMCraft elements ({e})"))?;
    if raw.bimcraft != KIND {
        return Err("the clipboard holds no BIMCraft elements".into());
    }
    if raw.version > VERSION {
        return Err(format!("the clipboard was written by a newer BIMCraft (clipboard version {}); update to paste it", raw.version));
    }
    // Held to the memory this machine has free, as a file is (`memory.rs`): the text may come from any program's clipboard.
    let budget = crate::memory::Budget::start(text.len() as u64, crate::memory::PROJECT)
        .map_err(|e| format!("the clipboard: {e}"))?
        .unwrap_or_else(crate::memory::Budget::none);
    let (model, _) = Model::read_within(raw.project.get(), &budget).map_err(|e| format!("the clipboard's project can't be read: {e}"))?;
    if let Some(id) = raw.source.ids.iter().find(|id| !model.elements.contains_key(**id)) {
        return Err(format!("the clipboard is damaged: it has no element {id}"));
    }
    if raw.source.ids.len() > crate::model::MAX_POINTS {
        return Err("the clipboard holds too many elements".into());
    }
    Ok(Clip { source: raw.source, model })
}

/// How a paste places what it pastes: Revit's Paste and its Aligned options.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Paste from Clipboard: into the view, moved `by` in its terms (a plan's x and y; a section's along and up, the height
    /// taking model elements to the level nearest it; a 3D view's x and y, onto `level` where given: the level of the point
    /// picked on the work plane, else on their own levels).
    Manual { by: P2, level: Option<Guid> },
    /// Aligned to Current View: view-specific elements where they were in theirs; model elements onto the plan's level.
    CurrentView,
    /// Aligned to Same Place: where they were, on their levels (by name in another project).
    SamePlace,
    /// Aligned to Selected Levels: model elements onto each of these levels.
    Levels(Vec<Guid>),
    /// Aligned to Selected Views: view-specific elements into each of these views.
    Views(Vec<Guid>),
    /// Aligned to Picked Level: model elements onto this level, keeping their places along the view.
    PickedLevel(Guid),
}

/// What a paste made.
#[derive(Debug, Default)]
pub struct Pasted {
    /// The elements pasted, in this project.
    pub ids: Vec<Id>,
    /// What wasn't pasted, and why.
    pub left_out: Vec<String>,
}

impl Clip {
    /// The model elements and the view-specific ones copied.
    fn split(&self) -> (Vec<Id>, Vec<Id>) {
        self.source.ids.iter().partition(|id| self.model.elements.get(**id).and_then(|e| sort(e).ok()) == Some(Sort::Model))
    }

    /// The elevation of the level the model elements stood on.
    fn base_elevation(&self) -> f64 {
        self.source.level.map_or(0.0, |l| self.model.elevation(l))
    }

    /// The frame of view-specific element `e`'s own view.
    fn frame_of(&self, e: &Element) -> Option<&Frame> {
        view_of(&self.model, e).and_then(|v| self.source.views.get(&v))
    }

    /// The points that say where what was copied is, in the terms of a view of frame `to`, before a paste moves them: what
    /// a paste by hand centres on the cursor, and its outline follows it.
    pub fn points_in(&self, to: &Frame) -> Vec<P2> {
        let mut pts = Vec::new();
        let (model, notes) = self.split();
        let with: BTreeSet<Id> = self.source.ids.iter().copied().collect();
        let up = self.base_elevation();
        for id in model {
            for p in points_of(&self.model, id, &with) {
                pts.push(match to {
                    Frame::Section { start, .. } => to.along().map_or(p, |u| P2((p - *start).dot(u), up)),
                    _ => p,
                });
            }
        }
        if !matches!(to, Frame::ThreeD) {
            for id in notes {
                let Some(e) = self.model.elements.get(id) else { continue };
                let shift = self.frame_of(e).map_or(P2(0.0, 0.0), |f| f.to(to));
                pts.extend(points_of(&self.model, id, &with).into_iter().map(|p| p + shift));
            }
        }
        pts
    }

    /// How copied element `id` moves to stand in a view of frame `to`, before a paste by hand moves it: none when it isn't
    /// pasted there. What a window's preview of a paste draws each element by.
    pub fn shift_of(&self, id: Id, to: &Frame) -> Option<P2> {
        let e = self.model.elements.get(id)?;
        match sort(e).ok()? {
            Sort::Model => (!matches!(to, Frame::Sheet)).then_some(P2(0.0, 0.0)),
            Sort::Annotation => fits(e, to).then(|| self.frame_of(e).map_or(P2(0.0, 0.0), |f| f.to(to))),
        }
    }

    /// Whether it holds model elements, and whether view-specific ones: what the Paste options a window offers depend on.
    pub fn holds(&self) -> (bool, bool) {
        let (model, notes) = self.split();
        (!model.is_empty(), !notes.is_empty())
    }

    /// The box round [`Clip::points_in`]; none when nothing copied has a place there.
    pub fn extent_in(&self, to: &Frame) -> Option<[P2; 2]> {
        let pts = self.points_in(to);
        let first = *pts.first()?;
        Some(pts.iter().fold([first, first], |[lo, hi], p| [P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1))]))
    }

    /// The middle of what was copied in a view of frame `to`: the point a paste by hand puts at the cursor.
    pub fn middle_in(&self, to: &Frame) -> Option<P2> {
        self.extent_in(to).map(|[lo, hi]| (lo + hi) * 0.5)
    }
}

/// The points element `id` of `m` is placed by, in its own terms: each point a move would move.
fn points_of(m: &Model, id: Id, with: &BTreeSet<Id>) -> Vec<P2> {
    let seen = std::cell::RefCell::new(Vec::new());
    let record = |p: P2| {
        seen.borrow_mut().push(p);
        p
    };
    let mo = crate::commands::Motion { map: &record, slide: Some(P2(0.0, 0.0)), mirror: false };
    // A door or window alone is where its wall puts it.
    if let Some(Element::Door(o) | Element::Window(o)) = m.elements.get(id)
        && m.elements.id(o.host).is_none_or(|h| !with.contains(&h))
    {
        return m.wall(o.host).map(|w| vec![Axis::of(w).at(o.offset, 0.0)]).unwrap_or_default();
    }
    let _ = crate::commands::transformed("paste", m, id, &mo, with);
    seen.into_inner()
}

/// Where one paste puts things: the view, how model elements and view-specific ones move, and which levels stand for which.
struct Target {
    /// The view view-specific elements go into, and its frame; none: they aren't pasted.
    view: Option<(Guid, Frame)>,
    /// How model elements move in plan.
    model_by: P2,
    /// How view-specific ones move in their view's terms, beside what their frame's change asks.
    view_by: P2,
    /// What each level of the clipboard's project is here: none where this project has no level at that height.
    levels: BTreeMap<Guid, Option<Guid>>,
    /// Whether nothing moved: then what isn't copied but this project has (the same project's wall a tag names) still serves.
    in_place: bool,
}

/// Paste `clip` into `to`, in view `view` (the open view: a level's plan, a plan view, a section, an elevation, a sheet or a
/// 3D view), as `mode` says. Unchecked: the engine checks the model after the command.
pub fn paste(to: &mut Model, clip: &Clip, mode: &Mode, view: Option<Id>) -> Result<Pasted, String> {
    let here = view.and_then(|v| Some((to.elements.guid(v)?, Frame::of(to, v)?)));
    if view.is_some() && here.is_none() {
        return Err("paste into a plan, a section, an elevation, a sheet or a 3D view".into());
    }
    let (model, notes) = clip.split();
    let base = clip.base_elevation();
    let mut targets = Vec::new();
    match mode {
        Mode::Manual { by, level } => {
            let Some((v, frame)) = here.clone() else { return Err("a paste by hand needs the view it is made in: give `view`".into()) };
            if level.is_some() && !matches!(frame, Frame::ThreeD) {
                return Err(format!(
                    "`level` goes with a paste by hand in a 3D view; in {} the view says where model elements go",
                    frame.name()
                ));
            }
            let (model_by, dz) = match &frame {
                Frame::Plan { elevation, .. } => (*by, Some(*elevation - base)),
                Frame::Section { .. } => {
                    let u = frame.along().unwrap_or(P2(1.0, 0.0));
                    // The height taking the model elements to the level nearest it.
                    let dz = nearest_level(to, base + by.1).map(|l| to.elevation(l) - base);
                    (u * by.0, dz.or(Some(0.0)))
                }
                Frame::ThreeD => match level {
                    Some(l) => (*by, Some(to.level(*l).ok_or_else(|| format!("no level {l}"))?.elevation - base)),
                    None => (*by, None),
                },
                Frame::Sheet => (P2(0.0, 0.0), None),
            };
            if !model.is_empty() && matches!(frame, Frame::Sheet) {
                return Err("model elements can't be pasted onto a sheet: paste them into a plan, a section or a 3D view".into());
            }
            let levels = match dz {
                Some(dz) => by_height(clip, to, dz),
                None => by_name(clip, to),
            };
            targets.push(Target { view: Some((v, frame)), model_by, view_by: *by, levels, in_place: false });
        }
        Mode::CurrentView => {
            let Some((v, frame)) = here.clone() else { return Err("Aligned to Current View needs the open view: give `view`".into()) };
            let levels = match &frame {
                Frame::Plan { elevation, .. } => by_height(clip, to, *elevation - base),
                _ => by_name(clip, to),
            };
            if !model.is_empty() && matches!(frame, Frame::Sheet) {
                return Err("model elements can't be pasted onto a sheet: paste them into a plan, a section or a 3D view".into());
            }
            let in_place =
                clip.source.project == to.uid && matches!(&frame, Frame::Plan { level, .. } if Some(*level) == clip.source.level);
            targets.push(Target { view: Some((v, frame)), model_by: P2(0.0, 0.0), view_by: P2(0.0, 0.0), levels, in_place });
        }
        Mode::SamePlace => {
            // View-specific elements into their own view where this project has it, else into the open one.
            let own = clip.source.views.keys().next().copied().filter(|v| to.elements.contains_key(*v));
            let view = match own {
                Some(v) => to.elements.id(v).and_then(|id| Some((v, Frame::of(to, id)?))),
                None => here.clone(),
            };
            let in_place = clip.source.project == to.uid;
            targets.push(Target { view, model_by: P2(0.0, 0.0), view_by: P2(0.0, 0.0), levels: by_name(clip, to), in_place });
        }
        Mode::Levels(levels) => {
            if model.is_empty() {
                return Err("Aligned to Selected Levels pastes model elements; the clipboard holds only view-specific ones".into());
            }
            if levels.is_empty() {
                return Err("give `levels`, the levels to paste onto".into());
            }
            for l in levels {
                let dz = to.level(*l).ok_or_else(|| format!("no level {l}"))?.elevation - base;
                targets.push(Target {
                    view: None,
                    model_by: P2(0.0, 0.0),
                    view_by: P2(0.0, 0.0),
                    levels: by_height(clip, to, dz),
                    in_place: false,
                });
            }
        }
        Mode::PickedLevel(l) => {
            if model.is_empty() {
                return Err("Aligned to Picked Level pastes model elements; the clipboard holds only view-specific ones".into());
            }
            let dz = to.level(*l).ok_or_else(|| format!("no level {l}"))?.elevation - base;
            targets.push(Target {
                view: None,
                model_by: P2(0.0, 0.0),
                view_by: P2(0.0, 0.0),
                levels: by_height(clip, to, dz),
                in_place: false,
            });
        }
        Mode::Views(views) => {
            if notes.is_empty() {
                return Err("Aligned to Selected Views pastes view-specific elements; the clipboard holds only model elements".into());
            }
            if views.is_empty() {
                return Err("give `views`, the views to paste into".into());
            }
            for v in views {
                let id = to.elements.id(*v).ok_or_else(|| format!("no view {v}"))?;
                let frame = Frame::of(to, id).ok_or_else(|| format!("{id} is not a view a paste goes into"))?;
                let in_place = clip.source.project == to.uid && clip.source.views.contains_key(v);
                targets.push(Target {
                    view: Some((*v, frame)),
                    model_by: P2(0.0, 0.0),
                    view_by: P2(0.0, 0.0),
                    levels: BTreeMap::new(),
                    in_place,
                });
            }
        }
    }
    let models = !matches!(mode, Mode::Views(_));
    let mut out = Pasted::default();
    for t in &targets {
        let one = paste_one(to, clip, t, models)?;
        out.ids.extend(one.ids);
        out.left_out.extend(one.left_out);
    }
    if out.ids.is_empty() {
        return Err(match out.left_out.first() {
            Some(why) => format!("nothing was pasted: {why}"),
            None => "nothing was pasted".into(),
        });
    }
    Ok(out)
}

/// Each level of the clipboard's project, here: the one `dz` higher, where this project has one.
fn by_height(clip: &Clip, to: &Model, dz: f64) -> BTreeMap<Guid, Option<Guid>> {
    let here = to.levels();
    clip.model
        .levels()
        .into_iter()
        .map(|(id, l)| {
            let want = l.elevation + dz;
            let found = here.iter().find(|(_, h)| (h.elevation - want).abs() <= SAME_HEIGHT).map(|(h, _)| to.guid(*h));
            (clip.model.guid(id), found)
        })
        .collect()
}

/// Each level of the clipboard's project, here: itself in the same project; else the one of its name, else of its height.
fn by_name(clip: &Clip, to: &Model) -> BTreeMap<Guid, Option<Guid>> {
    let here = to.levels();
    clip.model
        .levels()
        .into_iter()
        .map(|(id, l)| {
            let g = clip.model.guid(id);
            let found = if to.level(g).is_some() {
                Some(g)
            } else {
                here.iter()
                    .find(|(_, h)| h.name == l.name)
                    .or_else(|| here.iter().find(|(_, h)| (h.elevation - l.elevation).abs() <= SAME_HEIGHT))
                    .map(|(h, _)| to.guid(*h))
            };
            (g, found)
        })
        .collect()
}

/// The level of `m` whose elevation is nearest `z`.
fn nearest_level(m: &Model, z: f64) -> Option<Id> {
    m.levels().into_iter().min_by(|(_, a), (_, b)| (a.elevation - z).abs().total_cmp(&(b.elevation - z).abs())).map(|(id, _)| id)
}

/// One paste of `clip` into `to`, at `t`: the model elements with `models`, the view-specific ones where `t` has a view.
fn paste_one(to: &mut Model, clip: &Clip, t: &Target, models: bool) -> Result<Pasted, String> {
    let from = &clip.model;
    let asked: BTreeSet<Id> = clip.source.ids.iter().copied().collect();
    let asked_guids: BTreeSet<Guid> = asked.iter().map(|id| from.guid(*id)).collect();
    let mut staged: Vec<(Id, Element)> = Vec::new();
    let mut left_out = Vec::new();
    let mut leave = |id: Id, e: &Element, why: String| left_out.push(format!("{} {id}: {why}", e.category()));
    let model_mo = |d: P2| move |q: P2| q + d;
    for &id in &asked {
        let Some(e) = from.elements.get(id) else { continue };
        match sort(e) {
            Ok(Sort::Model) if models => {
                let by = t.model_by;
                let map = model_mo(by);
                let mo = crate::commands::Motion { map: &map, slide: Some(by), mirror: false };
                let mut e = match e {
                    // A door or a window without its wall goes into a wall here, at that place.
                    Element::Door(o) | Element::Window(o) if !asked_guids.contains(&o.host) => match rehost(to, from, o, by, &t.levels) {
                        Ok(o) => {
                            if matches!(e, Element::Door(_)) {
                                Element::Door(o)
                            } else {
                                Element::Window(o)
                            }
                        }
                        Err(why) => {
                            leave(id, e, why);
                            continue;
                        }
                    },
                    _ => match crate::commands::transformed("paste", from, id, &mo, &asked) {
                        Ok(e) => e,
                        Err(why) => {
                            leave(id, e, why.trim_start_matches("paste: ").to_string());
                            continue;
                        }
                    },
                };
                if let Err(why) = relevel(&mut e, from, to, &t.levels) {
                    leave(id, &e, why);
                    continue;
                }
                staged.push((id, e));
            }
            Ok(Sort::Annotation) => {
                let Some((view, frame)) = &t.view else {
                    leave(id, e, "it is drawn in a view: paste it into one (`view`, or `mode` views)".into());
                    continue;
                };
                if !fits(e, frame) {
                    leave(id, e, format!("can't be pasted into {}", frame.name()));
                    continue;
                }
                let shift = clip.frame_of(e).map_or(P2(0.0, 0.0), |f| f.to(frame)) + t.view_by;
                let map = model_mo(shift);
                let mo = crate::commands::Motion { map: &map, slide: Some(shift), mirror: false };
                let mut e = match crate::commands::transformed("paste", from, id, &mo, &asked) {
                    Ok(e) => e,
                    Err(why) => {
                        leave(id, e, why.trim_start_matches("paste: ").to_string());
                        continue;
                    }
                };
                into_view(&mut e, *view, frame);
                staged.push((id, e));
            }
            Ok(Sort::Model) => leave(id, e, "it is a model element: paste it onto levels, not into views".into()),
            Err(_) => {}
        }
    }
    // What a pasted element points at that wasn't copied: a tie it drops, or what it can't be without.
    let mut kept = Vec::new();
    for (id, mut e) in staged {
        // A group's copy places its members anew (`groups::keep`), never the original's.
        crate::groups::commands::as_copy(&mut e);
        let here = |g: &Guid| t.in_place && to.elements.contains_key(*g);
        let copied = |g: &Guid| asked_guids.contains(g);
        let mut need = None;
        match &mut e {
            Element::Wall(w) => {
                for at in [&mut w.attach_top, &mut w.attach_base] {
                    *at = at.filter(|g| copied(g) || here(g));
                }
            }
            Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => {
                for at in [&mut c.attach_top, &mut c.attach_base] {
                    *at = at.filter(|g| copied(g) || here(g));
                }
            }
            Element::Roof(r) => r.join = r.join.filter(|g| copied(g) || here(g)),
            Element::Room(r) => {
                for f in [&mut r.floor, &mut r.ceiling] {
                    if let Finish::Connected(g) = f
                        && !copied(g)
                        && !here(g)
                    {
                        *f = Finish::None;
                    }
                }
            }
            Element::Component(c) => {
                if let Some(p) = &mut c.panel {
                    p.supply_from = p.supply_from.filter(|g| copied(g) || here(g));
                }
                if let Some(h) = c.host
                    && !copied(&h.host.guid())
                    && !here(&h.host.guid())
                {
                    need = Some("its host wasn't copied");
                }
            }
            Element::View3D(v) => {
                if let Some(wp) = &mut v.work_plane {
                    wp.ref_plane = wp.ref_plane.filter(|g| copied(g) || here(g));
                }
            }
            Element::Railing(r) if r.host.is_some_and(|h| !copied(&h) && !here(&h)) => {
                need = Some("it goes with its stair or ramp; copy that too")
            }
            Element::Rebar(r) if r.host.is_some_and(|h| !copied(&h) && !here(&h)) => {
                need = Some("it goes with the element it reinforces; copy that too")
            }
            Element::Tag(tg) if !copied(&tg.host) && !here(&tg.host) => need = Some("it goes with what it names; copy that too"),
            Element::Spot(s) if s.host.is_some_and(|h| !copied(&h) && !here(&h)) => {
                need = Some("it goes with what it measures; copy that too")
            }
            Element::Dimension(d) if d.refs.iter().any(|r| !copied(&r.target()) && !here(&r.target())) => {
                need = Some("it goes with what it measures; copy that too")
            }
            Element::Dimension3D(_) if !t.in_place => need = Some("it measures the model where it was; paste it to the same place"),
            _ => {}
        }
        if let Some(why) = need {
            leave(id, &e, why.into());
            continue;
        }
        // Whatever else it points at must be copied with it or be this project's.
        if let Some(g) = e.refs().into_iter().find(|g| !copied(g) && !to.elements.contains_key(*g)) {
            leave(
                id,
                &e,
                format!(
                    "it points at {} that wasn't copied",
                    from.elements.id(g).map_or_else(|| format!("{g:?}"), |i| format!("element {i}"))
                ),
            );
            continue;
        }
        kept.push((id, e));
    }
    if kept.is_empty() {
        return Ok(Pasted { ids: Vec::new(), left_out });
    }
    // Brought in as Insert from File brings elements: what they point at among themselves follows them.
    let mut src = from.clone();
    let ids: Vec<Id> = kept.iter().map(|(id, _)| *id).collect();
    for (id, e) in kept {
        src.elements.insert(id, e);
    }
    let made = crate::insert::insert(to, &src, &ids)?;
    let pasted: Vec<Id> = ids.iter().filter_map(|id| made.ids.get(id).copied()).collect();
    renumber(to, &pasted);
    left_out.extend(made.left_out);
    Ok(Pasted { ids: pasted, left_out })
}

/// Door or window `o` of `from`, whose wall wasn't copied, in a wall of `to` where it comes to be: moved `by`, on the
/// level its wall's stands for here.
fn rehost(
    to: &Model,
    from: &Model,
    o: &crate::model::Opening,
    by: P2,
    levels: &BTreeMap<Guid, Option<Guid>>,
) -> Result<crate::model::Opening, String> {
    let w = from.wall(o.host).ok_or("it was copied without its wall")?;
    let at = Axis::of(w).at(o.offset, 0.0) + by;
    let level = levels.get(&w.level).copied().flatten().or_else(|| to.level(w.level).map(|_| w.level));
    let host = to
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Wall(h) if level.is_none_or(|l| h.level == l) => Some((*id, h)),
            _ => None,
        })
        .find(|(_, h)| {
            let a = Axis::of(h);
            let (s, off) = a.project(at);
            off.abs() <= to.thickness(h) / 2.0 + 1.0 && s > TOL && s < a.length() - TOL
        });
    let Some((hid, h)) = host else {
        return Err(format!("there's no wall at [{:.0}, {:.0}] here to put it in", at.0, at.1));
    };
    let mut o = o.clone();
    o.host = to.guid(hid);
    o.offset = (Axis::of(h).project(at).0 * 1e3).round() / 1e3;
    Ok(o)
}

/// Model element `e` of `from`, on the levels here `levels` says: a top it can't keep stands unconnected, as tall as it was;
/// a base it can't keep goes to the level nearest its height.
fn relevel(e: &mut Element, from: &Model, to: &Model, levels: &BTreeMap<Guid, Option<Guid>>) -> Result<(), String> {
    let lost = |g: &Option<Guid>| g.is_some_and(|g| levels.get(&g).is_some_and(Option::is_none));
    // A top this project hasn't, as a height.
    match e {
        Element::Wall(w) if lost(&w.top_level) => {
            let top = w.top_level.map_or(0.0, |t| from.elevation(t)) + w.top_offset;
            w.height = (top - from.elevation(w.level) - w.base_offset).max(1.0);
            (w.top_level, w.top_offset) = (None, 0.0);
        }
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) if lost(&c.top_level) => {
            let top = c.top_level.map_or(0.0, |t| from.elevation(t)) + c.top_offset;
            c.height = (top - from.elevation(c.level) - c.base_offset).max(1.0);
            (c.top_level, c.top_offset) = (None, 0.0);
        }
        Element::Shaft(crate::model::Shaft { level, top_level, height, base_offset, .. })
        | Element::Mass(crate::model::Mass { level, top_level, height, base_offset, .. })
            if lost(top_level) =>
        {
            let top = top_level.map_or(0.0, |t| from.elevation(t));
            *height = (top - from.elevation(*level) - *base_offset).max(1.0);
            *top_level = None;
        }
        Element::Stair(crate::model::Stair { top_level, .. }) | Element::Ramp(crate::model::Ramp { top_level, .. }) if lost(top_level) => {
            *top_level = None;
        }
        Element::Room(r) if lost(&r.top_level) => {
            r.limit_offset += r.top_level.map_or(0.0, |t| from.elevation(t)) - from.elevation(r.level);
            r.top_level = None;
        }
        _ => {}
    }
    let mut missing = None;
    e.refs_mut(&mut |g| {
        if let Some(found) = levels.get(g) {
            match found {
                Some(h) => *g = *h,
                None => match nearest_level(to, from.elevation(*g)) {
                    Some(h) => *g = to.guid(h),
                    None => missing = Some(()),
                },
            }
        }
    });
    if missing.is_some() {
        return Err("this project has no level to put it on".into());
    }
    Ok(())
}

/// The doors' and windows' marks and the rooms' numbers among `ids` another element of `m` has, made the next.
fn renumber(m: &mut Model, ids: &[Id]) {
    for &id in ids {
        let (kind, taken) = match m.elements.get(id) {
            Some(Element::Door(o)) => ("door", o.mark.clone()),
            Some(Element::Window(o)) => ("window", o.mark.clone()),
            Some(Element::Room(r)) => ("room", r.number.clone()),
            _ => continue,
        };
        let marks = |m: &Model| -> Vec<String> {
            m.elements
                .iter()
                .filter(|(i, _)| **i != id)
                .filter_map(|(_, e)| match (kind, e) {
                    ("door", Element::Door(o)) | ("window", Element::Window(o)) => Some(o.mark.clone()),
                    ("room", Element::Room(r)) => Some(r.number.clone()),
                    _ => None,
                })
                .collect()
        };
        let others = marks(m);
        if taken.is_empty() || !others.contains(&taken) {
            continue;
        }
        let next = crate::commands::next_number(others.iter().map(String::as_str));
        match m.elements.get_mut(id) {
            Some(Element::Door(o) | Element::Window(o)) => o.mark = next,
            Some(Element::Room(r)) => r.number = next,
            _ => {}
        }
    }
}
