//! Aerial imagery, pure: the source table, the plan (which zoom, which tiles), the mosaic and the warp to the project's axes.

use crate::geo::{Bbox, tile_lonlat, tile_xy};
use crate::geom::P2;
use crate::georef::Location;
use crate::imagery::*;

fn at(latitude: f64, longitude: f64, true_north: f64) -> Location {
    Location { latitude, longitude, elevation: 0.0, true_north, crs: None }
}

#[test]
fn the_table_has_sources_that_can_be_asked_for_tiles() {
    let mut ids = std::collections::BTreeSet::new();
    for s in SOURCES {
        assert!(ids.insert(s.id), "{} twice", s.id);
        assert!(s.min_zoom <= s.max_zoom && s.max_zoom <= MAX_ZOOM, "{}: zooms {} to {}", s.id, s.min_zoom, s.max_zoom);
        assert!(Bbox::new(s.bounds[0], s.bounds[1], s.bounds[2], s.bounds[3]).is_ok(), "{}: bounds", s.id);
        assert!(!s.name.is_empty() && !s.credit.is_empty() && s.terms.starts_with("https://"), "{}: name, credit and terms", s.id);
        assert!(s.template.starts_with("https://") && !s.template.contains(char::is_whitespace), "{}: https, no spaces", s.id);
        let url = s.url(s.min_zoom, 0, 0).unwrap_or_else(|e| panic!("{}: {e}", s.id));
        assert!(url.starts_with("https://") && !url.contains('{'), "{url}");
        // Tiles outside the zooms it has, or off the map, are not asked for.
        assert!(s.url(s.max_zoom + 1, 0, 0).is_err() && s.url(s.max_zoom, u32::MAX, 0).is_err(), "{}", s.id);
        if s.min_zoom > 0 {
            assert!(s.url(s.min_zoom - 1, 0, 0).is_err(), "{}", s.id);
        }
    }
    assert!(ids.contains("flanders") && ids.contains("pdok") && ids.contains("usgs") && ids.contains("eox2016"));
    assert_eq!(source("pdok").map(|s| s.id), Some("pdok"));
    for not in ["bing", "esri", "google", "", "PDOK"] {
        assert!(source(not).is_none(), "{not}");
    }
}

#[test]
fn flanders_is_asked_with_an_empty_style_as_every_published_request_for_that_service_is() {
    let flanders = source("flanders").expect("the table has Flanders");
    let url = flanders.url(17, 66888, 43848).unwrap_or_default();
    assert!(url.contains("&style=&") && !url.contains("default"), "{url}");
    assert!(flanders.credit.starts_with("(c) Digitaal Vlaanderen"), "the credit Digitaal Vlaanderen recommends: {}", flanders.credit);
}

#[test]
fn a_box_is_offered_the_sources_that_have_its_place_finest_first() {
    let ids = |lon: f64, lat: f64| -> Vec<&str> {
        let b = Bbox::around(lon, lat, 200.0).unwrap();
        candidates(&b).iter().map(|s| s.id).collect()
    };
    // The rectangles overlap at a border: the tiles decide, and the first source that has real imagery of the place is taken.
    assert_eq!(ids(3.7174, 51.0543), ["flanders", "pdok", "eox2016"], "Ghent");
    assert_eq!(ids(4.9041, 52.3676), ["pdok", "eox2016"], "Amsterdam");
    assert_eq!(ids(-77.0369, 38.9072), ["usgs", "eox2016"], "Washington");
    assert_eq!(ids(-30.0, 0.0), ["eox2016"], "mid-Atlantic");
}

