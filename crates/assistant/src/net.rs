//! The assistant's network: HTTPS POSTs to the three providers' API hosts, through ureq with rustls (Mozilla's root certificates
//! built in), and nowhere else. A request goes only to an address [`allowed`] says yes to, a redirect is never followed (so no
//! reply can send the key anywhere else), and a reply is read up to [`MAX_REPLY`]. The key travels in a header, never in an
//! address, and no message made here has it in it.

use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use ureq::Agent;

#[cfg(not(target_arch = "wasm32"))]
use crate::MAX_REPLY;

/// A request as the providers take it: where, with which headers (the key is one), and the JSON body.
#[derive(Clone, PartialEq)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Not the headers: the key is in one.
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Request {{ url: {:?}, body: {} bytes }}", self.url, self.body.len())
    }
}

/// The network as the assistant needs it: one POST, answered with the status and the body whatever the status is. The desktop's
/// is [`Ureq`]; the tests' is `testing::Fake`.
pub trait Net: Send + Sync {
    fn post(&self, request: &Request) -> Result<(u16, String), String>;
}

/// The hosts the assistant talks to, and no others.
pub const HOSTS: [&str; 3] = ["api.anthropic.com", "api.openai.com", "generativelanguage.googleapis.com"];

/// Whether a request to `url` is one the assistant makes: HTTPS, to one of [`HOSTS`] as named (no port, no credentials in front of it).
pub fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.contains([':', '@']) && HOSTS.iter().any(|h| h.eq_ignore_ascii_case(host))
}

/// `url` for a message: its host, never the path or the query.
#[cfg(not(target_arch = "wasm32"))]
fn host_of(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .filter(|host| !host.is_empty())
        .unwrap_or("the address")
        .to_string()
}

/// A reply to a message of a few kilobytes comes in seconds; one that thinks first can take minutes.
#[cfg(not(target_arch = "wasm32"))]
const WAIT: Duration = Duration::from_secs(5 * 60);

#[cfg(not(target_arch = "wasm32"))]
pub struct Ureq {
    agent: Agent,
    /// In a test, the one address a request may go to (a server on this machine); none: what [`allowed`] allows.
    only: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Ureq {
    fn default() -> Self {
        Ureq::new()
    }
}

/// An agent that follows no redirect, gives up on a stalled connection and hands a 4xx or 5xx back as a reply to be read: the
/// provider's error says what is wrong with the key or the request.
#[cfg(not(target_arch = "wasm32"))]
fn agent(https_only: bool) -> Agent {
    Agent::config_builder()
        .https_only(https_only)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(WAIT))
        .user_agent(format!("BIMCraft/{} (+https://github.com/IPedrax/BIMCraft)", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

#[cfg(not(target_arch = "wasm32"))]
impl Ureq {
    pub fn new() -> Ureq {
        Ureq { agent: agent(true), only: None }
    }

    /// For tests: talks plain HTTP to the server at `base` on this machine, and to nothing else.
    #[cfg(test)]
    pub(crate) fn loopback(base: &str) -> Ureq {
        Ureq { agent: agent(false), only: Some(format!("{base}/")) }
    }

    fn allowed(&self, url: &str) -> bool {
        self.only.as_ref().map_or_else(|| allowed(url), |only| url.starts_with(only))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Net for Ureq {
    fn post(&self, request: &Request) -> Result<(u16, String), String> {
        if !self.allowed(&request.url) {
            return Err(format!("won't send to {}", host_of(&request.url)));
        }
        let host = host_of(&request.url);
        let mut call = self.agent.post(&request.url).header("Content-Type", "application/json");
        for (name, value) in &request.headers {
            call = call.header(name, value);
        }
        let response = call.send(request.body.as_bytes()).map_err(|e| format!("{host}: {e}"))?;
        let status = response.status().as_u16();
        // ureq's limit fails the read after the last byte it allows, so one more than the cap: the cap itself is checked below.
        let mut body = Vec::new();
        response.into_body().into_with_config().limit(MAX_REPLY + 1).reader().read_to_end(&mut body).map_err(|e| format!("{host}: {e}"))?;
        if body.len() as u64 > MAX_REPLY {
            return Err(format!("{host} sent a reply larger than {} MB: not read", MAX_REPLY >> 20));
        }
        Ok((status, String::from_utf8_lossy(&body).into_owned()))
    }
}
