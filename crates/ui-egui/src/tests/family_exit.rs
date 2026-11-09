//! F10, the Family Editor's exit test: a parametric window drawn from blank in the family document, as a user draws it, then
//! placed in a project's wall, flexed through its types, scheduled and exported to IFC. Every step goes through the window:
//! the ribbon, the Project Browser, Properties, the Options Bar, Family Types, clicks in the views and keys.
//!
//! The window: reference planes Left and Right of the opening (plan) and Head (Front elevation), and the blank's Sill, on Ref.
//! Level and locked to it (a window's axes rise from its sill); Aligned Dimensions Left to Right labelled Width and Sill (Tab
//! picks it off Ref. Level) to Head labelled Height, with EQ about Center (Left/Right); a frame extruded on Center (Front/Back)
//! whose outer lines are picked from the planes with Pick Lines' Lock and trimmed to corners, and whose inner lines are
//! dimensioned and locked 50 mm inside them; a void extrusion, its outline Align-locked to the planes, through the wall: the
//! opening, listed after the frame and cutting the wall alone (Cuts: Host).
use super::family_views::{click_family, editor};
use super::*;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{CENTER_FRONT_BACK, CENTER_LEFT_RIGHT, DimRef, Family, PlaneRef, REF_LEVEL};
use bimcraft_core::geom::P2;
use std::collections::BTreeMap;

/// The project: four walls round a room on a level, and the four building elevations.
const PROJECT: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
elevation {}"#;

/// The family's name.
const NAME: &str = "Exit Window";

/// The types: name, Width, Height.
const TYPES: [(&str, f64, f64); 3] = [("900 x 1200", 900.0, 1200.0), ("1200 x 1500", 1200.0, 1500.0), ("600 x 600", 600.0, 600.0)];

/// The frame's members, in mm: how far inside the opening its inner lines are.
const MEMBER: f64 = 50.0;

/// The project in the window, in Rev, as large as the views need.
fn project() -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(PROJECT).unwrap();
    let mut app = BimApp::new(s);
    (app.profile, app.docking) = (crate::style::Profile::Rev, crate::dock::Docking::revit());
    let mut h = Harness::builder()
        .with_size([1600.0, 1100.0])
        .with_max_steps(32)
        .with_step_dt(0.05)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
    h.run();
    h
}

fn status(h: &Harness<BimApp>) -> String {
    h.state().status.text.clone()
}

/// The family document's view `which`, from its Project Browser.
fn view(h: &mut Harness<BimApp>, which: Option<Which>) {
    super::family_views::view(h, which);
}

