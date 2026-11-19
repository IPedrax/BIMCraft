//! Rebar details: the sets of a beam's bars, their tags, and the drawing, through the commands every front end drives. The hand case
//! is `reinforce_tests`' beam, 200 x 500 and 6 m long, whose stirrups and closer ends are worked out there.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::model::Element;
use crate::sheet::detail::{Layout, Op, layout};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A beam 200 x 500 and 6 m long, armed with three bars of 16 below, two of 12.5 above and stirrups of 5 every 150 with 100 for
/// 600 mm at each end: 7, 31 and 7 of them (`reinforce_tests::stirrups_get_closer_near_the_ends_and_skin_bars_fill_the_web`).
fn armed(s: &mut Session) -> u64 {
    let host = run(s, "beam", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    reinforce(s, host, 100.0);
    host
}

fn reinforce(s: &mut Session, host: u64, ends: f64) {
    run(
        s,
        "reinforce",
        json!({
            "host": host, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "top": {"count": 2, "diameter": 12.5},
            "stirrup": {"diameter": 5, "spacing": 150, "ends": {"length": 600, "spacing": ends}}
        }),
    );
}

fn view(s: &mut Session, p: Value) -> Value {
    run(s, "rebar_detail", p)["views"][0].clone()
}

fn sets_of(v: &Value) -> Vec<Value> {
    v["sets"].as_array().cloned().unwrap_or_default()
}

fn laid(s: &Session, id: u64) -> Layout {
    let Some(Element::RebarDetail(rd)) = s.model.elements.get(id) else { panic!("{id} is no rebar detail") };
    layout(&s.model, rd, crate::sheet::DENSITY / rd.scale).expect("it lays out")
}

fn words(l: &Layout) -> Vec<String> {
    l.ops.iter().filter_map(|op| if let Op::Text { text, .. } = op { Some(text.clone()) } else { None }).collect()
}

#[test]
fn a_beam_with_closer_end_stirrups_has_three_sets_7_31_and_7() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let v = view(&mut s, json!({"host": host}));
    let sets = sets_of(&v);
    // Bottom bars, top bars, and the stirrups' three zones: the boundary bars go to the closer ends.
    let stirrups: Vec<&Value> = sets.iter().filter(|x| x["layer"] == "stirrup").collect();
    assert_eq!(stirrups.iter().map(|x| x["count"].as_u64().unwrap()).collect::<Vec<_>>(), [7, 31, 7], "{sets:?}");
    let steps: Vec<f64> = stirrups.iter().map(|x| x["spacing"].as_f64().unwrap()).collect();
    assert!((steps[0] - 100.0).abs() < 0.01 && (steps[1] - 4745.0 / 32.0).abs() < 0.01 && (steps[2] - 100.0).abs() < 0.01, "{steps:?}");
    assert_eq!(sets.len(), 5, "{sets:?}");
}

#[test]
fn a_hosts_sets_add_up_to_its_bars() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let v = view(&mut s, json!({"host": host}));
    let counted: u64 = sets_of(&v).iter().map(|x| x["count"].as_u64().unwrap()).sum();
    let bars = s.model.elements.values().filter(|e| matches!(e, Element::Rebar(_))).count() as u64;
    assert_eq!((counted, bars), (50, 50));
    // Each bar is in one set, once.
    let mut ids: Vec<u64> = sets_of(&v).iter().flat_map(|x| x["ids"].as_array().unwrap().iter().map(|i| i.as_u64().unwrap())).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 50);
}

#[test]
fn a_set_is_tagged_mark_count_diameter_spacing_and_length() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let v = view(&mut s, json!({"host": host}));
    let tags: Vec<String> = sets_of(&v).iter().map(|x| x["tag"].as_str().unwrap().to_string()).collect();
    // The bottom bars lie behind one another in an elevation, so they have no spacing; the stirrups spread along the beam, and the
    // zones of one stirrup share a mark.
    assert_eq!(tags, ["N1 3 ø16 C=5950", "N2 2 ø12.5 C=5950", "N3 7 ø5 c/100 C=1180", "N3 31 ø5 c/148 C=1180", "N3 7 ø5 c/100 C=1180"]);
}

#[test]
fn a_project_in_centimetres_tags_c_slash_10_and_c_equals_118() {
    let mut s = Session::new();
    let host = armed(&mut s);
    run(&mut s, "units", json!({"length": "cm"}));
    let v = view(&mut s, json!({"host": host}));
    let tags: Vec<String> = sets_of(&v).iter().map(|x| x["tag"].as_str().unwrap().to_string()).collect();
    assert!(tags.contains(&"N3 7 ø5 c/10 C=118".to_string()), "{tags:?}");
}

