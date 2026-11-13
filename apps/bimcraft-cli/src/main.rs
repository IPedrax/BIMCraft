//! `bimcraft-cli`: BIMCraft from the command line. Scripts, conversion, and an MCP server for
//! agents, all through the same commands.

mod mcp;
mod revit;
mod window;

use std::io::Write;
use std::process::ExitCode;

use bimcraft_core::cad::DwgReader;
use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_plugin::{Level, Plugins};
use serde_json::{Value, json};

const USAGE: &str = "USAGE:
  bimcraft-cli run [FILE] [--sample | --highrise] [--cmd 'ID {json}']… [--script FILE]… [--save PATH]…
      Open FILE (a .bimc project or an earlier .bimcraft, or an .ifc or Revit .rvt file to import) or start empty, then in order: build the sample house, run
      commands and scripts (one `ID {json}` per line), save. --save writes by extension:
      .bimc (the project; .bimt the same), .ifc (IFC4), .glb (the 3D view's scene as glTF), .svg or .png (the lowest level's plan).
      Scripts may name a line `@name = ID {json}`; later values use `@name`, `$last`, `$-2` or a field such as `@walls.ids.0`.
      --highrise builds an original 20-storey performance sample (about 10,000 elements).
  bimcraft-cli info FILE         levels, element counts, types and warnings of a project or an IFC file; of a Revit .rvt or
                                 .rfa file, its release, last save, levels and element counts by category
  bimcraft-cli convert IN OUT    a project, an IFC file or a Revit .rvt file to .ifc, .glb, .svg, .png or .bimc
  bimcraft-cli commands          every command and its parameters
  bimcraft-cli mcp [--read-only] [FILE | --connect [PORT]]
                                 an MCP server on stdio, for agents: on a headless project of its own (FILE, opened first, or an
                                 empty one), or with --connect on the project open in a window whose control port is open
                                 (`bimcraft --control PORT` or File > Settings > Agents; without PORT, the last one). With
                                 --read-only the agent may read the project, never change it or write over its file
  bimcraft-cli --version         the release";

fn print(out: &mut impl Write, v: &Value) -> Result<(), String> {
    writeln!(out, "{v}").map_err(|e| format!("stdout: {e}"))
}

fn print_pretty(out: &mut impl Write, v: &Value) -> Result<(), String> {
    writeln!(out, "{}", serde_json::to_string_pretty(v).unwrap_or_default()).map_err(|e| format!("stdout: {e}"))
}

/// What a plugin log line says its level is, for the command line.
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Info => "info",
        Level::Warning => "warning",
        Level::Error => "error",
    }
}

/// Reads the plugin folder into `session`, and says what became of each plugin on stderr: the command line and
/// the MCP server have no plugin window to show it in. A missing folder is quiet.
pub(crate) fn load_plugins(session: &mut Session) -> Plugins {
    let plugins = bimcraft_plugin::load();
    plugins.register(session);
    for e in plugins.log() {
        eprintln!("bimcraft: plugin `{}` {}: {}", e.plugin, level_name(e.level), e.text);
    }
    plugins
}

/// Says the log lines added since `seen`, and remembers how many there are.
pub(crate) fn note_plugins(plugins: &Plugins, seen: &mut usize) {
    let entries = plugins.log();
    for e in entries.iter().skip(*seen) {
        eprintln!("bimcraft: plugin `{}` {}: {}", e.plugin, level_name(e.level), e.text);
    }
    *seen = entries.len();
}

fn cli(args: &[String], out: &mut impl Write) -> Result<(), String> {
    let mut s = Session::new();
    s.dwg = dwg_reader();
    dwg_writer(&mut s);
    let plugins = load_plugins(&mut s);
    let mut seen = plugins.log().len();
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
                note_plugins(&plugins, &mut seen);
            }
            Ok(())
        }
        [cmd, file] if cmd == "info" && revit::is_revit(file) => print_pretty(out, &revit::info(file)?),
        [cmd, file, flag] if cmd == "info" && revit::is_revit(file) && (flag == "--elements-detail" || flag == "--json") => {
            print_pretty(out, &revit::info_with_options(file, true)?)
        }
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
        [cmd] if cmd == "commands" => {
            let list = s.catalog();
            note_plugins(&plugins, &mut seen);
            print_pretty(out, &list)
        }
        [flag] if flag == "--version" || flag == "-V" => {
            writeln!(out, "bimcraft-cli {}", env!("CARGO_PKG_VERSION")).map_err(|e| format!("stdout: {e}"))
        }
        _ => Err(USAGE.into()),
    }
}

/// What follows `mcp`.
#[derive(Debug, Default, PartialEq)]
struct McpArgs {
    /// `--connect`, with a port or without: none inside is a window, whichever the bridge file names.
    connect: Option<Option<u16>>,
    /// `--read-only`: the agent may read the project and never change it.
    read_only: bool,
    /// The project a headless server opens before it answers.
    file: Option<String>,
}

