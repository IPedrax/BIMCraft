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

/// Where a request may go: a host, exactly as named, and a path that starts as given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Allow(Vec<(String, String)>);

/// The Overpass endpoint used unless another is named.
pub const OVERPASS: &str = "https://overpass-api.de/api/interpreter";

impl Allow {
    /// The services the fetcher uses: Overpass, the terrain tiles, the Copernicus bucket.
    pub fn standard() -> Allow {
        Allow(vec![
            ("overpass-api.de".into(), "/api/".into()),
            ("s3.amazonaws.com".into(), "/elevation-tiles-prod/".into()),
            ("copernicus-dem-30m.s3.amazonaws.com".into(), "/".into()),
        ])
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
        if uri.scheme_str() != Some("https") || uri.query().is_some() || uri.port().is_some() || authority.as_str().contains(['@', ':']) {
            return false;
        }
        let (host, path) = (authority.host().to_ascii_lowercase(), uri.path());
        if path.contains("..") || path.contains('%') {
            return false;
        }
        self.0.iter().any(|(h, prefix)| match prefix.strip_prefix('=') {
            Some(exact) => *h == host && path == exact,
            None => *h == host && path.starts_with(prefix.as_str()),
        })
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
        self.0.push((host, format!("={path}")));
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
                return Err(NetError { message: format!("{host}: HTTP {}", status.as_u16()), status: Some(status.as_u16()), retry_after });
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
