//! The guest SDK of BIMCraft's WASM plugins (host ABI 1): typed calls to the host, the exported entry points a plugin
//! module needs, and a macro that turns safe Rust handlers into the module's functions.
//!
//! A plugin crate is a `cdylib` for `wasm32-unknown-unknown`:
//!
//! ```ignore
//! use serde_json::{Value, json};
//!
//! fn summary(_: &Value) -> Result<Value, String> {
//!     let rooms = bimcraft_plugin_sdk::execute("elements", &json!({"category": "room"}))?;
//!     Ok(json!({"rooms": rooms["count"]}))
//! }
//!
//! bimcraft_plugin_sdk::export! { "summary" => summary }
//! ```
//!
//! The host calls `bimcraft_run` with the function's name and its parameters as JSON, both in the guest's linear memory.
//! What the handler returns is the call's JSON; an `Err` comes back to the caller as a failed call and, inside the
//! plugin's transaction, as nothing changed. `execute` is the only way to the model: the plugin has no files, no clock,
//! no randomness and no network, because the host doesn't import them.
//!
//! The module is the first version of the contract ([`ABI`]); a manifest whose `abi` isn't this one is refused at load.

use serde_json::json;

/// The host ABI this SDK speaks: the manifest's `abi` must be this number.
pub const ABI: u32 = 1;

/// The level of a log line: an information, a warning or an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl Level {
    #[cfg(target_arch = "wasm32")]
    fn code(self) -> i32 {
        match self {
            Level::Info => 0,
            Level::Warning => 1,
            Level::Error => 2,
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "bimcraft")]
unsafe extern "C" {
    #[link_name = "execute"]
    fn host_execute(ptr: u32, len: u32) -> i64;
    #[link_name = "log"]
    fn host_log(level: i32, ptr: u32, len: u32);
    #[link_name = "info"]
    fn host_info_raw() -> i64;
}

/// Runs a BIMCraft command with JSON parameters and gives its result, exactly as `Session::execute` would: the model's
/// validation, the rollback of a failure and the plugin's undo step come with it. A command the plugin may not run
/// (a file command without the `files` permission, another plugin's command) is an error here.
#[cfg(target_arch = "wasm32")]
pub fn execute(command: &str, params: &Value) -> Result<Value, String> {
    let request =
        serde_json::to_string(&json!({"command": command, "params": params})).map_err(|e| format!("the request isn't JSON: {e}"))?;
    let packed = unsafe { host_execute(request.as_ptr() as u32, request.len() as u32) };
    let reply = take(packed)?;
    let envelope: Value = serde_json::from_slice(&reply).map_err(|e| format!("the host's reply isn't JSON: {e}"))?;
    if envelope["ok"].as_bool() == Some(true) {
        Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(envelope["error"].as_str().unwrap_or("the command failed").to_string())
    }
}

/// A build for the host (a test, a `cargo check`): there is no host to call.
#[cfg(not(target_arch = "wasm32"))]
pub fn execute(_: &str, _: &Value) -> Result<Value, String> {
    Err("a plugin runs inside WebAssembly; build it for wasm32-unknown-unknown".to_string())
}

/// Writes a line to the plugin's log, which the Plugins window and the command line show.
pub fn note(level: Level, text: &str) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        host_log(level.code(), text.as_ptr() as u32, text.len() as u32);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (level, text);
    }
}

pub fn info_log(text: &str) {
    note(Level::Info, text);
}

pub fn warn(text: &str) {
    note(Level::Warning, text);
}

pub fn error(text: &str) {
    note(Level::Error, text);
}

