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

/// What the family document keeps of its own: its history and its preview's size.
pub(crate) struct State {
    /// The 3D preview's size, in pixels: the view's.
    pub(super) preview_size: [u32; 2],
    /// The draft before each step, newest last, and the steps undone.
    undo: Vec<Draft>,
    redo: Vec<Draft>,
    /// The draft as the last step left it: what an edit still going on (a drag, a field being typed in, a dialog) began from.
    mark: Option<Draft>,
    /// The draft when the dialog open was, which its Cancel goes back to.
    dialog_saved: Option<Draft>,
    /// A form picked in the Project Browser, which Properties opens.
    pub(super) reveal: Option<usize>,
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
    /// The shared families nested in this one that the project already has, defined otherwise, which "Shared Family Already
    /// Loaded" asks about on loading (`exists`). T6 (nested families) works them out; until then only a test puts them here.
    pub(super) shared_nested: Vec<String>,
    /// Why Family Category and Parameters' Apply or OK was refused, while the draft is as it was then.
    options_refused: Option<(Draft, String)>,
    /// What Family Category and Parameters' category list is filtered by, and whether the list has been shown since it opened.
    category_filter: String,
    category_shown: bool,
}

impl Default for State {
    fn default() -> State {
        State {
            preview_size: [360, 300],
            undo: Vec::new(),
            redo: Vec::new(),
            mark: None,
            dialog_saved: None,
            reveal: None,
            refused: None,
            clean: None,
            preview_visibility: false,
            temp: Default::default(),
            loaded_to_close: false,
            shared_nested: Vec::new(),
            options_refused: None,
            category_filter: String::new(),
            category_shown: false,
        }
    }
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
    adaptive: Option<Definition>,
    picked: usize,
    /// Its reference planes: drawn and moved in its plans and elevations.
    planes: Vec<RefPlane>,
    /// Its options: Family Category and Parameters' table, Properties' Other group.
    options: super::options::OptionsText,
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
            adaptive: ed.adaptive.clone(),
            picked: ed.picked,
            planes: ed.planes.clone(),
            options: ed.options.clone(),
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
        ed.options = self.options;
        // What is selected and no longer there goes from the selection.
        let nodes = ed.adaptive.as_ref().map(|d| d.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>()).unwrap_or_default();
        ed.modeller.selection.retain(|id| nodes.contains(id));
        ed.problem = None;
    }
}

impl FamilyEditor {
    /// The draft as it is now, for `saved_as` once it has been saved.
    pub(super) fn draft(&self) -> Draft {
        Draft::of(self)
    }

    /// The draft `d` was saved or loaded into the project: changes are counted from it.
    pub(super) fn saved_as(&mut self, d: Draft) {
        self.document.clean = Some(d);
    }

