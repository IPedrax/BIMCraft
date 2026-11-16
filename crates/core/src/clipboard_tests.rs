//! Copy to Clipboard, Cut and Paste, driven through commands as a front end drives them.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    match s.execute(cmd, &p) {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {p}: {e}"),
    }
}

fn err(s: &mut Session, cmd: &str, p: Value) -> String {
    s.execute(cmd, &p).err().unwrap_or_else(|| panic!("{cmd} {p} was accepted"))
}

fn ids(v: &Value) -> Vec<u64> {
    v["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

/// A box of walls on Level 1 (100000; Level 2 is 100001, 3000 up), and a door in the first.
fn house(s: &mut Session) -> (Vec<u64>, u64) {
    let walls = ids(&run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true})));
    let door = run(s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap_or_default();
    (walls, door)
}

fn walls_on(s: &Session, level: u64) -> Vec<crate::model::Wall> {
    let g = s.model.guid(level);
    s.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Wall(w) if w.level == g => Some(w.clone()),
            _ => None,
        })
        .collect()
}

fn doors(s: &Session) -> Vec<crate::model::Opening> {
    s.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Door(o) => Some(o.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn walls_pasted_aligned_to_selected_levels_bring_their_doors_and_stand_as_tall_where_no_level_tops_them() {
    let mut s = Session::new();
    let (walls, _) = house(&mut s);
    let copied = run(&mut s, "copy_to_clipboard", json!({"ids": walls, "view": 100000}));
    assert_eq!(copied["count"], 5, "four walls and the door in one: {copied}");
    let pasted = run(&mut s, "paste", json!({"mode": "levels", "levels": ["Level 2"]}));
    assert_eq!(ids(&pasted).len(), 5, "{pasted}");
    let up = walls_on(&s, 100001);
    assert_eq!(up.len(), 4);
    // No level over Level 2: they stand unconnected, 3000 tall, as they were.
    assert!(up.iter().all(|w| w.top_level.is_none() && (w.height - 3000.0).abs() < 1e-6), "{up:?}");
    assert_eq!(up[0].start, crate::geom::P2(0.0, 0.0));
    let marks: Vec<String> = doors(&s).into_iter().map(|o| o.mark).collect();
    assert_eq!(marks.len(), 2);
    assert_ne!(marks[0], marks[1], "a pasted door is marked anew: {marks:?}");
    // One paste is one undo step.
    run(&mut s, "undo", json!({}));
    assert!(walls_on(&s, 100001).is_empty());
}

#[test]
fn a_paste_by_hand_puts_the_middle_of_what_was_copied_where_it_is_clicked() {
    let mut s = Session::new();
    let (walls, _) = house(&mut s);
    run(&mut s, "copy_to_clipboard", json!({"ids": walls, "view": 100000}));
    let pasted = run(&mut s, "paste", json!({"view": 100000, "at": [20000, 3000]}));
    assert_eq!(ids(&pasted).len(), 5);
    let mut xs: Vec<f64> = walls_on(&s, 100000).iter().flat_map(|w| [w.start.0, w.end.0]).collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!((xs.first().copied(), xs.last().copied()), (Some(0.0), Some(24000.0)), "the copy runs 16000 to 24000");
    // Into Level 2's plan by hand: onto Level 2.
    run(&mut s, "paste", json!({"view": 100001, "by": [0, 0]}));
    assert_eq!(walls_on(&s, 100001).len(), 4);
}

#[test]
fn a_door_copied_without_its_wall_is_pasted_into_a_wall_there_or_not_at_all() {
    let mut s = Session::new();
    let (walls, door) = house(&mut s);
    run(&mut s, "copy_to_clipboard", json!({"ids": [door], "view": 100000}));
    let pasted = run(&mut s, "paste", json!({"view": 100000, "by": [3000, 0]}));
    let made = ids(&pasted);
    assert_eq!(made.len(), 1);
    let Some(Element::Door(o)) = s.model.elements.get(made[0]) else { panic!("no door") };
    assert_eq!((o.host, o.offset), (s.model.guid(walls[0]), 5000.0), "in the same wall, 3000 along");
    // Nowhere near a wall: refused, saying why.
    let e = err(&mut s, "paste", json!({"view": 100000, "by": [0, 3000]}));
    assert!(e.contains("no wall"), "{e}");
}

#[test]
fn a_detail_line_goes_to_the_same_place_between_plans_and_by_the_views_own_terms_into_a_section() {
    let mut s = Session::new();
    house(&mut s);
    let line = run(&mut s, "detail_line", json!({"view": 100000, "start": [1000, 1000], "end": [3000, 1000]}))["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [line], "view": 100000}));
    let pasted = run(&mut s, "paste", json!({"mode": "current_view", "view": 100001}));
    let Some(Element::DetailLine(l)) = s.model.elements.get(ids(&pasted)[0]) else { panic!("no line") };
    assert_eq!(l.view, s.model.guid(100001u64));
    assert_eq!(l.curve.points(), vec![crate::geom::P2(1000.0, 1000.0), crate::geom::P2(3000.0, 1000.0)]);
    // A section along x from 0 to 8000 at y 3000: the plan's coordinates as the section's own.
    let sec = run(&mut s, "section", json!({"start": [0, 3000], "end": [8000, 3000]}))["id"].as_u64().unwrap();
    let pasted = run(&mut s, "paste", json!({"mode": "current_view", "view": sec}));
    let Some(Element::DetailLine(l)) = s.model.elements.get(ids(&pasted)[0]) else { panic!("no line") };
    assert_eq!(l.view, s.model.guid(sec));
    assert_eq!(l.curve.points()[0], crate::geom::P2(1000.0, 1000.0));
    // Model elements can't go onto a sheet, nor a plan's line into a 3D view.
    let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
    let pasted = run(&mut s, "paste", json!({"mode": "current_view", "view": sheet}));
    assert_eq!(ids(&pasted).len(), 1, "a line goes onto a sheet");
}

#[test]
fn a_note_keeps_its_place_on_the_model_between_parallel_sections() {
    let mut s = Session::new();
    house(&mut s);
    let a = run(&mut s, "section", json!({"start": [0, 3000], "end": [8000, 3000]}))["id"].as_u64().unwrap();
    let b = run(&mut s, "section", json!({"start": [2000, 5000], "end": [9000, 5000]}))["id"].as_u64().unwrap();
    let note = run(&mut s, "text", json!({"text": "Lintel", "at": [4000, 2100], "view": a}))["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [note], "view": a}));
    let pasted = run(&mut s, "paste", json!({"mode": "views", "views": [b]}));
    let Some(Element::Text(n)) = s.model.elements.get(ids(&pasted)[0]) else { panic!("no note") };
    assert_eq!((n.view, n.at), (s.model.guid(b), crate::geom::P2(2000.0, 2100.0)), "x 4000 is 2000 along b");
}

#[test]
fn another_project_pastes_the_clipboards_text_with_the_types_it_hasnt() {
    let mut a = Session::new();
    run(&mut a, "type", json!({"category": "wall", "name": "Brick 230", "thickness": 230}));
    let w = ids(&run(&mut a, "wall", json!({"points": [[0, 0], [5000, 0]], "type": "Brick 230"})));
    let text = run(&mut a, "copy_to_clipboard", json!({"ids": w}))["clipboard"].as_str().unwrap_or_default().to_string();
    assert!(crate::clipboard::is_clipboard(&text));
    let mut b = Session::new();
    let pasted = run(&mut b, "paste", json!({"clipboard": text, "mode": "same_place"}));
    let Some(Element::Wall(wall)) = b.model.elements.get(ids(&pasted)[0]) else { panic!("no wall") };
    assert_eq!((wall.type_name.as_str(), wall.level), ("Brick 230", b.model.guid(100000u64)));
    assert!(b.model.types.wall.contains_key("Brick 230"));
}

#[test]
fn cut_takes_them_away_and_paste_puts_them_back() {
    let mut s = Session::new();
    let (walls, _) = house(&mut s);
    let cut = run(&mut s, "cut_to_clipboard", json!({"ids": walls}));
    assert_eq!(cut["count"], 5);
    assert!(walls_on(&s, 100000).is_empty() && doors(&s).is_empty());
    run(&mut s, "paste", json!({"mode": "same_place"}));
    assert_eq!((walls_on(&s, 100000).len(), doors(&s).len()), (4, 1));
}

#[test]
fn a_dimension_without_what_it_measures_and_what_cant_be_copied_are_left_out_saying_why() {
    let mut s = Session::new();
    house(&mut s);
    let e = err(&mut s, "copy_to_clipboard", json!({"ids": [100000]}));
    assert!(e.contains("level"), "{e}");
    let e = err(&mut s, "paste", json!({"mode": "same_place"}));
    assert!(e.contains("empty"), "{e}");
    let e = err(&mut s, "paste", json!({"clipboard": "{\"bimcraft\": \"clipboard\"}", "mode": "same_place"}));
    assert!(e.contains("no BIMCraft"), "{e}");
    let e = err(
        &mut s,
        "paste",
        json!({"clipboard": "{\"bimcraft\": \"bimcraft clipboard\", \"version\": 99, \"source\": {\"project\": 1, \"ids\": []}, \"project\": {}}", "mode": "same_place"}),
    );
    assert!(e.contains("newer"), "{e}");
}

/// The hostile sweep's payloads for `paste` and the copies: a real clipboard of the base's house and its notes, pasted every
/// way, into every kind of view, with places and lists it can't have; and clipboards damaged or of another kind.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let all: Vec<u64> = base.model.elements.keys().copied().collect();
    let text = base.execute("copy_to_clipboard", &json!({"ids": all, "view": 100000})).map_or(Value::Null, |v| v["clipboard"].clone());
    let s = text.as_str().unwrap_or_default();
    let cut = s.get(..s.len() / 2).unwrap_or_default();
    let mut out = vec![
        ("copy_to_clipboard", json!({"ids": all, "view": "x"})),
        ("cut_to_clipboard", json!({"ids": all})),
        ("cut_to_clipboard", json!({"ids": [100000, 100002]})),
        ("paste", json!({"clipboard": cut, "mode": "same_place"})),
        ("paste", json!({"clipboard": s.replace("bimcraft clipboard", "x"), "mode": "same_place"})),
        ("paste", json!({"clipboard": 5, "mode": "same_place"})),
        ("paste", json!({"clipboard": text, "mode": "levels", "levels": [100000, "x", 100002, null]})),
        ("paste", json!({"clipboard": text, "mode": "levels", "levels": "Level 1"})),
        ("paste", json!({"clipboard": text, "mode": "views", "views": [100000, 100002, "x"]})),
        ("paste", json!({"clipboard": text, "mode": "picked_level", "level": 100002})),
        ("paste", json!({"clipboard": text, "mode": "manual", "view": 100000, "at": [1e300, 0]})),
        ("paste", json!({"clipboard": text, "mode": "manual", "view": 100000, "by": [f64::MAX, f64::MAX]})),
        ("paste", json!({"clipboard": text, "view": 100000, "at": [5e8, -5e8]})),
        ("paste", json!({"clipboard": text, "mode": "zap"})),
    ];
    for view in base.model.elements.keys().copied().collect::<Vec<_>>() {
        if crate::clipboard::Frame::of(&base.model, view).is_some() {
            out.push(("paste", json!({"clipboard": text, "mode": "current_view", "view": view})));
            out.push(("paste", json!({"clipboard": text, "view": view, "by": [100, 100]})));
            out.push(("paste", json!({"clipboard": text, "mode": "views", "views": [view]})));
        }
    }
    out.push(("paste", json!({"clipboard": text, "mode": "same_place"})));
    out.push(("paste", json!({"clipboard": text, "mode": "levels", "levels": ["Level 1", "Level 2"]})));
    out
}

#[test]
fn what_a_mode_cant_place_is_listed_with_the_reason() {
    let mut s = Session::new();
    let (walls, _) = house(&mut s);
    let line = run(&mut s, "detail_line", json!({"view": 100000, "start": [1000, 1000], "end": [3000, 1000]}))["id"].as_u64().unwrap();
    let mut both = walls.clone();
    both.push(line);
    run(&mut s, "copy_to_clipboard", json!({"ids": both, "view": 100000}));
    // Onto a level: the walls go, the line (which is drawn in a view) is named as left out.
    let pasted = run(&mut s, "paste", json!({"mode": "levels", "levels": ["Level 2"]}));
    let why = pasted["left_out"].to_string();
    assert!(why.contains("drawn in a view"), "{why}");
    // Into a view: the line goes, the walls are named.
    let pasted = run(&mut s, "paste", json!({"mode": "views", "views": [100001]}));
    assert_eq!(ids(&pasted).len(), 1);
    assert!(pasted["left_out"].to_string().contains("model element"), "{pasted}");
}

#[test]
fn a_paste_by_hand_in_a_3d_view_goes_onto_the_level_given_with_its_middle_at_the_point() {
    let mut s = Session::new();
    let (walls, _) = house(&mut s);
    let cam = run(&mut s, "camera", json!({"eye": [-5000, -5000, 1700], "target": [4000, 3000, 1000]}))["id"].as_u64().unwrap_or_default();
    run(&mut s, "copy_to_clipboard", json!({"ids": walls, "view": 100000}));
    let pasted = run(&mut s, "paste", json!({"view": cam, "at": [20000, 3000], "level": "Level 2"}));
    assert_eq!(ids(&pasted).len(), 5, "{pasted}");
    let up = walls_on(&s, 100001);
    assert_eq!(up.len(), 4, "onto Level 2");
    assert!(up.iter().any(|w| w.start == crate::geom::P2(16000.0, 0.0)), "the middle at the point: {up:?}");
    // Without one, on their own levels.
    let pasted = run(&mut s, "paste", json!({"view": cam, "at": [-20000, 3000]}));
    assert_eq!(ids(&pasted).len(), 5, "{pasted}");
    assert_eq!(walls_on(&s, 100000).len(), 8);
    // The default 3D view by its name, made by the paste where nothing has made it yet.
    assert!(s.model.default_3d().is_none());
    let pasted = run(&mut s, "paste", json!({"view": "{3D}", "at": [0, 20000], "level": "Level 1"}));
    assert_eq!(ids(&pasted).len(), 5, "{pasted}");
    assert!(s.model.default_3d().is_some(), "{{3D}} made");
    // A plan's paste is on the plan's level: no `level` beside it.
    let e = err(&mut s, "paste", json!({"view": 100000, "at": [0, 0], "level": "Level 2"}));
    assert!(e.contains("3D"), "{e}");
}
