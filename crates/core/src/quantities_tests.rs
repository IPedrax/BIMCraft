//! Quantities: what a member comes to in concrete, formwork and steel, through the commands every front end drives. Each rule has a
//! hand case: the numbers are worked out here on paper and the model is held to them.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn made(s: &mut Session, id: &str, p: Value) -> u64 {
    let r = run(s, id, p);
    r["id"].as_u64().or_else(|| r["ids"].as_array().and_then(|a| a.first()).and_then(Value::as_u64)).unwrap()
}

fn near(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

/// `(volume_m3, formwork_m2)` of element `id`.
fn take(s: &Session, id: u64) -> (Option<f64>, Option<f64>) {
    let j = element_json(&s.model, id);
    (j["volume_m3"].as_f64(), j["formwork_m2"].as_f64())
}

fn beam_type(s: &mut Session, name: &str, width: u32, depth: u32) {
    run(s, "type", json!({"category": "beam", "name": name, "width": width, "depth": depth, "material": "Concrete"}));
}

#[test]
fn a_beam_with_nothing_around_it_is_its_section_by_its_length() {
    let mut s = Session::new();
    beam_type(&mut s, "B 300 x 600", 300, 600);
    let b = made(&mut s, "beam", json!({"points": [[0, 0], [5000, 0]], "type": "B 300 x 600"}));
    // 0.3 x 0.6 x 5 m, formed on its bottom and its two sides: 0.3 x 5 + 2 x 0.6 x 5.
    let (v, f) = take(&s, b);
    assert!(near(v, 0.9) && near(f, 7.5), "{v:?} {f:?}");
}

#[test]
fn a_round_beam_is_formed_all_round() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "beam", "name": "Round 400", "width": 400, "round": true, "material": "Concrete"}));
    let b = made(&mut s, "beam", json!({"points": [[0, 0], [5000, 0]], "type": "Round 400"}));
    let (v, f) = take(&s, b);
    // pi x 0.2^2 x 5 and pi x 0.4 x 5, to a thousandth and a hundredth.
    let pi = std::f64::consts::PI;
    assert!(near(v, (pi * 0.04 * 5.0 * 1e3).round() / 1e3) && near(f, (pi * 0.4 * 5.0 * 1e2).round() / 1e2), "{v:?} {f:?}");
}

#[test]
fn a_steel_beam_has_no_concrete_volume() {
    let mut s = Session::new();
    let b = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "IPE 300"}));
    let j = element_json(&s.model, b);
    assert!(j.get("volume_m3").is_none() && j.get("formwork_m2").is_none(), "its weight covers it: {j}");
    let r = run(&mut s, "schedule", json!({"name": "Steel", "category": "beam", "fields": ["Id", "Volume", "Formwork Area", "Weight"]}));
    let row = &r["rows"][0];
    // A schedule shows a number the member has none of as 0.
    assert!(near(row[1].as_f64(), 0.0) && near(row[2].as_f64(), 0.0) && row[3].as_f64().is_some_and(|w| w > 0.0), "{row}");
}

#[test]
fn a_concrete_t_beam_is_counted_by_its_profile_area() {
    let mut s = Session::new();
    let t = json!({"category": "beam", "name": "T", "shape": "T", "width": 1500, "depth": 1000, "web": 400, "flange": 500,
                   "material": "Concrete"});
    run(&mut s, "type", t);
    let b = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "type": "T"}));
    let j = element_json(&s.model, b);
    // The section's area (cm²) over 10,000 is m²; 6 m of it is the volume. A profile is formed by its whole perimeter.
    let want = j["section_area_cm2"].as_f64().unwrap() * 6.0 / 1e4;
    assert!((j["volume_m3"].as_f64().unwrap() - want).abs() < 1e-3, "{j}");
    assert!(j["formwork_m2"].as_f64().is_some_and(|f| f > 0.0), "{j}");
}

#[test]
fn a_footing_is_formed_on_its_sides_only() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "footing", "name": "F 1000", "width": 1000, "depth": 1000}));
    let f = made(&mut s, "footing", json!({"at": [0, 0], "thickness": 500, "type": "F 1000"}));
    // 1 x 1 x 0.5 m, and its four sides 4 x 1 x 0.5; it is cast on the soil.
    let (v, a) = take(&s, f);
    assert!(near(v, 0.5) && near(a, 2.0), "{v:?} {a:?}");
}

