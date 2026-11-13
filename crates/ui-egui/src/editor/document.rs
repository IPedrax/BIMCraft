//! Every family edited as Revit edits a family: a document that takes the whole window. Its own ribbon (Create and Modify,
//! and for an adaptive family a contextual tab such as "Modify | Reference Points" for what is selected or the tool in hand),
//! Properties on the left with Revit's groups, a Project Browser of the family's views, the view in the middle with its tab and
//! view control bar (an adaptive family's modelling view, or the 3D preview of a family that isn't adaptive), and the Options
//! Bar over it. What the ribbon asks for (Load into Project, closing it) is done after the frame is drawn (`family_asked`);
//! Family Types and Family Category and Parameters are dialogs, as Revit's. The document has its own Undo and Redo, through
//! the draft's history (`Draft`): the project's is never touched until the family is loaded into it.
use super::*;
use crate::adaptive_view::{self, Draw};
use crate::dock::Palette;
use crate::icons::Icon;
use crate::properties::group_header;
use crate::ribbon;
use crate::{big_button, icon_button, tab_button};
use bimcraft_core::adaptive::{Definition, Evaluated, Plane, PointType, WorkPlane};
use bimcraft_core::units::Units;
use egui::{Align2, FontId, Frame, Margin, UiBuilder};

/// What the family document keeps of its own: its history.
#[derive(Default)]
pub(crate) struct State {
    /// The draft before each step, newest last, and the steps undone.
    undo: Vec<Draft>,
    redo: Vec<Draft>,
    /// The draft as the last step left it: what an edit still going on (a drag, a field being typed in, a dialog) began from.
    mark: Option<Draft>,
    /// The draft when the dialog open was, which its Cancel goes back to.
    pub(super) dialog_saved: Option<Draft>,
    /// What Load into Project was refused for: said until the draft changes.
    pub(super) refused: Option<String>,
    /// The draft as it was opened, last saved to a file or the Favorites, or last loaded into the project: any other has
    /// unsaved changes.
    clean: Option<Draft>,
    /// Revit's Preview Visibility: the preview shows the family as placed, the forms its type hides left out, rather than
    /// ghosted. The view's own: no part of the draft, so neither saved nor a step of its history.
    pub(super) preview_visibility: bool,
    /// Temporary Hide/Isolate: what the view hides for now (`temp_hide.rs`). The view's own, as Preview Visibility.
    pub(super) temp: super::temp_hide::Temp,
    /// Load into Project and Close went in: the document closes once the frame's work is done.
    pub(super) loaded_to_close: bool,
    /// Why Family Category and Parameters' Apply or OK was refused, while the draft is as it was then.
    options_refused: Option<(Draft, String)>,
    /// What Family Category and Parameters' category list is filtered by, and whether the list has been shown since it opened.
    category_filter: String,
    category_shown: bool,
}

/// The most steps the document's history keeps.
const STEPS: usize = 100;

/// What a step of the document's history holds: what is typed (even when it doesn't build yet) and the adaptive definition.
#[derive(Clone, PartialEq)]
pub(super) struct Draft {
    name: String,
    category: Category,
    operation: Option<Operation>,
    params: Vec<ParamText>,
    types: Vec<(String, BTreeMap<String, String>)>,
    forms: Vec<FormText>,
    symbols: Vec<SymbolText>,
    connectors: Vec<super::connectors::ConnectorText>,
    adaptive: Option<Definition>,
    picked: usize,
    /// Its reference planes: drawn and moved in its plans and elevations.
    planes: Vec<RefPlane>,
    /// Its dimensions: drawn, labelled and locked in its plans and elevations.
    dimensions: Vec<bimcraft_core::family::FamilyDimension>,
    /// Its options: Family Category and Parameters' table, Properties' Other group.
    options: super::options::OptionsText,
    /// Its nested families and their instances (T6a).
    nested: Vec<bimcraft_core::family::NestedFamily>,
    nested_instances: Vec<bimcraft_core::family::NestedInstance>,
}

impl Draft {
    fn of(ed: &FamilyEditor) -> Draft {
        Draft {
            name: ed.name.clone(),
            category: ed.category,
            operation: ed.operation,
            params: ed.params.clone(),
            types: ed.types.clone(),
            forms: ed.forms.clone(),
            symbols: ed.symbols.clone(),
            connectors: ed.connectors.clone(),
            adaptive: ed.adaptive.clone(),
            picked: ed.picked,
            planes: ed.planes.clone(),
            dimensions: ed.dimensions.clone(),
            options: ed.options.clone(),
            nested: ed.nested.clone(),
            nested_instances: ed.nested_instances.clone(),
        }
    }

    fn restore(self, ed: &mut FamilyEditor) {
        ed.name = self.name;
        ed.category = self.category;
        ed.operation = self.operation;
        ed.params = self.params;
        ed.types = self.types;
        ed.forms = self.forms;
        ed.symbols = self.symbols;
        ed.adaptive = self.adaptive;
        ed.picked = self.picked;
        ed.planes = self.planes;
        ed.dimensions = self.dimensions;
        ed.options = self.options;
        ed.nested = self.nested;
        ed.nested_instances = self.nested_instances;
        // What is selected and no longer there goes from the selection.
        let nodes = ed.adaptive.as_ref().map(|d| d.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>()).unwrap_or_default();
        ed.modeller.selection.retain(|id| nodes.contains(id));
        ed.problem = None;
    }
}

impl FamilyEditor {
    /// Whether something of the family's own is selected that Delete takes away: a form, a plane of its own (a datum stays), a
    /// dimension, a nested instance or a reference line.
    pub(crate) fn deletable(&self) -> bool {
        self.selected_form().is_some()
            || self.selected_plane().is_some_and(|g| self.planes.iter().any(|p| p.guid == g && !p.datum()))
            || self.selected_dimension().is_some()
            || self.selected_nested().is_some()
            || self.selected_line().is_some()
            || self.selected_symbol().is_some()
            || self.selected_connector().is_some()
    }

    /// Delete (DE, the Delete key, the ribbon's Delete): what is selected taken away, with what depends on it, or why not.
    pub(crate) fn delete_selected(&mut self) -> Result<(), String> {
        if let Some(i) = self.selected_nested() {
            self.nested_instances.remove(i);
            self.select_nested(None);
        } else if let Some(g) = self.selected_dimension() {
            self.delete_dimension(g);
        } else if let Some(g) = self.selected_line() {
            self.delete_line(g)?;
        } else if let Some(g) = self.selected_plane().filter(|g| self.planes.iter().any(|p| p.guid == *g && !p.datum())) {
            self.delete_plane(g)?;
        } else if let Some(i) = self.selected_form() {
            self.remove_form(i);
        } else if let Some(i) = self.selected_symbol() {
            self.remove_symbol(i);
        } else if let Some(i) = self.selected_connector() {
            self.remove_connector(i);
        } else if let Some(d) = self.adaptive.as_mut() {
            adaptive_view::delete(d, &mut self.modeller);
        }
        Ok(())
    }

    /// The draft as it is now, for `saved_as` once it has been saved.
    pub(super) fn draft(&self) -> Draft {
        Draft::of(self)
    }

    /// The draft `d` was saved or loaded into the project: changes are counted from it.
    pub(super) fn saved_as(&mut self, d: Draft) {
        self.document.clean = Some(d);
    }

    /// Whether Preview Visibility is on: the preview shows the family as placed.
    pub(crate) fn preview_visibility(&self) -> bool {
        self.document.preview_visibility
    }

    /// Whether the draft has changes not saved or loaded into the project since it was opened.
    pub(crate) fn unsaved(&self) -> bool {
        self.document.clean.as_ref().is_some_and(|clean| *clean != Draft::of(self))
    }

    /// Whether a dialog of the document's is open: Family Types, Family Category and Parameters, or an association.
    fn modal(&self) -> bool {
        self.dialog.is_some()
            || self.associating.is_some()
            || self.views.unsatisfied.is_some()
            || self.views.creating.is_some()
            || self.views.dialog.is_some()
            || self.views.go_to.is_some()
    }

    /// The document's history, at the end of a frame: Undo or Redo when `ask` says so, else a step for what changed since the
    /// last one, once nothing is going on any more (no button held, no field being typed in, no dialog open), so a drag, a
    /// word typed or a dialog's whole visit is one step. Cancel in a dialog puts back what it began from, leaving no step.
    pub(super) fn history(&mut self, ctx: &Context, ask: &Asked) {
        let now = Draft::of(self);
        let Some(mark) = self.document.mark.take() else {
            // The document's first frame: the draft as it was opened.
            self.document.clean.get_or_insert_with(|| now.clone());
            self.document.mark = Some(now);
            return;
        };
        if ask.undo || ask.redo {
            self.document.refused = None;
            // An edit not made a step yet is one now, and the first undone.
            let mut current = now;
            if mark != current {
                self.document.undo.push(mark);
                self.document.redo.clear();
            }
            let (from, to) = if ask.undo {
                (&mut self.document.undo, &mut self.document.redo)
            } else {
                (&mut self.document.redo, &mut self.document.undo)
            };
            if let Some(step) = from.pop() {
                to.push(current);
                current = step.clone();
                step.restore(self);
            }
            self.document.mark = Some(current);
            return;
        }
        let busy = self.modal() || ctx.input(|i| i.pointer.any_down()) || ctx.text_edit_focused();
        if busy {
            self.document.mark = Some(mark);
            return;
        }
        if mark != now {
            // What was refused is no longer what is there.
            self.document.refused = None;
            if self.document.undo.len() >= STEPS {
                self.document.undo.remove(0);
            }
            self.document.undo.push(mark);
            self.document.redo.clear();
        }
        self.document.mark = Some(now);
    }
}

/// The contextual tab for what the family document has in hand: the tool, else what is selected, as Revit names them.
fn context(ed: &FamilyEditor) -> Option<&'static str> {
    let m = &ed.modeller;
    match m.tool {
        Draw::Point => return Some(msg!("Modify | Place Reference Point")),
        Draw::Line(_) => return Some(msg!("Modify | Place Lines")),
        Draw::Modify | Draw::Move { .. } | Draw::Rotate { .. } => {}
    }
    // A form of a family that isn't adaptive, selected.
    if ed.adaptive.is_none() {
        if let Some(title) = ed.sketch_title() {
            return Some(title);
        }
        if ed.editing_2d() && ed.views.drawing.is_some() {
            return Some(msg!("Modify | Place Reference Plane"));
        }
        if ed.placing.is_some() {
            return Some(msg!("Modify | Place Component"));
        }
        if ed.selected_nested().is_some() {
            return Some(msg!("Modify | Nested Instance"));
        }
        if ed.editing_2d() && ed.views.dimensioning.is_some() {
            return Some(msg!("Modify | Place Dimensions"));
        }
        if ed.editing_2d() && ed.views.aligning.is_some() {
            return Some(msg!("Modify | Align"));
        }
        if ed.selected_dimension().is_some() {
            return Some(msg!("Modify | Dimensions"));
        }
        if ed.editing_2d() && ed.views.pivoting.is_some() {
            return Some(msg!("Modify | Pivot"));
        }
        if ed.selected_plane().is_some() {
            return Some(msg!("Modify | Reference Planes"));
        }
        if ed.selected_line().is_some() {
            return Some(msg!("Modify | Reference Lines"));
        }
        if ed.selected_symbol().is_some() {
            return Some(msg!("Modify | Symbols"));
        }
        if ed.selected_connector().is_some() {
            return Some(msg!("Modify | Connectors"));
        }
        return ed.selected_form().map(|i| form_context(ed, i));
    }
    let d = ed.adaptive.as_ref()?;
    let r = d.evaluate(&ed.values(), &d.preview);
    let mut kinds = m.selection.iter().map(|id| match r.values.get(id.as_str()) {
        Some(Evaluated::Point { .. }) if d.point_type(id).is_some() => 0,
        Some(Evaluated::Curve { .. }) => 1,
        _ => 2,
    });
    let first = kinds.next()?;
    if kinds.any(|k| k != first) {
        return Some(msg!("Modify | Multi-Select"));
    }
    Some(match first {
        0 => msg!("Modify | Reference Points"),
        1 => msg!("Modify | Lines"),
        _ => msg!("Modify | Form"),
    })
}

