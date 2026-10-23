//! Pens: how wide a line is on paper. A line's weight is a pen, 1 to 16 as in Revit, each a width in paper millimetres,
//! so it prints the same at any scale and shows on screen as thick as the paper it stands for at the zoom; and pen 0, a
//! hairline, the thinnest line the screen, the PNG or the printer draws, at any scale and any zoom.
//!
//! What each pen draws is the project's [`LineWeights`], as Revit's Line Weights dialog keeps them: the model's pens at each
//! of a list of scales (a view takes the column of the scale nearest its own), and the annotation's pens at any scale.

use serde::{Deserialize, Deserializer, Serialize};

use crate::units::Units;

/// A pen: 0, a hairline, or 1 (the thinnest) to [`MAX_PEN`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Pen(pub u8);

/// Read as a whole number of a pen, saying what one is otherwise.
impl<'de> Deserialize<'de> for Pen {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Pen, D::Error> {
        let n = f64::deserialize(d)?;
        match n {
            n if n.fract() == 0.0 && (0.0..=f64::from(MAX_PEN)).contains(&n) => Ok(Pen(n as u8)),
            _ => Err(serde::de::Error::custom(format!("a line weight is a pen, a whole number from 0 (a hairline) to {MAX_PEN}, not {n}"))),
        }
    }
}

/// The widest pen.
pub const MAX_PEN: u8 = 16;

/// Each pen's width on paper, mm, from pen 0 (a hairline, no width of its own) up: our own steps, from a tenth of a
/// millimetre to four.
const WIDTHS: [f64; MAX_PEN as usize + 1] =
    [0.0, 0.10, 0.13, 0.16, 0.20, 0.25, 0.30, 0.40, 0.50, 0.60, 0.80, 1.00, 1.40, 2.00, 2.50, 3.00, 4.00];

/// Which pen each kind of line draws with: one place for the weights the drawings use, the model's (until Object Styles
/// gives each category its own) and the annotation's and the sheet's.
impl Pen {
    pub const HAIRLINE: Pen = Pen(0);
    /// What a cut passes through, outlined: walls, floors and roofs, in plan and in section alike.
    pub const CUT: Pen = Pen(10);
    /// A sheet's border and its title block's frame.
    pub const BORDER: Pen = Pen(8);
    /// Heavy: columns cut, an opening's jambs, a dimension's ticks, a schedule's frame.
    pub const HEAVY: Pen = Pen(7);
    /// Medium: a family's forms cut, a door's leaf, railings, stairs above the cut, masses, section and elevation marks,
    /// the rule under a schedule's headings.
    pub const MEDIUM: Pen = Pen(6);
    /// The bubbles of grids and elevation marks.
    pub const BUBBLE: Pen = Pen(5);
    /// What's seen beyond the cut, the lines between a cut's layers, and annotation's lines: dimensions, tags, levels, grids.
    pub const PROJECTION: Pen = Pen(4);
    /// A stipple's dots.
    pub const STIPPLE: Pen = Pen(3);
    /// The finest: hatching, a schedule's rows.
    pub const FINE: Pen = Pen(1);

    /// How wide it draws on paper, mm: 0 for a hairline (and for a pen past [`MAX_PEN`], which `Style::check` refuses).
    pub fn mm(self) -> f64 {
        WIDTHS.get(usize::from(self.0)).copied().unwrap_or(0.0)
    }

    /// Whether it is one: 0 to [`MAX_PEN`].
    pub fn is_valid(self) -> bool {
        self.0 <= MAX_PEN
    }

    /// The pen nearest `mm` wide on paper, of 1 to [`MAX_PEN`]: how a weight from before pens, in pixels of a sheet's
    /// drawing, comes in.
    pub fn nearest(mm: f64) -> Pen {
        let off = |p: &u8| (WIDTHS[usize::from(*p)] - mm).abs();
        Pen((1..=MAX_PEN).min_by(|a, b| off(a).total_cmp(&off(b))).unwrap_or(1))
    }
}

/// Pens 1 to [`MAX_PEN`]'s widths on paper, mm: a row of the line weights.
pub type Row = [f64; MAX_PEN as usize];

/// The most scales the model's line weights hold, and the widths a pen may draw, mm on paper.
pub const MAX_SCALES: usize = 32;
pub const WIDTH_RANGE: std::ops::RangeInclusive<f64> = 0.01..=20.0;

