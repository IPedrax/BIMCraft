//! Reinforcing bars: what a bar bent along its points weighs and measures, and the tubes it makes in 3D. A bar knows no host; the
//! model checks it, and the commands, IFC and the schedules ask this for its lengths.

use crate::model::TOL;
use crate::runs::Geom;
use crate::view3d::{P3, sub};

/// Steel's density, kg per m³.
pub const STEEL: f64 = 7850.0;
/// The thinnest and the thickest bar the model takes, mm.
pub const DIAMETERS: (f64, f64) = (3.0, 100.0);
/// The steel a bar is of unless told: Brazil's CA-50.
pub const GRADE: &str = "CA-50";
/// The most bars one command makes for one host.
pub const MAX_BARS: usize = 1000;
/// The most bars one command makes in all, and the most hosts it arms.
pub const MAX_BARS_ALL: usize = 20_000;
pub const MAX_HOSTS: usize = 1000;

/// The length of the bar along its points, unbent: what it is cut from.
pub fn length(path: &[P3]) -> f64 {
    path.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| norm(sub(*b, *a)))).sum()
}

/// What a bar of `diameter` mm and `length` mm weighs, kg.
pub fn weight_kg(diameter: f64, length: f64) -> f64 {
    std::f64::consts::PI * diameter * diameter / 4.0 * length * 1e-9 * STEEL
}

/// A tube of triangles along each piece of the bar, with the number of triangles at the ends of each, first.
pub fn tubes(path: &[P3], diameter: f64) -> Vec<(Vec<[P3; 3]>, usize)> {
    path.windows(2)
        .filter_map(|w| w.first().zip(w.get(1)))
        .filter(|(a, b)| norm(sub(**b, **a)) > TOL)
        .map(|(a, b)| Geom { a: *a, b: *b, width: diameter, depth: diameter, round: true }.tube())
        .collect()
}

fn norm(v: P3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bars_length_is_its_pieces_and_its_weight_is_its_volume_of_steel() {
        let hoop = [[0.0, 0.0, 0.0], [140.0, 0.0, 0.0], [140.0, 440.0, 0.0], [0.0, 440.0, 0.0], [0.0, 0.0, 0.0]];
        assert_eq!(length(&hoop), 1160.0);
        assert_eq!(length(&hoop[..1]), 0.0);
        // 12.5 mm steel weighs 0.963 kg a metre, as the bar tables have it.
        assert!((weight_kg(12.5, 1000.0) - 0.963).abs() < 1e-3);
        assert_eq!(tubes(&hoop, 8.0).len(), 4);
        // A repeated point makes no tube: a tube of no length has no direction.
        assert_eq!(tubes(&[hoop[0], hoop[0], hoop[1]], 8.0).len(), 1);
    }
}
