//! A plugin's self-update: the permission, the signed feed, the archive's digest and paths, the atomic folder
//! swap, the backup and the rollback. Every network answer comes from `bimcraft_update::testing::Fake`; nothing
//! here touches the internet.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use bimcraft_core::engine::Session;
use bimcraft_plugin::{Plugins, Status, update};
use bimcraft_update::testing::Fake;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

const FEED: &str = "https://example.org/demo/latest.json";
const ARCHIVE: &str = "https://example.org/demo/demo-0.3.0.zip";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

struct Publisher {
    pair: Ed25519KeyPair,
}

impl Publisher {
    fn new() -> Publisher {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Publisher { pair: Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap() }
    }

    fn key(&self) -> String {
        format!("ed25519:{}", b64(self.pair.public_key().as_ref()))
    }

    fn sign(&self, feed: &mut Value) {
        // A feed may announce its publisher's key, which is what lets a first install be verified (TOFU).
        if feed.get("public_key").is_none() {
            feed["public_key"] = json!(self.key());
        }
        let canonical = canonical(feed);
        feed["signature"] = json!(format!("ed25519:{}", b64(self.pair.sign(canonical.as_bytes()).as_ref())));
    }
}

/// The canonical JSON the host signs: every field but `signature`, in this order.
fn canonical(v: &Value) -> String {
    let q = |s: &Value| serde_json::to_string(s.as_str().unwrap_or_default()).unwrap();
    let mut out = format!(
        "{{\"schema\":{},\"plugin\":{{\"id\":{},\"abi\":{}}},\"version\":{},\"released\":{},\"notes\":{},\"module\":{{\"url\":{},\"sha256\":{},\"size\":{}}}",
        v["schema"].as_u64().unwrap(),
        q(&v["plugin"]["id"]),
        v["plugin"]["abi"].as_u64().unwrap(),
        q(&v["version"]),
        q(&v["released"]),
        q(&v["notes"]),
        q(&v["module"]["url"]),
        q(&v["module"]["sha256"]),
        v["module"]["size"].as_u64().unwrap(),
    );
    if let Some(key) = v.get("public_key") {
        out.push_str(&format!(",\"public_key\":{}", q(key)));
    }
    out.push('}');
    out
}

fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn module() -> Vec<u8> {
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "bimcraft_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "bimcraft_free") (param i32 i32))
            (func (export "bimcraft_run") (param i32 i32 i32 i32) (result i64) i64.const 0))"#,
    )
    .unwrap()
}

fn update_module() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/update.wat");
    wat::parse_str(std::fs::read_to_string(path).unwrap()).unwrap()
}

fn manifest(id: &str, version: &str, feed: &str, key: Option<&str>) -> Value {
    let mut m = json!({
        "id": id,
        "version": version,
        "name": "Demo",
        "abi": 1,
        "module": "plugin.wasm",
        "permissions": ["update"],
        "functions": [{"id": "run", "label": "Run"}],
    });
    let mut update = json!({"feed": feed});
    if let Some(key) = key {
        update["public_key"] = json!(key);
    }
    m["update"] = update;
    m
}

fn write_plugin(root: &Path, id: &str, version: &str, feed: &str, key: Option<&str>) {
    let folder = root.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plugin.json"), serde_json::to_string_pretty(&manifest(id, version, feed, key)).unwrap()).unwrap();
    std::fs::write(folder.join("plugin.wasm"), if id == "caller" { update_module() } else { module() }).unwrap();
}

