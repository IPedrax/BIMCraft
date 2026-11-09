//! Nested families in the family document (T6a), as Revit's Family Editor has them: Insert > Load Family loads a family file
//! into the family, asking Family Already Exists and Shared Family Already Loaded against it (`exists.rs`); Create > Component
//! places an instance of one on the view's work plane by a click in a plan or an elevation; a selected instance's Properties
//! show its type, where it stands and each of its family's parameters, every number with Revit's "=" (Associate Family
//! Parameter, `Field::Nested`); the Project Browser's Families lists them, with Edit Family and Delete. Edit Family opens a
//! nested family's own document over its host's: its Load into Project loads it back into the host. The core does the work
//! (`family::nested`): the draft keeps the nested families and instances as the family holds them.
use super::document::group;
use super::*;
use crate::adaptive_editor::{Associating, Field, NestedSlot, Tied, associating, equals, worth};
use crate::icons::Icon;
use crate::ribbon;
use bimcraft_core::family::nested::{self, Nesting};
use bimcraft_core::family::{Group, Keep, NestedInstance, Val};
use bimcraft_core::units::Units;

/// The largest number a row takes, mm or degrees.
const LIMIT: f64 = 1e7;

impl FamilyEditor {
    /// The nested instance selected, while it is there.
    pub(crate) fn selected_nested(&self) -> Option<usize> {
        self.nested_sel.filter(|i| *i < self.nested_instances.len())
    }

    /// Nested instance `i` selected (none: let go), what else was selected let go.
    pub(crate) fn select_nested(&mut self, i: Option<usize>) {
        self.nested_sel = i;
        if i.is_some() {
            self.form = None;
            self.connector = None;
            self.views.plane = None;
            self.views.dim = None;
        }
    }

    /// Create > Component: the nested family and type it places, its first's first type to begin with, in a plan (from {3D},
    /// the floor plan opens, as Revit asks for a view to place in).
    pub(crate) fn component_tool(&mut self) {
        let first = self.nested.first().and_then(|n| n.family.types.keys().next().map(|t| (n.name.clone(), t.clone())));
        self.placing = first;
        if self.placing.is_some() && self.views.shown.is_none() {
            self.views.show(Some(bimcraft_core::family::views::Which::Plan));
        }
    }

    /// An instance of the family and type in hand placed on `host` at `at`, `offset` along its normal, and selected: the tool
    /// is done, and the instance's Properties show (Component again places another).
    pub(crate) fn place_nested(&mut self, host: PlaneRef, at: (f64, f64), offset: f64) {
        let Some((family, type_name)) = self.placing.take() else { return };
        let round = |x: f64| (x * 1000.0).round() / 1000.0;
        self.nested_instances.push(NestedInstance {
            family,
            type_name,
            host,
            at: [Expr::Number(round(at.0)), Expr::Number(round(at.1))],
            offset: Expr::Number(round(offset)),
            ..Default::default()
        });
        self.nested_sel = Some(self.nested_instances.len() - 1);
    }

    /// What loading `incoming` as `name` into this family asks: whether it nests one of that name defined otherwise, and the
    /// shared families nested in `incoming` it has otherwise.
    pub(crate) fn nest_conflicts(&self, name: &str, incoming: &Family) -> (bool, Vec<String>) {
        nested::conflicts(incoming, name, &|n| nested::find(&self.nested, n).map(|f| f.family.clone()))
    }

    /// `incoming` loaded into this family as `name`, over one defined otherwise as `overwrite` says (`Some(true)`: keeping
    /// its values), its shared nested families as `answers` say.
    pub(crate) fn nest(
        &mut self,
        name: &str,
        incoming: Family,
        overwrite: Option<bool>,
        answers: &BTreeMap<String, Keep>,
    ) -> Result<Nesting, String> {
        if name.trim() == self.name.trim() || incoming.nests(self.name.trim()) {
            return Err(tf("{} can't be loaded into itself, nor into a family it nests", &[name]));
        }
        nested::nest_into(&mut self.nested, name, incoming, overwrite, answers)
    }

