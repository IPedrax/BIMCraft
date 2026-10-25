use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Imports `rounds` damaged copies of a synthetic project (one to eight bytes changed, one in seven cut short) and hands each
/// import that comes back to `check`, after checking it passes the model's rules: none may panic.
pub(super) fn sweep(
    p: &crate::project_fixture::Project,
    seed: u64,
    rounds: usize,
    check: impl Fn(&bimcraft_core::import::Imported, usize),
) {
    let bytes = p.partition_bytes();
    let mut seed = seed;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..rounds {
        let mut bad = bytes.clone();
        for _ in 0..1 + next() % 8 {
            let at = (next() % bad.len() as u64) as usize;
            bad[at] = match next() % 4 {
                0 => 0xff,
                1 => 0,
                _ => next() as u8,
            };
        }
        if round % 7 == 0 {
            bad.truncate(bad.len() - (next() % 400) as usize);
        }
        if let Ok(i) = crate::import(&p.file_from(&bad), "x") {
            assert!(i.model.check().is_ok(), "round {round}");
            check(&i, round);
        }
    }
}

/// The commit of rvt-rs the code was ported from; the headers and NOTICE name it.
const RVT_RS_COMMIT: &str = "b6086d41b9758fd9b92f42f0a47b8345b9c58056";

#[test]
fn only_the_command_line_and_the_desktop_take_the_reader_and_only_through_a_default_feature() {
    for manifest in ["crates/core/Cargo.toml", "crates/update/Cargo.toml", "crates/ui-egui/Cargo.toml"] {
        let text = read(manifest);
        assert!(!text.contains("bimcraft-rvt"), "{manifest} depends on bimcraft-rvt: the core, the updater and the UI never do");
    }
    for manifest in ["apps/bimcraft-cli/Cargo.toml", "apps/bimcraft/Cargo.toml"] {
        let text = read(manifest);
        // The tests of the command line and the desktop build synthetic files with the crate's `testing` feature: a dev-dependency.
        let (normal, dev) = text.split_once("[dev-dependencies]").unwrap_or((text.as_str(), ""));
        let dev = dev.split("\n[").next().unwrap_or("");
        assert!(dev.is_empty() || dev.contains("features = [\"testing\"]"), "{manifest}: {dev}");
        let dep = normal.lines().find(|l| l.starts_with("bimcraft-rvt")).unwrap_or_else(|| panic!("{manifest} has no bimcraft-rvt"));
        assert!(dep.contains("optional = true"), "{manifest}: the reader is an optional dependency, `{dep}`");
        assert!(text.contains("default = [\"rvt\"]"), "{manifest}: the `rvt` feature is on by default");
        assert!(text.contains("rvt = [\"dep:bimcraft-rvt\"]"), "{manifest}: the `rvt` feature turns the reader on and nothing else");
    }
}

#[test]
fn the_crate_is_apache_only_and_carries_the_licence_and_the_notice_apache_asks_for() {
    let manifest = read("crates/rvt/Cargo.toml");
    assert!(manifest.contains("license = \"Apache-2.0\""), "the reader holds Apache-2.0 code, so it is Apache-2.0 only");
    assert!(!manifest.contains("license.workspace"), "the workspace's MIT OR Apache-2.0 does not apply to it");
    assert_eq!(read("crates/rvt/LICENSE"), read("LICENSE-APACHE"), "the Apache-2.0 text");
    let notice = read("crates/rvt/NOTICE");
    assert!(notice.contains("Copyright 2026 Griffin Long"), "rvt-rs's copyright line");
    assert!(notice.contains(RVT_RS_COMMIT), "NOTICE names the commit it was taken from");
    let readme = read("README.md");
    assert!(readme.contains("distributed under Apache-2.0"), "the README says builds with the reader are Apache-2.0");
}

