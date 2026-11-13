//! Dimension Type Properties, the dialog Edit Type opens for a dimension, as Revit's: its graphics (the string, the tick
//! mark, the pens, the colour, the dimension line's extension and the witness lines) and its text (font, size, lettering,
//! offset, background, read convention, units and the symbols), Duplicate and Rename, and OK, Cancel and Apply. And which
//! type a new dimension of a kind takes.

use crate::popup::Modal;

use bimcraft_core::dims::{Background, DimString, DimensionType, Read, Tick, Witness};
use bimcraft_core::model::{DimKind, Element, Id, Model};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{ComboBox, Context, DragValue, Grid, Id as UiId, TextEdit};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::typeprops::{self, Head, Naming, Table};

/// Dimension Type Properties: one dimension type's values, and the dimensions it opened for, which OK puts on the type shown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DimensionTypeDialog {
    name: String,
    t: DimensionType,
    elements: Vec<Id>,
    naming: Naming,
    error: Option<String>,
    find: String,
}

impl DimensionTypeDialog {
    pub(crate) fn new(m: &Model, name: String, elements: Vec<Id>) -> DimensionTypeDialog {
        let t = m.dimension_types.get(&name).cloned().unwrap_or_default();
        DimensionTypeDialog { name, t, elements, naming: Naming::None, error: None, find: String::new() }
    }

    /// What stopped OK, Apply, Duplicate or Rename, if anything did.
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The `dimension_type` command for the type as typed, under `name` (from the one shown, for a duplicate).
    fn params(&self, name: &str) -> Value {
        let mut p = serde_json::to_value(&self.t).unwrap_or_else(|_| json!({}));
        if let Some(o) = p.as_object_mut() {
            o.insert("name".into(), json!(name));
            o.insert("like".into(), json!(self.name));
            // What isn't set is set back to the project's: null.
            o.entry("units").or_insert(Value::Null);
            o.entry("decimals").or_insert(Value::Null);
        }
        p
    }
}

/// Lengths on paper, from 0 to 50 mm.
fn mm(x: &mut f64) -> DragValue<'_> {
    DragValue::new(x).range(0.0..=50.0).speed(0.1).max_decimals(2).suffix(" mm")
}

/// A row choosing one of `options`, each a value and its label.
fn choice<T: Copy + PartialEq>(tb: &mut Table, label: &str, value: &mut T, options: &[(T, &str)]) {
    let labels: Vec<String> = options.iter().map(|(_, l)| t(l).to_string()).collect();
    let mut now = options.iter().position(|(v, _)| v == value).and_then(|i| labels.get(i)).cloned().unwrap_or_default();
    tb.pick(label, &mut now, &labels);
    if let Some((v, _)) = labels.iter().position(|l| *l == now).and_then(|i| options.get(i)) {
        *value = *v;
    }
}

impl BimApp {
    /// The type a new dimension of `kind` is placed in: the one the Type Selector names, while the project has it, else the
    /// one its kind starts in.
    pub(crate) fn dimension_type_for(&self, kind: DimKind) -> Option<String> {
        let types = &self.session.model.dimension_types;
        match self.types.get("dimension") {
            Some(name) if types.contains_key(name) => Some(name.clone()),
            _ => bimcraft_core::dims::default_type(types, kind).cloned(),
        }
    }

