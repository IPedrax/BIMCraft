//! Text notes and their types, driven through commands as a front end drives them: the types, the fonts, the parts of
//! a note bold, italic or underlined, a file from before text types, and what is drawn (pixels of the PNG).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id, LeaderKind, Model, Note};
use crate::render::{self, View};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

fn note(s: &Session, id: Id) -> Note {
    match s.model.elements.get(id) {
        Some(Element::Text(n)) => n.clone(),
        e => panic!("{id} is {e:?}"),
    }
}

/// How many dark pixels the plan of the first level draws at 0.1 px a mm round its middle (at 1 : 100, a 5 mm capital
/// is 50 px high), and the leftmost and rightmost of them.
fn ink(m: &Model) -> (usize, u32, u32) {
    let view = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 400 };
    let d = render::draw(m, 100_000, view);
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    let (mut n, mut lo, mut hi) = (0, u32::MAX, 0);
    for (i, p) in pm.pixels().iter().enumerate() {
        if u32::from(p.red()) + u32::from(p.green()) + u32::from(p.blue()) < 300 {
            n += 1;
            let x = i as u32 % pm.width();
            (lo, hi) = (lo.min(x), hi.max(x));
        }
    }
    (n, lo, hi)
}

/// A session whose plan holds one note saying `text` at the plan's middle, at 1:100, of `type`.
fn with_note(text: &str, kind: &str, more: Value) -> (Session, Id) {
    let mut s = Session::new();
    let mut p = json!({"text": text, "at": [-3000, 500], "type": kind});
    if let (Some(p), Some(more)) = (p.as_object_mut(), more.as_object()) {
        p.extend(more.clone());
    }
    let id = id_of(&run(&mut s, "text", p));
    (s, id)
}

