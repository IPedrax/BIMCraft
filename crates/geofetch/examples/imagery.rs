//! A developer's tool to try the aerial imagery services, which no test does: which tile addresses answer, with what, and what each
//! service says about its terms. It is not part of BIMCraft and is not shipped; what it finds decides how the fetcher is built.
//!
//! ```text
//! cargo run -p bimcraft-geofetch --example imagery -- probe [OUT_DIR]
//! cargo run -p bimcraft-geofetch --example imagery -- tile 'ADDRESS_WITH_{z}_{x}_{y}' LAT LON ZOOM [OUT_DIR]
//! cargo run -p bimcraft-geofetch --example imagery -- caps 'CAPABILITIES_URL' [OUT_DIR] [LAYER]
//! cargo run -p bimcraft-geofetch --example imagery -- warp SOURCE LAT LON WIDTH_M HEIGHT_M [TRUE_NORTH] [MAX_SIDE] [OUT_DIR]
//! ```
//!
//! `warp` is what BIMCraft will do for the terrain, on its own: it puts a project's origin at LAT LON, turned TRUE_NORTH degrees,
//! fetches the tiles of SOURCE (`auto`: the first of the table that has the place) over WIDTH_M by HEIGHT_M metres round it, and
//! lays them into the project's own axes, north up. It writes the picture, the picture with a 100 m grid, a world file (`.pgw`) and
//! a note of the grid it is in, so the result can be set beside a map or opened in a GIS.
//!
//! `probe` fetches tiles of the sources BIMCraft means to use, at a place in each, and prints a table; the addresses it tries are
//! guesses from memory unless a capabilities document gave them (see `CANDIDATES`), and `caps` is how to find the right ones. A tile
//! goes through the same client the app uses (`Ureq` under an `Allow` of exactly that address), so a redirect to another host is
//! refused by name. An address with a query string is something the app's client refuses; here it is fetched with a plain client
//! and the row says so, so a service that can only be asked by query can be tried before the app's rules are changed. The tiles are
//! saved in OUT_DIR (the temp folder's `bimcraft-imagery-spike`) to be looked at.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use std::sync::Arc;

use bimcraft_core::geo::GeoSource;
use bimcraft_core::geom::P2;
use bimcraft_core::georef::Location;
use bimcraft_core::imagery;
use bimcraft_core::texture::Picture;
use bimcraft_geofetch::Geofetch;
use bimcraft_geofetch::cache::Cache;
use bimcraft_geofetch::net::{Allow, Net, OVERPASS, Ureq};
use bimcraft_geofetch::probe::{Format, encode_png, expand, grid_overlay, kvp_address, metres_per_pixel, scan_capabilities, tile_at};

/// The most a tile may be, bytes, and the most of a capabilities document that is read.
const MAX_TILE: u64 = 2 << 20;
const MAX_CAPS: u64 = 4 << 20;
/// A pause between two requests: a handful of tiles is a light load, and still no hurry.
const PAUSE: Duration = Duration::from_millis(300);

/// A source to try.
struct Candidate {
    name: &'static str,
    /// A tile address with `{z}`, `{x}` and `{y}`. Those of PDOK, USGS and EOX answered or were corrected from their capabilities
    /// documents; Flanders' is built from its document the way a query-only WMTS service is asked, and its style is a guess.
    template: &'static str,
    place: &'static str,
    longitude: f64,
    latitude: f64,
    /// Zooms to try there, and one finer than the service is expected to go.
    zooms: &'static [u8],
    beyond: u8,
    caps: &'static str,
    /// What the research and the capabilities documents found about its terms. To verify at the service.
    licence: &'static str,
}

