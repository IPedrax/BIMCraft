//! DWG drawings, as the DXF text the core reads.
//!
//! DWG has no permissively licensed reader of BIMCraft's own, so this crate is a bridge: the `acadrust` crate (MPL-2.0, used
//! unmodified as a separate library; its DWG code is ported from ACadSharp, MIT, and follows the Open Design Alliance's
//! published DWG specification) reads the file, and its DXF writer writes what it read as ASCII DXF, which `bimcraft-core`'s
//! `dxf.rs` reads as it reads any other. Nothing outside this crate sees `acadrust`'s types, and the core never depends on
//! it: the apps hand a `Session` a DWG reader ([`reader`], the core's `cad::DwgReader`), as the window is handed the Revit reader.
//!
//! A file is hostile. Its size is held to [`Limits`] before the decoder runs, the DXF it becomes while it is written, and the
//! decoder runs under `catch_unwind`. That isn't enough on its own: a small file that declares a huge table can make the
//! decoder allocate until the process aborts, which nothing in the process can catch. So the apps read a DWG in a child
//! process of their own ([`in_child`], answered by [`child_main`]): a file that brings the decoder down takes the child with
//! it, and the window, or the command line, says the file couldn't be read.

use std::io::{Cursor, Read, Write};
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

/// The argument that makes an app the child that reads one DWG: `<app> --dwg-to-dxf <path>`.
pub const CHILD_ARG: &str = "--dwg-to-dxf";
/// The argument that makes an app the child that writes one DWG: `<app> --dxf-to-dwg`, the DXF on its standard input and the
/// DWG on its standard output.
pub const WRITE_ARG: &str = "--dxf-to-dwg";

/// How large a DWG, and the DXF it becomes, may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The largest DWG file read at all, checked before the decoder runs.
    pub dwg_bytes: usize,
    /// The largest DXF a DWG may become, checked while it is written: a DWG is compressed, and its DXF is several times
    /// larger (about six in CADCraft's measures).
    pub dxf_bytes: usize,
}

impl Limits {
    /// 64 MB of DWG, and 128 MB of DXF, which is the most the core's DXF reader reads (`dxf::MAX_BYTES`).
    pub const DEFAULT: Limits = Limits { dwg_bytes: 64 << 20, dxf_bytes: 128 << 20 };
}

/// A DWG read: the DXF it became, and notes on what the read left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Converted {
    pub dxf: Vec<u8>,
    pub notes: Vec<String>,
}

/// Whether `bytes` begin as a DWG does: `AC10` and two digits, its version (AC1015 is 2000, AC1032 is 2018).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && bytes.starts_with(b"AC10") && bytes.get(4..6).is_some_and(|v| v.iter().all(u8::is_ascii_digit))
}

/// The DXF the DWG `bytes` hold, within `limits`.
pub fn to_dxf(bytes: &[u8], limits: Limits) -> Result<Converted, String> {
    if bytes.len() > limits.dwg_bytes {
        return Err(format!("the file is {}, larger than the {} of DWG BIMCraft reads", mb(bytes.len()), mb(limits.dwg_bytes)));
    }
    if !is_dwg(bytes) {
        return Err("it isn't a DWG file (a DWG begins with its version, AC1015 to AC1032)".into());
    }
    // Failsafe: an object the decoder can't read is left out and counted, rather than failing the whole file.
    let read = std::panic::catch_unwind(|| {
        acadrust::DwgReader::from_stream_with_options(Cursor::new(bytes), acadrust::DwgReadOptions::failsafe()).read_with_stats()
    })
    .map_err(|_| "the DWG reader failed on this file".to_string())?
    .map_err(|e| format!("the DWG couldn't be read: {e}"))?;
    if !read.stats.record_stream_read {
        let why = read.stats.diagnostics.first().map(|d| d.message.trim_start_matches("Failsafe: ").to_string());
        return Err(format!("the DWG couldn't be read: {}", why.unwrap_or_else(|| "its objects couldn't be read".into())));
    }
    let notes = skipped(&read.stats);
    let mut out = Capped { buf: Vec::new(), limit: limits.dxf_bytes, over: false };
    let written = std::panic::catch_unwind(AssertUnwindSafe(|| acadrust::DxfWriter::new(&read.document).write_to_writer(&mut out)))
        .map_err(|_| "the DWG reader failed writing this file as DXF".to_string())?;
    if out.over {
        return Err(format!(
            "this {} DWG became more than {} of DXF, the most BIMCraft reads; the file may be damaged",
            mb(bytes.len()),
            mb(limits.dxf_bytes)
        ));
    }
    written.map_err(|e| format!("the DWG couldn't be written as DXF: {e}"))?;
    Ok(Converted { dxf: out.buf, notes })
}

