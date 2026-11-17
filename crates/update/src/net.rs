//! The desktop's network: HTTPS to this repository's releases on GitHub and the CDN they are served from, through ureq with
//! rustls (Mozilla's root certificates built in), and nowhere else. A request goes only to an address [`hop_allowed`] says yes
//! to, and a redirect is followed by hand, each hop under the same rule, so no reply can send the updater somewhere else.

use std::io::{self, Write};
use std::time::Duration;

use ureq::Agent;
use ureq::http::Uri;

use crate::{MAX_JSON, Net};

/// A release asset is one redirect from github.com to the CDN; a few more are allowed before giving up.
const REDIRECTS: usize = 4;
/// A plugin's feed may hop three times, and only over https.
const FEED_REDIRECTS: usize = 3;

/// A reply as small as the API's takes seconds; a larger one is the installer, which on a slow line takes minutes.
const QUICK: Duration = Duration::from_secs(15);
const SLOW: Duration = Duration::from_secs(10 * 60);
/// A plugin's feed, its archive and the whole check are one budget: ten seconds to connect, thirty to finish.
const FEED_CONNECT: Duration = Duration::from_secs(10);
const FEED_TOTAL: Duration = Duration::from_secs(30);

/// Where a request may go: GitHub's releases, as the app's updater needs, or a plugin's own channel, which is any
/// https host except the machine itself unless a setting says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Policy {
    Github,
    Feed { allow_private: bool },
}

pub struct Ureq {
    agent: Agent,
    /// In a test, the one address a request may go to (a server on this machine); none: what the policy allows.
    only: Option<String>,
    policy: Policy,
}

impl Default for Ureq {
    fn default() -> Self {
        Ureq::new()
    }
}

