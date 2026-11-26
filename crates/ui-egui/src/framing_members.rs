//! A framed wall's members picked one by one, as a curtain wall's parts are (`curtain.rs`): Tab on a framed wall under the
//! cursor (selected, or any with Modify in hand) steps through the members there, nearest first, then back to the whole wall.
//! A plan looks at what its cut plane crosses; the 3D view casts the cursor's ray through the wall's members. Properties then
//! shows the member: its role, its section and plies (`framing_member`), its length; Delete leaves it out, never the wall.

use std::sync::Arc;

use bimcraft_core::framing::{self, Framing, Member, edit};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use egui::{RichText, Ui};
use serde_json::json;

use crate::BimApp;
use crate::lang::{t, tf};

/// A point in the model, mm.
pub(crate) type P3 = [f64; 3];

/// The framed wall Tab steps through the members of: the one under the cursor, if it's the one selected, or any with Modify.
pub(crate) fn tab_wall(m: &Model, selection: &[Id], under: Option<Id>, free: bool) -> Option<Id> {
    let id = under?;
    m.wall(id)?.framing.as_ref()?;
    (selection == [id] || free).then_some(id)
}

/// Wall `id`'s members as they're framed now, edits and all.
pub(crate) fn framing_of(m: &Model, id: Id) -> Option<Arc<Framing>> {
    framing::wall(m, id).ok().flatten()
}

/// The member after `now` in `list` (keys, nearest first), then none: the whole wall again.
pub(crate) fn next_member(now: Option<&(Id, String)>, id: Id, list: &[String]) -> Option<(Id, String)> {
    let at = now.filter(|(of, _)| *of == id).and_then(|(_, k)| list.iter().position(|x| x == k));
    list.get(at.map_or(0, |i| i + 1)).map(|k| (id, k.clone()))
}

/// The members of wall `id` under plan point `q`, a plan cutting at `cut`, within `reach` mm.
pub(crate) fn in_plan(m: &Model, id: Id, q: P2, cut: f64, reach: f64) -> Vec<String> {
    framing_of(m, id).map_or_else(Vec::new, |f| edit::in_plan(&f, q, cut, reach))
}

/// The members of wall `id` a ray from `from` toward `to` passes through, nearest first.
pub(crate) fn on_ray(m: &Model, id: Id, from: P3, to: P3) -> Vec<String> {
    framing_of(m, id).map_or_else(Vec::new, |f| edit::on_ray(&f, from, to, &|_| true).into_iter().map(|(k, _)| k).collect())
}

/// Member `key` of wall `id`, as framed now.
pub(crate) fn member(m: &Model, id: Id, key: &str) -> Option<(Arc<Framing>, Member)> {
    let f = framing_of(m, id)?;
    let member = f.members.iter().find(|x| x.key == key)?.clone();
    Some((f, member))
}

/// Member `key` of wall `id` where a plan cutting at `cut` crosses it: its outline in plan.
pub(crate) fn plan_ring(m: &Model, id: Id, key: &str, cut: f64) -> Option<Vec<P2>> {
    let (f, x) = member(m, id, key)?;
    let [s0, s1] = x.cut_at(cut)?;
    let [a0, a1] = x.across;
    Some(vec![f.at(s0, a0), f.at(s1, a0), f.at(s1, a1), f.at(s0, a1)])
}

/// Member `key` of wall `id` as the twelve edges of its box, in the model.
pub(crate) fn edges(m: &Model, id: Id, key: &str) -> Vec<[P3; 2]> {
    let Some((f, x)) = member(m, id, key) else { return Vec::new() };
    let c = edit::corners(&f, &x);
    let pairs = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    pairs.into_iter().map(|(a, b)| [c[a], c[b]]).collect()
}

/// A member marked on the screen: its outline (a plan's ring, closed) or its edges, in the accent; picked, bolder and lit.
pub(crate) fn mark_ring(p: &egui::Painter, ring: Vec<egui::Pos2>, picked: bool) {
    crate::curtain::mark_part(p, ring, picked);
}

/// A member's edges marked on the screen.
pub(crate) fn mark_edges(p: &egui::Painter, edges: &[[egui::Pos2; 2]], picked: bool) {
    let color = if picked { crate::curtain::picked_part_color() } else { crate::style::canvas_pal().accent };
    for e in edges {
        p.line_segment(*e, egui::Stroke::new(if picked { 3.0 } else { 2.0 }, color));
    }
}