/// A selected form's contextual tab, named for its kind as Revit's (Modify | Extrusion, Modify | Void Blend); a mesh from a
/// file, Modify | Form.
fn form_context(ed: &FamilyEditor, i: usize) -> &'static str {
    use bimcraft_core::family::forms::FormKind;
    let Some(f) = ed.forms.get(i).filter(|f| !f.mesh()).and_then(|f| f.form().ok()) else { return msg!("Modify | Form") };
    match (f.kind(), f.void) {
        (FormKind::Extrusion, false) => msg!("Modify | Extrusion"),
        (FormKind::Blend, false) => msg!("Modify | Blend"),
        (FormKind::Revolve, false) => msg!("Modify | Revolve"),
        (FormKind::Sweep, false) => msg!("Modify | Sweep"),
        (FormKind::SweptBlend, false) => msg!("Modify | Swept Blend"),
        (FormKind::Extrusion, true) => msg!("Modify | Void Extrusion"),
        (FormKind::Blend, true) => msg!("Modify | Void Blend"),
        (FormKind::Revolve, true) => msg!("Modify | Void Revolve"),
        (FormKind::Sweep, true) => msg!("Modify | Void Sweep"),
        (FormKind::SweptBlend, true) => msg!("Modify | Void Swept Blend"),
    }
}

/// What a family document's view tab and title call its view.
pub(crate) const VIEW: &str = "{3D}";

impl BimApp {
    /// Whether a family document has the window: a family open in the Family Editor.
    pub(crate) fn family_doc(&self) -> bool {
        self.active_family().is_some()
    }

    /// The title the window and its title bar give a family document: the family and its view.
    pub(crate) fn family_title(&self) -> Option<String> {
        let ed = self.active_family()?;
        Some(format!("{} · {}", ed.title(), super::views::view_title(ed.views.shown)))
    }

