//! The ribbon's panels: each panel's tools over its name, and the name a button that lists them all. A tab too wide for the window makes room from the right: a panel hands its last tools over to its list,
//! and when one tool isn't room enough it folds to one button that lists them; what still doesn't fit scrolls sideways.

use std::cell::{Cell, RefCell};

use egui::{
    Align2, FontId, Id, Popup, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::lang::t;
use crate::presets::Scale;
use crate::style::pal;

/// How wide a folded panel is.
const FOLDED: f32 = 72.0;
/// The hairline between panels, with its room.
const RULE: f32 = 9.0;
/// The room between a panel's tools; one handed over still takes it, at no size.
const GAP: f32 = 2.0;
/// What's kept clear at the tab's end, so a fit judged on last frame's widths isn't a pixel over.
const SLACK: f32 = 8.0;

/// How a panel fits: all its tools shown, its first so many, or folded to one button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fit {
    Show(usize),
    Fold,
}

/// A panel as the last frame drew it: its name, how wide it is with every tool shown and as it was drawn, how many tools it
/// has, whether it is kept whole ([`kept`]), the others making room before it, and whether it makes room first
/// ([`yielding`]); and how wide it is showing its first so many tools, as measured when it showed them all.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Seen {
    name: String,
    natural: f32,
    drawn: f32,
    tools: usize,
    kept: bool,
    yields: bool,
    widths: Vec<f32>,
}

impl Seen {
    /// The room the panel makes showing only its first `k` tools: as measured, its name's row included (no narrower than
    /// its name, however few tools it shows); else, not yet measured, its tools' average width for each handed over.
    fn makes(&self, k: usize) -> f32 {
        match self.widths.get(k) {
            Some(w) => (self.natural - w).max(0.0),
            None => self.tools.saturating_sub(k) as f32 * self.natural / self.tools.max(1) as f32,
        }
    }
}

thread_local! {
    /// Whether the tools drawn now are a panel's list: rows of an icon and its name, not the ribbon's buttons.
    static LISTING: Cell<bool> = const { Cell::new(false) };
    /// Whether the rows drawn now are BIMCraft's Tools palette, each panel's under its name ([`rows`]).
    static IN_PALETTE: Cell<bool> = const { Cell::new(false) };
    /// Whether the tabs' Select panel is left out: the Tools palette draws it once, over the tabs it shows.
    static SKIP_SELECT: Cell<bool> = const { Cell::new(false) };
    /// What the ribbon and the Tools palette leave out this frame, and whether Edit Tools is on (`presets.rs`).
    static TOOL_RULES: RefCell<(std::collections::BTreeSet<String>, Scale, bool)> = RefCell::new(Default::default());
    /// Whether a tab's tools are being drawn, on the ribbon or in the Tools palette: the rules apply there alone, not to the view
    /// control bar's buttons, which are drawn the same way.
    static IN_TOOLS: Cell<bool> = const { Cell::new(false) };
    /// The tools whose box was ticked or cleared in Edit Tools this frame, by name.
    static TOGGLED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The tab being drawn, for the keys of its panels: `tab:<tab>/<panel>` (`ТЗ-019`). A plugin's tab is `plugin:<name>`
    /// and a family document's `family:<DocTab>`.
    static CURRENT_TAB: RefCell<String> = const { RefCell::new(String::new()) };
    /// The keys of the panels the user took out of the window, drawn in Edit Tools alone.
    static PANELS_HIDDEN: RefCell<std::collections::BTreeSet<String>> = const { RefCell::new(std::collections::BTreeSet::new()) };
    /// The panels whose box was ticked or cleared in Edit Tools this frame, by key.
    static TOGGLED_PANELS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Every panel key drawn since the window started: what a preset's editor lists under its tabs.
    static SEEN_PANELS: RefCell<std::collections::BTreeSet<String>> = const { RefCell::new(std::collections::BTreeSet::new()) };
    /// How many panels the tab being drawn has drawn: none at all says so (every panel of it is hidden).
    static DRAWN_PANELS: Cell<usize> = const { Cell::new(0) };
    /// How many panels of the tab being drawn the user took out: it says so only when some were, not when the scale hid them all.
    static HIDDEN_PANELS: Cell<usize> = const { Cell::new(0) };
    /// How the panels of the tab being drawn fit, by name; one not named shows all its tools.
    static FITS: RefCell<Vec<(String, Fit)>> = const { RefCell::new(Vec::new()) };
    /// The panels of the tab being drawn, as they're drawn.
    static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
    /// The tools of the panel being drawn so far, and how many it shows.
    static TOOLS: Cell<(usize, usize)> = const { Cell::new((0, usize::MAX)) };
    /// How far right the panel being drawn with every tool reaches as each of its tools comes: its tools' row showing the ones
    /// before it. None outside such a panel.
    static REACH: RefCell<Option<Vec<f32>>> = const { RefCell::new(None) };
    /// Where the last panel drawn ends.
    static END: Cell<f32> = const { Cell::new(0.0) };
    /// The tab being drawn: the room it has, where it starts, and how much of what it has drawn is gaps (`push_right`'s).
    static LAID: Cell<(f32, f32, f32)> = const { Cell::new((0.0, 0.0, 0.0)) };
    /// A tool's choices were asked for (a shortcut's) while it has no button, being handed over or folded: its panel opens its list.
    static OPEN_LIST: Cell<bool> = const { Cell::new(false) };
    /// Whether the panel drawn next is kept whole while others make room ([`kept`]).
    static KEEP: Cell<bool> = const { Cell::new(false) };
    /// Whether the panel drawn next makes room before the others ([`yielding`]).
    static YIELD: Cell<bool> = const { Cell::new(false) };
    /// Whether the tab being drawn has been drawn before, so its panels' fit (which tools show) is known: its first frame lays out
    /// every tool, whatever the room.
    static FITTED: Cell<bool> = const { Cell::new(true) };
}