const CANDIDATES: [Candidate; 6] = [
    Candidate {
        name: "flanders",
        template: "https://geo.api.vlaanderen.be/OMWRGBMRVL/wmts?service=WMTS&request=GetTile&version=1.0.0&layer=omwrgbmrvl&style=default&tilematrixset=GoogleMapsVL&tilematrix={z}&tilerow={y}&tilecol={x}&format=image/jpeg",
        place: "Ghent",
        longitude: 3.7174,
        latitude: 51.0543,
        zooms: &[16, 17, 18, 19],
        beyond: 23,
        caps: "https://geo.api.vlaanderen.be/OMWRGBMRVL/wmts?request=GetCapabilities&service=WMTS",
        licence: "free of charge; the usage right is on Digitaal Vlaanderen's 'gebruiksrecht en privacyverklaring geografische webdiensten' page: what credit does it ask for?",
    },
    Candidate {
        name: "pdok",
        template: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/Actueel_orthoHR/EPSG:3857/{z}/{x}/{y}.jpeg",
        place: "Amsterdam",
        longitude: 4.9041,
        latitude: 52.3676,
        zooms: &[17, 19, 20, 21],
        beyond: 22,
        caps: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0?request=GetCapabilities&service=WMTS",
        licence: "CC-BY 4.0 per PDOK's dataset page (the document says fees and constraints: none): what wording does it ask for?",
    },
    Candidate {
        name: "pdok25",
        template: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/Actueel_ortho25/EPSG:3857/{z}/{x}/{y}.jpeg",
        place: "Amsterdam",
        longitude: 4.9041,
        latitude: 52.3676,
        zooms: &[15, 16, 17, 18],
        beyond: 22,
        caps: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0?request=GetCapabilities&service=WMTS",
        licence: "as pdok; the 25 cm layer",
    },
    Candidate {
        name: "pdok2024",
        template: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/2024_orthoHR/EPSG:3857/{z}/{x}/{y}.jpeg",
        place: "Amsterdam",
        longitude: 4.9041,
        latitude: 52.3676,
        zooms: &[17, 18, 19, 20],
        beyond: 22,
        caps: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0?request=GetCapabilities&service=WMTS",
        licence: "as pdok; one year's layer, so a project can name the year it shows",
    },
    Candidate {
        name: "usgs",
        template: "https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/tile/{z}/{y}/{x}",
        place: "Washington DC",
        longitude: -77.0369,
        latitude: 38.9072,
        zooms: &[12, 14, 15, 16],
        beyond: 17,
        caps: "https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/WMTS/1.0.0/WMTSCapabilities.xml",
        licence: "US public domain, and USGS asks for a credit; the document states no terms. The cache stopped at zoom 16 at DC.",
    },
    Candidate {
        name: "eox2016",
        template: "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/GoogleMapsCompatible/{z}/{y}/{x}.jpg",
        place: "Brussels",
        longitude: 4.3517,
        latitude: 50.8503,
        zooms: &[10, 12, 13, 14],
        beyond: 17,
        caps: "https://tiles.maps.eox.at/wmts/1.0.0/WMTSCapabilities.xml",
        licence: "attribution with links is mandatory, and 'additional restrictions may apply for individual layers as indicated in the respective abstract': read the layer's with `caps URL DIR s2cloudless_3857`; is this the 2016 mosaic?",
    },
];

/// One line of the table.
struct Row {
    source: String,
    zoom: String,
    metres: String,
    result: String,
    content_type: String,
    format: &'static str,
    bytes: usize,
    decoded: String,
    note: String,
}

impl Row {
    fn new(source: &str, zoom: u8, metres: Option<f64>) -> Row {
        Row {
            source: source.into(),
            zoom: zoom.to_string(),
            metres: metres.map_or("-".into(), |m| format!("{m:.2}")),
            result: String::new(),
            content_type: String::new(),
            format: "-",
            bytes: 0,
            decoded: "-".into(),
            note: String::new(),
        }
    }

    fn failed(source: &str, zoom: u8, metres: Option<f64>, result: String, note: &str) -> Row {
        Row { result, note: note.into(), ..Row::new(source, zoom, metres) }
    }
}

/// A table cell: one line, no bars, not long.
fn cell(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| {
            if c == '|' {
                '/'
            } else if c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    if flat.chars().count() > 110 { format!("{}...", flat.chars().take(107).collect::<String>()) } else { flat }
}

fn print_table(rows: &[Row]) {
    println!("| source | zoom | m/px | result | content-type | format | bytes | decoded | note |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for r in rows {
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(&r.source),
            r.zoom,
            r.metres,
            cell(&r.result),
            cell(&r.content_type),
            r.format,
            r.bytes,
            cell(&r.decoded),
            cell(&r.note)
        );
    }
}

