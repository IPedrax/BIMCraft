//! A form of a family's own selected in its document (from the Project Browser's Forms list or the Forms tab's Select), shown
//! as Revit shows an Extrusion: its instance Properties (Constraints, Graphics, Materials and Finishes, Identity Data), its
//! Depth in the Options Bar, and Edit Work Plane on its contextual tab. Edits go to the draft, so the document's history
//! undoes them. Solid/Void and Edit Work Plane run the `family` and `edit_work_plane` commands on a private session holding
//! the draft's family, as Save Family does, and take the form back from it: the project is never touched.
use super::document::group;
use super::*;
use crate::adaptive_editor::{Field, Slot, associating, worth};
use crate::icons::Icon;
use crate::ribbon;
use bimcraft_core::family::Val;
use bimcraft_core::units::Units;

/// The largest length or angle a row takes, mm or degrees.
const LIMIT: f64 = 1e7;

impl FamilyEditor {
    /// The form selected, while it is there.
    pub(crate) fn selected_form(&self) -> Option<usize> {
        self.form.filter(|i| *i < self.forms.len())
    }

    /// Form `i` selected, as a click on it selects it.
    pub(crate) fn select_form_at(&mut self, i: usize) {
        self.form = Some(i);
    }

    /// Form `i` selected, as a click on it selects it.
    #[cfg(test)]
    pub(crate) fn select_form(&mut self, i: usize) {
        self.select_form_at(i);
    }

    /// Escape: the form selected let go. Whether there was one.
    pub(crate) fn let_form_go(&mut self) -> bool {
        self.selected_form().is_some() && self.form.take().is_some()
    }

    /// The family as the draft has it now, built: what the document would load into the project.
    #[cfg(test)]
    pub(crate) fn draft_family(&self) -> Result<Family, String> {
        self.family()
    }

    /// `command` run on form `i` in a private session holding the draft's family (`params` given the family's name and the
    /// form's GUID there), and the form taken back from it, keeping the id and GUID the draft gave it.
    fn through(&mut self, base: &Arc<Model>, i: usize, command: &str, params: impl FnOnce(&str, Value) -> Value) -> Result<(), String> {
        let p = self.command(base)?;
        let mut s = Session::new();
        s.model = Arc::clone(base);
        s.execute("family", &p)?;
        let name = self.name.trim().to_string();
        let form = |s: &Session| s.model.families.get(&name).and_then(|f| f.forms.get(i)).cloned();
        let gone = || tf("the family has no form {}", &[&(i + 1).to_string()]);
        let guid = form(&s).ok_or_else(gone)?.guid;
        s.execute(command, &params(&name, json!(guid)))?;
        let mut back = form(&s).ok_or_else(gone)?;
        let Some(was) = self.forms.get_mut(i) else { return Err(gone()) };
        (back.id, back.guid) = (was.original.id, was.original.guid);
        *was = FormText { tag: was.tag, ..FormText::of(&back) };
        Ok(())
    }
}

/// The name Properties' Work Plane gives the plane a form is drawn on, as Placement Plane lists it (`Planes::listed`).
fn host_label(planes: bimcraft_core::family::Planes, host: &PlaneRef) -> String {
    match planes.listed().into_iter().find(|(p, _)| p == host) {
        Some((_, label)) => t(&label).to_string(),
        // An unnamed plane of the family's own, which Revit does not list either.
        None => t("Reference Plane").to_string(),
    }
}

/// How wide a row's field is beside its "=".
fn width(ui: &Ui) -> f32 {
    let column = ui.ctx().data(|w| w.get_temp::<f32>(UiId::new("family-group-column"))).unwrap_or(160.0);
    (column - 34.0).max(40.0)
}

/// A number typed into a field labelled by `label`, kept as typed while the field has the focus (so "2." is not read back as
/// "2"), shown as `shown` otherwise: what was typed, read by `parse`, when it changed and reads.
fn typed(ui: &mut Ui, key: UiId, label: egui::Id, shown: String, w: f32, parse: impl Fn(&str) -> Option<f64>) -> Option<f64> {
    let mut text = ui.data(|d| d.get_temp::<String>(key)).unwrap_or(shown);
    let r = ui.add(TextEdit::singleline(&mut text).desired_width(w)).labelled_by(label);
    let read = r.changed().then(|| parse(&text)).flatten().filter(|x| x.is_finite() && x.abs() <= LIMIT);
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    read
}

