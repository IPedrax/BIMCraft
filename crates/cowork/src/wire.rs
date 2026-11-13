//! What goes on a connection: the two handshake messages, each a 2-byte length and the message, then frames, each a 4-byte
//! length and the sealed message. A length is checked before anything is read; a message is a JSON object with an `op`. One
//! longer than [`DEFLATE_OVER`] goes deflated, marked by a first byte JSON never starts with, and is inflated no further than
//! [`MAX_INFLATED`]. One longer than a frame holds ([`MAX_FRAME`], a whole model) goes in parts, the first saying how long the
//! whole is; only a guest reads those, of its host, held to the memory its machine has free (issue #438).
//! And the writer thread both sides run: what the window queues goes out in order, and a ping when there has been nothing.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Value, json};

use bimcraft_core::memory::{Budget, bytes};

use crate::noise::Cipher;
use crate::{MAX_FRAME, MAX_INFLATED};

/// The longest handshake message read: a key, a tag and a small payload.
const MAX_RAW: usize = 1024;
/// The longest an `op` may be.
const MAX_OP: usize = 32;
/// How many messages may wait for a peer's writer: past it, the peer isn't keeping up and is dropped.
pub(crate) const QUEUE: usize = 1024;
/// A message longer than this goes deflated.
const DEFLATE_OVER: usize = 64 << 10;
/// The first byte of a deflated message.
const DEFLATED: u8 = 1;
/// The first byte of the first part of a message in parts; the whole's length in bytes follows, eight of them.
const PARTS: u8 = 2;
/// The first byte of each part after the first.
const PART: u8 = 3;

pub(crate) fn deflate(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(bytes)?;
    e.finish()
}

/// `bytes` inflated, or an error past `limit` bytes: a small message may not unpack into a huge one.
pub(crate) fn inflate(bytes: &[u8], limit: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(bytes).take(limit as u64 + 1).read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(io::Error::new(io::ErrorKind::OutOfMemory, "a message that unpacks past any a session sends"));
    }
    Ok(out)
}

/// Why a message from the host is too big for this machine, as the guest's window says it.
fn too_big(why: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, format!("the host's project: {why}"))
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

