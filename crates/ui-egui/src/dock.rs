//! Palettes that dock and float, and views that float, as Revit's do. A palette (Properties, the Project Browser and
//! any to come: a row of [`Palette`]) docks down the window's left or right side or along its bottom, in a stack of
//! its own (stacks on a side share it, one over the other) or as a tab of another's, or floats in a window of its
//! own. Its title, or its tab, drags it: drop zones show where it can land, and a drop anywhere else floats it there.
//! Its Dock menu does the same by name. A view's tab drags off the views' tab bar into a window (or its menu's Float),
//! and that window's title drags it back onto the bar. [`Docking`] is the palettes' layout, apart from how it's drawn,
//! so it can be tested and kept in the settings file; the view that takes the tools is the active one
//! ([`BimApp::sheet`]): any other on screen draws as it is, and a click on it makes it the active one.

use bimcraft_i18n::msg;
use bimcraft_plugin::{Dock as PluginDock, PluginPanel, Widget};

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::FRAC_PI_2;

use egui::{
    Align, Align2, Context, CornerRadius, FontId, Frame, Id as UiId, Layout, Order, Panel, Popup, Pos2, Rect, Sense, Stroke, StrokeKind,
    Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Sheet, icon_button};

/// A palette that docks. A new one is a row here, its body in [`BimApp::palette_body`] and its name in `lang.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Palette {
    Properties,
    Browser,
    /// BIMCraft's tools, in place of the ribbon: Rev, which has the ribbon, never shows it.
    Tools,
    /// The chat with a model that manages the project (`assistant.rs`): closed until the user opens it.
    Assistant,
    /// PyAddins > Player: the user's Python scripts, each to run with a form of its inputs.
    Scripts,
    /// A palette a plugin declared, by its place in [`plugin_palettes`]. The registry is the plugins' and is rebuilt
    /// when they are read: the index is stable within a session, and the settings file keeps the name alone.
    Plugin(usize),
}

/// A palette a plugin declares, as the window keeps it while the plugins are loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginPalette {
    /// `plugin:<plugin id>:<panel id>`, as the settings file keeps it.
    pub name: &'static str,
    /// The palette's title: the plugin's data, not translated.
    pub title: &'static str,
    pub plugin: &'static str,
    /// The plugin's name, for the palette's tooltip.
    pub plugin_name: &'static str,
    pub panel: &'static str,
    /// The side it docks on when it first opens.
    pub dock: Side,
    /// Whether it opens as a window of its own rather than docked.
    pub float: bool,
    pub on_open: Option<&'static str>,
    pub refresh: Option<&'static str>,
    pub widgets: Vec<Widget>,
}

thread_local! {
    /// The panels the loaded plugins declare, in the order of [`Palette::known`]. The window is one thread, and a test
    /// has its own registry, so plugins one test loads are no other test's.
    static PLUGIN_PALETTES: RefCell<Vec<&'static PluginPalette>> = const { RefCell::new(Vec::new()) };
}

/// Puts the panels of the plugins just read in the registry, in the order [`Plugins::panels`](bimcraft_plugin::Plugins::panels)
/// gave them. Their strings are kept for the session: a palette's name is `&'static str`, as every built-in palette's is.
pub fn set_plugin_palettes(panels: &[PluginPanel]) {
    let old = plugin_palettes();
    let list: Vec<&'static PluginPalette> = panels
        .iter()
        .map(|panel| {
            let name = panel.name();
            // A panel the folder has again, unchanged (a refresh, a permission granted), keeps its entry: a reload of the
            // same plugins must not grow the strings kept for the session.
            if let Some(entry) = old.iter().find(|entry| same_panel(entry, panel, &name)) {
                return *entry;
            }
            let entry: &'static PluginPalette = Box::leak(Box::new(PluginPalette {
                name: intern(&name),
                title: intern(&panel.title),
                plugin: intern(&panel.plugin),
                plugin_name: intern(&panel.plugin_name),
                panel: intern(&panel.id),
                dock: match panel.dock {
                    PluginDock::Left => Side::Left,
                    PluginDock::Right | PluginDock::Float => Side::Right,
                    PluginDock::Bottom => Side::Bottom,
                },
                float: panel.dock == PluginDock::Float,
                on_open: panel.on_open.as_deref().map(intern),
                refresh: panel.refresh.as_deref().map(intern),
                widgets: panel.widgets.clone(),
            }));
            entry
        })
        .collect();
    PLUGIN_PALETTES.with(|r| *r.borrow_mut() = list);
}

/// Whether `entry` is the palette `panel` declares, under the name `name`.
fn same_panel(entry: &PluginPalette, panel: &PluginPanel, name: &str) -> bool {
    entry.name == name
        && entry.title == panel.title
        && entry.plugin == panel.plugin
        && entry.plugin_name == panel.plugin_name
        && entry.panel == panel.id
        && entry.on_open == panel.on_open.as_deref()
        && entry.refresh == panel.refresh.as_deref()
        && entry.float == (panel.dock == PluginDock::Float)
        && entry.dock
            == match panel.dock {
                PluginDock::Left => Side::Left,
                PluginDock::Right | PluginDock::Float => Side::Right,
                PluginDock::Bottom => Side::Bottom,
            }
        && entry.widgets == panel.widgets
}

fn intern(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

/// The panels of the plugins loaded now, in registry order.
pub fn plugin_palettes() -> Vec<&'static PluginPalette> {
    PLUGIN_PALETTES.with(|r| r.borrow().clone())
}

/// The panel [`Palette::Plugin`] `i` stands for: none while the registry has fewer, as when a plugin was switched off.
pub fn plugin_palette(i: usize) -> Option<&'static PluginPalette> {
    PLUGIN_PALETTES.with(|r| r.borrow().get(i).copied())
}

impl Palette {
    /// The most palettes there are, and the longest name one has: a settings file's, and a plugin's.
    const MOST: usize = 512;
    const NAME: usize = 200;

    /// Every palette the window knows, the one place that lists them: the settings' switches and a preset's editor ask here, and
    /// the layout reads and writes its names through this list. A plugin's palettes come here with the plugin registry.
    pub fn known() -> Vec<Palette> {
        let mut all = vec![Palette::Properties, Palette::Browser, Palette::Tools, Palette::Assistant, Palette::Scripts];
        all.extend((0..plugin_palettes().len()).map(Palette::Plugin));
        all
    }

    /// Whether it is open in a window that starts afresh (and in one whose settings are from before it came).
    fn opens_at_start(self) -> bool {
        !matches!(self, Palette::Scripts | Palette::Plugin(_))
    }

    /// The side it docks on when it is first opened.
    fn first_side(self) -> Side {
        match self {
            Palette::Scripts => Side::Right,
            Palette::Plugin(i) => plugin_palette(i).map_or(Side::Right, |p| p.dock),
            _ => Side::Left,
        }
    }

    /// Its name: what its tab says (in the interface's language) and what the settings file keeps it as. A plugin's is
    /// `plugin:<plugin id>:<panel id>`, its data, and the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Palette::Properties => "Properties",
            Palette::Browser => msg!("Project Browser"),
            Palette::Tools => msg!("Tools"),
            Palette::Assistant => msg!("Assistant"),
            Palette::Scripts => msg!("Script Automation"),
            Palette::Plugin(i) => plugin_palette(i).map_or("plugin:unknown", |p| p.name),
        }
    }

    /// What its title and tab say in the profile this frame wears: BIMCraft calls Properties the Inspector and the Project
    /// Browser the Navigator. A plugin's palette says its own title.
    pub fn label(self) -> &'static str {
        match (self, crate::style::rev()) {
            (Palette::Properties, false) => msg!("Inspector"),
            (Palette::Browser, false) => msg!("Navigator"),
            (Palette::Plugin(i), _) => plugin_palette(i).map_or("", |p| p.title),
            _ => self.name(),
        }
    }

    /// What a menu, a tab or a title calls it: the interface's word for a built-in palette, and a plugin's own title,
    /// which is its data and is never translated.
    pub fn title(self) -> String {
        match self {
            Palette::Plugin(_) => self.label().to_string(),
            _ => t(self.label()).to_string(),
        }
    }

    /// The palette the layout calls `name`, of those `known` this build has: none for a plugin's, which a newer build may know.
    pub fn named(name: &str, known: &[Palette]) -> Option<Palette> {
        known.iter().copied().find(|p| p.name() == name)
    }
}

