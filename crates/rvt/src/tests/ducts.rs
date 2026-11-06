//! Ducts and cable trays: a straight run along the bounded line in their data, as wide and as deep as the data says.

use super::walls::{base, import, level_named};
use crate::project_fixture::{Block, Project, Rec};
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Element, Run};

const DUCTS: i64 = -2_008_000;
const CABLE_TRAYS: i64 = -2_008_130;

/// A run's start and end in plan, its end offset, level and type name.
type Got<'a> = (f64, f64, f64, f64, Option<f64>, bimcraft_core::ids::Guid, &'a str);

fn runs<'a>(i: &'a Imported, category: &str) -> Vec<&'a Run> {
    i.model.elements.values().filter(|e| e.category() == category).filter_map(|e| e.run()).collect()
}

/// An element's data holding its width and height (feet) 66 and 74 bytes from its header, and a bounded line from `a` to `b`
/// (the origin `a`, the unit direction, the parameters 0 and the length).
fn data(a: [f64; 3], b: [f64; 3], width: f64, height: f64) -> Vec<u8> {
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    // The header is 18 bytes (`ff ff ff ff`, the tag, `01 00 00 00` and the id), so 48 bytes of other data come first.
    let mut payload = vec![0x44; 48];
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    payload.extend(vec![0x55; 100]);
    payload.extend_from_slice(&[4, 0, 8, 1]);
    for x in [0.0, l, a[0], a[1], a[2], v[0] / l, v[1] / l, v[2] / l] {
        payload.extend_from_slice(&f64::to_le_bytes(x));
    }
    payload
}

fn element(p: &mut Project, id: u32, category: i64, a: [f64; 3], b: [f64; 3], width: f64, height: f64) {
    // The line's box, as thick as the section (enough to have a volume).
    let (rx, rz) = (width / 2.0, height / 2.0);
    let bbox =
        [a[0].min(b[0]) - rx, a[1].min(b[1]) - rx, a[2].min(b[2]) - rz, a[0].max(b[0]) + rx, a[1].max(b[1]) + rx, a[2].max(b[2]) + rz];
    p.recs.push(Rec::new(id, category).at(bbox, &[10, 11, u64::from(id)]));
    p.data.push(Block { id, payload: data(a, b, width, height) });
}

#[test]
fn a_duct_and_a_cable_tray_come_in_along_their_line_as_wide_and_deep_as_their_data_says() {
    for release in [2019, 2024] {
        let mut p = base(release);
        element(&mut p, 300, DUCTS, [0.0, 0.0, 10.0], [20.0, 0.0, 10.0], 2.0, 1.0); // a rectangular duct on level 2
        element(&mut p, 301, DUCTS, [0.0, 3.0, 10.0], [20.0, 3.0, 10.0], 1.0, 1.0); // as wide as deep: round
        element(&mut p, 302, DUCTS, [5.0, 5.0, 0.0], [5.0, 5.0, 10.0], 1.0, 2.0); // a riser
        element(&mut p, 303, CABLE_TRAYS, [0.0, 6.0, 10.0], [10.0, 6.0, 11.0], 0.5, 0.25); // slopes up a foot
        let i = import(&p);
        let (l1, l2) = (level_named(&i, "Level 1"), level_named(&i, "Level 2"));
        let got: Vec<Got> = runs(&i, "duct")
            .iter()
            .map(|r| (r.start.0, r.start.1, r.end.0, r.end.1, r.end_offset, r.level, r.type_name.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (0.0, 0.0, 6096.0, 0.0, None, l2, "Duct 609.6 x 304.8 mm"),
                (0.0, 914.4, 6096.0, 914.4, None, l2, "Round duct 304.8 mm"),
                (1524.0, 1524.0, 1524.0, 1524.0, Some(3048.0), l1, "Duct 304.8 x 609.6 mm"),
            ],
            "Revit {release}"
        );
        let tray = runs(&i, "cabletray");
        assert_eq!(tray.len(), 1);
        assert_eq!((tray[0].end.0, tray[0].end_offset, tray[0].type_name.as_str()), (3048.0, Some(304.8), "Cable tray 152.4 x 76.2 mm"));
        let t = &i.model.types.runs["duct"];
        assert_eq!(
            (t["Duct 609.6 x 304.8 mm"].width, t["Duct 609.6 x 304.8 mm"].depth, t["Duct 609.6 x 304.8 mm"].round),
            (609.6, 304.8, false)
        );
        assert_eq!((t["Round duct 304.8 mm"].width, t["Round duct 304.8 mm"].round), (304.8, true));
        assert!(i.model.check().is_ok());
        assert!(
            !i.skipped.iter().any(|s| s.contains("no reader") && (s.contains("Ducts") || s.contains("Cable Trays"))),
            "{:?}",
            i.skipped
        );
        assert!(i.model.elements.values().any(|e| matches!(e, Element::Duct(_))));
    }
}

#[test]
fn a_duct_whose_data_holds_no_straight_line_is_left_out_and_listed() {
    let mut p = base(2019);
    element(&mut p, 300, DUCTS, [0.0, 0.0, 10.0], [20.0, 0.0, 10.0], 2.0, 1.0);
    p.recs.push(Rec::new(301, DUCTS).at([0.0, 1.0, 9.0, 20.0, 2.0, 10.0], &[10, 11, 301]));
    p.data.push(Block { id: 301, payload: vec![0x66; 200] });
    p.recs.push(Rec::new(302, CABLE_TRAYS).at([0.0, 2.0, 9.0, 20.0, 2.5, 10.0], &[10, 11, 302]));
    let i = import(&p);
    assert_eq!(runs(&i, "duct").len(), 1);
    let says = |what: &str| i.skipped.iter().any(|s| s.contains(what));
    assert!(says("1 duct left out, its line and section are not in the record (ids 301)"), "{:?}", i.skipped);
    assert!(says("1 cable tray left out, its line and section are not in the record (ids 302)"), "{:?}", i.skipped);
}

#[test]
fn a_mutation_sweep_over_a_project_of_ducts_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = base(release);
        element(&mut p, 300, DUCTS, [0.0, 0.0, 10.0], [20.0, 0.0, 10.0], 2.0, 1.0);
        element(&mut p, 302, DUCTS, [5.0, 5.0, 0.0], [5.0, 5.0, 10.0], 1.0, 2.0);
        element(&mut p, 303, CABLE_TRAYS, [0.0, 6.0, 10.0], [10.0, 6.0, 11.0], 0.5, 0.25);
        p.members = 3;
        super::sweep(&p, 0x3f4e_5d6c_7b8a_9998 ^ u64::from(release), 800, |i, round| {
            assert!(runs(i, "duct").len() + runs(i, "cabletray").len() <= 3, "round {round}");
        });
    }
}
