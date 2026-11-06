//! Origin anchored background grids, shared by canvas drawing and snapping.

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::model::MAX_COORD;
use crate::pen::Pen;
use crate::render::View;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GridStyle {
    pub enabled: bool,
    pub color: [u8; 3],
    pub pen: Pen,
}

impl Default for GridStyle {
    fn default() -> Self {
        Self { enabled: false, color: [230, 230, 230], pen: Pen::HAIRLINE }
    }
}

/// A view's canvas grid; lengths stay in millimetres regardless of display units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackgroundGrid {
    pub visible: bool,
    pub main: GridStyle,
    pub sub: GridStyle,
    pub spacing: [f64; 2],
    pub divisions: [u16; 2],
    /// Counter-clockwise degrees about the fixed origin (0, 0).
    pub rotation: f64,
}

impl Default for BackgroundGrid {
    fn default() -> Self {
        Self {
            visible: true,
            main: GridStyle::default(),
            sub: GridStyle { color: [245, 245, 245], ..GridStyle::default() },
            spacing: [1000.0; 2],
            divisions: [5; 2],
            rotation: 0.0,
        }
    }
}

pub struct GridLine {
    pub points: [P2; 2],
    pub style: GridStyle,
}

impl BackgroundGrid {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn check(&self) -> Result<(), String> {
        if self.valid() {
            Ok(())
        } else {
            Err(format!(
                "background grid: spacing X/Y must be finite and between 0.001 and {MAX_COORD} mm; divisions X/Y 1–1000, rotation -360–360 degrees, and pens 0–16"
            ))
        }
    }

    pub fn valid(&self) -> bool {
        self.spacing.iter().all(|n| n.is_finite() && (0.001..=MAX_COORD).contains(n))
            && self.divisions.iter().all(|n| (1..=1000).contains(n))
            && self.rotation.is_finite()
            && (-360.0..=360.0).contains(&self.rotation)
            && self.main.pen.is_valid()
            && self.sub.pen.is_valid()
    }

    pub fn shown(&self) -> bool {
        self.visible && (self.main.enabled || self.sub.enabled) && self.valid()
    }

    fn rotate(&self, p: P2, inverse: bool) -> P2 {
        let (s, c) = (self.rotation.to_radians() * if inverse { -1.0 } else { 1.0 }).sin_cos();
        P2(c * p.0 - s * p.1, s * p.0 + c * p.1)
    }

    /// Nearest intersection of the enabled lattice, also when its display is hidden.
    /// The caller controls whether snapping is active and how it combines with object snaps.
    pub fn nearest(&self, p: P2) -> Option<P2> {
        if !self.valid() || !(self.main.enabled || self.sub.enabled) || !p.0.is_finite() || !p.1.is_finite() {
            return None;
        }
        let p = self.rotate(p, true);
        let step = |axis: usize| self.spacing[axis] / if self.sub.enabled { f64::from(self.divisions[axis]) } else { 1.0 };
        Some(self.rotate(P2((p.0 / step(0)).round() * step(0), (p.1 / step(1)).round() * step(1)), false))
    }

