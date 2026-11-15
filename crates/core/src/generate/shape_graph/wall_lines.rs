//! Wall lines: each plan draws its exterior and its interior walls with their own pen, lightness and fill, chosen by the seed
//! or pinned by `--ext-pen`, `--ext-light`, `--int-pen` and `--int-light`. The script applies them with one view filter per
//! wall type and a cut override on the plan view; the manifest's `wall_lines` records them.
//!
//! The choice has a random stream of its own, so the layout is the same with and without a style option.

use serde_json::{Value, json};

use super::emit::{EXTERIOR, INTERIOR};
use super::fit::Rnd;
use crate::pen::Pen;

/// The exterior pen's range, and the interior's.
pub const EXT_PEN: (i64, i64) = (8, 14);
pub const INT_PEN: (i64, i64) = (3, 9);
/// The lightness ranges, percent grey.
pub const EXT_LIGHT: (i64, i64) = (0, 45);
pub const INT_LIGHT: (i64, i64) = (0, 65);

/// The stream's salt: `u64::MAX` and `u64::MAX - 1` are the shape order and the interior choice.
const SALT: u64 = u64::MAX - 2;
/// The grey of BIMCraft's default wall fill (`render::POCHE`).
const GREY_FILL: &str = "#a8a8a8";
const WHITE_FILL: &str = "#ffffff";
/// The lineweights a DXF may hold, hundredths of a mm (`render/dxf.rs`).
const DXF_WEIGHTS: [i64; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

/// What the call pins; the seed chooses the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pins {
    pub ext_pen: Option<i64>,
    pub ext_light: Option<i64>,
    pub int_pen: Option<i64>,
    pub int_light: Option<i64>,
}

impl Pins {
    /// What is wrong with the pins, said so it can be fixed.
    pub fn check(&self) -> Result<(), String> {
        let ranges = [
            ("ext_pen", self.ext_pen, EXT_PEN),
            ("ext_light", self.ext_light, EXT_LIGHT),
            ("int_pen", self.int_pen, INT_PEN),
            ("int_light", self.int_light, INT_LIGHT),
        ];
        for (name, v, (lo, hi)) in ranges {
            if let Some(v) = v
                && !(lo..=hi).contains(&v)
            {
                return Err(format!("`{name}` is a whole number from {lo} to {hi}; got {v}"));
            }
        }
        if let (Some(e), Some(i)) = (self.ext_pen, self.int_pen)
            && e < i
        {
            return Err(format!("the exterior pen is never thinner than the interior pen: `ext_pen` {e} is under `int_pen` {i}"));
        }
        Ok(())
    }
}

/// One kind of wall's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub pen: i64,
    pub lightness: i64,
    pub fill: &'static str,
}

impl Side {
    /// The grey's channel: `255 * L / 100`, rounded (in whole numbers, so 10 is 26).
    fn channel(&self) -> i64 {
        (255 * self.lightness + 50) / 100
    }

    pub fn color(&self) -> String {
        let v = self.channel();
        format!("#{v:02x}{v:02x}{v:02x}")
    }

    fn pen_mm(&self) -> f64 {
        Pen(self.pen as u8).mm()
    }

    /// What a DXF writes for it: the lineweight nearest its width and the grey's channel (0..=255, not group 420's
    /// `channel * 0x010101`). Only for telling two sides apart.
    pub fn dxf_pair(&self) -> (i64, i64) {
        let hundredths = (self.pen_mm() * 100.0).round() as i64;
        let lw = DXF_WEIGHTS.iter().copied().min_by_key(|w| (w - hundredths).abs()).unwrap_or(0);
        (lw, self.channel())
    }

    fn json(&self) -> Value {
        json!({"pen": self.pen, "lineweight_mm": self.pen_mm(), "lightness": self.lightness, "color": self.color(), "fill": self.fill})
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallLines {
    pub exterior: Side,
    pub interior: Side,
}

/// The seed's choice, with the pins kept.
pub fn choose(seed: u64, pins: &Pins) -> WallLines {
    let mut rng = Rnd::new(seed, SALT);
    // Fixed order, so a pin never moves what the seed chose for the rest.
    let mut draw = |(lo, hi): (i64, i64)| lo + rng.below((hi - lo + 1) as usize) as i64;
    let ext_light = draw(EXT_LIGHT);
    let mut int_light = draw(INT_LIGHT);
    let fills = [draw((0, 1)), draw((0, 1))];
    let ext_pen_draw = draw(EXT_PEN);
    let int_pen_draw = draw(INT_PEN);
    let spare = draw(INT_LIGHT);
    let fill = |n: i64| if n == 0 { WHITE_FILL } else { GREY_FILL };
    // The exterior pen is the pinned one, or the draw folded into what the interior allows; the interior pen then the same.
    let ext_pen = pins.ext_pen.unwrap_or_else(|| {
        let floor = EXT_PEN.0.max(pins.int_pen.unwrap_or(0));
        floor + (ext_pen_draw - EXT_PEN.0) % (EXT_PEN.1 - floor + 1)
    });
    let int_pen = pins.int_pen.unwrap_or_else(|| {
        let top = INT_PEN.1.min(ext_pen);
        INT_PEN.0 + (int_pen_draw - INT_PEN.0) % (top - INT_PEN.0 + 1)
    });
    let exterior = Side { pen: ext_pen, lightness: pins.ext_light.unwrap_or(ext_light), fill: fill(fills[0]) };
    let mut interior = Side { pen: int_pen, lightness: pins.int_light.unwrap_or(int_light), fill: fill(fills[1]) };
    // Two walls drawn alike in a DXF (the same lineweight and the same grey) cannot be told apart: an unpinned lightness moves.
    if exterior.dxf_pair() == interior.dxf_pair() && pins.int_light.is_none() {
        int_light = if spare == interior.lightness { (spare + 1) % (INT_LIGHT.1 + 1) } else { spare };
        interior.lightness = int_light;
    }
    WallLines { exterior, interior }
}

impl WallLines {
    /// The commands that draw them, for the end of the script: a view filter for each wall type, and the plan's cut overrides.
    pub fn script_lines(&self) -> Vec<(&'static str, Value)> {
        let filter = |name: &str, ty: &str| {
            ("filter", json!({"name": name, "categories": ["walls"], "rules": [{"field": "Type", "op": "equals", "value": ty}]}))
        };
        let cut = |name: &str, s: &Side| json!({"filter": name, "cut": {"weight": s.pen, "color": s.color(), "fill": s.fill}});
        vec![
            filter("Exterior walls", EXTERIOR),
            filter("Interior walls", INTERIOR),
            ("graphics", json!({"filters": [cut("Exterior walls", &self.exterior), cut("Interior walls", &self.interior)]})),
        ]
    }

    pub fn manifest_json(&self) -> Value {
        json!({"exterior": self.exterior.json(), "interior": self.interior.json()})
    }
}
