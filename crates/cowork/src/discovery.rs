//! Finding hosts on the LAN: a guest asks "who hosts?" in one small UDP datagram, and each host that is visible answers with
//! its name, its project's, its version, the port guests connect to and its code's tag. Nothing secret goes either way, and
//! what comes back is read strictly: a datagram that isn't an answer is passed over.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Both kinds of datagram say this, so nothing else on the port is taken for one.
const MAGIC: &str = "bimcraft-cowork";
/// The longest datagram read.
const MAX_DATAGRAM: usize = 1024;

/// A host that answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where to join it: the address it answered from, and its session port.
    pub addr: SocketAddr,
    pub name: String,
    pub project: String,
    pub version: String,
    /// Its code's tag ([`crate::Code::tag`]).
    pub tag: String,
}

fn who() -> Vec<u8> {
    json!({MAGIC: 1, "op": "who"}).to_string().into_bytes()
}

/// What a visible host answers.
pub(crate) fn here(name: &str, project: &str, version: &str, port: u16, tag: &str) -> Vec<u8> {
    json!({MAGIC: 1, "op": "here", "name": name, "project": project, "version": version, "port": port, "tag": tag}).to_string().into_bytes()
}

fn read(bytes: &[u8], op: &str) -> Option<Value> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    (v[MAGIC] == 1 && v["op"] == op).then_some(v)
}

/// The host's side: answers each "who hosts?" until `stop`.
pub(crate) fn answer(socket: &UdpSocket, here: &[u8], stop: &AtomicBool) {
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    let mut buf = [0u8; MAX_DATAGRAM];
    while !stop.load(Ordering::SeqCst) {
        if let Ok((n, from)) = socket.recv_from(&mut buf)
            && read(&buf[..n], "who").is_some()
        {
            let _ = socket.send_to(here, from);
        }
    }
}

/// An answer read, as a host at the address it came from.
fn found(bytes: &[u8], from: SocketAddr) -> Option<Found> {
    let v = read(bytes, "here")?;
    let text = |key: &str, max: usize| v[key].as_str().filter(|s| s.chars().count() <= max && !s.chars().any(char::is_control));
    let tag = text("tag", 8).filter(|t| t.len() == 8 && t.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let port = v["port"].as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p > 0)?;
    Some(Found {
        addr: SocketAddr::new(from.ip(), port),
        name: text("name", crate::MAX_NAME)?.to_string(),
        project: text("project", 200)?.to_string(),
        version: text("version", 32)?.to_string(),
        tag: tag.to_string(),
    })
}

/// Asks `targets` who hosts (the LAN's broadcast address and the discovery port, in the window) and lists the hosts that
/// answer within `wait`, each once.
pub fn discover(targets: &[SocketAddr], wait: Duration) -> Vec<Found> {
    let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else { return Vec::new() };
    let _ = socket.set_broadcast(true);
    for t in targets {
        let _ = socket.send_to(&who(), t);
    }
    let until = Instant::now() + wait;
    let mut hosts: Vec<Found> = Vec::new();
    let mut buf = [0u8; MAX_DATAGRAM];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || socket.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(f) = found(&buf[..n], from).filter(|f| !hosts.contains(f)) {
                    hosts.push(f);
                }
            }
            Err(e) if crate::wire::timed_out(&e) => break,
            // A target with nothing on the port says so (connection refused): the others may still answer.
            Err(_) => {}
        }
    }
    hosts
}