/// The ribbon's button `label` (a label Properties may show too).
fn button(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

/// The draft as it stands.
fn draft(h: &Harness<BimApp>) -> Family {
    editor(h).draft_family().unwrap()
}

/// A reference plane drawn by two clicks at family points `a` and `b` in the view shown, then named `name` in Properties.
fn reference_plane(h: &mut Harness<BimApp>, a: [f64; 3], b: [f64; 3], name: &str) -> Guid {
    shortcut(h, "RP");
    assert!(editor(h).views.drawing.is_some(), "Reference Plane is on");
    click_family(h, a);
    click_family(h, b);
    let g = editor(h).planes.last().unwrap().guid;
    assert_eq!(editor(h).selected_plane(), Some(g), "selected once drawn");
    key(h, Key::Escape);
    // Selected again, and named in Properties.
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
    key(h, Key::Escape);
    click_family(h, mid);
    assert_eq!(editor(h).selected_plane(), Some(g), "picked again at {mid:?}: {}", status(h));
    type_into(h, "Name", name);
    key(h, Key::Enter);
    key(h, Key::Escape);
    assert_eq!(editor(h).planes.iter().find(|p| p.guid == g).map(|p| p.name.as_str()), Some(name));
    g
}

/// Where plane `g` is along its normal, as type `t` flexes the draft.
fn offset_in(h: &Harness<BimApp>, t: &str, g: Guid) -> f64 {
    let f = draft(h);
    let v = f.values(t, &BTreeMap::new()).unwrap();
    let flexed = f.flexed(&v).unwrap();
    let p = flexed.planes.iter().find(|p| p.guid == g).unwrap();
    let z = p.frame().z;
    p.start[0] * z[0] + p.start[1] * z[1] + p.start[2] * z[2]
}

/// An Aligned Dimension picked through references at `refs` (family points), placed at `at`, then selected: its index.
fn aligned_dimension(h: &mut Harness<BimApp>, refs: &[[f64; 3]], at: [f64; 3]) -> usize {
    let before = editor(h).dimensions.len();
    button(h, "Aligned Dimension");
    for r in refs {
        click_family(h, *r);
    }
    click_family(h, at);
    assert_eq!(editor(h).dimensions.len(), before + 1, "placed: {}", status(h));
    key(h, Key::Escape);
    before
}

#[test]
fn a_parametric_window_drawn_from_blank_is_placed_flexed_scheduled_and_exported() {
    let mut h = project();
    // New family: a Window, from File > New > Family…, as Revit's.
    new_family_of(&mut h, "Window");
    assert!(h.state().active_family().is_some(), "the family document is open");
    type_into(&mut h, "Family name", NAME);
    key(&mut h, Key::Enter);
    // The blank window's starter frame (three boxes) goes: this one is drawn.
    assert_eq!(draft(&h).forms.len(), 3);
    for _ in 0..3 {
        super::family_views::select_form(&mut h, 1);
        key(&mut h, Key::Delete);
    }
    assert!(draft(&h).forms.is_empty(), "{}", status(&h));
    // Reference planes: Left and Right of the opening in plan; Head in the Front elevation (Ref. Level is the sill).
    view(&mut h, Some(Which::Plan));
    let left = reference_plane(&mut h, [-450.0, -1500.0, 0.0], [-450.0, 1500.0, 0.0], "Left");
    let right = reference_plane(&mut h, [450.0, -1500.0, 0.0], [450.0, 1500.0, 0.0], "Right");
    // In plan, Left to Right labelled Width, and EQ about Center (Left/Right). A window family has Width and Height always
    // (the opening's size: Family Types refuses deleting them), so the dimensions are labelled with them.
    let width = aligned_dimension(&mut h, &[[-450.0, -1000.0, 0.0], [450.0, -1000.0, 0.0]], [200.0, -1300.0, 0.0]);
    label(&mut h, [200.0, -1300.0, 0.0], width, "Width");
    let eq = aligned_dimension(&mut h, &[[-450.0, -1000.0, 0.0], [0.0, -1000.0, 0.0], [450.0, -1000.0, 0.0]], [300.0, -1600.0, 0.0]);
    click_family(&mut h, [-225.0, -1600.0, 0.0]);
    assert_eq!(editor(&h).selected_dimension(), Some(editor(&h).dimensions[eq].guid));
    click_off(&mut h, [0.0, -1600.0, 0.0], vec2(0.0, -30.0));
    assert!(editor(&h).dimensions[eq].equal, "EQ on: {}", status(&h));
    key(&mut h, Key::Escape);
    // In the Front elevation, Head, and Sill to Head labelled Height: the blank window's Sill lies on Ref. Level, locked to it
    // (F10c), and Tab picks it there.
    view(&mut h, Some(Which::Elevation(Side::Front)));
    let sill = editor(&h).planes.iter().find(|p| p.name == "Sill").map(|p| p.guid).expect("the blank window's Sill");
    let head = reference_plane(&mut h, [-1500.0, 0.0, 1200.0], [1500.0, 0.0, 1200.0], "Head");
    let before = editor(&h).dimensions.len();
    button(&mut h, "Aligned Dimension");
    tab_to(&mut h, [-1300.0, 0.0, 0.0], sill);
    dump(&mut h, "sill-tab");
    click_family(&mut h, [-1300.0, 0.0, 0.0]);
    click_family(&mut h, [-1300.0, 0.0, 1200.0]);
    click_family(&mut h, [-1700.0, 0.0, 600.0]);
    assert_eq!(editor(&h).dimensions.len(), before + 1, "placed: {}", status(&h));
    key(&mut h, Key::Escape);
    let height = before;
    label(&mut h, [-1700.0, 0.0, 300.0], height, "Height");
    let dims = &editor(&h).dimensions;
    assert_eq!(dims[width].refs, vec![DimRef::Plane(left), DimRef::Plane(right)]);
    assert_eq!(dims[eq].refs, vec![DimRef::Plane(left), DimRef::Plane(CENTER_LEFT_RIGHT), DimRef::Plane(right)]);
    assert_eq!(dims[height].refs, vec![DimRef::Plane(sill), DimRef::Plane(head)]);
    assert!(dims.iter().any(|d| d.alignment && d.refs == [DimRef::Plane(REF_LEVEL), DimRef::Plane(sill)]), "Sill locked on Ref. Level");
    // The frame: an extrusion sketched on Center (Front/Back), the Front elevation's work plane: the opening's outline and
    // a member's width inside it.
    button(&mut h, "Extrusion");
    assert!(editor(&h).sketching());
    assert_eq!(editor(&h).sketch.as_ref().unwrap().host, PlaneRef::Plane(CENTER_FRONT_BACK));
    // The outline picked from the planes with Pick Lines and its Lock (F10a): Left, Right, Head and Sill (Tab picks it off Ref.
    // Level), each a line locked to its plane, then trimmed to the corners where they meet (TR).
    button(&mut h, "Pick Lines");
    h.get_by_label("Lock").click();
    h.run();
    for (at, plane) in
        [([-450.0, 0.0, 1700.0], left), ([450.0, 0.0, 1700.0], right), ([1000.0, 0.0, 1200.0], head), ([1000.0, 0.0, 0.0], sill)]
    {
        tab_to(&mut h, at, plane);
        click_family(&mut h, at);
    }
    let locked: Vec<DimRef> = editor(&h).sketch.as_ref().unwrap().locks.iter().map(|d| d.refs[0]).collect();
    assert_eq!(locked, [DimRef::Plane(left), DimRef::Plane(right), DimRef::Plane(head), DimRef::Plane(sill)], "{}", status(&h));
    dump(&mut h, "pick-lines-lock");
    shortcut(&mut h, "TR");
    for (side, across) in [(-450.0, 0.0), (450.0, 0.0), (-450.0, 1200.0), (450.0, 1200.0)] {
        click_family(&mut h, [side, 0.0, 600.0]);
        click_family(&mut h, [0.0, 0.0, across]);
    }
    // Zoomed in, as a user does to draw 50 mm inside a line without snapping onto it, then out again.
    zoom(&mut h, [0.0, 0.0, 600.0], 240.0);
    rectangle(&mut h, P2(-450.0 + MEMBER, MEMBER), P2(450.0 - MEMBER, 1200.0 - MEMBER));
    zoom(&mut h, [0.0, 0.0, 600.0], -240.0);
    assert_eq!(editor(&h).sketch.as_ref().unwrap().sketch.lines.len(), 8, "{}", status(&h));
    assert_eq!(editor(&h).sketch.as_ref().unwrap().locks.len(), 4);
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let frame = draft(&h).forms.len() - 1;
    let frame_guid = draft(&h).forms[frame].guid;
    // Its depth across the wall, in its Properties: 40 each side of the wall's middle.
    assert_eq!(editor(&h).selected_form(), Some(frame), "the form made is selected");
    type_into(&mut h, "Extrusion Start", "-40");
    key(&mut h, Key::Enter);
    type_into(&mut h, "Extrusion End", "40");
    key(&mut h, Key::Enter);
    key(&mut h, Key::Escape);
    // The inner lines a member's width inside the planes: dimensioned and locked.
    for (reference, line, at, by) in [
        ([-450.0, 0.0, 1700.0], [-450.0 + MEMBER, 0.0, 600.0], [-1000.0, 0.0, 1750.0], vec2(0.0, 16.0)),
        ([450.0, 0.0, 1700.0], [450.0 - MEMBER, 0.0, 600.0], [1000.0, 0.0, 1750.0], vec2(0.0, 16.0)),
        ([1300.0, 0.0, 0.0], [200.0, 0.0, MEMBER], [1000.0, 0.0, -300.0], vec2(16.0, 0.0)),
        ([1300.0, 0.0, 1200.0], [200.0, 0.0, 1200.0 - MEMBER], [1000.0, 0.0, 1500.0], vec2(16.0, 0.0)),
    ] {
        let i = aligned_dimension(&mut h, &[reference, line], at);
        let d = editor(&h).dimensions[i].clone();
        assert!(d.refs.iter().any(|r| matches!(r, DimRef::Sketch(f, _) if *f == frame_guid)), "{:?}", d.refs);
        assert!(d.refs.iter().any(|r| matches!(r, DimRef::Plane(_))), "{:?}", d.refs);
        let mid = [(reference[0] + line[0]) / 2.0, 0.0, (reference[2] + line[2]) / 2.0];
        let mid = if by.x == 0.0 { [mid[0], 0.0, at[2]] } else { [at[0], 0.0, mid[2]] };
        click_family(&mut h, mid);
        assert_eq!(editor(&h).selected_dimension(), Some(d.guid), "{}", status(&h));
        click_off(&mut h, mid, by);
        assert_eq!(editor(&h).dimensions[i].locked.len(), 1, "locked at {MEMBER}: {}", status(&h));
        key(&mut h, Key::Escape);
    }
    // The void: a Void Extrusion on the same plane, the opening's outline locked to the planes, through the wall.
    h.get_by_role_and_label(Role::Button, "Void Forms").click();
    h.run();
    press(&mut h, "Void Extrusion");
    assert!(editor(&h).sketching());
    rectangle(&mut h, P2(-450.0, 0.0), P2(450.0, 1200.0));
    for (reference, line) in [
        ([-450.0, 0.0, 1700.0], [-450.0, 0.0, 600.0]),
        ([450.0, 0.0, 1700.0], [450.0, 0.0, 600.0]),
        ([-1300.0, 0.0, 0.0], [200.0, 0.0, 0.0]),
        ([-1300.0, 0.0, 1200.0], [200.0, 0.0, 1200.0]),
    ] {
        lock_line(&mut h, reference, line);
    }
    button(&mut h, "Finish Edit Mode");
    assert!(!editor(&h).sketching(), "{}", status(&h));
    let void = draft(&h).forms.len() - 1;
    assert!(draft(&h).forms[void].void);
    type_into(&mut h, "Extrusion Start", "-300");
    key(&mut h, Key::Enter);
    type_into(&mut h, "Extrusion End", "300");
    key(&mut h, Key::Enter);
    // Listed after the frame, it cuts the wall all the same (F10d), and the frame it overlaps, as a void cuts the forms listed
    // before it: Properties' Cuts, Host, has it cut the wall alone.
    let values = draft(&h).values(&draft(&h).types.keys().next().unwrap().clone(), &BTreeMap::new()).unwrap();
    assert_eq!(draft(&h).host_voids(&values).unwrap().len(), 1, "after the frame, it cuts the wall");
    assert_eq!(editor(&h).selected_form(), Some(void), "the void made is selected");
    press(&mut h, "Cuts");
    press_near(&mut h, "Host", "Cuts");
    assert_eq!(draft(&h).forms[void].cuts, Some(bimcraft_core::family::Cuts::Host));
    assert!(!draft(&h).forms[0].void, "the frame stays first");
    assert_eq!(draft(&h).host_voids(&values).unwrap().len(), 1, "the void cuts the wall");
    assert_eq!(draft(&h).pieces(&values).unwrap().len(), 1, "and leaves the frame");
    key(&mut h, Key::Escape);
    // Types, in Family Types: the blank's one renamed, two more made, each its Width and Height.
    press(&mut h, "Family Types");
    press(&mut h, "Rename Type");
    type_into(&mut h, "Name:", TYPES[0].0);
    press_near(&mut h, "OK", "Name:");
    for (name, w, ht) in &TYPES[1..] {
        press(&mut h, "New Type");
        type_into(&mut h, "Name:", name);
        press_near(&mut h, "OK", "Name:");
        type_into(&mut h, "Parameter Value Width", &w.to_string());
        key(&mut h, Key::Enter);
        type_into(&mut h, "Parameter Value Height", &ht.to_string());
        key(&mut h, Key::Enter);
    }
    // The type shown, as the document's views flex it: the middle one.
    press(&mut h, "Type Name");
    press_near(&mut h, TYPES[1].0, "Type Name");
    dump(&mut h, "family-types");
    press(&mut h, "OK");
    let f = draft(&h);
    assert_eq!(f.types.len(), 3, "{:?}", f.types.keys().collect::<Vec<_>>());
    let frame = f.forms.iter().position(|x| x.guid == frame_guid).unwrap();
    // Flexed in the family: the planes and the frame follow Width and Height, Center (Left/Right) between Left and Right.
    for (name, w, ht) in TYPES {
        assert!((offset_in(&h, name, right) - offset_in(&h, name, left) - w).abs() < 1e-6, "{name}: Left to Right");
        assert!((offset_in(&h, name, right) + offset_in(&h, name, left)).abs() < 1e-6, "{name}: EQ about the centre");
        assert!((offset_in(&h, name, head) - ht).abs() < 1e-6, "{name}: Head");
        let v = f.values(name, &BTreeMap::new()).unwrap();
        let b = &f.shapes(&v)[frame];
        assert!(near(b.min, [-w / 2.0, -40.0, 0.0]) && near(b.max, [w / 2.0, 40.0, ht]), "{name}: the frame {b:?}");
        assert_eq!(f.host_voids(&v).unwrap().len(), 1, "{name}: the void cuts the wall");
    }
    // The void, picked in the Project Browser's Forms: its Properties (Void, Cuts Host) say its outline is set by this type's
    // planes, not as it was drawn at 900 x 1200; the type flexes it there, the drawn numbers staying.
    h.get_by_label("2. Extrusion (void)").scroll_to_me();
    h.run();
    press(&mut h, "2. Extrusion (void)");
    let void = f.forms.iter().position(|x| x.void).unwrap();
    assert_eq!(editor(&h).selected_form(), Some(void));
    assert_eq!(h.get_by_label("Solid/Void").value().as_deref(), Some("Void"));
    assert_eq!(h.get_by_label("Cuts").value().as_deref(), Some("Host"));
    let numbers = |f: &Family| {
        let n = |e: &bimcraft_core::family::Expr| match e {
            bimcraft_core::family::Expr::Number(x) => *x,
            other => panic!("{other:?}"),
        };
        f.forms[void].corners().unwrap().iter().map(|c| [n(&c[0]), n(&c[1])]).collect::<Vec<_>>()
    };
    let flexed = f.flexed(&f.values(TYPES[1].0, &BTreeMap::new()).unwrap()).unwrap();
    assert_eq!(numbers(&flexed), [[-600.0, 0.0], [600.0, 0.0], [600.0, 1500.0], [-600.0, 1500.0]]);
    assert_eq!(numbers(&f), [[-450.0, 0.0], [450.0, 0.0], [450.0, 1200.0], [-450.0, 1200.0]], "as drawn");
    h.get_by_label_contains("Set by its dimensions and locks for the type 1200 x 1500").scroll_to_me();
    h.run();
    // A round trip to the project mid-way: its floor plan's tab shows the project's ribbon, and the family's tab comes back
    // with the draft as it was.
    let (before, back) = (draft(&h), h.state().sheet);
    let tab = |h: &Harness<BimApp>, sheet: &dyn Fn(&Sheet) -> bool| {
        h.state().tab_rects.iter().find(|(s, _)| sheet(s)).map(|(_, r)| r.center()).expect("its tab")
    };
    let at = tab(&h, &|s| matches!(s, Sheet::Plan(_)));
    click_at(&mut h, at);
    assert!(h.state().active_family().is_none() && matches!(h.state().sheet, Sheet::Plan(_)), "the project's floor plan");
    assert!(h.query_by_label("Architecture").is_some() && h.query_by_label("Load into Project").is_none(), "the project's ribbon");
    let at = tab(&h, &|s| *s == back);
    click_at(&mut h, at);
    assert_eq!(h.state().sheet, back, "back on {}", h.state().view_name(back));
    assert_eq!(draft(&h), before, "the draft as it was");
    // The family's views, its dimensions labelled, as the window shows them.
    view(&mut h, Some(Which::Plan));
    dump(&mut h, "family-plan");
    view(&mut h, Some(Which::Elevation(Side::Front)));
    dump(&mut h, "family-front");
    // Load into Project and Close: the project's family is the draft, every dimension and lock with it.
    let drawn = draft(&h);
    press(&mut h, "Load into Project and Close");
    assert!(h.state().active_family().is_none(), "{}", status(&h));
    assert!(matches!(h.state().sheet, Sheet::Plan(_)), "back on the project's floor plan tab it was opened from");
    let loaded = h.state().session.model.families.get(NAME).cloned().expect("loaded");
    assert_eq!((loaded.dimensions.len(), loaded.forms.len(), loaded.types.len()), (drawn.dimensions.len(), 2, 3));
    // Placed in the south wall, from the Architecture tab's Window, its first type picked in the Type Selector.
    let wall = find(&h, |e| matches!(e, Element::Wall(w) if w.start == P2(0.0, 0.0)));
    tool(&mut h, "Window");
    press(&mut h, "Type Selector");
    press(&mut h, &format!("{NAME}: {}", TYPES[0].0));
    click(&mut h, P2(4000.0, 0.0));
    let windows = ids(&h, "window");
    assert_eq!(windows.len(), 1, "{}", status(&h));
    let window = windows[0];
    assert_eq!((field(&h, window, "family"), field(&h, window, "type")), (json!(NAME), json!(TYPES[0].0)));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // Flexed through its types from the Type Selector: the wall's opening and the frame measured from what is built.
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection, [window], "the window selected");
    for (k, (name, w, ht)) in TYPES.iter().enumerate() {
        if k > 0 {
            press(&mut h, "Type Selector");
            press(&mut h, &format!("{NAME}: {name}"));
        }
        assert_eq!(field(&h, window, "type"), json!(name));
        built(&h, wall, window, *w, *ht);
        if k < 2 {
            views(&mut h, &name.replace(' ', ""));
            click(&mut h, P2(4000.0, 0.0));
            assert_eq!(h.state().selection, [window], "selected again on the plan");
        }
    }
    // Back to 1200 x 1500, and a second window of 600 x 600 in the east wall, for the schedule to list.
    press(&mut h, "Type Selector");
    press(&mut h, &format!("{NAME}: {}", TYPES[1].0));
    key(&mut h, Key::Escape);
    tool(&mut h, "Window");
    press(&mut h, "Type Selector");
    press(&mut h, &format!("{NAME}: {}", TYPES[2].0));
    click(&mut h, P2(8000.0, 3000.0));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    let windows = ids(&h, "window");
    assert_eq!(windows.len(), 2, "{}", status(&h));
    let east = windows.iter().copied().find(|w| *w != window).unwrap();
    // The schedule: View > Schedules > Schedule/Quantities…, Windows; its rows the two windows with their Width and Height.
    press(&mut h, "View");
    press(&mut h, "Schedules");
    press(&mut h, "Schedule/Quantities…");
    pick(&mut h, Role::Button, "Windows");
    type_into(&mut h, "Name:", "Exit Windows");
    press(&mut h, "OK");
    let sc = ids(&h, "schedule")[0];
    assert_eq!(h.state().sheet, crate::Sheet::Table(sc));
    let Some(Element::Schedule(schedule)) = h.state().session.model.elements.get(sc).cloned() else { panic!("no schedule") };
    let table = bimcraft_core::schedule::table(&h.state().session.model, &schedule).unwrap();
    let col = |name: &str| table.columns.iter().position(|c| c.heading == name).unwrap_or_else(|| panic!("no {name}"));
    let (tc, wc, hc) = (col("Type"), col("Width"), col("Height"));
    let mut rows: Vec<(String, String, String)> =
        table.rows.iter().map(|r| (r[tc].as_str().unwrap_or_default().to_string(), cell(&r[wc]), cell(&r[hc]))).collect();
    rows.sort();
    assert_eq!(
        rows,
        [("1200 x 1500".into(), "1200".into(), "1500".into()), ("600 x 600".into(), "600".into(), "600".into())],
        "{:?}",
        table.rows
    );
    assert!(h.query_by_label("1500").is_some() && h.query_by_label("600 x 600").is_some(), "the schedule's view shows them");
    press(&mut h, "Close");
    away(&mut h);
    dump(&mut h, "schedule");
    // IFC: File > Export's IFC, as the dialog hands it a path; then read back: each window an IfcWindow of its size, filling an
    // opening that voids its wall, with its size in Qto_WindowBaseQuantities.
    let dir = std::env::var("BIMCRAFT_F10_DUMP")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join(format!("bimcraft-f10-{}", std::process::id())));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("f10-window.ifc");
    assert!(h.state_mut().save_to(&path), "{}", status(&h));
    let ifc = std::fs::read_to_string(&path).unwrap();
    check_ifc(&ifc, &[(1200.0, 1500.0), (600.0, 600.0)]);
    assert_eq!(table.ids.iter().flatten().copied().collect::<Vec<_>>(), [window, east], "a row each");
    if std::env::var("BIMCRAFT_F10_DUMP").is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A schedule's cell as it reads: a number without its decimals, or the text.
