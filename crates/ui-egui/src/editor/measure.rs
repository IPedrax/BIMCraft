//! Measure (the Measure panel's) and Create Similar in a family's plans and elevations, as a project's views have them.
//! Measure lays a ruler by two clicks, snapped as the Modify tools' points are, and reads it in the status
//! bar and over the view (its length, its angle from the view's across, and its parts across and up), changing nothing; the
//! ruler stays on show until the next first click, Escape or another tool, as the project's Measure does. Create Similar
//! starts the tool that makes the one thing selected, as it is: a form's Create (its kind, solid or void), Reference Plane for
//! a plane, the dimension tool of a dimension's kind, another symbol of a symbol's part, a connector of a connector's kind,
//! and Component with a nested instance's family and type. Neither is a step of the document's history.
use super::select_box::Picked;
use super::*;
use bimcraft_core::family::DimKind;
use bimcraft_core::family::views::Which;
use bimcraft_core::geom::P2;
use bimcraft_core::snap::{Ruler, ruler};

/// The Measure tool, on in a plan or an elevation: its first click once made, and the ruler last laid (its ends, the view's
/// millimetres, and the view it was laid in).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Measuring {
    pub(crate) first: Option<P2>,
    pub(crate) laid: Option<(P2, P2, Which)>,
}

/// What a ruler reads, as the status bar and the view say it: its length, its angle and its parts, in the project's units.
pub(super) fn read(r: &Ruler, units: bimcraft_core::units::Units) -> String {
    tf(
        "Measure: {}, {}°, across {}, up {}",
        &[&units.with_unit(r.length), &format!("{:.2}", r.angle), &units.with_unit(r.dx), &units.with_unit(r.dy)],
    )
}

impl FamilyEditor {
    /// Measure: the tool, in a plan or an elevation of a family that isn't adaptive, while nothing is sketched; the
    /// selection goes, as with any tool.
    pub(crate) fn measure_tool(&mut self) {
        if self.editing_2d() && self.sketch.is_none() {
            self.drop_tools();
            self.views.measuring = Some(Measuring::default());
        }
    }

    /// Whether the Measure tool is on.
    pub(crate) fn measuring(&self) -> bool {
        self.views.measuring.is_some()
    }

    /// What the ruler last laid reads; none before the second click, or once another first click begins one.
    #[cfg(test)]
    pub(crate) fn measured(&self) -> Option<Ruler> {
        self.views.measuring.as_ref().and_then(|m| m.laid).map(|(a, b, _)| ruler(a, b))
    }

    /// A click of the Measure tool at `q` (the view's millimetres, snapped) in view `which`: its first, or its second, which
    /// lays the ruler and says what it reads.
    pub(super) fn measure_click(&mut self, q: P2, which: Which, units: bimcraft_core::units::Units) -> Option<String> {
        let m = self.views.measuring.as_mut()?;
        match m.first {
            Some(a) if a.dist(q) > 1e-6 => {
                m.first = None;
                m.laid = Some((a, q, which));
                Some(read(&ruler(a, q), units))
            }
            Some(_) => None,
            None => {
                m.first = Some(q);
                m.laid = None;
                None
            }
        }
    }

    /// Escape with the Measure tool on: the first click let go, else the tool put down. Whether it was on.
    pub(super) fn measure_escape(&mut self) -> bool {
        match self.views.measuring.as_mut() {
            Some(m) if m.first.is_some() => m.first = None,
            Some(_) => self.views.measuring = None,
            None => return false,
        }
        true
    }

    /// Create Similar: the tool that makes the one thing selected, as it is; nothing with none or several selected,
    /// while a form is sketched or in an adaptive family. Why not, where it can't.
    pub(crate) fn create_similar(&mut self) -> Result<(), String> {
        if self.adaptive.is_some() || self.sketch.is_some() {
            return Ok(());
        }
        let Some(p) = self.only_selected().filter(|p| self.holds(*p)) else { return Ok(()) };
        // Their tools draw in a plan or an elevation.
        if matches!(p, Picked::Plane(_) | Picked::Line(_) | Picked::Dimension(_)) && !self.editing_2d() {
            let why = msg!("Create Similar makes a reference plane, a reference line or a dimension in a plan or an elevation.");
            return Err(t(why).to_string());
        }
        match p {
            Picked::Form(i) => {
                let f = self.forms.get(i).ok_or_else(|| t("the form is gone").to_string())?.form()?;
                if !f.mesh.is_empty() {
                    return Err(t("A form of a file's triangles has no tool that makes another.").to_string());
                }
                self.create_form(f.kind(), f.void);
            }
            Picked::Plane(_) => self.reference_plane_tool(),
            Picked::Line(_) => self.reference_line_tool(),
            Picked::Dimension(g) => match self.dimensions.iter().find(|d| d.guid == g).map(|d| d.kind) {
                Some(DimKind::Aligned) | None => self.dimension_tool(),
                Some(kind) => self.radial_tool(kind),
            },
            Picked::Symbol(i) => {
                let part = self.symbols.get(i).map(|s| s.part).ok_or_else(|| t("the symbol is gone").to_string())?;
                self.add_symbol(part);
            }
            Picked::Connector(i) => {
                let kind = self.connectors.get(i).map(|c| c.kind()).ok_or_else(|| t("the connector is gone").to_string())?;
                self.add_connector(kind);
            }
            Picked::Nested(i) => {
                let n = self.nested_instances.get(i).ok_or_else(|| t("the nested instance is gone").to_string())?;
                let (family, type_name) = (n.family.clone(), n.type_name.clone());
                self.component_tool();
                self.placing = Some((family, type_name));
            }
        }
        Ok(())
    }
}