/// Whether the tools drawn now go in a panel's list.
pub(crate) fn listing() -> bool {
    LISTING.with(Cell::get)
}

/// Whether the rows drawn now are the Tools palette's (not a panel's list, which is narrower and in a popup).
pub(crate) fn in_palette() -> bool {
    IN_PALETTE.with(Cell::get)
}

/// Draws a tab's panels, `add` drawing them, as rows down `ui`, each panel's under its name: BIMCraft's Tools palette, which
/// shows as rows what the ribbon lays out as buttons. (After PR 194's tool palette, by PolyglotOpenstreetmap.)
pub(crate) fn rows(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    LISTING.with(|l| l.set(true));
    IN_PALETTE.with(|p| p.set(true));
    IN_TOOLS.with(|i| i.set(true));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui);
    });
    IN_TOOLS.with(|i| i.set(false));
    IN_PALETTE.with(|p| p.set(false));
    LISTING.with(|l| l.set(false));
}

/// What the tools drawn this frame leave out (the user's hidden tools, the project's scale), and whether Edit Tools is on.
pub(crate) fn tool_rules(hidden: std::collections::BTreeSet<String>, scale: Scale, editing: bool) {
    TOOL_RULES.with(|r| *r.borrow_mut() = (hidden, scale, editing));
}

/// Whether Edit Tools is on, for a tool of a tab being drawn: every one is a box to tick, what it shows.
pub(crate) fn editing_tools() -> bool {
    IN_TOOLS.with(Cell::get) && TOOL_RULES.with(|r| r.borrow().2)
}

/// Whether the user took the tool named `name` (its English name) out.
pub(crate) fn tool_hidden(name: &str) -> bool {
    TOOL_RULES.with(|r| r.borrow().0.contains(name))
}

/// Whether the project's scale leaves out the panel or the tool named `name`.
pub(crate) fn scale_hides(name: &str) -> bool {
    TOOL_RULES.with(|r| r.borrow().1.hides(name))
}

/// Whether the tool named `name` is left out of what is drawn: the scale's, or the user's but in Edit Tools, where it shows unticked.
pub(crate) fn left_out(name: &str) -> bool {
    IN_TOOLS.with(Cell::get) && (scale_hides(name) || (tool_hidden(name) && !editing_tools()))
}

/// Ticks or clears the tool named `name` in Edit Tools.
pub(crate) fn toggle(name: &str) {
    TOGGLED.with(|t| t.borrow_mut().push(name.to_string()));
}

/// The tools ticked or cleared since the last call.
pub(crate) fn take_toggled() -> Vec<String> {
    TOGGLED.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

/// The tab the panels drawn next belong to: its English name, `plugin:<name>` for a plugin's tab, `family:<DocTab>` for a family
/// document's. A panel's key is built from it ([`panel_key`]).
pub(crate) fn set_tab(name: &str) {
    CURRENT_TAB.with(|t| *t.borrow_mut() = name.to_string());
}

/// The key of the panel named `title` of the tab being drawn: `tab:<tab>/<panel>`, never translated.
pub(crate) fn panel_key(title: &str) -> String {
    let tab = CURRENT_TAB.with(|t| t.borrow().clone());
    format!("tab:{tab}/{title}")
}

/// What the panels drawn now leave out: the user's hidden panels (`panels_hidden` in `settings.json`).
pub(crate) fn panel_rules(hidden: std::collections::BTreeSet<String>) {
    PANELS_HIDDEN.with(|h| *h.borrow_mut() = hidden);
}

/// Whether the user took the panel whose key is `key` out of the window.
pub(crate) fn panel_hidden(key: &str) -> bool {
    PANELS_HIDDEN.with(|h| h.borrow().contains(key))
}

/// Ticks or clears the panel whose key is `key` in Edit Tools.
pub(crate) fn toggle_panel(key: &str) {
    TOGGLED_PANELS.with(|t| t.borrow_mut().push(key.to_string()));
}

/// The panels ticked or cleared since the last call.
pub(crate) fn take_toggled_panels() -> Vec<String> {
    TOGGLED_PANELS.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

/// Starts counting the panels the tab drawn next has: [`drawn_panels`] says whether any was drawn.
pub(crate) fn begin_tab() {
    DRAWN_PANELS.with(|d| d.set(0));
    HIDDEN_PANELS.with(|h| h.set(0));
}

/// How many panels the tab being drawn drew.
pub(crate) fn drawn_panels() -> usize {
    DRAWN_PANELS.with(Cell::get)
}

/// How many panels of the tab being drawn the user took out.
pub(crate) fn hidden_panels() -> usize {
    HIDDEN_PANELS.with(Cell::get)
}

/// Keeps a panel's key for the preset editor.
fn note_panel(key: &str) {
    SEEN_PANELS.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() < 2048 {
            s.insert(key.to_string());
        }
    });
}

