//! The map a place is picked on: where a view of the Web Mercator map is, what its pixels are on the Earth, which tiles cover it and
//! how a drag becomes a box. All pure (no tiles, no window): a front end draws what this says, so its widget stays thin and this is
//! tested without one. The view's size is in pixels, its origin the top left; the map's own tiles are 256 pixels at their zoom.

use crate::geo::{Bbox, MAX_FETCH_AREA_KM2};

/// The coarsest the map is drawn: the whole world in a window a few hundred pixels across.
pub const MIN_ZOOM: f64 = 1.0;
/// The finest: OpenStreetMap's own tiles go no further.
pub const MAX_ZOOM: f64 = 19.0;
/// The most pixels across a view is worked out for.
pub const MAX_VIEW: f64 = 4096.0;
/// The most tiles one view may ask for (a window is some twenty).
pub const MAX_VISIBLE: usize = 100;
/// The side of a tile, pixels, at its own zoom.
pub const TILE: f64 = 256.0;
/// The map ends at this latitude.
const MAX_LAT: f64 = 85.051_128_779_8;

/// A tile of the map: its zoom, and its column and row at it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TileKey {
    pub zoom: u8,
    pub x: u32,
    pub y: u32,
}

/// A tile where it lies in a view: its top left corner and its side, pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub key: TileKey,
    pub at: (f64, f64),
    pub side: f64,
}

/// What part of the map a window shows: the longitude and latitude at its middle, and a zoom (not a whole number: the map is drawn
/// between two tile zooms while it is zoomed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    pub lon: f64,
    pub lat: f64,
    pub zoom: f64,
}

/// A place on the unit square of the map: x from the west, y from the north.
fn unit(lon: f64, lat: f64) -> (f64, f64) {
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    ((lon + 180.0) / 360.0, (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0)
}

fn lonlat(u: f64, v: f64) -> (f64, f64) {
    (u * 360.0 - 180.0, (std::f64::consts::PI * (1.0 - 2.0 * v)).sinh().atan().to_degrees())
}

/// A longitude brought into [-180, 180).
fn wrap(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0) - 180.0
}

impl MapView {
    /// A view at a place, which is made fit the map: a longitude wrapped round, a latitude and a zoom kept to what is drawn. A number
    /// that is not a number is the middle of the map at the coarsest zoom.
    pub fn new(lon: f64, lat: f64, zoom: f64) -> MapView {
        let ok = |x: f64| if x.is_finite() { x } else { 0.0 };
        MapView {
            lon: wrap(ok(lon)),
            lat: ok(lat).clamp(-MAX_LAT, MAX_LAT),
            zoom: if zoom.is_finite() { zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { MIN_ZOOM },
        }
    }

    /// The pixels of the whole map at this zoom, across.
    fn world(&self) -> f64 {
        TILE * 2f64.powf(self.zoom)
    }

    /// The zoom of the tiles drawn: the nearest whole one.
    pub fn tile_zoom(&self) -> u8 {
        self.zoom.round().clamp(0.0, MAX_ZOOM) as u8
    }

    /// Where a place is in a view `size` across, pixels. A place on the other side of the date line is where it would be if the map
    /// went on round.
    pub fn to_screen(&self, lon: f64, lat: f64, size: (f64, f64)) -> (f64, f64) {
        let ((cu, cv), (u, v)) = (unit(self.lon, self.lat), unit(lon, lat));
        let du = (u - cu + 0.5).rem_euclid(1.0) - 0.5;
        (size.0 / 2.0 + du * self.world(), size.1 / 2.0 + (v - cv) * self.world())
    }

    /// The longitude and latitude at a pixel of a view `size` across.
    pub fn from_screen(&self, p: (f64, f64), size: (f64, f64)) -> (f64, f64) {
        let (cu, cv) = unit(self.lon, self.lat);
        let (u, v) = (cu + (p.0 - size.0 / 2.0) / self.world(), (cv + (p.1 - size.1 / 2.0) / self.world()).clamp(0.0, 1.0));
        let (lon, lat) = lonlat(u, v);
        (wrap(lon), lat)
    }

    /// The map dragged `dx`, `dy` pixels (right and down): what was under the pointer stays under it.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (cu, cv) = unit(self.lon, self.lat);
        let (u, v) = (cu - dx / self.world(), (cv - dy / self.world()).clamp(0.0, 1.0));
        let (lon, lat) = lonlat(u, v);
        (self.lon, self.lat) = (wrap(lon), lat.clamp(-MAX_LAT, MAX_LAT));
    }