#[test]
fn every_package_says_apache_and_ships_the_notice_that_a_build_with_the_reader_owes() {
    // The root NOTICE is what the packages carry; it must say what the crate's does.
    assert_eq!(read("NOTICE"), read("crates/rvt/NOTICE"), "the root NOTICE is a copy of the crate's");
    for (file, wants) in [
        ("packaging/linux/nfpm.yaml", "license: Apache-2.0"),
        ("packaging/linux/arch-package.sh", "license=('Apache-2.0')"),
        ("packaging/linux/io.github.ipedrax.bimcraft.metainfo.xml.in", "<project_license>Apache-2.0</project_license>"),
        ("packaging/macos/Info.plist.in", "BIMCraft contributors. Apache-2.0."),
    ] {
        assert!(read(file).contains(wants), "{file} says what the default build is under: `{wants}`");
        assert!(!read(file).contains("MIT OR Apache-2.0"), "{file}");
    }
    for (file, wants) in [("packaging/env.sh", "LICENSE-APACHE NOTICE"), ("packaging/windows/bimcraft.wxs", "Source=\"NOTICE\"")] {
        assert!(read(file).contains(wants), "{file} ships the NOTICE");
    }
}

#[test]
fn every_source_file_is_in_the_provenance_document_and_ported_ones_say_where_from() {
    let provenance = read("docs/rvt-provenance.md");
    let dir = root().join("crates/rvt/src");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".rs") || name == "tests.rs" {
            continue;
        }
        assert!(provenance.contains(&format!("`{name}`")), "docs/rvt-provenance.md has no row for {name}");
        let text = std::fs::read_to_string(&path).unwrap();
        // The attribution header is plain `//` comments above the module's `//!` documentation.
        let header: String = text.lines().take_while(|l| l.starts_with("//") && !l.starts_with("//!")).collect::<Vec<_>>().join("\n");
        if header.contains("rvt-rs") {
            assert!(header.contains(RVT_RS_COMMIT), "{name}: a header that names rvt-rs names its commit");
            assert!(header.contains("src/"), "{name}: and the rvt-rs file it was taken from");
            assert!(header.to_lowercase().contains("changed"), "{name}: and says the code was changed");
            assert!(header.contains("Copyright 2026 Griffin Long"), "{name}: and carries rvt-rs's copyright line");
        }
    }
}

#[test]
fn the_fence_is_written_into_the_policy_documents() {
    for doc in ["CLAUDE.md", "ROADMAP.md"] {
        let text = read(doc);
        assert!(text.contains("docs/rvt-provenance.md"), "{doc} points at the fence");
        assert!(text.contains("no public"), "{doc} says the format has no public specification");
    }
    let provenance = read("docs/rvt-provenance.md");
    for rule in ["Autodesk SDK", "ODA", "revit-ifc", "not a lawyer"] {
        assert!(provenance.contains(rule), "the provenance document states `{rule}`");
    }
}

mod columns;
mod decoders;
mod floors;
mod import;
mod inventory;
mod metadata;
mod openings;
mod records;
mod roofs;
mod rooms;
mod rvt_rs;
mod sketch;
mod walls;

// ---------- the container ----------

use crate::fixture::{container, global, gzip, paged, partition, stored_gzip, truncated_gzip};
use crate::gzip::{PAGE_PAYLOAD, PAGE_STORED, inflate_first, inflate_members, strip_pages};
use crate::{Error, Limits, Revit};

/// A stand-in for BasicFileInfo: the container only needs the stream to be there.
const BASIC: &[u8] = b"b\0f\0i\0";

fn revit(streams: &[(&str, &[u8])], limits: Limits) -> Result<Revit, Error> {
    let mut all = vec![("BasicFileInfo", BASIC)];
    all.extend_from_slice(streams);
    Revit::from_bytes(container(&all), limits)
}

