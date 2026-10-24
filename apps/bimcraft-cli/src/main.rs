//! `bimcraft-cli`: BIMCraft from the command line. Scripts, conversion, and an MCP server for
//! agents, all through the same commands.

mod mcp;
mod revit;
mod window;

use std::io::Write;
use std::process::ExitCode;

use bimcraft_core::engine::{Session, catalog};
use bimcraft_core::sample::SAMPLE;
use serde_json::{Value, json};

const USAGE: &str = "USAGE:
  bimcraft-cli run [FILE] [--sample | --highrise] [--cmd 'ID {json}']… [--script FILE]… [--save PATH]…
      Open FILE (a .bimcraft project, or an .ifc or Revit .rvt file to import) or start empty, then in order: build the sample house, run
      commands and scripts (one `ID {json}` per line), save. --save writes by extension:
      .bimcraft (the project), .ifc (IFC4), .svg or .png (the lowest level's plan).
      --highrise builds an original 20-storey performance sample (about 10,000 elements).
  bimcraft-cli info FILE         levels, element counts, types and warnings of a project or an IFC file; of a Revit .rvt or
                                 .rfa file, its release, last save, levels and element counts by category
  bimcraft-cli convert IN OUT    a project, an IFC file or a Revit .rvt file to .ifc, .svg, .png or .bimcraft
  bimcraft-cli commands          every command and its parameters
  bimcraft-cli mcp [--connect [PORT]]
                                 an MCP server on stdio, for agents: on a headless project of its own, or with --connect on the
                                 project open in a window started with `bimcraft --control PORT` (without PORT, the last one)
  bimcraft-cli --version         the release";

fn print(out: &mut impl Write, v: &Value) -> Result<(), String> {
    writeln!(out, "{v}").map_err(|e| format!("stdout: {e}"))
}

fn print_pretty(out: &mut impl Write, v: &Value) -> Result<(), String> {
    writeln!(out, "{}", serde_json::to_string_pretty(v).unwrap_or_default()).map_err(|e| format!("stdout: {e}"))
}

fn cli(args: &[String], out: &mut impl Write) -> Result<(), String> {
    let mut s = Session::new();
    match args {
        [cmd, rest @ ..] if cmd == "run" => {
            let mut rest = rest.iter();
            while let Some(arg) = rest.next() {
                let mut value = || rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"));
                let results = match arg.as_str() {
                    "--sample" => s.script(SAMPLE).map(|_| Vec::new())?,
                    "--highrise" => vec![s.execute("highrise", &json!({}))?],
                    "--cmd" => s.script(value()?)?,
                    "--script" => {
                        let path = value()?;
                        s.script(&std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?)?
                    }
                    "--save" => vec![s.execute("save", &json!({ "path": value()? }))?],
                    flag if flag.starts_with("--") => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
                    file if revit::is_revit(file) => vec![revit::open(&mut s, file)?],
                    file => vec![s.execute("open", &json!({ "path": file }))?],
                };
                for r in results.iter().filter(|r| !r.is_null()) {
                    print(out, r)?;
                }
            }
            Ok(())
        }
        [cmd, file] if cmd == "info" && revit::is_revit(file) => print_pretty(out, &revit::info(file)?),
        [cmd, file] if cmd == "info" => {
            s.execute("open", &json!({ "path": file }))?;
            print_pretty(out, &s.execute("inspect", &json!({}))?)
        }
        [cmd, from, to] if cmd == "convert" => {
            if revit::is_revit(from) {
                revit::open(&mut s, from)?;
            } else {
                s.execute("open", &json!({ "path": from }))?;
            }
            print(out, &s.execute("save", &json!({ "path": to }))?)
        }
        [cmd] if cmd == "commands" => print_pretty(out, &catalog()),
        [flag] if flag == "--version" || flag == "-V" => {
            writeln!(out, "bimcraft-cli {}", env!("CARGO_PKG_VERSION")).map_err(|e| format!("stdout: {e}"))
        }
        _ => Err(USAGE.into()),
    }
}

/// What follows `mcp`: nothing, or `--connect` with a port or without. The inner None is a window, whichever the bridge file names.
fn connect_option(args: &[String]) -> Result<Option<Option<u16>>, String> {
    match args {
        [] => Ok(None),
        [flag] if flag == "--connect" => Ok(Some(None)),
        [flag, port] if flag == "--connect" => {
            port.parse().map(|p| Some(Some(p))).map_err(|_| format!("`{port}` is not a port: a number up to 65535"))
        }
        _ => Err("usage: bimcraft-cli mcp [--connect [PORT]]".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "mcp") {
        let mut server = match connect_option(&args[1..]).and_then(|port| port.map(window::Window::connect).transpose()) {
            Ok(Some(window)) => mcp::Server::attached(window),
            Ok(None) => mcp::Server::new(),
            Err(e) => {
                eprintln!("bimcraft-cli mcp: {e}");
                return ExitCode::FAILURE;
            }
        };
        return match server.serve(std::io::stdin().lock(), std::io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("bimcraft-cli mcp: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match cli(&args, &mut std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn cli_out(args: &[&str]) -> Result<String, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        cli(&args, &mut out).map(|()| String::from_utf8(out).unwrap())
    }

    #[test]
    fn mcp_is_headless_or_connects_to_a_window_with_a_port_or_without() {
        let args = |text: &str| text.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert_eq!(connect_option(&args("")), Ok(None));
        assert_eq!(connect_option(&args("--connect")), Ok(Some(None)));
        assert_eq!(connect_option(&args("--connect 7000")), Ok(Some(Some(7000))));
        for bad in ["--connect x", "--connect 70000", "--connect 1 2", "--attach", "7000"] {
            assert!(connect_option(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn run_builds_the_sample_and_saves_each_format() {
        let base = std::env::temp_dir().join(format!("bimcraft-cli-{}", std::process::id()));
        let path = |ext: &str| base.with_extension(ext).to_string_lossy().into_owned();
        let (project, ifc, png, svg) = (path("bimcraft"), path("ifc"), path("png"), path("svg"));
        let roof = r#"level {"elevation": 3200, "name": "Roof"}"#;
        let out = cli_out(&["run", "--sample", "--cmd", roof, "--save", &project, "--save", &ifc, "--save", &png]).unwrap();
        assert!(out.contains("\"Roof\""), "{out}");
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        assert!(std::fs::read_to_string(&ifc).unwrap().contains("IFCWALL("));
        let info: Value = serde_json::from_str(&cli_out(&["info", &project]).unwrap()).unwrap();
        assert_eq!(info["counts"]["room"], 4);
        assert_eq!(info["levels"][2]["name"], "Roof", "levels sort by elevation");
        cli_out(&["convert", &project, &svg]).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg"));
        for p in [project, ifc, png, svg] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn highrise_option_builds_the_shareable_performance_fixture() {
        let out = cli_out(&["run", "--highrise", "--cmd", "inspect {}"]).unwrap();
        let reply: Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
        assert_eq!(reply["storeys"], 20);
        assert_eq!(reply["elements"], 9521);
        let info: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
        assert_eq!(info["counts"]["level"], 21);
        assert_eq!(info["counts"]["beam"], 2200);
    }

    #[test]
    fn bad_usage_and_failing_commands_are_errors() {
        assert!(cli_out(&[]).unwrap_err().contains("USAGE"));
        assert!(cli_out(&["run", "--frobnicate"]).unwrap_err().contains("--frobnicate"));
        assert!(cli_out(&["run", "--save"]).unwrap_err().contains("needs"));
        assert!(cli_out(&["run", "--cmd", r#"door {"at": [0, 0]}"#]).unwrap_err().contains("no wall"));
        assert!(cli_out(&["commands"]).unwrap().contains("\"wall\""));
    }

    #[test]
    fn version_names_the_release() {
        let line = format!("bimcraft-cli {}\n", env!("CARGO_PKG_VERSION"));
        assert_eq!(cli_out(&["--version"]).unwrap(), line);
        assert_eq!(cli_out(&["-V"]).unwrap(), line);
        assert!(cli_out(&["--version", "x"]).unwrap_err().contains("USAGE"));
    }

    /// A synthetic Revit project on disk, as a test file name in a folder of its own.
    #[cfg(feature = "rvt")]
    fn revit_file(name: &str, bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-rvt-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn info_of_a_revit_file_shows_the_release_the_last_save_the_levels_and_the_counts() {
        use bimcraft_rvt::project_fixture::{Lev, Project, Rec};
        let mut p = Project::new(2024);
        p.saved_by = "jdoe".into();
        p.recs = vec![Rec::new(100, -2_000_011), Rec::new(101, -2_000_011), Rec::new(200, -2_000_023), Rec::new(300, -2_009_999)];
        p.levels = vec![
            Lev::new(11, "Level 2", 9.84251968503937),
            Lev { story: Some(true), ..Lev::new(10, "Level 1", 0.0) },
            Lev { story: Some(false), owner: Some(4321), ..Lev::new(12, "Inside a family", 4.0) },
        ];
        // Its own name: another test writes a house.rvt of its own in the same folder, at the same time.
        let path = revit_file("info.rvt", &p.file());
        let info: Value = serde_json::from_str(&cli_out(&["info", &path]).unwrap()).unwrap();
        assert_eq!(info["revit"]["release"], 2024);
        assert_eq!(info["revit"]["worksharing"], "Not enabled");
        assert_eq!(info["saved_by"], "jdoe");
        assert!(info["saved_to"].as_str().unwrap().ends_with("house.rvt"), "the path the file says it was saved to");
        assert_eq!(info["elements"]["levels"][0]["name"], "Level 1");
        assert_eq!(info["elements"]["levels"][1]["elevation"], 3000.0, "millimetres");
        assert_eq!(info["elements"]["levels"][0]["building_story"], true);
        assert!(info["elements"]["levels"][1]["building_story"].is_null(), "a block with no setting says none");
        assert_eq!(info["elements"]["levels"].as_array().unwrap().len(), 2, "a level inside another element is not one");
        assert_eq!(info["elements"]["owned_levels"], json!([12]));
        let cat = |id: i64| info["elements"]["categories"].as_array().unwrap().iter().find(|c| c["category"] == id).cloned();
        assert_eq!(cat(-2_000_011).unwrap()["name"], "Walls");
        assert_eq!(cat(-2_000_011).unwrap()["elements"], 2);
        assert_eq!(cat(-2_000_240).unwrap()["elements"], 3, "the levels are elements too");
        assert!(cat(-2_009_999).unwrap()["name"].is_null(), "a category with no known name has none");
        assert_eq!(info["elements"]["total"], 7);
        // The saver's name and the path stay out of everything but the report: the project of the same file has neither.
        let mut opened = Vec::new();
        cli(&["run".into(), path.clone(), "--save".into(), format!("{path}.bimcraft")], &mut opened).unwrap();
        let project = std::fs::read_to_string(format!("{path}.bimcraft")).unwrap();
        assert!(!project.contains("jdoe") && !project.contains(&path), "the saver's name or the path got into the project");
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn run_and_convert_open_a_revit_file_and_save_it_as_a_project_or_ifc() {
        let path = revit::tests::house("house.rvt");
        let (project, ifc, converted) = (format!("{path}.bimcraft"), format!("{path}.ifc"), format!("{path}-converted.ifc"));
        let out = cli_out(&["run", &path, "--cmd", r#"elements {"category": "wall"}"#, "--save", &project, "--save", &ifc]).unwrap();
        let lines: Vec<Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!((lines[0]["format"].as_str(), lines[0]["name"].as_str()), (Some("Revit 2024"), Some("house")), "{out}");
        assert_eq!(lines[0]["elements"], json!({"level": 2, "wall": 1}));
        assert_eq!(lines[0]["skipped"], json!([]));
        assert_eq!(lines[1]["elements"][0]["end"], json!([6096.0, 0.0]), "20 ft in millimetres");
        assert!(std::fs::read_to_string(&ifc).unwrap().contains("IFCWALL("));
        // What was saved opens again as a project of its own.
        let info: Value = serde_json::from_str(&cli_out(&["info", &project]).unwrap()).unwrap();
        assert_eq!(info["counts"], json!({"level": 2, "wall": 1}));
        cli_out(&["convert", &path, &converted]).unwrap();
        assert!(std::fs::read_to_string(&converted).unwrap().contains("IFCWALL("));
        for f in [project, ifc, converted] {
            let _ = std::fs::remove_file(f);
        }
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn a_revit_file_that_does_not_open_is_an_error_that_names_it_and_saves_nothing() {
        let fake = revit_file("fake.rvt", b"this is not a Revit file at all");
        let out = format!("{fake}.ifc");
        for args in [vec!["run", "--sample", &fake], vec!["convert", &fake, &out]] {
            let e = cli_out(&args).unwrap_err();
            assert!(e.contains("fake.rvt") && e.contains("Compound File"), "{e}");
        }
        assert!(!std::path::Path::new(&out).exists());
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn info_of_a_file_that_is_not_a_project_or_not_revit_says_what_is_wrong() {
        let not = revit_file("fake.rvt", b"this is not a Revit file at all");
        let e = cli_out(&["info", &not]).unwrap_err();
        assert!(e.contains("fake.rvt") && e.contains("Compound File"), "{e}");
        // A file with the metadata and no partition (a family, say) still shows what it has, and says why it has no elements.
        let bytes = bimcraft_rvt::fixture::container(&[(
            "BasicFileInfo",
            &bimcraft_rvt::fixture::basic_file_info(&bimcraft_rvt::fixture::Basic::default()),
        )]);
        let family = revit_file("chair.rfa", &bytes);
        let info: Value = serde_json::from_str(&cli_out(&["info", &family]).unwrap()).unwrap();
        assert_eq!(info["revit"]["release"], 2024);
        assert!(info["elements"]["error"].as_str().unwrap().contains("Formats/Latest"), "{info}");
        assert!(cli_out(&["info", "/no/such/file.rvt"]).unwrap_err().contains("/no/such/file.rvt"));
    }

    #[cfg(not(feature = "rvt"))]
    #[test]
    fn a_build_without_revit_support_says_so_and_how_to_get_it() {
        for args in [vec!["info", "x.rvt"], vec!["run", "x.rvt"], vec!["convert", "x.rvt", "y.ifc"]] {
            let e = cli_out(&args).unwrap_err();
            assert!(e.contains("without Revit support") && e.contains("`rvt` feature"), "{e}");
        }
    }
}
