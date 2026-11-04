//! The network: HTTPS to the hosts [`Allow`] names and nowhere else, through ureq with rustls, a request at a time. A redirect is
//! followed by hand, each hop under the same rule, so no reply can send the fetcher somewhere else; a reply is read up to a cap.

use std::io::Read;
use std::time::Duration;

use ureq::Agent;
use ureq::http::Uri;

/// A reply: its body, and the `Content-Type` it said.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

/// Why a request failed: in words, and, when the server answered with an error, its status and how many seconds it said to wait.
#[derive(Clone, Debug, PartialEq)]
pub struct NetError {
    pub message: String,
    pub status: Option<u16>,
    pub retry_after: Option<u64>,
}

impl NetError {
    pub fn new(message: impl Into<String>) -> NetError {
        NetError { message: message.into(), status: None, retry_after: None }
    }
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What the fetcher asks the network: three kinds of request, each with a cap on the reply.
pub trait Net: Send + Sync {
    fn get(&self, url: &str, limit: u64) -> Result<Reply, NetError>;
    /// `len` bytes of `url` from `start`: a part of a big file.
    fn get_range(&self, url: &str, start: u64, len: u64) -> Result<Reply, NetError>;
    fn post(&self, url: &str, content_type: &str, body: &[u8], limit: u64) -> Result<Reply, NetError>;
}

/// A part of a path made from a tile address template: text as written, or a number to be put in (one to ten digits).
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Text(String),
    Number,
}

/// The paths of an entry that are its.
#[derive(Clone, Debug, PartialEq)]
enum Path {
    /// Any path that starts so.
    Prefix(String),
    /// This path and no other.
    Exact(String),
    /// A tile address's: its text as written and its numbers where they go, nothing before, between or after.
    Pattern(Vec<Piece>),
}

impl Path {
    fn matches(&self, path: &str) -> bool {
        match self {
            Path::Prefix(prefix) => path.starts_with(prefix.as_str()),
            Path::Exact(exact) => path == exact,
            Path::Pattern(pieces) => {
                let mut rest = path;
                for piece in pieces {
                    match piece {
                        Piece::Text(text) => match rest.strip_prefix(text.as_str()) {
                            Some(after) => rest = after,
                            None => return false,
                        },
                        Piece::Number => {
                            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                            if !(1..=10).contains(&digits) {
                                return false;
                            }
                            rest = rest.get(digits..).unwrap_or_default();
                        }
                    }
                }
                rest.is_empty()
            }
        }
    }
}

/// Where a request may go: a host, exactly as named, the paths it may take, and, for a service that can only be asked by query, the
/// query it may carry: each pair in turn, its key, and its value either exactly as written or, where it is a number to be put in,
/// one to ten digits.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    host: String,
    path: Path,
    query: Option<Vec<(String, Option<String>)>>,
}

impl Entry {
    fn prefix(host: &str, prefix: &str) -> Entry {
        Entry { host: host.into(), path: Path::Prefix(prefix.into()), query: None }
    }

    /// The entry for the tile addresses of `template` (`https://host/path/{z}/{x}/{y}.ext`, or `https://host/path?a=b&c={y}`): the
    /// path as the template has it, and the query pair by pair. None if the template is not one that can be matched so.
    fn of_template(template: &str) -> Option<Entry> {
        let (host, rest) = template.strip_prefix("https://")?.split_once('/')?;
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (rest, None),
        };
        let host = host.to_ascii_lowercase();
        // The path in pieces: its text, and a number where there is a `{...}`. A number is followed by text that is not a digit.
        let mut pieces = Vec::new();
        let mut rest = format!("/{path}");
        while let Some(open) = rest.find('{') {
            let close = open + rest.get(open..)?.find('}')?;
            if open > 0 {
                pieces.push(Piece::Text(rest.get(..open)?.to_string()));
            } else if pieces.last() == Some(&Piece::Number) {
                return None;
            }
            pieces.push(Piece::Number);
            rest = rest.get(close + 1..)?.to_string();
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
        }
        if !rest.is_empty() {
            pieces.push(Piece::Text(rest));
        }
        let Some(query) = query else {
            return matches!(pieces.first(), Some(Piece::Text(t)) if t.len() > 1).then(|| Entry {
                host,
                path: Path::Pattern(pieces),
                query: None,
            });
        };
        let [Piece::Text(path)] = pieces.as_slice() else { return None };
        let pairs = query
            .split('&')
            .map(|pair| {
                let (key, value) = pair.split_once('=')?;
                let number = value.starts_with('{') && value.ends_with('}');
                Some((key.to_string(), (!number).then(|| value.to_string())))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Entry { host, path: Path::Exact(path.clone()), query: Some(pairs) })
    }

