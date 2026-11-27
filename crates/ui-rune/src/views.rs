//! Which view the window shows, and what the Project Browser lists of the model. No toolkit here: what a kind of view is, whether
//! the model still has it, how it is drawn and framed (a plan's, for now), its name, and the browser's tree in the groups egui's
//! browser has (floor plans, ceiling plans, 3D views, elevations, sections, callouts, schedules, panel schedules, reinforcement
//! details, sheets, the families' types, the linked models), kept to a tab (All, Views, Schedules, Sheets, Families) and to what a search
//! with wildcards finds, and which views are picked together. The selection belongs to the model, so opening a view never touches it
//! (`App::open_view`).

use std::collections::BTreeSet;

mod find;
pub use find::{MAX_FIND, spans, words};

use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model};
use bimcraft_core::render::{self, Drawing, View};
use bimcraft_core::schedule;
use bimcraft_i18n::msg;

use crate::lang::{t, tc, tf};
use crate::status::capital;
use crate::typeprops;

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

/// What the browser keeps to: its tabs, as egui's (Legends, which has nothing yet, is left out).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tab {
    #[default]
    All,
    Views,
    Schedules,
    Sheets,
    Families,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::All, Tab::Views, Tab::Schedules, Tab::Sheets, Tab::Families];

    /// The English name; the window says it through `t`.
    pub fn name(self) -> &'static str {
        match self {
            Tab::All => msg!("All"),
            Tab::Views => msg!("Views"),
            Tab::Schedules => "Schedules",
            Tab::Sheets => msg!("Sheets"),
            Tab::Families => "Families",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Tab::All => "tab-all",
            Tab::Views => "tab-views",
            Tab::Schedules => "tab-schedules",
            Tab::Sheets => "tab-sheets",
            Tab::Families => "tab-families",
        }
    }
}

/// What a press on a row does besides lighting it: a view opens; a type of a family opens Type Properties; a linked model is selected.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    Open,
    Type(String, String),
    Select(Id),
}

/// A row of the browser: a view, a type or a linked model, by the name the model gives it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub label: String,
    /// The view it opens; `Nothing` for a row that is no view.
    pub kind: ViewKind,
    /// What a test or a screen reader finds it by.
    pub key: String,
    pub pick: Pick,
}

impl Entry {
    fn new(label: String, kind: ViewKind) -> Entry {
        Entry { label, key: kind.key(), kind, pick: Pick::Open }
    }
}

/// A group of the browser. Its label is the English message; the window says it through `t`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Group {
    pub label: &'static str,
    /// The tab that lists it, besides All, which lists every group (`All` here: that tab alone).
    pub tab: Tab,
    pub entries: Vec<Entry>,
}

/// What a right click on the browser was over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    View(ViewKind),
    Type(String, String),
    Link(Id),
    Group(&'static str),
}

/// The Families group's label: it starts folded, being the longest.
pub const FAMILIES: &str = msg!("Families");

/// What the browser holds: the tree, which groups are folded, the tab, the views picked together, the menu open, and the name of the
/// view shown (the status bar's).
#[derive(Default)]
pub struct Views {
    pub groups: Vec<Group>,
    pub folded: BTreeSet<&'static str>,
    seeded: bool,
    pub tab: Tab,
    /// The views picked together with Ctrl or Shift, for Create Print Set, and where a Shift run starts.
    pub picked: Vec<ViewKind>,
    anchor: Option<ViewKind>,
    /// A right click's place in the window, and what it was over.
    pub menu: Option<(f32, f32, Target)>,
    pub title: String,
    /// Counts the changes a row's look depends on, so the browser is built again only then.
    pub rev: u64,
}

impl Views {
    /// After the model may have changed: the tree and the title made again. The views picked that the model no longer has are let go.
    pub fn rebuild(&mut self, m: &Model, shown: ViewKind) {
        let (groups, title) = (tree(m), title(m, shown));
        if !self.seeded {
            // The families are the longest list and start folded.
            (self.seeded, self.folded) = (true, BTreeSet::from([FAMILIES]));
        }
        let before = (self.picked.len(), self.anchor);
        self.picked.retain(|k| exists(m, *k));
        self.anchor = self.anchor.filter(|k| exists(m, *k));
        if groups != self.groups || title != self.title || before != (self.picked.len(), self.anchor) {
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

    /// Expand All, or Collapse All.
    pub fn fold_all(&mut self, folded: bool) {
        self.folded = if folded { self.groups.iter().map(|g| g.label).collect() } else { BTreeSet::new() };
        self.rev += 1;
    }

    pub fn set_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.rev += 1;
        }
    }

    /// The groups the tab keeps to, each with the rows that show for what is typed in the search (`find`, the words of it): a word
    /// must match a row's name or its group's (the words the group's name matches are spent), as egui's browser takes them. A group
    /// with no row left is not listed while something is typed. Folds are not applied: they are how the list is drawn.
    pub fn rows(&self, find: &str) -> Vec<(&Group, Vec<&Entry>)> {
        let words = words(find);
        self.groups
            .iter()
            .filter(|g| self.tab == Tab::All || g.tab == self.tab)
            .filter_map(|g| {
                let name = t(g.label);
                let shown: Vec<&Entry> = g.entries.iter().filter(|e| words.is_empty() || find::shows(name, &e.label, &words)).collect();
                (words.is_empty() || !shown.is_empty()).then_some((g, shown))
            })
            .collect()
    }

