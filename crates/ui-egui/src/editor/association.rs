//! Revit's "=" on the rows of a family's own forms and symbols (its numbers, Yes/No and materials): Associate Family Parameter,
//! the dialog an adaptive element's rows open too (`adaptive_editor::associate`), through `adaptive_editor::Tied`. A row tied to
//! a parameter holds its name, shows what it comes to for the type previewed, greyed, and takes no typing; untied with <None>,
//! it keeps what it came to.
use super::*;
use crate::adaptive_editor::{Associating, Field, Slot, Tied, associating, equals, worth};
use bimcraft_core::family::Val;

/// What a row of a form or a symbol comes to for `values`, as typed: a number worked out, a material a parameter holds, else
/// as it is.
fn worked_out(text: &str, kind: Holds, values: &BTreeMap<String, Val>) -> String {
    match kind {
        Holds::Material => match values.get(text.trim()) {
            Some(Val::Text(m)) => m.clone(),
            _ => text.to_string(),
        },
        Holds::YesNo => (if shows(text, values) { "1" } else { "0" }).into(),
        _ => worth(&text_expr(text), values).map(number_text).unwrap_or_else(|| text.to_string()),
    }
}

/// The "=" of the rows of a form's or a symbol's fields, drawn: what they may be tied to, and the association asked for.
pub(super) struct Controls {
    params: Vec<(String, Holds)>,
    values: BTreeMap<String, Val>,
    /// Associate Family Parameter, when an "=" was pressed.
    pub(super) request: Option<Associating>,
}

impl Controls {
    pub(super) fn new(ed: &FamilyEditor) -> Controls {
        Controls { params: ed.parameter_kinds(), values: ed.values(), request: None }
    }

    /// Whether what is typed in a row holding a `kind` is a parameter of that kind's name: the row is tied to it.
    pub(super) fn bound(&self, text: &str, kind: Holds) -> bool {
        self.params.iter().any(|(n, k)| n == text.trim() && *k == kind)
    }

    /// A row's field, `label` naming it, with Revit's "=" at its end: tied, it shows what the parameter comes to, greyed.
    /// `row` is which of its form's or symbol's fields it is, and that form's or symbol's place among them.
    pub(super) fn field(&mut self, ui: &mut Ui, label: String, text: &mut String, width: f32, kind: Holds, row: (Field, usize)) {
        ui.horizontal(|ui| {
            if self.bound(text, kind) {
                let mut shown = worked_out(text, kind, &self.values);
                ui.add_enabled_ui(false, |ui| super::field(ui, label.clone(), &mut shown, width));
            } else {
                super::field(ui, label.clone(), text, width);
            }
            self.button(ui, label, text, kind, row);
        });
    }

    /// Revit's "=" alone, for a row drawn otherwise (a material's list).
    pub(super) fn button(&mut self, ui: &mut Ui, label: String, text: &str, kind: Holds, (row, index): (Field, usize)) {
        if equals(ui, &label, self.bound(text, kind)) {
            self.request = Some(associating(&index.to_string(), row, kind, &label, Some(text.trim()), &self.params));
        }
    }
}

/// Associate Family Parameter's rows of a family's own forms and symbols, each by its place among them.
pub(super) struct Rows<'a> {
    pub(super) forms: &'a mut [FormText],
    pub(super) symbols: &'a mut [SymbolText],
}

impl Rows<'_> {
    /// What is typed in the row `a` is open on.
    fn text(&mut self, a: &Associating) -> Option<&mut String> {
        let i = a.id.parse::<usize>().ok()?;
        match a.field {
            Field::Form(slot) => {
                let f = self.forms.get_mut(i)?;
                match slot {
                    Slot::From => Some(&mut f.from),
                    Slot::To => Some(&mut f.to),
                    Slot::Visible => Some(&mut f.visible),
                    Slot::Material => Some(&mut f.material),
                    Slot::Centre(k) => f.circle.get_mut(k),
                    Slot::Axis(k) => f.axis.get_mut(k),
                    Slot::Radius => f.circle.get_mut(2),
                }
            }
            Field::Symbol(slot) => {
                let y = self.symbols.get_mut(i)?;
                match slot {
                    Slot::From => Some(&mut y.from),
                    Slot::To => Some(&mut y.to),
                    Slot::Visible => Some(&mut y.visible),
                    Slot::Centre(k) => y.centre.get_mut(k),
                    Slot::Radius => Some(&mut y.radius),
                    Slot::Material | Slot::Axis(_) => None,
                }
            }
            _ => None,
        }
    }
}

impl Tied for Rows<'_> {
    fn tie(&mut self, a: &Associating, values: &BTreeMap<String, Val>) {
        let Some(text) = self.text(a) else { return };
        *text = match &a.chosen {
            Some(p) => p.clone(),
            // Untied, a Yes/No is shown unless it came to no: blank is what a form or a symbol is unless it says otherwise.
            None if a.kind == Holds::YesNo => (if shows(text, values) { "" } else { "0" }).into(),
            None => worked_out(text, a.kind, values),
        };
    }

    fn value_now(&mut self, a: &Associating, values: &BTreeMap<String, Val>) -> String {
        self.text(a).map(|text| worked_out(text, a.kind, values)).unwrap_or_default()
    }
}
