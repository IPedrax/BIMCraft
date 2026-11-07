//! Revit's Preview Visibility in the family document: a form hidden in the type previewed is drawn ghosted, so it can still be
//! edited; the view control bar's Preview Visibility shows the family as placed (the form gone), with a coloured frame round the
//! view and its name over it. It is the document's own, off at first, and neither an unsaved change nor a step of its history.

use super::*;
use egui::epaint::ColorMode;
use egui::{Color32, ImageData, Shape};

/// A cabinet whose second form shows only where Show Top is ticked: in type A, not in type B.
pub(super) const CABINET: &str = r#"family {"name": "Cabinet", "category": "generic", "parameters": [{"name": "Show Top", "kind": "yesno", "group": "visibility", "value": true}], "types": {"A": {}, "B": {"Show Top": false}}, "forms": [{"profile": [[0, 0], [1000, 0], [1000, 500], [0, 500]], "to": 500}, {"profile": [[2000, 0], [3000, 0], [3000, 500], [2000, 500]], "to": 600, "visible": "Show Top"}]}"#;

/// The last whole picture handed to the harness for each texture: what a renderer would draw, kept to be read.
#[derive(Clone, Default)]
pub(super) struct Pictures(pub(super) Arc<std::sync::Mutex<Vec<(egui::TextureId, egui::ColorImage)>>>);

impl egui_kittest::TestRenderer for Pictures {
    fn handle_delta(&mut self, delta: &mut egui::TexturesDelta) {
        let mut kept = self.0.lock().unwrap();
        for (id, deltas) in &delta.set {
            for d in deltas.iter().filter(|d| d.pos.is_none()) {
                let ImageData::Color(image) = &d.image;
                kept.push((*id, (**image).clone()));
            }
        }
        delta.clear();
    }

    fn render(&mut self, _: &egui::Context, _: &egui::FullOutput) -> Result<image::RgbaImage, String> {
        Err("this renderer only keeps pictures".into())
    }
}

/// The Cabinet open in the family document, with the preview's pictures kept.
pub(super) fn cabinet() -> (Harness<'static, BimApp>, Pictures) {
    let mut s = Session::new();
    s.script(&format!("{BOX}\n{CABINET}")).unwrap();
    let pictures = Pictures::default();
    let mut h = Harness::builder()
        .with_size([1600.0, 900.0])
        .with_max_steps(32)
        .renderer(pictures.clone())
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h.state_mut().edit_family("Cabinet");
    h.run();
    (h, pictures)
}

/// The preview's last picture.
pub(super) fn preview(h: &Harness<BimApp>, pictures: &Pictures) -> egui::ColorImage {
    let named = |id: &egui::TextureId| h.ctx.tex_manager().read().meta(*id).is_some_and(|m| m.name == "family-preview");
    let kept = pictures.0.lock().unwrap();
    kept.iter().rev().find(|(id, _)| named(id)).map(|(_, p)| p.clone()).expect("the preview was drawn")
}

/// Type `name` previewed, picked in Family Types.
fn preview_type(h: &mut Harness<BimApp>, name: &str) {
    press(h, "Family Types");
    press(h, "Type Name");
    press_near(h, name, "Type Name");
    press(h, "OK");
}

/// The view control bar's Preview Visibility pressed: the lowest node of the name, the banner being over the view.
pub(super) fn toggle(h: &mut Harness<BimApp>) {
    let at = h.get_all_by_label("Preview Visibility").map(|n| n.rect().center()).max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    click_at(h, at);
}

/// Whether Preview Visibility is on in the family document.
fn previewing(h: &Harness<BimApp>) -> bool {
    h.state().family_editor.as_ref().is_some_and(|ed| ed.preview_visibility())
}

