//! The oracle: random edits of a small tower, each checked by what changed and by the whole model. The engine holds the
//! two checks to the same answer after every command a test runs (`Session::execute`); this drives it through edits
//! no other test makes, valid and not, from a seed, so a failure can be run again.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id};
use crate::sample::tower::{Plate, tower_on};

/// xorshift64*: enough to pick edits, the same every run.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn mm(&mut self, span: i64) -> i64 {
        (self.next() % (2 * span as u64 + 1)) as i64 - span
    }
}

/// An element of one of `categories`, picked by the dice.
fn pick(s: &Session, dice: &mut Dice, categories: &[&str]) -> Option<Id> {
    let ids: Vec<Id> = s.model.elements.iter().filter(|(_, e)| categories.contains(&e.category())).map(|(id, _)| *id).collect();
    ids.get(dice.below(ids.len())).copied()
}

fn edit(s: &mut Session, dice: &mut Dice) -> (String, Value) {
    let any = ["wall", "door", "window", "component", "structuralcolumn", "beam", "pipe", "conduit", "room", "ceiling", "tag"];
    let by = |dice: &mut Dice| json!([dice.mm(3000), dice.mm(3000)]);
    match dice.below(9) {
        0 | 1 => ("move".into(), json!({"ids": [pick(s, dice, &any)], "by": by(dice)})),
        2 => ("copy".into(), json!({"ids": [pick(s, dice, &any)], "by": by(dice)})),
        3 => ("delete".into(), json!({"ids": [pick(s, dice, &any)]})),
        4 => {
            let (x, y) = (dice.mm(18_000) + 18_000, dice.mm(18_000) + 18_000);
            ("wall".into(), json!({"points": [[x, y], [x + dice.mm(6000), y + dice.mm(6000)]], "height": 4000}))
        }
        5 => ("door".into(), json!({"at": [dice.mm(18_000) + 18_000, dice.mm(18_000) + 18_000]})),
        6 => (
            "rotate".into(),
            json!({"ids": [pick(s, dice, &["component", "structuralcolumn"])], "about": [18_000, 18_000], "angle": dice.mm(180)}),
        ),
        7 => {
            let wall = pick(s, dice, &["wall"]);
            ("move".into(), json!({"ids": [wall], "by": by(dice), "end": if dice.below(2) == 0 { "start" } else { "end" }}))
        }
        _ => ("room".into(), json!({"point": [dice.mm(18_000) + 18_000, dice.mm(18_000) + 18_000]})),
    }
}

#[test]
fn random_edits_of_a_tower_check_the_same_by_what_changed_as_whole() {
    let model = tower_on(Plate::SMALL, 1).unwrap();
    for seed in [1u64, 2, 3] {
        let mut s = Session::new();
        s.reset(model.clone(), None);
        // The first command checks the whole tower; from then on, the engine checks what each changes and, here, both.
        s.execute("move", &json!({"ids": [], "by": [0, 0]})).ok();
        let mut dice = Dice(0x9e37_79b9_7f4a_7c15 ^ seed);
        let (mut done, mut refused) = (0, 0);
        for _ in 0..60 {
            let (cmd, p) = edit(&mut s, &mut dice);
            match s.execute(&cmd, &p) {
                Ok(_) => done += 1,
                Err(_) => refused += 1,
            }
        }
        assert!(done > 10 && refused > 0, "seed {seed}: {done} done, {refused} refused: the edits should do both");
        assert_eq!(s.model.check(), Ok(()));
        assert!(s.model.elements.values().any(|e| matches!(e, Element::Room(_))));
    }
}

/// A 2D edit of a plan: a detail line, a note, a filled region or a dimension made, moved, copied or taken away, a wall
/// its dimensions measure moved, an edit undone or redone.
fn edit_2d(s: &mut Session, dice: &mut Dice) -> (String, Value) {
    let any = ["line", "text", "region", "dimension"];
    let at = |dice: &mut Dice| [dice.mm(18_000) + 18_000, dice.mm(18_000) + 18_000];
    let by = |dice: &mut Dice| json!([dice.mm(3000), dice.mm(3000)]);
    match dice.below(12) {
        0 => {
            let a = at(dice);
            ("detail_line".into(), json!({"start": a, "end": [a[0] + dice.mm(4000), a[1] + dice.mm(4000)]}))
        }
        1 => ("text".into(), json!({"text": "Note", "at": at(dice)})),
        2 => {
            let [x, y] = at(dice);
            let (w, h) = (dice.mm(2000), dice.mm(2000));
            let c = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
            ("filled_region".into(), json!({"sketch": [[c[0], c[1]], [c[1], c[2]], [c[2], c[3]], [c[3], c[0]]]}))
        }
        // Across an office, from wall to wall: one of the long sides' rows.
        3 => {
            let (k, y) = (dice.below(12) as i64, if dice.below(2) == 0 { 2000 } else { 34_000 });
            ("dimension".into(), json!({"refs": [[3000 * k, y], [3000 * k + 3000, y]], "at": [3000 * k + 1500, y - 500]}))
        }
        4 | 5 => ("move".into(), json!({"ids": [pick(s, dice, &any)], "by": by(dice)})),
        6 => ("copy".into(), json!({"ids": [pick(s, dice, &any)], "by": by(dice)})),
        7 => ("delete".into(), json!({"ids": [pick(s, dice, &any)]})),
        // A wall slid along itself or moved across: the dimensions on it follow, or can't, and the edit is refused.
        8 => {
            let wall = pick(s, dice, &["wall"]);
            ("move".into(), json!({"ids": [wall], "by": if dice.below(2) == 0 { json!([dice.mm(500), 0]) } else { by(dice) }}))
        }
        9 => ("undo".into(), json!({})),
        10 => ("redo".into(), json!({})),
        _ => ("delete".into(), json!({"ids": [pick(s, dice, &["wall", "door"])]})),
    }
}

