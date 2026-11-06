//! Overpass: the query posted, the reply checked and kept. A busy server (429, 502, 503, 504) is asked again a couple of times
//! after the pause it gives; a reply that is an error page, or JSON that says the query could not finish, is an error that says
//! what to do (a smaller box) and is never kept; a good reply is kept for a day.

use std::sync::Arc;
use std::time::Duration;

use bimcraft_core::geo::{Bbox, Reply};
use serde_json::Value;

use crate::cache::{Cache, key};
use crate::net::{Net, NetError};

/// The most a reply may be, bytes.
pub const MAX_REPLY: u64 = 64 << 20;
/// How long a reply is kept, seconds: a day, for buildings do not change by the hour.
pub const TTL: u64 = 24 * 3600;
/// How many times a busy server is asked again.
const RETRIES: usize = 2;

/// What waits: sleeps in use, and a recorder in tests.
pub type Pause = Arc<dyn Fn(Duration) + Send + Sync>;

pub struct Overpass {
    net: Arc<dyn Net>,
    cache: Arc<Cache>,
    endpoint: String,
    pause: Pause,
}

/// `text` as a form value: everything but letters, digits and `-_.~` as `%XX`.
pub fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The start of `text` for a message, cut at a character.
fn start_of(text: &str, most: usize) -> String {
    let cut: String = text.chars().take(most).collect();
    if cut.len() < text.len() { format!("{cut}…") } else { cut }
}

/// Whether `text` is an answer to the query, or why not. Overpass says a query that could not finish in its own words in the reply.
fn check(text: &str) -> Result<(), String> {
    let trimmed = text.trim();
    let gave_up = |remark: &str| {
        let r = remark.to_ascii_lowercase();
        r.contains("runtime error") || r.contains("timed out") || r.contains("out of memory")
    };
    if trimmed.len() <= 4 << 20 {
        let v: Value =
            serde_json::from_str(trimmed).map_err(|_| format!("Overpass did not answer with JSON: `{}`", start_of(trimmed, 60)))?;
        if let Some(remark) = v.get("remark").and_then(Value::as_str).filter(|r| gave_up(r)) {
            return Err(format!("Overpass could not finish the query ({}): use a smaller box", start_of(remark, 120)));
        }
        if !v.get("elements").is_some_and(Value::is_array) {
            return Err("Overpass's reply has no `elements` list".into());
        }
        return Ok(());
    }
    // A big reply is not read twice: its ends say what it is, since Overpass puts a remark last.
    let head: String = trimmed.chars().take(2000).collect();
    let tail: String = trimmed.chars().rev().take(2000).collect::<Vec<_>>().into_iter().rev().collect();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(format!("Overpass did not answer with JSON: `{}`", start_of(trimmed, 60)));
    }
    if tail.contains("\"remark\"") && gave_up(&tail) {
        return Err("Overpass could not finish the query: use a smaller box".into());
    }
    if !head.contains("\"elements\"") {
        return Err("Overpass's reply has no `elements` list".into());
    }
    Ok(())
}

/// What a failed request is to say, after the asking again has been done.
fn why(e: &NetError) -> String {
    match e.status {
        Some(429) => format!("Overpass is busy ({e}): wait a minute and try again, or use a smaller box"),
        Some(504) => format!("Overpass timed out ({e}): use a smaller box"),
        Some(502 | 503) => format!("Overpass is not available just now ({e}): try again later"),
        _ => e.to_string(),
    }
}

impl Overpass {
    pub fn new(net: Arc<dyn Net>, cache: Arc<Cache>, endpoint: &str) -> Overpass {
        Overpass { net, cache, endpoint: endpoint.into(), pause: Arc::new(std::thread::sleep) }
    }

    /// The same, waiting by `pause`, for tests.
    pub fn with_pause(mut self, pause: Pause) -> Overpass {
        self.pause = pause;
        self
    }

