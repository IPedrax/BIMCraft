//! `open` held to the memory the machine has free (`memory.rs`), with what the process allocates counted as the apps count it
//! and what's free set by the test. One test, since the meter is the whole process's.

#[cfg(test)]
mod tests {
    use std::alloc::System;
    use std::sync::atomic::{AtomicU64, Ordering};

    use bimcraft_core::engine::Session;
    use bimcraft_core::memory::{self, Meter};
    use serde_json::json;

    #[global_allocator]
    static ALLOC: cap::Cap<System> = cap::Cap::new(System, usize::MAX);

    /// What the meter says the machine has free.
    static FREE: AtomicU64 = AtomicU64::new(0);

    /// What a read leaves free for the rest of the machine (`memory::MARGIN`).
    const MARGIN: u64 = 256 << 20;

    fn free(n: u64) {
        FREE.store(n, Ordering::Relaxed);
    }

    #[test]
    fn open_takes_a_file_as_large_as_the_memory_thats_free_and_refuses_or_stops_one_larger() {
        memory::install(Meter { used: || ALLOC.allocated(), free: || Some(FREE.load(Ordering::Relaxed)) });
        let dir = tempfile::tempdir().unwrap();
        let mut house = Session::new();
        house.script(bimcraft_core::sample::SAMPLE).unwrap();
        let saved = dir.path().join("house.bimc");
        house.execute("save", &json!({"path": saved})).unwrap();

        // Room enough: it opens.
        free(64 << 30);
        let mut s = Session::new();
        s.execute("open", &json!({"path": saved})).unwrap();
        assert_eq!(s.model.elements.len(), house.model.elements.len());
        let before = s.model.to_json().unwrap();

        // A kilobyte to spare: refused before it's read, and the open project stays.
        free(MARGIN + 1_000);
        let e = s.execute("open", &json!({"path": saved})).unwrap_err();
        assert!(e.contains("house.bimc: it needs about"), "{e}");
        assert_eq!(s.model.to_json().unwrap(), before);

        // A file whose size says it fits, eight times over, but that unpacks into far more: four million values at 32 bytes
        // each from 8 MB of text. Stopped as it passes what's free, not when the system kills the process.
        let crafted = dir.path().join("crafted.bimc");
        std::fs::write(&crafted, format!("{{\"format\": 1, \"x\": [{}]}}", vec!["0"; 4_000_000].join(","))).unwrap();
        free(MARGIN + 100_000_000);
        let e = s.execute("open", &json!({"path": crafted})).unwrap_err();
        assert!(e.contains("crafted.bimc: it took more than the 100 MB of memory this machine had free, so it was stopped"), "{e}");
        assert_eq!(s.model.to_json().unwrap(), before);

        // An IFC file is measured at fourteen times its size.
        let ifc = dir.path().join("house.ifc");
        house.execute("save", &json!({"path": ifc})).unwrap();
        let size = std::fs::metadata(&ifc).unwrap().len();
        free(MARGIN + size * 13);
        let e = s.execute("open", &json!({"path": ifc})).unwrap_err();
        assert!(e.contains("house.ifc: it needs about"), "{e}");
        free(64 << 30);
        s.execute("open", &json!({"path": ifc})).unwrap();

        // Past the 256 MiB a project was held to before: not refused for its size (it isn't a project, which is what's said).
        let big = dir.path().join("big.bimc");
        std::fs::File::create(&big).unwrap().set_len(300 << 20).unwrap();
        let e = s.execute("open", &json!({"path": big})).unwrap_err();
        assert!(e.contains("is not a BIMCraft project") && !e.contains("too large"), "{e}");
    }
}
