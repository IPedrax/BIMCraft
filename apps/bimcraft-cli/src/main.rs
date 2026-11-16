//! `bimcraft-cli`: BIMCraft from the command line. Scripts, conversion, and an MCP server for
//! agents, all through the same commands.

mod mcp;
mod revit;
mod window;

use std::io::Write;
use std::process::ExitCode;

use bimcraft_core::cad::{DwgReader, DwgWriter};
use bimcraft_core::engine::{Session, catalog};
use bimcraft_core::sample::SAMPLE;
use serde_json::{Value, json};

const USAGE: &str = "USAGE:
  bimcraft-cli run [FILE] [--sample | --highrise] [--cmd 'ID {json}']… [--script FILE]… [--save PATH]…
      Open FILE (a .bimcraft project, or an .ifc or Revit .rvt file to import) or start empty, then in order: build the sample house, run
      commands and scripts (one `ID {json}` per line), save. --save writes by extension:
      .bimcraft (the project), .ifc (IFC4), .glb (the 3D view's scene as glTF), .svg or .png (the lowest level's plan).
      --highrise builds an original 20-storey performance sample (about 10,000 elements).
  bimcraft-cli generate --archetype office|clinic|hospital|house|studio-office|office-pattern|medical-day --seed N (--area M2 | --area-sqft SQFT) [--delivery on|off] [--gridlines on|off] --out DIR
                                 write a generated plan's run directory: program, topology and building graphs, a script for
                                 `run --script`, a report and a manifest. The same brief gives the same files; a brief that
                                 can't be met (an area too small) exits non-zero with a JSON error (`error`, a code, and `message`) on stderr, and writes nothing
  bimcraft-cli deliver --brief BRIEF.json --delivery-id ID --rights (FILE | emailed) --out ROOT [--rerun N] [--jobs J]
                                 write ROOT/<generator_version>/ID/ (manifest.csv, RIGHTS.txt, dwg/) for the brief's seeds, and
                                 ID-rerun/ for N seeds (50; 0 for none) spread through them, made again; J threads. Each DWG
                                 holds the generator_version drawing property. schema_version 1 briefs are frozen; schema_version 2
                                 batches choose reviewed shape-graph families, shape, room count, interior, bumps and gridlines from
                                 the seed unless building_types/interior/bumps/gridlines pin them. Never overwrites
  bimcraft-cli generate (--archetype clinic|hospital|house|medical-day|office-pattern|school|studio-office | --family-file FILE) --seed N --rooms R [--rooms-tol T] [--shape ID] [--interior aligned|jogged|mixed] [--bumps on|off] [--gridlines on|off] [--ext-pen 8..14] [--ext-light 0..45] [--int-pen 3..9] [--int-light 0..65] [--unreviewed allow] --out DIR
                                 the shape graph engine: R rooms (between R-T and R+T) laid out in one of nine outlines (bar, narrow_bar,
                                 l_shape, corner_notch, stepped, t_shape, side_nub, opposite_wings, court; --shape pins one) by the
                                 family file of the archetype (crates/core/data/shape-families), or the family file given with
                                 --family-file (checked like a built-in one: invalid_family if wrong). The area is a result, not an input;
                                 --rooms with --area is refused, as is an option of this form without --rooms. Same six files
  bimcraft-cli topology --model FILE.bimcraft --out FILE.json
                                 read a saved project and write its 2D topology graph from geometry alone: rooms as nodes, door,
                                 open and adjacent edges, the entry, the footprint and typed diagnostics (a room no walls enclose).
                                 Written even with diagnostics; prints a one-line summary
  bimcraft-cli topology --model FILE.bimcraft [--out FILE.json] --expect EXPECTED.json
                                 also compare the graph with a traced plan's expected file (rooms, links, entry). Exit 0 when they
                                 agree; on a disagreement exit non-zero with a JSON error `topology_mismatch` on stderr holding
                                 missing_rooms, extra_rooms, missing_links (in the model, not the file), extra_links (in the file,
                                 not the model), entry and diagnostics. --out is written either way
  bimcraft-cli rule-pack seed --out DIR
                                 write the frontier-model rule pack seed library: a manifest, a validation report and one pack of
                                 rules for each of office, clinic, hospital, datacenter and airport. The same files every time; a
                                 library that fails its own checks exits non-zero, naming why, and writes nothing
  bimcraft-cli rule-pack check --archetype house|office|clinic|hospital|studio-office|office-pattern|medical-day --graph GRAPH.json
                                 judge a topology graph by the archetype's rule pack and print each rule's PASS or FAIL. Exit 0
                                 when none fails; otherwise exit non-zero with a JSON error `rule_violation` on stderr holding
                                 `failed` (the rule ids) and `checks`. House graphs need a `kind` on each node (the extractor
                                 gives none); office, clinic and hospital graphs a `kind` and a `role`; studio-office, office-pattern and medical-day graphs a `kind`
  bimcraft-cli drawing-miner cad --corpus DIR --out DIR
                                 mine a CAD corpus described by DIR/sources.json into normalized scene, semantic element, space graph,
                                 overlay, candidate-rule, report and manifest artifacts. Oracles in the corpus are used for fixture
                                 metrics; candidate rules are exploratory only
  bimcraft-cli info FILE         levels, element counts, types and warnings of a project or an IFC file; of a Revit .rvt or
                                 .rfa file, its release, last save, levels and element counts by category
  bimcraft-cli convert IN OUT    a project, an IFC file or a Revit .rvt file to .ifc, .glb, .svg, .png or .bimcraft
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

