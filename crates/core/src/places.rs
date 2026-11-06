//! Places: countries and cities, found by name offline, each paired with the map systems surveys there are in, so a map
//! system can be found by where the project is ([coordinates](../../../docs/areas/coordinates.md)).
//!
//! The places are Natural Earth's (public domain): its countries, with their names in a few languages, and its populated
//! places, biggest first (`assets/map/places.tsv`). Which systems a country's surveys use is ours: its national grid, the UTM
//! zone (or Gauss-Krüger band) of the place on the country's datum, and last the zone on WGS 84, which is anywhere's.

use std::collections::HashMap;
use std::sync::OnceLock;

use bimcraft_i18n::ui_msg;
use serde_json::{Value, json};

use crate::crs::{self, Info};

/// Whether a place is a country or a city.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Country,
    City,
}

/// A place found.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub kind: Kind,
    pub name: String,
    /// A city's state or province; empty for a country.
    pub region: String,
    pub country: String,
    /// ISO 3166-1 alpha-2.
    pub iso: String,
    /// WGS84 degrees: the city, or the middle of the country's label.
    pub lon: f64,
    pub lat: f64,
}

impl Place {
    /// "São Paulo, São Paulo, Brazil"; "Brazil".
    pub fn label(&self) -> String {
        let mut parts = vec![self.name.as_str()];
        if self.kind == Kind::City {
            if !self.region.is_empty() && self.region != self.name {
                parts.push(&self.region);
            }
            parts.push(&self.country);
        }
        parts.join(", ")
    }

    /// The place and its systems, as the `crs` command lists it.
    pub fn json(&self) -> Value {
        let systems: Vec<Value> = systems(self)
            .iter()
            .map(|s| {
                let mut v = s.info.json();
                v["why"] = json!(s.why);
                v
            })
            .collect();
        json!({
            "kind": if self.kind == Kind::Country { "country" } else { "city" },
            "name": self.name, "region": self.region, "country": self.country, "iso": self.iso,
            "longitude": self.lon, "latitude": self.lat, "systems": systems,
        })
    }
}

/// A map system for a place, and why: the first is the one to pick.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub info: Info,
    /// An English interface label (`ui_msg!`): "National grid", ….
    pub why: &'static str,
}

struct Country {
    iso: String,
    lon: f64,
    lat: f64,
    /// Its names, English first.
    names: Vec<String>,
    folded: Vec<String>,
}

struct City {
    iso: String,
    lon: f64,
    lat: f64,
    name: String,
    region: String,
    /// Its name and its ASCII spelling, folded.
    folded: String,
    /// Its region and its country's names, folded: words a search may add.
    context: String,
}

struct Data {
    countries: Vec<Country>,
    cities: Vec<City>,
}

/// Lower case, accents off, anything not a letter or a digit a space: "São Paulo" is "sao paulo".
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'æ' => "ae",
            'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => "c",
            'ď' | 'đ' | 'ð' => "d",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'ğ' | 'ģ' => "g",
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
            'ķ' => "k",
            'ł' | 'ľ' | 'ļ' | 'ĺ' => "l",
            'ñ' | 'ń' | 'ň' | 'ņ' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
            'œ' => "oe",
            'ř' | 'ŕ' => "r",
            'ś' | 'š' | 'ş' | 'ș' => "s",
            'ß' => "ss",
            'ť' | 'ţ' | 'ț' => "t",
            'þ' => "th",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
            'ý' | 'ÿ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            '\'' | '’' => "",
            c if c.is_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => " ",
        };
        out.push_str(plain);
    }
    out
}

fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| read(include_str!("../../../assets/map/places.tsv")))
}

