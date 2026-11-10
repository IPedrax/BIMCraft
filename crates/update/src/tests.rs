//! The updater's checks, with a network that is a table of answers: no test touches the real one.

use std::io::Write;
use std::path::Path;

use semver::Version;
use serde_json::{Value, json};

use crate::testing::{Cut, Fake, Scratch, digest_field};
use crate::*;

const V040: &str = include_str!("../testdata/latest-v0.4.0.json");
const NAME: &str = "bimcraft-0.5.0-windows-x64.msi";

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

const WIN: Option<&Target> = Some(&Target::Windows);

/// A reply read as the Windows installer's release, which is what most of these tests are about.
fn parse(json: &[u8]) -> Result<Release, String> {
    parse_release(json, Some(&Target::Windows))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    out.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref());
    out
}

/// A reply for release `tag` with these assets, flags as GitHub sends them.
fn reply(tag: &str, assets: Value) -> Vec<u8> {
    json!({"tag_name": tag, "draft": false, "prerelease": false, "html_url": "https://example.test/x", "assets": assets})
        .to_string()
        .into_bytes()
}

fn asset(name: &str, size: Value, digest: &str) -> Value {
    json!({"name": name, "size": size, "digest": digest, "state": "uploaded", "browser_download_url": "https://evil.example/x.msi"})
}

/// The installer of release 0.5.0 that `bytes` would make.
fn installer(bytes: &[u8]) -> Installer {
    Installer {
        name: NAME.into(),
        url: format!("https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/{NAME}"),
        size: bytes.len() as u64,
        sha256: sha(bytes),
        checksums: None,
    }
}

#[test]
fn a_tag_is_a_v_and_a_stable_version() {
    assert_eq!(tag_version("v0.5.0"), Some(v("0.5.0")));
    assert_eq!(tag_version("v10.20.30"), Some(v("10.20.30")));
    let bad = [
        "",
        "v",
        "0.5.0",
        "V0.5.0",
        "vv0.5.0",
        " v0.5.0",
        "v0.5.0 ",
        "v0.5.0\n",
        "v0.5",
        "v0.5.0.1",
        "v0.5.0-rc.1",
        "v0.5.0-",
        "v0.5.0+build",
        "v01.5.0",
        "v-1.0.0",
        "v0.5.0/../x",
        "v\u{660}.\u{665}.\u{660}",
        "v99999999999999999999.0.0",
        "nightly",
    ];
    for tag in bad {
        assert_eq!(tag_version(tag), None, "{tag:?}");
    }
    assert_eq!(tag_version(&format!("v1.2.{}", "9".repeat(5000))), None);
}

#[test]
fn newer_is_semver_order_not_text_order() {
    assert!(is_newer(&v("0.5.0"), &v("0.4.0")));
    assert!(is_newer(&v("0.10.0"), &v("0.9.0")), "ten is more than nine");
    assert!(is_newer(&v("1.0.0"), &v("0.99.99")));
    assert!(!is_newer(&v("0.4.0"), &v("0.4.0")));
    assert!(!is_newer(&v("0.3.9"), &v("0.4.0")));
    assert!(is_newer(&v("0.5.0"), &v("0.5.0-rc.1")), "a release is newer than its own candidate");
    assert!(!is_newer(&v("0.5.0"), &v("0.5.1-rc.1")));
}

#[test]
fn the_real_v0_4_0_reply_parses() {
    let r = parse(V040.as_bytes()).unwrap();
    assert_eq!(r.version, v("0.4.0"));
    assert_eq!(r.page, "https://github.com/IPedrax/BIMCraft/releases/tag/v0.4.0");
    let i = r.installer.unwrap();
    assert_eq!(i.name, "bimcraft-0.4.0-windows-x64.msi");
    assert_eq!(i.url, "https://github.com/IPedrax/BIMCraft/releases/download/v0.4.0/bimcraft-0.4.0-windows-x64.msi");
    assert_eq!(i.size, 8_966_144);
    assert_eq!(hex(&i.sha256), "52ec6b0f58ed78fef0353e5f2e75ab230bf6e0e66ddc3d4cc3add4baffeacb68");
}

#[test]
fn drafts_prereleases_and_odd_tags_are_not_updates() {
    let real: Value = serde_json::from_str(V040).unwrap();
    for field in ["draft", "prerelease"] {
        let mut flagged = real.clone();
        flagged[field] = json!(true);
        assert!(parse(flagged.to_string().as_bytes()).is_err(), "{field}");
        let mut unsaid = real.clone();
        unsaid.as_object_mut().unwrap().remove(field);
        assert!(parse(unsaid.to_string().as_bytes()).is_err(), "no {field}");
    }
    for tag in ["v0.5.0-rc.1", "nightly", "0.5.0", ""] {
        let mut odd = real.clone();
        odd["tag_name"] = json!(tag);
        assert!(parse(odd.to_string().as_bytes()).is_err(), "{tag:?}");
    }
}

#[test]
fn garbage_replies_are_errors_not_panics() {
    let deep = "[".repeat(100_000);
    let long = format!("{{\"tag_name\": \"{}\"}}", "v".repeat(2_000_000));
    for junk in [
        "",
        "null",
        "true",
        "0",
        "\"v0.5.0\"",
        "[]",
        "{}",
        "{\"tag_name\": 5}",
        "{\"tag_name\": \"v0.5.0\"}",
        "<html>rate limited</html>",
        "\u{0}\u{1}",
        &deep,
        &long,
    ] {
        assert!(parse(junk.as_bytes()).is_err(), "{:.30}", junk);
    }
    assert!(parse(&[0xff, 0xfe, 0x00]).is_err());
}

#[test]
fn only_our_own_whole_installer_with_a_digest_is_offered() {
    let good = digest_field(b"an installer");
    let release = |assets: Value| parse(&reply("v0.5.0", assets)).unwrap();
    let i = release(json!([asset(NAME, json!(12), &good)])).installer.unwrap();
    assert_eq!((i.name.as_str(), i.size), (NAME, 12));
    assert_eq!(hex(&i.sha256), good.strip_prefix("sha256:").unwrap());
    let digest = "a".repeat(64);
    let cases = [
        ("another name", asset("bimcraft-0.5.0-windows-x64.zip", json!(12), &good)),
        ("a path with backslashes", asset("..\\bimcraft-0.5.0-windows-x64.msi", json!(12), &good)),
        ("a path with slashes", asset("sub/bimcraft-0.5.0-windows-x64.msi", json!(12), &good)),
        ("another version", asset("bimcraft-0.4.0-windows-x64.msi", json!(12), &good)),
        ("no digest", json!({"name": NAME, "size": 12, "state": "uploaded"})),
        ("a null digest", json!({"name": NAME, "size": 12, "digest": null, "state": "uploaded"})),
        ("an md5", asset(NAME, json!(12), &format!("md5:{}", "a".repeat(32)))),
        ("a short digest", asset(NAME, json!(12), &format!("sha256:{}", &digest[1..]))),
        ("a long digest", asset(NAME, json!(12), &format!("sha256:{digest}0"))),
        ("a digest that isn't hex", asset(NAME, json!(12), &format!("sha256:{}", "g".repeat(64)))),
        ("a digest with no algorithm", asset(NAME, json!(12), &digest)),
        ("no bytes", asset(NAME, json!(0), &good)),
        ("too many bytes", asset(NAME, json!(MAX_DOWNLOAD + 1), &good)),
        ("a negative size", asset(NAME, json!(-1), &good)),
        ("a fractional size", asset(NAME, json!(12.5), &good)),
        ("a size in words", asset(NAME, json!("12"), &good)),
        ("an upload still going", json!({"name": NAME, "size": 12, "digest": good, "state": "open"})),
        ("junk", json!(5)),
    ];
    for (what, a) in cases {
        let r = release(json!([a]));
        assert!(r.installer.is_none(), "{what}");
        assert_eq!(r.version, v("0.5.0"), "{what}: the release is still there to tell the user about");
    }
    let at_the_cap = release(json!([asset(NAME, json!(MAX_DOWNLOAD), &good)]));
    assert_eq!(at_the_cap.installer.map(|i| i.size), Some(MAX_DOWNLOAD), "the cap itself is allowed");
    // A bad asset beside the good one doesn't hide it, and the first of two of a name is the one.
    let other = asset(NAME, json!(99), &digest_field(b"another"));
    let kept = release(json!([5, null, "x", {"name": 7}, asset(NAME, json!(12), &good), other]));
    assert_eq!(kept.installer.unwrap().size, 12);
}

#[test]
fn the_download_address_is_built_here_not_read_from_the_reply() {
    let mut j: Value = serde_json::from_slice(&reply("v0.5.0", json!([asset(NAME, json!(12), &digest_field(b"x"))]))).unwrap();
    j["html_url"] = json!("javascript:alert(1)");
    j["assets"][0]["url"] = json!("https://evil.example/api");
    let r = parse(j.to_string().as_bytes()).unwrap();
    assert_eq!(r.page, "https://github.com/IPedrax/BIMCraft/releases/tag/v0.5.0");
    assert_eq!(r.installer.unwrap().url, format!("https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/{NAME}"));
}

#[test]
fn check_reads_the_latest_release_and_compares_it_with_the_running_version() {
    let net = Fake::default().answer(LATEST_URL, V040);
    assert_eq!(check(&net, &v("0.3.0"), WIN).unwrap().map(|r| r.version), Some(v("0.4.0")));
    for running in ["0.4.0", "0.4.1", "1.0.0", "0.5.0-rc.1"] {
        assert!(check(&net, &v(running), WIN).unwrap().is_none(), "{running}");
    }
    assert_eq!(net.asked(), [LATEST_URL; 5]);
}

#[test]
fn check_treats_a_bad_reply_or_a_bad_network_as_an_error() {
    let current = v("0.3.0");
    let e = check(&Fake::default().fail(LATEST_URL, "offline"), &current, WIN).unwrap_err();
    assert!(e.contains("offline"), "{e}");
    assert!(check(&Fake::default().answer(LATEST_URL, "<html>"), &current, WIN).is_err());
    assert!(check(&Fake::default(), &current, WIN).is_err(), "a 404");
    assert!(check(&Cut(b"{\"tag_name\"".to_vec()), &current, WIN).is_err(), "a dropped connection");
}

#[test]
fn check_refuses_a_reply_over_the_cap_and_takes_one_at_it() {
    let padded = |n: u64| {
        let mut body = V040.as_bytes().to_vec();
        body.resize(n as usize, b' ');
        body
    };
    let current = v("0.3.0");
    assert!(check(&Fake::default().answer(LATEST_URL, padded(MAX_JSON)), &current, WIN).unwrap().is_some());
    let e = check(&Fake::default().answer(LATEST_URL, padded(MAX_JSON + 1)), &current, WIN).unwrap_err();
    assert!(e.contains("too large"), "{e}");
}

#[test]
fn a_download_that_matches_is_kept_in_a_folder_of_its_own() {
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let i = installer(&bytes);
    let net = Fake::default().answer(&i.url, bytes.clone());
    let parent = Scratch::new("good");
    let mut seen = Vec::new();
    let file = download(&net, &i, &parent.0, &mut |n| seen.push(n)).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(file.file_name().unwrap(), NAME);
    let folder = file.parent().unwrap();
    assert_eq!(folder.parent().unwrap(), parent.0);
    assert!(folder.file_name().unwrap().to_string_lossy().starts_with("bimcraft-update-"));
    assert_eq!(net.asked(), std::slice::from_ref(&i.url));
    assert!(seen.windows(2).all(|w| w[0] < w[1]), "progress only goes up: {seen:?}");
    assert_eq!(seen.last(), Some(&200_000));
    let again = download(&net, &i, &parent.0, &mut |_| {}).unwrap();
    assert_ne!(again.parent(), file.parent(), "each download has a fresh folder");
}