fn cell(v: &Value) -> String {
    match v {
        Value::Number(n) => n.as_f64().map_or_else(String::new, |x| format!("{x}")),
        other => other.as_str().unwrap_or_default().to_string(),
    }
}

/// The STEP file's entities, by number: each one's class and its arguments split at the top level.
fn entities(ifc: &str) -> BTreeMap<u64, (String, Vec<String>)> {
    let mut out = BTreeMap::new();
    for line in ifc.lines() {
        let Some((n, rest)) = line.strip_prefix('#').and_then(|l| l.split_once('=')) else { continue };
        let (Ok(n), Some(open)) = (n.trim().parse::<u64>(), rest.find('(')) else { continue };
        let body = rest[open + 1..].trim_end().trim_end_matches(';');
        let body = body.strip_suffix(')').unwrap_or(body);
        let (mut args, mut depth, mut quoted, mut at) = (Vec::new(), 0, false, String::new());
        for c in body.chars() {
            match c {
                '\'' => quoted = !quoted,
                '(' if !quoted => depth += 1,
                ')' if !quoted => depth -= 1,
                ',' if !quoted && depth == 0 => {
                    args.push(std::mem::take(&mut at));
                    continue;
                }
                _ => {}
            }
            at.push(c);
        }
        args.push(at);
        out.insert(n, (rest[..open].to_string(), args));
    }
    out
}

