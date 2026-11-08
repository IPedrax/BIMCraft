//! F8: Revit's Family Element Visibility Settings in the family document, for a form (its contextual tab's Visibility
//! Settings, or Properties' Visibility/Graphics Overrides Edit…), a nested instance and a symbol (its row's Visibility…, the
//! detail levels only, as Revit's symbolic lines have them). View Specific Display (Plan/RCP, Front/Back, Left/Right, Only
//! shown when cut) and Detail Levels (Coarse, Medium, Fine); Default ticks them as a new element is, OK writes them to the
//! draft (a step of its history), Cancel leaves them. What the dialog sets is `bimcraft_core::family::visibility`'s: drawing
//! only, never the model's or IFC's.
use super::*;
use bimcraft_core::family::Visibility;
use bimcraft_core::family::visibility::SYMBOL_DEFAULT;

/// What the dialog sets the Visibility Settings of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Form(usize),
    Nested(usize),
    Symbol(usize),
}

/// The dialog, open: what it sets and what is ticked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dialog {
    pub(crate) target: Target,
    pub(crate) ticked: Visibility,
}

impl FamilyEditor {
    /// What `target` has now, as the dialog ticks it: none is Revit's default, everything ticked (a symbol's, Medium and Fine).
    pub(crate) fn visibility_of(&self, target: Target) -> Option<Visibility> {
        match target {
            Target::Form(i) => self.forms.get(i).map(|f| f.original.visibility.unwrap_or_default()),
            Target::Nested(i) => self.nested_instances.get(i).map(|n| n.visibility.unwrap_or_default()),
            Target::Symbol(i) => self.symbols.get(i).map(|y| y.visibility.unwrap_or(SYMBOL_DEFAULT)),
        }
    }

    /// The dialog opened on `target`, ticked as it is now.
    pub(crate) fn open_visibility(&mut self, target: Target) {
        if let Some(ticked) = self.visibility_of(target) {
            self.views.visibility = Some(Dialog { target, ticked });
        }
    }

    /// `v` given to `target`, in the draft. Left as it was when it ticks what it has.
    pub(crate) fn set_visibility(&mut self, target: Target, v: Visibility) {
        if self.visibility_of(target) == Some(v) {
            return;
        }
        match target {
            Target::Form(i) => {
                if let Some(f) = self.forms.get_mut(i) {
                    f.original.visibility = Some(v);
                }
            }
            Target::Nested(i) => {
                if let Some(n) = self.nested_instances.get_mut(i) {
                    n.visibility = Some(v);
                }
            }
            Target::Symbol(i) => {
                if let Some(y) = self.symbols.get_mut(i) {
                    y.visibility = Some(v);
                }
            }
        }
    }

    /// The dialog, while open.
    pub(crate) fn visibility_dialog(&mut self, ctx: &Context) {
        let Some(mut d) = self.views.visibility else { return };
        let symbol = matches!(d.target, Target::Symbol(_));
        let (mut ok, mut cancel, mut default) = (false, false, false);
        let title = t("Family Element Visibility Settings");
        let r = Modal::new(UiId::new("family-visibility"), title).width(380.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                let v = &mut d.ticked;
                ui.label(RichText::new(t("View Specific Display")).strong());
                let off = msg!("A symbol is drawn only in plan: only its detail levels change");
                ui.add_enabled_ui(!symbol, |ui| {
                    ui.label(t("Display in 3D views and:"));
                    for (b, label) in
                        [(&mut v.plan, msg!("Plan/RCP")), (&mut v.front_back, msg!("Front/Back")), (&mut v.left_right, msg!("Left/Right"))]
                    {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.checkbox(b, t(label));
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.checkbox(&mut v.only_when_cut, t("Only shown when cut"));
                    });
                })
                .response
                .on_disabled_hover_text(t(off));
                ui.add_space(8.0);
                ui.label(RichText::new(t("Detail Levels")).strong());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.checkbox(&mut v.coarse, t("Coarse"));
                    ui.checkbox(&mut v.medium, t("Medium"));
                    ui.checkbox(&mut v.fine, t("Fine"));
                });
                ui.add_space(4.0);
                ui.label(RichText::new(t("What a view leaves out stays in the model, its schedules and IFC.")).small().color(pal().muted));
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                    default = ui.button(t("Default")).clicked();
                });
            });
        });
        if default {
            d.ticked = if symbol { SYMBOL_DEFAULT } else { Visibility::default() };
        }
        if ok {
            self.views.visibility = None;
            self.set_visibility(d.target, d.ticked);
        } else if cancel || r.should_close() {
            self.views.visibility = None;
        } else {
            self.views.visibility = Some(d);
        }
    }
}
