//! Palettes that dock and float, and views that float, as Revit's do. A palette (Properties, the Project Browser and
//! any to come: a row of [`Palette`]) docks down the window's left or right side or along its bottom, in a stack of
//! its own (stacks on a side share it, one over the other) or as a tab of another's, or floats in a window of its
//! own. Its title, or its tab, drags it: drop zones show where it can land, and a drop anywhere else floats it there.
//! Its Dock menu does the same by name. A view's tab drags off the views' tab bar into a window (or its menu's Float),
//! and that window's title drags it back onto the bar. [`Docking`] is the palettes' layout, apart from how it's drawn,
//! so it can be tested and kept in the settings file; the view that takes the tools is the active one
//! ([`BimApp::sheet`]): any other on screen draws as it is, and a click on it makes it the active one.

use bimcraft_i18n::msg;

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
}

impl Palette {
    pub const ALL: [Palette; 2] = [Palette::Properties, Palette::Browser];

    /// Its name: what its tab says (in the interface's language) and what the settings file keeps it as.
    pub fn name(self) -> &'static str {
        match self {
            Palette::Properties => "Properties",
            Palette::Browser => msg!("Project Browser"),
        }
    }

    fn named(name: &str) -> Option<Palette> {
        Palette::ALL.into_iter().find(|p| p.name() == name)
    }
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
/// another palette, or floating with its corner at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Side(Side),
    With(Palette),
    Float(Pos2),
}

/// A palette's window, when it starts floating.
pub const FLOAT_SIZE: Vec2 = vec2(300.0, 420.0);
/// A view's window, when it starts floating.
pub const VIEW_SIZE: Vec2 = vec2(640.0, 440.0);
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
}

impl Default for Docking {
    /// Revit's: Properties over the Project Browser, down the left side.
    fn default() -> Docking {
        Docking {
            left: vec![Stack::of(Palette::Properties), Stack::of(Palette::Browser)],
            right: Vec::new(),
            bottom: Vec::new(),
            floats: Vec::new(),
            home: BTreeMap::new(),
        }
    }
}

impl Docking {
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

    /// Puts `p` where `to` says, out of where it was. A tab dropped on its own stack, or on a palette that isn't
    /// open, stays where it was.
    pub fn put(&mut self, p: Palette, to: Target) {
        if let Target::With(q) = to
            && (q == p || !self.shown(q))
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
            Target::Float(at) => self.floats.push(Float { stack: Stack::of(p), at, size: FLOAT_SIZE }),
        }
    }

    /// Docks the whole window `p` floats in: a stack of its own on a side, or its tabs after another stack's. Dropped
    /// anywhere else, it stays a window (dragging it moved it already).
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
                let side = self.home.get(&p).copied().unwrap_or(Side::Left);
                self.put(p, Target::Side(side));
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

    /// As the settings file keeps it.
    pub fn to_json(&self) -> Value {
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
        v["closed"] = Palette::ALL.into_iter().filter(|p| !self.shown(*p)).map(Palette::name).collect();
        v["home"] = self.home.iter().map(|(p, s)| (p.name().to_string(), json!(s.key()))).collect::<serde_json::Map<_, _>>().into();
        v
    }

    /// Reads back what [`to_json`](Self::to_json) wrote, as hostile as any file: a palette it doesn't know or names
    /// twice is left out, numbers are finite and in range, and a palette it neither places nor closes (one added since)
    /// docks on the left. Anything but an object is the default layout.
    pub fn from_json(v: &Value) -> Docking {
        if !v.is_object() {
            return Docking::default();
        }
        let mut seen = BTreeSet::new();
        let mut stack = |v: &Value| -> Option<Stack> {
            let tabs: Vec<Palette> =
                v["tabs"].as_array()?.iter().filter_map(|n| Palette::named(n.as_str()?)).filter(|p| seen.insert(*p)).collect();
            let active = v["active"].as_u64().and_then(|a| usize::try_from(a).ok()).filter(|a| *a < tabs.len()).unwrap_or(0);
            (!tabs.is_empty()).then_some(Stack { tabs, active })
        };
        let mut d = Docking { left: Vec::new(), right: Vec::new(), bottom: Vec::new(), floats: Vec::new(), home: BTreeMap::new() };
        for side in Side::ALL {
            let stacks: Vec<Stack> = v[side.key()].as_array().map(|a| a.iter().filter_map(&mut stack).collect()).unwrap_or_default();
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
            if let Some(stack) = stack(f) {
                d.floats.push(Float { stack, at: at.to_pos2(), size });
            }
        }
        for (name, side) in v["home"].as_object().into_iter().flatten() {
            if let (Some(p), Some(s)) = (Palette::named(name), Side::ALL.into_iter().find(|s| side.as_str() == Some(s.key()))) {
                d.home.insert(p, s);
            }
        }
        let closed: BTreeSet<Palette> = v["closed"].as_array().into_iter().flatten().filter_map(|n| Palette::named(n.as_str()?)).collect();
        for p in Palette::ALL {
            if !d.shown(p) && !closed.contains(&p) {
                d.put(p, Target::Side(Side::Left));
            }
        }
        d
    }
}