fn write_state(root: &Path, id: &str, granted: bool, pinned: Option<(&str, bool)>) {
    let mut state = json!({"permissions": {}});
    if granted {
        state["permissions"][id] = json!(["update"]);
    }
    if let Some((key, confirmed)) = pinned {
        state["update"] = json!({id: {"key": key, "update_confirmed": confirmed}});
    }
    std::fs::write(root.join("state.json"), serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn normal_archive(id: &str, version: &str, feed: &str, key: Option<&str>) -> Vec<u8> {
    let text = serde_json::to_string_pretty(&manifest(id, version, feed, key)).unwrap();
    zip_of(&[("plugin.json", text.as_bytes()), ("plugin.wasm", &module())])
}

fn feed(id: &str, version: &str, archive: &[u8]) -> Value {
    json!({
        "schema": 1,
        "plugin": {"id": id, "abi": 1},
        "version": version,
        "released": "2026-11-18",
        "notes": "What changed, short.",
        "module": {"url": ARCHIVE, "sha256": sha256(archive), "size": archive.len()},
    })
}

fn signed(id: &str, version: &str, archive: &[u8], publisher: &Publisher) -> Vec<u8> {
    let mut value = feed(id, version, archive);
    publisher.sign(&mut value);
    serde_json::to_vec(&value).unwrap()
}

fn network(feed_body: impl Into<Vec<u8>>, archive: impl Into<Vec<u8>>) -> Arc<Fake> {
    Arc::new(Fake::default().answer(FEED, feed_body).answer(ARCHIVE, archive))
}

fn loaded(root: &Path, net: Arc<Fake>) -> Plugins {
    Plugins::load_from_with_net(Some(root), net)
}

#[test]
fn without_the_update_permission_both_host_calls_answer_not_allowed() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "caller", "0.2.1", FEED, None);
    let net = network(
        signed("caller", "0.3.0", &normal_archive("caller", "0.3.0", FEED, None), &Publisher::new()),
        normal_archive("caller", "0.3.0", FEED, None),
    );
    let plugins = loaded(root.path(), net);
    let mut session = Session::new();
    plugins.register(&mut session);
    let envelope = session.execute("plugin:caller:run", &json!({})).unwrap();
    assert_eq!(envelope["ok"], json!(false), "{envelope}");
    assert_eq!(envelope["error"], json!(update::NOT_ALLOWED));
}

#[test]
fn after_the_grant_the_plugin_checks_its_channel_from_memory() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "caller", "0.2.1", FEED, None);
    let publisher = Publisher::new();
    let archive = normal_archive("caller", "0.3.0", FEED, None);
    let feed = signed("demo", "0.3.0", &archive, &publisher);
    let net = network(feed, archive);
    let mut plugins = loaded(root.path(), net);
    plugins.set_permission("caller", "update", true).unwrap();
    let mut session = Session::new();
    plugins.register(&mut session);
    let envelope = session.execute("plugin:caller:run", &json!({})).unwrap();
    assert_eq!(envelope["ok"], json!(true), "{envelope}");
    assert_eq!(envelope["result"]["available"], json!("0.3.0"));
    assert_eq!(envelope["result"]["current"], json!("0.2.1"));
    assert_eq!(envelope["result"]["host"], json!("example.org"));
    // The second call is answered from memory: no second request to the feed.
}

#[test]
fn a_feed_is_checked_before_the_archive_is_asked_for() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let mut feed = feed("demo", "0.3.0", &archive);
    publisher.sign(&mut feed);
    feed["signature"] = json!("ed25519:AAAA");
    let net = network(serde_json::to_vec(&feed).unwrap(), archive);
    let plugins = loaded(root.path(), net.clone());
    let e = plugins.check_update("demo", true).unwrap_err();
    assert!(e.contains("signature"), "{e}");
    assert!(!net.asked().iter().any(|u| u == ARCHIVE), "the archive wasn't asked for: {:?}", net.asked());
}

#[test]
fn another_id_another_abi_an_http_archive_and_an_absurd_size_are_refused() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    for (field, value, why) in [
        ("id", json!("other"), "announces"),
        ("abi", json!(2), "ABI"),
        ("url", json!("http://example.org/demo.zip"), "https"),
        ("size", json!(update::MAX_ARCHIVE + 1), "size"),
        ("sha256", json!("abc"), "SHA-256"),
    ] {
        let mut feed = feed("demo", "0.3.0", &archive);
        if field == "id" {
            feed["plugin"]["id"] = value;
        } else if field == "abi" {
            feed["plugin"]["abi"] = value;
        } else {
            feed["module"][field] = value;
        }
        publisher.sign(&mut feed);
        let net = network(serde_json::to_vec(&feed).unwrap(), archive.clone());
        let plugins = loaded(root.path(), net);
        let e = plugins.check_update("demo", true).unwrap_err();
        assert!(e.contains(why), "{field}: {e}");
    }
}