#[test]
fn a_beams_default_sections_are_the_middle_of_each_zone_of_stirrups() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let v = view(&mut s, json!({"host": host}));
    // 27.5 to 627.5, 775.8 to 5224.2 (the middle's 31 bars), and 5372.5 to 5972.5.
    let at: Vec<f64> = v["stations"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!(at.len(), 3, "{at:?}");
    assert!((at[0] - 327.5).abs() < 0.2 && (at[1] - 3000.0).abs() < 0.2 && (at[2] - 5672.5).abs() < 0.2, "{at:?}");
}

#[test]
fn a_second_call_keeps_the_detail_its_name_and_its_place_and_takes_what_is_given() {
    let mut s = Session::new();
    let host = armed(&mut s);
    run(&mut s, "sheet", json!({}));
    let first = run(&mut s, "rebar_detail", json!({"host": host, "sheet": "A101"}));
    let (id, name) = (first["views"][0]["id"].as_u64().unwrap(), first["views"][0]["name"].as_str().unwrap().to_string());
    assert_eq!(first["viewports"].as_array().map(Vec::len), Some(1), "{first}");
    let again = view(&mut s, json!({"host": host, "stations": [1000, 3000], "scale": 25, "list": false}));
    assert_eq!((again["id"].as_u64(), again["name"].as_str()), (Some(id), Some(name.as_str())));
    assert_eq!(
        (again["scale"].as_f64(), again["list"].as_bool(), again["stations"].as_array().map(Vec::len)),
        (Some(25.0), Some(false), Some(2))
    );
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::RebarDetail(_))).count(), 1);
    // `replace` takes it back to what a new one has.
    let back = view(&mut s, json!({"host": host, "replace": true}));
    assert_eq!((back["scale"].as_f64(), back["list"].as_bool()), (Some(50.0), Some(true)));
}

#[test]
fn a_detail_of_a_beam_with_no_bars_is_refused_saying_to_reinforce_it() {
    let mut s = Session::new();
    let host = run(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    let before = s.model.clone();
    let e = s.execute("rebar_detail", &json!({"host": host})).unwrap_err();
    assert!(e.contains("reinforce"), "{e}");
    assert_eq!(*s.model, *before);
}

#[test]
fn only_a_beam_has_a_detail_and_a_bad_ask_changes_nothing() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let wall = run(&mut s, "wall", json!({"points": [[0, 3000], [4000, 3000]]}))["ids"][0].as_u64().unwrap();
    let before = s.model.clone();
    for (p, word) in [
        (json!({"host": wall}), "beam"),
        (json!({"host": host, "scale": 0}), "scale"),
        (json!({"host": host, "stations": [1, 2, 3, 4, 5, 6, 7]}), "stations"),
        (json!({"host": host, "stations": [99999]}), "station"),
        (json!({"host": host, "stations": [-1]}), "station"),
        (json!({"host": host, "list": 5}), "list"),
        (json!({}), "required"),
    ] {
        let e = s.execute("rebar_detail", &p).unwrap_err();
        assert!(e.contains(word), "{p}: {e}");
    }
    assert_eq!(*s.model, *before);
}

#[test]
fn a_detail_goes_with_its_beam() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let id = view(&mut s, json!({"host": host}))["id"].as_u64().unwrap();
    assert!(s.model.elements.contains_key(id));
    run(&mut s, "delete", json!({"ids": [host]}));
    assert!(!s.model.elements.contains_key(id));
}

#[test]
fn a_detail_lays_out_the_tags_in_a_column_no_two_of_which_overlap() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let id = view(&mut s, json!({"host": host}))["id"].as_u64().unwrap();
    let l = laid(&s, id);
    let said = words(&l);
    for tag in ["N1 3 ø16 C=5950", "N3 31 ø5 c/148 C=1180"] {
        assert!(said.iter().any(|w| w == tag), "{tag} in {said:?}");
    }
    // Every tag, in the elevation and in the sections, is a box of its own.
    let boxes: Vec<[f32; 4]> = l
        .ops
        .iter()
        .filter_map(|op| match op {
            Op::Text { text, at, height } if text.starts_with('N') && text.contains('ø') => {
                let half = crate::font::width(text, *height) / 2.0;
                Some([at.0 - half, at.1 - height / 2.0, at.0 + half, at.1 + height / 2.0])
            }
            _ => None,
        })
        .collect();
    assert!(boxes.len() >= 5);
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1], "{a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn an_elevation_draws_the_bars_heavy_over_the_concretes_outline_and_a_zone_as_a_first_and_a_last() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let id = view(&mut s, json!({"host": host}))["id"].as_u64().unwrap();
    let l = laid(&s, id);
    let heavy = l.ops.iter().filter(|op| matches!(op, Op::Line { pen, .. } if *pen == crate::pen::Pen::HEAVY)).count();
    let medium = l.ops.iter().filter(|op| matches!(op, Op::Line { pen, .. } if *pen == crate::pen::Pen::MEDIUM)).count();
    // Three bottom bars behind one another are one line and two top bars another; each zone of stirrups is two lines, not its bars.
    assert!(heavy >= 2, "{heavy}");
    assert!(medium >= 6, "{medium}");
    assert!(medium < 45, "not every stirrup is drawn: {medium}");
}