pub(crate) fn write_raw(w: &mut impl Write, msg: &[u8]) -> io::Result<()> {
    let len = u16::try_from(msg.len()).map_err(|_| invalid("a handshake message too long"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(msg)?;
    w.flush()
}

pub(crate) fn read_raw(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len)?;
    let len = usize::from(u16::from_be_bytes(len));
    if len > MAX_RAW {
        return Err(invalid("not a handshake"));
    }
    let mut msg = vec![0u8; len];
    r.read_exact(&mut msg)?;
    Ok(msg)
}

pub(crate) fn write_bytes(w: &mut impl Write, cipher: &mut Cipher, msg: &[u8]) -> io::Result<()> {
    write_in(w, cipher, msg, MAX_FRAME)
}

/// [`write_bytes`] in frames of at most `most` bytes (the tests' are small): a message longer goes in parts.
pub(crate) fn write_in(w: &mut impl Write, cipher: &mut Cipher, msg: &[u8], most: usize) -> io::Result<()> {
    let packed;
    let msg = if msg.len() > DEFLATE_OVER {
        packed = [&[DEFLATED][..], &deflate(msg)?].concat();
        &packed
    } else {
        msg
    };
    if msg.len() <= most {
        return write_sealed(w, cipher, msg);
    }
    let (first, rest) = msg.split_at(most.saturating_sub(9).max(1).min(msg.len()));
    write_sealed(w, cipher, &[&[PARTS][..], &(msg.len() as u64).to_be_bytes(), first].concat())?;
    for part in rest.chunks(most.saturating_sub(1).max(1)) {
        write_sealed(w, cipher, &[&[PART][..], part].concat())?;
    }
    Ok(())
}

fn write_sealed(w: &mut impl Write, cipher: &mut Cipher, msg: &[u8]) -> io::Result<()> {
    let sealed = cipher.seal(msg).map_err(invalid)?;
    let len = u32::try_from(sealed.len()).map_err(|_| invalid("a message too long"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&sealed)?;
    w.flush()
}

pub(crate) fn write_frame(w: &mut impl Write, cipher: &mut Cipher, msg: &Value) -> io::Result<()> {
    write_bytes(w, cipher, msg.to_string().as_bytes())
}

/// The next message: a frame no longer than [`MAX_FRAME`], that opens, and holds a message.
pub(crate) fn read_frame(r: &mut impl Read, cipher: &mut Cipher) -> io::Result<Value> {
    read_message(r, cipher, None)
}

/// The next message, where it may come in parts and be as large as this machine has memory free for (the core's `memory.rs`):
/// a guest reading its host, told by `coming` when a message in parts begins. Without `coming`, [`read_frame`].
pub(crate) fn read_message(r: &mut impl Read, cipher: &mut Cipher, coming: Option<&mut dyn FnMut()>) -> io::Result<Value> {
    let mut plain = read_sealed(r, cipher)?;
    let mut most = MAX_INFLATED as u64;
    // Only a big message is measured: one in parts, or deflated.
    if let Some(coming) = coming
        && matches!(plain.first(), Some(&PARTS | &DEFLATED))
    {
        let total = match plain.first() {
            Some(&PARTS) => {
                plain.get(1..9).and_then(|b| b.try_into().ok()).map(u64::from_be_bytes).ok_or_else(|| invalid("parts of no length"))?
            }
            _ => plain.len() as u64,
        };
        let budget = Budget::start(total, 1).map_err(too_big)?;
        if budget.is_none() && total > most {
            return Err(too_big(format!("it is {} to receive, past the {} a session sends", bytes(total), bytes(most))));
        }
        if plain.first() == Some(&PARTS) {
            coming();
            plain = gather(r, cipher, &plain, total, budget.as_ref())?;
        }
        if let Some(left) = budget.as_ref().and_then(Budget::left) {
            most = left;
        }
    }
    match plain.first() {
        Some(&PARTS | &PART) => return Err(invalid("a message in parts, which only a host sends")),
        Some(&DEFLATED) => {
            plain = inflate(&plain[1..], usize::try_from(most).unwrap_or(usize::MAX)).map_err(|e| match e.kind() {
                io::ErrorKind::OutOfMemory => {
                    too_big(format!("it unpacks to more than the {} of memory this machine has free", bytes(most)))
                }
                _ => e,
            })?;
        }
        _ => {}
    }
    let msg: Value = serde_json::from_slice(&plain).map_err(|e| invalid(format!("not JSON: {e}")))?;
    match msg.get("op").and_then(Value::as_str) {
        Some(op) if !op.is_empty() && op.len() <= MAX_OP => Ok(msg),
        _ => Err(invalid("not a message: an object with an `op`")),
    }
}

/// A frame no longer than [`MAX_FRAME`], opened.
fn read_sealed(r: &mut impl Read, cipher: &mut Cipher) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME + 16 {
        return Err(invalid("a message longer than any may be"));
    }
    let mut sealed = vec![0u8; len];
    r.read_exact(&mut sealed)?;
    cipher.open(&sealed).map_err(invalid)
}

/// A message in parts, from its `first` part: `total` bytes in all, held to `budget` as each part comes.
fn gather(r: &mut impl Read, cipher: &mut Cipher, first: &[u8], total: u64, budget: Option<&Budget>) -> io::Result<Vec<u8>> {
    let total = usize::try_from(total).map_err(|_| too_big(format!("it is {} to receive", bytes(total))))?;
    let mut whole = Vec::new();
    whole.try_reserve_exact(total).map_err(|_| too_big(format!("this machine can't hold the {} it is to receive", bytes(total as u64))))?;
    whole.extend_from_slice(first.get(9..).unwrap_or_default());
    if whole.len() > total {
        return Err(invalid("a message's parts that don't add up"));
    }
    while whole.len() < total {
        let part = read_sealed(r, cipher)?;
        match part.split_first() {
            Some((&PART, part)) if whole.len() + part.len() <= total => whole.extend_from_slice(part),
            _ => return Err(invalid("a message's parts that don't add up")),
        }
        if let Some(b) = budget {
            b.check().map_err(too_big)?;
        }
    }
    Ok(whole)
}

/// Whether a read stopped because nothing came for the read timeout.
pub(crate) fn timed_out(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

/// What a writer thread is given.
pub(crate) enum Out {
    /// A message, as JSON: made once for every peer it goes to.
    Frame(Arc<[u8]>),
    /// Hang up, after what was queued before.
    End,
}

impl Out {
    pub(crate) fn of(msg: &Value) -> Out {
        Out::Frame(msg.to_string().into_bytes().into())
    }
}

/// A side's writer: sends what comes in `rx`, in order, in frames of at most `frame` bytes, and a ping after `ping` with nothing;
/// hangs up at [`Out::End`], when the queue goes, or when the peer stops taking what it is sent (the stream's write timeout).
pub(crate) fn writer(mut stream: TcpStream, mut cipher: Cipher, rx: Receiver<Out>, ping: Duration, frame: usize) {
    let pinged: Arc<[u8]> = json!({"op": "ping"}).to_string().into_bytes().into();
    loop {
        let bytes = match rx.recv_timeout(ping) {
            Ok(Out::Frame(bytes)) => bytes,
            Err(RecvTimeoutError::Timeout) => Arc::clone(&pinged),
            Ok(Out::End) | Err(RecvTimeoutError::Disconnected) => break,
        };
        if write_in(&mut stream, &mut cipher, &bytes, frame.min(MAX_FRAME)).is_err() {
            break;
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}