    /// The reply to `query` for `bbox`: from the cache if it is there and fresh, else from the server.
    pub fn fetch(&self, bbox: &Bbox, query: &str) -> Result<Reply, String> {
        let _ = bbox;
        let name = key(&[b"overpass", self.endpoint.as_bytes(), query.as_bytes()]);
        if let Some(text) = self.cache.read(&name, TTL).and_then(|b| String::from_utf8(b).ok()).filter(|t| check(t).is_ok()) {
            return Ok(Reply { text, cached: true });
        }
        let body = format!("data={}", form_encode(query));
        let mut attempt = 0;
        let reply = loop {
            match self.net.post(&self.endpoint, "application/x-www-form-urlencoded", body.as_bytes(), MAX_REPLY) {
                Ok(reply) => break reply,
                Err(e) if matches!(e.status, Some(429 | 502 | 503 | 504)) && attempt < RETRIES => {
                    // The pause it asks for, in the range that is no insult to it or to the person waiting.
                    let wait = e.retry_after.map_or(5 * (attempt as u64 + 1), |s| s.clamp(1, 30));
                    (self.pause)(Duration::from_secs(wait));
                    attempt += 1;
                }
                Err(e) => return Err(why(&e)),
            }
        };
        let text = String::from_utf8(reply.body).map_err(|_| "Overpass's reply is not text".to_string())?;
        check(&text)?;
        self.cache.write(&name, text.as_bytes());
        Ok(Reply { text, cached: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::OVERPASS;
    use crate::testing::{Fake, Scratch};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    const GOOD: &str = r#"{"version":0.6,"elements":[{"type":"way","id":1}]}"#;

    fn bbox() -> Bbox {
        Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap()
    }

    /// A client with no pause but a record of them, and a cache in a folder of its own.
    fn client(net: Arc<Fake>, dir: &Scratch) -> (Overpass, Arc<Mutex<Vec<u64>>>, Arc<AtomicU64>) {
        let waits = Arc::new(Mutex::new(Vec::new()));
        let log = waits.clone();
        let now = Arc::new(AtomicU64::new(1_000_000));
        let read = now.clone();
        let cache = Arc::new(Cache::new(Some(dir.0.clone())).with_clock(Arc::new(move || read.load(Ordering::Relaxed))));
        let o = Overpass::new(net, cache, OVERPASS).with_pause(Arc::new(move |d| log.lock().unwrap().push(d.as_secs())));
        (o, waits, now)
    }

    #[test]
    fn a_query_is_posted_as_a_form_and_the_reply_comes_back() {
        assert_eq!(form_encode("abc-_.~XYZ019"), "abc-_.~XYZ019");
        assert_eq!(form_encode("[out:json];way[\"a b\"](1,2);"), "%5Bout%3Ajson%5D%3Bway%5B%22a%20b%22%5D%281%2C2%29%3B");
        assert_eq!(form_encode("é€"), "%C3%A9%E2%82%AC");
        let dir = Scratch::new("op");
        let net = Arc::new(Fake::new().post(OVERPASS, GOOD.as_bytes()));
        let (o, waits, _) = client(net.clone(), &dir);
        let r = o.fetch(&bbox(), "[out:json];out;").unwrap();
        assert_eq!((r.text.as_str(), r.cached), (GOOD, false));
        let asked = net.asked();
        assert_eq!(asked.len(), 1);
        assert_eq!((asked[0].0.as_str(), asked[0].1.as_str()), ("POST", OVERPASS));
        assert_eq!(asked[0].2, b"data=%5Bout%3Ajson%5D%3Bout%3B");
        assert!(waits.lock().unwrap().is_empty());
    }

    #[test]
    fn a_good_reply_is_kept_for_a_day_and_asking_again_does_not_ask_the_server() {
        let dir = Scratch::new("op-cache");
        let net = Arc::new(Fake::new().post(OVERPASS, GOOD.as_bytes()));
        let (o, _, now) = client(net.clone(), &dir);
        o.fetch(&bbox(), "q1").unwrap();
        let again = o.fetch(&bbox(), "q1").unwrap();
        assert_eq!((again.text.as_str(), again.cached), (GOOD, true));
        assert_eq!(net.asked().len(), 1);
        o.fetch(&bbox(), "q2").unwrap();
        assert_eq!(net.asked().len(), 2, "another query, another ask");
        now.store(1_000_000 + TTL + 1, Ordering::Relaxed);
        assert!(!o.fetch(&bbox(), "q1").unwrap().cached, "a day on, it is asked again");
        assert_eq!(net.asked().len(), 3);
    }

    #[test]
    fn a_busy_server_is_asked_again_after_the_pause_it_gives_and_then_given_up_on() {
        let dir = Scratch::new("op-busy");
        let net = Arc::new(Fake::new().status("POST", OVERPASS, 429, Some(2)).post(OVERPASS, GOOD.as_bytes()));
        let (o, waits, _) = client(net.clone(), &dir);
        assert!(o.fetch(&bbox(), "q").is_ok());
        assert_eq!(*waits.lock().unwrap(), [2]);
        // 504 and 503 are asked again too, with a pause of their own when none is given; a huge one is cut.
        let net = Arc::new(
            Fake::new().status("POST", OVERPASS, 504, None).status("POST", OVERPASS, 503, Some(100_000)).post(OVERPASS, GOOD.as_bytes()),
        );
        let (o, waits, _) = client(net, &Scratch::new("op-busy2"));
        assert!(o.fetch(&bbox(), "q").is_ok());
        let w = waits.lock().unwrap().clone();
        assert_eq!(w.len(), 2);
        assert!(w[0] >= 1 && w[1] == 30, "{w:?}");
        // Three busy answers are the end of it, and the words say what to do.
        let net = Arc::new(Fake::new().status("POST", OVERPASS, 429, None));
        let (o, _, _) = client(net.clone(), &Scratch::new("op-busy3"));
        let e = o.fetch(&bbox(), "q").unwrap_err();
        assert!(e.contains("busy") && e.contains("429"), "{e}");
        assert_eq!(net.asked().len(), 3);
        // Other errors are not asked again.
        let net = Arc::new(Fake::new().status("POST", OVERPASS, 400, None));
        let (o, _, _) = client(net.clone(), &Scratch::new("op-bad"));
        assert!(o.fetch(&bbox(), "q").unwrap_err().contains("400"));
        assert_eq!(net.asked().len(), 1);
        let gone = Arc::new(Fake::new());
        let (o, _, _) = client(gone, &Scratch::new("op-404"));
        assert!(o.fetch(&bbox(), "q").unwrap_err().contains("404"));
    }

    #[test]
    fn a_reply_that_is_not_the_answer_is_an_error_and_is_not_kept() {
        for (text, says) in [
            (
                r#"{"version":0.6,"elements":[],"remark":"runtime error: Query timed out in \"query\" at line 3 after 61 seconds."}"#,
                "smaller box",
            ),
            (
                r#"{"version":0.6,"elements":[],"remark":"runtime error: Query run out of memory using about 2048 MB of RAM."}"#,
                "smaller box",
            ),
            ("<html><body>Too many requests</body></html>", "JSON"),
            ("<?xml version=\"1.0\"?><osm><remark>runtime error</remark></osm>", "JSON"),
            ("", "JSON"),
            ("{\"elements\": 5}", "elements"),
            ("{\"version\": 0.6}", "elements"),
            ("{not json", "JSON"),
        ] {
            let dir = Scratch::new("op-junk");
            let net = Arc::new(Fake::new().post(OVERPASS, text.as_bytes()));
            let (o, _, _) = client(net.clone(), &dir);
            let e = o.fetch(&bbox(), "q").unwrap_err();
            assert!(e.contains(says), "{text}: {e}");
            assert!(o.fetch(&bbox(), "q").is_err());
            assert_eq!(net.asked().len(), 2, "{text}: not kept");
        }
        let net = Arc::new(Fake::new().post(OVERPASS, &[0xff, 0xfe, b'{']));
        let (o, _, _) = client(net, &Scratch::new("op-bytes"));
        assert!(o.fetch(&bbox(), "q").unwrap_err().contains("text"));
        // A remark that is only a note (a warning) is not an error.
        let note = r#"{"version":0.6,"elements":[],"remark":"just so you know"}"#;
        let (o, _, _) = client(Arc::new(Fake::new().post(OVERPASS, note.as_bytes())), &Scratch::new("op-note"));
        assert!(o.fetch(&bbox(), "q").is_ok());
    }

    #[test]
    fn a_server_the_user_named_is_the_one_asked() {
        let dir = Scratch::new("op-own");
        let own = "https://overpass.example.org/api/interpreter";
        let net = Arc::new(Fake::new().post(own, GOOD.as_bytes()));
        let cache = Arc::new(Cache::new(Some(dir.0.clone())));
        let o = Overpass::new(net.clone(), cache, own);
        o.fetch(&bbox(), "q").unwrap();
        assert_eq!(net.asked()[0].1, own);
    }
}
