//! The key the user asks to be remembered, kept in `settings.json` scrambled and not as it was typed: sealed with AES-256-GCM (a nonce
//! of its own, the format's name as associated data) under a key stretched with PBKDF2-HMAC-SHA256 from the program's own secret
//! and a salt of its own, both from `ring`, which the updater already uses.
//!
//! **What this is, and is not.** The secret is built into the program, so it is no secret from anyone who has the program: it keeps
//! the saved key from being read at a glance in a file that gets copied (a backup, a synced folder, a settings file sent for support),
//! and not from someone who has the file and a BIMCraft. The official builds are made with the secret in `BIMCRAFT_KEY_SECRET`, a
//! GitHub Actions secret, so the text is not in this repository; any other build (a source build, the tests) uses [`DEV_SECRET`],
//! which is public, and a key saved by one kind of build does not open in the other (the user types it again). Something stronger
//! (the system's keychain, or a passphrase the user types) is a change here, and nothing outside this module.
//!
//! The text is `bc1:<iterations>:<salt>:<nonce>:<sealed key and tag>`, the last three in lower-case hex. It is read as hostilely as
//! any file: its parts have exact lengths, the iterations are held between [`MIN_ITERATIONS`] and [`MAX_ITERATIONS`], and nothing
//! is derived from a text that is not well formed.

use std::num::NonZeroU32;

use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};

use crate::{KEY_MAX, key_ok};

const NAME: &str = "bc1";
/// What the seal is bound to, so a text sealed for something else does not open here.
const AAD: &[u8] = b"bimcraft-assistant-key-v1";
/// What a build without `BIMCRAFT_KEY_SECRET` seals with: public, so such a build hides nothing, and says so ([`built_with_secret`]).
pub const DEV_SECRET: &str = "bimcraft-development-build-not-a-secret";
/// Rounds of PBKDF2: with a secret the program carries, more would protect nothing, so only enough to make the key from the salt.
pub const ITERATIONS: u32 = 10_000;
/// What a file may ask for, either way.
pub const MIN_ITERATIONS: u32 = 1_000;
pub const MAX_ITERATIONS: u32 = 2_000_000;
const SALT: usize = 16;
const NONCE: usize = 12;
const TAG: usize = 16;

const WRONG: &str = "The saved key does not open: it was saved by another build of BIMCraft, or the file was changed";

/// The secret this build seals with: the one it was built with, else [`DEV_SECRET`].
fn secret() -> &'static str {
    match option_env!("BIMCRAFT_KEY_SECRET") {
        Some(s) if !s.trim().is_empty() => s,
        _ => DEV_SECRET,
    }
}

/// Whether this build has a secret of its own: the official builds do, and a release must (the release workflow sets
/// `BIMCRAFT_REQUIRE_KEY_SECRET`, and a test fails the release when a build has none).
pub fn built_with_secret() -> bool {
    secret() != DEV_SECRET
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Bytes from lower-case hex, of exactly `len` bytes when that is given.
fn unhex(text: &str, len: Option<usize>) -> Option<Vec<u8>> {
    let digits = text.as_bytes();
    if !digits.len().is_multiple_of(2) || len.is_some_and(|n| digits.len() != 2 * n) {
        return None;
    }
    let value = |d: u8| match d {
        b'0'..=b'9' => Some(d - b'0'),
        b'a'..=b'f' => Some(d - b'a' + 10),
        _ => None,
    };
    digits.chunks(2).map(|pair| Some(value(pair[0])? << 4 | value(pair[1])?)).collect()
}

fn derive(secret: &str, salt: &[u8], iterations: u32) -> Result<LessSafeKey, String> {
    let rounds = NonZeroU32::new(iterations).ok_or("no iterations")?;
    let mut derived = [0u8; 32];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, rounds, salt, secret.as_bytes(), &mut derived);
    UnboundKey::new(&aead::AES_256_GCM, &derived).map(LessSafeKey::new).map_err(|_| "could not make the key".to_string())
}

/// [`seal`] with the secret, salt, nonce and iterations given: for the test that the format is the standards'. Never reuse a nonce for a key.
pub fn seal_with(secret: &str, key: &str, salt: &[u8], nonce: &[u8], iterations: u32) -> Result<String, String> {
    if !key_ok(key) {
        return Err("there is no key to keep".into());
    }
    if secret.is_empty() || salt.len() != SALT || nonce.len() != NONCE || !(MIN_ITERATIONS..=MAX_ITERATIONS).contains(&iterations) {
        return Err("a secret, salt, nonce or number of iterations of the wrong size".into());
    }
    let sealing = derive(secret, salt, iterations)?;
    let unique = Nonce::try_assume_unique_for_key(nonce).map_err(|_| "a nonce of the wrong size".to_string())?;
    let mut text = key.as_bytes().to_vec();
    sealing.seal_in_place_append_tag(unique, Aad::from(AAD), &mut text).map_err(|_| "could not seal the key".to_string())?;
    Ok(format!("{NAME}:{iterations}:{}:{}:{}", hex(salt), hex(nonce), hex(&text)))
}

/// The key sealed under this build's secret, with a salt and a nonce of its own.
pub fn seal(key: &str) -> Result<String, String> {
    let (rng, mut salt, mut nonce) = (SystemRandom::new(), [0u8; SALT], [0u8; NONCE]);
    rng.fill(&mut salt).and_then(|()| rng.fill(&mut nonce)).map_err(|_| "the system has no random numbers".to_string())?;
    seal_with(secret(), key.trim(), &salt, &nonce, ITERATIONS)
}

struct Parts {
    iterations: u32,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    sealed: Vec<u8>,
}

fn parse(text: &str) -> Option<Parts> {
    let mut parts = text.split(':');
    let (name, iterations, salt, nonce, sealed) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if name != NAME || parts.next().is_some() || iterations.len() > 10 || !iterations.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let iterations: u32 = iterations.parse().ok().filter(|n| (MIN_ITERATIONS..=MAX_ITERATIONS).contains(n))?;
    let sealed = unhex(sealed, None).filter(|s| (TAG + 1..=TAG + KEY_MAX).contains(&s.len()))?;
    Some(Parts { iterations, salt: unhex(salt, Some(SALT))?, nonce: unhex(nonce, Some(NONCE))?, sealed })
}

/// Whether `text` could be a sealed key: the parts of the right sizes, hex, and iterations in range. Nothing is derived.
pub fn well_formed(text: &str) -> bool {
    parse(text).is_some()
}

/// The key `sealed` holds, under `secret`; an error when it is another secret, or the text was changed.
pub fn open_with(secret: &str, sealed: &str) -> Result<String, String> {
    let parts = parse(sealed).ok_or("that is not a saved key")?;
    let opening = derive(secret, &parts.salt, parts.iterations)?;
    let unique = Nonce::try_assume_unique_for_key(&parts.nonce).map_err(|_| WRONG.to_string())?;
    let mut text = parts.sealed;
    let plain = opening.open_in_place(unique, Aad::from(AAD), &mut text).map_err(|_| WRONG.to_string())?;
    String::from_utf8(plain.to_vec()).ok().filter(|k| key_ok(k)).ok_or_else(|| WRONG.to_string())
}

/// The key `sealed` holds, under this build's secret.
pub fn open(sealed: &str) -> Result<String, String> {
    open_with(secret(), sealed)
}