/// What a role is called in Properties.
fn role_name(role: framing::Role) -> &'static str {
    use framing::Role::*;
    match role {
        BottomPlate => bimcraft_i18n::msg!("Bottom Plate"),
        TopPlate => bimcraft_i18n::msg!("Top Plate"),
        Stud => bimcraft_i18n::msg!("Stud"),
        King => bimcraft_i18n::msg!("King Stud"),
        Jack => bimcraft_i18n::msg!("Jack Stud"),
        Cripple => bimcraft_i18n::msg!("Cripple"),
        Header => bimcraft_i18n::msg!("Header"),
        Sill => bimcraft_i18n::msg!("Sill"),
        Blocking => bimcraft_i18n::msg!("Blocking"),
        Joist => bimcraft_i18n::msg!("Joist"),
        Rim => bimcraft_i18n::msg!("Rim"),
        Trimmer => bimcraft_i18n::msg!("Trimmer"),
        Rafter => bimcraft_i18n::msg!("Rafter"),
        Ridge => bimcraft_i18n::msg!("Ridge Board"),
    }
}

impl BimApp {
    /// The member Tab went to: what it is, its section and plies to change, its length; Exclude and, edited, Reset.
    pub(crate) fn framing_member_panel(&mut self, ui: &mut Ui, model: &Arc<Model>) {
        let Some((id, key)) = self.framing_member.clone() else { return };
        let Some((_, x)) = member(model, id, &key) else { return };
        let units = model.units;
        ui.label(RichText::new(tf("Framing Member: {}", &[t(role_name(x.role))])).strong());
        ui.label(RichText::new(&key).small().color(crate::style::pal().muted));
        let mut change: Option<serde_json::Value> = None;
        crate::properties::grid(ui, ("framing-member", id, &key), |ui| {
            for (i, label) in [(0usize, bimcraft_i18n::msg!("Thickness")), (1, bimcraft_i18n::msg!("Depth"))] {
                let name = ui.label(t(label));
                let field = format!("framing/{key}/{i}");
                if let Some(typed) = self.field(ui, id, &field, units.length(x.section[i]), name.id, "")
                    && let Some(v) = units.parse(&typed).filter(|v| v.is_finite() && *v >= 1.0)
                {
                    let mut s = x.section;
                    s[i] = v;
                    change = Some(json!({"section": s}));
                }
                ui.end_row();
            }
            let name = ui.label(t("Plies"));
            let mut plies = x.plies;
            let r = ui.add(egui::DragValue::new(&mut plies).range(1..=edit::MAX_PLIES)).labelled_by(name.id);
            if r.changed() && plies != x.plies {
                change = Some(json!({"plies": plies}));
            }
            ui.end_row();
            ui.label(t("Length"));
            ui.label(units.length(x.length));
            ui.end_row();
        });
        let edited = edit::of(model, id, &key).is_some();
        ui.horizontal(|ui| {
            if ui.button(t("Exclude")).on_hover_text(t("Frame the wall without this member (Delete)")).clicked() {
                change = Some(json!({"exclude": true}));
            }
            if edited && ui.button(t("Reset")).on_hover_text(t("As the framing type lays it out")).clicked() {
                change = Some(json!({"reset": true}));
            }
        });
        ui.separator();
        if let Some(mut p) = change {
            let excluding = p["exclude"] == true;
            p["id"] = json!(id);
            p["key"] = json!(key);
            if self.run("framing_member", p).is_some() && excluding {
                self.framing_member = None;
            }
        }
    }

    /// Delete with a member picked: it's left out, never the wall.
    pub(crate) fn exclude_framing_member(&mut self) {
        let Some((id, key)) = self.framing_member.clone() else { return };
        if self.run("framing_member", json!({"id": id, "key": key, "exclude": true})).is_some() {
            self.framing_member = None;
        }
    }

    /// The wall's members edited by hand, all put back as the framing type lays them out.
    pub(crate) fn reset_framing_members(&mut self, id: Id) {
        let keys: Vec<String> =
            self.session.model.wall(id).map(|w| w.framing_edits.iter().map(|e| e.key.clone()).collect()).unwrap_or_default();
        if !keys.is_empty() {
            self.run("framing_member", json!({"id": id, "keys": keys, "reset": true}));
        }
    }
}

impl BimApp {
    /// A framed wall alone selected, with members edited by hand: how many, and Reset to put them all back.
    pub(crate) fn framing_edits_note(&mut self, ui: &mut Ui, model: &Arc<Model>) {
        if self.framing_member.is_some() {
            return;
        }
        let &[id] = self.selection.as_slice() else { return };
        let Some(n) = model.wall(id).map(|w| w.framing_edits.len()).filter(|n| *n > 0) else { return };
        let mut reset = false;
        ui.horizontal(|ui| {
            ui.label(tf("{} framing members edited", &[&n.to_string()]));
            reset = ui.button(t("Reset Members")).on_hover_text(t("As the framing type lays them out")).clicked();
        });
        ui.separator();
        if reset {
            self.reset_framing_members(id);
        }
    }
}
