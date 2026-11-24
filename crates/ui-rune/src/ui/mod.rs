//! The window's elements, each a function of the app's state: the plan, the Properties panel and the status bar. A handler here
//! only calls a method of `App`; what it does, and every change to the model, is `app`'s.

use rune::tok;
use rune::{Cx, El, Key, KeyEvent, Tx, col, row};

use crate::app::App;
use crate::profile::Profile;

mod browser;
mod plan;
mod properties;
mod ribbon;
mod status_bar;
mod tools;
mod topbar;

/// The Properties panel's width, points.
pub const PANEL_WIDTH: f32 = 300.0;

pub fn view(s: &App, _: &mut Cx<App>) -> El {
    let th = tok::theme(true);
    // The right-hand column both profiles share: the selected element's properties over the project's levels.
    let side = col().w(PANEL_WIDTH).h_full().child(properties::panel(s, &th)).child(browser::panel(s, &th));
    let middle = match s.profile {
        Profile::Bimcraft => row().grow(1.0).min_h(0.0).child(tools::palette(s, &th)).child(plan::plan(s)).child(side),
        Profile::Rev => row().grow(1.0).min_h(0.0).child(plan::plan(s)).child(side),
    };
    let top = match s.profile {
        Profile::Bimcraft => None,
        Profile::Rev => Some(ribbon::ribbon(s, &th)),
    };
    col().key("root").w_full().h_full().fill(th.bg).child(topbar::bar(s, &th)).children(top).child(middle).child(status_bar::bar(s, &th))
}

/// Delete deletes the selection; Ctrl+Z undoes, Ctrl+Shift+Z and Ctrl+Y do again (Cmd on a Mac).
pub fn on_key(s: &mut App, key: &KeyEvent, _: &mut Tx) -> bool {
    match (&key.key, key.mods.cmd, key.mods.shift) {
        (Key::Delete, false, _) => {
            s.delete_selection();
        }
        (Key::Command('z'), true, false) => {
            s.undo();
        }
        (Key::Command('z'), true, true) | (Key::Command('y'), true, _) => {
            s.redo();
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests;