fn cli(args: &[String], out: &mut impl Write) -> Result<(), String> {
    let mut s = Session::new();
    s.dwg = dwg_reader();
    s.dwg_writer = dwg_writer();
    // Fixed when the CLI was built; empty when git and `BIMCRAFT_COMMIT` knew no commit, and `deliver` then refuses.
    s.generator_version = Some(env!("BIMCRAFT_GENERATOR_VERSION").to_string()).filter(|v| !v.is_empty());
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
        [cmd, rest @ ..] if cmd == "generate" => {
            let params = generate_params(rest)?;
            print(out, &s.execute("generate", &params).map_err(typed_refusal)?)
        }
        [cmd, rest @ ..] if cmd == "deliver" => {
            let params = deliver_params(rest)?;
            print(out, &s.execute("deliver", &params).map_err(typed_refusal)?)
        }
        [cmd, rest @ ..] if cmd == "topology" => {
            let (model, params) = topology_params(rest)?;
            s.execute("open", &json!({ "path": model }))?;
            let g = s.execute("topology", &params)?;
            let count = |k: &str| g[k].as_array().map_or(0, Vec::len);
            let kinds = |kind: &str| g["edges"].as_array().map_or(0, |e| e.iter().filter(|e| e["kind"] == kind).count());
            let c = &g["comparison"];
            if c["agree"] == false {
                let mut m = json!({"error": "topology_mismatch", "path": g["path"], "expect": c["expect"]});
                for k in ["missing_rooms", "extra_rooms", "missing_links", "extra_links", "entry", "diagnostics"] {
                    m[k] = c[k].clone();
                }
                return Err(m.to_string());
            }
            let mut reply = json!({
                "path": g["path"], "nodes": count("nodes"), "door": kinds("door"), "open": kinds("open"),
                "adjacent": kinds("adjacent"), "entry": g["entry"]["room"], "diagnostics": count("diagnostics"),
            });
            if c["agree"] == true {
                reply["expect"] = c["expect"].clone();
                reply["agree"] = json!(true);
            }
            print(out, &reply)
        }
        [cmd, sub, rest @ ..] if cmd == "rule-pack" && sub == "seed" => {
            let params = rule_pack_params(rest)?;
            print(out, &s.execute("rule_pack", &params)?)
        }
        [cmd, sub, rest @ ..] if cmd == "rule-pack" && sub == "check" => {
            let params = rule_check_params(rest)?;
            let reply = s.execute("rule_check", &params)?;
            if reply["pass"] == true {
                return print(out, &reply);
            }
            let failed: Vec<&Value> =
                reply["checks"].as_array().into_iter().flatten().filter(|c| c["status"] == "FAIL").map(|c| &c["id"]).collect();
            Err(json!({
                "error": "rule_violation", "archetype": reply["archetype"], "pack_id": reply["pack_id"],
                "failed": failed, "checks": reply["checks"],
            })
            .to_string())
        }
        [cmd, sub, rest @ ..] if cmd == "drawing-miner" && sub == "cad" => {
            let params = drawing_miner_params(rest)?;
            print(out, &s.execute("drawing_miner", &params)?)
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

/// A refusal the generator words as `code: message`, as the JSON object `{"error": code, "message": message}` on stderr, so a
/// program can read the code. Anything else is left as it is.
fn typed_refusal(e: String) -> String {
    match e.split_once(": ") {
        Some((code, message)) if !code.is_empty() && code.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') => {
            json!({"error": code, "message": message}).to_string()
        }
        _ => e,
    }
}

/// What follows `generate`, as the command's params: all four options, each once.
fn generate_params(args: &[String]) -> Result<Value, String> {
    let (mut archetype, mut seed, mut area, mut area_sqft, mut out) = (None, None, None, None, None);
    // The shape graph engine's options: `--rooms` selects it, and the core refuses the rest without it (as JSON).
    let (mut rooms, mut rooms_tol, mut shape, mut interior, mut bumps) = (None, None, None, None, None);
    let (mut family_file, mut delivery, mut gridlines, mut unreviewed) = (None, None, None, None);
    let (mut ext_pen, mut ext_light, mut int_pen, mut int_light) = (None, None, None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
        let slot = match arg.as_str() {
            "--ext-pen" => &mut ext_pen,
            "--ext-light" => &mut ext_light,
            "--int-pen" => &mut int_pen,
            "--int-light" => &mut int_light,
            "--archetype" => &mut archetype,
            "--seed" => &mut seed,
            "--area" => &mut area,
            "--area-sqft" => &mut area_sqft,
            "--out" => &mut out,
            "--rooms" => &mut rooms,
            "--rooms-tol" => &mut rooms_tol,
            "--shape" => &mut shape,
            "--interior" => &mut interior,
            "--bumps" => &mut bumps,
            "--family-file" => &mut family_file,
            "--delivery" => &mut delivery,
            "--gridlines" => &mut gridlines,
            "--unreviewed" => &mut unreviewed,
            other => return Err(format!("unknown option `{other}` for generate\n\n{USAGE}")),
        };
        if slot.replace(value).is_some() {
            return Err(format!("{arg} given twice\n\n{USAGE}"));
        }
    }
    let need = |what: Option<&String>, flag: &str| what.cloned().ok_or_else(|| format!("generate needs {flag}\n\n{USAGE}"));
    let seed = need(seed, "--seed")?;
    if area.is_some() && area_sqft.is_some() {
        return Err(format!("generate needs only one of --area or --area-sqft, not both\n\n{USAGE}"));
    }
    if area.is_none() && area_sqft.is_none() && rooms.is_none() {
        return Err(format!("generate needs one of --area, --area-sqft or --rooms\n\n{USAGE}"));
    }
    let area_m2 = match area {
        Some(area) => Some(area.parse::<f64>().map_err(|_| format!("--area is a number of square metres, got `{area}`"))?),
        None => None,
    };
    let sqft = match area_sqft {
        Some(area) => Some(area.parse::<f64>().map_err(|_| format!("--area-sqft is a number of square feet, got `{area}`"))?),
        None => None,
    };
    let mut p = json!({
        "seed": seed.parse::<u64>().map_err(|_| format!("--seed is a whole number from 0, got `{seed}`"))?,
        "path": need(out, "--out")?,
    });
    if let Some(area) = area_m2 {
        p["area_m2"] = json!(area);
    }
    if let Some(area) = sqft {
        p["area_sqft"] = json!(area);
    }
    for (key, flag, value) in [
        ("rooms", "--rooms", rooms),
        ("rooms_tol", "--rooms-tol", rooms_tol),
        ("ext_pen", "--ext-pen", ext_pen),
        ("ext_light", "--ext-light", ext_light),
        ("int_pen", "--int-pen", int_pen),
        ("int_light", "--int-light", int_light),
    ] {
        if let Some(v) = value {
            p[key] = json!(v.parse::<i64>().map_err(|_| format!("{flag} is a whole number, got `{v}`"))?);
        }
    }
    // The core refuses both, or neither, as typed JSON.
    for (key, value) in [
        ("archetype", archetype),
        ("family_file", family_file),
        ("shape", shape),
        ("interior", interior),
        ("bumps", bumps),
        ("delivery", delivery),
        ("gridlines", gridlines),
        ("unreviewed", unreviewed),
    ] {
        if let Some(v) = value {
            p[key] = json!(v);
        }
    }
    Ok(p)
}

/// What follows `deliver`, as the command's params: each option once. `--rights` may be left out, so the core refuses with
/// `missing_rights`.
fn deliver_params(args: &[String]) -> Result<Value, String> {
    let (mut brief, mut id, mut rights, mut out, mut rerun, mut jobs) = (None, None, None, None, None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
        let slot = match arg.as_str() {
            "--brief" => &mut brief,
            "--delivery-id" => &mut id,
            "--rights" => &mut rights,
            "--out" => &mut out,
            "--rerun" => &mut rerun,
            "--jobs" => &mut jobs,
            other => return Err(format!("unknown option `{other}` for deliver\n\n{USAGE}")),
        };
        if slot.replace(value).is_some() {
            return Err(format!("{arg} given twice\n\n{USAGE}"));
        }
    }
    let need = |what: Option<&String>, flag: &str| what.cloned().ok_or_else(|| format!("deliver needs {flag}\n\n{USAGE}"));
    let mut p = json!({"brief": need(brief, "--brief")?, "delivery_id": need(id, "--delivery-id")?, "path": need(out, "--out")?});
    if let Some(r) = rights {
        p["rights"] = json!(r);
    }
    for (key, flag, value) in [("rerun", "--rerun", rerun), ("jobs", "--jobs", jobs)] {
        if let Some(v) = value {
            p[key] = json!(v.parse::<i64>().map_err(|_| format!("{flag} is a whole number, got `{v}`"))?);
        }
    }
    Ok(p)
}

/// What follows `rule-pack seed`, as the command's params: `--out`, once.
fn rule_pack_params(args: &[String]) -> Result<Value, String> {
    let mut out: Option<&String> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
        if arg != "--out" {
            return Err(format!("unknown option `{arg}` for rule-pack seed\n\n{USAGE}"));
        }
        if out.replace(value).is_some() {
            return Err(format!("{arg} given twice\n\n{USAGE}"));
        }
    }
    let out = out.ok_or_else(|| format!("rule-pack seed needs --out\n\n{USAGE}"))?;
    Ok(json!({ "path": out }))
}

