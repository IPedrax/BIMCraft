//! The pure parts of trying an imagery service: a tile address from a template, what a reply is by its first bytes, the size of a
//! pixel on the ground, the tile a place is on, and what a WMTS capabilities document says (its layers, its tile sizes, the
//! addresses it gives for tiles, its fees and constraints). The developer's `imagery` example is a thin shell over these; none of
//! it touches the network, and what a document says is hostile, so it is read to a cap and never indexed blindly.

use bimcraft_core::geo::tile_xy;
// The address of a tile from a template is the core's, because its table of sources is made of such templates.
pub use bimcraft_core::imagery::{MAX_TEMPLATE, MAX_ZOOM, expand};

/// The most of a capabilities document that is read, bytes.
pub const MAX_CAPABILITIES: usize = 4 << 20;
/// The most of each kind a document gives back, and the longest text it gives.
pub const MAX_ITEMS: usize = 200;
pub const MAX_TEXT: usize = 2048;
/// The most tags and texts read from a document.
const MAX_TOKENS: usize = 400_000;
/// Web Mercator reaches this far north and south, degrees.
const MAX_LATITUDE: f64 = 85.0511;
/// The ground a pixel of a 256-pixel tile at zoom 0 stands for at the equator, metres.
const METRES_AT_ZOOM_0: f64 = 156_543.033_928_040_97;

/// What a reply is, told by its first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Empty,
    Png,
    Jpeg,
    Webp,
    /// XML, HTML or JSON: what a service sends instead of a tile when it refuses.
    Text,
    Other,
}

impl Format {
    /// What `bytes` are. A service's `Content-Type` is not asked: an error page is often sent as the picture it was asked for.
    pub fn sniff(bytes: &[u8]) -> Format {
        match bytes {
            [] => Format::Empty,
            [0x89, b'P', b'N', b'G', ..] => Format::Png,
            [0xFF, 0xD8, 0xFF, ..] => Format::Jpeg,
            [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Format::Webp,
            [b'G', b'I', b'F', b'8', ..] => Format::Other,
            _ if bytes.iter().take(64).all(|b| b.is_ascii_graphic() || b.is_ascii_whitespace()) => Format::Text,
            _ => Format::Other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Empty => "empty",
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Webp => "WebP",
            Format::Text => "text",
            Format::Other => "other",
        }
    }

    /// The extension a reply of this kind is saved under.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Webp => "webp",
            Format::Text => "txt",
            Format::Empty | Format::Other => "bin",
        }
    }
}

/// The ground a pixel of a 256-pixel tile stands for at `latitude` and `zoom`, metres.
pub fn metres_per_pixel(latitude: f64, zoom: u8) -> Option<f64> {
    if !latitude.is_finite() || latitude.abs() > MAX_LATITUDE || zoom > MAX_ZOOM {
        return None;
    }
    Some(METRES_AT_ZOOM_0 * latitude.to_radians().cos() / f64::from(1u32 << zoom))
}

/// The column and row of the tile `longitude` and `latitude` are on at `zoom`, if the map has a tile there.
pub fn tile_at(longitude: f64, latitude: f64, zoom: u8) -> Option<(u32, u32)> {
    if !longitude.is_finite() || !latitude.is_finite() || longitude.abs() > 180.0 || latitude.abs() > MAX_LATITUDE || zoom > MAX_ZOOM {
        return None;
    }
    let (x, y) = tile_xy(longitude, latitude, zoom);
    let n = f64::from(1u32 << zoom);
    if !(0.0..n).contains(&x) || !(0.0..n).contains(&y) {
        return None;
    }
    Some((x.floor() as u32, y.floor() as u32))
}

/// A layer of a capabilities document. Its `description` is the document's abstract, where a service may put restrictions that
/// apply to this layer alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    pub name: String,
    pub description: String,
    pub styles: Vec<String>,
    pub formats: Vec<String>,
    /// The tile matrix sets it is offered in.
    pub sets: Vec<String>,
}

/// A level of a tile matrix set, as the document writes it: `scale` and `top_left` are kept as text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Matrix {
    pub id: String,
    pub scale: String,
    pub top_left: String,
    pub columns: u32,
    pub rows: u32,
}

/// A tile matrix set: its coordinate system and its levels, in the document's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatrixSet {
    pub name: String,
    pub crs: String,
    pub matrices: Vec<Matrix>,
}

/// What Web Mercator's first level is: one tile, whose top left corner is the corner of the map, at the scale of 256-pixel tiles.
const MERCATOR_EDGE: f64 = 20_037_508.342_789_244;
const MERCATOR_SCALE: f64 = 559_082_264.028_717_8;

impl MatrixSet {
    /// Whether this is the ordinary Web Mercator pyramid that OpenStreetMap's tiles are numbered by (so a tile `z/x/y` is the same
    /// place), or in what way it is not.
    pub fn web_mercator(&self) -> Result<(), String> {
        let first = self.matrices.first().ok_or("it has no levels")?;
        if (first.columns, first.rows) != (1, 1) {
            return Err(format!("its first level is {} x {} tiles, not 1 x 1", first.columns, first.rows));
        }
        match first.scale.trim().parse::<f64>() {
            Ok(scale) if scale.is_finite() && (scale / MERCATOR_SCALE - 1.0).abs() < 1e-6 => {}
            _ => {
                return Err(format!(
                    "its first level has scale denominator `{}`, where Web Mercator's is {MERCATOR_SCALE:.2}",
                    first.scale
                ));
            }
        }
        let corner: Vec<f64> = first.top_left.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        match corner.as_slice() {
            [x, y] if (x + MERCATOR_EDGE).abs() < 1.0 && (y - MERCATOR_EDGE).abs() < 1.0 => Ok(()),
            [x, y] if (x - MERCATOR_EDGE).abs() < 1.0 && (y + MERCATOR_EDGE).abs() < 1.0 => {
                Err("its top left corner is written with the axis order swapped (north first)".into())
            }
            _ => Err(format!("its top left corner is `{}`, where Web Mercator's is -20037508.34 20037508.34", first.top_left)),
        }
    }
}