    /// The zoom changed by `by`, about the pixel `p` of a view `size` across: the place under it stays under it.
    pub fn zoom_at(&mut self, p: (f64, f64), size: (f64, f64), by: f64) {
        if !by.is_finite() {
            return;
        }
        let (lon, lat) = self.from_screen(p, size);
        self.zoom = (self.zoom + by).clamp(MIN_ZOOM, MAX_ZOOM);
        // The middle that puts that place back under `p`.
        let (u, v) = unit(lon, lat);
        let (cu, cv) = (u - (p.0 - size.0 / 2.0) / self.world(), (v - (p.1 - size.1 / 2.0) / self.world()).clamp(0.0, 1.0));
        let (clon, clat) = lonlat(cu, cv);
        (self.lon, self.lat) = (wrap(clon), clat.clamp(-MAX_LAT, MAX_LAT));
    }

    /// The tiles that cover a view `size` across, the nearest its middle first, each with where it lies; at most [`MAX_VISIBLE`].
    /// A column past the date line is the column on the other side of the map, drawn where it lies.
    pub fn visible(&self, size: (f64, f64)) -> Vec<Placed> {
        if !(size.0 > 0.0 && size.1 > 0.0 && size.0.is_finite() && size.1.is_finite()) {
            return Vec::new();
        }
        // Whatever the window, no more than a screen's worth is worked out.
        let size = (size.0.min(MAX_VIEW), size.1.min(MAX_VIEW));
        let z = self.tile_zoom();
        let n = 2f64.powi(i32::from(z));
        let side = TILE * 2f64.powf(self.zoom - f64::from(z));
        let (cu, cv) = unit(self.lon, self.lat);
        let (cx, cy) = (cu * n, cv * n);
        let (x0, x1) = ((cx - size.0 / 2.0 / side).floor(), (cx + size.0 / 2.0 / side).floor());
        let (y0, y1) = ((cy - size.1 / 2.0 / side).floor().max(0.0), (cy + size.1 / 2.0 / side).floor().min(n - 1.0));
        let mut placed = Vec::new();
        let mut y = y0;
        while y <= y1 {
            let mut x = x0;
            while x <= x1 {
                let key = TileKey { zoom: z, x: x.rem_euclid(n) as u32, y: y as u32 };
                placed.push(Placed { key, at: (size.0 / 2.0 + (x - cx) * side, size.1 / 2.0 + (y - cy) * side), side });
                x += 1.0;
            }
            y += 1.0;
        }
        let middle = |p: &Placed| {
            let (dx, dy) = (p.at.0 + p.side / 2.0 - size.0 / 2.0, p.at.1 + p.side / 2.0 - size.1 / 2.0);
            dx * dx + dy * dy
        };
        placed.sort_by(|a, b| middle(a).total_cmp(&middle(b)));
        placed.truncate(MAX_VISIBLE);
        placed
    }

    /// The view that shows `bbox` whole, with a margin, in a window `size` across.
    pub fn fit(bbox: &Bbox, size: (f64, f64)) -> MapView {
        let ((u0, v0), (u1, v1)) = (unit(bbox.west, bbox.north), unit(bbox.east, bbox.south));
        let (w, h) = ((u1 - u0).max(1e-12), (v1 - v0).max(1e-12));
        let (usable_w, usable_h) = ((size.0 * 0.8).max(1.0), (size.1 * 0.8).max(1.0));
        let zoom = (usable_w / (TILE * w)).min(usable_h / (TILE * h)).log2();
        let (lon, lat) = bbox.centre();
        MapView::new(lon, lat, zoom)
    }