#[test]
fn a_download_is_checked_against_a_known_sha256() {
    // FIPS 180-2's example: SHA-256("abc").
    let abc = Installer { name: NAME.into(), url: "https://x".into(), size: 3, sha256: sha(b"abc"), checksums: None };
    assert_eq!(hex(&abc.sha256), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let parent = Scratch::new("abc");
    assert!(download(&Fake::default().answer("https://x", "abc"), &abc, &parent.0, &mut |_| {}).is_ok());
    assert!(download(&Fake::default().answer("https://x", "abd"), &abc, &parent.0, &mut |_| {}).is_err());
}

#[test]
fn a_download_that_fails_a_check_leaves_nothing_behind() {
    let bytes = vec![7u8; 5000];
    let i = installer(&bytes);
    let mut flipped = bytes.clone();
    flipped[2500] ^= 1;
    let mut long = bytes.clone();
    long.push(0);
    let cases = [
        ("a flipped bit", Fake::default().answer(&i.url, flipped), "SHA-256"),
        ("a byte too many", Fake::default().answer(&i.url, long), "larger than the 5000 bytes"),
        ("a byte short", Fake::default().answer(&i.url, &bytes[..4999]), "4999 bytes, not the 5000"),
        ("nothing", Fake::default().answer(&i.url, ""), "0 bytes, not the 5000"),
        ("an error", Fake::default().fail(&i.url, "connection reset"), "connection reset"),
        ("no answer", Fake::default(), "404"),
    ];
    for (what, net, says) in cases {
        let parent = Scratch::new("bad");
        let e = download(&net, &i, &parent.0, &mut |_| {}).unwrap_err();
        assert!(e.contains(says), "{what}: {e}");
        assert_eq!(std::fs::read_dir(&parent.0).unwrap().count(), 0, "{what}: {e}");
    }
    let parent = Scratch::new("cut");
    assert!(download(&Cut(bytes[..3000].to_vec()), &i, &parent.0, &mut |_| {}).is_err());
    assert_eq!(std::fs::read_dir(&parent.0).unwrap().count(), 0, "a dropped connection");
}

#[test]
fn a_download_stops_taking_bytes_at_the_size_the_release_gave() {
    // The cap is the release's own size, which is far under MAX_DOWNLOAD, so a long answer is cut off early.
    struct Endless(std::sync::atomic::AtomicU64);
    impl Net for Endless {
        fn get(&self, _: &str, _: u64, out: &mut dyn std::io::Write) -> Result<(), String> {
            loop {
                out.write_all(&[0; 1000]).map_err(|e| e.to_string())?;
                self.0.fetch_add(1000, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    let net = Endless(Default::default());
    let parent = Scratch::new("endless");
    assert!(download(&net, &installer(&[0; 5000]), &parent.0, &mut |_| {}).is_err());
    assert!(net.0.load(std::sync::atomic::Ordering::Relaxed) <= 6000, "it read on after the size was passed");
}

#[test]
fn a_download_that_cannot_make_its_folder_is_an_error() {
    let parent = Scratch::new("file");
    let file = parent.0.join("a file, not a folder");
    std::fs::write(&file, "x").unwrap();
    let i = installer(b"abc");
    let net = Fake::default().answer(&i.url, "abc");
    assert!(download(&net, &i, &file, &mut |_| {}).is_err());
    assert!(download(&net, &i, &parent.0.join("missing").join("deeper"), &mut |_| {}).is_err());
}

#[test]
fn a_file_is_hashed_as_it_is_checked_before_it_runs() {
    let parent = Scratch::new("hash");
    let file = parent.0.join("f");
    std::fs::write(&file, "abc").unwrap();
    assert_eq!(hex(&sha256_file(&file).unwrap()), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert!(sha256_file(Path::new("/no/such/file")).is_err());
}

/// The manifests (path from the root, text) that break the network rule: `ureq` and the other HTTP clients belong to the two crates
/// that reach the network, the updater and the assistant, and those two are dependencies of the desktop (`apps/bimcraft`, which
/// attaches them, and `crates/ui-egui`, which draws them) and of nothing else.
fn network_offenders(manifests: &[(String, String)]) -> Vec<String> {
    let owners = ["crates/update/", "crates/assistant/"];
    let desktop = ["apps/bimcraft/", "crates/ui-egui/"];
    let depends = |text: &str, dep: &str| text.lines().any(|l| l.trim_start().starts_with(dep));
    let mut bad = Vec::new();
    for (path, text) in manifests {
        for dep in ["ureq", "rustls", "reqwest", "hyper"] {
            if depends(text, dep) && !owners.iter().any(|o| path.starts_with(o)) {
                bad.push(format!("{path} depends on {dep}: only the updater and the assistant reach the network"));
            }
        }
        for dep in ["bimcraft-update", "bimcraft-assistant"] {
            if depends(text, dep) && !desktop.iter().any(|d| path.starts_with(d)) {
                bad.push(format!("{path} depends on {dep}: the core, the CLI and the MCP server never reach the network"));
            }
        }
    }
    bad
}

#[test]
fn only_the_desktop_app_can_reach_the_network() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut manifests = Vec::new();
    for folder in ["crates", "apps"] {
        for entry in std::fs::read_dir(root.join(folder)).unwrap() {
            let dir = entry.unwrap().path();
            if let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) {
                manifests.push((format!("{folder}/{}/Cargo.toml", dir.file_name().unwrap().to_string_lossy()), text));
            }
        }
    }
    assert!(manifests.len() > 8, "the manifests were found: {}", manifests.len());
    assert_eq!(network_offenders(&manifests), Vec::<String>::new());
}

#[test]
fn the_network_rule_catches_a_manifest_that_breaks_it() {
    let m = |path: &str, text: &str| (path.to_string(), text.to_string());
    let fine = [
        m("crates/update/Cargo.toml", "[dependencies]\nureq = { workspace = true }\n"),
        m("crates/assistant/Cargo.toml", "[dependencies]\nureq = { workspace = true }\nbimcraft-core = { workspace = true }\n"),
        m("crates/ui-egui/Cargo.toml", "bimcraft-update = { workspace = true }\nbimcraft-assistant = { workspace = true }\n"),
        m("apps/bimcraft/Cargo.toml", "bimcraft-update = { workspace = true }\nbimcraft-assistant = { workspace = true }\n"),
        m("crates/core/Cargo.toml", "name = \"bimcraft-core\"\n"),
    ];
    assert!(network_offenders(&fine).is_empty());
    for (path, text) in [
        ("crates/core/Cargo.toml", "bimcraft-assistant = { workspace = true }\n"),
        ("apps/bimcraft-cli/Cargo.toml", "bimcraft-assistant = { workspace = true }\n"),
        ("apps/bimcraft-cli/Cargo.toml", "bimcraft-update = { workspace = true }\n"),
        ("crates/core/Cargo.toml", "ureq = \"3\"\n"),
        ("crates/plugin/Cargo.toml", "reqwest = \"0.12\"\n"),
        ("apps/bimcraft-cli/Cargo.toml", "  hyper = \"1\"\n"),
    ] {
        assert_eq!(network_offenders(&[m(path, text)]).len(), 1, "{path}: {text}");
    }
}

// ---------- preferences ----------

use std::ffi::OsString;
use std::path::PathBuf;

use crate::prefs::{self, Prefs};

#[test]
fn preferences_check_by_default_and_keep_what_was_saved() {
    let dir = Scratch::new("prefs");
    let path = dir.0.join("BIMCraft").join("update.json");
    assert_eq!(Prefs::load(&path), Prefs::default());
    assert_eq!(Prefs::default(), Prefs { check: true, skip: None });
    let chosen = Prefs { check: false, skip: Some(v("0.5.0")) };
    chosen.save(&path).unwrap();
    assert_eq!(Prefs::load(&path), chosen);
    assert!(!path.with_extension("json.part").exists(), "written whole, or not at all");
}

#[test]
fn a_damaged_preferences_file_gives_the_defaults_a_field_at_a_time() {
    let dir = Scratch::new("damaged");
    let path = dir.0.join("update.json");
    let load = |text: &str| {
        std::fs::write(&path, text).unwrap();
        Prefs::load(&path)
    };
    for junk in ["", "null", "[]", "{", "\u{0}", "{\"check\": \"no\"}", "{\"check\": null}"] {
        assert_eq!(load(junk), Prefs::default(), "{junk:?}");
    }
    assert_eq!(load(r#"{"check": false, "skip": "banana"}"#), Prefs { check: false, skip: None });
    assert_eq!(load(r#"{"check": 5, "skip": "0.6.1", "more": "fields"}"#), Prefs { check: true, skip: Some(v("0.6.1")) });
    let padded = |size: usize| format!(r#"{{"check": false}}{}"#, " ".repeat(size - r#"{"check": false}"#.len()));
    assert_eq!(load(&padded(65_536)), Prefs { check: false, skip: None }, "a file at the limit is read");
    assert_eq!(load(&padded(65_537)), Prefs::default(), "one byte over is too large to be a settings file");
    assert_eq!(Prefs::load(&dir.0), Prefs::default(), "a folder where the file should be");
}

#[test]
fn preferences_sit_beside_the_favorites_under_a_name_of_their_own() {
    let same = |favorites: &str, update: &str| assert_eq!(prefs::beside(Path::new(favorites)), PathBuf::from(update), "{favorites}");
    same("/h/.config/bimcraft/favorites.json", "/h/.config/bimcraft/update.json");
    same("/Users/x/Library/Application Support/BIMCraft/favorites.json", "/Users/x/Library/Application Support/BIMCraft/update.json");
    same("favorites.json", "update.json");
    // Wherever the user pointed the favorites (`BIMCRAFT_FAVORITES`), whatever the file is called.
    same("/data/mine.json", "/data/update.json");
}

// ---------- the network's own rule ----------

#[test]
fn a_redirect_may_only_go_where_github_serves_releases() {
    let ok = [
        "https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/x.msi",
        "https://api.github.com/repos/IPedrax/BIMCraft/releases/latest",
        "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r&sv=2018",
        "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2",
        "https://RELEASE-ASSETS.githubusercontent.com/x",
        "https://GitHub.com/ipedrax/bimcraft/releases/download/v0.5.0/x.msi",
    ];
    for url in ok {
        assert!(net::hop_allowed(url), "{url}");
    }
    let bad = [
        "",
        "http://github.com/x",
        "https://github.com.evil.example/x",
        "https://evilgithub.com/x",
        "https://github.com@evil.example/x",
        "https://evil.example/https://github.com/",
        "https://github.com:8443/x",
        "https://github.com./x",
        "ftp://github.com/x",
        "file:///etc/passwd",
        "//github.com/x",
        "github.com/x",
        "https:///x",
        "https://githubusercontent.com/x",
        "https://xrelease-assets.githubusercontent.com/x",
        "https://release-assets.githubusercontent.com.evil.example/",
        "https://github.com\\@evil.example/",
        " https://github.com/x",
        "https://github.com/x y",
        "https://raw.githubusercontent.com/x",
        "https://user:pass@github.com/x",
        "/relative/path",
        // The right addresses, in the clear.
        "http://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/x.msi",
        "http://api.github.com/repos/IPedrax/BIMCraft/releases/latest",
        "http://release-assets.githubusercontent.com/github-production-release-asset/1/2",
        // GitHub's own hosts, but not this repository's releases.
        "https://github.com/",
        "https://github.com/evil/repo/releases/download/v1/x.msi",
        "https://github.com/IPedrax/BIMCraft/archive/refs/heads/main.zip",
        "https://github.com/IPedrax/BIMCraft2/releases/download/v1/x.msi",
        "https://github.com/IPedrax/BIMCraft/releases/download/../../../evil/x",
        "https://github.com/IPedrax/BIMCraft/releases/download/v%2e%2e/x",
        "https://api.github.com/user",
        "https://api.github.com/repos/evil/repo/releases/latest",
        "https://api.github.com/repos/IPedrax/BIMCraft/releases/../../../../user",
    ];
    for url in bad {
        assert!(!net::hop_allowed(url), "{url:?}");
    }
}

/// A web server on this machine, for what the real network code does with what it's sent: it answers each request with
/// what `route` makes of the request's path and the server's own address, as the bytes of a whole HTTP response.
fn serve(route: impl Fn(&str, &str) -> Vec<u8> + Send + 'static) -> String {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let own = base.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                request.push(byte[0]);
            }
            let line = String::from_utf8_lossy(&request).lines().next().unwrap_or_default().to_string();
            let path = line.split(' ').nth(1).unwrap_or("/").to_string();
            let _ = stream.write_all(&route(&path, &own));
        }
    });
    base
}

fn http(status: &str, headers: &[String], body: &[u8]) -> Vec<u8> {
    let mut reply = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    headers.iter().for_each(|h| reply += &format!("{h}\r\n"));
    reply += "\r\n";
    [reply.into_bytes(), body.to_vec()].concat()
}

#[test]
fn the_network_code_takes_a_body_of_exactly_the_limit_and_nothing_more() {
    // The real downloader failed at the end of a real 8.9 MB installer, because ureq's own limit fails the read after the last byte.
    let base = serve(|path, _| match path {
        "/exact" => http("200 OK", &[], &[7; 5000]),
        "/long" => http("200 OK", &[], &[7; 5001]),
        _ => http("404 Not Found", &[], b"nothing here"),
    });
    let net = net::Ureq::loopback(&base);
    let mut got = Vec::new();
    net.get(&format!("{base}/exact"), 5000, &mut got).unwrap();
    assert_eq!(got, vec![7; 5000]);
    assert!(net.get(&format!("{base}/long"), 5000, &mut Vec::new()).is_err(), "a byte over the limit");
    let missing = net.get(&format!("{base}/missing"), 5000, &mut Vec::new()).unwrap_err();
    assert!(missing.contains("404"), "{missing}");
    // And through the updater's own writers: the exact size passes, and one byte over is cut off.
    let installer = Installer { name: NAME.into(), url: format!("{base}/exact"), size: 5000, sha256: sha(&[7; 5000]), checksums: None };
    let parent = Scratch::new("loopback");
    assert!(download(&net, &installer, &parent.0, &mut |_| {}).is_ok());
    let long = Installer { url: format!("{base}/long"), ..installer };
    assert!(download(&net, &long, &parent.0, &mut |_| {}).is_err());
    assert_eq!(std::fs::read_dir(&parent.0).unwrap().count(), 1, "only the good download's folder is left");
}

#[test]
fn redirects_are_followed_by_hand_and_only_to_where_they_may_go() {
    // A second server, which the first redirects to and the updater may not go to: it must see no request.
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let elsewhere = serve(move |_, _| {
        counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        http("200 OK", &[], b"not for you")
    });
    let away = format!("Location: {elsewhere}/x");
    let base = serve(move |path, own| match path {
        "/redirect" => http("302 Found", &[format!("Location: {own}/exact")], b""),
        "/exact" => http("200 OK", &[], b"the file"),
        "/away" => http("302 Found", std::slice::from_ref(&away), b""),
        "/relative" => http("302 Found", &["Location: /exact".into()], b""),
        "/loop" => http("302 Found", &[format!("Location: {own}/loop")], b""),
        "/nowhere" => http("302 Found", &[], b""),
        _ => http("404 Not Found", &[], b""),
    });
    let net = net::Ureq::loopback(&base);
    let mut got = Vec::new();
    net.get(&format!("{base}/redirect"), 100, &mut got).unwrap();
    assert_eq!(got, b"the file");
    for (path, why) in
        [("/away", "won't fetch"), ("/relative", "won't fetch"), ("/loop", "too many redirects"), ("/nowhere", "redirect to nowhere")]
    {
        let e = net.get(&format!("{base}{path}"), 100, &mut Vec::new()).unwrap_err();
        assert!(e.contains(why), "{path}: {e}");
    }
    assert_eq!(hits.load(std::sync::atomic::Ordering::Relaxed), 0, "a redirect to another address is never followed");
    // The production network refuses what isn't GitHub before it sends anything.
    assert!(net::Ureq::new().get(&format!("{base}/exact"), 100, &mut Vec::new()).is_err());
}

// ---------- installing ----------

use crate::install::{self, Finish};

#[test]
fn only_the_exe_the_installer_put_in_program_files_is_updated_in_place() {
    let pf = "C:\\Program Files";
    let yes = [
        "C:\\Program Files\\BIMCraft\\bimcraft.exe",
        "c:\\program files\\bimcraft\\BIMCRAFT.EXE",
        "\\\\?\\C:\\Program Files\\BIMCraft\\bimcraft.exe",
        "C:/Program Files/BIMCraft/bimcraft.exe",
    ];
    for exe in yes {
        assert!(install::is_installed(exe, pf), "{exe}");
    }
    assert!(install::is_installed("C:\\Program Files\\BIMCraft\\bimcraft.exe", "C:\\Program Files\\"));
    let no = [
        "D:\\dev\\BIMCraft\\target\\release\\bimcraft.exe",
        "C:\\Program Files\\BIMCraft\\sub\\bimcraft.exe",
        "C:\\Program Files (x86)\\BIMCraft\\bimcraft.exe",
        "C:\\Program Files\\Other\\bimcraft.exe",
        "C:\\Program Files\\BIMCraft\\bimcraft-cli.exe",
        "C:\\Program Files\\BIMCraft\\bimcraft.exe.bak",
        "/usr/bin/bimcraft",
        "",
    ];
    for exe in no {
        assert!(!install::is_installed(exe, pf), "{exe}");
    }
    assert!(!install::is_installed("C:\\Program Files\\BIMCraft\\bimcraft.exe", ""));
}

#[test]
fn the_installer_is_run_by_its_full_path_when_windows_says_where_it_is() {
    let found = install::msiexec_path(Some(OsString::from("C:\\Windows")));
    assert_eq!(found, PathBuf::from("C:\\Windows").join("System32").join("msiexec.exe"));
    assert_eq!(install::msiexec_path(None), PathBuf::from("msiexec"));
}

#[test]
fn a_folder_that_can_be_written_to_marks_a_process_with_administrator_rights() {
    let dir = Scratch::new("probe");
    assert!(install::writable(&dir.0));
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0, "the probe leaves nothing behind");
    assert!(!install::writable(&dir.0.join("missing")));
    #[cfg(target_os = "linux")]
    assert!(!install::writable(Path::new("/proc")), "not even root writes there");
}

#[test]
fn the_copy_does_nothing_for_arguments_an_update_does_not_make() {
    let finish = |msi: &str, exe: &str| Finish { msi: msi.into(), sha256: [0; 32], exe: exe.into(), project: None };
    let (installed, good) = ("C:\\Program Files\\BIMCraft\\bimcraft.exe", "/tmp/bimcraft-update-1f-2a-0/bimcraft-0.5.0-windows-x64.msi");
    let pf = Some("C:\\Program Files");
    assert!(install::plausible(&finish(good, installed), pf));
    assert!(install::plausible(&finish(good, "/anything"), None), "where there's no Program Files to check against");
    let bad = [
        ("/home/me/thesis.docx", installed),
        ("/tmp/bimcraft-update-1/bimcraft-0.5.0-windows-x64.msi.bak", installed),
        ("/tmp/bimcraft-update-1/bimcraft-01.5.0-windows-x64.msi", installed),
        ("/tmp/bimcraft-update-1/bimcraft-0.5.0-rc.1-windows-x64.msi", installed),
        ("/tmp/bimcraft-update-1/bimcraft-0.5.0-windows-arm64.msi", installed),
        ("/tmp/bimcraft-update-1/other-0.5.0-windows-x64.msi", installed),
        ("/tmp/Documents/bimcraft-0.5.0-windows-x64.msi", installed),
        ("bimcraft-0.5.0-windows-x64.msi", installed),
        (good, "C:\\evil\\any.exe"),
        (good, "C:\\Program Files\\BIMCraft\\other.exe"),
    ];
    for (msi, exe) in bad {
        assert!(!install::plausible(&finish(msi, exe), pf), "{msi} then {exe}");
    }
}

#[test]
fn the_handover_names_what_to_install_and_what_to_start_again() {
    let (msi, exe, project) =
        (Path::new("/t/bimcraft-update-1/x.msi"), Path::new("/p/BIMCraft/bimcraft.exe"), Path::new("/w/house.bimcraft"));
    let digest = sha(b"x");
    let parse = |args: Vec<OsString>| install::parse(args);
    let with = parse(install::helper_args(msi, &digest, exe, Some(project))).unwrap();
    assert_eq!(with, Finish { msi: msi.into(), sha256: digest, exe: exe.into(), project: Some(project.into()) });
    let without = parse(install::helper_args(msi, &digest, exe, None)).unwrap();
    assert_eq!(without.project, None);
    let mut extra = install::helper_args(msi, &digest, exe, Some(project));
    extra.push("one more".into());
    assert_eq!(parse(extra), None, "a handover has at most the project after the program");
    let mut wrong_flag = install::helper_args(msi, &digest, exe, None);
    wrong_flag[0] = "--finish-updates".into();
    assert_eq!(parse(wrong_flag), None, "the flag is what says it's a handover");
    let good = hex(&digest);
    let os = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
    let bad = [
        os(&[]),
        os(&["file.bimcraft"]),
        os(&["--sample"]),
        os(&["--finish-update"]),
        os(&["--finish-update", "a.msi", &good]),
        os(&["--finish-update", "a.msi", "nothex", "app"]),
        os(&["--finish-update", "a.msi", &good[1..], "app"]),
        os(&["--finish-update", "a.msi", &good, "app", "project", "one more"]),
        os(&["x", "--finish-update", "a.msi", &good, "app"]),
    ];
    for args in bad {
        assert_eq!(parse(args.clone()), None, "{args:?}");
    }
}

#[cfg(unix)]
mod unix {
    //! The installer and the app are scripts that write down how they were run, so the order and the arguments show.

    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime};

    use super::*;

    /// One test at a time writes a program and starts one: a process forked meanwhile by another test inherits the file while
    /// it's open for writing, and starting the program then fails with "Text file busy".
    pub(super) fn serial() -> std::sync::MutexGuard<'static, ()> {
        static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A reader that blocks until its sender goes, as the helper's stdin does until the app exits.
    struct Gate(mpsc::Receiver<()>);

    impl std::io::Read for Gate {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            let _ = self.0.recv();
            Ok(0)
        }
    }

    /// What the scripts wrote, once there are `lines` of it.
    pub(super) fn log(path: &Path, lines: usize) -> Vec<String> {
        for _ in 0..200 {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if text.lines().count() >= lines {
                return text.lines().map(String::from).collect();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::read_to_string(path).unwrap_or_default().lines().map(String::from).collect()
    }

    struct Rig {
        dir: Scratch,
        msi: PathBuf,
        log: PathBuf,
        installer: PathBuf,
        app: PathBuf,
    }

    fn rig(installer_exit: i32) -> Rig {
        let dir = Scratch::new("finish");
        let msi = dir.0.join("bimcraft-0.5.0-windows-x64.msi");
        std::fs::write(&msi, "installer bytes").unwrap();
        let log = dir.0.join("log");
        let installer = script(&dir.0, "msiexec", &format!("echo installer \"$@\" >> {}\nexit {installer_exit}", log.display()));
        let app = script(&dir.0, "app", &format!("echo app \"$@\" >> {}", log.display()));
        Rig { dir, msi, log, installer, app }
    }

    fn finish(rig: &Rig, bytes: &[u8], project: Option<&str>) -> Finish {
        Finish { msi: rig.msi.clone(), sha256: sha(bytes), exe: rig.app.clone(), project: project.map(PathBuf::from) }
    }

    #[test]
    fn the_helper_waits_for_the_app_to_exit_then_installs_then_starts_it_again() {
        let _one = serial();
        let rig = rig(0);
        let f = finish(&rig, b"installer bytes", Some("/w/house.bimcraft"));
        let (exit, gate) = mpsc::channel::<()>();
        let helper = {
            let (f, installer) = (f.clone(), rig.installer.clone());
            std::thread::spawn(move || install::finish_with(&f, &mut Gate(gate), &installer))
        };
        std::thread::sleep(Duration::from_millis(150));
        assert!(!rig.log.exists(), "nothing runs while the app is still there");
        drop(exit);
        helper.join().unwrap();
        let lines = log(&rig.log, 2);
        let installer_log = rig.dir.0.join("install.log");
        assert_eq!(lines[0], format!("installer /i {} /passive /norestart /l*v {}", rig.msi.display(), installer_log.display()));
        assert_eq!(lines[1], "app /w/house.bimcraft");
        assert!(!rig.msi.exists(), "the installer is taken away once it has run");
    }

    #[test]
    fn an_installer_windows_will_not_open_held_is_run_again_unheld() {
        let _one = serial();
        let rig = rig(0);
        let marker = rig.dir.0.join("second");
        let body =
            format!("echo installer >> {0}\nif [ -f {1} ]; then exit 0; fi\ntouch {1}\nexit 84", rig.log.display(), marker.display());
        let installer = script(&rig.dir.0, "msiexec-picky", &body);
        let ran = install::finish_after(&finish(&rig, b"installer bytes", None), &mut std::io::empty(), &installer, 84);
        assert!(ran);
        assert_eq!(log(&rig.log, 3), ["installer", "installer", "app"], "refused once, then run");
    }

    #[test]
    fn an_installer_changed_while_it_refused_is_not_run_again() {
        let _one = serial();
        let rig = rig(0);
        let body = format!("echo installer >> {0}\necho swapped > {1}\nexit 84", rig.log.display(), rig.msi.display());
        let installer = script(&rig.dir.0, "msiexec-swap", &body);
        install::finish_after(&finish(&rig, b"installer bytes", None), &mut std::io::empty(), &installer, 84);
        assert_eq!(log(&rig.log, 2), ["installer", "app"]);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(std::fs::read_to_string(&rig.log).unwrap().lines().count(), 2, "and not a third line, for a second run");
    }

    #[test]
    fn the_copy_runs_nothing_for_arguments_that_are_not_an_update_and_the_installer_by_its_full_path_for_one() {
        let _one = serial();
        let dir = Scratch::new("run");
        let system = dir.0.join("System32");
        std::fs::create_dir(&system).unwrap();
        let trace = dir.0.join("log");
        script(&system, "msiexec.exe", &format!("echo installer >> {}", trace.display()));
        let app = script(&dir.0, "app", &format!("echo app >> {}", trace.display()));
        let folder = dir.0.join("bimcraft-update-1");
        std::fs::create_dir(&folder).unwrap();
        let made = |msi: PathBuf| {
            std::fs::write(&msi, "installer bytes").unwrap();
            Finish { msi, sha256: sha(b"installer bytes"), exe: app.clone(), project: None }
        };
        let root = Some(dir.0.clone().into_os_string());
        // Not named for a version, or not in a folder an update made: nothing is run, deleted or started.
        let odd = made(dir.0.join("thesis.docx"));
        assert!(!install::run_with(&odd, &mut std::io::empty(), None, root.clone()));
        let misplaced = made(dir.0.join("bimcraft-0.5.0-windows-x64.msi"));
        assert!(!install::run_with(&misplaced, &mut std::io::empty(), None, root.clone()));
        std::thread::sleep(Duration::from_millis(150));
        assert!(!trace.exists() && odd.msi.exists() && misplaced.msi.exists(), "nothing happened");
        // What an update makes runs Windows' own installer, from where Windows says its folder is.
        let real = made(folder.join("bimcraft-0.5.0-windows-x64.msi"));
        assert!(install::run_with(&real, &mut std::io::empty(), None, root));
        assert_eq!(log(&trace, 2), ["installer", "app"]);
    }

    #[test]
    fn the_app_starts_again_whatever_the_installer_did() {
        let _one = serial();
        for exit in [0, 1602, 3010] {
            let rig = rig(exit);
            install::finish_with(&finish(&rig, b"installer bytes", None), &mut std::io::empty(), &rig.installer);
            assert_eq!(log(&rig.log, 2)[1], "app", "installer exit {exit}");
        }
    }

    #[test]
    fn an_installer_that_is_not_what_was_checked_is_not_run() {
        let _one = serial();
        let rig = rig(0);
        let ran = install::finish_with(&finish(&rig, b"other bytes", None), &mut std::io::empty(), &rig.installer);
        assert!(!ran);
        assert_eq!(log(&rig.log, 1), ["app"], "the app starts again; the installer never ran");
        let missing = Finish { msi: rig.dir.0.join("gone.msi"), ..finish(&rig, b"x", None) };
        assert!(!install::finish_with(&missing, &mut std::io::empty(), &rig.installer));
        assert_eq!(log(&rig.log, 2), ["app", "app"]);
    }

    #[test]
    fn the_handover_copies_the_app_beside_the_installer_and_starts_the_copy() {
        let _one = serial();
        let dir = Scratch::new("handover");
        let folder = dir.0.join("bimcraft-update-1");
        std::fs::create_dir(&folder).unwrap();
        let msi = folder.join("bimcraft-0.5.0-windows-x64.msi");
        std::fs::write(&msi, "installer bytes").unwrap();
        let log = dir.0.join("log");
        // The stand-in for the app notes its arguments, then reads its input to the end, as the helper does, and notes that too.
        let exe = script(&dir.0, "bimcraft", &format!("echo \"$@\" >> {0}\ncat > /dev/null\necho eof >> {0}", log.display()));
        install::start_with(&exe, &msi, &sha(b"installer bytes"), Some(Path::new("/w/house.bimcraft"))).unwrap();
        let copy = folder.join(format!("bimcraft-update{}", std::env::consts::EXE_SUFFIX));
        assert!(copy.exists(), "a copy that no installer replaces, beside the installer");
        let args = log_args(&log);
        let finish = install::parse(args).unwrap();
        assert_eq!(finish, Finish { msi, sha256: sha(b"installer bytes"), exe, project: Some("/w/house.bimcraft".into()) });
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1, "the copy's input stays open as long as this process lives");
    }

    fn log_args(path: &Path) -> Vec<OsString> {
        log(path, 1).iter().flat_map(|l| l.split(' ').map(OsString::from).collect::<Vec<_>>()).collect()
    }

    #[test]
    fn old_update_folders_are_swept_and_nothing_else() {
        let _one = serial();
        let dir = Scratch::new("sweep");
        let make = |name: &str, days: u64| {
            let path = dir.0.join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("f"), "x").unwrap();
            std::fs::File::open(&path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(days * 86_400)).unwrap();
            path
        };
        let (old, new, other) = (make("bimcraft-update-aaa", 3), make("bimcraft-update-bbb", 0), make("somebody-else", 30));
        let kept = make("bimcraft-update-ccc", 5);
        std::os::unix::fs::symlink(&other, dir.0.join("bimcraft-update-link")).unwrap();
        std::fs::write(dir.0.join("bimcraft-update-file"), "x").unwrap();
        install::sweep(&dir.0, Some(&kept));
        assert!(!old.exists());
        assert!(kept.exists(), "the folder of an update that's ready is not swept");
        assert!(new.exists() && other.join("f").exists());
        assert!(dir.0.join("bimcraft-update-link").exists() && dir.0.join("bimcraft-update-file").exists());
    }
}

// ---------- the updater ----------

use std::sync::Arc;

use crate::testing::release_json;

const INSTALLER: &[u8] = b"pretend this is an installer";

/// An updater of version 0.4.0 on this network, doing its work as it's asked, with a settings file and a temp folder of its own.
fn updater(net: &Arc<Fake>, dir: &Scratch) -> Updater {
    Updater::new(net.clone(), v("0.4.0"), Some(dir.0.join("update.json")), || {})
        .inline()
        .target(Some(Target::Windows))
        .in_folder(dir.0.clone())
}

fn latest(tag: &str, installer: Option<&[u8]>) -> Arc<Fake> {
    let installer_url =
        format!("https://github.com/IPedrax/BIMCraft/releases/download/{tag}/{}", Target::Windows.asset_name(tag.trim_start_matches('v')));
    Arc::new(Fake::default().answer(LATEST_URL, release_json(tag, installer)).answer(&installer_url, installer.unwrap_or_default()))
}

fn offered(u: &Updater) -> Option<Version> {
    match u.state() {
        State::Available(r) => Some(r.version.clone()),
        _ => None,
    }
}

#[test]
fn at_startup_a_newer_release_is_offered_and_nothing_is_said() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u1"));
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    assert!(u.poll().is_none());
    assert_eq!(offered(&u), Some(v("0.5.0")));
}

#[test]
fn an_equal_release_is_quiet_at_startup_and_up_to_date_by_hand() {
    let (net, dir) = (latest("v0.4.0", Some(INSTALLER)), Scratch::new("u2"));
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    assert!(u.poll().is_none() && matches!(u.state(), State::Idle));
    u.check_now();
    assert_eq!(u.poll(), Some(Note::UpToDate));
}

#[test]
fn a_failed_check_is_quiet_at_startup_and_said_by_hand() {
    let (net, dir) = (Arc::new(Fake::default().fail(LATEST_URL, "offline")), Scratch::new("u3"));
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    assert!(u.poll().is_none() && matches!(u.state(), State::Idle));
    u.check_now();
    assert!(matches!(u.poll(), Some(Note::CheckFailed(e)) if e.contains("offline")));
}

#[test]
fn the_startup_check_can_be_switched_off_and_stays_off() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u4"));
    let mut u = updater(&net, &dir);
    assert!(u.checks_at_startup());
    u.set_checks_at_startup(false).unwrap();
    u.check_at_startup();
    assert!(net.asked().is_empty(), "off means off");
    let mut again = updater(&net, &dir);
    assert!(!again.checks_at_startup(), "it was kept");
    again.check_at_startup();
    assert!(net.asked().is_empty());
    again.check_now();
    again.poll();
    assert_eq!(offered(&again), Some(v("0.5.0")), "a check by hand still works");
}