#[test]
fn a_zoom_is_the_finest_that_gives_the_pixels_asked_for_and_the_tiles_that_cover_the_box() {
    let pdok = source("pdok").unwrap();
    let around = |lon: f64, lat: f64, half: f64| Bbox::around(lon, lat, half).unwrap();
    // A 300 by 200 m site at 52 N, 1024 px along the long side: 0.29 m a pixel wanted, so tiles of 0.37 m (zoom 18) are the finest.
    let site = around(4.9041, 52.3676, 180.0);
    let p = plan(pdok, &site, 300.0, 200.0, 1024).unwrap();
    assert_eq!(p.zoom, 18, "{p:?}");
    assert!((p.metres_per_pixel - 0.3626).abs() < 0.01, "{}", p.metres_per_pixel);
    let pixels = |metres: f64| (metres / p.metres_per_pixel).ceil() as u32;
    assert_eq!((p.width, p.height), (pixels(300.0), pixels(200.0)), "the raster has the tiles' pixels, no more, so no blur");
    assert!(p.width < 1024, "{p:?}");
    assert!(p.cols >= 2 && p.rows >= 2 && (p.cols * p.rows) as usize <= MAX_TILES, "{p:?}");
    // It covers the box.
    let (x0, y0) = tile_xy(site.west, site.north, p.zoom);
    let (x1, y1) = tile_xy(site.east, site.south, p.zoom);
    assert!(f64::from(p.x0) <= x0 && f64::from(p.y0) <= y0 && f64::from(p.x0 + p.cols) >= x1 && f64::from(p.y0 + p.rows) >= y1, "{p:?}");
    // A source that does not go that fine gives its finest, and the raster has that many pixels: 12 m tiles over 5 km.
    let eox = source("eox2016").unwrap();
    let wide = plan(eox, &around(4.35, 50.85, 2500.0), 5000.0, 5000.0, 1024).unwrap();
    assert_eq!(wide.zoom, 14, "{wide:?}");
    assert!((wide.metres_per_pixel - 6.1).abs() < 0.3 && wide.width == wide.height && wide.width < 1024, "{wide:?}");
    // More pixels asked for, a finer zoom; fewer, a coarser.
    assert!(plan(pdok, &site, 300.0, 200.0, 2048).unwrap().zoom > p.zoom);
    assert!(plan(pdok, &site, 300.0, 200.0, 256).unwrap().zoom < p.zoom);
    // A box too big for the tiles it would need goes coarser, down to the source's coarsest zoom, and then is refused.
    let huge = around(4.9, 52.3, 49_000.0);
    let coarse = plan(pdok, &huge, 98_000.0, 98_000.0, 2048).unwrap();
    assert!((coarse.cols * coarse.rows) as usize <= MAX_TILES, "{coarse:?}");
    assert!(plan(pdok, &around(4.9, 52.3, 900_000.0), 1.8e6, 1.8e6, 2048).is_err());
}

#[test]
fn a_plan_says_how_many_tiles_it_takes_what_they_weigh_about_and_whether_they_are_too_coarse_for_buildings() {
    let site = Bbox::around(4.9041, 52.3676, 180.0).unwrap();
    let p = plan(source("pdok").unwrap(), &site, 300.0, 200.0, 1024).unwrap();
    assert_eq!(p.tiles(), u64::from(p.cols) * u64::from(p.rows));
    assert_eq!(p.estimated_bytes(), p.tiles() * TYPICAL_TILE_BYTES);
    assert!(!p.is_coarse(), "0.36 m a pixel shows buildings: {p:?}");
    let wide = plan(source("eox2016").unwrap(), &Bbox::around(4.35, 50.85, 2500.0).unwrap(), 5000.0, 5000.0, 1024).unwrap();
    assert!(wide.is_coarse(), "6 m a pixel does not: {wide:?}");
    // The edge: exactly the limit is not coarse, a little over is, and a number that is not one is not.
    let at = |metres: f64| Plan { metres_per_pixel: metres, ..p.clone() };
    assert!(!at(COARSE_METRES).is_coarse() && at(COARSE_METRES + 0.01).is_coarse() && !at(f64::NAN).is_coarse());
    // Whatever a plan holds, the arithmetic does not overflow.
    let big = Plan { cols: u32::MAX, rows: u32::MAX, ..p.clone() };
    assert_eq!(big.tiles(), u64::from(u32::MAX) * u64::from(u32::MAX));
    assert_eq!(big.estimated_bytes(), u64::MAX);
}

#[test]
fn a_plan_asked_for_nonsense_says_what_is_wrong() {
    let pdok = source("pdok").unwrap();
    let site = Bbox::around(4.9, 52.3, 100.0).unwrap();
    for (w, h, side) in [
        (f64::NAN, 100.0, 1024),
        (100.0, f64::INFINITY, 1024),
        (0.0, 100.0, 1024),
        (-5.0, 100.0, 1024),
        (1e12, 100.0, 1024),
        (100.0, 100.0, 0),
        (100.0, 100.0, MIN_SIDE - 1),
        (100.0, 100.0, MAX_SIDE + 1),
        (100.0, 100.0, u32::MAX),
    ] {
        assert!(plan(pdok, &site, w, h, side).is_err(), "{w} {h} {side}");
    }
}

