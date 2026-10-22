//! `bimcraft-cli`: BIMCraft from the command line. Scripts, conversion, and an MCP server for
//! agents, all through the same commands.

mod mcp;

use std::io::Write;
use std::process::ExitCode;

use bimcraft_core::engine::{Session, catalog};
use bimcraft_core::sample::SAMPLE;
use serde_json::{Value, json};

const USAGE: &str = "USAGE:
  bimcraft-cli run [FILE] [--sample | --highrise] [--cmd 'ID {json}']… [--script FILE]… [--save PATH]…
      Open FILE (a .bimcraft project, or an .ifc file to import) or start empty, then in order: build the sample house, run
      commands and scripts (one `ID {json}` per line), save. --save writes by extension:
      .bimcraft (the project), .ifc (IFC4), .svg or .png (the lowest level's plan).
      --highrise builds an original 20-storey performance sample (about 10,000 elements).
  bimcraft-cli info FILE         levels, element counts, types and warnings of a project or an IFC file
  bimcraft-cli convert IN OUT    a project or an IFC file to .ifc, .svg, .png or .bimcraft
  bimcraft-cli commands          every command and its parameters
  bimcraft-cli mcp               an MCP server on stdio, for agents
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
                    file => vec![s.execute("open", &json!({ "path": file }))?],
                };
                for r in results.iter().filter(|r| !r.is_null()) {
                    print(out, r)?;
                }
            }
            Ok(())
        }
        [cmd, file] if cmd == "info" => {
            s.execute("open", &json!({ "path": file }))?;
            print_pretty(out, &s.execute("inspect", &json!({}))?)
        }
        [cmd, from, to] if cmd == "convert" => {
            s.execute("open", &json!({ "path": from }))?;
            print(out, &s.execute("save", &json!({ "path": to }))?)
        }
        [cmd] if cmd == "commands" => print_pretty(out, &catalog()),
        [flag] if flag == "--version" || flag == "-V" => {
            writeln!(out, "bimcraft-cli {}", env!("CARGO_PKG_VERSION")).map_err(|e| format!("stdout: {e}"))
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "mcp") {
        return match mcp::Server::new().serve(std::io::stdin().lock(), std::io::stdout().lock()) {
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
}