/// The most characters a value in a tile address's query may have, and the characters it may have. The same rule is the network
/// layer's for a service that can only be asked by query.
pub const QUERY_VALUE_MAX: usize = 48;

/// Whether `value` is one the query of a tile address may carry.
pub fn query_value_ok(value: &str) -> bool {
    (1..=QUERY_VALUE_MAX).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'-'))
}

/// The address of a tile of a WMTS service that has no tile addresses of its own and is asked by query: `endpoint` (without a
/// query), then the layer, style (empty for a layer that lists none), tile matrix set and format, and `{z}`, `{y}` and `{x}` to be put
/// in by [`expand`].
pub fn kvp_address(endpoint: &str, layer: &str, style: &str, set: &str, format: &str) -> Result<String, String> {
    if !endpoint.starts_with("https://") || endpoint.contains(['?', '#']) || endpoint.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!("the endpoint is an https:// address without a query, a fragment or spaces, not `{endpoint}`"));
    }
    // A layer that lists no style is asked with an empty one (`style=`), which is what the services of this kind take; only the style may be empty.
    for (what, value) in [("layer", layer), ("style", style), ("tile matrix set", set), ("format", format)] {
        if !(query_value_ok(value) || (what == "style" && value.is_empty())) {
            return Err(format!("the {what} `{value}` is not 1 to {QUERY_VALUE_MAX} letters, digits and . _ : / -"));
        }
    }
    Ok(format!(
        "{endpoint}?service=WMTS&request=GetTile&version=1.0.0&layer={layer}&style={style}&tilematrixset={set}&tilematrix={{z}}&tilerow={{y}}&tilecol={{x}}&format={format}"
    ))
}

/// `rgba` (a picture of `width` by `height` pixels covering `extent`, west, south, east, north in the project's plane, mm) with a line
/// on every pixel that a multiple of `every_m` metres, in x or in y, passes through: a grid to check a picture against a map.
pub fn grid_overlay(rgba: &[u8], width: u32, height: u32, extent: [f64; 4], every_m: f64) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let [west, south, east, north] = extent;
    let sound = extent.iter().all(|v| v.is_finite())
        && west < east
        && south < north
        && every_m.is_finite()
        && every_m > 0.0
        && w.checked_mul(h).and_then(|n| n.checked_mul(4)).is_some_and(|n| n > 0 && n == rgba.len());
    let mut out = rgba.to_vec();
    if !sound {
        return out;
    }
    // The metres a pixel is across each way, and where the picture's west and north edges are.
    let (px, py) = ((east - west) / 1000.0 / w as f64, (north - south) / 1000.0 / h as f64);
    let (west_m, north_m) = (west / 1000.0, north / 1000.0);
    let mut paint = |x: usize, y: usize| {
        if let Some(pixel) = out.get_mut((y * w + x) * 4..(y * w + x) * 4 + 4) {
            pixel.copy_from_slice(&[255, 0, 255, 255]);
        }
    };
    for x in 0..w {
        let (a, b) = ((west_m + x as f64 * px) / every_m, (west_m + (x + 1) as f64 * px) / every_m);
        if a.floor() != b.floor() {
            (0..h).for_each(|y| paint(x, y));
        }
    }
    for y in 0..h {
        let (a, b) = ((north_m - y as f64 * py) / every_m, (north_m - (y + 1) as f64 * py) / every_m);
        if a.floor() != b.floor() {
            (0..w).for_each(|x| paint(x, y));
        }
    }
    out
}

/// `rgba` (`width` by `height` pixels) as the bytes of a PNG file.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let wanted = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || wanted != Some(rgba.len()) {
        return Err(format!("a {width} x {height} picture is {wanted:?} bytes of RGBA, not {}", rgba.len()));
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// What a WMTS capabilities document says that matters for fetching from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capabilities {
    pub title: Option<String>,
    pub fees: Vec<String>,
    pub constraints: Vec<String>,
    pub layers: Vec<Layer>,
    pub matrix_sets: Vec<MatrixSet>,
    pub tile_sizes: Vec<u32>,
    pub resource_urls: Vec<String>,
}

enum Token<'a> {
    Start { name: &'a str, attrs: &'a str, empty: bool },
    End { name: &'a str },
    Text(&'a str),
}

fn after(s: &str, from: usize) -> &str {
    s.get(from..).unwrap_or_default()
}

fn between(s: &str, from: usize, to: usize) -> &str {
    s.get(from..to).unwrap_or_default()
}

/// A tag's name without the prefix of its namespace: `ows:Identifier` is `Identifier`.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Where the tag that `s` starts with ends: the `>` that is not inside a quote. None if it never does.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            (None, _) => {}
        }
    }
    None
}

