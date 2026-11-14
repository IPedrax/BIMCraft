//! Printing: File > Export… with a printer for its destination writes the PDF the page setup asks for, as an export to a
//! file does, then hands it to the system's spooler. On Linux and macOS that is CUPS (`lpstat` lists the printers, `lp`
//! prints); on Windows, the PDF reader's PrintTo through PowerShell. The spooler is a trait so the tests print to one that
//! only remembers what it was given.

use std::path::Path;
use std::process::Command;
#[cfg(test)]
use std::sync::Mutex;

/// What prints.
pub(crate) trait Spooler: Send + Sync {
    /// The printers there are, the default first.
    fn printers(&self) -> Result<Vec<String>, String>;
    /// Prints the PDF `file` on `printer`, `copies` times, on `media` (a paper's name the spooler knows) if given.
    fn print(&self, printer: &str, file: &Path, copies: u32, media: Option<&str>, title: &str) -> Result<(), String>;
}

/// The paper's name as CUPS knows it, for the papers it surely does; none, the printer's own choice by the page's size.
pub(crate) fn media(paper: Option<&str>) -> Option<&'static str> {
    Some(match paper?.trim() {
        "A0" => "A0",
        "A1" => "A1",
        "A2" => "A2",
        "A3" => "A3",
        "A4" => "A4",
        "Letter" => "Letter",
        "Legal" => "Legal",
        "Tabloid" => "Tabloid",
        _ => return None,
    })
}

/// A program's output, or why it couldn't be had.
fn output(command: &mut Command) -> Result<String, String> {
    let name = command.get_program().to_string_lossy().into_owned();
    let out = command.output().map_err(|e| format!("{name}: {e}"))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() { format!("{name} failed ({})", out.status) } else { why });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The system's spooler.
pub(crate) struct System;

#[cfg(not(windows))]
impl Spooler for System {
    fn printers(&self) -> Result<Vec<String>, String> {
        let mut names: Vec<String> =
            output(Command::new("lpstat").arg("-e"))?.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
        // "system default destination: NAME"
        let default = output(Command::new("lpstat").arg("-d"))
            .ok()
            .and_then(|s| s.split_once(':').map(|(_, n)| n.trim().to_string()))
            .filter(|n| !n.is_empty());
        if let Some(d) = default
            && let Some(i) = names.iter().position(|n| *n == d)
        {
            let d = names.remove(i);
            names.insert(0, d);
        }
        Ok(names)
    }

    fn print(&self, printer: &str, file: &Path, copies: u32, media: Option<&str>, title: &str) -> Result<(), String> {
        let mut lp = Command::new("lp");
        lp.arg("-d").arg(printer).arg("-n").arg(copies.clamp(1, 999).to_string()).arg("-t").arg(title);
        if let Some(media) = media {
            lp.arg("-o").arg(format!("media={media}"));
        }
        output(lp.arg("--").arg(file)).map(|_| ())
    }
}

#[cfg(windows)]
impl Spooler for System {
    fn printers(&self) -> Result<Vec<String>, String> {
        let names = output(Command::new("powershell").args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_Printer | Sort-Object -Property Default -Descending).Name",
        ]))?;
        Ok(names.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    fn print(&self, printer: &str, file: &Path, copies: u32, _: Option<&str>, _: &str) -> Result<(), String> {
        // The PDF reader the system has prints it: PrintTo names the printer.
        let quote = |s: &str| s.replace('\'', "''");
        let script = format!(
            "Start-Process -FilePath '{}' -Verb PrintTo -ArgumentList '\"{}\"' -Wait",
            quote(&file.to_string_lossy()),
            quote(printer)
        );
        for _ in 0..copies.clamp(1, 99) {
            output(Command::new("powershell").args(["-NoProfile", "-Command", &script]))?;
        }
        Ok(())
    }
}

/// A job a [`Fake`] was given: the printer, the copies, the paper's name, and the file's first bytes.
#[cfg(test)]
pub(crate) type Printed = (String, u32, Option<String>, Vec<u8>);

/// A spooler that prints nothing and remembers what it was given: its printers, then each job's printer, copies, media
/// and the file's first bytes.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Fake {
    pub(crate) names: Vec<String>,
    pub(crate) jobs: Mutex<Vec<Printed>>,
}

#[cfg(test)]
impl Spooler for Fake {
    fn printers(&self) -> Result<Vec<String>, String> {
        Ok(self.names.clone())
    }

    fn print(&self, printer: &str, file: &Path, copies: u32, media: Option<&str>, _: &str) -> Result<(), String> {
        let head = std::fs::read(file).map_err(|e| e.to_string())?.into_iter().take(8).collect();
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.push((printer.to_string(), copies, media.map(String::from), head));
        }
        Ok(())
    }
}
