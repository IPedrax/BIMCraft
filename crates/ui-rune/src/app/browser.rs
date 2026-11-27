//! The Project Browser's handlers on `App`: a press on a row (a view opens, or several are picked together; a type opens its Type
//! Properties; a linked model is selected), the tab, and the right-click menu with what its items do. What the browser lists, and the
//! search, are `views.rs`; the elements are `ui/browser.rs`.

use bimcraft_core::ids::Id;
use serde_json::json;

use super::App;
use crate::context::{self, Action, Item};
use crate::lang::{t, tf};
use crate::typeprops::TypeKey;
use crate::views::{self, Pick, Tab, Target, ViewKind};

impl App {
    /// The text typed in the browser's search.
    pub fn views_find_text(&self) -> &str {
        &self.views_find.text
    }

    pub fn set_browser_tab(&mut self, tab: Tab) {
        self.views.set_tab(tab);
    }

    /// A press on a row of the browser, with what was held. A view opens, and is the one picked; with Ctrl or Shift held views are picked
    /// together instead, and none opens. A type of a family opens its Type Properties; a linked model is selected.
    pub fn press_row(&mut self, pick: &Pick, kind: ViewKind, shift: bool, ctrl: bool) {
        match pick {
            Pick::Open if shift || ctrl => {
                let order = self.views.order(&self.views_find.text);
                self.views.pick(kind, shift, ctrl, &order, self.view);
            }
            Pick::Open => {
                self.views.pick_one(kind);
                self.open_view(kind);
            }
            Pick::Type(category, name) => {
                self.open_type(TypeKey { category: category.clone(), name: name.clone() });
            }
            Pick::Select(id) => self.select(vec![*id]),
        }
    }

    /// A right click at window point `x`, `y` over `target` opens the browser's menu there; the plan's menu closes.
    pub fn open_browser_menu(&mut self, target: Target, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.close_menu();
            self.views.menu = Some((x, y, target));
            self.views.rev += 1;
        }
    }

    pub fn close_browser_menu(&mut self) {
        if self.views.menu.take().is_some() {
            self.views.rev += 1;
        }
    }

    /// What the browser's menu offers over what the click was on.
    pub fn browser_menu_items(&self) -> Vec<Vec<Item>> {
        match &self.views.menu {
            Some((_, _, target)) => {
                let mut groups = context::browser_items(&self.session.model, target);
                // A plan or a section can go under the plan on screen, and that plan's underlay can be taken off: before the folding items.
                if let Target::View(kind @ (ViewKind::Plan(_) | ViewKind::Section(_))) = target {
                    let item = |action, enabled| Item { action, enabled };
                    let at = groups.len().saturating_sub(1);
                    groups.insert(
                        at,
                        vec![item(Action::ShowAsUnderlay, self.can_underlay(*kind)), item(Action::RemoveUnderlay, self.has_underlay())],
                    );
                }
                groups
            }
            None => Vec::new(),
        }
    }

    /// Does what an item of the browser's menu says, to what the click was over, and closes the menu.
    pub(super) fn act_browser(&mut self, action: Action) {
        let Some((_, _, target)) = self.views.menu.take() else { return };
        self.views.rev += 1;
        match (action, target) {
            (Action::ExpandAll, _) => self.views.fold_all(false),
            (Action::CollapseAll, _) => self.views.fold_all(true),
            (Action::Duplicate, Target::View(kind)) => self.duplicate_view(kind, "duplicate"),
            (Action::DuplicateDetailing, Target::View(kind)) => self.duplicate_view(kind, "detailing"),
            (Action::DuplicateDependent, Target::View(kind)) => self.duplicate_view(kind, "dependent"),
            (Action::DeleteView, Target::View(kind)) => {
                if let Some(id) = views::element_id(&self.session.model, kind) {
                    self.run("delete", &json!({"ids": [id]}));
                }
            }
            (Action::CreatePrintSet, Target::View(kind)) => self.create_print_set(kind),
            (Action::ShowAsUnderlay, Target::View(kind)) => {
                if let Some(source) = views::element_id(&self.session.model, kind) {
                    self.show_as_underlay(source);
                }
            }
            (Action::RemoveUnderlay, _) => {
                self.remove_underlay();
            }
            (Action::EditType, Target::Type(category, name)) => {
                self.open_type(TypeKey { category, name });
            }
            (Action::DeleteType, Target::Type(category, name)) => {
                self.run("delete_type", &json!({"category": category, "type": name}));
            }
            (Action::ReloadLink, Target::Link(id)) => {
                self.run("reload", &json!({"id": id}));
            }
            (Action::RemoveLink, Target::Link(id)) => {
                self.run("delete", &json!({"ids": [id]}));
            }
            _ => {}
        }
    }

    /// A new plan view of a plan, in a mode of the command's (`duplicate`, `detailing`, `dependent`), opened when it is made.
    fn duplicate_view(&mut self, kind: ViewKind, mode: &str) {
        let Some(id) = views::element_id(&self.session.model, kind) else { return };
        match self.session.execute("duplicate_view", &json!({"view": id, "mode": mode})) {
            Ok(reply) => {
                self.message = None;
                self.settle();
                if let Some(new) = reply["id"].as_u64() {
                    self.open_view(ViewKind::Plan(new));
                }
            }
            Err(why) => self.message = Some(why),
        }
    }

    /// A print set of the views picked together (when the click was over one of them), else of the one it was over. Named `Print Set n`.
    fn create_print_set(&mut self, kind: ViewKind) {
        let picked = if self.views.picked.len() > 1 && self.views.picked.contains(&kind) { self.views.picked.clone() } else { vec![kind] };
        let mut ids: Vec<Id> = Vec::new();
        for k in picked {
            if let Some(id) = views::element_id(&self.session.model, k)
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            self.message = Some(t("None of these can be printed yet.").to_string());
            return;
        }
        let sets = &self.session.model.print_sets;
        let name = (1..).map(|n| tf("Print Set {}", &[&n.to_string()])).find(|n| !sets.contains_key(n)).unwrap_or_default();
        self.run("print_set", &json!({"name": name, "views": ids}));
    }
}

#[cfg(test)]
mod tests;
