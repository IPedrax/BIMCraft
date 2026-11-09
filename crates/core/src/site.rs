//! Property boundary geometry. Coordinates are millimetres, x east and y north.
use crate::geom::P2;

/// Survey bearing (north/south toward east/west), and azimuth clockwise from north.
/// Both are rounded to the nearest second; integer arithmetic carries seconds into minutes/degrees.
pub fn bearing(a: P2, b: P2) -> (String, f64) {
    let d = b - a;
    let azimuth = d.0.atan2(d.1).to_degrees().rem_euclid(360.0);
    let seconds = (azimuth * 3600.0).round() as i64 % (360 * 3600);
    let (ns, angle, ew) = match seconds {
        s if s <= 90 * 3600 => ("N", s, "E"),
        s if s <= 180 * 3600 => ("S", 180 * 3600 - s, "E"),
        s if s <= 270 * 3600 => ("S", s - 180 * 3600, "W"),
        s => ("N", 360 * 3600 - s, "W"),
    };
    (format!("{ns} {}°{:02}'{:02}\" {ew}", angle / 3600, angle / 60 % 60, angle % 60), seconds as f64 / 3600.0)
}

/// Reuse the bounded sketch solver: reject crossings, touching nonadjacent edges, spikes and zero-length edges.
/// Unlike legacy slab outlines, property lines have no historical malformed geometry to preserve.
pub(crate) fn check(who: &str, points: &[P2]) -> Result<(), String> {
    crate::sketch::solve_one(&crate::sketch::lines_of(points, &[])).map(|_| ()).map_err(|problems| {
        format!("{who}: its property boundary isn't simple: {}", crate::sketch::say(&problems, &|i| format!("edge {i}")))
    })
}
