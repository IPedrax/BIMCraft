//! A family's clipboard (`family/clipboard.rs`): what Copy holds, what each Paste adds, what neither does, and its text.

use std::collections::BTreeSet;

use serde_json::json;

use crate::curves::Curve;
use crate::engine::Session;
use crate::family::clipboard::{self as clipboard, Copied, PasteRefused, SketchClip, Source};
use crate::family::constraints::{CopyRefused, Copying, DimRef, DimView, FamilyDimension};
use crate::family::{CENTER_LEFT_RIGHT, Family, PlaneRef};
use crate::geom::P2;
use crate::ids::Guid;
use crate::model::Lock;
use crate::sketch::SketchLine;

const LEFT: &str = "fe140000-0000-4000-8000-000000000001";
const RIGHT: &str = "fe140000-0000-4000-8000-000000000002";
const EDGE: &str = "fe140000-0000-4000-8000-000000000003";
const BOX: &str = "fe140000-0000-4000-8000-000000000004";
const WING: &str = "fe140000-0000-4000-8000-000000000005";
const WIDTH: &str = "fe140000-0000-4000-8000-000000000006";
const LOCK: &str = "fe140000-0000-4000-8000-000000000007";
const PLAIN: &str = "fe140000-0000-4000-8000-000000000008";

fn g(s: &str) -> Guid {
    serde_json::from_value(json!(s)).unwrap()
}

/// Planes Left and Right (x ±500) labelled Width, Right locked 500 from Edge (x 1000), a plain dimension from Center
/// (Left/Right) to Edge; a box on Ref. Level and a wing drawn on Right.
fn family() -> Family {
    let plane =
        |guid: &str, name: &str, x: f64| json!({"guid": guid, "name": name, "start": [x, -1500, 0], "end": [x, 1500, 0], "up": [0, 0, 1]});
    let f = json!({
        "name": "Clip", "parameters": [{"name": "Width", "kind": "length", "value": 1000}], "types": {"A": {}},
        "ref_planes": [plane(LEFT, "Left", -500.0), plane(RIGHT, "Right", 500.0), plane(EDGE, "Edge", 1000.0)],
        "forms": [
            {"guid": BOX, "sketch": [
                {"start": [100, 100], "end": [400, 100]}, {"start": [400, 100], "end": [400, 400]},
                {"start": [400, 400], "end": [100, 400]}, {"start": [100, 400], "end": [100, 100]}
            ], "from": 0, "to": 300},
            {"guid": WING, "host": {"plane": RIGHT}, "profile": [[0, 0], [200, 0], [200, 100], [0, 100]], "from": 0, "to": 50}
        ],
        "dimensions": [
            {"guid": WIDTH, "view": "plan", "refs": [{"plane": LEFT}, {"plane": RIGHT}], "at": [0, 1200, 0], "label": "Width"},
            {"guid": LOCK, "view": "plan", "refs": [{"plane": RIGHT}, {"plane": EDGE}], "at": [0, 1300, 0], "locked": [{"segment": 0, "length": 500}]},
            {"guid": PLAIN, "view": "plan", "refs": [{"plane": CENTER_LEFT_RIGHT}, {"plane": EDGE}], "at": [0, -1300, 0]}
        ]
    });
    let mut s = Session::new();
    s.execute("family", &f).unwrap_or_else(|e| panic!("{e}"));
    s.model.families["Clip"].clone()
}

fn planes(gs: &[&str]) -> Copying {
    Copying { planes: gs.iter().map(|s| g(s)).collect(), ..Copying::default() }
}

#[test]
fn a_clip_keeps_the_constraints_among_the_copies_and_counts_those_it_leaves() {
    let f = family();
    let clip = f.clip(&planes(&[LEFT, RIGHT])).unwrap();
    assert!(clip.model() && !clip.view_specific());
    assert_eq!(clip.count(), 2);
    // Width comes along; the lock from Right to Edge is left behind, and counted.
    assert_eq!(clip.dropped, 1);
    let pasted = f.paste(&clip).unwrap();
    assert_eq!(pasted.planes.len(), 2);
    assert_eq!(pasted.dimensions.len(), 1);
    let width = &pasted.dimensions[0];
    assert_eq!(width.label.as_deref(), Some("Width"));
    let new: BTreeSet<Guid> = pasted.planes.iter().map(|p| p.guid).collect();
    assert!(width.refs.iter().all(|r| matches!(r, DimRef::Plane(p) if new.contains(p))), "{:?}", width.refs);
    assert!(pasted.selected.is_empty(), "Width wasn't selected");
    // Each paste has GUIDs of its own, none the originals'.
    let again = f.paste(&clip).unwrap();
    let all: BTreeSet<Guid> = pasted.planes.iter().chain(&again.planes).map(|p| p.guid).collect();
    assert_eq!(all.len(), 4);
    assert!(!all.contains(&g(LEFT)) && !all.contains(&g(RIGHT)));
    assert_ne!(pasted.dimensions[0].guid, again.dimensions[0].guid);
    // Pasted into the family, every type solves and builds with them.
    let mut with = f.clone();
    with.planes.extend(pasted.planes);
    with.dimensions.extend(pasted.dimensions);
    let v = with.values("A", &Default::default()).unwrap();
    with.solve(&v).unwrap();
    with.check(&|_| true).unwrap();
}