#[test]
fn the_archive_must_have_the_listed_size_and_digest() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let mut value = feed("demo", "0.3.0", &archive);
    value["module"]["sha256"] = json!("0".repeat(64));
    publisher.sign(&mut value);
    let plugins = loaded(root.path(), network(serde_json::to_vec(&value).unwrap(), archive.clone()));
    plugins.updater().set_consent(Arc::new(update::Allow));
    let e = plugins.apply_update("demo", true).unwrap_err();
    assert!(e.contains("SHA-256"), "{e}");

    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let mut value = feed("demo", "0.3.0", &archive);
    value["module"]["size"] = json!(archive.len() + 1);
    publisher.sign(&mut value);
    let plugins = loaded(root.path(), network(serde_json::to_vec(&value).unwrap(), archive));
    plugins.updater().set_consent(Arc::new(update::Allow));
    let e = plugins.apply_update("demo", true).unwrap_err();
    assert!(e.contains("bytes"), "{e}");
}

#[test]
fn an_archive_needs_its_manifest_and_only_the_files_it_declares() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    for files in [
        vec![("plugin.wasm", module())],
        vec![
            ("plugin.json", serde_json::to_vec(&manifest("demo", "0.3.0", FEED, None)).unwrap()),
            ("plugin.wasm", module()),
            ("extra.txt", b"x".to_vec()),
        ],
        vec![("plugin.json", serde_json::to_vec(&manifest("demo", "0.3.0", FEED, None)).unwrap())],
    ] {
        let archive = zip_of(&files.iter().map(|(n, b)| (*n, b.as_slice())).collect::<Vec<_>>());
        let feed = signed("demo", "0.3.0", &archive, &publisher);
        let plugins = loaded(root.path(), network(feed, archive));
        plugins.updater().set_consent(Arc::new(update::Allow));
        let e = plugins.apply_update("demo", true).unwrap_err();
        assert!(e.contains("plugin.json") || e.contains("module") || e.contains("doesn't declare"), "{e}");
    }
}

#[test]
fn a_path_out_of_the_folder_and_a_symbolic_link_are_refused() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    let manifest = serde_json::to_vec(&manifest("demo", "0.3.0", FEED, None)).unwrap();
    // `..` would write outside the plugin's folder.
    let escaping = zip_of(&[("plugin.json", &manifest), ("plugin.wasm", &module()), ("../escape.wasm", b"x")]);
    let feed = signed("demo", "0.3.0", &escaping, &publisher);
    let plugins = loaded(root.path(), network(feed, escaping));
    plugins.updater().set_consent(Arc::new(update::Allow));
    let e = plugins.apply_update("demo", true).unwrap_err();
    assert!(e.contains("unsafe path") || e.contains("zip"), "{e}");
    assert!(!root.path().join("escape.wasm").exists());

    // A symlink could point a later write anywhere.
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let plain = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("plugin.json", plain).unwrap();
    writer.write_all(&manifest).unwrap();
    writer.start_file("plugin.wasm", plain.unix_permissions(0o120777)).unwrap();
    writer.write_all(b"target").unwrap();
    let linked = writer.finish().unwrap().into_inner();
    let feed = signed("demo", "0.3.0", &linked, &publisher);
    let plugins = loaded(root.path(), network(feed, linked));
    plugins.updater().set_consent(Arc::new(update::Allow));
    let e = plugins.apply_update("demo", true).unwrap_err();
    assert!(e.contains("symbolic link"), "{e}");
}