/// The IFC holds an IfcWindow of each size (`w`, `h`), each filling an IfcOpeningElement that an IfcRelVoidsElement makes
/// a void of an IfcWall, with Width and Height in its Qto_WindowBaseQuantities.
fn check_ifc(ifc: &str, sizes: &[(f64, f64)]) {
    let e = entities(ifc);
    let of = |class: &str| e.iter().filter(|(_, (c, _))| c == class).map(|(n, (_, a))| (*n, a.clone())).collect::<Vec<_>>();
    let refn = |s: &str| s.trim().trim_start_matches('#').parse::<u64>().ok();
    let num = |s: &str| s.trim().parse::<f64>().unwrap_or(f64::NAN);
    let windows = of("IFCWINDOW");
    assert_eq!(windows.len(), sizes.len(), "one IfcWindow each");
    for (w, ht) in sizes {
        let (id, args) =
            windows.iter().find(|(_, a)| num(&a[8]) == *ht && num(&a[9]) == *w).unwrap_or_else(|| panic!("no IfcWindow {w} x {ht}"));
        assert!(args[2].contains(NAME), "named after its family: {}", args[2]);
        // Its opening, and the wall it voids.
        let fills = of("IFCRELFILLSELEMENT");
        let (_, fill) = fills.iter().find(|(_, a)| refn(&a[5]) == Some(*id)).expect("an IfcRelFillsElement");
        let opening = refn(&fill[4]).unwrap();
        assert_eq!(e[&opening].0, "IFCOPENINGELEMENT");
        let voids = of("IFCRELVOIDSELEMENT");
        let (_, void) = voids.iter().find(|(_, a)| refn(&a[5]) == Some(opening)).expect("an IfcRelVoidsElement");
        assert_eq!(e[&refn(&void[4]).unwrap()].0, "IFCWALL", "the opening voids a wall");
        // The opening's body: the family's void, a tessellated body.
        let shape = refn(&e[&opening].1[6]).unwrap();
        assert!(e[&shape].1[2].contains('#'), "the opening has a body");
        // Its size, in its quantities.
        let defines = of("IFCRELDEFINESBYPROPERTIES");
        let sets: Vec<u64> = defines
            .iter()
            .filter(|(_, a)| a[4].contains(&format!("#{id})")) || a[4].contains(&format!("#{id},")))
            .filter_map(|(_, a)| refn(&a[5]))
            .collect();
        let qto =
            sets.iter().find(|s| e[s].0 == "IFCELEMENTQUANTITY" && e[s].1[2].contains("Qto_WindowBaseQuantities")).expect("its quantities");
        let quantities: Vec<(String, f64)> = e[qto].1[5]
            .trim_matches(|c| c == '(' || c == ')')
            .split(',')
            .filter_map(refn)
            .filter(|q| e[q].0 == "IFCQUANTITYLENGTH")
            .map(|q| (e[&q].1[0].trim_matches('\'').to_string(), num(&e[&q].1[3])))
            .collect();
        assert!(quantities.contains(&("Width".into(), *w)) && quantities.contains(&("Height".into(), *ht)), "{quantities:?}");
    }
}

