//! The sandbox: the wasmi interpreter, the host functions of ABI 1, and the limits a plugin call runs under.
//!
//! A plugin call gets a fresh instance, its own memory, fuel for about a second of work, a command budget and
//! a reply cap. It sees no files, no clock, no randomness and no network: those are not imported, so they are
//! not there to be called. A trap, an out-of-fuel error, a memory limit or bad JSON is an error of the call;
//! the application and the session stay up.
//!
//! A plugin that runs out of fuel or traps never gets to say where it was, and "all fuel consumed by
//! WebAssembly" names nothing. So every call keeps the last steps the plugin took — each command with its
//! parameters, each log line, `info` — with the fuel left when each began: a stopped call's error names the last,
//! and the plugin log keeps the whole trail, which says what the fuel went on.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wasmi::{Caller, Config, Engine, Error, Extern, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

use bimcraft_core::engine::{self, Session};

use crate::Log;
use crate::manifest::ABI;

/// Fuel of one call: wasmi counts an instruction as about one, so this is a second or so of interpreted work.
pub const FUEL: u64 = 20_000_000;
/// The most memory a guest may have, bytes.
pub const MEMORY: usize = 64 << 20;
/// The most elements a guest's tables may hold, in all: a host keeps room for a call's own tables, and no plugin
/// needs more than the few thousand a function table takes.
pub const TABLE_ELEMENTS: usize = 16_384;
/// The most tables, instances and memories a guest's instance may declare.
pub const TABLES: usize = 4;
pub const INSTANCES: usize = 8;
pub const MEMORIES: usize = 1;
/// The most commands one plugin call may run.
pub const MAX_COMMANDS: usize = 1000;
/// The most a plugin's reply may be, bytes (the bridge's line limit).
pub const MAX_REPLY: usize = 16 << 20;
/// The most one log line may be, characters.
pub const MAX_LOG: usize = 4096;
/// The most real time one call may take, checked at each host command and after the call.
pub const WALL: Duration = Duration::from_secs(5);
/// The most steps of a call the trace keeps: the last ones are the ones an error is about.
const MAX_TRACE: usize = 8;
/// The most of a step's text the trace keeps, characters: a command's parameters can be long.
const MAX_STEP: usize = 160;

/// The exports a plugin module must have.
const EXPORTS: [&str; 3] = ["memory", "bimcraft_alloc", "bimcraft_run"];
/// The host functions ABI 1 provides.
const HOST: [&str; 3] = ["execute", "log", "info"];

/// The commands that touch the disk are marked on the [`bimcraft_core::engine::Command`] itself, next to their
/// parameters, and [`engine::touches_files`] reads the mark and, for a command that only sometimes takes a path,
/// its parameters. The plugin crate keeps no list of its own, so a command added since cannot slip through.
fn needs_files(cmd: &bimcraft_core::engine::Command, params: &Value) -> Option<bimcraft_core::engine::Files> {
    engine::touches_files(cmd, params)
}

/// A compiled module, and the engine it belongs to. One is made per plugin at load and shared by its calls.
pub struct Runtime {
    engine: Engine,
    module: Module,
}

/// What a host function sees of the call: the session the plugin works on, its permissions, how many commands
/// it has run, and the log.
struct HostState {
    /// The session the call works on: in the store while the interpreter runs, and back in the caller's slot after
    /// it, whatever way the call ends ([`Runtime::invoke`]).
    session: Option<Session>,
    plugin: String,
    files: bool,
    calls: usize,
    started: Instant,
    log: std::sync::Arc<Log>,
    limits: StoreLimits,
    /// The last things the plugin did, for the error and the log of a call the sandbox stops: a plugin that
    /// runs out of fuel or traps never gets to say where it was.
    trace: Vec<Step>,
}

/// What a plugin did in a call: the host function it called, with its arguments, and the fuel that was left
/// when it called it. Two of them read together say what the first one used.
#[derive(Clone, Debug)]
struct Step {
    /// `execute elements {"category":"wall"}`, `log …` or `info`.
    text: String,
    /// Fuel left when the step began.
    fuel: u64,
}

