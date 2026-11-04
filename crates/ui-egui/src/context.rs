//! The right-click menu every view has (plans, ceiling plans, sections, elevations, the 3D view,
//! sheets and schedules), as Revit's: what the open view and the selection can do. A menu is groups
//! of [`Item`]s, drawn with separators between them; [`BimApp::context_items`] says which items the
//! moment offers, and [`BimApp::act`] does what one says. A new item is an [`Action`] variant, its
//! label (a row in `lang::WORDS`), the group it joins in `context_items`, and its arm in `act`.

use bimcraft_i18n::msg;

use egui::{Button, Response};

use crate::lang::t;
use crate::{BimApp, Sheet, Tool};

pub(crate) struct WireConnectionDialog {
    wire: bimcraft_core::model::Guid,
    device: bimcraft_core::model::Guid,
    end: u8,
    carrier: Option<bimcraft_core::model::Guid>,
    wall: Option<bimcraft_core::model::Guid>,
    frozen_surface: bool,
    exterior: bool,
    bend_radius: String,
    bend_radius_edited: bool,
    point: [String; 3],
    in_carrier: bool,
}

pub(crate) struct CarrierAttachmentDialog {
    carrier: bimcraft_core::model::Guid,
    wires: Vec<bimcraft_core::model::Guid>,
    selected: Option<bimcraft_core::model::Guid>,
}

pub(crate) struct CarrierFromWireDialog {
    wire: bimcraft_core::model::Guid,
    type_name: String,
    placement: &'static str,
    lateral: String,
}

/// What an item of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    MountToSurface,
    /// Esc: the tool's points, else the tool, else the selection.
    Cancel,
    /// Enter: a spline or a polyline made of the points clicked so far.
    Finish,
    ZoomToFit,
    ZoomIn,
    ZoomOut,
    /// The 3D view's default orientation.
    HomeView,
    EditType,
    SelectAllInstances,
    HideElement,
    HideCategory,
    OverrideGraphics,
    CreateGroup,
    Ungroup,
    Delete,
    /// The linked CAD drawings selected, read again from their files.
    ReloadCad,
    CreateCarrierFromWire,
    ConnectEquipmentToWire,
    AttachExistingWire,
}

impl Action {
    /// Its label, in English: the menu translates it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Action::Cancel => "Cancel",
            Action::Finish => msg!("Finish Drawing"),
            Action::ZoomToFit => "Zoom to Fit",
            Action::ZoomIn => "Zoom In",
            Action::ZoomOut => "Zoom Out",
            Action::HomeView => "Home View",
            Action::EditType => "Edit Type",
            Action::SelectAllInstances => msg!("Select All Instances"),
            Action::HideElement => "Hide Element",
            Action::HideCategory => "Hide Category",
            Action::OverrideGraphics => "Override Graphics",
            Action::CreateGroup => msg!("Create Group"),
            Action::Ungroup => msg!("Ungroup"),
            Action::Delete => "Delete",
            Action::ReloadCad => msg!("Reload CAD"),
            Action::MountToSurface => msg!("Mount to Surface"),
            Action::CreateCarrierFromWire => msg!("Create Carrier from Wire"),
            Action::ConnectEquipmentToWire => msg!("Connect Equipment to Wire"),
            Action::AttachExistingWire => msg!("Attach Existing Wire"),
        }
    }

    /// The keys that do the same, shown beside it.
    pub(crate) fn shortcut(self) -> Option<&'static str> {
        match self {
            Action::Cancel => Some("Esc"),
            Action::Finish => Some("Enter"),
            Action::ZoomToFit => Some("ZF"),
            Action::SelectAllInstances => Some("SA"),
            Action::HideElement => Some("EH"),
            Action::HideCategory => Some("VH"),
            Action::CreateGroup => Some("GP"),
            Action::Ungroup => Some("UG"),
            Action::Delete => Some("Del"),
            _ => None,
        }
    }
}

/// An item of the menu: what it does, and whether it can now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub action: Action,
    pub enabled: bool,
}

impl Item {
    fn new(action: Action, enabled: bool) -> Item {
        Item { action, enabled }
    }
}