    /// The shared nested families the project already has otherwise, which loading asks about: T6's to work out, and until
    /// then a test's to inject.
    #[cfg(test)]
    pub(crate) fn set_shared_nested(&mut self, names: Vec<String>) {
        self.document.shared_nested = names;
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
        self.dialog.is_some() || self.associating.is_some()
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
        if ed.editing_2d() && ed.views.drawing.is_some() {
            return Some(msg!("Modify | Place Reference Plane"));
        }
        if ed.selected_plane().is_some() {
            return Some(msg!("Modify | Reference Planes"));
        }
        return ed.selected_form().map(|_| msg!("Modify | Form"));
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

/// What a family document's view tab and title call its view.
pub(crate) const VIEW: &str = "{3D}";

impl BimApp {
    /// Whether a family document has the window: a family open in the Family Editor.
    pub(crate) fn family_doc(&self) -> bool {
        self.family_editor.is_some()
    }

    /// The title the window and its title bar give a family document: the family and its view.
    pub(crate) fn family_title(&self) -> Option<String> {
        let ed = self.family_editor.as_ref()?;
        Some(format!("{} · {}", ed.title(), super::views::view_title(ed.views.shown)))
    }

    /// The family document's ribbon: its tabs, then the active tab's panels, folding to fit as the project's do.
    pub(crate) fn family_ribbon(&mut self, ui: &mut Ui) {
        let Some(mut ed) = self.family_editor.take() else { return };
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
        self.family_editor = Some(ed);
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
                for label in [msg!("Extrusion"), msg!("Blend"), msg!("Revolve"), msg!("Sweep"), msg!("Swept Blend"), msg!("Void Forms")] {
                    later(ui, Icon::CreateForm, label);
                }
            }),
            _ => {}
        }
        match tab {
            DocTab::Create => {
                ribbon::panel(ui, msg!("Model"), |ui| {
                    later(ui, Icon::Component, "Component");
                    later(ui, Icon::Text, msg!("Model Text"));
                });
                ribbon::panel(ui, msg!("Datum"), |ui| {
                    later(ui, Icon::Level, "Level");
                    // Drawn in a plan or an elevation of a family that isn't adaptive, as Revit's (F4).
                    let tip = msg!("Reference Plane (RP): a plane drawn by two clicks in a plan or an elevation");
                    let on = ed.views.drawing.is_some();
                    if ui.add_enabled_ui(ed.editing_2d(), |ui| big_button(ui, Icon::Line, msg!("Reference Plane"), on, tip)).inner.clicked()
                    {
                        ed.reference_plane_tool();
                    }
                });
                // In {3D} too, where Show draws it over the preview.
                if !adaptive {
                    views_work_plane_panel(ui, ed);
                }
                if adaptive {
                    self.form_panel(ui, ed, &lines);
                    self.test_placement_panel(ui, ed);
                }
            }
            DocTab::Insert => {
                ribbon::panel(ui, msg!("Family"), |ui| {
                    if big_button(ui, Icon::LoadFamily, msg!("Load Family"), false, msg!("Load Family: open a family file here")).clicked()
                    {
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
            }
            DocTab::Modify | DocTab::Context => {
                if tab == DocTab::Context && drawing {
                    draw_panel(ui, ed);
                    work_plane_panel(ui, ed);
                }
                ribbon::panel(ui, msg!("Clipboard"), |ui| {
                    later(ui, Icon::Copy, msg!("Paste"));
                });
                let form = ed.selected_form();
                ribbon::panel(ui, msg!("Modify"), |ui| {
                    let some = !ed.modeller.selection.is_empty();
                    for (icon, label) in [(Icon::Align, "Align"), (Icon::Offset, "Offset"), (Icon::MirrorPick, msg!("Mirror - Pick Axis"))]
                    {
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
                    let plane = ed.selected_plane().filter(|g| ed.planes.iter().any(|p| p.guid == *g && !p.datum()));
                    if ui
                        .add_enabled_ui(some || form.is_some() || plane.is_some(), |ui| big_button(ui, Icon::Delete, "Delete", false, tip))
                        .inner
                        .clicked()
                    {
                        if let Some(g) = plane {
                            if let Err(e) = ed.delete_plane(g) {
                                self.status = Status { text: e, error: true };
                            }
                        } else if let Some(i) = form {
                            ed.forms.remove(i);
                            ed.form = None;
                        } else if let Some(d) = ed.adaptive.as_mut() {
                            adaptive_view::delete(d, &mut ed.modeller);
                        }
                    }
                });
                ribbon::panel(ui, msg!("Measure"), |ui| {
                    later(ui, Icon::Measure, msg!("Measure"));
                });
                if tab == DocTab::Context {
                    if let Some(i) = ed.selected_form() {
                        self.form_work_plane_panel(ui, ed, i);
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
        ribbon::panel(ui, msg!("Family Editor"), |ui| {
            let tip = msg!("Load into Project: the project takes the family as it is here, and the family stays open");
            if big_button(ui, Icon::LoadIntoProject, msg!("Load into Project"), false, tip).clicked() {
                ed.ask.apply = true;
            }
            let tip = msg!("Load into Project and Close: the project takes the family, and the family closes");
            if big_button(ui, Icon::LoadIntoProjectClose, msg!("Load into Project and Close"), false, tip).clicked() {
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
                ed.views.drawing = None;
                ed.views.picking_plane = false;
                ed.views.plane = None;
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
        let Some(mut ed) = self.family_editor.take() else { return };
        let values = ed.values();
        let category = category_label(ed.category);
        let selected = ed.modeller.selection.clone();
        let params = ed.parameter_kinds();
        // A reference plane selected in a plan or an elevation: its name.
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
            }
            self.family_editor = Some(ed);
            return;
        }
        // A form selected: its own Properties, as Revit's show only what is selected.
        if let Some(i) = ed.selected_form() {
            selector(ui, Icon::CreateForm, &ed.forms[i].describe());
            ui.add_space(6.0);
            self.form_properties(ui, &mut ed, i);
            self.family_editor = Some(ed);
            return;
        }
        if ed.adaptive.is_none() {
            let materials: Vec<String> = self.session.model.materials.keys().cloned().collect();
            selector(ui, Icon::FamilyEditor, &tf("Family: {}", &[&category]));
            ui.add_space(6.0);
            identity(ui, &mut ed.name, &category);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (tab, label) in [(Tab::Forms, msg!("Forms")), (Tab::Symbols, msg!("Symbols")), (Tab::Adaptive, msg!("Adaptive"))] {
                    if ui.selectable_label(ed.tab == tab, t(label)).clicked() {
                        ed.tab = tab;
                    }
                }
            });
            ui.separator();
            match ed.tab {
                Tab::Forms => forms_tab(&mut ed, ui, &materials),
                Tab::Symbols => symbols_tab(&mut ed, ui),
                Tab::Adaptive => {
                    if let Some(a) = crate::adaptive_editor::show(ui, &mut ed.adaptive, &values, &params) {
                        ed.associating = Some(a);
                    }
                }
            }
            // Last, as Revit's palette lists Other.
            ui.add_space(6.0);
            other(ui, &mut ed, self.session.model.units);
            self.family_editor = Some(ed);
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
        self.family_editor = Some(ed);
    }

    /// The family document's Project Browser: its views, as a family template has them, and the forms of a family that isn't
    /// adaptive, each opening its Properties.
    pub(crate) fn family_browser(&mut self, ui: &mut Ui) {
        let picked = self.family_editor.as_ref().and_then(FamilyEditor::selected_form);
        let forms: Vec<String> = match &self.family_editor {
            Some(ed) if ed.adaptive.is_none() => ed.forms.iter().enumerate().map(|(i, f)| format!("{}. {}", i + 1, f.describe())).collect(),
            _ => Vec::new(),
        };
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
                    let shown = self.family_editor.as_ref().and_then(|ed| ed.views.shown);
                    for (group, views) in groups {
                        egui::CollapsingHeader::new(t(group)).id_salt(("family-views", group)).default_open(true).show(ui, |ui| {
                            for (which, view) in views {
                                if ui.selectable_label(shown == which, t(view)).clicked()
                                    && let Some(ed) = self.family_editor.as_mut()
                                {
                                    ed.views.show(which);
                                }
                            }
                        });
                    }
                });
                if !forms.is_empty() {
                    egui::CollapsingHeader::new(t("Forms")).id_salt("family-forms").default_open(true).show(ui, |ui| {
                        for (i, form) in forms.iter().enumerate() {
                            if ui.selectable_label(picked == Some(i), form).clicked()
                                && let Some(ed) = self.family_editor.as_mut()
                            {
                                // Selected, as Revit's browser selects what it lists; its own row in the Forms tab opens for
                                // when it is let go.
                                ed.tab = Tab::Forms;
                                ed.document.reveal = Some(i);
                                ed.form = Some(i);
                            }
                        }
                    });
                }
            });
        });
    }

    /// The family document's view: the modelling view (an adaptive family's) or the 3D preview (another's, turned by a drag, or
    /// as Revit's by Shift and the middle button) filling the pane, its tab over its top left corner with the family's close
    /// button, and its view control bar at the bottom.
    pub(crate) fn family_central(&mut self, ui: &mut Ui) {
        let Some(mut ed) = self.family_editor.take() else { return };
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
            let response = ui.interact(pane, UiId::new("family-preview"), Sense::click_and_drag());
            // A click in the view lets the selected form go, as a click on nothing does in Revit.
            if response.clicked() {
                ed.form = None;
            }
            ed.document.preview_size = [pane.width().clamp(64.0, 1600.0) as u32, pane.height().clamp(64.0, 1200.0) as u32];
            let orbit = response.dragged_by(PointerButton::Primary)
                || (response.dragged_by(PointerButton::Middle) && ui.input(|i| i.modifiers.shift));
            if orbit {
                let d = response.drag_delta();
                ed.look = (ed.look.0 - f64::from(d.x) * 0.5, (ed.look.1 + f64::from(d.y) * 0.5).clamp(-89.0, 89.0));
            }
            let base = Arc::clone(&self.session.model);
            ed.refresh(ui.ctx(), &base);
            if let Some(p) = &ed.preview {
                ui.painter().image(p.tile.id(), pane, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        let card = crate::style::card([true, false, true, false]).inner_margin(Margin::same(2));
        let mut over = ui.new_child(UiBuilder::new().id_salt("family-view-tabs").max_rect(pane));
        let foot = card.show(&mut over, |ui| {
            ui.horizontal(|ui| {
                crate::tab_button(ui, &format!("{} · {}", ed.title(), super::views::view_tab(ed.views.shown)), true, false);
                if icon_button(ui, Icon::Close, msg!("Close Family")).clicked() {
                    ed.ask.cancel = true;
                }
            });
        });
        self.tabs_foot = foot.response.rect.bottom();
        // The view control bar, at the view's bottom left.
        let mut bar = ui.new_child(UiBuilder::new().id_salt("family-view-bar").max_rect(pane).layout(Layout::bottom_up(Align::Min)));
        crate::style::card([true, true, true, true]).inner_margin(Margin::symmetric(6, 2)).show(&mut bar, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Shaded")).color(pal().muted));
                if icon_button(ui, Icon::Fit, "Zoom to Fit").clicked() {
                    ed.modeller.camera = None;
                    ed.look = (AZIMUTH, ELEVATION);
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
        self.family_editor = Some(ed);
    }

    /// The family document's Options Bar: the contextual tab's name; drawing, Revit's Placement Plane; with lines selected, the
    /// depth Create Form gives a solid or a void.
    pub(crate) fn family_options(&mut self, ui: &mut Ui) -> bool {
        let units = self.session.model.units;
        let Some(ed) = self.family_editor.as_mut() else { return false };
        let Some(name) = context(ed) else { return false };
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
            // A selected extrusion's Depth, as Revit's Options Bar has it.
            if let Some(i) = ed.selected_form()
                && let Some(f) = ed.forms.get_mut(i)
                && !f.revolve
                && !f.mesh()
            {
                ui.separator();
                super::extrusion::depth(ui, units, f, &values);
            }
        });
        true
    }

    /// Whether the family document's Options Bar has anything to show: only with a contextual tab, as Revit's.
    pub(crate) fn family_has_options(&self) -> bool {
        self.family_editor.as_ref().is_some_and(|ed| context(ed).is_some())
    }

    /// What the family document's status bar asks for: an adaptive family's modelling view's tool, or how another's preview turns.
    pub(crate) fn family_prompt(&self) -> Option<&'static str> {
        let ed = self.family_editor.as_ref()?;
        Some(match (ed.views.shown, ed.adaptive.is_some()) {
            (Some(_), true) => msg!("An adaptive family's plans and elevations show its test placement; model it in {3D}."),
            (Some(_), false) if ed.views.picking_plane => msg!("Click a reference plane or a datum: the view's work plane."),
            (Some(_), false) if ed.views.drawing.is_some() => msg!("Click two points: a reference plane through them."),
            (Some(_), false) => msg!("Click a form or a reference plane to select it; drag a selected plane to move it."),
            (None, true) => adaptive_view::prompt(&ed.modeller),
            (None, false) => msg!("Drag the picture to turn it."),
        })
    }

    /// Undo in a family document: one step back through its own history, done when the frame ends; the project's is not touched.
    pub(crate) fn family_undo(&mut self) {
        if let Some(ed) = self.family_editor.as_mut() {
            ed.ask.undo = true;
        }
    }

    /// Redo in a family document: a step undone done again.
    pub(crate) fn family_redo(&mut self) {
        if let Some(ed) = self.family_editor.as_mut() {
            ed.ask.redo = true;
        }
    }

    /// The family document's keys: Ctrl+Z undoes a step of its own, Ctrl+Y and Ctrl+Shift+Z redo one, and the two letters of
    /// Temporary Hide/Isolate (`temp_hide.rs`), unless a field being typed in (whose own they are) or a dialog has them.
    pub(crate) fn family_keys(&mut self, ctx: &Context) {
        let Some(ed) = self.family_editor.as_ref() else { return };
        if ed.modal() || ctx.text_edit_focused() {
            return;
        }
        // Letters typed, but not while a menu has the keys.
        if !egui::Popup::is_any_open(ctx) {
            self.family_typed(ctx);
        }
        let Some(ed) = self.family_editor.as_mut() else { return };
        // Escape lets a selected form go, though a button clicked has the keyboard (the window's own Escape does it otherwise).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // The plane being drawn first, then the tool, then the selection.
            if ed.views.picking_plane {
                ed.views.picking_plane = false;
            } else if let Some(first) = ed.views.drawing {
                ed.views.drawing = first.map(|_| None);
            } else if ed.views.plane.take().is_none() {
                ed.let_form_go();
            }
        }
        let pressed = |m: egui::Modifiers, k: egui::Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z) || pressed(egui::Modifiers::COMMAND, egui::Key::Y) {
            ed.ask.redo = true;
        } else if pressed(egui::Modifiers::COMMAND, egui::Key::Z) {
            ed.ask.undo = true;
        }
    }