    /// The family document's ribbon: its tabs, then the active tab's panels, folding to fit as the project's do.
    pub(crate) fn family_ribbon(&mut self, ui: &mut Ui) {
        let Some(mut ed) = self.take_family() else { return };
        // The contextual tab comes up when it appears or changes, and goes back to the tab it came from when it goes.
        let now = context(&ed);
        let seen = UiId::new("family-context");
        let before = ui.ctx().data(|d| d.get_temp::<Option<&'static str>>(seen)).flatten();
        if now.is_some() && now != before && ed.doc_tab != DocTab::Context {
            ed.doc_back = ed.doc_tab;
            ed.doc_tab = DocTab::Context;
        }
        if now.is_none() && ed.doc_tab == DocTab::Context {
            ed.doc_tab = ed.doc_back;
        }
        ui.ctx().data_mut(|d| d.insert_temp(seen, now));
        Frame::new().inner_margin(Margin { left: 6, right: 6, top: 5, bottom: 5 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (tab, label) in [
                    (DocTab::Create, msg!("Create")),
                    (DocTab::Insert, msg!("Insert")),
                    (DocTab::Annotate, "Annotate"),
                    (DocTab::View, msg!("View")),
                    (DocTab::Manage, msg!("Manage")),
                    (DocTab::Modify, "Modify"),
                ] {
                    if tab_button(ui, label, ed.doc_tab == tab, false).clicked() {
                        ed.doc_tab = tab;
                    }
                }
                if let Some(name) = now
                    && tab_button(ui, name, ed.doc_tab == DocTab::Context, true).clicked()
                {
                    ed.doc_tab = DocTab::Context;
                }
            });
        });
        let base = ui.min_rect();
        ui.painter().hline(base.x_range(), base.bottom(), Stroke::new(1.0, pal().border));
        self.ribbon_bottom = ui.min_rect().left_bottom();
        Frame::new().inner_margin(Margin { left: 6, right: 6, top: 4, bottom: 4 }).show(ui, |ui| {
            let key = UiId::new(("family-ribbon-widths", ed.doc_tab as u8));
            let room = ui.available_width();
            ScrollArea::horizontal().id_salt(("family-ribbon", ed.doc_tab as u8)).auto_shrink([false, true]).show(ui, |ui| {
                ribbon::tab(ui, key, room, |ui| {
                    ui.horizontal(|ui| self.family_panels(ui, &mut ed));
                });
            });
        });
        self.put_family(ed);
    }

    /// The panels of the family document's active tab, as Revit's family editor lays them out: Create (drawing), Insert, View,
    /// Manage, Modify (editing what is selected), and the contextual tab, Modify's panels with the selection's own. A tool
    /// BIMCraft does not have yet shows where Revit has it, greyed.
    fn family_panels(&mut self, ui: &mut Ui, ed: &mut FamilyEditor) {
        let values = ed.values();
        let (points, lines) = match ed.adaptive.as_ref() {
            Some(d) => adaptive_view::selected(d, &ed.modeller, &values),
            None => (Vec::new(), Vec::new()),
        };
        let tab = ed.doc_tab;
        let drawing = !matches!(ed.modeller.tool, Draw::Modify);
        let adaptive = ed.adaptive.is_some();
        // Sketch mode (F5): its contextual tab alone, as Revit's "Modify | Create Extrusion".
        if ed.sketching() {
            self.select_and_properties(ui, ed);
            self.form_sketch_panels(ui, ed);
            return;
        }
        if matches!(tab, DocTab::Create | DocTab::Modify | DocTab::Context) {
            self.select_and_properties(ui, ed);
        }
        match tab {
            DocTab::Create if adaptive => {
                draw_panel(ui, ed);
                work_plane_panel(ui, ed);
            }
            // A family that isn't adaptive is made of forms drawn in sketches, which BIMCraft doesn't have yet: its forms are
            // added and edited in Properties.
            DocTab::Create => ribbon::panel(ui, msg!("Forms"), |ui| {
                // Extrusion and Revolve sketch on the work plane (F5); Blend, Sweep and Swept Blend too (F6).
                use bimcraft_core::family::forms::FormKind;
                let tip = msg!("Extrusion: sketch a profile on the work plane, extruded along its normal");
                if big_button(ui, Icon::CreateForm, msg!("Extrusion"), false, tip).clicked() {
                    ed.create_form(FormKind::Extrusion, false);
                }
                let tip = msg!("Blend: sketch a base and a top on the work plane, joined between their two ends");
                if big_button(ui, Icon::CreateForm, msg!("Blend"), false, tip).clicked() {
                    ed.create_form(FormKind::Blend, false);
                }
                let tip = msg!("Revolve: sketch a profile and an axis line on the work plane, turned about the axis");
                if big_button(ui, Icon::CreateForm, msg!("Revolve"), false, tip).clicked() {
                    ed.create_form(FormKind::Revolve, false);
                }
                let tip = msg!("Sweep: sketch or pick a path on the work plane, then a profile square to it, swept along it");
                if big_button(ui, Icon::CreateForm, msg!("Sweep"), false, tip).clicked() {
                    ed.create_form(FormKind::Sweep, false);
                }
                let tip = msg!("Swept Blend: a path of one line or arc, and a profile at each end, blended along it");
                if big_button(ui, Icon::CreateForm, msg!("Swept Blend"), false, tip).clicked() {
                    ed.create_form(FormKind::SweptBlend, false);
                }
                let tip = msg!(
                    "Void Forms: a void extrusion, blend, revolve, sweep or swept blend, sketched as a solid one is, which cuts the solids before it"
                );
                let r = big_button(ui, Icon::CreateForm, msg!("Void Forms"), false, tip);
                ribbon::choices(ui, &r, false, |ui| {
                    for (label, kind) in [
                        (msg!("Void Extrusion"), FormKind::Extrusion),
                        (msg!("Void Blend"), FormKind::Blend),
                        (msg!("Void Revolve"), FormKind::Revolve),
                        (msg!("Void Sweep"), FormKind::Sweep),
                        (msg!("Void Swept Blend"), FormKind::SweptBlend),
                    ] {
                        if ui.button(t(label)).clicked() {
                            ed.create_form(kind, true);
                            ui.close();
                        }
                    }
                });
            }),
            _ => {}
        }
        match tab {
            DocTab::Create => {
                // Model and Datum make room first in a narrow window: their last tools are greyed, not in BIMCraft yet.
                ribbon::yielding(ui, msg!("Model"), |ui| {
                    // A family loaded into this one placed (T6a): in a plan or an elevation, on the view's work plane.
                    let tip =
                        msg!("Component (CM): an instance of a family loaded into this one, placed by a click in a plan or an elevation");
                    let can = !adaptive && !ed.nested.is_empty();
                    if ui.add_enabled_ui(can, |ui| big_button(ui, Icon::Component, "Component", ed.placing.is_some(), tip)).inner.clicked()
                    {
                        ed.component_tool();
                    }
                    later(ui, Icon::Text, msg!("Model Text"));
                });
                ribbon::yielding(ui, msg!("Datum"), |ui| {
                    // Drawn in a plan or an elevation of a family that isn't adaptive, as Revit's (F4).
                    let tip = msg!("Reference Plane (RP): a plane drawn by two clicks in a plan or an elevation");
                    let on = ed.views.drawing.is_some();
                    if ui.add_enabled_ui(ed.editing_2d(), |ui| big_button(ui, Icon::Line, msg!("Reference Plane"), on, tip)).inner.clicked()
                    {
                        ed.reference_plane_tool();
                    }
                    later(ui, Icon::Level, "Level");
                });
                // Drawn in a plan or an elevation of a family that isn't adaptive (F7).
                if !adaptive {
                    ribbon::panel(ui, msg!("Dimension"), |ui| {
                        let tip = msg!(
                            "Aligned Dimension (DI): pick parallel reference planes, extrusion ends or lines of forms, then click where it goes"
                        );
                        let on = ed.views.dimensioning.is_some();
                        let aligned = ed.views.dim_kind == bimcraft_core::family::DimKind::Aligned;
                        if ui
                            .add_enabled_ui(ed.editing_2d() && !ed.sketching(), |ui| {
                                big_button(ui, Icon::Dimension, msg!("Aligned Dimension"), on && aligned, tip)
                            })
                            .inner
                            .clicked()
                        {
                            ed.dimension_tool();
                        }
                        // The angles between lines that cross (F7c); a circle or an arc of a sketch, its radius or its diameter
                        // (F7b).
                        for (kind, icon, label, tip) in [
                            (
                                bimcraft_core::family::DimKind::Angular,
                                Icon::Angular,
                                msg!("Angular"),
                                msg!(
                                    "Angular Dimension: pick reference lines, reference planes or lines of forms that cross, then click where its arc goes"
                                ),
                            ),
                            (
                                bimcraft_core::family::DimKind::Radial,
                                Icon::Radial,
                                msg!("Radial"),
                                msg!("Radial Dimension: pick a circle or an arc of a form's sketch, then click where it goes"),
                            ),
                            (
                                bimcraft_core::family::DimKind::Diameter,
                                Icon::Diameter,
                                msg!("Diameter"),
                                msg!("Diameter Dimension: pick a circle or an arc of a form's sketch, then click where it goes"),
                            ),
                        ] {
                            let lit = on && ed.views.dim_kind == kind;
                            let usable = ed.editing_2d() && !ed.sketching();
                            if ui.add_enabled_ui(usable, |ui| crate::small_button(ui, icon, label, lit, tip)).inner.clicked() {
                                ed.radial_tool(kind);
                            }
                        }
                    });
                }
                // In {3D} too, where Show draws it over the preview.
                if !adaptive {
                    views_work_plane_panel(ui, ed);
                }
                if adaptive {
                    self.form_panel(ui, ed, &lines);
                    self.test_placement_panel(ui, ed);
                }
            }
            DocTab::Annotate => {
                // What the family's plans draw besides its forms (a door's swing): Revit's Symbolic Line and Text, and a dot. Each
                // is made selected, its numbers typed in Properties.
                if !adaptive {
                    use super::Part;
                    ribbon::panel(ui, msg!("Detail"), |ui| {
                        let tip = msg!("Symbolic Line: a line or an arc the family's plans draw, not part of its forms (a door's swing)");
                        let r = big_button(ui, Icon::Line, msg!("Symbolic Line"), false, tip);
                        ribbon::choices(ui, &r, false, |ui| {
                            for (part, label) in [(Part::Line, msg!("Line")), (Part::Arc, msg!("Arc"))] {
                                if ui.button(t(label)).clicked() {
                                    ed.add_symbol(part);
                                    ui.close();
                                }
                            }
                        });
                        let tip = msg!("Text: a label the family's plans draw");
                        if big_button(ui, Icon::Text, "Text", false, tip).clicked() {
                            ed.add_symbol(Part::Text);
                        }
                        let tip = msg!("Dot: a filled circle the family's plans draw");
                        if big_button(ui, Icon::PointElement, msg!("Dot"), false, tip).clicked() {
                            ed.add_symbol(Part::Dot);
                        }
                    });
                }
            }
            DocTab::Insert => {
                ribbon::panel(ui, msg!("Family"), |ui| {
                    let tip = msg!("Load Family: a family file loaded into this family, to place with Component");
                    if big_button(ui, Icon::LoadFamily, msg!("Load Family"), false, tip).clicked() {
                        ed.ask.load = true;
                    }
                    if big_button(ui, Icon::Save, msg!("Save Family"), false, msg!("Save Family: the family to a file")).clicked() {
                        ed.ask.save = true;
                    }
                    let tip = msg!("Add to Favorites: the family to your Favorites");
                    if big_button(ui, Icon::Favorite, msg!("Add to Favorites"), false, tip).clicked() {
                        ed.ask.favorite = true;
                    }
                });
                ribbon::panel(ui, msg!("Import"), |ui| {
                    later(ui, Icon::ImportCad, msg!("Import CAD"));
                    later(ui, Icon::Materials, msg!("Image"));
                });
            }
            DocTab::View => {
                ribbon::panel(ui, msg!("Graphics"), |ui| {
                    later(ui, Icon::Visibility, msg!("Visibility/Graphics"));
                    later(ui, Icon::ThinLines, msg!("Thin Lines"));
                });
                ribbon::panel(ui, msg!("Windows"), |ui| {
                    later(ui, Icon::CloseInactive, msg!("Close Inactive Views"));
                });
            }
            DocTab::Manage => {
                ribbon::panel(ui, msg!("Settings"), |ui| {
                    later(ui, Icon::ObjectStyles, msg!("Object Styles"));
                    later(ui, Icon::Materials, "Materials");
                    later(ui, Icon::Units, msg!("Project Units"));
                });
                // An adaptive family is placed by its points, as Revit's Generic Model Adaptive: made so here, its forms then
                // drawn through its points in the modelling view.
                if !adaptive {
                    ribbon::panel(ui, msg!("Adaptive Component"), |ui| {
                        let tip = msg!(
                            "Make Family Adaptive: its points are placed where each instance is clicked, as Revit's Generic Model Adaptive"
                        );
                        if big_button(ui, Icon::MakeAdaptive, msg!("Make Family Adaptive"), false, tip).clicked() {
                            ed.adaptive = Some(bimcraft_core::adaptive::Definition::default());
                        }
                        let tip = msg!("Four-Point Panel: the family made adaptive, as a panel on four placement points");
                        if big_button(ui, Icon::MakeAdaptive, msg!("Four-Point Panel"), false, tip).clicked() {
                            ed.adaptive = Some(bimcraft_core::adaptive::four_point_panel());
                        }
                    });
                }
            }
            DocTab::Modify | DocTab::Context => {
                if tab == DocTab::Context && drawing {
                    draw_panel(ui, ed);
                    work_plane_panel(ui, ed);
                }
                ribbon::panel(ui, msg!("Clipboard"), |ui| {
                    later(ui, Icon::Copy, msg!("Paste"));
                });
                ribbon::panel(ui, msg!("Modify"), |ui| {
                    let some = !ed.modeller.selection.is_empty();
                    // Align, in a plan or an elevation of a family that isn't adaptive (F7): what is picked second onto what is
                    // picked first, and a padlock that locks them.
                    if ed.editing_2d() {
                        let tip = msg!("Align (AL): pick a reference, then what moves onto it; its padlock locks them");
                        if crate::small_button(ui, Icon::Align, "Align", ed.views.aligning.is_some(), tip).clicked() {
                            ed.align_tool();
                        }
                    } else {
                        later_small(ui, Icon::Align, "Align");
                    }
                    for (icon, label) in [(Icon::Offset, "Offset"), (Icon::MirrorPick, msg!("Mirror - Pick Axis"))] {
                        later_small(ui, icon, label);
                    }
                    later_small(ui, Icon::MirrorDraw, msg!("Mirror - Draw Axis"));
                    // Move, Copy and Rotate, as Revit's: two clicks in the view.
                    let tool = ed.modeller.tool.clone();
                    let tip = msg!("Move (MV): what is selected, from a point clicked to another");
                    if ui
                        .add_enabled_ui(some, |ui| {
                            crate::small_button(ui, Icon::Move, "Move", matches!(tool, Draw::Move { copy: false, .. }), tip)
                        })
                        .inner
                        .clicked()
                    {
                        ed.modeller.tool = Draw::Move { from: None, copy: false };
                    }
                    let tip = msg!("Copy (CO): copies of what is selected, from a point clicked to another");
                    if ui
                        .add_enabled_ui(some, |ui| {
                            crate::small_button(ui, Icon::Copy, "Copy", matches!(tool, Draw::Move { copy: true, .. }), tip)
                        })
                        .inner
                        .clicked()
                    {
                        ed.modeller.tool = Draw::Move { from: None, copy: true };
                    }
                    let tip = msg!("Rotate (RO): what is selected, about its centre, by the angle between two clicks");
                    if ui
                        .add_enabled_ui(some, |ui| {
                            crate::small_button(ui, Icon::Rotate, "Rotate", matches!(tool, Draw::Rotate { .. }), tip)
                        })
                        .inner
                        .clicked()
                        && let Some(d) = ed.adaptive.as_ref()
                        && let Some(centre) = adaptive_view::selection_centre(d, &ed.modeller, &values)
                    {
                        ed.modeller.tool = Draw::Rotate { centre, start: None };
                    }
                    later_small(ui, Icon::Trim, msg!("Trim/Extend"));
                    later_small(ui, Icon::Split, msg!("Split Element"));
                    let tip = msg!("Delete (DE): what is selected, with what depends on it");
                    if ui.add_enabled_ui(some || ed.deletable(), |ui| big_button(ui, Icon::Delete, "Delete", false, tip)).inner.clicked()
                        && let Err(e) = ed.delete_selected()
                    {
                        self.status = Status { text: e, error: true };
                    }
                });
                ribbon::panel(ui, msg!("Measure"), |ui| {
                    later(ui, Icon::Measure, msg!("Measure"));
                });
                if tab == DocTab::Context {
                    if let Some(i) = ed.selected_nested() {
                        self.nested_mode_panel(ui, ed, i);
                    }
                    // F7d: a reference line or a plane of the family's own turns about a point it is pinned to.
                    if ed.editing_2d() && ed.pivot_candidate().is_some() {
                        ribbon::panel(ui, msg!("Rotation"), |ui| {
                            let tip = msg!(
                                "Pivot: click the point it turns about (where two planes cross, a line's end, a point of a sketch, or anywhere); its Swing Angle turns it"
                            );
                            if big_button(ui, Icon::Rotate, msg!("Pivot"), ed.views.pivoting.is_some(), tip).clicked() {
                                ed.pivot_tool();
                            }
                            let pivoted = ed.pivot_candidate().filter(|g| ed.draft_pivot(*g).is_some());
                            let tip = msg!("Remove Pivot: it stops turning and stays where it is drawn");
                            if ui
                                .add_enabled_ui(pivoted.is_some(), |ui| big_button(ui, Icon::Delete, msg!("Remove Pivot"), false, tip))
                                .inner
                                .clicked()
                                && let Some(g) = pivoted
                            {
                                ed.remove_pivot(g);
                            }
                        });
                    }
                    if let Some(i) = ed.selected_form() {
                        self.form_work_plane_panel(ui, ed, i);
                        self.form_mode_panel(ui, ed, i);
                        visibility_panel(ui, ed, super::visibility::Target::Form(i));
                    }
                    if let Some(i) = ed.selected_nested() {
                        visibility_panel(ui, ed, super::visibility::Target::Nested(i));
                    }
                    if !lines.is_empty() {
                        self.form_panel(ui, ed, &lines);
                    }
                    if !points.is_empty() {
                        ribbon::panel(ui, msg!("Adaptive Component"), |ui| {
                            let tip = msg!(
                                "Make Adaptive: the points selected become placement points, numbered in the order selected; adaptive already, they go back to reference points"
                            );
                            if big_button(ui, Icon::MakeAdaptive, msg!("Make Adaptive"), false, tip).clicked()
                                && let Some(d) = ed.adaptive.as_mut()
                            {
                                adaptive_view::make_adaptive(d, &points);
                            }
                        });
                    }
                }
            }
        }
        // Where the family joins a system (#269), as Revit's Connectors panel, before Family Editor, in every category's family:
        // each placed at the family's origin and selected, its place and what it carries typed in Properties.
        if tab == DocTab::Create && ed.adaptive.is_none() {
            connectors_panel(ui, ed);
        }
        let into_host = ed.into_host;
        // Kept whole while the tab makes room, as Revit's: the family's way back into the project stays in sight.
        ribbon::kept(ui, msg!("Family Editor"), |ui| {
            // Opened from a family it is nested in, it loads back into that one (T6a), as Revit's does into the open family.
            let (load, tip) = if into_host {
                (msg!("Load into Family"), msg!("Load into Family: the family it is nested in takes it as it is here, and it stays open"))
            } else {
                (
                    msg!("Load into Project"),
                    msg!("Load into Project: the project takes the family as it is here, and the family stays open"),
                )
            };
            if big_button(ui, Icon::LoadIntoProject, load, false, tip).clicked() {
                ed.ask.apply = true;
            }
            let (load, tip) = if into_host {
                (msg!("Load into Family and Close"), msg!("Load into Family and Close: the family it is nested in takes it, and it closes"))
            } else {
                (
                    msg!("Load into Project and Close"),
                    msg!("Load into Project and Close: the project takes the family, and the family closes"),
                )
            };
            if big_button(ui, Icon::LoadIntoProjectClose, load, false, tip).clicked() {
                ed.ask.ok = true;
            }
        });
    }

    /// Revit's Select and Properties panels, which Create, Modify and the contextual tab begin with.
    fn select_and_properties(&mut self, ui: &mut Ui, ed: &mut FamilyEditor) {
        ribbon::panel(ui, msg!("Select"), |ui| {
            let tip = msg!("Modify (MD): select elements");
            if big_button(ui, Icon::Modify, "Modify", ed.modeller.tool == Draw::Modify && ed.views.drawing.is_none(), tip).clicked() {
                ed.modeller.tool = Draw::Modify;
                ed.modeller.picking_plane = false;
                ed.form = None;
                ed.symbol = None;
                ed.connector = None;
                ed.views.drawing = None;
                ed.views.picking = None;
                ed.views.plane = None;
                ed.views.dimensioning = None;
                ed.views.aligning = None;
                ed.views.dim = None;
            }
        });
        let shown = self.docking.shown(Palette::Properties);
        ribbon::panel(ui, msg!("Properties"), |ui| {
            if big_button(ui, Icon::Properties, "Properties", shown, msg!("Properties (PP): show or hide the palette")).clicked() {
                self.show_palette(Palette::Properties, !shown);
            }
            let tip = msg!("Family Category and Parameters: the family's name and category");
            if big_button(ui, Icon::Parameters, msg!("Family Category and Parameters"), ed.dialog == Some(DocDialog::Category), tip)
                .clicked()
            {
                ed.dialog = Some(DocDialog::Category);
            }
            let tip = msg!("Family Types: the family's parameters, their formulas, and its types");
            if big_button(ui, Icon::FamilyTypes, msg!("Family Types"), ed.dialog == Some(DocDialog::Types), tip).clicked() {
                ed.dialog = Some(DocDialog::Types);
            }
        });
    }

    /// BIMCraft's own Test Placement panel: replaying the instance the family was opened from, capturing the project's
    /// selection, giving the test placement to an instance or placing a new one.
    fn test_placement_panel(&mut self, ui: &mut Ui, ed: &mut FamilyEditor) {
        let sources = !self.capture_sources().is_empty();
        let source = ed.source;
        let mut replay = false;
        ribbon::panel(ui, msg!("Test Placement"), |ui| {
            if source.is_some() {
                let tip = msg!(
                    "Replay Instance: the family takes the placement, type and parameters of the instance it was opened from, to see it as it is or to model from it"
                );
                replay = big_button(ui, Icon::Component, msg!("Replay Instance"), ed.replaying().is_some(), tip).clicked();
            }
            let tip = msg!("Capture Placement: the adaptive instances selected in the project give their points as the test placement");
            if ui.add_enabled_ui(sources, |ui| big_button(ui, Icon::PickFaces, msg!("Capture Placement"), false, tip)).inner.clicked() {
                ed.ask.capture = true;
            }
            if ed.replaying().is_some() {
                let tip = msg!("Apply to Instance: the instance the family was opened on takes the test placement");
                if big_button(ui, Icon::Check, msg!("Apply to Instance"), false, tip).clicked() {
                    ed.ask.apply_placement = true;
                }
            }
            let tip = msg!("Place Instance: a new instance of the family on the test placement, in the project");
            if big_button(ui, Icon::Component, msg!("Place Instance"), false, tip).clicked() {
                ed.ask.place = true;
            }
        });
        if replay {
            let model = Arc::clone(&self.session.model);
            ed.replay_from(&model);
        }
    }

    /// Revit's Form panel: Create Form, a solid or a void of the lines selected.
    fn form_panel(&mut self, ui: &mut Ui, ed: &mut FamilyEditor, lines: &[String]) {
        let mut made = None;
        ribbon::panel(ui, msg!("Form"), |ui| {
            let tip = msg!("Create Form: a solid or a void of the lines selected: one closed profile extruded, two or more lofted");
            let r = ui.add_enabled_ui(!lines.is_empty(), |ui| big_button(ui, Icon::CreateForm, msg!("Create Form"), false, tip)).inner;
            ribbon::choices(ui, &r, false, |ui| {
                if ui.button(t("Solid Form")).clicked() {
                    made = Some(false);
                    ui.close();
                }
                if ui.button(t("Void Form")).clicked() {
                    made = Some(true);
                    ui.close();
                }
            });
        });
        if let Some(void) = made
            && let Some(d) = ed.adaptive.as_mut()
            && let Err(e) = adaptive_view::create_form(d, &mut ed.modeller, lines, void)
        {
            self.status = Status { text: e, error: true };
        }
    }

    /// The family document's Properties palette: what is selected in its view, with Revit's groups, or the family itself; for a
    /// family that isn't adaptive, the family and its forms and symbols.
    pub(crate) fn family_properties(&mut self, ui: &mut Ui) {
        let Some(mut ed) = self.take_family() else { return };
        // A form being sketched: what it will be.
        if ed.sketching() {
            self.form_sketch_properties(ui, &mut ed);
            self.put_family(ed);
            return;
        }
        let values = ed.values();
        let category = category_label(ed.category);
        let selected = ed.modeller.selection.clone();
        let params = ed.parameter_kinds();
        // A reference line selected (F7d): its pivot.
        if let Some(g) = ed.selected_line() {
            let i = ed.lines.iter().position(|l| l.guid == g).unwrap_or_default();
            selector(ui, Icon::Line, &tf("Reference Line {}", &[&(i + 1).to_string()]));
            ui.add_space(6.0);
            if let Some(e) = ed.pivot_properties(ui, g, self.session.model.units) {
                self.status = Status { text: e, error: true };
            }
            self.put_family(ed);
            return;
        }
        // A reference plane selected in a plan or an elevation: its name, and its pivot (F7d).
        if let Some(g) = ed.selected_plane() {
            selector(ui, Icon::Line, t("Reference Plane"));
            ui.add_space(6.0);
            if let Some(p) = ed.planes.iter_mut().find(|p| p.guid == g) {
                let datum = p.datum();
                group(ui, msg!("Identity Data"), |ui| {
                    let l = ui.label(t("Name"));
                    if datum {
                        ui.label(t(&p.name)).labelled_by(l.id);
                    } else {
                        ui.add(TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY)).labelled_by(l.id);
                    }
                    ui.end_row();
                });
                if !datum && let Some(e) = ed.pivot_properties(ui, g, self.session.model.units) {
                    self.status = Status { text: e, error: true };
                }
            }
            // Defines Origin (O1): where the planes ticked cross is the insertion point the family is placed by. Ticked on one
            // plane, the other of its direction lets go; a datum ticked stays so until another plane takes it. An adaptive family
            // has no insertion point.
            if ed.adaptive.is_none()
                && let Some(p) = ed.planes.iter().find(|p| p.guid == g)
            {
                let (mut on, square, datum) = (p.defines_origin, p.direction().is_some(), p.datum());
                let mut changed = false;
                group(ui, msg!("Other"), |ui| {
                    let l = ui.label(t("Defines Origin"));
                    let enabled = square && !(datum && on);
                    let r = ui.add_enabled(enabled, egui::Checkbox::without_text(&mut on)).labelled_by(l.id);
                    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, on, t("Defines Origin")));
                    changed = r.changed();
                    ui.end_row();
                });
                if changed {
                    ed.define_origin(g, on);
                }
            }
            self.put_family(ed);
            return;
        }
        // A nested instance selected: its type, where it stands and its family's parameters (T6a).
        if let Some(i) = ed.selected_nested() {
            let what = ed.nested_instances.get(i).map(|n| format!("{} : {}", n.family, n.type_name)).unwrap_or_default();
            selector(ui, Icon::Component, &what);
            ui.add_space(6.0);
            self.nested_properties(ui, &mut ed, i);
            self.put_family(ed);
            return;
        }
        // A form selected: its own Properties, as Revit's show only what is selected.
        if let Some(i) = ed.selected_form() {
            selector(ui, Icon::CreateForm, &ed.forms[i].describe());
            ui.add_space(6.0);
            self.form_properties(ui, &mut ed, i);
            self.put_family(ed);
            return;
        }
        // A symbol selected: its own rows.
        if let Some(i) = ed.selected_symbol() {
            selector(ui, Icon::Line, &ed.symbol_name(i));
            ui.add_space(6.0);
            super::symbol_properties(&mut ed, ui, i);
            self.put_family(ed);
            return;
        }
        // A connector selected (#269): its own rows, as Revit's Connector Element shows them.
        if let Some(i) = ed.selected_connector() {
            let icon = if ed.connectors[i].kind() == bimcraft_core::family::ConnectorKind::Pipe { Icon::Pipe } else { Icon::Device };
            selector(ui, icon, t(super::connectors::kind_name(ed.connectors[i].kind())));
            ui.add_space(6.0);
            super::connectors::connector_properties(&mut ed, ui, i);
            self.put_family(ed);
            return;
        }
        // Nothing selected: the family's own, as Revit's Properties show them (Identity Data, then Family Category and
        // Parameters' Other). Its forms, symbols and connectors are in the Project Browser, made on the Create tab.
        if ed.adaptive.is_none() {
            selector(ui, Icon::FamilyEditor, &tf("Family: {}", &[&category]));
            ui.add_space(6.0);
            identity(ui, &mut ed.name, &category);
            ui.add_space(6.0);
            other(ui, &mut ed, self.session.model.units);
            self.put_family(ed);
            return;
        }
        if let Some(d) = ed.adaptive.as_ref() {
            // The type selector's place: what is selected, as Revit names it, or the family.
            let (what, icon) = match selected.as_slice() {
                [] => (tf("Family: {}", &[&category]), Icon::FamilyEditor),
                [id] => match d.point_type(id) {
                    Some(PointType::Placement) => (t("Adaptive Point").to_string(), Icon::MakeAdaptive),
                    Some(PointType::ShapeHandle) => (t("Shape Handle Point").to_string(), Icon::MakeAdaptive),
                    Some(PointType::Reference) => (t("Reference Point").to_string(), Icon::PointElement),
                    None if d.nodes.iter().any(|n| &n.id == id && n.is_form()) => (t("Form").to_string(), Icon::CreateForm),
                    None => (t("Reference Line").to_string(), Icon::Line),
                },
                many => (tf("Multi-Select ({})", &[&many.len().to_string()]), Icon::Modify),
            };
            selector(ui, icon, &what);
            ui.add_space(6.0);
            if selected.is_empty() {
                identity(ui, &mut ed.name, &category);
                if let Some(id) = ed.replay {
                    ui.label(
                        RichText::new(tf("Replaying instance {} with its exact parameters and placement", &[&id.to_string()]))
                            .color(pal().muted),
                    );
                }
                other(ui, &mut ed, self.session.model.units);
            }
        }
        if let Some(d) = ed.adaptive.as_mut() {
            let mut delete = Vec::new();
            for id in &selected {
                ui.push_id(id, |ui| {
                    let planes = bimcraft_core::family::Planes { planes: &ed.planes, lines: &ed.lines };
                    if let Some(a) = crate::adaptive_editor::point_properties(ui, d, planes, id, &values, &params) {
                        ed.associating = Some(a);
                    }
                    if let Some(node) = d.nodes.iter_mut().find(|n| &n.id == id) {
                        let open = ui.ctx().data(|w| w.get_temp::<bool>(UiId::new("family-relationship"))).unwrap_or(true);
                        if group_header(ui, t("Relationship"), open) {
                            ui.ctx().data_mut(|w| w.insert_temp(UiId::new("family-relationship"), !open));
                        }
                        if open {
                            Frame::new().inner_margin(Margin { left: 8, right: 4, top: 4, bottom: 4 }).show(ui, |ui| {
                                let (deleted, asked) = crate::adaptive_editor::node_properties(ui, node, &values, &params);
                                if deleted {
                                    delete.push(id.clone());
                                }
                                if asked.is_some() {
                                    ed.associating = asked;
                                }
                            });
                        }
                    }
                });
            }
            if !delete.is_empty() {
                d.remove(&delete);
                ed.modeller.selection.retain(|id| !delete.contains(id));
            }
            ui.add_space(8.0);
            crate::adaptive_editor::diagnostics(ui, &d.evaluate(&values, &d.preview));
        }
        if let Some(p) = ed.problem() {
            ui.label(RichText::new(p).color(pal().error));
        }
        if let Some(a) = ui
            .collapsing(t("All elements and the test placement"), |ui| crate::adaptive_editor::show(ui, &mut ed.adaptive, &values, &params))
            .body_returned
            .flatten()
        {
            ed.associating = Some(a);
        }
        self.put_family(ed);
    }

    /// The family document's Project Browser: its views, as a family template has them, and the forms and plan symbols of a
    /// family that isn't adaptive. A click selects one, so Properties show it; its right-click menu has Select, Delete, and for a
    /// form Move Up and Move Down (the order says which forms a void cuts: those listed above it) and Edit Extrusion and the like.
    pub(crate) fn family_browser(&mut self, ui: &mut Ui) {
        /// What a row of Forms or Symbols asked for, done once the browser is drawn.
        enum Asked {
            Form(usize),
            Symbol(usize),
            Move(usize, isize),
            DeleteForm(usize),
            DeleteSymbol(usize),
            Connector(usize),
            DeleteConnector(usize),
            Edit(usize),
        }
        let picked = self.active_family().and_then(FamilyEditor::selected_form);
        let picked_symbol = self.active_family().and_then(FamilyEditor::selected_symbol);
        let picked_connector = self.active_family().and_then(FamilyEditor::selected_connector);
        let connectors: Vec<String> = match self.active_family() {
            Some(ed) if ed.adaptive.is_none() => (0..ed.connectors.len()).map(|i| ed.connector_name(i)).collect(),
            _ => Vec::new(),
        };
        type Row = (String, bool, &'static str);
        let (forms, symbols): (Vec<Row>, Vec<String>) = match self.active_family() {
            Some(ed) if ed.adaptive.is_none() => (
                ed.forms
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let edit = f.form().map(|x| super::extrusion::edit_label(x.kind())).unwrap_or(msg!("Edit Extrusion"));
                        (format!("{}. {}", i + 1, f.describe()), f.mesh(), edit)
                    })
                    .collect(),
                (0..ed.symbols.len()).map(|i| ed.symbol_name(i)).collect(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        let mut asked = None;
        ScrollArea::vertical().id_salt("family-browser").auto_shrink([false, false]).show(ui, |ui| {
            Frame::new().inner_margin(8.0).show(ui, |ui| {
                egui::CollapsingHeader::new(t("Views (all)")).id_salt("family-views").default_open(true).show(ui, |ui| {
                    use bimcraft_core::family::views::{Side, Which};
                    let elevations: Vec<(Option<Which>, &str)> = Side::ALL.iter().map(|s| (Some(Which::Elevation(*s)), s.name())).collect();
                    type Entries<'a> = Vec<(Option<Which>, &'a str)>;
                    let groups: [(&str, Entries); 4] = [
                        (msg!("Floor Plans"), vec![(Some(Which::Plan), "Ref. Level")]),
                        (msg!("Ceiling Plans"), vec![(Some(Which::Ceiling), "Ref. Level")]),
                        (msg!("3D Views"), vec![(None, VIEW)]),
                        (msg!("Elevations"), elevations),
                    ];
                    let shown = self.active_family().and_then(|ed| ed.views.shown);
                    for (group, views) in groups {
                        egui::CollapsingHeader::new(t(group)).id_salt(("family-views", group)).default_open(true).show(ui, |ui| {
                            for (which, view) in views {
                                if ui.selectable_label(shown == which, t(view)).clicked()
                                    && let Some(ed) = self.active_family_mut()
                                {
                                    ed.views.show(which);
                                }
                            }
                        });
                    }
                });
                self.nested_browser(ui);
                if !forms.is_empty() {
                    egui::CollapsingHeader::new(t("Forms")).id_salt("family-forms").default_open(true).show(ui, |ui| {
                        let last = forms.len() - 1;
                        for (i, (form, mesh, edit)) in forms.iter().enumerate() {
                            let r = ui.selectable_label(picked == Some(i), form);
                            if r.clicked() {
                                asked = Some(Asked::Form(i));
                            }
                            r.context_menu(|ui| {
                                if ui.button(t("Select")).clicked() {
                                    asked = Some(Asked::Form(i));
                                }
                                if ui.add_enabled(!mesh, egui::Button::new(t(edit))).clicked() {
                                    asked = Some(Asked::Edit(i));
                                }
                                ui.separator();
                                if ui.add_enabled(i > 0, egui::Button::new(t("Move Up"))).clicked() {
                                    asked = Some(Asked::Move(i, -1));
                                }
                                if ui.add_enabled(i < last, egui::Button::new(t("Move Down"))).clicked() {
                                    asked = Some(Asked::Move(i, 1));
                                }
                                ui.separator();
                                if ui.button(t("Delete")).clicked() {
                                    asked = Some(Asked::DeleteForm(i));
                                }
                            });
                        }
                    });
                }
                if !symbols.is_empty() {
                    egui::CollapsingHeader::new(t("Symbols")).id_salt("family-symbols").default_open(true).show(ui, |ui| {
                        for (i, symbol) in symbols.iter().enumerate() {
                            let r = ui.selectable_label(picked_symbol == Some(i), symbol);
                            if r.clicked() {
                                asked = Some(Asked::Symbol(i));
                            }
                            r.context_menu(|ui| {
                                if ui.button(t("Select")).clicked() {
                                    asked = Some(Asked::Symbol(i));
                                }
                                if ui.button(t("Delete")).clicked() {
                                    asked = Some(Asked::DeleteSymbol(i));
                                }
                            });
                        }
                    });
                }
                // Its connectors, as Revit's browser lists none but its Properties show each: a click selects one.
                if !connectors.is_empty() {
                    egui::CollapsingHeader::new(t("Connectors")).id_salt("family-connectors").default_open(true).show(ui, |ui| {
                        for (i, connector) in connectors.iter().enumerate() {
                            let r = ui.selectable_label(picked_connector == Some(i), connector);
                            if r.clicked() {
                                asked = Some(Asked::Connector(i));
                            }
                            r.context_menu(|ui| {
                                if ui.button(t("Select")).clicked() {
                                    asked = Some(Asked::Connector(i));
                                }
                                if ui.button(t("Delete")).clicked() {
                                    asked = Some(Asked::DeleteConnector(i));
                                }
                            });
                        }
                    });
                }
            });
        });
        let Some(ed) = self.active_family_mut() else { return };
        match asked {
            // Selected, as Revit's browser selects what it lists: its Properties show.
            Some(Asked::Form(i)) => ed.select_only_form(i),
            Some(Asked::Symbol(i)) => ed.select_symbol(i),
            Some(Asked::Move(i, by)) => ed.move_form(i, by),
            Some(Asked::DeleteForm(i)) => ed.remove_form(i),
            Some(Asked::DeleteSymbol(i)) => ed.remove_symbol(i),
            Some(Asked::Connector(i)) => ed.select_connector(i),
            Some(Asked::DeleteConnector(i)) => ed.remove_connector(i),
            Some(Asked::Edit(i)) => {
                ed.select_only_form(i);
                if let Err(e) = ed.edit_form_sketch(i) {
                    self.status = Status { text: e, error: true };
                }
            }
            None => {}
        }
    }

    /// The family document's view: the modelling view (an adaptive family's) or the 3D preview (another's, turned by a drag, or
    /// as Revit's by Shift and the middle button) filling the pane, its tab over its top left corner with the family's close
    /// button, and its view control bar at the bottom.
    pub(crate) fn family_central(&mut self, ui: &mut Ui) {
        let Some(mut ed) = self.take_family() else { return };
        ed.snaps = self.snaps_now();
        let pane = ui.available_rect_before_wrap();
        self.view_rect = pane;
        self.pane = pane;
        let values = ed.values();
        let hidden = ed.adaptive.as_ref().map(|d| ed.temp_hidden(d)).unwrap_or_default();
        if let Some(which) = ed.views.shown {
            // A plan or an elevation (F4): the project's own drawing of the family in a scratch model.
            let base = Arc::clone(&self.session.model);
            if let Some(e) = ed.view_2d(ui, &base, which, pane) {
                self.status = Status { text: e, error: true };
            }
        } else if let Some(d) = ed.adaptive.as_mut() {
            ui.scope_builder(UiBuilder::new().max_rect(pane), |ui| {
                let planes = bimcraft_core::family::Planes { planes: &ed.planes, lines: &ed.lines };
                adaptive_view::show(ui, d, planes, &mut ed.modeller, &values, pane.size(), &hidden);
            });
            // What a tool in the view could not do, in the status bar.
            if let Some(e) = ui.ctx().data_mut(|w| w.remove_temp::<String>(UiId::new("adaptive-view-problem"))) {
                self.status = Status { text: e, error: true };
            }
        } else {
            self.family_3d(ui, &mut ed, pane);
        }
        // The window's tabs, the project's and each family document's, over the view's top left corner.
        let card = crate::style::card([true, false, true, false]).inner_margin(Margin::same(2));
        let mut over = ui.new_child(UiBuilder::new().id_salt("family-view-tabs").max_rect(pane));
        // While the bar is drawn the document is in its place, so its tabs are named.
        self.put_family(ed);
        let foot = card.show(&mut over, |ui| self.view_tabs(ui));
        let Some(mut ed) = self.take_family() else {
            self.tabs_foot = foot.response.rect.bottom();
            return;
        };
        self.tabs_foot = foot.response.rect.bottom();
        // The view control bar, at the view's bottom left.
        let mut bar = ui.new_child(UiBuilder::new().id_salt("family-view-bar").max_rect(pane).layout(Layout::bottom_up(Align::Min)));
        crate::style::card([true, true, true, true]).inner_margin(Margin::symmetric(6, 2)).show(&mut bar, |ui| {
            ui.horizontal(|ui| {
                // F8: the document's Detail Level, which each of its views draws its forms and symbols at.
                if ed.adaptive.is_none() {
                    let now = ed.views.detail;
                    let tip = msg!("Detail Level: the forms and symbols each view of the family draws, as their Visibility Settings say");
                    let r = crate::footer::menu_button(ui, "Detail Level", now.name(), tip, None);
                    egui::Popup::menu(&r).show(|ui| {
                        for d in bimcraft_core::graphics::Detail::ALL {
                            if ui.selectable_label(now == d, t(d.name())).clicked() {
                                ed.views.detail = d;
                            }
                        }
                    });
                }
                ui.label(RichText::new(t("Shaded")).color(pal().muted));
                if icon_button(ui, Icon::Fit, "Zoom to Fit").clicked() {
                    ed.modeller.camera = None;
                    ed.orbit.fit();
                }
                // Revit's Preview Visibility: the family as placed, the forms its type hides left out rather than ghosted.
                if ed.adaptive.is_none() {
                    let on = ed.preview_visibility();
                    if crate::small_button(ui, Icon::Visibility, PREVIEW_VISIBILITY, on, PREVIEW_VISIBILITY).clicked() {
                        ed.document.preview_visibility = !on;
                    }
                }
                self.family_temp_menu(ui, &mut ed);
            });
        });
        self.work = pane.with_min_y(pane.top().max(self.tabs_foot));
        // While it hides anything for now, the project's cyan frame round the view, as Revit draws.
        let hiding = ed.temporary().hides();
        if hiding {
            ui.painter().rect_stroke(pane.shrink(1.5), 0.0, Stroke::new(3.0, crate::footer::FRAME), StrokeKind::Inside);
        }
        // While it is on, a frame round the view and its name over it, as Revit marks a view showing what is placed: inside the
        // cyan one when both are, so each shows.
        if ed.adaptive.is_none() && ed.preview_visibility() {
            let inset = if hiding { 4.5 } else { 1.5 };
            ui.painter().rect_stroke(pane.shrink(inset), 0.0, Stroke::new(3.0, super::PREVIEW_FRAME), StrokeKind::Inside);
            let text = t(PREVIEW_VISIBILITY);
            let font = FontId::proportional(13.0);
            let size = ui.painter().layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE).size() + vec2(20.0, 6.0);
            let banner = Rect::from_center_size(pos2(pane.center().x, self.work.top() + 6.0 + size.y / 2.0), size);
            ui.painter().rect_filled(banner, 3.0, super::PREVIEW_FRAME);
            ui.put(banner, egui::Label::new(RichText::new(text).font(font).color(Color32::WHITE).strong()).selectable(false));
        }
        // Why a family that isn't adaptive can't be tried or loaded, in the view under its tab (a dialog open says it itself).
        if ed.adaptive.is_none()
            && ed.dialog.is_none()
            && let Some(problem) = ed.problem()
        {
            let room = self.work.shrink(12.0);
            ui.scope_builder(UiBuilder::new().max_rect(room), |ui| {
                crate::style::card([true; 4]).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
                    ui.label(RichText::new(problem).color(pal().error));
                });
            });
        }
        self.put_family(ed);
    }

    /// The family document's Options Bar: the contextual tab's name; drawing, Revit's Placement Plane; with lines selected, the
    /// depth Create Form gives a solid or a void.
    pub(crate) fn family_options(&mut self, ui: &mut Ui) -> bool {
        let units = self.session.model.units;
        let Some(ed) = super::tabs::active_in(&mut self.families, self.sheet) else { return false };
        let Some(name) = context(ed) else { return false };
        if ed.sketching() {
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                ui.label(t(name));
                ui.separator();
                super::sketch::options_bar(ui, ed, units);
            });
            return true;
        }
        let values = ed.values();
        let lines = ed.adaptive.as_ref().is_some_and(|d| !adaptive_view::selected(d, &ed.modeller, &values).1.is_empty());
        let drawing = !matches!(ed.modeller.tool, Draw::Modify);
        let planes = ed.adaptive.as_ref().map(|d| work_planes(d, ed.plane_set())).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.set_min_height(24.0);
            ui.label(t(name));
            if drawing {
                ui.separator();
                let l = ui.label(t("Placement Plane"));
                let shown = if ed.modeller.picking_plane {
                    t(PICK).to_string()
                } else {
                    planes.iter().find(|(p, _)| *p == ed.modeller.work_plane).map(|(_, n)| n.clone()).unwrap_or_default()
                };
                let list = ComboBox::from_id_salt("family-placement-plane").selected_text(shown).width(280.0).show_ui(ui, |ui| {
                    // As Revit's: Pick first, a plane picked in the view.
                    if ui.selectable_label(ed.modeller.picking_plane, t(PICK)).clicked() {
                        ed.modeller.picking_plane = true;
                    }
                    for (plane, label) in &planes {
                        ui.selectable_value(&mut ed.modeller.work_plane, plane.clone(), label);
                    }
                });
                list.response.labelled_by(l.id);
            }
            // How deep Create Form extrudes a closed profile.
            if lines {
                ui.separator();
                let l = ui.label(t("Depth (mm)"));
                ui.add(egui::DragValue::new(&mut ed.modeller.depth).speed(5.).range(1.0..=100_000.0)).labelled_by(l.id);
            }
            // Component's family and type (T6a).
            super::nested::placing_options(ui, ed);
            // A dimension's Label, as Revit's Options Bar has it (T19).
            if let Some(g) = ed.selected_dimension() {
                ui.separator();
                if let Some(e) = ed.dimension_options(ui, g) {
                    self.status = Status { text: e, error: true };
                }
            }
            // A selected extrusion's Depth, as Revit's Options Bar has it.
            let driven = ed.selected_form().map(|i| ed.driven_ends(i)).unwrap_or_default();
            if let Some(i) = ed.selected_form()
                && let Some(f) = ed.forms.get_mut(i)
                && !f.revolve
                && !f.mesh()
            {
                ui.separator();
                super::extrusion::depth(ui, units, f, &values, driven);
            }
        });
        true
    }

    /// Whether the family document's Options Bar has anything to show: only with a contextual tab, as Revit's.
    pub(crate) fn family_has_options(&self) -> bool {
        self.active_family().is_some_and(|ed| context(ed).is_some())
    }

    /// What the family document's status bar asks for: an adaptive family's modelling view's tool, or how another's preview turns.
    pub(crate) fn family_prompt(&self) -> Option<&'static str> {
        let ed = self.active_family()?;
        if let Some(s) = ed.sketch.as_ref() {
            // Align on the sketch's lines (F7b).
            if ed.views.aligning.is_some() {
                return Some(match s.align_from {
                    None => msg!("Click the reference to align to: a reference plane, a reference line or a line."),
                    Some(_) => msg!("Click the line of the sketch that moves onto it; its padlock locks them."),
                });
            }
            return Some(super::sketch::prompt(s));
        }
        Some(match (ed.views.shown, ed.adaptive.is_some()) {
            (Some(_), true) => msg!("An adaptive family's plans and elevations show its test placement; model it in {3D}."),
            (Some(_), false) if ed.views.picking.is_some_and(|p| p.line) => {
                msg!("Click a reference line: the work plane it was sketched in. Esc goes back to the dialog.")
            }
            (Some(_), false) if ed.views.picking.is_some() => {
                msg!("Click a reference plane or a datum for the work plane. Esc goes back to the dialog.")
            }
            (Some(_), false) if ed.views.drawing.is_some() => msg!("Click two points: a reference plane through them."),
            (Some(_), false) if ed.views.dimensioning.is_some() && ed.views.dim_kind == bimcraft_core::family::DimKind::Angular => {
                msg!("Click lines that cross (reference lines, reference planes, lines of forms), then click where the dimension goes.")
            }
            (Some(_), false) if ed.views.dimensioning.is_some() && ed.views.dim_kind != bimcraft_core::family::DimKind::Aligned => {
                msg!("Click a circle or an arc, then click where the dimension goes.")
            }
            (Some(_), false) if ed.views.dimensioning.is_some() => {
                msg!("Click parallel reference planes, extrusion ends or lines of forms, then click where the dimension goes.")
            }
            (Some(_), false) if ed.views.aligning.as_ref().is_some_and(Option::is_none) => msg!("Click the reference to align to."),
            (Some(_), false) if ed.views.aligning.is_some() => msg!("Click what moves onto it."),
            (Some(_), false) if ed.placing.is_some() => msg!("Click where the component goes, on the view's work plane."),
            (Some(_), false) if ed.views.pivoting.is_some() => {
                msg!("Click the point it turns about: where two planes cross, a line's end, a point of a sketch, or anywhere.")
            }
            (Some(_), false) => msg!(
                "Click a form, a nested instance, a reference plane, a reference line or a dimension to select it; drag a selected plane to move it, a grip to turn or stretch."
            ),
            (None, true) => adaptive_view::prompt(&ed.modeller),
            (None, false) => msg!(
                "Click a form or a nested instance to select it. A drag or Shift and the middle button orbit, the middle button pans, the wheel zooms."
            ),
        })
    }

    /// Undo in a family document: one step back through its own history, done when the frame ends; the project's is not touched.
    pub(crate) fn family_undo(&mut self) {
        if let Some(ed) = self.active_family_mut() {
            // While sketching, the sketch's own last change.
            if ed.sketching() {
                ed.undo_sketch();
            } else {
                ed.ask.undo = true;
            }
        }
    }

    /// Redo in a family document: a step undone done again.
    pub(crate) fn family_redo(&mut self) {
        if let Some(ed) = self.active_family_mut().filter(|ed| !ed.sketching()) {
            ed.ask.redo = true;
        }
    }

    /// What Find lists on a family tab, by name in `shortcuts::ACTIONS`: what the family document does itself (Delete, Align and
    /// Aligned Dimension in a plan or an elevation, Temporary Hide/Isolate, Zoom to Fit in {3D}, its {3D} view) and what is the
    /// window's (Properties, Project Browser, Keyboard Shortcuts). The project's tools are left out: from a family tab they
    /// would change the project out of sight.
    pub(crate) fn family_find_actions(&self) -> Vec<&'static str> {
        let Some(ed) = self.active_family() else { return Vec::new() };
        let mut out = vec!["Delete"];
        if ed.editing_2d() && ed.sketch.is_none() {
            out.extend(["Aligned Dimension", "Align"]);
        }
        out.extend([
            "Temporary Hide Element",
            "Temporary Hide Category",
            "Temporary Isolate Element",
            "Temporary Isolate Category",
            "Reset Temporary Hide/Isolate",
            "3D View",
        ]);
        if matches!(self.sheet, crate::Sheet::Family(_, None)) {
            out.push("Zoom to Fit");
        }
        out.extend(["Properties", "Project Browser", "Keyboard Shortcuts"]);
        out
    }

    /// One of `family_find_actions` done on the active family document, as its own keys and ribbon do it; whether it was one.
    pub(crate) fn family_find_action(&mut self, action: &str) -> bool {
        if !self.family_find_actions().contains(&action) {
            return false;
        }
        let temp = |category| super::temp_hide::Asked::Hide { category };
        let isolate = |category| super::temp_hide::Asked::Isolate { category };
        match action {
            "Delete" => {
                let Some(ed) = self.active_family_mut() else { return false };
                // A sketch's lines picked, else what is selected, as the Delete key does.
                if let Some(s) = ed.sketch.as_mut() {
                    s.delete_picked();
                } else if ed.deletable()
                    && let Err(text) = ed.delete_selected()
                {
                    self.status = Status { text, error: true };
                }
            }
            "Aligned Dimension" | "Align" => {
                let Some(ed) = self.active_family_mut() else { return false };
                if action == "Align" {
                    ed.align_tool();
                } else {
                    ed.dimension_tool();
                }
            }
            "Temporary Hide Element" => self.family_temp(temp(false)),
            "Temporary Hide Category" => self.family_temp(temp(true)),
            "Temporary Isolate Element" => self.family_temp(isolate(false)),
            "Temporary Isolate Category" => self.family_temp(isolate(true)),
            "Reset Temporary Hide/Isolate" => self.family_temp(super::temp_hide::Asked::Reset),
            "3D View" => {
                if let Some(ed) = self.active_family_mut() {
                    ed.views.show(None);
                }
            }
            // The family's {3D}, the window's palettes and dialogs: as anywhere.
            _ => return self.execute_shortcut_action(action),
        }
        true
    }

    /// Save and Save As on a family tab (the Quick Access Toolbar's Save, File > Save Family and Save Family As…, Ctrl+S and
    /// Ctrl+Shift+S): the active family document to its file, as Revit saves the active document; the project never.
    pub(crate) fn family_save(&mut self, ask: bool) {
        if let Some(ed) = self.active_family_mut() {
            if ask {
                ed.ask.save = true;
            } else {
                ed.ask.save_in_place = true;
            }
        }
    }

    /// The family document's keys: Ctrl+S saves it (Ctrl+Shift+S asks where), Ctrl+K opens Find, Ctrl+Z undoes a step of its
    /// own, Ctrl+Y and Ctrl+Shift+Z redo one, and the two letters of Temporary Hide/Isolate (`temp_hide.rs`), unless a field
    /// being typed in (whose own they are) or a dialog has them; Ctrl+S and Ctrl+K work while a field has the keyboard, as on
    /// a project's tab.
    pub(crate) fn family_keys(&mut self, ctx: &Context) {
        let Some(ed) = self.active_family() else { return };
        if ed.modal() {
            return;
        }
        if !self.keys_taken(ctx) {
            let pressed = |m: egui::Modifiers, k: egui::Key| ctx.input_mut(|i| i.consume_key(m, k));
            if pressed(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::S) {
                self.family_save(true);
            } else if pressed(egui::Modifiers::COMMAND, egui::Key::S) {
                self.family_save(false);
            }
            if pressed(egui::Modifiers::COMMAND, egui::Key::K) {
                self.find = Some(String::new());
            }
        }
        if ctx.text_edit_focused() {
            return;
        }
        // Letters typed, but not while a menu has the keys.
        if !egui::Popup::is_any_open(ctx) {
            self.family_typed(ctx);
        }
        let Some(ed) = self.active_family_mut() else { return };
        // Sketch mode: Escape lets the shape's points go, then its tool; Delete takes the lines picked away; Ctrl+Z takes back
        // the sketch's own last change. The document's history waits for Finish.
        if let Some(s) = ed.sketch.as_mut() {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                // Align's reference let go, then Align itself; else the sketch's own Escape.
                if ed.views.aligning.is_some() && s.align_from.is_none() && s.offer.is_none() {
                    ed.views.aligning = None;
                } else {
                    s.escape();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                s.delete_picked();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                ed.undo_sketch();
            }
            return;
        }
        // Escape lets a selected form go, though a button clicked has the keyboard (the window's own Escape does it otherwise).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // The plane being drawn first, then the tool, then the selection.
            if ed.stop_picking() || ed.views.pivoting.take().is_some() {
            } else if let Some(refs) = ed.views.dimensioning.as_mut() {
                if refs.is_empty() {
                    ed.views.dimensioning = None;
                } else {
                    refs.clear();
                }
            } else if let Some(first) = ed.views.aligning {
                ed.views.aligning = first.map(|_| None);
                ed.views.align_lock = None;
            } else if ed.views.dim.is_some() {
                ed.views.dim = None;
            } else if let Some(first) = ed.views.drawing {
                ed.views.drawing = first.map(|_| None);
            } else if ed.views.plane.take().is_none() && ed.views.line.take().is_none() {
                ed.let_form_go();
            }
        }
        let pressed = |m: egui::Modifiers, k: egui::Key| ctx.input_mut(|i| i.consume_key(m, k));
        let mut refused = None;
        // Delete takes what is selected away, as the ribbon's Delete does (a text field with the keyboard keeps the key).
        if ed.deletable()
            && !ctx.egui_wants_keyboard_input()
            && pressed(egui::Modifiers::NONE, egui::Key::Delete)
            && let Err(e) = ed.delete_selected()
        {
            refused = Some(e);
        }
        if pressed(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z) || pressed(egui::Modifiers::COMMAND, egui::Key::Y) {
            ed.ask.redo = true;
        } else if pressed(egui::Modifiers::COMMAND, egui::Key::Z) {
            ed.ask.undo = true;
        }
        if let Some(text) = refused {
            self.status = Status { text, error: true };
        }
    }

    /// The dialogs a family document's ribbon opens: Family Types (its types and one table of its parameters: `types_dialog`) and Family
    /// Category and Parameters (its name, category, options (`options.rs`) and, for a door or a window, how it opens). Apply keeps what is typed,
    /// Cancel puts back what was there when the dialog opened or last applied, OK keeps it and closes the dialog.
    pub(crate) fn document_dialogs(&mut self, ctx: &Context, ed: &mut FamilyEditor, materials: &[String]) {
        // Associate Family Parameter, from a row's "=": a parameter New Parameter makes goes in the family at once, as in Revit.
        ed.associate_dialog(ctx);
        // F7: Constraints are not satisfied, and Create Parameter… for a dimension's label.
        ed.constraints_dialog(ctx);
        ed.create_parameter_dialog(ctx);
        // T12: the Work Plane dialog and Go To View.
        let base = Arc::clone(&self.session.model);
        if let Some(e) = ed.work_plane_dialog(ctx, &base) {
            self.status = Status { text: e, error: true };
        }
        ed.go_to_view_dialog(ctx);
        // F8: Family Element Visibility Settings.
        ed.visibility_dialog(ctx);
        let Some(dialog) = ed.dialog else {
            ed.document.dialog_saved = None;
            ed.document.category_shown = false;
            return;
        };
        if ed.document.dialog_saved.is_none() {
            ed.document.dialog_saved = Some(Draft::of(ed));
        }
        let title = match dialog {
            DocDialog::Types => t("Family Types"),
            DocDialog::Category => t("Family Category and Parameters"),
        };
        let base = Arc::clone(&self.session.model);
        let units = base.units;
        let (mut apply, mut ok, mut cancel) = (false, false, false);
        let width = if dialog == DocDialog::Category { 560.0 } else { 900.0 };
        let r = Modal::new(UiId::new(("family-dialog", dialog as u8)), title).width(width).show(ctx, |ui| {
            crate::popup::body(ui, |ui| match dialog {
                DocDialog::Types => super::types_dialog::body(ed, ui, &base, materials),
                DocDialog::Category => category_body(ed, ui, units),
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ok = ui.button(t("OK")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    apply = ui.button(t("Apply")).clicked();
                });
            });
        });
        if dialog == DocDialog::Types {
            // Name and Parameter Properties, over Family Types.
            super::types_dialog::over(ctx, ed);
        }
        // A formula that doesn't read is refused, as Revit's Family Types refuses it, with T9's words; Cancel still goes back.
        if (ok || apply)
            && !cancel
            && dialog == DocDialog::Types
            && let Some(why) = ed.formula_refusal()
        {
            ed.refuse_types(why);
            (ok, apply) = (false, false);
        }
        // Values a type's dimensions can't hold (a labelled length of 0): "Constraints are not satisfied", over the dialog.
        if (ok || apply) && !cancel && dialog == DocDialog::Types && ed.constraints_refusal(ok) {
            (ok, apply) = (false, false);
        }
        // Options the `family` command would refuse (an OmniClass number that isn't one), said in the dialog, which stays open.
        if (ok || apply)
            && !cancel
            && dialog == DocDialog::Category
            && let Some(why) = ed.options_refusal()
        {
            ed.document.options_refused = Some((Draft::of(ed), why));
            (ok, apply) = (false, false);
        }
        let closing = cancel || r.should_close() || ok;
        if closing {
            ed.types_closed();
            ed.document.options_refused = None;
            ed.document.category_filter.clear();
            ed.document.category_shown = false;
        }
        if cancel || r.should_close() {
            if let Some(saved) = ed.document.dialog_saved.take() {
                saved.restore(ed);
            }
            ed.dialog = None;
        } else if ok {
            ed.dialog = None;
            ed.document.dialog_saved = None;
        } else if apply {
            ed.document.dialog_saved = Some(Draft::of(ed));
        }
    }
}

