//! OpenStreetMap data: Overpass JSON (the `out geom` form, which carries the geometry on each way, and the plain form, whose ways
//! name nodes) and OSM XML, read into one shape, and the buildings found in it: closed ways and multipolygon relations tagged
//! `building`, their outlines in longitude and latitude, their heights, and why anything was left out. No I/O and no model:
//! `import_osm` projects the outlines and makes the masses. What a file holds is hostile: sizes are capped, coordinates are
//! checked, and a feature that cannot be a building is counted, not a failure.
//!
//! Data (c) OpenStreetMap contributors, under the Open Database Licence (ODbL).

use std::collections::{BTreeMap, HashMap};

use crate::geom::P2;

/// The most elements (nodes, ways and relations) one file may hold.
pub const MAX_ELEMENTS: usize = 3_000_000;
/// The most points one way may have.
pub const MAX_WAY_POINTS: usize = 20_000;
/// The most buildings one import makes.
pub const MAX_BUILDINGS: usize = 20_000;
/// The most members one relation may have.
pub const MAX_MEMBERS: usize = 2_000;
/// Rings with more corners than this are not checked for crossing themselves (the check is quadratic), and so are left out.
pub const MAX_RING_CHECK: usize = 2_000;
/// The longest a tag's text is kept, characters.
const MAX_TAG: usize = 256;
/// Most tags kept on one element.
const MAX_TAGS: usize = 256;

/// A longitude and latitude, degrees. NaN where the file gave none that make sense.
pub type LonLat = (f64, f64);

type Tags = BTreeMap<String, String>;

#[derive(Clone, Debug, Default)]
struct Way {
    refs: Vec<i64>,
    inline: Vec<LonLat>,
    tags: Tags,
    /// It has more points than are read.
    too_long: bool,
}

#[derive(Clone, Debug)]
struct Member {
    kind: String,
    id: i64,
    role: String,
    inline: Vec<LonLat>,
}

#[derive(Clone, Debug, Default)]
struct Relation {
    members: Vec<Member>,
    tags: Tags,
}

/// What a file holds: its nodes, ways and relations.
#[derive(Clone, Debug, Default)]
pub struct Osm {
    nodes: HashMap<i64, LonLat>,
    ways: BTreeMap<i64, Way>,
    relations: BTreeMap<i64, Relation>,
}

/// What to make of the buildings.
#[derive(Clone, Debug)]
pub struct Options {
    /// Parts of buildings (`building:part`) and columns (`man_made=column`) are buildings too: the default, as a building is its parts.
    pub parts: bool,
    /// How tall a level is where only `building:levels` says, mm.
    pub level_height: f64,
    /// How tall a building is where nothing says, mm.
    pub default_height: f64,
    /// Only buildings with a corner inside: south, west, north, east, degrees.
    pub bbox: Option<[f64; 4]>,
}

impl Default for Options {
    fn default() -> Options {
        Options { parts: true, level_height: 3000.0, default_height: 6000.0, bbox: None }
    }
}

/// A building: its outline and courtyards in longitude and latitude (not closed: the last point is not the first again), its
/// height above the ground and what it is lifted off it, mm.
#[derive(Clone, Debug, PartialEq)]
pub struct Building {
    /// `way/123`, or `relation/456` (`relation/456#2` for a second outline of one).
    pub osm_id: String,
    pub outer: Vec<LonLat>,
    pub holes: Vec<Vec<LonLat>>,
    pub height: f64,
    pub min_height: f64,
    pub name: String,
    /// What the `building` tag says: `yes`, `house`, `apartments`.
    pub kind: String,
    /// What `building:levels` says, as given.
    pub levels: String,
    /// How tall the roof is on top of `height`, mm: 0 where no roof is told apart from the facade. `height` is the facade's.
    pub roof_height: f64,
    /// `roof:shape`, lower case: `gabled`, `hipped`, `flat`, ...; empty if not told.
    pub roof_shape: String,
    /// `roof:angle`, degrees, when it is one between 0 and 90.
    pub roof_angle: Option<f64>,
    /// `roof:orientation`: `along` or `across`, else empty.
    pub roof_orientation: String,
    /// `roof:direction`, as given (degrees or a compass point).
    pub roof_direction: String,
    /// `building:colour` (else `colour`), `building:material` (else `material`), and the roof's, as given.
    pub colour: String,
    pub material: String,
    pub roof_colour: String,
    pub roof_material: String,
    /// For a part: the outline it belongs to (`way/1`), when there is one.
    pub part_of: String,
    /// It is a part of a building (tagged `building:part`, or a column), not an outline.
    pub is_part: bool,
}

/// A feature that is not a building here, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Skipped {
    pub osm_id: String,
    pub why: String,
}

/// The buildings a file holds.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub buildings: Vec<Building>,
    pub skipped: Vec<Skipped>,
    /// How many of the buildings have no usable height and take the default.
    pub defaulted: usize,
    /// How many outlines were left out because their building parts stand in their place.
    pub replaced: usize,
    /// How many outlines were kept although they have parts, because the parts cover too little of them.
    pub kept: usize,
}

/// A coordinate that is on the Earth, or NaN, which nothing built from it will accept.
fn coordinate(lat: Option<f64>, lon: Option<f64>) -> LonLat {
    match (lat, lon) {
        (Some(lat), Some(lon)) if lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0 => (lon, lat),
        _ => (f64::NAN, f64::NAN),
    }
}

fn clip(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// Reads Overpass JSON or OSM XML, by its first character.
pub fn parse(text: &str) -> Result<Osm, String> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    match text.chars().next() {
        Some('{') => from_json(text),
        Some('<') => from_xml(text),
        _ => Err("it is neither Overpass JSON nor OSM XML: a file of either begins with `{` or `<`".into()),
    }
}

fn json_tags(v: Option<&serde_json::Value>) -> Tags {
    v.and_then(serde_json::Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| Some((clip(k, 64), clip(v.as_str()?, MAX_TAG)))).take(MAX_TAGS).collect())
        .unwrap_or_default()
}

fn json_geometry(v: Option<&serde_json::Value>) -> (Vec<LonLat>, bool) {
    let Some(a) = v.and_then(serde_json::Value::as_array) else { return (Vec::new(), false) };
    let points = a
        .iter()
        .take(MAX_WAY_POINTS)
        .map(|p| coordinate(p.get("lat").and_then(serde_json::Value::as_f64), p.get("lon").and_then(serde_json::Value::as_f64)))
        .collect();
    (points, a.len() > MAX_WAY_POINTS)
}

fn from_json(text: &str) -> Result<Osm, String> {
    use serde_json::Value;
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let elements = v
        .get("elements")
        .and_then(Value::as_array)
        .ok_or("it has no `elements` list: Overpass JSON has one (ask Overpass for `[out:json]`)")?;
    if elements.len() > MAX_ELEMENTS {
        return Err(format!("it has {} elements; at most {MAX_ELEMENTS} are read", elements.len()));
    }
    let mut osm = Osm::default();
    for el in elements {
        let Some(id) = el.get("id").and_then(Value::as_i64) else { continue };
        match el.get("type").and_then(Value::as_str) {
            Some("node") => {
                osm.nodes.insert(id, coordinate(el.get("lat").and_then(Value::as_f64), el.get("lon").and_then(Value::as_f64)));
            }
            Some("way") => {
                let refs = el.get("nodes").and_then(Value::as_array);
                let (inline, long) = json_geometry(el.get("geometry"));
                osm.ways.insert(
                    id,
                    Way {
                        refs: refs.map(|a| a.iter().take(MAX_WAY_POINTS).filter_map(Value::as_i64).collect()).unwrap_or_default(),
                        inline,
                        tags: json_tags(el.get("tags")),
                        too_long: long || refs.is_some_and(|a| a.len() > MAX_WAY_POINTS),
                    },
                );
            }
            Some("relation") => {
                let members = el.get("members").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
                let members = members
                    .iter()
                    .take(MAX_MEMBERS)
                    .filter_map(|m| {
                        Some(Member {
                            kind: m.get("type")?.as_str()?.to_string(),
                            id: m.get("ref")?.as_i64()?,
                            role: clip(m.get("role").and_then(Value::as_str).unwrap_or(""), 64),
                            inline: json_geometry(m.get("geometry")).0,
                        })
                    })
                    .collect();
                osm.relations.insert(id, Relation { members, tags: json_tags(el.get("tags")) });
            }
            _ => {}
        }
    }
    Ok(osm)
}

