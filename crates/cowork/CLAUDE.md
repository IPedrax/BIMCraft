# bimcraft-cowork

Co-Work's transport: a host's window and its guests over TCP, everything encrypted under the join code, and hosts found on the
LAN. It carries JSON messages (each an object with an `op`) and knows nothing of what they say; the desktop builds and reads
them. The design is in [COWORK-PLAN.md](../../COWORK-PLAN.md), what is built in [Co-Work](../../docs/areas/cowork.md). Only the
desktop depends on it (`network_offenders` in `crates/update` reads every manifest).

It talks only to peers the user chose, by a code or an address, never to a server, and what comes in is hostile:

- **The handshake is Noise `NNpsk0_25519_ChaChaPoly_SHA256`** (`noise.rs`), written on `ring`, with the code's key mixed in
  first. `the_handshake_is_noise_nnpsk0_as_the_published_vectors_say` holds it to cacophony's vector for that protocol byte for
  byte (fixed ephemeral keys through `ring::test::rand::FixedSliceRandom`); a change to it keeps that test passing, or it is no
  longer Noise. Don't change the protocol name, the prologue or the order of the tokens.
- **Every length is checked before anything is read** (`wire.rs`): a handshake message at most 1 KB, a frame at most
  `MAX_FRAME`. A frame that fails to open, isn't JSON or has no `op` ends the connection.
- **A guest is dropped** when it floods (`MAX_RATE` messages a second), falls silent for `Timing::silence`, or stops taking what
  it is sent (`QUEUE`). An address that gets the code wrong `LOCKOUT_AFTER` times waits `Timing::lockout`. At most
  `MAX_GUESTS` are in, and `MAX_PENDING` in their handshake.
- **Each side pings** when it has said nothing for `Timing::ping`, so silence means a broken line; a guest tells a host that
  said `ended` (`GuestEvent::Ended`) from one that vanished (`GuestEvent::Lost`).
- **The host's threads never wait for the window.** Presence is relayed by the guest's own thread, and every event calls the
  `Wake` the window gave, its `request_repaint`, so a minimised window answers too.

Tests run on this machine's address only, with short `Timing`s (`tests::quick`), and take about two seconds:
`cargo test -p bimcraft-cowork`.
