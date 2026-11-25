//! What the Properties palette shows, as a value: for the selected elements (or, with none, the plan's own view) the groups of rows
//! they have, each row with its label, its text in the project's units and the editor it takes. It is egui's `parameters` without the
//! drawing: the same table of rows (`table.rs`), the same rules for which row shows and what it turns into, the same shared-or-blank
//! values across a selection. The window draws a `Panel`; `apply.rs` turns an edit of one of its rows into a command.

use std::collections::{BTreeMap, BTreeSet};

use bimcraft_core::element_json_with;
use bimcraft_core::model::{Element, Guid, Id, Location, Model};
use bimcraft_core::rooms::Rooms;
use bimcraft_core::units::Units;
use serde_json::{Value, json};

use super::kind::{Kind, Kind::*, MAX_VALUE, cut, field_of, relevant, shown, text};
use super::names::category as category_name;
use super::table::{Def, PLAN, rows};
use super::types::Selector;
use super::view;
use crate::lang::{t, tf};

/// The most elements whose rows are worked out together: past it the palette says how many and shows no rows.
pub const MAX_IDS: usize = 1000;
/// The most options a list offers (a project may have a thousand levels or line styles).
const MAX_OPTIONS: usize = 500;
/// The most corners of an outline a field takes: past it the outline is shown and not typed.
const MAX_CORNERS: usize = 64;

/// What the palette is about.
#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    /// No element and no level: nothing to show.
    Nothing,
    /// One element.
    Element(Id),
    /// Several elements, all of one category.
    Elements(&'static str),
    /// Several elements of different categories: how many.
    Mixed(usize),
    /// Nothing selected: the plan of this level, its own properties.
    Plan(Id),
}

/// One option of a list: what it says, the value `modify` takes for it, and a second field the same command sets with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub label: String,
    pub value: Value,
    pub also: Option<(&'static str, Value)>,
}

/// How a row is edited.
#[derive(Clone, Debug, PartialEq)]
pub enum Editor {
    /// Shown, not changed here.
    Read,
    /// A field, committed on Enter or when it loses the keyboard; `hint` names what blank means.
    Text {
        hint: &'static str,
    },
    Check(bool),
    /// A list: the option now in force, if one is.
    Choice {
        current: Option<usize>,
        options: Vec<Pick>,
    },
    /// An element it points at: a link that selects it.
    Link(Id),
    /// What a dialog does, which this window does not have yet: its name.
    Seam(&'static str),
}

/// A row that is changed by a command of its own, given the view: `{view, <key>: value}`, not a `modify` of elements.
#[derive(Clone, Debug, PartialEq)]
pub struct Via {
    pub command: &'static str,
    pub key: &'static str,
    pub view: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The field as `elements` names it, which is what an edit is applied to.
    pub key: &'static str,
    pub label: String,
    pub kind: Kind,
    /// Its value as text, in the project's units: what a field starts from, what a list or a link says.
    pub shown: String,
    pub editor: Editor,
    /// Set for the rows of a view's own graphics, which have commands of their own.
    pub via: Option<Via>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The English name, which what is folded is kept by.
    pub key: &'static str,
    /// Its name in the interface's language.
    pub name: String,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub subject: Subject,
    /// "Walls (3)", "Multiple Categories (4)", or the plan's name.
    pub title: String,
    /// The elements an edit changes, in the order selected.
    pub ids: Vec<Id>,
    /// With several categories selected, each and how many: what the filter lists.
    pub categories: Vec<(&'static str, usize)>,
    pub groups: Vec<Group>,
    /// Said under the rows: what is wrong with the element.
    pub notes: Vec<String>,
    /// The Type Selector, for elements of one category.
    pub selector: Option<Selector>,
}

impl Panel {
    /// The palette with nothing in it.
    pub fn empty() -> Panel {
        Panel {
            subject: Subject::Nothing,
            title: String::new(),
            ids: Vec::new(),
            categories: Vec::new(),
            groups: Vec::new(),
            notes: Vec::new(),
            selector: None,
        }
    }