/// A project's line weights: what each pen draws on paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineWeights {
    /// The model's pens at each scale, finest first: what a view at that scale draws its walls, doors, patterns and the
    /// rest of the model in.
    pub model: Vec<ScalePens>,
    /// The annotation's pens, at any scale: dimensions, tags, text, grids, levels, and the marks of sections and elevations.
    pub annotation: Row,
}

/// A column of the model's line weights: pens 1 to 16 at scale 1 : `scale`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalePens {
    pub scale: f64,
    pub widths: Row,
}

/// What pens 0 to 16 draw in one view, mm on paper: the model's at its scale and the annotation's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    pub model: [f64; MAX_PEN as usize + 1],
    pub annotation: [f64; MAX_PEN as usize + 1],
}

impl Default for Weights {
    /// Each pen its own width ([`Pen::mm`]), model and annotation alike: what a project's line weights are at 1 : 100.
    fn default() -> Weights {
        Weights { model: WIDTHS, annotation: WIDTHS }
    }
}

impl Weights {
    /// How wide `pen` draws, mm on paper: as annotation, or as the model; 0 for a hairline.
    pub fn mm(&self, pen: Pen, annotation: bool) -> f64 {
        let row = if annotation { &self.annotation } else { &self.model };
        row.get(usize::from(pen.0)).copied().unwrap_or(0.0)
    }
}

/// Pens 0 to 16 from a row of 1 to 16: pen 0 a hairline.
fn with_hairline(row: &Row) -> [f64; MAX_PEN as usize + 1] {
    let mut all = [0.0; MAX_PEN as usize + 1];
    all[1..].copy_from_slice(row);
    all
}

impl LineWeights {
    /// The line weights a project in `units` starts with, our own: the annotation's pens [`Pen::mm`]; the model's the
    /// same at 1 : 100 and, at the scales a project in those units most draws at, wider at finer scales and thinner at
    /// coarser ones, by the fourth root of how many times finer, to the hundredth of a millimetre.
    pub fn default_for(units: Units) -> LineWeights {
        let scales: &[f64] =
            if units.imperial() { &[12.0, 24.0, 48.0, 96.0, 192.0, 384.0] } else { &[10.0, 20.0, 50.0, 100.0, 200.0, 500.0] };
        let row = |k: f64| -> Row {
            let mut r = [0.0; MAX_PEN as usize];
            for (w, base) in r.iter_mut().zip(&WIDTHS[1..]) {
                *w = ((base * k * 100.0).round() / 100.0).clamp(*WIDTH_RANGE.start(), *WIDTH_RANGE.end());
            }
            r
        };
        LineWeights {
            model: scales.iter().map(|&n| ScalePens { scale: n, widths: row((100.0 / n).powf(0.25)) }).collect(),
            annotation: row(1.0),
        }
    }

    /// What the pens draw in a view at 1 : `n`: the model's of the scale nearest it (by how many times finer or coarser;
    /// the finer of two as near), the annotation's as they are.
    pub fn at(&self, n: f64) -> Weights {
        let off = |s: &ScalePens| (s.scale / n).ln().abs();
        let nearest = self.model.iter().min_by(|a, b| off(a).total_cmp(&off(b)).then(a.scale.total_cmp(&b.scale)));
        Weights { model: nearest.map_or(WIDTHS, |s| with_hairline(&s.widths)), annotation: with_hairline(&self.annotation) }
    }

    /// The scale's column of the model's pens, if it has one.
    pub fn column(&self, n: f64) -> Option<&ScalePens> {
        self.model.iter().find(|s| s.scale == n)
    }

    /// An error saying what to fix, unless it is line weights a project can hold: a scale or more, each once and a scale
    /// a view may have, at most [`MAX_SCALES`], every width in [`WIDTH_RANGE`].
    pub fn check(&self) -> Result<(), String> {
        let (lo, hi) = (crate::graphics::SCALES.start(), crate::graphics::SCALES.end());
        if self.model.is_empty() || self.model.len() > MAX_SCALES {
            return Err(format!("the model's line weights hold 1 to {MAX_SCALES} scales, not {}", self.model.len()));
        }
        for (i, s) in self.model.iter().enumerate() {
            if !crate::graphics::SCALES.contains(&s.scale) {
                return Err(format!("line weights at 1 : {}: a scale is 1 : {lo} to 1 : {hi}", s.scale));
            }
            if self.model[..i].iter().any(|t| t.scale == s.scale) {
                return Err(format!("line weights at 1 : {} are given twice", s.scale));
            }
        }
        let rows = self.model.iter().map(|s| (format!("model pen {{}} at 1 : {}", s.scale), &s.widths));
        for (what, row) in rows.chain(std::iter::once(("annotation pen {}".to_string(), &self.annotation))) {
            for (i, w) in row.iter().enumerate() {
                if !(w.is_finite() && WIDTH_RANGE.contains(w)) {
                    let (lo, hi) = (WIDTH_RANGE.start(), WIDTH_RANGE.end());
                    return Err(format!("{} is {w} mm wide: a pen is {lo} to {hi} mm on paper", what.replace("{}", &(i + 1).to_string())));
                }
            }
        }
        Ok(())
    }