/// How a layout is read: the window's own (`settings.json`) opens a palette the file never named, so a palette added by a newer
/// version still shows; a preset's keeps exactly the palettes it was saved with, and one added later stays closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Live,
    Preset,
}

/// A side of the window a palette docks on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Left,
    Right,
    Bottom,
}

impl Side {
    pub const ALL: [Side; 3] = [Side::Left, Side::Right, Side::Bottom];

    fn key(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Bottom => "bottom",
        }
    }

    /// What the Dock menu and the drop zone call it.
    pub fn label(self) -> &'static str {
        match self {
            Side::Left => "Dock Left",
            Side::Right => "Dock Right",
            Side::Bottom => "Dock Bottom",
        }
    }
}

/// Palettes docked together, as tabs: never none.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub tabs: Vec<Palette>,
    /// The tab shown.
    pub active: usize,
}

impl Stack {
    fn of(p: Palette) -> Stack {
        Stack { tabs: vec![p], active: 0 }
    }

    /// The palette it shows.
    pub fn shown(&self) -> Option<Palette> {
        self.tabs.get(self.active).or(self.tabs.first()).copied()
    }
}

/// A stack in a window of its own: where its top left corner is, and its size, in points.
#[derive(Clone, Debug, PartialEq)]
pub struct Float {
    pub stack: Stack,
    pub at: Pos2,
    pub size: Vec2,
}

/// Where a palette goes: docked on a side (a stack of its own, after the others there), a tab of the stack that holds
/// another palette, a stack of its own just before the docked stack that holds another, or floating with its corner at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Side(Side),
    With(Palette),
    Before(Palette),
    Float(Pos2),
}

/// How wide a side holding the Tools palette alone starts, in points.
const TOOLS_WIDTH: f32 = 240.0;
/// How much wider the right side starts than the left, in points.
const RIGHT_EXTRA: f32 = 12.0;
/// A palette's window, when it starts floating.
pub const FLOAT_SIZE: Vec2 = vec2(300.0, 420.0);
/// A view's window, when it starts floating.
pub const VIEW_SIZE: Vec2 = vec2(640.0, 440.0);
/// The least height the last card on a side is given: its header, its tabs and its search, with room for a few rows.
const LAST_MIN: f32 = 150.0;
/// How wide the strip along the docked view's edge is that docks a palette there, in points.
const EDGE: f32 = 36.0;
/// How far the strips stand in from the view's edge, in points.
const INSET: f32 = 6.0;
/// How far a floating window's corner may be from the origin, in points: a settings file can't put one at infinity.
const FAR: f32 = 20_000.0;

/// Where the palettes are.
#[derive(Clone, Debug, PartialEq)]
pub struct Docking {
    pub left: Vec<Stack>,
    pub right: Vec<Stack>,
    pub bottom: Vec<Stack>,
    pub floats: Vec<Float>,
    /// The side each palette was last docked on, where showing it again puts it.
    home: BTreeMap<Palette, Side>,
    /// Names a file kept that this build has no palette for (a plugin's, while its plugin is off): not drawn, but written back in
    /// `closed`, so turning the plugin on again finds the layout as it was.
    pub unknown: Vec<String>,
    /// The unknown names a file had in a stack or a window, which were open: when their plugin arrives they are shown
    /// again, where the plugin says. A name only ever closed is not here.
    pub unknown_open: Vec<String>,
}

impl Default for Docking {
    /// BIMCraft's: the tools down the left, the Inspector over the Navigator down the right.
    fn default() -> Docking {
        Docking {
            left: vec![Stack::of(Palette::Tools)],
            right: vec![Stack::of(Palette::Properties), Stack::of(Palette::Browser)],
            bottom: Vec::new(),
            floats: Vec::new(),
            home: BTreeMap::new(),
            unknown: Vec::new(),
            unknown_open: Vec::new(),
        }
    }
}

impl Docking {
    /// Rev's, as Revit's: Properties over the Project Browser, down the left side.
    pub fn revit() -> Docking {
        Docking {
            left: vec![Stack::of(Palette::Properties), Stack::of(Palette::Browser)],
            right: Vec::new(),
            bottom: Vec::new(),
            floats: Vec::new(),
            home: BTreeMap::from([(Palette::Tools, Side::Left)]),
            unknown: Vec::new(),
            unknown_open: Vec::new(),
        }
    }