/// The project's plan, the wall's elevation and the 3D view with the window at the type `name`, saved by [`dump`].
fn views(h: &mut Harness<BimApp>, name: &str) {
    if std::env::var("BIMCRAFT_F10_DUMP").is_err() {
        return;
    }
    key(h, Key::Escape);
    // The plan zoomed in on the window, the pointer off it so nothing shows hovered.
    let at = h.state().screen(P2(4000.0, 0.0));
    wheel(h, at, 360.0);
    away(h);
    dump(h, &format!("project-plan-{name}"));
    wheel(h, at, -360.0);
    browser_entry(h, "South");
    away(h);
    dump(h, &format!("project-elevation-{name}"));
    shortcut(h, "3D");
    away(h);
    dump(h, &format!("project-3d-{name}"));
    browser_entry(h, "Level 1");
}

/// The Project Browser's first entry `label` (a floor plan's before its ceiling plan's), not a view's tab of that name.
fn browser_entry(h: &mut Harness<BimApp>, label: &str) {
    use egui_kittest::kittest::NodeT;
    // The browser is docked on the left, under Properties; the views' tabs start right of it.
    let entry = h
        .get_all_by_label(label)
        .filter(|n| n.rect().left() < 300.0)
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .map(|n| n.accesskit_node().id());
    h.get_by(|n| Some(n.id()) == entry).click_accesskit();
    h.run();
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.5)
}

