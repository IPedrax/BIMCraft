//! A 3D view's background, as Revit's Graphic Display Options > Background: what shows where the model doesn't. None (the
//! canvas's paper), one colour, two colours meeting at the horizon (the picture's middle), a gradient from the top through the
//! horizon to the bottom, or a picture the project holds, filling the view, fitted inside it or stretched over it. The
//! `background` command sets it; both painters leave the pixels where nothing is drawn for it, and [`Background::paint`]
//! fills them in a picture the core makes (`render`'s PNG).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Camera, Image};
use crate::render::{PAPER, Rgb};
use crate::texture::Picture;

/// How a picture lies on the view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Over the whole view, its aspect kept: what spills over is cut off.
    #[default]
    Fill,
    /// Whole, in the middle of the view, its aspect kept: paper round it.
    Fit,
    /// Over the whole view, its aspect not kept.
    Stretch,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Fill, Fit::Fit, Fit::Stretch];

    pub fn name(self) -> &'static str {
        match self {
            Fit::Fill => "fill",
            Fit::Fit => "fit",
            Fit::Stretch => "stretch",
        }
    }

    pub fn named(name: &str) -> Option<Fit> {
        Fit::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(name))
    }

    /// Where a `picture` px picture lies in a `view` px view: the part of the view it covers, and the part of the picture shown
    /// there, each `[left, top, right, bottom]` as shares of its width and height.
    pub fn place(self, picture: (f64, f64), view: (f64, f64)) -> ([f64; 4], [f64; 4]) {
        let whole = [0.0, 0.0, 1.0, 1.0];
        let (pw, ph, vw, vh) = (picture.0.max(1.0), picture.1.max(1.0), view.0.max(1.0), view.1.max(1.0));
        // How much wider the picture is than the view, for its height.
        let wider = (pw / ph) / (vw / vh);
        match self {
            Fit::Stretch => (whole, whole),
            Fit::Fill if wider >= 1.0 => (whole, [0.5 - 0.5 / wider, 0.0, 0.5 + 0.5 / wider, 1.0]),
            Fit::Fill => (whole, [0.0, 0.5 - 0.5 * wider, 1.0, 0.5 + 0.5 * wider]),
            Fit::Fit if wider >= 1.0 => ([0.0, 0.5 - 0.5 / wider, 1.0, 0.5 + 0.5 / wider], whole),
            Fit::Fit => ([0.5 - 0.5 * wider, 0.0, 0.5 + 0.5 * wider, 1.0], whole),
        }
    }
}

/// A colour of a gradient: where it is, percent of the way from the view's top (0) to its bottom (100), and the colour.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stop {
    pub at: f64,
    pub color: [u8; 3],
}

/// The most colours a gradient has.
pub const MAX_STOPS: usize = 16;

/// What a 3D view's background is filled with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fill {
    /// The canvas's paper, white or the dark theme's.
    #[default]
    None,
    Solid {
        color: [u8; 3],
    },
    /// The top half one colour, the bottom half another.
    TwoColor {
        top: [u8; 3],
        bottom: [u8; 3],
    },
    /// Blended from colour to colour down the view, each at its place (percent from the top); above the first, the first's
    /// colour, under the last, the last's.
    Gradient {
        stops: Vec<Stop>,
    },
    /// A picture the project holds, by its id (`Model::images`).
    Image {
        picture: String,
        #[serde(default, skip_serializing_if = "is_fill")]
        fit: Fit,
    },
}

/// A 3D view's background: what fills it, and with a horizon, the colour of the ground: everything the view shows at elevation 0
/// and below, its edge following the camera as it turns ([`horizon_y`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Background {
    #[serde(default, skip_serializing_if = "Fill::is_none")]
    pub fill: Fill,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<[u8; 3]>,
}

fn is_fill(f: &Fit) -> bool {
    *f == Fit::Fill
}

/// The kinds of fill, as the `background` command names them.
pub const KINDS: [&str; 5] = ["none", "solid", "two_color", "gradient", "image"];

/// A sky's, a horizon's and a ground's colours: what a new gradient or horizon starts as.
pub const SKY: [u8; 3] = [120, 166, 214];
pub const HAZE: [u8; 3] = [236, 240, 244];
pub const GROUND: [u8; 3] = [176, 170, 156];