fn from_xml(text: &str) -> Result<Osm, String> {
    let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 50_000_000, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, options).map_err(|e| format!("not valid OSM XML: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "osm" {
        return Err(format!("the XML is not OSM: its root element is `{}`, not `osm`", clip(root.tag_name().name(), 64)));
    }
    let num = |n: roxmltree::Node, k: &str| n.attribute(k).and_then(|v| v.trim().parse::<f64>().ok());
    let int = |n: roxmltree::Node, k: &str| n.attribute(k).and_then(|v| v.trim().parse::<i64>().ok());
    let tags = |n: roxmltree::Node| -> Tags {
        n.children()
            .filter(|c| c.has_tag_name("tag"))
            .filter_map(|c| Some((clip(c.attribute("k")?, 64), clip(c.attribute("v")?, MAX_TAG))))
            .take(MAX_TAGS)
            .collect()
    };
    // A way's `nd` elements: the node they name, and where it is, if the file says.
    let nds = |n: roxmltree::Node| -> (Vec<i64>, Vec<LonLat>, bool) {
        let all: Vec<_> = n.children().filter(|c| c.has_tag_name("nd")).collect();
        let refs = all.iter().take(MAX_WAY_POINTS).filter_map(|c| int(*c, "ref")).collect();
        let inline = if all.iter().any(|c| c.attribute("lat").is_some()) {
            all.iter().take(MAX_WAY_POINTS).map(|c| coordinate(num(*c, "lat"), num(*c, "lon"))).collect()
        } else {
            Vec::new()
        };
        (refs, inline, all.len() > MAX_WAY_POINTS)
    };
    let mut osm = Osm::default();
    let mut count = 0usize;
    for el in root.children().filter(roxmltree::Node::is_element) {
        count += 1;
        if count > MAX_ELEMENTS {
            return Err(format!("it has more than {MAX_ELEMENTS} elements"));
        }
        let Some(id) = int(el, "id") else { continue };
        match el.tag_name().name() {
            "node" => {
                osm.nodes.insert(id, coordinate(num(el, "lat"), num(el, "lon")));
            }
            "way" => {
                let (refs, inline, too_long) = nds(el);
                osm.ways.insert(id, Way { refs, inline, tags: tags(el), too_long });
            }
            "relation" => {
                let members = el
                    .children()
                    .filter(|c| c.has_tag_name("member"))
                    .take(MAX_MEMBERS)
                    .filter_map(|m| {
                        Some(Member {
                            kind: m.attribute("type")?.to_string(),
                            id: int(m, "ref")?,
                            role: clip(m.attribute("role").unwrap_or(""), 64),
                            inline: nds(m).1,
                        })
                    })
                    .collect();
                osm.relations.insert(id, Relation { members, tags: tags(el) });
            }
            _ => {}
        }
    }
    Ok(osm)
}

/// The most a length may be, mm: a kilometre.
const MAX_LENGTH: f64 = 1_000_000.0;

/// A length as OSM writes it, in mm: `12`, `12.5`, `12 m`, `12,5 m`, `40 ft`, `40'`, `40' 6"`.
pub fn length(text: &str) -> Option<f64> {
    let t = text.trim().to_lowercase();
    let number = |s: &str| s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
    let mm = if let Some((feet, rest)) = t.split_once('\'') {
        let rest = rest.trim().trim_end_matches('"').trim();
        let inches = if rest.is_empty() { 0.0 } else { number(rest)? };
        (number(feet)? * 12.0 + inches) * 25.4
    } else {
        let split = t.find(|c: char| c.is_alphabetic()).unwrap_or(t.len());
        let value = number(&t[..split])?;
        match t[split..].trim() {
            "" | "m" | "meter" | "meters" | "metre" | "metres" => value * 1000.0,
            "ft" | "foot" | "feet" => value * 304.8,
            _ => return None,
        }
    };
    (mm.is_finite() && mm <= MAX_LENGTH).then_some(mm)
}

/// A ring's corners as a key two equal places share (to a ten-millionth of a degree, a centimetre).
fn key(p: LonLat) -> (i64, i64) {
    ((p.0 * 1e7).round() as i64, (p.1 * 1e7).round() as i64)
}

/// Whether segment `a`-`b` meets segment `c`-`d`, touching included.
fn meets(a: LonLat, b: LonLat, c: LonLat, d: LonLat) -> bool {
    let side = |p: LonLat, q: LonLat, r: LonLat| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let on = |p: LonLat, q: LonLat, r: LonLat| r.0 >= p.0.min(q.0) && r.0 <= p.0.max(q.0) && r.1 >= p.1.min(q.1) && r.1 <= p.1.max(q.1);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    (d1 == 0.0 && on(c, d, a)) || (d2 == 0.0 && on(c, d, b)) || (d3 == 0.0 && on(a, b, c)) || (d4 == 0.0 && on(a, b, d))
}

/// A closed ring made into an open one (no closing point) that is a simple polygon, or why it is not.
fn clean(points: &[LonLat]) -> Result<Vec<LonLat>, String> {
    if points.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
        return Err("a corner of it is missing from the file or is not on the Earth".into());
    }
    let mut ring: Vec<LonLat> = Vec::with_capacity(points.len());
    for p in points {
        if ring.last().is_none_or(|q| key(*q) != key(*p)) {
            ring.push(*p);
        }
    }
    if ring.len() < 4 || ring.first().map(|p| key(*p)) != ring.last().map(|p| key(*p)) {
        return Err("it is not a closed outline, or has fewer than three corners".into());
    }
    ring.pop();
    if ring.len() > MAX_RING_CHECK {
        return Err(format!("it has more than {MAX_RING_CHECK} corners to check, so it is left out"));
    }
    let n = ring.len();
    for i in 0..n {
        for j in i + 1..n {
            // Neighbours meet at their shared corner, as they should.
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if meets(ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]) {
                return Err("its outline crosses itself".into());
            }
        }
    }
    // From the first corner: the corners of a column are a hair apart, a long way from (0, 0), and the sum would lose them.
    let (ox, oy) = ring.first().copied().unwrap_or_default();
    let at = |i: usize| (ring[i % ring.len()].0 - ox, ring[i % ring.len()].1 - oy);
    let twice: f64 = (0..ring.len()).map(|i| at(i).0 * at(i + 1).1 - at(i + 1).0 * at(i).1).sum();
    if twice.abs() < 1e-14 {
        return Err("it encloses no area".into());
    }
    Ok(ring)
}

/// Pieces of outline joined end to end, in either direction, into closed rings; how many pieces were left over.
fn join(mut pieces: Vec<Vec<LonLat>>) -> (Vec<Vec<LonLat>>, usize) {
    let (mut rings, mut open) = (Vec::new(), 0);
    pieces.retain(|p| p.len() >= 2);
    while let Some(mut ring) = pieces.pop() {
        loop {
            if ring.len() >= 4 && ring.first().map(|p| key(*p)) == ring.last().map(|p| key(*p)) {
                rings.push(ring);
                break;
            }
            let Some(end) = ring.last().map(|p| key(*p)) else { break };
            if let Some(i) = pieces.iter().position(|p| p.first().map(|q| key(*q)) == Some(end)) {
                ring.extend(pieces.remove(i).into_iter().skip(1));
            } else if let Some(i) = pieces.iter().position(|p| p.last().map(|q| key(*q)) == Some(end)) {
                let mut piece = pieces.remove(i);
                piece.reverse();
                ring.extend(piece.into_iter().skip(1));
            } else {
                open += 1;
                break;
            }
        }
    }
    (rings, open)
}

fn inside(ring: &[LonLat], p: LonLat) -> bool {
    let ring: Vec<P2> = ring.iter().map(|q| P2(q.0, q.1)).collect();
    crate::geom::contains(&ring, P2(p.0, p.1))
}

impl Osm {
    /// A way's points: the geometry it carries, else the nodes it names.
    fn points(&self, w: &Way) -> Result<Vec<LonLat>, String> {
        if w.too_long {
            return Err(format!("it has more than {MAX_WAY_POINTS} points"));
        }
        if !w.inline.is_empty() {
            return Ok(w.inline.clone());
        }
        w.refs.iter().map(|r| self.nodes.get(r).copied().ok_or_else(|| "a node of it is not in the file".to_string())).collect()
    }
}

/// What the height tags of a building make: the facade's height (what the walls rise to) and the roof's on top of it, how far off the
/// ground it starts, mm; and whether the height is the default.
struct Heights {
    facade: f64,
    roof: f64,
    min: f64,
    defaulted: bool,
}

/// The first of `keys` a building has, trimmed and cut to the length kept.
fn text(tags: &Tags, keys: &[&str]) -> String {
    keys.iter().find_map(|k| tags.get(*k)).map(|t| clip(t.trim(), MAX_TAG)).unwrap_or_default()
}

/// `roof:shape`, lower case.
fn roof_shape(tags: &Tags) -> String {
    text(tags, &["roof:shape"]).to_lowercase()
}

