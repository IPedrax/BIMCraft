//! The graph of a studio office, sampled by the seed: how many WCs, meeting rooms and stores there are, in what order they lie
//! along the corridor, how wide the corridor is, how the doors swing, and whether the studio has a way out of its own. Every
//! choice is inside what the studio-office pack allows, so the graph passes its checker before any geometry exists for it.

use super::super::Rng;
use super::{Kind, Link, Means, Room, StudioGraph};

const SALT: u64 = 0x5700_0001;

fn push(rooms: &mut Vec<Room>, id: String, name: String, kind: Kind) -> usize {
    rooms.push(Room { id, name, kind });
    rooms.len() - 1
}

/// The graph of a studio office of about `area_m2` square metres.
pub fn sample(seed: u64, area_m2: f64) -> StudioGraph {
    let mut rng = Rng::new(seed, SALT);
    let mut chance = |percent: u64| rng.below(100) < percent;
    let wc_count = if area_m2 < 300.0 {
        2 + usize::from(chance(40)) + usize::from(chance(25))
    } else {
        3 + usize::from(chance(55)) + usize::from(chance(40))
    };
    let meetings = 1 + usize::from(area_m2 >= 300.0 && chance(55));
    let stores = 1 + usize::from(area_m2 >= 330.0 && chance(55));
    let studio_door = chance(70);
    let mut rng = Rng::new(seed, SALT + 1);
    let corridor_width = [1500, 2000, 2400][rng.below(3) as usize];
    let double_meeting: Vec<bool> = (0..meetings).map(|_| rng.below(100) < 50).collect();

    let mut rooms = Vec::new();
    let reception = push(&mut rooms, "reception".into(), "Reception".into(), Kind::Reception);
    let corridor = push(&mut rooms, "corridor".into(), "Corridor".into(), Kind::Hall);
    let passage = push(&mut rooms, "passage".into(), "Passage".into(), Kind::Hall);
    let studio = push(&mut rooms, "studio".into(), "Studio".into(), Kind::Office);
    let wcs: Vec<usize> = (1..=wc_count).map(|k| push(&mut rooms, format!("wc_{k}"), format!("WC {k}"), Kind::Bath)).collect();
    let mut order: Vec<(Kind, usize)> = (0..meetings).map(|k| (Kind::Meeting, k)).chain((0..stores).map(|k| (Kind::Storage, k))).collect();
    let mut rng = Rng::new(seed, SALT + 2);
    rng.shuffle(&mut order);
    let mut upper = Vec::new();
    let mut links = vec![
        Link { a: reception, b: corridor, means: if corridor_width >= 2000 { Means::DoubleSwing } else { Means::Swing } },
        Link { a: reception, b: passage, means: Means::Swing },
        Link { a: passage, b: studio, means: Means::Open },
        Link { a: corridor, b: studio, means: Means::Swing },
    ];
    for &w in &wcs {
        links.push(Link { a: passage, b: w, means: Means::Swing });
    }
    for (kind, k) in order {
        let (id, name, means) = match kind {
            Kind::Meeting => {
                let (id, name) = if meetings == 1 {
                    ("meeting".into(), "Meeting Room".into())
                } else {
                    (format!("meeting_{}", k + 1), format!("Meeting Room {}", k + 1))
                };
                (id, name, if double_meeting[k] { Means::DoubleSwing } else { Means::Swing })
            }
            _ => {
                let (id, name) =
                    if stores == 1 { ("store".into(), "Store".into()) } else { (format!("store_{}", k + 1), format!("Store {}", k + 1)) };
                (id, name, Means::Swing)
            }
        };
        let i = push(&mut rooms, id, name, kind);
        links.push(Link { a: corridor, b: i, means });
        upper.push(i);
    }
    StudioGraph { rooms, links, entry: reception, reception, corridor, passage, studio, wcs, upper, studio_door, corridor_width }
}
