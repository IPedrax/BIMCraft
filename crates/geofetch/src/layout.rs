//! Where the nodes of an elevation grid are, the same for every source: a node every `spacing` metres from the box's north west
//! corner, one more each way than the box has room for, so that the grid covers the box.

use bimcraft_core::geo::{Bbox, MAX_GRID_NODES};

/// Metres in a degree of latitude, near enough: the core's figure.
const METRES_PER_DEGREE: f64 = 111_320.0;
/// The Web Mercator map's last latitude, rounded down.
const MAX_LAT: f64 = 85.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub dlon: f64,
    pub dlat: f64,
    pub cols: usize,
    pub rows: usize,
}

impl Layout {
    pub fn new(bbox: &Bbox, spacing: f64) -> Result<Layout, String> {
        if !spacing.is_finite() || spacing <= 0.0 {
            return Err(format!("`spacing` is a distance in metres above 0, not {spacing}"));
        }
        let dlat = spacing / METRES_PER_DEGREE;
        let dlon = spacing / (METRES_PER_DEGREE * bbox.centre().1.to_radians().cos().max(1e-3));
        let (cols, rows) = (((bbox.east - bbox.west) / dlon).floor() + 2.0, ((bbox.north - bbox.south) / dlat).floor() + 2.0);
        if cols * rows > MAX_GRID_NODES as f64 {
            return Err(format!(
                "a `spacing` of {spacing} m over this box would make {} nodes, more than {MAX_GRID_NODES}: use a larger `spacing` or a smaller box",
                cols * rows
            ));
        }
        Ok(Layout { dlon, dlat, cols: cols as usize, rows: rows as usize })
    }

    /// What the grid covers: the box, and the node past its south and east edges.
    pub fn extent(&self, bbox: &Bbox) -> Result<Bbox, String> {
        Bbox::new(
            (bbox.north - (self.rows - 1) as f64 * self.dlat).max(-MAX_LAT),
            bbox.west,
            bbox.north.min(MAX_LAT),
            (bbox.west + (self.cols - 1) as f64 * self.dlon).min(180.0),
        )
        .map_err(|_| "the box is too near the poles or the edge of the map for this elevation data".to_string())
    }

    /// Where node (`col`, `row`) is: longitude and latitude.
    pub fn at(&self, bbox: &Bbox, col: usize, row: usize) -> (f64, f64) {
        (bbox.west + col as f64 * self.dlon, bbox.north - row as f64 * self.dlat)
    }
}
