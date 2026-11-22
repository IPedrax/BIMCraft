//! FE-2: Shift uses the project's orthogonal constraint in family views.
use super::family_views::{editor, open, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{CENTER_FRONT_BACK, PlaneRef, REF_LEVEL};

const FAMILY: &str = r#"family {"name":"Ortho","types":{"T":{}},"forms":[{"profile":[[400,300],[1400,300],[1400,900],[400,900]],"to":1500}],"ref_planes":[{"name":"Edge","start":[-400,-2000,0],"end":[-400,2000,0],"up":[0,0,1]}]}"#;

fn button(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

fn click_mod(h: &mut Harness<BimApp>, pos: Pos2, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
    }
    h.run();
}

fn sketch_point(h: &Harness<BimApp>, q: P2) -> Pos2 {
    let ed = editor(h);
    let frame = ed.plane_set().frame(&ed.sketch.as_ref().unwrap().host).unwrap();
    let p = frame.point(q, 0.0);
    match ed.views.shown {
        Some(_) => ed.views.screen_of(p).unwrap(),
        None => ed.drawn_3d().unwrap().screen(h.state().view_rect, p),
    }
}

#[test]
fn family_ortho_shift_constrains_sketch_lines_in_plan_elevation_and_3d() {
    for which in [Some(Which::Plan), Some(Which::Elevation(Side::Front)), None] {
        let (mut h, _) = open(FAMILY, "Ortho");
        view(&mut h, which);
        let host = if matches!(which, Some(Which::Elevation(_))) { CENTER_FRONT_BACK } else { REF_LEVEL };
        h.state_mut().active_family_mut().unwrap().views.set_work_plane(PlaneRef::Plane(host));
        button(&mut h, "Extrusion");
        button(&mut h, "Line");
        for (q, modifiers) in [
            (P2(630.0, 450.0), Modifiers::NONE),
            (P2(1130.0, 590.0), Modifiers::SHIFT),
            (P2(1290.0, 1190.0), Modifiers::SHIFT),
            (P2(630.0, 900.0), Modifiers::NONE),
        ] {
            let pos = sketch_point(&h, q);
            click_mod(&mut h, pos, modifiers);
        }
        let sketch = &editor(&h).sketch.as_ref().unwrap().sketch;
        assert_eq!(sketch.lines.len(), 3, "{which:?}: three chained lines");
        let a = &sketch.lines[0].curve;
        let b = &sketch.lines[1].curve;
        assert!((a.start().1 - a.end().1).abs() < 1e-6, "{which:?}: horizontal: {a:?}");
        assert!((b.start().0 - b.end().0).abs() < 1e-6, "{which:?}: vertical: {b:?}");
        let c = &sketch.lines[2].curve;
        assert!(
            (c.start().0 - c.end().0).abs() > 50.0 && (c.start().1 - c.end().1).abs() > 50.0,
            "{which:?}: releasing Shift restores free drawing: {c:?}"
        );
    }
}

#[test]
fn family_ortho_shift_leaves_other_shapes_and_an_arcs_third_point_free() {
    use bimcraft_core::draw::Shape;
    for shape in [Shape::Rectangle, Shape::Circle, Shape::Inscribed, Shape::Circumscribed, Shape::Arc, Shape::CentreArc] {
        let mut drawings = Vec::new();
        for shifted in [false, true] {
            let (mut h, _) = open(FAMILY, "Ortho");
            view(&mut h, Some(Which::Plan));
            button(&mut h, "Extrusion");
            h.state_mut().active_family_mut().unwrap().sketch.as_mut().unwrap().tool = Some(shape);
            h.run();
            for (i, q) in
                [P2(-900.0, -700.0), P2(-200.0, -450.0), P2(-550.0, -150.0)].into_iter().take(shape.clicks(&Default::default())).enumerate()
            {
                // An arc's end can be constrained; its third point must stay free.
                let shift = shifted && i > 0 && (shape != Shape::Arc || i == 2);
                let pos = sketch_point(&h, q);
                click_mod(&mut h, pos, if shift { Modifiers::SHIFT } else { Modifiers::NONE });
            }
            drawings.push(editor(&h).sketch.as_ref().unwrap().sketch.lines.iter().map(|l| l.curve.clone()).collect::<Vec<_>>());
        }
        assert!(!drawings[0].is_empty(), "{shape:?}: the control draws a shape");
        assert_eq!(drawings[0], drawings[1], "{shape:?}: Shift must leave this construction free");
    }
}

#[test]
fn family_ortho_shift_constrains_reference_planes_in_plan_and_elevation() {
    for which in [Which::Plan, Which::Elevation(Side::Front)] {
        let (mut h, _) = open(FAMILY, "Ortho");
        view(&mut h, Some(which));
        let before = editor(&h).planes.len();
        shortcut(&mut h, "RP");
        let (a, b) = match which {
            Which::Plan => ([-900.0, -700.0, 0.0], [-200.0, -450.0, 0.0]),
            _ => ([-900.0, 0.0, 300.0], [-200.0, 0.0, 550.0]),
        };
        let pos = editor(&h).views.screen_of(a).unwrap();
        click_mod(&mut h, pos, Modifiers::NONE);
        let pos = editor(&h).views.screen_of(b).unwrap();
        click_mod(&mut h, pos, Modifiers::SHIFT);
        assert_eq!(editor(&h).planes.len(), before + 1, "a plane was drawn");
        let plane = editor(&h).planes.last().unwrap();
        let axis = if which == Which::Plan { 1 } else { 2 };
        assert!((plane.start[axis] - plane.end[axis]).abs() < 1e-6, "{which:?}: {plane:?}");
    }
}

#[test]
fn family_ortho_shift_constrains_a_reference_plane_drag_from_its_origin() {
    let (mut h, _) = open(FAMILY, "Ortho");
    view(&mut h, Some(Which::Plan));
    super::family_views::click_family(&mut h, [-400.0, -1000.0, 0.0]);
    let guid = editor(&h).selected_plane().unwrap();
    let a = editor(&h).views.screen_of([-400.0, 0.0, 0.0]).unwrap();
    let b = editor(&h).views.screen_of([-100.0, 600.0, 0.0]).unwrap();
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::SHIFT });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT });
    h.run();
    let plane = editor(&h).planes.iter().find(|p| p.guid == guid).unwrap();
    assert!((plane.start[0] + 400.0).abs() < 1e-6, "vertical Shift drag must not move the vertical plane sideways: {plane:?}");
}