/// The table, a line a place: `C iso lon lat names|…` or `P iso lon lat name ascii region`, tab-separated; anything else is
/// left out.
fn read(text: &str) -> Data {
    let mut countries = Vec::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (Some(lon), Some(lat)) = (f.get(2).and_then(|v| v.parse::<f64>().ok()), f.get(3).and_then(|v| v.parse::<f64>().ok())) else {
            continue;
        };
        if !(lon.is_finite() && lat.is_finite() && lon.abs() <= 180.0 && lat.abs() <= 90.0) {
            continue;
        }
        let iso = f.get(1).copied().unwrap_or("").to_string();
        match (f.first().copied(), f.get(4)) {
            (Some("C"), Some(names)) => {
                let names: Vec<String> = names.split('|').filter(|n| !n.is_empty()).map(String::from).collect();
                if !names.is_empty() {
                    let folded = names.iter().map(|n| fold(n)).collect();
                    countries.push(Country { iso, lon, lat, names, folded });
                }
            }
            (Some("P"), Some(name)) if !name.is_empty() => {
                let ascii = f.get(5).copied().unwrap_or("");
                rows.push((iso, lon, lat, name.to_string(), fold(&format!("{name} {ascii}")), f.get(6).copied().unwrap_or("").to_string()));
            }
            _ => {}
        }
    }
    let names_of: HashMap<&str, String> = countries.iter().map(|c| (c.iso.as_str(), c.folded.join(" "))).collect();
    let cities = rows
        .into_iter()
        .map(|(iso, lon, lat, name, folded, region)| {
            let context = format!("{} {}", fold(&region), names_of.get(iso.as_str()).map_or("", String::as_str));
            City { iso, lon, lat, name, region, folded, context }
        })
        .collect();
    Data { countries, cities }
}

fn country_name(iso: &str) -> String {
    data().countries.iter().find(|c| c.iso == iso).and_then(|c| c.names.first()).cloned().unwrap_or_else(|| iso.to_string())
}

/// Every word of `words` begins a word of `text`.
fn has_all(text: &str, words: &[&str]) -> bool {
    words.iter().all(|w| text.split_whitespace().any(|t| t.starts_with(w)))
}

/// How well `name` answers `query`: 0 exactly, 1 its beginning, 2 a word of it.
fn rank(name: &str, query: &str) -> u8 {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name == query {
        0
    } else if name.starts_with(query) {
        1
    } else {
        2
    }
}

/// The countries and cities `text` names, best first, up to `limit`: a country by any of its names, a city by its name
/// (accents aside), and words after it may name its state or country ("paulo brasil"). The biggest city first of those
/// matched as well.
pub fn find(text: &str, limit: usize) -> Vec<Place> {
    let query = fold(text);
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() || words.iter().all(|w| w.chars().all(|c| c.is_ascii_digit())) {
        return Vec::new();
    }
    let query = words.join(" ");
    let d = data();
    // (rank, a city after a country, the order of the table, the place).
    let mut hits: Vec<(u8, u8, usize, Place)> = Vec::new();
    for (i, c) in d.countries.iter().enumerate() {
        if let Some(best) = c.folded.iter().filter(|n| has_all(n, &words)).map(|n| rank(n, &query)).min() {
            let name = c.names.first().cloned().unwrap_or_default();
            let place = Place {
                kind: Kind::Country,
                name: name.clone(),
                region: String::new(),
                country: name,
                iso: c.iso.clone(),
                lon: c.lon,
                lat: c.lat,
            };
            hits.push((best, 0, i, place));
        }
    }
    for (i, c) in d.cities.iter().enumerate() {
        // One word at least is the city's, the rest its or its place's.
        let named = words.iter().any(|w| c.folded.split_whitespace().any(|t| t.starts_with(w)));
        if !named || !words.iter().all(|w| c.folded.split_whitespace().chain(c.context.split_whitespace()).any(|t| t.starts_with(w))) {
            continue;
        }
        let r = if has_all(&c.folded, &words) { rank(&c.folded, &query).min(rank(&fold(&c.name), &query)) } else { 2 };
        let place = Place {
            kind: Kind::City,
            name: c.name.clone(),
            region: c.region.clone(),
            country: country_name(&c.iso),
            iso: c.iso.clone(),
            lon: c.lon,
            lat: c.lat,
        };
        hits.push((r, 1, i, place));
        if hits.len() > limit.saturating_mul(20).max(200) {
            break;
        }
    }
    hits.sort_by_key(|h| (h.0, h.1, h.2));
    hits.into_iter().take(limit).map(|h| h.3).collect()
}