    /// Whether a request to `path` (and `query`, if it has one) on `host` is one this entry names.
    fn allows(&self, host: &str, path: &str, query: Option<&str>) -> bool {
        if self.host != host || !self.path.matches(path) {
            return false;
        }
        match (&self.query, query) {
            (None, None) => true,
            (Some(rule), Some(query)) => {
                let pairs: Vec<&str> = query.split('&').collect();
                pairs.len() == rule.len()
                    && pairs.iter().zip(rule).all(|(pair, (key, value))| match pair.split_once('=') {
                        Some((k, v)) if k == key => match value {
                            Some(literal) => v == literal,
                            None => (1..=10).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_digit()),
                        },
                        _ => false,
                    })
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Allow(Vec<Entry>);

/// The Overpass endpoint used unless another is named.
pub const OVERPASS: &str = "https://overpass-api.de/api/interpreter";

impl Allow {
    /// The services the fetcher uses: Overpass, the terrain tiles, the Copernicus bucket, OpenStreetMap's map tiles, and the tiles of
    /// each aerial imagery source in the core's table (`bimcraft_core::imagery::SOURCES`), as far as its addresses go and no further.
    pub fn standard() -> Allow {
        let mut entries = vec![
            Entry::prefix("overpass-api.de", "/api/"),
            Entry::prefix("s3.amazonaws.com", "/elevation-tiles-prod/"),
            Entry::prefix("copernicus-dem-30m.s3.amazonaws.com", "/"),
            // The map the picker draws: one host, tiles only.
            Entry::prefix("tile.openstreetmap.org", "/"),
        ];
        entries.extend(bimcraft_core::imagery::SOURCES.iter().filter_map(|s| Entry::of_template(s.template)));
        Allow(entries)
    }

    /// Whether a request to `url` may be made: HTTPS, to a host as an entry names it (no port, no credentials in front of it, nothing
    /// after the path), on a path that starts as the entry says (an entry that begins with `=` is that path and no other); and a path
    /// with `..`, an escape or a backslash is no way to leave it.
    pub fn allows(&self, url: &str) -> bool {
        if url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') || url.contains('#') {
            return false;
        }
        let Ok(uri) = url.parse::<Uri>() else { return false };
        let Some(authority) = uri.authority() else { return false };
        if uri.scheme_str() != Some("https") || uri.port().is_some() || authority.as_str().contains(['@', ':']) {
            return false;
        }
        let (host, path) = (authority.host().to_ascii_lowercase(), uri.path());
        if path.contains("..") || path.contains('%') {
            return false;
        }
        self.0.iter().any(|e| e.allows(&host, path, uri.query()))
    }

    /// The same, and the one address `endpoint` too: an Overpass server the user names must be HTTPS, with no port or credentials,
    /// no query, and a name rather than an address of this machine or network.
    pub fn with_endpoint(mut self, endpoint: &str) -> Result<Allow, String> {
        let wrong =
            || format!("an Overpass endpoint is an https:// address with a host name, no port, credentials or query, not `{endpoint}`");
        if endpoint.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') || endpoint.contains('#') {
            return Err(wrong());
        }
        let uri: Uri = endpoint.parse().map_err(|_| wrong())?;
        let authority = uri.authority().ok_or_else(wrong)?;
        let host = authority.host().to_ascii_lowercase();
        let (path, bad_host) = (uri.path(), host == "localhost" || host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('['));
        if uri.scheme_str() != Some("https")
            || uri.query().is_some()
            || uri.port().is_some()
            || authority.as_str().contains(['@', ':'])
            || bad_host
            || !host.contains('.')
            || !path.starts_with('/')
            || path.len() < 2
            || path.contains("..")
            || path.contains('%')
        {
            return Err(wrong());
        }
        self.0.push(Entry { host, path: Path::Exact(path.to_string()), query: None });
        Ok(self)
    }
}

/// How many redirects are followed.
const REDIRECTS: usize = 4;
/// How long a request may take in all: a tile, and an Overpass query (which is given 60 s to run, and a reply to send).
const QUICK: Duration = Duration::from_secs(60);
const SLOW: Duration = Duration::from_secs(150);

pub struct Ureq {
    agent: Agent,
    allow: Allow,
    /// In a test, the one address a request may go to (a server on this machine); none: what `allow` allows.
    only: Option<String>,
}

/// An agent that follows no redirect itself (that is done by hand, under [`Allow`]), answers an error status as an answer, and gives
/// up on a stalled connection.
fn agent(https_only: bool) -> Agent {
    Agent::config_builder()
        .https_only(https_only)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent(format!("BIMCraft/{} (+https://github.com/IPedrax/BIMCraft)", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// How much of an error's body is looked at, and how long the reason made of it may be.
const REASON_BYTES: usize = 1024;
/// How many bytes of an error's page are read at all.
const ERROR_PAGE: u64 = 512;
const REASON_CHARS: usize = 200;

/// The value of `key` in the attributes of a tag (`<Exception exceptionCode="x">`), in either kind of quote.
fn attribute<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let needle = format!(" {key}={quote}");
        if let Some(at) = tag.find(&needle) {
            let value = tag.get(at + needle.len()..)?;
            return value.find(quote).and_then(|end| value.get(..end));
        }
    }
    None
}

/// What a server's error page says, in one short line: the text of its body with the tags taken out (an OGC exception report's code
/// and `locator`, which name the parameter it did not like, are kept), white space made one space, control characters dropped, cut at
/// 200 characters. `None` for a body that is empty, binary, not UTF-8 or only tags. The body is hostile, so only its first kilobyte is looked at.
pub fn reason(body: &[u8]) -> Option<String> {
    let body = body.get(..REASON_BYTES).unwrap_or(body);
    if body.contains(&0) {
        return None;
    }
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        // A character cut by the end of what was read is no fault of the server's.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(body.get(..e.valid_up_to())?).ok()?,
        Err(_) => return None,
    };
    let (mut chars, mut controls) = (0usize, 0usize);
    for c in text.chars() {
        chars += 1;
        controls += usize::from(c.is_control() && !c.is_whitespace());
    }
    if controls * 10 > chars {
        return None;
    }
    let mut plain = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        plain.push_str(rest.get(..open).unwrap_or_default());
        let Some(after) = rest.get(open + 1..) else { break };
        let Some(close) = after.find('>') else {
            // A tag that never ends: nothing after it is text.
            rest = "";
            break;
        };
        let tag = after.get(..close).unwrap_or_default();
        plain.push(' ');
        for key in ["exceptionCode", "code"] {
            if let Some(code) = attribute(tag, key) {
                plain.push_str(code);
                plain.push(' ');
                break;
            }
        }
        if let Some(locator) = attribute(tag, "locator") {
            plain.push_str("locator=");
            plain.push_str(locator);
            plain.push(' ');
        }
        rest = after.get(close + 1..).unwrap_or_default();
    }
    plain.push_str(rest);
    let plain: String = plain.chars().filter(|c| !c.is_control() || c.is_whitespace()).collect();
    let line = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() > REASON_CHARS {
        let mut cut: String = line.chars().take(REASON_CHARS).collect();
        cut.push('…');
        return Some(cut);
    }
    Some(line)
}

