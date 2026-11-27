//! Groups in the window, after egui's `groups.rs`: Create Group (GP) of a selection all of one kind, Ungroup (UG), Edit Group (GE) with Add,
//! Remove, Finish and Cancel, and Exclude (EM) and Restore All Excluded (RA). The core makes and keeps them (`bimcraft_core::groups`, the
//! group commands); this reads the selection for what it can be asked, runs the command and selects what it made. They are items of the plan's
//! menu (`App::group_items`) and what the Revit shortcuts type (`app/keys.rs`). Not here yet: Place Model Group and Place Detail Group, Repeating
//! Detail, Add and Remove as picks on the canvas (a click takes an element in or out), and the question a change that would break a group asks.
//! A child of `app` so it reaches the private handlers.

use bimcraft_core::groups::{self as core_groups, Kind};
use bimcraft_core::ids::Id;
use serde_json::{Value, json};

use super::App;
use crate::context::{Action, Item};
use crate::lang::t;

impl App {
    /// Whether the selection makes one group: not empty, none of it in a group already, all of one kind.
    pub fn groupable(&self) -> bool {
        let m = &self.session.model;
        !self.selection.is_empty()
            && !self.selection.iter().any(|id| core_groups::group_of(m, *id).is_some())
            && core_groups::kind_of_all(m, &self.selection).is_ok()
    }

    /// The group instances selected.
    pub fn groups_selected(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.selection.iter().copied().filter(|id| m.elements.get(*id).and_then(core_groups::instance).is_some()).collect()
    }

    /// The selected members of groups, other than the group being edited: what Exclude takes out of its group here.
    pub fn members_selected(&self) -> Vec<Id> {
        let m = &self.session.model;
        let editing = self.session.editing_group();
        self.selection.iter().copied().filter(|id| core_groups::group_of(m, *id).is_some_and(|g| Some(m.guid(g)) != editing)).collect()
    }

    /// The selected groups that have members excluded, for Restore All Excluded.
    pub fn groups_excluding(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.groups_selected()
            .into_iter()
            .filter(|id| m.elements.get(*id).and_then(core_groups::instance).is_some_and(|i| i.members.iter().any(|s| s.excluded)))
            .collect()
    }

    /// The group in Edit Group, if one is.
    pub fn editing_group(&self) -> Option<Id> {
        self.session.model.elements.id(self.session.editing_group()?)
    }

    /// What the plan's menu offers about groups now, one group of items: while one is edited, Add, Remove, Finish and Cancel; else what the
    /// selection can be (made a group, ungrouped, edited, excluded from its group, restored).
    pub fn group_items(&self) -> Vec<Item> {
        let on = |action| Item { action, enabled: true };
        if self.editing_group().is_some() {
            let some = !self.selection.is_empty();
            return vec![
                Item { action: Action::AddToGroup, enabled: some },
                Item { action: Action::RemoveFromGroup, enabled: some },
                on(Action::FinishGroup),
                on(Action::CancelGroup),
            ];
        }
        let mut items = Vec::new();
        if self.groupable() {
            items.push(on(Action::CreateGroup));
        }
        let groups = self.groups_selected();
        if !groups.is_empty() {
            items.push(on(Action::Ungroup));
        }
        if groups.len() == 1 {
            items.push(on(Action::EditGroup));
        }
        if !self.members_selected().is_empty() {
            items.push(on(Action::ExcludeMember));
        }
        if !self.groups_excluding().is_empty() {
            items.push(on(Action::RestoreExcluded));
        }
        items
    }

    /// `name` through the engine and what it replied; a refusal is the status bar's message and the model stays as it was.
    fn reply(&mut self, name: &str, params: &Value) -> Option<Value> {
        match self.session.execute(name, params) {
            Ok(reply) => {
                self.message = None;
                self.settle();
                Some(reply)
            }
            Err(why) => {
                self.message = Some(why);
                None
            }
        }
    }

