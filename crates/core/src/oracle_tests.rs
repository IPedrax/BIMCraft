//! The oracle: random edits of a small tower, each checked by what changed and by the whole model. The engine holds the
//! two checks to the same answer after every command a test runs (`Session::execute`); this drives it through edits
//! no other test makes, valid and not, from a seed, so a failure can be run again.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id};
use crate::sample::tower::{Plate, tower_on};

/// xorshift64*: enough to pick edits, the same every run.
pub(crate) struct Dice(pub(crate) u64);

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

pub(crate) fn edit(s: &mut Session, dice: &mut Dice) -> (String, Value) {
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

/// An edit of the kinds the other mixes leave out: 3D dimensions and camera views, how a 3D view draws and
/// what it sketches on, a wall's top offset, a room's colour, a floor's shape points, split lines and falls, a roof with a
/// slope for each edge, a schedule of several categories, a length typed into a dimension and a segment locked, bars a beam is
/// reinforced with, plan symbols and a component of them, a spot slope, the project's voltages; then moved, copied, deleted,
/// undone and redone.
pub(crate) fn edit_more(s: &mut Session, dice: &mut Dice) -> (String, Value) {
    let at = |dice: &mut Dice| [dice.mm(15_000) + 18_000, dice.mm(15_000) + 18_000];
    let level = |s: &Session, dice: &mut Dice| {
        let levels = s.model.levels();
        levels.get(dice.below(levels.len())).map(|(id, _)| *id)
    };
    let new = ["dimension3d", "view3d", "roof", "rebar", "spot", "component", "schedule"];
    // Free walls east of the plate, each a U: two walls 3 m apart joined at the top, which a dimension measures and a typed
    // length moves apart.
    let free = |k: i64| 42_000 + 7000 * k;
    match dice.below(25) {
        0 => {
            let ([x, y], z) = (at(dice), dice.mm(2000) + 2000);
            let to = [x + dice.mm(6000), y, z];
            ("dimension_3d".into(), json!({"refs": [{"at": [x, y, z]}, {"at": to}], "at": [x, y - 1000 - dice.mm(500), z]}))
        }
        1 => {
            let [x, y] = at(dice);
            let p = json!({"eye": [x - 20_000, y - 20_000, 1700 + dice.mm(1000)], "target": [x, y, 1500]});
            ("camera".into(), p)
        }
        // A camera view's eye and target written back, as the 3D view does once the hand lets it be.
        2 => match pick(s, dice, &["view3d"]) {
            Some(v) => {
                let [x, y] = at(dice);
                ("camera".into(), json!({"view": v, "eye": [x - 15_000, y + 15_000, 3000], "target": [x, y, dice.mm(1000) + 1000]}))
            }
            None => ("camera".into(), json!({"eye": [-10_000, -10_000, 2000], "target": [18_000, 18_000, 0]})),
        },
        3 => (
            "graphic_display_options".into(),
            json!({"edges": dice.below(2) == 0, "silhouette": dice.below(5), "ambient_shadows": dice.below(2) == 0}),
        ),
        4 => ("work_plane".into(), json!({"level": level(s, dice), "offset": dice.mm(1000)})),
        5 => {
            let wall = pick(s, dice, &["wall"]);
            ("modify".into(), json!({"ids": [wall], "top_level": level(s, dice), "top_offset": dice.mm(1500)}))
        }
        6 => {
            let room = pick(s, dice, &["room"]);
            ("modify".into(), json!({"ids": [room], "colour": [dice.below(256), dice.below(256), dice.below(256)]}))
        }
        7 => ("add_point".into(), json!({"id": pick(s, dice, &["floor", "roof"]), "at": at(dice), "offset": dice.mm(80)})),
        8 => {
            let a = at(dice);
            ("add_split_line".into(), json!({"id": pick(s, dice, &["floor", "roof"]), "a": a, "b": [a[0] + dice.mm(9000), a[1]]}))
        }
        9 => ("falls".into(), json!({"id": pick(s, dice, &["floor", "roof"]), "drains": [at(dice)], "fall": 1.5})),
        10 => ("reset_shape".into(), json!({"id": pick(s, dice, &["floor", "roof"])})),
        11 => {
            let [x, y] = at(dice);
            let (w, d) = (6000 + dice.below(6000) as i64, 6000 + dice.below(6000) as i64);
            let slopes: Vec<usize> = (0..4).map(|_| dice.below(4) * 10).collect();
            ("roof".into(), json!({"boundary": [[x, y], [x + w, y], [x + w, y + d], [x, y + d]], "slope": 0, "slopes": slopes}))
        }
        12 => {
            let roof = pick(s, dice, &["roof"]);
            ("modify".into(), json!({"ids": [roof], "slopes": [dice.below(3) * 15, 0, dice.below(3) * 15, 0]}))
        }
        13 => {
            let n = dice.below(1000);
            let categories = json!(["walls", "doors", "rooms"]);
            ("schedule".into(), json!({"name": format!("Mixed {n}"), "category": "multicategory", "categories": categories}))
        }
        // A length typed into a dimension; or, now and then, one from a free wall to the next.
        14 => match pick(s, dice, &["dimension"]) {
            Some(d) if dice.below(3) > 0 => ("dimension_value".into(), json!({"id": d, "value": 2000 + dice.below(2000)})),
            _ => {
                let x = free(dice.below(3) as i64);
                ("dimension".into(), json!({"refs": [[x, 2000], [x + 3000, 2000]], "at": [x + 1500, 1500]}))
            }
        },
        15 => ("dimension_constraint".into(), json!({"id": pick(s, dice, &["dimension"]), "locked": [0]})),
        16 => {
            let bars =
                json!({"cover": 25, "bottom": {"count": 2 + dice.below(3), "diameter": 16}, "stirrup": {"diameter": 5, "spacing": 150}});
            let mut p = json!({"host": pick(s, dice, &["beam"])});
            if let (Some(p), Some(bars)) = (p.as_object_mut(), bars.as_object()) {
                p.extend(bars.clone());
            }
            ("reinforce".into(), p)
        }
        17 => ("symbol_set".into(), json!({"set": "arei"})),
        18 => {
            let symbols: Vec<String> =
                s.model.families.iter().filter(|(_, f)| f.forms.is_empty() && !f.symbols.is_empty()).map(|(n, _)| n.clone()).collect();
            match symbols.get(dice.below(symbols.len())) {
                Some(family) => ("component".into(), json!({"family": family, "at": at(dice), "level": level(s, dice)})),
                None => ("symbol_set".into(), json!({"set": "arei"})),
            }
        }
        19 => ("spot_slope".into(), json!({"at": at(dice)})),
        20 => ("voltage".into(), json!({"name": format!("{} V", 100 + dice.below(300)), "nominal": 100 + dice.below(300)})),
        21 => {
            let by = json!([dice.mm(3000), dice.mm(3000)]);
            if dice.below(2) == 0 {
                ("move".into(), json!({"ids": [pick(s, dice, &new)], "by": by}))
            } else {
                ("copy".into(), json!({"ids": [pick(s, dice, &new)], "by": by}))
            }
        }
        22 => ("delete".into(), json!({"ids": [pick(s, dice, &new)]})),
        23 => {
            let x = free(dice.below(3) as i64);
            ("wall".into(), json!({"points": [[x, 0], [x, 4000], [x + 3000, 4000], [x + 3000, 0]], "height": 3000}))
        }
        _ => (if dice.below(2) == 0 { "undo" } else { "redo" }.into(), json!({})),
    }
}

/// The kinds `edit` and `edit_2d` leave out edited at random on a small tower, each command checked by what changed and whole, its library
/// synced both ways, and the plan drawn now and then from the lookup's elements and from every element (the engine and the
/// plan hold both to the same answer in tests).
#[test]
fn random_edits_of_the_other_kinds_check_sync_and_draw_the_same_by_what_changed_as_whole() {
    let model = tower_on(Plate::SMALL, 2).unwrap();
    let mut kinds = std::collections::BTreeSet::new();
    for seed in [1u64, 2, 3] {
        let mut s = Session::new();
        s.reset(model.clone(), None);
        s.execute("move", &json!({"ids": [], "by": [0, 0]})).ok();
        let level = s.model.levels()[0].0;
        let mut dice = Dice(0x3d3d_7a11_c0de_0188 ^ seed);
        let (mut done, mut refused) = (0, 0);
        for n in 0..70 {
            let (cmd, p) = edit_more(&mut s, &mut dice);
            match s.execute(&cmd, &p) {
                Ok(_) => {
                    done += 1;
                    kinds.insert(cmd);
                }
                Err(_) => refused += 1,
            }
            if n % 10 == 9 {
                crate::render::plan(&s.model, level, 800, 600);
            }
        }
        assert!(done > 25 && refused > 0, "seed {seed}: {done} done, {refused} refused: the edits should do both");
        assert_eq!(s.model.check(), Ok(()));
        assert!(s.model.library_in_step());
    }
    for cmd in [
        "dimension_3d",
        "camera",
        "graphic_display_options",
        "work_plane",
        "modify",
        "add_point",
        "roof",
        "schedule",
        "dimension_value",
        "reinforce",
        "symbol_set",
    ] {
        assert!(kinds.contains(cmd), "no `{cmd}` was taken: {kinds:?}");
    }
}