/// A row of a form's lengths (or a revolve's angles, `units` none) with Revit's "=" at its end: a number is typed in the
/// project's units; tied to a parameter or worked out by a formula, it shows what that comes to, greyed, and takes no typing.
fn number_row(
    ui: &mut Ui,
    units: Option<Units>,
    label: &str,
    text: &mut String,
    values: &BTreeMap<String, Val>,
    links: &mut association::Controls,
    row: (Field, usize),
) {
    let l = ui.label(t(label));
    let w = width(ui);
    let show = move |x: f64| units.map_or_else(|| number_text(x), |u| u.length(x));
    let free = text.trim().parse::<f64>().is_ok_and(f64::is_finite);
    ui.horizontal(|ui| {
        if free {
            let shown = worth(&text_expr(text), values).map(show).unwrap_or_default();
            let parse = move |s: &str| match units {
                Some(u) => u.parse(s),
                None => s.trim().parse().ok(),
            };
            if let Some(x) = typed(ui, ui.make_persistent_id(("form-row", label)), l.id, shown, w, parse) {
                *text = number_text(x);
            }
        } else {
            let mut shown = worth(&text_expr(text), values).map(show).unwrap_or_else(|| "?".into());
            ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(w)).labelled_by(l.id);
        }
        let kind = if units.is_some() { Holds::Length } else { Holds::Angle };
        links.button(ui, t(label).to_string(), text, kind, row);
    });
    ui.end_row();
}