    /// Create Group: the selection, all model elements or all of one view's annotation, made one group, selected. A selection of both kinds
    /// asks to be made of one; a selection with some of one kind only makes a group of that kind and leaves the rest out.
    pub fn create_group(&mut self) -> bool {
        if self.selection.is_empty() {
            self.message = Some(t("select the elements to group first").to_string());
            return false;
        }
        let only = if self.groupable() {
            None
        } else {
            let m = &self.session.model;
            let split = core_groups::split(m, &self.selection);
            match (split.model.is_empty(), split.detail.is_empty()) {
                (true, true) => {
                    self.message = Some(match core_groups::kind_of_all(m, &self.selection) {
                        Err(why) => why,
                        Ok(_) => t("what is selected is in a group already: ungroup it, or exclude it from its group, first").to_string(),
                    });
                    return false;
                }
                (false, false) => {
                    self.message =
                        Some(t("the selection holds model elements and a view's annotation: select one kind and try again").to_string());
                    return false;
                }
                (false, true) => Some(Kind::Model),
                (true, false) => Some(Kind::Detail),
            }
        };
        let mut p = json!({"ids": self.selection});
        if let Some(kind) = only {
            p["only"] = json!(kind.name());
        }
        let Some(reply) = self.reply("create_group", &p) else { return false };
        self.select(reply["id"].as_u64().into_iter().collect());
        true
    }

    /// Ungroup: each selected group's members, plain elements again, selected.
    pub fn ungroup(&mut self) -> bool {
        let groups = self.groups_selected();
        if groups.is_empty() {
            self.message = Some(t("select a group to ungroup").to_string());
            return false;
        }
        let Some(reply) = self.reply("ungroup", &json!({"ids": groups})) else { return false };
        self.select(reply["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect());
        true
    }

    /// Edit Group, of the one group selected.
    pub fn edit_group(&mut self) -> bool {
        let Some(&group) = self.groups_selected().first() else {
            self.message = Some(t("select a group to edit").to_string());
            return false;
        };
        let edited = self.reply("edit_group", &json!({"id": group})).is_some();
        if edited {
            self.select(Vec::new());
        }
        edited
    }

    /// Finish Edit Group: the group's type becomes what the group now is.
    pub fn finish_group(&mut self) -> bool {
        self.end_edit("finish_group")
    }

    /// Cancel Edit Group: the group and everything else as it was when the edit began.
    pub fn cancel_group(&mut self) -> bool {
        self.end_edit("cancel_group")
    }

    fn end_edit(&mut self, command: &str) -> bool {
        let ended = self.reply(command, &json!({})).is_some();
        if ended {
            self.select(Vec::new());
        }
        ended
    }

    /// In Edit Group, the selected elements join the group at Finish.
    pub fn add_to_group(&mut self) -> bool {
        let ids = self.selection.clone();
        !ids.is_empty() && self.reply("add_to_group", &json!({"ids": ids})).is_some()
    }

    /// In Edit Group, the selected members leave the group at Finish, staying as plain elements.
    pub fn remove_from_group(&mut self) -> bool {
        let ids = std::mem::take(&mut self.selection);
        !ids.is_empty() && self.reply("remove_from_group", &json!({"ids": ids})).is_some()
    }

    /// Exclude: the selected members out of their groups here.
    pub fn exclude_selected(&mut self) -> bool {
        let members = self.members_selected();
        if members.is_empty() {
            self.message = Some(t("Tab to a member of a group and select it to exclude it").to_string());
            return false;
        }
        let excluded = self.reply("exclude_member", &json!({"ids": members})).is_some();
        if excluded {
            self.select(Vec::new());
        }
        excluded
    }

    /// Restore All Excluded, of the selected groups.
    pub fn restore_excluded(&mut self) -> bool {
        let groups = self.groups_excluding();
        !groups.is_empty() && self.reply("restore_excluded", &json!({"ids": groups})).is_some()
    }
}

#[cfg(test)]
#[path = "groups_tests.rs"]
mod tests;
