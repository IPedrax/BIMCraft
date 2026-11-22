//! Manage > Purge Unused in a family document (FE-19): what nothing in the family uses (`Family::unused`), its parameters and
//! the families nested in it, offered unticked; OK takes the ticked ones out of the draft, one step of the family's history.

use egui::{Align, Context, Id as UiId, Layout, RichText};

use super::FamilyEditor;
use crate::lang::t;
use crate::popup::Modal;
use crate::style::pal;
use bimcraft_core::family::purge::Unused;

/// Purge Unused, open: each parameter and nested family offered, and whether it is ticked.
#[derive(Clone, Debug, Default)]
pub(crate) struct Purging {
    parameters: Vec<(String, bool)>,
    nested: Vec<(String, bool)>,
}

impl Purging {
    fn of(u: Unused) -> Purging {
        Purging {
            parameters: u.parameters.into_iter().map(|n| (n, false)).collect(),
            nested: u.nested.into_iter().map(|n| (n, false)).collect(),
        }
    }
}

impl FamilyEditor {
    /// Purge Unused opened on the draft: `held` says which parameters a placed component of the project's holds a value of,
    /// `has_material` which materials the project has.
    pub(crate) fn open_purge(&mut self, held: &dyn Fn(&str) -> bool, has_material: &dyn Fn(&str) -> bool) {
        if let Ok(f) = self.family() {
            self.purging = Some(Purging::of(f.unused(held, has_material)));
        }
    }

    /// Whether Purge Unused is open.
    pub(crate) fn purging(&self) -> bool {
        self.purging.is_some()
    }

    /// Purge Unused, while it is open: OK takes what is ticked out of the draft.
    pub(crate) fn purge_dialog(&mut self, ctx: &Context) {
        let Some(mut d) = self.purging.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("family-purge"), t("Purge Unused")).width(380.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                if d.parameters.is_empty() && d.nested.is_empty() {
                    ui.label(t("Nothing in the family is unused."));
                    return;
                }
                ui.label(t("Tick what to take out of the family. Only what nothing in it uses is listed."));
                ui.add_space(6.0);
                for (title, list) in [(t("Parameters"), &mut d.parameters), (t("Nested Families"), &mut d.nested)] {
                    if list.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(title).strong());
                    for (name, ticked) in list.iter_mut() {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.checkbox(ticked, name.as_str());
                        });
                    }
                    ui.add_space(4.0);
                }
                ui.label(
                    RichText::new(t("It goes from the family's draft; Load into Project takes it out of the project's."))
                        .small()
                        .color(pal().muted),
                );
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if ok {
            let picked = |l: &[(String, bool)]| l.iter().filter(|(_, on)| *on).map(|(n, _)| n.clone()).collect::<Vec<_>>();
            self.purge_draft(&picked(&d.parameters), &picked(&d.nested));
        } else if !(cancel || r.should_close()) {
            self.purging = Some(d);
        }
    }

    /// Parameters `parameters` and nested families `nested` taken out of the draft, a parameter's values in each type with it.
    fn purge_draft(&mut self, parameters: &[String], nested: &[String]) {
        self.params.retain(|p| !parameters.contains(&p.name));
        for (_, values) in self.types.iter_mut() {
            values.retain(|k, _| !parameters.contains(k));
        }
        self.nested.retain(|n| !nested.contains(&n.name));
    }

    /// Whether the draft has parameter `name`.
    #[cfg(test)]
    pub(crate) fn has_parameter(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.name == name)
    }
}
