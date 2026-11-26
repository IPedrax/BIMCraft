//! The middle of the window: the canvas of the view on screen, one for each kind of view. A plan has the plan canvas; a section or an
//! elevation, a sheet and the 3D view have a picture made on a thread of their own; a schedule is a grid.

use rune::El;
use rune::tok::Theme;

use super::{plan, schedule, section, sheet, view3d};
use crate::app::App;
use crate::views::ViewKind;

pub fn canvas(s: &App, th: &Theme) -> El {
    match s.view {
        ViewKind::Nothing | ViewKind::Plan(_) | ViewKind::Ceiling(_) => plan::plan(s),
        ViewKind::Section(_) => section::view(s),
        ViewKind::Sheet(_) => sheet::view(s),
        ViewKind::View3D(_) => view3d::view(s),
        ViewKind::Schedule(_) | ViewKind::Table(_) => schedule::view(s, th),
    }
}

#[cfg(test)]
mod tests;