/// Data that deflate cannot shrink, so a gzip member of it is longer than a page.
fn noise(len: usize) -> Vec<u8> {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

#[test]
fn a_container_lists_its_streams_by_path_and_reads_them_back() {
    let r = &mut revit(&[("Global/Latest", b"abc"), ("Partitions/21", b"defg")], Limits::default()).unwrap();
    assert_eq!(r.stream_names(), ["BasicFileInfo", "Global/Latest", "Partitions/21"]);
    assert_eq!(r.stream_len("Global/Latest"), Some(3));
    assert_eq!(r.stream_len("Global/Nope"), None);
    assert_eq!(r.read_stream("Partitions/21").unwrap(), b"defg");
    assert!(r.read_stream("Nope").unwrap_err().to_string().contains("no stream named Nope"));
}

#[test]
fn what_is_not_a_compound_file_is_refused_and_the_error_says_what_it_starts_with() {
    for (bytes, says) in [
        (b"PK\x03\x04rest of a zip".to_vec(), "50 4B 03 04"),
        (b"short".to_vec(), "73 68 6F 72 74"),
        (Vec::new(), "is empty"),
        (b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1".to_vec(), "Compound File"),
    ] {
        let e = Revit::from_bytes(bytes, Limits::default()).unwrap_err();
        assert!(e.to_string().contains(says), "{e}");
    }
    let signature = Revit::from_bytes(b"PK\x03\x04".to_vec(), Limits::default()).unwrap_err();
    assert!(signature.to_string().contains("D0 CF 11 E0 A1 B1 1A E1"), "{signature}");
}

#[test]
fn a_compound_file_that_is_not_a_revit_file_is_refused() {
    let bytes = container(&[("WordDocument", b"hello"), ("1Table", b"x")]);
    let e = Revit::from_bytes(bytes, Limits::default()).unwrap_err();
    assert!(e.to_string().contains("BasicFileInfo"), "{e}");
}

#[test]
fn a_file_over_the_size_limit_is_refused_whether_it_is_in_memory_or_on_disk() {
    let bytes = container(&[("BasicFileInfo", BASIC)]);
    let small = Limits { max_file: bytes.len() as u64 - 1, ..Limits::default() };
    assert!(matches!(Revit::from_bytes(bytes.clone(), small), Err(Error::TooBig { .. })));
    let path = std::env::temp_dir().join(format!("bimcraft-rvt-{}.rvt", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    assert!(Revit::open(&path, Limits::default()).is_ok());
    let e = Revit::open(&path, small).unwrap_err();
    assert!(e.to_string().contains("limit"), "{e}");
    let _ = std::fs::remove_file(&path);
    let missing = Revit::open(Path::new("/no/such/file.rvt"), Limits::default()).unwrap_err();
    assert!(missing.to_string().contains("/no/such/file.rvt"), "{missing}");
}

#[test]
fn a_stream_that_declares_gigabytes_is_refused_without_reading_it() {
    let mut bytes = container(&[("BasicFileInfo", BASIC), ("Big", &[7; 100])]);
    // The directory entry for `Big`: its name in UTF-16, 128-byte entries, the stream size at +120.
    let name: Vec<u8> = "Big".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let at = (0..bytes.len() - 128).step_by(128).find(|&i| bytes[i..].starts_with(&name)).unwrap();
    bytes[at + 120..at + 128].copy_from_slice(&(6u64 << 30).to_le_bytes());
    let mut r = Revit::from_bytes(bytes, Limits::default()).unwrap();
    let e = r.read_stream("Big").unwrap_err();
    assert!(matches!(e, Error::TooBig { .. }), "{e}");
    assert!(r.inflate("Big").is_err() && r.inflate_all("Big").is_err());
}

#[test]
fn a_stream_over_the_limit_and_too_many_streams_are_refused() {
    let big = [("Big", &[1u8; 5000][..])];
    let r = &mut revit(&big, Limits { max_stream: 4999, ..Limits::default() }).unwrap();
    assert!(matches!(r.read_stream("Big"), Err(Error::TooBig { .. })));
    let r = &mut revit(&big, Limits { max_stream: 5000, ..Limits::default() }).unwrap();
    assert_eq!(r.read_stream("Big").unwrap().len(), 5000);
    let many: Vec<(String, Vec<u8>)> = (0..40).map(|i| (format!("S{i}"), vec![0u8; 4])).collect();
    let mut all: Vec<(&str, &[u8])> = many.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    all.push(("BasicFileInfo", BASIC));
    let e = Revit::from_bytes(container(&all), Limits { max_streams: 10, ..Limits::default() }).unwrap_err();
    assert!(e.to_string().contains("41 streams"), "{e}");
}

// ---------- gzip ----------

#[test]
fn a_gzip_member_inflates_with_or_without_its_trailer_and_says_whether_the_trailer_matched() {
    let data = b"Revit writes gzip and leaves the trailer out, or not".repeat(20);
    let r = &mut revit(
        &[
            ("Global/Latest", &global(&data)),
            ("Formats/Latest", &truncated_gzip(&data)),
            ("Global/ElemTable", &{
                let mut g = global(&data);
                let n = g.len();
                g[n - 8] ^= 0xff; // the CRC is wrong
                g
            }),
        ],
        Limits::default(),
    )
    .unwrap();
    let ok = r.inflate("Global/Latest").unwrap();
    assert_eq!((ok.bytes.as_slice(), ok.trailers_ok, ok.members), (data.as_slice(), 1, 1));
    let cut = r.inflate("Formats/Latest").unwrap();
    assert_eq!((cut.bytes.as_slice(), cut.trailers_ok), (data.as_slice(), 0));
    let wrong = r.inflate("Global/ElemTable").unwrap();
    assert_eq!((wrong.bytes.as_slice(), wrong.trailers_ok), (data.as_slice(), 0), "the data is returned, and the mismatch is reported");
}

#[test]
fn a_gzip_bomb_stops_at_the_cap_and_the_error_says_so() {
    let bomb = global(&vec![0u8; 8 << 20]); // 8 MiB of zeros in a few KiB
    assert!(bomb.len() * 100 < 8 << 20, "{}", bomb.len());
    let limits = Limits { max_inflated: 1 << 20, ..Limits::default() };
    let r = &mut revit(&[("Global/Latest", &bomb), ("Partitions/21", &partition(&[&vec![0u8; 8 << 20]]))], limits).unwrap();
    for e in [r.inflate("Global/Latest").unwrap_err(), r.inflate_all("Partitions/21").unwrap_err()] {
        assert!(matches!(e, Error::TooMuch { .. }), "{e}");
        assert!(e.to_string().contains("1048576"), "{e}");
    }
    // The cap is for the whole stream: members that are each under it, together over it.
    let half = vec![0u8; 700_000];
    let r = &mut revit(&[("Partitions/22", &partition(&[&half, &half]))], limits).unwrap();
    assert!(matches!(r.inflate_all("Partitions/22"), Err(Error::TooMuch { .. })));
}

#[test]
fn a_stream_with_no_gzip_in_it_or_a_damaged_member_is_an_error_that_names_the_stream() {
    let r = &mut revit(
        &[
            ("Global/Latest", &[5; 300]),
            ("Global/History", &{
                let mut g = global(b"hello hello hello hello hello");
                g.truncate(g.len() - 14);
                g.iter_mut().skip(12).for_each(|b| *b = 0xff);
                g
            }),
        ],
        Limits::default(),
    )
    .unwrap();
    let none = r.inflate("Global/Latest").unwrap_err();
    assert!(none.to_string().contains("Global/Latest") && none.to_string().contains("gzip"), "{none}");
    let bad = r.inflate("Global/History").unwrap_err();
    assert!(bad.to_string().contains("Global/History"), "{bad}");
}

#[test]
fn stripping_pages_removes_exactly_each_full_pages_trailer() {
    assert_eq!((PAGE_STORED, PAGE_PAYLOAD), (65_249, 64_896));
    let payload = noise(PAGE_PAYLOAD * 2 + 1000);
    let stored = paged(&payload);
    assert_eq!(stored.len(), payload.len() + 2 * (PAGE_STORED - PAGE_PAYLOAD));
    assert_eq!(strip_pages(stored), payload);
    let short = noise(PAGE_STORED - 1); // a short last page is kept whole
    assert_eq!(strip_pages(short.clone()), short);
    assert_eq!(strip_pages(Vec::new()), Vec::<u8>::new());
}

/// The layout as the sample files show it, written out apart from `strip_pages` so that a test can ask what other numbers
/// would do: the byte at offset `o` is data when `o % page < data`. (Unlike `strip_pages` it also drops the part of a run a
/// cut-off stream ends in.)
fn pages_removed(stored: &[u8], page: usize, data: usize) -> Vec<u8> {
    stored.iter().enumerate().filter(|&(o, _)| o % page < data).map(|(_, &b)| b).collect()
}

#[test]
fn the_page_boundaries_are_where_the_sample_files_put_them_and_a_neighbouring_number_is_not_good_enough() {
    // The numbers are written out here, not read from the crate, so a wrong constant cannot take this test with it:
    // 64,895 is the last byte of data of the first page, 64,896 to 65,248 are its run, 65,249 is data again.
    let stored: Vec<u8> = (0..3 * 65_249 + 100).map(|o| if o % 65_249 < 64_896 { 1 } else { 0xee }).collect();
    let data = strip_pages(stored.clone());
    assert_eq!(data.len(), 3 * 64_896 + 100);
    assert!(data.iter().all(|&b| b == 1), "a byte of a run was kept, or a byte of data was dropped for one");
    assert_eq!(data, pages_removed(&stored, 65_249, 64_896));
    // What each wrong neighbour would leave of the same stream: never the same length.
    for (page, kept) in [(65_250, 64_896), (65_248, 64_896), (65_249, 64_897), (65_249, 64_895)] {
        assert_ne!(pages_removed(&stored, page, kept), data, "{page} / {kept} gives the same bytes");
    }
}

#[test]
fn stripping_pages_takes_exactly_353_bytes_from_each_full_page_whatever_the_length() {
    // Every length around the edges of one to three pages, and the lengths between: nothing panics, `len - 353 * full pages`
    // bytes come out, and only a stream cut off inside the last 352 bytes of a page keeps part of a run, as documented.
    for pages in 0..=3usize {
        for edge in [0, 1, 2, 64_894, 64_895, 64_896, 64_897, 65_247, 65_248] {
            let len = pages * 65_249 + edge;
            let stored: Vec<u8> = (0..len).map(|o| if o % 65_249 < 64_896 { 1 } else { 0xee }).collect();
            let got = strip_pages(stored.clone());
            assert_eq!(got.len(), len - 353 * pages, "{pages} pages and {edge}");
            let kept_run = got.iter().filter(|&&b| b == 0xee).count();
            assert_eq!(kept_run, edge.saturating_sub(64_896), "{pages} pages and {edge}");
            if edge <= 64_896 {
                assert_eq!(got, pages_removed(&stored, 65_249, 64_896), "{pages} pages and {edge}");
            }
        }
    }
}

#[test]
fn a_paged_stream_inflates_across_its_page_trailers_and_an_unpaged_one_is_left_alone() {
    let data = noise(150_000);
    let member = gzip(&data);
    assert!(member.len() > 2 * PAGE_PAYLOAD);
    let r = &mut revit(
        &[("Formats/Latest", &paged(&member)), ("Global/Latest", &paged(&global(&data))), ("Other", &member)],
        Limits::default(),
    )
    .unwrap();
    for name in ["Formats/Latest", "Global/Latest"] {
        let got = r.inflate(name).unwrap();
        assert!(got.bytes == data && got.trailers_ok == 1, "{name}");
    }
    // `Other` is not a stream Revit pages, so nothing is stripped, and a plain member still inflates.
    assert_eq!(r.inflate("Other").unwrap().bytes, data);
}

#[test]
fn partition_members_join_in_order_and_a_false_magic_inside_one_is_not_a_member() {
    let first = b"first chunk ".repeat(50);
    let second = noise(300);
    // Stored verbatim inside the member: a gzip member of its own, which is data here and not a member of the stream.
    let tricky = [&b"before "[..], &gzip(b"hidden"), b" after"].concat();
    let mut stream = partition(&[&first, &second]);
    // A member that stores its bytes verbatim carries a gzip header inside itself.
    stream.extend_from_slice(&[0x77; 32]);
    stream.extend_from_slice(&stored_gzip(&tricky));
    let r = &mut revit(&[("Partitions/21", &stream)], Limits::default()).unwrap();
    let got = r.inflate_all("Partitions/21").unwrap();
    let all = [first.as_slice(), &second, &tricky].concat();
    assert_eq!(got.bytes, all);
    assert_eq!(got.ends, [first.len(), first.len() + second.len(), all.len()]);
    assert_eq!((got.members, got.trailers_ok), (3, 2), "the verbatim member has no trailer");
}

#[test]
fn junk_between_members_and_a_member_cut_short_are_skipped() {
    let a = b"alpha ".repeat(40);
    let b = b"omega ".repeat(40);
    let mut cut = gzip(&noise(500));
    cut.truncate(60);
    let mut stream = partition(&[&a]);
    stream.extend_from_slice(&[0x33; 17]);
    stream.extend_from_slice(&cut);
    stream.extend_from_slice(&[0x44; 9]);
    stream.extend_from_slice(&gzip(&b));
    let r = &mut revit(&[("Partitions/21", &stream)], Limits::default()).unwrap();
    let got = r.inflate_all("Partitions/21").unwrap();
    assert_eq!(got.bytes, [a, b].concat());
    assert_eq!(got.skipped, 1, "the cut-short member is counted");
}

#[test]
fn the_inflate_cap_allows_exactly_its_size_and_a_header_with_a_reserved_flag_is_not_a_member() {
    let limits = Limits { max_inflated: 1000, ..Limits::default() };
    let at =
        |n: usize| revit(&[("Global/Latest", &global(&vec![7u8; n])), ("Partitions/21", &partition(&[&vec![7u8; n]]))], limits).unwrap();
    let (r, over) = (&mut at(1000), &mut at(1001));
    assert_eq!((r.inflate("Global/Latest").unwrap().bytes.len(), r.inflate_all("Partitions/21").unwrap().bytes.len()), (1000, 1000));
    assert!(matches!(over.inflate("Global/Latest"), Err(Error::TooMuch { .. })));
    assert!(matches!(over.inflate_all("Partitions/21"), Err(Error::TooMuch { .. })));
    let mut flagged = global(b"hello hello hello");
    flagged[8 + 3] = 0x20; // the gzip flags byte, with a reserved bit set
    let r = &mut revit(&[("Global/Latest", &flagged)], Limits::default()).unwrap();
    assert!(r.inflate("Global/Latest").is_err());
}

#[test]
fn a_file_with_exactly_as_many_streams_as_the_limit_is_read_and_one_more_is_not() {
    let many: Vec<(String, Vec<u8>)> = (0..9).map(|i| (format!("S{i}"), vec![0u8; 4])).collect();
    let mut all: Vec<(&str, &[u8])> = many.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    all.push(("BasicFileInfo", BASIC));
    assert_eq!(all.len(), 10);
    let limits = Limits { max_streams: 10, ..Limits::default() };
    assert!(Revit::from_bytes(container(&all), limits).is_ok());
    all.push(("S9", &[0; 4]));
    assert!(matches!(Revit::from_bytes(container(&all), limits), Err(Error::TooBig { .. })));
}

// ---------- hostile input ----------

#[test]
fn a_bounded_mutation_sweep_never_panics_and_never_returns_more_than_the_limits_allow() {
    let data = noise(20_000);
    let streams: Vec<(&str, Vec<u8>)> = vec![
        ("BasicFileInfo", BASIC.to_vec()),
        ("Global/Latest", global(&data)),
        ("Formats/Latest", truncated_gzip(&data)),
        ("Partitions/21", partition(&[&data[..9000], &data[9000..]])),
        ("TransmissionData", vec![0x41; 6000]),
    ];
    let borrowed: Vec<(&str, &[u8])> = streams.iter().map(|(n, d)| (*n, d.as_slice())).collect();
    let original = container(&borrowed);
    let limits = Limits { max_file: 1 << 20, max_streams: 64, max_stream: 1 << 20, max_inflated: 1 << 20 };
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut opened = 0;
    let header = original.len().min(1536);
    for round in 0..1500 {
        let mut bytes = original.clone();
        // Half the rounds break the headers and tables at the front, the rest anything anywhere.
        let span = if round % 2 == 0 { header } else { bytes.len() };
        for _ in 0..1 + next() % 4 {
            let at = (next() % span as u64) as usize;
            bytes[at] = match next() % 3 {
                0 => 0xff,
                1 => 0x00,
                _ => next() as u8,
            };
        }
        if round % 17 == 0 {
            bytes.truncate((next() % bytes.len() as u64) as usize);
        }
        let Ok(mut r) = Revit::from_bytes(bytes, limits) else { continue };
        opened += 1;
        for name in r.stream_names().into_iter().map(String::from).collect::<Vec<_>>() {
            if let Ok(stored) = r.read_stream(&name) {
                assert!(stored.len() as u64 <= limits.max_stream);
            }
            for inflated in [r.inflate(&name), r.inflate_all(&name)].into_iter().flatten() {
                assert!(inflated.bytes.len() <= limits.max_inflated);
                assert_eq!(inflated.ends.last().copied().unwrap_or(0), inflated.bytes.len());
            }
        }
    }
    assert!(opened > 100, "the sweep must reach the readers, not only the front door: {opened}");
}

#[test]
fn a_file_cut_short_anywhere_is_an_error_or_a_smaller_file_never_a_panic() {
    let data = noise(30_000);
    let whole = container(&[("BasicFileInfo", BASIC), ("Global/Latest", &global(&data)), ("Partitions/21", &partition(&[&data]))]);
    let limits = Limits { max_stream: 1 << 20, max_inflated: 1 << 20, ..Limits::default() };
    for len in (0..whole.len()).step_by(97) {
        let Ok(mut r) = Revit::from_bytes(whole[..len].to_vec(), limits) else { continue };
        for name in r.stream_names().into_iter().map(String::from).collect::<Vec<_>>() {
            let _ = (r.read_stream(&name), r.inflate(&name), r.inflate_all(&name));
        }
    }
}

// ---------- real files ----------

/// The sample files, when `BIMCRAFT_RVT_SAMPLES` names folders of them (several are parted like a `PATH`, each file may be in a
/// folder of its own). Autodesk's files are never committed, so these tests skip without it.
fn all_samples() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rvt" || e == "rfa") {
                out.push(path);
            }
        }
    }
    let Some(dirs) = std::env::var_os("BIMCRAFT_RVT_SAMPLES") else { return Vec::new() };
    let mut out = Vec::new();
    for dir in std::env::split_paths(&dirs) {
        walk(&dir, &mut out);
    }
    out.sort();
    out
}