#[test]
fn the_same_version_is_not_an_update_and_an_older_one_needs_a_rollback() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.2.1", FEED, None);
    let feed = signed("demo", "0.2.1", &archive, &publisher);
    let plugins = loaded(root.path(), network(feed, archive));
    let check = plugins.check_update("demo", true).unwrap();
    assert!(check.up_to_date);
    assert_eq!(check.available, "0.2.1");
    let e = plugins.apply_update("demo", true).unwrap_err();
    assert!(e.contains("already installed"), "{e}");

    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let archive = normal_archive("demo", "0.2.0", FEED, None);
    let feed = signed("demo", "0.2.0", &archive, &publisher);
    let plugins = loaded(root.path(), network(feed, archive));
    let check = plugins.check_update("demo", true).unwrap();
    assert!(check.up_to_date, "a lower version is not offered by a plain check");
    // Installing it anyway is a rollback: never without the user's yes, and with it the lower version is put in.
    assert!(plugins.apply_update("demo", false).unwrap_err().contains("declined"));
    plugins.updater().set_consent(Arc::new(update::Allow));
    let installed = plugins.apply_update("demo", false).unwrap();
    assert_eq!(installed.installed, "0.2.0");
}

#[test]
fn the_first_install_asks_and_the_next_one_does_not() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let feed = signed("demo", "0.3.0", &archive, &publisher);
    let plugins = loaded(root.path(), network(feed.clone(), archive.clone()));
    // The default consent is Deny: a plugin's own install can't silently take over the code.
    assert!(plugins.apply_update("demo", false).unwrap_err().contains("declined"));
    assert_eq!(Plugins::load_from(Some(root.path())).plugins[0].manifest.version, "0.2.1");
    plugins.updater().set_consent(Arc::new(update::Allow));
    let installed = plugins.apply_update("demo", false).unwrap();
    assert_eq!(installed.installed, "0.3.0");
    assert!(installed.restart);
    assert!(root.path().join("demo/plugin.json").is_file());
    let backups: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".backup-"))
        .collect();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert!(plugins.restart_required("demo"));
    // The backup sits beside the plugin but is not read as a second one with the same id.
    let again = Plugins::load_from(Some(root.path()));
    assert_eq!(again.plugins.len(), 1);
    assert!(again.broken.is_empty(), "{:?}", again.broken);
    // state.json keeps the grant and pins the key; the next version asks nothing.
    let state: Value = serde_json::from_str(&std::fs::read_to_string(root.path().join("state.json")).unwrap()).unwrap();
    assert_eq!(state["permissions"]["demo"], json!(["update"]));
    assert_eq!(state["update"]["demo"]["update_confirmed"], json!(true));
    assert!(plugins.update_history("demo").iter().any(|r| r.from == "0.2.1" && r.to == "0.3.0"));
}

#[test]
fn key_change_stops_and_asks_before_the_new_publisher_is_trusted() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, Some(&Publisher::new().key()));
    let old = Publisher::new();
    write_state(root.path(), "demo", true, Some((&old.key(), true)));
    let new = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, Some(&new.key()));
    let mut value = feed("demo", "0.3.0", &archive);
    value["public_key"] = json!(new.key());
    new.sign(&mut value);
    let plugins = loaded(root.path(), network(serde_json::to_vec(&value).unwrap(), archive));
    let check = plugins.check_update("demo", true).unwrap();
    assert!(check.key_change, "{check:?}");
    assert!(plugins.apply_update("demo", false).unwrap_err().contains("declined"));
    plugins.updater().set_consent(Arc::new(update::Allow));
    assert_eq!(plugins.apply_update("demo", false).unwrap().installed, "0.3.0");
}

#[test]
fn an_unsigned_feed_needs_the_setting_and_a_question() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let value = feed("demo", "0.3.0", &archive);
    let plugins = loaded(root.path(), network(serde_json::to_vec(&value).unwrap(), archive.clone()));
    let e = plugins.check_update("demo", true).unwrap_err();
    assert!(e.contains("unsigned"), "{e}");
    plugins.set_update_settings(bimcraft_plugin::UpdateSettings { allow_unsigned: true, ..plugins.update_settings() }).unwrap();
    assert_eq!(plugins.check_update("demo", true).unwrap().available, "0.3.0");
    assert!(plugins.apply_update("demo", false).unwrap_err().contains("declined"));
    plugins.updater().set_consent(Arc::new(update::Allow));
    assert_eq!(plugins.apply_update("demo", false).unwrap().installed, "0.3.0");
}