    /// The same, the model's scales in order, finest first.
    pub fn sorted(mut self) -> LineWeights {
        self.model.sort_by(|a, b| a.scale.total_cmp(&b.scale));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pens_widen_from_a_hairline_and_a_width_finds_the_nearest() {
        assert_eq!(Pen::HAIRLINE.mm(), 0.0);
        assert!((1..=MAX_PEN).all(|p| Pen(p).mm() > Pen(p - 1).mm()), "each wider than the one before");
        assert!(Pen(MAX_PEN).is_valid() && !Pen(MAX_PEN + 1).is_valid());
        assert_eq!(Pen(MAX_PEN + 1).mm(), 0.0);
        // A sheet's drawing has 5 px to the mm: the weights from before pens find theirs exactly.
        for (px, pen) in [(0.5, 1), (0.8, 3), (1.0, 4), (1.5, 6), (2.0, 7), (2.5, 8), (3.0, 9), (4.0, 10), (20.0, 16)] {
            assert_eq!(Pen::nearest(px / 5.0), Pen(pen), "{px} px");
        }
        assert_eq!(Pen::nearest(0.0), Pen(1), "a width is never a hairline");
    }

    #[test]
    fn line_weights_are_the_projects_set_whole_saved_and_undone() {
        use crate::engine::Session;
        use serde_json::json;
        let mut s = Session::new();
        // Asking says what they are, as the units name the scales, and changes nothing.
        let r = s.execute("line_weights", &json!({})).unwrap();
        assert_eq!(r["model"][3], json!({"scale": 100.0, "name": "1 : 100", "widths": WIDTHS[1..].to_vec()}));
        assert_eq!((r["annotation"][9].clone(), r["own"].clone()), (json!(0.8), json!(false)));
        assert!(s.history().is_empty());
        // Given, whole, in one undo step; the scales kept finest first.
        let model = json!([{"scale": 200, "widths": vec![0.1; 16]}, {"scale": 20, "widths": vec![0.5; 16]}]);
        let r = s.execute("line_weights", &json!({"model": model, "annotation": vec![0.3; 16]})).unwrap();
        assert_eq!((r["model"][0]["scale"].clone(), r["own"].clone()), (json!(20.0), json!(true)));
        assert_eq!(s.history(), ["Line Weights"]);
        let lw = s.model.line_weights();
        assert_eq!((lw.at(30.0).mm(Pen(4), false), lw.at(1000.0).mm(Pen(4), false), lw.at(1000.0).mm(Pen(4), true)), (0.5, 0.1, 0.3));
        // Only the annotation: the model's stay.
        s.execute("line_weights", &json!({"annotation": vec![0.2; 16]})).unwrap();
        assert_eq!(s.model.line_weights().model.len(), 2);
        // Saved with the project and read back.
        let (m, _) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
        assert_eq!(m.line_weights, s.model.line_weights);
        // Undone, and reset to the project's own start.
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.model.line_weights().annotation, [0.3; 16]);
        s.execute("line_weights", &json!({"reset": true})).unwrap();
        assert_eq!(s.model.line_weights, None);
        // What isn't line weights is refused, saying why, and changes nothing.
        let before = s.model.clone();
        for (p, says) in [
            (json!({"model": []}), "1 to 32 scales"),
            (json!({"annotation": vec![0.2; 15]}), "16"),
            (json!({"annotation": vec![-1.0; 16]}), "annotation pen 1 is -1 mm wide"),
            (json!({"model": [{"scale": 50, "widths": vec![0.2; 16]}, {"scale": 50, "widths": vec![0.2; 16]}]}), "twice"),
            (json!({"model": [{"scale": 50, "widths": vec![0.2; 16], "pen": 3}]}), "pen"),
            (json!({"reset": "yes"}), "reset"),
        ] {
            let e = s.execute("line_weights", &p).unwrap_err();
            assert!(e.contains(says), "{p}: {e}");
        }
        assert_eq!(*s.model, *before);
    }

