//! The ribbon's panels: each panel's tools over its name, and the name a button that lists them all. A tab too wide for the window makes room from the right: a panel hands its last tools over to its list,
//! and when one tool isn't room enough it folds to one button that lists them; what still doesn't fit scrolls sideways.

use std::cell::{Cell, RefCell};

use egui::{Align2, FontId, Id, Popup, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::lang::t;
use crate::presets::Scale;
use crate::style::pal;

/// How wide a folded panel is.
const FOLDED: f32 = 72.0;
/// How tall a big ribbon button is (`big_button`).
const BIG: f32 = 60.0;
/// The hairline between panels, with its room.
const RULE: f32 = 9.0;
/// What's kept clear at the tab's end, so a fit judged on last frame's widths isn't a pixel over.
const SLACK: f32 = 8.0;

/// How a panel fits: all its tools shown, its first so many, or folded to one button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fit {
    Show(usize),
    Fold,
}

/// A panel as the last frame drew it: its name, how wide it is with every tool shown and as it was drawn, how many tools it
/// has.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Seen {
    name: String,
    natural: f32,
    drawn: f32,
    tools: usize,
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
    /// How the panels of the tab being drawn fit, by name; one not named shows all its tools.
    static FITS: RefCell<Vec<(String, Fit)>> = const { RefCell::new(Vec::new()) };
    /// The panels of the tab being drawn, as they're drawn.
    static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
    /// The tools of the panel being drawn so far, and how many it shows.
    static TOOLS: Cell<(usize, usize)> = const { Cell::new((0, usize::MAX)) };
    /// Where the last panel drawn ends.
    static END: Cell<f32> = const { Cell::new(0.0) };
    /// The tab being drawn: the room it has, where it starts, and how much of what it has drawn is gaps (`push_right`'s).
    static LAID: Cell<(f32, f32, f32)> = const { Cell::new((0.0, 0.0, 0.0)) };
    /// A tool's choices were asked for (a shortcut's) while it has no button, being handed over or folded: its panel opens its list.
    static OPEN_LIST: Cell<bool> = const { Cell::new(false) };
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
    (n >= shown).then(|| ui.allocate_response(Vec2::ZERO, Sense::hover()))
}

/// Draw a tab's panels, `add` drawing them, in `room`: those it can't hold make room from the right, as the last frame's
/// widths say they must.
pub(crate) fn tab(ui: &mut Ui, key: Id, room: f32, add: impl FnOnce(&mut Ui)) {
    // The last frame's panels, how wide the whole tab would have been with every tool shown, and what the tools' average widths
    // missed at the room it had then: asked of the panels again, so a tab they misjudged still ends up fitting.
    let (seen, whole, missed, then): (Vec<Seen>, f32, f32, f32) = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
    let missed = if (then - room).abs() < 0.5 { missed } else { 0.0 };
    let fitted = !seen.is_empty();
    FITTED.with(|f| f.set(fitted));
    FITS.with(|f| *f.borrow_mut() = fits(&seen, whole + missed, room - SLACK));
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
    let missed = (missed + over).min(whole);
    if over > 0.0 && missed < whole {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, (seen, whole, missed, room)));
    FITS.with(|f| f.borrow_mut().clear());
    FITTED.with(|f| f.set(true));
}

/// Pushes what is drawn next to the right end of the tab being drawn, a panel `width` wide (one that stands apart, as Edit Mode's
/// does). The gap is not width the tab needs, so it never makes the panels before it hand their tools over.
pub(crate) fn push_right(ui: &mut Ui, width: f32) {
    let (room, left, gaps) = LAID.with(Cell::get);
    let gap = (room - (END.with(Cell::get) - left - gaps) - width).max(0.0);
    ui.add_space(gap);
    LAID.with(|l| l.set((room, left, gaps + gap)));
}

/// How the panels `seen` fit, for a tab `whole` wide with every tool shown, in `room`: from the last panel back, each hands
/// over as few tools as make it fit, or, with one left not enough, folds and the next is asked.
pub(crate) fn fits(seen: &[Seen], whole: f32, room: f32) -> Vec<(String, Fit)> {
    let mut over = whole - room;
    let mut fits = Vec::new();
    for p in seen.iter().rev() {
        if over <= 0.0 {
            break;
        }
        let each = if p.tools > 0 { p.natural / p.tools as f32 } else { 0.0 };
        if let Some(k) = (1..p.tools).rev().find(|k| (p.tools - k) as f32 * each >= over) {
            fits.push((p.name.clone(), Fit::Show(k)));
            break;
        }
        if p.natural > FOLDED {
            over -= p.natural - FOLDED;
            fits.push((p.name.clone(), Fit::Fold));
        }
    }
    fits
}