#[test]
fn a_strip_footing_is_formed_on_its_two_sides() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "stripfooting", "name": "S 600 x 400", "width": 600, "depth": 400}));
    let f = made(&mut s, "strip_footing", json!({"points": [[0, 0], [5000, 0]], "type": "S 600 x 400"}));
    // 0.6 x 0.4 x 5 m, and two sides of 0.4 x 5 (its ends are not formed).
    let (v, a) = take(&s, f);
    assert!(near(v, 1.2) && near(a, 4.0), "{v:?} {a:?}");
}

#[test]
fn a_column_is_its_section_by_its_height_formed_on_its_sides() {
    let mut s = Session::new();
    let c = made(&mut s, "structural_column", json!({"at": [0, 0]}));
    // The default 300 x 300 column is 3 m tall.
    let (v, f) = take(&s, c);
    assert!(near(v, 0.27) && near(f, 3.6), "{v:?} {f:?}");
}

#[test]
fn a_beam_schedule_shows_volume_and_formwork() {
    let mut s = Session::new();
    beam_type(&mut s, "B 300 x 600", 300, 600);
    made(&mut s, "beam", json!({"points": [[0, 0], [5000, 0]], "type": "B 300 x 600"}));
    let r = run(&mut s, "schedule", json!({"name": "Beams", "category": "beam", "fields": ["Type", "Volume", "Formwork Area"]}));
    let row = &r["rows"][0];
    assert_eq!(row[0], "B 300 x 600");
    assert!(near(row[1].as_f64(), 0.9) && near(row[2].as_f64(), 7.5), "{row}");
    // A schedule of several categories reads each one's own.
    let r = run(&mut s, "schedule", json!({"name": "All", "category": "multicategory", "fields": ["Category", "Volume", "Formwork Area"]}));
    let beam = r["rows"].as_array().unwrap().iter().find(|row| row[0] == "beam" || row[0] == "Beam").unwrap();
    assert!(near(beam[1].as_f64(), 0.9) && near(beam[2].as_f64(), 7.5), "{beam}");
}

/// Two 300 x 300 structural columns 5 m apart, from the ground to 3 m, and a 300 x 600 beam on their centres with its top at 3 m.
fn frame(s: &mut Session) -> u64 {
    made(s, "structural_column", json!({"at": [0, 0]}));
    made(s, "structural_column", json!({"at": [5000, 0]}));
    beam_type(s, "B 300 x 600", 300, 600);
    made(s, "beam", json!({"points": [[0, 0], [5000, 0]], "type": "B 300 x 600", "offset": 3000}))
}

#[test]
fn a_beam_between_two_columns_is_counted_to_their_faces() {
    let mut s = Session::new();
    let b = frame(&mut s);
    // Its axis is 5,000 centre to centre and 4,700 between the columns' faces: 0.3 x 0.6 x 4.7, and 0.3 x 4.7 + 2 x 0.6 x 4.7.
    let (v, f) = take(&s, b);
    assert!(near(v, 0.846) && near(f, 7.05), "{v:?} {f:?}");
}

#[test]
fn a_column_that_does_not_reach_the_beam_takes_nothing_off() {
    let mut s = Session::new();
    beam_type(&mut s, "B 300 x 600", 300, 600);
    // Stood above the beam's height and beside it, neither meets it.
    made(&mut s, "structural_column", json!({"at": [0, 0], "base_offset": 3000, "height": 3000}));
    made(&mut s, "structural_column", json!({"at": [5000, 2000]}));
    let b = made(&mut s, "beam", json!({"points": [[0, 0], [5000, 0]], "type": "B 300 x 600", "offset": 3000}));
    let (v, f) = take(&s, b);
    assert!(near(v, 0.9) && near(f, 7.5), "{v:?} {f:?}");
}

/// Level 2 at 3 m and, over the frame's beam, a structural floor of the default 200 from `y0` to y = 2,000, its top at 3 m: the
/// beam's top is flush with it.
fn slab(s: &mut Session, y0: i32, structural: bool) {
    run(s, "level", json!({"name": "L2", "elevation": 3000}));
    let boundary = json!([[-1000, y0], [6000, y0], [6000, 2000], [-1000, 2000]]);
    made(s, "floor", json!({"boundary": boundary, "level": "L2", "structural": structural}));
}