/// Family Category and Parameters' body, as Revit's: the family's name; Family Category, the categories in a list under a
/// filter, the family's picked; Family Parameters, a table of Parameter and Value (its options, `options.rs`, and a door's or a
/// window's Operation); then what Apply or OK was refused for, or why the draft can't be tried.
fn category_body(ed: &mut FamilyEditor, ui: &mut Ui, units: Units) {
    let h = ui.spacing().interact_size.y;
    Grid::new("family-category").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        let l = ui.label(t("Family name"));
        ui.add_sized([260.0, h], TextEdit::singleline(&mut ed.name)).labelled_by(l.id);
        ui.end_row();
    });
    ui.add_space(8.0);
    ui.label(RichText::new(t("Family Category")).strong());
    ui.horizontal(|ui| {
        let l = ui.label(t("Filter"));
        let field = TextEdit::singleline(&mut ed.document.category_filter).hint_text(t("Search…"));
        ui.add_sized([260.0, h], field).labelled_by(l.id);
    });
    let filter = ed.document.category_filter.trim().to_lowercase();
    // The list scrolls to the family's category when the dialog opens.
    let first = !std::mem::replace(&mut ed.document.category_shown, true);
    Frame::new().stroke(Stroke::new(1.0, pal().border)).inner_margin(Margin::same(4)).show(ui, |ui| {
        ScrollArea::vertical().id_salt("family-category-list").max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.with_layout(Layout::top_down_justified(Align::Min), |ui| category_list(ed, ui, &filter, first));
        });
    });
    ui.add_space(8.0);
    ui.label(RichText::new(t("Family Parameters")).strong());
    // The table as wide as the dialog: the names' column, the values' the rest.
    const NAME_W: f32 = 230.0;
    let value_w = (ui.available_width() - NAME_W - 12.0).max(160.0);
    Grid::new("family-parameters").num_columns(2).striped(true).spacing(vec2(12.0, 4.0)).min_col_width(NAME_W).show(ui, |ui| {
        ui.label(RichText::new(t("Parameter")).strong());
        ui.label(RichText::new(t("Value")).strong());
        ui.end_row();
        super::options::rows(ui, &mut ed.options, ed.category, units, "family-category-dialog", value_w);
        if ed.category.hosted() {
            let l = ui.label(t("Operation"));
            let shown = ed.operation.map_or(t("(none)"), |op| t(op.label()));
            let c = ComboBox::from_id_salt("family-operation").selected_text(shown).width(value_w).show_ui(ui, |ui| {
                if ui.selectable_label(ed.operation.is_none(), t("(none)")).clicked() {
                    ed.operation = None;
                }
                // A window offers no gate's operation, which the family would refuse.
                for op in Operation::ALL.into_iter().filter(|op| ed.category != Category::Window || !op.industrial()) {
                    if ui.selectable_label(ed.operation == Some(op), t(op.label())).clicked() {
                        ed.operation = Some(op);
                    }
                }
            });
            c.response.labelled_by(l.id);
            ui.end_row();
        }
    });
    // Said in the dialog: the view's would be under it.
    let refused = ed.document.options_refused.as_ref().filter(|(d, _)| *d == Draft::of(ed)).map(|(_, w)| w.clone());
    if refused.is_none() {
        ed.document.options_refused = None;
    }
    if let Some(p) = refused.or_else(|| ed.problem().map(str::to_string)) {
        ui.add_space(6.0);
        ui.label(RichText::new(p).color(pal().error));
    }
}