/// The tags and texts of `xml`, in order, at most [`MAX_TOKENS`]. Comments, declarations and what is not closed are left out, and
/// every step moves forward, so a document of any shape is read once.
fn tokens(xml: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = xml;
    while !rest.is_empty() && out.len() < MAX_TOKENS {
        let Some(open) = rest.find('<') else {
            out.push(Token::Text(rest));
            break;
        };
        if open > 0 {
            out.push(Token::Text(between(rest, 0, open)));
            rest = after(rest, open);
            continue;
        }
        // A comment, a declaration or a doctype: skipped, and if it never ends, so is the rest.
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = if rest.starts_with("<!--") { rest.find("-->").map(|i| i + 3) } else { rest.find('>').map(|i| i + 1) };
            let Some(end) = end else { break };
            rest = after(rest, end);
            continue;
        }
        let Some(end) = tag_end(rest) else { break };
        let inside = between(rest, 1, end);
        rest = after(rest, end + 1);
        if let Some(name) = inside.strip_prefix('/') {
            out.push(Token::End { name: local(name.trim()) });
        } else {
            let (empty, inside) = match inside.strip_suffix('/') {
                Some(i) => (true, i),
                None => (false, inside),
            };
            let (name, attrs) = inside.split_once(char::is_whitespace).unwrap_or((inside, ""));
            out.push(Token::Start { name: local(name), attrs, empty });
        }
    }
    out
}