impl BimApp {
    /// The selected form's Properties, as Revit's for an Extrusion: Constraints (Extrusion End and Start, each with "=", and
    /// the Work Plane it is drawn on, read-only), Graphics (Visible with "="; Visibility/Graphics Overrides, F8's, greyed),
    /// Materials and Finishes (Material with "="), Identity Data (Solid/Void). A revolve's are its angles, a mesh's none.
    pub(super) fn form_properties(&mut self, ui: &mut Ui, ed: &mut FamilyEditor, i: usize) {
        let units = self.session.model.units;
        let mut materials = vec![String::new()];
        materials.extend(self.session.model.materials.keys().cloned());
        let (values, params) = (ed.values(), ed.parameter_kinds());
        let mut links = association::Controls::new(ed);
        let host = host_label(ed.plane_set(), &ed.forms[i].host);
        let Some(f) = ed.forms.get_mut(i) else { return };
        let (mut visible, mut void) = (false, None);
        group(ui, msg!("Constraints"), |ui| {
            if f.mesh() {
                ui.label(t("Mesh"));
                ui.label(RichText::new(tf("{} triangles, from a file", &[&f.original.mesh.len().to_string()])).color(pal().muted));
                ui.end_row();
                return;
            }
            let (end, start, u) = if f.revolve {
                (msg!("End Angle"), msg!("Start Angle"), None)
            } else {
                (msg!("Extrusion End"), msg!("Extrusion Start"), Some(units))
            };
            number_row(ui, u, end, &mut f.to, &values, &mut links, (Field::Form(Slot::To), i));
            number_row(ui, u, start, &mut f.from, &values, &mut links, (Field::Form(Slot::From), i));
            ui.label(t("Work Plane"));
            ui.label(&host);
            ui.end_row();
        });
        group(ui, msg!("Graphics"), |ui| {
            visible = visible_row(ui, &mut f.visible, t("Visible").to_string(), &values);
            // F8's, not built yet: where Revit has it, greyed.
            ui.label(t("Visibility/Graphics Overrides"));
            ui.add_enabled(false, egui::Button::new(t("Edit…"))).on_disabled_hover_text(t("Not in BIMCraft yet"));
            ui.end_row();
        });
        group(ui, msg!("Materials and Finishes"), |ui| {
            let label = msg!("Material");
            let l = ui.label(t(label));
            let w = width(ui);
            ui.horizontal(|ui| {
                if links.bound(&f.material, Holds::Material) {
                    let mut shown = match values.get(f.material.trim()) {
                        Some(Val::Text(m)) => m.clone(),
                        _ => String::new(),
                    };
                    ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(w)).labelled_by(l.id);
                } else {
                    let shown = if f.material.is_empty() { t("(none)").to_string() } else { f.material.clone() };
                    let c = ComboBox::from_id_salt(("form-material", i)).selected_text(shown).width(w).truncate().show_ui(ui, |ui| {
                        for m in &materials {
                            if ui.selectable_label(*m == f.material, if m.is_empty() { t("(none)") } else { m }).clicked() {
                                f.material.clone_from(m);
                            }
                        }
                    });
                    c.response.labelled_by(l.id);
                }
                links.button(ui, t(label).to_string(), &f.material, Holds::Material, (Field::Form(Slot::Material), i));
            });
            ui.end_row();
        });
        group(ui, msg!("Identity Data"), |ui| {
            let l = ui.label(t("Solid/Void"));
            let name = |v: bool| if v { t("Void") } else { t("Solid") };
            let c =
                ComboBox::from_id_salt(("form-solid-void", i)).selected_text(name(f.void)).width(width(ui)).truncate().show_ui(ui, |ui| {
                    for v in [false, true] {
                        if ui.selectable_label(f.void == v, name(v)).clicked() && f.void != v {
                            void = Some(v);
                        }
                    }
                });
            c.response.labelled_by(l.id);
            ui.end_row();
        });
        if visible {
            let typed = f.visible.trim().to_string();
            ed.associating =
                Some(associating(&i.to_string(), Field::Form(Slot::Visible), Holds::YesNo, msg!("Visible"), Some(&typed), &params));
        }
        if links.request.is_some() {
            ed.associating = links.request;
        }
        if let Some(v) = void {
            let base = Arc::clone(&self.session.model);
            if let Err(e) = ed.through(&base, i, "family", |name, guid| json!({"name": name, "form": {"guid": guid, "void": v}})) {
                self.status = Status { text: e, error: true };
            }
        }
    }

    /// Revit's Work Plane panel on a selected form's contextual tab: Edit Work Plane, dropping down the family's planes
    /// (`Planes::listed`); the one picked runs `edit_work_plane`, so the form moves with its plane. Revit's Work Plane dialog is
    /// T12's.
    pub(super) fn form_work_plane_panel(&mut self, ui: &mut Ui, ed: &mut FamilyEditor, i: usize) {
        let Some(f) = ed.forms.get(i) else { return };
        let (current, mesh) = (f.host, f.mesh());
        let planes = ed.plane_set().listed();
        let mut picked = None;
        ribbon::panel(ui, msg!("Work Plane"), |ui| {
            let tip = msg!("Edit Work Plane: the form onto another of the family's planes, moving with it");
            let r = ui.add_enabled_ui(!mesh, |ui| crate::big_button(ui, Icon::PickFaces, msg!("Edit Work Plane"), false, tip)).inner;
            ribbon::choices(ui, &r, false, |ui| {
                for (host, label) in &planes {
                    if ui.selectable_label(*host == current, t(label)).clicked() {
                        picked = Some(*host);
                        ui.close();
                    }
                }
            });
        });
        if let Some(host) = picked.filter(|h| *h != current) {
            let base = Arc::clone(&self.session.model);
            let call = |name: &str, guid: Value| json!({"family": name, "form": guid, "host": host});
            if let Err(e) = ed.through(&base, i, "edit_work_plane", call) {
                self.status = Status { text: e, error: true };
            }
        }
    }
}

/// The Options Bar's Depth of a selected extrusion: its End less its Start, in the project's units. Typed, Start stays and
/// End moves; End tied to a parameter or worked out by a formula, it is shown greyed and takes no typing.
pub(super) fn depth(ui: &mut Ui, units: Units, f: &mut FormText, values: &BTreeMap<String, Val>) {
    let l = ui.label(t("Depth"));
    let (from, to) = (worth(&text_expr(&f.from), values), worth(&text_expr(&f.to), values));
    let shown = from.zip(to).map(|(a, b)| units.length(b - a)).unwrap_or_else(|| "?".into());
    let free = f.to.trim().parse::<f64>().is_ok_and(f64::is_finite);
    match from {
        Some(start) if free => {
            if let Some(d) = typed(ui, UiId::new("family-options-depth"), l.id, shown, 90.0, |s| units.parse(s)) {
                f.to = number_text(start + d);
            }
        }
        _ => {
            let mut shown = shown;
            ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(90.0)).labelled_by(l.id);
        }
    }
}