#[test]
fn a_project_starts_with_text_types_that_are_library_entries_and_text_type_makes_renames_and_deletes_them() {
    let mut s = Session::new();
    let listed = run(&mut s, "text_type", json!({}));
    assert_eq!(listed["types"].as_object().unwrap().keys().collect::<Vec<_>>(), ["2.5mm", "3.5mm", "5mm"]);
    let fonts: Vec<&str> = listed["fonts"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    for f in ["Arial", "IBM Plex Sans", "IBM Plex Mono", "BIMCraft Lettering"] {
        assert!(fonts.contains(&f), "{f} in {fonts:?}");
    }
    assert_eq!(listed["types"]["2.5mm"]["font"], "Arial", "the default font");
    assert!(listed["drawn_with"]["2.5mm"].is_string());
    // A new one, from another, in another font with a border.
    let t = run(&mut s, "text_type", json!({"name": "Title", "like": "5mm", "font": "ibm plex mono", "bold": true, "border": true}));
    assert_eq!(
        (t["type"]["size"].as_f64(), t["type"]["font"].as_str(), t["type"]["border"].as_bool()),
        (Some(5.0), Some("IBM Plex Mono"), Some(true))
    );
    assert!(s.model.library.id(&crate::library::Kind::TextType, "Title").is_some(), "a text type is a library entry");
    let n = id_of(&run(&mut s, "text", json!({"text": "Hall", "at": [0, 0], "type": "Title"})));
    // Its notes follow a rename; it can't go while one is of it.
    run(&mut s, "text_type", json!({"name": "Title", "rename": "Heading"}));
    assert_eq!(note(&s, n).type_name, "Heading");
    assert!(err(&mut s, "text_type", json!({"name": "Heading", "delete": true})).contains(&n.to_string()));
    // A file names it by GUID, and opens with it.
    let file = s.model.to_json().unwrap();
    let guid = s.model.library.guid(&crate::library::Kind::TextType, "Heading").unwrap().to_string();
    assert!(file.contains(&guid));
    let (opened, left_out) = Model::read(&file).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(opened.elements.get(n).map(|e| matches!(e, Element::Text(t) if t.type_name == "Heading")), Some(true));
    for (p, what) in [
        (json!({"name": "X", "font": "No Such Font Zq"}), "no font `No Such Font Zq`"),
        (json!({"name": "X", "size": 0}), "size"),
        (json!({"name": "X", "width_factor": 20}), "width factor"),
        (json!({"name": "X", "border_offset": -1}), "border offset"),
        (json!({"name": "X", "border_pen": 17}), "pen"),
        (json!({"name": "X", "like": "Nope"}), "Nope"),
        (json!({"name": "2.5mm", "rename": "3.5mm"}), "already"),
        (json!({"name": "X", "colour": "#ff0000"}), "colour"),
    ] {
        let e = err(&mut s, "text_type", p.clone());
        assert!(e.contains(what), "{p}: {e}");
    }
}

#[test]
fn a_note_is_of_a_type_and_its_runs_override_its_formatting_where_it_lets_them() {
    let mut s = Session::new();
    let n = id_of(&run(&mut s, "text", json!({"text": "Bold and plain", "at": [0, 0]})));
    assert_eq!(note(&s, n).type_name, "2.5mm", "the type a new note gets");
    assert!(err(&mut s, "text", json!({"text": "x", "at": [0, 0], "size": 3})).contains("text type"));
    assert!(err(&mut s, "text", json!({"text": "x", "at": [0, 0], "type": "Nope"})).contains("2.5mm"));
    assert!(err(&mut s, "text", json!({"text": "x", "at": [0, 0], "runs": [{"from": 0, "to": 9, "bold": true}]})).contains("run 0"));
    let runs = json!([{"from": 0, "to": 4, "bold": true}]);
    run(&mut s, "modify", json!({"id": n, "override_format": true, "runs": runs}));
    // A new type keeps them; so does new text, on what didn't change.
    run(&mut s, "modify", json!({"id": n, "type": "5mm"}));
    assert_eq!(serde_json::to_value(&note(&s, n).runs).unwrap(), runs);
    run(&mut s, "modify", json!({"id": n, "text": "Very Bold and plain"}));
    let r = note(&s, n).runs;
    assert_eq!((r[0].from, r[0].to), (5, 9), "the bold word moved along with what came before it");
    // Lines and every character count: a note in Hungarian, Greek and Cyrillic.
    let u = id_of(&run(&mut s, "text", json!({"text": "Árvíztűrő\nΣ Ж", "at": [0, 0], "runs": [{"from": 10, "to": 11, "italic": true}]})));
    assert_eq!(note(&s, u).runs[0].from, 10);
    let e = s.execute("elements", &json!({"ids": [u]})).unwrap();
    assert!(e.to_string().contains("Árvíztűrő"), "{e}");
}

#[test]
fn a_file_from_before_text_types_gives_each_note_a_type_of_its_size() {
    let mut s = Session::new();
    let a = id_of(&run(&mut s, "text", json!({"text": "A", "at": [0, 0]})));
    let b = id_of(&run(&mut s, "text", json!({"text": "B", "at": [0, 0]})));
    let c = id_of(&run(&mut s, "text", json!({"text": "C", "at": [0, 0]})));
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    // As 0.9 wrote a project: format 7, no text types, notes with their sizes (one at the default, which a note left out).
    file["format"] = json!(7);
    file.as_object_mut().unwrap().remove("text_types");
    let library = file["library"].as_object_mut().unwrap();
    library.retain(|_, v| !v.to_string().contains("text-type"));
    for (id, size) in [(a, None), (b, Some(3.5)), (c, Some(4.0))] {
        let e = file["elements"][id.to_string()].as_object_mut().unwrap();
        e.remove("type");
        if let Some(size) = size {
            e.insert("size".into(), json!(size));
        }
    }
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let kind = |id: Id| match m.elements.get(id) {
        Some(Element::Text(n)) => (n.type_name.clone(), n.size),
        _ => panic!(),
    };
    assert_eq!([kind(a), kind(b), kind(c)], [("2.5mm".into(), None), ("3.5mm".into(), None), ("4mm".into(), None)]);
    assert_eq!(m.text_types["4mm"].size, 4.0);
    assert_eq!(m.check(), Ok(()));
    assert!(m.library_in_step());
}

#[test]
fn a_note_is_drawn_in_its_types_font_size_and_weight_and_its_bold_runs_draw_heavier() {
    let (plain, _) = with_note("Heavy text", "5mm", json!({}));
    let (bold, _) = with_note("Heavy text", "5mm", json!({"override_format": true, "runs": [{"from": 0, "to": 5, "bold": true}]}));
    let (ignored, _) = with_note("Heavy text", "5mm", json!({"runs": [{"from": 0, "to": 5, "bold": true}]}));
    let (small, _) = with_note("Heavy text", "2.5mm", json!({}));
    let (p, b, i, sm) = (ink(&plain.model), ink(&bold.model), ink(&ignored.model), ink(&small.model));
    assert!(p.0 > 200, "{p:?}");
    assert!(b.0 > p.0 * 11 / 10, "bold draws heavier: {} against {}", b.0, p.0);
    assert_eq!(i, p, "runs do nothing until the note overrides its formatting");
    assert!(sm.0 < p.0 / 2 && sm.2 - sm.1 < (p.2 - p.1) * 2 / 3, "half the size: {sm:?} against {p:?}");
    // Underlined, a line under it; in the stroke lettering, other letters.
    let (under, _) = with_note("Heavy text", "5mm", json!({"override_format": true, "runs": [{"from": 0, "to": 10, "underline": true}]}));
    assert!(ink(&under.model).0 > p.0 + 100);
    let (mut lettered, n) = with_note("Heavy text", "5mm", json!({}));
    run(&mut lettered, "text_type", json!({"name": "5mm", "font": "BIMCraft Lettering"}));
    assert_ne!(ink(&lettered.model), p);
    // A border round it, which a leader starts from.
    let (mut boxed, _) = with_note("Heavy text", "5mm", json!({}));
    run(&mut boxed, "text_type", json!({"name": "5mm", "border": true, "border_offset": 2}));
    let x = ink(&boxed.model);
    assert!(x.0 > p.0 && x.1 < p.1 && x.2 > p.2, "a box round it, past its letters: {x:?} against {p:?}");
    assert!(note(&lettered, n).text == "Heavy text");
}

#[test]
fn letters_no_font_has_are_question_marks_and_every_other_one_is_drawn() {
    let (hungarian, _) = with_note("őű", "5mm", json!({}));
    // Of the last private-use plane: no font has them.
    let (unknown, _) = with_note("\u{10FFFD}\u{10FFFD}", "5mm", json!({}));
    let (mut asked, n) = with_note("??", "5mm", json!({}));
    run(&mut asked, "text_type", json!({"name": "Lettered", "font": "BIMCraft Lettering", "size": 5}));
    run(&mut asked, "modify", json!({"id": n, "type": "Lettered"}));
    assert_eq!(ink(&unknown.model), ink(&asked.model), "drawn as the lettering's question mark");
    assert_ne!(ink(&hungarian.model), ink(&unknown.model));
    assert!(ink(&hungarian.model).0 > 50);
}

#[test]
fn a_notes_frame_turns_its_layout_into_the_drawing_and_back() {
    let (s, n) = with_note("Turned\nnote", "5mm", json!({"angle": 30}));
    let view = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 400 };
    let d = render::draw(&s.model, 100_000, view);
    let n = note(&s, n);
    let f = d.note_frame(&n);
    let layout = render::note_layout(&s.model, &n);
    let (x, y) = (layout.lines[1].chars[2].x, layout.lines[1].baseline);
    let back = f.mm(f.px((x, y)));
    assert!((back.0 - x).abs() < 1e-3 && (back.1 - y).abs() < 1e-3, "{back:?}");
    assert_eq!(layout.index_at(back), layout.lines[1].chars[2].index);
}

