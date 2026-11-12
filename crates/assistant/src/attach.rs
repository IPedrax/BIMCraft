//! Files the user sends with a message. A file the user picks (or drops on the palette) is copied into a temporary folder that
//! belongs to the chat and goes when the chat does (a new chat, a project replaced, the window closing: `TempDir` removes it as
//! it is dropped), so what is sent is the copy as it was when picked, whatever happens to the original. A file is a text file
//! (read as UTF-8, up to [`MAX_FILE_TEXT`]) or a picture (PNG, JPEG, GIF or WebP, up to [`MAX_FILE_IMAGE`]); nothing else is
//! sent, and the model has no tool to read a file: what it sees is what went in the message.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::{MAX_FILE_IMAGE, MAX_FILE_TEXT, MAX_FILES};

/// What a sent file holds, in the form the providers take.
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Text(String),
    Image { mime: &'static str, b64: String },
}

/// A file as it went with a message.
#[derive(Clone, Debug, PartialEq)]
pub struct Attached {
    pub name: String,
    pub body: Body,
}

impl Attached {
    /// About how many bytes it adds to a request.
    pub fn len(&self) -> usize {
        match &self.body {
            Body::Text(t) => t.len(),
            Body::Image { b64, .. } => b64.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A file picked and not yet sent: its name, its size and where its copy is.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub size: u64,
    pub image: bool,
    pub(crate) path: PathBuf,
}

impl Attachment {
    /// Reads the staged copy into what goes out.
    pub(crate) fn load(&self) -> Result<Attached, String> {
        let bytes = fs::read(&self.path).map_err(|e| format!("could not read {}: {e}", self.name))?;
        let body = match image_mime(&self.name) {
            Some(mime) => Body::Image { mime, b64: base64(&bytes) },
            None => Body::Text(String::from_utf8(bytes).map_err(|_| format!("{} is not text", self.name))?),
        };
        Ok(Attached { name: self.name.clone(), body })
    }
}

/// The media type of a picture the providers take, by its extension.
pub fn image_mime(name: &str) -> Option<&'static str> {
    match name.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// A file's name as the model and the palette see it: no quotes, angle brackets or control characters, which would end the
/// `<file name="…">` block early.
fn clean(name: &str) -> String {
    name.chars().map(|c| if c.is_control() || matches!(c, '"' | '<' | '>' | '/' | '\\') { '_' } else { c }).collect()
}

/// Copies `from` into `dir` as the chat's own, after checking that it is something that can be sent. `n` makes the copy's name
/// unique. `Err` is what the user is told.
pub(crate) fn stage(from: &Path, dir: &Path, n: usize, held: usize) -> Result<Attachment, String> {
    let name =
        from.file_name().and_then(|n| n.to_str()).map(clean).filter(|n| !n.trim().is_empty()).ok_or("that file has no usable name")?;
    if held >= MAX_FILES {
        return Err(format!("{MAX_FILES} files at most go with one message: send first, or remove one"));
    }
    let meta = fs::metadata(from).map_err(|e| format!("could not read {name}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{name} is not a file"));
    }
    let image = image_mime(&name).is_some();
    let cap = if image { MAX_FILE_IMAGE } else { MAX_FILE_TEXT };
    if meta.len() > cap {
        let what = if image { "a picture" } else { "a text file" };
        return Err(format!("{name} is {} KB: {what} is at most {} KB", meta.len().div_ceil(1000), cap / 1000));
    }
    // Read through `take`, so a file that grew since the check (or has no end) is still cut at the cap.
    let mut bytes = Vec::new();
    File::open(from).and_then(|f| f.take(cap + 1).read_to_end(&mut bytes)).map_err(|e| format!("could not read {name}: {e}"))?;
    if bytes.len() as u64 > cap {
        return Err(format!("{name} is larger than {} KB", cap / 1000));
    }
    // shortcut: a picture is taken by its extension, so a mislabelled one is the provider's error to report.
    if !image && std::str::from_utf8(&bytes).is_err() {
        return Err(format!("{name} is not text or a picture (PNG, JPEG, GIF, WebP): only those are sent"));
    }
    let path = dir.join(format!("{n}-{name}"));
    fs::write(&path, &bytes).map_err(|e| format!("could not keep a copy of {name}: {e}"))?;
    Ok(Attachment { name, size: bytes.len() as u64, image, path })
}

/// Standard base64, with padding: what the providers take for a picture. The standard library has none and a few lines are not a
/// dependency.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(A[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn cleans_a_name_that_could_end_the_file_block_early() {
        assert_eq!(super::clean("a\"><b.txt"), "a___b.txt");
    }
}