/// Why a call failed: what to say, and whether the sandbox stopped it before the plugin could say anything
/// itself, which is when the steps it took are worth showing.
struct Failed {
    why: String,
    stopped: bool,
}

impl Failed {
    /// The plugin's own error: it said why, so where it was adds nothing.
    fn said(why: impl Into<String>) -> Failed {
        Failed { why: why.into(), stopped: false }
    }

    /// The sandbox's: a trap, the fuel, the memory or the wall clock. The plugin never got to say where it was.
    fn halted(why: impl Into<String>) -> Failed {
        Failed { why: why.into(), stopped: true }
    }
}

/// Keeps the last steps of a call, dropping the oldest, with a step's text cut so a long parameter list can't
/// fill the log.
fn remember(state: &mut HostState, text: String, fuel: u64) {
    if state.trace.len() == MAX_TRACE {
        state.trace.remove(0);
    }
    let text = if text.chars().count() <= MAX_STEP {
        text
    } else {
        let mut short: String = text.chars().take(MAX_STEP).collect();
        short.push('…');
        short
    };
    state.trace.push(Step { text, fuel });
}

/// The steps of a failed call as one log line: how much of the call's fuel went, and what each of the last
/// steps left.
fn trail(trace: &[Step], calls: usize, left: u64) -> String {
    let mut text = format!(
        "the call ran {calls} command(s) and used {:.1} of {:.1} M fuel; the last steps, and the fuel left when each ran:",
        FUEL.saturating_sub(left) as f64 / 1e6,
        FUEL as f64 / 1e6
    );
    for step in trace {
        text.push_str(&format!(" `{}` {:.1} M,", step.text, step.fuel as f64 / 1e6));
    }
    text.pop();
    text
}

