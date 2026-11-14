//! Coordination: a 3D view as a BCF 2.1 topic (`.bcfzip`), buildingSMART's open format for issues about a model, which
//! Solibri, BIMcollab, Navisworks, Revit's plug-ins and the rest read.
//!
//! The file is a zip of `bcf.version`, and a folder for the topic (named by its GUID) holding its `markup.bcf` (the title,
//! the status, when and by what it was made), the view's `viewpoint.bcfv` and a `snapshot.png` of it. The viewpoint is the
//! view's camera (perspective through its lens, or parallel), in metres in the coordinates an IFC export from the internal
//! origin writes; its components name elements by their IFC GlobalId (each element's GUID, as the IFC export writes it), so
//! the issue lands on the same elements in the IFC file: those the options say are selected, and those the view hides.
//!
//! Its GUIDs come from the view's, so the same view exported again is the same topic, which a tool updates rather than
//! adds.

use std::fmt::Write;

use super::super::{Content, Ctx, Family, Format, Options, Out, Prepared, Takes};
use crate::graphics::Look;
use crate::model::{Element, Model};
use crate::view3d::{Camera, FILM, MAX_SIDE, P3, Scene};
use crate::xlsx::{Part, escape, zip};

/// The snapshot's width, pixels, as BCF viewers show it.
const SNAPSHOT: f64 = 1280.0;
/// The most components a viewpoint lists, selected or hidden.
const MAX_COMPONENTS: usize = 50_000;

pub struct Bcf;

impl Format for Bcf {
    fn id(&self) -> &'static str {
        "bcf"
    }
    fn label(&self) -> &'static str {
        "BCF"
    }
    fn extension(&self) -> &'static str {
        "bcfzip"
    }
    fn family(&self) -> super::super::Family {
        Family::Coordination
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Model]
    }
    fn write(&self, items: &[Prepared], options: &Options, ctx: &Ctx) -> Result<Vec<Out>, String> {
        let [item] = items else { return Err("a BCF file here holds one view's topic".to_string()) };
        let Content::Model { model, view } = &item.content else { return Err(format!("{} isn't a model to write", item.title)) };
        let seen = crate::link::seen(model);
        // The view's own, or the default 3D view's (as it is before anything changes it, when the project has none).
        let id = view.or_else(|| seen.default_3d());
        let default = crate::model::View3D::default_view();
        let (v, look) = match id.map(|id| (id, seen.elements.get(id))) {
            Some((id, Some(Element::View3D(v)))) => (v, Look::of(&seen, id)),
            Some(_) => return Err(format!("{} isn't a 3D view", item.title)),
            None => (&default, Look::of_3d(&seen)),
        };
        // Fitted at a small scale, then at the one that makes it the snapshot's width.
        let small = crate::view3d::camera_of(&seen, v, 0.01, 0.0, &look);
        let k = SNAPSHOT / f64::from(small.width.max(1));
        let cam = crate::view3d::camera_of(&seen, v, 0.01 * k, 20.0, &look);
        if cam.width > MAX_SIDE || cam.height > MAX_SIDE {
            return Err("the view is too tall to take a snapshot of".to_string());
        }
        let scene = Scene::new_cancellable(&seen, ctx.cancelled).ok_or("cancelled")?;
        let image = scene.render_in(&cam, &[], &look);
        let size = tiny_skia::IntSize::from_wh(image.width, image.height).ok_or("the snapshot has no size")?;
        let snapshot = tiny_skia::Pixmap::from_vec(image.rgba, size).ok_or("the snapshot's pixels don't fit it")?;
        let snapshot = snapshot.encode_png().map_err(|e| e.to_string())?;
        // What the view hides: the scene's elements it doesn't show.
        let mut hidden: Vec<crate::model::Id> = Vec::new();
        for f in scene.facets(&Look::default()) {
            if hidden.last() != Some(&f.owner) && !look.shows(f.owner) {
                hidden.push(f.owner);
            }
        }
        hidden.sort_unstable();
        hidden.dedup();
        let selected: Vec<String> =
            options.selected.iter().filter(|g| seen.elements.id(**g).is_some()).map(|g| global_id(g.as_u128())).collect();
        let hidden: Vec<String> = hidden.into_iter().map(|id| global_id(seen.guid(id).as_u128())).collect();
        if selected.len() + hidden.len() > MAX_COMPONENTS {
            return Err(format!("a viewpoint here names at most {MAX_COMPONENTS} elements; select fewer, or show more in the view"));
        }
        // The default view's own, or one of the project's for the default 3D view it hasn't made yet.
        let view_guid = id.map_or(u128::from(seen.uid) << 64 | 0x3d, |id| seen.guid(id).as_u128());
        let topic = uuid(view_guid ^ 0x5bcf_0001);
        let viewpoint = uuid(view_guid ^ 0x5bcf_0002);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let markup = markup(&seen, &v.name, &topic, &viewpoint, &crate::ifc::utc(now));
        let viewpoint_xml = viewpoint_xml(&viewpoint, &cam, &selected, &hidden);
        let folder = |name: &str| format!("{topic}/{name}");
        let (m, vp, png) = (folder("markup.bcf"), folder("viewpoint.bcfv"), folder("snapshot.png"));
        let bytes = zip(&[
            Part { name: "bcf.version", data: VERSION.as_bytes(), store: false },
            Part { name: &m, data: markup.as_bytes(), store: false },
            Part { name: &vp, data: viewpoint_xml.as_bytes(), store: false },
            // A PNG is packed already.
            Part { name: &png, data: &snapshot, store: true },
        ]);
        Ok(vec![Out { suffix: None, bytes }])
    }
}