/// The DXF the DWG file at `path` holds, read in this process.
pub fn read_file(path: &Path, limits: Limits) -> Result<Converted, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > limits.dwg_bytes as u64 {
        return Err(format!("the file is {}, larger than the {} of DWG BIMCraft reads", mb(len as usize), mb(limits.dwg_bytes)));
    }
    to_dxf(&std::fs::read(path).map_err(|e| e.to_string())?, limits)
}

/// The note on what a failsafe read left out: how many object records, of which kinds; none when it read them all.
fn skipped(stats: &acadrust::ReadStats) -> Vec<String> {
    let n = stats.skipped_source_records;
    if n == 0 {
        return Vec::new();
    }
    let mut kinds: Vec<String> =
        stats.diagnostics.iter().filter(|d| d.record_handle.is_some()).filter_map(|d| d.record_type.clone()).collect();
    kinds.sort();
    kinds.dedup();
    kinds.truncate(6);
    let of = if stats.source_records > 0 { format!(" of {}", stats.source_records) } else { String::new() };
    let which = if kinds.is_empty() { String::new() } else { format!(" (of types {})", kinds.join(", ")) };
    vec![format!("{n}{of} of the DWG's object records couldn't be read and were left out{which}")]
}

/// The DXF as it's written, refused once it would pass `limit`.
struct Capped {
    buf: Vec<u8>,
    limit: usize,
    over: bool,
}

impl Write for Capped {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.over || self.buf.len().saturating_add(b.len()) > self.limit {
            self.over = true;
            return Err(std::io::Error::other("the DXF limit"));
        }
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn mb(n: usize) -> String {
    format!("{:.1} MB", n as f64 / f64::from(1u32 << 20))
}

// ---------- The child process ----------

/// The longest a child may read one DWG for.
pub const CHILD_TIME: Duration = Duration::from_secs(300);

/// The most a child says on its error stream.
const MAX_SAID: u64 = 64 << 10;

/// What an app does when it's started as the child that reads a DWG (`args`, its arguments after its own name, are
/// [`CHILD_ARG`] and a path): the DXF on its standard output, each note on its error stream as `note: ` and a line, and why on
/// its error stream when it can't. `None` when `args` aren't a child's, and the app goes on as itself.
pub fn child_main(args: &[String]) -> Option<ExitCode> {
    if let [arg] = args
        && arg == WRITE_ARG
    {
        return Some(write_child());
    }
    let [arg, path] = args else { return None };
    if arg != CHILD_ARG {
        return None;
    }
    let mut err = std::io::stderr();
    match read_file(Path::new(path), Limits::DEFAULT) {
        Ok(read) => {
            for note in &read.notes {
                let _ = writeln!(err, "note: {}", note.replace(['\r', '\n'], " "));
            }
            let mut out = std::io::stdout().lock();
            match out.write_all(&read.dxf).and_then(|()| out.flush()) {
                Ok(()) => Some(ExitCode::SUCCESS),
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    Some(ExitCode::FAILURE)
                }
            }
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            Some(ExitCode::FAILURE)
        }
    }
}

