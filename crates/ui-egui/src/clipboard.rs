//! Modify > Clipboard: Copy to Clipboard (Ctrl+C), Cut to Clipboard (Ctrl+X) and Paste (Ctrl+V) with Revit's Aligned options
//! ([copy and paste](../../../docs/areas/copy-paste.md)). What's copied goes on the system clipboard as the core's text, so
//! another BIMCraft window pastes it; the window keeps its own copy too, for a system clipboard that has lost it.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

use bimcraft_core::clipboard::{self as clip, Clip, Frame};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Guid, Id};
use bimcraft_core::render::{self, Drawing, View};
use egui::{Align, Button, Context, Event, Layout, RichText, ScrollArea, Ui, ViewportCommand};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::{BimApp, Sheet, Status, Tool};
use bimcraft_i18n::msg;

/// How long a paste the ribbon asked the system clipboard for waits for its text, seconds, before taking the window's own.
const PASTE_WAIT: f64 = 0.3;

/// Revit's ways of pasting: Paste from Clipboard and the Aligned options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PasteMode {
    Manual,
    SelectedLevels,
    SelectedViews,
    CurrentView,
    SamePlace,
    PickedLevel,
    /// Ctrl+V in a 3D view: the window asks whether in the same place or by hand on the work plane.
    Where3D,
}

impl PasteMode {
    pub(crate) const ALL: [PasteMode; 6] = [
        PasteMode::Manual,
        PasteMode::SelectedLevels,
        PasteMode::SelectedViews,
        PasteMode::CurrentView,
        PasteMode::SamePlace,
        PasteMode::PickedLevel,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            PasteMode::Manual => msg!("Paste from Clipboard"),
            PasteMode::SelectedLevels => msg!("Aligned to Selected Levels"),
            PasteMode::SelectedViews => msg!("Aligned to Selected Views"),
            PasteMode::CurrentView => msg!("Aligned to Current View"),
            PasteMode::SamePlace => msg!("Aligned to Same Place"),
            PasteMode::PickedLevel => msg!("Aligned to Picked Level"),
            PasteMode::Where3D => msg!("Paste"),
        }
    }

    fn tip(self) -> &'static str {
        match self {
            PasteMode::Manual | PasteMode::Where3D => {
                msg!("Paste (Ctrl+V): the copied elements follow the cursor; click where they go")
            }
            PasteMode::SelectedLevels => msg!("Model elements onto each level picked from a list, at the same place in plan"),
            PasteMode::SelectedViews => msg!("View-specific elements into each view picked from a list"),
            PasteMode::CurrentView => msg!("Into this view where they were in theirs; model elements onto this plan's level"),
            PasteMode::SamePlace => msg!("Where they were, on their levels"),
            PasteMode::PickedLevel => msg!("In a section or an elevation: click a level; model elements go onto it"),
        }
    }
}

/// A row of a panel's small buttons; in its list (a folded panel's, or its name's), the buttons go one under another as rows
/// do: a row side by side there would ask the list for room it grows to give, frame on frame.
pub(crate) fn row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    if crate::ribbon::listing() {
        add(ui);
    } else {
        ui.horizontal(add);
    }
}

/// What the right-click menu (or a family tab's Find) asked of the clipboard, done on the next frame, which has the context
/// the system clipboard needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Cut,
    Copy,
    Paste,
    /// A paste placed as one of the Aligned options says.
    Aligned(PasteMode),
}

/// A command of the Clipboard panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Cut,
    Copy,
    Paste(PasteMode),
}