/// What the host is: the app's version, the ABI, the units (`mm`) and the project's name. Read only.
#[cfg(target_arch = "wasm32")]
pub fn host_info() -> Result<Value, String> {
    let packed = unsafe { host_info_raw() };
    let info = take(packed)?;
    serde_json::from_slice(&info).map_err(|e| format!("the host's info isn't JSON: {e}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn host_info() -> Result<Value, String> {
    Err("a plugin runs inside WebAssembly; build it for wasm32-unknown-unknown".to_string())
}

#[cfg(target_arch = "wasm32")]
fn take(packed: i64) -> Result<Vec<u8>, String> {
    let ptr = (packed >> 32) as u32;
    let len = (packed & 0xffff_ffff) as u32;
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr == 0 {
        return Err("the host returned no buffer".to_string());
    }
    // SAFETY: the host wrote `len` bytes at `ptr` in this module's linear memory, which is the ABI's contract, and
    // `free` gives it back at once.
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }.to_vec();
    free(ptr, len);
    Ok(bytes)
}

/// The memory the host writes its inputs into, and reads the replies from. The ABI's allocation contract: a block of
/// exactly `len` bytes, which [`free`] takes back.
#[doc(hidden)]
pub fn alloc(len: u32) -> u32 {
    if len == 0 {
        return 0;
    }
    let mut buffer = Vec::<u8>::with_capacity(len as usize);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr as u32
}

/// Gives back a block [`alloc`] made.
#[doc(hidden)]
pub fn free(ptr: u32, len: u32) {
    if ptr == 0 || len == 0 {
        return;
    }
    // SAFETY: the pointer and length are `alloc`'s, whose capacity is `len` and whose contents are bytes; the vector is
    // dropped, never read.
    unsafe { drop(Vec::from_raw_parts(ptr as *mut u8, 0, len as usize)) };
}

/// The body of `bimcraft_run`: reads the name and parameters the host put in memory, calls the handler, and returns its
/// reply in a fresh block. A handler's `Err` becomes `{"bimcraftError": "..."}`, which the host turns back into a failed
/// call and rolls the whole plugin call back.
#[doc(hidden)]
pub fn run(name_ptr: u32, name_len: u32, params_ptr: u32, params_len: u32, handler: impl Fn(&str, &Value) -> Result<Value, String>) -> i64 {
    let name = read(name_ptr, name_len);
    let params = read(params_ptr, params_len);
    let params: Value = serde_json::from_slice(params.as_bytes()).unwrap_or(Value::Null);
    let value = match handler(&name, &params) {
        Ok(v) => v,
        Err(e) => json!({"bimcraftError": e}),
    };
    let text = serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"bimcraftError\":\"the reply isn't JSON\"}".to_vec());
    write(&text)
}

fn read(ptr: u32, len: u32) -> String {
    if ptr == 0 || len == 0 {
        return String::new();
    }
    // SAFETY: the host wrote `len` bytes at `ptr`, as the ABI says.
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn write(bytes: &[u8]) -> i64 {
    let ptr = alloc(bytes.len() as u32);
    if ptr != 0 && !bytes.is_empty() {
        // SAFETY: `alloc` made a block of exactly `bytes.len()` bytes in this module's memory, and nothing else holds it.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()) };
    }
    (i64::from(ptr) << 32) | i64::from(bytes.len() as u32)
}

/// Declares the functions a plugin brings and the Rust handlers that run them, and exports the entry points the host
/// looks for: `bimcraft_run`, `bimcraft_alloc` and `bimcraft_free`.
///
/// ```ignore
/// bimcraft_plugin_sdk::export! {
///     "summary" => summary,
///     "write_note" => write_note,
/// }
/// ```
#[macro_export]
macro_rules! export {
    ($($name:literal => $handler:path),* $(,)?) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn bimcraft_alloc(len: u32) -> u32 {
            $crate::alloc(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn bimcraft_free(ptr: u32, len: u32) {
            $crate::free(ptr, len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn bimcraft_run(name_ptr: u32, name_len: u32, params_ptr: u32, params_len: u32) -> i64 {
            $crate::run(name_ptr, name_len, params_ptr, params_len, |name: &str, params: &$crate::Value| {
                match name {
                    $($name => $handler(params),)*
                    other => Err(format!("the plugin has no function `{other}`")),
                }
            })
        }
    };
}

/// The `Value` the macro's dispatcher speaks: re-exported so a plugin needs only this crate and `serde_json`.
#[doc(hidden)]
pub use serde_json::Value;