#[test]
fn a_section_draws_each_bar_it_cuts_as_a_dot_at_its_place_and_says_its_scale() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let id = view(&mut s, json!({"host": host, "stations": [3000]}))["id"].as_u64().unwrap();
    let l = laid(&s, id);
    // Three bottom bars and two top bars.
    assert_eq!(l.ops.iter().filter(|op| matches!(op, Op::Dot { .. })).count(), 5);
    assert!(words(&l).iter().any(|w| w == "A-A   1 : 25"), "{:?}", words(&l));
}

#[test]
fn the_list_under_a_detail_totals_the_weight_of_the_steel_list_of_that_beam() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let id = view(&mut s, json!({"host": host}))["id"].as_u64().unwrap();
    let kg = run(&mut s, "steel_list", json!({}))["totals"]["weight_kg"].as_f64().unwrap();
    let said = words(&laid(&s, id));
    assert!(said.contains(&format!("{kg:.2}")), "{kg} in {said:?}");
    // Without the list there is no table.
    view(&mut s, json!({"host": host, "list": false}));
    assert!(!words(&laid(&s, id)).contains(&format!("{kg:.2}")));
}

#[test]
fn a_detail_follows_a_change_of_its_beams_bars_with_no_command() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let id = view(&mut s, json!({"host": host}))["id"].as_u64().unwrap();
    let before = words(&laid(&s, id));
    reinforce(&mut s, host, 120.0);
    let after = words(&laid(&s, id));
    assert_ne!(before, after);
    assert!(after.iter().any(|w| w.contains("c/120")), "{after:?}");
}

#[test]
fn a_detail_on_a_sheet_is_drawn_at_its_scale_and_renders() {
    let mut s = Session::new();
    let host = armed(&mut s);
    run(&mut s, "sheet", json!({}));
    let r = run(&mut s, "rebar_detail", json!({"host": host, "sheet": "A101", "scale": 50}));
    let (id, name) = (r["views"][0]["id"].as_u64().unwrap(), r["views"][0]["name"].as_str().unwrap().to_string());
    // Half the scale is half the room for the member, so the frame is narrower, though not by half: the lettering keeps its size.
    let wide = crate::sheet::size(&s.model, id, 25.0).unwrap();
    let narrow = crate::sheet::size(&s.model, id, 100.0).unwrap();
    assert!(narrow.0 < wide.0, "{narrow:?} {wide:?}");
    let sheet = s.model.sheets()[0];
    assert!(crate::sheet::draw(&s.model, sheet).is_some_and(|d| d.prims.len() > 100));
    // `render` draws it by name.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("detail.png");
    let r = run(&mut s, "render", json!({"view": name, "path": path.to_str().unwrap()}));
    assert_eq!(r["kind"], "rebar detail", "{r}");
    assert!(std::fs::metadata(&path).is_ok_and(|m| m.len() > 1000));
}

#[test]
fn a_detail_is_in_the_elements_json_and_in_the_views() {
    let mut s = Session::new();
    let host = armed(&mut s);
    let id = view(&mut s, json!({"host": host}))["id"].as_u64().unwrap();
    let j = element_json(&s.model, id);
    assert_eq!((j["host_id"].as_u64(), j["scale"].as_f64()), (Some(host), Some(50.0)), "{j}");
    assert!(s.model.views().contains(&id));
}

/// What the sweep throws at `rebar_detail`: a beam with bars is in the base, made here.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let host = armed(base);
    let many: Vec<u64> = (0..1_001).collect();
    let p = |v: Value| ("rebar_detail", v);
    vec![
        p(json!({})),
        p(json!({"host": host})),
        p(json!({"host": host, "stations": [3000]})),
        p(json!({"host": host, "stations": [f64::NAN]})),
        p(json!({"host": host, "stations": [-1]})),
        p(json!({"host": host, "stations": [1e300]})),
        p(json!({"host": host, "stations": [1, 2, 3, 4, 5, 6, 7]})),
        p(json!({"host": host, "stations": "all"})),
        p(json!({"host": host, "scale": 0})),
        p(json!({"host": host, "scale": -1})),
        p(json!({"host": host, "scale": 1e300})),
        p(json!({"host": host, "scale": "big"})),
        p(json!({"host": host, "list": 5})),
        p(json!({"host": host, "replace": "yes"})),
        p(json!({"host": host, "sheet": 1})),
        p(json!({"host": host, "sheet": "nowhere"})),
        p(json!({"host": host, "sheet": []})),
        p(json!({"hosts": many})),
        p(json!({"hosts": [u64::MAX]})),
        p(json!({"hosts": []})),
        p(json!({"host": u64::MAX})),
        p(json!({"host": 1})),
        p(json!({"host": host, "unknown": 1})),
    ]
}