/// What the project builds of `window` in `wall`: the frame's pieces exactly `w` by `ht` over the sill, and a picture of the 3D
/// view looking north at the window, probed across its edges: the wall just outside the opening, the frame just inside it, and
/// through the frame's light.
fn built(h: &Harness<BimApp>, wall: Id, window: Id, w: f64, ht: f64) {
    let m = &h.state().session.model;
    let e = m.elements.get(window).unwrap();
    let Element::Window(o) = e else { panic!("a window") };
    let sill = o.sill;
    let pieces = m.opening_pieces(e);
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for t in pieces.iter().flat_map(|p| p.tris.iter()).flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(t[k]);
            hi[k] = hi[k].max(t[k]);
        }
    }
    assert!(near(lo, [4000.0 - w / 2.0, -40.0, sill]) && near(hi, [4000.0 + w / 2.0, 40.0, sill + ht]), "{w} x {ht}: frame {lo:?} {hi:?}");
    let scene = bimcraft_core::view3d::Scene::new(m);
    let cam = bimcraft_core::view3d::Camera {
        azimuth: 270.0,
        elevation: 0.0,
        scale: 0.5,
        target: [4000.0, 0.0, sill + ht / 2.0],
        width: 1000,
        height: 1000,
        lens: None,
    };
    let img = scene.render(&cam, &[]);
    let at = |x: f64, z: f64| {
        let (px, py, _) = cam.project([x, -200.0, z]);
        img.pick(px, py)
    };
    let (l, r, b, t, mid) = (4000.0 - w / 2.0, 4000.0 + w / 2.0, sill, sill + ht, sill + ht / 2.0);
    for (x, z, what) in [
        (l - 30.0, mid, Some(wall)),
        (r + 30.0, mid, Some(wall)),
        (4000.0, b - 30.0, Some(wall)),
        (4000.0, t + 30.0, Some(wall)),
        (l + 25.0, mid, Some(window)),
        (r - 25.0, mid, Some(window)),
        (4000.0, b + 25.0, Some(window)),
        (4000.0, t - 25.0, Some(window)),
    ] {
        assert_eq!(at(x, z), what, "{w} x {ht}: at ({x}, {z})");
    }
    for (x, z) in [(4000.0, mid), (l + MEMBER + 30.0, mid), (r - MEMBER - 30.0, mid)] {
        let seen = at(x, z);
        assert!(seen != Some(wall) && seen != Some(window), "{w} x {ht}: through the light at ({x}, {z}): {seen:?}");
    }
}

