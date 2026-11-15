//! `deliver` through the real binary and its real DWG writer: a seed's delivered DWG is what `generate` then `run` write, with
//! the `generator_version` drawing property.

use std::path::PathBuf;
use std::process::{Command, Output};

const EXE: &str = env!("CARGO_BIN_EXE_bimcraft-cli");

fn dir(test: &str) -> Result<PathBuf, String> {
    let d = std::env::temp_dir().join(format!("bimcraft-cli-deliver-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn cli(cwd: &std::path::Path, args: &[&str]) -> Result<Output, String> {
    Command::new(EXE).current_dir(cwd).args(args).output().map_err(|e| e.to_string())
}

#[test]
fn the_version_is_the_release_and_a_commit() {
    let v = env!("BIMCRAFT_GENERATOR_VERSION");
    let release = env!("CARGO_PKG_VERSION");
    // A build with no git has none, and `deliver` then refuses; a build in the repository has a short commit. A version
    // given by BIMCRAFT_GENERATOR_VERSION at build time is the caller's own.
    if !v.is_empty() && env!("BIMCRAFT_GENERATOR_VERSION_FROM") == "git" {
        let rest = v.strip_prefix(&format!("bimcraft-{release}+")).expect("bimcraft-<release>+<commit>");
        let commit = rest.strip_suffix(".dirty").unwrap_or(rest);
        assert!(commit.len() >= 7 && commit.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)), "{v}");
    }
}

#[test]
fn a_delivered_dwg_is_what_generate_then_run_write_with_the_version_property() {
    let d = dir("bytes").unwrap();
    let brief = d.join("brief.json");
    std::fs::write(
        &brief,
        r#"{"schema_version": 1, "batches": [{"archetype": "house", "first_seed": 1, "count": 3, "rooms": [8, 12], "rooms_tol": 2}]}"#,
    )
    .expect("brief");
    std::fs::write(d.join("rights.txt"), "signed").expect("rights");
    let root = d.join("root");
    std::fs::create_dir_all(&root).expect("root");
    let s = |p: &PathBuf| p.display().to_string();
    let out = cli(
        &d,
        &[
            "deliver",
            "--brief",
            &s(&brief),
            "--delivery-id",
            "t",
            "--rights",
            &s(&d.join("rights.txt")),
            "--rerun",
            "2",
            "--jobs",
            "2",
            "--out",
            &s(&root),
        ],
    )
    .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if env!("BIMCRAFT_GENERATOR_VERSION").is_empty() {
        assert!(!out.status.success() && stderr.contains("invalid_params"), "{stderr}");
        return;
    }
    assert!(out.status.success(), "{stderr}");
    let summary: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON object");
    assert_eq!(summary["files"], 3);
    let folder = PathBuf::from(summary["folder"].as_str().expect("folder"));
    let csv = std::fs::read_to_string(folder.join("manifest.csv")).expect("csv");
    let row = csv.lines().nth(1).expect("a row");
    let rooms = {
        // The rooms the seed was asked for are in the plan's manifest: find the one `generate` reproduces.
        let mut found = None;
        for r in 6..=14 {
            let g = d.join(format!("gen{r}"));
            let o = cli(
                &d,
                &["generate", "--archetype", "house", "--seed", "1", "--rooms", &r.to_string(), "--rooms-tol", "2", "--out", &s(&g)],
            )
            .unwrap();
            if !o.status.success() {
                continue;
            }
            let script = g.join("script.bimcraft.txt");
            let mut text = std::fs::read_to_string(&script).expect("script");
            text.push_str(&format!(
                "\ndrawing_property {{\"name\": \"generator_version\", \"value\": {:?}}}\n",
                env!("BIMCRAFT_GENERATOR_VERSION")
            ));
            std::fs::write(&script, text).expect("script");
            let run = cli(&g, &["run", "--script", "script.bimcraft.txt", "--save", "plan.dwg"]).unwrap();
            assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
            if std::fs::read(g.join("plan.dwg")).expect("dwg") == std::fs::read(folder.join("dwg/house_s1.dwg")).expect("delivered") {
                found = Some(r);
                break;
            }
        }
        found
    };
    assert!(rooms.is_some(), "no `generate` + `run` makes the delivered DWG; row {row}");
}