    #[test]
    fn a_line_draws_its_pen_as_the_line_weights_say_the_model_by_scale_annotation_at_any() {
        use crate::engine::Session;
        use crate::render::{Prim, View};
        use serde_json::json;
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0]]}
               grid {"start": [0, -3000], "end": [0, 3000]}
               grid {"start": [8000, -3000], "end": [8000, 3000]}
               dimension {"refs": [{"id": 100003}, {"id": 100004}], "at": [4000, -1500]}"#,
        )
        .unwrap();
        let (wall, dim) = (100002, 100005);
        let widest = |s: &Session, id| {
            let view = View { scale: 0.1, cx: 4000.0, cy: 0.0, width: 1000, height: 800 };
            let d = crate::render::draw(&s.model, 100000, view);
            (d.prims.iter().zip(&d.owners))
                .filter_map(|(p, o)| match p {
                    Prim::Line { width, dash: None, .. } if *o == Some(id) => Some(*width),
                    _ => None,
                })
                .fold(0.0, f32::max)
        };
        // 1 : 50 at a tenth of a pixel to the mm: 5 px to a mm of paper. The wall's cut outline is model pen 10, the
        // dimension's lines annotation pens.
        s.execute("scale", &json!({"view": 100000, "scale": 50})).unwrap();
        let w50 = s.model.line_weights().at(50.0).mm(Pen(10), false) as f32 * 5.0;
        assert!((widest(&s, wall) - w50).abs() < 1e-4, "{} {w50}", widest(&s, wall));
        let dim_before = widest(&s, dim);
        let mut model = s.model.line_weights().model.clone();
        for col in &mut model {
            col.widths = [1.0; 16];
        }
        s.execute("line_weights", &json!({"model": model})).unwrap();
        assert!((widest(&s, wall) - 5.0).abs() < 1e-4, "1 mm");
        assert_eq!(widest(&s, dim), dim_before, "annotation keeps its own");
        s.execute("line_weights", &json!({"annotation": vec![0.6; 16]})).unwrap();
        assert!((widest(&s, dim) - 3.0).abs() < 1e-4, "{}", widest(&s, dim));
        // An override's pen draws as the line weights say too.
        s.execute("graphics", &json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 3}}}})).unwrap();
        assert!((widest(&s, wall) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn a_projects_line_weights_start_as_the_pens_at_1_100_wider_finer_and_take_the_nearest_scale() {
        let lw = LineWeights::default_for(Units::Millimetres);
        assert_eq!(lw.model.iter().map(|s| s.scale).collect::<Vec<_>>(), [10.0, 20.0, 50.0, 100.0, 200.0, 500.0]);
        assert_eq!(LineWeights::default_for(Units::FeetInches).model[3].scale, 96.0, "1/8\" = 1'-0\" among an imperial project's");
        assert_eq!(lw.at(100.0), Weights::default(), "at 1 : 100, model and annotation are the pens' own widths");
        assert!(lw.at(50.0).mm(Pen(10), false) > 0.8 && lw.at(200.0).mm(Pen(10), false) < 0.8);
        assert_eq!(lw.at(50.0).mm(Pen(10), true), 0.8, "annotation, whatever the scale");
        assert_eq!((lw.at(50.0).mm(Pen(0), false), lw.at(50.0).mm(Pen(0), true)), (0.0, 0.0), "a hairline");
        // Between two, the nearer by how many times: 1 : 70 is nearer 1 : 50 (1.4 times) than 1 : 100 (1.43).
        assert_eq!(lw.at(70.0), lw.at(50.0));
        assert_eq!(lw.at(75.0), lw.at(100.0));
        assert_eq!(lw.at(5000.0), lw.at(500.0));
        assert!(lw.check().is_ok());
        let bad = |f: &dyn Fn(&mut LineWeights)| {
            let mut l = lw.clone();
            f(&mut l);
            l.check().unwrap_err()
        };
        assert!(bad(&|l| l.model.clear()).contains("1 to 32 scales"));
        assert!(bad(&|l| l.model[1].scale = 10.0).contains("given twice"));
        assert!(bad(&|l| l.model[0].scale = 0.0).contains("a scale is"));
        assert!(bad(&|l| l.model[2].widths[4] = 0.0).contains("model pen 5 at 1 : 50 is 0 mm wide"));
        assert!(bad(&|l| l.annotation[0] = f64::NAN).contains("annotation pen 1"));
    }
}