const VERSION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Version VersionId="2.1" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="version.xsd"><DetailedVersion>2.1</DetailedVersion></Version>"#;

/// 128 bits as a UUID, lower case as BCF writes them.
fn uuid(v: u128) -> String {
    let h = format!("{v:032x}");
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// An element's IFC GlobalId, as the IFC export writes it.
fn global_id(v: u128) -> String {
    crate::ifc::compress(v)
}

fn markup(m: &Model, title: &str, topic: &str, viewpoint: &str, now: &str) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Markup xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="markup.xsd">"#,
    );
    let _ = write!(
        out,
        "<Topic Guid=\"{topic}\" TopicType=\"Issue\" TopicStatus=\"Open\"><Title>{}</Title><CreationDate>{now}</CreationDate>\
         <CreationAuthor>{}</CreationAuthor><Description>{}</Description></Topic>",
        escape(&format!("{} — {title}", m.name)),
        escape(&format!("BIMCraft {}", env!("CARGO_PKG_VERSION"))),
        escape(&format!("The view {title} of {}, as BIMCraft shows it.", m.name)),
    );
    let _ =
        write!(out, "<Viewpoints Guid=\"{viewpoint}\"><Viewpoint>viewpoint.bcfv</Viewpoint><Snapshot>snapshot.png</Snapshot></Viewpoints>");
    out.push_str("</Markup>");
    out
}

/// A point or a direction as BCF writes them: metres for a point (`k` 0.001), as it is for a direction.
fn xyz(tag: &str, p: P3, k: f64) -> String {
    let n = |v: f64| {
        let v = v * k + 0.0;
        if v.is_finite() { format!("{v:.6}") } else { "0".to_string() }
    };
    format!("<{tag}><X>{}</X><Y>{}</Y><Z>{}</Z></{tag}>", n(p[0]), n(p[1]), n(p[2]))
}