/// Family Category and Parameters' list: the categories `filter` keeps, the family's picked; scrolled to it when the dialog
/// opens (`first`).
fn category_list(ed: &mut FamilyEditor, ui: &mut Ui, filter: &str, first: bool) {
    let mut any = false;
    for c in CATEGORIES {
        let label = category_label(c);
        if !filter.is_empty() && !label.to_lowercase().contains(filter) {
            continue;
        }
        any = true;
        let r = ui.selectable_label(ed.category == c, &label);
        if first && ed.category == c {
            r.scroll_to_me(Some(Align::Center));
        }
        if r.clicked() {
            ed.category = c;
        }
    }
    if !any {
        ui.label(RichText::new(t("Nothing matches the search.")).color(pal().muted));
    }
}

/// Revit's Preview Visibility: the view control bar's toggle and the banner over the view while it is on.
const PREVIEW_VISIBILITY: &str = msg!("Preview Visibility");

/// Placement Plane's first entry: a plane picked in the view.
const PICK: &str = msg!("Pick…");

/// The work planes a family offers, as Placement Plane lists them: the family's planes (its datums, its named planes, its
/// reference lines' faces: `Planes::listed`), then each point's three planes.
pub(crate) fn work_planes(d: &bimcraft_core::adaptive::Definition, planes: bimcraft_core::family::Planes) -> Vec<(WorkPlane, String)> {
    let mut out: Vec<(WorkPlane, String)> =
        planes.listed().into_iter().map(|(r, label)| (WorkPlane::Plane(r), t(&label).to_string())).collect();
    for n in d.nodes.iter().filter(|n| matches!(n.operation, bimcraft_core::adaptive::Operation::Point { .. })) {
        for (plane, name) in [(Plane::Xy, "XY"), (Plane::Xz, "XZ"), (Plane::Yz, "YZ")] {
            let label = tf("{} : {} plane", &[if n.name.is_empty() { &n.id } else { &n.name }, name]);
            out.push((WorkPlane::Point { point: n.id.clone(), plane }, label));
        }
    }
    out
}

