//! Grid Type Properties, the dialog Edit Type opens for a grid, as Revit's: its graphics (the line pattern, weight and
//! colour) and its text (font, size, lettering and width factor), Duplicate and Rename, and OK, Cancel and Apply.

use crate::popup::Modal;

use bimcraft_core::grids::GridType;
use bimcraft_core::lines::SOLID;
use bimcraft_core::model::{Element, Id, Model};
use egui::{ComboBox, Context, DragValue, Id as UiId, TextEdit};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::typeprops::{self, Head, Naming};

/// Grid Type Properties: one grid type's values, and the grids it opened for, which OK puts on the type shown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GridTypeDialog {
    name: String,
    t: GridType,
    elements: Vec<Id>,
    naming: Naming,
    error: Option<String>,
    find: String,
}

impl GridTypeDialog {
    pub(crate) fn new(m: &Model, name: String, elements: Vec<Id>) -> GridTypeDialog {
        let t = m.grid_types.get(&name).cloned().unwrap_or_default();
        GridTypeDialog { name, t, elements, naming: Naming::None, error: None, find: String::new() }
    }

    /// The `grid_type` command for the type as typed, under `name` (from the one shown, for a duplicate).
    fn params(&self, name: &str) -> Value {
        let mut p = serde_json::to_value(&self.t).unwrap_or_else(|_| json!({}));
        if let Some(o) = p.as_object_mut() {
            o.insert("name".into(), json!(name));
            o.insert("like".into(), json!(self.name));
            // A solid line is no pattern at all, which a file leaves out: say so, so changing back applies.
            if self.t.line_pattern.is_none() {
                o.insert("line_pattern".into(), Value::Null);
            }
        }
        p
    }
}

impl BimApp {
    /// The type a new grid is drawn in: the one the Type Selector names, while the project has it, else the default.
    pub(crate) fn grid_type_for(&self) -> Option<String> {
        let types = &self.session.model.grid_types;
        match self.types.get("grid") {
            Some(name) if types.contains_key(name) => Some(name.clone()),
            _ => bimcraft_core::grids::default_type(types, self.session.model.language).cloned(),
        }
    }

    /// Grid Type Properties, while it's open.
    pub(crate) fn grid_type_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.grid_type_dialog.take() else { return };
        let model = std::sync::Arc::clone(&self.session.model);
        let names: Vec<String> = model.grid_types.keys().cloned().collect();
        let fonts = bimcraft_core::text::families();
        let mut patterns: Vec<String> = vec![t(SOLID).to_string()];
        patterns.extend(model.line_patterns.keys().cloned());
        let (mut ok, mut cancel, mut apply, mut asked) = (false, false, false, None);
        let r = Modal::new(UiId::new("grid-type-properties"), t("Type Properties")).width(crate::typeprops::WIDTH).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                asked = typeprops::head(ui, "grid-type", &tf("System Family: {}", &[t("Grid")]), &names, &d.name, &mut d.naming, true);
                let below = typeprops::foot_height(ui, d.error.is_some());
                typeprops::table(ui, "grid-type", below, |tb| {
                    tb.group("Graphics");
                    let mut pattern = d.t.line_pattern.clone().unwrap_or_else(|| t(SOLID).to_string());
                    tb.pick(t("Line Pattern"), &mut pattern, &patterns);
                    d.t.line_pattern = (pattern != t(SOLID)).then_some(pattern);
                    tb.drag(t("Line Weight"), DragValue::new(&mut d.t.line_pen.0).range(0..=bimcraft_core::pen::MAX_PEN));
                    tb.color(t("Color"), &mut d.t.color);
                    tb.group("Text");
                    let font = &mut d.t.font;
                    let find = &mut d.find;
                    tb.row(t("Text Font"), |ui, l| {
                        ComboBox::from_id_salt("grid-type-font")
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
                    // What letters it on this system, where that is another font.
                    let drawn = bimcraft_core::text::resolve(&d.t.font, false, false);
                    if !drawn.family().eq_ignore_ascii_case(d.t.font.trim()) {
                        tb.shown("", &tf("Not on this system: drawn with {}", &[drawn.family()]));
                    }
                    tb.drag(t("Text Size"), DragValue::new(&mut d.t.text_size).range(0.5..=50.0).speed(0.1).max_decimals(2).suffix(" mm"));
                    tb.check(t("Bold"), &mut d.t.bold);
                    tb.check(t("Italic"), &mut d.t.italic);
                    tb.check(t("Underline"), &mut d.t.underline);
                    tb.drag(t("Width Factor"), DragValue::new(&mut d.t.width_factor).range(0.1..=10.0).speed(0.01).max_decimals(2));
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
            Some(Head::Switch(name)) => d = GridTypeDialog { elements: d.elements, ..GridTypeDialog::new(&model, name, Vec::new()) },
            Some(Head::Duplicate(name)) => match self.exec("grid_type", &d.params(&name)) {
                Ok(_) => d = GridTypeDialog { elements: d.elements, ..GridTypeDialog::new(&self.session.model, name, Vec::new()) },
                Err(e) => d.error = Some(e),
            },
            Some(Head::Rename(to)) => match self.exec("grid_type", &json!({"name": d.name, "rename": to})) {
                Ok(_) => {
                    if self.types.get("grid") == Some(&d.name) {
                        self.types.insert("grid", to.clone());
                    }
                    // What was typed and not yet applied stays.
                    let typed = d.t.clone();
                    d = GridTypeDialog { elements: d.elements, ..GridTypeDialog::new(&self.session.model, to, Vec::new()) };
                    d.t = typed;
                }
                Err(e) => d.error = Some(e),
            },
            None => {}
        }
        if ok || apply {
            let changed = model.grid_types.get(&d.name) != Some(&d.t);
            let mut result = if changed { self.exec("grid_type", &d.params(&d.name)).map(|_| ()) } else { Ok(()) };
            let moved: Vec<Id> = d
                .elements
                .iter()
                .copied()
                .filter(|id| matches!(self.session.model.elements.get(*id), Some(Element::Grid(g)) if g.type_name != d.name))
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
        self.grid_type_dialog = Some(d);
    }
}