fn heights(tags: &Tags, o: &Options, roof_only: bool) -> Result<Heights, String> {
    let level_height = if o.level_height.is_finite() && o.level_height > 0.0 { o.level_height } else { 3000.0 };
    let levels = |key: &str| {
        tags.get(key).and_then(|t| t.trim().replace(',', ".").parse::<f64>().ok()).filter(|n| n.is_finite() && (0.0..=500.0).contains(n))
    };
    let by_height = ["height", "building:height"].iter().find_map(|k| tags.get(*k).and_then(|t| length(t))).filter(|h| *h > 0.0);
    let by_levels = levels("building:levels").filter(|n| *n > 0.0).map(|n| n * level_height);
    let roof_tag = tags.get("roof:height").and_then(|t| length(t)).filter(|h| *h > 0.0);
    let roof_levels = levels("roof:levels").filter(|n| *n > 0.0).map(|n| n * level_height);
    let shape = roof_shape(tags);
    let pitched = !shape.is_empty() && shape != "flat";
    let min =
        tags.get("min_height").and_then(|t| length(t)).or_else(|| levels("building:min_level").map(|n| n * level_height)).unwrap_or(0.0);
    let default = if o.default_height.is_finite() && o.default_height > 0.0 { o.default_height.min(MAX_LENGTH) } else { 6000.0 };
    // The facade and the roof the tags make, and the facade there is where no roof is told apart.
    let (facade, roof, plain, defaulted) = match (by_height, by_levels) {
        // `height` is the whole: the roof is what `roof:height` says, or, for a pitched roof, what the storeys leave of it.
        (Some(h), l) => {
            let roof = roof_tag.or(if pitched { l.map(|f| h - f) } else { None }).unwrap_or(0.0);
            (h - roof, roof, h, false)
        }
        // The storeys are the facade and the roof comes on top of them.
        (None, Some(f)) => {
            let roof = roof_tag.or(if pitched { roof_levels } else { None }).unwrap_or(0.0);
            (f, roof, f, false)
        }
        (None, None) => {
            let roof = roof_tag.unwrap_or(0.0);
            (default - roof, roof, default, true)
        }
    };
    // A roof is believed only if the facade under it stays above what the building is lifted by.
    let (facade, roof) = if roof > 0.0 && facade - min >= 1.0 { (facade, roof) } else { (plain, 0.0) };
    // A part that is a roof (`building:part=roof`) has no walls: with no `min_height` it is the roof's own slab at the top, 100 mm thick
    // (a flat roof: as thick as `roof:height` says), and a pitched roof's slab is at the eaves, with the roof on it.
    let lifted = tags.contains_key("min_height") || tags.contains_key("building:min_level");
    let (facade, roof, min) = if roof_only && !lifted {
        const SLAB: f64 = 100.0;
        if pitched && roof > 0.0 {
            (facade, roof, (facade - SLAB).max(0.0))
        } else {
            let top = facade + roof;
            let thickness = if roof > 0.0 { roof } else { SLAB };
            (top, 0.0, (top - thickness).max(0.0))
        }
    } else {
        (facade, roof, min)
    };
    if min >= facade - 1.0 {
        return Err(format!("its min_height ({min} mm) is not below its height ({facade} mm)"));
    }
    Ok(Heights { facade, roof, min, defaulted })
}

/// What an `OSM Building` parameter says: the `building` tag; for a part with none, its `building:part` value, or `part`; `column`.
fn kind_of(tags: &Tags, part: bool) -> String {
    if let Some(b) = tags.get("building").filter(|b| b.as_str() != "no") {
        return b.clone();
    }
    if tags.get("man_made").map(String::as_str) == Some("column") {
        return "column".into();
    }
    match tags.get("building:part").map(|v| v.trim()) {
        Some(v) if part && !v.is_empty() && v != "yes" => clip(v, MAX_TAG),
        _ if part => "part".into(),
        _ => String::new(),
    }
}

/// `roof:angle` in degrees, when it is between 0 and 90 (exclusive).
fn roof_angle(tags: &Tags) -> Option<f64> {
    tags.get("roof:angle")
        .and_then(|t| t.trim().replace(',', ".").trim_end_matches('°').trim().parse::<f64>().ok())
        .filter(|a| a.is_finite() && *a > 0.0 && *a < 90.0)
}

/// The buildings in `osm`.
pub fn buildings(osm: &Osm, o: &Options) -> Found {
    let mut found = Found::default();
    let in_box = |ring: &[LonLat]| o.bbox.is_none_or(|[s, w, n, e]| ring.iter().any(|p| p.1 >= s && p.1 <= n && p.0 >= w && p.0 <= e));
    let named = |tags: &Tags, key: &str| tags.get(key).filter(|v| v.as_str() != "no").is_some();
    // Whether this is a building (or a part, if parts are wanted): `Ok(true)` a part only; `Err`, to be passed over.
    let wanted = |id: &str, tags: &Tags, found: &mut Found| -> Option<bool> {
        let (building, part) = (named(tags, "building"), named(tags, "building:part"));
        let column = tags.get("man_made").map(String::as_str) == Some("column");
        if !building && !part && !(column && o.parts) {
            return None;
        }
        if !building && !o.parts {
            found.skipped.push(Skipped {
                osm_id: id.into(),
                why: "it is a building part (building:part); import with parts to include parts".into(),
            });
            return None;
        }
        // A feature tagged as a part is a part, whatever else it is tagged (a kiosk that is its own only part).
        Some(o.parts && (part || column))
    };
    let make = |found: &mut Found, id: String, tags: &Tags, part: bool, outer: Vec<LonLat>, holes: Vec<Vec<LonLat>>| {
        if !in_box(&outer) {
            return;
        }
        if found.buildings.len() >= MAX_BUILDINGS {
            if !found.skipped.iter().any(|s| s.why.contains("more than")) {
                found
                    .skipped
                    .push(Skipped { osm_id: id, why: format!("more than {MAX_BUILDINGS} buildings: this and the rest are left out") });
            }
            return;
        }
        let roof_only = part && tags.get("building:part").is_some_and(|v| v.trim() == "roof");
        match heights(tags, o, roof_only) {
            Ok(h) => {
                found.defaulted += usize::from(h.defaulted);
                found.buildings.push(Building {
                    osm_id: id,
                    outer,
                    holes,
                    height: h.facade,
                    min_height: h.min,
                    name: tags.get("name").cloned().unwrap_or_default(),
                    kind: kind_of(tags, part),
                    levels: tags.get("building:levels").cloned().unwrap_or_default(),
                    roof_height: h.roof,
                    roof_shape: roof_shape(tags),
                    roof_angle: roof_angle(tags),
                    roof_orientation: Some(text(tags, &["roof:orientation"]).to_lowercase())
                        .filter(|t| t == "along" || t == "across")
                        .unwrap_or_default(),
                    roof_direction: text(tags, &["roof:direction"]),
                    colour: text(tags, &["building:colour", "colour"]),
                    material: text(tags, &["building:material", "material"]),
                    roof_colour: text(tags, &["roof:colour"]),
                    roof_material: text(tags, &["roof:material"]),
                    part_of: String::new(),
                    is_part: part,
                });
            }
            Err(why) => found.skipped.push(Skipped { osm_id: id, why }),
        }
    };
    for (id, w) in &osm.ways {
        let osm_id = format!("way/{id}");
        let Some(part) = wanted(&osm_id, &w.tags, &mut found) else { continue };
        match osm.points(w).and_then(|p| clean(&p)) {
            Ok(ring) => make(&mut found, osm_id, &w.tags, part, ring, Vec::new()),
            Err(why) => found.skipped.push(Skipped { osm_id, why }),
        }
    }
    // The outline and parts a `type=building` relation groups, by the ids the buildings made from them go by.
    let mut groups: Vec<Group> = Vec::new();
    for (id, r) in &osm.relations {
        let osm_id = format!("relation/{id}");
        if r.tags.get("type").map(String::as_str) == Some("building") {
            let member = |role: &str| -> Vec<String> {
                r.members
                    .iter()
                    .filter(|m| m.role == role && (m.kind == "way" || m.kind == "relation"))
                    .map(|m| format!("{}/{}", m.kind, m.id))
                    .collect()
            };
            if let Some(outline) = member("outline").into_iter().next() {
                groups.push(Group { outline, parts: member("part"), name: text(&r.tags, &["name"]) });
            }
            continue;
        }
        let Some(part) = wanted(&osm_id, &r.tags, &mut found) else { continue };
        let skip = |found: &mut Found, why: &str| found.skipped.push(Skipped { osm_id: osm_id.clone(), why: why.into() });
        if r.tags.get("type").map(String::as_str) != Some("multipolygon") {
            skip(&mut found, "it is a relation but not a multipolygon");
            continue;
        }
        let (mut outers, mut inners) = (Vec::new(), Vec::new());
        let mut broken = None;
        for m in r.members.iter().filter(|m| m.kind == "way") {
            let points = if m.inline.is_empty() {
                osm.ways.get(&m.id).ok_or_else(|| "a way of it is not in the file".to_string()).and_then(|w| osm.points(w))
            } else {
                Ok(m.inline.clone())
            };
            match points {
                Ok(p) if p.iter().all(|q| q.0.is_finite() && q.1.is_finite()) => {
                    if m.role == "inner" {
                        inners.push(p)
                    } else {
                        outers.push(p)
                    }
                }
                Ok(_) => broken = Some("a corner of it is missing from the file or is not on the Earth".to_string()),
                Err(why) => broken = Some(why),
            }
        }
        if let Some(why) = broken {
            skip(&mut found, &why);
            continue;
        }
        if outers.is_empty() {
            skip(&mut found, "it has no outer way");
            continue;
        }
        let (rings, open) = join(outers);
        if open > 0 && rings.is_empty() {
            skip(&mut found, "its outer ways do not close into an outline");
            continue;
        }
        let holes: Vec<Vec<LonLat>> = join(inners).0.iter().filter_map(|h| clean(h).ok()).collect();
        let mut n = 0;
        for ring in rings {
            match clean(&ring) {
                Ok(outer) => {
                    n += 1;
                    let own: Vec<Vec<LonLat>> = holes.iter().filter(|h| inside(&outer, h[0])).cloned().collect();
                    let id = if n == 1 { osm_id.clone() } else { format!("{osm_id}#{n}") };
                    make(&mut found, id, &r.tags, part, outer, own);
                }
                Err(why) => skip(&mut found, &why),
            }
        }
    }
    if o.parts {
        assign_parts(&mut found, &groups);
    }
    found
}

