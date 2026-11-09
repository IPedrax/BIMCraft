//! Page setup, and how a drawing lies on the pages it is printed on: the one function the export dialog's preview and the
//! writers share, so what the preview shows is what the file holds.
//!
//! A drawing has an extent in its own millimetres (a sheet's are paper's, a plan's the model's) and the scale it is drawn
//! at (1 : `original`; a sheet is 1 : 1). The setup picks the paper, the scale (the drawing's own, fitted to the page, or
//! one typed), the margin and where the drawing sits; [`layout`] works out the scale on paper, how many pages across and
//! down it takes, and which part of the drawing each page shows.

use serde::{Deserialize, Serialize};

use crate::geom::P2;

/// The most pages one drawing may be tiled across.
pub const MAX_PAGES: u32 = 256;
/// The smallest and largest side of a paper, mm.
pub const MIN_PAPER: f64 = 20.0;
pub const MAX_PAPER: f64 = 10_000.0;
/// The widest margin, mm.
pub const MAX_MARGIN: f64 = 200.0;
/// The smallest and largest scale typed (1 : n).
pub const MIN_RATIO: f64 = 0.01;
pub const MAX_RATIO: f64 = 1_000_000.0;
/// The lowest and highest resolution of a picture made of a page, dots per inch.
pub const MIN_DPI: u32 = 36;
pub const MAX_DPI: u32 = 1200;

/// Which way up the paper is. `Auto` turns it the way the drawing is longer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Auto,
    Landscape,
    Portrait,
}

/// The scale a drawing is printed at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// The drawing's own: a sheet at full size, a view at its view scale.
    #[default]
    Original,
    /// As large as fits the page.
    Fit,
    /// 1 : n of the drawing's millimetres.
    Custom(f64),
}

/// How a page's colours are printed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Colour {
    #[default]
    Colour,
    Grey,
    /// Black and white: every ink black, every fill white unless it is dark.
    Black,
}

/// Where the drawing sits on the pages when it doesn't fill them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Centre,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Anchor; 9] = [
        Anchor::TopLeft,
        Anchor::Top,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Centre,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::Bottom,
        Anchor::BottomRight,
    ];

    /// Across and down, each 0 (left, top), 1 (middle) or 2 (right, bottom).
    pub fn cell(self) -> (u8, u8) {
        let i = Anchor::ALL.iter().position(|a| *a == self).unwrap_or(4) as u8;
        (i % 3, i / 3)
    }
}

/// How a drawing is put on paper.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PageSetup {
    /// A paper by name (`A3`, `ARCH D`, `Letter`: [`crate::model::paper`]); none, the drawing's own (a sheet's size), or one
    /// as large as the drawing printed at its scale.
    pub paper: Option<String>,
    /// A paper of its own, width and height in mm, over `paper`.
    pub size: Option<P2>,
    pub orientation: Orientation,
    /// The blank round the edge of each page, mm.
    pub margin: f64,
    pub scale: Scale,
    pub anchor: Anchor,
    pub colour: Colour,
    /// Every line as thin as the output can draw it.
    pub hairlines: bool,
    /// Pictures made of a page (PNG, JPEG), dots per inch; none, 150. A PDF's shaded views are made at it too.
    pub dpi: Option<u32>,
    /// Which pages, first and last, counted from 1; none, all.
    pub pages: Option<[u32; 2]>,
    /// Only what lies inside this rectangle of the view, two opposite corners `[x1, y1, x2, y2]` in its own coordinates (a
    /// plan's model mm, a section's along its line and up, a sheet's paper mm from its bottom left); none, all of it. Every
    /// drawing format takes it, not only those of pages.
    pub region: Option<[f64; 4]>,
}

impl PageSetup {
    pub fn dpi(&self) -> u32 {
        self.dpi.unwrap_or(150).clamp(MIN_DPI, MAX_DPI)
    }
}

