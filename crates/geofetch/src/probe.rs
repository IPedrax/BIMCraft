//! The pure parts of trying an imagery service: a tile address from a template, what a reply is by its first bytes, the size of a
//! pixel on the ground, the tile a place is on, and what a WMTS capabilities document says (its layers, its tile sizes, the
//! addresses it gives for tiles, its fees and constraints). The developer's `imagery` example is a thin shell over these; none of
//! it touches the network, and what a document says is hostile, so it is read to a cap and never indexed blindly.

use bimcraft_core::geo::tile_xy;

/// The finest zoom an address is made for: 2^24 columns and rows.
pub const MAX_ZOOM: u8 = 24;
/// The longest address template taken, characters.
pub const MAX_TEMPLATE: usize = 2048;
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

/// The address of a tile: `template` with `{z}`, `{x}` and `{y}` (or `{-y}`, rows from the south) put in.
///
/// WMTS's own names for them (`{TileMatrix}`, `{TileCol}`, `{TileRow}`) and `{zoom}` are taken too, so an address copied from a
/// capabilities document works. Anything else in braces (`{Style}`, `{TileMatrixSet}`) is refused by name, to be written in by hand.
pub fn expand(template: &str, zoom: u8, x: u32, y: u32) -> Result<String, String> {
    if template.len() > MAX_TEMPLATE {
        return Err(format!("the address is {} characters; up to {MAX_TEMPLATE} are taken", template.len()));
    }
    if zoom > MAX_ZOOM {
        return Err(format!("zoom {zoom} is finer than the {MAX_ZOOM} an address is made for"));
    }
    let n = 1u64 << zoom;
    if u64::from(x) >= n || u64::from(y) >= n {
        return Err(format!("there is no tile {x}/{y} at zoom {zoom}: the map has {n} columns and rows there"));
    }
    let (mut out, mut rest) = (String::new(), template);
    let (mut has_zoom, mut has_column, mut has_row) = (false, false, false);
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(rest.get(..at).unwrap_or_default());
        let tail = rest.get(at..).unwrap_or_default();
        if tail.starts_with('}') {
            return Err("the address has a } with no { before it".into());
        }
        let Some(close) = tail.find('}') else { return Err("the address has a { with no } after it".into()) };
        match tail.get(1..close).unwrap_or_default() {
            "z" | "zoom" | "TileMatrix" => {
                out.push_str(&zoom.to_string());
                has_zoom = true;
            }
            "x" | "TileCol" => {
                out.push_str(&x.to_string());
                has_column = true;
            }
            "y" | "TileRow" => {
                out.push_str(&y.to_string());
                has_row = true;
            }
            "-y" => {
                out.push_str(&(n - 1 - u64::from(y)).to_string());
                has_row = true;
            }
            other => {
                return Err(format!(
                    "the address has {{{other}}}, which is none of {{z}}, {{x}}, {{y}} and {{-y}}: write that part in by hand"
                ));
            }
        }
        rest = tail.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    if !(has_zoom && has_column && has_row) {
        return Err("an address needs {z}, {x} and {y} (or {-y}) in it".into());
    }
    Ok(out)
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

/// What a WMTS capabilities document says that matters for fetching from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capabilities {
    pub title: Option<String>,
    pub fees: Vec<String>,
    pub constraints: Vec<String>,
    pub layers: Vec<String>,
    pub matrix_sets: Vec<String>,
    pub matrices: Vec<String>,
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
                    stack.push(name);
                }
            }
            Token::End { name } => {
                if stack.last() == Some(&name) {
                    stack.pop();
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
                    (Some("Identifier"), Some("Layer")) => keep(&mut caps.layers, text, false),
                    (Some("Identifier"), Some("TileMatrixSet")) => keep(&mut caps.matrix_sets, text, true),
                    (Some("Identifier"), Some("TileMatrix")) => keep(&mut caps.matrices, text, true),
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
   <ows:Identifier>Actueel_orthoHR</ows:Identifier>
   <Format>image/jpeg</Format>
   <TileMatrixSetLink><TileMatrixSet>EPSG:3857</TileMatrixSet></TileMatrixSetLink>
   <ResourceURL format="image/jpeg" resourceType="tile" template="https://example.org/wmts/Actueel_orthoHR/EPSG:3857/{TileMatrix}/{TileCol}/{TileRow}.jpeg"/>
  </Layer>
  <TileMatrixSet>
   <ows:Identifier>EPSG:3857</ows:Identifier>
   <ows:SupportedCRS>urn:ogc:def:crs:EPSG::3857</ows:SupportedCRS>
   <TileMatrix><ows:Identifier>0</ows:Identifier><TileWidth>256</TileWidth><TileHeight>256</TileHeight></TileMatrix>
   <TileMatrix><ows:Identifier>1</ows:Identifier><TileWidth>256</TileWidth><TileHeight>256</TileHeight></TileMatrix>
  </TileMatrixSet>
 </Contents>
</Capabilities>"#;

    #[test]
    fn a_capabilities_document_gives_its_layers_sizes_addresses_and_terms() {
        let caps = scan_capabilities(WMTS);
        assert_eq!(caps.title.as_deref(), Some("Luchtfoto"), "the service's, not a layer's");
        assert_eq!(caps.fees, ["none"]);
        assert_eq!(caps.constraints, ["CC-BY 4.0"]);
        assert_eq!(caps.layers, ["Actueel_orthoHR"], "the layer's name, not the set it links to");
        assert_eq!(caps.matrix_sets, ["EPSG:3857"], "the set that is defined, once");
        assert_eq!(caps.matrices, ["0", "1"]);
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
        assert_eq!(caps.layers, ["ortho"]);
        assert_eq!(caps.fees, ["free & open"], "entities undone, comments skipped");
        assert_eq!(caps.resource_urls, ["image/png https://example.org/t?layer=a&z={TileMatrix}"]);
        // An attribute whose name ends the same is not the one asked for.
        let other =
            r#"<ResourceURL format="image/png" subtemplate="https://wrong.example/" template="https://right.example/{TileMatrix}"/>"#;
        assert_eq!(scan_capabilities(other).resource_urls, ["image/png https://right.example/{TileMatrix}"]);
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