/// A tool Revit's family editor has and BIMCraft does not yet: where Revit has it, greyed.
fn later(ui: &mut Ui, icon: Icon, label: &str) {
    ui.add_enabled_ui(false, |ui| big_button(ui, icon, label, false, msg!("Not in BIMCraft yet")));
}

/// Where a point of the family's axes shows on `pane`, the {3D} picture `size` pixels wide drawn through `cam` and stretched over
/// it; none behind the eye.
fn on_pane(cam: bimcraft_core::view3d::Camera, size: [u32; 2], pane: Rect) -> impl Fn([f64; 3]) -> Option<egui::Pos2> {
    move |q| {
        let (x, y, _) = cam.project(q);
        let (sx, sy) = (pane.width() / size[0].max(1) as f32, pane.height() / size[1].max(1) as f32);
        (x.is_finite() && y.is_finite()).then(|| pane.min + vec2(x as f32 * sx, y as f32 * sy))
    }
}

/// Create > Connectors: one Connector button whose list places a pipe, power, switch, data or earthing connector.
fn connectors_panel(ui: &mut Ui, ed: &mut FamilyEditor) {
    ribbon::panel(ui, msg!("Connectors"), |ui| {
        let tip = msg!("Connector: a pipe, power, switch, data or earthing connector, where the family joins a system");
        let r = big_button(ui, Icon::Pipe, msg!("Connector"), false, tip);
        ribbon::choices(ui, &r, false, |ui| {
            for (kind, label, tip) in super::connectors::KINDS {
                if ui.button(t(label)).on_hover_text(t(tip)).clicked() {
                    ed.add_connector(kind);
                    ui.close();
                }
            }
        });
    });
}

