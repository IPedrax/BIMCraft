//! Electrical Settings, with no window: the three tables as the project has them, what a draft asks of the core, in what order, and the refusals.

use super::*;
use crate::app::App;

fn type_in(d: &mut Electrical, i: usize, text: &str) {
    if let Some(e) = d.edit_mut(i) {
        *e = TextEdit::new(text);
    }
}

/// The row of the voltage called `name`.
fn voltage_at(d: &Electrical, name: &str) -> usize {
    d.voltages.iter().position(|r| typed(&r.edits[0]) == name).expect("a voltage with that name")
}

#[test]
fn it_opens_on_the_projects_settings_and_a_draft_left_alone_asks_nothing() {
    let app = App::sample();
    let m = &*app.session.model;
    let mut d = Electrical::open(m);
    assert_eq!(d.commands(m), Ok(Vec::new()), "nothing changed, nothing asked");
    assert_eq!(d.tabs().len(), 3);
    for tab in 0..3 {
        d.tab = tab;
        assert!(!d.sections().is_empty(), "tab {tab}");
    }
    assert_eq!(d.voltages.len(), m.voltages.len());
    assert!(
        d.edit(at(0, 0, 1)).is_some() && d.edit(at(0, 999, 1)).is_none() && d.edit(at(1, 0, 3)).is_none(),
        "a place is a box or it is none"
    );
}

#[test]
fn a_voltage_added_renamed_changed_and_deleted_is_asked_in_the_order_the_core_can_do_it() {
    let mut app = App::sample();
    let mut d = Electrical::open(&app.session.model);
    let before = d.voltages.len();
    d.press("add_voltage", 0, &mut app);
    let row = d.voltages.len() - 1;
    type_in(&mut d, at(0, row, 0), "Lab");
    type_in(&mut d, at(0, row, 1), "400");
    let asked = d.commands(&app.session.model).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        asked,
        [("voltage", json!({"name": "Lab", "nominal": 400.0, "min": 360.0, "max": 440.0}))],
        "a band untouched follows the nominal"
    );
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Close, "{:?}", d.error());
    assert_eq!(app.session.model.voltages.len(), before + 1);
    assert_eq!(app.session.model.voltages.get("Lab").map(|v| (v.nominal, v.min, v.max)), Some((400.0, 360.0, 440.0)));

    let mut d = Electrical::open(&app.session.model);
    let row = voltage_at(&d, "Lab");
    type_in(&mut d, at(0, row, 0), "Lab 2");
    type_in(&mut d, at(0, row, 2), "350");
    let asked = d.commands(&app.session.model).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(asked[0], ("voltage", json!({"name": "Lab", "rename": "Lab 2"})), "the rename goes first");
    assert_eq!(asked[1].1["min"], json!(350.0), "and the band typed over stays: {asked:?}");
    assert_eq!(d.press("apply", 0, &mut app), Outcome::Keep, "Apply keeps it open");
    assert!(d.error().is_none() && app.session.model.voltages.contains_key("Lab 2") && !app.session.model.voltages.contains_key("Lab"));
    assert_eq!(voltage_at(&d, "Lab 2"), row, "and shows the project as it is now");

    d.press("delete_voltage", row, &mut app);
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Close, "{:?}", d.error());
    assert!(!app.session.model.voltages.contains_key("Lab 2"));
    assert!(app.undo() && app.session.model.voltages.contains_key("Lab 2"), "one undo step for the draft");
}