/// The last line a note draws, past its lettering and its border: its leader.
fn leader_line(s: &Session, id: Id) -> (render::Drawing, Vec<(f32, f32)>) {
    let view = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 400 };
    let d = render::draw(&s.model, 100_000, view);
    let pts = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(id))
        .filter_map(|(p, _)| match p {
            render::Prim::Line { pts, .. } => Some(pts.clone()),
            _ => None,
        })
        .next_back()
        .unwrap_or_else(|| panic!("{id} draws no leader"));
    (d, pts)
}

/// #381: a note's leader is straight, in two segments (level out of the note for its lettering's height, then to the
/// point), or an arc bowing from that shoulder's corner.
#[test]
fn a_notes_leader_is_straight_two_segments_or_an_arc() {
    let target = [3000.0, 1500.0];
    let at = |d: &render::Drawing| d.view.px(P2(target[0], target[1]));
    // Straight: from the note's side to the point in one stroke.
    let (s, n) = with_note("Lintel", "5mm", json!({"leader": target}));
    let (d, straight) = leader_line(&s, n);
    assert_eq!(straight.len(), 2);
    assert_eq!(straight[1], at(&d));
    // Two segments: level out of the note, then to the point.
    let (s, n) = with_note("Lintel", "5mm", json!({"leader": target, "leader_kind": "two_segment"}));
    let (d, two) = leader_line(&s, n);
    assert_eq!((two.len(), two[2]), (3, at(&d)));
    assert_eq!(two[0].1, two[1].1, "the shoulder runs level");
    assert!(two[1].0 - two[0].0 > 1.0, "the shoulder runs toward the point");
    // A point the note's lettering itself stands over still gets its shoulder away from the note, never back across the
    // text it belongs to: the point sits just inside where its straight leader leaves the note, in the note's own frame.
    let from = d.view.mm(straight[0]);
    let (s, n) = with_note("Lintel", "5mm", json!({"leader": [from.0 - 200.0, 1200.0], "leader_kind": "two_segment"}));
    let (_, over) = leader_line(&s, n);
    assert_eq!(over.len(), 3);
    assert!(over[1].0 > over[0].0, "the shoulder runs away from the note");
    // Curved: an arc, in steps, bowed away from the straight line between its ends.
    let (s, n) = with_note("Lintel", "5mm", json!({"leader": target, "leader_kind": "curved"}));
    let (d, curve) = leader_line(&s, n);
    assert!(curve.len() > 3);
    assert_eq!(curve[curve.len() - 1], at(&d));
    let (a, b) = (curve[0], curve[curve.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy).max(1e-6);
    let bow = curve.iter().map(|p| ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len).fold(0.0, f32::max);
    assert!(bow > 1.0, "bowed {bow} px from the chord");
}

/// #381: a leader's kind is the note's, straight unless told, and left out of the file while it is straight.
#[test]
fn a_leader_kind_is_the_notes_and_a_straight_one_is_left_out_of_the_file() {
    let mut s = Session::new();
    let n = id_of(&run(&mut s, "text", json!({"text": "Note", "at": [0, 0], "leader": [1000, 1000]})));
    assert_eq!(note(&s, n).leader_kind, LeaderKind::Straight);
    assert!(!s.model.to_json().unwrap().contains("leader_kind"), "a straight leader writes nothing extra");
    run(&mut s, "modify", json!({"id": n, "leader_kind": "curved"}));
    assert_eq!(note(&s, n).leader_kind, LeaderKind::Curved);
    assert_eq!(serde_json::to_value(note(&s, n)).unwrap()["leader_kind"], json!("curved"));
    // A kind given before the note has a leader is kept for the one it may get.
    let m = id_of(&run(&mut s, "text", json!({"text": "Note", "at": [0, 0], "leader_kind": "two_segment"})));
    assert_eq!(note(&s, m).leader_kind, LeaderKind::TwoSegment);
    let e = err(&mut s, "text", json!({"text": "x", "at": [0, 0], "leader_kind": "zigzag"}));
    assert!(e.contains("two_segment"), "{e}");
}

#[test]
fn a_font_this_system_has_not_opens_and_is_drawn_with_what_stands_in_for_it() {
    let (mut s, n) = with_note("Elsewhere", "5mm", json!({}));
    run(&mut s, "text_type", json!({"name": "5mm", "font": "IBM Plex Sans"}));
    let plex = ink(&s.model);
    // As a project from a machine with Futura Zq names it.
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let types = file["text_types"].as_object_mut().unwrap();
    let five = types.values_mut().find(|t| t["size"] == 5.0).unwrap();
    five["font"] = json!("Futura Zq");
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty() && m.check().is_ok());
    assert_eq!(m.text_types["5mm"].font, "Futura Zq", "kept as the file has it");
    assert_eq!(ink(&m), plex, "drawn in Plex Sans, which stands in for what no one has");
    // A command may not name it, but Type Properties' OK sends it back unchanged, and that stands.
    let mut t = Session::new();
    t.model = std::sync::Arc::new(m);
    assert!(err(&mut t, "text_type", json!({"name": "3.5mm", "font": "Futura Zq"})).contains("no font"));
    run(&mut t, "text_type", json!({"name": "5mm", "font": "futura zq", "size": 6}));
    assert_eq!(t.model.text_types["5mm"].font, "Futura Zq");
    assert!(note(&s, n).type_name == "5mm");
}
