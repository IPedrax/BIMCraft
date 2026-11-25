//! The window's elements, each a function of the app's state: the plan, the Properties panel and the status bar. A handler here
//! only calls a method of `App`; what it does, and every change to the model, is `app`'s.

use std::time::Duration;

use rune::tok;
use rune::{Cx, El, Key, KeyEvent, Tx, col, row};

use crate::app::App;
use crate::profile::Profile;

mod browser;
mod dialog;
mod find;
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
    // A dialog that is open is a scrim and a panel over the whole window: absolute children of the root, so nothing wraps the plan.
    let mut layer = Vec::new();
    if s.dialogs.any() {
        dialog::layers(s, &rune_kit::Kit::neutral(true), &mut layer);
    }
    col()
        .key("root")
        .w_full()
        .h_full()
        .fill(th.bg)
        .child(topbar::bar(s, &th))
        .children(top)
        .child(middle)
        .child(status_bar::bar(s, &th))
        .children(layer)
}

/// Every frame: the plan on screen, which the camera moves through Rune's live offset and scale, is drawn exactly once the camera has
/// been still for `app::SETTLE`. True when it was, so the view runs again; else asks for a frame when it is due.
pub fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    s.tick(clock);
    let drawn = s.settle_if_still();
    if let Some(due) = s.settle_in() {
        tx.wake_after(Duration::from_secs_f64(due));
    }
    drawn
}

/// Ctrl+K opens Find (or closes it). Delete deletes the selection; Ctrl+Z undoes, Ctrl+Shift+Z and Ctrl+Y do again (Cmd on a Mac); Tab and Shift+Tab step through what
/// lies under the pointer; Escape puts the tool down, then clears the selection. A key none of these takes goes on to the window.
pub fn on_key(s: &mut App, key: &KeyEvent, _: &mut Tx) -> bool {
    if key.mods.cmd && matches!(key.key, Key::Command('k')) {
        s.toggle_find();
        return true;
    }
    // With a dialog open the keys are its.
    if s.dialogs.any() {
        return false;
    }
    match (&key.key, key.mods.cmd, key.mods.shift) {
        (Key::Tab, false, back) => return s.step(back),
        (Key::Escape, false, _) => return s.escape(),
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
mod dialog_tests;
#[cfg(test)]
mod tests;