    /// The views as the browser lists them now, in order: what a Shift run goes over.
    pub fn order(&self, find: &str) -> Vec<ViewKind> {
        self.rows(find).into_iter().flat_map(|(_, rows)| rows).filter(|e| e.pick == Pick::Open).map(|e| e.kind).collect()
    }

    /// A press on `kind` with Ctrl or Shift held picks views together as a list of the system's does: Ctrl toggles one (the view on
    /// screen counts as picked first), Shift picks the run from the last one picked, Shift with Ctrl adds the run. `order` is
    /// [`Views::order`], `shown` the view on screen.
    pub fn pick(&mut self, kind: ViewKind, shift: bool, ctrl: bool, order: &[ViewKind], shown: ViewKind) {
        let at = |k: &ViewKind| order.iter().position(|o| o == k);
        match (shift, self.anchor.as_ref().and_then(at), at(&kind)) {
            (true, Some(a), Some(b)) => {
                let run = order.get(a.min(b)..=a.max(b)).map(<[ViewKind]>::to_vec).unwrap_or_default();
                if ctrl {
                    for k in run {
                        if !self.picked.contains(&k) {
                            self.picked.push(k);
                        }
                    }
                } else {
                    self.picked = run;
                }
            }
            _ => {
                if self.picked.is_empty() && order.contains(&shown) {
                    self.picked.push(shown);
                }
                match self.picked.iter().position(|p| *p == kind) {
                    Some(i) => {
                        self.picked.remove(i);
                    }
                    None => self.picked.push(kind),
                }
                self.anchor = Some(kind);
            }
        }
        self.rev += 1;
    }

    /// A plain press on `kind`: it alone is picked.
    pub fn pick_one(&mut self, kind: ViewKind) {
        (self.picked, self.anchor) = (vec![kind], Some(kind));
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

/// The element a view is: what Delete takes out and a print set holds. None for a view that is none yet (the default `{3D}`
/// before a project has made it, a schedule every project has until it is its own) and for a ceiling plan, which is no element of its own.
pub fn element_id(m: &Model, kind: ViewKind) -> Option<Id> {
    match kind {
        ViewKind::Plan(id) | ViewKind::Section(id) | ViewKind::Sheet(id) | ViewKind::Table(id) | ViewKind::View3D(Some(id)) => {
            exists(m, kind).then_some(id)
        }
        ViewKind::View3D(None) => m.default_3d(),
        ViewKind::Schedule(cat) => schedule::standard(m, cat).0,
        ViewKind::Ceiling(_) | ViewKind::Nothing => None,
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

/// The types of the project's families (each category's), as rows `Wall: Generic - 200mm`, by category and name; a press on one opens
/// its Type Properties.
fn types(m: &Model) -> Vec<Entry> {
    let mut rows: Vec<(String, String, String)> = typeprops::all(m)
        .into_iter()
        .map(|(key, _)| (tc("category", &capital(&key.category)).to_string(), key.name, key.category))
        .collect();
    rows.sort_by_key(|(label, name, _)| (label.to_lowercase(), name.to_lowercase()));
    rows.into_iter()
        .map(|(label, name, category)| Entry {
            label: format!("{label}: {name}"),
            kind: ViewKind::Nothing,
            key: format!("type-{category}-{}", name.replace('/', " ")),
            pick: Pick::Type(category, name),
        })
        .collect()
}

/// The linked models, each with whether it is loaded; a press on one selects it.
fn links(m: &Model) -> Vec<Entry> {
    m.rare("link")
        .into_iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Link(l)) => {
                let label = if m.loaded.contains_key(&l.path) { l.name.clone() } else { tf("{} (not loaded)", &[&l.name]) };
                Some(Entry { label, kind: ViewKind::Nothing, key: format!("link-{id}"), pick: Pick::Select(id) })
            }
            _ => None,
        })
        .collect()
}

/// Everything the browser lists. The floor plans, ceiling plans, 3D views, schedules and families are always there; the other groups
/// only when the project has something in them.
pub fn tree(m: &Model) -> Vec<Group> {
    let mut schedules: Vec<Entry> =
        schedule::VIEWS.iter().map(|(cat, english)| Entry::new(standard_name(m, cat, english), ViewKind::Schedule(cat))).collect();
    schedules.extend(tables(m, "schedule"));
    let always = [
        (msg!("Floor Plans"), Tab::Views, plans(m, false)),
        (msg!("Ceiling Plans"), Tab::Views, plans(m, true)),
        (msg!("3D Views"), Tab::Views, views_3d(m)),
        (msg!("Elevations (Building Elevation)"), Tab::Views, sections(m, "elevation")),
        (msg!("Sections (Building Section)"), Tab::Views, sections(m, "section")),
        (msg!("Callouts"), Tab::Views, callouts(m)),
        (msg!("Schedules/Quantities (all)"), Tab::Schedules, schedules),
        (msg!("Panel Schedules"), Tab::Schedules, tables(m, "panelschedule")),
        (msg!("Reinforcement Details"), Tab::Schedules, tables(m, "rebardetail")),
        (msg!("Sheets (all)"), Tab::Sheets, sheets(m)),
        (FAMILIES, Tab::Families, types(m)),
        (msg!("Linked Models"), Tab::All, links(m)),
    ];
    let kept = ["Floor Plans", "Ceiling Plans", "3D Views", "Schedules/Quantities (all)", FAMILIES];
    always
        .into_iter()
        .filter(|(label, _, entries)| kept.contains(label) || !entries.is_empty())
        .map(|(label, tab, entries)| Group { label, tab, entries })
        .collect()
}

#[cfg(test)]
mod tests;
