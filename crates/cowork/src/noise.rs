//! The handshake every connection starts with: `Noise_NNpsk0_25519_ChaChaPoly_SHA256`, as the Noise Protocol Framework
//! (revision 34) defines it, on `ring`. The guest is the initiator and the host the responder; both mix the code's key in first,
//! so a guest with the wrong code fails the host's check of its very first message. Each side makes a key for this connection
//! alone, so what one connection said can't be read with what another learnt. The tests hold it to cacophony's published
//! vector for this protocol, byte for byte.
//!
//! After it, each side seals what it sends with its own [`Cipher`]: ChaCha20-Poly1305 with a counter as the nonce, so frames are
//! taken only in the order they were sent. Frames are longer than Noise's 64 KB transport messages may be; the sealing is the same.

use ring::rand::SecureRandom;
use ring::{aead, agreement, digest, hmac};

const NAME: &[u8] = b"Noise_NNpsk0_25519_ChaChaPoly_SHA256";
/// What both sides mix in first, so a peer of another protocol or version never gets through.
pub(crate) const PROLOGUE: &[u8] = b"bimcraft-cowork/1";
/// A public key, and the tag that seals each message.
const KEY: usize = 32;
const TAG: usize = 16;

/// One direction's key, and how many messages it has sealed or opened.
#[cfg_attr(test, derive(Clone))]
pub(crate) struct Cipher {
    key: [u8; 32],
    n: u64,
}

impl Cipher {
    fn new(key: [u8; 32]) -> Cipher {
        Cipher { key, n: 0 }
    }

    fn sealer(&self) -> Result<(aead::LessSafeKey, aead::Nonce), String> {
        // Noise keeps the last nonce back.
        if self.n == u64::MAX {
            return Err("too many messages on one connection".into());
        }
        let key = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, &self.key).map_err(|_| "a key of the wrong length".to_string())?;
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&self.n.to_le_bytes());
        Ok((aead::LessSafeKey::new(key), aead::Nonce::assume_unique_for_key(nonce)))
    }

    fn seal_with(&mut self, ad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String> {
        let (key, nonce) = self.sealer()?;
        let mut out = plain.to_vec();
        key.seal_in_place_append_tag(nonce, aead::Aad::from(ad), &mut out).map_err(|_| "a message could not be sealed".to_string())?;
        self.n += 1;
        Ok(out)
    }

    fn open_with(&mut self, ad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, String> {
        let (key, nonce) = self.sealer()?;
        let mut buf = sealed.to_vec();
        let len = key
            .open_in_place(nonce, aead::Aad::from(ad), &mut buf)
            .map_err(|_| "a message failed its check: another code, or changed on the way".to_string())?
            .len();
        buf.truncate(len);
        self.n += 1;
        Ok(buf)
    }

    pub(crate) fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>, String> {
        self.seal_with(&[], plain)
    }

    pub(crate) fn open(&mut self, sealed: &[u8]) -> Result<Vec<u8>, String> {
        self.open_with(&[], sealed)
    }
}

/// What a finished handshake gives each side: a cipher for each direction.
pub(crate) struct Transport {
    pub(crate) send: Cipher,
    pub(crate) recv: Cipher,
    /// The handshake's hash, the same on both sides; the vectors check it, and a knock's digits are made of it.
    pub(crate) hash: [u8; 32],
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut c = digest::Context::new(&digest::SHA256);
    parts.iter().for_each(|p| c.update(p));
    let mut out = [0u8; 32];
    out.copy_from_slice(c.finish().as_ref());
    out
}

fn mac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut c = hmac::Context::with_key(&hmac::Key::new(hmac::HMAC_SHA256, key));
    parts.iter().for_each(|p| c.update(p));
    let mut out = [0u8; 32];
    out.copy_from_slice(c.sign().as_ref());
    out
}

/// Noise's HKDF: three outputs (the callers take what they need).
fn hkdf(ck: &[u8; 32], ikm: &[u8]) -> [[u8; 32]; 3] {
    let temp = mac(ck, &[ikm]);
    let one = mac(&temp, &[&[1]]);
    let two = mac(&temp, &[&one, &[2]]);
    let three = mac(&temp, &[&two, &[3]]);
    [one, two, three]
}

/// Noise's SymmetricState.
struct State {
    ck: [u8; 32],
    h: [u8; 32],
    k: Option<Cipher>,
}

impl State {
    fn new(prologue: &[u8]) -> State {
        // The name is longer than a hash, so it is hashed.
        let h = sha256(&[NAME]);
        let mut s = State { ck: h, h, k: None };
        s.mix_hash(prologue);
        s
    }

    fn mix_hash(&mut self, data: &[u8]) {
        self.h = sha256(&[&self.h, data]);
    }

