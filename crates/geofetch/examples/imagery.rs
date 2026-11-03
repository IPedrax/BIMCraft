//! A developer's tool to try the aerial imagery services, which no test does: which tile addresses answer, with what, and what each
//! service says about its terms. It is not part of BIMCraft and is not shipped; what it finds decides how the fetcher is built.
//!
//! ```text
//! cargo run -p bimcraft-geofetch --example imagery -- probe [OUT_DIR]
//! cargo run -p bimcraft-geofetch --example imagery -- tile 'ADDRESS_WITH_{z}_{x}_{y}' LAT LON ZOOM [OUT_DIR]
//! cargo run -p bimcraft-geofetch --example imagery -- caps 'CAPABILITIES_URL' [OUT_DIR]
//! ```
//!
//! `probe` fetches tiles of the four sources BIMCraft means to use, at a place in each, and prints a table; the addresses it tries
//! are guesses from memory (see `CANDIDATES`), and `caps` is how to find the right ones. A tile goes through the same client the
//! app uses (`Ureq` under an `Allow` of exactly that address), so a redirect to another host is refused by name and an address with a
//! query string is refused as the app would. The tiles are saved in OUT_DIR (the temp folder's `bimcraft-imagery-spike`) to be looked at.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bimcraft_core::texture::Picture;
use bimcraft_geofetch::net::{Allow, Net, Ureq};
use bimcraft_geofetch::probe::{Format, expand, metres_per_pixel, scan_capabilities, tile_at};

/// The most a tile may be, bytes, and the most of a capabilities document that is read.
const MAX_TILE: u64 = 2 << 20;
const MAX_CAPS: u64 = 4 << 20;
/// A pause between two requests to one service: a handful of tiles is a light load, and still no hurry.
const PAUSE: Duration = Duration::from_millis(300);

/// A source to try. Everything here is from memory and unchecked, because the sandbox that wrote it cannot reach the services.
struct Candidate {
    name: &'static str,
    /// A tile address with `{z}`, `{x}` and `{y}`; none where there is no guess, and `caps` is how to find it.
    template: Option<&'static str>,
    place: &'static str,
    longitude: f64,
    latitude: f64,
    /// Four zooms to try there, and one finer than the service is expected to go.
    zooms: [u8; 4],
    beyond: u8,
    caps: &'static str,
    /// What the research found about its terms. To verify against the service's own words.
    licence: &'static str,
}