/// The window as it is, saved as `f10-<name>.png` where `BIMCRAFT_F10_DUMP` says (only then).
fn dump(h: &mut Harness<BimApp>, name: &str) {
    let Ok(folder) = std::env::var("BIMCRAFT_F10_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    h.run();
    h.render().unwrap().save(std::path::Path::new(&folder).join(format!("f10-{name}.png"))).unwrap();
}

/// The wheel turned by `delta` points at `at` on screen: in (up) or out.
fn wheel(h: &mut Harness<BimApp>, at: Pos2, delta: f32) {
    h.hover_at(at);
    h.run();
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, delta),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// The pointer moved near the view's bottom right corner, where nothing is drawn.
fn away(h: &mut Harness<BimApp>) {
    let r = h.state().view_rect;
    h.hover_at(r.right_bottom() + vec2(-150.0, -100.0));
    h.run();
}

/// The wheel turned by `delta` points over family point `p`: in (up) or out.
fn zoom(h: &mut Harness<BimApp>, p: [f64; 3], delta: f32) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    h.hover_at(at);
    h.run();
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, delta),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// A rectangle drawn with Rectangle from `a` to `b` on the plane being sketched on.
fn rectangle(h: &mut Harness<BimApp>, a: P2, b: P2) {
    button(h, "Rectangle");
    click_plane(h, a);
    click_plane(h, b);
}

