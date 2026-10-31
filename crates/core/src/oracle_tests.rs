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
