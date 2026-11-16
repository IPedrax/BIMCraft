//! What reading a file may take of the machine's memory. A file is hostile, and running out of memory isn't an error Rust can
//! catch: the process is aborted, or killed by the system with no error at all. So a read is given a [`Budget`], the memory the
//! machine has free when it starts, less a margin. It is refused at once when the file's size says it can't fit, and stopped,
//! with an error and nothing changed, when what the process has allocated passes the budget ([`Budget::check`], called between
//! the values, elements and entities it reads).
//!
//! The apps count what they allocate and say what the machine has free ([`install`]: a counting global allocator and sysinfo,
//! so the `unsafe` stays in those crates). Without a meter (the tests, a program using the core as a library) a read keeps the
//! fixed limits it had.

use std::cell::Cell;
use std::fmt;
use std::sync::OnceLock;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

/// About how many times its size a project file takes in memory while it is read: 8.1 measured opening the 50-storey reference
/// tower (184 MB of file, 1.49 GB at the peak).
pub const PROJECT: u64 = 8;
/// About how many times its size an IFC file takes while it is read: 12 to 14 measured on Revit-like, meshed and
/// BIMCraft-written files of 17 to 81 MB.
pub const IFC: u64 = 14;
/// What a read leaves free for the rest of the machine, at least: a tenth of what's free when that's more.
const MARGIN: u64 = 256 << 20;

/// How an app measures memory: what the process has allocated now, and what the machine has free (none when it can't say).
#[derive(Clone, Copy)]
pub struct Meter {
    pub used: fn() -> usize,
    pub free: fn() -> Option<u64>,
}

static METER: OnceLock<Meter> = OnceLock::new();

/// Measures memory with `meter` from now on, in this whole process; the first one installed stays.
pub fn install(meter: Meter) {
    let _ = METER.set(meter);
}

/// What the process has allocated now, bytes, by the meter the app installed; none without one.
pub fn used() -> Option<usize> {
    METER.get().map(|m| (m.used)())
}

/// What the machine has free now, bytes, by the meter the app installed; none without one, or when it can't say.
pub fn free() -> Option<u64> {
    METER.get().and_then(|m| (m.free)())
}

/// What the machine's memory is doing: how much swap (a pagefile, on Windows) is in use, and how much memory is free, of all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pressure {
    pub swap_used: u64,
    pub available: u64,
    pub total: u64,
}

static PRESSURE: OnceLock<fn() -> Option<Pressure>> = OnceLock::new();

/// Reads the machine's [`Pressure`] with `read` from now on, in this whole process (the desktop's, through sysinfo).
pub fn install_pressure(read: fn() -> Option<Pressure>) {
    let _ = PRESSURE.set(read);
}

/// The machine's memory now, where the app reads it.
pub fn pressure() -> Option<Pressure> {
    PRESSURE.get().and_then(|read| read())
}

/// How much more swap than when it started a machine may use before it is said to be swapping: past what a system pages out
/// of programs left idle anyway.
const SWAPPING: u64 = 512 << 20;

/// Whether a machine is swapping, from samples of its [`Pressure`]: when the swap in use has grown by [`SWAPPING`] since the
/// first sample, and less than a tenth of its memory is free. Said once, until the swap shrinks back or memory frees up, so a
/// window warns once and not every frame.
#[derive(Debug, Default)]
pub struct SwapWatch {
    baseline: Option<u64>,
    swapping: bool,
}

/// What [`SwapWatch::sample`] says.
#[derive(Debug, PartialEq, Eq)]
pub enum Swap {
    /// It began swapping: this much more swap is in use than when the watch began.
    Began(u64),
    /// It isn't any more.
    Ended,
}

impl SwapWatch {
    pub fn sample(&mut self, p: Pressure) -> Option<Swap> {
        let base = *self.baseline.get_or_insert(p.swap_used);
        // Swap freed below where it began is the new floor: what grows from there is what this session pushed out.
        if p.swap_used < base {
            self.baseline = Some(p.swap_used);
        }
        let grown = p.swap_used.saturating_sub(self.baseline.unwrap_or(base));
        let short = p.total > 0 && p.available < p.total / 10;
        match (self.swapping, grown >= SWAPPING && short) {
            (false, true) => {
                self.swapping = true;
                Some(Swap::Began(grown))
            }
            // Ended once it is well clear, so it doesn't flicker on and off at the edge.
            (true, false) if grown < SWAPPING / 2 || p.available > p.total / 5 => {
                self.swapping = false;
                Some(Swap::Ended)
            }
            _ => None,
        }
    }

    pub fn swapping(&self) -> bool {
        self.swapping
    }
}