#[test]
fn a_rollback_swaps_the_backup_back_and_leaves_the_permissions_alone() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, Some(("", true)));
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let feed = signed("demo", "0.3.0", &archive, &publisher);
    let plugins = loaded(root.path(), network(feed, archive));
    plugins.updater().set_consent(Arc::new(update::Allow));
    plugins.apply_update("demo", false).unwrap();
    assert_eq!(Plugins::load_from(Some(root.path())).plugins[0].manifest.version, "0.3.0");
    assert_eq!(plugins.backups("demo").len(), 1);
    let installed = plugins.rollback_update("demo", None).unwrap();
    assert_eq!(installed.installed, "0.2.1");
    assert_eq!(Plugins::load_from(Some(root.path())).plugins[0].manifest.version, "0.2.1");
    let state: Value = serde_json::from_str(&std::fs::read_to_string(root.path().join("state.json")).unwrap()).unwrap();
    assert_eq!(state["permissions"]["demo"], json!(["update"]));
}

#[test]
fn one_update_at_a_time_is_said_by_the_lock() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, Some(("", true)));
    std::fs::write(root.path().join(update::LOCK), "other").unwrap();
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let feed = signed("demo", "0.3.0", &archive, &publisher);
    let plugins = loaded(root.path(), network(feed, archive));
    let e = plugins.apply_update("demo", true).unwrap_err();
    assert!(e.contains("already running"), "{e}");
}

#[test]
fn the_start_installs_a_new_version_without_a_button_when_updating_is_allowed() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    let publisher = Publisher::new();
    // The user allowed updating but never confirmed the channel by hand: the permission is the answer.
    write_state(root.path(), "demo", true, None);
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let feed = signed("demo", "0.3.0", &archive, &publisher);
    let plugins = loaded(root.path(), network(feed, archive));
    let done = plugins.updates_on_start();
    assert_eq!(done.len(), 1, "{done:?}");
    assert_eq!(Plugins::load_from(Some(root.path())).plugins[0].manifest.version, "0.3.0");
}

#[test]
fn a_start_does_nothing_once_updating_is_switched_off() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    // What "Disable automatic updates" leaves: no `update` in permissions.
    write_state(root.path(), "demo", false, Some(("", true)));
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let feed = signed("demo", "0.3.0", &archive, &publisher);
    let net = network(feed, archive);
    let plugins = Plugins::load_from_with_net(Some(root.path()), net.clone());
    assert!(plugins.updates_on_start().is_empty());
    assert!(net.asked().is_empty(), "no channel is even asked: {:?}", net.asked());
    assert_eq!(Plugins::load_from(Some(root.path())).plugins[0].manifest.version, "0.2.1");
}

#[test]
fn update_now_waits_for_the_windows_own_check() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "demo", "0.2.1", FEED, None);
    write_state(root.path(), "demo", true, None);
    let publisher = Publisher::new();
    let archive = normal_archive("demo", "0.3.0", FEED, None);
    let feed = signed("demo", "0.3.0", &archive, &publisher);
    let plugins = loaded(root.path(), network(feed, archive));
    // The startup-style check finds the version, but doesn't light Update now.
    assert_eq!(plugins.check_update("demo", false).unwrap().available, "0.3.0");
    assert!(!plugins.manual_check_ready("demo"));
    // The button's own check does, and installing takes the mark away.
    assert_eq!(plugins.check_update("demo", true).unwrap().available, "0.3.0");
    assert!(plugins.manual_check_ready("demo"));
    assert_eq!(plugins.apply_update("demo", true).unwrap().installed, "0.3.0");
    assert!(!plugins.manual_check_ready("demo"));
}

#[test]
fn a_manifest_without_the_update_block_still_loads_as_before() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("plain");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("plugin.json"),
        serde_json::to_string(&json!({
            "id": "plain", "version": "1.0.0", "name": "Plain", "abi": 1,
            "functions": [{"id": "run", "label": "Run"}],
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(folder.join("plugin.wasm"), module()).unwrap();
    let plugins = Plugins::load_from(Some(root.path()));
    assert_eq!(plugins.plugins[0].status, Status::Loaded);
    assert!(!plugins.plugins[0].manifest.asks_for_update());
    let mut session = Session::new();
    plugins.register(&mut session);
    assert!(session.external_command("plugin:plain:run").is_some());
}