#[test]
fn a_dimension_selected_with_what_it_measures_is_selected_in_the_paste() {
    let f = family();
    let mut c = planes(&[RIGHT, EDGE]);
    c.dimensions.push(g(LOCK));
    let clip = f.clip(&c).unwrap();
    // The lock comes along (selected and holding a constraint), Width to Left doesn't.
    assert_eq!(clip.dropped, 1);
    assert_eq!(clip.count(), 3);
    let pasted = f.paste(&clip).unwrap();
    assert_eq!(pasted.dimensions.len(), 1);
    assert_eq!(pasted.selected, vec![pasted.dimensions[0].guid]);
    assert_eq!(pasted.dimensions[0].locked, vec![Lock { segment: 0, length: 500.0 }]);
}

#[test]
fn a_copy_drawn_on_what_isnt_copied_needs_it_where_it_is_pasted() {
    let f = family();
    let wing = f.forms.iter().position(|x| x.guid == g(WING)).unwrap();
    let clip = f.clip(&Copying { forms: vec![wing], ..Copying::default() }).unwrap();
    let pasted = f.paste(&clip).unwrap();
    assert_eq!(pasted.forms[0].host, PlaneRef::Plane(g(RIGHT)), "drawn on the original's plane");
    assert_ne!(pasted.forms[0].guid, g(WING));
    // Right gone (cut, say): the wing has nothing to be drawn on.
    let mut gone = f.clone();
    gone.planes.retain(|p| p.guid != g(RIGHT));
    assert_eq!(gone.paste(&clip).unwrap_err(), PasteRefused::Host(g(RIGHT)));
    // The box, on Ref. Level, pastes anywhere: a datum is every family's.
    let boxed = f.clip(&Copying { forms: vec![0], ..Copying::default() }).unwrap();
    assert!(Family::default().paste(&boxed).is_ok());
}

#[test]
fn what_a_copy_refuses_the_clipboard_refuses() {
    let f = family();
    assert!(matches!(f.clip(&Copying { planes: vec![CENTER_LEFT_RIGHT], ..Copying::default() }), Err(CopyRefused::Datum(_))));
    assert!(f.clip(&Copying::default()).unwrap().is_empty());
}

#[test]
fn dimensions_copied_alone_are_pasted_into_views_measuring_the_same_references() {
    let f = family();
    let clip = f.clip(&Copying { dimensions: vec![g(PLAIN), g(WIDTH)], ..Copying::default() }).unwrap();
    assert!(clip.view_specific() && !clip.model());
    assert_eq!(clip.loose.len(), 2);
    // Width's label already holds in the plan: the copy measures without it, and it is counted.
    assert!(clip.loose.iter().all(|d| d.label.is_none() && d.locked.is_empty()));
    assert_eq!(clip.dropped, 1);
    assert_eq!(clip.views(), BTreeSet::from([DimView::Plan]));
    let pasted = f.paste_into_view(&clip, DimView::Ceiling).unwrap();
    assert!(pasted.iter().all(|d| d.view == DimView::Ceiling && d.guid != g(PLAIN) && d.guid != g(WIDTH)));
    let plain = &f.dimensions.iter().find(|d| d.guid == g(PLAIN)).unwrap().refs;
    assert!(pasted.iter().any(|d| d.refs == *plain), "{pasted:?}");
    let mut with = f.clone();
    with.dimensions.extend(pasted);
    with.check(&|_| true).unwrap();
    // Model elements aren't pasted into views, nor dimensions alone where they go.
    assert_eq!(f.paste(&clip).unwrap_err(), PasteRefused::Kind);
    let model = f.clip(&planes(&[LEFT])).unwrap();
    assert_eq!(f.paste_into_view(&model, DimView::Plan).unwrap_err(), PasteRefused::Kind);
    // What they measure gone: refused.
    let mut gone = f.clone();
    gone.planes.retain(|p| p.guid != g(EDGE));
    assert_eq!(gone.paste_into_view(&clip, DimView::Plan).unwrap_err(), PasteRefused::Gone);
}

