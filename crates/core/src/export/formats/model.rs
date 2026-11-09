//! The model in 3D: IFC (`ifc::export`) and glTF binary (`gltf::export`, what the default 3D view shows, links and all).

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

pub struct Ifc;

impl Format for Ifc {
    fn id(&self) -> &'static str {
        "ifc4"
    }
    fn label(&self) -> &'static str {
        "IFC4"
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
    fn write(&self, items: &[Prepared], _: &Options, _: &Ctx) -> Result<Vec<Out>, String> {
        Ok(vec![Out { suffix: None, bytes: crate::ifc::export(model(items)?).into_bytes() }])
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