    pub fn side(&self, side: Side) -> &[Stack] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
            Side::Bottom => &self.bottom,
        }
    }

    fn side_mut(&mut self, side: Side) -> &mut Vec<Stack> {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
            Side::Bottom => &mut self.bottom,
        }
    }

    /// Every stack, docked and floating.
    fn stacks_mut(&mut self) -> impl Iterator<Item = &mut Stack> {
        self.left.iter_mut().chain(self.right.iter_mut()).chain(self.bottom.iter_mut()).chain(self.floats.iter_mut().map(|f| &mut f.stack))
    }

    /// The stack `p` is in.
    pub fn stack_of(&self, p: Palette) -> Option<&Stack> {
        self.left.iter().chain(&self.right).chain(&self.bottom).chain(self.floats.iter().map(|f| &f.stack)).find(|s| s.tabs.contains(&p))
    }

    fn stack_of_mut(&mut self, p: Palette) -> Option<&mut Stack> {
        self.stacks_mut().find(|s| s.tabs.contains(&p))
    }

    /// Whether `p` is open, docked or floating.
    pub fn shown(&self, p: Palette) -> bool {
        self.stack_of(p).is_some()
    }

    /// The side `p` is docked on: none while it floats or is closed.
    pub fn docked(&self, p: Palette) -> Option<Side> {
        Side::ALL.into_iter().find(|s| self.side(*s).iter().any(|st| st.tabs.contains(&p)))
    }

    /// The window `p` floats in.
    pub fn float_of(&self, p: Palette) -> Option<&Float> {
        self.floats.iter().find(|f| f.stack.tabs.contains(&p))
    }

    pub fn float_of_mut(&mut self, p: Palette) -> Option<&mut Float> {
        self.floats.iter_mut().find(|f| f.stack.tabs.contains(&p))
    }

    /// Takes `p` out of the layout (a stack it was alone in goes with it), remembering the side it was docked on.
    pub fn take(&mut self, p: Palette) {
        if let Some(side) = self.docked(p) {
            self.home.insert(p, side);
        }
        for stack in self.stacks_mut() {
            if let Some(i) = stack.tabs.iter().position(|q| *q == p) {
                stack.tabs.remove(i);
                if stack.active > i || stack.active >= stack.tabs.len() {
                    stack.active = stack.active.saturating_sub(1);
                }
            }
        }
        for side in Side::ALL {
            self.side_mut(side).retain(|s| !s.tabs.is_empty());
        }
        self.floats.retain(|f| !f.stack.tabs.is_empty());
    }

    /// The side the docked stack holding `q` is on, and its place there.
    fn place_of(&self, q: Palette) -> Option<(Side, usize)> {
        let side = self.docked(q)?;
        Some((side, self.side(side).iter().position(|s| s.tabs.contains(&q))?))
    }

    /// Puts `p` where `to` says, out of where it was. A tab dropped on its own stack, on a palette that isn't open, or
    /// before itself or one that isn't docked, stays where it was.
    pub fn put(&mut self, p: Palette, to: Target) {
        if let Target::With(q) = to
            && (q == p || !self.shown(q))
        {
            return;
        }
        if let Target::Before(q) = to
            && (q == p || self.docked(q).is_none())
        {
            return;
        }
        self.take(p);
        match to {
            Target::Side(side) => {
                self.side_mut(side).push(Stack::of(p));
                self.home.insert(p, side);
            }
            Target::With(q) => {
                if let Some(side) = self.docked(q) {
                    self.home.insert(p, side);
                }
                if let Some(stack) = self.stack_of_mut(q) {
                    stack.tabs.push(p);
                    stack.active = stack.tabs.len() - 1;
                }
            }
            Target::Before(q) => {
                if let Some((side, at)) = self.place_of(q) {
                    self.side_mut(side).insert(at, Stack::of(p));
                    self.home.insert(p, side);
                }
            }
            Target::Float(at) => self.floats.push(Float { stack: Stack::of(p), at, size: FLOAT_SIZE }),
        }
    }

    /// Docks the whole window `p` floats in: a stack of its own on a side or before another docked stack, or its tabs after
    /// another stack's. Dropped anywhere else, it stays a window (dragging it moved it already).
    pub fn put_window(&mut self, p: Palette, to: Target) {
        let Some(i) = self.floats.iter().position(|f| f.stack.tabs.contains(&p)) else { return };
        match to {
            Target::Side(side) => {
                let f = self.floats.remove(i);
                for q in &f.stack.tabs {
                    self.home.insert(*q, side);
                }
                self.side_mut(side).push(f.stack);
            }
            Target::Before(q) => {
                if let Some((side, at)) = self.place_of(q) {
                    let f = self.floats.remove(i);
                    for tab in &f.stack.tabs {
                        self.home.insert(*tab, side);
                    }
                    self.side_mut(side).insert(at, f.stack);
                }
            }
            Target::With(q) if self.shown(q) && !self.floats[i].stack.tabs.contains(&q) => {
                let f = self.floats.remove(i);
                let shown = f.stack.shown();
                for tab in f.stack.tabs {
                    self.put(tab, Target::With(q));
                }
                if let Some(s) = shown {
                    self.activate(s);
                }
            }
            _ => {}
        }
    }

    /// Opens `p` where it was last docked (on the left, if it never was), or closes it.
    pub fn show(&mut self, p: Palette, on: bool) {
        match (on, self.shown(p)) {
            (true, false) => {
                // A plugin that asks for a window of its own gets one, over the views.
                if let Palette::Plugin(i) = p
                    && plugin_palette(i).is_some_and(|panel| panel.float)
                {
                    self.put(p, Target::Float(pos2(160.0 + 24.0 * self.floats.len() as f32, 160.0)));
                    return;
                }
                let side = self.home.get(&p).copied().unwrap_or(p.first_side());
                // A chat wants height: the Assistant opens as a tab of the Project Browser's stack, unless it was last put on another side.
                match self.docked(Palette::Browser) {
                    Some(b) if p == Palette::Assistant && self.home.get(&p).is_none_or(|s| *s == b) => {
                        self.put(p, Target::With(Palette::Browser))
                    }
                    _ => self.put(p, Target::Side(side)),
                }
            }
            (false, true) => self.take(p),
            _ => {}
        }
    }

    /// Brings `p` to the front of its stack.
    pub fn activate(&mut self, p: Palette) {
        if let Some(stack) = self.stack_of_mut(p)
            && let Some(i) = stack.tabs.iter().position(|q| *q == p)
        {
            stack.active = i;
        }
    }

    /// As the settings file keeps it: `known` names the palettes this build has, its own and, with the plugin registry, a plugin's.
    pub fn to_json(&self, known: &[Palette]) -> Value {
        let stack = |s: &Stack| json!({"tabs": s.tabs.iter().map(|p| p.name()).collect::<Vec<_>>(), "active": s.active});
        let mut v = json!({});
        for side in Side::ALL {
            v[side.key()] = self.side(side).iter().map(stack).collect();
        }
        v["floats"] = self
            .floats
            .iter()
            .map(|f| {
                let mut s = stack(&f.stack);
                s["at"] = json!([f.at.x, f.at.y]);
                s["size"] = json!([f.size.x, f.size.y]);
                s
            })
            .collect();
        let mut closed: Vec<&str> = known.iter().filter(|p| !self.shown(**p)).map(|p| p.name()).collect();
        // A palette this build has no name for is kept closed, as it was read: a plugin's survives its plugin being off.
        for name in &self.unknown {
            if !closed.contains(&name.as_str()) {
                closed.push(name);
            }
        }
        v["closed"] = json!(closed);
        v["home"] = self.home.iter().map(|(p, s)| (p.name().to_string(), json!(s.key()))).collect::<serde_json::Map<_, _>>().into();
        v
    }

    /// Reads back what [`to_json`](Self::to_json) wrote, `policy` saying what a palette it neither places nor closes does (see
    /// [`Policy`]), `known` what palettes this build has. As hostile as any file: a palette it names twice is left out, numbers are
    /// finite and in range, and a name it doesn't know is kept to write back. Anything but an object is the default layout.
    pub fn from_json(v: &Value, policy: Policy, known: &[Palette]) -> Docking {
        if !v.is_object() {
            return Docking::default();
        }
        let mut unknown = Vec::new();
        let mut unknown_open = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack = |v: &Value, unknown: &mut Vec<String>, open: &mut Vec<String>| -> Option<Stack> {
            let tabs: Vec<Palette> = v["tabs"]
                .as_array()?
                .iter()
                .filter_map(|n| n.as_str())
                .filter_map(|n| match Palette::named(n, known) {
                    Some(p) => Some(p),
                    None => {
                        if !n.is_empty() && n.chars().count() <= Palette::NAME && unknown.len() < Palette::MOST {
                            if !unknown.iter().any(|q| q == n) {
                                unknown.push(n.to_string());
                            }
                            if !open.iter().any(|q| q == n) {
                                open.push(n.to_string());
                            }
                        }
                        None
                    }
                })
                .filter(|p| seen.insert(*p))
                .collect();
            let active = v["active"].as_u64().and_then(|a| usize::try_from(a).ok()).filter(|a| *a < tabs.len()).unwrap_or(0);
            (!tabs.is_empty()).then_some(Stack { tabs, active })
        };
        let mut d = Docking {
            left: Vec::new(),
            right: Vec::new(),
            bottom: Vec::new(),
            floats: Vec::new(),
            home: BTreeMap::new(),
            unknown: Vec::new(),
            unknown_open: Vec::new(),
        };
        for side in Side::ALL {
            let stacks: Vec<Stack> = v[side.key()]
                .as_array()
                .map(|a| a.iter().filter_map(|s| stack(s, &mut unknown, &mut unknown_open)).collect())
                .unwrap_or_default();
            *d.side_mut(side) = stacks;
        }
        let pair = |v: &Value, lo: f32, hi: f32| -> Option<Vec2> {
            let a = v.as_array()?;
            // Clamped before it's an f32, which 1e300 would overflow.
            let n = |i: usize| a.get(i)?.as_f64().filter(|x| x.is_finite()).map(|x| x.clamp(f64::from(lo), f64::from(hi)) as f32);
            Some(vec2(n(0)?, n(1)?))
        };
        for f in v["floats"].as_array().into_iter().flatten() {
            let (Some(at), Some(size)) = (pair(&f["at"], -FAR, FAR), pair(&f["size"], 120.0, 4000.0)) else { continue };
            if let Some(s) = stack(f, &mut unknown, &mut unknown_open) {
                d.floats.push(Float { stack: s, at: at.to_pos2(), size });
            }
        }
        for (name, side) in v["home"].as_object().into_iter().flatten() {
            if let (Some(p), Some(s)) = (Palette::named(name, known), Side::ALL.into_iter().find(|s| side.as_str() == Some(s.key()))) {
                d.home.insert(p, s);
            }
        }
        let closed: BTreeSet<Palette> = v["closed"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_str())
            .filter_map(|n| match Palette::named(n, known) {
                Some(p) => Some(p),
                None => {
                    if !n.is_empty()
                        && n.chars().count() <= Palette::NAME
                        && unknown.len() < Palette::MOST
                        && !unknown.iter().any(|q| q == n)
                    {
                        unknown.push(n.to_string());
                    }
                    None
                }
            })
            .collect();
        d.unknown = unknown;
        d.unknown_open = unknown_open;
        for p in known {
            // The window's own layout opens what the file never named, so a palette added since still shows; a preset keeps exactly
            // what it was saved with, and one added later stays closed.
            let opens = match policy {
                Policy::Live => p.opens_at_start() && *p != Palette::Assistant,
                Policy::Preset => false,
            };
            if !d.shown(*p) && !closed.contains(p) && opens {
                // First on the left: the Tools palette, which a file from before it holds nothing of, stands where it would have.
                d.put(*p, Target::Side(Side::Left));
                d.left.rotate_right(1);
            }
        }
        d
    }

    /// Forgets where the plugin palettes were docked: their registry is rebuilt and their indices move, so the side each
    /// remembered is no longer about the same palette.
    pub fn forget_plugin_palettes(&mut self) {
        self.home.retain(|p, _| !matches!(p, Palette::Plugin(_)));
    }

    /// Shows the palettes a file named while their plugin was off, now that `known` has them: those a file had open,
    /// where the plugin says. A name this build still hasn't is kept for the next read.
    pub fn adopt(&mut self, known: &[Palette]) {
        let names = std::mem::take(&mut self.unknown_open);
        for name in names {
            match Palette::named(&name, known) {
                Some(p) if !self.shown(p) => self.show(p, true),
                Some(_) => {}
                None => self.unknown_open.push(name),
            }
        }
        self.unknown.retain(|name| Palette::named(name, known).is_none());
    }
}