/// The host of an address, for a message and a file name.
fn host_of(url: &str) -> String {
    let host = url.trim_start_matches("https://").split(['/', '?']).next().unwrap_or("service");
    host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

/// A plain GET, with none of the app's rules: its status (an error status is an answer), content type and body, cut at `limit`.
/// For what the app's client refuses (an address with a query string) and for capabilities documents.
fn plain_get(url: &str, limit: u64) -> Result<(u16, Option<String>, Vec<u8>), String> {
    if !url.starts_with("https://") || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("an https:// address without spaces is wanted".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .http_status_as_error(false)
        .user_agent(format!("BIMCraft-imagery-spike/{} (+https://github.com/IPedrax/BIMCraft)", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let response = agent
        .get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .call()
        .map_err(|e| format!("{}: {e}", host_of(url)))?;
    let status = response.status().as_u16();
    let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);
    let mut body = Vec::new();
    response
        .into_body()
        .into_with_config()
        .limit(limit.saturating_add(2))
        .reader()
        .take(limit)
        .read_to_end(&mut body)
        .map_err(|e| format!("{}: {e}", host_of(url)))?;
    Ok((status, content_type, body))
}

/// The size of a picture BIMCraft can read and the one colour it is made of, if it is, or why it cannot be read.
fn decoded(bytes: &[u8], format: Format) -> (String, Option<[u8; 3]>) {
    match format {
        Format::Png | Format::Jpeg => match Picture::import("probe", bytes) {
            Ok(picture) => {
                let (w, h) = picture.size();
                (format!("{w}x{h}"), picture.flat())
            }
            Err(why) => (format!("unreadable: {why}"), None),
        },
        _ => ("-".into(), None),
    }
}

/// The tile of `template` that `longitude` and `latitude` are on at `zoom`, fetched and saved in `out`. An address without a query
/// goes through the app's own client; one with a query string is fetched plainly, and the row says so.
fn try_tile(source: &str, template: &str, longitude: f64, latitude: f64, zoom: u8, out: &Path, why: &str) -> Row {
    let metres = metres_per_pixel(latitude, zoom);
    let Some((x, y)) = tile_at(longitude, latitude, zoom) else {
        return Row::failed(source, zoom, metres, "no tile there".into(), why);
    };
    let url = match expand(template, zoom, x, y) {
        Ok(url) => url,
        Err(e) => return Row::failed(source, zoom, metres, format!("bad address: {e}"), why),
    };
    let plain = url.contains('?');
    let reply: Result<(Option<String>, Vec<u8>), String> = if plain {
        plain_get(&url, MAX_TILE).and_then(|(status, content_type, body)| {
            if status >= 400 { Err(format!("{}: HTTP {status}", host_of(&url))) } else { Ok((content_type, body)) }
        })
    } else {
        Allow::default()
            .with_endpoint(&url)
            .map_err(|_| "the app's allow-list would not take this address".to_string())
            .and_then(|allow| Ureq::new(allow).get(&url, MAX_TILE).map(|r| (r.content_type, r.body)).map_err(|e| e.message))
    };
    std::thread::sleep(PAUSE);
    let (content_type, body) = match reply {
        Ok(reply) => reply,
        Err(e) => return Row::failed(source, zoom, metres, e, why),
    };
    let format = Format::sniff(&body);
    let (decoded, flat) = decoded(&body, format);
    let content_type = content_type.unwrap_or_default();
    let mut notes = vec![why.to_string()];
    if plain {
        notes.push("plain client: the app's allow-list refuses a query string".into());
    }
    match format {
        Format::Text => notes.push("a text reply, not a picture: see the saved file".into()),
        Format::Webp => notes.push("WebP: BIMCraft cannot read it yet".into()),
        Format::Png | Format::Jpeg if body.len() < 1500 => notes.push("very small: blank?".into()),
        _ => {}
    }
    if let Some([r, g, b]) = flat {
        notes.push(format!("one flat colour ({r}, {g}, {b}): no imagery here?"));
    }
    if content_type.starts_with("image/") && format == Format::Text {
        notes.push("says it is an image".into());
    }
    let file = out.join(format!("{source}-z{zoom}-{x}-{y}.{}", format.extension()));
    if let Err(e) = std::fs::write(&file, &body) {
        notes.push(format!("not saved: {e}"));
    }
    let note = notes.into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join("; ");
    Row { result: "ok".into(), content_type, format: format.name(), bytes: body.len(), decoded, note, ..Row::new(source, zoom, metres) }
}

fn out_dir(arg: Option<&String>) -> Result<PathBuf, String> {
    let dir = arg.map_or_else(|| std::env::temp_dir().join("bimcraft-imagery-spike"), PathBuf::from);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn number(args: &[String], i: usize, what: &str) -> Result<f64, String> {
    args.get(i).and_then(|a| a.parse().ok()).ok_or_else(|| format!("{what}: a number is wanted"))
}

fn probe(args: &[String]) -> Result<(), String> {
    let out = out_dir(args.first())?;
    let mut rows = Vec::new();
    for c in &CANDIDATES {
        eprintln!("{} ({})...", c.name, c.place);
        for zoom in c.zooms {
            rows.push(try_tile(c.name, c.template, c.longitude, c.latitude, *zoom, &out, c.place));
        }
        let far = c.zooms.get(1).copied().unwrap_or(16);
        rows.push(try_tile(c.name, c.template, -30.0, 0.0, far, &out, "far outside any coverage (mid-Atlantic)"));
        rows.push(try_tile(c.name, c.template, c.longitude, c.latitude, c.beyond, &out, "finer than the service is expected to go"));
    }
    println!();
    print_table(&rows);
    println!("\nThe tiles are in {}. Terms, as found so far (verify at the service):", out.display());
    for c in &CANDIDATES {
        println!("- {}: {}", c.name, c.licence);
    }
    println!("\nEach service's own capabilities document gives its layers, addresses and terms:");
    for c in CANDIDATES.iter().filter(|c| c.name != "pdok25" && c.name != "pdok2024") {
        println!("  cargo run -p bimcraft-geofetch --example imagery -- caps '{}'", c.caps);
    }
    Ok(())
}

fn tile(args: &[String]) -> Result<(), String> {
    let template = args.first().ok_or("the address of a tile is wanted, with {z}, {x} and {y} in it")?;
    let (latitude, longitude) = (number(args, 1, "latitude")?, number(args, 2, "longitude")?);
    let zoom = u8::try_from(number(args, 3, "zoom")? as i64).map_err(|_| "zoom: 0 to 24".to_string())?;
    let out = out_dir(args.get(4))?;
    let row = try_tile("tile", template, longitude, latitude, zoom, &out, "");
    print_table(&[row]);
    Ok(())
}

fn caps(args: &[String]) -> Result<(), String> {
    let url = args.first().ok_or("the address of a capabilities document, or the file of one saved before, is wanted")?;
    let out = out_dir(args.get(1))?;
    // From the network (and saved), or from a file saved before: no request, and the endpoint a query address would use is a stand-in.
    let (heading, body, endpoint) = if url.starts_with("https://") {
        let (status, content_type, body) = plain_get(url, MAX_CAPS)?;
        let file = out.join(format!("caps-{}.xml", host_of(url)));
        let saved = std::fs::write(&file, &body).map_or_else(|e| format!("not saved: {e}"), |()| format!("saved to {}", file.display()));
        let heading =
            format!("HTTP {status}, {}, {} bytes ({}), {saved}", content_type.unwrap_or_default(), body.len(), Format::sniff(&body).name());
        (heading, body, url.split('?').next().unwrap_or(url).to_string())
    } else {
        let mut body = Vec::new();
        std::fs::File::open(url).and_then(|f| f.take(MAX_CAPS).read_to_end(&mut body)).map_err(|e| format!("{url}: {e}"))?;
        (format!("read from {url}, {} bytes ({})", body.len(), Format::sniff(&body).name()), body, "https://ENDPOINT".to_string())
    };
    println!("{heading}");
    let text = String::from_utf8_lossy(&body);
    let c = scan_capabilities(&text);
    let list = |name: &str, items: &[String]| println!("{name}: {}", if items.is_empty() { "-".to_string() } else { items.join(" | ") });
    println!("title: {}", c.title.as_deref().unwrap_or("-"));
    list("fees", &c.fees);
    list("access constraints", &c.constraints);
    let names: Vec<&str> = c.layers.iter().map(|l| l.name.as_str()).collect();
    println!(
        "layers ({}): {}{}",
        names.len(),
        names.iter().take(14).copied().collect::<Vec<_>>().join(", "),
        if names.len() > 14 { ", ..." } else { "" }
    );
    // The layer asked for (a third argument), else the first.
    let chosen = args.get(2).and_then(|w| c.layers.iter().find(|l| l.name.eq_ignore_ascii_case(w))).or(c.layers.first());
    if let Some(l) = chosen {
        println!("\nlayer {}:", l.name);
        println!("  description: {}", if l.description.is_empty() { "-" } else { l.description.as_str() });
        println!("  styles: {}", l.styles.join(", "));
        println!("  formats: {}", l.formats.join(", "));
        println!("  offered in: {}", l.sets.join(", "));
    } else {
        println!("no layers found");
    }
    println!("\ntile matrix sets:");
    for s in &c.matrix_sets {
        let first: Vec<&str> = s.matrices.iter().take(3).map(|m| m.id.as_str()).collect();
        let verdict = s.web_mercator().map_or_else(|why| format!("no: {why}"), |()| "yes".to_string());
        println!("  {} ({}): {} levels, first {}; ordinary Web Mercator: {verdict}", s.name, s.crs, s.matrices.len(), first.join(", "));
    }
    println!("tile sizes: {}", c.tile_sizes.iter().map(u32::to_string).collect::<Vec<_>>().join(", "));
    println!("tile addresses ({}):", c.resource_urls.len());
    for u in c.resource_urls.iter().take(20) {
        println!("  {u}");
    }
    if c.resource_urls.iter().any(|u| u.contains(" http://")) {
        println!("(the document writes http://: the app fetches over https only, so try the same address with https://)");
    }
    if let (true, Some(l)) = (c.resource_urls.is_empty(), chosen) {
        // No addresses of its own: it is asked by query. Offer the addresses for the sets that are ordinary Web Mercator.
        let style = l.styles.first().map_or("default", String::as_str);
        let format = l.formats.first().map_or("image/jpeg", String::as_str);
        for set in l.sets.iter().filter(|name| c.matrix_sets.iter().any(|s| &s.name == *name && s.web_mercator().is_ok())) {
            match kvp_address(&endpoint, &l.name, style, set, format) {
                Ok(address) => println!(
                    "\nasked by query ({set}); try it with:\n  cargo run -p bimcraft-geofetch --example imagery -- tile '{address}' LAT LON ZOOM"
                ),
                Err(why) => println!("\nno address for {set}: {why}"),
            }
        }
    } else if c.layers.is_empty() && c.resource_urls.is_empty() {
        println!("no layers or addresses found. It began:\n{}", text.chars().take(400).collect::<String>());
    } else {
        println!(
            "\nAn address with {{TileMatrix}}, {{TileCol}} and {{TileRow}} can be tried as it is with the tile command; write in any\n\
             {{Style}} or {{TileMatrixSet}} by hand. If the tile matrices are not plain numbers (e.g. EPSG:3857:17), write that into the\n\
             address around {{z}}."
        );
    }
    Ok(())
}

/// The optional number at `args[i]`, or `default` if there is none.
fn number_or(args: &[String], i: usize, what: &str, default: f64) -> Result<f64, String> {
    if args.get(i).is_some() { number(args, i, what) } else { Ok(default) }
}

fn warp(args: &[String]) -> Result<(), String> {
    let wanted = args.first().ok_or("a source is wanted: auto, flanders, pdok, usgs or eox2016")?;
    let (latitude, longitude) = (number(args, 1, "latitude")?, number(args, 2, "longitude")?);
    let (width_m, height_m) = (number(args, 3, "width in metres")?, number(args, 4, "height in metres")?);
    let true_north = number_or(args, 5, "true north in degrees", 0.0)?;
    let max_side = number_or(args, 6, "pixels along the long side", f64::from(imagery::DEFAULT_SIDE))? as u32;
    let out = out_dir(args.get(7))?;
    // The project: its origin at the place, turned as asked; the picture covers the box round the origin.
    let location = Location { latitude, longitude, elevation: 0.0, true_north, crs: None };
    let projector = location.projector()?;
    let lonlat = |p: P2| projector.lonlat(p);
    let extent = [-width_m * 500.0, -height_m * 500.0, width_m * 500.0, height_m * 500.0];
    let envelope = imagery::envelope(extent, &lonlat)?;
    let cache = Arc::new(Cache::new(Some(std::env::temp_dir().join("bimcraft-geofetch-cache"))));
    let fetcher = Geofetch::new(Arc::new(Ureq::new(Allow::standard())), cache, OVERPASS);
    let sources: Vec<&imagery::Source> = if wanted == "auto" {
        imagery::candidates(&envelope)
    } else {
        vec![imagery::source(wanted).ok_or_else(|| format!("there is no source `{wanted}`: auto, or one of the table's ids"))?]
    };
    let mut last = "no source of the table has this place".to_string();
    for source in sources {
        let plan = imagery::plan(source, &envelope, width_m, height_m, max_side)?;
        eprintln!("{}: zoom {} ({:.2} m a pixel), {} x {} tiles...", source.name, plan.zoom, plan.metres_per_pixel, plan.cols, plan.rows);
        let got = match fetcher.imagery(source.id, &plan) {
            Ok(got) => got,
            Err(why) => {
                eprintln!("  {why}");
                last = why;
                continue;
            }
        };
        let raster = imagery::warp(&got.mosaic, extent, plan.width, plan.height, &lonlat)?;
        let name = out.join(format!("{}-warp", source.id));
        let save = |suffix: &str, bytes: &[u8]| {
            let path = PathBuf::from(format!("{}{suffix}", name.display()));
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display())).map(|()| path)
        };
        let plain = save(".png", &encode_png(raster.width, raster.height, &raster.rgba)?)?;
        let gridded = save(
            "-grid.png",
            &encode_png(raster.width, raster.height, &grid_overlay(&raster.rgba, raster.width, raster.height, extent, 100.0))?,
        )?;
        let grid = location.grid()?;
        let [a, d, b, e, c, f] = imagery::world_file(&grid, extent, raster.width, raster.height);
        let world = save(".pgw", format!("{a:.10}\n{d:.10}\n{b:.10}\n{e:.10}\n{c:.4}\n{f:.4}\n").as_bytes())?;
        let clear = raster.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
        let note = format!(
            "source: {} ({})\ncredit: {}\nterms: {}\nfetched from the cache: {}\nzoom {}, tiles of {:.3} m a pixel, {} x {} tiles\npicture: {} x {} px over {width_m} x {height_m} m, {:.1}% without imagery\nproject: origin at {latitude}, {longitude}, true north {true_north} degrees\ngrid of the world file: EPSG:{} ({}); the file has no CRS of its own, name it when opening\nextent in the project's plane, mm: west {}, south {}, east {}, north {}\n",
            source.name,
            source.id,
            source.credit,
            source.terms,
            got.cached,
            plan.zoom,
            plan.metres_per_pixel,
            plan.cols,
            plan.rows,
            raster.width,
            raster.height,
            100.0 * clear as f64 / (raster.width as f64 * raster.height as f64),
            grid.epsg,
            grid.name,
            extent[0],
            extent[1],
            extent[2],
            extent[3],
        );
        let notes = save(".txt", note.as_bytes())?;
        println!("{note}\nwritten:\n  {}\n  {}\n  {}\n  {}", plain.display(), gridded.display(), world.display(), notes.display());
        println!(
            "\nTo check it: north is up where the project is not turned; the lines of the grid are 100 m apart from the origin (the middle);\nroads and buildings should sit where a map puts them to a few metres."
        );
        return Ok(());
    }
    Err(last)
}

fn run(args: &[String]) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("probe") => probe(rest),
        Some("tile") => tile(rest),
        Some("caps") => caps(rest),
        Some("warp") => warp(rest),
        _ => Err("the first argument is probe, tile, caps or warp (see the top of examples/imagery.rs)".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(why) = run(&args) {
        eprintln!("{why}");
        std::process::exit(1);
    }
}
