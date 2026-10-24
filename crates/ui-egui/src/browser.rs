//! The Project Browser: its tabs, which keep to one kind of thing, the search over them, and the tree, as Revit's.
//!
//! The tree is built as `Node`s, then drawn through the search: each word typed (`*` any run of characters, `?` any one,
//! case aside) must match an entry's name or the name of a group it is in, so `ceiling 2` finds Level 2's ceiling plan,
//! and what matched is marked.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, CollapsingHeader, Layout, Rect, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, WidgetInfo, WidgetType, vec2};

use bimcraft_core::model::{Element, Id};
use bimcraft_core::schedule;

use crate::icons::{self, Icon};
use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, SCHEDULES, Sheet, Tool, icon_button};

/// How long a search may be, in characters.
pub const MAX_FIND: usize = 64;

/// What the Project Browser keeps to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    All,
    Views,
    Schedules,
    Sheets,
    Families,
    /// Not yet: drafting legends are still to come.
    Legends,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::All, Tab::Views, Tab::Schedules, Tab::Sheets, Tab::Families, Tab::Legends];

    pub fn name(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Views => "Views",
            Tab::Schedules => "Schedules",
            Tab::Sheets => "Sheets",
            Tab::Families => "Families",
            Tab::Legends => "Legends",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Tab::All => Icon::ThinLines,
            Tab::Views => Icon::Plan,
            Tab::Schedules => Icon::Schedule,
            Tab::Sheets => Icon::Sheet,
            Tab::Families => Icon::Component,
            Tab::Legends => Icon::Legend,
        }
    }

    fn ready(self) -> bool {
        self != Tab::Legends
    }
}

/// The categories of the system's families the browser lists, besides the runs': what Edit Type edits and the Type Selector picks.
const SYSTEM_FAMILIES: [&str; 10] =
    ["wall", "floor", "roof", "ceiling", "door", "window", "column", "railing", "structuralcolumn", "footing"];

/// What a click on an entry does.
#[derive(Clone, Debug, PartialEq)]
enum Act {
    /// A group: it folds.
    Group,
    Open(Sheet),
    /// A family's type: the Component tool places it.
    Place(String, String),
    /// A plan's right-click menu: a duplicate of it, in a mode of the command's.
    Duplicate(Id, &'static str),
    /// A right-click menu's Delete: the elements it names go, as Delete takes them.
    Delete(Vec<Id>),
    /// A right-click menu's command, with its parameters: Delete Type.
    Do(&'static str, serde_json::Value),
    /// A system family's type: the Type Selector's now, for what is placed next.
    Pick(&'static str, String),
    /// A link: it is the selection, for Properties.
    Select(Id),
}

/// An entry of the tree, or a group of them.
#[derive(Clone, Debug)]
struct Node {
    label: String,
    act: Act,
    /// Picked: the view on screen, the type the Component tool places.
    on: bool,
    /// A group starts open.
    open: bool,
    kids: Vec<Node>,
    /// Its right-click menu: each item's label and what it does.
    menu: Vec<(&'static str, Act)>,
}

impl Node {
    fn group(label: &str, open: bool, kids: Vec<Node>) -> Node {
        Node { label: label.to_string(), act: Act::Group, on: false, open, kids, menu: Vec::new() }
    }

    fn leaf(label: &str, act: Act, on: bool) -> Node {
        Node { label: label.to_string(), act, on, open: false, kids: Vec::new(), menu: Vec::new() }
    }

    fn with_menu(mut self, menu: Vec<(&'static str, Act)>) -> Node {
        self.menu = menu;
        self
    }

    /// Whether it shows for the words still to match (those its groups didn't): it matches all of them, or, a group,
    /// something in it does.
    fn shows(&self, words: &[String]) -> bool {
        let left: Vec<String> = words.iter().filter(|w| spans(&self.label, w).is_none()).cloned().collect();
        left.is_empty() || self.kids.iter().any(|k| k.shows(&left))
    }
}

/// Where `word` matches in `text`, case aside: `*` any run of characters, `?` any one. The spans, in bytes, each as
/// short as it can be; none when it doesn't match, and an empty list when it matches nothing but itself (`*`).
pub fn spans(text: &str, word: &str) -> Option<Vec<(usize, usize)>> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let at: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, fold(c))).collect();
    let pattern: Vec<char> = word.chars().map(fold).collect();
    let n = at.len();
    let byte = |i: usize| at.get(i).map_or(text.len(), |(b, _)| *b);
    // The shortest match from `start`, if any: where it ends.
    let end_from = |start: usize| {
        let mut can = vec![false; n + 1];
        can[start] = true;
        for &c in &pattern {
            let mut next = vec![false; n + 1];
            match c {
                '*' => {
                    if let Some(first) = can.iter().position(|b| *b) {
                        next[first..].iter_mut().for_each(|b| *b = true);
                    }
                }
                _ => {
                    for i in 0..n {
                        if can[i] && (c == '?' || at[i].1 == c) {
                            next[i + 1] = true;
                        }
                    }
                }
            }
            can = next;
        }
        can.iter().position(|b| *b)
    };
    let mut out = Vec::new();
    let mut matched = false;
    let mut start = 0;
    while start <= n {
        match end_from(start) {
            Some(end) if end > start => {
                out.push((byte(start), byte(end)));
                matched = true;
                start = end;
            }
            Some(_) => {
                matched = true;
                start += 1;
            }
            None => start += 1,
        }
    }
    matched.then_some(out)
}

/// The words of a search.
pub fn words(find: &str) -> Vec<String> {
    find.split_whitespace().map(str::to_string).collect()
}

impl BimApp {
    /// The Project Browser's palette: its tabs and its search over the tree, which scrolls under them.
    pub(crate) fn browser_palette(&mut self, ui: &mut Ui) {
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 4 }).show(ui, |ui| {
            self.browser_tabs(ui);
            ui.add_space(4.0);
            self.browser_search(ui);
        });
        ui.separator();
        let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        scroll.id_salt("browser").show(ui, |ui| egui::Frame::new().inner_margin(8.0).show(ui, |ui| self.browser(ui)));
    }