/// The menu of `groups` on `response`'s right click; the item clicked goes to `chosen`. Free of the
/// app, so a view drawing in a closure that holds it can still offer the menu.
pub(crate) fn menu(response: &Response, groups: &[Vec<Item>], chosen: &mut Option<Action>) {
    response.context_menu(|ui| {
        for (i, group) in groups.iter().filter(|g| !g.is_empty()).enumerate() {
            if i > 0 {
                ui.separator();
            }
            for item in group {
                let mut button = Button::new(t(item.action.label()));
                if let Some(keys) = item.action.shortcut() {
                    button = button.shortcut_text(keys);
                }
                if ui.add_enabled(item.enabled, button).clicked() {
                    *chosen = Some(item.action);
                    ui.close();
                }
            }
        }
    });
}

impl BimApp {
    /// What the menu offers now, in groups: cancelling; what the selection can do; what the view
    /// can (zooming, but not in a schedule).
    pub(crate) fn context_items(&self) -> Vec<Vec<Item>> {
        let some = !self.selection.is_empty();
        let mut first = vec![Item::new(Action::Cancel, some || self.tool != Tool::Modify)];
        if self.finishable() {
            first.insert(0, Item::new(Action::Finish, true));
        }
        let mut groups = vec![first];
        if self.wire_and_equipment().is_some() {
            groups.push(vec![Item::new(Action::ConnectEquipmentToWire, true)]);
        }
        if some {
            groups.push(vec![
                Item::new(Action::EditType, self.type_subject().is_some()),
                Item::new(Action::SelectAllInstances, true),
                Item::new(Action::Delete, true),
            ]);
            // Detail lines, regions and notes group; a group comes apart.
            let m = &self.session.model;
            if let [id] = self.selection.as_slice() {
                if m.elements.get(*id).is_some_and(|element| m.run_geom(element).is_some()) {
                    groups.push(vec![Item::new(Action::MountToSurface, true)]);
                }
                if let Some(bimcraft_core::model::Element::Wire(wire)) = m.elements.get(*id) {
                    groups.push(vec![Item::new(Action::CreateCarrierFromWire, wire.carrier.is_none() && wire.feeder.is_none())]);
                }
                if matches!(m.elements.get(*id), Some(bimcraft_core::model::Element::CableCarrier(_))) {
                    groups.push(vec![Item::new(Action::AttachExistingWire, m.carrier_wire_candidates(*id).is_ok_and(|v| !v.is_empty()))]);
                }
            }
            let groupable = self.selection.iter().all(|id| m.elements.get(*id).is_some_and(bimcraft_core::groups::holds));
            if groupable || self.group_selected() {
                groups.push(vec![Item::new(Action::CreateGroup, groupable), Item::new(Action::Ungroup, self.group_selected())]);
            }
            if !self.linked_cads().is_empty() {
                groups.push(vec![Item::new(Action::ReloadCad, true)]);
            }
            // What the view on screen hides and paints.
            if self.graphic_view().is_some() {
                groups.push(vec![
                    Item::new(Action::HideElement, true),
                    Item::new(Action::HideCategory, true),
                    Item::new(Action::OverrideGraphics, true),
                ]);
            }
        }
        let zoom = vec![Item::new(Action::ZoomToFit, true), Item::new(Action::ZoomIn, true), Item::new(Action::ZoomOut, true)];
        match self.sheet {
            Sheet::Plan(_) | Sheet::Ceiling(_) | Sheet::Section(_) | Sheet::Paper(_) => groups.push(zoom),
            Sheet::View3D => groups.push([vec![Item::new(Action::HomeView, true)], zoom].concat()),
            Sheet::Schedule(_) | Sheet::Table(_) => {}
        }
        groups
    }