    /// The palette for `selection`, narrowed to the category `filter` names if some of it is of that category; with nothing selected,
    /// the plan of `level`.
    pub fn of(m: &Model, selection: &[Id], filter: Option<&str>, level: Option<Id>) -> Panel {
        let alive: Vec<(Id, &'static str)> = selection.iter().filter_map(|id| m.elements.get(*id).map(|e| (*id, e.category()))).collect();
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for (_, category) in &alive {
            *counts.entry(*category).or_insert(0) += 1;
        }
        let narrowed: Vec<Id> = match filter {
            Some(f) => alive.iter().filter(|(_, c)| *c == f).map(|(id, _)| *id).collect(),
            None => Vec::new(),
        };
        let ids: Vec<Id> = if narrowed.is_empty() { alive.iter().map(|(id, _)| *id).collect() } else { narrowed };
        let categories: Vec<(&'static str, usize)> = if counts.len() > 1 { counts.into_iter().collect() } else { Vec::new() };
        let kinds: BTreeSet<&'static str> = alive.iter().filter(|(id, _)| ids.contains(id)).map(|(_, c)| *c).collect();
        let n = ids.len();
        let first = kinds.first().copied();
        let (subject, title) = match (ids.as_slice(), first) {
            ([], _) | (_, None) => return Panel::plan(m, level),
            ([id], Some(c)) => (Subject::Element(*id), format!("{} (1)", category_name(c, true))),
            (_, Some(c)) if kinds.len() == 1 => (Subject::Elements(c), format!("{} ({n})", category_name(c, true))),
            _ => (Subject::Mixed(n), tf("Multiple Categories ({})", &[&n.to_string()])),
        };
        let mut panel = Panel { subject, title, ids: ids.clone(), categories, ..Panel::empty() };
        if n > MAX_IDS {
            panel.notes.push(tf("Too many to list ({})", &[&n.to_string()]));
            return panel;
        }
        let camera =
            matches!(panel.subject, Subject::Element(id) if matches!(m.elements.get(id), Some(Element::View3D(v)) if v.camera.is_some()));
        let defs: &'static [Def] = match first {
            Some(c) if !camera && matches!(panel.subject, Subject::Element(_) | Subject::Elements(_)) => rows(c),
            _ => &[],
        };
        let category = first.filter(|_| matches!(panel.subject, Subject::Element(_) | Subject::Elements(_)));
        panel.fill(m, defs, category);
        panel
    }

    /// The plan of `level`, as its own properties (a level's own plan: this window has no plan views of a level besides it yet).
    fn plan(m: &Model, level: Option<Id>) -> Panel {
        let nothing = Panel::empty();
        let Some((id, name)) = level.and_then(|l| m.level(l).map(|x| (l, x.name.clone()))) else { return nothing };
        let mut panel = Panel { subject: Subject::Plan(id), title: tf("Floor Plan: {}", &[&name]), ids: vec![id], ..nothing };
        panel.fill(m, PLAN, None);
        // The view's own graphics: its scale, detail level and north, and the buttons of what a dialog sets.
        panel.groups.push(Group { key: view::GROUP, name: t(view::GROUP).to_string(), rows: view::rows(m, id) });
        panel
    }

    /// The groups of `defs`, with the rows that show for `self.ids`.
    fn fill(&mut self, m: &Model, defs: &'static [Def], category: Option<&'static str>) {
        if defs.is_empty() && category.is_none() {
            return;
        }
        let rooms = Rooms::new(m);
        let all: Vec<Value> = self.ids.iter().map(|id| element_json_with(m, *id, &rooms)).collect();
        let Some(first) = all.first() else { return };
        self.selector = category.map(|c| Selector::of(m, c, &self.ids, &all));
        let ctx = Ctx {
            m,
            ids: &self.ids,
            all: &all,
            first,
            units: m.units,
            levels: m.levels().into_iter().take(MAX_OPTIONS).map(|(id, l)| (id, l.name.clone())).collect(),
            // Components all of work plane-based families show where they are on their work planes.
            based: self.ids.iter().all(|id| match m.elements.get(*id) {
                Some(Element::Component(c)) => m.families.get(&c.family).is_some_and(|f| f.work_plane_based),
                _ => false,
            }),
            // A note on a sheet sits in paper mm.
            paper: self.ids.first().is_some_and(|id| matches!(m.elements.get(*id), Some(Element::Text(n)) if m.sheet(n.view).is_some())),
            // A floor or a ceiling that follows a room: its level and offset are the room's, shown and not typed.
            followed: self.ids.iter().any(|id| m.finish_of(*id).is_some()),
        };
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut groups: Vec<Group> = Vec::new();
        for &(group, key, label, kind) in defs {
            if seen.contains(key) {
                continue;
            }
            let Some(row) = ctx.row(key, label, kind) else { continue };
            seen.insert(key);
            match groups.iter_mut().find(|g| g.key == group) {
                Some(g) => g.rows.push(row),
                None => groups.push(Group { key: group, name: t(group).to_string(), rows: vec![row] }),
            }
        }
        if self.ids.len() == 1 && first["enclosed"] == json!(false) {
            self.notes.push(t("Not enclosed: the walls around it don't close.").to_string());
        }
        self.groups = groups;
    }

    /// The row for field `key`, if the palette has one.
    pub fn row(&self, key: &str) -> Option<&Row> {
        self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.key == key)
    }