    /// One tab a kind, its icon, and the one in force named too, so they fit a narrow palette.
    fn browser_tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for tab in Tab::ALL {
                let on = self.browser_tab == tab;
                let name = t(tab.name());
                // Named apart from the ribbon's Schedules and the tree's Families, for screen readers and tests.
                let what = tf("List {}", &[name]);
                let font = TextStyle::Small.resolve(ui.style()).clone();
                let text = on.then(|| ui.painter().layout_no_wrap(name.to_string(), font, pal().text));
                let width = 26.0 + text.as_ref().map_or(0.0, |g| g.size().x + 6.0);
                let sense = if tab.ready() { Sense::click() } else { Sense::hover() };
                let (rect, r) = ui.allocate_exact_size(vec2(width, 24.0), sense);
                r.widget_info(|| WidgetInfo::selected(WidgetType::Button, tab.ready(), on, &what));
                let p = ui.painter();
                let fill = if on {
                    pal().select_fill
                } else if r.hovered() && tab.ready() {
                    pal().hover_fill
                } else {
                    egui::Color32::TRANSPARENT
                };
                p.rect(rect, 4.0, fill, if on { Stroke::new(1.0, pal().accent) } else { Stroke::NONE }, StrokeKind::Inside);
                if r.has_focus() {
                    p.rect_stroke(rect, 4.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
                }
                let ink = match (on, tab.ready()) {
                    (true, _) => pal().accent,
                    (false, true) => pal().icon,
                    (false, false) => pal().muted.gamma_multiply(0.6),
                };
                let icon = Rect::from_center_size(rect.left_center() + vec2(13.0, 0.0), vec2(16.0, 16.0));
                icons::paint(p, tab.icon(), icon, ink);
                if let Some(g) = text {
                    p.galley(rect.left_center() + vec2(24.0, -g.size().y / 2.0), g, pal().accent);
                }
                let r = if tab.ready() { r.on_hover_text(&what) } else { r.on_hover_text(t("Coming in a later version")) };
                if r.clicked() {
                    self.browser_tab = tab;
                }
            }
        });
    }

    /// The search: as it's typed, the tree keeps to what matches.
    fn browser_search(&mut self, ui: &mut Ui) {
        // Its own where it's drawn: in the frame a palette docks or floats it is drawn in two places.
        let id = ui.make_persistent_id("browser-find");
        // The hint goes once the field is clicked into, not only once something is typed.
        let focused = ui.memory(|m| m.has_focus(id));
        let frame = egui::Frame::new()
            .fill(pal().surface)
            .stroke(Stroke::new(1.0, if focused { pal().accent } else { pal().border }))
            .corner_radius(4.0)
            .inner_margin(egui::Margin { left: 4, right: 2, top: 1, bottom: 1 });
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 22.0), Sense::hover());
                icons::paint(ui.painter(), Icon::Search, Rect::from_center_size(r.center(), vec2(15.0, 15.0)), pal().muted);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !self.browser_find.is_empty() && icon_button(ui, Icon::Close, "Clear the Search").clicked() {
                        self.browser_find.clear();
                    }
                    let hint = if focused { String::new() } else { t("Search…").to_string() };
                    let edit = TextEdit::singleline(&mut self.browser_find)
                        .id(id)
                        .hint_text(hint)
                        .frame(egui::Frame::NONE)
                        .char_limit(MAX_FIND)
                        .desired_width(ui.available_width());
                    ui.add(edit).widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, t("Search the Project Browser")));
                });
            });
        });
    }

    /// The tree: what the tab keeps to, as far as it matches the search.
    fn browser(&mut self, ui: &mut Ui) {
        if self.browser_tab == Tab::Legends {
            ui.label(egui::RichText::new(t("Legends are coming in a later version.")).color(pal().muted));
            return;
        }
        let tree = self.browser_tree();
        let words = words(&self.browser_find);
        let shown: Vec<&Node> = tree.iter().filter(|n| n.shows(&words)).collect();
        if shown.is_empty() {
            let note = if words.is_empty() { t("Nothing here yet.") } else { t("Nothing matches the search.") };
            ui.label(egui::RichText::new(note).color(pal().muted));
            return;
        }
        let mut act = None;
        for node in shown {
            self.browser_node(ui, node, &words, "", &mut act);
        }
        match act {
            Some(Act::Open(sheet)) => self.open_sheet(sheet),
            Some(Act::Place(family, ty)) => {
                self.component = Some((family, ty));
                self.set_tool(Tool::Component);
            }
            Some(Act::Duplicate(view, mode)) => self.duplicate_view_of(view, mode),
            Some(Act::Delete(ids)) => {
                self.run("delete", serde_json::json!({"ids": ids}));
            }
            Some(Act::Do(command, params)) => {
                self.run(command, params);
            }
            Some(Act::Pick(category, name)) => {
                self.types.insert(category, name);
            }
            Some(Act::Select(id)) => self.selection = vec![id],
            _ => {}
        }
    }

    /// An entry, or a group and what in it shows: the words its groups matched are spent, the rest must match below.
    fn browser_node(&self, ui: &mut Ui, node: &Node, words: &[String], path: &str, act: &mut Option<Act>) {
        let left: Vec<String> = words.iter().filter(|w| spans(&node.label, w).is_none()).cloned().collect();
        let text = marked(ui, &node.label, words);
        if node.act != Act::Group {
            let r = ui.selectable_label(node.on, text);
            if r.clicked() {
                *act = Some(node.act.clone());
            }
            if !node.menu.is_empty() {
                r.context_menu(|ui| {
                    for (label, item) in &node.menu {
                        if ui.button(t(label)).clicked() {
                            *act = Some(item.clone());
                            ui.close();
                        }
                    }
                });
            }
            return;
        }
        let path = format!("{path}/{}", node.label);
        let mut header = CollapsingHeader::new(text).id_salt(("browser", &path)).default_open(node.open);
        if !words.is_empty() {
            header = header.open(Some(true));
        }
        let shown = header.show(ui, |ui| {
            for kid in node.kids.iter().filter(|k| k.shows(&left)) {
                self.browser_node(ui, kid, &left, &path, act);
            }
        });
        if !node.menu.is_empty() {
            shown.header_response.context_menu(|ui| {
                for (label, item) in &node.menu {
                    if ui.button(t(label)).clicked() {
                        *act = Some(item.clone());
                        ui.close();
                    }
                }
            });
        }
    }

    /// What the browser lists under the tab in force.
    fn browser_tree(&self) -> Vec<Node> {
        let tab = self.browser_tab;
        let wants = |t: Tab| tab == Tab::All || tab == t;
        let mut out = Vec::new();
        if wants(Tab::Views) {
            out.push(self.views_node());
        }
        if wants(Tab::Schedules) {
            out.push(self.schedules_node());
        }
        if wants(Tab::Sheets) && (tab == Tab::Sheets || !self.sheets().is_empty()) {
            let sheets = self.sheets().into_iter().map(|(id, name)| self.view_leaf(&name, Sheet::Paper(id))).collect();
            out.push(Node::group(t("Sheets (all)"), true, sheets));
        }
        if wants(Tab::Families) {
            out.push(self.families_node(tab == Tab::Families));
        }
        if tab == Tab::All
            && let Some(links) = self.links_node()
        {
            out.push(links);
        }
        // Keeping to one kind, its group is all there is: what's in it, without it.
        if tab != Tab::All && out.len() == 1 {
            return out.pop().map(|n| n.kids).unwrap_or_default();
        }
        out
    }

    fn view_leaf(&self, name: &str, sheet: Sheet) -> Node {
        Node::leaf(name, Act::Open(sheet), self.sheet == sheet)
    }

    /// Views (all): plans, ceiling plans, the 3D view, elevations and sections.
    fn views_node(&self) -> Node {
        let m = &self.session.model;
        let levels: Vec<(Id, String)> = m.levels().iter().map(|(id, l)| (*id, l.name.clone())).collect();
        // Sections and elevations, by name.
        let views = |category: &str| {
            let mut v: Vec<(Id, String)> = m
                .elements
                .iter()
                .filter(|(_, e)| e.category() == category)
                .filter_map(|(id, _)| Some((*id, m.section(*id)?.name.clone())))
                .collect();
            v.sort_by(|a, b| a.1.cmp(&b.1));
            v
        };
        // Each level's own plan, then its plan views, which also have a right-click Delete.
        let keyed: Vec<(Id, String, bool)> = levels
            .iter()
            .flat_map(|(id, name)| {
                let views = m.plan_views(*id).into_iter().map(|(v, pv)| (v, pv.name.clone(), true));
                std::iter::once((*id, name.clone(), false)).chain(views)
            })
            .collect();
        let plan_leaf = |id: Id, name: &str, view: bool, sheet: Sheet| {
            let mut menu = vec![
                ("Duplicate", Act::Duplicate(id, "duplicate")),
                ("Duplicate with Detailing", Act::Duplicate(id, "detailing")),
                ("Duplicate as Dependent", Act::Duplicate(id, "dependent")),
            ];
            if view {
                menu.push(("Delete", Act::Delete(vec![id])));
            }
            self.view_leaf(name, sheet).with_menu(menu)
        };
        let plans = keyed.iter().map(|(id, name, view)| plan_leaf(*id, name, *view, Sheet::Plan(*id))).collect();
        let ceilings = keyed.iter().map(|(id, name, view)| plan_leaf(*id, name, *view, Sheet::Ceiling(*id))).collect();
        let mut kids = vec![
            Node::group(t("Floor Plans"), true, plans),
            Node::group(t("Ceiling Plans"), true, ceilings),
            Node::group(t("3D Views"), true, vec![self.view_leaf("{3D}", Sheet::View3D)]),
        ];
        for (group, category) in [("Elevations (Building Elevation)", "elevation"), ("Sections (Building Section)", "section")] {
            let views = views(category);
            if !views.is_empty() {
                kids.push(Node::group(t(group), true, views.iter().map(|(id, name)| self.view_leaf(name, Sheet::Section(*id))).collect()));
            }
        }
        Node::group(t("Views (all)"), true, kids)
    }

    /// Schedules/Quantities (all): the six every project has, then the project's own.
    fn schedules_node(&self) -> Node {
        let m = &self.session.model;
        let mut kids: Vec<Node> = SCHEDULES
            .iter()
            .map(|(cat, name)| {
                let name = match schedule::standard(m, cat) {
                    (Some(_), sc) => sc.name,
                    (None, _) => t(name).to_string(),
                };
                self.view_leaf(&name, Sheet::Schedule(cat))
            })
            .collect();
        kids.extend(self.schedules().into_iter().map(|(id, name)| self.view_leaf(name, Sheet::Table(id))));
        Node::group(t("Schedules/Quantities (all)"), true, kids)
    }

    /// The linked models, each with a menu to read it again or take it out; none where the project links nothing.
    fn links_node(&self) -> Option<Node> {
        let m = &self.session.model;
        let kids: Vec<Node> = m
            .elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Link(l) => {
                    let label = if m.loaded.contains_key(&l.path) { l.name.clone() } else { tf("{} (not loaded)", &[&l.name]) };
                    let menu = vec![("Reload", Act::Do("reload", serde_json::json!({"id": id}))), ("Remove", Act::Delete(vec![*id]))];
                    Some(Node::leaf(&label, Act::Select(*id), self.selection == [*id]).with_menu(menu))
                }
                _ => None,
            })
            .collect();
        (!kids.is_empty())
            .then(|| Node::group(t("Linked Models"), true, kids).with_menu(vec![("Reload All", Act::Do("reload", serde_json::json!({})))]))
    }

    /// The sheets, by number, as "A101 - Name".
    fn sheets(&self) -> Vec<(Id, String)> {
        let m = &self.session.model;
        let mut sheets: Vec<(Id, String)> =
            m.elements.keys().filter_map(|id| m.sheet(*id).map(|s| (*id, format!("{} - {}", s.number, s.name)))).collect();
        sheets.sort_by(|a, b| a.1.cmp(&b.1));
        sheets
    }

    /// Families, each its types: one picked, the Component tool places it. Folded but on its own tab.
    /// Families: each category of the system's (walls, floors, doors…) with its types, then the project's component families.
    /// A right click on a type or a family has Delete, which the project refuses while something uses it.
    fn families_node(&self, open: bool) -> Node {
        let m = &self.session.model;
        let delete = |params: serde_json::Value| vec![("Delete", Act::Do("delete_type", params))];
        let system = SYSTEM_FAMILIES.iter().chain(&bimcraft_core::model::RUN_CATEGORIES).filter_map(|category| {
            let names = m.type_names(category);
            let leaves = names
                .iter()
                .map(|ty| {
                    let on = self.types.get(category).is_some_and(|t| t == ty);
                    Node::leaf(ty, Act::Pick(category, ty.to_string()), on)
                        .with_menu(delete(serde_json::json!({"category": category, "type": ty})))
                })
                .collect();
            (!names.is_empty()).then(|| Node::group(&crate::lang::category(category, true), false, leaves))
        });
        let components = m.families.iter().map(|(name, f)| {
            let types = f
                .types
                .keys()
                .map(|ty| {
                    let on = self.tool == Tool::Component && self.component.as_ref() == Some(&(name.clone(), ty.clone()));
                    Node::leaf(ty, Act::Place(name.clone(), ty.clone()), on)
                        .with_menu(delete(serde_json::json!({"family": name, "type": ty})))
                })
                .collect();
            Node::group(name, false, types).with_menu(delete(serde_json::json!({"family": name})))
        });
        Node::group(t("Families"), open, system.chain(components).collect())
    }
}

