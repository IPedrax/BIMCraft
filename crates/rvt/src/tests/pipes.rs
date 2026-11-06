//! Pipes: a straight run between the origins of the two connectors in the record's data, as wide as its box says.

use super::walls::{base, import, level_named};
use crate::project_fixture::{Block, Project, Rec};
use bimcraft_core::import::Imported;
use bimcraft_core::model::{Element, Justify, Run};

const PIPES: i64 = -2_008_044;

/// A pipe's start and end in plan, its end offset, level and type name.
type Got<'a> = (f64, f64, f64, f64, Option<f64>, bimcraft_core::ids::Guid, &'a str);

fn pipes(i: &Imported) -> Vec<&Run> {
    i.model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Pipe(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// A connector as a pipe's data holds it: the marker word 1, its origin (feet) and a fourth number, the word 3, its index and an
/// unset id.
fn connector(index: u32, at: [f64; 3]) -> Vec<u8> {
    let mut b = vec![1, 0, 0, 0];
    for v in at {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&288.7f64.to_le_bytes());
    b.extend_from_slice(&[3, 0, 0, 0]);
    b.extend_from_slice(&index.to_le_bytes());
    b.extend_from_slice(&[0xff; 4]);
    b
}

/// The box (feet) of a round pipe of outside diameter `d` from `a` to `b`.
fn cylinder_box(a: [f64; 3], b: [f64; 3], d: f64) -> [f64; 6] {
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let half = |k: usize| l / 2.0 * (v[k] / l).abs() + d / 2.0 * (1.0 - (v[k] / l).powi(2)).sqrt();
    let c = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
    [c[0] - half(0), c[1] - half(1), c[2] - half(2), c[0] + half(0), c[1] + half(1), c[2] + half(2)]
}

/// A pipe of outside diameter `d` ft from `a` to `b`, its data holding its two connectors.
fn pipe(p: &mut Project, id: u32, a: [f64; 3], b: [f64; 3], d: f64) {
    p.recs.push(Rec::new(id, PIPES).at(cylinder_box(a, b, d), &[10, 11, u64::from(id)]));
    let mut payload = vec![0x44; 30];
    payload.extend(connector(0, a));
    payload.extend(vec![0x55; 20]);
    payload.extend(connector(1, b));
    p.data.push(Block { id, payload });
}

#[test]
fn a_pipe_comes_in_between_its_connectors_as_wide_as_its_box_with_the_level_below_it() {
    for release in [2019, 2024] {
        let mut p = base(release);
        pipe(&mut p, 300, [0.0, 0.0, 10.0], [20.0, 0.0, 10.0], 0.5); // along x, at level 2
        pipe(&mut p, 301, [5.0, 5.0, 0.0], [5.0, 5.0, 10.0], 0.25); // a riser from level 1
        pipe(&mut p, 302, [0.0, 3.0, 10.0], [20.0, 3.0, 11.0], 0.5); // slopes up a foot
        pipe(&mut p, 303, [0.0, 6.0, 15.0], [10.0, 6.0, 15.0], 0.5); // above level 2
        let i = import(&p);
        let (l1, l2) = (level_named(&i, "Level 1"), level_named(&i, "Level 2"));
        let got: Vec<Got> =
            pipes(&i).iter().map(|r| (r.start.0, r.start.1, r.end.0, r.end.1, r.end_offset, r.level, r.type_name.as_str())).collect();
        assert_eq!(
            got,
            [
                (0.0, 0.0, 6096.0, 0.0, None, l2, "Pipe 152.4 mm"),
                (1524.0, 1524.0, 1524.0, 1524.0, Some(3048.0), l1, "Pipe 76.2 mm"),
                (0.0, 914.4, 6096.0, 914.4, Some(304.8), l2, "Pipe 152.4 mm"),
                (0.0, 1828.8, 3048.0, 1828.8, None, l2, "Pipe 152.4 mm"),
            ],
            "Revit {release}"
        );
        let offsets: Vec<f64> = pipes(&i).iter().map(|r| r.offset).collect();
        assert_eq!(offsets, [0.0, 0.0, 0.0, 1524.0], "Revit {release}");
        assert!(pipes(&i).iter().all(|r| r.justify == Justify::Centre && r.system.is_empty()));
        let types = &i.model.types.runs["pipe"];
        let widths: Vec<(&str, f64, bool)> =
            types.iter().filter(|(n, _)| n.starts_with("Pipe ")).map(|(n, t)| (n.as_str(), t.width, t.round)).collect();
        assert!(widths.contains(&("Pipe 152.4 mm", 152.4, true)) && widths.contains(&("Pipe 76.2 mm", 76.2, true)), "{widths:?}");
        assert!(i.model.check().is_ok());
        // They come in as pipes, so the notes do not list their category as unread.
        assert!(!i.skipped.iter().any(|s| s.contains("no reader") && s.contains("Pipes")), "{:?}", i.skipped);
    }
}

#[test]
fn a_pipe_whose_data_holds_no_two_connectors_is_left_out_and_listed() {
    let mut p = base(2019);
    pipe(&mut p, 300, [0.0, 0.0, 10.0], [20.0, 0.0, 10.0], 0.5);
    // A second pipe with only one connector, and a third with no data.
    p.recs.push(Rec::new(301, PIPES).at([0.0, 1.0, 9.0, 20.0, 1.5, 10.0], &[10, 11, 301]));
    p.data.push(Block { id: 301, payload: connector(0, [0.0, 1.25, 9.5]) });
    p.recs.push(Rec::new(302, PIPES).at([0.0, 2.0, 9.0, 20.0, 2.5, 10.0], &[10, 11, 302]));
    let i = import(&p);
    assert_eq!(pipes(&i).len(), 1);
    assert!(
        i.skipped.iter().any(|s| s.starts_with("2 pipes left out, its two connectors are not in the record (ids 301, 302)")),
        "{:?}",
        i.skipped
    );
}

#[test]
fn a_mutation_sweep_over_a_project_of_pipes_never_panics_and_what_comes_in_checks_out() {
    for release in [2019, 2024] {
        let mut p = base(release);
        pipe(&mut p, 300, [0.0, 0.0, 10.0], [20.0, 0.0, 10.0], 0.5);
        pipe(&mut p, 301, [5.0, 5.0, 0.0], [5.0, 5.0, 10.0], 0.25);
        pipe(&mut p, 302, [0.0, 3.0, 10.0], [20.0, 3.0, 11.0], 0.5);
        p.members = 3;
        super::sweep(&p, 0x1d2c_3b4a_5968_7786 ^ u64::from(release), 800, |i, round| {
            assert!(pipes(i).len() <= 3, "round {round}");
        });
    }
}
