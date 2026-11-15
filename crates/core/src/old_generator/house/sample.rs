//! The graph of a house, sampled by the seed: how many rooms of each kind, which have a closet or an en-suite bath, how the
//! shared rooms are joined and where the way in and the porch are. Every choice is made inside what the house rule pack
//! allows (`data/graph-rules/house.json`), so the graph passes its checker before any geometry exists for it.

use super::super::Rng;
use super::{Bed, HouseGraph, Kind, LARGE_AREA, Link, Means, Room};

const SALT: u64 = 0x4A05_E001;

fn push(rooms: &mut Vec<Room>, id: String, name: String, kind: Kind) -> usize {
    rooms.push(Room { id, name, kind });
    rooms.len() - 1
}

/// The graph of a house of about `area_m2` square metres.
pub fn sample(seed: u64, area_m2: f64) -> HouseGraph {
    if area_m2 > LARGE_AREA {
        return large(seed, area_m2);
    }
    let mut rng = Rng::new(seed, SALT);
    let mut chance = |percent: u64| rng.below(100) < percent;
    let bedrooms: usize = match area_m2 {
        a if a < 78.0 => 1,
        a if a < 100.0 => 1 + usize::from(chance(60)),
        a if a < 125.0 => 2,
        a if a < 150.0 => 2 + usize::from(chance(60)),
        _ => 3,
    };
    let dining = area_m2 >= 100.0 && chance(if area_m2 >= 125.0 { 90 } else { 45 });
    let porch = area_m2 >= 55.0 && chance(60);
    let cap_kind = if chance(45) { Some(if chance(50) { Kind::Closet } else { Kind::Laundry }) } else { None };
    let hall_open = chance(40);
    let first_open = chance(50);

    let mut rooms = Vec::new();
    let mut public: Vec<usize> = Vec::new();
    let mut order = vec![Kind::Living, Kind::Kitchen];
    if dining {
        order.push(Kind::Dining);
    }
    let mut rng = Rng::new(seed, SALT + 1);
    rng.shuffle(&mut order);
    for kind in order {
        public.push(push(&mut rooms, kind.name().into(), kind.label().into(), kind));
    }
    let hall = push(&mut rooms, "hall".into(), "Hall".into(), Kind::Hall);

    let mut rng = Rng::new(seed, SALT + 2);
    let mut beds = Vec::new();
    for k in 1..=bedrooms {
        let room = push(&mut rooms, format!("bedroom_{k}"), format!("Bedroom {k}"), Kind::Bedroom);
        beds.push(Bed { room, closet: None, bath: None });
    }
    let hall_bath = push(&mut rooms, "bath".into(), "Bath".into(), Kind::Bath);
    let mut links = Vec::new();
    for (k, bed) in beds.iter_mut().enumerate() {
        let k = k + 1;
        if rng.below(100) < 72 {
            bed.closet = Some(push(&mut rooms, format!("closet_{k}"), format!("Closet {k}"), Kind::Closet));
        }
        if rng.below(100) < 30 {
            bed.bath = Some(push(&mut rooms, format!("ensuite_{k}"), format!("Ensuite {k}"), Kind::Bath));
        }
    }
    let sliding_closets: Vec<bool> = beds.iter().map(|_| rng.below(100) < 60).collect();
    let cap = cap_kind.map(|kind| push(&mut rooms, kind.name().into(), kind.label().into(), kind));
    let mut rng = Rng::new(seed, SALT + 3);
    let porch_off = public[rng.below(public.len() as u64) as usize];
    let porch = porch.then(|| push(&mut rooms, "porch".into(), "Porch".into(), Kind::Porch));
    let hall_to = public[rng.below(public.len() as u64) as usize];
    let porch_sliding = rng.below(100) < 50;

    // The shared rooms lie in a row; neighbours are joined, and a dining room only openly.
    let mut opened = false;
    for pair in public.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let needs_open = rooms[a].kind == Kind::Dining || rooms[b].kind == Kind::Dining;
        let open = needs_open || (first_open && !opened);
        opened |= open;
        links.push(Link { a, b, means: if open { Means::Open } else { Means::Swing } });
    }
    // A porch is closed off by room separation lines, which a house with no open edge has none of.
    if porch.is_some()
        && !opened
        && let Some(link) = links.first_mut()
    {
        link.means = Means::Open;
    }
    links.push(Link { a: hall, b: hall_to, means: if hall_open { Means::Open } else { Means::Swing } });
    for bed in &beds {
        links.push(Link { a: hall, b: bed.room, means: Means::Swing });
    }
    links.push(Link { a: hall, b: hall_bath, means: Means::Swing });
    for (bed, sliding) in beds.iter().zip(sliding_closets) {
        if let Some(c) = bed.closet {
            links.push(Link { a: bed.room, b: c, means: if sliding { Means::Sliding } else { Means::Swing } });
        }
        if let Some(b) = bed.bath {
            links.push(Link { a: bed.room, b, means: Means::Swing });
        }
    }
    if let Some(c) = cap {
        links.push(Link { a: hall, b: c, means: Means::Swing });
    }
    if let Some(p) = porch {
        links.push(Link { a: porch_off, b: p, means: if porch_sliding { Means::Sliding } else { Means::Swing } });
    }
    // The way in: a living room or a kitchen, never the room the porch is off.
    let living = rooms.iter().position(|r| r.kind == Kind::Living).unwrap_or(0);
    let kitchen = rooms.iter().position(|r| r.kind == Kind::Kitchen).unwrap_or(0);
    let entry = match (porch.is_some(), porch_off == living, porch_off == kitchen) {
        (true, true, _) => kitchen,
        (true, _, true) => living,
        _ => {
            if rng.below(100) < 60 {
                living
            } else {
                kitchen
            }
        }
    };
    HouseGraph { rooms, links, entry, public, hall, hall_to, beds, hall_bath, cap, porch, porch_off, deck: None, deck_off: Vec::new() }
}