/// The Clipboard panel: Paste and its options, Cut and Copy to Clipboard, small buttons. `pasting`, whether a paste follows
/// the cursor now; `copy_tip`, Copy's tooltip; `blocked`, why a command can't be used now (greyed, the reason its tooltip
/// where there is one). The command clicked. A project's tab and a family's draw the same panel.
pub(crate) fn panel(ui: &mut Ui, pasting: bool, copy_tip: &'static str, blocked: impl Fn(Command) -> Option<String>) -> Option<Command> {
    let mut asked = None;
    let greyed = |r: egui::Response, why: Option<String>| match why {
        Some(w) if !w.is_empty() => r.on_disabled_hover_text(w),
        _ => r,
    };
    ui.vertical(|ui| {
        row(ui, |ui| {
            let tip = msg!("Paste (Ctrl+V), and its Aligned options");
            let r = crate::small_button(ui, crate::icons::Icon::Paste, msg!("Paste"), pasting, tip);
            crate::ribbon::choices(ui, &r, false, |ui| {
                for mode in PasteMode::ALL {
                    let why = blocked(Command::Paste(mode));
                    let b = ui.add_enabled(why.is_none(), Button::new(t(mode.label()))).on_hover_text(t(mode.tip()));
                    if greyed(b, why).clicked() {
                        asked = Some(Command::Paste(mode));
                    }
                }
            });
        });
        row(ui, |ui| {
            let tip = msg!("Cut to Clipboard (Ctrl+X): the selection, taken away to paste elsewhere");
            let why = blocked(Command::Cut);
            let r = ui
                .add_enabled_ui(why.is_none(), |ui| crate::small_button(ui, crate::icons::Icon::Cut, msg!("Cut to Clipboard"), false, tip));
            if greyed(r.inner, why).clicked() {
                asked = Some(Command::Cut);
            }
            let why = blocked(Command::Copy);
            let r = ui.add_enabled_ui(why.is_none(), |ui| {
                crate::small_button(ui, crate::icons::Icon::CopyClipboard, msg!("Copy to Clipboard"), false, copy_tip)
            });
            if greyed(r.inner, why).clicked() {
                asked = Some(Command::Copy);
            }
        });
    });
    asked
}

/// What a paste's preview draws: drawings of the clipboard's project, each with how far it moves into the view.
pub(crate) type Drawn = Rc<Vec<(Drawing, P2)>>;

/// A clipboard read and ready to paste: its text and what it holds.
#[derive(Clone)]
pub(crate) struct Pasting {
    pub(crate) text: Arc<str>,
    pub(crate) clip: Arc<Clip>,
    /// What its preview draws, for the view it was last drawn in ([`Pasting::drawings`]).
    drawn: Rc<RefCell<Option<(View, Drawn)>>>,
}

impl Pasting {
    fn new(text: Arc<str>, clip: Clip) -> Pasting {
        Pasting { text, clip: Arc::new(clip), drawn: Rc::default() }
    }

    /// What Paste from Clipboard's preview draws in a plan of frame `to` seen through `view`: the clipboard's project drawn
    /// as plans draw it (the plan of the level its model elements stood on, and of each plan its view-specific ones were
    /// in), each with how far it moves to stand in that plan; and what was drawn in another kind of view, as a tool's
    /// preview draws it. Drawn once for a view: as the cursor moves, only where it is drawn changes.
    pub(crate) fn drawings(&self, view: View, to: &Frame) -> Drawn {
        if let Some((v, d)) = self.drawn.borrow().as_ref()
            && *v == view
        {
            return d.clone();
        }
        let c = &self.clip;
        let m = &c.model;
        let mut out = Vec::new();
        let mut plans: Vec<Guid> = c.source.level.into_iter().collect();
        plans.extend(c.source.views.iter().filter(|(_, f)| matches!(f, Frame::Plan { .. })).map(|(v, _)| *v));
        plans.sort();
        plans.dedup();
        for key in plans {
            if m.elements.contains_key(key) {
                let shift = c.source.views.get(&key).map_or(P2(0.0, 0.0), |f| f.to(to));
                out.push((render::draw(m, key, view), shift));
            }
        }
        // Drawn in a section, an elevation or on a sheet: each as a tool previews it.
        for &id in &c.source.ids {
            let Some(e) = m.elements.get(id) else { continue };
            let from_plan = m.annotation_plan(e).is_some_and(|v| matches!(c.source.views.get(&v), Some(Frame::Plan { .. })));
            if m.annotation_plan(e).is_some()
                && !from_plan
                && let Some(shift) = c.shift_of(id, to)
            {
                out.push((render::sketch(m, view, id, e), shift));
            }
        }
        let out = Rc::new(out);
        *self.drawn.borrow_mut() = Some((view, out.clone()));
        out
    }
}

/// Aligned to Selected Levels' or Selected Views' list, while it's open.
pub(crate) struct Choose {
    pasting: Pasting,
    views: bool,
    /// Each level or view offered, with its name.
    offered: Vec<(Id, String)>,
    picked: BTreeSet<Id>,
}

