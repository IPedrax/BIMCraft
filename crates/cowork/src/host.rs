//! The host's side: a listening port, a thread for each guest that reads what it says, a writer thread for each that sends
//! what the window queues, and the discovery answer. The window takes [`HostEvent`]s and sends with [`Host::send`] and
//! [`Host::broadcast`]; presence is relayed to the other guests by the guest's own thread, without waiting for a frame.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError, channel, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::noise::{self, PROLOGUE};
use crate::wire::{self, Out, QUEUE};
use crate::{Code, MAX_GUESTS, MAX_NAME, MAX_RATE, Timing, Wake, discovery};

/// How many connections may be in their handshake at once: the rest are closed as they come.
const MAX_PENDING: usize = 16;
/// Wrong codes from one address before it waits.
const LOCKOUT_AFTER: u32 = 5;
/// What guests are told when the host goes without saying why.
const CLOSED: &str = "the host closed the session";

pub struct HostConfig {
    /// Where it listens: 0.0.0.0 and the session port, unless the user says otherwise.
    pub addr: SocketAddr,
    /// The host's version: a guest must have the same.
    pub version: String,
    /// What the LAN is told: the host's name and the project's.
    pub name: String,
    pub project: String,
    /// The UDP port it answers "who hosts?" on, or none when it isn't visible on the network (0: any, for tests).
    pub discovery: Option<u16>,
    pub timing: Timing,
    pub wake: Wake,
}

/// What happened, for the host's window.
#[derive(Debug)]
pub enum HostEvent {
    /// A guest came in: its number, its name (made safe), its colour (by join order, the host's is 0) and its hello as it said it.
    Joined { peer: u32, name: String, colour: usize, hello: Value },
    /// A guest said something (presence too, which the other guests already have).
    Message { peer: u32, msg: Value },
    /// A guest went: it left, was kicked, fell silent, flooded or broke the line.
    Left { peer: u32, why: String },
}

/// A guest in the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub id: u32,
    pub name: String,
    pub colour: usize,
}

struct Peer {
    person: Person,
    tx: SyncSender<Out>,
    stream: TcpStream,
    writer: Option<JoinHandle<()>>,
}

/// What the host's threads share.
struct Shared {
    code: Code,
    version: String,
    timing: Timing,
    wake: Wake,
    events: Sender<HostEvent>,
    stop: AtomicBool,
    peers: Mutex<Vec<Peer>>,
    next: AtomicU32,
    pending: AtomicUsize,
    /// Wrong codes by address: how many, and when the last was.
    failures: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn event(&self, e: HostEvent) {
        let _ = self.events.send(e);
        (self.wake)();
    }

    fn locked_out(&self, ip: IpAddr) -> bool {
        let mut failures = lock(&self.failures);
        match failures.get(&ip) {
            Some((n, last)) if *n >= LOCKOUT_AFTER && last.elapsed() < self.timing.lockout => true,
            Some((n, _)) if *n >= LOCKOUT_AFTER => {
                failures.remove(&ip);
                false
            }
            _ => false,
        }
    }

    fn failed(&self, ip: IpAddr) {
        let mut failures = lock(&self.failures);
        let entry = failures.entry(ip).or_insert((0, Instant::now()));
        *entry = (entry.0 + 1, Instant::now());
    }

    /// Takes `id` out of the session, once: the writer hangs up after what it has queued, and the window hears why.
    fn remove(&self, id: u32, why: &str) -> Option<Peer> {
        let peer = {
            let mut peers = lock(&self.peers);
            let at = peers.iter().position(|p| p.person.id == id)?;
            peers.remove(at)
        };
        let _ = peer.tx.try_send(Out::End);
        self.event(HostEvent::Left { peer: id, why: why.to_string() });
        Some(peer)
    }