#[test]
fn a_voltage_a_system_supplies_is_not_deleted_and_names_and_numbers_are_checked_with_what_was_typed() {
    let mut app = App::sample();
    let mut d = Electrical::open(&app.session.model);
    d.press("add_system", 0, &mut app);
    let supplied =
        d.voltages.iter().position(|r| Some(r.key) == d.systems.last().and_then(|s| s.supply.l_l)).expect("the voltage it supplies");
    d.press("delete_voltage", supplied, &mut app);
    assert!(d.error().is_some_and(|e| e.contains("supplied by")), "{:?}", d.error());
    assert!(d.voltages.len() == app.session.model.voltages.len(), "it stays");

    let mut d = Electrical::open(&app.session.model);
    d.press("add_voltage", 0, &mut app);
    let n = d.voltages.len();
    let taken = typed(&d.voltages[0].edits[0]);
    type_in(&mut d, at(0, n - 1, 0), &taken);
    assert!(d.commands(&app.session.model).err().unwrap_or_default().contains("Two voltages are called"));
    type_in(&mut d, at(0, n - 1, 0), "  ");
    assert!(d.commands(&app.session.model).err().unwrap_or_default().contains("needs a name"));
    type_in(&mut d, at(0, n - 1, 0), "Lab");
    type_in(&mut d, at(0, n - 1, 1), "lots");
    let why = d.commands(&app.session.model).err().unwrap_or_default();
    assert!(why.contains(&format!("Voltage {n}")) && why.contains("Nominal") && why.contains("lots"), "{why}");
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Keep);
    assert!(d.error().is_some_and(|e| e.contains("lots")), "said inside the dialog: {:?}", d.error());
}

#[test]
fn a_system_is_made_from_its_lists_and_the_lists_name_the_voltages() {
    let mut app = App::sample();
    let mut d = Electrical::open(&app.session.model);
    d.press("tab", 1, &mut app);
    d.press("add_system", 0, &mut app);
    let row = d.systems.len() - 1;
    let phases = d.choice(at(1, row, 1)).expect("the phases list");
    assert_eq!(
        (phases.options.as_slice(), phases.current),
        (["1", "2", "3"].map(String::from).as_slice(), Some(2)),
        "three phases to begin with"
    );
    let line = d.choice(at(1, row, 3)).expect("a voltage list");
    assert_eq!(
        (line.options[0].as_str(), line.current),
        (NONE, Some(1)),
        "none first, then the voltages: the first is the one it supplies"
    );
    d.choose(at(1, row, 3), 0, &mut app);
    d.choose(at(1, row, 1), 0, &mut app);
    d.choose(at(1, row, 2), 0, &mut app);
    d.choose(at(1, row, 6), 1, &mut app);
    let s = &d.systems[row].supply;
    assert_eq!((s.phases, s.wires, s.l_l, s.ground.as_deref()), (1, 2, None, Some("TN-S")));
    type_in(&mut d, at(1, row, 0), "Lab system");
    let asked = d.commands(&app.session.model).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (asked[0].0, &asked[0].1["name"], &asked[0].1["phases"], &asked[0].1["ground"]),
        ("distribution_system", &json!("Lab system"), &json!(1), &json!("TN-S"))
    );
}

#[test]
fn a_wire_type_has_a_material_and_a_table_of_sizes_which_grows_by_one_larger() {
    let mut app = App::sample();
    let mut d = Electrical::open(&app.session.model);
    d.press("tab", 2, &mut app);
    d.press("add_wire", 0, &mut app);
    assert_eq!(d.wire, d.wires.len() - 1, "the new one is the one shown");
    d.press("add_size", 0, &mut app);
    assert_eq!(d.wires[d.wire].sizes.len(), 2);
    assert_eq!(typed(&d.wires[d.wire].sizes[1][0]), "2.3", "one more than the last, larger");
    type_in(&mut d, at(2, 0, 0), "Lab wire");
    d.choose(MATERIAL_LIST, 1, &mut app);
    type_in(&mut d, at(2, 2, 2), "250 kcmil");
    let asked = d.commands(&app.session.model).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!((&asked[0].1["material"], asked[0].1["sizes"].as_array().map(Vec::len)), (&json!("aluminium"), Some(2)));
    assert_eq!(d.press("ok", 0, &mut app), Outcome::Close, "{:?}", d.error());
    let made = app.session.model.wire_types.get("Lab wire").expect("the wire type");
    assert_eq!((made.material.as_str(), made.sizes.len(), made.sizes[1].label.as_deref()), ("aluminium", 2, Some("250 kcmil")));

    let mut d = Electrical::open(&app.session.model);
    d.wire = d.wires.iter().position(|r| typed(&r.edits[0]) == "Lab wire").expect("it is listed");
    d.press("delete_size", 0, &mut app);
    assert_eq!(d.wires[d.wire].sizes.len(), 1);
    type_in(&mut d, at(2, 1, 1), "far");
    assert!(d.commands(&app.session.model).err().unwrap_or_default().contains("size 1"), "a size's number that is none names its row");
}