/// Where the horizon lies across a picture through `cam`, px from its top: above it the background's fill shows, under it the
/// ground (elevation 0 and below). In perspective it is where the eye's level meets the sky, far off. A parallel view has no eye,
/// so it is drawn as if seen through the default lens: as far above the middle as the camera looks down (below it as it looks
/// up), the focal length times the tangent of that angle. Either way it moves as the camera turns up or down, straight across
/// (the camera never rolls), and looking straight down the ground fills the picture, straight up the sky.
pub fn horizon_y(cam: &Camera) -> f64 {
    let (back, ..) = cam.axes();
    let level = back[0].hypot(back[1]);
    if level < 1e-9 {
        return if back[2] > 0.0 { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    match cam.eye() {
        Some(eye) => {
            let far = 1.0e9;
            let ahead = [eye[0] - back[0] / level * far, eye[1] - back[1] / level * far, eye[2]];
            cam.project(ahead).1
        }
        None => {
            let focal = f64::from(cam.width) * super::LENS / super::FILM;
            // `back` turns up toward the eye as the camera looks down.
            f64::from(cam.height) / 2.0 - focal * back[2] / level
        }
    }
}

impl Fill {
    pub fn is_none(&self) -> bool {
        *self == Fill::None
    }

    /// Its kind, as [`KINDS`] names it.
    pub fn kind(&self) -> &'static str {
        match self {
            Fill::None => "none",
            Fill::Solid { .. } => "solid",
            Fill::TwoColor { .. } => "two_color",
            Fill::Gradient { .. } => "gradient",
            Fill::Image { .. } => "image",
        }
    }

    /// The colour of a row `down` of the way from the top of the view (0) to its bottom (1), for a fill of colours; none for
    /// none or a picture.
    pub fn color_at(&self, down: f64) -> Option<[u8; 3]> {
        match self {
            Fill::Solid { color } => Some(*color),
            Fill::TwoColor { top, bottom } => Some(if down < 0.5 { *top } else { *bottom }),
            Fill::Gradient { stops } => gradient_at(stops, down * 100.0),
            Fill::None | Fill::Image { .. } => None,
        }
    }
}

/// The colour `stops` give at `at` percent down: blended between the two about it, the nearest past either end.
pub fn gradient_at(stops: &[Stop], at: f64) -> Option<[u8; 3]> {
    let mut sorted: Vec<&Stop> = stops.iter().filter(|s| s.at.is_finite()).collect();
    sorted.sort_by(|a, b| a.at.total_cmp(&b.at));
    let (first, last) = (sorted.first()?, sorted.last()?);
    if at <= first.at {
        return Some(first.color);
    }
    if at >= last.at {
        return Some(last.color);
    }
    let i = sorted.windows(2).position(|w| at >= w[0].at && at <= w[1].at)?;
    let (a, b) = (sorted[i], sorted[i + 1]);
    let t = if b.at - a.at > 1e-9 { (at - a.at) / (b.at - a.at) } else { 0.0 };
    Some([0, 1, 2].map(|k| (f64::from(a.color[k]) + (f64::from(b.color[k]) - f64::from(a.color[k])) * t).round().clamp(0.0, 255.0) as u8))
}

impl Background {
    pub fn is_none(&self) -> bool {
        self.fill.is_none() && self.ground.is_none()
    }

    /// The picture it names, if it is one.
    pub fn picture(&self) -> Option<&str> {
        match &self.fill {
            Fill::Image { picture, .. } => Some(picture),
            _ => None,
        }
    }

    /// Whether it may stand in a model holding `images`: a picture it names is one of them, and a gradient's colours are
    /// placed from 0 to 100 percent, at least one and at most [`MAX_STOPS`].
    pub fn check(&self, images: &BTreeMap<String, Picture>) -> Result<(), String> {
        if let Some(p) = self.picture().filter(|p| !images.contains_key(*p)) {
            return Err(format!("its background is picture `{p}`, which the project doesn't hold"));
        }
        if let Fill::Gradient { stops } = &self.fill {
            if stops.is_empty() || stops.len() > MAX_STOPS {
                return Err(format!("its background's gradient has {} colours; it has 1 to {MAX_STOPS}", stops.len()));
            }
            if let Some(s) = stops.iter().find(|s| !(0.0..=100.0).contains(&s.at)) {
                return Err(format!("its background's gradient places a colour at {}%; colours go from 0 to 100%", s.at));
            }
        }
        Ok(())
    }

    /// `image`, a picture through `cam`, with this background where nothing is drawn: no surface there, and the paper showing.
    pub fn paint(&self, image: &mut Image, images: &BTreeMap<String, Picture>, cam: &Camera) {
        if self.is_none() {
            return;
        }
        let (w, h) = (image.width as usize, image.height as usize);
        let picture = self.picture().and_then(|p| images.get(p));
        let fit = match &self.fill {
            Fill::Image { fit, .. } => *fit,
            _ => Fit::Fill,
        };
        let placed = picture.map(|p| {
            let (pw, ph, px) = p.pixels();
            (pw, ph, px, fit.place((pw as f64, ph as f64), (w as f64, h as f64)))
        });
        let horizon = horizon_y(cam);
        for y in 0..h {
            let down = (y as f64 + 0.5) / h.max(1) as f64;
            let row = match self.ground {
                Some(g) if y as f64 + 0.5 >= horizon => Some(g),
                _ => self.fill.color_at(down),
            };
            for x in 0..w {
                let i = y * w + x;
                if !image.empty_at(i) {
                    continue;
                }
                let c = match (&placed, row) {
                    (_, Some(c)) => c,
                    (Some((pw, ph, px, (on, uv))), None) => {
                        let (sx, sy) = ((x as f64 + 0.5) / w.max(1) as f64, down);
                        if sx < on[0] || sx > on[2] || sy < on[1] || sy > on[3] {
                            [PAPER.0, PAPER.1, PAPER.2]
                        } else {
                            // Nearest texel of the part shown.
                            let (u, v) = ((sx - on[0]) / (on[2] - on[0]).max(1e-9), (sy - on[1]) / (on[3] - on[1]).max(1e-9));
                            let (u, v) = (uv[0] + (uv[2] - uv[0]) * u, uv[1] + (uv[3] - uv[1]) * v);
                            let tx = ((u * *pw as f64) as usize).min(pw.saturating_sub(1));
                            let ty = ((v * *ph as f64) as usize).min(ph.saturating_sub(1));
                            let k = (ty * pw + tx) * 3;
                            match px.get(k..k + 3) {
                                Some([r, g, b]) => [*r, *g, *b],
                                _ => [PAPER.0, PAPER.1, PAPER.2],
                            }
                        }
                    }
                    (None, None) => continue,
                };
                if let Some(p) = image.rgba.get_mut(i * 4..i * 4 + 3) {
                    p.copy_from_slice(&c);
                }
            }
        }
    }
}

/// `c` as a colour of the core's pictures.
pub fn rgb(c: [u8; 3]) -> Rgb {
    Rgb(c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_fills_cuts_fits_or_stretches_its_view() {
        // Twice as wide as tall, in a square view.
        let (on, uv) = Fit::Fill.place((200.0, 100.0), (100.0, 100.0));
        assert_eq!((on, uv), ([0.0, 0.0, 1.0, 1.0], [0.25, 0.0, 0.75, 1.0]));
        let (on, uv) = Fit::Fit.place((200.0, 100.0), (100.0, 100.0));
        assert_eq!((on, uv), ([0.0, 0.25, 1.0, 0.75], [0.0, 0.0, 1.0, 1.0]));
        assert_eq!(Fit::Stretch.place((200.0, 100.0), (100.0, 100.0)), ([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]));
        // Taller than the view.
        let (on, uv) = Fit::Fill.place((100.0, 200.0), (100.0, 100.0));
        assert_eq!((on, uv), ([0.0, 0.0, 1.0, 1.0], [0.0, 0.25, 1.0, 0.75]));
        // Nothing is no size: never a division by none.
        assert!(Fit::Fit.place((0.0, 0.0), (0.0, 0.0)).0.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn colours_split_or_blend_down_the_view() {
        let (sky, white, green) = ([0, 0, 255], [255, 255, 255], [0, 128, 0]);
        let two = Fill::TwoColor { top: sky, bottom: green };
        assert_eq!((two.color_at(0.1), two.color_at(0.49), two.color_at(0.51)), (Some(sky), Some(sky), Some(green)));
        // Stops anywhere, in any order: blended between, the end ones' past them.
        let stops = vec![Stop { at: 80.0, color: green }, Stop { at: 20.0, color: sky }, Stop { at: 50.0, color: white }];
        let g = Fill::Gradient { stops };
        assert_eq!((g.color_at(0.0), g.color_at(0.2), g.color_at(0.5), g.color_at(0.95)), (Some(sky), Some(sky), Some(white), Some(green)));
        assert_eq!(g.color_at(0.35), Some([128, 128, 255]));
        assert_eq!(Fill::None.color_at(0.5), None);
        // Written as the command names its kinds.
        let b = Background { fill: g, ground: Some(green) };
        let v = serde_json::to_value(&b).unwrap_or_default();
        assert_eq!(v["fill"]["kind"], "gradient");
        let back: Background = serde_json::from_value(v).unwrap_or_default();
        assert_eq!(back, b);
    }

    #[test]
    fn the_horizon_follows_the_camera_up_and_down() {
        let cam = |elevation: f64, lens: Option<f64>| Camera {
            azimuth: -90.0,
            elevation,
            scale: 0.1,
            target: [0.0, 0.0, 1500.0],
            width: 400,
            height: 300,
            lens,
        };
        // Parallel, as through the default lens: level, across the middle; higher looking down; off the top looking from above,
        // so the ground fills the picture; off the bottom looking from below.
        let level = horizon_y(&cam(0.0, None));
        assert!((level - 150.0).abs() < 1.0, "{level}");
        assert!(horizon_y(&cam(30.0, None)) < level);
        assert!(horizon_y(&cam(89.0, None)) < 0.0, "from the top, all ground");
        assert!(horizon_y(&cam(-89.0, None)) > 300.0, "from below, all sky");
        // As high as a perspective view looking as far down puts it.
        assert!((horizon_y(&cam(20.0, None)) - horizon_y(&cam(20.0, Some(35.0)))).abs() < 2.0);
        // In perspective, the eye's level: above the middle looking down, below it looking up.
        assert!(horizon_y(&cam(20.0, Some(35.0))) < 150.0);
        assert!(horizon_y(&cam(-20.0, Some(35.0))) > 150.0);
    }
}