    /// The box a drag between two pixels makes, or why it makes none (a drag of no size, or off the map).
    pub fn box_between(&self, a: (f64, f64), b: (f64, f64), size: (f64, f64)) -> Result<Bbox, String> {
        let ((lon_a, lat_a), (lon_b, lat_b)) = (self.from_screen(a, size), self.from_screen(b, size));
        // A drag across the date line would be a box round the world: the map is not asked for that.
        if (self.to_screen(lon_a, lat_a, size).0 - a.0).abs() > 1.0 || (lon_a - lon_b).abs() > 180.0 {
            return Err("the box crosses the date line".into());
        }
        Bbox::new(lat_a.min(lat_b), lon_a.min(lon_b), lat_a.max(lat_b), lon_a.max(lon_b))
    }

    /// The lines of longitude and of latitude drawn over a map that has no picture, a round number of degrees apart: as far apart as
    /// keeps a few in the view.
    pub fn graticule(&self, size: (f64, f64)) -> (Vec<f64>, Vec<f64>) {
        let ((west, north), (east, south)) = (self.from_screen((0.0, 0.0), size), self.from_screen(size, size));
        let span = (east - west).rem_euclid(360.0).max(1e-9).min((north - south).abs() * 2.0 + 360.0);
        let step = [0.0001, 0.0002, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0]
            .into_iter()
            .find(|s| span / s <= 6.0)
            .unwrap_or(60.0);
        let lines = |from: f64, to: f64| -> Vec<f64> {
            let first = (from / step).ceil();
            (0..=((to - from) / step).floor().max(0.0) as usize)
                .take(12)
                .map(|i| (first + i as f64) * step)
                .filter(|v| (from..=to).contains(v))
                .collect()
        };
        let lons = if west <= east { lines(west, east) } else { Vec::new() };
        (lons, lines(south, north))
    }
}