/// What follows `rule-pack check`, as the command's params: `--archetype` and `--graph`, each once.
fn rule_check_params(args: &[String]) -> Result<Value, String> {
    let (mut archetype, mut graph) = (None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
        let slot = match arg.as_str() {
            "--archetype" => &mut archetype,
            "--graph" => &mut graph,
            other => return Err(format!("unknown option `{other}` for rule-pack check\n\n{USAGE}")),
        };
        if slot.replace(value).is_some() {
            return Err(format!("{arg} given twice\n\n{USAGE}"));
        }
    }
    let need = |what: Option<&String>, flag: &str| what.cloned().ok_or_else(|| format!("rule-pack check needs {flag}\n\n{USAGE}"));
    Ok(json!({"archetype": need(archetype, "--archetype")?, "graph": need(graph, "--graph")?}))
}

/// What follows `topology`: the model to read, and the `topology` command's params (`--out` as `path`); each option once.
fn topology_params(args: &[String]) -> Result<(String, Value), String> {
    let (mut model, mut out, mut expect) = (None, None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
        let slot = match arg.as_str() {
            "--model" => &mut model,
            "--out" => &mut out,
            "--expect" => &mut expect,
            other => return Err(format!("unknown option `{other}` for topology\n\n{USAGE}")),
        };
        if slot.replace(value).is_some() {
            return Err(format!("{arg} given twice\n\n{USAGE}"));
        }
    }
    let model = model.cloned().ok_or_else(|| format!("topology needs --model\n\n{USAGE}"))?;
    if out.is_none() && expect.is_none() {
        return Err(format!("topology needs --out or --expect\n\n{USAGE}"));
    }
    let mut params = json!({});
    if let Some(out) = out {
        params["path"] = json!(out);
    }
    if let Some(expect) = expect {
        params["expect"] = json!(expect);
    }
    Ok((model, params))
}