    /// Nested family `name` taken away, refused while an instance places it.
    pub(crate) fn delete_nested_family(&mut self, name: &str) -> Result<(), String> {
        if self.nested_instances.iter().any(|n| n.family == name) {
            return Err(tf("Instances of {} are placed in the family: delete them first.", &[name]));
        }
        self.nested.retain(|n| n.name != name);
        Ok(())
    }
}

/// Associate Family Parameter's rows of the family's nested instances.
pub(super) struct NestedRows<'a> {
    pub(super) instances: &'a mut [NestedInstance],
}

/// Which instance (and which of its family's parameters) a row's `id` names: `3` or `3/Width`.
fn row_of(id: &str) -> Option<(usize, Option<&str>)> {
    match id.split_once('/') {
        Some((i, p)) => Some((i.parse().ok()?, Some(p))),
        None => Some((id.parse().ok()?, None)),
    }
}

impl NestedRows<'_> {
    fn expr(&mut self, a: &Associating) -> Option<&mut Expr> {
        let (i, param) = row_of(&a.id)?;
        let inst = self.instances.get_mut(i)?;
        let Field::Nested(slot) = a.field else { return None };
        Some(match slot {
            NestedSlot::X => &mut inst.at[0],
            NestedSlot::Y => &mut inst.at[1],
            NestedSlot::Offset => &mut inst.offset,
            NestedSlot::Angle => &mut inst.angle,
            NestedSlot::Visible => inst.visible.get_or_insert(Expr::Number(1.0)),
            NestedSlot::Param => inst.params.entry(param?.to_string()).or_insert(Expr::Number(0.0)),
        })
    }
}

/// What `e`, a row holding a `kind`, comes to for the host's `values`, as typed.
fn worked_out(e: &Expr, kind: Holds, values: &BTreeMap<String, Val>) -> String {
    match (kind.text(), e) {
        (true, Expr::Formula(s)) => match values.get(s.as_str()) {
            Some(Val::Text(t)) => t.clone(),
            _ => s.clone(),
        },
        _ => worth(e, values).map(number_text).unwrap_or_else(|| expr_text(e)),
    }
}

impl Tied for NestedRows<'_> {
    fn tie(&mut self, a: &Associating, values: &BTreeMap<String, Val>) {
        let kind = a.kind;
        let Some(e) = self.expr(a) else { return };
        *e = match &a.chosen {
            Some(p) => Expr::Formula(p.clone()),
            None if kind.text() => Expr::Formula(worked_out(e, kind, values)),
            None => Expr::Number(worth(e, values).unwrap_or(0.0)),
        };
    }

    fn value_now(&mut self, a: &Associating, values: &BTreeMap<String, Val>) -> String {
        let kind = a.kind;
        self.expr(a).map(|e| worked_out(e, kind, values)).unwrap_or_default()
    }
}

/// How wide a row's field is beside its "=".
fn width(ui: &Ui) -> f32 {
    let column = ui.ctx().data(|w| w.get_temp::<f32>(UiId::new("family-group-column"))).unwrap_or(160.0);
    (column - 34.0).max(40.0)
}

/// A number typed into a field labelled by `label`, kept as typed while the field has the focus, shown as `shown` otherwise:
/// what was typed, read by `parse`, when it changed and reads.
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

/// What a row of a nested instance draws with: the host's parameters and values, the project's units and materials, and the
/// association asked for.
struct Rows<'a> {
    params: &'a [(String, Holds)],
    values: &'a BTreeMap<String, Val>,
    units: Units,
    materials: &'a [String],
    asked: Option<Associating>,
}

