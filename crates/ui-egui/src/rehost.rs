//! Pick New Host in a plan: a snapshot-bound pick, previewed through the same command as its click.

use std::{cell::RefCell, sync::Arc};

use bimcraft_core::engine::Session;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Guid, Id, Model};
use bimcraft_core::walls::Axis;
use serde_json::{Value, json};

use crate::lang::t;
use crate::{BimApp, Sheet, Status, Tool};

pub(crate) struct Pending {
    id: Guid,
    model: Arc<Model>,
    view: Sheet,
    preview: RefCell<Option<Preview>>,
}

struct Preview {
    params: Value,
    result: Result<Arc<Model>, String>,
}

impl BimApp {
    pub(crate) fn rehost_selected(&self) -> bool {
        matches!(self.selection.as_slice(), [id] if matches!(self.session.model.elements.get(*id), Some(Element::Door(_) | Element::Window(_))))
    }

    pub(crate) fn rehost_available(&self) -> bool {
        self.rehost_selected()
            && matches!(self.sheet, Sheet::Plan(_) | Sheet::Ceiling(_))
            && self.sketch.is_none()
            && self.session.editing_group().is_none()
            && self.selection.first().is_some_and(|id| bimcraft_core::rehost::opening(&self.session.model, *id).is_ok())
    }

    pub(crate) fn start_rehost(&mut self) {
        if !self.rehost_available() {
            self.status = Status {
                text: t("Pick New Host: select one unpinned, ungrouped door or window in an ordinary straight wall, in a plan.").into(),
                error: true,
            };
            return;
        }
        let Some(&id) = self.selection.first() else { return };
        let id = self.session.model.guid(id);
        self.set_tool(Tool::PickNewHost);
        self.rehost = Some(Pending { id, model: Arc::clone(&self.session.model), view: self.sheet, preview: RefCell::new(None) });
    }

    pub(crate) fn keep_rehost(&mut self) {
        let stale = self.rehost.as_ref().is_some_and(|p| {
            !Arc::ptr_eq(&p.model, &self.session.model)
                || p.view != self.sheet
                || self.session.editing_group().is_some()
                || self.tool != Tool::PickNewHost
                || self.selection.as_slice() != p.model.elements.id(p.id).as_slice()
        });
        if stale {
            self.set_tool(Tool::Modify);
            self.status = Status { text: t("Pick New Host cancelled because the project or view changed.").into(), error: false };
        }
    }

    pub(crate) fn rehost_params(&self, wall: Option<Id>, at: P2, reach: f64) -> Result<Value, String> {
        let pending = self.rehost.as_ref().ok_or_else(|| t("Start Pick New Host again.").to_string())?;
        if !Arc::ptr_eq(&pending.model, &self.session.model) || pending.view != self.sheet {
            return Err(t("Pick New Host cancelled because the project or view changed.").into());
        }
        let m = &self.session.model;
        let wall = wall.ok_or_else(|| t("Pick another ordinary straight wall.").to_string())?;
        let w = m.wall(wall).ok_or_else(|| t("Pick another ordinary straight wall.").to_string())?;
        let Some(Element::Door(o) | Element::Window(o)) = m.elements.get(pending.id) else {
            return Err(t("Start Pick New Host again.").into());
        };
        let mut placed = o.clone();
        placed.host = m.guid(wall);
        placed.offset = Axis::of(w).project(at).0;
        if let Some(centre) = m.centred(&placed, reach) {
            placed.offset = centre;
        }
        Ok(json!({"id": pending.id, "wall": placed.host, "offset": placed.offset}))
    }

    pub(crate) fn rehost_preview(&self, params: &Value) -> Result<Arc<Model>, String> {
        let pending = self.rehost.as_ref().ok_or_else(|| t("Start Pick New Host again.").to_string())?;
        if let Some(held) = pending.preview.borrow().as_ref().filter(|p| p.params == *params) {
            return held.result.clone();
        }
        let mut scratch = Session::new();
        // This snapshot is the live session's checked model. The scratch owns no live history or document.
        scratch.adopt((*pending.model).clone());
        scratch.phase = self.session.phase.clone();
        scratch.option = self.session.option.clone();
        let result = scratch.execute("pick_new_host", params).map(|_| scratch.model);
        *pending.preview.borrow_mut() = Some(Preview { params: params.clone(), result: result.clone() });
        result
    }

    pub(crate) fn rehost_click(&mut self, wall: Option<Id>, at: P2, reach: f64) {
        match self.rehost_params(wall, at, reach) {
            Ok(params) => {
                if self.run("pick_new_host", params).is_some() {
                    self.set_tool(Tool::Modify);
                }
            }
            Err(text) => self.status = Status { text, error: true },
        }
    }
}