fn mosaic_of(plan: &Plan) -> Mosaic {
    Mosaic::new(plan)
}

#[test]
fn a_mosaic_that_is_not_one_is_refused_before_it_is_used() {
    let plan = Plan { zoom: 10, x0: 5, y0: 6, cols: 2, rows: 2, metres_per_pixel: 1.0, width: 100, height: 100 };
    let good = mosaic_of(&plan);
    good.check().unwrap();
    let wrong = |f: &dyn Fn(&mut Mosaic)| {
        let mut m = good.clone();
        f(&mut m);
        m.check().unwrap_err()
    };
    assert!(wrong(&|m| m.zoom = 40).contains("zoom"));
    assert!(wrong(&|m| m.zoom = 2).contains("map"), "tiles past the edge of the map");
    assert!(wrong(&|m| m.cols = 0).contains("tiles"));
    assert!(wrong(&|m| (m.cols, m.rows) = (u32::MAX, u32::MAX)).contains("tiles"), "a count that overflows");
    assert!(wrong(&|m| (m.cols, m.rows) = (11, 10)).contains("tiles"), "more tiles than one fetch may take");
    assert!(wrong(&|m| m.rgb.pop().map(drop).unwrap_or_default()).contains("pixels"));
    assert!(wrong(&|m| m.has.push(true)).contains("tiles"));
    // A tile is put in by place, and only a tile.
    let mut m = good.clone();
    assert!(m.put(1, 1, &vec![9; TILE * TILE * 3]).is_ok() && m.has == [false, false, false, true]);
    assert!(m.put(2, 0, &vec![9; TILE * TILE * 3]).is_err() && m.put(0, 2, &vec![9; TILE * TILE * 3]).is_err());
    assert!(m.put(0, 0, &[1, 2, 3]).is_err());
    m.check().unwrap();
}

const K_LON: f64 = 256_000.0;
const K_LAT: f64 = 384_000.0;

/// A mosaic over `plan` whose pixel colour is a plain function of its longitude and latitude, so a warp can be told from the truth.
fn painted(plan: &Plan, origin: (f64, f64)) -> Mosaic {
    let mut m = Mosaic::new(plan);
    for row in 0..plan.rows {
        for col in 0..plan.cols {
            let mut tile = Vec::with_capacity(TILE * TILE * 3);
            for y in 0..TILE {
                for x in 0..TILE {
                    let (lon, lat) = tile_lonlat(
                        f64::from(plan.x0 + col) + (x as f64 + 0.5) / TILE as f64,
                        f64::from(plan.y0 + row) + (y as f64 + 0.5) / TILE as f64,
                        plan.zoom,
                    );
                    tile.extend_from_slice(&colour_at(lon, lat, origin));
                }
            }
            m.put(col, row, &tile).unwrap();
        }
    }
    m
}

fn colour_at(lon: f64, lat: f64, origin: (f64, f64)) -> [u8; 3] {
    let r = 128.0 + (lon - origin.0) * K_LON;
    let g = 128.0 + (lat - origin.1) * K_LAT;
    [r.round().clamp(0.0, 255.0) as u8, g.round().clamp(0.0, 255.0) as u8, 77]
}

/// A 60 by 40 m site round the origin, the plan for it from `source`, and a mosaic painted for it.
fn site(location: Location, source: &str) -> (Location, [f64; 4], Plan, Mosaic) {
    let extent = [-30_000.0, -20_000.0, 30_000.0, 20_000.0];
    let projector = location.projector().unwrap();
    let envelope = envelope(extent, &|p| projector.lonlat(p)).unwrap();
    let plan = plan(self::source(source).unwrap(), &envelope, 60.0, 40.0, 1024).unwrap();
    let mosaic = painted(&plan, (location.longitude, location.latitude));
    (location, extent, plan, mosaic)
}