    /// The dialogs a family document's ribbon opens: Family Types (its types and one table of its parameters: `types_dialog`) and Family
    /// Category and Parameters (its name, category, options (`options.rs`) and, for a door or a window, how it opens). Apply keeps what is typed,
    /// Cancel puts back what was there when the dialog opened or last applied, OK keeps it and closes the dialog.
    pub(crate) fn document_dialogs(&mut self, ctx: &Context, ed: &mut FamilyEditor, materials: &[String]) {
        // Associate Family Parameter, from a row's "=": a parameter New Parameter makes goes in the family at once, as in Revit.
        ed.associate_dialog(ctx);
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
                for op in Operation::ALL {
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

/// Work Plane in a plan or an elevation of a family that isn't adaptive (F4): Set picks the view's work plane from the
/// family's planes, as A4's list names them, or by a click on one (Pick); Show tints it. Revit's Work Plane dialog is T12's.
fn views_work_plane_panel(ui: &mut Ui, ed: &mut FamilyEditor) {
    let listed = ed.plane_set().listed();
    let now = ed.views.work_plane();
    let mut chosen = None;
    ribbon::panel(ui, msg!("Work Plane"), |ui| {
        let tip = msg!("Set: the view's work plane, a plane picked in the view or from the list");
        let r = big_button(ui, Icon::PickFaces, msg!("Set"), ed.views.picking_plane, tip);
        // Pick takes a plane clicked in a plan or an elevation; {3D} offers the list alone.
        let pickable = ed.views.shown.is_some();
        ribbon::choices(ui, &r, false, |ui| {
            if pickable && ui.button(t(PICK)).clicked() {
                ed.views.picking_plane = true;
                ui.close();
            }
            for (plane, label) in &listed {
                if ui.selectable_label(now == *plane, t(label)).clicked() {
                    chosen = Some(*plane);
                    ui.close();
                }
            }
        });
        crate::stacked(ui, |ui| {
            let tip = msg!("Show: the view's work plane, tinted");
            if crate::stacked_button(ui, Icon::Visibility, msg!("Show"), ed.views.show_work_plane, tip).clicked() {
                ed.views.show_work_plane = !ed.views.show_work_plane;
            }
        });
    });
    if let Some(p) = chosen {
        ed.views.set_work_plane(p);
        ed.views.picking_plane = false;
    }
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