/// A view floating in a window of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatView {
    pub sheet: Sheet,
    pub at: Pos2,
    pub size: Vec2,
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
    /// The views' tab bar, before the tab at this place.
    Tabs(usize),
}

impl BimApp {
    /// The docked palettes, round the views; call between the bars and the views.
    pub(crate) fn docks(&mut self, ui: &mut Ui) {
        self.drop_headers.clear();
        let frame = Frame::new().fill(pal().panel).stroke(Stroke::new(1.0, pal().border));
        for side in Side::ALL {
            if self.docking.side(side).is_empty() {
                continue;
            }
            let panel = match side {
                Side::Left => Panel::left("dock-left").default_size(290.0),
                Side::Right => Panel::right("dock-right").default_size(290.0),
                Side::Bottom => Panel::bottom("dock-bottom").default_size(240.0),
            };
            panel.frame(frame).resizable(true).min_size(140.0).show(ui, |ui| self.dock_side(ui, side));
        }
    }

    /// The stacks docked on a side: one over the other, or side by side along the bottom, each but the last resizable.
    fn dock_side(&mut self, ui: &mut Ui, side: Side) {
        let stacks = self.docking.side(side).to_vec();
        let n = stacks.len();
        let frame = Frame::new().fill(pal().panel).stroke(Stroke::new(1.0, pal().border));
        for (i, stack) in stacks.iter().enumerate() {
            if i + 1 == n {
                self.stack_ui(ui, stack, false);
                continue;
            }
            // Named for the palette it holds, so a stack keeps its size while the others come and go.
            let id = UiId::new(("dock", side, stack.tabs[0]));
            // An even share of what's left, but of the last two the first takes a little more (Properties over the
            // Project Browser, as they start).
            let share = if n - i == 2 { 0.55 } else { 1.0 / (n - i) as f32 };
            let panel = match side {
                Side::Bottom => Panel::left(id).default_size(ui.available_width() * share),
                _ => Panel::top(id).default_size(ui.available_height() * share),
            };
            panel.frame(frame).resizable(true).min_size(80.0).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.stack_ui(ui, stack, false);
            });
        }
    }

    /// The palettes floating in windows of their own.
    pub(crate) fn palette_windows(&mut self, ctx: &Context) {
        for f in self.docking.floats.clone() {
            let Some(first) = f.stack.tabs.first().copied() else { continue };
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
                    self.stack_ui(ui, &f.stack, true);
                });
            // Where egui put it (it keeps a window on screen, and a resize moves it), and its size, with what its title
            // was dragged by meanwhile.
            if let Some(r) = shown
                && let Some(now) = self.docking.float_of_mut(first)
            {
                (now.at, now.size) = (r.response.rect.min + (now.at - f.at), r.response.rect.size());
            }
        }
    }

    /// A stack: its title (or its tabs), its Dock menu and its close button, then the palette it shows.
    fn stack_ui(&mut self, ui: &mut Ui, stack: &Stack, floating: bool) {
        let Some(shown) = stack.shown() else { return };
        let (rect, back) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click_and_drag());
        let p = ui.painter();
        p.rect_filled(rect, 0.0, pal().bar);
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, pal().border));
        self.drop_headers.push((shown, rect, floating));
        let lone = stack.tabs.len() == 1;
        if lone {
            let title = self.palette_title(shown);
            back.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &title));
            let at = rect.left_center() + vec2(10.0, 0.0);
            ui.painter().text(at, Align2::LEFT_CENTER, title, FontId::proportional(13.0), pal().text);
        }
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
                    let r = palette_tab(ui, t(tab.name()), *tab == shown);
                    if r.clicked() {
                        self.docking.activate(*tab);
                    }
                    if r.drag_started() {
                        self.dragging = Some(Drag::Palette(*tab));
                    }
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let name = t(shown.name());
                if icon_button(ui, Icon::Close, &tf("Close {}", &[name])).clicked() {
                    self.docking.take(shown);
                }
                let r = icon_button(ui, Icon::Dock, &tf("Dock {}", &[name]));
                Popup::menu(&r).show(|ui| self.dock_menu(ui, shown, rect));
            });
        });
        let body = Frame::new().fill(pal().panel);
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
            Palette::Properties => t(p.name()).to_string(),
            Palette::Browser => tf("Project Browser · {}", &[&self.session.model.name]),
        }
    }

    /// What a palette shows, under its title: as wide as where it's docked, its scroll bar at the edge.
    pub(crate) fn palette_body(&mut self, ui: &mut Ui, p: Palette) {
        let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
        match p {
            Palette::Properties => {
                scroll.id_salt("properties").show(ui, |ui| Frame::new().inner_margin(8.0).show(ui, |ui| self.properties(ui)));
            }
            Palette::Browser => self.browser_palette(ui),
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
        self.floating.push(FloatView { sheet, at, size: VIEW_SIZE });
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
    }

    /// Whether a view floats.
    pub(crate) fn floats(&self, sheet: Sheet) -> bool {
        self.floating.iter().any(|f| f.sheet == sheet)
    }

    /// The views floating in windows of their own: each its title, Dock and Close, and the view.
    pub(crate) fn view_windows(&mut self, ctx: &Context) {
        for f in self.floating.clone() {
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
                    let (rect, back) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click_and_drag());
                    let title = self.view_title(f.sheet);
                    back.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &title));
                    let active = f.sheet == self.sheet;
                    let p = ui.painter();
                    p.rect_filled(rect, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, if active { pal().select_fill } else { pal().bar });
                    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, pal().border));
                    let ink = if active { pal().accent } else { pal().text };
                    p.text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, &title, FontId::proportional(13.0), ink);
                    if back.drag_started() {
                        self.dragging = Some(Drag::ViewWindow(f.sheet));
                        self.grab = ui.input(|i| i.pointer.press_origin()).unwrap_or(f.at) - f.at;
                    }
                    // The window's corner keeps where the title was taken from under the pointer.
                    if back.dragged()
                        && let Some(at) = ui.input(|i| i.pointer.latest_pos())
                        && let Some(w) = self.floating.iter_mut().find(|w| w.sheet == f.sheet)
                    {
                        w.at = at - self.grab;
                    }
                    if back.clicked() {
                        self.open_sheet(f.sheet);
                    }
                    ui.scope_builder(
                        UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 2.0))).layout(Layout::right_to_left(Align::Center)),
                        |ui| {
                            if icon_button(ui, Icon::Close, &tf("Close {}", &[&name])).clicked() {
                                self.close_view(f.sheet);
                            }
                            if icon_button(ui, Icon::Dock, &tf("Dock {}", &[&name])).clicked() {
                                self.dock_view(f.sheet, usize::MAX);
                            }
                        },
                    );
                    self.view_pane(ui, f.sheet);
                });
            if let Some(r) = shown
                && let Some(now) = self.floating.iter_mut().find(|w| w.sheet == f.sheet)
            {
                (now.at, now.size) = (r.response.rect.min + (now.at - f.at), r.response.rect.size());
            }
        }
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
                for (q, rect, _) in &self.drop_headers {
                    // Not its own stack: a tab dropped there stays where it was, and so does a window.
                    let mine = own.as_ref().is_some_and(|s| s.tabs.contains(q));
                    if !mine {
                        out.insert(0, (Land::With(*q), *rect, tf("Tab with {}", &[t(q.name())])));
                    }
                }
                out
            }
            Drag::View(_) | Drag::ViewWindow(_) => {
                vec![(Land::Tabs(self.tab_index(self.pointer)), self.tab_bar, t("Dock in the Views").to_string())]
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
                Land::Side(Side::Left) => -FRAC_PI_2,
                Land::Side(Side::Right) => FRAC_PI_2,
                _ => 0.0,
            };
            if on || !matches!(land, Land::With(_)) {
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
            (Drag::Palette(p), _) => self.docking.put(p, Target::Float(corner)),
            (Drag::Window(p), Some(Land::Side(s))) => self.docking.put_window(p, Target::Side(s)),
            (Drag::Window(p), Some(Land::With(q))) => self.docking.put_window(p, Target::With(q)),
            (Drag::View(s), Some(Land::Tabs(i))) => {
                // Dropped back on the bar, it moves along it.
                let from = self.views.iter().position(|v| *v == s).unwrap_or(i);
                self.views.retain(|v| *v != s);
                self.views.insert(if i > from { i - 1 } else { i }.min(self.views.len()), s);
            }
            (Drag::View(s), _) => self.float_view(s, corner),
            (Drag::ViewWindow(s), Some(Land::Tabs(i))) => self.dock_view(s, i),
            _ => {}
        }
    }
}

/// Where a drop lands on a zone: a strip along an edge takes it out to the edge and past it too, where the hand that
/// throws a palette at the side of the window ends up.
fn reach(land: Land, r: Rect) -> Rect {
    match land {
        Land::Side(Side::Left) => r.with_min_x(f32::NEG_INFINITY),
        Land::Side(Side::Right) => r.with_max_x(f32::INFINITY),
        Land::Side(Side::Bottom) => r.with_max_y(f32::INFINITY),
        _ => r,
    }
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
        p.rect_stroke(rect, 4.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, if active { pal().text } else { pal().muted });
    r
}
