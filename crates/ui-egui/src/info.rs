//! The tool palette's info panel for a selection (`BimApp::tool_palette`): what is selected (its category and type), its type's total
//! thickness and layers, read only, and Edit Type, which changes them. With a tool in hand the panel shows the tool's options instead
//! (`BimApp::options`). The rest of a selection's settings are the Properties palette's.

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::units::Units;
use egui::{Align, Layout, RichText, Ui};

use crate::lang::{self, t, tf};
use crate::properties::function_name;
use crate::style::pal;
use crate::{BimApp, Tool};

/// A length as the summary says it: in the project's units, with the unit's name where the number has none (millimetres, not feet).
fn length(units: Units, mm: f64) -> String {
    let n = units.length(mm);
    if n.ends_with(|c: char| c.is_ascii_digit()) { format!("{n} {}", units.name()) } else { n }
}

impl BimApp {
    /// Whether the info panel has a selection to describe: elements picked, Modify in hand, nothing being sketched or written.
    pub(crate) fn selection_info_shown(&self) -> bool {
        matches!(self.tool, Tool::Modify) && self.sketch.is_none() && self.note_edit.is_none() && !self.selection.is_empty()
    }

    /// Whether a tool has options of its own to show (a tool in hand, a note being written, a sketch), not only a name.
    pub(crate) fn has_options(&self) -> bool {
        !matches!(self.tool, Tool::Modify) || self.note_edit.is_some() || self.sketch.is_some()
    }

    /// Whether the info panel has anything to show.
    pub(crate) fn info_shown(&self) -> bool {
        self.has_options() || self.selection_info_shown()
    }

    /// The selection's summary: its category and type, the type's total thickness and its layers (a wall's from its exterior in, the
    /// others' from the top down), and Edit Type.
    pub(crate) fn selection_info(&mut self, ui: &mut Ui) {
        let model = Arc::clone(&self.session.model);
        let Some((category, name, ids)) = self.type_subject() else {
            // Elements of several categories: no one type to say.
            let categories: BTreeSet<&str> = self.selection.iter().filter_map(|id| model.elements.get(*id)).map(|e| e.category()).collect();
            ui.label(RichText::new(tf("Multiple Categories ({})", &[&categories.len().to_string()])).strong());
            return;
        };
        let several = ids.len() > 1;
        let what = lang::category(category, several);
        let what = if several { format!("{what} ({})", ids.len()) } else { what };
        ui.label(RichText::new(format!("{what} \u{b7} {name}")).strong());
        if let Some((thickness, layers)) = model.layered_types().find(|(c, n, ..)| *c == category && *n == name).map(|(_, _, t, l)| (t, l))
        {
            let total = if layers.is_empty() { thickness } else { layers.iter().map(|l| l.thickness).sum() };
            ui.label(tf("Total thickness: {}", &[&length(model.units, total)]));
            for layer in layers {
                ui.horizontal(|ui| {
                    let named =
                        if layer.material.is_empty() { t(function_name(layer.function)).to_string() } else { layer.material.clone() };
                    ui.label(named);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(model.units.length(layer.thickness)));
                });
            }
        }
        ui.add_space(4.0);
        if ui.button(t("Edit Type")).clicked() {
            self.edit_type();
        }
        ui.label(RichText::new(t("Or double-click the element")).color(pal().muted).size(11.5));
    }
}