impl Runtime {
    /// Compiles `bytes`. The module must export the ABI's memory, allocation and run functions, and may import
    /// only the host's three functions: anything else is refused before it can run.
    pub fn new(bytes: &[u8]) -> Result<Runtime, String> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, bytes).map_err(|e| format!("the module doesn't load: {e}"))?;
        for name in EXPORTS {
            if module.get_export(name).is_none() {
                return Err(format!("the module doesn't export `{name}`: build it with the host ABI {ABI} SDK"));
            }
        }
        if !matches!(module.get_export("memory"), Some(wasmi::ExternType::Memory(_))) {
            return Err("the module's `memory` export is not a linear memory".to_string());
        }
        for import in module.imports() {
            if import.module() != "bimcraft" || !HOST.contains(&import.name()) {
                return Err(format!(
                    "the module imports `{}` `{}`; host ABI {ABI} has only {}",
                    import.module(),
                    import.name(),
                    HOST.iter().map(|h| format!("`bimcraft.{h}`")).collect::<Vec<_>>().join(", ")
                ));
            }
            if !matches!(import.ty(), wasmi::ExternType::Func(_)) {
                return Err(format!("`bimcraft.{}` is imported as something other than a function", import.name()));
            }
        }
        Ok(Runtime { engine, module })
    }

    /// Runs one function of the plugin on `session`, in the transaction its caller opened. The result is the
    /// JSON the guest returned; any trap, limit or malformed reply is the call's error.
    pub fn invoke(
        &self,
        session: &mut Session,
        plugin: &str,
        function: &str,
        files: bool,
        params: &Value,
        log: &std::sync::Arc<Log>,
    ) -> Result<Value, String> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(MEMORY)
            .table_elements(TABLE_ELEMENTS)
            .tables(TABLES)
            .instances(INSTANCES)
            .memories(MEMORIES)
            .trap_on_grow_failure(true)
            .build();
        let state = HostState {
            session: None,
            plugin: plugin.to_string(),
            files,
            calls: 0,
            started: Instant::now(),
            log: std::sync::Arc::clone(log),
            limits,
            trace: Vec::new(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s: &mut HostState| &mut s.limits);
        if let Err(e) = store.set_fuel(FUEL) {
            return Err(format!("plugin `{plugin}`: couldn't set the fuel: {e}"));
        }
        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        if let Err(e) = install(&mut linker) {
            return Err(format!("plugin `{plugin}`: {e}"));
        }
        // The session is in the store only while the interpreter runs, and a guard takes it out again on every way
        // out — the end of the call, an early return, or a panic the interpreter raises: the caller's slot gets the
        // session back, with its path and the registry of the plugins' commands, whatever ended the call.
        let mut guard = SessionGuard { slot: session, store: &mut store };
        guard.take_in();
        let mut outcome = self.run(guard.store, &linker, function, params);
        if outcome.is_ok() && guard.store.data().started.elapsed() > WALL {
            outcome = Err(Failed::halted(format!("the call took more than {} s", WALL.as_secs())));
        }
        let left = guard.store.get_fuel().unwrap_or(0);
        let (trace, calls, call_log) = {
            let HostState { trace, calls, log, .. } = guard.store.data();
            (trace.clone(), *calls, std::sync::Arc::clone(log))
        };
        guard.give_back();
        outcome.map_err(|failed| {
            // The log keeps what the sandbox's error can't: the steps the plugin took and what each cost, which
            // is where a call that runs out of fuel or traps actually went. A plugin's own error said its why.
            if failed.stopped && !trace.is_empty() {
                call_log.note(plugin, crate::Level::Error, trail(&trace, calls, left));
            }
            let mut why = format!("plugin `{plugin}`, function `{function}`: {}", failed.why);
            if failed.stopped
                && let Some(last) = trace.last()
            {
                why.push_str(&format!("; it stopped at `{}`", last.text));
            }
            why
        })
    }

    fn run(&self, store: &mut Store<HostState>, linker: &Linker<HostState>, function: &str, params: &Value) -> Result<Value, Failed> {
        let instance =
            linker.instantiate_and_start(&mut *store, &self.module).map_err(|e| Failed::halted(format!("it didn't start: {e}")))?;
        // A module may declare more memory than the limit asks for: the limit is on the instance, not only on growth.
        let memory = instance
            .get_export(&mut *store, "memory")
            .and_then(Extern::into_memory)
            .ok_or_else(|| Failed::halted("it has no exported memory"))?;
        if memory.data(&*store).len() > MEMORY {
            return Err(Failed::halted(format!("it asks for more than {} MB of memory", MEMORY >> 20)));
        }
        // A table the limiter doesn't measure at declaration is measured here: one element a page.
        let mut elements = 0usize;
        for name in instance.exports(&*store).map(|e| e.name().to_string()).collect::<Vec<_>>() {
            let Some(table) = instance.get_export(&mut *store, &name).and_then(Extern::into_table) else { continue };
            elements += table.size(&*store) as usize;
        }
        if elements > TABLE_ELEMENTS {
            return Err(Failed::halted(format!("its tables hold {elements} elements, more than the call's {TABLE_ELEMENTS}")));
        }
        let input = serde_json::to_string(params).map_err(|e| Failed::halted(format!("the parameters aren't JSON: {e}")))?;
        if input.len() > MAX_REPLY {
            return Err(Failed::halted(format!("the parameters are over {} MB", MAX_REPLY >> 20)));
        }
        let (name_ptr, name_len) = write_guest(store, instance, function.as_bytes()).map_err(Failed::halted)?;
        let (params_ptr, params_len) = write_guest(store, instance, input.as_bytes()).map_err(Failed::halted)?;
        let run = instance
            .get_typed_func::<(i32, i32, i32, i32), i64>(&mut *store, "bimcraft_run")
            .map_err(|e| Failed::halted(format!("`bimcraft_run` has the wrong signature: {e}")))?;
        let packed =
            run.call(&mut *store, (name_ptr, name_len, params_ptr, params_len)).map_err(|e| Failed::halted(format!("it failed: {e}")))?;
        let (out_ptr, out_len) = unpack(packed).map_err(Failed::halted)?;
        let reply = read_guest(store, instance, out_ptr, out_len).map_err(Failed::halted)?;
        // The buffers are the guest's; give them back when it offers a free.
        free_guest(store, instance, name_ptr, name_len);
        free_guest(store, instance, params_ptr, params_len);
        free_guest(store, instance, out_ptr, out_len);
        let value: Value = serde_json::from_slice(&reply).map_err(|e| Failed::halted(format!("its reply isn't JSON: {e}")))?;
        // A handler's error, as the SDK writes it: the call failed, so the transaction takes everything back.
        if let Some(e) = value.get("bimcraftError").and_then(Value::as_str) {
            return Err(Failed::said(e));
        }
        Ok(value)
    }
}