/// The DWG file at `path` read by `exe` started as a child ([`child_main`]), within [`Limits::DEFAULT`] and `time`. A child
/// that aborts, runs past `time` or says more than the limits allow is stopped, and the reply says the file couldn't be read.
pub fn in_child(exe: &Path, path: &Path, time: Duration) -> Result<Converted, String> {
    let cap = Limits::DEFAULT.dxf_bytes as u64 + 1;
    let ran = run_child(exe, &[CHILD_ARG.as_ref(), path.as_os_str()], None, cap, time, &|| false)
        .map_err(|e| format!("the DWG reader couldn't start: {e}"))?;
    let (notes, why) = ran.said();
    match ran.status {
        None => Err(format!("the DWG reader took longer than {} s on this file and was stopped", time.as_secs())),
        Some(s) if s.success() && ran.out.len() as u64 >= cap => {
            Err(format!("this DWG became more than {} of DXF, the most BIMCraft reads", mb(Limits::DEFAULT.dxf_bytes)))
        }
        Some(s) if s.success() => Ok(Converted { dxf: ran.out, notes }),
        Some(_) if !why.is_empty() => Err(why.join("; ")),
        Some(s) => Err(format!("the DWG reader stopped on this file ({s}); the file may be damaged")),
    }
}

/// What a child did: how it ended (none: it was stopped), what it wrote, and what it said on its error stream.
struct Ran {
    status: Option<std::process::ExitStatus>,
    out: Vec<u8>,
    said: Vec<u8>,
}

impl Ran {
    /// Its notes (`note: ` lines) and the rest of what it said, why it failed.
    fn said(&self) -> (Vec<String>, Vec<String>) {
        let said = String::from_utf8_lossy(&self.said);
        let (notes, why): (Vec<&str>, Vec<&str>) = said.lines().filter(|l| !l.trim().is_empty()).partition(|l| l.starts_with("note: "));
        (notes.iter().map(|l| l.trim_start_matches("note: ").to_string()).collect(), why.iter().map(|l| l.to_string()).collect())
    }
}

/// `exe args` run as a child: `input` on its standard input, what it writes taken up to `cap` bytes, for at most `time`, and
/// stopped as soon as `stop` says so. Both streams are read while the child runs, so neither fills and stalls it; past its
/// limit a stream is dropped, and the child, its writes refused, stops.
fn run_child(
    exe: &Path,
    args: &[&std::ffi::OsStr],
    input: Option<Vec<u8>>,
    cap: u64,
    time: Duration,
    stop: &dyn Fn() -> bool,
) -> std::io::Result<Ran> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Fed on a thread of its own: a child that writes before it has read all it is given would otherwise wait on us.
    let feed = child.stdin.take().zip(input).map(|(mut stdin, bytes)| {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        })
    });
    let out = child.stdout.take().map(|s| std::thread::spawn(move || drain(s, cap)));
    let said = child.stderr.take().map(|s| std::thread::spawn(move || drain(s, MAX_SAID)));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < time && !stop() => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    if let Some(f) = feed {
        let _ = f.join();
    }
    let out = out.and_then(|t| t.join().ok()).unwrap_or_default();
    let said = said.and_then(|t| t.join().ok()).unwrap_or_default();
    Ok(Ran { status, out, said })
}

/// The DWG reader an app hands its `Session`: this app itself, started as the child that reads one ([`in_child`]). None when
/// the app can't say where its own program is.
pub fn reader() -> Option<bimcraft_core::cad::DwgReader> {
    let exe = std::env::current_exe().ok()?;
    Some(std::sync::Arc::new(move |path: &Path| in_child(&exe, path, CHILD_TIME).map(|read| (read.dxf, read.notes))))
}

/// What `stream` gives, up to `cap` bytes.
fn drain(stream: impl Read, cap: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = stream.take(cap).read_to_end(&mut buf);
    buf
}

// ---------- Writing ----------

/// The oldest DWG written: AutoCAD 2004 (AC1018). A 2000 DWG has no true colours, which every drawing BIMCraft writes uses.
const OLDEST: acadrust::DxfVersion = acadrust::DxfVersion::AC1018;