const CANDIDATES: [Candidate; 4] = [
    Candidate {
        name: "flanders",
        template: None,
        place: "Ghent",
        longitude: 3.7174,
        latitude: 51.0543,
        zooms: [16, 17, 18, 19],
        beyond: 22,
        caps: "https://geo.api.vlaanderen.be/OMWRGBMRVL/wmts?request=GetCapabilities&service=WMTS",
        licence: "Flemish open data licence (v1.2 on the 2025 notice, 'modellicentie' on the 2022 one): which applies to which layer?",
    },
    Candidate {
        name: "pdok",
        template: Some("https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/Actueel_orthoHR/EPSG:3857/{z}/{x}/{y}.jpeg"),
        place: "Amsterdam",
        longitude: 4.9041,
        latitude: 52.3676,
        zooms: [16, 17, 18, 19],
        beyond: 22,
        caps: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0?request=GetCapabilities&service=WMTS",
        licence: "CC-BY 4.0 (older years may say 'unknown licence'): what wording does it ask for?",
    },
    Candidate {
        name: "naip",
        template: Some("https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/tile/{z}/{y}/{x}"),
        place: "Washington DC",
        longitude: -77.0369,
        latitude: 38.9072,
        zooms: [14, 16, 17, 18],
        beyond: 21,
        caps: "https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/WMTS/1.0.0/WMTSCapabilities.xml",
        licence: "US public domain; USGS asks for a credit, and this layer mixes NAIP with other imagery: is it all NAIP here?",
    },
    Candidate {
        name: "eox2016",
        template: Some("https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2016_3857/default/g/{z}/{y}/{x}.jpg"),
        place: "Brussels",
        longitude: 4.3517,
        latitude: 50.8503,
        zooms: [10, 12, 13, 14],
        beyond: 17,
        caps: "https://tiles.maps.eox.at/wmts/1.0.0/WMTSCapabilities.xml",
        licence: "CC BY 4.0 for the 2016 layer only; 2018 on is non-commercial share-alike",
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

/// The size of a picture BIMCraft can read, or why it cannot.
fn decoded(bytes: &[u8], format: Format) -> String {
    match format {
        Format::Png | Format::Jpeg => match Picture::import("probe", bytes) {
            Ok(picture) => {
                let (w, h) = picture.size();
                format!("{w}x{h}")
            }
            Err(why) => format!("unreadable: {why}"),
        },
        _ => "-".into(),
    }
}

/// The tile of `template` that `longitude` and `latitude` are on at `zoom`, fetched through the app's own client and saved in `out`.
fn try_tile(source: &str, template: &str, longitude: f64, latitude: f64, zoom: u8, out: &Path, why: &str) -> Row {
    let metres = metres_per_pixel(latitude, zoom);
    let Some((x, y)) = tile_at(longitude, latitude, zoom) else {
        return Row::failed(source, zoom, metres, "no tile there".into(), why);
    };
    let url = match expand(template, zoom, x, y) {
        Ok(url) => url,
        Err(e) => return Row::failed(source, zoom, metres, format!("bad address: {e}"), why),
    };
    let allow = match Allow::default().with_endpoint(&url) {
        Ok(allow) => allow,
        Err(_) => {
            let note = format!(
                "{why} geofetch's client takes no query string, port or credentials: a service that needs one cannot be used as it is"
            );
            return Row::failed(source, zoom, metres, "refused by the allow-list".into(), note.trim());
        }
    };
    let reply = Ureq::new(allow).get(&url, MAX_TILE);
    std::thread::sleep(PAUSE);
    let reply = match reply {
        Ok(reply) => reply,
        Err(e) => return Row::failed(source, zoom, metres, e.message, why),
    };
    let format = Format::sniff(&reply.body);
    let decoded = decoded(&reply.body, format);
    let mut notes = vec![why.to_string()];
    let content_type = reply.content_type.clone().unwrap_or_default();
    match format {
        Format::Text => notes.push("a text reply, not a picture: see the saved file".into()),
        Format::Webp => notes.push("WebP: BIMCraft cannot read it yet".into()),
        Format::Png | Format::Jpeg if reply.body.len() < 1500 => notes.push("very small: blank?".into()),
        _ => {}
    }
    if content_type.starts_with("image/") && format == Format::Text {
        notes.push("says it is an image".into());
    }
    let file = out.join(format!("{source}-z{zoom}-{x}-{y}.{}", format.extension()));
    if let Err(e) = std::fs::write(&file, &reply.body) {
        notes.push(format!("not saved: {e}"));
    }
    let note = notes.into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join("; ");
    Row {
        result: "ok".into(),
        content_type,
        format: format.name(),
        bytes: reply.body.len(),
        decoded,
        note,
        ..Row::new(source, zoom, metres)
    }
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
        let Some(template) = c.template else {
            rows.push(Row::failed(c.name, c.zooms[0], None, "no address guessed".into(), "run the caps command below"));
            continue;
        };
        for zoom in c.zooms {
            rows.push(try_tile(c.name, template, c.longitude, c.latitude, zoom, &out, c.place));
        }
        rows.push(try_tile(c.name, template, -30.0, 0.0, c.zooms[1], &out, "far outside any coverage (mid-Atlantic)"));
        rows.push(try_tile(c.name, template, c.longitude, c.latitude, c.beyond, &out, "finer than the service is expected to go"));
    }
    println!();
    print_table(&rows);
    println!("\nThe tiles are in {}. Terms, as the research found them (verify at the service):", out.display());
    for c in &CANDIDATES {
        println!("- {}: {}", c.name, c.licence);
    }
    println!("\nThe addresses above are guesses. Each service's own capabilities document gives the right ones, and its terms:");
    for c in &CANDIDATES {
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

/// The host of an address, for a file name.
fn host_of(url: &str) -> String {
    let host = url.trim_start_matches("https://").split(['/', '?']).next().unwrap_or("service");
    host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

fn caps(args: &[String]) -> Result<(), String> {
    let url = args.first().ok_or("the address of a capabilities document is wanted")?;
    if !url.starts_with("https://") || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("an https:// address without spaces is wanted".into());
    }
    let out = out_dir(args.get(1))?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .http_status_as_error(false)
        .user_agent(format!("BIMCraft-imagery-spike/{} (+https://github.com/IPedrax/BIMCraft)", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let response = agent
        .get(url.as_str())
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .call()
        .map_err(|e| format!("{}: {e}", host_of(url)))?;
    let status = response.status().as_u16();
    let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let mut body = Vec::new();
    response
        .into_body()
        .into_with_config()
        .limit(MAX_CAPS + 2)
        .reader()
        .take(MAX_CAPS)
        .read_to_end(&mut body)
        .map_err(|e| format!("{}: {e}", host_of(url)))?;
    let file = out.join(format!("caps-{}.xml", host_of(url)));
    let saved = std::fs::write(&file, &body).map_or_else(|e| format!("not saved: {e}"), |()| format!("saved to {}", file.display()));
    println!("HTTP {status}, {content_type}, {} bytes ({}), {saved}", body.len(), Format::sniff(&body).name());
    let text = String::from_utf8_lossy(&body);
    let c = scan_capabilities(&text);
    let list = |name: &str, items: &[String], show: usize| {
        let more = if items.len() > show { format!(" ... ({} in all)", items.len()) } else { String::new() };
        println!("{name}: {}{more}", items.iter().take(show).cloned().collect::<Vec<_>>().join(", "));
    };
    println!("title: {}", c.title.as_deref().unwrap_or("-"));
    list("fees", &c.fees, 5);
    list("access constraints", &c.constraints, 5);
    list("layers", &c.layers, 12);
    list("tile matrix sets", &c.matrix_sets, 12);
    list("tile matrices", &c.matrices, 3);
    println!("tile sizes: {}", c.tile_sizes.iter().map(u32::to_string).collect::<Vec<_>>().join(", "));
    println!("tile addresses ({}):", c.resource_urls.len());
    for u in c.resource_urls.iter().take(20) {
        println!("  {u}");
    }
    if c.layers.is_empty() && c.resource_urls.is_empty() {
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

fn run(args: &[String]) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("probe") => probe(rest),
        Some("tile") => tile(rest),
        Some("caps") => caps(rest),
        _ => Err("the first argument is probe, tile or caps (see the top of examples/imagery.rs)".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(why) = run(&args) {
        eprintln!("{why}");
        std::process::exit(1);
    }
}