/// How a request goes.
enum Kind<'a> {
    Get,
    Range(u64, u64),
    Post(&'a str, &'a [u8]),
}

/// `url` for a message: its host, not what is after it.
fn host_of(url: &str) -> String {
    url.parse::<Uri>().ok().and_then(|u| u.host().map(str::to_string)).unwrap_or_else(|| "the address".into())
}

/// Where a redirect from `base` to `to` goes, if `to` is an address or a path on the same host.
fn resolve(base: &str, to: &str) -> Option<String> {
    if to.starts_with("http://") || to.starts_with("https://") {
        return Some(to.to_string());
    }
    if to.starts_with('/') && !to.starts_with("//") {
        let uri: Uri = base.parse().ok()?;
        return Some(format!("{}://{}{to}", uri.scheme_str()?, uri.authority()?));
    }
    None
}

impl Ureq {
    pub fn new(allow: Allow) -> Ureq {
        Ureq { agent: agent(true), allow, only: None }
    }

    /// For tests: talks plain HTTP to the server at `base` on this machine, and to nothing else.
    #[cfg(any(test, feature = "testing"))]
    pub fn loopback(base: &str) -> Ureq {
        Ureq { agent: agent(false), allow: Allow::default(), only: Some(format!("{base}/")) }
    }

    fn allowed(&self, url: &str) -> bool {
        self.only.as_ref().map_or_else(|| self.allow.allows(url), |only| url.starts_with(only))
    }