/// What a read may take: it stops when the process has allocated past `stop`.
pub struct Budget {
    meter: Option<(fn() -> usize, usize)>,
    /// What the read had to work in, for the message.
    room: u64,
    /// Whether it ran out: what the read had made is freed by the time its caller asks why it stopped.
    stopped: Cell<bool>,
    /// Who waits for the read: it stops where it checks its memory once they cancel.
    progress: Option<std::sync::Arc<crate::progress::Progress>>,
}

impl Budget {
    /// No limit, for a read the caller has limited some other way.
    pub fn none() -> Budget {
        Budget { meter: None, room: 0, stopped: Cell::new(false), progress: None }
    }

    /// For reading `size` bytes that take about `factor` times that in memory, measured by the meter the app installed: none
    /// when there is none or it can't say what's free, so the caller keeps its fixed limit. An error when it can't fit.
    pub fn start(size: u64, factor: u64) -> Result<Option<Budget>, String> {
        METER.get().map_or(Ok(None), |m| Budget::with(m, size, factor))
    }

    /// [`Budget::start`] measured by `meter`.
    pub fn with(meter: &Meter, size: u64, factor: u64) -> Result<Option<Budget>, String> {
        let Some(free) = (meter.free)() else { return Ok(None) };
        let room = free.saturating_sub((free / 10).max(MARGIN));
        let need = size.saturating_mul(factor);
        if need > room {
            return Err(format!(
                "it needs about {} of memory and this machine has {} free: close other programs and try again, or use a \
                 machine with more memory",
                bytes(need),
                bytes(free)
            ));
        }
        let stop = (meter.used)().saturating_add(usize::try_from(room).unwrap_or(usize::MAX));
        Ok(Some(Budget { meter: Some((meter.used, stop)), room, stopped: Cell::new(false), progress: None }))
    }

    /// This budget, the read stopping where it checks it once `progress` is cancelled.
    pub fn watched(mut self, progress: Option<std::sync::Arc<crate::progress::Progress>>) -> Budget {
        self.progress = progress;
        self
    }

    /// What the read reports its progress to, if anyone waits for it.
    pub fn progress(&self) -> Option<&crate::progress::Progress> {
        self.progress.as_deref()
    }

    /// An error once the process has allocated past what the read may take, and every time after; or once it is cancelled.
    pub fn check(&self) -> Result<(), String> {
        if let Some(p) = &self.progress {
            p.check()?;
        }
        self.stopped.set(self.stopped.get() || self.meter.is_some_and(|(used, stop)| used() > stop));
        match self.stopped.get() {
            true => Err(format!(
                "it took more than the {} of memory this machine had free, so it was stopped: close other programs and try \
                 again, or use a machine with more memory",
                bytes(self.room)
            )),
            false => Ok(()),
        }
    }

    /// How many more bytes the read may take now; none when there's no limit.
    pub fn left(&self) -> Option<u64> {
        self.meter.map(|(used, stop)| stop.saturating_sub(used()) as u64)
    }
}

/// `n` bytes for people: megabytes below a gigabyte, gigabytes with a decimal above.
pub fn bytes(n: u64) -> String {
    if n >= 1_000_000_000 { format!("{:.1} GB", n as f64 / 1e9) } else { format!("{} MB", n.div_ceil(1_000_000)) }
}

/// The JSON document `text`, read as `serde_json::from_str` reads it, with `budget` checked at each value of an array or an
/// object: a file of a few megabytes can unpack into gigabytes of values.
pub(crate) fn json(text: &str, budget: &Budget) -> Result<Value, String> {
    let mut de = serde_json::Deserializer::from_str(text);
    // A stop says why itself, rather than as a parse error at a line and column.
    Metered(budget)
        .deserialize(&mut de)
        .and_then(|v| de.end().map(|()| v))
        .map_err(|e| budget.check().err().unwrap_or_else(|| e.to_string()))
}

struct Metered<'a>(&'a Budget);

impl<'de> DeserializeSeed<'de> for Metered<'_> {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Metered<'_> {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::from(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = seq.next_element_seed(Metered(self.0))? {
            self.0.check().map_err(<A::Error as de::Error>::custom)?;
            out.push(v);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = serde_json::Map::new();
        while let Some(k) = map.next_key::<String>()? {
            let v = map.next_value_seed(Metered(self.0))?;
            self.0.check().map_err(<A::Error as de::Error>::custom)?;
            out.insert(k, v);
        }
        Ok(Value::Object(out))
    }
}

/// A meter for the tests: each test runs on a thread of its own, so what it says is the thread's.
#[cfg(test)]
pub(crate) mod testing {
    use super::{Budget, MARGIN, Meter, PROJECT};
    use std::cell::Cell;

    thread_local! {
        static USED: Cell<usize> = const { Cell::new(0) };
        static FREE: Cell<Option<u64>> = const { Cell::new(None) };
    }

