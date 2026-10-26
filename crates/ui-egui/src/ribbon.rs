//! The ribbon's panels: each panel's tools over its name, and the name a button that lists them all. A tab too wide for the window makes room from the right: a panel hands its last tools over to its list,
//! and when one tool isn't room enough it folds to one button that lists them; what still doesn't fit scrolls sideways.

use std::cell::{Cell, RefCell};

use egui::{Align2, FontId, Id, Popup, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::lang::t;
use crate::style::pal;

/// How wide a folded panel is.
const FOLDED: f32 = 72.0;
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
    /// How the panels of the tab being drawn fit, by name; one not named shows all its tools.
    static FITS: RefCell<Vec<(String, Fit)>> = const { RefCell::new(Vec::new()) };
    /// The panels of the tab being drawn, as they're drawn.
    static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
    /// The tools of the panel being drawn so far, and how many it shows.
    static TOOLS: Cell<(usize, usize)> = const { Cell::new((0, usize::MAX)) };
    /// Where the last panel drawn ends.
    static END: Cell<f32> = const { Cell::new(0.0) };
}

/// Whether the tools drawn now go in a panel's list.
pub(crate) fn listing() -> bool {
    LISTING.with(Cell::get)
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
    // The last frame's panels, and how wide the whole tab would have been with every tool shown.
    let (seen, whole): (Vec<Seen>, f32) = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
    FITS.with(|f| *f.borrow_mut() = fits(&seen, whole, room - SLACK));
    SEEN.with(|s| s.borrow_mut().clear());
    let left = ui.cursor().left();
    END.with(|e| e.set(left));
    add(ui);
    let drawn = END.with(Cell::get) - left;
    let seen = SEEN.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let whole = drawn + seen.iter().map(|p| (p.natural - p.drawn).max(0.0)).sum::<f32>();
    ui.ctx().data_mut(|d| d.insert_temp(key, (seen, whole)));
    FITS.with(|f| f.borrow_mut().clear());
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
    if listing() {
        // Inside another panel's list (a folded panel's): the tools as rows.
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
            add(ui);
        });
        list(&r, &mut add);
        r.rect.width()
    } else {
        let shown = ui.vertical(|ui| {
            let row = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                add(ui);
            });
            let font = FontId::proportional(11.5);
            let text = ui.painter().layout_no_wrap(name.to_string(), font.clone(), pal().muted).size().x;
            let (r, title_row) = ui.allocate_exact_size(vec2(row.response.rect.width().max(text + 24.0), 16.0), Sense::click());
            if title_row.hovered() {
                ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(text + 22.0, 16.0)), 4.0, pal().hover_fill);
            }
            ui.painter().text(r.center() - vec2(5.0, 0.0), Align2::CENTER_CENTER, name, font, pal().muted);
            caret(ui, pos2(r.center().x + text / 2.0 + 3.0, r.center().y), pal().muted);
            let label = format!("{name} ▾");
            title_row.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
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
    // A hairline between panels, short of the card's edges.
    let (rule, _) = ui.allocate_exact_size(vec2(RULE, ui.available_height().max(70.0)), Sense::hover());
    ui.painter().vline(rule.center().x, rule.y_range().shrink(4.0), Stroke::new(1.0, pal().border));
    END.with(|e| e.set(rule.right()));
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
    if r.hovered() || open {
        ui.painter().rect(rect.shrink2(vec2(0.0, 2.0)), 6.0, pal().hover_fill, Stroke::NONE, StrokeKind::Inside);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 6.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
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