    fn request(&self, url: &str, kind: &Kind, limit: u64, budget: Duration) -> Result<Reply, NetError> {
        let mut url = url.to_string();
        for _ in 0..=REDIRECTS {
            if !self.allowed(&url) {
                return Err(NetError::new(format!("won't fetch from {}", host_of(&url))));
            }
            let host = host_of(&url);
            let response = match kind {
                Kind::Get => self.agent.get(&url).config().timeout_global(Some(budget)).build().call(),
                Kind::Range(start, len) => {
                    let range = format!("bytes={start}-{}", start.saturating_add(len.saturating_sub(1)));
                    self.agent.get(&url).config().timeout_global(Some(budget)).build().header("Range", range.as_str()).call()
                }
                Kind::Post(content_type, body) => {
                    self.agent.post(&url).config().timeout_global(Some(budget)).build().header("Content-Type", *content_type).send(*body)
                }
            }
            .map_err(|e| NetError::new(format!("{host}: {e}")))?;
            let status = response.status();
            if status.is_redirection() {
                if matches!(kind, Kind::Post(..)) {
                    return Err(NetError::new(format!("{host}: it answered with a redirect, which a post is not sent on to")));
                }
                let to = response
                    .headers()
                    .get("location")
                    .and_then(|l| l.to_str().ok())
                    .ok_or_else(|| NetError::new(format!("{host}: a redirect to nowhere")))?;
                url = resolve(&url, to).ok_or_else(|| NetError::new(format!("{host}: a redirect to nowhere it may go")))?;
                continue;
            }
            if status.as_u16() >= 400 {
                // How long to wait, if it says in seconds: a date, or anything else, is not taken.
                let retry_after =
                    response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
                // Why, if the server says: a few hundred bytes of its page, as one line of text (an error that cannot be read is no reason).
                let mut page = Vec::new();
                let _ = response.into_body().into_with_config().limit(ERROR_PAGE * 2).reader().take(ERROR_PAGE).read_to_end(&mut page);
                let message = match reason(&page) {
                    Some(why) => format!("{host}: HTTP {}: {why}", status.as_u16()),
                    None => format!("{host}: HTTP {}", status.as_u16()),
                };
                return Err(NetError { message, status: Some(status.as_u16()), retry_after });
            }
            if let Kind::Range(start, _) = kind
                && *start > 0
                && status.as_u16() != 206
            {
                return Err(NetError::new(format!("{host}: it did not honour the range asked for")));
            }
            let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);
            // The cap is exact: a body of the cap's size is fine, one byte more is too large.
            let mut body = Vec::new();
            let reader = response.into_body().into_with_config().limit(limit.saturating_add(2)).reader();
            reader.take(limit.saturating_add(1)).read_to_end(&mut body).map_err(|e| NetError::new(format!("{host}: {e}")))?;
            if body.len() as u64 > limit {
                return Err(NetError::new(format!("{host}: the reply is too large (over {limit} bytes)")));
            }
            return Ok(Reply { body, content_type });
        }
        Err(NetError::new("too many redirects"))
    }
}

impl Net for Ureq {
    fn get(&self, url: &str, limit: u64) -> Result<Reply, NetError> {
        self.request(url, &Kind::Get, limit, QUICK)
    }

    fn get_range(&self, url: &str, start: u64, len: u64) -> Result<Reply, NetError> {
        self.request(url, &Kind::Range(start, len), len, QUICK)
    }

