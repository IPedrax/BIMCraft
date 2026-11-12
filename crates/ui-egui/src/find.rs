//! Find (Ctrl+K in both profiles, and the Find button on BIMCraft's title bar): every tool and command the window runs by name
//! (`shortcuts::ACTIONS`, the ones a shortcut can be given) and every view the Project Browser opens, found by the words typed.
//! Enter takes the first, a click the one clicked; Escape or a click elsewhere closes it. On a family tab it lists what the
//! family document does and its own views, and runs them on the document (`family_find_actions`): never the project's tools,
//! which would change the project out of sight.

use egui::{Align2, Area, Context, Frame, Id as UiId, Key, Order, RichText, Sense, TextEdit, WidgetInfo, WidgetType, vec2};

use bimcraft_core::model::Id;

use crate::lang::t;
use crate::style::pal;
use crate::{BimApp, Sheet};

/// The most rows Find lists.
const MOST: usize = 12;

/// What a row of Find does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Found {
    /// A tool or command, by its stable action id.
    Action(crate::shortcuts::ActionId),
    /// A family document's own command, by its English name.
    FamilyAction(&'static str),
    View(Sheet),
    /// A 3D view: the default `{3D}` when none.
    View3D(Option<Id>),
    /// A view of the active family document: none, its {3D}.
    FamilyView(Option<bimcraft_core::family::views::Which>),
    /// A function a plugin registered, by its command id (`plugin:<plugin>:<function>`), run as its ribbon button runs it.
    Plugin(String),
}

/// Whether `label` has every word of `query` in it, whatever their case. Nothing typed matches nothing.
pub(crate) fn matches(label: &str, query: &str) -> bool {
    let label = label.to_lowercase();
    let mut words = query.split_whitespace().peekable();
    words.peek().is_some() && words.all(|w| label.contains(&w.to_lowercase()))
}

/// The Find button on BIMCraft's title bar: a search icon, its name, and Ctrl K.
pub(crate) fn button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(116.0, 24.0), Sense::click());
    let name = t("Find");
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    let p = ui.painter();
    let (ink, ground) = crate::style::icon_inks();
    p.rect_filled(rect, 6.0, if r.hovered() { ground } else { pal().app_bar_hover.gamma_multiply(0.6) });
    if r.has_focus() {
        p.rect_stroke(rect, 6.0, crate::style::focus_ring(), egui::StrokeKind::Inside);
    }
    let icon = egui::Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(15.0, 15.0));
    crate::icons::paint(p, crate::icons::Icon::Search, icon, ink);
    let font = egui::FontId::proportional(12.5);
    p.text(rect.left_center() + vec2(28.0, 0.0), Align2::LEFT_CENTER, name, font.clone(), ink);
    p.text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, "Ctrl K", font, pal().app_bar_muted);
    r.on_hover_text(t("Find a tool, a view or a setting (Ctrl+K)"))
}

impl BimApp {
    /// What Find lists for `query`: the tools and commands, then the views, `MOST` at most; each with the bindings it has.
    pub(crate) fn find_matches(&self, query: &str) -> Vec<(String, Option<String>, Found)> {
        if self.active_family().is_some() {
            return self.family_find_matches(query);
        }
        let actions = crate::shortcuts::registry().actions().iter().filter(|a| a.kind.bindable()).map(|a| {
            let label = t(a.label).to_string();
            let keys = self.shortcuts.hints(&a.id, 2).join(", ");
            (label, if keys.is_empty() { None } else { Some(keys) }, Found::Action(a.id.clone()))
        });
        let views = self.openable().into_iter().map(|(label, f)| (label, None, f));
        // The plugins' functions, as their ribbon tabs and the Tools palette list them.
        let owners = self.plugins.owners();
        let plugin_commands = self.plugins.plugins.iter().filter(|p| p.status.is_loaded()).flat_map(|p| {
            p.manifest
                .functions
                .iter()
                .filter_map(|f| {
                    let id = format!("plugin:{}:{}", p.manifest.id, f.id);
                    let owner = owners.get(&id)?;
                    Some((format!("{} · {}", f.label, owner), None, Found::Plugin(id)))
                })
                .collect::<Vec<_>>()
        });
        actions
            .chain(plugin_commands)
            .chain(views)
            .filter(|(label, keys, found)| {
                let all_bindings = match found {
                    Found::Action(id) => {
                        self.shortcuts.bindings(id).iter().map(|b| crate::shortcuts::display_binding(*b)).collect::<Vec<_>>().join(" ")
                    }
                    _ => String::new(),
                };
                matches(label, query)
                    || matches(keys.as_deref().unwrap_or(""), query)
                    || matches(&all_bindings, query)
                    || matches(
                        match found {
                            Found::Action(id) => id,
                            _ => "",
                        },
                        query,
                    )
            })
            .take(MOST)
            .collect()
    }