/// A ribbon panel: its tools over its name, which lists them; or, folded, one button that does.
pub(crate) fn panel(ui: &mut Ui, title: &str, mut add: impl FnMut(&mut Ui)) {
    // A panel the project's scale leaves out: not drawn, on the ribbon or in the Tools palette.
    if scale_hides(title) {
        return;
    }
    if listing() {
        // Inside another panel's list (a folded panel's): the tools as rows; in the Tools palette, under the panel's name.
        if in_palette() {
            // Painted: a panel may be named as a tab is (the Structure tab's Structure panel), and its tools say the rest.
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
            let at = r.left_bottom() - vec2(0.0, 6.0);
            ui.painter().text(at, Align2::LEFT_BOTTOM, t(title), FontId::proportional(11.5), pal().muted);
        }
        add(ui);
        return;
    }
    let name = t(title);
    let id = ui.id().with(("ribbon-panel", title));
    let fit = FITS.with(|f| f.borrow().iter().find(|(n, _)| n == title).map(|(_, fit)| *fit));
    let shows = match fit {
        Some(Fit::Show(k)) => k,
        _ => usize::MAX,
    };
    TOOLS.with(|t| t.set((0, shows)));
    let drawn = if fit == Some(Fit::Fold) {
        let r = folded_button(ui, name);
        // Counted, not drawn.
        TOOLS.with(|t| t.set((0, 0)));
        ui.scope(|ui| {
            ui.set_invisible();
            // Counted at no size and no spacing either: else each handed-over tool would widen the folded button's room.
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            add(ui);
        });
        open_if_asked(ui, &r);
        list(&r, &mut add);
        r.rect.width()
    } else {
        let shown = ui.vertical(|ui| {
            let row = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // Rev's bands make one line along the ribbon's foot: each panel as tall as a big button.
                if crate::style::rev() {
                    ui.set_min_height(BIG);
                }
                add(ui);
            });
            let font = FontId::proportional(11.5);
            let text = ui.painter().layout_no_wrap(name.to_string(), font.clone(), pal().muted).size().x;
            let (r, title_row) = ui.allocate_exact_size(vec2(row.response.rect.width().max(text + 24.0), 16.0), Sense::click());
            if crate::style::rev() {
                // Rev's name is on a band as wide as the panel, along the ribbon's foot.
                ui.painter().rect_filled(r, 0.0, if title_row.hovered() { pal().hover_fill } else { pal().bar });
            } else if title_row.hovered() {
                ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(text + 22.0, 16.0)), 4.0, pal().hover_fill);
            }
            ui.painter().text(r.center() - vec2(5.0, 0.0), Align2::CENTER_CENTER, name, font, pal().muted);
            caret(ui, pos2(r.center().x + text / 2.0 + 3.0, r.center().y), pal().muted);
            let label = format!("{name} ▾");
            title_row.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
            open_if_asked(ui, &title_row);
            list(&title_row.on_hover_text(name), &mut add);
        });
        shown.response.rect.width()
    };
    let tools = TOOLS.with(Cell::get).0;
    TOOLS.with(|t| t.set((0, usize::MAX)));
    // How wide it is with every tool shown: as drawn when it was, else as it was then.
    let natural = match fit {
        None => {
            ui.ctx().data_mut(|d| d.insert_temp(id, drawn));
            drawn
        }
        Some(_) => ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(drawn),
    };
    SEEN.with(|s| s.borrow_mut().push(Seen { name: title.to_string(), natural, drawn, tools }));
    // A hairline between panels, short of the card's edges (Rev's runs its height, down to the bands).
    let (rule, _) = ui.allocate_exact_size(vec2(RULE, ui.available_height().max(70.0)), Sense::hover());
    let short = if crate::style::rev() { 0.0 } else { 4.0 };
    ui.painter().vline(rule.center().x, rule.y_range().shrink(short), Stroke::new(1.0, pal().border));
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
    let corner = crate::style::corner(6.0);
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
        list.iter().enumerate().map(|(i, &(w, n))| Seen { name: format!("p{i}"), natural: w, drawn: w, tools: n }).collect()
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
}