#[test]
fn family_ortho_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FE2_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for profile in [crate::style::Profile::Rev, crate::style::Profile::BimCraft] {
        for (name, theme, light_canvas) in [
            ("light", egui::ThemePreference::Light, false),
            ("dark", egui::ThemePreference::Dark, false),
            ("mixed", egui::ThemePreference::Dark, true),
        ] {
            let mut h = harness(FAMILY, 0.25, profile);
            h.set_size(vec2(1600.0, 1000.0));
            h.ctx.options_mut(|o| o.theme_preference = theme);
            h.state_mut().light_canvas = light_canvas;
            h.state_mut().edit_family("Ortho");
            h.run();
            view(&mut h, Some(Which::Plan));
            button(&mut h, "Extrusion");
            button(&mut h, "Line");
            let pos = sketch_point(&h, P2(630.0, 450.0));
            click_mod(&mut h, pos, Modifiers::NONE);
            let pos = sketch_point(&h, P2(1130.0, 590.0));
            h.event(Event::ModifiersChanged(Modifiers::SHIFT));
            h.hover_at(pos);
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe2-{profile:?}-{name}-plan.png"))).unwrap();
            if profile == crate::style::Profile::Rev && name == "light" {
                h.event(Event::ModifiersChanged(Modifiers::NONE));
                view(&mut h, None);
                let pos = sketch_point(&h, P2(1130.0, 590.0));
                h.event(Event::ModifiersChanged(Modifiers::SHIFT));
                h.hover_at(pos);
                h.run();
                h.render().unwrap().save(std::path::Path::new(&folder).join("fe2-Rev-light-3d.png")).unwrap();
                h.event(Event::ModifiersChanged(Modifiers::NONE));
                button(&mut h, "Cancel Edit Mode");
                view(&mut h, Some(Which::Elevation(Side::Front)));
                h.state_mut().active_family_mut().unwrap().views.set_work_plane(PlaneRef::Plane(CENTER_FRONT_BACK));
                button(&mut h, "Extrusion");
                button(&mut h, "Line");
                let pos = sketch_point(&h, P2(630.0, 450.0));
                click_mod(&mut h, pos, Modifiers::NONE);
                let pos = sketch_point(&h, P2(1130.0, 590.0));
                h.event(Event::ModifiersChanged(Modifiers::SHIFT));
                h.hover_at(pos);
                h.run();
                h.render().unwrap().save(std::path::Path::new(&folder).join("fe2-Rev-light-elevation.png")).unwrap();
            }
        }
    }
}