/// The panel keys drawn since the window started, for the preset editor: taken once a frame.
pub(crate) fn take_seen_panels() -> Vec<String> {
    SEEN_PANELS.with(|s| std::mem::take(&mut *s.borrow_mut()).into_iter().collect())
}

/// Leaves the tabs' Select panel out of what is drawn next, or puts it back.
pub(crate) fn skip_select(skip: bool) {
    SKIP_SELECT.with(|s| s.set(skip));
}

/// Whether the Select panel is being left out.
pub(crate) fn skipping_select() -> bool {
    SKIP_SELECT.with(Cell::get)
}

/// Counts a ribbon tool, and says whether its panel has handed it over to its list: then it isn't drawn on the ribbon. A
/// handed-over tool takes no room and has no name, so it's found only in the list.
pub(crate) fn handed_over(ui: &mut Ui) -> Option<Response> {
    if listing() {
        return None;
    }
    let (n, shown) = TOOLS.with(Cell::get);
    TOOLS.with(|t| t.set((n + 1, shown)));
    // Where what its panel has drawn so far ends: a ui with nothing in it yet (a tool's own scope, a column's first) begins a gap
    // after it.
    let drawn = ui.min_rect();
    let reach = if drawn.width() > 0.0 { drawn.right() } else { ui.cursor().left() - GAP };
    REACH.with(|r| r.borrow_mut().as_mut().map(|v| v.push(reach)));
    (n >= shown).then(|| ui.allocate_response(Vec2::ZERO, Sense::hover()))
}

/// Draw a tab's panels, `add` drawing them, in `room`: those it can't hold make room from the right, as the last frame's
/// widths say they must.
pub(crate) fn tab(ui: &mut Ui, key: Id, room: f32, add: impl FnOnce(&mut Ui)) {
    // The last frame's panels, how wide the whole tab would have been with every tool shown, and what the panels' widths missed
    // at the room it had then: asked of the panels again, so a tab they misjudged still ends up fitting.
    let (before, whole, missed, then): (Vec<Seen>, f32, f32, f32) = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
    let missed = if (then - room).abs() < 0.5 { missed } else { 0.0 };
    let fitted = !before.is_empty();
    FITTED.with(|f| f.set(fitted));
    FITS.with(|f| *f.borrow_mut() = fits(&before, whole + missed, room - SLACK));
    SEEN.with(|s| s.borrow_mut().clear());
    let left = ui.cursor().left();
    END.with(|e| e.set(left));
    LAID.with(|l| l.set((room - SLACK, left, 0.0)));
    IN_TOOLS.with(|i| i.set(true));
    add(ui);
    IN_TOOLS.with(|i| i.set(false));
    let drawn = END.with(Cell::get) - left - LAID.with(Cell::get).2;
    let seen = SEEN.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let whole = drawn + seen.iter().map(|p| (p.natural - p.drawn).max(0.0)).sum::<f32>();
    // Still over though fitted (a first frame lays out every tool): another frame, asking more of the panels, until it fits or all
    // there is to ask is asked.
    let over = if fitted { (drawn - (room - SLACK)).max(0.0) } else { 0.0 };
    // Nothing more to ask once the panels make all the room they can (a kept panel folded last, a yielding one keeping its
    // first tool): what is still over then scrolls, and no frame more changes it.
    let asked = FITS.with(|f| f.borrow().clone());
    let exhausted = asked == fits(&seen, whole.max(1.0) * 4.0 + room, room - SLACK);
    // What was missed was the panels' that were there: a tab that now draws others under the same key (another selection's
    // contextual tab, its panels named alike but their tools not as wide) starts again from what they say, else it would hand
    // over or fold what has room.
    let same = before.len() == seen.len()
        && before.iter().zip(&seen).all(|(a, b)| a.name == b.name && a.tools == b.tools && (a.natural - b.natural).abs() < 0.5);
    let missed = if same { (missed + over).min(whole) } else { 0.0 };
    if over > 0.0 && (!same || (missed < whole && !exhausted)) {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, (seen, whole, missed, room)));
    FITS.with(|f| f.borrow_mut().clear());
    FITTED.with(|f| f.set(true));
}

/// Pushes what is drawn next to the right end of the tab being drawn, a panel `width` wide (one that stands apart, as Edit Mode's
/// does). The gap is not width the tab needs, so it never makes the panels before it hand their tools over.
pub(crate) fn push_right(ui: &mut Ui, width: f32) {
    // Rows (the Tools palette's, a panel's list) have no right end, and the room kept here is the last ribbon's: a window that wore
    // the other profile, or another test's, would leave a gap down the palette.
    if listing() {
        return;
    }
    let (room, left, gaps) = LAID.with(Cell::get);
    let gap = (room - (END.with(Cell::get) - left - gaps) - width).max(0.0);
    ui.add_space(gap);
    LAID.with(|l| l.set((room, left, gaps + gap)));
}