    /// Dimension Type Properties, while it's open.
    pub(crate) fn dimension_type_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.dimension_type_dialog.take() else { return };
        let model = std::sync::Arc::clone(&self.session.model);
        let names: Vec<String> = model.dimension_types.keys().cloned().collect();
        let fonts = bimcraft_core::text::families();
        let (mut ok, mut cancel, mut apply, mut asked) = (false, false, false, None);
        let r = Modal::new(UiId::new("dimension-type-properties"), t("Type Properties")).width(crate::typeprops::WIDTH).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                asked = typeprops::head(
                    ui,
                    "dimension-type",
                    &tf("System Family: {}", &[t("Dimension")]),
                    &names,
                    &d.name,
                    &mut d.naming,
                    true,
                );
                let below = typeprops::foot_height(ui, d.error.is_some());
                typeprops::table(ui, "dimension-type", below, |tb| {
                    tb.group("Graphics");
                    choice(
                        tb,
                        t("Dimension String Type"),
                        &mut d.t.string,
                        &[
                            (DimString::Continuous, msg!("Continuous")),
                            (DimString::Baseline, msg!("Baseline")),
                            (DimString::Ordinate, msg!("Ordinate")),
                        ],
                    );
                    choice(
                        tb,
                        t("Tick Mark"),
                        &mut d.t.tick,
                        &[
                            (Tick::Diagonal, msg!("Diagonal")),
                            (Tick::Arrow, msg!("Arrow")),
                            (Tick::OpenArrow, msg!("Open Arrow")),
                            (Tick::Dot, msg!("Dot")),
                            (Tick::None, msg!("None")),
                        ],
                    );
                    tb.drag(t("Tick Mark Size"), mm(&mut d.t.tick_size));
                    tb.drag(t("Line Weight"), DragValue::new(&mut d.t.line_pen.0).range(0..=bimcraft_core::pen::MAX_PEN));
                    tb.drag(t("Tick Mark Line Weight"), DragValue::new(&mut d.t.tick_pen.0).range(0..=bimcraft_core::pen::MAX_PEN));
                    tb.drag(t("Dimension Line Extension"), mm(&mut d.t.line_extension));
                    choice(
                        tb,
                        t("Witness Line Control"),
                        &mut d.t.witness,
                        &[(Witness::GapToElement, msg!("Gap to Element")), (Witness::Fixed, msg!("Fixed to Dimension Line"))],
                    );
                    tb.drag(t("Witness Line Length"), mm(&mut d.t.witness_length));
                    tb.drag(t("Witness Line Gap to Element"), mm(&mut d.t.witness_gap));
                    tb.drag(t("Witness Line Extension"), mm(&mut d.t.witness_extension));
                    tb.drag(t("Baseline Spacing"), mm(&mut d.t.baseline_spacing));
                    tb.check(t("Center Marks"), &mut d.t.centre_mark);
                    tb.color(t("Color"), &mut d.t.color);
                    tb.group("Text");
                    let font = &mut d.t.font;
                    let find = &mut d.find;
                    tb.row(t("Text Font"), |ui, l| {
                        ComboBox::from_id_salt("dimension-type-font")
                            .selected_text(font.as_str())
                            .width(ui.available_width())
                            .height(320.0)
                            .show_ui(ui, |ui| {
                                ui.add(TextEdit::singleline(find).hint_text(t("Search fonts")).desired_width(180.0));
                                let find = find.trim().to_lowercase();
                                for f in fonts.iter().filter(|f| f.to_lowercase().contains(&find)) {
                                    ui.selectable_value(font, f.clone(), f);
                                }
                            })
                            .response
                            .labelled_by(l)
                    });
                    tb.drag(t("Text Size"), DragValue::new(&mut d.t.text_size).range(0.1..=100.0).speed(0.1).max_decimals(2).suffix(" mm"));
                    tb.drag(t("Text Offset"), mm(&mut d.t.text_offset));
                    tb.check(t("Bold"), &mut d.t.bold);
                    tb.check(t("Italic"), &mut d.t.italic);
                    tb.check(t("Underline"), &mut d.t.underline);
                    tb.drag(t("Width Factor"), DragValue::new(&mut d.t.width_factor).range(0.1..=10.0).speed(0.01).max_decimals(2));
                    choice(
                        tb,
                        t("Text Background"),
                        &mut d.t.background,
                        &[(Background::Opaque, msg!("Opaque")), (Background::Transparent, msg!("Transparent"))],
                    );
                    choice(
                        tb,
                        t("Read Convention"),
                        &mut d.t.read,
                        &[(Read::UpThenLeft, msg!("Up, then Left")), (Read::UpThenRight, msg!("Up, then Right"))],
                    );
                    let mut units: Vec<(Option<Units>, &str)> = vec![(None, msg!("Use Project Settings"))];
                    units.extend(Units::ALL.iter().map(|u| (Some(*u), u.label())));
                    choice(tb, t("Units Format"), &mut d.t.units, &units);
                    let mut places: Vec<(Option<u8>, String)> = vec![(None, t("As the units show").to_string())];
                    places.extend((0..=6u8).map(|n| (Some(n), n.to_string())));
                    let options: Vec<(Option<u8>, &str)> = places.iter().map(|(v, l)| (*v, l.as_str())).collect();
                    choice(tb, t("Decimal Places"), &mut d.t.decimals, &options);
                    tb.text(t("Radius Symbol"), &mut d.t.radius_symbol, "R");
                    tb.text(t("Diameter Symbol"), &mut d.t.diameter_symbol, "Ø");
                    tb.text(t("Equality Text"), &mut d.t.equality_text, "EQ");
                });
                typeprops::error(ui, d.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                (ok, cancel, apply) = typeprops::foot(ui);
            });
        });
        if cancel || r.should_close() {
            return;
        }
        match asked {
            Some(Head::Switch(name)) => {
                d = DimensionTypeDialog { elements: d.elements, ..DimensionTypeDialog::new(&model, name, Vec::new()) }
            }
            Some(Head::Duplicate(name)) => match self.exec("dimension_type", &d.params(&name)) {
                Ok(_) => {
                    d = DimensionTypeDialog { elements: d.elements, ..DimensionTypeDialog::new(&self.session.model, name, Vec::new()) }
                }
                Err(e) => d.error = Some(e),
            },
            Some(Head::Rename(to)) => match self.exec("dimension_type", &json!({"name": d.name, "rename": to})) {
                Ok(_) => {
                    if self.types.get("dimension") == Some(&d.name) {
                        self.types.insert("dimension", to.clone());
                    }
                    let typed = d.t.clone();
                    d = DimensionTypeDialog { elements: d.elements, ..DimensionTypeDialog::new(&self.session.model, to, Vec::new()) };
                    d.t = typed;
                }
                Err(e) => d.error = Some(e),
            },
            None => {}
        }
        if ok || apply {
            let changed = model.dimension_types.get(&d.name) != Some(&d.t);
            let mut result = if changed { self.exec("dimension_type", &d.params(&d.name)).map(|_| ()) } else { Ok(()) };
            let moved: Vec<Id> = d
                .elements
                .iter()
                .copied()
                .filter(|id| matches!(self.session.model.elements.get(*id), Some(Element::Dimension(x)) if x.type_name != d.name))
                .collect();
            if ok && result.is_ok() && !moved.is_empty() {
                result = self.exec("modify", &json!({"ids": moved, "type": d.name})).map(|_| ());
            }
            match result {
                Ok(()) if ok => return,
                Ok(()) => d.error = None,
                Err(e) => d.error = Some(e),
            }
        }
        self.dimension_type_dialog = Some(d);
    }
}