fn viewpoint_xml(guid: &str, cam: &Camera, selected: &[String], hidden: &[String]) -> String {
    let (back, _, up) = cam.axes();
    let direction = [-back[0], -back[1], -back[2]];
    let mut out = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<VisualizationInfo Guid="{guid}" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="visinfo.xsd">"#
    );
    out.push_str("<Components>");
    if !selected.is_empty() {
        out.push_str("<Selection>");
        for g in selected {
            let _ = write!(out, "<Component IfcGuid=\"{g}\"/>");
        }
        out.push_str("</Selection>");
    }
    out.push_str("<Visibility DefaultVisibility=\"true\">");
    if !hidden.is_empty() {
        out.push_str("<Exceptions>");
        for g in hidden {
            let _ = write!(out, "<Component IfcGuid=\"{g}\"/>");
        }
        out.push_str("</Exceptions>");
    }
    out.push_str("</Visibility></Components>");
    match (cam.eye(), cam.lens) {
        (Some(eye), Some(lens)) => {
            // The vertical angle the lens sees: BCF's field of view.
            let focal = f64::from(cam.width.max(1)) * lens / FILM;
            let fov = (2.0 * (f64::from(cam.height) / 2.0 / focal).atan()).to_degrees();
            let _ = write!(
                out,
                "<PerspectiveCamera>{}{}{}<FieldOfView>{:.4}</FieldOfView></PerspectiveCamera>",
                xyz("CameraViewPoint", eye, 0.001),
                xyz("CameraDirection", direction, 1.0),
                xyz("CameraUpVector", up, 1.0),
                fov.clamp(1.0, 179.0)
            );
        }
        _ => {
            // Back from the target along the view, as far as the picture is tall, so the model is in front of it.
            let height = f64::from(cam.height) / cam.scale.max(1e-12);
            let at = [cam.target[0] + back[0] * height, cam.target[1] + back[1] * height, cam.target[2] + back[2] * height];
            let _ = write!(
                out,
                "<OrthogonalCamera>{}{}{}<ViewToWorldScale>{:.6}</ViewToWorldScale></OrthogonalCamera>",
                xyz("CameraViewPoint", at, 0.001),
                xyz("CameraDirection", direction, 1.0),
                xyz("CameraUpVector", up, 1.0),
                height / 1000.0
            );
        }
    }
    out.push_str("</VisualizationInfo>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use crate::export::prepare;
    use serde_json::json;

    fn topic(s: &Session, view: Option<&str>, selected: Vec<crate::ids::Guid>) -> std::collections::BTreeMap<String, String> {
        let m = crate::link::seen(&s.model);
        let id = view.and_then(|v| m.view_named(v));
        let p = prepare(&s.model, &m, id, Takes::Model).unwrap();
        let out = Bcf.write(&[p], &Options { selected, ..Options::default() }, &Ctx::quiet()).unwrap();
        crate::xlsx::unzip(&out[0].bytes)
    }

    #[test]
    fn a_3d_view_is_a_topic_with_its_viewpoint_and_snapshot() {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        let wall = s.model.guid(100002);
        let parts = topic(&s, None, vec![wall]);
        let names: Vec<&String> = parts.keys().collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(parts["bcf.version"].contains("VersionId=\"2.1\""));
        let folder = names.iter().find(|n| n.ends_with("/markup.bcf")).unwrap().trim_end_matches("/markup.bcf").to_string();
        assert_eq!(folder.len(), 36, "a UUID: {folder}");
        let markup = &parts[&format!("{folder}/markup.bcf")];
        assert!(markup.contains(&format!("<Topic Guid=\"{folder}\"")) && markup.contains("{3D}"), "{markup}");
        let vp = &parts[&format!("{folder}/viewpoint.bcfv")];
        // The default 3D view is parallel: an orthogonal camera, looking down from the south-west.
        assert!(vp.contains("<OrthogonalCamera>") && vp.contains("<ViewToWorldScale>"), "{vp}");
        // The wall selected by its IFC GlobalId, as the IFC export writes it.
        let ifc = crate::ifc::export(&s.model);
        let gid = crate::ifc::compress(wall.as_u128());
        assert!(ifc.contains(&gid) && vp.contains(&format!("<Selection><Component IfcGuid=\"{gid}\"/></Selection>")), "{vp}");
        assert!(parts.contains_key(&format!("{folder}/snapshot.png")));
        // The same view again: the same topic.
        assert!(topic(&s, None, Vec::new()).contains_key(&format!("{folder}/markup.bcf")));
    }

    #[test]
    fn a_camera_views_topic_is_seen_through_its_lens_with_what_it_hides_hidden() {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        s.execute("camera", &json!({"eye": [-6000, -8000, 1700], "target": [4000, 3000, 1500], "name": "Corner"})).unwrap();
        s.execute("graphics", &json!({"view": "Corner", "hide_elements": [100002]})).unwrap();
        let parts = topic(&s, Some("Corner"), Vec::new());
        let vp = parts.iter().find(|(n, _)| n.ends_with("viewpoint.bcfv")).map(|(_, v)| v.clone()).unwrap();
        assert!(vp.contains("<PerspectiveCamera>") && vp.contains("<FieldOfView>"), "{vp}");
        // The wall it hides, an exception to what it shows; nothing selected.
        let gid = crate::ifc::compress(s.model.guid(100002).as_u128());
        assert!(vp.contains(&format!("<Exceptions><Component IfcGuid=\"{gid}\"/></Exceptions>")), "{vp}");
        assert!(!vp.contains("<Selection>"));
        // The eye where the camera stands, in metres.
        assert!(vp.contains("<CameraViewPoint><X>-6.000000</X><Y>-8.000000</Y><Z>1.700000</Z></CameraViewPoint>"), "{vp}");
    }
}
