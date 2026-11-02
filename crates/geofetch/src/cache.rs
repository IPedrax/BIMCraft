//! What was fetched, kept on disk for a while: an Overpass reply, an elevation tile, under a name made from the request, so asking
//! again is not asking the server again. Best effort: no folder, no room or a damaged file is a miss, never an error. What is read
//! back is checked (a header, an age, a digest of the payload), because the folder is the user's and anything can be in it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ring::digest::{SHA256, digest};

/// The most one entry may hold, bytes.
pub const MAX_ENTRY: u64 = 96 << 20;
/// The most the folder may hold, bytes: when it holds more, the oldest entries go.
pub const MAX_TOTAL: u64 = 512 << 20;
const MAGIC: &[u8; 8] = b"BIMGEO1\n";
const HEADER: usize = 8 + 8 + 32;

/// The time, as seconds since 1970.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct Cache {
    dir: Option<PathBuf>,
    clock: Clock,
}

/// A name for what a request is: the SHA-256 of its parts, in hex.
pub fn key(parts: &[&[u8]]) -> String {
    let mut all = Vec::new();
    for p in parts {
        all.extend_from_slice(&(p.len() as u64).to_be_bytes());
        all.extend_from_slice(p);
    }
    digest(&SHA256, &all).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

impl Cache {
    /// A cache in `dir`, or none (`None`: nothing is kept).
    pub fn new(dir: Option<PathBuf>) -> Cache {
        Cache { dir, clock: Arc::new(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())) }
    }

    /// The same, told the time by `clock`, for tests.
    pub fn with_clock(mut self, clock: Clock) -> Cache {
        self.clock = clock;
        self
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        let ok = key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit());
        self.dir.as_ref().filter(|_| ok).map(|d| d.join(format!("{key}.bin")))
    }

    /// What was kept under `key` no more than `ttl` seconds ago, if it is whole. A file that is not is taken away.
    pub fn read(&self, key: &str, ttl: u64) -> Option<Vec<u8>> {
        let path = self.path(key)?;
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let gone = |path: &PathBuf| {
            let _ = std::fs::remove_file(path);
            None
        };
        if meta.len() < HEADER as u64 || meta.len() > MAX_ENTRY + HEADER as u64 {
            return gone(&path);
        }
        let bytes = std::fs::read(&path).ok()?;
        let (head, payload) = bytes.split_at_checked(HEADER)?;
        if &head[..8] != MAGIC || digest(&SHA256, payload).as_ref() != &head[16..] {
            return gone(&path);
        }
        let stamp = u64::from_be_bytes(head[8..16].try_into().ok()?);
        let now = (self.clock)();
        // Stamped later than a day from now is a clock that was wrong, and no reason to trust it for ever.
        if stamp > now.saturating_add(86_400) || now.saturating_sub(stamp) > ttl {
            return gone(&path);
        }
        Some(payload.to_vec())
    }

    /// Keeps `bytes` under `key`, if it can: a file made whole beside it and put in its place.
    pub fn write(&self, key: &str, bytes: &[u8]) {
        let _ = self.try_write(key, bytes);
    }

    fn try_write(&self, key: &str, bytes: &[u8]) -> Option<()> {
        let path = self.path(key)?;
        if bytes.len() as u64 > MAX_ENTRY {
            return None;
        }
        std::fs::create_dir_all(path.parent()?).ok()?;
        let mut file = Vec::with_capacity(HEADER + bytes.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&(self.clock)().to_be_bytes());
        file.extend_from_slice(digest(&SHA256, bytes).as_ref());
        file.extend_from_slice(bytes);
        let part = path.with_extension("bin.part");
        if std::fs::write(&part, &file).is_err() || std::fs::rename(&part, &path).is_err() {
            let _ = std::fs::remove_file(&part);
            return None;
        }
        self.prune(MAX_TOTAL);
        Some(())
    }

    /// Removes the oldest entries while the folder holds more than `most` bytes, and any half-made file left over a day.
    pub fn prune(&self, most: u64) {
        let Some(dir) = &self.dir else { return };
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut kept = Vec::new();
        for e in entries.flatten() {
            let (path, Ok(meta)) = (e.path(), e.metadata()) else { continue };
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            let age = meta.modified().ok().and_then(|t| t.elapsed().ok()).map_or(0, |d| d.as_secs());
            if name.ends_with(".bin.part") && age > 86_400 {
                let _ = std::fs::remove_file(&path);
            } else if name.ends_with(".bin") && meta.is_file() {
                kept.push((meta.modified().ok(), meta.len(), path));
            }
        }
        let mut total: u64 = kept.iter().map(|(_, n, _)| n).sum();
        kept.sort_by_key(|(t, _, _)| *t);
        for (_, n, path) in kept {
            if total <= most {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Scratch;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A clock the test sets.
    fn clock(start: u64) -> (Clock, Arc<AtomicU64>) {
        let now = Arc::new(AtomicU64::new(start));
        let read = now.clone();
        (Arc::new(move || read.load(Ordering::Relaxed)), now)
    }

    fn files(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().filter_map(|e| Some(e.ok()?.file_name().to_str()?.to_string())).collect();
        names.sort();
        names
    }

    #[test]
    fn what_was_kept_comes_back_until_it_is_too_old() {
        let dir = Scratch::new("cache");
        let (clk, now) = clock(1_000_000);
        let cache = Cache::new(Some(dir.0.clone())).with_clock(clk);
        let k = key(&[b"overpass", b"query"]);
        assert_eq!(cache.read(&k, 100), None, "nothing yet");
        cache.write(&k, b"the reply");
        assert_eq!(cache.read(&k, 100).as_deref(), Some(&b"the reply"[..]));
        now.store(1_000_099, Ordering::Relaxed);
        assert!(cache.read(&k, 100).is_some(), "99 seconds old is fresh for 100");
        now.store(1_000_101, Ordering::Relaxed);
        assert_eq!(cache.read(&k, 100), None, "101 is not");
        // Newer than a day ahead of the clock is a clock gone wrong, not a fresh file.
        now.store(1_000_000 - 100_000, Ordering::Relaxed);
        assert_eq!(cache.read(&k, 1 << 40), None);
        // Another thing, another name; the same thing, the same name.
        assert_ne!(key(&[b"a", b"bc"]), key(&[b"ab", b"c"]), "where the parts end matters");
        assert_eq!(key(&[b"x"]).len(), 64);
        assert_eq!(key(&[b"x"]), key(&[b"x"]));
    }

    #[test]
    fn a_damaged_file_is_a_miss_and_is_taken_away() {
        let dir = Scratch::new("damaged");
        let cache = Cache::new(Some(dir.0.clone()));
        let k = key(&[b"tile"]);
        cache.write(&k, b"0123456789");
        let path = dir.0.join(format!("{k}.bin"));
        let good = std::fs::read(&path).unwrap();
        for (what, bytes) in [
            ("a payload byte changed", {
                let mut b = good.clone();
                *b.last_mut().unwrap() ^= 1;
                b
            }),
            ("cut short", good[..good.len() - 3].to_vec()),
            ("only the header", good[..HEADER].to_vec()),
            ("no header", good[..10].to_vec()),
            ("another magic", {
                let mut b = good.clone();
                b[0] = b'X';
                b
            }),
            ("empty", Vec::new()),
            ("longer than it says", {
                let mut b = good.clone();
                b.extend_from_slice(b"more");
                b
            }),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(cache.read(&k, 1 << 40), None, "{what}");
            assert!(!path.exists(), "{what}: taken away");
        }
        // And a file in the way that is not a file.
        std::fs::create_dir_all(&path).unwrap();
        assert_eq!(cache.read(&k, 1 << 40), None);
    }

    #[test]
    fn no_folder_a_hostile_name_and_a_huge_entry_keep_nothing_and_do_no_harm() {
        let none = Cache::new(None);
        none.write(&key(&[b"x"]), b"data");
        assert_eq!(none.read(&key(&[b"x"]), 100), None);
        let dir = Scratch::new("hostile");
        let cache = Cache::new(Some(dir.0.join("not").join("yet")));
        let k = key(&[b"x"]);
        cache.write(&k, b"made on the way");
        assert!(cache.read(&k, 100).is_some(), "the folders are made");
        for bad in ["../x", "", "x", &"g".repeat(64), &format!("{}/..", &k[..60]), "/etc/passwd"] {
            cache.write(bad, b"no");
            assert_eq!(cache.read(bad, 100), None, "{bad}");
        }
        assert_eq!(files(&dir.0.join("not").join("yet")), [format!("{k}.bin")], "nothing else was written, and no .part is left");
        let big = vec![0u8; MAX_ENTRY as usize + 1];
        cache.write(&key(&[b"big"]), &big);
        assert_eq!(cache.read(&key(&[b"big"]), 100), None);
    }

    #[test]
    fn the_oldest_entries_go_when_the_folder_is_over_its_size() {
        let dir = Scratch::new("prune");
        let (clk, now) = clock(1_000);
        let cache = Cache::new(Some(dir.0.clone())).with_clock(clk);
        let keys: Vec<String> = (0..4).map(|i| key(&[&[i]])).collect();
        for (i, k) in keys.iter().enumerate() {
            now.store(1_000 + i as u64, Ordering::Relaxed);
            cache.write(k, &[i as u8; 1000]);
            // The files' own times are what a real folder has; make them follow the clock.
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + i as u64);
            let f = std::fs::File::options().write(true).open(dir.0.join(format!("{k}.bin"))).unwrap();
            f.set_modified(t).unwrap();
        }
        cache.prune(2 * (1000 + HEADER as u64));
        let left: Vec<bool> = keys.iter().map(|k| dir.0.join(format!("{k}.bin")).exists()).collect();
        assert_eq!(left, [false, false, true, true], "the two oldest");
        cache.prune(0);
        assert!(files(&dir.0).is_empty());
    }
}
