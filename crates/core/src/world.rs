//! An offline world map, for the place a project's location is on the globe: Natural Earth's 1:110m coastlines and land
//! borders (public domain), packed small in `assets/map/world.bin` (34 KB; how, in its README). A placeholder until a tile
//! map ([coordinates](../../../docs/areas/coordinates.md)).

use std::sync::OnceLock;

/// A line of the map: a land ring (closed) or a border, as longitude and latitude, degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub land: bool,
    pub points: Vec<(f32, f32)>,
}

const WORLD: &[u8] = include_bytes!("../../../assets/map/world.bin");

/// Every line of the map, read once. None of it if the file were ever unreadable: a map is a nicety, never a failure.
pub fn lines() -> &'static [Line] {
    static LINES: OnceLock<Vec<Line>> = OnceLock::new();
    LINES.get_or_init(|| read(WORLD).unwrap_or_default())
}

fn read(b: &[u8]) -> Option<Vec<Line>> {
    let mut at = b.strip_prefix(b"BCWM1")?;
    let mut take = |n: usize| -> Option<&[u8]> {
        let (head, rest) = (at.get(..n)?, at.get(n..)?);
        at = rest;
        Some(head)
    };
    let count = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
    let mut out = Vec::with_capacity(count.min(10_000));
    for _ in 0..count {
        let land = take(1)?[0] == 1;
        let n = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let mut points = Vec::with_capacity(n);
        for _ in 0..n {
            let p = take(4)?;
            let lon = i16::from_le_bytes([p[0], p[1]]);
            let lat = i16::from_le_bytes([p[2], p[3]]);
            points.push((f32::from(lon) / 100.0, f32::from(lat) / 100.0));
        }
        out.push(Line { land, points });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_has_the_continents_and_their_borders_on_the_globe() {
        let all = lines();
        assert!(all.iter().filter(|l| l.land).count() > 100 && all.iter().any(|l| !l.land), "{}", all.len());
        assert!(all.iter().flat_map(|l| &l.points).all(|(lon, lat)| lon.abs() <= 180.0 && lat.abs() <= 90.0));
        // São Paulo's coast is there: a land point within a degree of 46.3° W, 24° S.
        assert!(
            all.iter().filter(|l| l.land).flat_map(|l| &l.points).any(|(lon, lat)| (lon + 46.3).abs() < 1.0 && (lat + 24.0).abs() < 1.0)
        );
    }

    #[test]
    fn a_broken_map_reads_as_none_and_never_panics() {
        assert_eq!(read(b""), None);
        assert_eq!(read(b"BCWM1\xff\xff\xff\xff\x01\x05\x00"), None);
        assert_eq!(read(&WORLD[..WORLD.len() - 3]), None);
    }
}