#[test]
fn a_slab_flush_with_a_beams_top_takes_its_thickness_from_the_beam() {
    let mut s = Session::new();
    let b = frame(&mut s);
    slab(&mut s, -2000, true);
    // The slab holds the top 200 of the beam on both sides: 0.3 x 0.4 x 4.7 and 0.3 x 4.7 + 2 x 0.4 x 4.7.
    let (v, f) = take(&s, b);
    assert!(near(v, 0.564) && near(f, 5.17), "{v:?} {f:?}");
}

#[test]
fn a_slab_on_one_side_of_a_beam_takes_half() {
    let mut s = Session::new();
    let b = frame(&mut s);
    slab(&mut s, 0, true);
    // Its edge is on the beam's axis, so only one side is covered: half the width by 200 by 4.7 less, and 0.2 x 4.7 less of forms.
    let (v, f) = take(&s, b);
    assert!(near(v, 0.705) && near(f, 6.11), "{v:?} {f:?}");
}

#[test]
fn a_floor_that_is_not_structural_takes_nothing_off_a_beam() {
    let mut s = Session::new();
    let b = frame(&mut s);
    slab(&mut s, -2000, false);
    let (v, f) = take(&s, b);
    assert!(near(v, 0.846) && near(f, 7.05), "{v:?} {f:?}");
}

#[test]
fn a_column_under_a_slab_loses_the_slabs_thickness() {
    let mut s = Session::new();
    let c = made(&mut s, "structural_column", json!({"at": [0, 0]}));
    slab(&mut s, -2000, true);
    // The slab's 200 are its own: the column is 2.8 m of concrete, 0.09 x 2.8, and 1.2 x 2.8 of forms.
    let (v, f) = take(&s, c);
    assert!(near(v, 0.252) && near(f, 3.36), "{v:?} {f:?}");
}

#[test]
fn a_column_outside_the_slab_is_whole() {
    let mut s = Session::new();
    let c = made(&mut s, "structural_column", json!({"at": [0, 0]}));
    // The slab's edge is a metre away from it.
    slab(&mut s, 1000, true);
    let (v, f) = take(&s, c);
    assert!(near(v, 0.27) && near(f, 3.6), "{v:?} {f:?}");
}

fn within(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 0.006)
}

const ROOM: [[i32; 2]; 4] = [[0, 0], [6000, 0], [6000, 4000], [0, 4000]];

#[test]
fn a_floors_forms_are_its_soffit_and_its_free_edges() {
    let mut s = Session::new();
    let f = made(&mut s, "floor", json!({"boundary": ROOM, "structural": true}));
    // 6 x 4 m of 200: 4.8 m³ of concrete, a soffit of 24 m² and 20 m of edge, 0.2 high.
    let (v, a) = take(&s, f);
    assert!(near(v, 4.8) && near(a, 28.0), "{v:?} {a:?}");
}

#[test]
fn a_floor_that_is_not_structure_has_a_volume_and_no_forms() {
    let mut s = Session::new();
    let f = made(&mut s, "floor", json!({"boundary": ROOM}));
    let j = element_json(&s.model, f);
    assert!(near(j["volume_m3"].as_f64(), 4.8) && j.get("formwork_m2").is_none(), "{j}");
}

#[test]
fn a_floors_soffit_is_net_of_the_beams_and_columns_under_it() {
    let mut s = Session::new();
    frame(&mut s);
    slab(&mut s, -2000, true);
    let f = s.model.elements.iter().find_map(|(id, e)| matches!(e, crate::model::Element::Floor(_)).then_some(*id)).unwrap();
    // 7 x 4 m is 28 m². The beam's strip is 5 x 0.3 and each column adds the 0.15 x 0.3 that is outside it: 1.59 m² come off the
    // soffit, and the 22 m of free edge are 0.2 high: 28 - 1.59 + 4.4.
    let (v, a) = take(&s, f);
    assert!(near(v, 5.6) && within(a, 30.81), "{v:?} {a:?}");
}