    fn mix_key(&mut self, ikm: &[u8]) {
        let [ck, k, _] = hkdf(&self.ck, ikm);
        (self.ck, self.k) = (ck, Some(Cipher::new(k)));
    }

    fn mix_key_and_hash(&mut self, ikm: &[u8]) {
        let [ck, h, k] = hkdf(&self.ck, ikm);
        self.ck = ck;
        self.mix_hash(&h);
        self.k = Some(Cipher::new(k));
    }

    fn encrypt_and_hash(&mut self, plain: &[u8]) -> Result<Vec<u8>, String> {
        let out = match &mut self.k {
            Some(k) => k.seal_with(&self.h, plain)?,
            None => plain.to_vec(),
        };
        self.mix_hash(&out);
        Ok(out)
    }

    fn decrypt_and_hash(&mut self, sealed: &[u8]) -> Result<Vec<u8>, String> {
        let out = match &mut self.k {
            Some(k) => k.open_with(&self.h, sealed)?,
            None => sealed.to_vec(),
        };
        self.mix_hash(sealed);
        Ok(out)
    }

    /// The initiator's cipher, then the responder's.
    fn split(&self) -> (Cipher, Cipher) {
        let [one, two, _] = hkdf(&self.ck, &[]);
        (Cipher::new(one), Cipher::new(two))
    }

    /// `e` as said, and as received: in a handshake with a pre-shared key, it is mixed into the key too.
    fn mix_ephemeral(&mut self, public: &[u8]) {
        self.mix_hash(public);
        self.mix_key(public);
    }
}

fn ephemeral(rng: &dyn SecureRandom) -> Result<(agreement::EphemeralPrivateKey, Vec<u8>), String> {
    let e = agreement::EphemeralPrivateKey::generate(&agreement::X25519, rng).map_err(|_| "no random numbers for a key".to_string())?;
    let public = e.compute_public_key().map_err(|_| "a key had no public half".to_string())?;
    Ok((e, public.as_ref().to_vec()))
}

fn dh(e: agreement::EphemeralPrivateKey, theirs: &[u8]) -> Result<Vec<u8>, String> {
    agreement::agree_ephemeral(e, &agreement::UnparsedPublicKey::new(&agreement::X25519, theirs), <[u8]>::to_vec)
        .map_err(|_| "the other side's key is no key".to_string())
}

/// A message's key, and what follows it: at least a sealed payload's tag.
fn key_of(msg: &[u8]) -> Result<(&[u8], &[u8]), String> {
    if msg.len() < KEY + TAG {
        return Err("a handshake message too short to be one".into());
    }
    Ok(msg.split_at(KEY))
}

/// The guest's side, between its message and the host's answer.
pub(crate) struct Initiator {
    state: State,
    e: agreement::EphemeralPrivateKey,
}

/// The guest's first message (`-> psk, e`), with `payload` sealed in it.
pub(crate) fn initiate(psk: &[u8; 32], prologue: &[u8], payload: &[u8], rng: &dyn SecureRandom) -> Result<(Initiator, Vec<u8>), String> {
    let mut state = State::new(prologue);
    state.mix_key_and_hash(psk);
    let (e, mut msg) = ephemeral(rng)?;
    state.mix_ephemeral(&msg);
    msg.extend(state.encrypt_and_hash(payload)?);
    Ok((Initiator { state, e }, msg))
}

impl Initiator {
    /// The host's answer (`<- e, ee`) read: its payload, and the connection's ciphers.
    pub(crate) fn finish(mut self, msg: &[u8]) -> Result<(Vec<u8>, Transport), String> {
        let (theirs, sealed) = key_of(msg)?;
        self.state.mix_ephemeral(theirs);
        self.state.mix_key(&dh(self.e, theirs)?);
        let payload = self.state.decrypt_and_hash(sealed)?;
        let (send, recv) = self.state.split();
        Ok((payload, Transport { send, recv, hash: self.state.h }))
    }
}

/// The host's side: the guest's first message read (its payload), the answer with `payload` sealed in it, and the ciphers.
pub(crate) fn respond(
    psk: &[u8; 32],
    prologue: &[u8],
    msg: &[u8],
    payload: &[u8],
    rng: &dyn SecureRandom,
) -> Result<(Vec<u8>, Vec<u8>, Transport), String> {
    let mut state = State::new(prologue);
    state.mix_key_and_hash(psk);
    let (theirs, sealed) = key_of(msg)?;
    state.mix_ephemeral(theirs);
    let said = state.decrypt_and_hash(sealed)?;
    let (e, mut answer) = ephemeral(rng)?;
    state.mix_ephemeral(&answer);
    state.mix_key(&dh(e, theirs)?);
    answer.extend(state.encrypt_and_hash(payload)?);
    let (recv, send) = state.split();
    Ok((said, answer, Transport { send, recv, hash: state.h }))
}