    /// Every row, in order.
    pub fn all_rows(&self) -> impl Iterator<Item = &Row> {
        self.groups.iter().flat_map(|g| g.rows.iter())
    }
}

/// Whether row `label` shows for components that are (`based`) or aren't of work plane-based families.
fn work_plane_row(label: &str, based: bool) -> bool {
    const WORK_PLANE_ROWS: [&str; 3] = ["Schedule Level", "Work Plane", "Offset from Host"];
    const LEVEL_ROWS: [&str; 1] = ["Level"];
    if based { !LEVEL_ROWS.contains(&label) } else { !WORK_PLANE_ROWS.contains(&label) }
}

/// What the rows of one panel are worked out from.
struct Ctx<'a> {
    m: &'a Model,
    ids: &'a [Id],
    all: &'a [Value],
    first: &'a Value,
    units: Units,
    levels: Vec<(Id, String)>,
    based: bool,
    paper: bool,
    followed: bool,
}

/// A list of `options` with the one labelled `now` in force.
fn list(now: &str, mut options: Vec<Pick>) -> (String, Editor) {
    options.truncate(MAX_OPTIONS);
    let current = options.iter().position(|o| o.label == now);
    (now.to_string(), Editor::Choice { current, options })
}

fn plain(label: String, value: Value) -> Pick {
    Pick { label, value, also: None }
}

impl Ctx<'_> {
    /// Field `key`'s value in all the elements, as `kind` reads it: what they share, else none.
    fn shared(&self, key: &str, kind: Kind) -> Value {
        let one = field_of(self.first, key, kind);
        if self.all.iter().all(|e| field_of(e, key, kind) == one) { one } else { Value::Null }
    }

    /// What an element points at is in its JSON as a GUID, or as an id for the one beside it.
    fn id_of(&self, v: &Value) -> Option<Id> {
        v.as_u64().or_else(|| v.as_str().and_then(Guid::parse).and_then(|g| self.m.elements.id(g)))
    }

    fn level_name(&self, v: &Value) -> String {
        cut(self.id_of(v).and_then(|l| self.m.level(l)).map_or_else(String::new, |l| l.name.clone()), MAX_VALUE)
    }

    /// The levels as options, each by its name.
    fn level_options(&self) -> Vec<Pick> {
        self.levels.iter().map(|(_, name)| plain(name.clone(), json!(name))).collect()
    }