fn line(a: (f64, f64), b: (f64, f64)) -> SketchLine {
    SketchLine {
        id: 7,
        guid: Guid::new(),
        curve: Curve::Poly { points: vec![P2(a.0, a.1), P2(b.0, b.1)], arcs: vec![0.0], closed: false },
        style: None,
        invisible: false,
        slope: None,
    }
}

fn lock(a: DimRef, b: DimRef) -> FamilyDimension {
    FamilyDimension {
        id: 0,
        guid: Guid::new(),
        view: DimView::Plan,
        refs: vec![a, b],
        at: [0.0; 3],
        label: None,
        locked: vec![Lock { segment: 0, length: 100.0 }],
        equal: false,
        alignment: false,
        kind: Default::default(),
    }
}

#[test]
fn a_sketchs_lines_paste_with_new_guids_and_the_locks_among_them() {
    let form = Guid::new();
    let (a, b, c) = (line((0.0, 0.0), (100.0, 0.0)), line((0.0, 100.0), (100.0, 100.0)), line((0.0, 300.0), (100.0, 300.0)));
    let s = |l: &SketchLine| DimRef::Sketch(form, l.guid);
    let among = lock(s(&a), s(&b));
    let across = lock(s(&b), s(&c));
    let plain = FamilyDimension { locked: Vec::new(), ..lock(s(&a), s(&b)) };
    let clip = SketchClip::of(form, 0, &[a.clone(), b.clone()], &[among.clone(), across, plain], &|_| false);
    assert_eq!(clip.constraints.len(), 1, "the lock between a and b; the plain dimension holds nothing");
    assert_eq!(clip.dropped, 1, "the lock from b to c, which isn't copied");
    assert_eq!(clip.centre(), Some(P2(50.0, 50.0)));
    let pasted = clip.paste(P2(1000.0, 0.0), [1000.0, 0.0, 0.0]);
    assert_eq!(pasted.lines.len(), 2);
    assert!(pasted.lines.iter().all(|l| l.guid != a.guid && l.guid != b.guid && l.id == 0));
    assert_eq!(pasted.lines[0].curve.points(), vec![P2(1000.0, 0.0), P2(1100.0, 0.0)]);
    let d = &pasted.constraints[0];
    assert_eq!(d.refs, vec![DimRef::Sketch(form, pasted.lines[0].guid), DimRef::Sketch(form, pasted.lines[1].guid)]);
    assert_ne!(d.guid, among.guid);
    assert_eq!(d.at, [1000.0, 0.0, 0.0]);
    // Twice: new GUIDs again.
    let again = clip.paste(P2(0.0, 0.0), [0.0; 3]);
    assert!(again.lines.iter().all(|l| pasted.lines.iter().all(|m| m.guid != l.guid)));
}

#[test]
fn the_text_is_marked_a_familys_and_its_source_reads_back_alone() {
    let f = family();
    let clip = f.clip(&planes(&[LEFT, RIGHT])).unwrap();
    let source = Source { family: "Clip".into(), copy: Guid::new() };
    let text = clipboard::text(&source, Copied::Elements(&clip)).unwrap();
    assert!(clipboard::is_family_clipboard(&text));
    assert!(!crate::clipboard::is_clipboard(&text), "a project's paste doesn't take it for its own");
    assert_eq!(clipboard::source(&text).unwrap(), source);
    assert!(text.contains("\"planes\"") && text.contains("\"label\":\"Width\""), "the text holds the copy: {text}");
    // A sketch's lines too.
    let line = SketchLine::new(P2(0.0, 0.0), P2(100.0, 0.0));
    let lines = SketchClip::of(Guid::new(), 0, &[line], &[], &|_| false);
    assert_eq!(clipboard::source(&clipboard::text(&source, Copied::Lines(&lines)).unwrap()).unwrap(), source);
    // Not a family's, or a newer one's: refused, saying why.
    assert!(!clipboard::is_family_clipboard("some text"));
    assert!(clipboard::source(r#"{"bimcraft":"bimcraft clipboard","version":1}"#).is_err());
    let newer = text.replacen(&format!("\"version\":{}", clipboard::VERSION), "\"version\":99", 1);
    assert!(clipboard::source(&newer).unwrap_err().contains("newer BIMCraft"));
}