#[test]
fn a_floors_edge_is_not_formed_where_a_beam_lies_under_it() {
    let mut s = Session::new();
    frame(&mut s);
    slab(&mut s, 0, true);
    let f = s.model.elements.iter().find_map(|(id, e)| matches!(e, crate::model::Element::Floor(_)).then_some(*id)).unwrap();
    // The edge along y = 0 runs over the beam and the two columns from x = -150 to 5150, so 1.7 m of it is free, with the other
    // three edges' 11 m: 12.7 x 0.2 = 2.54. The soffit is 7 x 2 = 14 less 0.75 (the beam's half strip) and 0.045 (the columns').
    let (_, a) = take(&s, f);
    assert!(within(a, 13.205 + 2.54), "{a:?}");
}

#[test]
fn a_sloped_floors_forms_are_its_true_area() {
    let mut s = Session::new();
    let sloped = json!({"boundary": ROOM, "structural": true, "slope": 30, "arrow": [[0, 2000], [6000, 2000]]});
    let f = made(&mut s, "floor", sloped);
    // Its soffit is the plan's 24 m² over cos 30°, and its edges are 20 m by its depth, the 0.2 thickness over cos 30°.
    let (_, a) = take(&s, f);
    assert!(within(a, (24.0 + 20.0 * 0.2) / 30f64.to_radians().cos()), "{a:?}");
}

#[test]
fn steel_per_cubic_metre_is_bar_weight_over_host_volume() {
    let mut s = Session::new();
    let b = frame(&mut s);
    let spec = json!({"host": b, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "top": {"count": 2, "diameter": 12.5},
                      "stirrup": {"diameter": 5, "spacing": 150}, "grade_stirrup": "CA-60"});
    run(&mut s, "reinforce", spec);
    // Every bar here is the beam's, so what the steel list totals is the beam's, and its ratio is that over its 0.846 m³.
    let kg = run(&mut s, "steel_list", json!({}))["totals"]["weight_kg"].as_f64().unwrap();
    let j = element_json(&s.model, b);
    assert!(kg > 10.0 && (j["rebar_kg"].as_f64().unwrap() - kg).abs() < 0.01, "{j} {kg}");
    assert!((j["rebar_ratio"].as_f64().unwrap() - kg / 0.846).abs() < 0.1, "{j} {kg}");
}

#[test]
fn a_member_with_no_bars_has_no_steel_and_a_ratio_of_nothing() {
    let mut s = Session::new();
    let b = frame(&mut s);
    let j = element_json(&s.model, b);
    assert!(near(j["rebar_kg"].as_f64(), 0.0) && near(j["rebar_ratio"].as_f64(), 0.0), "{j}");
    // A steel beam has no concrete to be a ratio of, nor fields of its own.
    let ipe = made(&mut s, "beam", json!({"points": [[0, 3000], [6000, 3000]], "type": "IPE 300"}));
    assert!(element_json(&s.model, ipe).get("rebar_kg").is_none());
}

#[test]
fn a_beam_schedule_shows_the_rebar_and_its_ratio() {
    let mut s = Session::new();
    let b = frame(&mut s);
    run(
        &mut s,
        "reinforce",
        json!({"host": b, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}}),
    );
    let r = run(&mut s, "schedule", json!({"name": "Beams", "category": "beam", "fields": ["Volume", "Rebar Weight", "Rebar Ratio"]}));
    let row = &r["rows"][0];
    let (v, kg, ratio) = (row[0].as_f64().unwrap(), row[1].as_f64().unwrap(), row[2].as_f64().unwrap());
    assert!(near(Some(v), 0.846) && kg > 0.0 && (ratio - kg / v).abs() < 0.1, "{row}");
}

/// The `quantities` reply's row for `category` on `level`, with a `by` value when `value` is given.
fn row<'a>(r: &'a Value, level: &str, category: &str) -> &'a Value {
    r["rows"].as_array().unwrap().iter().find(|row| row["level"] == level && row["category"] == category).unwrap_or(&Value::Null)
}