#[test]
fn skip_this_version_hides_it_until_something_newer_comes() {
    let dir = Scratch::new("u5");
    let mut u = updater(&latest("v0.5.0", Some(INSTALLER)), &dir);
    u.check_at_startup();
    u.poll();
    u.skip().unwrap();
    assert!(matches!(u.state(), State::Idle));
    let mut next = updater(&latest("v0.5.0", Some(INSTALLER)), &dir);
    next.check_at_startup();
    next.poll();
    assert!(matches!(next.state(), State::Idle), "skipped stays skipped across runs");
    next.check_now();
    next.poll();
    assert_eq!(offered(&next), Some(v("0.5.0")), "asked by hand, it is shown");
    let mut newer = updater(&latest("v0.5.1", Some(INSTALLER)), &dir);
    newer.check_at_startup();
    newer.poll();
    assert_eq!(offered(&newer), Some(v("0.5.1")));
}

#[test]
fn later_hides_it_for_this_run_only() {
    let dir = Scratch::new("u6");
    let mut u = updater(&latest("v0.5.0", Some(INSTALLER)), &dir);
    u.check_at_startup();
    u.poll();
    u.later();
    assert!(matches!(u.state(), State::Idle));
    let mut next = updater(&latest("v0.5.0", Some(INSTALLER)), &dir);
    next.check_at_startup();
    next.poll();
    assert_eq!(offered(&next), Some(v("0.5.0")));
}