/// The samples named `file`, wherever they are.
/// Whether an import is of a release another reader's export covers (2023 to 2025): a sample saved again by a newer release is a
/// file of its own, whose boxes and geometry need not be the older one's, so the comparisons pass it by.
fn pinned(i: &bimcraft_core::import::Imported) -> bool {
    i.schema.rsplit(' ').next().and_then(|y| y.parse::<u32>().ok()).is_some_and(|y| y <= 2025)
}

fn samples_of(file: &str) -> Vec<PathBuf> {
    all_samples().into_iter().filter(|p| p.file_name().is_some_and(|n| n == file)).collect()
}

/// Autodesk's structural basic sample project of releases 2023 to 2026, which the first tests pin.
fn samples() -> Vec<PathBuf> {
    samples_of("rstbasicsampleproject.rvt")
}

#[test]
fn every_sample_opens_and_every_gzip_stream_inflates_with_a_matching_trailer() {
    for path in all_samples() {
        let r = &mut Revit::open(&path, Limits::default()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for name in ["BasicFileInfo", "Formats/Latest", "Global/ElemTable", "Global/Latest"] {
            assert!(r.stream_len(name).is_some(), "{}: no {name}", path.display());
        }
        let mut members = 0;
        for name in r.stream_names().into_iter().map(String::from).collect::<Vec<_>>() {
            let whole = name.starts_with("Partitions/");
            let single = name == "Formats/Latest" || name.starts_with("Global/");
            if !whole && !single {
                continue;
            }
            let got = if whole { r.inflate_all(&name) } else { r.inflate(&name) }.unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            eprintln!(
                "{} {name}: {} bytes, {} members, {} trailers match, {} skipped",
                path.display(),
                got.bytes.len(),
                got.members,
                got.trailers_ok,
                got.skipped
            );
            assert_eq!(
                got.trailers_ok,
                got.members,
                "{} {name}: the page trailers are stripped exactly when every member's CRC matches",
                path.display()
            );
            assert_eq!(got.skipped, 0, "{} {name}", path.display());
            members += got.members;
        }
        assert!(members > 8, "{}", path.display());
    }
}

/// Whether a stream, with its pages stripped some way, still inflates with a matching CRC-32 and length on every member.
fn verifies(name: &str, stripped: &[u8]) -> bool {
    let room = 1 << 26;
    if name.starts_with("Partitions/") {
        inflate_members(stripped, room).is_ok_and(|got| got.skipped == 0 && got.trailers_ok == got.members)
    } else {
        inflate_first(stripped, room).is_ok_and(|got| got.trailers_ok == 1)
    }
}

#[test]
fn a_wrong_page_layout_breaks_the_real_files() {
    // The numbers of `gzip.rs` are observed, not trusted: with them every member of a real stream verifies, and with any
    // neighbour (a byte more or less of page, of data, a start shifted by one byte or by the eight-byte prefix of a
    // `Global/*` stream, or no stripping at all) the streams that cross a page boundary do not. The partition is the long
    // one (88 to 92 runs), so only the first sample's is tried.
    let wrong: [(&str, usize, usize, usize); 7] = [
        ("a byte more per page", 65_250, 64_896, 0),
        ("a byte less per page", 65_248, 64_896, 0),
        ("a byte more of data", 65_249, 64_897, 0),
        ("a byte less of data", 65_249, 64_895, 0),
        ("pages that start a byte later", 65_249, 64_896, 1),
        ("pages that start after the 8-byte prefix", 65_249, 64_896, 8),
        ("no stripping", 65_249, 65_249, 0),
    ];
    let mut partition_done = false;
    for path in samples() {
        let r = &mut Revit::open(&path, Limits::default()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for name in r.stream_names().into_iter().map(String::from).collect::<Vec<_>>() {
            let partition = name.starts_with("Partitions/");
            if !(partition || name == "Formats/Latest" || name.starts_with("Global/")) || (partition && partition_done) {
                continue;
            }
            let stored = r.read_stream(&name).unwrap();
            if stored.len() <= PAGE_PAYLOAD {
                continue; // under a page, there is no run to strip
            }
            assert!(verifies(&name, &strip_pages(stored.clone())), "{} {name}: the observed layout", path.display());
            for (what, page, data, shift) in wrong {
                let mut layout = stored[..shift].to_vec();
                layout.extend(pages_removed(&stored[shift..], page, data));
                assert!(!verifies(&name, &layout), "{} {name} still verifies with {what}", path.display());
            }
            partition_done |= partition;
        }
    }
}

#[test]
fn damaging_a_real_files_container_never_panics_and_stays_within_the_limits() {
    let Some(path) = samples().into_iter().next() else { return };
    let original = std::fs::read(&path).unwrap();
    let limits = Limits { max_file: 64 << 20, max_streams: 64, max_stream: 16 << 20, max_inflated: 64 << 20 };
    let mut seed = 0x1234_5678_9abc_def1u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut opened = 0;
    for round in 0..400 {
        let mut bytes = original.clone();
        // The header sector, the front of the file, and the last sectors, where this file's tables are.
        for _ in 0..1 + next() % 3 {
            let at = match round % 3 {
                0 => next() % 1024,
                1 => next() % 8192,
                _ => bytes.len() as u64 - 1 - next() % 65_536,
            } as usize;
            bytes[at] = next() as u8;
        }
        let Ok(mut r) = Revit::from_bytes(bytes, limits) else { continue };
        opened += 1;
        for name in ["BasicFileInfo", "Global/ElemTable", "Global/PartitionTable", "Contents"] {
            if let Ok(stored) = r.read_stream(name) {
                assert!(stored.len() as u64 <= limits.max_stream);
            }
            if let Ok(got) = r.inflate(name) {
                assert!(got.bytes.len() <= limits.max_inflated);
            }
        }
    }
    assert!(opened > 50, "{opened}");
}