    /// Do what `action` says.
    pub(crate) fn act(&mut self, action: Action) {
        match action {
            Action::Cancel => self.escape(),
            Action::Finish => self.finish_open_shape(),
            Action::ZoomToFit => self.fit(),
            Action::ZoomIn => self.zoom_view(1.25),
            Action::ZoomOut => self.zoom_view(0.8),
            Action::HomeView => self.orbit.camera = None,
            Action::EditType => self.edit_type(),
            Action::SelectAllInstances => self.select_all_instances(),
            Action::HideElement => self.hide_selection(false),
            Action::HideCategory => self.hide_selection(true),
            Action::OverrideGraphics => self.override_selection(),
            Action::CreateGroup => self.create_group(),
            Action::Ungroup => self.ungroup(),
            Action::Delete => self.delete(),
            Action::AttachExistingWire => {
                if let [id] = self.selection.as_slice() {
                    self.offer_carrier_attachment(*id);
                }
            }
            Action::MountToSurface => {
                if let [id] = self.selection.as_slice() {
                    self.mount_run = Some(*id);
                }
            }
            Action::ConnectEquipmentToWire => {
                if let Some((wire, device)) = self.wire_and_equipment() {
                    let m = &self.session.model;
                    let connection =
                        m.elements.get(wire).and_then(|e| e.run()).and_then(|r| r.connections.iter().find(|c| c.device == m.guid(device)));
                    self.wire_connection = Some(WireConnectionDialog {
                        wire: m.guid(wire),
                        device: m.guid(device),
                        end: connection.map_or(0, |c| c.end),
                        carrier: connection.and_then(|c| c.carrier),
                        wall: connection.and_then(|c| c.surface.as_ref()?.host),
                        frozen_surface: connection.and_then(|c| c.surface.as_ref()).is_some_and(|s| s.host.is_none()),
                        exterior: connection.and_then(|c| c.surface.as_ref()).is_some_and(|s| s.face == "exterior"),
                        bend_radius: connection.and_then(|c| c.min_bend_radius).map(|r| m.units.length(r)).unwrap_or_default(),
                        bend_radius_edited: false,
                        point: connection.map_or([0.0; 3], |c| c.point).map(|v| m.units.length(v)),
                        in_carrier: connection.is_some_and(|c| c.in_carrier),
                    });
                }
            }
            Action::CreateCarrierFromWire => {
                if let [id] = self.selection.as_slice() {
                    let id = *id;
                    if self
                        .session
                        .model
                        .elements
                        .get(id)
                        .is_some_and(|element| matches!(element, bimcraft_core::model::Element::Wire(wire) if wire.carrier.is_none() && wire.feeder.is_none()))
                    {
                        let m = &self.session.model;
                        let type_name = m.carrier_settings.as_ref().map(|v| v.default_type.as_str()).filter(|v| !v.is_empty())
                            .or_else(|| m.default_type("cablecarrier")).unwrap_or_default().to_owned();
                        self.carrier_from_wire = Some(CarrierFromWireDialog {
                            wire: m.guid(id), type_name, placement: "centre", lateral: m.units.length(0.0),
                        });
                    }
                }
            }
            Action::ReloadCad => {
                for cad in self.linked_cads() {
                    self.run("reload_cad", serde_json::json!({"cad": cad}));
                }
            }
        }
    }

    /// The linked CAD drawings the selection places, when all it holds are imports of linked drawings; else none.
    fn linked_cads(&self) -> Vec<String> {
        let m = &self.session.model;
        let mut names = Vec::new();
        for id in &self.selection {
            match m.elements.get(*id) {
                Some(bimcraft_core::model::Element::CadImport(ci)) if m.cads.get(&ci.cad).is_some_and(|c| c.path().is_some()) => {
                    if !names.contains(&ci.cad) {
                        names.push(ci.cad.clone());
                    }
                }
                _ => return Vec::new(),
            }
        }
        names
    }

    fn wire_and_equipment(&self) -> Option<(bimcraft_core::model::Id, bimcraft_core::model::Id)> {
        use bimcraft_core::model::Element;
        let [a, b] = self.selection.as_slice() else { return None };
        let m = &self.session.model;
        for (wire, device) in [(*a, *b), (*b, *a)] {
            if matches!(m.elements.get(wire),Some(Element::Wire(r)) if r.feeder.is_none())
                && matches!(m.elements.get(device),Some(Element::Component(c)) if c.is_placed())
            {
                return Some((wire, device));
            }
        }
        None
    }

