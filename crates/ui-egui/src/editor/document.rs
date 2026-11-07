//! An adaptive family edited as Revit edits a family: a document that takes the whole window. Its own ribbon (Create and Modify,
//! and a contextual tab such as "Modify | Reference Points" for what is selected or the tool in hand), Properties on the left
//! with Revit's groups, a Project Browser of the family's views, the modelling view in the middle with its tab and view control
//! bar, and the Options Bar over it. What the ribbon asks for (Load into Project, closing it) goes through the same code the
//! Family Editor dialog's buttons do (`family_asked`); Family Types and Family Category and Parameters are dialogs, as Revit's.
use super::*;
use crate::adaptive_view::{self, Draw};
use crate::dock::Palette;
use crate::icons::Icon;
use crate::properties::group_header;
use crate::ribbon;
use crate::{big_button, icon_button, tab_button};
use bimcraft_core::adaptive::{Evaluated, Plane, PointType, WorkPlane};
use egui::{Align2, FontId, Frame, Margin, UiBuilder};

/// The contextual tab for what the family document has in hand: the tool, else what is selected, as Revit names them.
fn context(ed: &FamilyEditor) -> Option<&'static str> {
    let m = &ed.modeller;
    match m.tool {
        Draw::Point => return Some(msg!("Modify | Place Reference Point")),
        Draw::Line(_) => return Some(msg!("Modify | Place Lines")),
        Draw::Modify | Draw::Move { .. } | Draw::Rotate { .. } => {}
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
    /// Whether a family document has the window: an adaptive family open in the Family Editor.
    pub(crate) fn family_doc(&self) -> bool {
        self.family_editor.as_ref().is_some_and(FamilyEditor::document)
    }

    /// The title the window and its title bar give a family document: the family and its view.
    pub(crate) fn family_title(&self) -> Option<String> {
        let ed = self.family_editor.as_ref().filter(|e| e.document())?;
        Some(format!("{} · {}", ed.title(), tf("3D View: {}", &[VIEW])))
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
        if matches!(tab, DocTab::Create | DocTab::Modify | DocTab::Context) {
            self.select_and_properties(ui, ed);
        }
        match tab {
            DocTab::Create => {
                draw_panel(ui, ed);
                work_plane_panel(ui, ed);
                ribbon::panel(ui, msg!("Model"), |ui| {
                    later(ui, Icon::Component, "Component");
                    later(ui, Icon::Text, msg!("Model Text"));
                });
                ribbon::panel(ui, msg!("Datum"), |ui| {
                    later(ui, Icon::Level, "Level");
                    later(ui, Icon::Line, msg!("Reference Plane"));
                });
                self.form_panel(ui, ed, &lines);
                self.test_placement_panel(ui, ed);
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
                    if ui.add_enabled_ui(some, |ui| big_button(ui, Icon::Delete, "Delete", false, tip)).inner.clicked()
                        && let Some(d) = ed.adaptive.as_mut()
                    {
                        adaptive_view::delete(d, &mut ed.modeller);
                    }
                });
                ribbon::panel(ui, msg!("Measure"), |ui| {
                    later(ui, Icon::Measure, msg!("Measure"));
                });
                if tab == DocTab::Context {
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
                                adaptive_view::make_adaptive(d, &mut ed.modeller, &points);
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
            if big_button(ui, Icon::Modify, "Modify", ed.modeller.tool == Draw::Modify, tip).clicked() {
                ed.modeller.tool = Draw::Modify;
                ed.modeller.picking_plane = false;
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
                if !matches!(ed.tab, Tab::Parameters | Tab::Types) {
                    ed.tab = Tab::Parameters;
                }
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

    /// The family document's Properties palette: what is selected in its view, with Revit's groups, or the family itself.
    pub(crate) fn family_properties(&mut self, ui: &mut Ui) {
        let Some(mut ed) = self.family_editor.take() else { return };
        let values = ed.values();
        let category = category_label(ed.category);
        let selected = ed.modeller.selection.clone();
        let params = ed.parameter_kinds();
        if let Some(d) = ed.adaptive.as_mut() {
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
                group(ui, msg!("Identity Data"), |ui| {
                    let l = ui.label(t("Family name"));
                    ui.add(TextEdit::singleline(&mut ed.name).desired_width(f32::INFINITY)).labelled_by(l.id);
                    ui.end_row();
                    ui.label(t("Category"));
                    ui.label(category.clone());
                    ui.end_row();
                });
                if let Some(id) = ed.replay {
                    ui.label(
                        RichText::new(tf("Replaying instance {} with its exact parameters and placement", &[&id.to_string()]))
                            .color(pal().muted),
                    );
                }
            }
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

    /// The family document's Project Browser: its views, as a Generic Model Adaptive template has them.
    pub(crate) fn family_browser(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().id_salt("family-browser").auto_shrink([false, false]).show(ui, |ui| {
            Frame::new().inner_margin(8.0).show(ui, |ui| {
                egui::CollapsingHeader::new(t("Views (all)")).id_salt("family-views").default_open(true).show(ui, |ui| {
                    let groups: [(&str, &[&str]); 4] = [
                        (msg!("Floor Plans"), &["Ref. Level"]),
                        (msg!("Ceiling Plans"), &["Ref. Level"]),
                        (msg!("3D Views"), &[VIEW]),
                        (msg!("Elevations"), &["Back", "Front", "Left", "Right"]),
                    ];
                    for (group, views) in groups {
                        egui::CollapsingHeader::new(t(group)).id_salt(("family-views", group)).default_open(true).show(ui, |ui| {
                            for view in views {
                                let on = group == msg!("3D Views");
                                if ui.selectable_label(on, t(view)).clicked() && !on {
                                    self.status = Status {
                                        text: t("A family's plans and elevations come with its work planes").into(),
                                        error: false,
                                    };
                                }
                            }
                        });
                    }
                });
            });
        });
    }

    /// The family document's view: the modelling view filling the pane, its tab over its top left corner with the family's
    /// close button, and its view control bar at the bottom.
    pub(crate) fn family_central(&mut self, ui: &mut Ui) {
        let Some(mut ed) = self.family_editor.take() else { return };
        let pane = ui.available_rect_before_wrap();
        self.view_rect = pane;
        self.pane = pane;
        let values = ed.values();
        if let Some(d) = ed.adaptive.as_mut() {
            ui.scope_builder(UiBuilder::new().max_rect(pane), |ui| {
                let planes = bimcraft_core::family::Planes { planes: &ed.planes, lines: &ed.lines };
                adaptive_view::show(ui, d, planes, &mut ed.modeller, &values, pane.size());
            });
            // What a tool in the view could not do, in the status bar.
            if let Some(e) = ui.ctx().data_mut(|w| w.remove_temp::<String>(UiId::new("adaptive-view-problem"))) {
                self.status = Status { text: e, error: true };
            }
        }
        let card = crate::style::card([true, false, true, false]).inner_margin(Margin::same(2));
        let mut over = ui.new_child(UiBuilder::new().id_salt("family-view-tabs").max_rect(pane));
        let foot = card.show(&mut over, |ui| {
            ui.horizontal(|ui| {
                crate::tab_button(ui, &format!("{} · {VIEW}", ed.title()), true, false);
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
                }
            });
        });
        self.work = pane.with_min_y(pane.top().max(self.tabs_foot));
        self.family_editor = Some(ed);
    }

    /// The family document's Options Bar: the contextual tab's name; drawing, Revit's Placement Plane; with lines selected, the
    /// depth Create Form gives a solid or a void.
    pub(crate) fn family_options(&mut self, ui: &mut Ui) -> bool {
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
        });
        true
    }

    /// Whether the family document's Options Bar has anything to show: only with a contextual tab, as Revit's.
    pub(crate) fn family_has_options(&self) -> bool {
        self.family_editor.as_ref().is_some_and(|ed| context(ed).is_some())
    }

    /// What the family document's status bar asks for.
    pub(crate) fn family_prompt(&self) -> Option<&'static str> {
        let ed = self.family_editor.as_ref().filter(|e| e.document())?;
        Some(adaptive_view::prompt(&ed.modeller))
    }

    /// Undo in a family document: one step back through its edits.
    pub(crate) fn family_undo(&mut self) {
        if let Some(ed) = self.family_editor.as_mut()
            && let Some(d) = ed.adaptive.as_mut()
        {
            adaptive_view::undo(d, &mut ed.modeller);
        }
    }

    /// The dialogs a family document's ribbon opens: Family Types (its parameters and types, on two tabs) and Family
    /// Category and Parameters (its name, category and, for a door or a window, how it opens).
    pub(crate) fn document_dialogs(&mut self, ctx: &Context, ed: &mut FamilyEditor, materials: &[String]) {
        // Associate Family Parameter, from a row's "=": a parameter New Parameter makes goes in the family at once, as in Revit.
        ed.associate_dialog(ctx);
        let Some(dialog) = ed.dialog else { return };
        let title = match dialog {
            DocDialog::Types => t("Family Types"),
            DocDialog::Category => t("Family Category and Parameters"),
        };
        let mut done = false;
        let r = Modal::new(UiId::new(("family-dialog", dialog as u8)), title).width(900.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| match dialog {
                DocDialog::Types => {
                    ui.horizontal(|ui| {
                        for (tab, label) in [(Tab::Parameters, msg!("Parameters")), (Tab::Types, msg!("Types"))] {
                            if ui.selectable_label(ed.tab == tab, t(label)).clicked() {
                                ed.tab = tab;
                            }
                        }
                    });
                    ui.separator();
                    let room = (ctx.content_rect().height() - 260.0).clamp(160.0, 560.0);
                    ScrollArea::vertical().max_height(room).auto_shrink([false, true]).show(ui, |ui| match ed.tab {
                        Tab::Types => types_tab(ed, ui, materials),
                        _ => parameters_tab(ed, ui, materials),
                    });
                }
                DocDialog::Category => {
                    Grid::new("family-category").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                        let l = ui.label(t("Family name"));
                        ui.add_sized([260.0, ui.spacing().interact_size.y], TextEdit::singleline(&mut ed.name)).labelled_by(l.id);
                        ui.end_row();
                        let l = ui.label(t("Category"));
                        let c = ComboBox::from_id_salt("family-category-list")
                            .selected_text(category_label(ed.category))
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                for c in CATEGORIES {
                                    if ui.selectable_label(ed.category == c, category_label(c)).clicked() {
                                        ed.category = c;
                                    }
                                }
                            });
                        c.response.labelled_by(l.id);
                        ui.end_row();
                    });
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    done = ui.button(t("OK")).clicked();
                });
            });
        });
        if done || r.should_close() {
            ed.dialog = None;
        }
    }
}

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