/// How the panels `seen` fit, for a tab `whole` wide with every tool shown, in `room`: from the last panel back, each hands
/// over as few tools as make it fit, or, with one left not enough, folds and the next is asked. Yielding panels are asked
/// first, and hand over all but their first tool at most; kept panels last, once every other has folded.
pub(crate) fn fits(seen: &[Seen], whole: f32, room: f32) -> Vec<(String, Fit)> {
    let mut over = whole - room;
    let mut fits = Vec::new();
    let rank = |p: &Seen| {
        if p.yields {
            0
        } else if p.kept {
            2
        } else {
            1
        }
    };
    let order = (0..3).flat_map(|k| seen.iter().rev().filter(move |p| rank(p) == k));
    for p in order {
        if over <= 0.0 {
            break;
        }
        // A yielding panel hands over what it can and keeps its first tool: it never folds, and the rest is the others'.
        if p.yields {
            let k = (1..p.tools).rev().find(|&k| p.makes(k) >= over).unwrap_or(1);
            if k < p.tools {
                over -= p.makes(k);
                fits.push((p.name.clone(), Fit::Show(k)));
            }
            continue;
        }
        if let Some(k) = (1..p.tools).rev().find(|&k| p.makes(k) >= over) {
            over -= p.makes(k);
            fits.push((p.name.clone(), Fit::Show(k)));
            break;
        }
        if p.natural > FOLDED {
            over -= p.natural - FOLDED;
            fits.push((p.name.clone(), Fit::Fold));
        }
    }
    // A panel folded on the way may not have been needed: the ones before it, asked after, can make more room than was left
    // over (a whole tool's worth). Each fold that room still holds is given back, the first made first; a kept panel's
    // before any other's, and none other while a kept one stays folded, so it still gives way last. A fold the room can't
    // hold whole is given back in part when it holds the panel's first tools, measured: its first so many shown, the rest
    // handed over, wider than its folded button by no more than what is spare.
    let mut spare = -over;
    for kept in [true, false] {
        let mut stays = false;
        fits.retain_mut(|(name, fit)| {
            let Some(p) = seen.iter().find(|p| &p.name == name && p.kept == kept).filter(|_| *fit == Fit::Fold) else {
                return true;
            };
            let made = p.natural - FOLDED;
            if made <= spare {
                spare -= made;
                return false;
            }
            stays = true;
            if let Some(k) = (1..p.tools).rev().find(|&k| p.widths.get(k).is_some_and(|w| w - FOLDED <= spare)) {
                spare -= p.widths[k] - FOLDED;
                *fit = Fit::Show(k);
            }
            true
        });
        if kept && stays {
            break;
        }
    }
    fits
}

/// A ribbon panel kept whole while the tab makes room: every other panel hands its tools over or folds first, whatever its
/// place, as the Family Editor panel (Load into Project) stays in sight in a family's narrow window. It gives way only when
/// the others, all folded, still leave it no room.
pub(crate) fn kept(ui: &mut Ui, title: &str, add: impl FnMut(&mut Ui)) {
    KEEP.with(|k| k.set(true));
    panel(ui, title, add);
    KEEP.with(|k| k.set(false));
}

/// A ribbon panel that makes room before the others when the tab is too wide, whatever its place: one whose last tools are
/// greyed, not in BIMCraft yet (the family's Model Text and Level), which cost nothing to hand over to its list.
pub(crate) fn yielding(ui: &mut Ui, title: &str, add: impl FnMut(&mut Ui)) {
    YIELD.with(|k| k.set(true));
    panel(ui, title, add);
    YIELD.with(|k| k.set(false));
}