    /// What Find lists on a family tab: the family document's commands, then its views.
    fn family_find_matches(&self, query: &str) -> Vec<(String, Option<String>, Found)> {
        use bimcraft_core::family::views::{Side, Which};
        let actions = self.family_find_actions().into_iter().map(|a| (t(a).to_string(), None, Found::FamilyAction(a)));
        let views = [None, Some(Which::Plan), Some(Which::Ceiling)]
            .into_iter()
            .chain(Side::ALL.iter().map(|s| Some(Which::Elevation(*s))))
            .map(|w| (crate::editor::views::view_title(w), None, Found::FamilyView(w)));
        actions.chain(views).filter(|(label, ..)| matches(label, query)).take(MOST).collect()
    }

    /// Find, while it's open: a field across the top of the window, and under it what matches.
    pub(crate) fn find_window(&mut self, ctx: &Context) {
        // Whether it was open last frame: the click that opened it (its button's) is no click beside it.
        let was_open = UiId::new("find-was-open");
        let Some(mut query) = self.find.take() else {
            ctx.data_mut(|d| d.insert_temp(was_open, false));
            return;
        };
        let found = self.find_matches(&query);
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut run = None;
        let shown = Area::new(UiId::new("find")).order(Order::Foreground).anchor(Align2::CENTER_TOP, vec2(0.0, 52.0)).show(ctx, |ui| {
            Frame::popup(ui.style()).fill(pal().surface).show(ui, |ui| {
                ui.set_width(460.0);
                let field = ui.add(
                    TextEdit::singleline(&mut query)
                        .hint_text(t("Find a tool, a view or a setting"))
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Heading),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    run = found.first().map(|(.., f)| f.clone());
                }
                field.request_focus();
                if !found.is_empty() {
                    ui.add_space(4.0);
                }
                for (label, key, f) in &found {
                    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    row.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
                    if row.hovered() {
                        ui.painter().rect_filled(rect, 4.0, pal().hover_fill);
                    }
                    let font = egui::FontId::proportional(13.0);
                    ui.painter().text(rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, label, font.clone(), pal().text);
                    if let Some(key) = key {
                        ui.painter().text(rect.right_center() - vec2(8.0, 0.0), Align2::RIGHT_CENTER, key.as_str(), font, pal().muted);
                    }
                    if row.clicked() {
                        run = Some(f.clone());
                    }
                }
                if found.is_empty() && !query.trim().is_empty() {
                    ui.label(RichText::new(t("Nothing by that name")).color(pal().muted));
                }
            });
        });
        // A click beside it closes it, as a menu's does.
        let (clicked, at) = ctx.input(|i| (i.pointer.primary_clicked(), i.pointer.interact_pos()));
        if clicked && at.is_some_and(|p| !shown.response.rect.contains(p)) && ctx.data(|d| d.get_temp::<bool>(was_open)) == Some(true) {
            close = true;
        }
        ctx.data_mut(|d| d.insert_temp(was_open, run.is_none() && !close));
        match run {
            // On a family tab, the family document's own (Find lists nothing else there).
            Some(Found::Action(a)) if self.active_family().is_some() => {
                if let Some(def) = crate::shortcuts::registry().get(&a) {
                    self.family_find_action(def.label);
                }
            }
            Some(Found::FamilyAction(a)) => {
                self.family_find_action(a);
            }
            Some(Found::Action(a)) => {
                self.execute_action(ctx, &a);
            }
            Some(Found::FamilyView(which)) => {
                if let Some(ed) = self.active_family_mut() {
                    ed.views.show(which);
                }
            }
            Some(Found::View(sheet)) => self.open_sheet(sheet),
            Some(Found::View3D(id)) => self.open_3d(id),
            Some(Found::Plugin(id)) => {
                let label = self.plugins.owners().get(id.as_str()).cloned().unwrap_or_else(|| id.clone());
                self.run_plugin_function(&id, label, ctx);
            }
            None if !close => self.find = Some(query),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn every_word_typed_is_in_the_label_whatever_its_case_and_order() {
        assert!(matches("Floor Plan: Level 2", "level 2 plan"));
        assert!(matches("Wall", "WA"));
        assert!(!matches("Ceiling", "wall"));
        assert!(!matches("Wall", ""), "nothing typed lists nothing");
    }
}
