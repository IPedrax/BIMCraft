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
    fn post(&self, url: &str, content_type: &str, body: &[u8], limit: u64) -> Result<Reply, NetError>;
}

/// The paths of an entry that are its.
#[derive(Clone, Debug, PartialEq)]
enum Path {
    /// Any path that starts so.
    Prefix(String),
    /// This path and no other.
    Exact(String),
}

impl Path {
    fn matches(&self, path: &str) -> bool {
        match self {
            Path::Prefix(prefix) => path.starts_with(prefix.as_str()),
            Path::Exact(exact) => path == exact,
        }
    }
}

/// Where a request may go: a host, exactly as named, and the paths it may take. A request carries no query.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    host: String,
    path: Path,
}

impl Entry {
    fn prefix(host: &str, prefix: &str) -> Entry {
        Entry { host: host.into(), path: Path::Prefix(prefix.into()) }
    }

    /// Whether a request to `path` on `host` is one this entry names.
    fn allows(&self, host: &str, path: &str, query: Option<&str>) -> bool {
        query.is_none() && self.host == host && self.path.matches(path)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Allow(Vec<Entry>);

/// The Overpass endpoint used unless another is named.
pub const OVERPASS: &str = "https://overpass-api.de/api/interpreter";

impl Allow {
    /// The services the fetcher uses: Overpass and OpenStreetMap's map tiles, as far as their addresses go and no further.
    pub fn standard() -> Allow {
        let entries = vec![
            Entry::prefix("overpass-api.de", "/api/"),
            // The map the picker draws: one host, tiles only.
            Entry::prefix("tile.openstreetmap.org", "/"),
        ];
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
        self.0.push(Entry { host, path: Path::Exact(path.to_string()) });
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

/// `line` with the five entities an error page writes decoded (`&lt; &gt; &quot; &#39; &apos; &amp;`) and no others: a numeric
/// reference in general could be a control character. `&amp;` is last, so that what it decodes to is not decoded again.
fn decode_entities(line: &str) -> String {
    line.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// What a server's error page says, in one short line: the text of its body with the tags taken out (an OGC exception report's code
/// and `locator`, which name the parameter it did not like, are kept), white space made one space, control characters dropped, five entities decoded, cut at
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
    let line = decode_entities(&plain.split_whitespace().collect::<Vec<_>>().join(" "));
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
            "https://tile.openstreetmap.org/12/1517/2332.png",
            "https://TILE.openstreetmap.org/0/0/0.png",
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
            "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/14/6069/9294.png",
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
    fn a_reason_has_the_five_entities_of_a_page_decoded_and_no_others() {
        assert_eq!(reason(b"Style &#39;default&#39; is invalid."), Some("Style 'default' is invalid.".into()));
        assert_eq!(reason(b"a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;"), Some(r#"a & b <c> "d" 'e'"#.into()));
        // Once: what an entity decodes to is not decoded again.
        assert_eq!(reason(b"&amp;lt;b&amp;gt; &amp;amp;"), Some("&lt;b&gt; &amp;".into()));
        // Not a numeric reference in general: a control character, a tag's bracket or an unknown name stays as written.
        assert_eq!(reason(b"x&#0;y&#x1b;z&#60;&nbsp;&bogus;"), Some("x&#0;y&#x1b;z&#60;&nbsp;&bogus;".into()));
        // A decoded bracket is text, not a tag taken out afterwards.
        assert_eq!(reason(b"&lt;script&gt; ran"), Some("<script> ran".into()));
        // And it is still one line of at most 200 characters.
        let long = reason(&b"&quot;word&quot; ".repeat(300)).unwrap_or_default();
        assert!(long.chars().count() <= 201 && !long.contains('&'), "{} characters", long.chars().count());
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
        assert!(server.requests().is_empty(), "nothing reached it");
    }
}