/// The place in a part that is certainly inside it: the middle of its biggest triangle (a corner may lie on the outline's own side,
/// as a part's sides match its outline's).
fn interior(ring: &[LonLat]) -> LonLat {
    let pts: Vec<P2> = ring.iter().map(|q| P2(q.0, q.1)).collect();
    let mean = || {
        let n = pts.len().max(1) as f64;
        (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n)
    };
    // Ear clipping is cubic: a part of thousands of corners takes its corners' mean.
    if pts.len() > 200 {
        return mean();
    }
    let best = crate::geom::triangulate(&pts).into_iter().max_by(|a, b| {
        let area = |t: &[usize; 3]| crate::geom::area(&[pts[t[0]], pts[t[1]], pts[t[2]]]).abs();
        area(a).total_cmp(&area(b))
    });
    match best {
        Some(t) => ((pts[t[0]].0 + pts[t[1]].0 + pts[t[2]].0) / 3.0, (pts[t[0]].1 + pts[t[1]].1 + pts[t[2]].1) / 3.0),
        None => mean(),
    }
}

/// What a `type=building` relation groups: the outline, its parts, and the name it gives them.
struct Group {
    outline: String,
    parts: Vec<String>,
    name: String,
}

/// How much of an outline its parts cover, 0 to 1: the share of a grid of places in it (24 by 24) that lie in some part.
fn coverage(outline: &Building, parts: &[&Building]) -> f64 {
    const N: usize = 24;
    let (x0, x1) = outline.outer.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let (y0, y1) = outline.outer.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let in_building = |b: &Building, p: LonLat| inside(&b.outer, p) && !b.holes.iter().any(|h| inside(h, p));
    let (mut within, mut covered) = (0usize, 0usize);
    for i in 0..N {
        for j in 0..N {
            let p = (x0 + (i as f64 + 0.5) / N as f64 * (x1 - x0), y0 + (j as f64 + 0.5) / N as f64 * (y1 - y0));
            if !in_building(outline, p) {
                continue;
            }
            within += 1;
            covered += usize::from(parts.iter().any(|q| in_building(q, p)));
        }
    }
    if within == 0 { 1.0 } else { covered as f64 / within as f64 }
}