const MCP_USAGE: &str = "usage: bimcraft-cli mcp [--read-only] [FILE | --connect [PORT]]";

fn mcp_args(args: &[String]) -> Result<McpArgs, String> {
    let mut a = McpArgs::default();
    let mut rest = args.iter().peekable();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--read-only" => a.read_only = true,
            "--connect" if a.connect.is_none() => {
                let port = rest.next_if(|p| !p.starts_with("--"));
                a.connect = Some(port.map(|p| p.parse().map_err(|_| format!("`{p}` is not a port: a number up to 65535"))).transpose()?);
            }
            flag if flag.starts_with('-') => return Err(format!("`{flag}`: {MCP_USAGE}")),
            file if a.file.is_none() => a.file = Some(file.to_string()),
            _ => return Err(MCP_USAGE.into()),
        }
    }
    if a.connect.is_some() && a.file.is_some() {
        return Err(format!("a window has its own project: --connect takes no FILE ({MCP_USAGE})"));
    }
    Ok(a)
}

/// The MCP server `args` asks for: attached to a window, or on a session of its own with its file open, and read-only when asked.
fn mcp_server(args: McpArgs) -> Result<mcp::Server, String> {
    let mut server = match args.connect {
        Some(port) => mcp::Server::attached(window::Window::connect(port)?),
        None => mcp::Server::new(),
    };
    if let Some(file) = &args.file {
        server.open(file)?;
    }
    if args.read_only {
        server.read_only();
    }
    Ok(server)
}

/// The DWG reader a session is given: `bimcraft-dwg`'s, which reads a drawing in a child process of this program, behind the
/// `dwg` feature. A build without it hands none, and a DWG says to save it as DXF.
#[cfg(feature = "dwg")]
fn dwg_reader() -> Option<DwgReader> {
    bimcraft_dwg::reader()
}

#[cfg(not(feature = "dwg"))]
fn dwg_reader() -> Option<DwgReader> {
    None
}

/// The DWG writer a session's exports are given: `bimcraft-dwg`'s, which writes a drawing in a child process of this program,
/// behind the `dwg` feature. A build without it leaves DWG listed as planned.
#[cfg(feature = "dwg")]
pub(crate) fn dwg_writer(session: &mut Session) {
    if let Some(w) = bimcraft_dwg::writer() {
        session.exports.register(w);
    }
}

#[cfg(not(feature = "dwg"))]
pub(crate) fn dwg_writer(_: &mut Session) {}

/// What this program does when it's started as the child that reads a DWG (`bimcraft_dwg::CHILD_ARG`): none when it isn't.
#[cfg(feature = "dwg")]
fn dwg_child(args: &[String]) -> Option<ExitCode> {
    bimcraft_dwg::child_main(args)
}

#[cfg(not(feature = "dwg"))]
fn dwg_child(_: &[String]) -> Option<ExitCode> {
    None
}

/// Counts what this program allocates, so a file being read is held to the memory the machine has free (the core's `memory.rs`).
#[global_allocator]
static ALLOC: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

/// What the core measures a file being read by: what this program has allocated, and what the machine has free (less, where a
/// container holds it to less).
fn meter() -> bimcraft_core::memory::Meter {
    bimcraft_core::memory::Meter { used: || ALLOC.allocated(), free: free_memory }
}

fn free_memory() -> Option<u64> {
    let mut s = sysinfo::System::new().ok()?;
    s.refresh_memory();
    let free = s.cgroup_limits().map_or(s.available_memory(), |c| c.free_memory.min(s.available_memory()));
    (free > 0).then_some(free)
}

