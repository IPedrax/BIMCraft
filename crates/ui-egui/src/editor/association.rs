//! Parameter associations shared by expression-valued form and symbol properties.
use super::*;
use bimcraft_core::family::{Val, eval, nameable};

#[derive(Clone, Copy)]
pub(super) enum Slot {
    From,
    To,
    Visible,
    Material,
    Circle(usize),
    Axis(usize),
    Radius,
}
#[derive(Clone, Copy)]
pub(super) enum Target {
    Form(usize, Slot),
    Symbol(usize, Slot),
}
impl Target {
    fn text(self, ed: &mut FamilyEditor) -> Option<&mut String> {
        match self {
            Self::Form(i, slot) => {
                let f = ed.forms.get_mut(i)?;
                match slot {
                    Slot::From => Some(&mut f.from),
                    Slot::To => Some(&mut f.to),
                    Slot::Visible => Some(&mut f.visible),
                    Slot::Material => Some(&mut f.material),
                    Slot::Circle(k) => f.circle.get_mut(k),
                    Slot::Axis(k) => f.axis.get_mut(k),
                    _ => None,
                }
            }
            Self::Symbol(i, slot) => {
                let y = ed.symbols.get_mut(i)?;
                match slot {
                    Slot::From => Some(&mut y.from),
                    Slot::To => Some(&mut y.to),
                    Slot::Visible => Some(&mut y.visible),
                    Slot::Circle(k) => y.centre.get_mut(k),
                    Slot::Radius => Some(&mut y.radius),
                    _ => None,
                }
            }
        }
    }
}

pub(super) struct Choice {
    target: Target,
    kind: Holds,
    label: String,
    selected: String,
    literal: Result<String, String>,
    new_name: Option<String>,
    instance: bool,
    error: Option<String>,
    shown: bool,
}

pub(super) struct Controls {
    params: Vec<ParamText>,
    values: BTreeMap<String, Val>,
    pub(super) request: Option<Choice>,
}
impl Controls {
    pub(super) fn new(ed: &FamilyEditor) -> Self {
        let values =
            ed.family().and_then(|f| f.values(ed.types.get(ed.picked).map_or("", |(n, _)| n), &BTreeMap::new())).unwrap_or_default();
        Self { params: ed.params.clone(), values, request: None }
    }
    pub(super) fn bound(&self, text: &str, kind: Holds) -> bool {
        self.params.iter().any(|p| p.name == text.trim() && p.kind == kind)
    }
    fn literal(&self, text: &str, kind: Holds) -> Result<String, String> {
        if kind == Holds::Material {
            return Ok(match self.values.get(text.trim()) {
                Some(Val::Text(v)) => v.clone(),
                _ => text.into(),
            });
        }
        if text.trim().is_empty() {
            return Ok(if kind == Holds::YesNo { "1" } else { "0" }.into());
        }
        eval(text, &mut |name| match self.values.get(name) {
            Some(Val::Number(x)) => Ok(*x),
            _ => Err(tf("{} is not a numeric parameter", &[name])),
        })
        .map(number_text)
    }
    pub(super) fn field(&mut self, ui: &mut Ui, label: String, text: &mut String, width: f32, kind: Holds, target: Target) {
        ui.horizontal(|ui| {
            if self.bound(text, kind) {
                let mut shown = self.literal(text, kind).unwrap_or_else(|_| text.clone());
                ui.add_enabled_ui(false, |ui| field(ui, label.clone(), &mut shown, width));
            } else {
                field(ui, label.clone(), text, width);
            }
            self.button(ui, label, text, kind, target);
        });
    }
    pub(super) fn button(&mut self, ui: &mut Ui, label: String, text: &str, kind: Holds, target: Target) {
        let response = ui.small_button("=").on_hover_text(t("Associate Family Parameter"));
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Associate {}", &[&label])));
        if response.clicked() {
            self.request = Some(Choice {
                target,
                kind,
                label,
                selected: if self.bound(text, kind) { text.trim().into() } else { String::new() },
                literal: self.literal(text, kind),
                new_name: None,
                instance: false,
                error: None,
                shown: false,
            });
        }
    }
}

pub(super) fn dialog(ctx: &Context, ed: &mut FamilyEditor) {
    let Some(mut choice) = ed.document.association.take() else { return };
    let (mut ok, mut cancel) = (false, false);
    let adding = choice.new_name.is_some();
    let response = Modal::new(UiId::new("family-association"), if adding { t("New Parameter") } else { t("Associate Family Parameter") })
        .width(440.)
        .show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(&choice.label);
                if let Some(name) = &mut choice.new_name {
                    field(ui, t("Parameter name").into(), name, 280.);
                    ui.label(t(kind_name(choice.kind)));
                    ui.checkbox(&mut choice.instance, t("Instance Parameter"));
                } else {
                    ui.selectable_value(&mut choice.selected, String::new(), t("<None>"));
                    for p in ed.params.iter().filter(|p| p.kind == choice.kind) {
                        ui.selectable_value(&mut choice.selected, p.name.clone(), &p.name);
                    }
                    if ui.button(t("New Parameter…")).clicked() {
                        choice.new_name = Some(String::new());
                    }
                }
                if let Some(error) = &choice.error {
                    ui.label(RichText::new(error).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ok = ui.button(t("OK")).clicked();
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
    let was_shown = choice.shown;
    choice.shown = true;
    if cancel || (was_shown && response.should_close()) {
        if adding {
            choice.new_name = None;
            choice.error = None;
            ed.document.association = Some(choice);
        }
        return;
    }
    if ok {
        if let Some(name) = choice.new_name.as_ref() {
            let name = name.trim();
            if !nameable(name) || ed.params.iter().any(|p| p.name == name) {
                choice.error = Some(t("Choose a unique parameter name starting with a letter.").into());
            } else {
                match &choice.literal {
                    Ok(value) => {
                        ed.params.push(ParamText {
                            original: Parameter::default(),
                            name: name.into(),
                            kind: choice.kind,
                            instance: choice.instance,
                            value: value.clone(),
                            formula: String::new(),
                        });
                        choice.selected = name.into();
                        choice.new_name = None;
                        choice.error = None;
                    }
                    Err(error) => choice.error = Some(error.clone()),
                }
            }
        } else {
            let value = if choice.selected.is_empty() { choice.literal.clone() } else { Ok(choice.selected.clone()) };
            match value {
                Ok(value) => {
                    if let Some(text) = choice.target.text(ed) {
                        *text = value;
                    }
                    return;
                }
                Err(error) => choice.error = Some(error),
            }
        }
    }
    ed.document.association = Some(choice);
}
