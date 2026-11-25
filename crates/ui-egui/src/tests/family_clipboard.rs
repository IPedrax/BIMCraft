//! The Clipboard panel in a family that isn't adaptive. Ctrl+C and Ctrl+X, as the window's system hands them over, put the
//! selection on the clipboard (Cut taking it away as one step) and its text on the system clipboard; Ctrl+V has the copies
//! follow the pointer in a plan or an elevation, a click putting them there as one step and Escape giving it up; Paste's
//! Aligned options put them back in the same place, or dimensions copied alone into the view shown or the views picked. In a
//! sketch the same on the lines picked. What can't be done says why, nothing changed.
use super::family_drag_move::{
    A, A_LINE, CENTER_LR, DIM, EDGE, ed, family, family_with, g, near, pick, plan, plan_of, points, shift, status, untouched,
};
use super::family_selection::command;
use super::family_views::{editor, view};
use super::*;
use crate::clipboard::{Command, PasteMode};
use crate::editor::select_box::Picked;
use bimcraft_core::family::constraints::DimView;
use bimcraft_core::family::views::{Side, Which};
use bimcraft_core::family::{DimRef, Expr};

fn click3(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    click_at(h, at);
}

fn hover3(h: &mut Harness<BimApp>, p: [f64; 3]) {
    let at = editor(h).views.screen_of(p).expect("a view shown");
    h.hover_at(at);
    h.run();
}

/// Ctrl and `k`, as the window gets Ctrl+Z.
fn ctrl(h: &mut Harness<BimApp>, k: Key) {
    h.key_press_modifiers(Modifiers::COMMAND, k);
    h.run();
}

/// Ctrl+C, Ctrl+X and Ctrl+V as the window's system hands them over: Copy, Cut, and Paste with the system clipboard's text,
/// here what the window copied last.
fn ctrl_c(h: &mut Harness<BimApp>) {
    h.event(Event::Copy);
    h.run();
}

fn ctrl_x(h: &mut Harness<BimApp>) {
    h.event(Event::Cut);
    h.run();
}

fn ctrl_v(h: &mut Harness<BimApp>) {
    let text = h.state().copied.clone().unwrap_or_default();
    h.event(Event::Paste(text));
    h.run();
}

fn paste(h: &mut Harness<BimApp>, how: PasteMode) {
    h.state_mut().family_clipboard(Command::Paste(how));
    h.run();
    h.run();
}

/// A lock from box A's east line (x 500) to Edge (x 1000), and Len labelling Center (Left/Right) to its west line.
fn constrained() -> String {
    family_with(&format!(
        r#", {{"refs": [{{"form": "{A}", "line": "{A_LINE}1"}}, {{"plane": "{EDGE}"}}], "view": "plan", "at": [700, 0, 0], "locked": [{{"segment": 0, "length": 500}}]}}, {{"refs": [{{"plane": "{CENTER_LR}"}}, {{"form": "{A}", "line": "{A_LINE}3"}}], "view": "plan", "at": [50, 0, 0], "label": "Len"}}"#
    ))
}

#[test]
fn ctrl_c_and_ctrl_v_in_plan_put_a_copy_where_the_click_is_as_one_step() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    let a = points(&h, 0);
    ctrl_c(&mut h);
    assert!(status(&h).contains("1 element"), "{}", status(&h));
    assert!(untouched(&h), "a copy changes nothing");
    ctrl_v(&mut h);
    assert!(editor(&h).pasting());
    assert!(h.state().prompt().contains("click where the copies go"), "{}", h.state().prompt());
    // The box's middle (300, 300) under the pointer: the click puts it at (1300, 1100).
    hover3(&mut h, [1300.0, 1100.0, 0.0]);
    click3(&mut h, [1300.0, 1100.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    assert_eq!(points(&h, 0), a, "the original stays");
    let d = shift(&a, &points(&h, 2));
    assert!(near(d[0], 1000.0) && near(d[1], 800.0), "{d:?}");
    assert!(f.forms[2].guid != g(A) && !f.forms[2].guid.is_nil());
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(2)], "the copy is selected");
    assert_eq!(editor(&h).steps(), 1);
    assert!(!editor(&h).pasting());
    // The clipboard pastes again, with GUIDs of its own.
    ctrl_v(&mut h);
    click3(&mut h, [1300.0, -1100.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 4, "{}", status(&h));
    assert_ne!(f.forms[3].guid, f.forms[2].guid);
    assert_eq!(editor(&h).steps(), 2);
}