/// A click at point `q` of the plane being sketched on (its own axes).
fn click_plane(h: &mut Harness<BimApp>, q: P2) {
    let ed = editor(h);
    let frame = ed.plane_set().frame(&ed.sketch.as_ref().expect("sketching").host).unwrap();
    let at = ed.views.screen_of(frame.point(q, 0.0)).expect("a view shown");
    click_at(h, at);
}

/// The pointer over family point `p`, and Tab pressed until the plane a click there takes is `g` (F10c).
fn tab_to(h: &mut Harness<BimApp>, p: [f64; 3], g: Guid) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    h.hover_at(at);
    h.run();
    for _ in 0..4 {
        let ed = editor(h);
        if ed.views.under.len() < 2 || ed.views.under.get(ed.views.cycle) == Some(&g) {
            break;
        }
        key(h, Key::Tab);
    }
}

/// Align while sketching: the reference at `reference`, then the sketch's line at `line` (family points), and its padlock.
fn lock_line(h: &mut Harness<BimApp>, reference: [f64; 3], line: [f64; 3]) {
    shortcut(h, "AL");
    assert!(editor(h).views.aligning.is_some(), "Align, while sketching");
    click_family(h, reference);
    click_family(h, line);
    let offer = editor(h).sketch_padlocks_shown().into_iter().find(|(_, w)| *w == crate::editor::sketch::Padlock::Offer);
    let (at, _) = offer.unwrap_or_else(|| panic!("a padlock offered for {line:?}: {}", status(h)));
    click_at(h, at);
    key(h, Key::Escape);
}

/// The dimension `i`, selected by a click at `on`, labelled with the type parameter `name` from the Options Bar's Label.
fn label(h: &mut Harness<BimApp>, on: [f64; 3], i: usize, name: &str) {
    click_family(h, on);
    assert_eq!(editor(h).selected_dimension(), Some(editor(h).dimensions[i].guid), "selected: {}", status(h));
    h.get_by_label("Label").click();
    h.run();
    press_near(h, name, "Label");
    assert_eq!(editor(h).dimensions[i].label.as_deref(), Some(name));
    let p = draft(h).parameters.into_iter().find(|p| p.name == name).expect("the parameter");
    assert!(!p.instance && p.kind == bimcraft_core::family::Kind::Length, "a type length: {p:?}");
    key(h, Key::Escape);
}

/// A click `by` pixels from family point `p` on screen.
fn click_off(h: &mut Harness<BimApp>, p: [f64; 3], by: egui::Vec2) {
    let at = editor(h).views.screen_of(p).expect("a view shown") + by;
    click_at(h, at);
}
