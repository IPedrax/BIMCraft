//! F7c: angular dimensions in a family's views. Angular Dimension between a reference plane and a reference line in plan,
//! labelled from the Options Bar with an Angle parameter, its padlock, and the rig flexing through its types (the line and
//! the fin on its face turning); Create Parameter… making an Angle; and Radial Dimension on an arc.
use super::family_views::{click_family, editor, open, view};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{DimKind, DimRef, Face, Family};
use bimcraft_core::ids::Guid;
use std::collections::BTreeMap;

const RIG: &str = "66666666-6666-4666-8666-000000000001";
const FIN: &str = "77777777-7777-4777-8777-000000000001";
const ARCH: &str = "99999999-9999-4999-8999-000000000001";
const CURVE: &str = "99999999-9999-4999-8999-000000000002";
const CFB: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a2";
const CLR: &str = "9e3c0a10-1f0b-4d8c-8a5e-0000000000a3";

/// The rig: a reference line on Ref. Level from the origin, 1000 long, drawn at 30°, its start locked to both centre planes;
/// a fin drawn on its vertical face; an arch (an arc turning 90° from (-1500, -1000) to (-500, -1000), closed by a straight
/// line); Angle 30 (A30) or 90 (B90); `dims`, more dimensions as JSON.
fn family(dims: &str) -> String {
    let (c, s) = (30f64.to_radians().cos() * 1000.0, 30f64.to_radians().sin() * 1000.0);
    format!(
        r#"family {{"name": "Rig", "category": "generic", "parameters": [{{"name": "Angle", "kind": "angle", "value": 30}}, {{"name": "R", "value": 707.1067811865476}}], "types": {{"A30": {{"Angle": 30, "R": 707.1067811865476}}, "B90": {{"Angle": 90, "R": 600}}}}, "ref_lines": [{{"guid": "{RIG}", "start": [0, 0, 0], "end": [{c}, {s}, 0]}}], "forms": [{{"guid": "{FIN}", "host": {{"line": "{RIG}", "face": "vertical"}}, "sketch": [{{"start": [200, 0], "end": [800, 0]}}, {{"start": [800, 0], "end": [800, 300]}}, {{"start": [800, 300], "end": [200, 300]}}, {{"start": [200, 300], "end": [200, 0]}}], "from": -25, "to": 25}}, {{"guid": "{ARCH}", "sketch": [{{"guid": "{CURVE}", "start": [-1500, -1000], "end": [-500, -1000], "turn": 90}}, {{"start": [-500, -1000], "end": [-1500, -1000]}}], "to": 100}}], "dimensions": [{{"refs": [{{"plane": "{CLR}"}}, {{"line": "{RIG}", "point": "start"}}], "at": [0, 0, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}, {{"refs": [{{"plane": "{CFB}"}}, {{"line": "{RIG}", "point": "start"}}], "at": [0, 0, 0], "locked": [{{"segment": 0, "length": 0}}], "alignment": true}}{dims}]}}"#
    )
}

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

fn in_plan(dims: &str) -> Harness<'static, BimApp> {
    let (mut h, _) = open(&family(dims), "Rig");
    view(&mut h, Some(Which::Plan));
    h
}

fn ed_mut<'a>(h: &'a mut Harness<'_, BimApp>) -> &'a mut crate::editor::FamilyEditor {
    h.state_mut().family_editor.as_mut().unwrap()
}

fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

/// The rig's line as type `t` flexes the draft.
fn line_end(h: &Harness<BimApp>, t: &str) -> [f64; 3] {
    let f = editor(h).draft_family().unwrap();
    let v = f.values(t, &BTreeMap::new()).unwrap();
    let flexed: Family = f.flexed(&v).unwrap().into_owned();
    flexed.lines[0].end
}

/// The point on screen `r` pixels from where the rig's lines cross, at `deg` degrees (counter-clockwise in plan).
fn round_the_origin(h: &Harness<BimApp>, r: f32, deg: f32) -> egui::Pos2 {
    let v = editor(h).views.screen_of([0.0, 0.0, 0.0]).unwrap();
    v + vec2(deg.to_radians().cos(), -deg.to_radians().sin()) * r
}