/// Parts say whose they are, and an outline with parts is left out: its parts are its 3D (the scheme's rule). A `type=building`
/// relation names the outline and the parts wherever they lie; otherwise a part belongs to the outline it stands inside.
fn assign_parts(found: &mut Found, groups: &[Group]) {
    let base = |id: &str| id.split('#').next().unwrap_or(id).to_string();
    // By the relations.
    let by_relation: HashMap<String, String> =
        groups.iter().flat_map(|g| g.parts.iter().map(move |p| (p.clone(), g.outline.clone()))).collect();
    let group_name: HashMap<&str, &str> =
        groups.iter().filter(|g| !g.name.is_empty()).map(|g| (g.outline.as_str(), g.name.as_str())).collect();
    // By place: the outlines in a grid of cells about 55 m across, so a part meets only those near it.
    const CELL: f64 = 0.0005;
    const MAX_CELLS: i64 = 400;
    let cell = |v: f64| (v / CELL).floor() as i64;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut big: Vec<usize> = Vec::new();
    for (i, b) in found.buildings.iter().enumerate().filter(|(_, b)| !b.is_part) {
        let (x0, x1) = b.outer.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
        let (y0, y1) = b.outer.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
        let (nx, ny) = (cell(x1) - cell(x0) + 1, cell(y1) - cell(y0) + 1);
        if nx * ny > MAX_CELLS {
            big.push(i);
            continue;
        }
        for cx in cell(x0)..=cell(x1) {
            for cy in cell(y0)..=cell(y1) {
                grid.entry((cx, cy)).or_default().push(i);
            }
        }
    }
    let mut owner: Vec<Option<usize>> = vec![None; found.buildings.len()];
    let mut named: Vec<String> = vec![String::new(); found.buildings.len()];
    let mut grouped: Vec<Option<String>> = vec![None; found.buildings.len()];
    let position: HashMap<String, usize> =
        found.buildings.iter().enumerate().filter(|(_, b)| !b.is_part).map(|(i, b)| (b.osm_id.clone(), i)).collect();
    // By index: a part is set while the outlines it is matched with are read.
    #[allow(clippy::needless_range_loop)]
    for i in 0..found.buildings.len() {
        if !found.buildings[i].is_part {
            continue;
        }
        let part = &found.buildings[i];
        if let Some(outline) = by_relation.get(&base(&part.osm_id)) {
            found.buildings[i].part_of = outline.clone();
            owner[i] = position.get(outline).copied();
            if owner[i].is_none() {
                grouped[i] = group_name.get(outline.as_str()).map(|n| n.to_string());
            }
            continue;
        }
        let p = interior(&part.outer);
        let near = grid.get(&(cell(p.0), cell(p.1))).into_iter().flatten().chain(big.iter());
        let hit = near.copied().find(|&k| {
            let o = &found.buildings[k];
            inside(&o.outer, p) && !o.holes.iter().any(|h| inside(h, p))
        });
        if let Some(k) = hit {
            owner[i] = Some(k);
            found.buildings[i].part_of = found.buildings[k].osm_id.clone();
        }
    }
    // A part without a name has its outline's, or its group's.
    for (i, k) in owner.iter().enumerate() {
        let name = match k {
            Some(k) => found.buildings[*k].name.clone(),
            None => grouped[i].clone().unwrap_or_default(),
        };
        named[i] = name;
    }
    for (i, name) in named.into_iter().enumerate() {
        if found.buildings[i].name.is_empty() && found.buildings[i].is_part {
            found.buildings[i].name = name;
        }
    }
    // An outline is replaced by its parts when a relation says it is theirs, or they cover half of it; a column or a roof slab is
    // no part of its walls.
    let mut parts_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, k) in owner.iter().enumerate() {
        if let Some(k) = k {
            parts_of.entry(*k).or_default().push(i);
        }
    }
    let mut drop: HashMap<usize, usize> = HashMap::new();
    for (k, parts) in &parts_of {
        let outline = &found.buildings[*k];
        let related = parts.iter().any(|i| by_relation.get(&base(&found.buildings[*i].osm_id)).is_some_and(|o| *o == outline.osm_id));
        let volumes: Vec<&Building> =
            parts.iter().map(|i| &found.buildings[*i]).filter(|b| b.kind != "column" && b.kind != "roof").collect();
        if related || coverage(outline, &volumes) >= 0.5 {
            drop.insert(*k, parts.len());
        } else {
            found.kept += 1;
        }
    }
    if drop.is_empty() {
        return;
    }
    let mut kept = Vec::with_capacity(found.buildings.len());
    for (i, b) in std::mem::take(&mut found.buildings).into_iter().enumerate() {
        match drop.get(&i) {
            Some(n) => {
                found.replaced += 1;
                found.skipped.push(Skipped { osm_id: b.osm_id, why: format!("replaced by its building parts ({n} parts)") });
            }
            None => kept.push(b),
        }
    }
    found.buildings = kept;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square of about 20 m by 20 m near São Paulo: its corners, longitude and latitude, closed.
    fn square(lon: f64, lat: f64, size: f64) -> Vec<LonLat> {
        let (dx, dy) = (size * 0.000_01, size * 0.000_009);
        vec![(lon, lat), (lon + dx, lat), (lon + dx, lat + dy), (lon, lat + dy), (lon, lat)]
    }

    fn geometry(points: &[LonLat]) -> String {
        points.iter().map(|(lon, lat)| format!(r#"{{"lat":{lat},"lon":{lon}}}"#)).collect::<Vec<_>>().join(",")
    }

    /// An Overpass `out geom` file of `elements`.
    fn file(elements: &[String]) -> String {
        format!(r#"{{"version":0.6,"generator":"Overpass API","elements":[{}]}}"#, elements.join(","))
    }

    fn way(id: i64, points: &[LonLat], tags: &str) -> String {
        format!(r#"{{"type":"way","id":{id},"geometry":[{}],"tags":{{{tags}}}}}"#, geometry(points))
    }

    fn found(elements: &[String]) -> Found {
        buildings(&parse(&file(elements)).unwrap(), &Options::default())
    }

    const HOUSE: LonLat = (-46.6333, -23.5505);

    #[test]
    fn a_closed_way_tagged_building_is_a_building_with_its_height() {
        let f = found(&[way(10, &square(HOUSE.0, HOUSE.1, 20.0), r#""building":"house","height":"12 m","name":"Casa""#)]);
        assert_eq!(f.buildings.len(), 1, "{:?}", f.skipped);
        let b = &f.buildings[0];
        assert_eq!((b.osm_id.as_str(), b.kind.as_str(), b.name.as_str()), ("way/10", "house", "Casa"));
        assert_eq!((b.height, b.min_height, b.outer.len(), b.holes.len()), (12000.0, 0.0, 4, 0), "the closing point is not repeated");
        assert_eq!(f.defaulted, 0);
    }

    #[test]
    fn height_comes_from_height_then_levels_then_the_default() {
        let at = |tags: &str| found(&[way(1, &square(HOUSE.0, HOUSE.1, 20.0), tags)]);
        assert_eq!(at(r#""building":"yes","height":"9""#).buildings[0].height, 9000.0);
        assert_eq!(at(r#""building":"yes","building:levels":"4""#).buildings[0].height, 12000.0);
        assert_eq!(at(r#""building":"yes","height":"10","building:levels":"4""#).buildings[0].height, 10000.0, "height first");
        let none = at(r#""building":"yes""#);
        assert_eq!((none.buildings[0].height, none.defaulted), (6000.0, 1));
        let bad = at(r#""building":"yes","height":"tall","building:levels":"two""#);
        assert_eq!((bad.buildings[0].height, bad.defaulted), (6000.0, 1), "what is not a length is not read");
        let o = Options { level_height: 3500.0, default_height: 4000.0, ..Options::default() };
        let osm = parse(&file(&[way(1, &square(HOUSE.0, HOUSE.1, 20.0), r#""building":"yes","building:levels":"4""#)])).unwrap();
        assert_eq!(buildings(&osm, &o).buildings[0].height, 14000.0);
    }

    #[test]
    fn lengths_are_read_in_the_units_osm_uses() {
        for (text, mm) in [
            ("12", 12000.0),
            ("12.5", 12500.0),
            ("12,5", 12500.0),
            (" 12 m ", 12000.0),
            ("12m", 12000.0),
            ("40 ft", 12192.0),
            ("40'", 12192.0),
            ("40' 6\"", 12344.4),
            ("0.5", 500.0),
        ] {
            assert!(length(text).is_some_and(|v| (v - mm).abs() < 0.01), "{text}: {:?}", length(text));
        }
        for text in ["", "tall", "-3", "m", "12 parsecs", "1e999", "nan", "99999999999"] {
            assert_eq!(length(text), None, "{text}");
        }
    }

    #[test]
    fn a_part_lifted_off_the_ground_has_a_min_height() {
        let tags = r#""building:part":"yes","height":"20","min_height":"8""#;
        let osm = parse(&file(&[way(1, &square(HOUSE.0, HOUSE.1, 20.0), tags)])).unwrap();
        let none = buildings(&osm, &Options { parts: false, ..Options::default() });
        assert!(none.buildings.is_empty() && none.skipped[0].why.contains("part"), "{:?}", none.skipped);
        let with = buildings(&osm, &Options { parts: true, ..Options::default() });
        assert_eq!((with.buildings[0].height, with.buildings[0].min_height), (20000.0, 8000.0));
        let by_level = r#""building:part":"yes","building:levels":"6","building:min_level":"2""#;
        let osm = parse(&file(&[way(1, &square(HOUSE.0, HOUSE.1, 20.0), by_level)])).unwrap();
        let b = &buildings(&osm, &Options { parts: true, ..Options::default() }).buildings[0];
        assert_eq!((b.height, b.min_height), (18000.0, 6000.0));
    }

    #[test]
    fn a_multipolygon_joins_its_outer_ways_and_holds_its_courtyard() {
        // The outer ring in two ways, one of them running the other way; a courtyard.
        let outer = square(HOUSE.0, HOUSE.1, 40.0);
        let (a, b) = (outer[..3].to_vec(), outer[2..].to_vec());
        let mut b_reversed = b.clone();
        b_reversed.reverse();
        let court = square(HOUSE.0 + 0.00015, HOUSE.1 + 0.00013, 8.0);
        let rel = format!(
            r#"{{"type":"relation","id":77,"members":[
                {{"type":"way","ref":1,"role":"outer","geometry":[{}]}},
                {{"type":"way","ref":2,"role":"outer","geometry":[{}]}},
                {{"type":"way","ref":3,"role":"inner","geometry":[{}]}}],
               "tags":{{"type":"multipolygon","building":"apartments","height":"30"}}}}"#,
            geometry(&a),
            geometry(&b_reversed),
            geometry(&court)
        );
        let f = found(&[rel]);
        assert_eq!(f.buildings.len(), 1, "{:?}", f.skipped);
        let b = &f.buildings[0];
        assert_eq!((b.osm_id.as_str(), b.outer.len(), b.holes.len(), b.holes[0].len()), ("relation/77", 4, 1, 4));
        assert_eq!(b.height, 30000.0);
        // Two outlines are two buildings; one that does not close is left out.
        let far = square(HOUSE.0 + 0.01, HOUSE.1, 40.0);
        let two = format!(
            r#"{{"type":"relation","id":78,"members":[{{"type":"way","ref":1,"role":"outer","geometry":[{}]}},{{"type":"way","ref":2,"role":"outer","geometry":[{}]}}],"tags":{{"type":"multipolygon","building":"yes"}}}}"#,
            geometry(&outer),
            geometry(&far)
        );
        let f = found(&[two]);
        assert_eq!(f.buildings.iter().map(|b| b.osm_id.as_str()).collect::<Vec<_>>(), ["relation/78", "relation/78#2"]);
        let open = format!(
            r#"{{"type":"relation","id":79,"members":[{{"type":"way","ref":1,"role":"outer","geometry":[{}]}}],"tags":{{"type":"multipolygon","building":"yes"}}}}"#,
            geometry(&outer[..3])
        );
        let f = found(&[open]);
        assert!(f.buildings.is_empty() && f.skipped[0].why.contains("close"), "{:?}", f.skipped);
    }

    #[test]
    fn what_cannot_be_a_building_is_counted_with_why() {
        let sq = square(HOUSE.0, HOUSE.1, 20.0);
        let bowtie = vec![
            (HOUSE.0, HOUSE.1),
            (HOUSE.0 + 0.0002, HOUSE.1 + 0.0002),
            (HOUSE.0 + 0.0002, HOUSE.1),
            (HOUSE.0, HOUSE.1 + 0.0002),
            (HOUSE.0, HOUSE.1),
        ];
        let off = vec![(HOUSE.0, 95.0), (HOUSE.0 + 0.1, 95.0), (HOUSE.0 + 0.1, 96.0), (HOUSE.0, 95.0)];
        let f = found(&[
            way(1, &sq[..4], r#""building":"yes""#),
            way(2, &sq[..2], r#""building":"yes""#),
            way(3, &bowtie, r#""building":"yes""#),
            way(4, &sq, r#""building":"no""#),
            way(5, &sq, r#""highway":"residential""#),
            way(6, &off, r#""building":"yes""#),
            way(7, &sq, r#""building":"yes","height":"3","min_height":"5""#),
            way(8, &sq, r#""building":"yes""#),
        ]);
        let why = |id: &str| f.skipped.iter().find(|s| s.osm_id == id).map(|s| s.why.as_str()).unwrap_or("not skipped");
        assert!(why("way/1").contains("closed"), "{}", why("way/1"));
        assert!(why("way/2").contains("closed") || why("way/2").contains("corners"), "{}", why("way/2"));
        assert!(why("way/3").contains("cross"), "{}", why("way/3"));
        assert_eq!(why("way/4"), "not skipped", "building=no is not a building, and not worth a word");
        assert_eq!(why("way/5"), "not skipped", "a road is not asked about");
        assert!(why("way/6").contains("Earth"), "{}", why("way/6"));
        assert!(why("way/7").contains("min_height"), "{}", why("way/7"));
        assert_eq!(f.buildings.iter().map(|b| b.osm_id.as_str()).collect::<Vec<_>>(), ["way/8"]);
    }

    #[test]
    fn a_box_keeps_the_buildings_with_a_corner_in_it() {
        let near = way(1, &square(HOUSE.0, HOUSE.1, 20.0), r#""building":"yes""#);
        let far = way(2, &square(HOUSE.0 + 0.5, HOUSE.1, 20.0), r#""building":"yes""#);
        let osm = parse(&file(&[near, far])).unwrap();
        let o = Options { bbox: Some([HOUSE.1 - 0.001, HOUSE.0 - 0.001, HOUSE.1 + 0.001, HOUSE.0 + 0.001]), ..Options::default() };
        let f = buildings(&osm, &o);
        assert_eq!(f.buildings.iter().map(|b| b.osm_id.as_str()).collect::<Vec<_>>(), ["way/1"]);
        assert!(f.skipped.is_empty(), "outside the box is not a failure: {:?}", f.skipped);
    }

    /// The same two buildings as `out geom` JSON, plain JSON (ways before their nodes) and XML.
    fn same_in_three_forms() -> (String, String, String) {
        let (a, b) = (square(HOUSE.0, HOUSE.1, 20.0), square(HOUSE.0 + 0.001, HOUSE.1, 30.0));
        let geom = file(&[way(1, &a, r#""building":"yes","height":"7""#), way(2, &b, r#""building":"house","building:levels":"2""#)]);
        let node = |id: i64, p: LonLat| format!(r#"{{"type":"node","id":{id},"lat":{},"lon":{}}}"#, p.1, p.0);
        let (mut nodes, mut refs) = (Vec::new(), Vec::new());
        for (w, ring) in [&a, &b].into_iter().enumerate() {
            let mut ids = Vec::new();
            for (i, p) in ring[..4].iter().enumerate() {
                let id = 100 * (w as i64 + 1) + i as i64;
                nodes.push(node(id, *p));
                ids.push(id);
            }
            ids.push(ids[0]);
            refs.push(ids);
        }
        let list = |ids: &Vec<i64>| ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let plain = file(&[
            format!(r#"{{"type":"way","id":1,"nodes":[{}],"tags":{{"building":"yes","height":"7"}}}}"#, list(&refs[0])),
            format!(r#"{{"type":"way","id":2,"nodes":[{}],"tags":{{"building":"house","building:levels":"2"}}}}"#, list(&refs[1])),
            nodes.join(","),
        ]);
        let xml_nodes: String = [&a, &b]
            .iter()
            .enumerate()
            .flat_map(|(w, ring)| {
                ring[..4].iter().enumerate().map(move |(i, p)| format!(r#"<node id="{}" lat="{}" lon="{}"/>"#, 100 * (w + 1) + i, p.1, p.0))
            })
            .collect();
        let nd = |ids: &Vec<i64>| ids.iter().map(|i| format!(r#"<nd ref="{i}"/>"#)).collect::<String>();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><osm version="0.6" generator="Overpass API">{xml_nodes}
               <way id="1">{}<tag k="building" v="yes"/><tag k="height" v="7"/></way>
               <way id="2">{}<tag k="building" v="house"/><tag k="building:levels" v="2"/></way></osm>"#,
            nd(&refs[0]),
            nd(&refs[1])
        );
        (geom, plain, xml)
    }

    #[test]
    fn json_with_geometry_json_with_nodes_and_xml_give_the_same_buildings() {
        let (geom, plain, xml) = same_in_three_forms();
        let of = |text: &str| buildings(&parse(text).unwrap(), &Options::default());
        let (g, p, x) = (of(&geom), of(&plain), of(&xml));
        assert_eq!(g.buildings.len(), 2, "{:?}", g.skipped);
        for other in [&p, &x] {
            assert!(other.skipped.is_empty(), "{:?}", other.skipped);
            assert_eq!(other.buildings.len(), 2);
            for (a, b) in g.buildings.iter().zip(&other.buildings) {
                assert_eq!((&a.osm_id, a.height, &a.kind), (&b.osm_id, b.height, &b.kind));
                assert!(a.outer.iter().zip(&b.outer).all(|(p, q)| (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9));
            }
        }
        assert_eq!((g.buildings[1].height, g.buildings[1].levels.as_str()), (6000.0, "2"));
    }

    #[test]
    fn xml_with_geometry_on_its_nd_elements_is_read_too() {
        let ring = square(HOUSE.0, HOUSE.1, 20.0);
        let nds: String = ring.iter().map(|p| format!(r#"<nd ref="1" lat="{}" lon="{}"/>"#, p.1, p.0)).collect();
        let xml = format!(r#"<osm><way id="5">{nds}<tag k="building" v="yes"/></way></osm>"#);
        let f = buildings(&parse(&xml).unwrap(), &Options::default());
        assert_eq!(f.buildings.len(), 1, "{:?}", f.skipped);
    }

    #[test]
    fn a_file_that_is_not_osm_is_refused_with_why() {
        for (text, says) in [
            ("", "neither"),
            ("hello", "neither"),
            ("{", "JSON"),
            (r#"{"elements": 5}"#, "elements"),
            (r#"{"version":0.6}"#, "elements"),
            ("<html></html>", "osm"),
            ("<osm><way", "XML"),
            (r#"<!DOCTYPE osm [<!ENTITY a "aaaa"><!ENTITY b "&a;&a;&a;&a;">]><osm>&b;</osm>"#, "XML"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(says), "{text:?}: {e}");
        }
        assert!(parse("\u{feff}  {\"elements\": []}").is_ok(), "a byte order mark and spaces first");
        assert!(parse(r#"<osm></osm>"#).unwrap().nodes.is_empty());
    }

    #[test]
    fn hostile_values_in_a_file_are_left_out_not_trusted() {
        // Out-of-range and missing coordinates, a node that is not there, ids that are not numbers.
        let ring = square(HOUSE.0, HOUSE.1, 20.0);
        let text = file(&[
            r#"{"type":"node","id":"x","lat":1,"lon":2}"#.into(),
            r#"{"type":"node","id":1,"lat":999,"lon":2}"#.into(),
            r#"{"type":"way","id":9,"nodes":[1,2,3,4,1],"tags":{"building":"yes"}}"#.into(),
            r#"{"type":"way","id":"y","geometry":[],"tags":{"building":"yes"}}"#.into(),
            r#"{"type":"way","id":10,"geometry":[null,null,null,null],"tags":{"building":"yes"}}"#.into(),
            r#"{"type":"relation","id":11,"members":"x","tags":{"building":"yes"}}"#.into(),
            r#"{"type":"relation","id":12,"members":[{"type":"way"},{"ref":"a"},5],"tags":{"building":"yes","type":"multipolygon"}}"#
                .into(),
            r#"{"type":"way","id":13,"tags":{"building":["yes"]}}"#.into(),
            way(14, &ring, &format!(r#""building":"yes","name":"{}","height":"{}""#, "n".repeat(10_000), "9".repeat(10_000))),
        ]);
        let f = buildings(&parse(&text).unwrap(), &Options::default());
        assert_eq!(f.buildings.len(), 1, "{:?}", f.skipped);
        assert!(f.buildings[0].name.chars().count() <= 256, "tag text is cut");
        assert_eq!(f.buildings[0].height, 6000.0, "an absurd height is not read");
        assert!(f.skipped.len() >= 3, "{:?}", f.skipped);
    }

    #[test]
    fn a_ring_with_too_many_corners_to_check_is_left_out_and_many_buildings_are_capped() {
        let n = MAX_RING_CHECK + 50;
        let big: Vec<LonLat> = (0..=n)
            .map(|i| {
                let a = (i % n) as f64 / n as f64 * std::f64::consts::TAU;
                (HOUSE.0 + a.cos() * 0.001, HOUSE.1 + a.sin() * 0.001)
            })
            .collect();
        let f = found(&[way(1, &big, r#""building":"yes""#)]);
        assert!(f.buildings.is_empty() && f.skipped[0].why.contains("corners"), "{:?}", f.skipped);
        // More buildings than an import makes: the first ones, and a word about the rest.
        let many: Vec<String> =
            (0..MAX_BUILDINGS as i64 + 5).map(|i| way(i + 1, &square(HOUSE.0, HOUSE.1, 20.0), r#""building":"yes""#)).collect();
        let f = found(&many);
        assert_eq!(f.buildings.len(), MAX_BUILDINGS);
        assert!(f.skipped.iter().any(|s| s.why.contains("more than")), "{:?}", f.skipped.last());
    }

    // ---- Simple 3D Buildings: roof tags, the facade and the roof, parts ----

    fn one(tags: &str, o: &Options) -> Building {
        let osm = parse(&file(&[way(1, &square(HOUSE.0, HOUSE.1, 20.0), tags)])).unwrap();
        let f = buildings(&osm, o);
        assert_eq!(f.buildings.len(), 1, "{:?}", f.skipped);
        f.buildings[0].clone()
    }

    #[test]
    fn the_roof_is_read_and_the_facade_is_the_height_less_the_roof() {
        let o = Options::default();
        let b = one(
            r##""building":"house","height":"12","roof:height":"3","roof:shape":"gabled","roof:orientation":"across","roof:direction":"90","roof:angle":"35","building:colour":"#aabbcc","building:material":"brick","roof:colour":"red","roof:material":"tile""##,
            &o,
        );
        assert_eq!((b.height, b.roof_height), (9000.0, 3000.0), "the mass is the facade");
        assert_eq!((b.roof_shape.as_str(), b.roof_orientation.as_str(), b.roof_direction.as_str()), ("gabled", "across", "90"));
        assert_eq!(b.roof_angle, Some(35.0));
        assert_eq!(
            (b.colour.as_str(), b.material.as_str(), b.roof_colour.as_str(), b.roof_material.as_str()),
            ("#aabbcc", "brick", "red", "tile")
        );
        // No roof tag: the facade is the whole height, as before.
        let b = one(r#""building":"yes","height":"12""#, &o);
        assert_eq!((b.height, b.roof_height, b.roof_shape.as_str(), b.roof_angle), (12000.0, 0.0, "", None));
        // A pitched roof with the levels given: the roof is what the height has over the storeys.
        let b = one(r#""building":"yes","height":"12","building:levels":"3","roof:shape":"hipped""#, &o);
        assert_eq!((b.height, b.roof_height), (9000.0, 3000.0));
        // A flat (or untold) roof takes no share of the height for its levels.
        let b = one(r#""building":"yes","height":"12","building:levels":"3","roof:shape":"flat""#, &o);
        assert_eq!((b.height, b.roof_height), (12000.0, 0.0));
        let b = one(r#""building":"yes","height":"12","building:levels":"3""#, &o);
        assert_eq!((b.height, b.roof_height), (12000.0, 0.0));
        // Levels but no height: the storeys are the facade and the roof comes on top (roof:height, else roof:levels).
        let b = one(r#""building":"yes","building:levels":"3","roof:shape":"gabled","roof:height":"2.5""#, &o);
        assert_eq!((b.height, b.roof_height), (9000.0, 2500.0));
        let b = one(r#""building":"yes","building:levels":"3","roof:shape":"gabled","roof:levels":"1""#, &o);
        assert_eq!((b.height, b.roof_height), (9000.0, 3000.0));
        // A roof as tall as the building, or taller, is not believed: the building is all facade.
        let b = one(r#""building":"yes","height":"4","roof:height":"4""#, &o);
        assert_eq!((b.height, b.roof_height), (4000.0, 0.0));
        let b = one(r#""building":"yes","height":"4","min_height":"2","roof:height":"2.5""#, &o);
        assert_eq!((b.height, b.roof_height), (4000.0, 0.0), "the facade must stay above what the building is lifted by");
    }

    #[test]
    fn hostile_roof_tags_are_left_out_not_trusted() {
        let o = Options::default();
        let b = one(
            r#""building":"yes","height":"12","roof:height":"1e999","roof:angle":"200","roof:orientation":"sideways","roof:levels":"-4","roof:shape":"""#,
            &o,
        );
        assert_eq!(
            (b.height, b.roof_height, b.roof_angle, b.roof_orientation.as_str(), b.roof_shape.as_str()),
            (12000.0, 0.0, None, "", "")
        );
        for angle in ["0", "-5", "90", "nan", "steep", ""] {
            let b = one(&format!(r#""building":"yes","roof:angle":"{angle}""#), &o);
            assert_eq!(b.roof_angle, None, "{angle}");
        }
        let long = "x".repeat(5000);
        let b = one(&format!(r#""building":"yes","roof:shape":"{long}","building:colour":"{long}""#), &o);
        assert!(b.roof_shape.chars().count() <= 256 && b.colour.chars().count() <= 256);
        // The shape is read in lower case, as the scheme writes it.
        assert_eq!(one(r#""building":"yes","roof:shape":"Gabled ""#, &o).roof_shape, "gabled");
    }

    fn with_parts() -> Options {
        Options { parts: true, ..Options::default() }
    }

    /// The left and right halves of a 40 m by 20 m outline, as parts that share its sides.
    fn halves() -> (Vec<LonLat>, Vec<LonLat>, Vec<LonLat>) {
        let (dx, dy) = (40.0 * 0.000_01, 20.0 * 0.000_009);
        let (x, y) = HOUSE;
        let rect = |x0: f64, x1: f64| vec![(x0, y), (x1, y), (x1, y + dy), (x0, y + dy), (x0, y)];
        (rect(x, x + dx), rect(x, x + dx / 2.0), rect(x + dx / 2.0, x + dx))
    }

    #[test]
    fn an_outline_with_parts_inside_it_is_replaced_by_them() {
        let (outline, left, right) = halves();
        let osm = parse(&file(&[
            way(1, &outline, r#""building":"yes","name":"Hall""#),
            way(2, &left, r#""building:part":"yes","height":"8""#),
            way(3, &right, r#""building:part":"yes","height":"14","roof:shape":"gabled""#),
        ]))
        .unwrap();
        let f = buildings(&osm, &with_parts());
        let ids: Vec<&str> = f.buildings.iter().map(|b| b.osm_id.as_str()).collect();
        assert_eq!(ids, ["way/2", "way/3"], "{:?}", f.skipped);
        assert!(f.buildings.iter().all(|b| b.part_of == "way/1"), "each part says whose it is");
        assert_eq!(f.replaced, 1);
        let why = &f.skipped.iter().find(|s| s.osm_id == "way/1").expect("the outline is reported").why;
        assert!(why.starts_with("replaced by its building parts"), "{why}");
        // The outline's own attributes are not lost: the parts carry the name if they have none.
        assert!(f.buildings.iter().all(|b| b.name == "Hall"));
        // Without parts wanted the outline stays, and the parts are the skipped ones.
        let f = buildings(&osm, &Options { parts: false, ..Options::default() });
        assert_eq!(f.buildings.len(), 1);
        assert_eq!((f.replaced, f.buildings[0].osm_id.as_str(), f.buildings[0].part_of.as_str()), (0, "way/1", ""));
    }

    #[test]
    fn a_part_elsewhere_does_not_replace_an_outline_and_a_part_in_a_courtyard_neither() {
        let (outline, _, _) = halves();
        let far = square(HOUSE.0 + 0.01, HOUSE.1, 10.0);
        let osm = parse(&file(&[way(1, &outline, r#""building":"yes""#), way(2, &far, r#""building:part":"yes","height":"5""#)])).unwrap();
        let f = buildings(&osm, &with_parts());
        assert_eq!(f.buildings.len(), 2, "{:?}", f.skipped);
        assert!(f.buildings.iter().all(|b| b.part_of.is_empty()) && f.replaced == 0);
        // A part in the courtyard of a multipolygon outline is not in the outline.
        let outer = square(HOUSE.0, HOUSE.1, 60.0);
        let hole = square(HOUSE.0 + 0.00002, HOUSE.1 + 0.000_02, 20.0);
        let inner_part = square(HOUSE.0 + 0.000_03, HOUSE.1 + 0.000_03, 6.0);
        let rel = format!(
            r#"{{"type":"relation","id":9,"tags":{{"type":"multipolygon","building":"yes"}},"members":[{{"type":"way","ref":1,"role":"outer","geometry":[{}]}},{{"type":"way","ref":2,"role":"inner","geometry":[{}]}}]}}"#,
            geometry(&outer),
            geometry(&hole)
        );
        let osm = parse(&file(&[rel, way(5, &inner_part, r#""building:part":"yes","height":"5""#)])).unwrap();
        let f = buildings(&osm, &with_parts());
        assert_eq!(f.buildings.len(), 2, "{:?}", f.skipped);
        assert_eq!(f.replaced, 0);
    }

    #[test]
    fn a_building_relation_groups_an_outline_with_its_parts_wherever_they_lie() {
        let (outline, left, _) = halves();
        // The part is next door: only the relation says it belongs.
        let next_door = square(HOUSE.0 + 0.001, HOUSE.1, 10.0);
        let rel = r#"{"type":"relation","id":7,"tags":{"type":"building"},"members":[{"type":"way","ref":1,"role":"outline"},{"type":"way","ref":2,"role":"part"},{"type":"way","ref":3,"role":"part"}]}"#;
        let osm = parse(&file(&[
            way(1, &outline, r#""building":"yes""#),
            way(2, &left, r#""building:part":"yes","height":"8""#),
            way(3, &next_door, r#""building:part":"yes","height":"4""#),
            rel.to_string(),
        ]))
        .unwrap();
        let f = buildings(&osm, &with_parts());
        let ids: Vec<&str> = f.buildings.iter().map(|b| b.osm_id.as_str()).collect();
        assert_eq!(ids, ["way/2", "way/3"], "{:?}", f.skipped);
        assert!(f.buildings.iter().all(|b| b.part_of == "way/1"), "{:?}", f.buildings.iter().map(|b| &b.part_of).collect::<Vec<_>>());
        assert_eq!(f.replaced, 1);
        // The relation itself is a grouping, not a building and not a skipped one.
        assert!(f.skipped.iter().all(|s| s.osm_id != "relation/7"), "{:?}", f.skipped);
    }

    #[test]
    fn many_outlines_and_parts_are_matched_without_comparing_every_pair() {
        // 2,000 outlines with a part each, in a grid: the match is by place, not by trying every pair.
        let mut elements = Vec::new();
        for i in 0..2000i64 {
            let (x, y) = (HOUSE.0 + (i % 50) as f64 * 0.0005, HOUSE.1 + (i / 50) as f64 * 0.0005);
            elements.push(way(2 * i + 1, &square(x, y, 20.0), r#""building":"yes""#));
            elements.push(way(2 * i + 2, &square(x, y, 20.0), r#""building:part":"yes","height":"5""#));
        }
        let osm = parse(&file(&elements)).unwrap();
        let start = std::time::Instant::now();
        let f = buildings(&osm, &with_parts());
        assert_eq!((f.buildings.len(), f.replaced), (2000, 2000));
        assert!(start.elapsed().as_secs_f64() < 5.0, "{:?}", start.elapsed());
    }

    // ---- 7b: the bell tower: columns, roof slabs, a kiosk that is its own part ----

    /// A circle of `n` corners round (`lon`, `lat`), `size` metres across, closed.
    fn disc(lon: f64, lat: f64, size: f64, n: usize) -> Vec<LonLat> {
        let mut ring: Vec<LonLat> = (0..n)
            .map(|i| {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                (lon + size / 2.0 * 0.000_01 * a.cos(), lat + size / 2.0 * 0.000_009 * a.sin())
            })
            .collect();
        ring.push(ring[0]);
        ring
    }

    fn tower() -> String {
        let mut elements = vec![
            way(1, &disc(HOUSE.0, HOUSE.1, 1.0, 12), r#""building":"kiosk","building:part":"yes","height":"4","material":"concrete""#),
            way(2, &disc(HOUSE.0, HOUSE.1, 1.6, 12), r#""building:part":"roof","height":"6","roof:height":"0.05","roof:shape":"flat""#),
        ];
        for k in 0..7 {
            let (dx, dy) = (0.2 * f64::from(k).cos(), 0.2 * f64::from(k).sin());
            elements.push(way(
                10 + i64::from(k),
                &disc(HOUSE.0 + dx * 0.000_01, HOUSE.1 + dy * 0.000_009, 0.05, 8),
                r#""man_made":"column","column:shape":"circle","height":"6","min_height":"4""#,
            ));
        }
        file(&elements)
    }

    #[test]
    fn a_kiosk_tagged_as_its_own_part_a_roof_over_it_and_columns_are_all_kept() {
        let f = buildings(&parse(&tower()).unwrap(), &Options::default());
        assert_eq!(f.buildings.len(), 9, "{:?}", f.skipped);
        assert_eq!((f.replaced, f.kept), (0, 0));
        let kiosk = f.buildings.iter().find(|b| b.osm_id == "way/1").unwrap();
        assert_eq!((kiosk.kind.as_str(), kiosk.is_part, kiosk.height, kiosk.min_height), ("kiosk", true, 4000.0, 0.0));
        let columns: Vec<&Building> = f.buildings.iter().filter(|b| b.kind == "column").collect();
        assert_eq!(columns.len(), 7);
        assert!(columns.iter().all(|c| c.is_part && c.height == 6000.0 && c.min_height == 4000.0));
        let roof = f.buildings.iter().find(|b| b.osm_id == "way/2").unwrap();
        assert_eq!(
            (roof.kind.as_str(), roof.height, roof.min_height, roof.roof_height),
            ("roof", 6000.0, 5950.0, 0.0),
            "a slab, as thick as the roof is"
        );
        // With parts left out the kiosk is the building, and the roof and columns are not read.
        let f = buildings(&parse(&tower()).unwrap(), &Options { parts: false, ..Options::default() });
        assert_eq!(f.buildings.len(), 1);
        assert_eq!((f.buildings[0].osm_id.as_str(), f.buildings[0].is_part), ("way/1", false));
    }

    #[test]
    fn a_roof_part_is_a_slab_at_the_top_or_at_the_eaves_not_a_volume() {
        let o = Options::default();
        let part = |tags: &str| {
            let osm = parse(&file(&[way(1, &square(HOUSE.0, HOUSE.1, 20.0), tags)])).unwrap();
            buildings(&osm, &o).buildings.remove(0)
        };
        let b = part(r#""building:part":"roof","height":"6""#);
        assert_eq!((b.height, b.min_height, b.roof_height), (6000.0, 5900.0, 0.0), "100 mm thick by default");
        let b = part(r#""building:part":"roof","height":"6","roof:height":"0.3""#);
        assert_eq!((b.height, b.min_height), (6000.0, 5700.0), "a flat roof is as thick as roof:height says");
        // A pitched roof: the slab is at the eaves and the roof stands on it.
        let b = part(r#""building:part":"roof","height":"6","roof:height":"2","roof:shape":"cone""#);
        assert_eq!((b.height, b.min_height, b.roof_height, b.roof_shape.as_str()), (4000.0, 3900.0, 2000.0, "cone"));
        // With a min_height it is the slab between the two, as mapped.
        let b = part(r#""building:part":"roof","height":"17.1","min_height":"16.9""#);
        assert_eq!((b.height, b.min_height), (17100.0, 16900.0));
        // Any other part keeps its walls from the ground.
        let b = part(r#""building:part":"yes","height":"6""#);
        assert_eq!((b.height, b.min_height, b.kind.as_str()), (6000.0, 0.0, "part"));
        assert_eq!(part(r#""building:part":"corridor","height":"3""#).kind, "corridor");
    }

    #[test]
    fn an_outline_is_left_out_only_when_its_parts_cover_it() {
        let (outline, left, right) = halves();
        let small = square(HOUSE.0 + 0.00005, HOUSE.1 + 0.00002, 5.0);
        let at = |parts: &[(i64, &Vec<LonLat>, &str)]| {
            let mut e = vec![way(1, &outline, r#""building":"yes","name":"Hall""#)];
            e.extend(parts.iter().map(|(id, r, tags)| way(*id, r, tags)));
            buildings(&parse(&file(&e)).unwrap(), &Options::default())
        };
        // One small part inside: the house stays, and the part is a mass of its own.
        let f = at(&[(2, &small, r#""building:part":"yes","height":"9""#)]);
        assert_eq!((f.buildings.len(), f.replaced, f.kept), (2, 0, 1), "{:?}", f.skipped);
        assert!(f.buildings.iter().any(|b| b.osm_id == "way/1" && !b.is_part));
        // Parts that cover it replace it.
        let f = at(&[(2, &left, r#""building:part":"yes","height":"8""#), (3, &right, r#""building:part":"yes","height":"14""#)]);
        assert_eq!((f.buildings.len(), f.replaced, f.kept), (2, 1, 0));
        // A roof slab over the whole of it, or columns, are not its walls: it stays.
        let f = at(&[(2, &outline, r#""building:part":"roof","height":"9","roof:height":"0.2""#)]);
        assert_eq!((f.buildings.len(), f.replaced, f.kept), (2, 0, 1), "{:?}", f.skipped);
    }
}