/// Dimension Text, as Revit's: what one number of a dimension has before and after it, over and under it, or in its place.
/// A dimension of one number edits its own text; of several, the number double-clicked's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DimensionTextDialog {
    id: Id,
    /// Which number, when the dimension has several.
    segment: Option<usize>,
    /// Prefix, suffix, above, below and the replacement, as typed.
    typed: [String; 5],
    error: Option<String>,
}

const FIELDS: [(&str, &str); 5] = [
    ("prefix", msg!("Prefix:")),
    ("suffix", msg!("Suffix:")),
    ("above", msg!("Above:")),
    ("below", msg!("Below:")),
    ("replace", msg!("Replace With Text:")),
];

impl DimensionTextDialog {
    pub(crate) fn new(m: &Model, id: Id, segment: usize) -> DimensionTextDialog {
        let (segment, typed) = match m.elements.get(id) {
            Some(Element::Dimension(d)) => {
                let several = m.dimension_labels(d).len() > 1;
                let segment = several.then_some(segment);
                // A segment's own text; the dimension's shows through where it has none, as it draws.
                let typed = FIELDS.map(|(f, _)| match segment {
                    Some(i) => d.texts.iter().find(|t| t.segment == i).map_or_else(String::new, |t| {
                        t.fields().into_iter().find(|(n, _)| *n == f).map_or_else(String::new, |(_, v)| v.clone())
                    }),
                    None => d.text(0, f).to_string(),
                });
                (segment, typed)
            }
            _ => (None, Default::default()),
        };
        DimensionTextDialog { id, segment, typed, error: None }
    }

    /// The `dimension_text` command for what is typed.
    fn params(&self) -> Value {
        let mut p = json!({"id": self.id});
        if let Some(i) = self.segment {
            p["segment"] = json!(i);
        }
        for ((f, _), t) in FIELDS.iter().zip(&self.typed) {
            p[*f] = json!(t);
        }
        p
    }
}

impl BimApp {
    /// Dimension Text, while it's open.
    pub(crate) fn dimension_text_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.dimension_text_dialog.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("dimension-text"), t("Dimension Text")).width(580.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                if let Some(i) = d.segment {
                    ui.label(tf("Segment {}", &[&(i + 1).to_string()]));
                }
                Grid::new("dimension-text-fields").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    for ((_, label), typed) in FIELDS.iter().zip(d.typed.iter_mut()) {
                        let l = ui.label(t(label));
                        ui.add_sized([260.0, ui.spacing().interact_size.y], TextEdit::singleline(typed)).labelled_by(l.id);
                        ui.end_row();
                    }
                });
                if let Some(e) = &d.error {
                    ui.colored_label(crate::style::pal().error, e);
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    ok = ui.button(t("OK")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if cancel || r.should_close() {
            return;
        }
        if ok {
            match self.exec("dimension_text", &d.params()) {
                Ok(_) => return,
                Err(e) => d.error = Some(e),
            }
        }
        self.dimension_text_dialog = Some(d);
    }
}