#[test]
fn update_now_downloads_and_checks_the_installer() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u7"));
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    u.poll();
    u.update_now();
    assert!(u.poll().is_none());
    let State::Ready { release, file } = u.state() else { panic!("not ready: {:?}", u.state()) };
    assert_eq!(release.version, v("0.5.0"));
    assert_eq!(std::fs::read(file).unwrap(), INSTALLER);
    assert_eq!(net.asked().len(), 2, "the release, then the installer, and nothing else");
}

#[test]
fn install_hands_the_checked_installer_over_once() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u7b"));
    let handed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&handed);
    let mut u = updater(&net, &dir).handing_over(move |job: &Handover| {
        log.lock().unwrap().push((job.file.clone(), job.sha256, job.version.clone(), job.project.clone()));
        Ok(())
    });
    let project_open = std::env::temp_dir().join("house.bimcraft");
    u.install(None);
    assert!(handed.lock().unwrap().is_empty(), "nothing has been downloaded");
    u.check_at_startup();
    u.poll();
    u.update_now();
    u.poll();
    u.install(Some(&project_open));
    assert_eq!(u.poll(), Some(Note::Installed), "it's in, and the window closes");
    assert!(matches!(u.state(), State::Idle), "once handed over, the banner has nothing to offer");
    u.install(None);
    let handed = handed.lock().unwrap().clone();
    assert_eq!(handed.len(), 1, "and a second click starts no second one");
    let (file, digest, version, project) = &handed[0];
    assert_eq!((*digest, version, project.as_deref()), (sha(INSTALLER), &v("0.5.0"), Some(project_open.as_path())));
    assert_eq!(std::fs::read(file).unwrap(), INSTALLER, "the file that passed its checks");
}

#[test]
fn the_project_is_handed_over_by_an_absolute_path() {
    // The new version may start somewhere else (macOS's `open` starts it with `/` as its folder): a path as the file was opened by,
    // relative to where this one was started, would be no file there.
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u7h"));
    let handed = Arc::new(std::sync::Mutex::new(None));
    let log = Arc::clone(&handed);
    let mut u = ready(&net, &dir, move |job| {
        *log.lock().unwrap() = Some(job.project.clone());
        Ok(())
    });
    u.install(Some(Path::new("house.bimcraft")));
    assert_eq!(u.poll(), Some(Note::Installed));
    let project = handed.lock().unwrap().clone().unwrap().unwrap();
    assert!(project.is_absolute() && project.ends_with("house.bimcraft"), "{project:?}");
}

/// An updater whose update is downloaded and ready, and whose hand-over `handing_over` is.
fn ready(net: &Arc<Fake>, dir: &Scratch, handing_over: impl Fn(&Handover) -> Result<(), String> + Send + Sync + 'static) -> Updater {
    let mut u = updater(net, dir).handing_over(handing_over);
    u.check_at_startup();
    u.poll();
    u.update_now();
    u.poll();
    assert!(matches!(u.state(), State::Ready { .. }), "{:?}", u.state());
    u
}

#[test]
fn the_window_is_woken_each_time_work_comes_back() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u7g"));
    let woken = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count = Arc::clone(&woken);
    let wake = move || {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    };
    let mut u =
        Updater::new(net, v("0.4.0"), None, wake).inline().target(Some(Target::Windows)).in_folder(dir.0.clone()).handing_over(|_| Ok(()));
    let woken = || woken.load(std::sync::atomic::Ordering::SeqCst);
    u.check_at_startup();
    assert_eq!(woken(), 1, "the check is back");
    u.poll();
    u.update_now();
    assert_eq!(woken(), 2, "the download is");
    u.poll();
    u.install(None);
    assert_eq!(woken(), 3, "and the install");
}

#[test]
fn a_hand_over_that_fails_says_why_and_leaves_the_update_ready_to_try_again() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u7c"));
    let tries = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counted = Arc::clone(&tries);
    let mut u = ready(&net, &dir, move |_| match counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
        0 => Err("no room".into()),
        _ => Ok(()),
    });
    u.install(None);
    assert_eq!(u.poll(), Some(Note::InstallFailed("no room".into())));
    let State::Ready { file, .. } = u.state() else { panic!("not ready again: {:?}", u.state()) };
    assert_eq!(std::fs::read(file).unwrap(), INSTALLER, "the download is still there");
    u.install(None);
    assert_eq!(u.poll(), Some(Note::Installed), "the second try goes through");
}

#[test]
fn a_panic_in_a_hand_over_is_an_error_not_a_hang() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u7d"));
    let mut u = ready(&net, &dir, |_| panic!("the installer blew up"));
    u.install(None);
    assert!(matches!(u.poll(), Some(Note::InstallFailed(e)) if e.contains("unexpectedly")));
    assert!(matches!(u.state(), State::Ready { .. }));
}

/// Polls until `done` holds of the updater, or the notes have run out of time. The notes polled on the way, last one first.
#[cfg(unix)]
fn until(u: &mut Updater, done: impl Fn(&Updater) -> bool) -> Option<Note> {
    let mut note = None;
    for _ in 0..2000 {
        note = u.poll().or(note);
        if done(u) {
            return note;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("gave up waiting: {:?}", u.state());
}

#[cfg(unix)]
#[test]
fn while_it_installs_nothing_else_changes_the_update_or_takes_its_folder() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u7e"));
    let (open, gate) = std::sync::mpsc::channel::<()>();
    let gate = std::sync::Mutex::new(gate);
    // On threads of its own, as in the app: the hand-over waits at the gate, as a slow install does.
    let mut u = Updater::new(net.clone(), v("0.4.0"), Some(dir.0.join("update.json")), || {})
        .target(Some(Target::Windows))
        .in_folder(dir.0.clone())
        .handing_over(move |_| {
            gate.lock().map_err(|e| e.to_string())?.recv_timeout(std::time::Duration::from_secs(20)).map_err(|e| e.to_string())
        });
    u.check_at_startup();
    until(&mut u, |u| matches!(u.state(), State::Available(_)));
    u.update_now();
    until(&mut u, |u| matches!(u.state(), State::Ready { .. }));
    let State::Ready { file, .. } = u.state().clone() else { panic!("not ready: {:?}", u.state()) };
    u.install(None);
    assert!(matches!(u.state(), State::Installing { .. }), "{:?}", u.state());
    // Everything the window offers is a click away, and none of it may touch an update that's being put in.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400);
    std::fs::File::open(file.parent().unwrap()).unwrap().set_modified(old).unwrap();
    let before = net.asked().len();
    u.check_now();
    until(&mut u, |_| net.asked().len() > before);
    std::thread::sleep(std::time::Duration::from_millis(100));
    u.poll();
    u.later();
    u.update_now();
    u.install(None);
    assert!(matches!(u.state(), State::Installing { .. }), "a check, Later, Update now, Install: {:?}", u.state());
    assert!(file.exists(), "its folder was swept from under the install");
    open.send(()).unwrap();
    let note = until(&mut u, |u| matches!(u.state(), State::Idle));
    assert_eq!(note, Some(Note::Installed));
}

#[test]
fn a_download_that_fails_its_check_goes_back_to_the_offer_and_says_why() {
    let dir = Scratch::new("u8");
    let url = "https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/bimcraft-0.5.0-windows-x64.msi";
    let net =
        Arc::new(Fake::default().answer(LATEST_URL, release_json("v0.5.0", Some(INSTALLER))).answer(url, "pretend this is another one"));
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    u.poll();
    u.update_now();
    assert!(matches!(u.poll(), Some(Note::DownloadFailed(_))));
    assert_eq!(offered(&u), Some(v("0.5.0")), "it can be tried again");
    let left: Vec<_> =
        std::fs::read_dir(&dir.0).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("bimcraft-update-")).collect();
    assert!(left.is_empty(), "no folder with a failed download in it");
}

#[test]
fn update_now_needs_an_installer_and_a_way_to_run_it() {
    let dir = Scratch::new("u9");
    let net = latest("v0.5.0", None);
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    u.poll();
    u.update_now();
    assert_eq!(offered(&u), Some(v("0.5.0")), "a release with no installer is only told about");
    let net = latest("v0.5.0", Some(INSTALLER));
    let mut elsewhere = updater(&net, &dir).target(None);
    elsewhere.check_at_startup();
    elsewhere.poll();
    elsewhere.update_now();
    assert_eq!(offered(&elsewhere), Some(v("0.5.0")));
    assert_eq!(net.asked().len(), 1, "the installer was never asked for");
    elsewhere.install(None);
    assert!(elsewhere.poll().is_none() && offered(&elsewhere).is_some(), "nothing is ready to install");
}

#[cfg(unix)]
#[test]
fn a_ready_update_keeps_its_folder_however_long_the_banner_has_been_up() {
    let (net, dir) = (latest("v0.5.0", Some(INSTALLER)), Scratch::new("u11"));
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    u.poll();
    u.update_now();
    u.poll();
    let State::Ready { file, .. } = u.state().clone() else { panic!("not ready: {:?}", u.state()) };
    let folder = file.parent().unwrap().to_path_buf();
    let three_days = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400);
    std::fs::File::open(&folder).unwrap().set_modified(three_days).unwrap();
    u.check_now();
    u.poll();
    assert!(file.exists(), "swept from under a banner that still offers it");
    u.later();
    u.check_now();
    u.poll();
    assert!(!folder.exists(), "with nothing waiting on it, the old folder goes at the next check");
}

#[test]
fn a_panic_in_the_network_is_an_error_not_a_hang() {
    struct Boom;
    impl Net for Boom {
        fn get(&self, _: &str, _: u64, _: &mut dyn std::io::Write) -> Result<(), String> {
            panic!("the network blew up")
        }
    }
    let dir = Scratch::new("u10");
    let mut u = Updater::new(Arc::new(Boom), v("0.4.0"), None, || {}).inline();
    u.check_now();
    assert!(matches!(u.poll(), Some(Note::CheckFailed(_))));
    assert!(matches!(u.state(), State::Idle));
    drop(dir);
}

// ---------- a file for each kind of install, and the release's checksum file ----------

/// Every file the release workflow would publish for `version`, by name, each with bytes of its own.
fn files(version: &str) -> Vec<(String, Vec<u8>)> {
    let mut names = vec![format!("bimcraft-{version}-windows-x64.msi"), format!("bimcraft-{version}-windows-x64-portable.zip")];
    for arch in ["x86_64", "aarch64"] {
        for extension in ["AppImage", "AppImage.zsync", "deb", "rpm", "tar.gz"] {
            names.push(format!("bimcraft-{version}-linux-{arch}.{extension}"));
        }
    }
    names.push(format!("bimcraft-{version}-macos-universal.dmg"));
    names.push(format!("bimcraft-cli-{version}-macos-universal.zip"));
    names.into_iter().map(|name| (name.clone(), format!("the bytes of {name}").into_bytes())).collect()
}

/// What `sha256sum *` writes for them.
fn sums_of(files: &[(String, Vec<u8>)]) -> String {
    files.iter().map(|(name, bytes)| format!("{}  {name}\n", hex(&sha(bytes)))).collect()
}

/// A reply listing `files`, and a SHA256SUMS of `sums` when there is one.
fn release_of(tag: &str, files: &[(String, Vec<u8>)], sums: Option<&str>) -> Vec<u8> {
    let mut assets: Vec<Value> = files.iter().map(|(name, bytes)| asset(name, json!(bytes.len()), &digest_field(bytes))).collect();
    assets.extend(sums.map(|text| asset("SHA256SUMS", json!(text.len()), &digest_field(text.as_bytes()))));
    reply(tag, json!(assets))
}

const SUMS_URL: &str = "https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/SHA256SUMS";

#[test]
fn each_kind_of_install_has_its_own_file_in_a_release() {
    assert_eq!(Target::Windows.asset_name("0.5.0"), "bimcraft-0.5.0-windows-x64.msi");
    assert_eq!(Target::AppImage(Arch::X86_64).asset_name("0.5.0"), "bimcraft-0.5.0-linux-x86_64.AppImage");
    assert_eq!(Target::AppImage(Arch::Aarch64).asset_name("0.5.0"), "bimcraft-0.5.0-linux-aarch64.AppImage");
    assert_eq!(Target::MacApp.asset_name("0.5.0"), "bimcraft-0.5.0-macos-universal.dmg");
    assert_eq!((Arch::from_name("x86_64"), Arch::from_name("aarch64")), (Some(Arch::X86_64), Some(Arch::Aarch64)));
    for other in ["arm", "arm64", "x86", "riscv64", "powerpc64", "", "X86_64", "x86_64 "] {
        assert_eq!(Arch::from_name(other), None, "{other:?}");
    }
}