/// A ribbon panel: its tools over its name, which lists them; or, folded, one button that does. Taken out of the window, it is
/// drawn in Edit Tools alone, dimmed, under an empty box that brings it back.
pub(crate) fn panel(ui: &mut Ui, title: &str, mut add: impl FnMut(&mut Ui)) {
    let (kept, yields) = (KEEP.with(Cell::take), YIELD.with(Cell::take));
    // A panel the project's scale leaves out: not drawn, on the ribbon or in the Tools palette.
    if scale_hides(title) {
        return;
    }
    let key = panel_key(title);
    note_panel(&key);
    let editing = editing_tools();
    let on = !panel_hidden(&key);
    // Taken out, it is drawn in Edit Tools alone: there its box is, and its tools' boxes with it.
    if !on && !editing {
        HIDDEN_PANELS.with(|h| h.set(h.get() + 1));
        return;
    }
    DRAWN_PANELS.with(|d| d.set(d.get() + 1));
    if listing() {
        // Inside another panel's list (a folded panel's): the tools as rows; in the Tools palette, under the panel's name.
        if in_palette() {
            if editing {
                let mut shown = on;
                if ui.checkbox(&mut shown, t(title)).changed() {
                    toggle_panel(&key);
                }
            } else {
                // Painted: a panel may be named as a tab is (the Structure tab's Structure panel), and its tools say the rest.
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                let at = r.left_bottom() - vec2(0.0, 6.0);
                ui.painter().text(at, Align2::LEFT_BOTTOM, t(title), FontId::proportional(11.5), pal().muted);
            }
        }
        if !on {
            ui.scope(|ui| {
                ui.set_opacity(0.45);
                add(ui);
            });
        } else {
            add(ui);
        }
        return;
    }
    let name = t(title);
    let id = ui.id().with(("ribbon-panel", title));
    // Edit Tools shows every tool, its box to tick: no panel folds it to its list there.
    let fit = if editing { None } else { FITS.with(|f| f.borrow().iter().find(|(n, _)| n == title).map(|(_, fit)| *fit)) };
    let shows = match fit {
        Some(Fit::Show(k)) => k,
        _ => usize::MAX,
    };
    TOOLS.with(|t| t.set((0, shows)));
    REACH.with(|r| *r.borrow_mut() = fit.is_none().then(Vec::new));
    // Where its tools' row begins, and how wide its name's row is at least.
    let mut row_at = (0.0, 0.0);
    let drawn = if fit == Some(Fit::Fold) {
        let r = folded_button(ui, name);
        // Counted, not drawn, in a child that takes no room in the row: a scope would, at no size, still add the row's spacing
        // after the button, so a folded panel would be wider than the room its fold was judged to make.
        TOOLS.with(|t| t.set((0, 0)));
        let mut counted = ui.new_child(UiBuilder::new().id_salt(("ribbon-folded", title)).invisible());
        // Counted at no size and no spacing either: else each handed-over tool would widen the folded button's room.
        counted.spacing_mut().item_spacing = Vec2::ZERO;
        add(&mut counted);
        open_if_asked(ui, &r);
        list(&r, &mut add);
        r.rect.width()
    } else {
        let shown = ui.vertical(|ui| {
            if editing {
                // The panel's name is its box: ticked shows it, cleared takes it out and keeps its tools' boxes here. It stands
                // over the tools, where a short ribbon cannot clip it.
                let mut shown = on;
                let r = ui.checkbox(&mut shown, t(title));
                if r.changed() {
                    toggle_panel(&key);
                }
                row_at.1 = r.rect.width();
                ui.scope(|ui| {
                    if !on {
                        ui.set_opacity(0.45);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        row_at.0 = ui.min_rect().left();
                        add(ui);
                    })
                });
                r
            } else {
                let row = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    row_at.0 = ui.min_rect().left();
                    add(ui);
                });
                let font = FontId::proportional(11.5);
                let text = ui.painter().layout_no_wrap(name.to_string(), font.clone(), pal().muted).size().x;
                row_at.1 = text + 24.0;
                let (r, title_row) = ui.allocate_exact_size(vec2(row.response.rect.width().max(text + 24.0), 16.0), Sense::click());
                if title_row.hovered() {
                    ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(text + 22.0, 16.0)), 4.0, pal().hover_fill);
                }
                ui.painter().text(r.center() - vec2(5.0, 0.0), Align2::CENTER_CENTER, name, font, pal().muted);
                caret(ui, pos2(r.center().x + text / 2.0 + 3.0, r.center().y), pal().muted);
                let label = format!("{name} ▾");
                title_row.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
                open_if_asked(ui, &title_row);
                list(&title_row.clone().on_hover_text(name), &mut add);
                title_row
            }
        });
        shown.response.rect.width()
    };
    let tools = TOOLS.with(Cell::get).0;
    TOOLS.with(|t| t.set((0, usize::MAX)));
    // How wide it is with every tool shown, and showing its first so many: as drawn when it showed them all, else as it was then.
    let (natural, widths) = match fit {
        None => {
            // Showing its first `k` tools, its row reaches as far as it had when the next came, and each tool handed over still
            // takes its gap; the panel is no narrower than its name.
            let (left, name_row) = row_at;
            let reached = REACH.with(|r| r.borrow_mut().take()).unwrap_or_default();
            let mut far = left;
            let widths: Vec<f32> = (reached.iter().enumerate())
                .map(|(k, &x)| {
                    far = far.max(x);
                    (far - left + (reached.len() - k) as f32 * GAP).max(name_row)
                })
                .collect();
            ui.ctx().data_mut(|d| d.insert_temp(id, (drawn, widths.clone())));
            (drawn, widths)
        }
        Some(fit) => {
            let (natural, mut widths) = ui.ctx().data(|d| d.get_temp::<(f32, Vec<f32>)>(id)).unwrap_or((drawn, Vec::new()));
            // Showing its first so many, it is as wide as it was drawn now: a width judged from the full row a pixel out (a
            // column of tools stacked) is put right, so the next frame asks no more of it than it needs.
            if let Fit::Show(k) = fit
                && let Some(w) = widths.get_mut(k)
                && *w != drawn
            {
                *w = drawn;
                ui.ctx().data_mut(|d| d.insert_temp(id, (natural, widths.clone())));
            }
            (natural, widths)
        }
    };
    SEEN.with(|s| s.borrow_mut().push(Seen { name: title.to_string(), natural, drawn, tools, kept, yields, widths }));
    // A hairline between panels, short of the card's edges.
    let (rule, _) = ui.allocate_exact_size(vec2(RULE, ui.available_height().max(70.0)), Sense::hover());
    ui.painter().vline(rule.center().x, rule.y_range().shrink(4.0), Stroke::new(1.0, pal().border));
    END.with(|e| e.set(rule.right()));
}