/// Every projected CRS by its name, for the systems named by pattern.
fn by_name() -> &'static HashMap<String, u32> {
    static NAMES: OnceLock<HashMap<String, u32>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut out = HashMap::new();
        for c in 0..=u16::MAX {
            if let Some(d) = crs_definitions::from_code(c)
                && let Some(rest) = d.wkt.strip_prefix("PROJCS[\"")
                && let Some(name) = rest.split('"').next()
            {
                out.entry(name.to_string()).or_insert(u32::from(c));
            }
        }
        out
    })
}

const NATIONAL: &str = ui_msg!("National grid");
const DATUM: &str = ui_msg!("UTM zone on the national datum");
const BAND: &str = ui_msg!("Zone on the national datum");
const WORLD: &str = ui_msg!("UTM zone on WGS 84");

/// A country's national grids, by EPSG code.
fn grids(iso: &str) -> &'static [u32] {
    match iso {
        "GB" => &[27700],
        "IE" => &[2157],
        "NL" => &[28992],
        "FR" => &[2154],
        "CH" | "LI" => &[2056],
        "BE" => &[3812],
        "PT" => &[3763],
        "PL" => &[2180],
        "SE" => &[3006],
        "FI" => &[3067],
        "CZ" | "SK" => &[5514],
        "AT" => &[31287],
        "IT" => &[6875],
        "GR" => &[2100],
        "HU" => &[23700],
        "IS" => &[3057],
        "EE" => &[3301],
        "LT" => &[3346],
        "LV" => &[3059],
        "SI" => &[3794],
        "HR" => &[3765],
        "NZ" => &[2193],
        "CO" => &[3116],
        _ => &[],
    }
}

/// A country's datum's UTM zones (or bands), named as EPSG names them, `{z}` the zone and `{h}` N or S; and the systems
/// after them (Brazil's polyconic).
fn zones(iso: &str) -> (&'static [&'static str], &'static [u32]) {
    const ETRS: &[&str] = &["ETRS89 / UTM zone {z}N"];
    match iso {
        "BR" => (&["SIRGAS 2000 / UTM zone {z}{h}"], &[5880]),
        "AR" => (&["POSGAR 2007 / Argentina {ar}", "SIRGAS 2000 / UTM zone {z}{h}"], &[]),
        "UY" | "PY" | "BO" | "PE" | "EC" | "VE" | "CL" | "CO" | "GY" | "SR" | "CR" | "PA" | "GT" | "HN" | "SV" | "NI" => {
            (&["SIRGAS 2000 / UTM zone {z}{h}"], &[])
        }
        "MX" => (&["Mexico ITRF2008 / UTM zone {z}N"], &[]),
        "US" => (&["NAD83(2011) / UTM zone {z}N", "NAD83 / UTM zone {z}N"], &[]),
        "CA" => (&["NAD83(CSRS) / UTM zone {z}N"], &[]),
        "AU" => (&["GDA2020 / MGA zone {z}", "GDA94 / MGA zone {z}"], &[]),
        "JP" => (&["JGD2011 / UTM zone {z}N"], &[]),
        "CN" => (&["CGCS2000 / 3-degree Gauss-Kruger CM {cm}E"], &[]),
        "ZA" => (&["Hartebeesthoek94 / Lo{lo}"], &[]),
        "AT" | "BE" | "BG" | "HR" | "CY" | "CZ" | "DK" | "EE" | "FI" | "FR" | "DE" | "GR" | "HU" | "IE" | "IT" | "LV" | "LT" | "LU"
        | "MT" | "NL" | "PL" | "PT" | "RO" | "SK" | "SI" | "ES" | "SE" | "GB" | "NO" | "CH" | "IS" | "LI" | "AD" | "MC" | "SM" | "ME"
        | "RS" | "MK" | "AL" | "BA" | "XK" | "MD" | "UA" => (ETRS, &[]),
        _ => (&[], &[]),
    }
}