/// How the views' area is laid out: the standard tabbed view, equal columns and rows round the tabbed view (windows
/// snapped to the edges), or a grid of equal cells (Tile Views).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arrange {
    Tabs,
    Split,
    Tile,
}

/// The edge a floating view is snapped to while the workspace is split; none floats freely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    Left,
    Right,
    Bottom,
}

/// A view floating in a window of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatView {
    pub sheet: Sheet,
    pub at: Pos2,
    pub size: Vec2,
    /// The edge it is snapped to in `Arrange::Split`: none floats freely, over the columns and rows.
    pub edge: Option<Edge>,
}

/// What is being dragged: a palette (by its tab or its title), the window a palette floats in, a view's tab, or the
/// window a view floats in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    Palette(Palette),
    Window(Palette),
    View(Sheet),
    ViewWindow(Sheet),
}

/// Where a drag lands.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Land {
    Side(Side),
    With(Palette),
    /// The top edge of a docked palette's header (along the bottom, its left edge): a stack of its own before it.
    Before(Palette),
    /// The views' tab bar, before the tab at this place.
    Tabs(usize),
    /// An edge of the views' area: a view's column on the left or the right, or a row along the bottom.
    Snap(Edge),
}

impl BimApp {
    /// A stack as this frame draws it: in Rev the ribbon is the tools, so the Tools palette is left out of the drawing and takes no
    /// room, though it stays in the layout as the user's intent (`Docking::show`). None when nothing of it is left to draw.
    fn drawn_stack(&self, stack: &Stack) -> Option<Stack> {
        if !crate::style::rev() || !stack.tabs.contains(&Palette::Tools) {
            return Some(stack.clone());
        }
        let tabs: Vec<Palette> = stack.tabs.iter().copied().filter(|p| *p != Palette::Tools).collect();
        if tabs.is_empty() {
            return None;
        }
        let active = stack.shown().and_then(|p| tabs.iter().position(|q| *q == p)).unwrap_or(0);
        Some(Stack { tabs, active })
    }

    /// The docked palettes, round the views; call between the bars and the views.
    pub(crate) fn docks(&mut self, ui: &mut Ui) {
        self.drop_headers.clear();
        // The side holds the stacks' cards; between them and round them, the view's ground.
        let frame = Frame::NONE;
        for side in Side::ALL {
            let stacks: Vec<Stack> = self.docking.side(side).iter().filter_map(|s| self.drawn_stack(s)).collect();
            if stacks.is_empty() {
                continue;
            }
            // A side of the Tools palette alone is a rail and a column of names wide.
            let tools = stacks.len() == 1 && stacks[0].tabs == [Palette::Tools];
            let width = if tools { TOOLS_WIDTH } else { 290.0 };
            // A palette's content must never widen the side it docks on over most of the view: a plugin's table, or a
            // long label, would otherwise push the dock across the drawing. It is still resizable up to this cap.
            let most = (ui.available_width() * 0.5).max(320.0);
            let panel = match side {
                Side::Left => Panel::left("dock-left").default_size(width),
                // The right side's card meets the window's edge, where its scroll bar must clear the edge that resizes the window.
                Side::Right => Panel::right("dock-right").default_size(width + RIGHT_EXTRA),
                Side::Bottom => Panel::bottom("dock-bottom").default_size(240.0),
            };
            panel
                .frame(frame)
                .show_separator_line(false)
                .resizable(true)
                .min_size(140.0)
                .max_size(most)
                .show(ui, |ui| self.dock_side(ui, side, &stacks));
        }
    }

