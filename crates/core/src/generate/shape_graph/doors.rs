//! The doors of a plan: their families and widths, where they stand along a wall, where a cased opening is, and the families the
//! script defines (the folding, unequal double, double sliding and pocket doors; the others are BIMCraft's own).

use serde_json::{Map, Value, json};

use super::family::Conn;

/// Wall left each side of a door, mm.
pub const MARGIN: i64 = 150;
/// The wall left each side of a cased opening, and the shortest the wall may be longer than it, mm.
const CASED_SIDE: i64 = 300;
const CASED_MIN: i64 = 900;
const CASED_MAX: i64 = 2400;

/// A door's family, type name, width and operation, by its connection type and how long a run of wall it has.
pub fn door_for(conn: Conn, run: i64) -> Option<(&'static str, String, i64, &'static str)> {
    let (family, op) = conn.door()?;
    let widths: &[i64] = match conn {
        Conn::Single => &[900, 800, 750],
        Conn::Double => &[1800, 1600, 1500, 1200],
        Conn::UnequalDouble => &[1500, 1200],
        Conn::Sliding => &[1500, 1200, 900, 750],
        Conn::DoubleSliding => &[1800, 1600, 1500, 1200],
        Conn::Pocket => &[900, 800, 750],
        _ => &[900, 800, 750],
    };
    widths.iter().copied().find(|w| run >= w + 2 * MARGIN).map(|w| (family, format!("{w} x 2100"), w, op))
}

/// The along-wall position of a door of width `w` on a stretch of `len` from `lo`: `t` (from 0 up to 1) picks anywhere that
/// leaves at least 150 mm of wall each side.
pub fn door_centre(lo: i64, len: i64, w: i64, t: f64) -> i64 {
    let room = (len - w - 2 * MARGIN).max(0);
    lo + MARGIN + w / 2 + (t.clamp(0.0, 0.999_999) * room as f64).floor() as i64
}

/// A cased opening in a stretch of `len` from `lo`: its two ends. The gap is 900 to 2400 mm, at least 600 shorter than the
/// stretch, with at least 300 mm of wall each side; `t1` picks its length and `t2` its place. None if the stretch is too short.
pub fn cased_gap(lo: i64, len: i64, t1: f64, t2: f64) -> Option<(i64, i64)> {
    let most = CASED_MAX.min(len - 2 * CASED_SIDE);
    if most < CASED_MIN {
        return None;
    }
    let g = CASED_MIN + (t1.clamp(0.0, 0.999_999) * (most - CASED_MIN + 1) as f64).floor() as i64;
    let g = (g / 10 * 10).max(CASED_MIN);
    let spare = len - 2 * CASED_SIDE - g;
    let start = lo + CASED_SIDE + (t2.clamp(0.0, 0.999_999) * (spare + 1) as f64).floor() as i64;
    Some((start.min(lo + len - CASED_SIDE - g), start.min(lo + len - CASED_SIDE - g) + g))
}