/// Opens the list under `r`, the name of the panel just drawn, when a tool of it asked for its choices and had no button to show them.
fn open_if_asked(ui: &Ui, r: &Response) {
    if OPEN_LIST.with(Cell::take) {
        Popup::open_id(ui.ctx(), Popup::default_response_id(r));
    }
}

/// The choices a tool drops down from its button `r` (a ribbon button or, in a panel's list, its row), `open` asking for them at
/// once (a shortcut's): true when that is done, or handed to the panel (a tab's first frame lays out every tool, so what shows
/// is not known until the next). egui keeps one popup open: a popup of the row's own would close the list that holds the row, and
/// with it itself, so in a list the choices are a submenu of it. A tool the panel has handed over or folded has no button on the
/// ribbon to hang them from: a request to open them opens the panel's list, where its row is.
pub(crate) fn choices(ui: &Ui, r: &Response, open: bool, content: impl FnOnce(&mut Ui)) -> bool {
    // The Tools palette is in no popup: there a row's choices are a menu of its own, as a ribbon button's are.
    if listing() && !in_palette() {
        egui::containers::menu::SubMenu::new().show(ui, r, content);
        return false;
    }
    let drawn = r.rect.is_positive() && ui.is_rect_visible(r.rect);
    let asked = open && FITTED.with(Cell::get);
    if drawn {
        if asked {
            Popup::open_id(ui.ctx(), Popup::default_response_id(r));
        }
        Popup::menu(r).show(content);
    } else if asked {
        OPEN_LIST.with(|o| o.set(true));
    }
    asked
}

/// The list of a panel's tools, under `r` while it's open: `add` draws them as rows.
fn list(r: &Response, add: &mut impl FnMut(&mut Ui)) {
    Popup::menu(r).width(220.0).show(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        LISTING.with(|l| l.set(true));
        ui.vertical(|ui| add(ui));
        LISTING.with(|l| l.set(false));
    });
}

/// A folded panel: its name, over two lines if need be, and a caret, as wide as a tool.
fn folded_button(ui: &mut Ui, name: &str) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(FOLDED, 76.0), Sense::click());
    let label = format!("{name} ▾");
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&r));
    let corner = 6.0;
    if r.hovered() || open {
        ui.painter().rect(rect.shrink2(vec2(0.0, 2.0)), corner, pal().hover_fill, Stroke::NONE, StrokeKind::Inside);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, corner, crate::style::focus_ring(), StrokeKind::Inside);
    }
    let font = FontId::proportional(12.0);
    let galley = ui.painter().layout(name.to_string(), font, pal().text, FOLDED - 8.0);
    let at = pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0 - 6.0);
    caret(ui, pos2(rect.center().x, at.y + galley.size().y + 8.0), pal().muted);
    ui.painter().galley(at, galley, pal().text);
    r
}