#[test]
fn the_quantities_command_sums_the_members_of_each_level_and_says_its_rules() {
    let mut s = Session::new();
    let b = frame(&mut s);
    slab(&mut s, -2000, true);
    run(
        &mut s,
        "reinforce",
        json!({"host": b, "cover": 25, "bottom": {"count": 3, "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}}),
    );
    let r = run(&mut s, "quantities", json!({}));
    // The beam is on Level 1, the floor on L2; the two columns stand on Level 1 as well.
    let beam = row(&r, "Level 1", "beam");
    assert!(near(beam["volume_m3"].as_f64(), 0.564) && beam["rebar_kg"].as_f64().unwrap() > 0.0, "{beam}");
    assert!(near(row(&r, "Level 1", "structuralcolumn")["count"].as_f64(), 2.0), "{r}");
    assert!(near(row(&r, "L2", "floor")["volume_m3"].as_f64(), 5.6), "{r}");
    // The levels come in elevation order and their totals are their rows' sums; the project's total is theirs.
    let levels = r["levels"].as_array().unwrap();
    assert_eq!((levels[0]["level"].as_str(), levels[1]["level"].as_str()), (Some("Level 1"), Some("L2")), "{levels:?}");
    let sum: f64 = levels.iter().map(|l| l["volume_m3"].as_f64().unwrap()).sum();
    assert!((r["total"]["volume_m3"].as_f64().unwrap() - sum).abs() < 0.002, "{r}");
    let kg = run(&mut s, "steel_list", json!({}))["totals"]["weight_kg"].as_f64().unwrap();
    assert!((r["total"]["rebar_kg"].as_f64().unwrap() - kg).abs() < 0.01, "the project's steel is the steel list's: {r}");
    assert!(r["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("own rules")), "{r}");
}

#[test]
fn a_bar_with_no_host_is_in_its_levels_totals_once() {
    let mut s = Session::new();
    run(&mut s, "rebar", json!({"points": [[0, 0, 0], [3000, 0, 0]], "diameter": 20}));
    let r = run(&mut s, "quantities", json!({}));
    let kg = run(&mut s, "steel_list", json!({}))["totals"]["weight_kg"].as_f64().unwrap();
    let line = row(&r, "Level 1", "unhosted bars");
    assert!(near(line["rebar_kg"].as_f64(), (kg * 100.0).round() / 100.0), "{r}");
    assert!((r["total"]["rebar_kg"].as_f64().unwrap() - kg).abs() < 0.01, "{r}");
}

#[test]
fn a_level_and_a_category_narrow_the_reply() {
    let mut s = Session::new();
    frame(&mut s);
    slab(&mut s, -2000, true);
    let r = run(&mut s, "quantities", json!({"level": "L2"}));
    assert!(r["rows"].as_array().unwrap().iter().all(|row| row["level"] == "L2"), "{r}");
    let r = run(&mut s, "quantities", json!({"categories": ["beam"]}));
    assert!(r["rows"].as_array().unwrap().iter().all(|row| row["category"] == "beam"), "{r}");
    let e = s.execute("quantities", &json!({"categories": ["wall"]})).unwrap_err();
    assert!(e.contains("beam") && e.contains("floor"), "{e}");
}

#[test]
fn by_a_parameter_the_totals_split_and_still_add_up() {
    let mut s = Session::new();
    let b = frame(&mut s);
    run(&mut s, "parameter", json!({"name": "Stage", "kind": "text", "categories": ["beam"]}));
    run(&mut s, "modify", json!({"id": b, "params": {"Stage": "Precast"}}));
    made(&mut s, "beam", json!({"points": [[0, 2000], [5000, 2000]], "type": "B 300 x 600", "offset": 3000}));
    let r = run(&mut s, "quantities", json!({"by": "Stage", "categories": ["beam"]}));
    let rows = r["rows"].as_array().unwrap();
    let precast: f64 = rows.iter().filter(|row| row["value"] == "Precast").map(|row| row["volume_m3"].as_f64().unwrap()).sum();
    let rest: f64 = rows.iter().filter(|row| row["value"] == "").map(|row| row["volume_m3"].as_f64().unwrap()).sum();
    assert!(precast > 0.0 && rest > 0.0, "{r}");
    assert!((precast + rest - r["total"]["volume_m3"].as_f64().unwrap()).abs() < 0.002, "{r}");
    assert!(s.execute("quantities", &json!({"by": "No Such"})).unwrap_err().contains("project parameter"));
}

#[test]
fn csv_and_xlsx_hold_the_rows_of_the_reply() {
    let mut s = Session::new();
    frame(&mut s);
    let dir = std::env::temp_dir().join(format!("bimcraft-quantities-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("q.csv");
    run(&mut s, "quantities", json!({"path": csv}));
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.contains("Level") && text.contains("Volume") && text.contains("beam"), "{text}");
    let xlsx = dir.join("q.xlsx");
    run(&mut s, "quantities", json!({"path": xlsx}));
    assert!(std::fs::read(&xlsx).unwrap().starts_with(b"PK"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_materials_summary_adds_up_to_the_elements_of_its_level() {
    let mut s = Session::new();
    frame(&mut s);
    slab(&mut s, -2000, true);
    let made = run(&mut s, "schedule", json!({"template": "materials_summary"}));
    let rows = made["rows"].as_array().unwrap();
    assert!(!rows.is_empty(), "{made}");
    let q = run(&mut s, "quantities", json!({}));
    // The summary's volume column and the command's total are the same number, however the rows are grouped.
    let at = made["columns"].as_array().unwrap().iter().position(|c| c.as_str().is_some_and(|c| c.starts_with("Volume"))).unwrap();
    let volume: f64 = rows.iter().filter_map(|row| row[at].as_f64()).sum();
    assert!((volume - q["total"]["volume_m3"].as_f64().unwrap()).abs() < 0.002, "{volume} {made}");
    assert!(s.execute("schedule", &json!({"template": "materials_summary", "host": 1})).is_err());
}

/// Payloads for the sweep: a structural column and a structural floor made in the base project to be counted, and what the
/// command and the template are given that they are not meant for.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let _ = base.execute("structural_column", &json!({"at": [500, 500]}));
    let _ = base.execute("floor", &json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "structural": true}));
    let many: Vec<String> = (0..10_000).map(|i| format!("c{i}")).collect();
    let p = |cmd: &'static str, v: Value| (cmd, v);
    vec![
        p("quantities", json!({})),
        p("quantities", json!({"by": []})),
        p("quantities", json!({"by": "No Such"})),
        p("quantities", json!({"by": "x".repeat(10_000)})),
        p("quantities", json!({"level": "nowhere"})),
        p("quantities", json!({"level": []})),
        p("quantities", json!({"ids": [u64::MAX]})),
        p("quantities", json!({"ids": []})),
        p("quantities", json!({"categories": [1]})),
        p("quantities", json!({"categories": many})),
        p("quantities", json!({"categories": "beam"})),
        p("quantities", json!({"path": "/nonexistent/folder/q.csv"})),
        p("quantities", json!({"path": 5})),
        p("quantities", json!({"unknown": 1})),
        p("schedule", json!({"template": "materials_summary", "by": 5})),
        p("schedule", json!({"template": "materials_summary", "by": "No Such"})),
        p("schedule", json!({"template": "materials_summary", "level": "nowhere"})),
        p("schedule", json!({"template": "materials_summary", "category": "wall"})),
        p("schedule", json!({"template": "materials_summary", "fields": [5]})),
    ]
}

/// What the quantities cost for each member at two sizes of the tower, as `crates/core/CLAUDE.md` asks (compare the cost per member,
/// not the totals): `cargo test -p bimcraft-core quantities_cost -- --ignored --nocapture`.
#[test]
#[ignore = "a timing, run by hand"]
fn quantities_cost_per_member_at_two_sizes() {
    use crate::sample::tower::{Plate, tower_on};
    for storeys in [5_u32, 20] {
        let mut s = Session::new();
        s.reset(tower_on(Plate::SMALL, storeys).unwrap(), None);
        let started = std::time::Instant::now();
        let r = run(&mut s, "quantities", json!({}));
        let took = started.elapsed();
        let n = r["rows"].as_array().map_or(1, |rows| rows.iter().map(|row| row["count"].as_u64().unwrap_or(0)).sum::<u64>()).max(1);
        println!("{storeys} storeys, {n} members: quantities {took:?} ({:?} a member)", took / u32::try_from(n).unwrap_or(1));
    }
}

#[test]
fn the_materials_summary_says_that_quantities_splits_by_a_parameter() {
    let mut s = Session::new();
    frame(&mut s);
    let e = s.execute("schedule", &json!({"template": "materials_summary", "by": "Stage"})).unwrap_err();
    assert!(e.contains("quantities"), "{e}");
}
