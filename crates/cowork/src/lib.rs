//! Co-Work's transport (COWORK-PLAN.md, docs/areas/cowork.md): a host's window and its guests, over TCP, everything encrypted
//! under the join code, and hosts found on the LAN. It carries JSON messages, each an object with an `op`, and knows nothing of
//! what they say: the desktop builds them (the model's deltas, presence, the chat) and reads them.
//!
//! - [`Code`]: the 16 letters a host gives its guests; 80 random bits that are the key of the handshake too.
//! - The handshake is Noise's `NNpsk0` with X25519, ChaCha20-Poly1305 and SHA-256, on `ring` (`noise.rs`, checked against the
//!   published vectors); a wrong code fails before the host reads a byte of what a guest says, and every frame after it is
//!   sealed in order, so a changed, dropped or replayed one ends the connection.
//! - [`Host`]: listens, lets guests in (the right code, the same version, at most [`MAX_GUESTS`]), passes their messages to the
//!   window as [`HostEvent`]s, relays presence to the other guests at once, and says `ended` to each when it closes.
//! - [`Guest`]: joins, waits for the window's welcome, and passes the host's messages on as [`GuestEvent`]s; it can tell a host
//!   that ended the session from one that vanished.
//! - [`discover`]: asks the LAN who hosts, and a host answers while it is visible.
//!
//! Everything that comes in is hostile: lengths are capped before anything is read, a peer that floods, falls silent or says
//! what is no message is dropped, and an address that gets the code wrong five times waits. Each side pings the other when it
//! has said nothing for a while, so silence means a broken line.

mod code;
mod discovery;
mod guest;
mod host;
mod noise;
mod wire;

#[cfg(test)]
mod tests;

pub use code::Code;
pub use discovery::{Found, discover};
pub use guest::{Guest, GuestEvent};
pub use host::{Host, HostConfig, HostEvent, Person};

use std::time::Duration;

/// The most guests a host lets in at once.
pub const MAX_GUESTS: usize = 8;
/// The longest a name is kept, in characters.
pub const MAX_NAME: usize = 40;
/// The most messages a guest may send in a second, presence and pings included; past it, the host drops it.
pub const MAX_RATE: u32 = 100;
/// The longest message, in bytes: a big project's whole model, deflated, fits.
pub const MAX_FRAME: usize = 64 << 20;

/// How long things may take. The tests shorten them.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// A side that has sent nothing for this long sends a ping.
    pub ping: Duration,
    /// A side that has heard nothing for this long takes the line for broken.
    pub silence: Duration,
    /// How long a connection has to finish the handshake and say its hello.
    pub handshake: Duration,
    /// How long an address waits after five wrong codes.
    pub lockout: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            ping: Duration::from_secs(5),
            silence: Duration::from_secs(15),
            handshake: Duration::from_secs(10),
            lockout: Duration::from_secs(30),
        }
    }
}

/// What the threads call when something has come in for the window: its `request_repaint`, so a minimised window answers too.
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;
