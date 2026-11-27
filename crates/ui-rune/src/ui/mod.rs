//! The window's elements, each a function of the app's state: the plan, the Properties panel and the status bar. A handler here
//! only calls a method of `App`; what it does, and every change to the model, is `app`'s.

use std::time::Duration;

use rune::tok;
use rune::{Cx, El, Key, KeyEvent, Tx, col, row};

use bimcraft_core::shortcuts;

use crate::app::App;
use crate::keys;
use crate::profile::Profile;

mod browser;
mod canvas;
mod dialog;
mod draw_bar;
mod export;
mod find;
mod menu;
mod plan;
mod properties;
mod ribbon;
mod schedule;
mod section;
mod settings;
mod sheet;
mod status_bar;
mod tempdim;
mod tools;
mod topbar;
mod types;
mod view3d;

/// The Properties panel's width, points.
pub const PANEL_WIDTH: f32 = 300.0;

pub fn view(s: &App, _: &mut Cx<App>) -> El {
    let th = tok::theme(s.dark());
    // The plan's right-click menu, and the Project Browser's, when one is open, put their elements in `layer`, which go over the whole window.
    let mut layer = Vec::new();
    // The right-hand column both profiles share: the selected element's properties over the project's views.
    let side = col().w(PANEL_WIDTH).h_full().child(properties::panel(s, &th)).child(browser::panel(s, &th, &mut layer));
    let canvas = menu::with_menu(s, canvas::canvas(s, &th), &mut layer);
    let middle = match s.profile {
        Profile::Bimcraft => row().grow(1.0).min_h(0.0).child(tools::palette(s, &th)).child(canvas).child(side),
        Profile::Rev => row().grow(1.0).min_h(0.0).child(canvas).child(side),
    };
    let top = match s.profile {
        Profile::Bimcraft => None,
        Profile::Rev => Some(ribbon::ribbon(s, &th)),
    };
    // A dialog that is open is a scrim and a panel over the whole window: absolute children of the root, so nothing wraps the plan.
    let mut dialogs = Vec::new();
    if s.dialogs.any() {
        dialog::layers(s, &rune_kit::Kit::neutral(s.dark()), &mut dialogs);
    }
    let body = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(th.bg)
        .on_layout(|s: &mut App, r| s.window_seen(r[2], r[3]))
        .child(topbar::bar(s, &th))
        .children(top)
        .child(middle)
        .child(status_bar::bar(s, &th))
        .children(dialogs);
    if layer.is_empty() { body } else { rune_kit::root(body, layer) }
}

/// Every frame: the plan on screen, which the camera moves through Rune's live offset and scale, is drawn exactly once the camera has
/// been still for `app::SETTLE`. True when it was, so the view runs again; else asks for a frame when it is due.
pub fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    s.tick(clock);
    let drawn = s.settle_if_still();
    if let Some(due) = s.settle_in() {
        tx.wake_after(Duration::from_secs_f64(due));
    }
    // The window's size is kept in the settings once it has held still: nothing the view reads changes, so no frame is asked for.
    s.save_window_if_still();
    if let Some(due) = s.window_due() {
        tx.wake_after(Duration::from_secs_f64(due));
    }
    // A view made on a thread of its own (a section): its picture comes in when it is made, and the window comes back for it. Whether it
    // is busy is read before the answer is taken: an answer is queued before the thread says it is through, so a thread that is not
    // busy has its answer in the queue for this very call, and one that is gets a frame to come back in.
    let busy = s.views_busy();
    let made = s.poll_views();
    if busy {
        tx.wake_after(Duration::from_millis(16));
    }
    // The letters of a shortcut that waited too long are dropped, and the status bar stops showing them.
    let typed = s.expire_typed();
    if let Some(due) = s.typed_in() {
        tx.wake_after(Duration::from_secs_f64(due));
    }
    drawn || made || typed
}

/// Ctrl+K opens Find (or closes it) and Ctrl+, the settings. Delete deletes the selection; Ctrl+Z undoes, Ctrl+Shift+Z and Ctrl+Y do again
/// (Cmd on a Mac); Tab and Shift+Tab step through what lies under the pointer; Escape drops the letters of a shortcut that wait, then puts
/// the tool down, then clears the selection; with a tool in hand Enter finishes what it has picked and Backspace gives the last point back.
/// With a dialog open the keys are its. Then Revit's two-letter shortcuts, typed plainly (`WA`), and the chords the core's table names
/// (`Ctrl+0`, `F3`, `Ctrl+S`), in every view. A key none of these takes goes on to the window.
pub fn on_key(s: &mut App, key: &KeyEvent, tx: &mut Tx) -> bool {
    s.tick(tx.clock());
    if key.mods.cmd && matches!(key.key, Key::Command('k')) {
        s.toggle_find();
        return true;
    }
    if key.mods.cmd && matches!(key.key, Key::Command(',')) {
        s.toggle_settings();
        return true;
    }
    // With a dialog open the keys are its.
    if s.dialogs.any() {
        return false;
    }
    match (&key.key, key.mods.cmd, key.mods.shift) {
        (Key::Tab, false, back) => return s.step(back),
        (Key::Escape, false, _) => return s.drop_typed() || s.escape(),
        (Key::Enter, false, _) => return s.finish_tool(),
        (Key::Backspace, false, _) => return s.back_point(),
        (Key::Delete, false, _) => {
            s.delete_selection();
        }
        (Key::Command('z'), true, false) => {
            s.undo();
        }
        (Key::Command('z'), true, true) | (Key::Command('y'), true, _) => {
            s.redo();
        }
        // A letter or digit typed plainly, with no key held but Shift, is a shortcut's.
        (Key::Text(text), false, _) if !key.mods.alt && !key.mods.ctrl && !key.mods.logo => {
            return match (text.chars().next(), text.chars().count()) {
                (Some(c), 1) => s.type_key(c),
                _ => false,
            };
        }
        _ => return chord(s, key, tx),
    }
    true
}

/// A chord the core's table names: the ones that ask for a file are the top bar's and the export's, the rest the app's own.
fn chord(s: &mut App, key: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(action) = keys::written(&key.key, key.mods).and_then(|w| shortcuts::chord(&w)) else { return false };
    match action {
        "cmd.save" => topbar::save(s, tx),
        "cmd.open" => topbar::open(tx),
        "ui.file.save_as" => topbar::save_as(tx),
        "ui.file.export_pdf" => export::ask(s, tx, "pdf"),
        other => {
            s.run_action(other);
        }
    }
    true
}

#[cfg(test)]
mod dialog_tests;
#[cfg(test)]
mod keys_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tooling_tests;