impl Rows<'_> {
    /// Whether `e` names one of the host's parameters of `kind`: the row is tied to it.
    fn bound(&self, e: &Expr, kind: Holds) -> bool {
        matches!(e, Expr::Formula(f) if self.params.iter().any(|(n, k)| n == f.trim() && *k == kind))
    }

    /// A row of a number (`kind` a length, shown in the project's units; else as it is), with Revit's "=" at its end: tied to
    /// a parameter or worked out by a formula, it shows what that comes to, greyed.
    fn number(&mut self, ui: &mut Ui, label: &str, e: &mut Expr, kind: Holds, id: String, slot: NestedSlot) {
        let l = ui.label(label);
        let w = width(ui);
        let units = self.units;
        let length = kind == Holds::Length;
        let show = move |x: f64| if length { units.length(x) } else { number_text(x) };
        ui.horizontal(|ui| {
            match e {
                Expr::Number(x) => {
                    let parse = move |s: &str| if length { units.parse(s) } else { s.trim().parse().ok() };
                    if let Some(x) = typed(ui, ui.make_persistent_id(("nested-row", &id, label)), l.id, show(*x), w, parse) {
                        *e = Expr::Number(x);
                    }
                }
                Expr::Formula(_) => {
                    let mut shown = worth(e, self.values).map(show).unwrap_or_else(|| "?".into());
                    ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(w)).labelled_by(l.id);
                }
            }
            if equals(ui, label, self.bound(e, kind)) {
                let now = match e {
                    Expr::Formula(f) => Some(f.clone()),
                    Expr::Number(_) => None,
                };
                self.asked = Some(associating(&id, Field::Nested(slot), kind, label, now.as_deref(), self.params));
            }
        });
        ui.end_row();
    }

    /// A row of words (a material, text, a URL) of one of the nested family's parameters, with Revit's "=": tied, it shows what
    /// the host's parameter holds, greyed; a material is picked from the project's.
    fn words(&mut self, ui: &mut Ui, label: &str, e: &mut Expr, kind: Holds, id: String) {
        let l = ui.label(label);
        let w = width(ui);
        ui.horizontal(|ui| {
            let mut text = match e {
                Expr::Formula(s) => s.clone(),
                Expr::Number(x) => number_text(*x),
            };
            if self.bound(e, kind) {
                let mut shown = worked_out(e, kind, self.values);
                ui.add_enabled(false, TextEdit::singleline(&mut shown).desired_width(w)).labelled_by(l.id);
            } else if kind == Holds::Material {
                let c =
                    ComboBox::from_id_salt(("nested-material", &id)).selected_text(text.clone()).width(w).truncate().show_ui(ui, |ui| {
                        for m in self.materials {
                            if ui.selectable_label(*m == text, m).clicked() {
                                *e = Expr::Formula(m.clone());
                            }
                        }
                    });
                c.response.labelled_by(l.id);
            } else if ui.add(TextEdit::singleline(&mut text).desired_width(w)).labelled_by(l.id).changed() {
                *e = Expr::Formula(text.clone());
            }
            if equals(ui, label, self.bound(e, kind)) {
                let now = match e {
                    Expr::Formula(f) => Some(f.clone()),
                    Expr::Number(_) => None,
                };
                self.asked = Some(associating(&id, Field::Nested(NestedSlot::Param), kind, label, now.as_deref(), self.params));
            }
        });
        ui.end_row();
    }
}

