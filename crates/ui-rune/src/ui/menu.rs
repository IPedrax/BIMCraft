//! The right-click menu over the plan: rune_kit's `Menu` at the window point the secondary press came from, its items what
//! `App::menu_items` offers and its choices `App::act`. Nothing is built while the menu is closed, so the plan is as it was.

use rune::El;
use rune_kit::{Kit, Menu, MenuItem};

use crate::app::App;
use crate::lang::t;

/// `plan`, with the menu open over it when there is one; the menu's own elements go in `layer`, to be mounted over the window.
pub fn with_menu(s: &App, plan: El, layer: &mut Vec<El>) -> El {
    let Some((x, y)) = s.tooling.menu else { return plan };
    let kit = Kit::neutral(s.dark());
    let mut menu = Menu::new("plan-menu").trigger(plan).at(x, y).open(true).on_close(|s: &mut App| s.close_menu());
    for (n, group) in s.menu_items().into_iter().enumerate() {
        if n > 0 {
            menu = menu.separator();
        }
        for item in group {
            let action = item.action;
            let mut entry =
                MenuItem::new(t(action.label()).to_string()).disabled(!item.enabled).on_select(move |s: &mut App| s.act(action));
            if let Some(key) = action.key() {
                entry = entry.shortcut(key);
            }
            menu = menu.item(entry);
        }
    }
    menu.build(&kit, layer)
}
