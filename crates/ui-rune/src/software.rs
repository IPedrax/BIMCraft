//! A machine with no graphics card. Rune draws a window on its CPU renderer first and hands it to the GPU once there is one, which on
//! a software adapter (lavapipe, WARP) costs as much as egui does (docs/areas/rune-ui.md); `RUNE_CPU_FIRST=only` keeps it on the CPU,
//! where only the tiles that changed are painted. Rune reads that from the environment, and a program cannot set its own without
//! `unsafe` (`std::env::set_var` is), which this crate forbids: so `--software`, or `BIMCRAFT_SOFTWARE=1` as the egui app has it since
//! issue 628, starts the program again with the variable set. Rune's own `--snap` and `--script` runs are left as they are.

use std::convert::Infallible;
use std::ffi::OsString;
use std::process::Command;

pub const FLAG: &str = "--software";
const VARIABLE: &str = "RUNE_CPU_FIRST";

/// Whether this start should be made again on the CPU renderer: it was asked for (the flag, or `BIMCRAFT_SOFTWARE=1`) and the
/// variable is not set already, which is how a second start knows it is the second.
pub fn wanted(args: &[OsString], software: Option<&str>, cpu_first: Option<&str>) -> bool {
    (args.iter().any(|a| a == FLAG) || software == Some("1")) && cpu_first.is_none()
}

#[cfg(unix)]
fn start(mut command: Command) -> Result<Infallible, String> {
    use std::os::unix::process::CommandExt;
    // The process is replaced: it returns only when it could not be.
    Err(command.exec().to_string())
}

#[cfg(not(unix))]
fn start(mut command: Command) -> Result<Infallible, String> {
    let status = command.status().map_err(|e| e.to_string())?;
    std::process::exit(status.code().unwrap_or(1))
}

/// Starts this program again with the same arguments and `RUNE_CPU_FIRST=only`. It returns only to say why it could not.
pub fn relaunch(args: &[OsString]) -> Result<Infallible, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command.args(args).env(VARIABLE, "only");
    start(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_flag_or_the_variable_asks_for_the_cpu_renderer_once() {
        assert!(wanted(&args(&["--software"]), None, None));
        assert!(wanted(&args(&[]), Some("1"), None));
        assert!(wanted(&args(&["--sample", "--software"]), None, None));
        assert!(!wanted(&args(&["--sample"]), None, None), "nothing asked");
        assert!(!wanted(&args(&[]), Some("0"), None), "only 1 asks");
        assert!(!wanted(&args(&["--software"]), Some("1"), Some("only")), "the second start does not start a third");
        assert!(!wanted(&args(&["--software"]), None, Some("0")), "whoever set the variable has chosen");
    }
}