/// The same, a small button.
fn later_small(ui: &mut Ui, icon: Icon, label: &str) {
    ui.add_enabled_ui(false, |ui| crate::small_button(ui, icon, label, false, msg!("Not in BIMCraft yet")));
}

/// Revit's Draw panel: what a line is (Model, Reference, Plane) over the gallery of what draws, Line and Point Element working.
fn draw_panel(ui: &mut Ui, ed: &mut FamilyEditor) {
    ribbon::panel(ui, msg!("Draw"), |ui| {
        crate::stacked(ui, |ui| {
            crate::stacked_button(ui, Icon::Line, msg!("Model"), true, msg!("Model: lines drawn are model lines"));
            ui.add_enabled_ui(false, |ui| {
                crate::stacked_button(ui, Icon::PickLines, msg!("Reference"), false, msg!("Not in BIMCraft yet"))
            });
            ui.add_enabled_ui(false, |ui| crate::stacked_button(ui, Icon::Level, msg!("Plane"), false, msg!("Not in BIMCraft yet")));
        });
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let line = matches!(ed.modeller.tool, Draw::Line(_));
                let tip = msg!("Line (LI): click points, or the work plane, in turn; the first again closes a profile");
                if crate::small_button(ui, Icon::Line, "Line", line, tip).clicked() {
                    ed.modeller.tool = Draw::Line(Vec::new());
                }
                later_small(ui, Icon::Rectangle, "Rectangle");
                later_small(ui, Icon::Inscribed, msg!("Inscribed Polygon"));
                later_small(ui, Icon::Circle, "Circle");
            });
            ui.horizontal(|ui| {
                later_small(ui, Icon::CentreArc, msg!("Center-ends Arc"));
                later_small(ui, Icon::Spline, "Spline");
                later_small(ui, Icon::Ellipse, "Ellipse");
                later_small(ui, Icon::FitSpline, msg!("Spline Through Points"));
            });
            ui.horizontal(|ui| {
                let tip = msg!("Point Element: click the work plane for a reference point, or a line for one hosted on it");
                if crate::small_button(ui, Icon::PointElement, msg!("Point Element"), ed.modeller.tool == Draw::Point, tip).clicked() {
                    ed.modeller.tool = Draw::Point;
                }
                later_small(ui, Icon::PickLines, msg!("Pick Lines"));
                later_small(ui, Icon::PickFaces, msg!("Pick Faces"));
            });
        });
    });
}