#[test]
fn a_paste_in_an_elevation_moves_a_copy_up_by_its_start_and_end() {
    let mut h = plan();
    view(&mut h, Some(Which::Elevation(Side::Front)));
    pick(&mut h, &[Picked::Form(0)]);
    ctrl_c(&mut h);
    ctrl_v(&mut h);
    // The box (x 100 to 500, 0 to 300 high) seen from the front: its middle (300, 150) taken 1000 up and 2000 east.
    hover3(&mut h, [2300.0, 0.0, 1150.0]);
    click3(&mut h, [2300.0, 0.0, 1150.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    let num = |e: &Expr| match e {
        Expr::Number(x) => *x,
        _ => f64::NAN,
    };
    assert!(near(num(&f.forms[2].from), 1000.0) && near(num(&f.forms[2].to), 1300.0), "{:?} {:?}", f.forms[2].from, f.forms[2].to);
    assert!(near(shift(&points(&h, 0), &points(&h, 2))[0], 2000.0));
    assert_eq!(editor(&h).steps(), 1);
}

#[test]
fn escape_gives_a_paste_up_with_nothing_changed() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    ctrl_c(&mut h);
    ctrl_v(&mut h);
    hover3(&mut h, [1300.0, 300.0, 0.0]);
    key(&mut h, Key::Escape);
    assert!(!editor(&h).pasting());
    click3(&mut h, [1300.0, 300.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 2);
    assert!(untouched(&h));
}

#[test]
fn cut_takes_the_selection_away_as_one_step_and_undo_brings_it_back() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(1)]);
    let b = family(&h).forms[1].clone();
    ctrl_x(&mut h);
    assert_eq!(family(&h).forms.len(), 1, "{}", status(&h));
    assert_eq!(editor(&h).steps(), 1);
    // Pasted in the same place: a box where it was, a new one.
    paste(&mut h, PasteMode::SamePlace);
    let f = family(&h);
    assert_eq!(f.forms.len(), 2, "{}", status(&h));
    assert_eq!(
        f.forms[1].sketch.iter().map(|l| l.curve.clone()).collect::<Vec<_>>(),
        b.sketch.iter().map(|l| l.curve.clone()).collect::<Vec<_>>()
    );
    assert_ne!(f.forms[1].guid, b.guid);
    assert_eq!(editor(&h).steps(), 2);
    // Undo twice: the paste, then the cut, taken back.
    h.state_mut().family_undo();
    h.run();
    h.state_mut().family_undo();
    h.run();
    assert_eq!(family(&h).forms[1], b);
}

#[test]
fn cut_refuses_what_delete_refuses_and_copy_what_a_copy_refuses() {
    let mut h = plan();
    // A datum: neither copied nor taken away.
    pick(&mut h, &[Picked::Plane(g(CENTER_LR))]);
    ctrl_x(&mut h);
    assert!(!status(&h).is_empty() && h.state().status.error, "{}", status(&h));
    ctrl_c(&mut h);
    assert!(status(&h).contains("datum"), "{}", status(&h));
    assert!(untouched(&h));
    assert!(h.state().family_clip.is_none());
    // Nothing on the clipboard: Paste is greyed, saying why.
    let doc = h.state().active_doc().unwrap();
    let why = editor(&h).clipboard_blocked(Command::Paste(PasteMode::Manual), None, doc);
    assert!(why.is_some_and(|w| w.contains("Nothing has been copied")));
}