/// The graph of a house above `LARGE_AREA`, which the traced Unit 1 house is the model of: an entry hall that is the way in,
/// with the public rooms open to it along one band, a garage beside it reached by a door from it, the hall of the bedroom wing
/// opening into it, and a deck off the living and dining rooms. Its choices are drawn from salts of their own, so the houses
/// of `LARGE_AREA` and below do not change.
fn large(seed: u64, area_m2: f64) -> HouseGraph {
    let mut rng = Rng::new(seed, SALT + 4);
    let mut chance = |percent: u64| rng.below(100) < percent;
    let bedrooms = if area_m2 < 215.0 { 3 } else { 4 };
    // The biggest houses always have one: without it the shared rooms would be larger than a house's rooms are.
    let garage = chance(70) || area_m2 >= 240.0;
    let deck = chance(65);
    let deck_two = chance(75);
    let entry_open = chance(85);
    let shared_open = chance(85);
    let cap_kind = if chance(55) { Some(if chance(50) { Kind::Closet } else { Kind::Laundry }) } else { None };
    let cap_folding = chance(55);

    let mut rooms = Vec::new();
    let kitchen = push(&mut rooms, "kitchen".into(), "Kitchen".into(), Kind::Kitchen);
    let living = push(&mut rooms, "living".into(), "Living".into(), Kind::Living);
    let dining = push(&mut rooms, "dining".into(), "Dining".into(), Kind::Dining);
    let entry = push(&mut rooms, "entry".into(), "Entry".into(), Kind::Hall);
    let garage = garage.then(|| push(&mut rooms, "garage".into(), "Garage".into(), Kind::Garage));
    let hall = push(&mut rooms, "hall".into(), "Hall".into(), Kind::Hall);

    let mut rng = Rng::new(seed, SALT + 5);
    let mut beds = Vec::new();
    for k in 1..=bedrooms {
        let room = push(&mut rooms, format!("bedroom_{k}"), format!("Bedroom {k}"), Kind::Bedroom);
        beds.push(Bed { room, closet: None, bath: None });
    }
    let hall_bath = push(&mut rooms, "bath".into(), "Bath".into(), Kind::Bath);
    let mut ensuites = 0;
    for (k, bed) in beds.iter_mut().enumerate() {
        let k = k + 1;
        // Every bedroom has a closet, so that the bedrooms' strips are alike and the two columns come out level; the
        // ensuite baths are at most two, which with the hall's bath keeps the house to the pack's four.
        bed.closet = Some(push(&mut rooms, format!("closet_{k}"), format!("Closet {k}"), Kind::Closet));
        if rng.below(100) < 30 && ensuites < 2 {
            ensuites += 1;
            bed.bath = Some(push(&mut rooms, format!("ensuite_{k}"), format!("Ensuite {k}"), Kind::Bath));
        }
    }
    // A closet's door: swing, sliding or folding.
    let closet_means: Vec<Means> = beds
        .iter()
        .map(|_| match rng.below(100) {
            0..=39 => Means::Folding,
            40..=69 => Means::Sliding,
            _ => Means::Swing,
        })
        .collect();
    let cap = cap_kind.map(|kind| push(&mut rooms, kind.name().into(), kind.label().into(), kind));

    // The public band, west to east. Living and dining are neighbours so that one deck can serve both.
    let mut rng = Rng::new(seed, SALT + 6);
    let public: Vec<usize> = match (garage, rng.below(4)) {
        (Some(g), 0 | 1) => vec![g, entry, kitchen, living, dining],
        (Some(g), _) => vec![g, entry, living, dining, kitchen],
        (None, 0) => vec![kitchen, entry, living, dining],
        (None, 1) => vec![kitchen, entry, dining, living],
        (None, 2) => vec![living, dining, entry, kitchen],
        (None, _) => vec![dining, living, entry, kitchen],
    };
    let deck = deck.then(|| push(&mut rooms, "deck".into(), "Deck".into(), Kind::Deck));
    let deck_off = match deck {
        None => Vec::new(),
        Some(_) if deck_two => vec![living, dining],
        Some(_) => vec![living],
    };

    let mut links = Vec::new();
    for pair in public.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (kind_a, kind_b) = (rooms[a].kind, rooms[b].kind);
        let means = if kind_a == Kind::Garage || kind_b == Kind::Garage {
            Means::Swing
        } else if kind_a == Kind::Hall || kind_b == Kind::Hall {
            if entry_open { Means::Open } else { Means::Swing }
        } else if kind_a == Kind::Dining || kind_b == Kind::Dining || shared_open {
            Means::Open
        } else {
            Means::Swing
        };
        links.push(Link { a, b, means });
    }
    // The bedroom wing's hall opens into the entry hall; the pack allows no other means between two halls.
    links.push(Link { a: hall, b: entry, means: Means::Open });
    for bed in &beds {
        links.push(Link { a: hall, b: bed.room, means: Means::Swing });
    }
    links.push(Link { a: hall, b: hall_bath, means: Means::Swing });
    for (bed, means) in beds.iter().zip(closet_means) {
        if let Some(c) = bed.closet {
            links.push(Link { a: bed.room, b: c, means });
        }
        if let Some(b) = bed.bath {
            links.push(Link { a: bed.room, b, means: Means::Swing });
        }
    }
    if let Some(c) = cap {
        links.push(Link { a: hall, b: c, means: if cap_folding { Means::Folding } else { Means::Swing } });
    }
    if let Some(d) = deck {
        for &off in &deck_off {
            links.push(Link { a: off, b: d, means: Means::Sliding });
        }
    }
    HouseGraph { rooms, links, entry, public, hall, hall_to: entry, beds, hall_bath, cap, porch: None, porch_off: living, deck, deck_off }
}