#[test]
fn a_release_with_a_file_for_every_platform_gives_each_install_its_own() {
    let all = files("0.5.0");
    let json = release_of("v0.5.0", &all, Some(&sums_of(&all)));
    let wanted = [
        (Target::Windows, "bimcraft-0.5.0-windows-x64.msi"),
        (Target::AppImage(Arch::X86_64), "bimcraft-0.5.0-linux-x86_64.AppImage"),
        (Target::AppImage(Arch::Aarch64), "bimcraft-0.5.0-linux-aarch64.AppImage"),
        (Target::MacApp, "bimcraft-0.5.0-macos-universal.dmg"),
    ];
    for (target, name) in wanted {
        let i = parse_release(&json, Some(&target)).unwrap().installer.unwrap();
        let bytes = &all.iter().find(|(n, _)| n == name).unwrap().1;
        assert_eq!((i.name.as_str(), i.size, i.sha256), (name, bytes.len() as u64, sha(bytes)), "{target:?}");
        assert_eq!(i.url, format!("https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/{name}"), "{target:?}");
        assert_eq!(i.checksums.as_deref(), Some(SUMS_URL), "{target:?}");
    }
    let bare = parse_release(&json, None).unwrap();
    assert!(bare.installer.is_none(), "a copy that can't be updated in place has no file to take");
    assert_eq!(bare.version, v("0.5.0"));
}

#[test]
fn the_other_files_of_a_platform_are_never_taken_for_its_update() {
    let targets = [Target::Windows, Target::AppImage(Arch::X86_64), Target::AppImage(Arch::Aarch64), Target::MacApp];
    let names: Vec<String> = targets.iter().map(|t| t.asset_name("0.5.0")).collect();
    // The delta file, the packages, the tarball and the CLI are near misses, as are the files of another version.
    let mut others: Vec<_> = files("0.5.0").into_iter().filter(|(n, _)| !names.contains(n)).collect();
    others.extend(files("0.4.0").into_iter().filter(|(n, _)| n.ends_with(".msi") || n.ends_with(".dmg") || n.ends_with(".AppImage")));
    assert!(others.len() > 10);
    let json = release_of("v0.5.0", &others, Some(&sums_of(&others)));
    for target in targets {
        assert!(parse_release(&json, Some(&target)).unwrap().installer.is_none(), "{target:?}");
    }
}

#[test]
fn a_platforms_file_is_checked_on_its_own() {
    let assets: Vec<Value> = files("0.5.0")
        .iter()
        .map(|(name, bytes)| {
            // The disk image is listed with no digest.
            if name.ends_with(".dmg") {
                json!({"name": name, "size": bytes.len(), "state": "uploaded"})
            } else {
                asset(name, json!(bytes.len()), &digest_field(bytes))
            }
        })
        .collect();
    let json = reply("v0.5.0", json!(assets));
    assert!(parse_release(&json, Some(&Target::MacApp)).unwrap().installer.is_none(), "that costs the Mac its update");
    assert!(parse_release(&json, Some(&Target::Windows)).unwrap().installer.is_some(), "and nobody else theirs");
    assert!(parse_release(&json, Some(&Target::AppImage(Arch::Aarch64))).unwrap().installer.is_some());
}

#[test]
fn a_checksum_file_is_read_as_sha256sum_writes_it() {
    let (a, b) = (sha(b"a"), sha(b"b"));
    let text = format!("{}  one.msi\n{} *two.dmg\r\n{}  three.deb\n", hex(&a), hex(&b).to_uppercase(), hex(&a));
    assert_eq!(checksum_of(&text, "one.msi"), Ok(Some(a)));
    assert_eq!(checksum_of(&text, "two.dmg"), Ok(Some(b)), "binary mode, upper case digits, a Windows line end");
    assert_eq!(checksum_of(&text, "three.deb"), Ok(Some(a)));
    for absent in ["four.rpm", "one", "one.ms", "", " one.msi", "ONE.MSI"] {
        assert_eq!(checksum_of(&text, absent), Ok(None), "{absent:?}");
    }
    // A file listed twice with one sum is as good as once; with two, there is nothing to trust.
    assert_eq!(checksum_of(&format!("{0}  x\n{0} *x\n", hex(&a)), "x"), Ok(Some(a)));
    assert!(checksum_of(&format!("{}  x\n{}  x\n", hex(&a), hex(&b)), "x").is_err());
    // Lines that aren't sums are skipped, unless they are for the file asked about.
    for noise in ["", "\n\n\n", "just words", "  ", "# a comment", "\u{0}\u{1}\u{2}", "\t\t"] {
        assert_eq!(checksum_of(noise, "x"), Ok(None), "{noise:?}");
    }
    let broken = [
        "zz  x".to_string(),
        "abcd  x".into(),
        format!("{}0  x", hex(&a)),
        format!("sha256:{}  x", hex(&a)),
        format!("+{}  x", &hex(&a)[1..]),
    ];
    for line in broken {
        assert!(checksum_of(&line, "x").is_err(), "{line:?}");
    }
    assert_eq!(checksum_of(&"a".repeat(1_000_000), "x"), Ok(None));
}

/// The Windows installer of release 0.5.0 when it lists `sums` as its checksum file, and a network serving both.
fn with_sums(sums: &str) -> (Installer, Fake) {
    let all = files("0.5.0");
    let installer = parse_release(&release_of("v0.5.0", &all, Some(sums)), Some(&Target::Windows)).unwrap().installer.unwrap();
    let net = Fake::default().answer(&installer.url, all[0].1.clone()).answer(SUMS_URL, sums);
    (installer, net)
}

#[test]
fn a_checksum_file_that_agrees_lets_the_download_go_on() {
    let (installer, net) = with_sums(&sums_of(&files("0.5.0")));
    let parent = Scratch::new("sums-agree");
    assert!(download(&net, &installer, &parent.0, &mut |_| {}).unwrap().exists());
    assert_eq!(net.asked(), [SUMS_URL.to_string(), installer.url.clone()], "the small file first");
}

#[test]
fn a_checksum_file_that_disagrees_stops_the_download_before_it_starts() {
    let mut other = files("0.5.0");
    other[0].1 = b"another file".to_vec();
    let (installer, net) = with_sums(&sums_of(&other));
    let parent = Scratch::new("sums-disagree");
    let e = download(&net, &installer, &parent.0, &mut |_| {}).unwrap_err();
    assert!(e.contains("checksum file") && e.contains(&installer.name), "{e}");
    assert_eq!(net.asked(), [SUMS_URL], "the installer itself was never asked for");
    assert_eq!(std::fs::read_dir(&parent.0).unwrap().count(), 0, "and no folder was made");
}

#[test]
fn a_checksum_file_that_cannot_be_had_or_cannot_be_read_stops_the_download_too() {
    let all = files("0.5.0");
    let (installer, _) = with_sums(&sums_of(&all));
    let msi = all[0].1.clone();
    let name = &installer.name;
    let clash = format!("{}  {name}\n{}  {name}\n", hex(&sha(b"x")), hex(&sha(b"y")));
    let cases = [
        ("no answer", Fake::default().answer(&installer.url, msi.clone()), "404"),
        ("a failed answer", Fake::default().fail(SUMS_URL, "connection reset").answer(&installer.url, msi.clone()), "connection reset"),
        ("too large", Fake::default().answer(SUMS_URL, vec![b'#'; MAX_SUMS as usize + 1]).answer(&installer.url, msi.clone()), "too large"),
        ("two sums for the file", Fake::default().answer(SUMS_URL, clash).answer(&installer.url, msi.clone()), "two different"),
        (
            "a broken line for the file",
            Fake::default().answer(SUMS_URL, format!("nonsense  {name}\n")).answer(&installer.url, msi),
            "isn't a SHA-256",
        ),
    ];
    for (what, net, says) in cases {
        let parent = Scratch::new("sums-fail");
        let e = download(&net, &installer, &parent.0, &mut |_| {}).unwrap_err();
        assert!(e.contains(says), "{what}: {e}");
        assert!(!net.asked().contains(&installer.url), "{what}: the installer was asked for");
        assert_eq!(std::fs::read_dir(&parent.0).unwrap().count(), 0, "{what}");
    }
}

#[test]
fn a_checksum_file_with_nothing_on_the_file_changes_nothing() {
    let all = files("0.5.0");
    let (installer, net) = with_sums(&sums_of(&all[1..]));
    let parent = Scratch::new("sums-silent");
    assert!(download(&net, &installer, &parent.0, &mut |_| {}).is_ok());
}

#[test]
fn a_release_with_no_checksum_file_is_taken_on_githubs_digest_alone() {
    let all = files("0.5.0");
    let installer = parse_release(&release_of("v0.5.0", &all, None), Some(&Target::Windows)).unwrap().installer.unwrap();
    assert_eq!(installer.checksums, None);
    let net = Fake::default().answer(&installer.url, all[0].1.clone());
    let parent = Scratch::new("sums-none");
    assert!(download(&net, &installer, &parent.0, &mut |_| {}).is_ok());
    assert_eq!(net.asked(), std::slice::from_ref(&installer.url), "no request for a file the release doesn't list");
}

#[test]
fn only_a_whole_checksum_file_the_release_lists_is_asked_for() {
    let all = files("0.5.0");
    let good = |extra: Value| {
        let mut assets: Vec<Value> = all.iter().map(|(name, bytes)| asset(name, json!(bytes.len()), &digest_field(bytes))).collect();
        assets.push(extra);
        parse(&reply("v0.5.0", json!(assets))).unwrap().installer.unwrap().checksums
    };
    let named = |name: &str, size: Value, state: &str| json!({"name": name, "size": size, "state": state});
    assert_eq!(good(named("SHA256SUMS", json!(500), "uploaded")).as_deref(), Some(SUMS_URL));
    assert_eq!(good(named("SHA256SUMS", json!(MAX_SUMS), "uploaded")).as_deref(), Some(SUMS_URL), "at the cap");
    for (what, odd) in [
        ("still uploading", named("SHA256SUMS", json!(500), "open")),
        ("empty", named("SHA256SUMS", json!(0), "uploaded")),
        ("over the cap", named("SHA256SUMS", json!(MAX_SUMS + 1), "uploaded")),
        ("a size in words", named("SHA256SUMS", json!("500"), "uploaded")),
        ("another case", named("sha256sums", json!(500), "uploaded")),
        ("another name", named("SHA256SUMS.txt", json!(500), "uploaded")),
        ("a path", named("../SHA256SUMS", json!(500), "uploaded")),
    ] {
        assert_eq!(good(odd), None, "{what}");
    }
}

#[test]
fn a_checksum_file_that_disagrees_stops_an_update_and_says_so() {
    let mut other = files("0.5.0");
    other[0].1 = b"another file".to_vec();
    let sums = sums_of(&other);
    let all = files("0.5.0");
    let installer_url = "https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/bimcraft-0.5.0-windows-x64.msi";
    let net = Arc::new(
        Fake::default()
            .answer(LATEST_URL, release_of("v0.5.0", &all, Some(&sums)))
            .answer(SUMS_URL, sums)
            .answer(installer_url, all[0].1.clone()),
    );
    let dir = Scratch::new("u12");
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    u.poll();
    u.update_now();
    assert!(matches!(u.poll(), Some(Note::DownloadFailed(e)) if e.contains("checksum file")));
    assert_eq!(offered(&u), Some(v("0.5.0")));
    assert_eq!(net.asked(), [LATEST_URL, SUMS_URL]);
}

#[test]
fn an_update_checks_the_checksum_file_before_the_installer_and_is_ready_when_they_agree() {
    let all = files("0.5.0");
    let sums = sums_of(&all);
    let installer_url = "https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/bimcraft-0.5.0-windows-x64.msi";
    let net = Arc::new(
        Fake::default()
            .answer(LATEST_URL, release_of("v0.5.0", &all, Some(&sums)))
            .answer(SUMS_URL, sums)
            .answer(installer_url, all[0].1.clone()),
    );
    let dir = Scratch::new("u13");
    let mut u = updater(&net, &dir);
    u.check_at_startup();
    u.poll();
    u.update_now();
    assert!(u.poll().is_none());
    assert!(matches!(u.state(), State::Ready { .. }));
    assert_eq!(net.asked(), [LATEST_URL, SUMS_URL, installer_url]);
}

#[test]
fn an_update_that_has_no_target_has_no_file_and_a_disk_image_downloads_like_any_file() {
    let all = files("0.5.0");
    let net = Arc::new(Fake::default().answer(LATEST_URL, release_of("v0.5.0", &all, None)));
    let dir = Scratch::new("u14");
    let mut bare = updater(&net, &dir).target(None);
    bare.check_at_startup();
    bare.poll();
    assert!(matches!(bare.state(), State::Available(r) if r.installer.is_none()), "told of, with no file");
    let dmg = &all[all.len() - 2];
    let dmg_url = format!("https://github.com/IPedrax/BIMCraft/releases/download/v0.5.0/{}", dmg.0);
    let net = Arc::new(Fake::default().answer(LATEST_URL, release_of("v0.5.0", &all, None)).answer(&dmg_url, dmg.1.clone()));
    let mut mac = updater(&net, &dir).target(Some(Target::MacApp));
    mac.check_at_startup();
    mac.poll();
    assert!(matches!(mac.state(), State::Available(r) if r.installer.as_ref().is_some_and(|i| i.name.ends_with("macos-universal.dmg"))));
    mac.update_now();
    assert!(mac.poll().is_none() && matches!(mac.state(), State::Ready { .. }), "the disk image downloads and checks as any file");
}

// ---------- running programs, with a limit ----------

#[test]
fn a_new_build_has_to_say_its_name_and_exactly_the_version() {
    assert_eq!(version_line("0.5.0"), "bimcraft 0.5.0", "what `bimcraft --version` prints: the desktop makes it with this");
    assert!(crate::exec::says_version("bimcraft 0.5.0\n", "0.5.0").is_ok());
    for said in ["", "bimcraft 0.4.9", "bimcraft 10.5.0", "bimcraft 0.5.01", "other 0.5.0", "bimcraft 0.5.0 and more", "0.5.0"] {
        let e = crate::exec::says_version(said, "0.5.0").unwrap_err();
        assert!(e.contains(said.trim()) && e.contains("0.5.0"), "{said:?}: {e}");
    }
    let long = "x".repeat(500);
    assert!(crate::exec::says_version(&long, "0.5.0").unwrap_err().len() < 200, "what it said is cut short");
}

#[cfg(unix)]
mod exec {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use crate::exec::run;

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    #[test]
    fn a_program_that_succeeds_gives_what_it_printed() {
        assert_eq!(run(&mut sh("echo hello; echo problem >&2"), Duration::from_secs(10)).unwrap(), "hello\n");
    }