    /// The stacks docked on a side: one over the other, or side by side along the bottom, each but the last resizable.
    fn dock_side(&mut self, ui: &mut Ui, side: Side, stacks: &[Stack]) {
        let n = stacks.len();
        for (i, stack) in stacks.iter().enumerate() {
            // Each stack a card, standing off the window's edges and the next card: the last one off the bottom (or, along
            // the bottom, off the right) too.
            let last = i + 1 == n;
            let frame = match side {
                Side::Bottom => crate::style::card([true, last, true, true]),
                _ => crate::style::card([true, true, true, last]),
            };
            // Flush to the window's right edge (Compact), a card's scrollbar would lie in the zone that resizes the window.
            let frame = match crate::style::compact() && side == Side::Right && self.own_bar() {
                true => frame.inner_margin(egui::Margin { right: crate::titlebar::EDGE as i8, ..egui::Margin::ZERO }),
                false => frame,
            };
            if last {
                frame.show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.stack_ui(ui, stack, false);
                });
                continue;
            }
            // Named for the palette it holds, so a stack keeps its size while the others come and go.
            let id = UiId::new(("dock", side, stack.tabs[0]));
            // An even share of what's left, but of the last two the first takes a little more (Properties over the
            // Project Browser, as they start).
            let share = if n - i == 2 { 0.55 } else { 1.0 / (n - i) as f32 };
            // A card over others leaves them what they need, so the last never runs under the status bar when the window shrinks
            // under a card that was taller: the others' least, 80 each, and the last's header, tabs and search.
            let gap = f32::from(crate::style::gap());
            let most = ui.available_height() - (n - i - 2) as f32 * (80.0 + gap) - LAST_MIN - gap;
            let panel = match side {
                Side::Bottom => Panel::left(id).default_size(ui.available_width() * share),
                _ => Panel::top(id).default_size(ui.available_height() * share).max_size(most.max(80.0)),
            };
            panel.frame(frame).show_separator_line(false).resizable(true).min_size(80.0).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.stack_ui(ui, stack, false);
            });
        }
    }

    /// The palettes floating in windows of their own.
    pub(crate) fn palette_windows(&mut self, ctx: &Context) {
        for f in self.docking.floats.clone() {
            let Some(origin) = f.stack.tabs.first().copied() else { continue };
            // In Rev the Tools palette is left out of the drawing, but its window (with others in it) stays as it is in the layout.
            let Some(stack) = self.drawn_stack(&f.stack) else { continue };
            let Some(first) = stack.tabs.first().copied() else { continue };
            let frame = Frame::window(&ctx.global_style()).fill(pal().panel).inner_margin(0.0);
            let shown = egui::Window::new(t(first.name()))
                .id(UiId::new(("palette-window", first)))
                .title_bar(false)
                .resizable(true)
                .current_pos(f.at)
                .default_size(f.size)
                .min_size(vec2(180.0, 120.0))
                .frame(frame)
                .show(ctx, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.stack_ui(ui, &stack, true);
                });
            // Where egui put it (it keeps a window on screen, and a resize moves it), and its size, with what its title
            // was dragged by meanwhile.
            if let Some(r) = shown
                && let Some(now) = self.docking.float_of_mut(origin)
            {
                (now.at, now.size) = (r.response.rect.min + (now.at - f.at), r.response.rect.size());
            }
        }
    }

    /// A stack: its title (or its tabs), its Dock menu and its close button, then the palette it shows.
    fn stack_ui(&mut self, ui: &mut Ui, stack: &Stack, floating: bool) {
        let Some(shown) = stack.shown() else { return };
        let (rect, back) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click_and_drag());
        let p = ui.painter();
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, pal().border));
        self.drop_headers.push((shown, rect, floating));
        let lone = stack.tabs.len() == 1;
        if lone {
            let title = self.palette_title(shown);
            back.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &title));
            let at = rect.left_center() + vec2(12.0, 0.0);
            ui.painter().text(at, Align2::LEFT_CENTER, title, crate::style::semibold(13.0), pal().text);
        }
        let back = if let Palette::Plugin(i) = shown
            && let Some(panel) = plugin_palette(i)
        {
            back.on_hover_text(tf("Plugin panel: {}", &[panel.plugin_name]))
        } else {
            back
        };
        if back.drag_started() {
            self.dragging = Some(if floating { Drag::Window(shown) } else { Drag::Palette(shown) });
            let corner = self.docking.float_of(shown).map_or(rect.min, |f| f.at);
            self.grab = ui.input(|i| i.pointer.press_origin()).unwrap_or(corner) - corner;
        }
        if floating
            && back.dragged()
            && let Some(at) = ui.input(|i| i.pointer.latest_pos())
            && let Some(f) = self.docking.float_of_mut(shown)
        {
            f.at = at - self.grab;
        }
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 2.0))).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if !lone {
                for tab in &stack.tabs {
                    let r = palette_tab(ui, &tab.title(), *tab == shown);
                    if r.clicked() {
                        self.docking.activate(*tab);
                    }
                    if r.drag_started() {
                        self.dragging = Some(Drag::Palette(*tab));
                    }
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let name = shown.title();
                if icon_button(ui, Icon::Close, &tf("Close {}", &[&name])).clicked() {
                    self.docking.take(shown);
                }
                let r = icon_button(ui, Icon::Dock, &tf("Dock {}", &[&name]));
                Popup::menu(&r).show(|ui| self.dock_menu(ui, shown, rect));
            });
        });
        let body = Frame::NONE;
        body.show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            self.palette_body(ui, shown);
        });
    }

    /// The Dock menu: where else the palette can go.
    fn dock_menu(&mut self, ui: &mut Ui, p: Palette, header: Rect) {
        let docked = self.docking.docked(p);
        let alone = self.docking.stack_of(p).is_some_and(|s| s.tabs.len() == 1);
        for side in Side::ALL {
            // Docked alone on that side already, it has nowhere to go.
            let here = docked == Some(side) && alone;
            if ui.add_enabled(!here, egui::Button::new(t(side.label()))).clicked() {
                self.docking.put(p, Target::Side(side));
            }
        }
        let floating = self.docking.float_of(p).is_some_and(|f| f.stack.tabs.len() == 1);
        if ui.add_enabled(!floating, egui::Button::new(t("Float"))).clicked() {
            self.docking.put(p, Target::Float(header.left_top() + vec2(40.0, 40.0)));
        }
    }

    /// A palette's title: the Project Browser's names the project, as Revit's does.
    fn palette_title(&self, p: Palette) -> String {
        match p {
            Palette::Properties | Palette::Tools | Palette::Assistant | Palette::Scripts => t(p.label()).to_string(),
            Palette::Plugin(i) => {
                let title = plugin_palette(i).map_or_else(|| t("Plugin panel").to_string(), |panel| panel.title.to_string());
                let badge = self.plugin_panel_badge(i);
                if badge.is_empty() { title } else { format!("{title} · {badge}") }
            }
            Palette::Browser => match self.active_family() {
                Some(ed) => format!("{} · {}", t(p.label()), ed.title()),
                _ => format!("{} · {}", t(p.label()), self.session.model.name),
            },
        }
    }

    /// What a palette shows, under its title: as wide as where it's docked, its scroll bar at the edge.
    pub(crate) fn palette_body(&mut self, ui: &mut Ui, p: Palette) {
        // No wider than the palette: what is wider is cut, never pushes the card past its side (off the window, docked right).
        let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]).min_scrolled_height(0.0).max_width(ui.available_width());
        match p {
            Palette::Properties if self.family_doc() => {
                scroll.id_salt("family-properties").show(ui, |ui| Frame::new().inner_margin(8.0).show(ui, |ui| self.family_properties(ui)));
            }
            Palette::Properties => {
                scroll.id_salt("properties").show(ui, |ui| Frame::new().inner_margin(8.0).show(ui, |ui| self.properties(ui)));
            }
            Palette::Browser if self.family_doc() => self.family_browser(ui),
            Palette::Browser => self.browser_palette(ui),
            Palette::Tools => self.tool_column(ui),
            Palette::Assistant => self.assistant_palette(ui),
            Palette::Scripts => self.scripts_palette(ui),
            Palette::Plugin(i) => {
                scroll.id_salt(("plugin-panel", i)).show(ui, |ui| self.plugin_panel_body(ui, i));
            }
        }
    }

    /// Opens or closes a palette, as View > User Interface and PP do.
    pub(crate) fn show_palette(&mut self, p: Palette, on: bool) {
        self.docking.show(p, on);
    }

    // ---------- views ----------

    /// Takes a docked view off the tab bar into a window of its own, the active view; the last tab stays.
    pub(crate) fn float_view(&mut self, sheet: Sheet, at: Pos2) {
        if self.views.len() < 2 || !self.views.contains(&sheet) {
            return;
        }
        self.views.retain(|v| *v != sheet);
        self.floating.push(FloatView { sheet, at, size: VIEW_SIZE, edge: None });
        if self.front == sheet {
            self.front = self.views.last().copied().unwrap_or(sheet);
        }
        self.open_sheet(sheet);
    }

    /// Puts a floating view back on the tab bar, before the tab at `index`, the active view.
    pub(crate) fn dock_view(&mut self, sheet: Sheet, index: usize) {
        self.floating.retain(|f| f.sheet != sheet);
        self.views.retain(|v| *v != sheet);
        self.views.insert(index.min(self.views.len()), sheet);
        self.open_sheet(sheet);
        self.settle_arrange();
    }

    /// Whether a view floats.
    pub(crate) fn floats(&self, sheet: Sheet) -> bool {
        self.floating.iter().any(|f| f.sheet == sheet)
    }

    /// The views open in the project: the tabs that aren't family documents, and every window.
    pub(crate) fn project_views(&self) -> usize {
        self.views.iter().filter(|v| !matches!(v, Sheet::Family(..))).count() + self.floating.len()
    }

    /// Puts the layout back to the tabbed view where the one before it no longer makes sense: fewer than two project
    /// views, or no view snapped to an edge.
    pub(crate) fn settle_arrange(&mut self) {
        if self.project_views() < 2 || (self.arrange == Arrange::Split && !self.floating.iter().any(|f| f.edge.is_some())) {
            self.arrange = Arrange::Tabs;
            // The tabbed view knows no sides: a window floating over it is free, and the next snap counts them afresh.
            for f in &mut self.floating {
                f.edge = None;
            }
        }
    }

    /// Lays the views' area out for this frame: the tabbed pane and every window beside it, in `placements`.
    pub(crate) fn arrange_views(&mut self) {
        let r = self.views_rect;
        self.placements.clear();
        self.tab_pane = Rect::NOTHING;
        match self.arrange {
            Arrange::Tabs => self.tab_pane = r,
            Arrange::Split => {
                let left: Vec<Sheet> = self.floating.iter().filter(|f| f.edge == Some(Edge::Left)).map(|f| f.sheet).collect();
                let right: Vec<Sheet> = self.floating.iter().filter(|f| f.edge == Some(Edge::Right)).map(|f| f.sheet).collect();
                let bottom: Vec<Sheet> = self.floating.iter().filter(|f| f.edge == Some(Edge::Bottom)).map(|f| f.sheet).collect();
                let cols = left.len() + right.len() + 1;
                let rows = bottom.len() + 1;
                let widths = shares(r.width(), cols);
                let heights = shares(r.height(), rows);
                let top = heights[0];
                let mut x = r.left();
                for (i, sheet) in left.iter().enumerate() {
                    self.placements.push((*sheet, Rect::from_min_size(pos2(x, r.top()), vec2(widths[i], top))));
                    x += widths[i];
                }
                let tab = left.len();
                let tab_width = if right.is_empty() { r.right() - x } else { widths[tab] };
                self.tab_pane = Rect::from_min_size(pos2(x, r.top()), vec2(tab_width, top));
                x += tab_width;
                for (i, sheet) in right.iter().enumerate() {
                    let w = if i + 1 == right.len() { r.right() - x } else { widths[tab + 1 + i] };
                    self.placements.push((*sheet, Rect::from_min_size(pos2(x, r.top()), vec2(w, top))));
                    x += w;
                }
                let mut y = r.top() + top;
                for (i, sheet) in bottom.iter().enumerate() {
                    let h = if i + 2 == rows { r.bottom() - y } else { heights[i + 1] };
                    self.placements.push((*sheet, Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), h))));
                    y += h;
                }
            }
            Arrange::Tile => {
                let sheets: Vec<Sheet> = self.views.iter().filter(|s| !matches!(s, Sheet::Family(..))).copied().collect();
                let n = sheets.len();
                if n == 0 {
                    return;
                }
                let cols = (n as f32).sqrt().ceil() as usize;
                let rows = n.div_ceil(cols);
                let heights = shares(r.height(), rows);
                let mut y = r.top();
                for (row, &share_h) in heights.iter().enumerate() {
                    let h = if row + 1 == rows { r.bottom() - y } else { share_h };
                    // A row of fewer cells than the grid is wide shares the whole width between them, so a tile has no holes:
                    // three views make two cells over one, and the one below takes the room of the two it would have left empty.
                    let start = row * cols;
                    let count = (n - start).min(cols);
                    if count == 0 {
                        break;
                    }
                    let widths = shares(r.width(), count);
                    let mut x = r.left();
                    for (i, &share_w) in widths.iter().enumerate() {
                        let Some(sheet) = sheets.get(start + i) else { break };
                        let w = if i + 1 == count { r.right() - x } else { share_w };
                        self.placements.push((*sheet, Rect::from_min_size(pos2(x, y), vec2(w, h))));
                        x += w;
                    }
                    y += h;
                }
            }
        }
    }

    /// The rectangle the active view takes this frame: the pane its tab shows, or the column, row or cell it stands in.
    pub(crate) fn active_rect(&self) -> Rect {
        if let Some((_, r)) = self.placements.iter().find(|(s, _)| *s == self.sheet) {
            return *r;
        }
        if let Some(f) = self.floating.iter().find(|f| f.sheet == self.sheet) {
            return Rect::from_min_size(f.at, f.size);
        }
        self.tab_pane
    }

    /// Snaps a floating view to an edge, entering Split: on the first snap the other free windows take a side by where
    /// they stand, so two windows make three parts along the edge that was used.
    pub(crate) fn snap_view(&mut self, sheet: Sheet, edge: Edge) {
        if !self.floats(sheet) {
            return;
        }
        if self.arrange != Arrange::Split {
            let mid = self.views_rect.center().x;
            for f in &mut self.floating {
                if f.sheet != sheet && f.edge.is_none() {
                    f.edge = Some(match edge {
                        Edge::Bottom => Edge::Bottom,
                        Edge::Left | Edge::Right => {
                            if f.at.x + f.size.x / 2.0 < mid {
                                Edge::Left
                            } else {
                                Edge::Right
                            }
                        }
                    });
                }
            }
        }
        if let Some(i) = self.floating.iter().position(|f| f.sheet == sheet) {
            let mut f = self.floating.remove(i);
            f.edge = Some(edge);
            match edge {
                Edge::Left => self.floating.insert(0, f),
                Edge::Right | Edge::Bottom => self.floating.push(f),
            }
        }
        self.arrange = Arrange::Split;
    }

    /// Lets a snapped view float again where it is dropped, at the size it had before it was snapped.
    pub(crate) fn unsnap_view(&mut self, sheet: Sheet, at: Pos2) {
        if let Some(f) = self.floating.iter_mut().find(|f| f.sheet == sheet)
            && f.edge.take().is_some()
        {
            f.at = at;
        }
        self.settle_arrange();
    }

    /// Tile Views: every floating window back on the bar, quietly, then every project view in an equal cell.
    pub(crate) fn tile_views(&mut self) {
        for f in self.floating.clone() {
            if !self.views.contains(&f.sheet) {
                self.views.push(f.sheet);
            }
        }
        self.floating.clear();
        self.front = if self.views.contains(&self.sheet) { self.sheet } else { self.views.last().copied().unwrap_or(self.sheet) };
        self.arrange = if self.project_views() >= 2 { Arrange::Tile } else { Arrange::Tabs };
    }

    /// Dock All Views: every window back on the tab bar in the order it stands on screen, the tabbed view again.
    pub(crate) fn dock_all_views(&mut self) {
        let mut order: Vec<Sheet> = Vec::new();
        match self.arrange {
            Arrange::Tabs => order.extend(self.floating.iter().map(|f| f.sheet)),
            Arrange::Split => {
                for edge in [Edge::Left, Edge::Right, Edge::Bottom] {
                    order.extend(self.floating.iter().filter(|f| f.edge == Some(edge)).map(|f| f.sheet));
                }
                order.extend(self.floating.iter().filter(|f| f.edge.is_none()).map(|f| f.sheet));
            }
            Arrange::Tile => {
                // The cells row by row; what floats free over them last.
                order.extend(self.placements.iter().map(|(s, _)| *s));
                order.extend(self.floating.iter().map(|f| f.sheet));
            }
        }
        let mut moved: Vec<Sheet> = Vec::new();
        for sheet in order {
            if moved.contains(&sheet) {
                continue;
            }
            moved.push(sheet);
            self.floating.retain(|f| f.sheet != sheet);
            if !self.views.contains(&sheet) {
                self.views.push(sheet);
            }
        }
        self.floating.clear();
        self.arrange = Arrange::Tabs;
        self.front = if self.views.contains(&self.sheet) { self.sheet } else { self.views.last().copied().unwrap_or(self.sheet) };
    }

    /// The views floating in windows of their own: each its title, Dock and Close, and the view; or, in Split and Tile,
    /// the windows `placements` lays out.
    pub(crate) fn view_windows(&mut self, ctx: &Context) {
        if self.family_doc() {
            return;
        }
        match self.arrange {
            Arrange::Tabs => {
                for f in self.floating.clone() {
                    self.free_view_window(ctx, f);
                }
            }
            Arrange::Split => {
                // The tabbed pane's header is the tabs themselves, so every part of the split wears one header.
                let (pane, front) = (self.tab_pane, self.front);
                self.tabs_view_window(ctx, pane, front);
                for (sheet, rect) in self.placements.clone() {
                    self.placed_view_window(ctx, sheet, rect, true);
                }
                for f in self.floating.clone().into_iter().filter(|f| f.edge.is_none()) {
                    self.free_view_window(ctx, f);
                }
            }
            Arrange::Tile => {
                // A tile names every view in its own header: no tab bar above them to say it twice.
                self.tab_rects.clear();
                self.tab_bar = Rect::NOTHING;
                for (sheet, rect) in self.placements.clone() {
                    self.placed_view_window(ctx, sheet, rect, false);
                }
                for f in self.floating.clone().into_iter().filter(|f| f.edge.is_none()) {
                    self.free_view_window(ctx, f);
                }
            }
        }
    }

    /// The split's tabbed pane: a window whose 26 pt header is the views' tabs, and whose body is the view they show.
    fn tabs_view_window(&mut self, ctx: &Context, rect: Rect, front: Sheet) {
        let frame = Frame::window(&ctx.global_style()).fill(pal().surface).inner_margin(0.0);
        let name = self.view_name(front);
        let _ = egui::Window::new(&name)
            .id(UiId::new("view-tabs-window"))
            .title_bar(false)
            .resizable(false)
            .constrain(false)
            .fixed_rect(rect)
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                // The header, as the windows beside it wear: the tabs stand where their titles do.
                let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(head, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, pal().bar);
                p.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, pal().border));
                ui.scope_builder(UiBuilder::new().max_rect(head.shrink2(vec2(2.0, 1.0))), |ui| self.view_tabs(ui));
                self.view_pane(ui, front);
            });
    }

    /// A view floating freely, its corner and size its own: it follows the pointer by its title, and resizes by its edges.
    fn free_view_window(&mut self, ctx: &Context, f: FloatView) {
        let frame = Frame::window(&ctx.global_style()).fill(pal().surface).inner_margin(0.0);
        let name = self.view_name(f.sheet);
        let shown = egui::Window::new(&name)
            .id(UiId::new(("view-window", f.sheet)))
            .title_bar(false)
            .resizable(true)
            .current_pos(f.at)
            .default_size(f.size)
            .min_size(vec2(240.0, 160.0))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                self.view_window_title(ui, f.sheet, f.at, true, true);
                self.view_pane(ui, f.sheet);
            });
        if let Some(r) = shown
            && let Some(now) = self.floating.iter_mut().find(|w| w.sheet == f.sheet)
        {
            (now.at, now.size) = (r.response.rect.min + (now.at - f.at), r.response.rect.size());
        }
    }

    /// A view in its column, row or cell: its size the layout's, so it doesn't resize; snapped, its title still drags it.
    fn placed_view_window(&mut self, ctx: &Context, sheet: Sheet, rect: Rect, dockable: bool) {
        let frame = Frame::window(&ctx.global_style()).fill(pal().surface).inner_margin(0.0);
        let name = self.view_name(sheet);
        let _ = egui::Window::new(&name)
            .id(UiId::new(("view-window", sheet)))
            .title_bar(false)
            .resizable(false)
            .constrain(false)
            .fixed_rect(rect)
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                self.view_window_title(ui, sheet, rect.min, dockable, dockable);
                self.view_pane(ui, sheet);
            });
    }

    /// The 26 pt strip at a view window's top: its name, Close and (where it may float free) Dock; dragging it moves the
    /// window, and lets go of it onto a zone.
    fn view_window_title(&mut self, ui: &mut Ui, sheet: Sheet, origin: Pos2, dock: bool, drag: bool) {
        let sense = if drag { Sense::click_and_drag() } else { Sense::click() };
        let (rect, back) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), sense);
        let title = self.view_title(sheet);
        back.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &title));
        let active = sheet == self.sheet;
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, if active { pal().select_fill } else { pal().bar });
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, pal().border));
        let ink = if active { pal().accent_ink } else { pal().text };
        p.text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, &title, FontId::proportional(13.0), ink);
        if drag && back.drag_started() {
            self.dragging = Some(Drag::ViewWindow(sheet));
            self.grab = ui.input(|i| i.pointer.press_origin()).unwrap_or(origin) - origin;
        }
        // A free window's corner follows the pointer; a snapped one stays in its column until it is dropped.
        if drag
            && back.dragged()
            && let Some(at) = ui.input(|i| i.pointer.latest_pos())
            && let Some(w) = self.floating.iter_mut().find(|w| w.sheet == sheet)
            && w.edge.is_none()
        {
            w.at = at - self.grab;
        }
        if back.clicked() {
            self.open_sheet(sheet);
        }
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 2.0))).layout(Layout::right_to_left(Align::Center)), |ui| {
            let name = self.view_name(sheet);
            if icon_button(ui, Icon::Close, &tf("Close {}", &[&name])).clicked() {
                self.close_view(sheet);
            }
            if dock && icon_button(ui, Icon::Dock, &tf("Dock {}", &[&name])).clicked() {
                self.dock_view(sheet, usize::MAX);
            }
        });
    }

    /// A view and its view control bar: the active view as it takes tools, any other as it is, a click on it making it
    /// the active one, as in Revit.
    pub(crate) fn view_pane(&mut self, ui: &mut Ui, sheet: Sheet) {
        ui.push_id(("view", sheet), |ui| {
            if sheet == self.sheet {
                self.view_body(ui);
                return;
            }
            let rect = ui.available_rect_before_wrap();
            // It draws with nothing of what the hand is doing in the active view, takes no clicks, and what drawing it
            // leaves behind (a camera fitted, the cursor) is put back as the active view had it.
            let rest = self.canvas.at_rest();
            let (canvas, view_rect) = (std::mem::replace(&mut self.canvas, rest), self.view_rect);
            let active = std::mem::replace(&mut self.sheet, sheet);
            self.passive = true;
            ui.scope(|ui| {
                ui.disable();
                ui.set_opacity(1.0);
                self.view_body(ui);
            });
            self.passive = false;
            (self.sheet, self.canvas, self.view_rect) = (active, canvas, view_rect);
            if ui.interact(rect, ui.id().with("activate"), Sense::click()).clicked() {
                self.open_sheet(sheet);
            }
        });
    }

    // ---------- dragging ----------

    /// Where a drag may land, each with its rectangle and what it says: a palette docks by a strip along an edge of the
    /// docked view's drawing (never over the bars round it), or on another stack's title.
    fn landings(&self, drag: Drag) -> Vec<(Land, Rect, String)> {
        let w = self.work.shrink(INSET);
        match drag {
            Drag::Palette(p) | Drag::Window(p) => {
                let mut out = vec![
                    (
                        Land::Side(Side::Left),
                        Rect::from_min_max(w.left_top(), pos2(w.left() + EDGE, w.bottom())),
                        t("Dock Left").to_string(),
                    ),
                    (
                        Land::Side(Side::Right),
                        Rect::from_min_max(pos2(w.right() - EDGE, w.top()), w.right_bottom()),
                        t("Dock Right").to_string(),
                    ),
                    (
                        Land::Side(Side::Bottom),
                        Rect::from_min_max(pos2(w.left() + EDGE + INSET, w.bottom() - EDGE), pos2(w.right() - EDGE - INSET, w.bottom())),
                        t("Dock Bottom").to_string(),
                    ),
                ];
                let own = self.docking.stack_of(p).cloned();
                for (q, rect, floating) in &self.drop_headers {
                    // Not its own stack: a tab dropped there stays where it was, and so does a window.
                    let mine = own.as_ref().is_some_and(|s| s.tabs.contains(q));
                    if !mine {
                        out.insert(0, (Land::With(*q), *rect, tf("Tab with {}", &[t(q.name())])));
                        // A thin strip along the header's leading edge puts it there as a stack of its own; it comes ahead of the
                        // header's own zone, which makes a tab.
                        let before = match self.docking.docked(*q) {
                            Some(Side::Bottom) => Some((
                                Rect::from_min_max(rect.left_top() - vec2(8.0, 0.0), pos2(rect.left() + 6.0, rect.bottom())),
                                msg!("Left of {}"),
                            )),
                            Some(_) => Some((
                                Rect::from_min_max(rect.left_top() - vec2(0.0, 8.0), pos2(rect.right(), rect.top() + 6.0)),
                                msg!("Above {}"),
                            )),
                            None => None,
                        };
                        if let Some((edge, label)) = before.filter(|_| !*floating) {
                            out.insert(0, (Land::Before(*q), edge, tf(label, &[t(q.name())])));
                        }
                    }
                }
                out
            }
            Drag::View(_) | Drag::ViewWindow(_) => {
                let v = self.views_rect;
                let sides = v.bottom() - EDGE;
                vec![
                    (Land::Tabs(self.tab_index(self.pointer)), self.tab_bar, t("Dock in the Views").to_string()),
                    (Land::Snap(Edge::Left), Rect::from_min_max(v.left_top(), pos2(v.left() + EDGE, sides)), t("Snap Left").to_string()),
                    (
                        Land::Snap(Edge::Right),
                        Rect::from_min_max(pos2(v.right() - EDGE, v.top()), pos2(v.right(), sides)),
                        t("Snap Right").to_string(),
                    ),
                    (
                        Land::Snap(Edge::Bottom),
                        Rect::from_min_max(pos2(v.left(), v.bottom() - EDGE), v.right_bottom()),
                        t("Snap Bottom").to_string(),
                    ),
                ]
            }
        }
    }

    /// The place on the tab bar a drop at `at` takes: before the first tab whose middle is right of it.
    fn tab_index(&self, at: Pos2) -> usize {
        self.tab_rects.iter().filter(|(_, r)| r.center().x < at.x).count()
    }

    /// While something is dragged, the drop zones, and the drop: call once a frame, after everything is drawn.
    pub(crate) fn drag_and_drop(&mut self, ctx: &Context) {
        let Some(drag) = self.dragging else { return };
        if let Some(at) = ctx.pointer_latest_pos() {
            self.pointer = at;
        }
        let at = self.pointer;
        let landings = self.landings(drag);
        let hit = landings.iter().find(|(land, r, _)| reach(*land, *r).contains(at)).map(|(l, _, _)| *l);
        let p = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, UiId::new("dock-drop")));
        let accent = pal().accent;
        for (land, r, label) in &landings {
            let on = hit == Some(*land);
            p.rect(
                *r,
                4.0,
                accent.gamma_multiply(if on { 0.35 } else { 0.12 }),
                Stroke::new(if on { 2.0 } else { 1.0 }, accent),
                StrokeKind::Inside,
            );
            // Down a side the words run along it, reading up the left and down the right.
            let angle = match land {
                Land::Side(Side::Left) | Land::Snap(Edge::Left) => -FRAC_PI_2,
                Land::Side(Side::Right) | Land::Snap(Edge::Right) => FRAC_PI_2,
                _ => 0.0,
            };
            if on || !matches!(land, Land::With(_) | Land::Before(_)) {
                zone_label(&p, *r, label, angle, on);
            }
        }
        // Where it would float: dropped on none of them, a palette or a view's tab comes off there.
        if hit.is_none() && matches!(drag, Drag::Palette(_) | Drag::View(_)) {
            let size = if matches!(drag, Drag::View(_)) { VIEW_SIZE } else { FLOAT_SIZE };
            let ghost = Rect::from_min_size(at - vec2(40.0, 13.0), size * 0.5);
            p.rect(ghost, 4.0, accent.gamma_multiply(0.1), Stroke::new(1.0, accent), StrokeKind::Inside);
            zone_label(&p, ghost, t("Float"), 0.0, true);
        }
        if ctx.input(|i| i.pointer.primary_down()) {
            return;
        }
        self.dragging = None;
        let corner = at - vec2(40.0, 13.0);
        match (drag, hit) {
            (Drag::Palette(p), Some(Land::Side(s))) => self.docking.put(p, Target::Side(s)),
            (Drag::Palette(p), Some(Land::With(q))) => self.docking.put(p, Target::With(q)),
            (Drag::Palette(p), Some(Land::Before(q))) => self.docking.put(p, Target::Before(q)),
            (Drag::Palette(p), _) => self.docking.put(p, Target::Float(corner)),
            (Drag::Window(p), Some(Land::Side(s))) => self.docking.put_window(p, Target::Side(s)),
            (Drag::Window(p), Some(Land::With(q))) => self.docking.put_window(p, Target::With(q)),
            (Drag::Window(p), Some(Land::Before(q))) => self.docking.put_window(p, Target::Before(q)),
            (Drag::View(s), Some(Land::Tabs(i))) => {
                // Dropped back on the bar, it moves along it.
                let from = self.views.iter().position(|v| *v == s).unwrap_or(i);
                self.views.retain(|v| *v != s);
                self.views.insert(if i > from { i - 1 } else { i }.min(self.views.len()), s);
            }
            (Drag::View(s), Some(Land::Snap(e))) => {
                self.float_view(s, corner);
                self.snap_view(s, e);
            }
            (Drag::View(s), _) => self.float_view(s, corner),
            (Drag::ViewWindow(s), Some(Land::Tabs(i))) => self.dock_view(s, i),
            (Drag::ViewWindow(s), Some(Land::Snap(e))) => self.snap_view(s, e),
            (Drag::ViewWindow(s), None) => self.unsnap_view(s, corner),
            _ => {}
        }
    }
}