/// Revit's Work Plane panel: Set (a plane picked in the view, as Placement Plane's Pick), Show (the work plane drawn).
fn work_plane_panel(ui: &mut Ui, ed: &mut FamilyEditor) {
    ribbon::panel(ui, msg!("Work Plane"), |ui| {
        let tip = msg!("Set: the work plane, a plane picked in the view (a point's, the level or a reference plane)");
        if big_button(ui, Icon::PickFaces, msg!("Set"), ed.modeller.picking_plane, tip).clicked() {
            ed.modeller.picking_plane = true;
        }
        crate::stacked(ui, |ui| {
            let tip = msg!("Show: the work plane drawn, tinted, with a grid on it");
            if crate::stacked_button(ui, Icon::Visibility, msg!("Show"), ed.modeller.show_work_plane, tip).clicked() {
                ed.modeller.show_work_plane = !ed.modeller.show_work_plane;
            }
            ui.add_enabled_ui(false, |ui| crate::stacked_button(ui, Icon::View3D, msg!("Viewer"), false, msg!("Not in BIMCraft yet")));
        });
    });
}

/// Work Plane on the Create tab of a family that isn't adaptive: Set opens Revit's Work Plane dialog for the view's work plane
/// (T12: Name, Pick a plane, Pick a line, Go To View); Show tints it.
fn views_work_plane_panel(ui: &mut Ui, ed: &mut FamilyEditor) {
    ribbon::panel(ui, msg!("Work Plane"), |ui| {
        let tip = msg!("Set: the view's work plane, by name or picked in the view (the Work Plane dialog)");
        let on = ed.views.dialog.is_some() || ed.views.picking.is_some();
        if big_button(ui, Icon::PickFaces, msg!("Set"), on, tip).clicked() {
            ed.open_work_plane_dialog(super::work_plane::Target::View);
        }
        crate::stacked(ui, |ui| {
            let tip = msg!("Show: the view's work plane, tinted");
            if crate::stacked_button(ui, Icon::Visibility, msg!("Show"), ed.views.show_work_plane, tip).clicked() {
                ed.views.show_work_plane = !ed.views.show_work_plane;
            }
        });
    });
}

/// The family's Identity Data in Properties: its name, typed, and its category.
fn identity(ui: &mut Ui, name: &mut String, category: &str) {
    group(ui, msg!("Identity Data"), |ui| {
        let l = ui.label(t("Family name"));
        ui.add(TextEdit::singleline(name).desired_width(f32::INFINITY)).labelled_by(l.id);
        ui.end_row();
        ui.label(t("Category"));
        ui.label(category);
        ui.end_row();
    });
}

/// The family's Other group in Properties, as Revit's: its options (`options.rs`), edited in the draft. What the `family`
/// command would refuse of them is why the draft can't be tried, which the view says (Properties, for an adaptive family).
fn other(ui: &mut Ui, ed: &mut FamilyEditor, units: Units) {
    let category = ed.category;
    let w = (ui.available_width() / 2.0 - 16.0).max(60.0);
    group(ui, msg!("Other"), |ui| super::options::rows(ui, &mut ed.options, category, units, "family-properties-other", w));
}

/// Where Properties names what it shows, as Revit's type selector does: an icon and the name, on a box of its own.
fn selector(ui: &mut Ui, icon: Icon, what: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, pal().bar);
    p.rect_stroke(rect, 3.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    let swatch = Rect::from_min_size(rect.min + vec2(5.0, 5.0), vec2(40.0, 40.0));
    crate::icons::paint(p, icon, swatch.shrink(6.0), pal().icon);
    p.text(rect.left_center() + vec2(54.0, 0.0), Align2::LEFT_CENTER, what, FontId::proportional(14.0), pal().text);
}

/// A group of Properties as Revit's palette has one: its header, folding, over rows of a label and a value.
pub(crate) fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let key = UiId::new(("family-group", title));
    let open = ui.ctx().data(|w| w.get_temp::<bool>(key)).unwrap_or(true);
    if group_header(ui, t(title), open) {
        ui.ctx().data_mut(|w| w.insert_temp(key, !open));
    }
    if open {
        let half = (ui.available_width() / 2.0 - 12.0).max(60.0);
        ui.ctx().data_mut(|w| w.insert_temp(UiId::new("family-group-column"), half));
        Frame::new().inner_margin(Margin { left: 8, right: 4, top: 2, bottom: 4 }).show(ui, |ui| {
            Grid::new(key).num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(ui, add);
        });
    }
}

/// Revit's Visibility panel on a form's or a nested instance's contextual tab: Visibility Settings opens Family Element
/// Visibility Settings for it (F8).
fn visibility_panel(ui: &mut Ui, ed: &mut FamilyEditor, target: super::visibility::Target) {
    ribbon::panel(ui, msg!("Visibility"), |ui| {
        let tip = msg!(
            "Visibility Settings: the views it is drawn in (Plan/RCP, Front/Back, Left/Right, only when cut) and at which detail levels"
        );
        if big_button(ui, Icon::Visibility, msg!("Visibility Settings"), false, tip).clicked() {
            ed.open_visibility(target);
        }
    });
}

impl BimApp {
    /// The family's {3D} in `pane`, drawn as the project's 3D view is (`orbit.rs`): its scene and mesh made on the document's own
    /// worker, its picture on the card or the processor, and turned, panned and zoomed by the hand, the wheel, the 3D mouse, the
    /// triad and the ViewCube. A click selects the form or the nested instance under it (the connectors' symbols first, which lie
    /// over it), and on nothing lets the selection go, as in Revit and the project's 3D view; the pointer over one lights it
    /// first (T11). A double click on a form sketches it again, as Edit Extrusion or Edit Revolve does (F5).
    fn family_3d(&mut self, ui: &mut Ui, ed: &mut FamilyEditor, pane: Rect) {
        let response = ui.interact(pane, UiId::new("family-preview"), Sense::click_and_drag());
        self.canvas.track(ui, &response);
        // The triad stands left of the ViewCube, which is in the top right corner; both are laid over the view, so a click on
        // either is theirs.
        let cube_at = crate::orbit::Orbit::cube_rect(pane);
        let device = self.space_mouse.as_ref().and_then(crate::spacemouse::SpaceMouse::motion);
        let input = ed.orbit.input(ui, pane, &response, cube_at.left_top(), device);
        let cube = ui.interact(cube_at.union(crate::orbit::Orbit::home_rect(pane)), UiId::new("family-viewcube"), Sense::click());
        let base = Arc::clone(&self.session.model);
        ed.stage_3d(&base);
        let p = ui.painter_at(pane);
        let Some(model) = ed.scene3d.as_ref().map(|s| Arc::clone(&s.model)) else {
            p.rect_filled(pane, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
            return;
        };
        // On the card, unless the project's 3D view floats beside it: the card's picture is one, which that view draws in the same
        // frame.
        let card_ready = !self.floats(crate::Sheet::View3D) && self.card.ready().is_some();
        let (w, h) = input.size;
        // Fitted and not moved since, the view fits itself again as its pane settles or is resized.
        if ed.orbit.fitted.is_some_and(|at| at != (w, h)) {
            (ed.orbit.camera, ed.orbit.fitted) = (None, None);
        }
        if !ed.orbit.prepare(ui.ctx(), &model, (w, h), None, card_ready) {
            let error = ed.orbit.loading_error.clone();
            ed.orbit.wait(ui.ctx(), &p, &self.card, pane, error.as_deref().unwrap_or(t("Building 3D view…")), error.is_some());
            return;
        }
        if let Some(error) = ed.orbit.prepare_fit(ui.ctx(), &model) {
            self.status = Status { text: crate::lang::tf("Could not fit the 3D view: {}", &[&error]), error: true };
        }
        let (Some((_, scene)), Some(camera)) = (ed.orbit.scene.clone(), ed.orbit.camera) else { return };
        let fresh = ed.orbit.shows(&model);
        let cam = ed.orbit.zoom(ui.ctx(), &input, camera.sized(w, h, None), fresh.then_some(&*scene));
        // A face of the ViewCube looks at the family from its side; the house, Revit's default 3D view.
        let cam = match cube
            .clicked()
            .then(|| cube.interact_pointer_pos())
            .flatten()
            .and_then(|at| crate::orbit::Orbit::cube_click(&cam, pane, at))
        {
            Some(crate::orbit::Cube::Look(azimuth, elevation)) => bimcraft_core::view3d::Camera { azimuth, elevation, ..cam },
            Some(crate::orbit::Cube::Home) => {
                ed.orbit.home(None);
                cam
            }
            None => cam,
        };
        // While sketching, a drag from a picked line's end drags it rather than turn the view.
        let gripped = ed.sketch.as_ref().is_some_and(|s| s.sketch.drag.is_some());
        let quick = self.canvas.quick();
        let nav = ed.orbit.navigate(ui, &response, &input, cam, gripped, false, quick, fresh.then_some(&*scene));
        let cam = nav.cam;
        if cam != camera.sized(w, h, None) {
            ed.orbit.fitted = None;
        }
        let size = [w, h];
        let project = on_pane(cam, size, pane);
        let part_at = |ed: &FamilyEditor, at: egui::Pos2| {
            let id = ed.orbit.hit(&input, &cam, at)?.id;
            ed.scene3d.as_ref()?.part.get(&id).copied()
        };
        // What the pointer is over and what a click picks, in the picture on screen: the connectors' symbols first.
        let marks = ed.connector_marks();
        if ed.sketching() {
            ed.views.hover3d = None;
            ed.connector_hover = None;
        } else {
            let still = !response.dragged() && ui.input(|i| !i.pointer.any_down());
            let at = input.hover.filter(|_| still && !cube.hovered());
            ed.connector_hover = at.and_then(|p| super::connector_marks::pick(&marks, &project, p));
            ed.views.hover3d = at.filter(|_| ed.connector_hover.is_none()).map(|p| (p, cam, part_at(ed, p)));
            let click = response.clicked().then(|| response.interact_pointer_pos()).flatten().or(self.canvas.quick_press());
            if let Some(p) = click.filter(|p| !crate::orbit::Orbit::on_cube(pane, *p)) {
                match super::connector_marks::pick(&marks, &project, p) {
                    Some(i) => ed.select_connector(i),
                    None => ed.click_3d(part_at(ed, p)),
                }
            }
            if response.double_clicked()
                && let Some(p) = response.interact_pointer_pos()
                && let Some(bimcraft_core::family::views::Part::Form(i)) = part_at(ed, p)
            {
                ed.select_form_at(i);
                if let Err(e) = ed.edit_form_sketch(i) {
                    self.status = Status { text: e, error: true };
                }
            }
        }
        // The picture: what is selected and under the pointer tinted, the work plane and a datum selected laid over it.
        let tints = ed.scene3d.as_ref().map(|s| s.tints(&ed.lit_3d())).unwrap_or_default();
        let plane_marks = ed.marks_3d(&cam);
        let mesh = match ed.orbit.card_mesh(ui.ctx(), &mut self.card, card_ready) {
            crate::orbit::OnCard::Loading => {
                ed.orbit.wait(ui.ctx(), &p, &self.card, pane, t("Loading 3D view onto the card…"), false);
                return;
            }
            crate::orbit::OnCard::Ready(mesh, failed) => {
                if let Some(status) = failed {
                    self.status = status;
                }
                mesh
            }
        };
        let shot = crate::orbit::Shown { model: &model, scene: &scene, cam, tints: &tints, marks: &plane_marks, dragged: None, cut: None };
        if let Some(status) = ed.orbit.paint(ui.ctx(), &p, &mut self.card, mesh.as_ref(), shot) {
            self.status = status;
        }
        // The connectors over it, at their places in the family's axes.
        let lit = (crate::style::canvas_pal().accent, crate::style::canvas_pal().hover);
        super::connector_marks::paint(&p, &marks, &project, ed.selected_connector(), ed.connector_hover, lit);
        // The insertion point, where the planes that define the origin cross (O1).
        if let Some(at) = ed.origin_mark() {
            super::origin_mark::paint_3d(&p, &project, at);
        }
        // A form being sketched on its plane, over the picture (F5), with its padlocks.
        if let Some(e) = ed.sketch_in_3d(ui, &response, &p, &cam, size, pane) {
            self.status = Status { text: e, error: true };
        }
        crate::orbit::Orbit::draw_cube(&p, &cam, pane, input.hover.or_else(|| cube.hover_pos()));
        ed.orbit.triad_draw(&p, pane, cube_at.left_top(), &cam);
    }
}