    #[test]
    fn a_program_that_fails_says_how_and_what_it_said() {
        let e = run(&mut sh("echo no such luck >&2; exit 3"), Duration::from_secs(10)).unwrap_err();
        assert!(e.contains("no such luck") && e.contains('3') && e.starts_with("sh "), "{e}");
        let e = run(&mut Command::new("/no/such/program"), Duration::from_secs(10)).unwrap_err();
        assert!(e.contains("/no/such/program"), "{e}");
    }

    #[test]
    fn a_program_written_a_moment_ago_is_waited_for_not_failed() {
        let _one = super::unix::serial();
        let dir = crate::testing::Scratch::new("busy");
        let path = super::unix::script(&dir.0, "fresh", "echo started");
        // Open for writing, as a program is that another process forked while it was being written: running it says "busy".
        let held = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            drop(held);
        });
        assert_eq!(run(&mut Command::new(&path), Duration::from_secs(10)).unwrap(), "started\n");
        release.join().unwrap();
    }

    #[test]
    fn a_program_that_reads_its_input_finds_none() {
        assert_eq!(run(&mut sh("cat; echo done"), Duration::from_secs(10)).unwrap(), "done\n");
    }

    #[test]
    fn a_program_that_prints_a_flood_is_not_held_up_and_only_the_start_is_kept() {
        let said = run(&mut sh("head -c 1000000 /dev/zero | tr '\\0' x; echo >&2 end; exit 1"), Duration::from_secs(10));
        let e = said.unwrap_err();
        assert!(e.contains("end"), "{e}");
        let ok = run(&mut sh("head -c 1000000 /dev/zero | tr '\\0' x"), Duration::from_secs(10)).unwrap();
        assert_eq!(ok.len(), 16 << 10, "the first 16 KiB of it");
    }

    #[test]
    fn a_program_that_hangs_is_killed_at_the_limit() {
        let start = Instant::now();
        let e = run(&mut sh("sleep 30"), Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("took longer than"), "{e}");
        assert!(start.elapsed() < Duration::from_secs(20), "waited {:?}", start.elapsed());
    }

    #[test]
    fn a_program_with_a_lot_to_say_neither_fills_a_pipe_nor_the_memory() {
        let said = run(&mut sh("head -c 3000000 /dev/zero | tr '\\0' x"), Duration::from_secs(20)).unwrap();
        assert!(said.len() <= 16 << 10 && said.starts_with("xxx"), "{} bytes", said.len());
        let e = run(&mut sh("head -c 3000000 /dev/zero | tr '\\0' x >&2; exit 1"), Duration::from_secs(20)).unwrap_err();
        assert!(e.len() <= 17 << 10 && e.contains("xxx"), "{} bytes", e.len());
    }

    #[test]
    fn a_program_that_leaves_a_child_holding_its_output_does_not_hold_us() {
        let start = Instant::now();
        assert_eq!(run(&mut sh("sleep 20 & echo started"), Duration::from_secs(10)).unwrap(), "started\n");
        assert!(start.elapsed() < Duration::from_secs(15), "waited {:?}", start.elapsed());
    }
}

// ---------- which install this program is ----------

// The paths are Unix's: on Windows a path that starts with a slash is not absolute, and an AppImage is for Linux.
#[cfg(unix)]
#[test]
fn an_appimage_is_known_by_what_its_runtime_sets_and_by_this_program_being_inside_it() {
    let os = |s: &str| Some(OsString::from(s));
    let exe = Path::new("/tmp/.mount_BIMCraAbc123/usr/bin/bimcraft");
    let found = detect::appimage_file(os("/home/me/Apps/BIMCraft.AppImage"), os("/tmp/.mount_BIMCraAbc123"), exe);
    assert_eq!(found, Some(PathBuf::from("/home/me/Apps/BIMCraft.AppImage")));
    let cases = [
        ("no APPIMAGE", None, os("/tmp/.mount_BIMCraAbc123")),
        ("no APPDIR", os("/home/me/b.AppImage"), None),
        ("empty APPIMAGE", os(""), os("/tmp/.mount_BIMCraAbc123")),
        ("empty APPDIR", os("/home/me/b.AppImage"), os("")),
        ("a relative APPIMAGE", os("b.AppImage"), os("/tmp/.mount_BIMCraAbc123")),
        ("a relative APPDIR", os("/home/me/b.AppImage"), os("tmp/.mount_BIMCraAbc123")),
        ("this program is somewhere else", os("/home/me/b.AppImage"), os("/tmp/.mount_other")),
        ("another mount whose name starts the same", os("/home/me/b.AppImage"), os("/tmp/.mount_BIMCra")),
        ("the file is inside the mount", os("/tmp/.mount_BIMCraAbc123/usr/bin/bimcraft"), os("/tmp/.mount_BIMCraAbc123")),
        ("no file name", os("/"), os("/tmp/.mount_BIMCraAbc123")),
    ];
    for (what, file, dir) in cases {
        assert_eq!(detect::appimage_file(file, dir, exe), None, "{what}");
    }
}

#[test]
fn a_mac_app_is_known_by_the_bundle_this_program_stands_in() {
    for (exe, bundle) in [
        ("/Applications/BIMCraft.app/Contents/MacOS/BIMCraft", "/Applications/BIMCraft.app"),
        ("/Users/me/Applications/BIMCraft.app/Contents/MacOS/bimcraft", "/Users/me/Applications/BIMCraft.app"),
        ("/Volumes/Work Disk/BIMCraft.app/Contents/MacOS/BIMCraft", "/Volumes/Work Disk/BIMCraft.app"),
    ] {
        assert_eq!(detect::mac_bundle(Path::new(exe)), Some(PathBuf::from(bundle)), "{exe}");
    }
    for exe in [
        "",
        "BIMCraft",
        "/usr/local/bin/bimcraft",
        "/Users/me/BIMCraft/target/release/bimcraft",
        "/Applications/BIMCraft.app/Contents/Resources/bimcraft",
        "/Applications/BIMCraft.app/Contents/MacOS",
        "/Applications/BIMCraft/Contents/MacOS/BIMCraft",
        "/Applications/BIMCraft.app/MacOS/BIMCraft",
        "/Applications/BIMCraft.app/Contents/Frameworks/MacOS/BIMCraft",
        "/Applications/BIMCraft.app/Other/MacOS/BIMCraft",
        "/Applications/BIMCraft.app/Contents/Other/BIMCraft",
    ] {
        assert_eq!(detect::mac_bundle(Path::new(exe)), None, "{exe:?}");
    }
}

#[test]
fn a_bundle_is_ours_when_its_identifier_says_so() {
    let plist = |id: &str| {
        format!(
            "<?xml version=\"1.0\"?>\n<plist><dict>\n  <key>CFBundleName</key>\n  <string>BIMCraft</string>\n  <key>CFBundleIdentifier</key>\n  <string>{id}</string>\n</dict></plist>"
        )
    };
    assert!(detect::names_bundle(&plist("io.github.ipedrax.bimcraft")));
    for other in
        ["io.github.ipedrax.bimcraft.project", "io.github.ipedrax.bimcraft2", "com.example.bimcraft", "", " io.github.ipedrax.bimcraft"]
    {
        assert!(!detect::names_bundle(&plist(other)), "{other:?}");
    }
    // The name of our identifier somewhere else in the file, under another key, is not the bundle's identifier.
    let elsewhere = "<plist><dict><key>CFBundleName</key><string>io.github.ipedrax.bimcraft</string><key>CFBundleIdentifier</key><string>x</string></dict></plist>";
    assert!(!detect::names_bundle(elsewhere));
    for junk in ["", "<key>CFBundleIdentifier</key>", "<key>CFBundleIdentifier</key><string>", "bplist00\u{0}\u{1}", "\u{0}"] {
        assert!(!detect::names_bundle(junk), "{junk:?}");
    }
}

#[test]
fn a_flatpak_has_no_network_so_it_is_known_to_leave_it_alone() {
    assert!(detect::in_flatpak(Some(OsString::from("io.github.ipedrax.bimcraft")), false), "by its variable");
    assert!(detect::in_flatpak(None, true), "by its marker file");
    assert!(!detect::in_flatpak(None, false));
    assert!(!detect::in_flatpak(Some(OsString::new()), false), "an empty variable is none");
}

// ---------- updating an AppImage ----------

#[cfg(unix)]
mod appimage {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::unix::{log, script, serial};
    use super::*;
    use crate::appimage;

    /// A folder with the AppImage that is running (a script that says it's the old one), a download of the new one, and a log both
    /// write to. The new one answers `--version` with `reply`.
    struct Rig {
        dir: Scratch,
        target: PathBuf,
        new: PathBuf,
        log: PathBuf,
    }

    fn rig(reply: &str, exit: i32) -> Rig {
        rig_with(reply, exit, false)
    }

    /// `needs_unpacking`: the new file only answers when asked to unpack itself, as on a system with no FUSE.
    fn rig_with(reply: &str, exit: i32, needs_unpacking: bool) -> Rig {
        let dir = Scratch::new("appimage");
        let log = dir.0.join("log");
        let target = script(&dir.0, "BIMCraft.AppImage", &format!("echo old >> {}", log.display()));
        // Without the right to run, so that the new file only runs because the update gave it that.
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let download = dir.0.join("download");
        std::fs::create_dir(&download).unwrap();
        let body = format!(
            "if [ \"$1\" = \"--version\" ]; then echo \"version check, extract-and-run=$APPIMAGE_EXTRACT_AND_RUN\" >> {0}; if [ {needs} = 1 ] && [ -z \"$APPIMAGE_EXTRACT_AND_RUN\" ]; then echo \"fuse: failed to exec fusermount\" >&2; exit 1; fi; echo \"{reply}\"; exit {exit}; fi\necho \"ran $@ [$APPIMAGE_EXTRACT_AND_RUN]\" >> {0}",
            log.display(),
            needs = i32::from(needs_unpacking),
        );
        let new = script(&download, "bimcraft-0.5.0-linux-x86_64.AppImage", &body);
        Rig { dir, target, new, log }
    }

    fn leftovers(rig: &Rig) -> Vec<String> {
        let names = std::fs::read_dir(&rig.dir.0).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned());
        names.filter(|n| n.starts_with('.')).collect()
    }

    #[test]
    fn a_new_appimage_that_runs_replaces_the_old_one_and_starts() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0);
        let new_bytes = std::fs::read(&r.new).unwrap();
        appimage::replace(&r.new, &r.target, "0.5.0", Some(Path::new("/w/house.bimcraft"))).unwrap();
        assert_eq!(std::fs::read(&r.target).unwrap(), new_bytes, "the target is the new file now");
        assert!(std::fs::metadata(&r.target).unwrap().permissions().mode() & 0o111 != 0, "and it runs");
        assert!(leftovers(&r).is_empty(), "nothing left beside it: {:?}", leftovers(&r));
        // It was tried first, as it would be run, and then started with the project.
        let lines = log(&r.log, 2);
        assert_eq!(lines, ["version check, extract-and-run=", "ran /w/house.bimcraft []"]);
        assert!(r.new.exists(), "the download is the caller's to remove");
    }

    #[test]
    fn a_new_appimage_that_cannot_mount_itself_is_tried_again_unpacking_itself() {
        let _one = serial();
        let r = rig_with("bimcraft 0.5.0", 0, true);
        let new_bytes = std::fs::read(&r.new).unwrap();
        appimage::replace(&r.new, &r.target, "0.5.0", Some(Path::new("/w/house.bimcraft"))).unwrap();
        assert_eq!(std::fs::read(&r.target).unwrap(), new_bytes, "the second try shows it runs");
        assert_eq!(
            log(&r.log, 3),
            ["version check, extract-and-run=", "version check, extract-and-run=1", "ran /w/house.bimcraft [1]"],
            "and it is started that way too, or it would fail as it did the first time"
        );
        // And one that fails both ways is not installed.
        let r = rig_with("bimcraft 0.4.9", 0, true);
        let old_bytes = std::fs::read(&r.target).unwrap();
        assert!(appimage::replace(&r.new, &r.target, "0.5.0", None).is_err());
        assert_eq!(std::fs::read(&r.target).unwrap(), old_bytes);
    }

    #[test]
    fn a_new_appimage_that_will_not_start_is_said_to_be_installed() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0);
        // It runs once, and takes its own right to run away as it does, as a file that can't be started later has lost it.
        std::fs::write(&r.new, "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo bimcraft 0.5.0; chmod a-x \"$0\"; fi\n").unwrap();
        let e = appimage::replace(&r.new, &r.target, "0.5.0", None).unwrap_err();
        assert!(e.contains("is installed") && e.contains("starting it failed"), "{e}");
        assert!(leftovers(&r).is_empty());
    }

    #[test]
    fn a_name_someone_else_planted_for_the_new_file_is_never_written_through() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0);
        let victim = r.dir.0.join("precious");
        std::fs::write(&victim, "keep me").unwrap();
        let planted = r.dir.0.join(format!(".BIMCraft.AppImage.update-{}", std::process::id()));
        std::os::unix::fs::symlink(&victim, &planted).unwrap();
        let old_bytes = std::fs::read(&r.target).unwrap();
        let e = appimage::replace(&r.new, &r.target, "0.5.0", None).unwrap_err();
        assert!(e.contains("exists"), "{e}");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me", "the link was not followed");
        assert_eq!(std::fs::read(&r.target).unwrap(), old_bytes);
    }

    #[test]
    fn a_new_appimage_that_does_not_run_or_says_another_version_leaves_the_old_one_alone() {
        let _one = serial();
        for (what, reply, exit) in [("another version", "bimcraft 0.4.9", 0), ("a failure", "bimcraft 0.5.0", 1), ("nothing", "", 0)] {
            let r = rig(reply, exit);
            let old_bytes = std::fs::read(&r.target).unwrap();
            let e = appimage::replace(&r.new, &r.target, "0.5.0", None).unwrap_err();
            assert!(!e.is_empty(), "{what}");
            assert_eq!(std::fs::read(&r.target).unwrap(), old_bytes, "{what}: the old one is as it was");
            assert!(leftovers(&r).is_empty(), "{what}: {:?}", leftovers(&r));
            std::thread::sleep(Duration::from_millis(100));
            assert!(!std::fs::read_to_string(&r.log).unwrap_or_default().contains("ran"), "{what}: nothing was started");
        }
    }

    #[test]
    fn a_new_appimage_that_hangs_is_given_up_on() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0);
        std::fs::write(&r.new, "#!/bin/sh\nsleep 30\n").unwrap();
        let old_bytes = std::fs::read(&r.target).unwrap();
        let start = std::time::Instant::now();
        let e = appimage::replace_within(&r.new, &r.target, "0.5.0", None, Duration::from_millis(400)).unwrap_err();
        assert!(e.contains("took longer"), "{e}");
        assert!(start.elapsed() < Duration::from_secs(10));
        assert_eq!(std::fs::read(&r.target).unwrap(), old_bytes);
        assert!(leftovers(&r).is_empty());
    }

    #[test]
    fn an_appimage_that_cannot_be_written_beside_or_a_download_that_is_gone_is_an_error() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0);
        assert!(appimage::replace(&r.new, Path::new("/no/such/folder/BIMCraft.AppImage"), "0.5.0", None).is_err());
        assert!(appimage::replace(&r.dir.0.join("missing"), &r.target, "0.5.0", None).is_err());
        assert!(leftovers(&r).is_empty());
    }

    #[test]
    fn the_new_appimage_starts_with_the_project_and_with_none_of_the_old_ones_environment() {
        let with = appimage::relaunch(Path::new("/home/me/BIMCraft.AppImage"), Some(Path::new("/w/house.bimcraft")), false);
        assert_eq!(with.get_program(), "/home/me/BIMCraft.AppImage");
        assert_eq!(with.get_args().collect::<Vec<_>>(), ["/w/house.bimcraft"]);
        let removed: Vec<_> =
            with.get_envs().filter(|(_, value)| value.is_none()).map(|(key, _)| key.to_string_lossy().into_owned()).collect();
        for name in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
            assert!(removed.contains(&name.to_string()), "{name} should be removed: {removed:?}");
        }
        assert_eq!(appimage::relaunch(Path::new("/x"), None, false).get_args().count(), 0);
        let envs = |unpack: bool| -> Vec<(String, Option<String>)> {
            let command = appimage::relaunch(Path::new("/x"), None, unpack);
            let all = command.get_envs();
            all.filter(|(key, _)| key.to_string_lossy() == "APPIMAGE_EXTRACT_AND_RUN")
                .map(|(key, value)| (key.to_string_lossy().into_owned(), value.map(|v| v.to_string_lossy().into_owned())))
                .collect()
        };
        assert_eq!(envs(true), [("APPIMAGE_EXTRACT_AND_RUN".to_string(), Some("1".to_string()))]);
        assert!(envs(false).is_empty(), "left as the environment has it");
    }
}