/// The caller's slot and the store of one call: the session is moved between them, and put back however the call
/// ends — the guard's `Drop` runs on a panic too, which `Store::into_data` alone would not.
struct SessionGuard<'a> {
    slot: &'a mut Session,
    store: &'a mut Store<HostState>,
}

impl SessionGuard<'_> {
    /// Moves the caller's session into the store, for the host functions that run on it.
    fn take_in(&mut self) {
        self.store.data_mut().session = Some(std::mem::replace(self.slot, Session::new()));
    }

    /// Moves it back: whatever the call did, the caller's session is the one it had, with its path and its commands.
    fn give_back(&mut self) {
        if let Some(host) = self.store.data_mut().session.take() {
            *self.slot = host;
        }
    }
}

impl Drop for SessionGuard<'_> {
    fn drop(&mut self) {
        self.give_back();
    }
}

fn install(linker: &mut Linker<HostState>) -> Result<(), String> {
    linker.func_wrap("bimcraft", "execute", host_execute).map_err(|e| format!("bimcraft.execute: {e}"))?;
    linker.func_wrap("bimcraft", "log", host_log).map_err(|e| format!("bimcraft.log: {e}"))?;
    linker.func_wrap("bimcraft", "info", host_info).map_err(|e| format!("bimcraft.info: {e}"))?;
    Ok(())
}

fn host_execute(mut caller: Caller<'_, HostState>, ptr: i32, len: i32) -> Result<i64, Error> {
    let text = read_guest_caller(&mut caller, ptr, len, MAX_REPLY)?;
    let reply = match request(&mut caller, &text) {
        Ok(value) => json!({"ok": true, "result": value}),
        Err(e) => json!({"ok": false, "error": e}),
    };
    let text = serde_json::to_string(&reply).map_err(|e| Error::new(format!("the reply isn't JSON: {e}")))?;
    allocate_reply(&mut caller, &text)
}

/// The request a plugin makes of `bimcraft.execute`: `{"command": "...", "params": {...}}`.
fn request(caller: &mut Caller<'_, HostState>, text: &str) -> Result<Value, String> {
    let request: Value = serde_json::from_str(text).map_err(|e| format!("`execute` takes JSON: {e}"))?;
    let command = request.get("command").and_then(Value::as_str).ok_or("`execute` needs `command`, a string")?.to_string();
    let params = match request.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(p @ Value::Object(_)) => p.clone(),
        Some(_) => return Err("`params` is an object".to_string()),
    };
    let fuel = caller.get_fuel().unwrap_or(0);
    let state = caller.data_mut();
    remember(state, format!("{command} {params}"), fuel);
    if state.started.elapsed() > WALL {
        state.log.note(&state.plugin, crate::Level::Error, format!("`{command}` refused: the call is over {} s", WALL.as_secs()));
        return Err(format!("the call may take {} s of real time", WALL.as_secs()));
    }
    if state.calls >= MAX_COMMANDS {
        state.log.note(&state.plugin, crate::Level::Error, format!("`{command}` refused: the call's {MAX_COMMANDS} commands are used"));
        return Err(format!("the call may run {MAX_COMMANDS} commands, and it has"));
    }
    let Some(cmd) = engine::find(&command) else {
        let why = match state.session.as_ref().and_then(|s| s.external_command(&command)) {
            Some(_) => format!("a plugin may call BIMCraft's commands, not another plugin's (`{command}`)"),
            None => format!("unknown command `{command}`"),
        };
        state.log.note(&state.plugin, crate::Level::Error, why.clone());
        return Err(why);
    };
    let touched = needs_files(cmd, &params);
    if touched.is_some() && !state.files {
        let what = match touched {
            Some(bimcraft_core::engine::Files::Read) => "read files",
            Some(bimcraft_core::engine::Files::Write) => "write files",
            _ => "use files",
        };
        let why = format!("`{}` needs to {what}, and the plugin isn't given the files permission", cmd.id);
        state.log.note(&state.plugin, crate::Level::Warning, why.clone());
        return Err(why);
    }
    state.calls += 1;
    let Some(session) = state.session.as_mut() else {
        return Err("the call has no session to run the command on".to_string());
    };
    session.execute(cmd.id, &params)
}

