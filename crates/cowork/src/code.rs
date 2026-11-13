//! The join code: 80 random bits, written as 16 letters and digits of Crockford's base 32 in groups of four
//! (`7KQ2-M9XD-4TPA-R3HC`). It is the key of the handshake, so it never goes on the wire; the tag a host says on the LAN is a
//! keyed hash of it, which finds the host and tells nothing of the code.

use std::fmt;

use ring::rand::SecureRandom;
use ring::{digest, hmac};

/// Crockford's base 32: digits and letters, but not I, L, O (read as 1, 1 and 0) and U.
const LETTERS: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, PartialEq, Eq)]
pub struct Code([u8; 10]);

impl Code {
    /// A new code, from the system's random numbers.
    pub fn new() -> Result<Code, String> {
        let mut bytes = [0u8; 10];
        ring::rand::SystemRandom::new().fill(&mut bytes).map_err(|_| "the system gave no random numbers for a code".to_string())?;
        Ok(Code(bytes))
    }

    /// A code as typed: any case, with or without dashes and spaces, and O, I and L read as the digits they look like.
    pub fn parse(text: &str) -> Result<Code, String> {
        let mut n: u128 = 0;
        let mut count = 0;
        for c in text.chars().filter(|c| *c != '-' && !c.is_whitespace()) {
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            let at = LETTERS
                .iter()
                .position(|l| char::from(*l) == c)
                .ok_or_else(|| format!("`{c}` is not in a code: codes are digits and the letters A to Z but U"))?;
            count += 1;
            if count > 16 {
                return Err("a code has 16 letters and digits: this is longer".into());
            }
            n = n << 5 | at as u128;
        }
        if count != 16 {
            return Err(format!("a code has 16 letters and digits, not {count}"));
        }
        let mut bytes = [0u8; 10];
        bytes.copy_from_slice(&n.to_be_bytes()[6..]);
        Ok(Code(bytes))
    }

    /// What a host says on the LAN so a guest with the code can pick it out: 32 bits of HMAC-SHA256 under the code, in hex.
    pub fn tag(&self) -> String {
        let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, &self.0), b"bimcraft-cowork tag");
        tag.as_ref()[..4].iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The handshake's pre-shared key.
    pub(crate) fn psk(&self) -> [u8; 32] {
        let mut c = digest::Context::new(&digest::SHA256);
        c.update(b"bimcraft-cowork psk");
        c.update(&self.0);
        let mut psk = [0u8; 32];
        psk.copy_from_slice(c.finish().as_ref());
        psk
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut wide = [0u8; 16];
        wide[6..].copy_from_slice(&self.0);
        let n = u128::from_be_bytes(wide);
        for i in 0..16 {
            if i > 0 && i % 4 == 0 {
                f.write_str("-")?;
            }
            let at = (n >> (5 * (15 - i)) & 31) as usize;
            write!(f, "{}", char::from(LETTERS[at]))?;
        }
        Ok(())
    }
}

/// A code is a key: a log or a panic message never shows it.
impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Code(…)")
    }
}