    /// Where a room's top stays as its Upper Limit becomes `upper`: its Limit Offset, now measured from there. One room only.
    fn limit_offset(&self, upper: Option<Id>) -> Option<(&'static str, Value)> {
        let [id] = self.ids else { return None };
        let Some(Element::Room(r)) = self.m.elements.get(*id) else { return None };
        let (_, top) = self.m.room_span(r);
        let from = upper.map_or_else(|| self.m.elevation(r.level), |u| self.m.elevation(u));
        let offset = top - from;
        offset.is_finite().then(|| ("limit_offset", json!(offset)))
    }

    /// An element as a link says it: its category and its id.
    fn link_text(&self, id: Id) -> String {
        let category = self.m.elements.get(id).map_or("element", |e| e.category());
        format!("{} {id}", category_name(category, false))
    }

    /// The row of one definition, or none where it means nothing for what is selected.
    fn row(&self, key: &'static str, label: &'static str, kind: Kind) -> Option<Row> {
        if !work_plane_row(label, self.based) {
            return None;
        }
        let val = self.shared(key, kind);
        let one = self.ids.len() == 1;
        // A length one element hasn't got (a straight wall's radius): no row.
        if one && matches!(kind, LengthShown | LengthIf) && val.is_null() {
            return None;
        }
        // An image has a page, a paper and a scale when it is a PDF's page, and no row of them else; nor does it snap to its lines
        // when it has none to snap to.
        let flag = key == "front" || (key == "snaps" && !self.first["paper"].is_null());
        if one && self.first["category"] == "image" && val.is_null() && !flag {
            return None;
        }
        if one && !relevant(self.first, key) {
            return None;
        }
        let kind = match kind {
            Height if !self.shared("top_level", Shown).is_null() => LengthShown,
            Length if key == "offset" && !self.shared("host", Shown).is_null() => LengthShown,
            Point if self.paper => PaperPoint,
            Length if self.followed && key == "offset" => LengthShown,
            kind => kind,
        };
        let (shown, editor) = self.editor(key, kind, &val, one);
        Some(Row { key, label: t(label).to_string(), kind, shown, editor, via: None })
    }