/// The DWG the DXF `dxf` becomes, within `limits` (the DXF's size checked before the library reads it, the DWG's after it
/// is written), at least AutoCAD 2004's, with the drawing variables dictionary AutoCAD writes ([`variables`]). The library
/// runs under `catch_unwind`; an allocation it can't make still ends the process, so the apps write in a child
/// ([`write_in_child`]).
pub fn to_dwg(dxf: &[u8], limits: Limits) -> Result<Vec<u8>, String> {
    if dxf.len() > limits.dxf_bytes {
        return Err(format!("the drawing is {} of DXF, more than the {} BIMCraft writes as DWG", mb(dxf.len()), mb(limits.dxf_bytes)));
    }
    let data = dxf.to_vec();
    let mut doc = std::panic::catch_unwind(move || acadrust::DxfReader::from_reader(Cursor::new(data)).and_then(acadrust::DxfReader::read))
        .map_err(|_| "the DWG writer failed reading the drawing".to_string())?
        .map_err(|e| format!("the drawing couldn't be read for DWG: {e}"))?;
    if doc.version < OLDEST {
        doc.version = OLDEST;
    }
    std::panic::catch_unwind(AssertUnwindSafe(|| variables(&mut doc))).map_err(|_| "the DWG writer failed on this drawing".to_string())?;
    let dwg = std::panic::catch_unwind(AssertUnwindSafe(|| acadrust::DwgWriter::write_to_vec(&doc)))
        .map_err(|_| "the DWG writer failed on this drawing".to_string())?
        .map_err(|e| format!("the DWG couldn't be written: {e}"))?;
    if dwg.len() > limits.dwg_bytes {
        return Err(format!("the DWG would be {}, more than the {} BIMCraft writes", mb(dwg.len()), mb(limits.dwg_bytes)));
    }
    Ok(dwg)
}

/// The drawing variables dictionary (`AcDbVariableDictionary`) AutoCAD writes in every drawing's named objects, with the
/// variables of a drawing in standard styles at 1:1, when the drawing has none. A reader may take a 2004 DWG without it for
/// an empty drawing, as CADCraft found Vectric Aspire does.
fn variables(doc: &mut acadrust::CadDocument) {
    use acadrust::objects::{Dictionary, DictionaryVariable, ObjectType};
    const NAME: &str = "AcDbVariableDictionary";
    let root = doc.header.named_objects_dict_handle;
    match doc.objects.get(&root) {
        Some(ObjectType::Dictionary(d)) if d.entries.iter().all(|(key, _)| !key.eq_ignore_ascii_case(NAME)) => {}
        _ => return,
    }
    let handle = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = root;
    for (name, value) in
        [("CANNOSCALE", "1:1"), ("CMLEADERSTYLE", "Standard"), ("CTABLESTYLE", "Standard"), ("DIMASSOC", "2"), ("HIDETEXT", "1")]
    {
        let h = doc.allocate_handle();
        let mut variable = DictionaryVariable::new(name, value);
        variable.handle = h;
        variable.owner_handle = handle;
        dictionary.add_entry(name, h);
        doc.objects.insert(h, ObjectType::DictionaryVariable(variable));
    }
    doc.objects.insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(r)) = doc.objects.get_mut(&root) {
        r.add_entry(NAME, handle);
    }
}