impl BimApp {
    /// A selected nested instance's Properties: its type (any nested family's), Constraints (its Work Plane, X, Y, Offset and
    /// Angle, each with "="), Graphics (Visible with "="), and its family's parameters under their groups, each set here (a
    /// number, words, or "=" tied to one of the host's) or left as its type has it.
    pub(super) fn nested_properties(&mut self, ui: &mut Ui, ed: &mut FamilyEditor, i: usize) {
        let units = self.session.model.units;
        let materials: Vec<String> = self.session.model.materials.keys().cloned().collect();
        let (values, params) = (ed.values(), ed.parameter_kinds());
        let host = host_label(ed.plane_set(), &ed.nested_instances[i].host);
        let types: Vec<(String, String)> =
            ed.nested.iter().flat_map(|n| n.family.types.keys().map(|t| (n.name.clone(), t.clone()))).collect();
        let nested = ed.nested.clone();
        let Some(inst) = ed.nested_instances.get_mut(i) else { return };
        let mut rows = Rows { params: &params, values: &values, units, materials: &materials, asked: None };
        let id = i.to_string();
        group(ui, msg!("Constraints"), |ui| {
            let l = ui.label(t("Type"));
            let shown = format!("{} : {}", inst.family, inst.type_name);
            let c = ComboBox::from_id_salt(("nested-type", i)).selected_text(shown).width(width(ui) + 26.0).truncate().show_ui(ui, |ui| {
                for (f, ty) in &types {
                    if ui.selectable_label(*f == inst.family && *ty == inst.type_name, format!("{f} : {ty}")).clicked() {
                        // Another family's: what it set of the one before that this one hasn't goes.
                        if *f != inst.family
                            && let Some(n) = nested::find(&nested, f)
                        {
                            inst.params.retain(|k, _| n.family.parameters.iter().any(|p| p.name == *k));
                        }
                        (inst.family, inst.type_name) = (f.clone(), ty.clone());
                    }
                }
            });
            c.response.labelled_by(l.id);
            ui.end_row();
            ui.label(t("Work Plane"));
            ui.label(&host);
            ui.end_row();
            let [x, y] = &mut inst.at;
            rows.number(ui, t("X"), x, Holds::Length, id.clone(), NestedSlot::X);
            rows.number(ui, t("Y"), y, Holds::Length, id.clone(), NestedSlot::Y);
            rows.number(ui, t("Offset"), &mut inst.offset, Holds::Length, id.clone(), NestedSlot::Offset);
            rows.number(ui, t("Angle"), &mut inst.angle, Holds::Angle, id.clone(), NestedSlot::Angle);
        });
        group(ui, msg!("Graphics"), |ui| {
            let label = t("Visible");
            let l = ui.label(label);
            ui.horizontal(|ui| {
                let tied = inst.visible.as_ref().is_some_and(|e| rows.bound(e, Holds::YesNo));
                let mut on = inst.visible.as_ref().is_none_or(|e| worth(e, &values) != Some(0.0));
                let free = matches!(inst.visible, None | Some(Expr::Number(_)));
                if ui.add_enabled(free && !tied, egui::Checkbox::without_text(&mut on)).labelled_by(l.id).changed() {
                    inst.visible = (!on).then_some(Expr::Number(0.0));
                }
                if equals(ui, label, tied) {
                    let now = match &inst.visible {
                        Some(Expr::Formula(f)) => Some(f.clone()),
                        _ => None,
                    };
                    rows.asked = Some(associating(&id, Field::Nested(NestedSlot::Visible), Holds::YesNo, label, now.as_deref(), &params));
                }
            });
            ui.end_row();
        });
        // Its family's parameters, as Revit lists a nested instance's: under their groups, those that may be set from here.
        if let Some(n) = nested::find(&nested, &inst.family) {
            let own = n.family.values(&inst.type_name, &BTreeMap::new()).unwrap_or_default();
            for g in Group::ALL {
                let settable: Vec<&Parameter> = n
                    .family
                    .parameters
                    .iter()
                    .filter(|p| p.group() == g && p.formula.is_none() && !p.reporting && p.kind != Holds::FamilyType)
                    .collect();
                if settable.is_empty() {
                    continue;
                }
                group(ui, g.label(), |ui| {
                    for p in settable {
                        let row = format!("{i}/{}", p.name);
                        let mut e = inst.params.get(&p.name).cloned();
                        // Unset, it is what its type gives it; typed, it is the instance's.
                        let set = e.is_some();
                        let mut value = e.take().unwrap_or_else(|| match own.get(&p.name) {
                            Some(Val::Number(x)) => Expr::Number(*x),
                            Some(Val::Text(s)) => Expr::Formula(s.clone()),
                            None => Expr::Number(0.0),
                        });
                        let before = value.clone();
                        if p.kind.text() {
                            rows.words(ui, &p.name, &mut value, p.kind, row);
                        } else {
                            rows.number(ui, &p.name, &mut value, p.kind, row, NestedSlot::Param);
                        }
                        if set || value != before {
                            inst.params.insert(p.name.clone(), value);
                        }
                    }
                });
            }
        }
        if rows.asked.is_some() {
            ed.associating = rows.asked;
        }
    }