impl BimApp {
    /// The view on screen a copy is made in and a paste goes into: a plan, a section or an elevation, a sheet, the 3D view.
    pub(crate) fn clipboard_view(&self) -> Option<Id> {
        match self.sheet {
            Sheet::Plan(k) | Sheet::Ceiling(k) => Some(k),
            Sheet::Section(id) | Sheet::Paper(id) => Some(id),
            Sheet::View3D => self.active_3d(),
            _ => None,
        }
    }

    /// The view on screen, as `paste` takes it: the 3D view by its name where nothing has made it yet.
    fn paste_view(&self) -> Option<Value> {
        match (self.clipboard_view(), self.sheet) {
            (Some(v), _) => Some(json!(v)),
            (None, Sheet::View3D) => Some(json!("{3D}")),
            (None, _) => None,
        }
    }

    /// The frame of the view on screen.
    fn clipboard_frame(&self) -> Option<Frame> {
        match self.sheet {
            Sheet::View3D => Some(Frame::ThreeD),
            _ => self.clipboard_view().and_then(|v| Frame::of(&self.session.model, v)),
        }
    }

    /// Whether `mode` can paste into the view on screen.
    fn paste_fits(&self, mode: PasteMode) -> bool {
        let frame = self.clipboard_frame();
        match mode {
            PasteMode::Manual | PasteMode::Where3D => frame.is_some(),
            PasteMode::CurrentView => frame.is_some(),
            PasteMode::PickedLevel => matches!(frame, Some(Frame::Section { .. })),
            PasteMode::SelectedLevels | PasteMode::SelectedViews | PasteMode::SamePlace => true,
        }
    }

    /// Revit's Clipboard panel: Paste and its options, Cut and Copy to Clipboard.
    pub(crate) fn clipboard_panel(&mut self, ui: &mut Ui) {
        // Small buttons, as Revit's are; where the ribbon is short it keeps Paste and lists Cut and Copy before any other panel
        // folds (Ctrl+C, Ctrl+X, Ctrl+V and the right-click menu do as it does).
        crate::ribbon::yielding(ui, msg!("Clipboard"), |ui| {
            let some = !self.selection.is_empty();
            let tip = msg!("Copy to Clipboard (Ctrl+C): the selection, to paste in any view, or in another BIMCraft");
            let asked = panel(ui, self.tool == Tool::Paste, tip, |c| {
                let fits = match c {
                    Command::Paste(mode) => self.paste_fits(mode),
                    Command::Cut | Command::Copy => some,
                };
                (!fits).then(String::new)
            });
            match asked {
                Some(Command::Paste(mode)) => self.ask_paste(ui.ctx(), mode),
                Some(Command::Cut) => self.copy_to_clipboard(ui.ctx(), true),
                Some(Command::Copy) => self.copy_to_clipboard(ui.ctx(), false),
                None => {}
            }
        });
    }