/// What an app does as the child that writes a DWG ([`WRITE_ARG`]): the DXF from its standard input, the DWG on its standard
/// output, and why on its error stream when it can't.
fn write_child() -> ExitCode {
    let mut err = std::io::stderr();
    let mut dxf = Vec::new();
    let read = std::io::stdin().lock().take(Limits::DEFAULT.dxf_bytes as u64 + 1).read_to_end(&mut dxf);
    let written = read.map_err(|e| e.to_string()).and_then(|_| to_dwg(&dxf, Limits::DEFAULT));
    match written {
        Ok(dwg) => {
            let mut out = std::io::stdout().lock();
            match out.write_all(&dwg).and_then(|()| out.flush()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            ExitCode::FAILURE
        }
    }
}

/// The DWG the DXF `dxf` becomes, written by `exe` started as the child that writes one ([`WRITE_ARG`]) within `time`, and
/// stopped when `stop` says so.
pub fn write_in_child(exe: &Path, dxf: &[u8], time: Duration, stop: &dyn Fn() -> bool) -> Result<Vec<u8>, String> {
    let cap = Limits::DEFAULT.dwg_bytes as u64 + 1;
    let ran = run_child(exe, &[WRITE_ARG.as_ref()], Some(dxf.to_vec()), cap, time, stop)
        .map_err(|e| format!("the DWG writer couldn't start: {e}"))?;
    let (_, why) = ran.said();
    match ran.status {
        None if stop() => Err("cancelled".to_string()),
        None => Err(format!("the DWG writer took longer than {} s and was stopped", time.as_secs())),
        Some(s) if s.success() && ran.out.len() as u64 >= cap => Err("the DWG came out larger than BIMCraft writes".to_string()),
        Some(s) if s.success() && is_dwg(&ran.out) => Ok(ran.out),
        Some(s) if s.success() => Err("the DWG writer gave back no DWG".to_string()),
        Some(_) if !why.is_empty() => Err(why.join("; ")),
        Some(s) => Err(format!("the DWG writer stopped on this drawing ({s})")),
    }
}

/// DWG, as the export engine takes a format: the core's DXF of the view or sheet, written as DWG by this program started as
/// a child. Experimental: acadrust writes it, and DWG has no public specification to hold it to.
pub struct Dwg {
    exe: std::path::PathBuf,
}

impl bimcraft_core::export::Format for Dwg {
    fn id(&self) -> &'static str {
        "dwg"
    }
    fn label(&self) -> &'static str {
        "DWG"
    }
    fn extension(&self) -> &'static str {
        "dwg"
    }
    fn family(&self) -> bimcraft_core::export::Family {
        bimcraft_core::export::Family::Line
    }
    fn status(&self) -> bimcraft_core::export::Status {
        bimcraft_core::export::Status::Experimental
    }
    fn takes(&self) -> &'static [bimcraft_core::export::Takes] {
        &[bimcraft_core::export::Takes::Drawing]
    }
    fn write(
        &self,
        items: &[bimcraft_core::export::Prepared],
        options: &bimcraft_core::export::Options,
        ctx: &bimcraft_core::export::Ctx,
    ) -> Result<Vec<bimcraft_core::export::Out>, String> {
        let dxf = bimcraft_core::export::formats::Dxf.write(items, options, ctx)?;
        dxf.into_iter()
            .map(|out| {
                let bytes = write_in_child(&self.exe, &out.bytes, CHILD_TIME, ctx.cancelled)?;
                Ok(bimcraft_core::export::Out { suffix: out.suffix, bytes })
            })
            .collect()
    }
}

/// The DWG writer an app registers with its `Session`'s exports: this app itself, started as the child that writes one. None
/// when the app can't say where its own program is.
pub fn writer() -> Option<std::sync::Arc<dyn bimcraft_core::export::Format>> {
    let exe = std::env::current_exe().ok()?;
    Some(std::sync::Arc::new(Dwg { exe }))
}

/// The DWG a DXF becomes, written by the same library: tests make their DWG files with it, since Autodesk's own are never
/// committed.
#[cfg(any(test, feature = "testing"))]
pub fn dwg_from_dxf(dxf: &str) -> Result<Vec<u8>, String> {
    let doc = acadrust::DxfReader::from_reader(Cursor::new(dxf.as_bytes().to_vec()))
        .and_then(acadrust::DxfReader::read)
        .map_err(|e| format!("the DXF couldn't be read: {e}"))?;
    acadrust::DwgWriter::write_to_vec(&doc).map_err(|e| format!("the DWG couldn't be written: {e}"))
}

#[cfg(test)]
mod tests;
