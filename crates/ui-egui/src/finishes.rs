//! A room's floor and ceiling in the window: the rows at the head of Properties that say what follows what, with Disconnect and
//! Regenerate, and what Delete asks about them. The work is the core's (`room_finish`, `delete {keep_finishes}`).

use bimcraft_core::model::{Element, Finish, Id, Model, Part};
use bimcraft_i18n::msg;
use egui::{Context, Id as UiId, RichText, Ui};
use serde_json::json;

use crate::BimApp;
use crate::lang::t;
use crate::popup::Modal;
use crate::properties::{grid, group_header, link};
use crate::style::pal;

/// The group a room's floor and ceiling are under, and the one a floor or a ceiling names its room under.
const OF_ROOM: &str = msg!("Connected Floor and Ceiling");
const TO_ROOM: &str = msg!("Connected Room");

/// What a row's button asks for.
enum Ask {
    Select(Id),
    Finish(Id, Part, &'static str),
}

impl BimApp {
    /// Whether any of `ids` is a floor or a ceiling that follows a room: its type, level, offset and outline are the room's.
    pub(crate) fn follows_a_room(&self, ids: &[Id]) -> bool {
        let m = &self.session.model;
        ids.iter().any(|id| m.finish_of(*id).is_some())
    }

    /// The first rows of Properties for one room with a floor or a ceiling of its own, or for one floor or ceiling that
    /// follows a room: nothing for any other element.
    pub(crate) fn connected_finishes(&mut self, ui: &mut Ui, model: &Model, id: Id) {
        let mut ask = None;
        match model.elements.get(id) {
            Some(Element::Room(r)) => {
                let slots: Vec<(Part, Finish)> = Part::BOTH.into_iter().map(|p| (p, r.finish(p))).filter(|(_, f)| !f.is_none()).collect();
                if slots.is_empty() || !self.finish_group(ui, OF_ROOM) {
                    return;
                }
                let wanted = |part| model.types.room.get(&r.type_name).and_then(|ty| ty.finish(part)).is_some();
                grid(ui, ("room finishes", id), |ui| {
                    for (part, finish) in slots {
                        let (label, let_go, again) = match part {
                            Part::Floor => (msg!("Floor"), msg!("Disconnect Floor"), msg!("Regenerate Floor")),
                            Part::Ceiling => (msg!("Ceiling"), msg!("Disconnect Ceiling"), msg!("Regenerate Ceiling")),
                        };
                        ui.label(t(label));
                        ui.vertical(|ui| match finish.connected().and_then(|g| model.elements.id(g)) {
                            Some(other) => {
                                let ty = bimcraft_core::element_json(model, other)["type"].as_str().unwrap_or_default().to_string();
                                ui.label(ty);
                                if link(ui, model, other) {
                                    ask = Some(Ask::Select(other));
                                }
                                let tip = t(msg!(
                                    "Let it go: it stays as an ordinary one, and the room stops moving it. It can't be connected again."
                                ));
                                if named(ui, t("Disconnect"), t(let_go), true).on_hover_text(tip).clicked() {
                                    ask = Some(Ask::Finish(id, part, "disconnect"));
                                }
                            }
                            None => {
                                ui.label(RichText::new(t(msg!("Disconnected"))).color(pal().muted));
                                let tip = t(msg!("Make a new one for the room, as its type says"));
                                let none = t(msg!("The room's type has none to make"));
                                let r = named(ui, t("Regenerate"), t(again), wanted(part));
                                if r.on_hover_text(tip).on_disabled_hover_text(none).clicked() {
                                    ask = Some(Ask::Finish(id, part, "regenerate"));
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
            }
            Some(Element::Floor(_) | Element::Ceiling(_)) => {
                let Some((room, part)) = model.finish_of(id) else { return };
                if !self.finish_group(ui, TO_ROOM) {
                    return;
                }
                let name = match model.elements.get(room) {
                    Some(Element::Room(r)) => format!("{} {}", r.number, r.name).trim().to_string(),
                    _ => String::new(),
                };
                grid(ui, ("finish room", id), |ui| {
                    ui.label(t(msg!("Connected to")));
                    ui.vertical(|ui| {
                        ui.label(name);
                        if link(ui, model, room) {
                            ask = Some(Ask::Select(room));
                        }
                        let tip =
                            t(msg!("Let it go: it stays as an ordinary one, and the room stops moving it. It can't be connected again."));
                        if ui.button(t("Disconnect")).on_hover_text(tip).clicked() {
                            ask = Some(Ask::Finish(room, part, "disconnect"));
                        }
                    });
                    ui.end_row();
                });
            }
            _ => {}
        }
        match ask {
            Some(Ask::Select(other)) => self.selection = vec![other],
            Some(Ask::Finish(room, part, action)) => {
                self.run("room_finish", json!({"id": room, "part": part.name(), "action": action}));
            }
            None => {}
        }
    }

    /// The header of one of the two groups, folded as Properties' other groups are: whether it is open.
    fn finish_group(&mut self, ui: &mut Ui, group: &'static str) -> bool {
        let open = !self.collapsed.contains(group);
        if group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        open
    }

    /// Delete `ids`. Rooms among them whose floor or ceiling isn't going with them anyway are asked about first: the
    /// floors and ceilings go too, or stay as ordinary ones.
    pub(crate) fn delete_elements(&mut self, ids: Vec<Id>) {
        let m = &self.session.model;
        let left = |f: Finish| f.connected().and_then(|g| m.elements.id(g)).is_some_and(|other| !ids.contains(&other));
        let asks = ids.iter().any(|id| matches!(m.elements.get(id), Some(Element::Room(r)) if left(r.floor) || left(r.ceiling)));
        if asks {
            self.deleting_rooms = Some(ids);
        } else if !ids.is_empty() {
            self.run("delete", json!({"ids": ids}));
        }
    }

    /// What Delete asks of rooms with a floor or a ceiling of their own.
    pub(crate) fn delete_rooms_window(&mut self, ctx: &Context) {
        let Some(ids) = self.deleting_rooms.clone() else { return };
        let mut keep = None;
        let mut cancel = false;
        let r = Modal::new(UiId::new("delete-rooms"), t(msg!("Delete Rooms"))).width(460.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t(msg!(
                    "These rooms have floors or ceilings connected to them. Delete those too, or keep them as ordinary floors and ceilings?"
                )));
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t(msg!("Delete Them Too"))).clicked() {
                        keep = Some(false);
                    }
                    if ui.button(t(msg!("Keep Them"))).clicked() {
                        keep = Some(true);
                    }
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if keep.is_some() || cancel || r.should_close() {
            self.deleting_rooms = None;
        }
        if let Some(keep) = keep {
            self.run("delete", json!({"ids": ids, "keep_finishes": keep}));
        }
    }
}

/// A button that reads `text` and is named `name` for a screen reader and a test: two rows' buttons read alike.
fn named(ui: &mut Ui, text: &str, name: &str, enabled: bool) -> egui::Response {
    let r = ui.add_enabled(enabled, egui::Button::new(text));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    r
}