/// An agent that follows no redirect itself (that's done by hand, under [`hop_allowed`]) and gives up on a stalled connection.
fn agent(https_only: bool) -> Agent {
    Agent::config_builder()
        .https_only(https_only)
        .max_redirects(0)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .user_agent(format!("BIMCraft/{} (+https://github.com/IPedrax/BIMCraft)", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

impl Ureq {
    pub fn new() -> Ureq {
        Ureq { agent: agent(true), only: None, policy: Policy::Github }
    }

    /// A network for a plugin's update channel: any https host, to the machine's own addresses only when the user
    /// allowed local development. Connections take ten seconds and the whole request thirty.
    pub fn for_feed(allow_private: bool) -> Ureq {
        let agent = Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .timeout_connect(Some(FEED_CONNECT))
            .timeout_global(Some(FEED_TOTAL))
            .user_agent(format!("BIMCraft/{} (+https://github.com/IPedrax/BIMCraft)", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Ureq { agent, only: None, policy: Policy::Feed { allow_private } }
    }

    /// For tests: talks plain HTTP to the server at `base` on this machine, and to nothing else.
    #[cfg(test)]
    pub(crate) fn loopback(base: &str) -> Ureq {
        Ureq { agent: agent(false), only: Some(format!("{base}/")), policy: Policy::Github }
    }

    fn allowed(&self, url: &str) -> bool {
        self.only.as_ref().map_or_else(
            || match self.policy {
                Policy::Github => hop_allowed(url),
                Policy::Feed { allow_private } => feed_hop_allowed(url, allow_private),
            },
            |only| url.starts_with(only),
        )
    }

    fn redirects(&self) -> usize {
        match self.policy {
            Policy::Github => REDIRECTS,
            Policy::Feed { .. } => FEED_REDIRECTS,
        }
    }
}

/// Whether a request to `url` is one the updater makes: HTTPS, to a host as named (no port, no credentials in front of it), and
/// where that host is GitHub's own, into this repository's releases (`..` and escapes are no way to leave them). The CDN's
/// addresses are signed and new each time, so any path on its two hosts goes.
pub fn hop_allowed(url: &str) -> bool {
    let Ok(uri) = url.parse::<Uri>() else { return false };
    let (Some(host), path) = (uri.authority(), uri.path().to_ascii_lowercase()) else { return false };
    let repo = crate::REPO.to_ascii_lowercase();
    let ours = |start: String| path.starts_with(&start) && !path.contains("..") && !path.contains('%');
    uri.scheme_str() == Some("https")
        && match host.as_str().to_ascii_lowercase().as_str() {
            "github.com" => ours(format!("/{repo}/releases/download/")),
            "api.github.com" => ours(format!("/repos/{repo}/releases/")),
            "release-assets.githubusercontent.com" | "objects.githubusercontent.com" => true,
            _ => false,
        }
}

/// Whether a plugin's update channel may be read from `url`: HTTPS, a host as named (no login), and not this
/// machine's own or link-local address unless `allow_private` says local development is intended.
pub fn feed_hop_allowed(url: &str, allow_private: bool) -> bool {
    if !url.starts_with("https://") || url.contains([' ', '\t', '\n', '\r']) || url.contains('#') {
        return false;
    }
    let Ok(uri) = url.parse::<Uri>() else { return false };
    let (Some(host), _) = (uri.authority(), uri.path()) else { return false };
    if host.as_str().contains('@') || host.host().is_empty() {
        return false;
    }
    let host = host.host().trim_matches(['[', ']']).to_ascii_lowercase();
    if host.contains('%') {
        // An IPv6 zone names an interface on this machine; it is no remote publisher's address.
        return false;
    }
    if allow_private {
        return true;
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return false;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => !is_local(&ip),
        Err(_) => true,
    }
}

/// An address of this machine or its immediate link: never an update channel a remote publisher could use.
fn is_local(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local() || v4.is_unspecified(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback() || v4.is_link_local())
        }
    }
}

/// `url` for a message: its host, not the signed query a CDN adds.
pub fn host_of(url: &str) -> String {
    url.parse::<Uri>().ok().and_then(|u| u.host().map(str::to_string)).unwrap_or_else(|| "the address".into())
}

#[cfg(test)]
mod tests {
    use super::feed_hop_allowed;

    #[test]
    fn a_plugin_feed_is_https_and_never_this_machine() {
        assert!(feed_hop_allowed("https://example.org/demo/latest.json", false));
        assert!(!feed_hop_allowed("http://example.org/latest.json", false));
        assert!(!feed_hop_allowed("https://user:pw@example.org/latest.json", false));
        assert!(!feed_hop_allowed("https://example.org/latest.json#frag", false));
        assert!(!feed_hop_allowed("https://localhost:8443/latest.json", false));
        assert!(!feed_hop_allowed("https://127.0.0.1/latest.json", false));
        assert!(!feed_hop_allowed("https://[::1]/latest.json", false));
        assert!(!feed_hop_allowed("https://169.254.1.1/latest.json", false));
        assert!(!feed_hop_allowed("https://[fe80::1]/latest.json", false));
        // A local development server is allowed only when the user turned the setting on.
        assert!(feed_hop_allowed("https://127.0.0.1:8443/latest.json", true));
        assert!(feed_hop_allowed("https://localhost/latest.json", true));
    }
}

impl Net for Ureq {
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<(), String> {
        let budget = match self.policy {
            Policy::Github => {
                if limit <= MAX_JSON {
                    QUICK
                } else {
                    SLOW
                }
            }
            Policy::Feed { .. } => FEED_TOTAL,
        };
        let mut url = url.to_string();
        for _ in 0..=self.redirects() {
            if !self.allowed(&url) {
                return Err(format!("won't fetch from {}", host_of(&url)));
            }
            let host = host_of(&url);
            let response = self
                .agent
                .get(&url)
                .config()
                .timeout_global(Some(budget))
                .build()
                .header("Accept", "application/vnd.github+json, application/octet-stream")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .call()
                .map_err(|e| format!("{host}: {e}"))?;
            if response.status().is_redirection() {
                let to = response
                    .headers()
                    .get("location")
                    .and_then(|l| l.to_str().ok())
                    .ok_or_else(|| format!("{host}: a redirect to nowhere"))?;
                url = to.to_string();
                continue;
            }
            // ureq's limit fails the read after the last byte it allows, so one more than the caller takes: the caller's own
            // cap is the exact one (a body of exactly `limit` bytes is what a download of the listed size is).
            let mut body = response.into_body().into_with_config().limit(limit.saturating_add(1)).reader();
            return io::copy(&mut body, out).map(|_| ()).map_err(|e| format!("{host}: {e}"));
        }
        Err("too many redirects".into())
    }
}