/// One page of a layout.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Tile {
    /// Across and down, from 0 at the top left.
    pub col: u32,
    pub row: u32,
    /// The part of the drawing the page's printable area shows: its top left corner and size, in the drawing's mm
    /// (measured as the drawing's pixels are, from its top left, y down).
    pub at: P2,
    pub size: P2,
}

/// A drawing laid on pages.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Layout {
    /// Each page's paper, mm, turned the way it is printed.
    pub paper: P2,
    pub margin: f64,
    /// Paper mm per drawing mm.
    pub factor: f64,
    /// The scale on paper, 1 : `ratio` of the drawing's mm.
    pub ratio: f64,
    pub cols: u32,
    pub rows: u32,
    /// Where the drawing lies on the pages' printable areas put side by side: its top left corner and size, paper mm.
    pub placed: (P2, P2),
    /// The pages printed, in reading order (the setup's range of them).
    pub pages: Vec<Tile>,
}

fn finite_positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// How the drawing `extent` mm across and down, drawn at 1 : `original` (a sheet, 1) on its own `own` paper if it has
/// one, lies on the pages `setup` asks for.
pub fn layout(setup: &PageSetup, extent: P2, original: f64, own: Option<P2>) -> Result<Layout, String> {
    if !finite_positive(extent.0) || !finite_positive(extent.1) {
        return Err(format!("the drawing has no size to print ({} × {} mm)", extent.0, extent.1));
    }
    if !finite_positive(original) {
        return Err(format!("the drawing's scale 1 : {original} is not a scale"));
    }
    let margin = setup.margin;
    if !(margin.is_finite() && (0.0..=MAX_MARGIN).contains(&margin)) {
        return Err(format!("`margin` is 0 to {MAX_MARGIN} mm, not {margin}"));
    }
    let factor = match setup.scale {
        Scale::Original => 1.0 / original,
        Scale::Custom(n) if n.is_finite() && (MIN_RATIO..=MAX_RATIO).contains(&n) => 1.0 / n,
        Scale::Custom(n) => return Err(format!("a scale is 1 : {MIN_RATIO} to 1 : {MAX_RATIO}, not 1 : {n}")),
        // Fitted once the paper is known.
        Scale::Fit => 0.0,
    };
    let named = match setup.paper.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(name) => Some(crate::model::paper(name).ok_or_else(|| format!("`{name}` isn't a paper size this knows"))?),
        None => None,
    };
    let paper = match (setup.size, named, own) {
        (Some(size), ..) => size,
        (None, Some(named), _) => named,
        (None, None, Some(own)) => own,
        // As large as the drawing at its scale, and a fitted one at its own.
        (None, None, None) => {
            let f = if factor > 0.0 { factor } else { 1.0 / original };
            P2(extent.0 * f + 2.0 * margin, extent.1 * f + 2.0 * margin)
        }
    };
    let side = |v: f64| v.is_finite() && (MIN_PAPER..=MAX_PAPER).contains(&v);
    if !side(paper.0) || !side(paper.1) {
        return Err(format!("a paper is {MIN_PAPER} to {MAX_PAPER} mm a side, not {:.1} × {:.1}", paper.0, paper.1));
    }
    let (long, short) = (paper.0.max(paper.1), paper.0.min(paper.1));
    let paper = match setup.orientation {
        Orientation::Landscape => P2(long, short),
        Orientation::Portrait => P2(short, long),
        // A paper given by size keeps its way up; a named one turns with the drawing.
        Orientation::Auto if setup.size.is_some() || (named.is_none() && own.is_some()) => paper,
        Orientation::Auto if extent.1 > extent.0 => P2(short, long),
        Orientation::Auto => P2(long, short),
    };
    let printable = P2(paper.0 - 2.0 * margin, paper.1 - 2.0 * margin);
    if printable.0 <= 1.0 || printable.1 <= 1.0 {
        return Err(format!("a {margin} mm margin leaves nothing of a {:.0} × {:.0} mm page to print on", paper.0, paper.1));
    }
    let factor = if factor > 0.0 { factor } else { (printable.0 / extent.0).min(printable.1 / extent.1) };
    let printed = P2(extent.0 * factor, extent.1 * factor);
    // A hair over a page's edge is not a second page.
    let count = |printed: f64, page: f64| ((printed / page) - 1e-6).ceil().max(1.0);
    let (cols, rows) = (count(printed.0, printable.0), count(printed.1, printable.1));
    if cols * rows > f64::from(MAX_PAGES) {
        return Err(format!(
            "at 1 : {:.0} the drawing takes {cols} × {rows} pages, more than {MAX_PAGES}: print it smaller or on larger paper",
            1.0 / factor
        ));
    }
    let (cols, rows) = (cols as u32, rows as u32);
    let grid = P2(f64::from(cols) * printable.0, f64::from(rows) * printable.1);
    let (ax, ay) = setup.anchor.cell();
    let offset = |anchor: u8, room: f64| f64::from(anchor) * room / 2.0;
    let at = P2(offset(ax, grid.0 - printed.0), offset(ay, grid.1 - printed.1));
    let (first, last) = match setup.pages {
        None => (1, cols * rows),
        Some([a, b]) if a >= 1 && a <= b => (a, b.min(cols * rows)),
        Some([a, b]) => return Err(format!("`pages` is a first and a last page from 1, not {a} to {b}")),
    };
    let pages: Vec<Tile> = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (col, row)))
        .enumerate()
        .filter(|(i, _)| (first..=last).contains(&(*i as u32 + 1)))
        .map(|(_, (col, row))| Tile {
            col,
            row,
            at: P2((f64::from(col) * printable.0 - at.0) / factor, (f64::from(row) * printable.1 - at.1) / factor),
            size: P2(printable.0 / factor, printable.1 / factor),
        })
        .collect();
    if pages.is_empty() {
        return Err(format!("the drawing takes {} pages; there is no page {first}", cols * rows));
    }
    Ok(Layout { paper, margin, factor, ratio: 1.0 / factor, cols, rows, placed: (at, printed), pages })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn a_sheet_prints_on_its_own_paper_at_full_size() {
        let l = layout(&PageSetup::default(), P2(841.0, 594.0), 1.0, Some(P2(841.0, 594.0))).unwrap();
        assert_eq!((l.cols, l.rows, l.pages.len()), (1, 1, 1));
        assert!(close(l.factor, 1.0) && l.paper == P2(841.0, 594.0));
        assert_eq!(l.pages[0].at, P2(0.0, 0.0));
    }

    #[test]
    fn an_a1_sheet_fitted_to_a3_is_printed_at_half_size() {
        let setup = PageSetup { paper: Some("A3".into()), scale: Scale::Fit, ..PageSetup::default() };
        let l = layout(&setup, P2(841.0, 594.0), 1.0, Some(P2(841.0, 594.0))).unwrap();
        assert_eq!(l.paper, P2(420.0, 297.0));
        assert!(close(l.factor, 297.0 / 594.0) || close(l.factor, 420.0 / 841.0));
        assert_eq!((l.cols, l.rows), (1, 1));
    }

    #[test]
    fn a_plan_too_big_for_the_page_at_its_scale_is_tiled_and_centred() {
        // 50 m × 15 m at 1:100 is 500 × 150 mm; A4 landscape with 10 mm margins prints 277 × 190.
        let setup = PageSetup { paper: Some("A4".into()), margin: 10.0, ..PageSetup::default() };
        let l = layout(&setup, P2(50_000.0, 15_000.0), 100.0, None).unwrap();
        assert_eq!(l.paper, P2(297.0, 210.0));
        assert_eq!((l.cols, l.rows, l.pages.len()), (2, 1, 2));
        assert!(close(l.ratio, 100.0));
        // Centred on the two pages' 554 × 190: 27 mm spare across, 20 down, each halved.
        assert!(close(l.placed.0.0, 27.0) && close(l.placed.0.1, 20.0));
        // The second page starts 277 mm of paper in, 27 of them before the drawing: 250 mm of paper, 25 m of plan.
        assert!(close(l.pages[1].at.0, 25_000.0), "{:?}", l.pages[1]);
        assert!(close(l.pages[1].size.0, 27_700.0));
    }

    #[test]
    fn the_anchor_moves_the_drawing_into_its_corner() {
        let base = PageSetup { paper: Some("A3".into()), ..PageSetup::default() };
        let at = |anchor| layout(&PageSetup { anchor, ..base.clone() }, P2(10_000.0, 10_000.0), 100.0, None).unwrap().placed.0;
        assert_eq!(at(Anchor::TopLeft), P2(0.0, 0.0));
        assert!(close(at(Anchor::BottomRight).0, 320.0) && close(at(Anchor::BottomRight).1, 197.0));
        assert!(close(at(Anchor::Centre).0, 160.0));
    }

    #[test]
    fn a_tall_drawing_turns_a_named_paper_upright() {
        let setup = PageSetup { paper: Some("A3".into()), scale: Scale::Fit, ..PageSetup::default() };
        assert_eq!(layout(&setup, P2(100.0, 300.0), 1.0, None).unwrap().paper, P2(297.0, 420.0));
        let landscape = PageSetup { orientation: Orientation::Landscape, ..setup };
        assert_eq!(layout(&landscape, P2(100.0, 300.0), 1.0, None).unwrap().paper, P2(420.0, 297.0));
    }

    #[test]
    fn a_typed_scale_and_a_page_range() {
        let setup = PageSetup { paper: Some("A4".into()), scale: Scale::Custom(50.0), pages: Some([2, 3]), ..PageSetup::default() };
        // 30 m × 10 m at 1:50 is 600 × 200 mm: three A4 landscape pages across, one down.
        let l = layout(&setup, P2(30_000.0, 10_000.0), 100.0, None).unwrap();
        assert_eq!((l.cols, l.rows), (3, 1));
        assert_eq!(l.pages.iter().map(|t| t.col).collect::<Vec<_>>(), vec![1, 2]);
        let past = PageSetup { pages: Some([4, 9]), ..setup };
        assert!(layout(&past, P2(30_000.0, 10_000.0), 100.0, None).unwrap_err().contains("no page 4"));
    }

    #[test]
    fn without_a_paper_the_page_is_as_large_as_the_drawing() {
        let setup = PageSetup { margin: 5.0, ..PageSetup::default() };
        let l = layout(&setup, P2(20_000.0, 10_000.0), 100.0, None).unwrap();
        assert_eq!(l.paper, P2(210.0, 110.0));
        assert_eq!((l.cols, l.rows), (1, 1));
    }

    #[test]
    fn hostile_setups_are_refused_not_panicked_on() {
        let d = PageSetup::default();
        assert!(layout(&d, P2(f64::NAN, 1.0), 1.0, None).is_err());
        assert!(layout(&d, P2(1.0, 1.0), 0.0, None).is_err());
        assert!(layout(&PageSetup { margin: -1.0, ..d.clone() }, P2(100.0, 100.0), 1.0, None).is_err());
        assert!(layout(&PageSetup { scale: Scale::Custom(f64::INFINITY), ..d.clone() }, P2(100.0, 100.0), 1.0, None).is_err());
        assert!(layout(&PageSetup { paper: Some("B9".into()), ..d.clone() }, P2(100.0, 100.0), 1.0, None).is_err());
        assert!(layout(&PageSetup { size: Some(P2(1e9, 10.0)), ..d.clone() }, P2(100.0, 100.0), 1.0, None).is_err());
        // A whole city at 1:1 on A4 would be millions of pages.
        let huge = PageSetup { paper: Some("A4".into()), ..d.clone() };
        assert!(layout(&huge, P2(1e7, 1e7), 1.0, None).unwrap_err().contains("more than"));
        assert!(layout(&PageSetup { pages: Some([0, 1]), ..d }, P2(100.0, 100.0), 1.0, None).is_err());
    }
}