    /// Queues `out` for `peer`; one that isn't keeping up is hung up on.
    fn queue(peer: &Peer, out: Out) -> bool {
        match peer.tx.try_send(out) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                let _ = peer.stream.shutdown(Shutdown::Both);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

pub struct Host {
    shared: Arc<Shared>,
    events: Receiver<HostEvent>,
    /// Where the listener is, as this machine reaches it: to wake its thread when the host closes.
    local: SocketAddr,
    discovery: Option<u16>,
    closed: bool,
}

impl Host {
    /// Listens where `config` says, with a new code, and answers on the LAN if it says so. A discovery port that can't be had
    /// (another host on this machine has it) leaves the host listening but out of sight: [`Host::discovery_port`] says none.
    pub fn start(config: HostConfig) -> io::Result<Host> {
        let code = Code::new().map_err(io::Error::other)?;
        let listener = TcpListener::bind(config.addr)?;
        let at = listener.local_addr()?;
        let local = match at.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => SocketAddr::from((Ipv4Addr::LOCALHOST, at.port())),
            IpAddr::V6(ip) if ip.is_unspecified() => SocketAddr::from((Ipv6Addr::LOCALHOST, at.port())),
            _ => at,
        };
        let (tx, events) = channel();
        let shared = Arc::new(Shared {
            code,
            version: config.version.clone(),
            timing: config.timing,
            wake: config.wake,
            events: tx,
            stop: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            next: AtomicU32::new(1),
            pending: AtomicUsize::new(0),
            failures: Mutex::new(HashMap::new()),
        });
        let discovery = config.discovery.and_then(|port| {
            let socket = UdpSocket::bind((at.ip(), port)).ok()?;
            let port = socket.local_addr().ok()?.port();
            let here = discovery::here(&config.name, &config.project, &config.version, at.port(), &shared.code.tag());
            let held = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("cowork-discovery".into())
                .spawn(move || discovery::answer(&socket, &here, &held.stop))
                .ok()?;
            Some(port)
        });
        let held = Arc::clone(&shared);
        std::thread::Builder::new().name("cowork-accept".into()).spawn(move || accept(&listener, &held))?;
        Ok(Host { shared, events, local, discovery, closed: false })
    }

    pub fn code(&self) -> &Code {
        &self.shared.code
    }

    /// The port guests connect to.
    pub fn port(&self) -> u16 {
        self.local.port()
    }

    /// The UDP port the host answers on, while it is visible on the network.
    pub fn discovery_port(&self) -> Option<u16> {
        self.discovery
    }

    /// The next thing that happened, if any.
    pub fn try_event(&self) -> Option<HostEvent> {
        self.events.try_recv().ok()
    }

    /// The guests, in join order.
    pub fn people(&self) -> Vec<Person> {
        lock(&self.shared.peers).iter().map(|p| p.person.clone()).collect()
    }

    /// Sends `msg` to one guest; false when it isn't there, or isn't keeping up (then it is hung up on).
    pub fn send(&self, peer: u32, msg: &Value) -> bool {
        let peers = lock(&self.shared.peers);
        peers.iter().find(|p| p.person.id == peer).is_some_and(|p| Shared::queue(p, Out::of(msg)))
    }

    /// Sends `msg` to every guest but `except`.
    pub fn broadcast(&self, msg: &Value, except: Option<u32>) {
        let bytes = msg.to_string().into_bytes();
        let bytes: Arc<[u8]> = bytes.into();
        for p in lock(&self.shared.peers).iter().filter(|p| Some(p.person.id) != except) {
            Shared::queue(p, Out::Frame(Arc::clone(&bytes)));
        }
    }

    /// Tells a guest it must go, and why, and lets it go.
    pub fn kick(&self, peer: u32, why: &str) {
        let Some(p) = lock(&self.shared.peers).iter().find(|p| p.person.id == peer).map(|p| p.tx.clone()) else { return };
        let _ = p.try_send(Out::of(&json!({"op": "ended", "why": why})));
        self.shared.remove(peer, &format!("kicked: {why}"));
    }

    /// Ends the session, telling every guest `why`.
    pub fn end(mut self, why: &str) {
        self.shut(why);
    }

    fn shut(&mut self, why: &str) {
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        self.shared.stop.store(true, Ordering::SeqCst);
        let peers = std::mem::take(&mut *lock(&self.shared.peers));
        for p in &peers {
            let _ = p.tx.try_send(Out::of(&json!({"op": "ended", "why": why})));
            let _ = p.tx.try_send(Out::End);
        }
        // What was said goes out before the window does: each writer is held to its write timeout.
        for mut p in peers {
            if let Some(w) = p.writer.take() {
                let _ = w.join();
            }
        }
        // The listening thread sees the stop when its next connection comes: this one.
        let _ = TcpStream::connect_timeout(&self.local, Duration::from_millis(500));
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.shut(CLOSED);
    }
}

fn accept(listener: &TcpListener, shared: &Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else {
            // Out of file descriptors, say: waited out, not spun on.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        if shared.pending.fetch_add(1, Ordering::SeqCst) >= MAX_PENDING {
            shared.pending.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let held = Arc::clone(shared);
        let spawned = std::thread::Builder::new().name("cowork-guest".into()).spawn(move || {
            let mut joined = None;
            let _ = serve(stream, &held, &mut joined);
            if joined.is_none() {
                held.pending.fetch_sub(1, Ordering::SeqCst);
            }
        });
        if spawned.is_err() {
            shared.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A name as the session shows it: no control characters, at most [`MAX_NAME`] characters, "Guest" for none.
fn clean(name: Option<&str>) -> String {
    let name: String = name.unwrap_or("").chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    let name = name.trim();
    if name.is_empty() { "Guest".into() } else { name.to_string() }
}

/// One connection: the handshake and the hello, then what the guest says until it goes. `joined` is its number once it is in.
fn serve(mut s: TcpStream, shared: &Shared, joined: &mut Option<u32>) -> io::Result<()> {
    let ip = s.peer_addr()?.ip();
    if shared.locked_out(ip) {
        return Ok(());
    }
    let timing = shared.timing;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(timing.handshake))?;
    s.set_write_timeout(Some(timing.handshake))?;
    let msg1 = wire::read_raw(&mut s)?;
    let Ok((_, msg2, mut t)) = noise::respond(&shared.code.psk(), PROLOGUE, &msg1, b"", &ring::rand::SystemRandom::new()) else {
        shared.failed(ip);
        return Ok(());
    };
    wire::write_raw(&mut s, &msg2)?;
    let hello = wire::read_frame(&mut s, &mut t.recv)?;
    lock(&shared.failures).remove(&ip);
    let version = hello["version"].as_str().unwrap_or("none");
    let refused = if hello["op"] != "hello" {
        Some("a guest says hello first".to_string())
    } else if version != shared.version {
        Some(format!(
            "the host runs BIMCraft {} and you run {}: a session needs the same version on both",
            shared.version,
            version.chars().take(32).collect::<String>()
        ))
    } else if shared.stop.load(Ordering::SeqCst) {
        Some(CLOSED.to_string())
    } else {
        None
    };
    let name = clean(hello["name"].as_str());
    // In, under the lock so two can't take the last place, unless refused or full.
    let mut peers = lock(&shared.peers);
    if let Some(why) = refused.or_else(|| (peers.len() >= MAX_GUESTS).then(|| format!("the session is full: it takes {MAX_GUESTS} guests")))
    {
        drop(peers);
        return wire::write_frame(&mut s, &mut t.send, &json!({"op": "ended", "why": why}));
    }
    let colour = (1..).find(|c| peers.iter().all(|p| p.person.colour != *c)).unwrap_or(0);
    let person = Person { id: shared.next.fetch_add(1, Ordering::SeqCst), name: name.clone(), colour };
    let (tx, rx) = sync_channel(QUEUE);
    let (w, ping) = (s.try_clone()?, timing.ping);
    let writer = std::thread::Builder::new().name("cowork-writer".into()).spawn(move || wire::writer(w, t.send, rx, ping))?;
    peers.push(Peer { person: person.clone(), tx: tx.clone(), stream: s.try_clone()?, writer: Some(writer) });
    drop(peers);
    let id = person.id;
    *joined = Some(id);
    shared.pending.fetch_sub(1, Ordering::SeqCst);
    s.set_read_timeout(Some(timing.silence))?;
    s.set_write_timeout(Some(timing.silence))?;
    shared.event(HostEvent::Joined { peer: id, name, colour: person.colour, hello });
    let why = listen(&mut s, &mut t.recv, shared, id, &tx);
    shared.remove(id, &why);
    Ok(())
}

/// What a guest says, until it goes; why it went.
fn listen(s: &mut TcpStream, recv: &mut noise::Cipher, shared: &Shared, id: u32, tx: &SyncSender<Out>) -> String {
    let (mut since, mut count) = (Instant::now(), 0u32);
    loop {
        let msg = match wire::read_frame(s, recv) {
            Ok(msg) => msg,
            Err(e) if wire::timed_out(&e) => return "sent nothing for a while: the line may have broken".into(),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return "the connection closed".into(),
            Err(e) => return format!("the connection broke: {e}"),
        };
        if since.elapsed() >= Duration::from_secs(1) {
            (since, count) = (Instant::now(), 0);
        }
        count += 1;
        if count > MAX_RATE {
            let _ = tx.try_send(Out::of(&json!({"op": "ended", "why": "you sent too many messages"})));
            return "sent too many messages".into();
        }
        match msg["op"].as_str() {
            Some("ping") => {}
            Some("bye") => return "left".into(),
            Some("presence") => {
                let mut relayed = msg.clone();
                relayed["from"] = json!(id);
                let bytes: Arc<[u8]> = relayed.to_string().into_bytes().into();
                for p in lock(&shared.peers).iter().filter(|p| p.person.id != id) {
                    // Presence is the latest of a stream: one that doesn't fit is let go.
                    let _ = p.tx.try_send(Out::Frame(Arc::clone(&bytes)));
                }
                shared.event(HostEvent::Message { peer: id, msg });
            }
            _ => shared.event(HostEvent::Message { peer: id, msg }),
        }
    }
}