    /// Ctrl+C, Ctrl+X and Ctrl+V, which the window's system hands over as copy, cut and paste (with the system clipboard's
    /// text), taken out of the frame's input; and the ribbon's paste, once its text has come or it has waited for it.
    pub(crate) fn clipboard_events(&mut self, ctx: &Context) {
        // A label's text selection (the status box, a report, a plugin's note) takes Ctrl+C and Ctrl+X: egui copies the
        // selected text when the event is still in the frame's input, so leave it there. Paste, and copy or cut of the
        // model's selection, go through this window's own handler as before.
        let text_selected = ctx.plugin_opt::<egui::text_selection::LabelSelectionState>().is_some_and(|state| state.lock().has_selection());
        let events: Vec<Event> = ctx.input_mut(|i| {
            let (ours, rest): (Vec<Event>, Vec<Event>) = std::mem::take(&mut i.events).into_iter().partition(|e| match e {
                Event::Copy | Event::Cut => !text_selected,
                Event::Paste(_) => true,
                _ => false,
            });
            i.events = rest;
            ours
        });
        let now = ctx.input(|i| i.time);
        // A paste asked on another tab (a project's, or another family document's) is given up, and the text it brings with
        // it: it was meant for that tab's clipboard.
        let events = match self.paste_asked {
            Some((_, _, doc)) if doc != self.active_doc() => {
                self.paste_asked = None;
                events.into_iter().filter(|e| !matches!(e, Event::Paste(_))).collect()
            }
            _ => events,
        };
        match self.clip_request.take() {
            Some(Request::Cut) => self.copy_to_clipboard(ctx, true),
            Some(Request::Copy) => self.copy_to_clipboard(ctx, false),
            Some(Request::Paste) => {
                let mode = self.ctrl_v_mode();
                self.ask_paste(ctx, mode);
            }
            Some(Request::Aligned(mode)) => self.ask_paste(ctx, mode),
            None => {}
        }
        for e in events {
            match e {
                Event::Copy => self.copy_to_clipboard(ctx, false),
                Event::Cut => self.copy_to_clipboard(ctx, true),
                Event::Paste(text) => {
                    let mode = self.paste_asked.take().map_or_else(|| self.ctrl_v_mode(), |(m, ..)| m);
                    self.pasted_text(Some(text), mode);
                }
                _ => {}
            }
        }
        // The system clipboard said nothing (empty, or no window system): the window's own copy.
        if let Some((mode, at, _)) = self.paste_asked
            && now - at >= PASTE_WAIT
        {
            self.paste_asked = None;
            self.pasted_text(None, mode);
        } else if self.paste_asked.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(PASTE_WAIT));
        }
    }

    /// What Ctrl+V does in the view on screen: by hand where the elements can follow the cursor; in a 3D view, as asked (the
    /// same place, or by hand on the work plane); else where they were.
    fn ctrl_v_mode(&self) -> PasteMode {
        // A family tab's: following the pointer in a plan, an elevation or a sketch; in its {3D}, where they were
        // (`editor::clipboard`).
        if let Some(ed) = self.active_family() {
            return if ed.editing_2d() || ed.sketching() { PasteMode::Manual } else { PasteMode::SamePlace };
        }
        match self.clipboard_frame() {
            Some(Frame::ThreeD) => PasteMode::Where3D,
            Some(_) => PasteMode::Manual,
            None => PasteMode::SamePlace,
        }
    }

    /// Copy (or with `cut`, cut) the selection to the clipboard: the system's, and the window's own.
    pub(crate) fn copy_to_clipboard(&mut self, ctx: &Context, cut: bool) {
        // A family tab's selection goes on the family's clipboard.
        if self.active_family().is_some() {
            self.family_copy(ctx, cut);
            return;
        }
        if self.selection.is_empty() {
            self.status = Status { text: t("select what to copy first").into(), error: true };
            return;
        }
        let mut p = json!({"ids": self.selection});
        if let Some(v) = self.clipboard_view() {
            p["view"] = json!(v);
        }
        let Some(reply) = self.run(if cut { "cut_to_clipboard" } else { "copy_to_clipboard" }, p) else { return };
        let Some(text) = reply["clipboard"].as_str() else { return };
        ctx.copy_text(text.to_string());
        self.copied = Some(text.to_string());
        self.clip_text = Some(Arc::from(text));
        if cut {
            self.selection.clear();
        }
        let count = reply["count"].as_u64().unwrap_or(0).to_string();
        let left = reply["left_out"].as_array().map_or(0, Vec::len);
        self.status = Status {
            text: match (cut, left) {
                (false, 0) => tf("Copied {} elements to the clipboard.", &[&count]),
                (true, 0) => tf("Cut {} elements to the clipboard.", &[&count]),
                (_, n) => {
                    tf("{} elements to the clipboard; {} left out (levels, views and sheets aren't copied).", &[&count, &n.to_string()])
                }
            },
            error: false,
        };
    }

    /// The ribbon's paste: the system clipboard is asked for its text, which comes as a paste on a later frame
    /// ([`BimApp::clipboard_events`]).
    pub(crate) fn ask_paste(&mut self, ctx: &Context, mode: PasteMode) {
        self.paste_asked = Some((mode, ctx.input(|i| i.time), self.active_doc()));
        ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
        ctx.request_repaint();
    }

    /// A paste of `text` (the system clipboard's; none, or not BIMCraft's: the window's own copy), as `mode` places it.
    pub(crate) fn pasted_text(&mut self, text: Option<String>, mode: PasteMode) {
        // On a family tab, the family's clipboard.
        if self.active_family().is_some() {
            self.family_pasted(text, mode);
            return;
        }
        // A family's copy, made since: the clipboard's last copy isn't the project's, so its older one isn't pasted.
        if text.as_deref().is_some_and(bimcraft_core::family::clipboard::is_family_clipboard) {
            self.status = Status { text: t("The clipboard holds a family's copy: paste it in a family tab.").into(), error: true };
            return;
        }
        let text: Option<Arc<str>> = match text {
            Some(t) if clip::is_clipboard(&t) => Some(Arc::from(t)),
            _ => self.clip_text.clone(),
        };
        let Some(text) = text else {
            self.status = Status { text: t("the clipboard holds nothing to paste: copy elements first (Ctrl+C)").into(), error: true };
            return;
        };
        let read = match clip::read(&text) {
            Ok(c) => c,
            Err(e) => {
                self.status = Status { text: e, error: true };
                return;
            }
        };
        let pasting = Pasting::new(text, read);
        match mode {
            PasteMode::Manual if self.paste_fits(PasteMode::Manual) => {
                self.pasting = Some(pasting);
                self.set_tool(Tool::Paste);
                let text = if self.sheet == Sheet::View3D {
                    t("Click on the work plane where the pasted elements go. Esc cancels.")
                } else {
                    t("Click where the pasted elements go. Esc cancels.")
                };
                self.status = Status { text: text.into(), error: false };
            }
            PasteMode::Where3D if self.paste_fits(PasteMode::Manual) => self.paste_where = Some(pasting),
            PasteMode::Manual | PasteMode::Where3D | PasteMode::SamePlace => self.paste_with(&pasting, json!({"mode": "same_place"})),
            PasteMode::CurrentView => self.paste_with(&pasting, json!({"mode": "current_view"})),
            PasteMode::PickedLevel => {
                self.pasting = Some(pasting);
                self.set_tool(Tool::PickLevel);
                self.status = Status { text: t("Click a level the pasted elements go onto. Esc cancels.").into(), error: false };
            }
            PasteMode::SelectedLevels => {
                let m = &self.session.model;
                let offered = m.levels().into_iter().map(|(id, l)| (id, l.name.clone())).collect();
                self.paste_choose = Some(Choose { pasting, views: false, offered, picked: BTreeSet::new() });
            }
            PasteMode::SelectedViews => {
                let offered = self.views_like(&pasting.clip);
                self.paste_choose = Some(Choose { pasting, views: true, offered, picked: BTreeSet::new() });
            }
        }
    }

    /// The views Aligned to Selected Views offers: those like the ones the view-specific elements were copied from (plans
    /// for a plan, sections and elevations looking the same way for a section or an elevation, sheets for a sheet).
    fn views_like(&self, c: &Clip) -> Vec<(Id, String)> {
        let m = &self.session.model;
        let from: Vec<&Frame> = c.source.views.values().collect();
        let like = |f: &Frame| {
            from.iter().any(|g| match (g, f) {
                (Frame::Sheet, Frame::Sheet) => true,
                _ => g.same_orientation(f),
            })
        };
        let mut views: Vec<(Id, String)> = m
            .elements
            .iter()
            .filter(|(_, e)| {
                matches!(e, Element::Level(_) | Element::PlanView(_) | Element::Section(_) | Element::Elevation(_) | Element::Sheet(_))
            })
            .filter_map(|(id, e)| {
                let f = Frame::of(m, *id)?;
                let name = match e {
                    Element::Sheet(s) => format!("{} - {}", s.number, s.name),
                    _ => m.view_name(*id)?.to_string(),
                };
                like(&f).then_some((*id, name))
            })
            .collect();
        views.sort_by(|a, b| a.1.cmp(&b.1));
        views
    }

    /// Paste `pasting` with `p`'s mode and places; the pasted elements selected.
    fn paste_with(&mut self, pasting: &Pasting, mut p: Value) {
        p["clipboard"] = json!(&*pasting.text);
        if p.get("view").is_none()
            && let Some(v) = self.paste_view()
        {
            p["view"] = v;
        }
        let Some(reply) = self.run("paste", p) else { return };
        let ids: Vec<Id> = reply["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        let left: Vec<String> =
            reply["left_out"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        self.status = match left.first() {
            None => Status { text: tf("Pasted {} elements.", &[&ids.len().to_string()]), error: false },
            Some(why) => Status {
                text: tf("Pasted {} elements; {} left out: {}", &[&ids.len().to_string(), &left.len().to_string(), why]),
                error: true,
            },
        };
        self.selection = ids;
    }

    /// Paste from Clipboard's click: the middle of what's pasted at `at`, in view `view`'s terms.
    pub(crate) fn paste_at(&mut self, view: Id, at: P2) {
        let Some(pasting) = self.pasting.clone() else {
            self.set_tool(Tool::Modify);
            return;
        };
        self.paste_with(&pasting, json!({"mode": "manual", "view": view, "at": [at.0, at.1]}));
        self.pasting = None;
        self.set_tool(Tool::Modify);
    }

    /// Paste from Clipboard's click in the 3D view, at `q` on its work plane: the middle of what's pasted there in plan, the
    /// model elements on the level that point stands on (the highest at or below it).
    pub(crate) fn paste_at3(&mut self, q: [f64; 3]) {
        let (Some(pasting), Some(view)) = (self.pasting.clone(), self.paste_view()) else {
            self.set_tool(Tool::Modify);
            return;
        };
        let mut p = json!({"mode": "manual", "view": view, "at": [q[0], q[1]]});
        if let Some(level) = self.session.model.camera_level(q[2]) {
            p["level"] = json!(level);
        }
        self.paste_with(&pasting, p);
        self.pasting = None;
        self.set_tool(Tool::Modify);
    }

    /// Ctrl+V's question in a 3D view, while it's asked: the same place, or by hand on the work plane.
    fn paste_where_window(&mut self, ctx: &Context) {
        let Some(pasting) = self.paste_where.clone() else { return };
        let (mut same, mut by_hand, mut cancel) = (false, false, false);
        let r = Modal::new(egui::Id::new("paste-where"), t("Paste in 3D")).width(360.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t(msg!("Where do the pasted elements go?")));
                ui.add_space(6.0);
                same = ui
                    .button(t(msg!("Same Place")))
                    .on_hover_text(t(msg!("Where they were, on their levels: Aligned to Same Place")))
                    .clicked();
                by_hand = ui
                    .button(t(msg!("Position on Work Plane")))
                    .on_hover_text(t(msg!("They follow the cursor over the view's work plane; click where they go")))
                    .clicked();
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if same || by_hand || cancel || r.should_close() {
            self.paste_where = None;
        }
        if same {
            self.paste_with(&pasting, json!({"mode": "same_place"}));
        } else if by_hand {
            self.pasting = Some(pasting);
            self.set_tool(Tool::Paste);
            self.status = Status { text: t("Click on the work plane where the pasted elements go. Esc cancels.").into(), error: false };
        }
    }

    /// Aligned to Picked Level's click on level `level`.
    pub(crate) fn paste_on_level(&mut self, level: Id) {
        let Some(pasting) = self.pasting.clone() else {
            self.set_tool(Tool::Modify);
            return;
        };
        if self.session.model.level(level).is_none() {
            self.status = Status { text: t("click a level line").into(), error: true };
            return;
        }
        self.paste_with(&pasting, json!({"mode": "picked_level", "level": level}));
        self.pasting = None;
        self.set_tool(Tool::Modify);
    }

    /// Where the paste's middle is, and its box, in the view of frame `to`: what follows the cursor.
    pub(crate) fn paste_extent(&self, to: &Frame) -> Option<[P2; 2]> {
        self.pasting.as_ref().and_then(|p| p.clip.extent_in(to))
    }

    /// Aligned to Selected Levels' and Selected Views' list, while one is open.
    pub(crate) fn paste_window(&mut self, ctx: &Context) {
        self.paste_where_window(ctx);
        let Some(d) = self.paste_choose.as_mut() else { return };
        let (mut ok, mut cancel) = (false, false);
        let title = if d.views { t(PasteMode::SelectedViews.label()) } else { t(PasteMode::SelectedLevels.label()) };
        let r = Modal::new(egui::Id::new("paste-choose"), title).width(360.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t(if d.views { msg!("Paste into these views:") } else { msg!("Paste onto these levels:") }));
                if d.offered.is_empty() {
                    ui.label(RichText::new(t(msg!("None of this project's views is like the one they were copied from."))).weak());
                }
                ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (id, name) in &d.offered {
                        let mut on = d.picked.contains(id);
                        if ui.checkbox(&mut on, name).changed() {
                            if on {
                                d.picked.insert(*id);
                            } else {
                                d.picked.remove(id);
                            }
                        }
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(!d.picked.is_empty(), Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok && let Some(d) = self.paste_choose.take() {
            let picked: Vec<Id> = d.picked.into_iter().collect();
            let p = if d.views { json!({"mode": "views", "views": picked}) } else { json!({"mode": "levels", "levels": picked}) };
            self.paste_with(&d.pasting, p);
        } else if cancel || r.should_close() {
            self.paste_choose = None;
        }
    }
}