// ---------- updating the macOS app ----------

#[test]
fn a_plists_strings_are_read_by_their_keys() {
    let plist = "<dict><key>CFBundleExecutable</key>\n  <string>BIMCraft</string><key>CFBundleName</key><string>Name</string></dict>";
    assert_eq!(detect::plist_string(plist, "CFBundleExecutable"), Some("BIMCraft"));
    assert_eq!(detect::plist_string(plist, "CFBundleName"), Some("Name"));
    for (plist, key) in [
        (plist, "CFBundleVersion"),
        ("", "CFBundleName"),
        ("<key>CFBundleName</key>", "CFBundleName"),
        ("<key>CFBundleName</key><string>x", "CFBundleName"),
    ] {
        assert_eq!(detect::plist_string(plist, key), None, "{plist:?}");
    }
}

#[cfg(unix)]
mod macos {
    use std::time::Duration;

    use super::unix::{log, script, serial};
    use super::*;
    use crate::macos::{self, Tools};

    const PLIST: &str = "<plist><dict><key>CFBundleExecutable</key><string>BIMCraft</string>\n<key>CFBundleIdentifier</key><string>io.github.ipedrax.bimcraft</string></dict></plist>";

    /// A disk image's contents (what `hdiutil attach` is made to mount) holding a BIMCraft.app that says it is `version`, and an installed
    /// BIMCraft.app that is the old one, in a folder of their own, with stand-ins for the macOS programs that write down how they were run.
    struct Rig {
        dir: Scratch,
        installed: PathBuf,
        dmg: PathBuf,
        log: PathBuf,
        tools: Tools,
    }

    /// `says`: what the new app prints for --version. The tools exit with the codes given (attach, ditto, codesign).
    fn rig(says: &str, attach: i32, ditto: i32, codesign: i32) -> Rig {
        let dir = Scratch::new("mac");
        let log = dir.0.join("log");
        let app = |root: &Path, marker: &str, version_script: &str| {
            let contents = root.join("BIMCraft.app").join("Contents");
            std::fs::create_dir_all(contents.join("MacOS")).unwrap();
            std::fs::create_dir_all(contents.join("Resources")).unwrap();
            std::fs::write(contents.join("Info.plist"), PLIST).unwrap();
            std::fs::write(contents.join("Resources").join(marker), "").unwrap();
            script(&contents.join("MacOS"), "BIMCraft", version_script);
        };
        let image = dir.0.join("image");
        app(&image, "new", &format!("if [ \"$1\" = \"--version\" ]; then echo \"{says}\"; fi"));
        let apps = dir.0.join("Applications");
        app(&apps, "old", "echo old >> /dev/null");
        let bin = dir.0.join("bin");
        std::fs::create_dir(&bin).unwrap();
        let hdiutil = script(
            &bin,
            "hdiutil",
            &format!(
                "echo \"hdiutil $@\" >> {0}\ncase \"$1\" in\n attach) if [ {1} -ne 0 ]; then exit {1}; fi; for a in \"$@\"; do if [ \"$p\" = mp ]; then mp=\"$a\"; fi; if [ \"$a\" = -mountpoint ]; then p=mp; else p=; fi; done; cp -R {2}/. \"$mp\" ;;\n detach) for a in \"$@\"; do mp=\"$a\"; done; rm -rf \"$mp\"/* ;;\nesac",
                log.display(),
                attach,
                image.display()
            ),
        );
        let ditto = script(
            &bin,
            "ditto",
            &format!("echo \"ditto $@\" >> {}\nif [ {} -ne 0 ]; then exit {}; fi\ncp -R \"$1\"/. \"$2\"", log.display(), ditto, ditto),
        );
        let codesign = script(&bin, "codesign", &format!("echo \"codesign $@\" >> {}\nexit {}", log.display(), codesign));
        let open = script(&bin, "open", &format!("echo \"open $@\" >> {}", log.display()));
        let dmg = dir.0.join("bimcraft-0.5.0-macos-universal.dmg");
        std::fs::write(&dmg, "a disk image").unwrap();
        Rig { installed: apps.join("BIMCraft.app"), dmg, log, tools: Tools { hdiutil, ditto, codesign, open }, dir }
    }

    fn marker(rig: &Rig, name: &str) -> bool {
        rig.installed.join("Contents").join("Resources").join(name).exists()
    }

    fn leftovers(rig: &Rig) -> Vec<String> {
        let folder = rig.installed.parent().unwrap();
        std::fs::read_dir(folder)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "BIMCraft.app")
            .collect()
    }

    fn work(rig: &Rig) -> PathBuf {
        let work = rig.dir.0.join("work");
        std::fs::create_dir_all(&work).unwrap();
        work
    }

    fn replace(rig: &Rig, project: Option<&str>) -> Result<(), String> {
        let work = work(rig);
        macos::replace_with(&rig.dmg, &rig.installed, &work, "0.5.0", project.map(Path::new), &rig.tools, Duration::from_secs(30))
    }

    #[test]
    fn a_new_app_that_verifies_and_runs_replaces_the_installed_one_and_starts() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        replace(&r, Some("/w/house.bimcraft")).unwrap();
        assert!(marker(&r, "new") && !marker(&r, "old"), "the installed app is the new one");
        assert!(leftovers(&r).is_empty(), "no copy, no old bundle left beside it: {:?}", leftovers(&r));
        let mount = r.dir.0.join("work").join("mount");
        let staged = r.installed.with_file_name(format!(".BIMCraft-update-{}.app", std::process::id()));
        let lines = log(&r.log, 5);
        assert_eq!(
            lines,
            [
                format!("hdiutil attach -nobrowse -readonly -noverify -noautoopen -mountpoint {} {}", mount.display(), r.dmg.display()),
                format!("ditto {}/BIMCraft.app {}", mount.display(), staged.display()),
                format!("hdiutil detach -force {}", mount.display()),
                format!("codesign --verify --strict --deep {}", staged.display()),
                format!("open -n {} --args /w/house.bimcraft", r.installed.display()),
            ]
        );
    }

    #[test]
    fn it_starts_without_a_project_too() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        replace(&r, None).unwrap();
        assert_eq!(log(&r.log, 5)[4], format!("open -n {}", r.installed.display()));
    }

    #[test]
    fn every_way_the_new_app_can_fail_leaves_the_installed_one_and_no_mount_or_copy() {
        let _one = serial();
        // (what, tool exit codes, what the new app prints, whether anything was mounted to detach)
        let cases = [
            ("the image won't mount", (7, 0, 0), "bimcraft 0.5.0", false),
            ("the copy fails", (0, 7, 0), "bimcraft 0.5.0", true),
            ("the signature doesn't verify", (0, 0, 7), "bimcraft 0.5.0", true),
            ("the app says another version", (0, 0, 0), "bimcraft 0.4.9", true),
            ("the app says nothing", (0, 0, 0), "", true),
        ];
        for (what, (attach, ditto, codesign), says, mounted) in cases {
            let r = rig(says, attach, ditto, codesign);
            let e = replace(&r, Some("/w/house.bimcraft")).unwrap_err();
            assert!(!e.is_empty(), "{what}");
            assert!(marker(&r, "old") && !marker(&r, "new"), "{what}: the installed app is as it was");
            assert!(leftovers(&r).is_empty(), "{what}: {:?}", leftovers(&r));
            let log = std::fs::read_to_string(&r.log).unwrap_or_default();
            assert_eq!(log.contains("hdiutil detach"), mounted, "{what}: the image is let go exactly when it was mounted\n{log}");
            assert!(!log.contains("open -n"), "{what}: nothing was started");
        }
    }

    #[test]
    fn an_image_with_no_app_in_it_is_an_error_and_is_let_go() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        std::fs::remove_dir_all(r.dir.0.join("image").join("BIMCraft.app")).unwrap();
        let e = replace(&r, None).unwrap_err();
        assert!(e.contains("the disk image holds no BIMCraft.app"), "{e}");
        assert!(std::fs::read_to_string(&r.log).unwrap().contains("hdiutil detach"));
        assert!(marker(&r, "old"));
    }

    #[test]
    fn a_tool_that_hangs_is_given_up_on_and_the_app_is_left_alone() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        script(r.tools.hdiutil.parent().unwrap(), "hdiutil", "sleep 30");
        let work = work(&r);
        let start = std::time::Instant::now();
        let e = macos::replace_with(&r.dmg, &r.installed, &work, "0.5.0", None, &r.tools, Duration::from_millis(400)).unwrap_err();
        assert!(e.contains("took longer"), "{e}");
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(marker(&r, "old") && leftovers(&r).is_empty());
    }

    #[test]
    fn an_app_that_cannot_be_swapped_in_is_an_error_and_nothing_is_started() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        std::fs::remove_dir_all(&r.installed).unwrap();
        let e = replace(&r, Some("/w/house.bimcraft")).unwrap_err();
        assert!(e.contains("BIMCraft.app"), "{e}");
        assert!(leftovers(&r).is_empty(), "the copy that was made is gone: {:?}", leftovers(&r));
        assert!(!std::fs::read_to_string(&r.log).unwrap().contains("open -n"), "nothing was started");
    }

    #[test]
    fn a_folder_someone_else_planted_for_the_new_app_is_never_copied_into() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        let victim = r.dir.0.join("precious");
        std::fs::create_dir(&victim).unwrap();
        let planted = r.installed.parent().unwrap().join(format!(".BIMCraft-update-{}.app", std::process::id()));
        std::os::unix::fs::symlink(&victim, &planted).unwrap();
        let e = replace(&r, None).unwrap_err();
        assert!(e.contains("exists"), "{e}");
        assert_eq!(std::fs::read_dir(&victim).unwrap().count(), 0, "the link was not followed");
        assert!(marker(&r, "old"));
        assert!(std::fs::read_to_string(&r.log).unwrap().contains("hdiutil detach"), "the image is let go all the same");
    }

    #[test]
    fn a_second_try_after_a_failed_one_goes_through() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 7);
        assert!(replace(&r, None).is_err());
        assert!(marker(&r, "old") && leftovers(&r).is_empty(), "nothing of the first try is left in the way");
        script(r.tools.codesign.parent().unwrap(), "codesign", "exit 0");
        replace(&r, None).unwrap();
        assert!(marker(&r, "new") && !marker(&r, "old"));
    }

    #[test]
    fn a_program_name_that_is_a_path_is_refused_and_nothing_is_run() {
        let _one = serial();
        for name in ["../MacOS/BIMCraft", "/bin/sh", "..", "", "a/b"] {
            let r = rig("bimcraft 0.5.0", 0, 0, 0);
            let plist = r.dir.0.join("image").join("BIMCraft.app").join("Contents").join("Info.plist");
            std::fs::write(&plist, PLIST.replace("<string>BIMCraft</string>", &format!("<string>{name}</string>"))).unwrap();
            let e = replace(&r, None).unwrap_err();
            assert!(e.contains("names no program"), "{name:?}: {e}");
            assert!(marker(&r, "old") && leftovers(&r).is_empty(), "{name:?}");
        }
    }

    #[test]
    fn an_app_that_is_in_but_will_not_start_is_said_to_be_installed() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        script(r.tools.open.parent().unwrap(), "open", "exit 7");
        let e = replace(&r, None).unwrap_err();
        assert!(e.contains("installed") && e.contains("starting it failed"), "{e}");
        assert!(marker(&r, "new") && !marker(&r, "old"), "the new app is where the old one was: it is the start that failed");
    }

    #[test]
    fn the_swap_puts_the_old_app_back_when_the_new_one_cannot_go_in() {
        let _one = serial();
        let r = rig("bimcraft 0.5.0", 0, 0, 0);
        let (staged, old) = (r.dir.0.join("Applications").join(".gone.app"), r.dir.0.join("Applications").join(".old.app"));
        assert!(macos::swap(&r.installed, &staged, &old).is_err(), "there is no new app to put in");
        assert!(marker(&r, "old"), "the installed app is back where it was");
        assert!(!old.exists());
        // And when there is one, the old one goes.
        let real = r.dir.0.join("Applications").join(".new.app");
        std::fs::create_dir_all(real.join("Contents").join("Resources")).unwrap();
        std::fs::write(real.join("Contents").join("Resources").join("new"), "").unwrap();
        macos::swap(&r.installed, &real, &old).unwrap();
        assert!(marker(&r, "new") && !marker(&r, "old") && !old.exists() && !real.exists());
    }

    #[test]
    fn the_programs_are_run_by_their_full_path() {
        let tools = Tools::system();
        assert_eq!(
            [tools.hdiutil, tools.ditto, tools.codesign, tools.open],
            ["/usr/bin/hdiutil", "/usr/bin/ditto", "/usr/bin/codesign", "/usr/bin/open"].map(PathBuf::from)
        );
    }
}

// ---------- which kind of install this is, and replacing it ----------

