//! What rooms' own floors and ceilings cost on the benchmark tower: a wall moved with and without them, every room's made
//! at once, and an edit that bounds nothing. `cargo run --release -p bimcraft-core --example room_finishes [storeys] [centre|finish]`.

use std::time::Instant;

use bimcraft_core::engine::Session;
use bimcraft_core::model::Element;
use serde_json::json;

fn main() -> Result<(), String> {
    let storeys: u32 = std::env::args().nth(1).map_or(Ok(10), |s| s.parse().map_err(|e| format!("storeys: {e}")))?;
    let mut s = Session::new();
    s.execute("tower", &json!({"storeys": storeys}))?;
    // Rooms measured at the walls' centres (the default) or at their faces: `centre` or `finish`.
    if let Some(at) = std::env::args().nth(2) {
        s.execute("room_area", &json!({"at": at}))?;
    }
    let count = |s: &Session, f: fn(&Element) -> bool| s.model.elements.values().filter(|e| f(e)).count();
    let rooms = count(&s, |e| matches!(e, Element::Room(_)));
    let walls: Vec<u64> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect();
    let step = |s: &mut Session, wall: u64, by: f64| s.execute("move", &json!({"ids": [wall], "by": [by, by]})).is_ok();
    // A wall from the middle of the list that moves and moves back.
    let wall = walls
        .iter()
        .skip(walls.len() / 2)
        .copied()
        .find(|w| step(&mut s, *w, 10.0) && step(&mut s, *w, -10.0))
        .ok_or("no wall of the tower moves")?;
    let ms = |t: Instant, n: f64| t.elapsed().as_secs_f64() * 1000.0 / n;
    let moved = |s: &mut Session| {
        let t = Instant::now();
        for _ in 0..5 {
            step(s, wall, 10.0);
            step(s, wall, -10.0);
        }
        ms(t, 10.0)
    };
    let note = |s: &mut Session| -> Result<f64, String> {
        let t = Instant::now();
        for _ in 0..5 {
            s.execute("text", &json!({"at": [0, 0], "text": "x"}))?;
        }
        Ok(ms(t, 5.0))
    };
    let (plain, plain_note) = (moved(&mut s), note(&mut s)?);
    let floor = s.model.type_names("floor").first().map(|n| n.to_string()).ok_or("no floor type")?;
    let ceiling = s.model.type_names("ceiling").first().map(|n| n.to_string()).ok_or("no ceiling type")?;
    let before = count(&s, |e| matches!(e, Element::Floor(_) | Element::Ceiling(_)));
    let t = Instant::now();
    s.execute("type", &json!({"category": "room", "name": "Standard", "floor": floor, "ceiling": ceiling}))?;
    let made = ms(t, 1.0);
    let finishes = count(&s, |e| matches!(e, Element::Floor(_) | Element::Ceiling(_))) - before;
    let (followed, followed_note) = (moved(&mut s), note(&mut s)?);
    println!("tower {storeys}: {} elements, {rooms} rooms, room_area {}", s.model.elements.len(), s.model.room_area.name());
    println!("a wall move: {plain:.1} ms without, {followed:.1} ms with rooms' floors and ceilings");
    println!("a text note: {plain_note:.2} ms without, {followed_note:.2} ms with");
    println!("{finishes} floors and ceilings made for the room type at once: {made:.0} ms");
    Ok(())
}