#[test]
fn angular_dimension_measures_a_line_from_a_plane_and_its_label_turns_the_rig() {
    let mut h = in_plan("");
    press(&mut h, "Angular");
    assert_eq!(editor(&h).views.dim_kind, DimKind::Angular);
    // Center (Front/Back)'s trace, west of the origin; then the line; then where the arc goes, between them.
    click_family(&mut h, [-700.0, 0.0, 0.0]);
    click_family(&mut h, [433.0, 250.0, 0.0]);
    assert_eq!(editor(&h).views.dimensioning.as_ref().map(Vec::len), Some(2), "{}", status(&h));
    click_family(&mut h, [580.0, 155.0, 0.0]);
    let d = editor(&h).dimensions.last().cloned().unwrap();
    assert_eq!(d.kind, DimKind::Angular, "{}", status(&h));
    assert_eq!(d.refs, vec![DimRef::Plane(g(CFB)), DimRef::Line(g(RIG), Face::Vertical)]);
    let f = editor(&h).draft_family().unwrap();
    let m = f.measured(&f.values("A30", &BTreeMap::new()).unwrap()).unwrap();
    assert!((m.last().unwrap()[0] - 30.0).abs() < 1e-6, "it measures 30°: {m:?}");
    // Escape; a click on its arc selects it, and the Options Bar's Label offers the Angle parameters.
    key(&mut h, Key::Escape);
    let r = (editor(&h).views.screen_of([580.0, 155.0, 0.0]).unwrap() - editor(&h).views.screen_of([0.0; 3]).unwrap()).length();
    {
        let at = round_the_origin(&h, r, 15.0);
        click_at(&mut h, at);
    }
    assert_eq!(editor(&h).selected_dimension(), Some(d.guid), "{}", status(&h));
    // Its padlock, inside the arc: locked at 30°, then unlocked.
    {
        let at = round_the_origin(&h, r - 20.0, 15.0);
        click_at(&mut h, at);
    }
    assert_eq!(
        editor(&h).dimensions.last().unwrap().locked,
        vec![bimcraft_core::model::Lock { segment: 0, length: 30.0 }],
        "{}",
        status(&h)
    );
    {
        let at = round_the_origin(&h, r - 20.0, 15.0);
        click_at(&mut h, at);
    }
    assert!(editor(&h).dimensions.last().unwrap().locked.is_empty());
    h.get_by_label("Label").click();
    h.run();
    assert!(h.query_all_by_label("R").next().is_none() || h.query_by_label("Angle").is_some(), "lengths aren't offered");
    press_near(&mut h, "Angle", "Label");
    assert_eq!(editor(&h).dimensions.last().unwrap().label.as_deref(), Some("Angle"), "{}", status(&h));
    // Flexed: A30 keeps the line at 30°, B90 turns it a quarter, the fin on its face with it.
    let end = line_end(&h, "B90");
    assert!(end[0].abs() < 1e-6 && (end[1] - 1000.0).abs() < 1e-6, "{end:?}");
    ed_mut(&mut h).preview_type("B90");
    h.run();
    let f = editor(&h).draft_family().unwrap();
    let flexed = f.flexed(&editor(&h).values()).unwrap();
    let tris = flexed.pieces(&editor(&h).values()).unwrap();
    let fin = tris.iter().flat_map(|p| p.tris.iter()).flatten().filter(|p| p[1] > 150.0 && p[2] > 1.0);
    assert!(fin.clone().count() > 0 && fin.clone().all(|p| p[0].abs() <= 25.0 + 1e-6), "the fin stands on the line, along y");
    // Create Parameter… makes an Angle with the angle it measures.
    ed_mut(&mut h).preview_type("A30");
    key(&mut h, Key::Escape);
    ed_mut(&mut h).radial_tool(DimKind::Angular);
    h.run();
    click_family(&mut h, [0.0, 700.0, 0.0]);
    click_family(&mut h, [433.0, 250.0, 0.0]);
    click_family(&mut h, [300.0, 520.0, 0.0]);
    let second = editor(&h).dimensions.last().cloned().unwrap();
    assert_eq!(second.refs, vec![DimRef::Plane(g(CLR)), DimRef::Line(g(RIG), Face::Vertical)], "{}", status(&h));
    key(&mut h, Key::Escape);
    ed_mut(&mut h).views.dim = Some(second.guid);
    h.run();
    h.get_by_label("Label").click();
    h.run();
    press_near(&mut h, "Create Parameter…", "Label");
    type_into(&mut h, "Name", "Rest");
    pick(&mut h, Role::CheckBox, "Reporting Parameter");
    press_near(&mut h, "OK", "Name");
    assert!(editor(&h).views.creating.is_none(), "{}", status(&h));
    let f = editor(&h).draft_family().unwrap();
    let rest = f.parameters.iter().find(|p| p.name == "Rest").unwrap();
    assert_eq!(rest.kind, bimcraft_core::family::Kind::Angle);
    assert_eq!(editor(&h).values().get("Rest"), Some(&bimcraft_core::family::Val::Number(60.0)), "it reports 90° less the angle");
}

