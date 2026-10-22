//! Original procedural office tower; never derived from a customer's IFC.
use std::collections::BTreeMap;

use serde_json::json;

use crate::engine::Session;
use crate::model::{Element, Level, Model};

/// A 48 × 36 m tower, with 6 m bays and 3.6 m storeys. Build one floor through the
/// normal commands, then instance its elements with fresh identities and remapped hosts.
/// This avoids thousands of throwaway undo snapshots while constructing a fixture.
pub fn highrise(storeys: u32) -> Result<Model, String> {
    if !(1..=80).contains(&storeys) {
        return Err("highrise: `storeys` must be between 1 and 80".into());
    }
    let mut s = Session::new();
    s.execute("new", &json!({"name": "Synthetic Highrise"}))?;
    let boundary = json!([[0, 0], [48000, 0], [48000, 36000], [0, 36000]]);
    s.execute("floor", &json!({"boundary": boundary, "structural": true}))?;
    s.execute("ceiling", &json!({"boundary": boundary, "offset": 3100}))?;
    for x in 0..=8 {
        for y in 0..=6 {
            s.execute("structural_column", &json!({"at": [x * 6000, y * 6000], "height": 3600}))?;
        }
    }
    for y in 0..=6 {
        let points: Vec<_> = (0..=8).map(|x| [x * 6000, y * 6000]).collect();
        s.execute("beam", &json!({"points": points, "offset": 3300}))?;
    }
    for x in 0..=8 {
        let points: Vec<_> = (0..=6).map(|y| [x * 6000, y * 6000]).collect();
        s.execute("beam", &json!({"points": points, "offset": 3300}))?;
    }
    // Clockwise facade panels with a window in each, and an enclosed service core.
    for (a, b, n) in [([0, 0], [0, 36000], 6), ([0, 36000], [48000, 36000], 8), ([48000, 36000], [48000, 0], 6), ([48000, 0], [0, 0], 8)] {
        for k in 0..n {
            let at = |j| [a[0] + (b[0] - a[0]) * j / n, a[1] + (b[1] - a[1]) * j / n];
            let (p, q) = (at(k), at(k + 1));
            s.execute("wall", &json!({"points": [p, q], "height": 3600, "type": "Exterior - 300mm"}))?;
            s.execute("window", &json!({"at": [(p[0]+q[0])/2, (p[1]+q[1])/2]}))?;
        }
    }
    s.execute(
        "wall",
        &json!({"points": [[18000, 12000], [18000, 24000], [30000, 24000], [30000, 12000]],
        "closed": true, "height": 3600}),
    )?;
    // Five simple original furniture instances in every bay; no external content.
    for x in 0..8 {
        for y in 0..6 {
            for (family, dx, dy) in
                [("Desk", 1800, 2000), ("Chair", 1800, 1000), ("Desk", 4200, 4000), ("Chair", 4200, 3000), ("Base Cabinet", 1000, 5000)]
            {
                s.execute("component", &json!({"family": family, "at": [x*6000+dx, y*6000+dy]}))?;
            }
        }
    }
    let source = &s.model;
    let base = source.levels().first().map(|(id, _)| source.guid(*id)).ok_or("highrise: no base level")?;
    let mut m = (**source).clone();
    m.elements.clear();
    for floor in 0..=storeys {
        m.insert(Element::Level(Level::new(format!("Level {}", floor + 1), f64::from(floor) * 3600.0)));
    }
    for e in source.elements.values().filter(|e| matches!(e, Element::View3D(_))) {
        m.insert(e.clone());
    }
    let levels: Vec<_> = m.levels().iter().map(|(id, _)| m.guid(*id)).collect();
    for level in levels.into_iter().take(storeys as usize) {
        let mut map = BTreeMap::from([(base, level)]);
        let mut inserted = Vec::new();
        for (&old, e) in &source.elements {
            if matches!(e, Element::Level(_) | Element::View3D(_)) {
                continue;
            }
            let id = m.insert(e.clone());
            map.insert(source.guid(old), m.guid(id));
            inserted.push(id);
        }
        for id in inserted {
            if let Some(e) = m.elements.get_mut(id) {
                e.refs_mut(&mut |g| {
                    if let Some(new) = map.get(g) {
                        *g = *new;
                    }
                });
            }
        }
    }
    m.sync_library();
    Ok(m)
}