#[test]
fn the_envelope_holds_every_corner_of_the_extent_however_the_project_is_turned() {
    for true_north in [0.0, 30.0, 90.0, -45.0, 180.0] {
        let l = at(50.85, 4.35, true_north);
        let projector = l.projector().unwrap();
        let extent = [-300_000.0, -200_000.0, 300_000.0, 200_000.0];
        let e = envelope(extent, &|p| projector.lonlat(p)).unwrap();
        for (x, y) in [(extent[0], extent[1]), (extent[2], extent[1]), (extent[2], extent[3]), (extent[0], extent[3]), (0.0, 0.0)] {
            let (lon, lat) = projector.lonlat(P2(x, y)).unwrap();
            assert!(e.contains(lon, lat), "{true_north}: ({x}, {y}) is ({lon}, {lat}), outside {e:?}");
        }
    }
    // What cannot be mapped is refused, and so is a box that is not one.
    let l = at(50.85, 4.35, 0.0);
    let projector = l.projector().unwrap();
    assert!(envelope([f64::NAN, 0.0, 1.0, 1.0], &|p| projector.lonlat(p)).is_err());
    assert!(envelope([5.0, 0.0, 1.0, 1.0], &|p| projector.lonlat(p)).is_err());
    assert!(envelope([0.0, 0.0, 1.0, 1.0], &|_| Err("off the map".into())).is_err());
}

#[test]
fn a_warp_puts_every_pixel_where_the_ground_is_with_the_project_turned_to_true_north() {
    // 60 by 40 m at Brussels, the project turned 30 degrees: the picture is in the project's axes, the ground's colours rotated in it.
    let (location, extent, plan, mosaic) = site(at(50.85, 4.35, 30.0), "pdok");
    let projector = location.projector().unwrap();
    let raster = warp(&mosaic, extent, plan.width, plan.height, &|p| projector.lonlat(p)).unwrap();
    assert_eq!(raster.rgba.len(), raster.width as usize * raster.height as usize * 4);
    let mut worst = 0.0f64;
    let mut checked = 0;
    for v in (0..raster.height).step_by(7) {
        for u in (0..raster.width).step_by(7) {
            let at_px = &raster.rgba[(v as usize * raster.width as usize + u as usize) * 4..][..4];
            // The pixel's centre in the project, and its colour on the ground.
            let p = P2(
                extent[0] + (f64::from(u) + 0.5) / f64::from(raster.width) * (extent[2] - extent[0]),
                extent[3] - (f64::from(v) + 0.5) / f64::from(raster.height) * (extent[3] - extent[1]),
            );
            let (lon, lat) = projector.lonlat(p).unwrap();
            let want = colour_at(lon, lat, (location.longitude, location.latitude));
            assert_eq!(at_px[3], 255, "({u}, {v}) is inside the mosaic");
            for k in 0..3 {
                worst = worst.max((f64::from(at_px[k]) - f64::from(want[k])).abs());
            }
            checked += 1;
        }
    }
    assert!(checked > 100 && worst <= 2.0, "{checked} pixels, the worst {worst} levels from the ground's colour");
}

#[test]
fn north_is_up_and_east_is_right_where_the_project_is_not_turned() {
    // On a central meridian there is no convergence: +y is north.
    let (location, extent, plan, mosaic) = site(at(10.0, 3.0, 0.0), "eox2016");
    let projector = location.projector().unwrap();
    let raster = warp(&mosaic, extent, plan.width, plan.height, &|p| projector.lonlat(p)).unwrap();
    let px = |u: u32, v: u32| &raster.rgba[(v as usize * raster.width as usize + u as usize) * 4..][..4];
    let (w, h) = (raster.width - 1, raster.height - 1);
    assert!(px(0, 0)[1] > px(0, h)[1], "latitude falls going down: the first row is the north one");
    assert!(px(w, 0)[0] > px(0, 0)[0], "longitude rises going right");
    assert!((i32::from(px(0, 0)[0]) - i32::from(px(0, h)[0])).abs() <= 2, "and does not change going down");
    assert!((i32::from(px(0, 0)[1]) - i32::from(px(w, 0)[1])).abs() <= 2, "or latitude going right");
}