#[test]
fn a_paste_a_formula_would_have_to_change_is_refused_with_nothing_changed() {
    let script = family_with("").replace(&super::family_drag_move::square(-1000, 100, -600, 500, None), r#"[{"start": ["-W", 100], "end": ["-W / 2", 100]}, {"start": ["-W / 2", 100], "end": ["-W / 2", 500]}, {"start": ["-W / 2", 500], "end": ["-W", 500]}, {"start": ["-W", 500], "end": ["-W", 100]}]"#).replace(r#""parameters": ["#, r#""parameters": [{"name": "W", "value": 1000}, "#);
    let mut h = plan_of(&script);
    pick(&mut h, &[Picked::Form(1)]);
    ctrl_c(&mut h);
    ctrl_v(&mut h);
    // East, its x would change: refused, nothing made, still pasting.
    click3(&mut h, [250.0, 300.0, 0.0]);
    assert!(status(&h).contains("formula"), "{}", status(&h));
    assert_eq!(family(&h).forms.len(), 2);
    assert!(untouched(&h));
    // North it keeps its formulas.
    click3(&mut h, [-750.0, 1300.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 3, "{}", status(&h));
}

#[test]
fn constraints_among_the_copies_come_along_and_those_to_what_stays_are_dropped() {
    let mut h = plan_of(&constrained());
    let dims = family(&h).dimensions.len();
    // The box and Edge: the lock between them comes along, Len to Center (Left/Right) is dropped.
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(EDGE))]);
    ctrl_c(&mut h);
    ctrl_v(&mut h);
    click3(&mut h, [800.0, 1300.0, 0.0]);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    assert_eq!(f.dimensions.len(), dims + 1, "{:?}", f.dimensions);
    let lock = f.dimensions.last().unwrap();
    let edge = f.planes.last().unwrap().guid;
    assert_eq!(lock.refs, vec![DimRef::Sketch(f.forms[2].guid, f.forms[2].sketch[1].guid), DimRef::Plane(edge)]);
    assert!(status(&h).contains("1 constraint"), "{}", status(&h));
    assert!(!h.state().status.error);
    // The box alone: both are dropped, and said.
    pick(&mut h, &[Picked::Form(0)]);
    ctrl_c(&mut h);
    paste(&mut h, PasteMode::SamePlace);
    assert_eq!(family(&h).dimensions.len(), dims + 1);
    assert!(status(&h).contains("2 constraint"), "{}", status(&h));
}

#[test]
fn dimensions_copied_alone_paste_into_the_view_shown_or_the_views_picked() {
    let mut h = plan();
    pick(&mut h, &[Picked::Dimension(g(DIM))]);
    ctrl_c(&mut h);
    let doc = h.state().active_doc().unwrap();
    // Not where they go: into views.
    let held = h.state().family_clip.clone();
    assert!(editor(&h).clipboard_blocked(Command::Paste(PasteMode::Manual), held.as_ref(), doc).is_some());
    assert!(editor(&h).clipboard_blocked(Command::Paste(PasteMode::SamePlace), held.as_ref(), doc).is_some());
    view(&mut h, Some(Which::Ceiling));
    paste(&mut h, PasteMode::CurrentView);
    let f = family(&h);
    let made = f.dimensions.last().unwrap();
    assert_eq!(made.view, DimView::Ceiling, "{}", status(&h));
    assert_ne!(made.guid, g(DIM));
    assert_eq!(made.refs, f.dimensions.iter().find(|d| d.guid == g(DIM)).unwrap().refs);
    assert_eq!(editor(&h).steps(), 1);
    // Selected Views: a dialog of the views that can draw it (plans: the elevations see Edge flat or edge-on along it).
    paste(&mut h, PasteMode::SelectedViews);
    let views: Vec<Which> = editor(&h).views.paste_views.as_ref().expect("the dialog").views.iter().map(|(w, _)| *w).collect();
    assert!(views.contains(&Which::Plan) && views.contains(&Which::Ceiling), "{views:?}");
    h.get_by_label("Floor Plan: Ref. Level").click();
    h.run();
    press(&mut h, "OK");
    h.run();
    let f = family(&h);
    assert_eq!(f.dimensions.iter().filter(|d| d.refs == made.refs).count(), 3, "{}", status(&h));
    assert_eq!(editor(&h).steps(), 2);
    // Model elements don't paste into views.
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    ctrl_c(&mut h);
    let held = h.state().family_clip.clone();
    assert!(editor(&h).clipboard_blocked(Command::Paste(PasteMode::CurrentView), held.as_ref(), doc).is_some());
    assert!(editor(&h).clipboard_blocked(Command::Paste(PasteMode::PickedLevel), held.as_ref(), doc).is_some());
}

