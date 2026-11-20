//! BasicFileInfo, the Atom entry, the preview PNG and the schema.

use super::*;
use crate::fixture::{self, Basic, Class};

// ---------- BasicFileInfo ----------

#[test]
fn basic_file_info_gives_the_release_build_guid_and_the_text_block_whatever_the_alignment() {
    for odd in [false, true] {
        let bytes = fixture::basic_file_info(&Basic { odd, username: "jdoe", ..Basic::default() });
        let info = crate::info::BasicFileInfo::parse(&bytes).unwrap();
        assert_eq!(info.release, 2024, "odd={odd}");
        assert_eq!(info.build.as_deref(), Some("20230308_1635(x64)"));
        assert_eq!(info.guid.as_deref(), Some("badabcab-cff7-4b43-8935-d6520041e7ca"));
        assert_eq!(info.locale.as_deref(), Some("ENU"));
        assert_eq!(info.username.as_deref(), Some("jdoe"));
        assert_eq!(info.last_save_path.as_deref(), Some("C:\\Users\\someone\\Documents\\house.rvt"));
        assert_eq!(info.worksharing.as_deref(), Some("Not enabled"));
        assert_eq!(info.central_model_path, None, "an empty value is none");
        // The block keeps every line in file order, the narrow `False` unpacked, and the lines after it intact.
        let keys: Vec<&str> = info.properties.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys.first(), Some(&"Worksharing"));
        assert_eq!(keys.last(), Some(&"ClientAppName"));
        assert_eq!(info.property("IsSingleUserCloudModel"), Some("False"));
        assert_eq!(info.property("Author"), Some("Autodesk Revit"));
        assert_eq!(info.property("ClientAppName"), Some("RevitApplication"), "the last line ends in a narrow CR LF");
        assert_eq!(info.property("Unique Document Increments"), Some("22"));
    }
}

#[test]
fn basic_file_info_without_a_username_has_none_and_an_older_release_is_read_from_its_build_line() {
    let info = crate::info::BasicFileInfo::parse(&fixture::basic_file_info(&Basic::default())).unwrap();
    assert_eq!(info.username, None);
    let old = fixture::basic_file_info(&Basic { release: 2016, build: "20150220_1515(x64)", legacy: true, ..Basic::default() });
    let info = crate::info::BasicFileInfo::parse(&old).unwrap();
    assert_eq!((info.release, info.build.as_deref()), (2016, Some("20150220_1515(x64)")));
}

#[test]
fn basic_file_info_that_is_not_what_revit_writes_is_an_error_that_says_so() {
    for bytes in [Vec::new(), vec![0u8; 100], b"just some text".to_vec(), noise(5000)] {
        let e = crate::info::BasicFileInfo::parse(&bytes).unwrap_err();
        assert!(e.to_string().contains("BasicFileInfo"), "{e}");
    }
    let not_a_year = fixture::basic_file_info(&Basic { release: 77, ..Basic::default() });
    assert!(crate::info::BasicFileInfo::parse(&not_a_year).unwrap_err().to_string().contains("release"));
}

#[test]
fn basic_file_info_is_read_from_a_container_and_survives_damage() {
    let bytes = fixture::basic_file_info(&Basic { username: "jdoe", ..Basic::default() });
    let r = &mut Revit::from_bytes(container(&[("BasicFileInfo", &bytes)]), Limits::default()).unwrap();
    assert_eq!(r.basic_file_info().unwrap().release, 2024);
    let mut seed = 7u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 24) as usize
    };
    for _ in 0..3000 {
        let mut damaged = bytes.clone();
        for _ in 0..1 + next() % 5 {
            let at = next() % damaged.len();
            damaged[at] = next() as u8;
        }
        if next() % 7 == 0 {
            damaged.truncate(next() % damaged.len());
        }
        let _ = crate::info::BasicFileInfo::parse(&damaged);
    }
}

// ---------- the Atom entry ----------