/// The value of attribute `name` in the text of a tag, as written (not yet unescaped).
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = after(attrs, from).find(name) {
        let at = from + i;
        let alone = at == 0 || attrs.get(..at).is_some_and(|before| before.ends_with(char::is_whitespace));
        if alone && let Some(rest) = after(attrs, at + name.len()).trim_start().strip_prefix('=') {
            let rest = rest.trim_start();
            let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let value = rest.get(1..)?;
            return Some(value.get(..value.find(quote)?)?.to_string());
        }
        from = at + name.len();
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// `s` unescaped and cut to [`MAX_TEXT`] bytes, without cutting a character in two.
fn clip(s: &str) -> String {
    let s = unescape(s);
    let mut end = s.len().min(MAX_TEXT);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default().to_string()
}

/// Adds `text` to `list` if there is room, and, when `once`, if it is not there already.
fn keep(list: &mut Vec<String>, text: &str, once: bool) {
    if text.is_empty() || list.len() >= MAX_ITEMS {
        return;
    }
    let text = clip(text);
    if !(once && list.contains(&text)) {
        list.push(text);
    }
}

/// What `xml` says, read by looking for tags and nothing more: it is not parsed, so a document that is not well formed gives what
/// can be read of it. Only the first [`MAX_CAPABILITIES`] bytes are read, and each kind of thing is kept to [`MAX_ITEMS`].
pub fn scan_capabilities(xml: &str) -> Capabilities {
    let mut end = xml.len().min(MAX_CAPABILITIES);
    while !xml.is_char_boundary(end) {
        end -= 1;
    }
    let mut caps = Capabilities::default();
    let mut stack: Vec<&str> = Vec::new();
    // Which layer, which matrix set and which of its levels the tags being read are inside.
    let (mut in_layer, mut in_set, mut in_matrix): (Option<usize>, Option<usize>, Option<usize>) = (None, None, None);
    for token in tokens(xml.get(..end).unwrap_or_default()) {
        match token {
            Token::Start { name, attrs, empty } => {
                if name == "ResourceURL"
                    && let Some(template) = attribute(attrs, "template")
                {
                    let format = attribute(attrs, "format").unwrap_or_default();
                    keep(&mut caps.resource_urls, format!("{format} {template}").trim(), false);
                }
                if !empty {
                    match name {
                        "Layer" => {
                            in_layer = (caps.layers.len() < MAX_ITEMS).then_some(caps.layers.len());
                            if in_layer.is_some() {
                                caps.layers.push(Layer::default());
                            }
                        }
                        "TileMatrix" => {
                            in_matrix = in_set.and_then(|s| caps.matrix_sets.get_mut(s)).and_then(|set| {
                                (set.matrices.len() < MAX_ITEMS).then(|| {
                                    set.matrices.push(Matrix::default());
                                    set.matrices.len() - 1
                                })
                            });
                        }
                        _ => {}
                    }
                    stack.push(name);
                }
            }
            Token::End { name } => {
                if stack.last() == Some(&name) {
                    stack.pop();
                    match name {
                        "Layer" => in_layer = None,
                        "TileMatrix" => in_matrix = None,
                        "TileMatrixSet" => (in_set, in_matrix) = (None, None),
                        _ => {}
                    }
                }
            }
            Token::Text(text) => {
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let mut up = stack.iter().rev().copied();
                match (up.next(), up.next()) {
                    (Some("Title"), _) if caps.title.is_none() => caps.title = Some(clip(text)),
                    (Some("Fees"), _) => keep(&mut caps.fees, text, false),
                    (Some("AccessConstraints"), _) => keep(&mut caps.constraints, text, false),
                    (Some("Identifier"), Some("Layer")) => {
                        if let Some(layer) = in_layer.and_then(|i| caps.layers.get_mut(i)) {
                            layer.name = clip(text);
                        }
                    }
                    (Some("Abstract"), Some("Layer")) => {
                        if let Some(layer) = in_layer.and_then(|i| caps.layers.get_mut(i)) {
                            layer.description = clip(text);
                        }
                    }
                    (Some("Identifier"), Some("Style")) => {
                        if let Some(layer) = in_layer.and_then(|i| caps.layers.get_mut(i)) {
                            keep(&mut layer.styles, text, true);
                        }
                    }
                    (Some("Format"), Some("Layer")) => {
                        if let Some(layer) = in_layer.and_then(|i| caps.layers.get_mut(i)) {
                            keep(&mut layer.formats, text, true);
                        }
                    }
                    (Some("TileMatrixSet"), Some("TileMatrixSetLink")) => {
                        if let Some(layer) = in_layer.and_then(|i| caps.layers.get_mut(i)) {
                            keep(&mut layer.sets, text, true);
                        }
                    }
                    (Some("Identifier"), Some("TileMatrixSet")) => {
                        in_set = (caps.matrix_sets.len() < MAX_ITEMS).then_some(caps.matrix_sets.len());
                        if in_set.is_some() {
                            caps.matrix_sets.push(MatrixSet { name: clip(text), ..MatrixSet::default() });
                        }
                    }
                    (Some("SupportedCRS"), Some("TileMatrixSet")) => {
                        if let Some(set) = in_set.and_then(|i| caps.matrix_sets.get_mut(i)) {
                            set.crs = clip(text);
                        }
                    }
                    (
                        Some(field @ ("Identifier" | "ScaleDenominator" | "TopLeftCorner" | "MatrixWidth" | "MatrixHeight")),
                        Some("TileMatrix"),
                    ) => {
                        if let Some(matrix) =
                            in_set.and_then(|s| caps.matrix_sets.get_mut(s)).and_then(|set| in_matrix.and_then(|m| set.matrices.get_mut(m)))
                        {
                            match field {
                                "Identifier" => matrix.id = clip(text),
                                "ScaleDenominator" => matrix.scale = clip(text),
                                "TopLeftCorner" => matrix.top_left = clip(text),
                                "MatrixWidth" => matrix.columns = text.parse().unwrap_or(0),
                                _ => matrix.rows = text.parse().unwrap_or(0),
                            }
                        }
                    }
                    (Some("TileWidth"), _) => {
                        if let Ok(width) = text.parse::<u32>()
                            && caps.tile_sizes.len() < MAX_ITEMS
                            && !caps.tile_sizes.contains(&width)
                        {
                            caps.tile_sizes.push(width);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_gets_its_zoom_column_and_row() {
        assert_eq!(expand("https://t.example/{z}/{x}/{y}.jpg", 17, 67146, 43905).unwrap(), "https://t.example/17/67146/43905.jpg");
        assert_eq!(expand("https://t.example/{zoom}/{y}/{x}", 3, 5, 1).unwrap(), "https://t.example/3/1/5");
        // WMTS's own names, as a capabilities document's ResourceURL gives them.
        assert_eq!(expand("https://t.example/{TileMatrix}/{TileCol}/{TileRow}.png", 3, 5, 1).unwrap(), "https://t.example/3/5/1.png");
        // Each as often as it is written.
        assert_eq!(expand("/{z}{z}/{x}-{x}/{y}", 2, 3, 0).unwrap(), "/22/3-3/0");
        // Text around a zoom, as GeoServer names its matrices.
        assert_eq!(expand("https://t.example/EPSG:3857:{z}/{x}/{y}", 7, 1, 2).unwrap(), "https://t.example/EPSG:3857:7/1/2");
    }

    #[test]
    fn minus_y_counts_the_rows_from_the_south() {
        assert_eq!(expand("{z}/{x}/{-y}", 3, 0, 1).unwrap(), "3/0/6");
        assert_eq!(expand("{z}/{x}/{-y}", 3, 0, 7).unwrap(), "3/0/0");
        assert_eq!(expand("{z}/{x}/{-y}", 0, 0, 0).unwrap(), "0/0/0");
    }

    #[test]
    fn what_is_not_a_template_or_not_a_tile_is_refused_in_words() {
        for (what, template) in [
            ("no zoom", "https://t.example/{x}/{y}"),
            ("no column", "https://t.example/{z}/{y}"),
            ("no row", "https://t.example/{z}/{x}"),
            ("a name nobody knows", "https://t.example/{z}/{x}/{y}/{style}"),
            ("a nested brace", "https://t.example/{z}/{{x}}/{y}"),
            ("unclosed", "https://t.example/{z}/{x}/{y"),
            ("a stray close", "https://t.example/{z}/{x}/{y}}"),
            ("empty", ""),
        ] {
            assert!(expand(template, 3, 1, 1).is_err(), "{what}");
        }
        let why = expand("https://t.example/{z}/{x}/{y}/{style}", 3, 1, 1).unwrap_err();
        assert!(why.contains("{style}"), "it names what it does not know: {why}");
        let long = format!("https://t.example/{{z}}/{{x}}/{{y}}/{}", "a".repeat(MAX_TEMPLATE));
        assert!(expand(&long, 3, 1, 1).is_err());
        // The map has 2^zoom columns and rows.
        assert!(expand("{z}/{x}/{y}", 3, 8, 0).is_err());
        assert!(expand("{z}/{x}/{y}", 3, 0, 8).is_err());
        assert!(expand("{z}/{x}/{y}", 3, 7, 7).is_ok());
        assert!(expand("{z}/{x}/{y}", MAX_ZOOM + 1, 0, 0).is_err());
        assert!(expand("{z}/{x}/{y}", MAX_ZOOM, (1 << MAX_ZOOM) - 1, 0).is_ok());
        assert!(expand("{z}/{x}/{y}", 255, 0, 0).is_err());
        assert!(expand("{z}/{x}/{y}", 3, u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn a_reply_is_told_by_its_first_bytes_and_never_by_what_it_says_it_is() {
        assert_eq!(Format::sniff(&[]), Format::Empty);
        assert_eq!(Format::sniff(b"\x89PNG\r\n\x1a\n...."), Format::Png);
        assert_eq!(Format::sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 16]), Format::Jpeg);
        assert_eq!(Format::sniff(b"RIFF\x10\0\0\0WEBPVP8 "), Format::Webp);
        assert_eq!(Format::sniff(b"<?xml version=\"1.0\"?><ServiceException>"), Format::Text);
        assert_eq!(Format::sniff(b"<html>429 Too Many Requests</html>"), Format::Text);
        assert_eq!(Format::sniff(br#"{"error":{"code":400}}"#), Format::Text);
        assert_eq!(Format::sniff(b"GIF89a"), Format::Other);
        assert_eq!(Format::sniff(&[0x49, 0x49, 0x2A, 0x00, 8, 0]), Format::Other, "a TIFF");
        // Too short to be a picture, and not text.
        assert_eq!(Format::sniff(&[0x89, b'P']), Format::Other);
        assert_eq!(Format::sniff(&[0xFF, 0xD8]), Format::Other);
        // Each has a name and a file extension to be saved under.
        for (format, name, extension) in [
            (Format::Png, "PNG", "png"),
            (Format::Jpeg, "JPEG", "jpg"),
            (Format::Webp, "WebP", "webp"),
            (Format::Text, "text", "txt"),
            (Format::Other, "other", "bin"),
            (Format::Empty, "empty", "bin"),
        ] {
            assert_eq!((format.name(), format.extension()), (name, extension));
        }
    }

    #[test]
    fn a_pixel_is_so_many_metres_at_a_latitude_and_zoom() {
        let near = |got: Option<f64>, want: f64| got.is_some_and(|g| (g - want).abs() < 0.005 * want);
        assert!(near(metres_per_pixel(0.0, 0), METRES_AT_ZOOM_0));
        assert!(near(metres_per_pixel(51.0, 17), 0.7516), "51 N, zoom 17: {:?}", metres_per_pixel(51.0, 17));
        assert!(near(metres_per_pixel(-23.0, 17), 1.0994), "23 S, zoom 17: {:?}", metres_per_pixel(-23.0, 17));
        // Each zoom halves it.
        let (a, b) = (metres_per_pixel(40.0, 10), metres_per_pixel(40.0, 11));
        assert!(a.zip(b).is_some_and(|(a, b)| (a / b - 2.0).abs() < 1e-9));
        for (latitude, zoom) in [(86.0, 5), (-90.0, 5), (f64::NAN, 5), (f64::INFINITY, 5), (0.0, MAX_ZOOM + 1), (0.0, 255)] {
            assert_eq!(metres_per_pixel(latitude, zoom), None, "{latitude} {zoom}");
        }
    }

    #[test]
    fn a_place_is_on_one_tile() {
        // Zoom 1 is two by two tiles, split at the prime meridian and the equator.
        assert_eq!(tile_at(0.0, 0.0, 1), Some((1, 1)));
        assert_eq!(tile_at(-0.001, 0.001, 1), Some((0, 0)));
        assert_eq!(tile_at(-180.0, 85.0, 0), Some((0, 0)));
        // North has the smaller row, east the larger column.
        let (south, north) = (tile_at(4.0, 50.0, 12), tile_at(4.0, 51.0, 12));
        assert!(south.zip(north).is_some_and(|(s, n)| n.1 < s.1 && n.0 == s.0));
        let (west, east) = (tile_at(3.0, 51.0, 12), tile_at(4.0, 51.0, 12));
        assert!(west.zip(east).is_some_and(|(w, e)| e.0 > w.0 && e.1 == w.1));
        // It agrees with the address of the tile it names being inside the map.
        assert!(tile_at(179.999, 0.0, 3).is_some_and(|(x, _)| x == 7));
        for (lon, lat, zoom) in [(180.0, 0.0, 3), (181.0, 0.0, 3), (0.0, 89.0, 3), (0.0, -89.0, 3), (f64::NAN, 0.0, 3), (0.0, f64::NAN, 3)]
        {
            assert_eq!(tile_at(lon, lat, zoom), None, "{lon} {lat} {zoom}");
        }
        assert_eq!(tile_at(0.0, 0.0, MAX_ZOOM + 1), None);
    }

    const WMTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Capabilities xmlns="http://www.opengis.net/wmts/1.0" xmlns:ows="http://www.opengis.net/ows/1.1" version="1.0.0">
 <ows:ServiceIdentification>
  <ows:Title>Luchtfoto</ows:Title>
  <ows:Fees>none</ows:Fees>
  <ows:AccessConstraints>CC-BY 4.0</ows:AccessConstraints>
 </ows:ServiceIdentification>
 <Contents>
  <Layer>
   <ows:Title>Actueel</ows:Title>
   <ows:Abstract>Aerial photographs, 7.5 cm</ows:Abstract>
   <ows:Identifier>Actueel_orthoHR</ows:Identifier>
   <Style isDefault="true"><ows:Identifier>default</ows:Identifier></Style>
   <Format>image/jpeg</Format>
   <TileMatrixSetLink><TileMatrixSet>EPSG:3857</TileMatrixSet></TileMatrixSetLink>
   <ResourceURL format="image/jpeg" resourceType="tile" template="https://example.org/wmts/Actueel_orthoHR/EPSG:3857/{TileMatrix}/{TileCol}/{TileRow}.jpeg"/>
  </Layer>
  <TileMatrixSet>
   <ows:Identifier>EPSG:3857</ows:Identifier>
   <ows:SupportedCRS>urn:ogc:def:crs:EPSG::3857</ows:SupportedCRS>
   <TileMatrix><ows:Identifier>0</ows:Identifier><ScaleDenominator>559082264.0287178</ScaleDenominator><TopLeftCorner>-20037508.342789244 20037508.342789244</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>1</MatrixWidth><MatrixHeight>1</MatrixHeight></TileMatrix>
   <TileMatrix><ows:Identifier>1</ows:Identifier><ScaleDenominator>279541132.0143589</ScaleDenominator><TopLeftCorner>-20037508.342789244 20037508.342789244</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>2</MatrixWidth><MatrixHeight>2</MatrixHeight></TileMatrix>
  </TileMatrixSet>
 </Contents>
</Capabilities>"#;

    #[test]
    fn a_capabilities_document_gives_its_layers_sizes_addresses_and_terms() {
        let caps = scan_capabilities(WMTS);
        assert_eq!(caps.title.as_deref(), Some("Luchtfoto"), "the service's, not a layer's");
        assert_eq!(caps.fees, ["none"]);
        assert_eq!(caps.constraints, ["CC-BY 4.0"]);
        assert_eq!(caps.layers.len(), 1);
        let layer = &caps.layers[0];
        assert_eq!(layer.name, "Actueel_orthoHR", "the layer's name, not the set it links to");
        assert_eq!(layer.description, "Aerial photographs, 7.5 cm", "the layer's abstract, not the service's");
        assert_eq!(
            (layer.styles.as_slice(), layer.formats.as_slice(), layer.sets.as_slice()),
            (&["default".to_string()][..], &["image/jpeg".to_string()][..], &["EPSG:3857".to_string()][..])
        );
        assert_eq!(caps.matrix_sets.len(), 1, "the set that is defined, once, not the link to it");
        let set = &caps.matrix_sets[0];
        assert_eq!((set.name.as_str(), set.crs.as_str()), ("EPSG:3857", "urn:ogc:def:crs:EPSG::3857"));
        assert_eq!(set.matrices.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["0", "1"]);
        assert_eq!(set.matrices.iter().map(|m| (m.columns, m.rows)).collect::<Vec<_>>(), [(1, 1), (2, 2)]);
        assert_eq!(set.matrices[0].top_left, "-20037508.342789244 20037508.342789244");
        assert_eq!(set.matrices[1].scale, "279541132.0143589");
        assert_eq!(set.web_mercator(), Ok(()));
        assert_eq!(caps.tile_sizes, [256], "each size once");
        assert_eq!(
            caps.resource_urls,
            ["image/jpeg https://example.org/wmts/Actueel_orthoHR/EPSG:3857/{TileMatrix}/{TileCol}/{TileRow}.jpeg"]
        );
    }

    #[test]
    fn a_document_written_with_prefixes_and_escapes_reads_the_same() {
        let xml = r#"<wmts:Capabilities xmlns:wmts="w" xmlns:ows="o">
          <wmts:Contents><wmts:Layer><ows:Identifier>ortho</ows:Identifier>
          <wmts:ResourceURL format='image/png' template='https://example.org/t?layer=a&amp;z={TileMatrix}'/></wmts:Layer></wmts:Contents>
          <ows:Fees>free &amp; open</ows:Fees><!-- <ows:Fees>not this</ows:Fees> --></wmts:Capabilities>"#;
        let caps = scan_capabilities(xml);
        assert_eq!(caps.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["ortho"]);
        assert_eq!(caps.fees, ["free & open"], "entities undone, comments skipped");
        assert_eq!(caps.resource_urls, ["image/png https://example.org/t?layer=a&z={TileMatrix}"]);
        // An attribute whose name ends the same is not the one asked for.
        let other =
            r#"<ResourceURL format="image/png" subtemplate="https://wrong.example/" template="https://right.example/{TileMatrix}"/>"#;
        assert_eq!(scan_capabilities(other).resource_urls, ["image/png https://right.example/{TileMatrix}"]);
    }

    #[test]
    fn each_layer_keeps_its_own_description_styles_formats_and_sets() {
        // The abstract may come before the name, and the next layer's is not this one's.
        let xml = r#"<Contents>
          <Layer><ows:Abstract>Free: CC BY 4.0</ows:Abstract><ows:Identifier>a</ows:Identifier>
            <Style><ows:Identifier>s1</ows:Identifier></Style><Style><ows:Identifier>s2</ows:Identifier></Style>
            <Format>image/png</Format><Format>image/jpeg</Format>
            <TileMatrixSetLink><TileMatrixSet>g</TileMatrixSet></TileMatrixSetLink>
            <TileMatrixSetLink><TileMatrixSet>w</TileMatrixSet></TileMatrixSetLink></Layer>
          <Layer><ows:Identifier>b</ows:Identifier><ows:Abstract>Non-commercial &amp; share-alike</ows:Abstract></Layer>
          </Contents>"#;
        let caps = scan_capabilities(xml);
        let [a, b] = caps.layers.as_slice() else { panic!("two layers wanted: {:?}", caps.layers) };
        assert_eq!((a.name.as_str(), a.description.as_str()), ("a", "Free: CC BY 4.0"));
        assert_eq!(
            (&a.styles, &a.formats, &a.sets),
            (
                &["s1", "s2"].map(String::from).to_vec(),
                &["image/png", "image/jpeg"].map(String::from).to_vec(),
                &["g", "w"].map(String::from).to_vec()
            )
        );
        assert_eq!((b.name.as_str(), b.description.as_str()), ("b", "Non-commercial & share-alike"));
        assert!(b.styles.is_empty() && b.formats.is_empty() && b.sets.is_empty());
        // A service's own abstract is not a layer's.
        let own =
            scan_capabilities("<ows:ServiceIdentification><ows:Abstract>about the service</ows:Abstract></ows:ServiceIdentification>");
        assert!(own.layers.is_empty());
    }

    #[test]
    fn matrices_are_listed_under_the_set_they_belong_to() {
        let xml = r#"<Contents>
          <TileMatrixSet><ows:Identifier>RD</ows:Identifier>
            <TileMatrix><ows:Identifier>00</ows:Identifier><MatrixWidth>1</MatrixWidth><MatrixHeight>1</MatrixHeight></TileMatrix>
            <TileMatrix><ows:Identifier>01</ows:Identifier></TileMatrix></TileMatrixSet>
          <TileMatrixSet><ows:Identifier>EPSG:3857</ows:Identifier>
            <TileMatrix><ows:Identifier>16</ows:Identifier></TileMatrix></TileMatrixSet></Contents>"#;
        let caps = scan_capabilities(xml);
        let names: Vec<(&str, Vec<&str>)> =
            caps.matrix_sets.iter().map(|s| (s.name.as_str(), s.matrices.iter().map(|m| m.id.as_str()).collect())).collect();
        assert_eq!(names, [("RD", vec!["00", "01"]), ("EPSG:3857", vec!["16"])]);
        assert_eq!(caps.matrix_sets[0].matrices[1].columns, 0, "what the document leaves out is nothing");
    }

    fn matrix(scale: &str, corner: &str, columns: u32, rows: u32) -> Matrix {
        Matrix { id: "0".into(), scale: scale.into(), top_left: corner.into(), columns, rows }
    }

    fn set_of(first: Matrix) -> MatrixSet {
        MatrixSet { name: "s".into(), crs: String::new(), matrices: vec![first] }
    }

    #[test]
    fn a_set_is_web_mercator_or_says_how_it_is_not() {
        let good = matrix("559082264.0287178", "-20037508.342789244 20037508.342789244", 1, 1);
        assert_eq!(set_of(good.clone()).web_mercator(), Ok(()));
        // The same numbers as other services round them.
        assert_eq!(set_of(matrix("559082264.029", "-20037508.34 20037508.34", 1, 1)).web_mercator(), Ok(()));
        let why = |m: Matrix| set_of(m).web_mercator().unwrap_err();
        assert!(why(matrix("559082264.0287178", "20037508.342789244 -20037508.342789244", 1, 1)).contains("order"));
        assert!(why(Matrix { columns: 2, ..good.clone() }).contains("2 x 1"));
        assert!(why(matrix("279541132.0143589", "-20037508.342789244 20037508.342789244", 1, 1)).contains("scale"));
        assert!(why(matrix("x", "-20037508.342789244 20037508.342789244", 1, 1)).contains("scale"));
        assert!(why(matrix("559082264.0287178", "-180 90", 1, 1)).contains("corner"));
        assert!(why(matrix("559082264.0287178", "garbage", 1, 1)).contains("corner"));
        assert!(MatrixSet::default().web_mercator().unwrap_err().contains("no levels"));
    }

    #[test]
    fn a_value_in_a_query_is_short_and_plain() {
        for ok in ["omwrgbmrvl", "GoogleMapsVL", "EPSG:3857", "image/jpeg", "1.0.0", "a-b_c.d"] {
            assert!(query_value_ok(ok), "{ok}");
        }
        let long = "a".repeat(QUERY_VALUE_MAX + 1);
        for bad in ["", " ", "a b", "a&b", "a=b", "a%20b", "a+b", "a;b", "a#b", "a?b", "a\nb", "é", "{z}", long.as_str()] {
            assert!(!query_value_ok(bad), "{bad:?}");
        }
        assert!(query_value_ok(&"a".repeat(QUERY_VALUE_MAX)));
    }

    #[test]
    fn a_service_asked_by_query_gets_an_address_made_of_plain_parts() {
        let url = kvp_address("https://geo.example/wmts", "omwrgbmrvl", "default", "GoogleMapsVL", "image/jpeg").unwrap();
        assert_eq!(
            url,
            "https://geo.example/wmts?service=WMTS&request=GetTile&version=1.0.0&layer=omwrgbmrvl&style=default&tilematrixset=GoogleMapsVL&tilematrix={z}&tilerow={y}&tilecol={x}&format=image/jpeg"
        );
        let tile = expand(&url, 17, 67146, 43905).unwrap();
        assert!(tile.ends_with("&tilematrix=17&tilerow=43905&tilecol=67146&format=image/jpeg"), "{tile}");
        // A layer with no styles is asked with an empty one, and only the style may be empty.
        let plain = kvp_address("https://geo.example/wmts", "omwrgbmrvl", "", "GoogleMapsVL", "image/png").unwrap();
        assert!(plain.contains("&layer=omwrgbmrvl&style=&tilematrixset=GoogleMapsVL&"), "{plain}");
        let long = "a".repeat(QUERY_VALUE_MAX + 1);
        for (what, endpoint, layer, style, set, format) in [
            ("an endpoint with a query", "https://geo.example/wmts?a=b", "l", "s", "m", "image/png"),
            ("an endpoint with a fragment", "https://geo.example/wmts#x", "l", "s", "m", "image/png"),
            ("an endpoint that is not https", "http://geo.example/wmts", "l", "s", "m", "image/png"),
            ("an endpoint with a space", "https://geo.example/w mts", "l", "s", "m", "image/png"),
            ("a space in the layer", "https://geo.example/wmts", "a b", "s", "m", "image/png"),
            ("an ampersand in the style", "https://geo.example/wmts", "l", "s&x=1", "m", "image/png"),
            ("a percent in the set", "https://geo.example/wmts", "l", "s", "a%20b", "image/png"),
            ("an empty format", "https://geo.example/wmts", "l", "s", "m", ""),
            ("an empty layer", "https://geo.example/wmts", "", "s", "m", "image/png"),
            ("an empty set", "https://geo.example/wmts", "l", "s", "", "image/png"),
            ("a long layer", "https://geo.example/wmts", long.as_str(), "s", "m", "image/png"),
        ] {
            assert!(kvp_address(endpoint, layer, style, set, format).is_err(), "{what}");
        }
    }

    #[test]
    fn a_grid_is_drawn_on_the_pixels_a_multiple_of_the_distance_passes_through() {
        let magenta = [255, 0, 255, 255];
        let plain = vec![10u8; 100 * 100 * 4];
        let at = |img: &[u8], x: usize, y: usize| img[(y * 100 + x) * 4..][..4].to_vec();
        // 100 m each way, a metre a pixel, the origin in the middle: the lines through it every 100 m are one column and one row.
        let one = grid_overlay(&plain, 100, 100, [-50_000.0, -50_000.0, 50_000.0, 50_000.0], 100.0);
        assert_eq!(one.len(), plain.len());
        let columns: Vec<usize> = (0..100).filter(|x| at(&one, *x, 3) == magenta).collect();
        let rows: Vec<usize> = (0..100).filter(|y| at(&one, 3, *y) == magenta).collect();
        assert_eq!((columns.len(), rows.len()), (1, 1), "{columns:?} {rows:?}");
        assert!((48..=50).contains(&columns[0]) && (48..=50).contains(&rows[0]), "through the middle: {columns:?} {rows:?}");
        assert_eq!(at(&one, 3, 3), vec![10, 10, 10, 10], "the rest is as it was");
        // 300 m across: lines at -100, 0 and 100.
        let three = grid_overlay(&plain, 100, 100, [-150_000.0, -150_000.0, 150_000.0, 150_000.0], 100.0);
        assert_eq!((0..100).filter(|x| at(&three, *x, 3) == magenta).count(), 3);
        // Nothing to draw on, or no distance, leaves the picture alone; a picture that is not its size is given back as it is.
        assert_eq!(grid_overlay(&plain, 100, 100, [0.0, 0.0, 1.0, 1.0], 0.0), plain);
        assert_eq!(grid_overlay(&plain, 100, 100, [f64::NAN, 0.0, 1.0, 1.0], 100.0), plain);
        assert_eq!(grid_overlay(&plain[..40], 100, 100, [-50_000.0, -50_000.0, 50_000.0, 50_000.0], 100.0), plain[..40].to_vec());
    }

    #[test]
    fn a_picture_is_encoded_as_a_png_that_reads_back() {
        let rgba: Vec<u8> = (0..4 * 3).flat_map(|i| [i as u8 * 20, 100, 200 - i as u8, 255]).collect();
        let bytes = encode_png(4, 3, &rgba).unwrap();
        assert_eq!(Format::sniff(&bytes), Format::Png);
        let (w, h, rgb) = bimcraft_core::texture::decode_rgb(&bytes).unwrap();
        assert_eq!((w, h), (4, 3));
        assert_eq!(&rgb[..6], &[0, 100, 200, 20, 100, 199]);
        for (what, w, h, data) in [
            ("no width", 0, 3, rgba.clone()),
            ("no height", 4, 0, rgba.clone()),
            ("too few bytes", 4, 3, vec![0; 47]),
            ("too many", 4, 3, vec![0; 49]),
        ] {
            assert!(encode_png(w, h, &data).is_err(), "{what}");
        }
    }

    #[test]
    fn a_hostile_document_gives_what_it_can_and_never_runs_away() {
        let big = "<".repeat(2 << 20);
        for xml in [
            "",
            "<",
            "<>",
            "<Layer>",
            "<ows:Identifier>abc",
            "<!--",
            "<!-- x --",
            "<?xml",
            "<a b=\"unterminated>",
            "</Layer></Layer>",
            "<ResourceURL template=\"\"/>",
            "<ResourceURL/>",
            "\u{0}\u{1}",
            "<Layer><Layer><Layer>",
            big.as_str(),
        ] {
            let _ = scan_capabilities(xml);
        }
        // Many of a kind are kept to a number, a long text to a length.
        let many: String = (0..5000).map(|i| format!("<Layer><ows:Identifier>L{i}</ows:Identifier></Layer>")).collect();
        assert_eq!(scan_capabilities(&many).layers.len(), MAX_ITEMS);
        let long = format!("<ows:Fees>{}</ows:Fees>", "x".repeat(100_000));
        let fees = scan_capabilities(&long).fees;
        assert!(fees.len() == 1 && fees.iter().all(|f| f.len() <= MAX_TEXT));
        // Opening tags that are never closed, and closing tags that open nothing, cost no more than reading them once.
        let deep = format!("{}{}", "<a>".repeat(100_000), "</b>".repeat(100_000));
        let _ = scan_capabilities(&deep);
        // Past the cap of what is read, the rest is left unread, and a character is not cut in two.
        let over = format!("a{}{}", "é".repeat(MAX_CAPABILITIES), "<Layer><ows:Identifier>late</ows:Identifier></Layer>");
        assert!(scan_capabilities(&over).layers.is_empty());
    }
}
