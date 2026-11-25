//! The middle of the window: the canvas of the view on screen. A plan has the plan canvas. The other kinds of view arrive one at a
//! time (sections and elevations, schedules, sheets, the 3D view); until one does, its canvas only names what was opened.

use rune::tok::{Theme, type_};
use rune::{El, col, text};

use super::{plan, schedule, section};
use crate::app::App;
use crate::lang::t;
use crate::views::ViewKind;

pub fn canvas(s: &App, th: &Theme) -> El {
    match s.view {
        ViewKind::Nothing | ViewKind::Plan(_) | ViewKind::Ceiling(_) => plan::plan(s),
        ViewKind::Section(_) => section::view(s),
        ViewKind::Schedule(_) | ViewKind::Table(_) => schedule::view(s, th),
        _ => pending(s, th),
    }
}

/// A view this window does not draw yet: named, so a press in the browser is seen to have opened it.
fn pending(s: &App, th: &Theme) -> El {
    col()
        .key("view-pending")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(th.bg)
        .center()
        .gap(6.0)
        .child(text(s.views.title.clone()).font(type_::TITLE).color(th.text))
        .child(text(t("This view is not drawn in this window yet.")).font(type_::BODY).color(th.text_secondary))
}

#[cfg(test)]
mod tests;