fn main() -> ExitCode {
    bimcraft_core::memory::install(meter());
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = dwg_child(&args) {
        return code;
    }
    if args.first().is_some_and(|a| a == "mcp") {
        let mut server = match mcp_args(&args[1..]).and_then(mcp_server) {
            Ok(server) => server,
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
    fn mcp_is_headless_on_a_file_or_none_or_connects_to_a_window_and_may_only_read_either() {
        let args = |text: &str| text.split_whitespace().map(String::from).collect::<Vec<_>>();
        let mcp = |connect, read_only, file: Option<&str>| Ok(McpArgs { connect, read_only, file: file.map(String::from) });
        assert_eq!(mcp_args(&args("")), mcp(None, false, None));
        assert_eq!(mcp_args(&args("--connect")), mcp(Some(None), false, None));
        assert_eq!(mcp_args(&args("--connect 7000")), mcp(Some(Some(7000)), false, None));
        assert_eq!(mcp_args(&args("--read-only house.bimcraft")), mcp(None, true, Some("house.bimcraft")));
        assert_eq!(mcp_args(&args("house.ifc --read-only")), mcp(None, true, Some("house.ifc")));
        assert_eq!(mcp_args(&args("--connect --read-only")), mcp(Some(None), true, None));
        assert_eq!(mcp_args(&args("--read-only --connect 7000")), mcp(Some(Some(7000)), true, None));
        for bad in ["--connect x", "--connect 70000", "--connect 1 2", "--attach", "a.bimcraft b.bimcraft", "--connect 7000 --connect"] {
            assert!(mcp_args(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_read_only_mcp_server_starts_on_the_file_it_is_given_and_a_file_that_is_not_there_stops_it() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-mcp-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("house.bimcraft");
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
        s.execute("save", &json!({"path": file})).unwrap();
        let given = McpArgs { connect: None, read_only: true, file: Some(file.to_string_lossy().into_owned()) };
        let mut server = mcp_server(given).unwrap();
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "inspect_model", "arguments": {}}});
        let reply: Value = serde_json::from_str(&server.handle_line(&line.to_string()).unwrap()).unwrap();
        assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("\"wall\": 1"), "{reply}");
        let missing = McpArgs { connect: None, read_only: true, file: Some(dir.join("gone.bimcraft").to_string_lossy().into_owned()) };
        assert!(mcp_server(missing).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn script_result_references_create_save_and_reopen_a_hosted_door() {
        let base = std::env::temp_dir().join(format!("bimcraft-cli-script-references-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let script = base.join("references.txt");
        let project = base.join("references.bimcraft");
        std::fs::write(
            &script,
            r#"@walls = wall {"points":[[0,0],[8000,0],[8000,6000]]}
door {"wall":"@walls.ids.1","offset":3000}
modify {"id":"$last","mark":"Entry"}"#,
        )
        .unwrap();
        let out = cli_out(&["run", "--script", &script.to_string_lossy(), "--save", &project.to_string_lossy()]).unwrap();
        let results: Vec<Value> = out.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let host = results[0]["ids"][1].as_u64().unwrap();
        let query = cli_out(&["run", &project.to_string_lossy(), "--cmd", "elements {\"category\":\"doors\"}"]).unwrap();
        let doors: Value = serde_json::from_str(query.lines().last().unwrap()).unwrap();
        assert_eq!(doors["count"], 1);
        assert_eq!(doors["elements"][0]["mark"], "Entry");
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path":project})).unwrap();
        let guid: bimcraft_core::model::Guid = serde_json::from_value(doors["elements"][0]["host"].clone()).unwrap();
        assert!(reopened.model.same(guid, host), "the saved door still belongs to the referenced wall");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn run_builds_the_sample_and_saves_each_format() {
        let base = std::env::temp_dir().join(format!("bimcraft-cli-{}", std::process::id()));
        let path = |ext: &str| base.with_extension(ext).to_string_lossy().into_owned();
        let (project, old, ifc, png, svg) = (path("bimc"), path("bimcraft"), path("ifc"), path("png"), path("svg"));
        let roof = r#"level {"elevation": 3200, "name": "Roof"}"#;
        let out = cli_out(&["run", "--sample", "--cmd", roof, "--save", &project, "--save", &old, "--save", &ifc, "--save", &png]).unwrap();
        assert!(out.contains("\"Roof\""), "{out}");
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        assert!(std::fs::read_to_string(&ifc).unwrap().contains("IFCWALL("));
        let info: Value = serde_json::from_str(&cli_out(&["info", &project]).unwrap()).unwrap();
        assert_eq!(info["counts"]["room"], 4);
        assert_eq!(info["levels"][2]["name"], "Roof", "levels sort by elevation");
        // The .bimcraft of earlier versions is the same file under its old name, and still opens.
        assert_eq!(std::fs::read(&old).unwrap(), std::fs::read(&project).unwrap());
        assert_eq!(serde_json::from_str::<Value>(&cli_out(&["info", &old]).unwrap()).unwrap()["counts"], info["counts"]);
        cli_out(&["convert", &old, &svg]).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg"));
        for p in [project, old, ifc, png, svg] {
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
        cli(&["run".into(), path.clone(), "--save".into(), format!("{path}.bimc")], &mut opened).unwrap();
        let project = std::fs::read_to_string(format!("{path}.bimc")).unwrap();
        assert!(!project.contains("jdoe") && !project.contains(&path), "the saver's name or the path got into the project");
        let detail: Value = serde_json::from_str(&cli_out(&["info", &path, "--elements-detail"]).unwrap()).unwrap();
        assert_eq!(detail["elements"]["elements_detail_placed_count"], 4);
        assert_eq!(detail["elements"]["partitions_count"], 1);
        let wall_det = detail["elements"]["elements_detail"].as_array().unwrap().iter().find(|e| e["id"] == 100).unwrap();
        assert_eq!(wall_det["category_name"], "Walls");
        assert!(wall_det["bbox_min_mm"].is_array() && wall_det["bbox_max_mm"].is_array() && wall_det["location_mm"].is_array());
    }

    #[cfg(feature = "rvt")]
    #[test]
    fn run_and_convert_open_a_revit_file_and_save_it_as_a_project_or_ifc() {
        let path = revit::tests::house("house.rvt");
        let (project, ifc, converted) = (format!("{path}.bimc"), format!("{path}.ifc"), format!("{path}-converted.ifc"));
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