/// What a box that is being drawn says about itself: its area, and whether one fetch may cover it.
pub fn box_note(b: &Bbox) -> (f64, bool) {
    let a = b.area_km2();
    (a, a <= MAX_FETCH_AREA_KM2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::tile_xy;

    const SIZE: (f64, f64) = (800.0, 600.0);
    const HOME: (f64, f64) = (-46.6333, -23.5505);

    fn near(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn the_middle_of_the_view_is_its_place_and_a_pixel_goes_there_and_back() {
        let v = MapView::new(HOME.0, HOME.1, 12.0);
        let (x, y) = v.to_screen(HOME.0, HOME.1, SIZE);
        assert!(near(x, 400.0, 1e-9) && near(y, 300.0, 1e-9), "{x} {y}");
        for p in [(0.0, 0.0), (799.0, 599.0), (123.4, 456.7), (400.0, 300.0)] {
            let (lon, lat) = v.from_screen(p, SIZE);
            let q = v.to_screen(lon, lat, SIZE);
            assert!(near(q.0, p.0, 1e-6) && near(q.1, p.1, 1e-6), "{p:?} -> {q:?}");
        }
        // At zoom 12 a pixel is 156543.03392 * cos(lat) / 2^12 metres across: 100 pixels east is that far.
        let (lon, _) = v.from_screen((500.0, 300.0), SIZE);
        let metres = (lon - HOME.0) * 111_319.49 * HOME.1.to_radians().cos();
        let want = 156_543.033_92 * HOME.1.to_radians().cos() / 4096.0 * 100.0;
        assert!(near(metres, want, want * 0.002), "{metres} {want}");
    }

    #[test]
    fn a_drag_moves_the_map_under_the_pointer_and_a_wheel_zooms_about_it() {
        let mut v = MapView::new(HOME.0, HOME.1, 14.0);
        let under = v.from_screen((250.0, 200.0), SIZE);
        v.pan(37.0, -21.0);
        let moved = v.to_screen(under.0, under.1, SIZE);
        assert!(near(moved.0, 287.0, 1e-6) && near(moved.1, 179.0, 1e-6), "{moved:?}");
        // The place under the cursor stays there as the zoom changes, in and out.
        for by in [1.0, 0.37, -2.5, 3.0] {
            let p = (610.0, 140.0);
            let before = v.from_screen(p, SIZE);
            v.zoom_at(p, SIZE, by);
            let after = v.to_screen(before.0, before.1, SIZE);
            assert!(near(after.0, p.0, 1e-6) && near(after.1, p.1, 1e-6), "{by}: {after:?}");
        }
    }

    #[test]
    fn the_view_stays_on_the_map() {
        let v = MapView::new(190.0, 95.0, 30.0);
        assert!(near(v.lon, -170.0, 1e-9) && near(v.lat, MAX_LAT, 1e-9) && v.zoom == MAX_ZOOM, "{v:?}");
        let v = MapView::new(f64::NAN, f64::INFINITY, f64::NAN);
        assert_eq!((v.lon, v.zoom), (0.0, MIN_ZOOM));
        let mut v = MapView::new(179.9, 0.0, 5.0);
        v.pan(-50_000.0, 0.0);
        assert!((-180.0..180.0).contains(&v.lon), "{}", v.lon);
        let mut v = MapView::new(0.0, 80.0, 3.0);
        v.pan(0.0, 1e9);
        assert!(v.lat.is_finite() && v.lat.abs() <= MAX_LAT);
        let mut v = MapView::new(0.0, 0.0, 5.0);
        v.zoom_at((1.0, 1.0), SIZE, f64::NAN);
        v.zoom_at((1.0, 1.0), SIZE, -100.0);
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn the_tiles_cover_the_view_and_the_one_at_the_middle_is_the_place_s() {
        let v = MapView::new(HOME.0, HOME.1, 12.0);
        let tiles = v.visible(SIZE);
        assert!((12..=20).contains(&tiles.len()), "{}", tiles.len());
        // The nearest the middle is the tile the place is in (tile_xy is the independent reckoning).
        let (tx, ty) = tile_xy(HOME.0, HOME.1, 12);
        assert_eq!((tiles[0].key.x, tiles[0].key.y), (tx.floor() as u32, ty.floor() as u32));
        // Between them they cover every pixel: the corners and the middle are each inside one.
        for p in [(0.5, 0.5), (799.5, 0.5), (0.5, 599.5), (799.5, 599.5), (400.0, 300.0)] {
            assert!(tiles.iter().any(|t| p.0 >= t.at.0 && p.0 < t.at.0 + t.side && p.1 >= t.at.1 && p.1 < t.at.1 + t.side), "{p:?}");
        }
        // Between two zooms the tiles of the nearer are drawn at their scale: 12.4 draws zoom 12 tiles 256 * 2^0.4 across.
        let v = MapView::new(HOME.0, HOME.1, 12.4);
        assert!(near(v.visible(SIZE)[0].side, 256.0 * 2f64.powf(0.4), 1e-9));
        assert_eq!(v.visible(SIZE)[0].key.zoom, 12);
        // The tile where the place is, is where the place is drawn.
        let t = v.visible(SIZE)[0];
        let (px, py) = v.to_screen(HOME.0, HOME.1, SIZE);
        assert!(px >= t.at.0 && px < t.at.0 + t.side && py >= t.at.1 && py < t.at.1 + t.side);
    }

    #[test]
    fn the_map_goes_round_the_date_line_and_ends_at_the_poles() {
        let v = MapView::new(179.9, 0.0, 4.0);
        let tiles = v.visible(SIZE);
        // Columns past the east edge are the west columns, drawn east of it.
        assert!(
            tiles.iter().any(|t| t.key.x == 0) && tiles.iter().any(|t| t.key.x == 15),
            "{:?}",
            tiles.iter().map(|t| t.key.x).collect::<Vec<_>>()
        );
        assert!(tiles.iter().all(|t| t.key.x < 16 && t.key.y < 16));
        let (x_east, _) = v.to_screen(-179.9, 0.0, SIZE);
        assert!(x_east > 400.0 && x_east < 420.0, "{x_east}");
        // No tile above or below the map.
        let v = MapView::new(0.0, 85.0, 2.0);
        assert!(v.visible(SIZE).iter().all(|t| t.key.y < 4));
    }

    #[test]
    fn a_view_asks_for_a_bounded_number_of_tiles_whatever_the_window() {
        let v = MapView::new(0.0, 0.0, 19.0);
        for size in [(1e9, 1e9), (0.0, 600.0), (f64::NAN, 1.0), (-5.0, -5.0), (16_000.0, 16_000.0)] {
            assert!(v.visible(size).len() <= MAX_VISIBLE, "{size:?}");
        }
        assert!(v.visible((0.0, 600.0)).is_empty() && v.visible((f64::NAN, 1.0)).is_empty());
    }

    #[test]
    fn fit_shows_a_box_whole_and_a_drag_makes_a_box() {
        let b = Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap();
        let v = MapView::fit(&b, SIZE);
        let ((x0, y0), (x1, y1)) = (v.to_screen(b.west, b.north, SIZE), v.to_screen(b.east, b.south, SIZE));
        assert!(x0 > 0.0 && y0 > 0.0 && x1 < 800.0 && y1 < 600.0, "{x0} {y0} {x1} {y1}");
        assert!((x1 - x0) > 300.0 || (y1 - y0) > 240.0, "it fills most of the view: {} x {}", x1 - x0, y1 - y0);
        // Dragged from any corner to any other, the box is the same.
        let (a, c) = ((x0, y0), (x1, y1));
        let first = v.box_between(a, c, SIZE).unwrap();
        for (p, q) in [((x1, y1), (x0, y0)), ((x0, y1), (x1, y0)), ((x1, y0), (x0, y1))] {
            let again = v.box_between(p, q, SIZE).unwrap();
            assert!(near(again.south, first.south, 1e-9) && near(again.east, first.east, 1e-9));
        }
        assert!(near(first.west, b.west, 1e-6) && near(first.north, b.north, 1e-6), "{first:?}");
        // A drag that goes nowhere is no box.
        assert!(v.box_between((100.0, 100.0), (100.0, 100.0), SIZE).is_err());
        assert!(v.box_between((100.0, 100.0), (100.0, 300.0), SIZE).is_err());
        // A fit of a world-sized box is a view that stays on the map.
        let world = Bbox::new(-80.0, -179.0, 80.0, 179.0).unwrap();
        let all = MapView::fit(&world, SIZE);
        assert!((MIN_ZOOM..=MAX_ZOOM).contains(&all.zoom) && all.zoom < 2.0, "{}", all.zoom);
        // A box of no size is a view at the finest zoom, not a number that is not one.
        let point = Bbox::new(10.0, 10.0, 10.000_000_1, 10.000_000_1).unwrap();
        assert_eq!(MapView::fit(&point, SIZE).zoom, MAX_ZOOM);
    }

    #[test]
    fn the_area_of_a_drawn_box_is_said_against_the_cap() {
        let small = Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap();
        assert!(box_note(&small).1);
        assert!(near(box_note(&small).0, 4.53, 0.01));
        let big = Bbox::new(-23.6, -46.7, -23.5, -46.6).unwrap();
        assert!(!box_note(&big).1);
    }

    #[test]
    fn the_graticule_has_a_few_round_lines() {
        let v = MapView::new(HOME.0, HOME.1, 12.0);
        let (lons, lats) = v.graticule(SIZE);
        assert!((2..=8).contains(&lons.len()) && (2..=8).contains(&lats.len()), "{lons:?} {lats:?}");
        for x in lons.iter().chain(&lats) {
            let step = (x * 1e4).round() / 1e4;
            assert!(near(*x, step, 1e-9), "{x} is not a round number of degrees");
        }
        // Zoomed right out: whole degrees, and no more lines than fit.
        let (lons, lats) = MapView::new(0.0, 0.0, 1.0).graticule(SIZE);
        assert!(lons.len() <= 12 && lats.len() <= 12);
    }
}