    /// The panels of a selected nested instance's contextual tab: Edit Family, which opens its family's document over this one.
    pub(super) fn nested_mode_panel(&mut self, ui: &mut Ui, ed: &mut FamilyEditor, i: usize) {
        ribbon::panel(ui, msg!("Mode"), |ui| {
            let tip = msg!("Edit Family: the nested family opens over this one; its Load into Project loads it back here");
            if crate::big_button(ui, Icon::FamilyEditor, msg!("Edit Family"), false, tip).clicked() {
                ed.open_nested = ed.nested_instances.get(i).map(|n| n.family.clone());
            }
        });
    }

    /// The Project Browser's Families: the families nested in the open one, each with its types; its right-click menu has Edit
    /// Family and Delete.
    pub(super) fn nested_browser(&mut self, ui: &mut Ui) {
        let Some(ed) = self.family_editor.as_ref() else { return };
        if ed.nested.is_empty() {
            return;
        }
        let listed: Vec<(String, Vec<String>)> =
            ed.nested.iter().map(|n| (n.name.clone(), n.family.types.keys().cloned().collect())).collect();
        let mut asked: Option<(String, bool)> = None;
        egui::CollapsingHeader::new(t("Families")).id_salt("family-nested").default_open(true).show(ui, |ui| {
            for (name, types) in &listed {
                let r = egui::CollapsingHeader::new(name.as_str()).id_salt(("family-nested", name)).default_open(true).show(ui, |ui| {
                    for ty in types {
                        ui.label(ty);
                    }
                });
                r.header_response.context_menu(|ui| {
                    if ui.button(t("Edit Family")).clicked() {
                        asked = Some((name.clone(), true));
                        ui.close();
                    }
                    if ui.button(t("Delete")).clicked() {
                        asked = Some((name.clone(), false));
                        ui.close();
                    }
                });
            }
        });
        let Some(ed) = self.family_editor.as_mut() else { return };
        match asked {
            Some((name, true)) => ed.open_nested = Some(name),
            Some((name, false)) => {
                if let Err(text) = ed.delete_nested_family(&name) {
                    self.status = Status { text, error: true };
                }
            }
            None => {}
        }
    }

    /// Edit Family on nested family `name` of `host`: its own document opens over the host's, which waits under it.
    pub(super) fn open_nested_family(&mut self, mut host: FamilyEditor, name: &str) {
        let Some(n) = nested::find(&host.nested, name) else {
            self.family_editor = Some(host);
            return;
        };
        let mut child = FamilyEditor::of(name, &n.family);
        child.into_host = true;
        host.placing = None;
        self.family_hosts.push(host);
        self.family_editor = Some(child);
    }

    /// Insert > Load Family in a family document: the family in the file at `path` loaded into it, asking first when it nests
    /// one of that name otherwise, or a shared family the file nests otherwise.
    pub(crate) fn nest_family_file(&mut self, path: &std::path::Path) {
        match read_family_file(path) {
            Ok((name, family)) => self.nest_family(name, family, exists::After::Nest),
            Err(text) => self.status = Status { text, error: true },
        }
    }
}

/// The Options Bar while Component places: the nested family and type it places.
pub(super) fn placing_options(ui: &mut Ui, ed: &mut FamilyEditor) {
    let Some((family, type_name)) = ed.placing.clone() else { return };
    let types: Vec<(String, String)> = ed.nested.iter().flat_map(|n| n.family.types.keys().map(|t| (n.name.clone(), t.clone()))).collect();
    ui.separator();
    let l = ui.label(t("Type"));
    let c = ComboBox::from_id_salt("nested-placing").selected_text(format!("{family} : {type_name}")).width(240.0).show_ui(ui, |ui| {
        for (f, ty) in types {
            let on = f == family && ty == type_name;
            let shown = format!("{f} : {ty}");
            if ui.selectable_label(on, shown).clicked() {
                ed.placing = Some((f, ty));
            }
        }
    });
    c.response.labelled_by(l.id);
}

/// The name Properties' Work Plane gives the plane an instance stands on, as Placement Plane lists it (`Planes::listed`).
fn host_label(planes: bimcraft_core::family::Planes, host: &PlaneRef) -> String {
    match planes.listed().into_iter().find(|(p, _)| p == host) {
        Some((_, label)) => t(&label).to_string(),
        None => t("Reference Plane").to_string(),
    }
}