/// The map systems surveys at `lon`, `lat` in country `iso` are in, the one to pick first: the national grid; the place's
/// zone on the country's datum; then the zone on WGS 84. Those this version doesn't know are left out.
pub fn systems_at(iso: &str, lon: f64, lat: f64) -> Vec<Suggestion> {
    let z = (((lon + 180.0) / 6.0).floor() as i64 + 1).clamp(1, 60);
    let h = if lat >= 0.0 { "N" } else { "S" };
    let cm = ((lon / 3.0).round() as i64 * 3).clamp(75, 135);
    let lo = (2 * ((lon - 1.0) / 2.0).round() as i64 + 1).clamp(15, 33);
    let ar = (((lon + 72.0) / 3.0).round() as i64 + 1).clamp(1, 7);
    let fill = |t: &str| {
        t.replace("{z}", &z.to_string())
            .replace("{h}", h)
            .replace("{cm}", &cm.to_string())
            .replace("{lo}", &lo.to_string())
            .replace("{ar}", &ar.to_string())
    };
    let (named, after) = zones(iso);
    let mut out: Vec<Suggestion> = Vec::new();
    let mut add = |code: Option<u32>, why: &'static str| {
        if let Some(info) = code.and_then(|c| crs::lookup(c).ok())
            && !out.iter().any(|s| s.info.code == info.code)
        {
            out.push(Suggestion { info, why });
        }
    };
    for c in grids(iso) {
        add(Some(*c), NATIONAL);
    }
    for t in named {
        let why = if t.contains("UTM") { DATUM } else { BAND };
        add(by_name().get(&fill(t)).copied(), why);
    }
    for c in after {
        add(Some(*c), NATIONAL);
    }
    add(by_name().get(&fill("WGS 84 / UTM zone {z}{h}")).copied(), WORLD);
    out
}

/// The map systems for a place.
pub fn systems(p: &Place) -> Vec<Suggestion> {
    systems_at(&p.iso, p.lon, p.lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_reads_and_folds() {
        assert!(data().countries.len() > 200 && data().cities.len() > 7000);
        assert_eq!(fold("São Paulo"), "sao paulo");
        assert_eq!(fold("Zürich, Straße"), "zurich  strasse");
        assert!(find("", 5).is_empty() && find("12345", 5).is_empty());
    }

    #[test]
    fn a_hostile_table_is_left_out_line_by_line() {
        let d = read("C\tXX\tnan\t0\tX\nP\tXX\t999\t0\tA\t\t\nP\tXX\t1\t2\nC\tXX\t1\t2\t\nP\tXX\t1\t2\tCity\t\tRegion\n\u{0}\t\t");
        assert_eq!((d.countries.len(), d.cities.len()), (0, 1));
    }

    #[test]
    fn every_country_s_systems_are_known() {
        // A pattern or a code this version's dataset doesn't name would be left out quietly: none is.
        for (iso, lon, lat, first) in [
            ("BR", -46.6, -23.5, "SIRGAS 2000 / UTM zone 23S"),
            ("AR", -58.4, -34.6, "POSGAR 2007 / Argentina 6"),
            ("MX", -99.1, 19.4, "Mexico ITRF2008 / UTM zone 14N"),
            ("US", -74.0, 40.7, "NAD83(2011) / UTM zone 18N"),
            ("CA", -79.4, 43.7, "NAD83(CSRS) / UTM zone 17N"),
            ("AU", 151.2, -33.9, "GDA2020 / MGA zone 56"),
            ("JP", 139.7, 35.7, "JGD2011 / UTM zone 54N"),
            ("CN", 116.4, 39.9, "CGCS2000 / 3-degree Gauss-Kruger CM 117E"),
            ("ZA", 28.0, -26.2, "Hartebeesthoek94 / Lo29"),
            ("DE", 13.4, 52.5, "ETRS89 / UTM zone 33N"),
            ("GB", -0.1, 51.5, "OSGB 1936 / British National Grid"),
        ] {
            let s = systems_at(iso, lon, lat);
            assert_eq!(s.first().map(|s| s.info.name.as_str()), Some(first), "{iso}: {s:?}");
            assert!(s.last().is_some_and(|s| s.info.name.starts_with("WGS 84 / UTM zone")), "{iso}: {s:?}");
        }
        for iso in [
            "GB", "IE", "NL", "FR", "CH", "BE", "PT", "PL", "SE", "FI", "CZ", "AT", "IT", "GR", "HU", "IS", "EE", "LT", "LV", "SI", "HR",
            "NZ", "CO",
        ] {
            assert!(grids(iso).iter().all(|c| crs::lookup(*c).is_ok()), "{iso}");
        }
    }
}