/// What the script defines for a family: its `family` command, or none when BIMCraft has the family built in.
pub fn definition(family: &str, types: Map<String, Value>) -> Option<Value> {
    let jamb = |lo: &str, hi: &str| {
        json!({
            "plane": "xy",
            "profile": [[lo, "-Wall Thickness / 2"], [hi, "-Wall Thickness / 2"], [hi, "Wall Thickness / 2"], [lo, "Wall Thickness / 2"]],
            "from": "0", "to": "Height", "material": "Frame Material",
        })
    };
    let leaf_form = |lo: &str, hi: &str, y: &str| {
        json!({
            "plane": "xy",
            "profile": [[lo, format!("-{y}")], [hi, format!("-{y}")], [hi, y], [lo, y]],
            "from": "0", "to": "Height - Frame Width", "material": "Frame Material",
        })
    };
    let parameters = json!([
        {"name": "Width", "kind": "length", "value": 1500},
        {"name": "Height", "kind": "length", "value": 2100},
        {"name": "Frame Width", "kind": "length", "value": 50},
        {"name": "Frame Material", "kind": "material", "value": "Timber"},
    ]);
    let (left, right) = ("-Width / 2 + Frame Width", "Width / 2 - Frame Width");
    let frame = vec![jamb("-Width / 2", left), jamb(right, "Width / 2")];
    let make = |operation: &str, mut forms: Vec<Value>, symbols: Vec<Value>| {
        forms.splice(0..0, frame.clone());
        json!({
            "name": family, "category": "door", "operation": operation, "parameters": parameters,
            "types": types, "forms": forms, "symbols": symbols,
        })
    };
    match family {
        super::super::house::FOLDING_FAMILY => Some(super::super::house::folding_family(types)),
        "Unequal Double Door" => {
            // A wide leaf (two thirds of the scheduled width) on the left jamb and a narrow one (a third) on the right.
            let (wide, narrow) = ("Width * 2 / 3", "Width / 3");
            Some(make(
                "double-swing",
                vec![leaf_form(left, "-Width / 2 + Frame Width + 1", "20"), leaf_form(right, "Width / 2 - Frame Width - 1", "20")],
                vec![
                    json!({"line": [[left, "Wall Thickness / 2"], [left, format!("Wall Thickness / 2 + {wide}")]]}),
                    json!({"arc": {"centre": [left, "Wall Thickness / 2"], "radius": wide, "from": 0, "to": 90}}),
                    json!({"line": [[right, "Wall Thickness / 2"], [right, format!("Wall Thickness / 2 + {narrow}")]]}),
                    json!({"arc": {"centre": [right, "Wall Thickness / 2"], "radius": narrow, "from": 90, "to": 180}}),
                ],
            ))
        }
        "Double Sliding Door" => {
            // Two leaves parting from the middle on one track, each drawn from its jamb to just short of the middle.
            Some(make(
                "double-sliding",
                vec![leaf_form(left, "-25", "20"), leaf_form("25", right, "20")],
                vec![
                    json!({"line": [[left, "0"], ["-30", "0"]]}),
                    json!({"line": [["30", "0"], [right, "0"]]}),
                    json!({"line": [["-30", "-30"], ["-30", "30"]]}),
                    json!({"line": [["30", "-30"], ["30", "30"]]}),
                ],
            ))
        }
        "Pocket Door" => {
            // One leaf across the opening on the wall's centre line, and the pocket marked at the right jamb.
            Some(make(
                "sliding",
                vec![leaf_form(left, right, "20")],
                vec![
                    json!({"line": [[left, "0"], [right, "0"]]}),
                    json!({"line": [[right, "-Frame Width"], ["Width / 2", "-Frame Width"], ["Width / 2", "Frame Width"]]}),
                ],
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doors_leave_150_each_side_and_cased_gaps_fit() {
        for conn in [Conn::Single, Conn::Double, Conn::UnequalDouble, Conn::Sliding, Conn::DoubleSliding, Conn::Pocket, Conn::Bifold] {
            for len in (conn.need()..=6000).step_by(70) {
                let (_, _, w, _) = door_for(conn, len).expect("a wall of the type's need holds one of its doors");
                for t in [0.0, 0.5, 0.9999] {
                    let c = door_centre(1000, len, w, t);
                    assert!(c - w / 2 - 1000 >= 150 && 1000 + len - (c + w / 2) >= 150, "{conn:?} {len} {t}: {c}");
                }
            }
        }
        for len in (Conn::Cased.need()..=6000).step_by(10) {
            for (t1, t2) in [(0.0, 0.0), (0.5, 0.5), (0.9999, 0.9999), (0.3, 0.0), (0.0, 0.9999)] {
                let (a, b) = cased_gap(500, len, t1, t2).expect("the need holds a gap");
                assert!((900..=2400).contains(&(b - a)), "{len}: gap {}", b - a);
                assert!(len - (b - a) >= 600, "{len}");
                assert!(a - 500 >= 300 && 500 + len - b >= 300, "{len}: {a} {b}");
            }
        }
        assert!(cased_gap(0, 1400, 0.5, 0.5).is_none());
    }

    #[test]
    fn unequal_double_leaves_differ_by_200() {
        let def = definition("Unequal Double Door", Map::new()).expect("defined by the script");
        assert_eq!(def["operation"], "double-swing");
        assert_eq!(def["category"], "door");
        assert_eq!(def["symbols"].as_array().map(Vec::len), Some(4));
        let text = def["symbols"].to_string();
        assert!(text.contains("Width * 2 / 3") && text.contains("Width / 3"), "{text}");
    }
}