    pub(crate) fn wire_connection_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.wire_connection.take() else { return };
        let mut connect = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("wire-connection")).show(ctx, |ui| {
            ui.heading(t("Connect Equipment to Wire"));
            let old_end = dialog.end;
            ui.horizontal(|ui| {
                ui.label(t(msg!("Cable end")));
                ui.selectable_value(&mut dialog.end, 0, t(msg!("First end")));
                ui.selectable_value(&mut dialog.end, 1, t(msg!("Second end")));
            });
            if old_end != dialog.end {
                let connection = self
                    .session
                    .model
                    .elements
                    .get(dialog.wire)
                    .and_then(|e| e.run())
                    .and_then(|r| r.connections.iter().find(|c| c.device == dialog.device && c.end == dialog.end));
                dialog.point = connection.map_or([0.0; 3], |c| c.point).map(|v| self.session.model.units.length(v));
                dialog.in_carrier = connection.is_some_and(|c| c.in_carrier);
                dialog.carrier = connection.and_then(|c| c.carrier);
                dialog.wall = connection.and_then(|c| c.surface.as_ref()?.host);
                dialog.frozen_surface = connection.and_then(|c| c.surface.as_ref()).is_some_and(|s| s.host.is_none());
                dialog.exterior = connection.and_then(|c| c.surface.as_ref()).is_some_and(|s| s.face == "exterior");
                dialog.bend_radius =
                    connection.and_then(|c| c.min_bend_radius).map(|r| self.session.model.units.length(r)).unwrap_or_default();
                dialog.bend_radius_edited = false;
            }
            let model = &self.session.model;
            let route_name = |guid| {
                model
                    .elements
                    .get(guid)
                    .and_then(|e| e.run())
                    .map(|run| format!("{}: {}", model.elements.id(guid).unwrap_or_default(), run.type_name))
                    .unwrap_or_else(|| t(msg!("Main wire route")).to_owned())
            };
            let label = ui.label(t(msg!("Route carrier")));
            egui::ComboBox::from_id_salt("connection-route-carrier")
                .selected_text(dialog.carrier.map_or_else(|| t("Main wire route").to_owned(), route_name))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut dialog.carrier, None, t("Main wire route"));
                    for (id, element) in model.elements.iter() {
                        if let bimcraft_core::model::Element::CableCarrier(run) = element
                            && run.feeder.is_none()
                        {
                            ui.selectable_value(&mut dialog.carrier, Some(model.guid(*id)), format!("{id}: {}", run.type_name));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            let can_carrier =
                dialog.carrier.is_some() || model.elements.get(dialog.wire).and_then(|e| e.run()).is_some_and(|r| r.carrier.is_some());
            let wall_name = |id| format!("{} {id}", t("Wall"));
            let label = ui.label(t("Mounting surface"));
            egui::ComboBox::from_id_salt("wire-mounting-surface")
                .selected_text(dialog.wall.and_then(|wall| model.elements.id(wall)).map_or_else(
                    || t(if dialog.frozen_surface { "Saved plane (wall deleted)" } else { "Fixed point" }).to_owned(),
                    wall_name,
                ))
                .show_ui(ui, |ui| {
                    if ui.selectable_label(dialog.wall.is_none() && !dialog.frozen_surface, t("Fixed point")).clicked() {
                        dialog.wall = None;
                        dialog.frozen_surface = false;
                    }
                    for (id, element) in model.elements.iter() {
                        if matches!(element, bimcraft_core::model::Element::Wall(_))
                            && ui.selectable_value(&mut dialog.wall, Some(model.guid(*id)), wall_name(*id)).clicked()
                        {
                            dialog.frozen_surface = false;
                        }
                    }
                })
                .response
                .labelled_by(label.id);
            if dialog.wall.is_some() {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut dialog.exterior, false, t("Interior face"));
                    ui.selectable_value(&mut dialog.exterior, true, t("Exterior face"));
                });
                if dialog.carrier.is_some() {
                    ui.horizontal(|ui| {
                        let label = ui.label(t("Minimum bend radius"));
                        dialog.bend_radius_edited |= ui
                            .add(egui::TextEdit::singleline(&mut dialog.bend_radius).desired_width(140.0))
                            .labelled_by(label.id)
                            .changed();
                    });
                }
            } else if dialog.frozen_surface {
                ui.label(t("The mounting wall was deleted. Select a new wall or a fixed point to reconnect."));
            } else {
                ui.label(t(msg!("Connection point relative to equipment insertion point")));
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut dialog.point) {
                    ui.horizontal(|ui| {
                        let label = ui.label(axis);
                        ui.add(egui::TextEdit::singleline(value).desired_width(140.0)).labelled_by(label.id);
                    });
                }
            }
            ui.add_enabled_ui(can_carrier, |ui| ui.checkbox(&mut dialog.in_carrier, t(msg!("Create carrier branches"))));
            ui.horizontal(|ui| {
                connect = ui.add_enabled(!dialog.frozen_surface, Button::new(t(msg!("Connect")))).clicked();
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
        if connect {
            let mut params = serde_json::json!({"wire":dialog.wire,"device":dialog.device,
                "end":dialog.end,"carrier":dialog.carrier,"in_carrier":dialog.in_carrier});
            let point: Option<Vec<f64>> = dialog.point.iter().map(|v| self.session.model.units.parse(v)).collect();
            if let Some(wall) = dialog.wall {
                params["surface"] = serde_json::json!({"host":wall,"face":if dialog.exterior {"exterior"} else {"interior"}});
                if dialog.carrier.is_some() && dialog.bend_radius_edited {
                    if dialog.bend_radius.trim().is_empty() {
                        params["min_bend_radius"] = serde_json::Value::Null;
                    } else if let Some(radius) = self.session.model.units.parse(&dialog.bend_radius) {
                        params["min_bend_radius"] = serde_json::json!(radius);
                    } else {
                        self.status = crate::Status { text: t("Enter a valid bend radius.").into(), error: true };
                        self.wire_connection = Some(dialog);
                        return;
                    }
                }
            } else if let Some(point) = point {
                params["point"] = serde_json::json!(point);
            } else {
                self.status = crate::Status { text: t(msg!("Enter valid connection coordinates.")).into(), error: true };
            }
            if (dialog.wall.is_some() || params.get("point").is_some()) && self.run("wire_connect", params).is_some() {
                return;
            }
        }
        if !cancel && !response.should_close() {
            self.wire_connection = Some(dialog);
        }
    }

    pub(crate) fn mount_run_window(&mut self, ctx: &egui::Context) {
        use bimcraft_core::model::Element;
        let Some(id) = self.mount_run.take() else { return };
        let mut target = None;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("mount_run")).show(ctx, |ui| {
            ui.heading(t(msg!("Mount to Surface")));
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for (host, element) in self.session.model.elements.iter() {
                    let faces = match element {
                        Element::Wall(_) => [("interior", msg!("Interior")), ("exterior", msg!("Exterior"))],
                        Element::Floor(_) | Element::Ceiling(_) => [("top", msg!("Top")), ("bottom", msg!("Bottom"))],
                        _ => continue,
                    };
                    ui.horizontal(|ui| {
                        ui.label(format!("{} {}", element.category(), host));
                        for (face, label) in faces {
                            if ui.button(t(label)).clicked() {
                                target = Some((host, face));
                            }
                        }
                    });
                }
            });
            cancel = ui.button(t("Cancel")).clicked();
        });
        if let Some((host, face)) = target
            && self.run("run_mount", serde_json::json!({"id":id,"host":host,"face":face})).is_some()
        {
            return;
        }
        if !cancel && !response.should_close() {
            self.mount_run = Some(id);
        }
    }

    pub(crate) fn carrier_from_wire_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.carrier_from_wire.take() else { return };
        let mut create = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("carrier-from-wire")).show(ctx, |ui| {
            ui.heading(t(msg!("Create Carrier from Wire")));
            let label = ui.label(t("Type:"));
            let combo = egui::ComboBox::from_id_salt("carrier-from-wire-type").selected_text(&dialog.type_name).show_ui(ui, |ui| {
                if let Some(types) = self.session.model.types.runs.get("cablecarrier") {
                    for name in types.keys() {
                        ui.selectable_value(&mut dialog.type_name, name.clone(), name);
                    }
                }
            });
            combo.response.labelled_by(label.id);
            ui.label(t(msg!("Wire placement inside carrier:")));
            ui.horizontal(|ui| {
                for (value, label) in [("top", "Top"), ("centre", "Centre"), ("bottom", "Bottom")] {
                    ui.radio_value(&mut dialog.placement, value, t(label));
                }
            });
            let label = ui.label(t(msg!("Lateral Offset:")));
            ui.add(egui::TextEdit::singleline(&mut dialog.lateral).desired_width(100.0)).labelled_by(label.id);
            ui.horizontal(|ui| {
                create = ui.button(t(msg!("Create Carrier"))).clicked();
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
        if create {
            if let Some(lateral) = self.session.model.units.parse(&dialog.lateral) {
                if self
                    .run(
                        "cable_carrier",
                        serde_json::json!({"from_wire":dialog.wire,"type":dialog.type_name,
                    "placement":dialog.placement,"local_offset":lateral}),
                    )
                    .is_some()
                {
                    return;
                }
            } else {
                self.status = crate::Status { text: t(msg!("Enter a valid lateral offset.")).into(), error: true };
            }
        }
        if !cancel && !response.should_close() {
            self.carrier_from_wire = Some(dialog);
        }
    }

    pub(crate) fn offer_carrier_move(&mut self, params: &serde_json::Value) -> bool {
        use bimcraft_core::model::Element;
        let model = &self.session.model;
        let Some(ids) = params["ids"].as_array() else { return false };
        let carriers: std::collections::BTreeSet<_> = ids
            .iter()
            .filter_map(serde_json::Value::as_u64)
            .filter(|id| matches!(model.elements.get(*id), Some(Element::CableCarrier(_))))
            .map(|id| model.guid(id))
            .collect();
        if carriers.is_empty()
            || !model.elements.values().filter_map(Element::run).any(|run| {
                run.connections.iter().any(|connection| connection.carrier.or(run.carrier).is_some_and(|route| carriers.contains(&route)))
            })
        {
            return false;
        }
        self.carrier_move = Some(params.clone());
        true
    }

    pub(crate) fn carrier_move_window(&mut self, ctx: &egui::Context) {
        let Some(mut params) = self.carrier_move.take() else { return };
        let mut choice = None;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("move-connected-carrier")).show(ctx, |ui| {
            ui.heading(t(msg!("Move Connected Cable Carrier")));
            ui.horizontal(|ui| {
                if ui.button(t(msg!("Keep Connection"))).clicked() {
                    choice = Some(false);
                }
                if ui.button(t(msg!("Disconnect Equipment"))).clicked() {
                    choice = Some(true);
                }
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
        if let Some(disconnect) = choice {
            params["disconnect_equipment"] = disconnect.into();
            if self.run("move", params).is_some() {
                self.tool = Tool::Modify;
            }
        } else if !cancel && !response.should_close() {
            self.carrier_move = Some(params);
        }
    }

    pub(crate) fn offer_carrier_attachment(&mut self, carrier: bimcraft_core::model::Id) {
        let m = &self.session.model;
        if let Ok(wires) = m.carrier_wire_candidates(carrier)
            && !wires.is_empty()
        {
            self.carrier_attachment = Some(CarrierAttachmentDialog {
                carrier: m.guid(carrier),
                wires: wires.into_iter().map(|id| m.guid(id)).collect(),
                selected: None,
            });
        }
    }

    pub(crate) fn carrier_attachment_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.carrier_attachment.take() else { return };
        let mut attach = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::new("carrier-attachment")).show(ctx, |ui| {
            ui.heading(t(msg!("Attach Existing Wire")));
            ui.label(t(msg!("Overlapping wires: select a wire and confirm the attachment.")));
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for wire in &dialog.wires {
                    if let Some(id) = self.session.model.elements.id(*wire) {
                        ui.radio_value(&mut dialog.selected, Some(*wire), crate::lang::tf(msg!("Wire {}"), &[&id.to_string()]));
                    }
                }
            });
            ui.horizontal(|ui| {
                attach = ui.add_enabled(dialog.selected.is_some(), Button::new(t(msg!("Attach Selected Wire")))).clicked();
                cancel = ui.button(t("Cancel")).clicked();
            });
        });
        if attach
            && let (Some(carrier), Some(wire)) =
                (self.session.model.elements.id(dialog.carrier), dialog.selected.and_then(|g| self.session.model.elements.id(g)))
        {
            if self.session.model.carrier_wire_candidates(carrier).is_ok_and(|wires| wires.contains(&wire)) {
                if self.run("modify", serde_json::json!({"id":wire,"carrier":dialog.carrier})).is_some() {
                    return;
                }
            } else {
                self.status = crate::Status { text: t(msg!("The wire no longer overlaps this carrier.")).into(), error: true };
            }
        }
        if !cancel && !response.should_close() {
            self.carrier_attachment = Some(dialog);
        }
    }

    /// The menu on a view's `response`, doing what's chosen from it.
    pub(crate) fn context_menu(&mut self, response: &Response) {
        let groups = self.context_items();
        let mut chosen = None;
        menu(response, &groups, &mut chosen);
        if let Some(action) = chosen {
            self.act(action);
        }
    }
}