fn host_log(mut caller: Caller<'_, HostState>, level: i32, ptr: i32, len: i32) -> Result<(), Error> {
    let text = read_guest_caller(&mut caller, ptr, len, MAX_LOG)?;
    let text = text.chars().take(MAX_LOG).collect::<String>();
    let fuel = caller.get_fuel().unwrap_or(0);
    let state = caller.data_mut();
    let level = match level {
        0 => crate::Level::Info,
        1 => crate::Level::Warning,
        _ => crate::Level::Error,
    };
    remember(state, format!("log {text}"), fuel);
    state.log.note(&state.plugin, level, text);
    Ok(())
}

fn host_info(mut caller: Caller<'_, HostState>) -> Result<i64, Error> {
    let fuel = caller.get_fuel().unwrap_or(0);
    remember(caller.data_mut(), "info".to_string(), fuel);
    let info = {
        let state = caller.data();
        json!({
            "app": "bimcraft",
            "version": env!("CARGO_PKG_VERSION"),
            "abi": ABI,
            "units": "mm",
            "project": state.session.as_ref().map(|s| s.model.name.clone()).unwrap_or_default(),
        })
    };
    let text = serde_json::to_string(&info).map_err(|e| Error::new(format!("the host info isn't JSON: {e}")))?;
    allocate_reply(&mut caller, &text)
}

fn allocate_reply(caller: &mut Caller<'_, HostState>, text: &str) -> Result<i64, Error> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_REPLY {
        return Err(Error::new(format!("a reply is up to {} MB", MAX_REPLY >> 20)));
    }
    let (ptr, len) = allocate(caller, bytes.len())?;
    if len > 0 {
        let memory = memory_of(caller)?;
        memory.write(&mut *caller, usize::try_from(ptr).map_err(|_| Error::new("a bad pointer"))?, bytes)?;
    }
    Ok(pack(ptr, len))
}

fn memory_of(caller: &mut Caller<'_, HostState>) -> Result<wasmi::Memory, Error> {
    caller.get_export("memory").and_then(Extern::into_memory).ok_or_else(|| Error::new("the plugin has no exported memory"))
}