/// What follows `drawing-miner cad`, as the command's params: `--corpus` and `--out`, each once.
fn drawing_miner_params(args: &[String]) -> Result<Value, String> {
    let (mut corpus, mut out) = (None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
        let slot = match arg.as_str() {
            "--corpus" => &mut corpus,
            "--out" => &mut out,
            other => return Err(format!("unknown option `{other}` for drawing-miner cad\n\n{USAGE}")),
        };
        if slot.replace(value).is_some() {
            return Err(format!("{arg} given twice\n\n{USAGE}"));
        }
    }
    let need = |what: Option<&String>, flag: &str| what.cloned().ok_or_else(|| format!("drawing-miner cad needs {flag}\n\n{USAGE}"));
    Ok(json!({"corpus": need(corpus, "--corpus")?, "out": need(out, "--out")?}))
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

/// The DWG writer a session is given: `bimcraft-dwg`'s, which writes a plan as DWG 2018, behind the `dwg` feature. A build
/// without it hands none, and saving a .dwg says to save it as DXF.
#[cfg(feature = "dwg")]
fn dwg_writer() -> Option<DwgWriter> {
    Some(bimcraft_dwg::writer())
}

#[cfg(not(feature = "dwg"))]
fn dwg_writer() -> Option<DwgWriter> {
    None
}

/// What this program does when it's started as the child that reads a DWG (`bimcraft_dwg::CHILD_ARG`): none when it isn't.
#[cfg(feature = "dwg")]
fn dwg_child(args: &[String]) -> Option<ExitCode> {
    bimcraft_dwg::child_main(args)
}

#[cfg(not(feature = "dwg"))]
fn dwg_child(_: &[String]) -> Option<ExitCode> {
    None
}

fn main() -> ExitCode {
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
    fn deliver_flags_become_params_and_a_mistake_is_said() {
        let args = |text: &str| text.split_whitespace().map(String::from).collect::<Vec<_>>();
        let p = deliver_params(&args("--brief b.json --delivery-id d --out r --rerun 5 --jobs 2")).unwrap();
        assert_eq!(p, json!({"brief": "b.json", "delivery_id": "d", "path": "r", "rerun": 5, "jobs": 2}));
        assert_eq!(deliver_params(&args("--brief b --delivery-id d --out r --rights emailed")).unwrap()["rights"], "emailed");
        for bad in [
            "--brief b --delivery-id d",
            "--brief b --brief c --delivery-id d --out r",
            "--brief b --delivery-id d --out r --jobs x",
            "--nope 1",
            "--brief",
        ] {
            assert!(deliver_params(&args(bad)).is_err(), "{bad}");
        }
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
    fn run_builds_the_sample_and_saves_each_format() {
        let base = std::env::temp_dir().join(format!("bimcraft-cli-{}", std::process::id()));
        let path = |ext: &str| base.with_extension(ext).to_string_lossy().into_owned();
        let (project, ifc, png, svg, dxf) = (path("bimcraft"), path("ifc"), path("png"), path("svg"), path("dxf"));
        let roof = r#"level {"elevation": 3200, "name": "Roof"}"#;
        let out = cli_out(&["run", "--sample", "--cmd", roof, "--save", &project, "--save", &ifc, "--save", &png, "--save", &dxf]).unwrap();
        assert!(out.contains("\"Roof\""), "{out}");
        assert!(out.contains(&json!({"path": dxf, "format": "dxf"}).to_string()), "{out}");
        let plan = bimcraft_core::dxf::read(&std::fs::read(&dxf).unwrap()).unwrap();
        assert_eq!(plan.unit, "mm");
        assert!(plan.entities.iter().any(|e| e.layer == "A-WALL"));
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        assert!(std::fs::read_to_string(&ifc).unwrap().contains("IFCWALL("));
        let info: Value = serde_json::from_str(&cli_out(&["info", &project]).unwrap()).unwrap();
        assert_eq!(info["counts"]["room"], 4);
        assert_eq!(info["levels"][2]["name"], "Roof", "levels sort by elevation");
        cli_out(&["convert", &project, &svg]).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg"));
        std::fs::remove_file(&dxf).unwrap();
        cli_out(&["convert", &project, &dxf]).unwrap();
        assert!(bimcraft_core::dxf::read(&std::fs::read(&dxf).unwrap()).unwrap().layer("A-DOOR").is_some());
        for p in [project, ifc, png, svg, dxf] {
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
    fn generate_writes_a_run_directory_whose_script_runs_and_saves() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-generate-{}", std::process::id()));
        let at = dir.to_string_lossy().into_owned();
        let out = cli_out(&["generate", "--archetype", "clinic", "--seed", "3", "--rooms", "10", "--out", &at]).unwrap();
        let reply: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!((reply["archetype"].as_str(), reply["seed"].as_u64()), (Some("clinic"), Some(3)));
        let script = dir.join("script.bimcraft.txt").to_string_lossy().into_owned();
        let (png, dxf) = (dir.join("plan.png").to_string_lossy().into_owned(), dir.join("plan.dxf").to_string_lossy().into_owned());
        cli_out(&["run", "--script", &script, "--save", &png, "--save", &dxf]).unwrap();
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        assert!(bimcraft_core::dxf::read(&std::fs::read(&dxf).unwrap()).unwrap().layer("A-WALL").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn generate_names_what_is_wrong_with_a_brief_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-generate-bad-{}", std::process::id()));
        let at = dir.to_string_lossy().into_owned();
        for (args, says) in [
            (vec!["--archetype", "clinic", "--seed", "1", "--rooms", "10"], "--out"),
            (vec!["--archetype", "clinic", "--seed", "-1", "--rooms", "10", "--out", &at], "--seed"),
            (vec!["--archetype", "house", "--archetype", "clinic", "--seed", "1", "--rooms", "10", "--out", &at], "twice"),
            (vec!["--archetype", "datacenter", "--seed", "1", "--rooms", "10", "--out", &at], "unknown_archetype"),
            (vec!["--colour", "red"], "unknown option"),
            (vec!["--seed"], "needs a value"),
        ] {
            let mut all = vec!["generate"];
            all.extend(args);
            let e = cli_out(&all).unwrap_err();
            assert!(e.contains(says), "{all:?}: {e}");
        }
        assert!(!dir.exists());
    }

    #[test]
    fn generate_takes_gridlines_and_the_core_refuses_other_values() {
        let args = |text: &str| text.split_whitespace().map(String::from).collect::<Vec<_>>();
        let p = generate_params(&args("--archetype house --seed 1 --rooms 10 --gridlines on --out d")).unwrap();
        assert_eq!(p["gridlines"], "on");
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-gridlines-{}", std::process::id()));
        let at = dir.to_string_lossy().into_owned();
        let e = cli_out(&["generate", "--archetype", "house", "--seed", "1", "--rooms", "10", "--gridlines", "maybe", "--out", &at])
            .unwrap_err();
        let refusal: Value = serde_json::from_str(&e).unwrap_or_else(|_| panic!("not JSON: {e}"));
        assert_eq!(refusal["error"], "invalid_params", "{e}");
        assert!(!dir.exists());
    }

    #[test]
    fn generate_takes_a_family_file_or_an_archetype_and_the_core_says_which() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-family-file-{}", std::process::id()));
        let at = dir.to_string_lossy().into_owned();
        let fam = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/core/data/shape-families/house.json");
        let f = fam.to_string_lossy().into_owned();
        let out = cli_out(&["generate", "--family-file", &f, "--seed", "1", "--rooms", "9", "--out", &at]).unwrap();
        let reply: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(reply["archetype"], "house");
        std::fs::remove_dir_all(&dir).unwrap();
        for args in [
            vec!["--archetype", "house", "--family-file", &f, "--seed", "1", "--rooms", "9", "--out", &at],
            vec!["--seed", "1", "--rooms", "9", "--out", &at],
        ] {
            let mut all = vec!["generate"];
            all.extend(args);
            let e = cli_out(&all).unwrap_err();
            let refusal: Value = serde_json::from_str(&e).unwrap();
            assert_eq!(refusal["error"], "invalid_params", "{e}");
        }
        assert!(!dir.exists());
    }

    #[test]
    fn rule_pack_seed_writes_the_library() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-rule-pack-{}", std::process::id()));
        let at = dir.to_string_lossy().into_owned();
        let out = cli_out(&["rule-pack", "seed", "--out", &at]).unwrap();
        let reply: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(reply["packs"].as_array().unwrap().len(), 5);
        let manifest: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("rule-pack-manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["library_id"], "frontier-model-seed-library");
        assert!(dir.join("validation-report.json").is_file() && dir.join("packs").join("airport.json").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rule_pack_seed_names_what_is_wrong_with_its_arguments() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-rule-pack-bad-{}", std::process::id()));
        let at = dir.to_string_lossy().into_owned();
        for (args, says) in [
            (vec!["rule-pack", "seed"], "--out"),
            (vec!["rule-pack", "seed", "--out", &at, "--out", &at], "twice"),
            (vec!["rule-pack", "seed", "--colour", "red"], "unknown option"),
            (vec!["rule-pack", "seed", "--out"], "needs a value"),
            (vec!["rule-pack", "grow", "--out", &at], "USAGE"),
            (vec!["rule-pack"], "USAGE"),
        ] {
            let e = cli_out(&args).unwrap_err();
            assert!(e.contains(says), "{args:?}: {e}");
        }
        assert!(!dir.exists());
    }

    #[test]
    fn rule_pack_check_passes_a_graph_or_exits_with_the_rules_that_failed() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-rule-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
        let room = |id: &str, kind: &str, area: f64| json!({"id": id, "kind": kind, "area_m2": area});
        let door = |a: &str, b: &str| json!({"kind": "door", "from": a, "to": b, "operation": "swing"});
        let mut g = json!({
            "entry": {"room": "living"},
            "nodes": [room("living", "living", 20.0), room("kitchen", "kitchen", 12.0), room("bed", "bedroom", 12.0), room("bath", "bath", 5.0),
                      room("hall", "hall", 4.0)],
            "edges": [door("hall", "living"), door("bed", "hall"), door("bath", "hall"), json!({"kind": "open", "from": "living", "to": "kitchen"})],
        });
        std::fs::write(path("good.json"), g.to_string()).unwrap();
        let out = cli_out(&["rule-pack", "check", "--archetype", "house", "--graph", &path("good.json")]).unwrap();
        let reply: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!((reply["pass"].clone(), reply["archetype"].clone()), (json!(true), json!("house")));
        // A second bedroom reached only through the first.
        g["nodes"].as_array_mut().unwrap().push(room("bed2", "bedroom", 12.0));
        g["edges"].as_array_mut().unwrap().push(door("bed2", "bed"));
        std::fs::write(path("bad.json"), g.to_string()).unwrap();
        let e = cli_out(&["rule-pack", "check", "--archetype", "house", "--graph", &path("bad.json")]).unwrap_err();
        let reply: Value = serde_json::from_str(e.trim()).unwrap();
        assert_eq!(reply["error"], "rule_violation");
        assert!(reply["failed"].as_array().unwrap().contains(&json!("HR-008")), "{reply}");
        // The nine house rules, then one check for each swing rule.
        let checks = reply["checks"].as_array().unwrap();
        let swing = checks.iter().filter(|c| c["id"].as_str().is_some_and(|i| i.starts_with("SW-"))).count();
        assert!(swing >= 1);
        assert_eq!(checks.len(), 13 + swing);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rule_pack_check_names_what_is_wrong_with_its_arguments() {
        for (args, says) in [
            (vec!["rule-pack", "check", "--archetype", "house"], "--graph"),
            (vec!["rule-pack", "check", "--graph", "g.json"], "--archetype"),
            (vec!["rule-pack", "check", "--archetype", "house", "--archetype", "office", "--graph", "g"], "twice"),
            (vec!["rule-pack", "check", "--colour", "red"], "unknown option"),
            (vec!["rule-pack", "check", "--graph"], "needs a value"),
            (vec!["rule-pack", "check", "--archetype", "barn", "--graph", "g.json"], "house"),
            (vec!["rule-pack", "check", "--archetype", "house", "--graph", "/nonexistent/g.json"], "/nonexistent/g.json"),
        ] {
            let e = cli_out(&args).unwrap_err();
            assert!(e.contains(says), "{args:?}: {e}");
        }
    }

    #[test]
    fn topology_writes_the_graph_of_a_saved_model() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-topology-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("m.bimcraft").to_string_lossy().into_owned();
        let out = dir.join("t.json").to_string_lossy().into_owned();
        cli_out(&["run", "--sample", "--save", &model]).unwrap();
        let reply: Value = serde_json::from_str(cli_out(&["topology", "--model", &model, "--out", &out]).unwrap().trim()).unwrap();
        assert!(reply["nodes"].as_u64().unwrap() >= 2, "{reply}");
        let g: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(g["graph_kind"], "extracted");
        assert!(g["nodes"].as_array().unwrap().len() >= 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn topology_expect_agrees_or_names_the_mismatch() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-expect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = |n: &str| dir.join(n).to_string_lossy().into_owned();
        let (model, out, good, bad, both) = (p("m.bimcraft"), p("t.json"), p("good.json"), p("bad.json"), p("both.json"));
        cli_out(&["run", "--sample", "--save", &model]).unwrap();
        cli_out(&["topology", "--model", &model, "--out", &out]).unwrap();
        let g: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        let rooms: Vec<Value> =
            g["nodes"].as_array().unwrap().iter().map(|n| json!({"id": n["id"], "name": n["name"], "kind": "room"})).collect();
        let links: Vec<Value> = g["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| (e["kind"] == "door" || e["kind"] == "open") && e["exterior"] != true)
            .map(|e| json!({"from": e["from"], "to": e["to"], "via": if e["kind"] == "open" { json!("open") } else { e["operation"].clone() }}))
            .collect();
        assert!(!links.is_empty(), "the sample has a door between rooms");
        let entry = g["entry"]["room"].clone();
        let file = |links: &[Value]| json!({"rooms": rooms, "links": links, "entry": {"room": entry}});
        std::fs::write(&good, file(&links).to_string()).unwrap();
        std::fs::write(&bad, file(&links[1..]).to_string()).unwrap();

        // Agreement: no `error` key, and `--expect` alone is accepted.
        let reply: Value = serde_json::from_str(cli_out(&["topology", "--model", &model, "--expect", &good]).unwrap().trim()).unwrap();
        assert!(reply.get("error").is_none() && reply["agree"] == true, "{reply}");
        // A mismatch is an error whose text is the JSON.
        let e = cli_out(&["topology", "--model", &model, "--expect", &bad, "--out", &both]).unwrap_err();
        let m: Value = serde_json::from_str(e.trim()).unwrap();
        assert_eq!(m["error"], "topology_mismatch", "{e}");
        for k in ["missing_rooms", "extra_rooms", "missing_links", "extra_links", "entry", "diagnostics"] {
            assert!(m.get(k).is_some(), "{k}: {e}");
        }
        assert_eq!(m["missing_links"].as_array().unwrap().len(), 1, "{e}");
        // --out is written on a mismatch, and as it is without --expect.
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&both).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn topology_names_what_is_wrong_with_its_arguments() {
        for (args, says) in [
            (vec!["topology", "--model", "a", "--expect", "x", "--expect", "y"], "twice"),
            (vec!["topology", "--model", "a", "--expect"], "needs a value"),
            (vec!["topology", "--out", "o.json"], "--model"),
            (vec!["topology", "--model", "m.bimcraft"], "--out or --expect"),
            (vec!["topology", "--model", "a", "--model", "b", "--out", "o"], "twice"),
            (vec!["topology", "--colour", "red"], "unknown option"),
            (vec!["topology", "--model"], "needs a value"),
        ] {
            let e = cli_out(&args).unwrap_err();
            assert!(e.contains(says), "{args:?}: {e}");
        }
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