#[test]
fn aligned_to_same_place_works_from_3d_where_paste_by_the_pointer_is_greyed() {
    let mut h = plan();
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    ctrl_c(&mut h);
    view(&mut h, None);
    let doc = h.state().active_doc().unwrap();
    let held = h.state().family_clip.clone();
    assert!(editor(&h).clipboard_blocked(Command::Paste(PasteMode::Manual), held.as_ref(), doc).is_some_and(|w| w.contains("{3D}")));
    paste(&mut h, PasteMode::SamePlace);
    let f = family(&h);
    let copy = f.planes.last().unwrap();
    assert!(copy.guid != g(EDGE) && copy.start[0] == 1000.0, "{copy:?} {}", status(&h));
    assert_eq!(editor(&h).selection.items(), &[Picked::Plane(copy.guid)]);
    // Another document's clipboard is refused, saying where to paste it.
    let other = crate::editor::tabs::DocId(doc.0 + 1);
    assert!(editor(&h).clipboard_blocked(Command::Paste(PasteMode::SamePlace), held.as_ref(), other).is_some_and(|w| w.contains("Movers")));
}

#[test]
fn the_ribbons_paste_drops_its_aligned_options_and_one_pastes() {
    let mut h = plan();
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    press(&mut h, "Copy to Clipboard");
    assert!(h.state().family_clip.is_some(), "{}", status(&h));
    press(&mut h, "Paste");
    press(&mut h, "Aligned to Same Place");
    for _ in 0..30 {
        h.step();
    }
    h.run();
    assert_eq!(family(&h).planes.iter().filter(|p| p.start[0] == 1000.0).count(), 2, "{}", status(&h));
}

