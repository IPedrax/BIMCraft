//! Which view the window shows, and what the Project Browser lists of the model. No toolkit here: what a kind of view is, whether
//! the model still has it, how it is drawn and framed (a plan's, for now), its name, and the browser's tree in the groups egui's
//! browser has (floor plans, ceiling plans, 3D views, elevations, sections, callouts, schedules, panel schedules, reinforcement
//! details, sheets). The selection belongs to the model, so opening a view never touches it (`App::open_view`).

use std::collections::BTreeSet;

use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model};
use bimcraft_core::render::{self, Drawing, View};
use bimcraft_core::schedule;
use bimcraft_i18n::msg;

use crate::lang::t;

/// The view on screen. A plan's key is a level or a plan view (a callout of a plan too), as `render::draw` takes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewKind {
    /// A project with no level has nothing to show.
    #[default]
    Nothing,
    Plan(Id),
    /// The reflected ceiling plan of a level or a plan view.
    Ceiling(Id),
    /// A section, an elevation, or a callout of one.
    Section(Id),
    /// A 3D view; none is the default `{3D}`, which a project makes when it is first changed.
    View3D(Option<Id>),
    Sheet(Id),
    /// One of the schedules every project has, by its category (`schedule::VIEWS`).
    Schedule(&'static str),
    /// Another schedule of the project's, a panel schedule or a reinforcement detail.
    Table(Id),
}

impl ViewKind {
    /// The element key a press on its browser row is found by.
    pub fn key(self) -> String {
        match self {
            ViewKind::Nothing => "view-none".into(),
            ViewKind::Plan(id) => format!("plan-{id}"),
            ViewKind::Ceiling(id) => format!("ceiling-{id}"),
            ViewKind::Section(id) => format!("section-{id}"),
            ViewKind::View3D(Some(id)) => format!("view3d-{id}"),
            ViewKind::View3D(None) => "view3d".into(),
            ViewKind::Sheet(id) => format!("sheet-{id}"),
            ViewKind::Schedule(cat) => format!("schedule-{cat}"),
            ViewKind::Table(id) => format!("table-{id}"),
        }
    }

    /// Whether the plan canvas draws it (the others have a canvas of their own).
    pub fn is_plan(self) -> bool {
        matches!(self, ViewKind::Plan(_) | ViewKind::Ceiling(_))
    }
}

/// A row of the browser: a view, by the name the model gives it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub label: String,
    pub kind: ViewKind,
    /// What a test or a screen reader finds it by.
    pub key: String,
}

impl Entry {
    fn new(label: String, kind: ViewKind) -> Entry {
        Entry { label, key: kind.key(), kind }
    }
}

/// A group of the browser. Its label is the English message; the window says it through `t`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Group {
    pub label: &'static str,
    pub entries: Vec<Entry>,
}

/// What the browser holds: the tree, which groups are folded, and the name of the view shown (the status bar's).
#[derive(Default)]
pub struct Views {
    pub groups: Vec<Group>,
    pub folded: BTreeSet<&'static str>,
    pub title: String,
    /// Counts the changes a row's look depends on, so the browser is built again only then.
    pub rev: u64,
}

impl Views {
    /// After the model may have changed: the tree and the title made again.
    pub fn rebuild(&mut self, m: &Model, shown: ViewKind) {
        let (groups, title) = (tree(m), title(m, shown));
        if groups != self.groups || title != self.title {
            self.rev += 1;
        }
        (self.groups, self.title) = (groups, title);
    }

    /// Folds a group, or opens it again.
    pub fn toggle(&mut self, label: &'static str) {
        if !self.folded.remove(label) {
            self.folded.insert(label);
        }
        self.rev += 1;
    }
}

/// Whether the model has this view.
pub fn exists(m: &Model, kind: ViewKind) -> bool {
    match kind {
        ViewKind::Nothing => false,
        ViewKind::Plan(id) | ViewKind::Ceiling(id) => m.plan_level(id).is_some(),
        ViewKind::Section(id) => m.section_geometry(id).is_some(),
        ViewKind::View3D(None) => true,
        ViewKind::View3D(Some(id)) => matches!(m.elements.get(id), Some(Element::View3D(_))),
        ViewKind::Sheet(id) => m.sheet(id).is_some(),
        ViewKind::Schedule(cat) => schedule::VIEWS.iter().any(|(c, _)| *c == cat),
        ViewKind::Table(id) => {
            matches!(m.elements.get(id), Some(Element::Schedule(_) | Element::PanelSchedule(_) | Element::RebarDetail(_)))
        }
    }
}

