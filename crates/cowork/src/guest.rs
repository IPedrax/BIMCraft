//! The guest's side: one connection to the host, a thread that reads it and a writer thread. [`Guest::join`] returns once the
//! host's window has said its welcome, or with why it didn't.

use std::io;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};

use crate::noise::{self, Cipher, PROLOGUE};
use crate::wire::{self, Out, QUEUE};
use crate::{Code, Timing, Wake};

/// How long a guest waits for the window's welcome once it is in: the window builds it, the whole model with it.
const WELCOME: Duration = Duration::from_secs(30);
/// The longest reason for an ending kept, in characters.
const MAX_WHY: usize = 300;

/// What happened, for the guest's window.
#[derive(Debug)]
pub enum GuestEvent {
    /// The host said something.
    Message(Value),
    /// The host ended the session, closed, or sent this guest away, and why.
    Ended(String),
    /// The host went without a word: the line broke, or it stopped.
    Lost(String),
}

pub struct Guest {
    tx: SyncSender<Out>,
    events: Receiver<GuestEvent>,
    stream: TcpStream,
    writer: Option<JoinHandle<()>>,
}

impl Guest {
    /// Joins the host at `addr` with `code`, says `hello` (the window's: its version, the person's name and the rest), and waits
    /// for the welcome. The error says why not, in words for the person joining.
    pub fn join(addr: SocketAddr, code: &Code, hello: Value, timing: Timing, wake: Wake) -> Result<(Guest, Value), String> {
        let mut s = TcpStream::connect_timeout(&addr, timing.handshake).map_err(|e| format!("couldn't reach {addr}: {e}"))?;
        let (guest, first) = Guest::on(&mut s, code, &hello, timing, wake).map_err(|e| format!("lost {addr} while joining: {e}"))?;
        let first = first.map_err(|why| why.to_string())?;
        match first {
            GuestEvent::Message(welcome) => Ok((guest, welcome)),
            GuestEvent::Ended(why) | GuestEvent::Lost(why) => Err(why),
        }
    }

    /// The handshake, the hello and the threads on a connection; then the first thing the host said.
    #[allow(clippy::type_complexity)]
    fn on(
        s: &mut TcpStream,
        code: &Code,
        hello: &Value,
        timing: Timing,
        wake: Wake,
    ) -> io::Result<(Guest, Result<GuestEvent, &'static str>)> {
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(timing.handshake))?;
        s.set_write_timeout(Some(timing.handshake))?;
        let (initiator, msg1) = noise::initiate(&code.psk(), PROLOGUE, b"", &ring::rand::SystemRandom::new()).map_err(io::Error::other)?;
        wire::write_raw(s, &msg1)?;
        let Ok(msg2) = wire::read_raw(s) else {
            return Err(io::Error::other(
                "the host closed the connection during the handshake: check the code (after five wrong ones, wait a moment)",
            ));
        };
        let (_, t) = initiator.finish(&msg2).map_err(|e| io::Error::other(format!("{e}: check the code")))?;
        let (mut send, recv) = (t.send, t.recv);
        wire::write_frame(s, &mut send, hello)?;
        s.set_read_timeout(Some(timing.silence))?;
        s.set_write_timeout(Some(timing.silence))?;
        let (tx, rx) = sync_channel(QUEUE);
        let w = s.try_clone()?;
        let writer = std::thread::Builder::new().name("cowork-writer".into()).spawn(move || wire::writer(w, send, rx, timing.ping))?;
        let (etx, events) = channel();
        let r = s.try_clone()?;
        std::thread::Builder::new().name("cowork-host".into()).spawn(move || read(r, recv, &etx, &wake))?;
        let guest = Guest { tx, events, stream: s.try_clone()?, writer: Some(writer) };
        // What the host sent before its welcome (a change being made as this guest came in) is in the welcome.
        let until = std::time::Instant::now() + WELCOME;
        let first = loop {
            match guest.events.recv_timeout(until.saturating_duration_since(std::time::Instant::now())) {
                Ok(GuestEvent::Message(m)) if m["op"] != "welcome" => {}
                Ok(e) => break Ok(e),
                Err(RecvTimeoutError::Timeout) => break Err("the host's window didn't answer"),
                Err(RecvTimeoutError::Disconnected) => break Err("the connection to the host closed"),
            }
        };
        Ok((guest, first))
    }

    /// Sends `msg` to the host; false once the connection is gone.
    pub fn send(&self, msg: &Value) -> bool {
        self.tx.try_send(Out::of(msg)).is_ok()
    }

    /// The next thing that happened, if any.
    pub fn try_event(&self) -> Option<GuestEvent> {
        self.events.try_recv().ok()
    }

    /// The next thing that happens within `wait`: a window waiting for the host's answer to a command.
    pub fn wait_event(&self, wait: Duration) -> Option<GuestEvent> {
        self.events.recv_timeout(wait).ok()
    }
}

impl Drop for Guest {
    /// Says it leaves, and hangs up once that is out.
    fn drop(&mut self) {
        let _ = self.tx.try_send(Out::of(&json!({"op": "bye"})));
        let _ = self.tx.try_send(Out::End);
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

/// What the host says, until it ends or goes.
fn read(mut s: TcpStream, mut recv: Cipher, events: &Sender<GuestEvent>, wake: &Wake) {
    let last = loop {
        match wire::read_frame(&mut s, &mut recv) {
            Ok(msg) => match msg["op"].as_str() {
                Some("ping") => {}
                Some("ended") => {
                    let why = msg["why"].as_str().unwrap_or("the host ended the session");
                    break GuestEvent::Ended(why.chars().filter(|c| !c.is_control()).take(MAX_WHY).collect());
                }
                _ => {
                    if events.send(GuestEvent::Message(msg)).is_err() {
                        return;
                    }
                    wake();
                }
            },
            Err(e) if wire::timed_out(&e) => break GuestEvent::Lost("no word from the host for a while: the line may have broken".into()),
            Err(_) => break GuestEvent::Lost("the connection to the host closed".into()),
        }
    };
    let _ = events.send(last);
    wake();
}