/// Where a drop lands on a zone: a strip along an edge takes it out to the edge and past it too, where the hand that
/// throws a palette at the side of the window ends up.
fn reach(land: Land, r: Rect) -> Rect {
    match land {
        Land::Side(Side::Left) | Land::Snap(Edge::Left) => r.with_min_x(f32::NEG_INFINITY),
        Land::Side(Side::Right) | Land::Snap(Edge::Right) => r.with_max_x(f32::INFINITY),
        Land::Side(Side::Bottom) | Land::Snap(Edge::Bottom) => r.with_max_y(f32::INFINITY),
        _ => r,
    }
}

/// `total` split into `parts` equal shares; the last takes the rounding, so they add up to `total`.
fn shares(total: f32, parts: usize) -> Vec<f32> {
    let n = parts.max(1);
    let mut out = vec![total / n as f32; n];
    let used: f32 = out.iter().sum();
    if let Some(last) = out.last_mut() {
        *last += total - used;
    }
    out
}

/// What a drop zone says, written over it (turned by `angle`, clockwise) and kept inside it.
fn zone_label(p: &egui::Painter, r: Rect, label: &str, angle: f32, on: bool) {
    let ink = if on { pal().text } else { pal().muted };
    let galley = p.layout_no_wrap(label.to_string(), FontId::proportional(if on { 14.0 } else { 13.0 }), ink);
    let corner = r.center() - galley.size() / 2.0;
    let text = egui::epaint::TextShape::new(corner, galley, ink).with_angle_and_anchor(angle, Align2::CENTER_CENTER);
    p.with_clip_rect(r).add(text);
}

/// A palette's tab, in a stack of several: click shows it, a drag takes it elsewhere.
fn palette_tab(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let font = FontId::proportional(12.5);
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size();
    let (rect, r) = ui.allocate_exact_size(vec2(size.x + 18.0, 22.0), Sense::click_and_drag());
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    let fill = match (active, r.hovered()) {
        (true, _) => pal().panel,
        (false, true) => pal().hover_fill,
        (false, false) => egui::Color32::TRANSPARENT,
    };
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 },
        fill,
        if active { Stroke::new(1.0, pal().border) } else { Stroke::NONE },
        StrokeKind::Inside,
    );
    if r.has_focus() {
        p.rect_stroke(rect, 4.0, crate::style::focus_ring(), StrokeKind::Inside);
    }
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, if active { pal().text } else { pal().muted });
    r
}