fn allocate(caller: &mut Caller<'_, HostState>, len: usize) -> Result<(i32, i32), Error> {
    let len = i32::try_from(len).map_err(|_| Error::new("a buffer is too long"))?;
    if len == 0 {
        return Ok((0, 0));
    }
    let alloc = caller
        .get_export("bimcraft_alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| Error::new("the plugin doesn't export `bimcraft_alloc`"))?;
    let alloc = alloc.typed::<i32, i32>(&*caller)?;
    let ptr = alloc.call(&mut *caller, len)?;
    if ptr <= 0 {
        return Err(Error::new("the plugin couldn't allocate a buffer"));
    }
    Ok((ptr, len))
}

fn write_guest(store: &mut Store<HostState>, instance: wasmi::Instance, bytes: &[u8]) -> Result<(i32, i32), String> {
    if bytes.is_empty() {
        return Ok((0, 0));
    }
    let len = i32::try_from(bytes.len()).map_err(|_| "the input is too long".to_string())?;
    let alloc = instance
        .get_export(&*store, "bimcraft_alloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| "the plugin doesn't export `bimcraft_alloc`".to_string())?
        .typed::<i32, i32>(&*store)
        .map_err(|e| format!("`bimcraft_alloc` has the wrong signature: {e}"))?;
    let ptr = alloc.call(&mut *store, len).map_err(|e| format!("it couldn't allocate: {e}"))?;
    if ptr <= 0 {
        return Err("the plugin couldn't allocate a buffer".to_string());
    }
    let memory = instance
        .get_export(&*store, "memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| "the plugin has no exported memory".to_string())?;
    memory
        .write(&mut *store, usize::try_from(ptr).map_err(|_| "a bad pointer".to_string())?, bytes)
        .map_err(|e| format!("it couldn't be written: {e}"))?;
    Ok((ptr, len))
}

fn read_guest(store: &mut Store<HostState>, instance: wasmi::Instance, ptr: i32, len: i32) -> Result<Vec<u8>, String> {
    let len = usize::try_from(len).map_err(|_| "a bad length".to_string())?;
    if len == 0 {
        return Ok(Vec::new());
    }
    if len > MAX_REPLY {
        return Err(format!("its reply is over {} MB", MAX_REPLY >> 20));
    }
    let memory = instance
        .get_export(&*store, "memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| "the plugin has no exported memory".to_string())?;
    let mut buffer = vec![0; len];
    memory
        .read(&*store, usize::try_from(ptr).map_err(|_| "a bad pointer".to_string())?, &mut buffer)
        .map_err(|e| format!("its reply couldn't be read: {e}"))?;
    Ok(buffer)
}

fn read_guest_caller(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32, cap: usize) -> Result<String, Error> {
    let len = usize::try_from(len).map_err(|_| Error::new("a bad length"))?;
    if len > cap {
        return Err(Error::new(format!("a buffer is over {cap} bytes")));
    }
    if len == 0 {
        return Ok(String::new());
    }
    let memory = memory_of(caller)?;
    let mut buffer = vec![0; len];
    memory.read(&*caller, usize::try_from(ptr).map_err(|_| Error::new("a bad pointer"))?, &mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn free_guest(store: &mut Store<HostState>, instance: wasmi::Instance, ptr: i32, len: i32) {
    if ptr == 0 || len == 0 {
        return;
    }
    let Some(free) = instance.get_export(&*store, "bimcraft_free").and_then(Extern::into_func) else { return };
    let Ok(free) = free.typed::<(i32, i32), ()>(&*store) else { return };
    let _ = free.call(&mut *store, (ptr, len));
}

fn pack(ptr: i32, len: i32) -> i64 {
    (i64::from(ptr) << 32) | i64::from(len)
}

fn unpack(packed: i64) -> Result<(i32, i32), String> {
    let ptr = i32::try_from(packed >> 32).map_err(|_| "a bad reply pointer".to_string())?;
    let len = i32::try_from(packed & 0xffff_ffff).map_err(|_| "a bad reply length".to_string())?;
    if len < 0 || ptr < 0 {
        return Err("a bad reply pointer".to_string());
    }
    Ok((ptr, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `id` called with `params` reaches the disk, by the command's own mark in the core.
    fn reaches(id: &str, params: Value) -> bool {
        let command = engine::find(id).unwrap_or_else(|| panic!("there's no command `{id}`"));
        needs_files(command, &params).is_some()
    }

    #[test]
    fn only_the_calls_that_touch_the_disk_ask_for_the_files_permission() {
        assert!(!reaches("render", json!({})));
        assert!(reaches("render", json!({"path": "plan.png"})));
        assert!(!reaches("family", json!({"name": "Desk"})));
        assert!(reaches("family", json!({"path": "desk.json"})));
        assert!(reaches("family", json!({"name": "Desk", "save": "desk.json"})));
        assert!(!reaches("favorite", json!({"action": "list"})));
        assert!(reaches("favorite", json!({"action": "save"})));
        assert!(reaches("save", json!({"path": "x.bimcraft"})));
        assert!(!reaches("level", json!({"elevation": 0})));
    }

    /// The commands the hand-kept list used to miss: a new command is now covered by its mark in the core.
    #[test]
    fn the_commands_the_old_list_missed_are_covered_by_their_mark() {
        for id in ["schedule", "export_crosscheck", "export_fill_patterns"] {
            assert!(reaches(id, json!({"path": "out.bin"})), "`{id}` writes a file");
        }
        for id in ["import_fill_patterns", "compare_crosscheck"] {
            assert!(reaches(id, json!({"path": "in.txt"})), "`{id}` reads a file");
        }
        assert!(reaches("material", json!({"texture": {"kind": "image", "path": "oak.png"}})), "a texture reads a picture");
        assert!(!reaches("material", json!({"name": "Oak"})), "a material without a picture touches no file");
    }
}
