//! Asking to join without the code: a guest knocks, the host's window lets it in or turns it away, and on the way both windows
//! show the same six digits, which the two people compare (side by side, on a call) before the host lets anyone in. The handshake
//! is the session's own (`noise.rs`) under a key everyone knows, so it is encrypted but proves nothing by itself: the digits are what
//! prove there is nobody in between. They come from the handshake and a nonce from each side, the guest's committed to before it
//! sees the host's, so someone in between can't search for two handshakes whose digits agree (the host's nonce comes after the
//! guest's commitment on one side; on the other, the commitment is all it has seen). Let in, the guest is told the code and joins
//! with it as anyone does.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};

use crate::noise::{self, PROLOGUE};
use crate::{Code, Timing, wire};

/// How long a knock waits for the host's window to answer.
pub const KNOCK_WAIT: Duration = Duration::from_secs(120);

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut c = digest::Context::new(&digest::SHA256);
    parts.iter().for_each(|p| c.update(p));
    let mut out = [0u8; 32];
    out.copy_from_slice(c.finish().as_ref());
    out
}

/// The handshake's key for a knock, which everyone knows.
pub(crate) fn psk() -> [u8; 32] {
    sha256(&[b"bimcraft-cowork knock psk"])
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

/// 32 bytes written in hex, read strictly.
pub(crate) fn unhex(v: &Value) -> Option<[u8; 32]> {
    let s = v.as_str().filter(|s| s.len() == 64)?;
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A nonce of the system's randomness.
pub(crate) fn nonce() -> Result<[u8; 32], String> {
    let mut n = [0u8; 32];
    SystemRandom::new().fill(&mut n).map_err(|_| "the system gave no random numbers".to_string())?;
    Ok(n)
}

/// What a guest sends of its nonce before it sees the host's.
pub(crate) fn commit(nonce: &[u8; 32]) -> String {
    hex(&sha256(&[b"bimcraft-cowork knock commit", nonce]))
}

/// Whether `nonce` is the one `commitment` was made of.
pub(crate) fn kept(commitment: &[u8; 32], nonce: &[u8; 32]) -> bool {
    hex(commitment) == commit(nonce)
}

/// The digits both windows show: six, as "482 913", from the handshake and the two nonces.
pub(crate) fn digits(hash: &[u8; 32], guest: &[u8; 32], host: &[u8; 32]) -> String {
    let d = sha256(&[b"bimcraft-cowork knock digits", hash, guest, host]);
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

/// Knocks at the host at `addr` as `name`: `shown` has the digits as soon as both sides know them, for the person to compare
/// with the host's, and the answer is the code (let in) or why not, in words for the person knocking.
pub fn knock(addr: SocketAddr, name: &str, version: &str, timing: Timing, shown: impl FnOnce(&str)) -> Result<Code, String> {
    let lost = |e: std::io::Error| format!("lost {addr} while knocking: {e}");
    let mut s = TcpStream::connect_timeout(&addr, timing.handshake).map_err(|e| format!("couldn't reach {addr}: {e}"))?;
    s.set_read_timeout(Some(timing.handshake)).map_err(lost)?;
    s.set_write_timeout(Some(timing.handshake)).map_err(lost)?;
    let (initiator, msg1) = noise::initiate(&psk(), PROLOGUE, b"", &SystemRandom::new())?;
    let Ok(msg2) = wire::write_raw(&mut s, &msg1).and_then(|()| wire::read_raw(&mut s)) else {
        return Err("the host doesn't take knocks: ask for the code".into());
    };
    let (_, mut t) = initiator.finish(&msg2)?;
    let mine = nonce()?;
    let knock = json!({"op": "knock", "version": version, "name": name, "commit": commit(&mine)});
    wire::write_frame(&mut s, &mut t.send, &knock).map_err(lost)?;
    let said = wire::read_frame(&mut s, &mut t.recv).map_err(lost)?;
    let Some(theirs) = unhex(&said["nonce"]).filter(|_| said["op"] == "nonce") else { return Err(why(&said)) };
    wire::write_frame(&mut s, &mut t.send, &json!({"op": "reveal", "nonce": hex(&mine)})).map_err(lost)?;
    shown(&digits(&t.hash, &mine, &theirs));
    s.set_read_timeout(Some(KNOCK_WAIT)).map_err(lost)?;
    let said = wire::read_frame(&mut s, &mut t.recv).map_err(|_| "the host didn't answer the knock".to_string())?;
    match said["code"].as_str().filter(|_| said["op"] == "let_in") {
        Some(code) => Code::parse(code),
        None => Err(why(&said)),
    }
}

/// Why the host ended a knock, as it said, made safe.
fn why(said: &Value) -> String {
    let why = said["why"].as_str().unwrap_or("the host ended the knock");
    why.chars().filter(|c| !c.is_control()).take(300).collect()
}