/// `kind` while the model still has it, else the plan of `level`, else nothing.
pub fn keep(m: &Model, kind: ViewKind, level: Option<Id>) -> ViewKind {
    if exists(m, kind) { kind } else { level.map_or(ViewKind::Nothing, ViewKind::Plan) }
}

/// The level a plan view stands on: the plan's level, a plan view's, a callout's.
pub fn level_of(m: &Model, kind: ViewKind) -> Option<Id> {
    match kind {
        ViewKind::Plan(id) | ViewKind::Ceiling(id) => m.plan_level(id),
        _ => None,
    }
}

/// The view as `view` shows it, for the views the plan canvas draws.
pub fn draw(m: &Model, kind: ViewKind, view: View) -> Option<Drawing> {
    match kind {
        ViewKind::Plan(id) => Some(render::draw(m, id, view)),
        ViewKind::Ceiling(id) => Some(render::draw_ceiling(m, id, view)),
        _ => None,
    }
}

/// Where to stand the view to fit a canvas of `width` x `height` points, for the views the plan canvas draws.
pub fn fit(m: &Model, kind: ViewKind, width: u32, height: u32) -> Option<View> {
    match kind {
        ViewKind::Plan(id) => Some(render::plan_fit(m, id, width, height, false)),
        ViewKind::Ceiling(id) => Some(render::plan_fit(m, id, width, height, true)),
        _ => None,
    }
}

/// A schedule every project has, by the name the project gave it once it is its own, else the English one in the language.
fn standard_name(m: &Model, cat: &str, english: &str) -> String {
    match schedule::standard(m, cat) {
        (Some(_), sc) => sc.name,
        (None, _) => t(english).to_string(),
    }
}

/// The view's own name: a level's, a section's, a sheet's number and name.
pub fn name(m: &Model, kind: ViewKind) -> String {
    match kind {
        ViewKind::Nothing => String::new(),
        ViewKind::Plan(id) | ViewKind::Ceiling(id) | ViewKind::Section(id) | ViewKind::Table(id) | ViewKind::View3D(Some(id)) => {
            m.view_name(id).unwrap_or_default().to_string()
        }
        ViewKind::View3D(None) => "{3D}".to_string(),
        ViewKind::Sheet(id) => m.sheet(id).map(|s| format!("{} - {}", s.number, s.name)).unwrap_or_default(),
        ViewKind::Schedule(cat) => {
            let english = schedule::VIEWS.iter().find(|(c, _)| *c == cat).map_or("", |(_, n)| *n);
            standard_name(m, cat, english)
        }
    }
}

/// What the status bar and a view's tab call it: a plan by its name, every other kind by its kind and name.
pub fn title(m: &Model, kind: ViewKind) -> String {
    let what = match kind {
        ViewKind::Nothing | ViewKind::Plan(_) => return name(m, kind),
        ViewKind::Ceiling(_) => msg!("Ceiling Plan"),
        ViewKind::Section(id) => match m.elements.get(id) {
            Some(Element::Elevation(_)) => msg!("Elevation"),
            Some(Element::DetailView(_)) => msg!("Callout"),
            _ => msg!("Section"),
        },
        ViewKind::View3D(_) => msg!("3D View"),
        ViewKind::Sheet(_) => msg!("Sheet"),
        ViewKind::Table(id) => match m.elements.get(id) {
            Some(Element::PanelSchedule(_)) => msg!("Panel Schedule"),
            Some(Element::RebarDetail(_)) => msg!("Reinforcement Detail"),
            _ => msg!("Schedule"),
        },
        ViewKind::Schedule(_) => msg!("Schedule"),
    };
    format!("{}: {}", t(what), name(m, kind))
}

/// Each level's plan, then the plan views made of it, as egui's browser lists them.
fn plans(m: &Model, ceiling: bool) -> Vec<Entry> {
    let of = |key: Id| if ceiling { ViewKind::Ceiling(key) } else { ViewKind::Plan(key) };
    let mut out = Vec::new();
    for (id, level) in m.levels() {
        let mut entry = Entry::new(level.name.clone(), of(id));
        // The row of a level's floor plan keeps the key it had when levels were all the browser listed.
        if !ceiling {
            entry.key = format!("level-{id}");
        }
        out.push(entry);
        out.extend(m.plan_views(id).into_iter().map(|(view, pv)| Entry::new(pv.name.clone(), of(view))));
    }
    out
}