#[test]
fn the_desktop_phones_home_unless_switched_off_or_sandboxed_with_no_network() {
    use crate::updater::phones_home;
    assert!(phones_home(None, false));
    assert!(phones_home(Some(OsString::new()), false), "an empty variable is none");
    for set in ["1", "yes", "0", "false"] {
        assert!(!phones_home(Some(set.into()), false), "{set}");
    }
    assert!(!phones_home(None, true), "a Flatpak can't reach the network");
}

#[test]
fn only_a_folder_an_update_made_is_removed_and_only_after_an_install_that_went_through() {
    let update = Path::new("/tmp/bimcraft-update-1f-2a-0");
    for (target, installed, folder, removed) in [
        (Target::AppImage(Arch::X86_64), true, update, true),
        (Target::AppImage(Arch::Aarch64), true, update, true),
        (Target::MacApp, true, update, true),
        (Target::AppImage(Arch::X86_64), false, update, false),
        (Target::MacApp, false, update, false),
        (Target::Windows, true, update, false),
        (Target::Windows, false, update, false),
        (Target::AppImage(Arch::X86_64), true, Path::new("/home/me/Downloads"), false),
        (Target::MacApp, true, Path::new("/tmp/not-a-bimcraft-update-1"), false),
        (Target::MacApp, true, Path::new("/"), false),
        (Target::MacApp, true, Path::new(""), false),
    ] {
        assert_eq!(detect::spent(target, installed, folder), removed, "{target:?} {installed} {folder:?}");
    }
}

#[test]
fn a_download_that_is_not_the_one_that_was_checked_is_not_installed_on_any_system() {
    let dir = Scratch::new("inst11");
    let file = dir.0.join("bimcraft-update-1").join("bimcraft-0.5.0-windows-x64.msi");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "what came down").unwrap();
    let job = |digest: [u8; 32]| Handover { version: v("0.5.0"), file: file.clone(), sha256: digest, project: None };
    for target in [Target::Windows, Target::AppImage(Arch::X86_64), Target::MacApp] {
        let install = Install { target, place: dir.0.join("place") };
        let e = install.hand_over(&job(sha(b"what was checked"))).unwrap_err();
        assert!(e.contains("not the one that was checked"), "{target:?}: {e}");
    }
    let gone = Install { target: Target::Windows, place: dir.0.join("place") };
    let nothing = Handover { file: dir.0.join("missing"), ..job(sha(b"what came down")) };
    assert!(gone.hand_over(&nothing).unwrap_err().contains("not the one that was checked"), "a file that is gone is not the one either");
    assert!(file.exists(), "nothing was touched");
}

#[test]
fn the_test_program_is_no_kind_of_install() {
    assert_eq!(Install::detect(), None, "a build run by cargo test is not an installed BIMCraft, whatever its environment");
}

#[cfg(unix)]
mod installs {
    use std::os::unix::fs::PermissionsExt;

    use super::unix::{log, script, serial};
    use super::*;

    const PLIST: &str = "<plist><dict><key>CFBundleIdentifier</key><string>io.github.ipedrax.bimcraft</string></dict></plist>";

    /// Takes the right to write away from `folder`: whether that stops this process, which it doesn't when it's root's.
    fn shut(folder: &Path) -> bool {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        !install::writable(folder)
    }

    fn open(folder: &Path) {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// What the AppImage runtime sets up: the file, and the program inside a mount of its own.
    struct Image {
        dir: Scratch,
        file: PathBuf,
        mount: PathBuf,
        exe: PathBuf,
    }

    fn image(name: &str) -> Image {
        let dir = Scratch::new(name);
        let file = dir.0.join("BIMCraft.AppImage");
        std::fs::write(&file, "an appimage").unwrap();
        let mount = dir.0.join("mnt");
        std::fs::create_dir_all(mount.join("usr").join("bin")).unwrap();
        let exe = mount.join("usr").join("bin").join("bimcraft");
        Image { dir, file, mount, exe }
    }

    fn found(image: &Image, file: &Path, arch: &str) -> Option<Install> {
        detect::appimage_install(Some(file.into()), Some(image.mount.clone().into()), &image.exe, arch)
    }

    #[test]
    fn an_appimage_that_can_be_replaced_is_found_by_its_file_and_the_processor() {
        let i = image("inst1");
        let place = std::fs::canonicalize(&i.file).unwrap();
        assert_eq!(found(&i, &i.file, "x86_64"), Some(Install { target: Target::AppImage(Arch::X86_64), place }));
        assert_eq!(found(&i, &i.file, "aarch64").map(|f| f.target), Some(Target::AppImage(Arch::Aarch64)));
        assert_eq!(found(&i, &i.file, "riscv64"), None, "there is no AppImage for it");
    }

    #[test]
    fn a_link_to_the_appimage_is_replaced_in_the_file_it_points_to() {
        let i = image("inst2");
        let link = i.dir.0.join("link.AppImage");
        std::os::unix::fs::symlink(&i.file, &link).unwrap();
        assert_eq!(found(&i, &link, "x86_64").map(|f| f.place), Some(std::fs::canonicalize(&i.file).unwrap()));
    }

    #[test]
    fn an_appimage_that_is_gone_or_no_file_is_left_alone() {
        let i = image("inst3");
        assert_eq!(found(&i, &i.dir.0.join("gone.AppImage"), "x86_64"), None);
        let folder = i.dir.0.join("elsewhere");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(found(&i, &folder, "x86_64"), None, "a folder is no AppImage");
    }

    #[test]
    fn an_install_can_be_replaced_where_its_folder_can_be_written_to() {
        let i = image("inst3b");
        let appimage = found(&i, &i.file, "x86_64").unwrap();
        assert!(appimage.replaceable());
        assert!(Install { target: Target::Windows, place: PathBuf::from("/no/such/folder/bimcraft.exe") }.replaceable(), "the installer's");
        assert!(!Install { target: Target::MacApp, place: PathBuf::new() }.replaceable(), "no folder at all");
        assert!(
            !Install { target: Target::MacApp, place: i.dir.0.join("gone").join("BIMCraft.app") }.replaceable(),
            "a folder that isn't there"
        );
        let shut_in = i.dir.0.join("shut");
        std::fs::create_dir(&shut_in).unwrap();
        let there = Install { target: Target::MacApp, place: shut_in.join("BIMCraft.app") };
        if shut(&shut_in) {
            assert!(!there.replaceable(), "a read-only folder");
        }
        open(&shut_in);
        assert!(there.replaceable(), "and once it is not");
        assert_eq!(std::fs::read_dir(&shut_in).unwrap().count(), 0, "the probe leaves nothing behind");
    }

    #[test]
    fn where_an_update_cannot_be_written_it_is_told_of_and_has_no_file() {
        let dir = Scratch::new("inst3c");
        let net = Arc::new(Fake::default().answer(LATEST_URL, release_of("v0.5.0", &files("0.5.0"), None)));
        let shut_in = dir.0.join("shut");
        std::fs::create_dir(&shut_in).unwrap();
        let place = Install { target: Target::AppImage(Arch::X86_64), place: shut_in.join("BIMCraft.AppImage") };
        let offered = |net: &Arc<Fake>, place: &Install| {
            let mut u = Updater::new(net.clone(), v("0.4.0"), None, || {}).inline().in_folder(dir.0.clone()).found(place.clone());
            u.check_at_startup();
            u.poll();
            match u.state() {
                State::Available(release) => release.installer.is_some(),
                other => panic!("not offered: {other:?}"),
            }
        };
        if shut(&shut_in) {
            assert!(!offered(&net, &place), "told of, with no file to install");
        }
        open(&shut_in);
        assert!(offered(&net, &place), "and with its file once the folder can be written to");
        assert_eq!(std::fs::read_dir(&shut_in).unwrap().count(), 0);
    }

    fn bundle(dir: &Scratch, plist: Option<&str>) -> PathBuf {
        let app = dir.0.join("Applications").join("BIMCraft.app");
        std::fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
        if let Some(plist) = plist {
            std::fs::write(app.join("Contents").join("Info.plist"), plist).unwrap();
        }
        app.join("Contents").join("MacOS").join("BIMCraft")
    }

    #[test]
    fn a_bundle_that_can_be_replaced_is_found() {
        let dir = Scratch::new("inst4");
        let exe = bundle(&dir, Some(PLIST));
        let app = std::fs::canonicalize(dir.0.join("Applications").join("BIMCraft.app")).unwrap();
        assert_eq!(detect::mac_install(&exe), Some(Install { target: Target::MacApp, place: app }));
    }

    #[test]
    fn a_bundle_reached_through_a_link_is_replaced_where_it_really_is() {
        let dir = Scratch::new("inst4b");
        bundle(&dir, Some(PLIST));
        let link = dir.0.join("link");
        std::os::unix::fs::symlink(dir.0.join("Applications"), &link).unwrap();
        let exe = link.join("BIMCraft.app").join("Contents").join("MacOS").join("BIMCraft");
        let place = detect::mac_install(&exe).unwrap().place;
        assert_eq!(place, std::fs::canonicalize(dir.0.join("Applications").join("BIMCraft.app")).unwrap());
    }

    #[test]
    fn a_bundle_that_is_not_ours_is_left_alone() {
        let dir = Scratch::new("inst5");
        let exe = bundle(&dir, None);
        assert_eq!(detect::mac_install(&exe), None, "no Info.plist");
        std::fs::write(exe.parent().unwrap().parent().unwrap().join("Info.plist"), PLIST.replace("ipedrax", "someone")).unwrap();
        assert_eq!(detect::mac_install(&exe), None, "another app's identifier");
        assert_eq!(detect::mac_install(&dir.0.join("bimcraft")), None, "not in an app at all");
        std::fs::write(exe.parent().unwrap().parent().unwrap().join("Info.plist"), PLIST).unwrap();
        assert!(detect::mac_install(&exe).is_some());
    }

    /// A folder an update made, holding the download.
    fn download(dir: &Scratch, name: &str, body: &str) -> (PathBuf, PathBuf) {
        let folder = dir.0.join("bimcraft-update-1");
        std::fs::create_dir_all(&folder).unwrap();
        (folder.clone(), script(&folder, name, body))
    }

    fn job(file: &Path, project: Option<&str>) -> Handover {
        let sha256 = sha(&std::fs::read(file).unwrap_or_default());
        Handover { version: v("0.5.0"), file: file.into(), sha256, project: project.map(PathBuf::from) }
    }

    #[test]
    fn handing_an_appimage_over_replaces_it_starts_it_and_clears_the_download_away() {
        let _one = serial();
        let dir = Scratch::new("inst6");
        let log_file = dir.0.join("log");
        let target = script(&dir.0, "BIMCraft.AppImage", "echo old");
        let body =
            format!("if [ \"$1\" = \"--version\" ]; then echo bimcraft 0.5.0; exit 0; fi\necho \"ran $@\" >> {}", log_file.display());
        let (folder, new) = download(&dir, "bimcraft-0.5.0-linux-x86_64.AppImage", &body);
        let new_bytes = std::fs::read(&new).unwrap();
        let install = Install { target: Target::AppImage(Arch::X86_64), place: target.clone() };
        install.hand_over(&job(&new, Some("/w/house.bimcraft"))).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), new_bytes);
        assert_eq!(log(&log_file, 1), ["ran /w/house.bimcraft"]);
        assert!(!folder.exists(), "the download's folder is gone");
    }

    #[test]
    fn a_download_that_is_not_the_one_that_was_checked_is_not_installed() {
        let _one = serial();
        let dir = Scratch::new("inst10");
        let target = script(&dir.0, "BIMCraft.AppImage", "echo old");
        let old_bytes = std::fs::read(&target).unwrap();
        let (folder, new) = download(&dir, "bimcraft-0.5.0-linux-x86_64.AppImage", "echo bimcraft 0.5.0");
        let changed = Handover { sha256: sha(b"the file that was checked"), ..job(&new, None) };
        let appimage = Install { target: Target::AppImage(Arch::X86_64), place: target.clone() };
        let e = appimage.hand_over(&changed).unwrap_err();
        assert!(e.contains("not the one that was checked"), "{e}");
        assert_eq!(std::fs::read(&target).unwrap(), old_bytes, "the install is as it was");
        // The disk image is turned down before anything is mounted.
        let mac = Install { target: Target::MacApp, place: dir.0.join("BIMCraft.app") };
        assert!(mac.hand_over(&changed).unwrap_err().contains("not the one that was checked"));
        assert!(new.exists() && folder.exists(), "and the download is kept");
    }

    #[test]
    fn a_download_in_any_other_folder_is_never_removed() {
        let _one = serial();
        let dir = Scratch::new("inst9");
        let target = script(&dir.0, "BIMCraft.AppImage", "echo old");
        let folder = dir.0.join("Downloads");
        std::fs::create_dir(&folder).unwrap();
        let new = script(&folder, "bimcraft-0.5.0-linux-x86_64.AppImage", "echo bimcraft 0.5.0");
        let install = Install { target: Target::AppImage(Arch::X86_64), place: target };
        install.hand_over(&job(&new, None)).unwrap();
        assert!(folder.exists() && new.exists(), "it is somebody's folder, whatever was in it");
    }

    #[test]
    fn a_hand_over_that_fails_leaves_the_install_and_the_download_as_they_were() {
        let _one = serial();
        let dir = Scratch::new("inst7");
        let target = script(&dir.0, "BIMCraft.AppImage", "echo old");
        let old_bytes = std::fs::read(&target).unwrap();
        let (folder, new) = download(&dir, "bimcraft-0.5.0-linux-x86_64.AppImage", "echo bimcraft 0.4.9");
        let install = Install { target: Target::AppImage(Arch::X86_64), place: target.clone() };
        assert!(install.hand_over(&job(&new, None)).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), old_bytes);
        assert!(new.exists(), "kept, so that it can be tried again");
        assert!(folder.exists());
    }

    // Not on a Mac, where this would hand a file that is no disk image to the real hdiutil, and what that does can't be known from
    // here, while a release job runs the tests there. Elsewhere there is no hdiutil, which is an error too.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_disk_image_that_hdiutil_cannot_open_leaves_the_app_and_the_download_alone() {
        let dir = Scratch::new("inst8");
        let bundle = dir.0.join("Applications").join("BIMCraft.app");
        std::fs::create_dir_all(bundle.join("Contents")).unwrap();
        let (folder, dmg) = download(&dir, "bimcraft-0.5.0-macos-universal.dmg", "not a disk image");
        let install = Install { target: Target::MacApp, place: bundle.clone() };
        let e = install.hand_over(&job(&dmg, None)).unwrap_err();
        assert!(e.contains("hdiutil"), "{e}");
        assert!(bundle.join("Contents").is_dir() && dmg.exists() && folder.exists());
        assert!(folder.join("mount").is_dir(), "the image was to be mounted in the download's folder");
        assert!(!bundle.join("mount").exists(), "and not inside the app");
        let beside: Vec<_> = std::fs::read_dir(bundle.parent().unwrap()).unwrap().flatten().collect();
        assert_eq!(beside.len(), 1, "nothing else beside the app");
    }
}