#[test]
fn where_there_is_no_imagery_the_picture_is_clear() {
    let (location, extent, plan, mut mosaic) = site(at(50.85, 4.35, 0.0), "pdok");
    let projector = location.projector().unwrap();
    // A tile with nothing in it, and a mosaic that covers only part of the extent.
    mosaic.has[0] = false;
    let raster = warp(&mosaic, extent, plan.width, plan.height, &|p| projector.lonlat(p)).unwrap();
    let alpha = |u: u32, v: u32| raster.rgba[(v as usize * raster.width as usize + u as usize) * 4 + 3];
    assert_eq!(alpha(0, 0), 0, "the corner in the empty tile is clear");
    assert_eq!(alpha(raster.width / 2, raster.height / 2), 255, "the middle is imagery");
    assert!(raster.rgba.chunks_exact(4).any(|p| p[3] == 0) && raster.rgba.chunks_exact(4).any(|p| p[3] == 255));
    // The same mosaic over an extent a kilometre wider than it is partly clear.
    let big = [-500_000.0, -500_000.0, 500_000.0, 500_000.0];
    let raster = warp(&mosaic, big, 64, 64, &|p| projector.lonlat(p)).unwrap();
    assert_eq!(raster.rgba[3], 0);
    assert!(raster.rgba.chunks_exact(4).filter(|p| p[3] == 255).count() < 64 * 64 / 4, "the site is a small part of it");
}

#[test]
fn a_world_file_puts_each_pixel_where_the_grid_says() {
    use crate::georef::lonlat_of_grid;
    // The picture is in the project's axes, which are turned and moved on the grid: a world file says how, so a GIS can place it.
    for l in [Location { crs: Some(32631), ..at(50.85, 4.35, 30.0) }, at(-23.5505, -46.6333, -75.0)] {
        let (grid, projector) = (l.grid().unwrap(), l.projector().unwrap());
        let extent = [-30_000.0, -20_000.0, 31_500.0, 21_000.0];
        let (width, height) = (615_u32, 410_u32); // 100 mm a pixel each way
        let [a, d, b, e, c, f] = world_file(&grid, extent, width, height);
        for (col, row) in [(0, 0), (width - 1, 0), (0, height - 1), (width - 1, height - 1), (300, 200), (17, 311)] {
            let (col_f, row_f) = (f64::from(col), f64::from(row));
            let (easting, northing) = (a * col_f + b * row_f + c, d * col_f + e * row_f + f);
            let (lon, lat) = lonlat_of_grid(grid.epsg, easting, northing).unwrap();
            // The middle of that pixel in the project's plane, 100 mm across, and where the project puts it.
            let p = P2(extent[0] + (col_f + 0.5) * 100.0, extent[3] - (row_f + 0.5) * 100.0);
            let (lon2, lat2) = projector.lonlat(p).unwrap();
            // Within a centimetre (1e-7 degrees): two transforms of one CRS agree to a millimetre, and a wrong turn is metres out.
            assert!(
                (lon - lon2).abs() < 1e-7 && (lat - lat2).abs() < 1e-7,
                "{l:?} pixel ({col}, {row}): {lon} {lat} against {lon2} {lat2}"
            );
        }
        // A step along a row is a step along the project's x: as long as the pixel, and turned as the project is.
        assert!(((a * a + d * d).sqrt() - grid.scale * 0.1).abs() < 1e-9 && ((b * b + e * e).sqrt() - grid.scale * 0.1).abs() < 1e-9);
    }
}

#[test]
fn a_warp_refuses_what_it_cannot_do_and_never_runs_off() {
    let (location, extent, plan, mosaic) = site(at(50.85, 4.35, 0.0), "pdok");
    let projector = location.projector().unwrap();
    let ok = |p: P2| projector.lonlat(p);
    for (what, extent, w, h) in [
        ("a NaN", [f64::NAN, 0.0, 1.0, 1.0], 8, 8),
        ("west of east", [5.0, 0.0, 1.0, 1.0], 8, 8),
        ("no height", [0.0, 1.0, 5.0, 1.0], 8, 8),
        ("no width", extent, 0, 8),
        ("no rows", extent, 8, 0),
        ("too wide", extent, MAX_SIDE + 1, 8),
        ("too tall", extent, 8, u32::MAX),
    ] {
        assert!(warp(&mosaic, extent, w, h, &ok).is_err(), "{what}");
    }
    assert!(warp(&mosaic, extent, plan.width, plan.height, &|_| Err("off the map".into())).is_err());
    assert!(warp(&mosaic, extent, plan.width, plan.height, &|_| Ok((f64::NAN, 0.0))).is_err());
    let mut broken = mosaic.clone();
    broken.rgb.pop();
    assert!(warp(&broken, extent, 8, 8, &ok).is_err(), "a mosaic that does not check");
}