    /// A row's text and editor.
    fn editor(&self, key: &str, kind: Kind, val: &Value, one: bool) -> (String, Editor) {
        let units = self.units;
        let read = |s: String| (s, Editor::Read);
        match kind {
            // A component's Work Plane, `Level : Level 1`: what it is on in the interface's words, its name as it is.
            Shown if key == "work_plane" => {
                let said = text(val);
                read(said.split_once(" : ").map_or_else(|| said.clone(), |(what, name)| format!("{} : {name}", t(what))))
            }
            Shown => read(t(&text(val)).to_string()),
            // An angle's degrees; else lengths.
            Values if self.shared("kind", Shown) == "angular" => read(format!("{}°", text(val))),
            LengthShown | AreaShown | Mm2Shown | VolumeShown | MetresShown | Values => read(shown(kind, val, units)),
            LevelName => read(self.level_name(val)),
            ViewName => read(cut(self.id_of(val).and_then(|v| self.m.view_name(v)).unwrap_or_default().to_string(), MAX_VALUE)),
            // More corners than a field shows would lose the rest when typed over: shown, and not typed.
            Outline if val.as_array().is_some_and(|a| a.len() > MAX_CORNERS) => read(cut(shown(kind, val, units), MAX_VALUE)),
            Range => (t("Edit…").to_string(), Editor::Seam("View Range")),
            Crop => {
                let on = !val.is_null();
                (t(if on { "Yes" } else { "No" }).to_string(), Editor::Check(on))
            }
            Flag => {
                let on = val.as_bool().unwrap_or(false);
                (t(if on { "Yes" } else { "No" }).to_string(), Editor::Check(on))
            }
            Level if self.followed && key == "level" => read(self.level_name(val)),
            Level => list(&self.level_name(val), self.level_options()),
            UpperLimit => {
                let none = t("Not Bounded").to_string();
                let mut options = vec![Pick { label: none.clone(), value: Value::Null, also: self.limit_offset(None) }];
                options.extend(self.levels.iter().map(|(id, name)| Pick {
                    label: name.clone(),
                    value: json!(name),
                    also: self.limit_offset(Some(*id)),
                }));
                let now = if val.is_null() { none } else { self.level_name(val) };
                list(&now, options)
            }
            TopLevel => {
                let none = t("Unconnected").to_string();
                let mut options = vec![plain(none.clone(), Value::Null)];
                options.extend(self.level_options());
                let now = if val.is_null() { none } else { self.level_name(val) };
                list(&now, options)
            }
            // Shown by its label; a centre line by default, which the wall's JSON leaves out.
            LocationLine => {
                let now = val.as_str().and_then(Location::from_name).unwrap_or_default();
                let options = Location::ALL.iter().map(|l| plain(t(l.label()).to_string(), json!(l.name()))).collect();
                list(t(now.label()), options)
            }
            LineStyle => {
                let options = self.m.line_styles.keys().map(|n| plain(n.clone(), json!(n))).collect();
                list(&cut(val.as_str().unwrap_or_default().to_string(), MAX_VALUE), options)
            }
            // Invisible lines first, then the styles; picked, none is null.
            RegionLines => {
                let invisible = t("<Invisible lines>").to_string();
                let mut options = vec![plain(invisible.clone(), Value::Null)];
                options.extend(self.m.line_styles.keys().map(|n| plain(n.clone(), json!(n))));
                let now = val.as_str().map_or_else(|| invisible.clone(), |s| cut(s.to_string(), MAX_VALUE));
                list(&now, options)
            }
            Justification => fixed(val, &[("top", "Top"), ("centre", "Centre"), ("bottom", "Bottom")], "Centre"),
            Choice(names) => fixed(val, names, ""),
            Framing => {
                let none = t("None").to_string();
                let mut options = vec![plain(none.clone(), Value::Null)];
                options.extend(self.m.types.framing.keys().map(|n| plain(n.clone(), json!(n))));
                let now = val.as_str().filter(|n| self.m.types.framing.contains_key(*n)).map_or(none, |n| cut(n.to_string(), MAX_VALUE));
                list(&now, options)
            }
            // Revit's greyed checkbox: Attach Top/Base and Detach Top/Base change it. Beside it, what it's attached to.
            Attached | Link => match self.id_of(val).filter(|id| self.m.elements.get(*id).is_some()) {
                Some(other) => (self.link_text(other), Editor::Link(other)),
                None if kind == Attached => read(t("No").to_string()),
                None => read(String::new()),
            },
            Text | Number | Angle | Length | LengthIf | Point | PaperPoint | Outline | Height | Limit(_) | NumberOr(_) | Scale => {
                // Blank across a selection may be values that differ, not nothing.
                let hint = match kind {
                    Limit(hint) | NumberOr(hint) if one => t(hint),
                    _ => "",
                };
                (shown(kind, val, units), Editor::Text { hint })
            }
        }
    }
}

/// A list of fixed `names` (the value a command names, and its label); the one `val` is, else `fallback` (the first with none).
fn fixed(val: &Value, names: &[(&str, &str)], fallback: &str) -> (String, Editor) {
    let now = names
        .iter()
        .find(|entry| val.as_str() == Some(entry.0))
        .map(|entry| entry.1)
        .or_else(|| names.iter().find(|entry| entry.1 == fallback).map(|entry| entry.1))
        .or_else(|| names.first().map(|entry| entry.1))
        .unwrap_or_default();
    let options = names.iter().map(|entry| plain(t(entry.1).to_string(), json!(entry.0))).collect();
    list(t(now), options)
}
