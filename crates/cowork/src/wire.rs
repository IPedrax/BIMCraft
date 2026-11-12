//! What goes on a connection: the two handshake messages, each a 2-byte length and the message, then frames, each a 4-byte
//! length and the sealed message. A length is checked before anything is read; a message is a JSON object with an `op`. One
//! longer than [`DEFLATE_OVER`] goes deflated, marked by a first byte JSON never starts with, and is inflated no further than
//! [`MAX_INFLATED`].
//! And the writer thread both sides run: what the window queues goes out in order, and a ping when there has been nothing.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Value, json};

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
        return Err(invalid("a message that unpacks past any a session sends"));
    }
    Ok(out)
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
    let packed;
    let msg = if msg.len() > DEFLATE_OVER {
        packed = [&[DEFLATED][..], &deflate(msg)?].concat();
        &packed
    } else {
        msg
    };
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
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME + 16 {
        return Err(invalid("a message longer than any may be"));
    }
    let mut sealed = vec![0u8; len];
    r.read_exact(&mut sealed)?;
    let mut plain = cipher.open(&sealed).map_err(invalid)?;
    if plain.first() == Some(&DEFLATED) {
        plain = inflate(&plain[1..], MAX_INFLATED)?;
    }
    let msg: Value = serde_json::from_slice(&plain).map_err(|e| invalid(format!("not JSON: {e}")))?;
    match msg.get("op").and_then(Value::as_str) {
        Some(op) if !op.is_empty() && op.len() <= MAX_OP => Ok(msg),
        _ => Err(invalid("not a message: an object with an `op`")),
    }
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

/// A side's writer: sends what comes in `rx`, in order, and a ping after `ping` with nothing; hangs up at [`Out::End`], when the
/// queue goes, or when the peer stops taking what it is sent (the stream's write timeout).
pub(crate) fn writer(mut stream: TcpStream, mut cipher: Cipher, rx: Receiver<Out>, ping: Duration) {
    let pinged: Arc<[u8]> = json!({"op": "ping"}).to_string().into_bytes().into();
    loop {
        let bytes = match rx.recv_timeout(ping) {
            Ok(Out::Frame(bytes)) => bytes,
            Err(RecvTimeoutError::Timeout) => Arc::clone(&pinged),
            Ok(Out::End) | Err(RecvTimeoutError::Disconnected) => break,
        };
        if write_bytes(&mut stream, &mut cipher, &bytes).is_err() {
            break;
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}
