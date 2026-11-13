//! The transport driven as the desktop drives it, on this machine's address only: a host and its guests, the code, the
//! handshake against the published Noise vectors, what a hostile peer can't do, and finding a host on the LAN.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::noise;
use crate::wire;
use crate::{Code, Guest, GuestEvent, Host, HostConfig, HostEvent, MAX_GUESTS, Timing, discover, knock};

const VERSION: &str = "9.9.9";

/// Short times, so a test sees a ping, a silence and a lockout in a moment.
fn quick() -> Timing {
    Timing {
        ping: Duration::from_millis(60),
        silence: Duration::from_millis(400),
        handshake: Duration::from_millis(800),
        lockout: Duration::from_millis(500),
    }
}

fn host() -> Host {
    host_with(None)
}

fn host_with(discovery: Option<u16>) -> Host {
    let config = HostConfig {
        addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        version: VERSION.into(),
        name: "Ana".into(),
        project: "House".into(),
        discovery,
        timing: quick(),
        wake: Arc::new(|| {}),
    };
    Host::start(config).unwrap()
}

fn at(host: &Host) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, host.port()))
}

fn hello(name: &str) -> Value {
    json!({"op": "hello", "version": VERSION, "name": name})
}

/// The next host event, waited for.
fn event(host: &Host) -> HostEvent {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(e) = host.try_event() {
            return e;
        }
        assert!(Instant::now() < until, "no host event came");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The next guest event, waited for.
fn guest_event(guest: &Guest) -> GuestEvent {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(e) = guest.try_event() {
            return e;
        }
        assert!(Instant::now() < until, "no guest event came");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A guest joined and welcomed, as the desktop does it: the join waits for the welcome the host's window sends.
fn join(host: &Host, name: &str) -> (Guest, u32) {
    let code = host.code().clone();
    let addr = at(host);
    let name2 = name.to_string();
    let joining = std::thread::spawn(move || Guest::join(addr, &code, hello(&name2), quick(), Arc::new(|| {})));
    let HostEvent::Joined { peer, name: said, .. } = event(host) else { panic!("a join") };
    assert_eq!(said, name);
    assert!(host.send(peer, &json!({"op": "welcome", "you": peer})));
    let (guest, welcome) = joining.join().unwrap().unwrap();
    assert_eq!(welcome["you"], json!(peer));
    (guest, peer)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn a_code_is_sixteen_letters_that_read_back_forgivingly_and_refuse_typos() {
    let code = Code::new().unwrap();
    let text = code.to_string();
    assert_eq!(text.len(), 19);
    assert!(text.chars().enumerate().all(|(i, c)| if i % 5 == 4 { c == '-' } else { c.is_ascii_alphanumeric() }), "{text}");
    assert_ne!(Code::new().unwrap(), code, "each code is new");
    assert_eq!(Code::parse(&text).unwrap(), code);
    assert_eq!(Code::parse(&text.to_lowercase().replace('-', " ")).unwrap(), code, "lower case, spaces");
    assert_eq!(Code::parse(&text.replace('-', "")).unwrap(), code, "no dashes");
    // Letters that look alike read as the digits.
    let zero = Code::parse("0000-0000-0000-0001").unwrap();
    assert_eq!(Code::parse("oooo-OOOO-0000-000i").unwrap(), zero);
    assert_eq!(Code::parse("0000 0000 0000 000L").unwrap(), zero);
    for typo in ["", "0000-0000-0000-000", "0000-0000-0000-00000", "0000-0000-0000-000U", "0000-0000-0000-000!", "ZZZZ-ZZZZ-ZZZZ-ZZZZ-Z"] {
        assert!(Code::parse(typo).is_err(), "{typo}");
    }
    // The tag finds the host, and says nothing of the code.
    assert_eq!(code.tag(), Code::parse(&text).unwrap().tag());
    assert_ne!(code.tag(), zero.tag());
    assert_eq!(code.tag().len(), 8);
    assert!(!format!("{code:?}").contains(&text[..4]), "Debug keeps it secret");
}

#[test]
#[allow(deprecated)]
fn the_handshake_is_noise_nnpsk0_as_the_published_vectors_say() {
    // cacophony's vector for Noise_NNpsk0_25519_ChaChaPoly_SHA256.
    let prologue = hex("4a6f686e2047616c74");
    let psk: [u8; 32] = hex("54686973206973206d7920417573747269616e20706572737065637469766521").try_into().unwrap();
    let (ie, re) = (
        hex("893e28b9dc6ca8d611ab664754b8ceb7bac5117349a4439a6b0569da977c464a"),
        hex("bbdb4cdbd309f1a1f2e1456967fe288cadd6f712d65dc7b7793d5e63da6b375b"),
    );
    let messages = [
        (
            "4c756477696720766f6e204d69736573",
            "ca35def5ae56cec33dc2036731ab14896bc4c75dbb07a61f879f8e3afa4c794479b962b8aff8485742ac32f905ba45369e2465fb59e138a93d67a0d1266b6a54",
        ),
        (
            "4d757272617920526f746862617264",
            "95ebc60d2b1fa672c1f46a8aa265ef51bfe38e7ccb39ec5be34069f144808843d6062704d5a9c422a8e834423f8c1feada7e8d0d910a1a2cd030fb584221e3",
        ),
        ("462e20412e20486179656b", "e632c3763d7669067383433197a3baddf146e9e70ad4b4e9e59e0f"),
        ("4361726c204d656e676572", "64c6bee32ea91c8474bb4c21d7a700109ad45af77b29764ba5eb1e"),
    ];
    let (m0, m1, m2, m3) = (&messages[0], &messages[1], &messages[2], &messages[3]);
    let (initiator, msg1) = noise::initiate(&psk, &prologue, &hex(m0.0), &ring::test::rand::FixedSliceRandom { bytes: &ie }).unwrap();
    assert_eq!(msg1, hex(m0.1));
    let (said, msg2, mut responder) =
        noise::respond(&psk, &prologue, &msg1, &hex(m1.0), &ring::test::rand::FixedSliceRandom { bytes: &re }).unwrap();
    assert_eq!((said, msg2.clone()), (hex(m0.0), hex(m1.1)));
    let (said, mut i) = initiator.finish(&msg2).unwrap();
    assert_eq!(said, hex(m1.0));
    assert_eq!(i.hash, hex("f4d03dc34495c95729ea6de9e1b59004b59733102488b3e24bc441e0be208eaf").as_slice());
    assert_eq!(responder.hash, i.hash);
    let sealed = i.send.seal(&hex(m2.0)).unwrap();
    assert_eq!(sealed, hex(m2.1));
    assert_eq!(responder.recv.open(&sealed).unwrap(), hex(m2.0));
    let sealed = responder.send.seal(&hex(m3.0)).unwrap();
    assert_eq!(sealed, hex(m3.1));
    assert_eq!(i.recv.open(&sealed).unwrap(), hex(m3.0));
}

#[test]
fn a_wrong_code_a_tampered_frame_and_a_replayed_one_are_refused() {
    let rng = ring::rand::SystemRandom::new();
    let (a, b) = (Code::new().unwrap(), Code::new().unwrap());
    let (_, msg1) = noise::initiate(&a.psk(), b"p", b"", &rng).unwrap();
    assert!(noise::respond(&b.psk(), b"p", &msg1, b"", &rng).is_err(), "another code");
    assert!(noise::respond(&a.psk(), b"q", &msg1, b"", &rng).is_err(), "another prologue");
    let mut bent = msg1.clone();
    bent[40] ^= 1;
    assert!(noise::respond(&a.psk(), b"p", &bent, b"", &rng).is_err(), "a byte changed");
    assert!(noise::respond(&a.psk(), b"p", &msg1[..20], b"", &rng).is_err(), "too short");
    // After it: frames in order only.
    let (i, msg1) = noise::initiate(&a.psk(), b"p", b"", &rng).unwrap();
    let (_, msg2, mut r) = noise::respond(&a.psk(), b"p", &msg1, b"", &rng).unwrap();
    let (_, mut i) = i.finish(&msg2).unwrap();
    let (one, two) = (i.send.seal(b"one").unwrap(), i.send.seal(b"two").unwrap());
    let mut bent = one.clone();
    bent[0] ^= 1;
    let fresh = || r.recv.clone();
    assert!(fresh().open(&bent).is_err(), "tampered");
    assert!(fresh().open(&two).is_err(), "out of order");
    assert_eq!(r.recv.open(&one).unwrap(), b"one");
    assert!(r.recv.open(&one).is_err(), "replayed");
}

#[test]
fn a_host_and_two_guests_join_talk_relay_presence_leave_and_are_kicked() {
    let host = host();
    let (a, ida) = join(&host, "Bo");
    let (b, idb) = join(&host, "Cy");
    let people: Vec<(u32, String, usize)> = host.people().into_iter().map(|p| (p.id, p.name, p.colour)).collect();
    assert_eq!(people, vec![(ida, "Bo".into(), 1), (idb, "Cy".into(), 2)], "in join order, colours after the host's 0");
    // A command goes to the host's window; what it sends back comes in order.
    assert!(a.send(&json!({"op": "execute", "command": "wall", "ref": 1})));
    match event(&host) {
        HostEvent::Message { peer, msg } => assert_eq!((peer, msg["command"].clone()), (ida, json!("wall"))),
        other => panic!("{other:?}"),
    }
    host.send(ida, &json!({"op": "delta", "seq": 1}));
    host.send(ida, &json!({"op": "done", "ref": 1}));
    let ops: Vec<String> = (0..2)
        .map(|_| match guest_event(&a) {
            GuestEvent::Message(m) => m["op"].as_str().unwrap().to_string(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(ops, ["delta", "done"]);
    // Presence goes to the other guests at once, saying whose, and to the host's window.
    a.send(&json!({"op": "presence", "cursor": [1, 2]}));
    match guest_event(&b) {
        GuestEvent::Message(m) => assert_eq!((m["from"].clone(), m["cursor"].clone()), (json!(ida), json!([1, 2]))),
        other => panic!("{other:?}"),
    }
    assert!(matches!(event(&host), HostEvent::Message { peer, msg } if peer == ida && msg["op"] == "presence"));
    // A broadcast reaches both, but the one left out.
    host.broadcast(&json!({"op": "people"}), Some(idb));
    assert!(matches!(guest_event(&a), GuestEvent::Message(m) if m["op"] == "people"));
    // Quiet guests stay: their pings keep them.
    std::thread::sleep(quick().silence * 3);
    assert_eq!(host.people().len(), 2);
    assert!(a.try_event().is_none() && b.try_event().is_none(), "pings aren't events");
    // One leaves, one is kicked.
    drop(b);
    assert!(matches!(event(&host), HostEvent::Left { peer, .. } if peer == idb));
    host.kick(ida, "the host asked you to leave");
    assert!(matches!(guest_event(&a), GuestEvent::Ended(why) if why.contains("asked you to leave")));
    assert!(matches!(event(&host), HostEvent::Left { peer, .. } if peer == ida));
    assert!(host.people().is_empty());
    assert!(!host.send(ida, &json!({"op": "x"})), "gone");
    // Kicked is out for the session, from that address, code or not.
    let e = Guest::join(at(&host), host.code(), hello("Bo"), quick(), Arc::new(|| {})).err().unwrap();
    assert!(e.contains("closed"), "{e}");
}

#[test]
fn a_big_message_goes_deflated_and_a_bomb_is_refused() {
    let host = host();
    let (a, ida) = join(&host, "Bo");
    let big = "wall ".repeat(2 << 20);
    assert!(host.send(ida, &json!({"op": "delta", "model": big})));
    assert!(matches!(guest_event(&a), GuestEvent::Message(m) if m["model"].as_str().is_some_and(|s| s.len() == big.len())));
    // What inflates past the limit is no message.
    let packed = wire::deflate(&vec![b' '; 5000]).unwrap();
    assert!(packed.len() < 100);
    assert_eq!(wire::inflate(&packed, 5000).unwrap().len(), 5000);
    assert!(wire::inflate(&packed, 4999).is_err());
    assert!(wire::inflate(b"not deflate", 5000).is_err());
}

#[test]
fn a_join_waits_for_the_welcome_past_what_comes_before_it() {
    let host = host();
    let code = host.code().clone();
    let addr = at(&host);
    let joining = std::thread::spawn(move || Guest::join(addr, &code, hello("Bo"), quick(), Arc::new(|| {})));
    let HostEvent::Joined { peer, .. } = event(&host) else { panic!("a join") };
    host.send(peer, &json!({"op": "delta", "seq": 7}));
    host.send(peer, &json!({"op": "welcome", "seq": 7}));
    let (guest, welcome) = joining.join().unwrap().unwrap();
    assert_eq!(welcome["op"], "welcome");
    assert!(guest.wait_event(Duration::from_millis(50)).is_none(), "what came before the welcome is in it");
    host.send(peer, &json!({"op": "delta", "seq": 8}));
    assert!(matches!(guest.wait_event(Duration::from_secs(5)), Some(GuestEvent::Message(m)) if m["seq"] == 8));
}

#[test]
fn a_full_session_another_version_and_a_wrong_code_are_refused_saying_why() {
    let host = host();
    let guests: Vec<_> = (0..MAX_GUESTS).map(|n| join(&host, &format!("G{n}"))).collect();
    let e = Guest::join(at(&host), host.code(), hello("Late"), quick(), Arc::new(|| {})).err().unwrap();
    assert!(e.contains("full"), "{e}");
    drop(guests);
    while host.try_event().is_some() || !host.people().is_empty() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut old = hello("Old");
    old["version"] = json!("0.1.0");
    let e = Guest::join(at(&host), host.code(), old, quick(), Arc::new(|| {})).err().unwrap();
    assert!(e.contains("0.1.0") && e.contains(VERSION), "{e}");
    let e = Guest::join(at(&host), &Code::new().unwrap(), hello("Eve"), quick(), Arc::new(|| {})).err().unwrap();
    assert!(e.contains("code"), "{e}");
}

#[test]
fn five_wrong_codes_lock_an_address_out_for_a_while() {
    let host = host();
    for _ in 0..5 {
        assert!(Guest::join(at(&host), &Code::new().unwrap(), hello("Eve"), quick(), Arc::new(|| {})).is_err());
    }
    // The right code, from the same address, waits out the lockout.
    let started = Instant::now();
    assert!(Guest::join(at(&host), host.code(), hello("Bo"), quick(), Arc::new(|| {})).is_err(), "locked out");
    while started.elapsed() < quick().lockout + Duration::from_millis(100) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_bo, _) = join(&host, "Bo");
}

#[test]
fn a_host_that_closes_says_so_and_one_that_vanishes_is_noticed() {
    let host = host();
    let (a, _) = join(&host, "Bo");
    drop(host);
    assert!(matches!(guest_event(&a), GuestEvent::Ended(why) if why.contains("closed")));
    let host = self::host();
    let (b, _) = join(&host, "Cy");
    host.end("Ana opened another project");
    assert!(matches!(guest_event(&b), GuestEvent::Ended(why) if why == "Ana opened another project"));
    // A "host" that finishes the handshake, takes the hello and then says nothing.
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let code = Code::new().unwrap();
    let psk = code.psk();
    let silent = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let msg1 = wire::read_raw(&mut s).unwrap();
        let (_, msg2, mut t) = noise::respond(&psk, noise::PROLOGUE, &msg1, b"", &ring::rand::SystemRandom::new()).unwrap();
        wire::write_raw(&mut s, &msg2).unwrap();
        let _hello = wire::read_frame(&mut s, &mut t.recv).unwrap();
        wire::write_frame(&mut s, &mut t.send, &json!({"op": "welcome"})).unwrap();
        std::thread::sleep(quick().silence * 4);
    });
    let (guest, _) = Guest::join(addr, &code, hello("Bo"), quick(), Arc::new(|| {})).unwrap();
    assert!(matches!(guest_event(&guest), GuestEvent::Lost(_)));
    silent.join().unwrap();
}

/// A guest of our own making: the handshake and the hello, then whatever the test sends.
fn raw_guest(host: &Host) -> (TcpStream, noise::Transport) {
    let mut s = TcpStream::connect(at(host)).unwrap();
    let (i, msg1) = noise::initiate(&host.code().psk(), noise::PROLOGUE, b"", &ring::rand::SystemRandom::new()).unwrap();
    wire::write_raw(&mut s, &msg1).unwrap();
    let msg2 = wire::read_raw(&mut s).unwrap();
    let (_, mut t) = i.finish(&msg2).unwrap();
    wire::write_frame(&mut s, &mut t.send, &hello("Eve")).unwrap();
    assert!(matches!(event(host), HostEvent::Joined { .. }));
    (s, t)
}

#[test]
fn a_guest_that_tampers_goes_silent_floods_or_says_nonsense_is_dropped() {
    let host = host();
    // A changed byte.
    let (mut s, mut t) = raw_guest(&host);
    let mut sealed = t.send.seal(br#"{"op":"execute"}"#).unwrap();
    sealed[3] ^= 1;
    s.write_all(&(sealed.len() as u32).to_be_bytes()).unwrap();
    s.write_all(&sealed).unwrap();
    assert!(matches!(event(&host), HostEvent::Left { .. }));
    // A frame longer than any may be.
    let (mut s, _) = raw_guest(&host);
    s.write_all(&u32::MAX.to_be_bytes()).unwrap();
    assert!(matches!(event(&host), HostEvent::Left { .. }));
    // Not a message.
    for text in [&b"[1, 2]"[..], b"{\"no\": \"op\"}", b"not json"] {
        let (mut s, mut t) = raw_guest(&host);
        let sealed = t.send.seal(text).unwrap();
        s.write_all(&(sealed.len() as u32).to_be_bytes()).unwrap();
        s.write_all(&sealed).unwrap();
        assert!(matches!(event(&host), HostEvent::Left { .. }), "{}", String::from_utf8_lossy(text));
    }
    // Silent.
    let (_s, _t) = raw_guest(&host);
    assert!(matches!(event(&host), HostEvent::Left { why, .. } if why.contains("nothing")));
    // A flood.
    let (mut s, mut t) = raw_guest(&host);
    for _ in 0..crate::MAX_RATE * 2 {
        if wire::write_frame(&mut s, &mut t.send, &json!({"op": "presence"})).is_err() {
            break;
        }
    }
    let left = loop {
        match event(&host) {
            HostEvent::Left { why, .. } => break why,
            HostEvent::Message { .. } => {}
            other => panic!("{other:?}"),
        }
    };
    assert!(left.contains("too many"), "{left}");
    // What isn't a handshake at all (a web page trying the port) costs nothing and is closed.
    let mut s = TcpStream::connect(at(&host)).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut buf = Vec::new();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    assert_eq!(s.read_to_end(&mut buf).map(|_| buf.len()).unwrap_or(0), 0);
    assert!(host.try_event().is_none());
}

#[test]
fn a_hello_is_read_strictly_and_its_name_made_safe() {
    let host = host();
    let code = host.code().clone();
    let addr = at(&host);
    let joining = std::thread::spawn(move || {
        Guest::join(
            addr,
            &code,
            json!({"op": "hello", "version": VERSION, "name": "\u{7}Bo\nthe great builder of houses, walls and roofs"}),
            quick(),
            Arc::new(|| {}),
        )
    });
    let HostEvent::Joined { peer, name, .. } = event(&host) else { panic!() };
    assert!(!name.chars().any(char::is_control) && name.chars().count() <= crate::MAX_NAME && name.starts_with("Bo"), "{name:?}");
    host.send(peer, &json!({"op": "welcome"}));
    let _bo = joining.join().unwrap().unwrap();
    for first in [json!({"op": "execute"}), json!({"op": "hello", "version": VERSION, "name": ""})] {
        let mut s = TcpStream::connect(at(&host)).unwrap();
        let (i, msg1) = noise::initiate(&host.code().psk(), noise::PROLOGUE, b"", &ring::rand::SystemRandom::new()).unwrap();
        wire::write_raw(&mut s, &msg1).unwrap();
        let (_, mut t) = i.finish(&wire::read_raw(&mut s).unwrap()).unwrap();
        wire::write_frame(&mut s, &mut t.send, &first).unwrap();
        if first["op"] == "hello" {
            // An empty name is a guest's.
            assert!(matches!(event(&host), HostEvent::Joined { name, .. } if name == "Guest"));
        } else {
            let reply = wire::read_frame(&mut s, &mut t.recv).unwrap();
            assert_eq!(reply["op"], "ended", "{reply}");
        }
    }
}

#[test]
fn a_name_keeps_no_invisible_format_characters_and_an_honest_one_is_kept() {
    // An override, a zero-width space, a soft hyphen and a byte-order mark: none of them shows, all of them deceive.
    let hidden = "Bo\u{202e}\u{200b}\u{00ad}\u{feff}";
    assert_eq!(crate::host::clean(Some(hidden), "Guest"), "Bo");
    assert_eq!(crate::host::clean(Some("\u{2066}Bo\u{2069}"), "Guest"), "Bo", "an isolate");
    assert_eq!(crate::host::clean(Some("\u{202e}\u{200b}\u{00ad}\u{feff}"), "Guest"), "Guest", "nothing but them");
    // A name's own letters and signs are its own, and the cut is by characters.
    for name in ["José", "李", "Ana 🏠"] {
        assert_eq!(crate::host::clean(Some(name), "Guest"), name);
    }
    assert_eq!(crate::host::clean(Some(&"李".repeat(crate::MAX_NAME * 2)), "Guest").chars().count(), crate::MAX_NAME);
}

#[test]
fn a_guest_finds_a_host_on_the_lan_by_its_code_and_ignores_junk() {
    let host = host_with(Some(0));
    let port = host.discovery_port().unwrap();
    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    // Junk to the host first: it still answers after.
    let junk = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    for bytes in [&b"hello"[..], &[0u8; 2000][..], br#"{"bimcraft-cowork": 2, "op": "who"}"#] {
        junk.send_to(bytes, target).unwrap();
    }
    // Asked twice (the broadcast address and this machine's, say), a host is listed once.
    let found = discover(&[target, target], Duration::from_millis(500));
    assert_eq!(found.len(), 1, "{found:?}");
    let f = &found[0];
    assert_eq!((f.addr, f.name.as_str(), f.project.as_str(), f.version.as_str()), (at(&host), "Ana", "House", VERSION));
    assert_eq!(f.tag, host.code().tag());
    // A "host" that answers junk is not listed.
    let liar = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let liar_at = liar.local_addr().unwrap();
    let answering = std::thread::spawn(move || {
        let mut buf = [0u8; 512];
        for reply in
            [&br#"{"bimcraft-cowork": 1, "op": "here", "port": 70000}"#[..], b"\xff\xfe", br#"{"bimcraft-cowork": 1, "op": "here"}"#]
        {
            let (_, from) = liar.recv_from(&mut buf).unwrap();
            liar.send_to(reply, from).unwrap();
        }
    });
    for _ in 0..3 {
        assert_eq!(discover(&[liar_at], Duration::from_millis(200)), vec![]);
    }
    answering.join().unwrap();
    // Out of sight, no answer.
    let hidden = self::host();
    assert_eq!(hidden.discovery_port(), None);
}

/// Bo knocks at `host` on a thread: the digits his window shows, and his answer once the host's window has given one.
fn knocking(host: &Host) -> (std::sync::mpsc::Receiver<String>, std::thread::JoinHandle<Result<Code, String>>) {
    let (addr, (tx, rx)) = (at(host), std::sync::mpsc::channel());
    let bo = std::thread::spawn(move || knock(addr, "Bo", VERSION, quick(), move |d| drop(tx.send(d.to_string()))));
    (rx, bo)
}

#[test]
fn a_guest_without_the_code_knocks_both_see_the_same_digits_and_the_host_lets_it_in_or_not() {
    let host = host();
    let (shown, bo) = knocking(&host);
    let HostEvent::Knock { knock: k, name, digits } = event(&host) else { panic!("a knock") };
    assert_eq!(name, "Bo");
    let his = shown.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(his, digits, "the two windows show the same digits");
    assert!(digits.len() == 7 && digits.chars().filter(char::is_ascii_digit).count() == 6, "{digits}");
    host.answer_knock(k, true);
    let code = bo.join().unwrap().unwrap();
    assert_eq!(code, *host.code(), "let in, he is told the code");
    // Turned away, he hears why; and another knock's digits are new.
    let (shown, bo) = knocking(&host);
    let HostEvent::Knock { knock: k, digits: again, .. } = event(&host) else { panic!("a knock") };
    assert_ne!(again, digits);
    let _ = shown.recv_timeout(Duration::from_secs(5)).unwrap();
    host.answer_knock(k, false);
    assert!(bo.join().unwrap().unwrap_err().contains("didn't let you in"));
    // A host that takes no knocks hangs up as on a wrong code.
    host.set_knocks(false);
    let (_, bo) = knocking(&host);
    assert!(bo.join().unwrap().is_err());
}

#[test]
fn a_guest_that_reveals_another_nonce_than_it_committed_to_is_turned_away() {
    let host = host();
    let mut s = TcpStream::connect(at(&host)).unwrap();
    let (i, msg1) = noise::initiate(&crate::knock::psk(), noise::PROLOGUE, b"", &ring::rand::SystemRandom::new()).unwrap();
    wire::write_raw(&mut s, &msg1).unwrap();
    let msg2 = wire::read_raw(&mut s).unwrap();
    let (_, mut t) = i.finish(&msg2).unwrap();
    let committed = crate::knock::commit(&[1; 32]);
    wire::write_frame(&mut s, &mut t.send, &json!({"op": "knock", "version": VERSION, "name": "Eve", "commit": committed})).unwrap();
    assert_eq!(wire::read_frame(&mut s, &mut t.recv).unwrap()["op"], "nonce");
    // Another nonce than the one committed to: the one a search between two handshakes would want.
    wire::write_frame(&mut s, &mut t.send, &json!({"op": "reveal", "nonce": "02".repeat(32)})).unwrap();
    let said = wire::read_frame(&mut s, &mut t.recv).unwrap();
    assert_eq!(said["op"], "ended", "{said}");
    assert!(host.try_event().is_none(), "the host's window never hears of it");
}