    fn post(&self, url: &str, content_type: &str, body: &[u8], limit: u64) -> Result<Reply, NetError> {
        self.request(url, &Kind::Post(content_type, body), limit, SLOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Server, http};

    #[test]
    fn only_the_named_hosts_and_paths_are_reached_and_never_by_a_trick() {
        let a = Allow::standard();
        for url in [
            "https://overpass-api.de/api/interpreter",
            "https://OVERPASS-API.DE/api/interpreter",
            "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/14/6069/9294.png",
            "https://tile.openstreetmap.org/12/1517/2332.png",
            "https://TILE.openstreetmap.org/0/0/0.png",
            "https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_S24_00_W047_00_DEM/Copernicus_DSM_COG_10_S24_00_W047_00_DEM.tif",
        ] {
            assert!(a.allows(url), "{url}");
        }
        for url in [
            "http://overpass-api.de/api/interpreter",
            "ftp://overpass-api.de/api/interpreter",
            "https://overpass-api.de:8443/api/interpreter",
            "https://user@overpass-api.de/api/interpreter",
            "https://user:pw@overpass-api.de/api/interpreter",
            "https://overpass-api.de.evil.com/api/interpreter",
            "https://evil.com/overpass-api.de/api/interpreter",
            "https://evil.com@overpass-api.de.evil.com/api/",
            "https://notoverpass-api.de/api/interpreter",
            "https://overpass-api.de/other",
            "https://overpass-api.de/api/../admin",
            "https://overpass-api.de/api/%2e%2e/admin",
            "https://overpass-api.de/api/interpreter?data=x",
            "https://overpass-api.de/api/interpreter#x",
            "https://overpass-api.de\\@evil.com/api/",
            "https://s3.amazonaws.com/other-bucket/x",
            "https://s3.amazonaws.com/elevation-tiles-prod/../other-bucket/x",
            "https://s3.amazonaws.com.evil.com/elevation-tiles-prod/x",
            "https://evil.s3.amazonaws.com/elevation-tiles-prod/x",
            "https://github.com/",
            "http://tile.openstreetmap.org/12/1517/2332.png",
            "https://tile.openstreetmap.org:8443/12/1517/2332.png",
            "https://a.tile.openstreetmap.org.evil.com/12/1517/2332.png",
            "https://tile.openstreetmap.org.evil.com/12/1517/2332.png",
            "https://tile.openstreetmap.org/12/../../x.png",
            "https://tile.openstreetmap.org/12/1517/2332.png?key=x",
            "https://user@tile.openstreetmap.org/12/1517/2332.png",
            "//overpass-api.de/api/interpreter",
            "overpass-api.de/api/interpreter",
            "",
            "https://",
            "https:///api/interpreter",
            "javascript:alert(1)",
            "https://127.0.0.1/api/interpreter",
            "https://[::1]/api/interpreter",
            "https://overpass-api.de/api/interpreter\r\nHost: evil.com",
        ] {
            assert!(!a.allows(url), "{url}");
        }
    }

    #[test]
    fn every_source_in_the_table_is_reached_by_the_addresses_it_makes_and_by_no_other() {
        use bimcraft_core::imagery::SOURCES;
        let a = Allow::standard();
        for s in SOURCES {
            for (z, x, y) in [(s.min_zoom, 0, 0), (s.max_zoom, 1, 1), (12, 1000, 7)] {
                if s.url(z, x, y).is_err() {
                    continue;
                }
                let url = s.url(z, x, y).unwrap();
                assert!(a.allows(&url), "{url}");
                let host = url.trim_start_matches("https://").split('/').next().unwrap().to_string();
                for (what, bad) in [
                    ("an extra pair", format!("{url}&extra=1")),
                    ("a query on the end", format!("{url}?extra=1")),
                    ("a fragment", format!("{url}#x")),
                    ("plain http", url.replacen("https://", "http://", 1)),
                    ("credentials", url.replacen("https://", "https://user@", 1)),
                    ("a port", url.replacen(&host, &format!("{host}:8443"), 1)),
                    ("a look-alike host", url.replacen("https://", "https://evil.", 1)),
                    ("a host with ours in its path", url.replacen("https://", "https://evil.com/", 1)),
                    ("a way up the path", url.replacen(&host, &format!("{host}/.."), 1)),
                    ("an escape", url.replacen(&host, &format!("{host}/%2e%2e"), 1)),
                ] {
                    assert!(!a.allows(&bad), "{} with {what}: {bad}", s.id);
                }
            }
        }
    }

    #[test]
    fn a_source_asked_by_query_is_reached_by_that_query_and_no_other() {
        use bimcraft_core::imagery::SOURCES;
        let Some(s) = SOURCES.iter().find(|s| s.template.contains('?')) else { return };
        let a = Allow::standard();
        let url = s.url(17, 5, 7).unwrap();
        assert!(a.allows(&url) && a.allows(&s.url(0, 0, 0).unwrap()), "{url}");
        let (path, query) = url.split_once('?').unwrap();
        let pairs: Vec<&str> = query.split('&').collect();
        let mut swapped = pairs.clone();
        swapped.swap(0, 1);
        let mut doubled = pairs.clone();
        doubled.push(pairs[pairs.len() - 1]);
        let mut cut = pairs.clone();
        cut.remove(1);
        let mut other = vec![];
        for (i, pair) in pairs.iter().enumerate() {
            // The literal pairs, each changed; the numbers, each made something else.
            let (key, value) = pair.split_once('=').unwrap();
            if !["tilematrix", "tilerow", "tilecol"].contains(&key) {
                let mut changed = pairs.clone();
                let wrong = format!("{key}={value}x");
                changed[i] = &wrong;
                other.push((format!("{key} changed"), format!("{path}?{}", changed.join("&"))));
            }
        }
        for bad in ["-7", "", "7.0", "0x7", "%37", "77777777777", "7a", "+7", " 7"] {
            other.push((
                format!("tilerow {bad:?}"),
                format!(
                    "{path}?{}",
                    pairs
                        .iter()
                        .map(|p| if p.starts_with("tilerow=") { format!("tilerow={bad}") } else { (*p).to_string() })
                        .collect::<Vec<_>>()
                        .join("&")
                ),
            ));
        }
        for (what, bad) in [
            ("two pairs swapped", format!("{path}?{}", swapped.join("&"))),
            ("a pair repeated", format!("{path}?{}", doubled.join("&"))),
            ("a pair left out", format!("{path}?{}", cut.join("&"))),
            ("no query", path.to_string()),
            ("an empty query", format!("{path}?")),
            ("another path", format!("{}/x?{query}", path)),
            ("the path's case", format!("{}?{query}", path.to_ascii_lowercase())),
            ("a semicolon", format!("{path}?{}", query.replacen('&', ";", 1))),
            ("a doubled &", format!("{path}?{}", query.replacen('&', "&&", 1))),
        ] {
            other.push((what.to_string(), bad));
        }
        for (what, bad) in other {
            assert!(!a.allows(&bad), "{what}: {bad}");
        }
    }

    #[test]
    fn a_query_pair_with_nothing_after_the_equals_sign_is_a_literal_like_any_other() {
        let Some(entry) = Entry::of_template("https://geo.example/wmts?layer=l&style=&row={y}") else {
            panic!("a template with an empty pair")
        };
        assert!(entry.allows("geo.example", "/wmts", Some("layer=l&style=&row=7")));
        for bad in [
            "layer=l&style=default&row=7",
            "layer=l&style=x&row=7",
            "layer=l&row=7",
            "layer=l&style&row=7",
            "layer=l&style=&&row=7",
            "layer=l&style=&row=",
            "layer=l&style=&row=x",
            "style=&layer=l&row=7",
        ] {
            assert!(!entry.allows("geo.example", "/wmts", Some(bad)), "{bad}");
        }
        assert!(Allow(vec![entry]).allows("https://geo.example/wmts?layer=l&style=&row=7"));
    }

    #[test]
    fn a_server_the_user_names_is_added_by_name_and_only_if_it_is_https_and_plain() {
        let a = Allow::standard().with_endpoint("https://overpass.example.org/api/interpreter").unwrap();
        assert!(a.allows("https://overpass.example.org/api/interpreter"));
        assert!(!a.allows("https://overpass.example.org/other") && !a.allows("https://overpass.example.org/"));
        assert!(a.allows(OVERPASS), "the standard ones stay");
        for bad in [
            "http://overpass.example.org/api/interpreter",
            "https://overpass.example.org:8443/api/interpreter",
            "https://user@overpass.example.org/api/interpreter",
            "https://overpass.example.org/api/../x",
            "https://overpass.example.org/api/interpreter?x=1",
            "overpass.example.org",
            "",
            "https://127.0.0.1/api/interpreter",
            "https://localhost/api/interpreter",
        ] {
            assert!(Allow::standard().with_endpoint(bad).is_err(), "{bad}");
        }
    }

    fn client(server: &Server) -> Ureq {
        Ureq::loopback(&server.base)
    }

    #[test]
    fn a_get_brings_the_body_and_says_who_is_asking() {
        let server = Server::start(|_, _| http(200, &[("Content-Type", "image/png")], b"hello"));
        let reply = client(&server).get(&format!("{}/tile", server.base), 100).unwrap();
        assert_eq!((reply.body.as_slice(), reply.content_type.as_deref()), (&b"hello"[..], Some("image/png")));
        let seen = &server.requests()[0];
        assert!(seen.header("user-agent").is_some_and(|u| u.starts_with("BIMCraft/") && u.contains("github.com")), "{:?}", seen.headers);
    }

    #[test]
    fn a_reply_is_read_up_to_exactly_its_cap() {
        let server = Server::start(|_, _| http(200, &[], &[7u8; 100]));
        let c = client(&server);
        assert_eq!(c.get(&format!("{}/x", server.base), 100).unwrap().body.len(), 100, "exactly the cap is fine");
        let e = c.get(&format!("{}/x", server.base), 99).unwrap_err();
        assert!(e.message.contains("too large") && e.message.contains("99"), "{e}");
    }

    #[test]
    fn an_error_status_keeps_its_number_and_how_long_to_wait() {
        let server = Server::start(|seen, _| match seen.path.as_str() {
            "/busy" => http(429, &[("Retry-After", "7")], b"slow down"),
            "/late" => http(504, &[("Retry-After", "Wed, 21 Oct 2026 07:28:00 GMT")], b""),
            _ => http(404, &[], b"no"),
        });
        let c = client(&server);
        let busy = c.get(&format!("{}/busy", server.base), 100).unwrap_err();
        assert_eq!((busy.status, busy.retry_after), (Some(429), Some(7)));
        let late = c.get(&format!("{}/late", server.base), 100).unwrap_err();
        assert_eq!((late.status, late.retry_after), (Some(504), None), "a date is not a number of seconds, and is not trusted");
        assert_eq!(c.get(&format!("{}/none", server.base), 100).unwrap_err().status, Some(404));
        assert!(!busy.message.contains(&server.base[7..]) || busy.message.contains("429"), "{busy}");
    }

    #[test]
    fn an_error_status_says_why_when_the_server_does() {
        let report = br#"<?xml version="1.0"?><ServiceExceptionReport><ServiceException code="InvalidParameterValue" locator="style">
            Style 'default' is not one of the styles of omwrgbmrvl</ServiceException></ServiceExceptionReport>"#;
        let server = Server::start(move |seen, _| match seen.path.as_str() {
            "/report" => http(400, &[("Content-Type", "application/xml")], report),
            "/binary" => http(400, &[], &[0, 159, 146, 150, 0, 1, 2, 3, 255, 254]),
            "/long" => http(500, &[], &b"x".repeat(100_000)),
            "/busy" => http(429, &[("Retry-After", "7")], b"slow down"),
            "/empty" => http(404, &[], b""),
            _ => http(200, &[], b"fine"),
        });
        let c = client(&server);
        let at = |p: &str| format!("{}{p}", server.base);
        let e = c.get(&at("/report"), 100).unwrap_err();
        assert_eq!(e.status, Some(400));
        assert!(e.message.contains("HTTP 400: "), "{e}");
        assert!(e.message.contains("InvalidParameterValue") && e.message.contains("style") && e.message.contains("not one of"), "{e}");
        assert!(!e.message.contains('<') && !e.message.contains('\n'), "tags and line breaks are not carried: {e}");
        let e = c.get(&at("/binary"), 100).unwrap_err();
        assert_eq!((e.status, e.message.ends_with("HTTP 400")), (Some(400), true), "a body that is not text says nothing: {e}");
        let e = c.get(&at("/long"), 100).unwrap_err();
        assert!(e.message.starts_with(&format!("{}: HTTP 500: ", host_of(&server.base))), "{e}");
        assert!(e.message.chars().count() < 300, "a long body is cut: {} characters", e.message.chars().count());
        let e = c.get(&at("/busy"), 100).unwrap_err();
        assert_eq!((e.status, e.retry_after), (Some(429), Some(7)), "the number to wait is kept beside the reason");
        assert!(e.message.contains("slow down"), "{e}");
        let e = c.get(&at("/empty"), 100).unwrap_err();
        assert!(e.message.ends_with("HTTP 404"), "no body, no colon: {e}");
    }

    #[test]
    fn a_reason_is_one_short_line_of_plain_text_whatever_the_body() {
        assert_eq!(reason(b"  Not  found\r\n here\t"), Some("Not found here".into()));
        assert_eq!(
            reason(b"<html><head><title>Bad</title></head><body><h1>Bad request</h1></body></html>"),
            Some("Bad Bad request".into())
        );
        assert_eq!(
            reason(
                br#"<Exception exceptionCode="TileOutOfRange" locator="TileRow"><ExceptionText>row 9 too big</ExceptionText></Exception>"#
            ),
            Some("TileOutOfRange locator=TileRow row 9 too big".into())
        );
        assert_eq!(reason(b""), None);
        assert_eq!(reason(b" \r\n\t "), None);
        assert_eq!(reason(b"<a><b></b></a>"), None, "only tags is nothing");
        assert_eq!(reason(&[0u8; 50]), None);
        assert_eq!(reason(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), None, "a picture is not a reason");
        assert_eq!(reason(&[0xff, 0xfe, 0xfd, 0xfc]), None, "not UTF-8");
        let long = reason(&b"word ".repeat(1000)).unwrap_or_default();
        assert!(long.chars().count() <= 201 && long.ends_with('…'), "{} characters", long.chars().count());
        // Control characters inside text are dropped, and an unterminated tag is not a loop or a panic.
        assert_eq!(
            reason(b"an error\x07 in the request, said the server\x1b[31m"),
            Some("an error in the request, said the server[31m".into())
        );
        assert_eq!(reason(b"\x07\x07\x07a"), None, "mostly control characters is not text");
        assert_eq!(reason(b"before <unterminated tag"), Some("before".into()));
        for hostile in [
            "é<<<>>>ü".as_bytes(),
            b"<",
            b">",
            b"<<<<<<<",
            b"<a locator=\"",
            b"<a locator=",
            b"<a locator=\"x",
            b"locator=\"x\"",
            b"\"\"\"\"",
            b"<\0>",
        ] {
            let _ = reason(hostile);
        }
    }

    #[test]
    fn a_post_sends_its_body_and_its_type() {
        let server = Server::start(|_, _| http(200, &[("Content-Type", "application/json")], b"{}"));
        let reply = client(&server).post(&format!("{}/api", server.base), "application/x-www-form-urlencoded", b"data=out", 100).unwrap();
        assert_eq!(reply.body, b"{}");
        let seen = &server.requests()[0];
        assert_eq!((seen.method.as_str(), seen.body.as_slice()), ("POST", &b"data=out"[..]));
        assert_eq!(seen.header("content-type"), Some("application/x-www-form-urlencoded"));
    }

    #[test]
    fn a_range_asks_for_those_bytes_and_takes_no_more_than_that() {
        let server = Server::start(|seen, _| match seen.header("range") {
            Some("bytes=10-19") => http(206, &[("Content-Range", "bytes 10-19/100")], &[1u8; 10]),
            Some(_) => http(416, &[], b""),
            // A server that ignores the range and sends everything.
            None => http(200, &[], &[2u8; 1000]),
        });
        let c = client(&server);
        assert_eq!(c.get_range(&format!("{}/f", server.base), 10, 10).unwrap().body, [1u8; 10]);
        assert_eq!(c.get_range(&format!("{}/f", server.base), 0, 5).unwrap_err().status, Some(416));
        let sent = Server::start(|_, _| http(200, &[], &[2u8; 1000]));
        let e = client(&sent).get_range(&format!("{}/f", sent.base), 10, 10).unwrap_err();
        assert!(e.message.contains("too large") || e.message.contains("range"), "{e}");
    }

    #[test]
    fn redirects_are_followed_by_hand_and_only_to_where_they_may_go() {
        let server = Server::start(|seen, base| match seen.path.as_str() {
            "/a" => http(302, &[("Location", "/b")], b""),
            "/b" => http(301, &[("Location", &format!("{base}/c"))], b""),
            "/c" => http(200, &[], b"arrived"),
            "/out" => http(302, &[("Location", "http://127.0.0.1:1/elsewhere")], b""),
            "/loop" => http(302, &[("Location", "/loop")], b""),
            "/nowhere" => http(302, &[], b""),
            _ => http(404, &[], b""),
        });
        let c = client(&server);
        let at = |p: &str| format!("{}{p}", server.base);
        assert_eq!(c.get(&at("/a"), 100).unwrap().body, b"arrived");
        assert!(c.get(&at("/out"), 100).unwrap_err().message.contains("won't fetch"));
        assert!(c.get(&at("/loop"), 100).unwrap_err().message.contains("too many redirects"));
        assert!(c.get(&at("/nowhere"), 100).unwrap_err().message.contains("nowhere"));
        // A POST is not sent again somewhere else.
        let e = c.post(&at("/a"), "text/plain", b"x", 100).unwrap_err();
        assert!(e.message.contains("redirect"), "{e}");
    }

    #[test]
    fn the_real_client_refuses_this_machine_and_anything_not_named() {
        let server = Server::start(|_, _| http(200, &[], b"no"));
        let c = Ureq::new(Allow::standard());
        for url in
            [format!("{}/api/interpreter", server.base), "https://example.com/".into(), "http://overpass-api.de/api/interpreter".into()]
        {
            assert!(c.get(&url, 100).unwrap_err().message.contains("won't fetch"), "{url}");
        }
        assert!(c.post("https://example.com/", "text/plain", b"x", 100).unwrap_err().message.contains("won't fetch"));
        assert!(c.get_range("https://example.com/", 0, 1).unwrap_err().message.contains("won't fetch"));
        assert!(server.requests().is_empty(), "nothing reached it");
    }
}