#[test]
fn radial_dimension_picks_an_arc_and_its_label_sets_the_turn() {
    let mut h = in_plan("");
    press(&mut h, "Radial");
    // The arc's edge, at its middle (below its chord: it turns counter-clockwise from its start), then where it goes.
    click_family(&mut h, [-1000.0, -1207.1067811865476, 0.0]);
    click_family(&mut h, [-1000.0, -1600.0, 0.0]);
    let d = editor(&h).dimensions.last().cloned().unwrap();
    assert_eq!((d.kind, d.refs.clone()), (DimKind::Radial, vec![DimRef::Sketch(g(ARCH), g(CURVE))]), "{}", status(&h));
    // Labelled R: B90's 600 turns the arc between its ends, which nothing holds, least.
    ed_mut(&mut h).set_label(d.guid, Some("R".into())).unwrap();
    let f = editor(&h).draft_family().unwrap();
    let m = f.measured(&f.values("B90", &BTreeMap::new()).unwrap()).unwrap();
    assert!((m.last().unwrap()[0] - 600.0).abs() < 1e-6, "{m:?}");
    // Not past the arc's ends: a click where the whole circle would be picks nothing.
    click_family(&mut h, [-1000.0, -292.89, 0.0]);
    assert!(editor(&h).views.dimensioning.as_ref().is_some_and(Vec::is_empty), "{}", status(&h));
}

/// The rig at 30° and turned to 90° in plan with its angle selected; its fin in the front elevation; the arch's radius; and an
/// angle being placed. Saved where `BIMCRAFT_F7C_DUMP` says (only then).
#[test]
fn family_angles_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_F7C_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let dims = format!(
        r#", {{"kind": "angular", "refs": [{{"plane": "{CFB}"}}, {{"line": "{RIG}", "face": "vertical"}}], "at": [580, 155, 0], "label": "Angle"}}, {{"kind": "radial", "refs": [{{"form": "{ARCH}", "line": "{CURVE}"}}], "at": [-700, -1700, 0], "label": "R"}}"#
    );
    let mut h = app(&family(&dims));
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Rig");
    h.run();
    view(&mut h, Some(Which::Plan));
    h.run();
    // The pointer off the drawing, so it lies over nothing shown.
    let save = |h: &mut Harness<BimApp>, name: &str| {
        deliver(h, vec![Event::PointerMoved(pos2(1590.0, 600.0))]);
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(name)).unwrap();
    };
    let r = (editor(&h).views.screen_of([580.0, 155.0, 0.0]).unwrap() - editor(&h).views.screen_of([0.0; 3]).unwrap()).length();
    {
        let at = round_the_origin(&h, r, 15.0);
        click_at(&mut h, at);
    }
    save(&mut h, "f7c-plan-30.png");
    ed_mut(&mut h).preview_type("B90");
    h.run();
    {
        let at = round_the_origin(&h, r, 45.0);
        click_at(&mut h, at);
    }
    save(&mut h, "f7c-plan-90.png");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    save(&mut h, "f7c-front-90.png");
    view(&mut h, Some(Which::Plan));
    ed_mut(&mut h).preview_type("A30");
    key(&mut h, Key::Escape);
    ed_mut(&mut h).radial_tool(DimKind::Angular);
    h.run();
    click_family(&mut h, [0.0, 700.0, 0.0]);
    click_family(&mut h, [433.0, 250.0, 0.0]);
    let hover = editor(&h).views.screen_of([250.0, 600.0, 0.0]).unwrap();
    deliver(&mut h, vec![Event::PointerMoved(hover)]);
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join("f7c-placing.png")).unwrap();
}