#[test]
fn in_a_sketch_lines_paste_into_it_with_the_locks_among_them() {
    let mut h = plan();
    ed(&mut h).edit_form_sketch(0).unwrap();
    h.run();
    let form = ed(&mut h).sketch.as_ref().unwrap().guid;
    let (l0, l1, l2) = {
        let s = &ed(&mut h).sketch.as_ref().unwrap().sketch;
        (s.lines[0].guid, s.lines[1].guid, s.lines[2].guid)
    };
    // A lock between lines 0 and 1, and one from 1 to 2.
    let lock = |a, b| bimcraft_core::family::FamilyDimension {
        id: 0,
        guid: bimcraft_core::ids::Guid::new(),
        view: DimView::Plan,
        refs: vec![DimRef::Sketch(form, a), DimRef::Sketch(form, b)],
        at: [0.0; 3],
        label: None,
        locked: vec![bimcraft_core::model::Lock { segment: 0, length: 400.0 }],
        equal: false,
        alignment: false,
        kind: Default::default(),
    };
    {
        let s = ed(&mut h).sketch.as_mut().unwrap();
        s.dims.push(lock(l0, l2));
        s.dims.push(lock(l1, l2));
        s.sketch.picked = [0, 2].into();
    }
    h.run();
    let n = ed(&mut h).sketch.as_ref().unwrap().sketch.lines.len();
    let dims = ed(&mut h).sketch.as_ref().unwrap().dims.len();
    ctrl_c(&mut h);
    assert!(status(&h).contains("2 line"), "{}", status(&h));
    ctrl_v(&mut h);
    assert!(h.state().prompt().contains("click where the copies go"), "{}", h.state().prompt());
    // Escape gives it up; then again, and a click: the two lines' middle (300, 300) put at (300, 1300).
    key(&mut h, Key::Escape);
    assert!(!editor(&h).pasting());
    assert_eq!(ed(&mut h).sketch.as_ref().unwrap().sketch.lines.len(), n);
    ctrl_v(&mut h);
    hover3(&mut h, [300.0, 1300.0, 0.0]);
    click3(&mut h, [300.0, 1300.0, 0.0]);
    let s = ed(&mut h).sketch.as_ref().unwrap();
    assert_eq!(s.sketch.lines.len(), n + 2, "{}", status(&h));
    assert_eq!(s.sketch.picked, [n, n + 1].into(), "the copies picked");
    let made = &s.sketch.lines[n];
    assert!(made.guid != l0 && !made.guid.is_nil());
    let p = made.curve.points();
    assert!(near(p[0].0, 100.0) && near(p[0].1, 1100.0), "{p:?}");
    // The lock between them came along, naming the copies; the one to line 1 was dropped, and said.
    assert_eq!(s.dims.len(), dims + 1);
    assert_eq!(
        s.dims.last().unwrap().refs,
        vec![DimRef::Sketch(form, s.sketch.lines[n].guid), DimRef::Sketch(form, s.sketch.lines[n + 1].guid)]
    );
    assert!(status(&h).contains("1 constraint"), "{}", status(&h));
    // Ctrl+Z, the sketch's own, takes the paste back in one go.
    ctrl(&mut h, Key::Z);
    assert_eq!(ed(&mut h).sketch.as_ref().unwrap().sketch.lines.len(), n);
    // Elements on the clipboard don't paste into a sketch.
    let doc = h.state().active_doc().unwrap();
    let held = Some(crate::editor::clipboard::Held::Elements {
        doc,
        family: "Movers".into(),
        copy: bimcraft_core::ids::Guid::new(),
        clip: Default::default(),
    });
    assert!(editor(&h).clipboard_blocked(Command::Paste(PasteMode::Manual), held.as_ref(), doc).is_some());
}

#[test]
fn find_lists_the_clipboards_commands_and_runs_them_on_the_next_frame() {
    let mut h = plan();
    let listed = h.state().family_find_actions();
    for (name, _) in crate::editor::clipboard::FIND {
        assert!(listed.contains(&name), "{name}");
    }
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    command(&mut h, "Copy to Clipboard");
    assert!(h.state().family_clip.is_some(), "{}", status(&h));
    command(&mut h, "Aligned to Same Place");
    // The system clipboard asked for its text, which a harness never gives: the window's own copy once it has waited.
    for _ in 0..30 {
        h.step();
    }
    h.run();
    assert_eq!(family(&h).planes.iter().filter(|p| p.start[0] == 1000.0).count(), 2, "{}", status(&h));
}

/// The Clipboard panel with Paste dropped down, and a paste following the pointer in plan, saved where
/// `BIMCRAFT_FAMILY_CLIPBOARD_DUMP` says (only then).
#[test]
fn family_clipboard_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_CLIPBOARD_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let mut h = app(&constrained());
    h.set_size(vec2(1600.0, 1000.0));
    h.state_mut().edit_family("Movers");
    h.run();
    view(&mut h, Some(Which::Plan));
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("clipboard-{name}.png"))).unwrap();
    };
    pick(&mut h, &[Picked::Form(0), Picked::Plane(g(EDGE))]);
    ctrl_c(&mut h);
    // The panel, its caption under it; then Paste dropped down, its list over the caption.
    save(&mut h, "clipboard-panel");
    press(&mut h, "Paste");
    save(&mut h, "paste-aligned");
    key(&mut h, Key::Escape);
    ctrl_v(&mut h);
    hover3(&mut h, [1300.0, -900.0, 0.0]);
    save(&mut h, "pasting");
    click3(&mut h, [1300.0, -900.0, 0.0]);
    save(&mut h, "pasted");
    // In a sketch: the lines following the pointer.
    ed(&mut h).edit_form_sketch(1).unwrap();
    h.run();
    ed(&mut h).sketch.as_mut().unwrap().sketch.picked = (0..4).collect();
    ctrl_c(&mut h);
    ctrl_v(&mut h);
    hover3(&mut h, [-800.0, 1100.0, 0.0]);
    save(&mut h, "sketch-pasting");
}