/// Random 2D edits of a tower's annotated plan, each checked by what changed and whole, and its library synced by what
/// changed and whole (the engine holds both to the same answer in tests).
#[test]
fn random_2d_edits_of_an_annotated_plan_check_and_sync_the_same_by_what_changed_as_whole() {
    let model = crate::sample::tower::annotated(Plate::SMALL, 1).unwrap_or_else(|e| panic!("{e}"));
    for seed in [1u64, 2, 3] {
        let mut s = Session::new();
        s.reset(model.clone(), None);
        s.execute("move", &json!({"ids": [], "by": [0, 0]})).ok();
        let mut dice = Dice(0x2d2d_2d2d_1234_5678 ^ seed);
        let (mut done, mut refused) = (0, 0);
        for _ in 0..80 {
            let (cmd, p) = edit_2d(&mut s, &mut dice);
            match s.execute(&cmd, &p) {
                Ok(_) => done += 1,
                Err(_) => refused += 1,
            }
        }
        assert!(done > 30 && refused > 0, "seed {seed}: {done} done, {refused} refused: the edits should do both");
        assert_eq!(s.model.check(), Ok(()));
        assert!(s.model.library_in_step());
    }
}

/// What a point given to a dimension measures to, found among the walls near it, is what every wall, door, window and
/// grid of the project would give: at the ends, faces, jambs and corners of a small tower's storey, and between them.
#[test]
fn a_point_finds_the_reference_and_the_end_near_it_that_every_element_would_give() {
    use crate::commands::{endpoint_near, endpoint_near_everywhere, reference_near, reference_near_everywhere};
    use crate::geom::P2;
    let mut s = Session::new();
    s.reset(crate::sample::tower::annotated(Plate::SMALL, 1).unwrap_or_else(|e| panic!("{e}")), None);
    s.execute("grid", &json!({"start": [-2000, 18_050], "end": [38_000, 18_050]})).unwrap();
    // A wall at a sharp angle to another, its mitre running far past its end.
    s.execute("wall", &json!({"points": [[9000, 30_000], [16_000, 30_400]], "height": 4000})).unwrap();
    let m = &s.model;
    let level = m.guid(m.levels()[0].0);
    let mut dice = Dice(0x005e_edd1);
    let mut points = Vec::new();
    for (_, e) in m.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_) | Element::Door(_))).step_by(3) {
        let near = match e {
            Element::Wall(w) => [w.start, w.end, (w.start + w.end) * 0.5][dice.below(3)],
            Element::Door(o) => m.wall(o.host).map_or(P2(0.0, 0.0), |w| w.start + (w.end - w.start).unit() * o.offset),
            _ => continue,
        };
        points.push(near + P2(dice.mm(400) as f64, dice.mm(400) as f64));
    }
    points.push(P2(9000.0, 18_100.0));
    points.push(P2(16_050.0, 30_380.0));
    assert!(points.len() > 40);
    let (mut found, mut ends) = (0, 0);
    for p in points {
        let first = reference_near(m, level, p, |_| true);
        assert_eq!(first, reference_near_everywhere(m, level, p, |_| true), "at {p:?}");
        found += usize::from(first.is_some());
        // Then one parallel to it, as a dimension's second reference is found.
        if let Some(line) = first.and_then(|r| m.reference_line(&r).ok()) {
            let keep = |l: crate::model::Line| crate::geom::parallel(line.0, line.1, l.0, l.1);
            let q = p + P2(0.0, 3000.0);
            assert_eq!(reference_near(m, level, q, keep), reference_near_everywhere(m, level, q, keep), "at {q:?}");
        }
        let end = endpoint_near(m, level, None, p);
        assert_eq!(end, endpoint_near_everywhere(m, level, None, p), "the end at {p:?}");
        ends += usize::from(end.is_some());
    }
    assert!(found > 20 && ends > 5, "{found} references and {ends} ends found");
}