/// A name with what the search's words matched in it marked.
fn marked(ui: &Ui, label: &str, words: &[String]) -> egui::WidgetText {
    let mut hits: Vec<(usize, usize)> = words.iter().filter_map(|w| spans(label, w)).flatten().collect();
    if hits.is_empty() {
        return label.into();
    }
    hits.sort();
    let font = TextStyle::Body.resolve(ui.style());
    let plain = TextFormat { font_id: font.clone(), color: ui.visuals().text_color(), ..Default::default() };
    let lit = TextFormat { background: pal().accent.gamma_multiply(0.3), ..plain.clone() };
    let mut job = LayoutJob::default();
    let mut at = 0;
    for (from, to) in hits {
        let from = from.max(at);
        if to <= from {
            continue;
        }
        job.append(&label[at..from], 0.0, plain.clone());
        job.append(&label[from..to], 0.0, lit.clone());
        at = to;
    }
    job.append(&label[at..], 0.0, plain);
    job.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_matches_anywhere_case_aside_and_marks_each_place() {
        assert_eq!(spans("Level 1", "lev"), Some(vec![(0, 3)]));
        assert_eq!(spans("Level 1", "EL"), Some(vec![(3, 5)]));
        assert_eq!(spans("Levels", "e"), Some(vec![(1, 2), (3, 4)]), "every place");
        assert_eq!(spans("Level 1", "x"), None);
    }

    #[test]
    fn a_star_is_any_run_and_a_question_mark_any_one() {
        assert_eq!(spans("Level 12", "l*1"), Some(vec![(0, 7)]), "as short as it can");
        assert_eq!(spans("Level 12", "level ?2"), Some(vec![(0, 8)]));
        assert_eq!(spans("Level 2", "level ?2"), None, "? is one character, not none");
        assert_eq!(spans("Door", "*"), Some(vec![]), "matches, with nothing to mark");
        assert_eq!(spans("A101 - Plans", "a1?1"), Some(vec![(0, 4)]));
    }

    #[test]
    fn spans_are_in_bytes_whatever_the_letters() {
        let text = "Nível 1";
        let hit = spans(text, "vel").unwrap_or_default();
        assert_eq!(hit.iter().map(|(a, b)| &text[*a..*b]).collect::<Vec<_>>(), ["vel"]);
        assert_eq!(spans("ÉLÉVATION", "élé"), Some(vec![(0, 5)]));
    }

    #[test]
    fn a_group_shows_for_words_it_or_what_is_in_it_matches() {
        let tree = Node::group(
            "Ceiling Plans",
            true,
            vec![Node::leaf("Level 1", Act::Open(Sheet::View3D), false), Node::leaf("Level 2", Act::Open(Sheet::View3D), false)],
        );
        assert!(tree.shows(&words("ceiling 2")), "the group the one word, an entry the other");
        assert!(tree.shows(&words("ceil*")));
        assert!(!tree.shows(&words("floor 2")));
        assert!(tree.kids[1].shows(&words("2")) && !tree.kids[0].shows(&words("2")));
    }
}