    /// A meter that says `free` is free, and counts a megabyte allocated each time it is asked.
    pub(crate) fn fake(free: Option<u64>) -> Meter {
        FREE.with(|f| f.set(free));
        USED.with(|u| u.set(0));
        Meter { used: || USED.with(|u| u.replace(u.get() + (1 << 20))), free: || FREE.with(Cell::get) }
    }

    /// A budget of `room` bytes on the fake meter: it runs out after about `room` megabytes' worth of checks.
    pub(crate) fn budget(room: u64) -> Budget {
        Budget::with(&fake(Some(MARGIN + room)), 0, PROJECT).ok().flatten().unwrap_or_else(Budget::none)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{budget, fake};
    use super::*;

    #[test]
    fn a_metered_read_of_json_is_the_one_serde_json_makes() {
        let mut s = crate::engine::Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let text = s.model.to_json().unwrap();
        assert_eq!(json(&text, &Budget::none()).unwrap(), serde_json::from_str::<Value>(&text).unwrap());
        let odd = r#"{"a": [1, -2, 18446744073709551615, -9223372036854775808, 1.5e300, 0.1, "xé\n", null, true, {}, []]}"#;
        assert_eq!(json(odd, &Budget::none()).unwrap(), serde_json::from_str::<Value>(odd).unwrap());
        for bad in ["", "{", "[1,]", "{} {}", "nul"] {
            assert_eq!(json(bad, &Budget::none()).unwrap_err(), serde_json::from_str::<Value>(bad).unwrap_err().to_string(), "{bad}");
        }
    }

    #[test]
    fn a_machine_is_said_to_swap_once_when_its_swap_grows_and_memory_runs_short_and_once_more_when_it_stops() {
        let gb = 1u64 << 30;
        let at = |swap: u64, available: u64| Pressure { swap_used: swap, available, total: 16 * gb };
        let mut w = SwapWatch::default();
        // Swap in use already when it starts, plenty free: nothing to say.
        assert_eq!(w.sample(at(2 * gb, 8 * gb)), None);
        // It grows, but memory isn't short: still nothing.
        assert_eq!(w.sample(at(3 * gb, 8 * gb)), None);
        // Short of memory, and a gigabyte more swap than at the start: said once.
        assert_eq!(w.sample(at(3 * gb, gb)), Some(Swap::Began(gb)));
        assert_eq!(w.sample(at(3 * gb + 100, gb / 2)), None);
        assert!(w.swapping());
        // Memory freed: said once more.
        assert_eq!(w.sample(at(3 * gb, 6 * gb)), Some(Swap::Ended));
        assert_eq!(w.sample(at(3 * gb, 6 * gb)), None);
        // Swap freed below where it began is the floor from then on.
        let mut w = SwapWatch::default();
        w.sample(at(4 * gb, 8 * gb));
        w.sample(at(gb, 8 * gb));
        assert_eq!(w.sample(at(gb + gb / 4, gb)), None, "a quarter of a gigabyte is not swapping");
        assert_eq!(w.sample(at(2 * gb, gb)), Some(Swap::Began(gb)));
    }

    #[test]
    fn a_read_too_big_for_whats_free_is_refused_before_it_starts() {
        let meter = fake(Some(4 << 30));
        // 4.3 GB free leaves 3.9 GB: 524 MB of project at 8 times its size is too much, 419 MB isn't.
        let e = Budget::with(&meter, 500 << 20, PROJECT).err().unwrap();
        assert!(e.starts_with("it needs about 4.2 GB of memory and this machine has 4.3 GB free"), "{e}");
        assert!(Budget::with(&meter, 400 << 20, PROJECT).unwrap().is_some());
        // A machine that can't say what's free keeps the caller's fixed limit.
        assert!(Budget::with(&fake(None), u64::MAX, PROJECT).unwrap().is_none());
    }

    #[test]
    fn a_read_that_grows_past_its_budget_is_stopped_with_an_error_not_a_crash() {
        // The fake counts a megabyte each time it is asked, so a budget of a few megabytes runs out within a few values.
        let text = format!("[{}]", vec!["0"; 1000].join(","));
        let e = json(&text, &budget(8_000_000)).unwrap_err();
        assert!(e.starts_with("it took more than the 8 MB of memory this machine had free, so it was stopped"), "{e}");
        assert!(Budget::none().check().is_ok() && Budget::none().left().is_none());
    }

    #[test]
    fn a_read_that_was_stopped_still_says_so_once_what_it_made_is_freed() {
        let budget = budget(8_000_000);
        while budget.check().is_ok() {}
        // Its values are dropped as the error comes back, and the caller asks why: the process is under the budget again.
        fake(Some(0));
        assert!(budget.check().is_err());
    }
}
