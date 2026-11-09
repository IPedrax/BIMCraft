//! The model in 3D: IFC4 and IFC4.3 (`ifc::export_as`) and glTF binary (`gltf::export`, what the default 3D view shows, links and all).

use super::super::{Content, Ctx, Family, Format, Options, Out, Prepared, Takes};
use crate::model::Model;

/// The one model a model format writes.
fn model(items: &[Prepared]) -> Result<&std::sync::Arc<Model>, String> {
    match items {
        [item] => match &item.content {
            Content::Model { model, .. } => Ok(model),
            _ => Err(format!("{} isn't a model to write", item.title)),
        },
        _ => Err("a model file holds one project".to_string()),
    }
}

/// IFC in its schema.
pub struct Ifc(pub crate::ifc::Schema);

impl Format for Ifc {
    fn id(&self) -> &'static str {
        match self.0 {
            crate::ifc::Schema::Ifc4 => "ifc4",
            crate::ifc::Schema::Ifc4x3 => "ifc4x3",
        }
    }
    fn label(&self) -> &'static str {
        match self.0 {
            crate::ifc::Schema::Ifc4 => "IFC4",
            crate::ifc::Schema::Ifc4x3 => "IFC4.3",
        }
    }
    fn extension(&self) -> &'static str {
        "ifc"
    }
    fn family(&self) -> Family {
        Family::Model
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Model]
    }
    fn write(&self, items: &[Prepared], options: &Options, _: &Ctx) -> Result<Vec<Out>, String> {
        Ok(vec![Out { suffix: None, bytes: crate::ifc::export_as(model(items)?, options.ifc_origin, self.0).into_bytes() }])
    }
}

pub struct Glb;

impl Format for Glb {
    fn id(&self) -> &'static str {
        "glb"
    }
    fn label(&self) -> &'static str {
        "glTF"
    }
    fn extension(&self) -> &'static str {
        "glb"
    }
    fn family(&self) -> Family {
        Family::Model
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Model]
    }
    fn write(&self, items: &[Prepared], _: &Options, _: &Ctx) -> Result<Vec<Out>, String> {
        let out = crate::gltf::export(&crate::link::seen(model(items)?))?;
        Ok(vec![Out { suffix: None, bytes: out.bytes }])
    }
}