/// The default `{3D}`, which is there before the project has made it, then every other 3D view.
fn views_3d(m: &Model) -> Vec<Entry> {
    let mut out = vec![Entry::new("{3D}".to_string(), ViewKind::View3D(m.default_3d()))];
    for id in m.rare("view3d") {
        if let Some(Element::View3D(v)) = m.elements.get(id)
            && v.name != "{3D}"
        {
            out.push(Entry::new(v.name.clone(), ViewKind::View3D(Some(id))));
        }
    }
    out
}

/// The sections or the elevations, by name.
fn sections(m: &Model, category: &str) -> Vec<Entry> {
    let mut v: Vec<(Id, String)> = m.rare(category).into_iter().filter_map(|id| Some((id, m.section(id)?.name.clone()))).collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v.into_iter().map(|(id, name)| Entry::new(name, ViewKind::Section(id))).collect()
}

/// The callouts, each named after the view it is a fragment of.
fn callouts(m: &Model) -> Vec<Entry> {
    let mut out: Vec<Entry> = m
        .rare("detail")
        .into_iter()
        .filter_map(|id| {
            let Some(Element::DetailView(v)) = m.elements.get(id) else { return None };
            let parent = m.elements.id(v.parent)?;
            let kind = if m.plan_level(id).is_some() { ViewKind::Plan(id) } else { ViewKind::Section(id) };
            Some(Entry::new(format!("{}: {}", m.view_name(parent).unwrap_or_default(), v.name), kind))
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

/// The project's schedules of one category of element (`schedule`, `panelschedule`, `rebardetail`), by name. The seven every
/// project has are listed apart, so a schedule of those names is not twice in the browser.
fn tables(m: &Model, category: &str) -> Vec<Entry> {
    let mut v: Vec<(Id, String)> = m
        .rare(category)
        .into_iter()
        .filter_map(|id| Some((id, m.view_name(id)?.to_string())))
        .filter(|(_, name)| category != "schedule" || schedule::builtin(Some(name), None).is_none())
        .collect();
    v.sort_by_key(|(_, name)| name.to_lowercase());
    v.into_iter().map(|(id, name)| Entry::new(name, ViewKind::Table(id))).collect()
}

/// The sheets, by number, as `A101 - Name`.
fn sheets(m: &Model) -> Vec<Entry> {
    let mut v: Vec<(Id, String)> =
        m.sheets().into_iter().filter_map(|id| m.sheet(id).map(|s| (id, format!("{} - {}", s.number, s.name)))).collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v.into_iter().map(|(id, label)| Entry::new(label, ViewKind::Sheet(id))).collect()
}

/// Everything the browser lists. The floor plans, ceiling plans, 3D views and schedules are always there; the other groups only
/// when the project has something in them.
pub fn tree(m: &Model) -> Vec<Group> {
    let mut schedules: Vec<Entry> =
        schedule::VIEWS.iter().map(|(cat, english)| Entry::new(standard_name(m, cat, english), ViewKind::Schedule(*cat))).collect();
    schedules.extend(tables(m, "schedule"));
    let always = [
        (msg!("Floor Plans"), plans(m, false)),
        (msg!("Ceiling Plans"), plans(m, true)),
        (msg!("3D Views"), views_3d(m)),
        (msg!("Elevations (Building Elevation)"), sections(m, "elevation")),
        (msg!("Sections (Building Section)"), sections(m, "section")),
        (msg!("Callouts"), callouts(m)),
        (msg!("Schedules/Quantities (all)"), schedules),
        (msg!("Panel Schedules"), tables(m, "panelschedule")),
        (msg!("Reinforcement Details"), tables(m, "rebardetail")),
        (msg!("Sheets (all)"), sheets(m)),
    ];
    let kept = ["Floor Plans", "Ceiling Plans", "3D Views", "Schedules/Quantities (all)"];
    always
        .into_iter()
        .filter(|(label, entries)| kept.contains(label) || !entries.is_empty())
        .map(|(label, entries)| Group { label, entries })
        .collect()
}

#[cfg(test)]
mod tests;