const FAMILY_ATOM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<entry xmlns="http://www.w3.org/2005/Atom" xmlns:A="urn:schemas-autodesk-com:partatom"><title>Chair &amp; Table</title><updated>2023-03-27T12:09:23Z</updated><A:taxonomy><term>adsk:revit</term><label>Autodesk Revit</label></A:taxonomy><category><term>23.40.20.14.17</term><scheme>std:oc1</scheme></category><category><term>Furniture</term></category><link rel="design-2d"><A:design-file><A:title>chair.rfa</A:title><A:product-version>2024</A:product-version></A:design-file></link><A:features><A:feature><A:title>Other</A:title></A:feature></A:features></entry>"#;

#[test]
fn an_atom_entry_gives_the_title_the_omniclass_and_the_product_version() {
    let atom = crate::atom::Atom::parse(FAMILY_ATOM.as_bytes()).unwrap();
    assert_eq!(atom.title.as_deref(), Some("Chair & Table"), "the entry's own title, entities decoded, not a nested A:title");
    assert_eq!(atom.updated.as_deref(), Some("2023-03-27T12:09:23Z"));
    assert_eq!(atom.omniclass.as_deref(), Some("23.40.20.14.17"));
    assert_eq!(atom.categories, ["23.40.20.14.17", "Furniture"]);
    assert_eq!(atom.product_version.as_deref(), Some("2024"));
    // A nested title that comes first is not the entry's.
    let nested =
        crate::atom::Atom::parse(br#"<entry><link><A:title>nested</A:title><title>deeper</title></link><title>Real</title></entry>"#)
            .unwrap();
    assert_eq!(nested.title.as_deref(), Some("Real"));
    let attrs = br#"<entry xmlns="http://www.w3.org/2005/Atom"><title>T&#233;st</title><category term="22.1.2" scheme="x"/><!-- <title>no</title> --><![CDATA[<title>no</title>]]></entry>"#;
    let attrs = crate::atom::Atom::parse(attrs).unwrap();
    assert_eq!((attrs.title.as_deref(), attrs.omniclass.as_deref()), (Some("T\u{e9}st"), Some("22.1.2")));
}

#[test]
fn an_atom_entry_that_is_not_xml_or_is_hostile_is_an_error() {
    for bad in [&b"<entry><title>cut"[..], b"\xff\xfe\x00 not utf8", b"<entry><a><b>"] {
        assert!(crate::atom::Atom::parse(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
    }
    let deep = format!("<entry>{}", "<a>".repeat(200));
    assert!(crate::atom::Atom::parse(deep.as_bytes()).unwrap_err().to_string().contains("deep"));
    let big = vec![b' '; crate::atom::MAX_ATOM + 1];
    assert!(crate::atom::Atom::parse(&big).unwrap_err().to_string().contains("limit"));
}

#[test]
fn a_projects_atom_entry_is_the_first_entry_of_a_zip_and_the_entry_name_is_never_read_out() {
    for deflate in [true, false] {
        let zip = fixture::zip_entry(FAMILY_ATOM.as_bytes(), deflate);
        let atom = crate::atom::project_information(&zip, 1 << 20).unwrap();
        assert_eq!(atom.title.as_deref(), Some("Chair & Table"));
        assert!(!format!("{atom:?}").contains("someone"), "the entry name holds the saving user's folder");
    }
    let zip = fixture::zip_entry(FAMILY_ATOM.as_bytes(), true);
    assert!(crate::atom::project_information(&zip, 16).unwrap_err().to_string().contains("limit"));
    let stored = fixture::zip_entry(b"x", false);
    for bad in [&b""[..], b"PK\x03\x04", b"not a zip at all", &zip[..40], &stored[..20]] {
        assert!(crate::atom::project_information(bad, 1 << 20).is_err());
    }
    let mut method = zip.clone();
    method[8] = 12;
    assert!(crate::atom::project_information(&method, 1 << 20).unwrap_err().to_string().contains("method 12"));
    // A container with either stream gives the entry; with neither, none.
    let family = container(&[("BasicFileInfo", BASIC), ("PartAtom", FAMILY_ATOM.as_bytes())]);
    let r = &mut Revit::from_bytes(family, Limits::default()).unwrap();
    assert_eq!(r.document_atom().unwrap().unwrap().omniclass.as_deref(), Some("23.40.20.14.17"));
    let project = container(&[("BasicFileInfo", BASIC), ("ProjectInformation", &zip)]);
    let r = &mut Revit::from_bytes(project, Limits::default()).unwrap();
    assert_eq!(r.document_atom().unwrap().unwrap().title.as_deref(), Some("Chair & Table"));
    let r = &mut Revit::from_bytes(container(&[("BasicFileInfo", BASIC)]), Limits::default()).unwrap();
    assert!(r.document_atom().unwrap().is_none());
}

// ---------- the preview ----------

#[test]
fn a_preview_png_is_found_behind_revits_wrapper_checked_and_trimmed() {
    let png = fixture::png(40, 30);
    let mut stream = noise(300); // the wrapper in front, which holds no PNG signature
    stream.retain(|&b| b != 0x89);
    stream.extend_from_slice(&png);
    stream.extend_from_slice(&[9; 20]); // junk after IEND
    let found = crate::preview::find(&stream).unwrap();
    assert_eq!((found.width, found.height), (40, 30));
    assert_eq!(found.png, png);
    let r = &mut Revit::from_bytes(container(&[("BasicFileInfo", BASIC), ("RevitPreview4.0", &stream)]), Limits::default()).unwrap();
    assert_eq!(r.preview().unwrap().unwrap().png, png);
    let none = &mut Revit::from_bytes(container(&[("BasicFileInfo", BASIC)]), Limits::default()).unwrap();
    assert!(none.preview().unwrap().is_none(), "no preview stream is not an error");
}

#[test]
fn a_preview_that_is_not_a_sound_png_is_refused() {
    let good = fixture::png(8, 8);
    let mut cases: Vec<(Vec<u8>, &str)> = vec![
        (b"no png here".to_vec(), "no PNG"),
        (fixture::png(0, 5), "dimensions"),
        (fixture::png(5, 0), "dimensions"),
        (fixture::png(100_000, 10), "dimensions"),
        (good[..good.len() - 12].to_vec(), "IEND"),
        (good[..20].to_vec(), "cut short"),
    ];
    let mut flipped = good.clone();
    flipped[20] ^= 1; // inside IHDR's data
    cases.push((flipped, "checksum"));
    let mut not_ihdr = good.clone();
    not_ihdr[12..16].copy_from_slice(b"IDAT");
    cases.push((not_ihdr, "IHDR"));
    let mut huge_chunk = good.clone();
    huge_chunk[33..37].copy_from_slice(&u32::MAX.to_be_bytes()); // the first IDAT claims 4 GiB
    cases.push((huge_chunk, "over the limit"));
    for (bytes, says) in cases {
        let e = crate::preview::find(&bytes).unwrap_err();
        assert!(e.to_string().contains(says), "{says}: {e}");
    }
}

// ---------- the schema ----------

#[test]
fn the_schema_names_each_class_with_its_tag_base_and_field_count() {
    let base = Class { name: "Element", fields: &["m_id", "m_name"], base: None };
    let bytes = fixture::schema(&[
        Class { name: "Outline", fields: &["m_min", "m_max", "m_z"], base: None },
        Class { name: "Wall", fields: &[], base: Some(&base) },
        Class { name: "ElementParents", fields: &["m_a"], base: None },
    ]);
    let schema = crate::schema::Schema::parse(&bytes);
    assert_eq!(schema.stopped, None);
    assert_eq!(schema.parsed_bytes, bytes.len());
    let got: Vec<(u16, &str, Option<u16>, u32)> = schema.classes.iter().map(|c| (c.tag, c.name.as_str(), c.base, c.field_count)).collect();
    // `Wall` takes 13 and its inline base `Element` 14, so `ElementParents` is 15; the list is in tag order.
    assert_eq!(got, [(12, "Outline", None, 3), (13, "Wall", Some(14), 0), (14, "Element", None, 2), (15, "ElementParents", None, 1)]);
    assert_eq!(schema.tag("Wall"), Some(13));
    assert_eq!(schema.by_tag(15).map(|c| c.name.as_str()), Some("ElementParents"));
    assert!(schema.by_tag(11).is_none() && schema.by_tag(16).is_none() && schema.tag("Nope").is_none());
}

#[test]
fn the_schema_keeps_each_fields_name_and_type() {
    use crate::fixture::{Ty, TypedClass};
    use crate::schema::{Field, Type};
    let pair = Ty::fixed(7, 2);
    let mut bytes = fixture::typed_classes(&[
        TypedClass {
            name: "Point",
            base: None,
            fields: &[("uv", Ty { kind: 0x0d, modifier: 0x10, count: Some(2), inner: Some(&pair), class: None })],
        },
        TypedClass {
            name: "Face",
            base: Some(12),
            fields: &[
                ("m_pSurf", Ty::object(1, None)),
                ("m_next", Ty { kind: 0x0e, modifier: 0x13, count: Some(2), inner: None, class: None }),
                ("m_points", Ty::object(0x50, Some(12))),
                ("m_id", Ty::plain(11)),
                ("m_flags", Ty::plain(2)),
            ],
        },
    ]);
    bytes.extend_from_slice(&0u16.to_le_bytes());
    let schema = crate::schema::Schema::parse(&bytes);
    assert_eq!(schema.stopped, None);
    let plain = |kind, modifier, count| Type { kind, modifier, count, inner: None, class: None };
    let field = |name: &str, ty| Field { name: name.to_string(), ty };
    let point = schema.by_tag(12).unwrap();
    let uv = Type { inner: Some(Box::new(plain(7, 0x10, Some(2)))), ..plain(0x0d, 0x10, Some(2)) };
    assert_eq!(point.fields, [field("uv", uv)]);
    let face = schema.by_tag(13).unwrap();
    assert_eq!((face.base, face.field_count), (Some(12), 5));
    assert_eq!(
        face.fields,
        [
            field("m_pSurf", plain(0x0e, 1, None)),
            field("m_next", plain(0x0e, 0x13, Some(2))),
            field("m_points", Type { class: Some(12), ..plain(0x0e, 0x50, None) }),
            field("m_id", plain(11, 0, None)),
            field("m_flags", plain(2, 0, None)),
        ]
    );
}

#[test]
fn a_schema_that_breaks_the_grammar_stops_there_and_keeps_the_classes_before_it() {
    let good = fixture::schema(&[Class { name: "One", fields: &["a"], base: None }, Class { name: "Two", fields: &[], base: None }]);
    let mut bad = good.clone();
    let at = good.windows(3).rposition(|w| w == b"Two").unwrap();
    bad[at - 2] = 0xf0; // Two's name length is now absurd
    let schema = crate::schema::Schema::parse(&bad);
    assert_eq!(schema.classes.len(), 1);
    assert!(schema.stopped.as_deref().unwrap().contains("name length"), "{:?}", schema.stopped);
    assert!(crate::schema::Schema::parse(&[]).classes.is_empty());
    assert!(crate::schema::Schema::parse(&[1, 2, 3, 4]).stopped.is_some());
    // An inline base whose reference repeats another tag than the one it was given is not trusted.
    let mut mismatched =
        fixture::schema(&[Class { name: "Wall", fields: &[], base: Some(&Class { name: "Element", fields: &[], base: None }) }]);
    let at = mismatched.windows(4).position(|w| w == b"Wall").unwrap() + 4;
    mismatched[at..at + 2].copy_from_slice(&(0x8000u16 | 99).to_le_bytes());
    let schema = crate::schema::Schema::parse(&mismatched);
    assert!(schema.classes.is_empty() && schema.stopped.as_deref().unwrap().contains("inline tag"), "{:?}", schema.stopped);
    // Inline bases nest; a file that nests them without end stops instead of overflowing the stack.
    let mut nested = Vec::new();
    for tag in 12u16..400 {
        if tag == 12 {
            nested.extend_from_slice(&0u16.to_le_bytes());
        }
        nested.extend_from_slice(&1u16.to_le_bytes());
        nested.push(b'A');
        nested.extend_from_slice(&(0x8000 | (tag + 1)).to_le_bytes());
        nested.extend_from_slice(&0u16.to_le_bytes());
    }
    assert!(crate::schema::Schema::parse(&nested).stopped.as_deref().unwrap().contains("nest"));
}

#[test]
fn the_schema_is_read_from_formats_latest_in_a_container() {
    let bytes = fixture::schema(&[Class { name: "Level", fields: &["m_elevation"], base: None }]);
    let stream = paged(&gzip(&bytes));
    let r = &mut Revit::from_bytes(container(&[("BasicFileInfo", BASIC), ("Formats/Latest", &stream)]), Limits::default()).unwrap();
    assert_eq!(r.schema().unwrap().tag("Level"), Some(12));
    let r = &mut Revit::from_bytes(container(&[("BasicFileInfo", BASIC)]), Limits::default()).unwrap();
    assert!(r.schema().unwrap_err().to_string().contains("Formats/Latest"));
}

// ---------- real files ----------

/// The year a sample sits in a folder of, when its folder is named for one.
pub(super) fn folder_year(path: &Path) -> Option<u32> {
    path.parent()?.file_name()?.to_str()?.parse().ok()
}

#[test]
fn every_samples_metadata_and_schema_match_what_rvt_rs_reads_on_the_same_file() {
    // rvt-rs at b6086d4 on Autodesk's rstbasicsampleproject of each release: class count, and the tags of three classes.
    let rvt_rs = |year| match year {
        2023 => Some((4418, 316, 1410, 565)),
        2024 => Some((4492, 326, 1451, 584)),
        2025 => Some((4600, 345, 1491, 605)),
        2026 => Some((4690, 353, 1521, 613)),
        _ => None,
    };
    for path in samples() {
        let r = &mut Revit::open(&path, Limits::default()).unwrap();
        let info = r.basic_file_info().unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        eprintln!("{}: {info:?}", path.display());
        assert!((2015..=2030).contains(&info.release));
        if let Some(year) = folder_year(&path) {
            assert_eq!(info.release, year, "{}", path.display());
        }
        assert_eq!(info.guid.as_ref().map(String::len), Some(36), "a GUID");
        assert_eq!(info.locale.as_deref(), Some("ENU"));
        assert_eq!(info.worksharing.as_deref(), Some("Not enabled"));
        assert_eq!(info.username, None, "a file that is not workshared records no user");
        assert!(info.last_save_path.as_deref().is_some_and(|p| p.ends_with("rstbasicsampleproject.rvt")));
        assert!(info.build.is_some());
        assert_eq!(info.property("ClientAppName"), Some("RevitApplication"));
        let atom = r.document_atom().unwrap().expect("a project has a ProjectInformation entry");
        assert_eq!(atom.title.as_deref(), Some("rstbasicsampleproject"));
        assert_eq!(atom.product_version, Some(info.release.to_string()));
        assert!(atom.updated.is_some());
        assert!(r.preview().unwrap().is_none(), "the sample projects carry no preview");
        let schema = r.schema().unwrap();
        assert_eq!(schema.stopped, None, "{}", path.display());
        assert_eq!(schema.parsed_bytes, r.inflate("Formats/Latest").unwrap().bytes.len(), "every byte of the schema is read");
        if let Some((count, outline, parents, plane)) = rvt_rs(info.release) {
            assert_eq!(schema.classes.len(), count);
            assert_eq!(
                (schema.tag("Outline"), schema.tag("ElementParents"), schema.tag("Plane")),
                (Some(outline), Some(parents), Some(plane))
            );
        }
        for name in ["Level", "SWall", "Floor", "FamilyInstance", "FamilySymbol", "ElementHeader"] {
            assert!(schema.tag(name).is_some(), "{name}");
        }
        assert_eq!(schema.by_tag(schema.tag("Level").unwrap()).map(|c| c.name.as_str()), Some("Level"));
    }
}