#[test]
fn select_previous_after_a_paste_takes_the_originals_and_after_cut_and_undo_what_was_cut() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    ctrl_c(&mut h);
    ctrl_v(&mut h);
    click3(&mut h, [1300.0, 1100.0, 0.0]);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(2)], "{}", status(&h));
    // Select Previous: the box copied, and again the copy.
    command(&mut h, "Select Previous");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)]);
    command(&mut h, "Select Previous");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(2)]);
    // Cut box B, as Delete takes it: undone, Select Previous takes what was selected before it (the copy, followed through
    // the cut and the undo to its place, 3rd again), as after a Delete undone.
    pick(&mut h, &[Picked::Form(1)]);
    ctrl_x(&mut h);
    assert_eq!(family(&h).forms.len(), 2, "{}", status(&h));
    h.state_mut().family_undo();
    h.run();
    assert_eq!(family(&h).forms.len(), 3);
    command(&mut h, "Select Previous");
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(2)], "{}", status(&h));
    assert_ne!(family(&h).forms[2].guid, g(A), "the copy, not box A");
}

#[test]
fn a_box_leaves_a_hidden_form_out_and_so_does_the_copy() {
    let mut h = plan();
    // Box B hidden for now; a window round both boxes takes A alone, and Ctrl+C copies A alone.
    pick(&mut h, &[Picked::Form(1)]);
    command(&mut h, "Temporary Hide Element");
    super::family_box_select::boxed(&mut h, [-1100.0, -50.0, 0.0], [600.0, 700.0, 0.0], Modifiers::NONE);
    assert_eq!(editor(&h).selection.items(), &[Picked::Form(0)], "{}", status(&h));
    ctrl_c(&mut h);
    assert!(status(&h).contains("1 element"), "{}", status(&h));
    paste(&mut h, PasteMode::SamePlace);
    let f = family(&h);
    assert_eq!(f.forms.len(), 3, "{}", status(&h));
    assert_eq!(
        f.forms[2].sketch.iter().map(|l| l.curve.clone()).collect::<Vec<_>>(),
        f.forms[0].sketch.iter().map(|l| l.curve.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn a_copy_puts_its_text_on_the_system_clipboard_and_ctrl_v_pastes_this_windows_copy() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    h.event(Event::Copy);
    // The frame that copies: what it asks the window's system to do.
    h.step();
    assert!(h.state().family_clip.is_some(), "{}", status(&h));
    let wrote: Vec<String> = h
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(wrote.len(), 1, "the system clipboard has the copy's text");
    assert!(bimcraft_core::family::clipboard::is_family_clipboard(&wrote[0]));
    assert!(!bimcraft_core::clipboard::is_clipboard(&wrote[0]), "not a project's");
    assert_eq!(h.state().copied.as_deref(), Some(wrote[0].as_str()));
    h.run();
    // That text: this window's copy, following the pointer.
    ctrl_v(&mut h);
    assert!(editor(&h).pasting(), "{}", status(&h));
    key(&mut h, Key::Escape);
    // Text of anything else's (the user's, a project's): this window's copy too.
    h.event(Event::Paste("some text of the user's".into()));
    h.run();
    assert!(editor(&h).pasting());
    click3(&mut h, [1300.0, 1100.0, 0.0]);
    assert_eq!(family(&h).forms.len(), 3, "{}", status(&h));
    // A family's copy made in another window: refused, naming its family, nothing changed.
    let source = bimcraft_core::family::clipboard::Source { family: "Elsewhere".into(), copy: bimcraft_core::ids::Guid::new() };
    let clip = bimcraft_core::family::clipboard::Clip::default();
    let other = bimcraft_core::family::clipboard::text(&source, bimcraft_core::family::clipboard::Copied::Elements(&clip)).unwrap();
    h.event(Event::Paste(other));
    h.run();
    assert!(!editor(&h).pasting());
    assert!(status(&h).contains("Elsewhere") && h.state().status.error, "{}", status(&h));
    assert_eq!(family(&h).forms.len(), 3);
}

#[test]
fn cut_in_a_revolves_sketch_leaves_the_axis_line_it_doesnt_copy() {
    let mut h = plan();
    ed(&mut h).create_form(bimcraft_core::family::forms::FormKind::Revolve, false);
    h.run();
    {
        let s = ed(&mut h).sketch.as_mut().expect("a sketch");
        let mut axis = bimcraft_core::sketch::SketchLine::new(P2(0.0, -500.0), P2(0.0, 500.0));
        axis.style = Some(crate::editor::sketch::AXIS.to_string());
        s.sketch.lines = vec![bimcraft_core::sketch::SketchLine::new(P2(100.0, 0.0), P2(300.0, 0.0)), axis];
        s.sketch.picked = [0, 1].into();
    }
    h.run();
    ctrl_x(&mut h);
    let s = &ed(&mut h).sketch.as_ref().unwrap().sketch;
    assert_eq!(s.lines.len(), 1, "{}", status(&h));
    assert_eq!(s.lines[0].style.as_deref(), Some(crate::editor::sketch::AXIS), "the Axis Line stays");
}

#[test]
fn a_projects_copy_made_since_isnt_taken_for_the_familys() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    ctrl_c(&mut h);
    // A project's clipboard text, as a project tab's Copy writes it (marked as the project's kind).
    let text = format!(r#"{{"bimcraft":"{}","version":1}}"#, bimcraft_core::clipboard::KIND);
    h.event(Event::Paste(text));
    h.run();
    assert!(!editor(&h).pasting());
    assert!(status(&h).contains("project's elements") && h.state().status.error, "{}", status(&h));
}

#[test]
fn a_paste_asked_on_another_tab_is_given_up() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    ctrl_c(&mut h);
    // Asked on the project's tab (no document), the answer coming while the family's is shown: neither pastes.
    let text = h.state().copied.clone().unwrap();
    h.state_mut().paste_asked = Some((PasteMode::SamePlace, 0.0, None));
    h.event(Event::Paste(text));
    h.run();
    assert!(h.state().paste_asked.is_none());
    assert_eq!(family(&h).forms.len(), 2, "{}", status(&h));
    assert!(!editor(&h).pasting());
}

#[test]
fn ctrl_v_in_3d_pastes_where_the_copies_were() {
    let mut h = plan();
    pick(&mut h, &[Picked::Plane(g(EDGE))]);
    ctrl_c(&mut h);
    view(&mut h, None);
    ctrl_v(&mut h);
    let f = family(&h);
    assert_eq!(f.planes.iter().filter(|p| p.start[0] == 1000.0).count(), 2, "{}", status(&h));
    assert!(!editor(&h).pasting());
}

#[test]
fn a_paste_following_the_pointer_draws_its_copies_once_as_the_pointer_moves() {
    let mut h = plan();
    pick(&mut h, &[Picked::Form(0)]);
    ctrl_c(&mut h);
    ctrl_v(&mut h);
    hover3(&mut h, [1300.0, 1100.0, 0.0]);
    let built = editor(&h).views.cache_builds;
    assert!(built >= 1);
    for x in [900.0, 1100.0, 1500.0, 1700.0] {
        hover3(&mut h, [x, 1100.0, 0.0]);
    }
    assert_eq!(editor(&h).views.cache_builds, built, "the pointer only moves what was drawn");
    // The draft changed (a paste made): drawn again for it.
    click3(&mut h, [1300.0, 1100.0, 0.0]);
    ctrl_v(&mut h);
    hover3(&mut h, [1300.0, -900.0, 0.0]);
    assert!(editor(&h).views.cache_builds > built);
}