/// The colours of the closed outlines the last frame stroked.
pub(super) fn outlines(h: &Harness<BimApp>) -> Vec<(egui::Rect, Color32)> {
    fn walk(s: &Shape, out: &mut Vec<(egui::Rect, Color32)>) {
        match s {
            Shape::Rect(r) => out.push((r.rect, r.stroke.color)),
            Shape::Path(p) if p.closed => {
                if let ColorMode::Solid(c) = p.stroke.color {
                    out.push((p.visual_bounding_rect(), c));
                }
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The pixels where two pictures differ.
pub(super) fn differ(a: &egui::ColorImage, b: &egui::ColorImage) -> Vec<usize> {
    assert_eq!(a.size, b.size);
    (0..a.pixels.len()).filter(|&i| a.pixels[i] != b.pixels[i]).collect()
}

#[test]
fn a_form_hidden_in_the_type_is_ghosted_and_preview_visibility_shows_the_family_as_placed() {
    let (mut h, pictures) = cabinet();
    assert!(!previewing(&h), "off at first");
    let shown = preview(&h, &pictures);
    let ground = shown.pixels[0];
    // Type B: Show Top is clear, so its form is drawn ghosted.
    preview_type(&mut h, "B");
    let ghosted = preview(&h, &pictures);
    // Preview Visibility: as placed, the form is gone.
    toggle(&mut h);
    assert!(previewing(&h));
    let placed = preview(&h, &pictures);
    let form = differ(&ghosted, &placed);
    assert!(form.len() > 500, "the ghosted form is drawn where the placed family has nothing: {} pixels", form.len());
    assert!(form.iter().all(|&i| placed.pixels[i] == ground), "as placed, where the form was is the view's ground");
    // Ghosted: not the ground, and not what the form looks like where it is visible (type A, the same camera).
    let ghosts = form.iter().filter(|&&i| ghosted.pixels[i] != ground).count();
    assert!(ghosts * 10 > form.len() * 9, "the ghost is drawn: {ghosts} of {}", form.len());
    let unlike = form.iter().filter(|&&i| ghosted.pixels[i] != shown.pixels[i]).count();
    assert!(unlike * 10 > form.len() * 9, "the ghost is unlike the visible form: {unlike} of {}", form.len());
    // A ghost is lighter than the form drawn solid: see-through, over the ground.
    let light = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
    let lighter = form.iter().filter(|&&i| light(ghosted.pixels[i]) > light(shown.pixels[i])).count();
    assert!(lighter * 2 > form.len(), "ghosted is paler than solid: {lighter} of {}", form.len());
    // Type A again, still previewing: the form is visible, so it is drawn as it is.
    preview_type(&mut h, "A");
    let a = preview(&h, &pictures);
    assert!(form.iter().filter(|&&i| a.pixels[i] == shown.pixels[i]).count() * 10 > form.len() * 9, "type A shows the form");
    // Type B, previewing: gone again; off: ghosted again.
    preview_type(&mut h, "B");
    assert!(form.iter().all(|&i| preview(&h, &pictures).pixels[i] == ground));
    toggle(&mut h);
    assert!(!previewing(&h));
    assert_eq!(differ(&preview(&h, &pictures), &ghosted), Vec::<usize>::new(), "off, the ghost is back");
}

#[test]
fn preview_visibility_frames_the_view_and_names_itself() {
    let (mut h, _) = cabinet();
    let frame = crate::editor::PREVIEW_FRAME;
    let pane = h.state().view_rect;
    let framed = |h: &Harness<BimApp>| {
        outlines(h).iter().any(|(r, c)| *c == frame && r.width() > pane.width() * 0.9 && r.height() > pane.height() * 0.9)
    };
    assert!(!framed(&h), "no frame while it is off");
    assert_eq!(h.query_all_by_label("Preview Visibility").count(), 1, "the toggle alone");
    toggle(&mut h);
    assert!(framed(&h), "a frame round the view: {:?}", outlines(&h));
    assert_eq!(h.query_all_by_label("Preview Visibility").count(), 2, "the toggle and the banner over the view");
    let banner = h.get_all_by_label("Preview Visibility").map(|n| n.rect()).find(|r| r.min.y < pane.center().y).expect("the banner");
    assert!(pane.contains_rect(banner), "the banner is over the view");
    toggle(&mut h);
    assert!(!framed(&h));
}

#[test]
fn preview_visibility_is_no_change_to_the_family_and_no_step() {
    let (mut h, _) = cabinet();
    toggle(&mut h);
    assert!(previewing(&h));
    assert!(!h.state().family_editor.as_ref().unwrap().unsaved(), "the toggle is not saved in the family");
    // Undo: nothing to undo, so Preview Visibility stays on.
    press(&mut h, "Undo");
    assert!(previewing(&h), "the toggle is no step of the history");
    // A family opened afresh starts with it off.
    press(&mut h, "Close Family");
    h.state_mut().edit_family("Cabinet");
    h.run();
    assert!(!previewing(&h));
}