    /// Viewport lines in model coordinates, subgrid first. Dense levels are omitted;
    /// main lines are thinned by whole multiples, retaining the fixed origin at every zoom.
    /// At most 4096 lines, even at the widest supported zoom.
    pub fn lines(&self, view: View) -> Vec<GridLine> {
        if !self.shown() || !view.scale.is_finite() || view.scale <= 0.0 {
            return Vec::new();
        }
        let corners = [(0.0, 0.0), (view.width as f32, 0.0), (0.0, view.height as f32), (view.width as f32, view.height as f32)]
            .map(|p| self.rotate(view.mm(p), true));
        let lo = P2(corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min), corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min));
        let hi = P2(
            corners.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
            corners.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
        );
        let mut lines = Vec::new();
        for (style, sub) in [(self.sub, true), (self.main, false)] {
            if !style.enabled {
                continue;
            }
            for axis in 0..2 {
                let division = if sub { f64::from(self.divisions[axis]) } else { 1.0 };
                let base = self.spacing[axis] / division;
                if sub && base * view.scale < 4.0 {
                    continue;
                }
                let (min, max) = if axis == 0 { (lo.0, hi.0) } else { (lo.1, hi.1) };
                let stride = (4.0 / (base * view.scale)).max((max - min) / (base * 1023.0)).ceil().max(1.0);
                let step = base * stride;
                let first = (min / step).ceil();
                let count = ((max / step).floor() - first + 1.0).clamp(0.0, 1024.0) as usize;
                for n in 0..count {
                    let index = first + n as f64;
                    // Main lines replace the coincident subgrid lines, so their colour and weight win.
                    if sub && self.main.enabled && (index * stride).rem_euclid(division).abs() < 1e-8 {
                        continue;
                    }
                    let at = index * step;
                    let points = if axis == 0 { [P2(at, lo.1), P2(at, hi.1)] } else { [P2(lo.0, at), P2(hi.0, at)] };
                    lines.push(GridLine { points: points.map(|p| self.rotate(p, false)), style });
                }
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::P2;
    use crate::render::View;

    #[test]
    fn background_grid_defaults_use_the_requested_neutral_greys() {
        let grid = BackgroundGrid::default();
        assert_eq!(grid.main.color, [230, 230, 230]);
        assert_eq!(grid.sub.color, [245, 245, 245]);
    }

    #[test]
    fn grid_rotation_spacing_and_negative_coordinates_share_the_origin() {
        let mut g = BackgroundGrid::default();
        g.main.enabled = true;
        g.spacing = [1000.0, 2000.0];
        assert_eq!(g.nearest(P2(-610.0, 1300.0)), Some(P2(-1000.0, 2000.0)));
        g.rotation = 90.0;
        let p = g.nearest(P2(-1700.0, 850.0)).unwrap();
        assert!(p.dist(P2(-2000.0, 1000.0)) < 1e-8);
        assert_eq!(g.nearest(P2(0.0, 0.0)), Some(P2(0.0, 0.0)));
        g.main.enabled = false;
        assert_eq!(g.nearest(P2(10.0, 10.0)), None);
    }

    #[test]
    fn subgrid_is_independent_and_drawing_is_bounded_at_extreme_zoom() {
        let mut g = BackgroundGrid::default();
        g.sub.enabled = true;
        g.spacing = [1000.0, 2000.0];
        g.divisions = [4, 5];
        assert_eq!(g.nearest(P2(370.0, -610.0)), Some(P2(250.0, -800.0)));
        let v = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600 };
        let lines = g.lines(v);
        assert!(!lines.is_empty());
        assert!(lines.iter().any(|l| l.points[0].0 == 0.0 && l.points[1].0 == 0.0));
        g.main.enabled = true;
        g.rotation = 33.0;
        assert!(g.lines(View { scale: 1e-6, ..v }).len() <= 4096);
        g.spacing[0] = 0.0;
        assert!(!g.valid());
        assert!(g.lines(v).is_empty());
        assert_eq!(g.nearest(P2(0.0, 0.0)), None);
    }

    #[test]
    fn background_grids_are_independent_saved_view_settings_with_undo() {
        use crate::{engine::Session, model::Model};
        use serde_json::json;
        let mut s = Session::new();
        let grid = json!({"main": {"enabled": true}, "spacing": [1200, 2400], "rotation": 25});
        s.execute("graphics", &json!({"view": "Level 1", "background_grid": grid})).unwrap();
        let read = |s: &mut Session, view: &str| s.execute("graphics", &json!({"view": view})).unwrap()["background_grid"].clone();
        let first = read(&mut s, "Level 1");
        assert_eq!(first["spacing"], json!([1200.0, 2400.0]));
        assert_eq!(read(&mut s, "Level 2")["main"]["enabled"], false);
        s.execute("duplicate_view", &json!({"view":"Level 1", "name":"Independent"})).unwrap();
        assert_eq!(read(&mut s, "Independent"), first);
        s.execute("graphics", &json!({"view":"Independent", "background_grid":{"rotation":70}})).unwrap();
        assert_eq!(read(&mut s, "Independent")["rotation"], 70.0);
        assert_eq!(read(&mut s, "Level 1"), first);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(read(&mut s, "Independent"), first);
        s.execute("section", &json!({"start":[0,0], "end":[8000,0]})).unwrap();
        s.execute("graphics", &json!({"view":"Section 1", "background_grid":{"sub":{"enabled":true}}})).unwrap();
        assert_eq!(read(&mut s, "Section 1")["sub"]["enabled"], true);
        assert_eq!(read(&mut s, "Level 1"), first);
        let saved = s.model.to_json().unwrap();
        let (back, left_out) = Model::read(&saved).unwrap();
        assert!(left_out.is_empty());
        assert_eq!(back, *s.model);
    }

    #[test]
    fn background_grid_templates_control_followers_and_reject_bypasses_and_bad_values() {
        use crate::engine::Session;
        use serde_json::json;
        let mut s = Session::new();
        s.execute("graphics", &json!({"view":"Level 1", "background_grid":{"main":{"enabled":true}, "rotation":20}})).unwrap();
        let template = s.execute("template", &json!({"name":"Grid template", "from":"Level 1", "controls":["background_grid"]})).unwrap();
        assert_eq!(template["background_grid"]["rotation"], 20.0);
        s.execute("graphics", &json!({"view":"Level 2", "template":"Grid template"})).unwrap();
        for (command, params) in [
            ("graphics", json!({"view":"Level 2", "background_grid":{"rotation":30}})),
            ("modify", json!({"id":100001, "graphics":{"background_grid":{"rotation":30}}})),
        ] {
            let err = s.execute(command, &params).unwrap_err();
            assert!(err.contains("Grid template") && err.contains("background"), "{err}");
        }
        s.execute("template", &json!({"name":"Grid template", "background_grid":{"rotation":45}})).unwrap();
        assert_eq!(s.execute("graphics", &json!({"view":"Level 2"})).unwrap()["background_grid"]["rotation"], 45.0);
        s.execute("graphics", &json!({"view":"Level 2", "template":null})).unwrap();
        assert_eq!(s.execute("graphics", &json!({"view":"Level 2"})).unwrap()["background_grid"]["main"]["enabled"], false);
        for bad in [
            json!({"spacing":[0,1000]}),
            json!({"divisions":[0,5]}),
            json!({"rotation":361}),
            json!({"main":{"pen":17}}),
            json!({"typo":true}),
            json!({"snapping":true}),
        ] {
            assert!(s.execute("graphics", &json!({"view":"Level 1", "background_grid":bad})).is_err(), "{bad}");
            assert!(s.execute("template", &json!({"name":"Grid template", "background_grid":bad})).is_err(), "{bad}");
        }
    }
}