/// A small caret pointing down, at `c`.
fn caret(ui: &Ui, c: egui::Pos2, ink: egui::Color32) {
    ui.painter().add(Shape::convex_polygon(vec![c + vec2(-3.0, -1.5), c + vec2(3.0, -1.5), c + vec2(0.0, 2.0)], ink, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels(list: &[(f32, usize)]) -> Vec<Seen> {
        list.iter()
            .enumerate()
            .map(|(i, &(w, n))| Seen { name: format!("p{i}"), natural: w, drawn: w, tools: n, ..Default::default() })
            .collect()
    }

    fn fit(list: &[(f32, usize)], room: f32) -> Vec<(String, Fit)> {
        let whole = list.iter().map(|(w, _)| w + RULE).sum();
        fits(&panels(list), whole, room)
    }

    #[test]
    fn a_tab_that_fits_is_left_alone_and_one_too_wide_hands_tools_over_from_the_right() {
        assert!(fit(&[(200.0, 4), (300.0, 6)], 600.0).is_empty());
        // 518 wide in 480: the last panel's tools are 50 each, and handing one over is room enough.
        assert_eq!(fit(&[(200.0, 4), (300.0, 6)], 480.0), [("p1".into(), Fit::Show(5))]);
        assert_eq!(fit(&[(200.0, 4), (300.0, 6)], 380.0), [("p1".into(), Fit::Show(3))]);
    }

    #[test]
    fn a_panel_that_one_tool_wont_fit_folds_and_the_one_before_it_makes_the_rest_of_the_room() {
        // 368 wide in 250: the last, with one tool left, would still be 18 over; folded it makes 78 of the 118.
        assert_eq!(fit(&[(200.0, 4), (150.0, 3)], 250.0), [("p1".into(), Fit::Fold), ("p0".into(), Fit::Show(3))]);
        // A panel of one tool, no wider than a folded one, is passed over.
        assert_eq!(fit(&[(200.0, 4), (60.0, 1)], 200.0), [("p0".into(), Fit::Show(2))]);
    }

    #[test]
    fn a_kept_panel_last_on_its_tab_stays_whole_while_the_ones_before_it_make_room() {
        let list = [(200.0, 4), (150.0, 3), (160.0, 2)];
        let whole = list.iter().map(|(w, _)| w + RULE).sum();
        let mut seen = panels(&list);
        seen[2].kept = true;
        // 537 wide in 480: the middle one hands two tools over (50 each), the kept last one is whole.
        assert_eq!(fits(&seen, whole, 480.0), [("p1".into(), Fit::Show(1))]);
        // In 330: the middle one folds and the first hands over the rest it must; the kept one still whole.
        assert_eq!(fits(&seen, whole, 330.0), [("p1".into(), Fit::Fold), ("p0".into(), Fit::Show(1))]);
        // With no room even then, it gives way last.
        assert!(fits(&seen, whole, 150.0).iter().any(|(n, _)| n == "p2"));
    }

    #[test]
    fn a_yielding_panel_hands_its_tools_over_before_the_ones_after_it() {
        let list = [(200.0, 4), (150.0, 3), (160.0, 2)];
        let whole = list.iter().map(|(w, _)| w + RULE).sum();
        let mut seen = panels(&list);
        seen[0].yields = true;
        // 537 wide in 480: the first one, yielding, hands one tool over (50 each); the others are left alone.
        assert_eq!(fits(&seen, whole, 480.0), [("p0".into(), Fit::Show(2))]);
        // In 330 (207 over): it keeps its first tool (150 made), and the last panel makes the rest.
        assert_eq!(fits(&seen, whole, 330.0), [("p0".into(), Fit::Show(1)), ("p2".into(), Fit::Show(1))]);
    }

    #[test]
    fn a_fold_the_panels_before_it_made_unneeded_is_given_back() {
        // 400 wide in 355, 45 over: the last panel, of one tool, folds (10 made), then the first must hand two tools over (60
        // made) all the same, which is room enough without the fold.
        assert_eq!(fit(&[(300.0, 10), (82.0, 1)], 355.0), [("p0".into(), Fit::Show(8))]);
        // 95 over: the fold and three tools (100 made) do it, where the first alone would hand four over; the fold stays.
        assert_eq!(fit(&[(300.0, 10), (82.0, 1)], 305.0), [("p1".into(), Fit::Fold), ("p0".into(), Fit::Show(7))]);
    }

    #[test]
    fn a_fold_the_room_cant_hold_whole_is_given_back_in_part() {
        // 380 wide in 330, 50 over: the last panel (two tools, 114 wide, 83 showing its first) can't make it alone and folds
        // (42 made); the first then hands a tool over (about 30 made), some 22 more than was left: room for the last panel's
        // first tool (11 wider than its fold), not for both.
        let mut seen = panels(&[(266.0, 9), (114.0, 2)]);
        seen[1].widths = vec![83.0, 83.0];
        assert_eq!(fits(&seen, 380.0, 330.0), [("p1".into(), Fit::Show(1)), ("p0".into(), Fit::Show(8))]);
    }

    #[test]
    fn a_kept_panel_still_folds_last_when_a_fold_is_given_back() {
        // 390 wide in 180, 210 over: the first folds (18 made), then the kept one must too (228 made), and the first's fold is
        // not given back while the kept one stays folded.
        let list = [(90.0, 1), (300.0, 1)];
        let mut seen = panels(&list);
        seen[1].kept = true;
        assert_eq!(fits(&seen, 390.0, 180.0), [("p0".into(), Fit::Fold), ("p1".into(), Fit::Fold)]);
        // In 220, 170 over: the kept one's fold alone is room enough, but it is asked last; both stay folded, as the rule says.
        assert_eq!(fits(&seen, 390.0, 220.0), [("p0".into(), Fit::Fold), ("p1".into(), Fit::Fold)]);
    }

    /// One frame of a tab `room` wide drawn in `ctx`, under `key`: each panel of `panels` a name and its tools' widths. Whether
    /// the frame asked for another.
    fn frame(ctx: &egui::Context, key: Id, room: f32, panels: &[(&str, &[f32])]) -> bool {
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(4000.0, 400.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            tab(ui, key, room, |ui| {
                ui.horizontal(|ui| {
                    for (name, tools) in panels {
                        panel(ui, name, |ui| {
                            for w in tools.iter() {
                                if handed_over(ui).is_none() {
                                    ui.allocate_exact_size(vec2(*w, 60.0), Sense::click());
                                }
                            }
                        });
                    }
                });
            });
        });
        out.textures_delta.clear();
        out.viewport_output.values().any(|v| v.repaint_delay.is_zero())
    }

    /// What `tab` keeps for `key`: the panels it saw, the tab's whole width, and what the panels' estimates missed.
    fn kept_for(ctx: &egui::Context, key: Id) -> (Vec<Seen>, f32, f32) {
        let (seen, whole, missed, _): (Vec<Seen>, f32, f32, f32) = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
        (seen, whole, missed)
    }

    /// The panels as `frames` frames of `room` leave them: their names and how wide each was drawn.
    fn settle(ctx: &egui::Context, key: Id, room: f32, panels: &[(&str, &[f32])], frames: usize) -> Vec<(String, f32)> {
        for _ in 0..frames {
            frame(ctx, key, room, panels);
        }
        kept_for(ctx, key).0.into_iter().map(|p| (p.name, p.drawn)).collect()
    }

    /// How wide the tab of `panels` is with every tool shown, and how wide each panel is.
    fn natural(panels: &[(&str, &[f32])]) -> (f32, Vec<f32>) {
        let ctx = egui::Context::default();
        let key = Id::new("measure");
        frame(&ctx, key, 10_000.0, panels);
        let (seen, whole, _) = kept_for(&ctx, key);
        (whole, seen.iter().map(|p| p.natural).collect())
    }

    #[test]
    fn a_folded_panel_takes_no_more_room_than_its_button() {
        let panels: &[(&str, &[f32])] = &[("Wide", &[50.0, 50.0, 50.0, 50.0]), ("Last", &[120.0])];
        let (whole, each) = natural(panels);
        // Room for the tab with Last folded, and 2 to spare: Last folds, and Wide keeps all its tools.
        let room = whole - (each[1] - FOLDED) + 2.0 + SLACK;
        let ctx = egui::Context::default();
        let key = Id::new("tab");
        let drawn = settle(&ctx, key, room, panels, 8);
        assert_eq!(drawn, [("Wide".into(), each[0]), ("Last".into(), FOLDED)]);
        // Its width with every tool shown is what it was, and nothing was missed.
        let (_, after, missed) = kept_for(&ctx, key);
        assert_eq!((after, missed), (whole, 0.0));
    }

    #[test]
    fn tools_are_handed_over_by_their_own_widths_and_their_panels_name_so_the_first_fit_fits() {
        // A big tool then small ones: their average would make more of handing the small ones over than they make.
        let uneven: &[(&str, &[f32])] = &[("Uneven", &[100.0, 20.0, 20.0, 20.0])];
        // A name wider than its tools: handing them over makes no room at all, so it folds.
        let named: &[(&str, &[f32])] = &[("Head", &[60.0, 60.0]), ("A panel named far wider than its tools", &[20.0, 20.0, 20.0])];
        for panels in [uneven, named] {
            let (whole, _) = natural(panels);
            let room = whole - 30.0 + SLACK;
            let ctx = egui::Context::default();
            let key = Id::new("tab");
            // The first frame lays out every tool; the second fits, missing nothing.
            frame(&ctx, key, room, panels);
            frame(&ctx, key, room, panels);
            let (seen, _, missed) = kept_for(&ctx, key);
            assert_eq!(missed, 0.0, "{seen:?}");
            let made: f32 = seen.iter().map(|p| p.natural - p.drawn).sum();
            assert!(made >= 30.0, "{seen:?}");
        }
    }

    #[test]
    fn what_one_tabs_panels_missed_is_not_asked_of_the_next_ones_drawn_under_its_key() {
        // A panel whose tools grow wider once it has handed one over (their labels changed): what it was judged to make falls
        // short, and the tab asks it again, missing what it misjudged.
        let narrow: &[(&str, &[f32])] = &[("Grows", &[60.0, 60.0, 60.0, 60.0])];
        let wider: &[(&str, &[f32])] = &[("Grows", &[80.0, 80.0, 80.0, 80.0])];
        let (whole, _) = natural(narrow);
        let room = whole - 50.0 + SLACK;
        let ctx = egui::Context::default();
        let key = Id::new("context tab");
        settle(&ctx, key, room, narrow, 1);
        settle(&ctx, key, room, wider, 8);
        assert!(kept_for(&ctx, key).2 > 0.0, "the panel was misjudged");
        // Another selection's panel under the same key, at the same room, with 2 to spare: left whole.
        let (probe, _) = natural(&[("Other", &[100.0])]);
        let tool = [100.0 + room - SLACK - 2.0 - probe];
        let other: &[(&str, &[f32])] = &[("Other", &tool)];
        let (w, each) = natural(other);
        assert!(w <= room - SLACK, "{w} fits in {}", room - SLACK);
        assert_eq!(settle(&ctx, key, room, other, 8), [("Other".into(), each[0])]);
    }

    #[test]
    fn a_tab_too_wide_for_any_fit_settles_and_stops_asking_for_frames() {
        let panels: &[(&str, &[f32])] = &[("A", &[60.0, 60.0, 60.0]), ("B", &[90.0, 90.0]), ("C", &[150.0])];
        let ctx = egui::Context::default();
        let key = Id::new("tab");
        let mut last = Vec::new();
        let mut asked = true;
        for _ in 0..16 {
            asked = frame(&ctx, key, 120.0, panels);
            last = kept_for(&ctx, key).0;
        }
        assert!(!asked, "no frame more once it can do no better");
        // Every panel folded, and the next frame draws them as this one did.
        assert!(last.iter().all(|p| p.drawn == FOLDED), "{last:?}");
        frame(&ctx, key, 120.0, panels);
        assert_eq!(kept_for(&ctx, key).0, last);
    }
}
