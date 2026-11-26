//! E57 (ASTM E2807): the vendor-neutral file scanners export. A file is pages of 1024 bytes, each ending in a checksum;
//! an XML section says what it holds, and each scan (`data3D`) keeps its points in a binary section of packets, each
//! field bit-packed in a stream of its own. Read from the public standard; every scan in a file comes in, placed by
//! its pose. The checksums aren't checked: a page that lies reads as points that lie, never as a crash.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{Read, SeekFrom};

use super::{Raw, Source};

/// Bytes in a page, and of them, the ones that hold data (the last four are its checksum).
const PAGE: u64 = 1024;
const DATA: u64 = PAGE - 4;
/// How deep the XML may nest, and how many elements it may hold.
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 1 << 20;
/// Bytes of XML held to be parsed: a scanner's runs to a few megabytes.
const MAX_XML: usize = 256 << 20;
/// Bytes read from the file at a time, and served from memory until a read falls outside them; small under test, so the
/// tests cross from one to the next.
const WINDOW: usize = if cfg!(test) { 8 << 10 } else { 1 << 20 };

/// What a file may ask before any of it is unpacked. A bit-packed field can take no bits at all (a constant), so a few
/// megabytes may declare billions of points, and every point costs a step for each field. A file is read a window at a
/// time and never held, so its size costs no memory; what's kept of its points is the cloud's to thin.
#[derive(Clone, Copy)]
struct Caps {
    /// Scans in a file.
    scans: usize,
    /// Points in all of them: a sanity cap, past the largest surveys, that keeps a lying count from hours of unpacking.
    points: u64,
    /// Fields a point is made of; scanners write a dozen.
    fields: usize,
    /// Bytes one stream may hold waiting for the others; a packet is 64 KiB at most.
    ahead: usize,
}

const CAPS: Caps = Caps { scans: 4096, points: 100_000_000_000, fields: 32, ahead: 4 << 20 };

/// What the scans before one have used up: the points left of the file's budget, and the sections they read.
struct Taken {
    points: u64,
    sections: HashSet<u64>,
}

/// Hands each point of every scan in an E57 file of `size` bytes to `f`, in metres where the file put it.
pub(super) fn read(src: &mut dyn Source, size: u64, f: &mut impl FnMut(Raw) -> Result<(), String>) -> Result<(), String> {
    read_within(src, size, f, CAPS)
}

fn read_within(src: &mut dyn Source, size: u64, f: &mut impl FnMut(Raw) -> Result<(), String>, caps: Caps) -> Result<(), String> {
    let mut header = Vec::with_capacity(48);
    src.seek(SeekFrom::Start(0)).map_err(super::unread)?;
    src.take(48).read_to_end(&mut header).map_err(super::unread)?;
    let bytes = header.as_slice();
    if !bytes.starts_with(b"ASTM-E57") {
        return Err("it isn't an E57 file: it doesn't start with ASTM-E57".into());
    }
    let short = || "its header is cut short".to_string();
    let u32_at = |i: usize| bytes.get(i..i + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(short);
    let u64_at = |i: usize| bytes.get(i..i + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).ok_or_else(short);
    let major = u32_at(8)?;
    if major != 1 {
        return Err(format!("it's E57 version {major}; BIMCraft reads version 1"));
    }
    let page = u64_at(40)?;
    if page != PAGE {
        return Err(format!("its pages are {page} bytes; E57's are {PAGE}"));
    }
    let (xml_at, xml_len) = (u64_at(24)?, u64_at(32)?);
    if xml_len > size {
        return Err("its XML is longer than the file".into());
    }
    let xml_len = usize::try_from(xml_len)
        .ok()
        .filter(|&n| n <= MAX_XML)
        .ok_or_else(|| format!("its XML is {xml_len} bytes; BIMCraft reads up to {MAX_XML}"))?;
    let file = Paged::new(src, size);
    let mut xml = Vec::new();
    file.take(file.logical(xml_at)?, xml_len, &mut xml)?;
    let root = parse(&String::from_utf8_lossy(&xml))?;
    drop(xml);
    let Some(data3d) = root.child("data3D") else { return Err("it holds no scans (no data3D)".into()) };
    if data3d.children.len() > caps.scans {
        return Err(format!("it holds {} scans; BIMCraft reads up to {}", data3d.children.len(), caps.scans));
    }
    let mut taken = Taken { points: caps.points, sections: HashSet::new() };
    for (i, scan) in data3d.children.iter().enumerate() {
        scan_points(&file, scan, f, &caps, &mut taken).map_err(|e| format!("scan {}: {e}", i + 1))?;
    }
    Ok(())
}

/// The file as E57 lays it out: logical bytes, 1020 to each page of 1024, read from the source a window of [`WINDOW`]
/// physical bytes at a time. Reading on from where the last read ended never seeks; a read outside the window fills it anew.
struct Paged<'a> {
    src: RefCell<&'a mut dyn Source>,
    size: u64,
    window: RefCell<Window>,
}

/// Physical bytes from `start` held, and where the source stands.
struct Window {
    start: u64,
    bytes: Vec<u8>,
    at: u64,
}

impl<'a> Paged<'a> {
    fn new(src: &'a mut dyn Source, size: u64) -> Paged<'a> {
        // Where the source stands is unknown until the first fill seeks.
        Paged { src: RefCell::new(src), size, window: RefCell::new(Window { start: 0, bytes: Vec::new(), at: u64::MAX }) }
    }

    /// The logical place of a physical offset, which mustn't fall on a checksum.
    fn logical(&self, physical: u64) -> Result<u64, String> {
        if physical % PAGE >= DATA {
            return Err(format!("offset {physical} falls on a page's checksum"));
        }
        Ok(physical / PAGE * DATA + physical % PAGE)
    }

    /// Appends `n` logical bytes from logical place `at`.
    fn take(&self, mut at: u64, mut n: usize, out: &mut Vec<u8>) -> Result<(), String> {
        let mut window = self.window.try_borrow_mut().map_err(|_| "the file is read twice at once".to_string())?;
        while n > 0 {
            let in_page = at % DATA;
            let physical = at.checked_div(DATA).and_then(|p| p.checked_mul(PAGE)).and_then(|p| p.checked_add(in_page));
            let physical = physical.ok_or("an offset past any file")?;
            let here = usize::try_from((DATA - in_page).min(n as u64)).unwrap_or(n);
            let end = physical.checked_add(here as u64).filter(|&e| e <= self.size);
            let end = end.ok_or_else(|| format!("it's cut short: byte {physical} is past its {} bytes", self.size))?;
            let held = window.start + window.bytes.len() as u64;
            if physical < window.start || end > held {
                self.fill(&mut window, physical)?;
            }
            let from = usize::try_from(physical - window.start).map_err(|_| "an offset past the window".to_string())?;
            let part = window.bytes.get(from..from + here).ok_or("the window doesn't hold what it was filled with")?;
            out.extend_from_slice(part);
            at += here as u64;
            n -= here;
        }
        Ok(())
    }

    /// Fills the window from physical byte `start`, as far as [`WINDOW`] or the end of the file.
    fn fill(&self, window: &mut Window, start: u64) -> Result<(), String> {
        let mut src = self.src.try_borrow_mut().map_err(|_| "the file is read twice at once".to_string())?;
        let len = usize::try_from(self.size.saturating_sub(start)).unwrap_or(WINDOW).min(WINDOW);
        let held = window.start + window.bytes.len() as u64;
        // What the window holds from `start` on is kept, and the source reads on from where it stands.
        let kept = if window.at == held && (window.start..=held).contains(&start) {
            window.bytes.drain(..usize::try_from(start - window.start).unwrap_or(0));
            window.bytes.len().min(len)
        } else {
            window.at = u64::MAX;
            window.bytes.clear();
            src.seek(SeekFrom::Start(start)).map_err(super::unread)?;
            0
        };
        window.start = start;
        window.bytes.resize(len, 0);
        if let Err(e) = src.read_exact(&mut window.bytes[kept..]) {
            window.bytes.clear();
            window.at = u64::MAX;
            return Err(super::unread(e));
        }
        window.at = start + len as u64;
        Ok(())
    }
}

/// An element of the XML: its name, its attributes, what it holds.
#[derive(Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// A number its attribute `name` holds, or `default` when it hasn't one.
    fn number(&self, name: &str, default: f64) -> Result<f64, String> {
        match self.attr(name) {
            None => Ok(default),
            Some(v) => v.trim().parse().map_err(|_| format!("<{}>'s {name} isn't a number: {v}", self.name)),
        }
    }

    fn integer(&self, name: &str, default: i64) -> Result<i64, String> {
        match self.attr(name) {
            None => Ok(default),
            Some(v) => v.trim().parse().map_err(|_| format!("<{}>'s {name} isn't an integer: {v}", self.name)),
        }
    }

    /// The number a child element holds, as `<w type="Float">1</w>`.
    fn value(&self, name: &str) -> Option<f64> {
        self.child(name).and_then(|c| c.text.trim().parse().ok())
    }
}

/// A small XML reader, enough for E57's: elements, attributes, text and CDATA; declarations and comments are passed over.
fn parse(s: &str) -> Result<Node, String> {
    let bad = |what: &str| format!("its XML is broken: {what}");
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut nodes = 0;
    let mut rest = s;
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&unescape(rest));
            }
            break;
        };
        if let Some(top) = stack.last_mut() {
            top.text.push_str(&unescape(&rest[..open]));
        }
        rest = &rest[open..];
        if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let end = r.find("]]>").ok_or_else(|| bad("a CDATA isn't closed"))?;
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&r[..end]);
            }
            rest = &r[end + 3..];
        } else if let Some(r) = rest.strip_prefix("<!--") {
            let end = r.find("-->").ok_or_else(|| bad("a comment isn't closed"))?;
            rest = &r[end + 3..];
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            let end = rest.find('>').ok_or_else(|| bad("a declaration isn't closed"))?;
            rest = &rest[end + 1..];
        } else if let Some(r) = rest.strip_prefix("</") {
            let end = r.find('>').ok_or_else(|| bad("an end tag isn't closed"))?;
            let name = r[..end].trim();
            let node = stack.pop().filter(|_| !stack.is_empty()).ok_or_else(|| bad(&format!("</{name}> closes nothing")))?;
            if node.name != name {
                return Err(bad(&format!("<{}> is closed by </{name}>", node.name)));
            }
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            }
            rest = &r[end + 1..];
        } else {
            let r = &rest[1..];
            let end = tag_end(r).ok_or_else(|| bad("a tag isn't closed"))?;
            let (tag, empty) = match r[..end].strip_suffix('/') {
                Some(t) => (t, true),
                None => (&r[..end], false),
            };
            nodes += 1;
            if nodes > MAX_NODES {
                return Err(format!("its XML holds more than {MAX_NODES} elements"));
            }
            let node = element(tag).ok_or_else(|| bad(&format!("<{tag}>")))?;
            if empty {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            } else {
                if stack.len() > MAX_DEPTH {
                    return Err(format!("its XML nests deeper than {MAX_DEPTH}"));
                }
                stack.push(node);
            }
            rest = &r[end + 1..];
        }
    }
    let [top] = <[Node; 1]>::try_from(stack).map_err(|s| bad(&format!("<{}> isn't closed", s.last().map_or("", |n| n.name.as_str()))))?;
    top.children.into_iter().next().ok_or_else(|| bad("it has no root element"))
}

/// Where a tag ends: its `>`, outside quotes.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// An element from its tag: a name, then `key="value"` pairs.
fn element(tag: &str) -> Option<Node> {
    let tag = tag.trim();
    let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
    let name = &tag[..name_end];
    if name.is_empty() {
        return None;
    }
    let mut node = Node { name: name.to_string(), ..Node::default() };
    let mut rest = tag[name_end..].trim_start();
    while !rest.is_empty() {
        let eq = rest.find('=')?;
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let q = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let close = after[1..].find(q)?;
        node.attrs.push((key.to_string(), unescape(&after[1..1 + close])));
        rest = after[close + 2..].trim_start();
    }
    Some(node)
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// How a field's values are packed.
#[derive(Clone, Copy, Debug)]
enum Pack {
    /// IEEE floats, 32 or 64 bits.
    Float { double: bool },
    /// An integer of `bits` bits above `min`, times `scale`, plus `offset` (1 and 0 for a plain Integer).
    Int { min: i64, max: i64, bits: u32, scale: f64, offset: f64 },
}

impl Pack {
    fn of(node: &Node) -> Result<Pack, String> {
        let int = |scale: f64, offset: f64| -> Result<Pack, String> {
            let (min, max) = (node.integer("minimum", i64::MIN)?, node.integer("maximum", i64::MAX)?);
            if max < min {
                return Err(format!("<{}>'s maximum {max} is under its minimum {min}", node.name));
            }
            // Bits enough for every value from the minimum to the maximum.
            let range = max.abs_diff(min);
            Ok(Pack::Int { min, max, bits: u64::BITS - range.leading_zeros(), scale, offset })
        };
        match node.attr("type") {
            Some("Float") => Ok(Pack::Float { double: node.attr("precision") != Some("single") }),
            Some("Integer") => int(1.0, 0.0),
            Some("ScaledInteger") => {
                let (scale, offset) = (node.number("scale", 1.0)?, node.number("offset", 0.0)?);
                if !(scale.is_finite() && scale != 0.0 && offset.is_finite()) {
                    return Err(format!("<{}>'s scale {scale} and offset {offset} aren't usable", node.name));
                }
                int(scale, offset)
            }
            t => Err(format!("its field <{}> is a {}, which BIMCraft doesn't read", node.name, t.unwrap_or("thing of no type"))),
        }
    }

    fn bits(self) -> u32 {
        match self {
            Pack::Float { double } => {
                if double {
                    64
                } else {
                    32
                }
            }
            Pack::Int { bits, .. } => bits,
        }
    }

    fn decode(self, raw: u64) -> f64 {
        match self {
            Pack::Float { double: true } => f64::from_bits(raw),
            Pack::Float { double: false } => f64::from(f32::from_bits(raw as u32)),
            Pack::Int { min, scale, offset, .. } => (min as i128 + raw as i128) as f64 * scale + offset,
        }
    }
}

/// One field's stream: its bytes not yet read and where in them the next value starts.
struct Stream {
    pack: Pack,
    bytes: Vec<u8>,
    bit: usize,
}

impl Stream {
    /// The next value, if all its bits have come.
    fn next(&mut self) -> Option<f64> {
        let bits = self.pack.bits() as usize;
        if bits == 0 {
            return Some(self.pack.decode(0));
        }
        if self.bit + bits > self.bytes.len() * 8 {
            return None;
        }
        let first = self.bit / 8;
        let mut word = [0u8; 16];
        let take = (self.bytes.len() - first).min(16);
        word[..take].copy_from_slice(&self.bytes[first..first + take]);
        let all = u128::from_le_bytes(word) >> (self.bit % 8);
        let raw = if bits == 64 { all as u64 } else { (all as u64) & ((1u64 << bits) - 1) };
        self.bit += bits;
        Some(self.pack.decode(raw))
    }

    /// Takes a packet's bytes after what's left of the last; a stream that holds more than `ahead` is waiting on the others for good.
    fn feed(&mut self, more: &[u8], ahead: usize) -> Result<(), String> {
        self.bytes.drain(..self.bit / 8);
        self.bit %= 8;
        self.bytes.extend_from_slice(more);
        if self.bytes.len() > ahead {
            return Err(format!("a stream of its points runs {} bytes ahead of the others; up to {ahead}", self.bytes.len()));
        }
        Ok(())
    }
}

/// Which stream holds what a point is made of.
#[derive(Default)]
struct Fields {
    cartesian: Option<[usize; 3]>,
    spherical: Option<[usize; 3]>,
    invalid: Option<usize>,
    intensity: Option<usize>,
    rgb: Option<[usize; 3]>,
}

fn scan_points(
    file: &Paged,
    scan: &Node,
    f: &mut impl FnMut(Raw) -> Result<(), String>,
    caps: &Caps,
    taken: &mut Taken,
) -> Result<(), String> {
    let points = scan.child("points").ok_or("it has no points")?;
    if points.attr("type") != Some("CompressedVector") {
        return Err("its points aren't a CompressedVector".into());
    }
    if points.child("codecs").is_some_and(|c| !c.children.is_empty()) {
        return Err("its points are packed by a codec BIMCraft doesn't know; E57 writes bit-packed ones".into());
    }
    let count = u64::try_from(points.integer("recordCount", -1)?).map_err(|_| "it doesn't say how many points it has")?;
    let at = u64::try_from(points.integer("fileOffset", -1)?).map_err(|_| "it doesn't say where its points are")?;
    let prototype = points.child("prototype").ok_or("its points have no prototype")?;
    if prototype.children.len() > caps.fields {
        return Err(format!("its points have {} fields; BIMCraft reads up to {}", prototype.children.len(), caps.fields));
    }
    let mut streams = Vec::new();
    let mut names = Vec::new();
    for field in &prototype.children {
        streams.push(Stream { pack: Pack::of(field)?, bytes: Vec::new(), bit: 0 });
        names.push(field.name.as_str());
    }
    let find = |n: &str| names.iter().position(|m| *m == n);
    let three = |a, b, c| Some([find(a)?, find(b)?, find(c)?]);
    let cartesian = three("cartesianX", "cartesianY", "cartesianZ");
    let fields = Fields {
        cartesian,
        spherical: three("sphericalRange", "sphericalAzimuth", "sphericalElevation"),
        invalid: find(if cartesian.is_some() { "cartesianInvalidState" } else { "sphericalInvalidState" }),
        intensity: find("intensity"),
        rgb: three("colorRed", "colorGreen", "colorBlue"),
    };
    if fields.cartesian.is_none() && fields.spherical.is_none() {
        return Err("its points have neither cartesian nor spherical coordinates".into());
    }
    if count == 0 {
        return Ok(());
    }
    if streams.iter().all(|s| s.pack.bits() == 0) {
        return Err("its points hold no data".into());
    }
    taken.points = taken
        .points
        .checked_sub(count)
        .ok_or_else(|| format!("the scans of a file hold up to {} points together; thin the survey where it was made", caps.points))?;
    if !taken.sections.insert(at) {
        return Err(format!("its points at offset {at} are the section another scan reads; each scan keeps its own"));
    }
    let pose = Pose::of(scan)?;
    let colour = Colour::of(scan, &streams, fields.rgb);

    // The section's header, then its packets from the first data packet on.
    let start = file.logical(at)?;
    let mut head = Vec::new();
    file.take(start, 32, &mut head)?;
    let word = |i: usize| head.get(i..i + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).unwrap_or(0);
    if head.first() != Some(&1) {
        return Err(format!("its points at offset {at} aren't a compressed vector section"));
    }
    let end = start.saturating_add(word(8));
    // The points can't take fewer bits than their fields' widths added: a count the section has no room for is a lie, told
    // before a packet is unpacked.
    let bits: u64 = streams.iter().map(|s| u64::from(s.pack.bits())).sum();
    let (need, room) = (count.saturating_mul(bits), word(8).saturating_sub(32).saturating_mul(8));
    if need > room {
        return Err(format!("it's cut short: its {count} points take {} bytes and its section holds {}", need.div_ceil(8), room / 8));
    }
    let mut next = file.logical(word(16))?;
    let mut left = count;
    let mut packet = Vec::new();
    let mut values = vec![0.0; streams.len()];
    while left > 0 {
        if next >= end {
            return Err(format!("it's cut short: {left} of its {count} points are missing"));
        }
        packet.clear();
        file.take(next, 4, &mut packet)?;
        let length = u64::from(u16::from_le_bytes([packet[2], packet[3]])) + 1;
        match packet[0] {
            1 => {
                file.take(next + 4, usize::try_from(length).unwrap_or(0).saturating_sub(4), &mut packet)?;
                feed(&packet, &mut streams, caps.ahead)?;
                // Every point whose fields have all come.
                while left > 0 && pull(&mut streams, &mut values) {
                    left -= 1;
                    if let Some(raw) = point(&values, &fields, &pose, &colour) {
                        f(raw)?;
                    }
                }
            }
            0 | 2 => {}
            t => return Err(format!("a packet of type {t}, which E57 doesn't have")),
        }
        next += length;
    }
    Ok(())
}

/// Hands a data packet's buffers to their streams.
fn feed(packet: &[u8], streams: &mut [Stream], ahead: usize) -> Result<(), String> {
    let u16_at = |i: usize| packet.get(i..i + 2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])));
    let short = || "a data packet is cut short".to_string();
    let n = u16_at(4).ok_or_else(short)?;
    if n != streams.len() {
        return Err(format!("a data packet has {n} streams; its prototype has {}", streams.len()));
    }
    let mut at = 6 + 2 * n;
    for (k, s) in streams.iter_mut().enumerate() {
        let len = u16_at(6 + 2 * k).ok_or_else(short)?;
        s.feed(packet.get(at..at + len).ok_or_else(short)?, ahead)?;
        at += len;
    }
    Ok(())
}

/// The next point's values, one from each stream, if each has one; none is taken unless all have.
fn pull(streams: &mut [Stream], values: &mut [f64]) -> bool {
    let ready = streams.iter().all(|s| s.pack.bits() == 0 || s.bit + s.pack.bits() as usize <= s.bytes.len() * 8);
    if !ready {
        return false;
    }
    for (s, v) in streams.iter_mut().zip(values.iter_mut()) {
        *v = s.next().unwrap_or(f64::NAN);
    }
    true
}

fn point(v: &[f64], fields: &Fields, pose: &Pose, colour: &Colour) -> Option<Raw> {
    // 1: a direction with no range; 2: no point at all.
    if fields.invalid.is_some_and(|i| v[i] != 0.0) {
        return None;
    }
    let p = match (fields.cartesian, fields.spherical) {
        (Some([x, y, z]), _) => [v[x], v[y], v[z]],
        (None, Some([r, az, el])) => {
            let (r, az, el) = (v[r], v[az], v[el]);
            [r * el.cos() * az.cos(), r * el.cos() * az.sin(), r * el.sin()]
        }
        (None, None) => return None,
    };
    Some(Raw { p: pose.apply(p), intensity: fields.intensity.map(|i| v[i]), rgb: fields.rgb.map(|c| colour.of_values(c.map(|k| v[k]))) })
}

/// Where a scan's own coordinates sit in the file's: a rotation, then a translation.
struct Pose {
    m: [[f64; 3]; 3],
    t: [f64; 3],
}

impl Pose {
    fn of(scan: &Node) -> Result<Pose, String> {
        let mut pose = Pose { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };
        let Some(node) = scan.child("pose") else { return Ok(pose) };
        if let Some(r) = node.child("rotation") {
            let q = ["w", "x", "y", "z"].map(|k| r.value(k).unwrap_or(0.0));
            let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
            if !(n.is_finite() && n > 1e-9) {
                return Err("its pose's rotation isn't a rotation".into());
            }
            let [w, x, y, z] = q.map(|v| v / n);
            pose.m = [
                [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
                [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
                [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
            ];
        }
        if let Some(t) = node.child("translation") {
            pose.t = ["x", "y", "z"].map(|k| t.value(k).unwrap_or(0.0));
            if pose.t.iter().any(|v| !v.is_finite()) {
                return Err("its pose's translation isn't a place".into());
            }
        }
        Ok(pose)
    }

    fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|i| self.m[i][0] * p[0] + self.m[i][1] * p[1] + self.m[i][2] * p[2] + self.t[i])
    }
}

/// A scan's colours to eight bits, from the range its `colorLimits` (or else its fields) give.
struct Colour {
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Colour {
    fn of(scan: &Node, streams: &[Stream], rgb: Option<[usize; 3]>) -> Colour {
        let mut c = Colour { lo: [0.0; 3], hi: [255.0; 3] };
        let limits = scan.child("colorLimits");
        for (k, name) in ["Red", "Green", "Blue"].iter().enumerate() {
            let given = limits.and_then(|l| Some((l.value(&format!("color{name}Minimum"))?, l.value(&format!("color{name}Maximum"))?)));
            let field = rgb.and_then(|r| streams.get(r[k])).and_then(|s| match s.pack {
                Pack::Int { min, max, scale, offset, .. } => Some((min as f64 * scale + offset, max as f64 * scale + offset)),
                _ => None,
            });
            if let Some((lo, hi)) = given.or(field).filter(|(lo, hi)| hi > lo) {
                (c.lo[k], c.hi[k]) = (lo, hi);
            }
        }
        c
    }

    fn of_values(&self, v: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| ((v[k] - self.lo[k]) / (self.hi[k] - self.lo[k]) * 255.0).round())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Format, Options, Scan, read as read_cloud};
    use super::*;

    /// A field as a test writes it: its name, how it's packed, and its values.
    #[derive(Clone)]
    struct Field {
        xml: String,
        pack: Pack,
        values: Vec<f64>,
    }

    fn float(name: &str, single: bool, values: &[f64]) -> Field {
        let xml = format!("<{name} type=\"Float\"{}/>", if single { " precision=\"single\"" } else { "" });
        Field { xml, pack: Pack::Float { double: !single }, values: values.to_vec() }
    }

    fn integer(name: &str, min: i64, max: i64, values: &[f64]) -> Field {
        scaled(name, min, max, 1.0, 0.0, values)
    }

    fn scaled(name: &str, min: i64, max: i64, scale: f64, offset: f64, values: &[f64]) -> Field {
        let plain = scale == 1.0 && offset == 0.0;
        let kind = if plain { "Integer" } else { "ScaledInteger" };
        let more = if plain { String::new() } else { format!(" scale=\"{scale}\" offset=\"{offset}\"") };
        let xml = format!("<{name} type=\"{kind}\" minimum=\"{min}\" maximum=\"{max}\"{more}>0</{name}>");
        let bits = u64::BITS - max.abs_diff(min).leading_zeros();
        Field { xml, pack: Pack::Int { min, max, bits, scale, offset }, values: values.to_vec() }
    }

    /// A scan as a test writes it: its fields, and what more its XML says (a pose, colour limits).
    struct Scan57 {
        fields: Vec<Field>,
        more: String,
    }

    fn cartesian(points: &[[f64; 3]]) -> Vec<Field> {
        ["cartesianX", "cartesianY", "cartesianZ"]
            .iter()
            .enumerate()
            .map(|(k, n)| float(n, true, &points.iter().map(|p| p[k]).collect::<Vec<_>>()))
            .collect()
    }

    /// The bits of a field's values, packed from the least significant up.
    fn stream(f: &Field) -> Vec<u8> {
        let (mut out, mut acc, mut n) = (Vec::new(), 0u128, 0u32);
        for &v in &f.values {
            let raw: u64 = match f.pack {
                Pack::Float { double: true } => v.to_bits(),
                Pack::Float { double: false } => u64::from((v as f32).to_bits()),
                Pack::Int { min, scale, offset, .. } => (((v - offset) / scale).round() as i64 - min) as u64,
            };
            acc |= u128::from(raw) << n;
            n += f.pack.bits();
            while n >= 8 {
                out.push(acc as u8);
                acc >>= 8;
                n -= 8;
            }
        }
        if n > 0 {
            out.push(acc as u8);
        }
        out
    }

    fn physical(logical: u64) -> u64 {
        logical / DATA * PAGE + logical % DATA
    }

    /// An E57 file of these scans: the header, each scan's section of packets (each stream cut at its own size, so
    /// values cross packets, with an empty packet among them), then the XML; laid in pages, checksums left nought.
    fn e57(scans: &[Scan57]) -> Vec<u8> {
        let mut logical = vec![0u8; 48];
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- written by a test -->\n\
             <e57Root type=\"Structure\" xmlns=\"http://www.astm.org/COMMIT/E57/2010-e57-v1.0\">\n\
             <formatName type=\"String\"><![CDATA[ASTM E57 3D Imaging Data File]]></formatName>\n\
             <data3D type=\"Vector\" allowHeterogeneousChildren=\"1\">\n",
        );
        for (i, scan) in scans.iter().enumerate() {
            let start = logical.len() as u64;
            let streams: Vec<Vec<u8>> = scan.fields.iter().map(stream).collect();
            let mut packets = Vec::new();
            let mut cut = vec![0usize; streams.len()];
            let mut first = true;
            while cut.iter().zip(&streams).any(|(c, s)| *c < s.len()) {
                let mut parts = Vec::new();
                for (k, s) in streams.iter().enumerate() {
                    let to = (cut[k] + 5 + 3 * k).min(s.len());
                    parts.push(&s[cut[k]..to]);
                    cut[k] = to;
                }
                let mut p = vec![1u8, 0, 0, 0];
                p.extend((parts.len() as u16).to_le_bytes());
                for part in &parts {
                    p.extend((part.len() as u16).to_le_bytes());
                }
                for part in &parts {
                    p.extend(*part);
                }
                while !p.len().is_multiple_of(4) {
                    p.push(0);
                }
                let len = (p.len() as u16 - 1).to_le_bytes();
                (p[2], p[3]) = (len[0], len[1]);
                packets.extend(p);
                if first {
                    packets.extend([2u8, 0, 7, 0, 0, 0, 0, 0]);
                    first = false;
                }
            }
            logical.extend([1u8, 0, 0, 0, 0, 0, 0, 0]);
            logical.extend((32 + packets.len() as u64).to_le_bytes());
            logical.extend(physical(start + 32).to_le_bytes());
            logical.extend(0u64.to_le_bytes());
            logical.extend(packets);
            while !logical.len().is_multiple_of(4) {
                logical.push(0);
            }
            let count = scan.fields.first().map_or(0, |f| f.values.len());
            let prototype: String = scan.fields.iter().map(|f| f.xml.clone()).collect();
            xml += &format!(
                "<vectorChild type=\"Structure\">\n<name type=\"String\"><![CDATA[scan {i} & more]]></name>\n{}\n\
                 <points type=\"CompressedVector\" fileOffset=\"{}\" recordCount=\"{count}\">\n\
                 <prototype type=\"Structure\">{prototype}</prototype>\n\
                 <codecs type=\"Vector\" allowHeterogeneousChildren=\"1\"></codecs>\n</points>\n</vectorChild>\n",
                scan.more,
                physical(start)
            );
        }
        xml += "</data3D>\n<images2D type=\"Vector\" allowHeterogeneousChildren=\"1\"/>\n</e57Root>\n";
        let xml_at = logical.len() as u64;
        logical.extend(xml.bytes());
        // The header, now that it knows where everything is.
        let pages = (logical.len() as u64).div_ceil(DATA);
        let mut h = b"ASTM-E57".to_vec();
        h.extend(1u32.to_le_bytes());
        h.extend(0u32.to_le_bytes());
        h.extend((pages * PAGE).to_le_bytes());
        h.extend(physical(xml_at).to_le_bytes());
        h.extend((xml.len() as u64).to_le_bytes());
        h.extend(PAGE.to_le_bytes());
        logical[..48].copy_from_slice(&h);
        let mut file = Vec::new();
        for chunk in logical.chunks(DATA as usize) {
            file.extend(chunk);
            file.resize(file.len().div_ceil(PAGE as usize) * PAGE as usize, 0);
        }
        file
    }

    fn read(b: &[u8]) -> Result<Scan, String> {
        read_cloud(b, Format::E57, &Options::default())
    }

    /// Each point in millimetres, to a thousandth.
    fn all(s: &Scan) -> Vec<[f64; 3]> {
        (0..s.points.len()).filter_map(|i| s.absolute(i)).map(|p| p.map(|v| (v * 1000.0).round() / 1000.0)).collect()
    }

    /// Enough points that their streams cross pages as well as packets.
    fn grid(n: usize) -> Vec<[f64; 3]> {
        (0..n).map(|i| [i as f64 * 0.01, (i % 7) as f64 * 0.5, -(i as f64) * 0.002]).collect()
    }

    #[test]
    fn an_e57_scan_comes_in_with_its_colours() {
        let pts = grid(700);
        let mut fields = cartesian(&pts);
        let shade = |k: usize| (0..pts.len()).map(|i| ((i * (k + 3)) % 256) as f64).collect::<Vec<_>>();
        fields.push(integer("colorRed", 0, 255, &shade(0)));
        fields.push(integer("colorGreen", 0, 255, &shade(1)));
        fields.push(integer("colorBlue", 0, 255, &shade(2)));
        let file = e57(&[Scan57 { fields, more: String::new() }]);
        assert!(file.len() > 3 * PAGE as usize, "the test's file spans pages");
        let s = read(&file).unwrap();
        assert_eq!((s.read, s.points.len()), (700, 700));
        for (got, p) in all(&s).iter().zip(&pts) {
            let want = p.map(|v| f64::from(v as f32) * 1000.0);
            assert!((0..3).all(|k| (got[k] - want[k]).abs() < 0.01), "{got:?} {want:?}");
        }
        let colours = s.colours.unwrap();
        assert_eq!(colours[0], [0, 0, 0]);
        assert_eq!(colours[100], [(300 % 256) as u8, (400 % 256) as u8, (500 % 256) as u8]);
    }

    #[test]
    fn every_scan_in_a_file_comes_in_placed_by_its_pose() {
        let one = Scan57 { fields: cartesian(&[[1.0, 0.0, 0.0]]), more: String::new() };
        // A quarter turn about z, then 10 m east and 2 m up.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let pose = format!(
            "<pose type=\"Structure\"><rotation type=\"Structure\"><w type=\"Float\">{h}</w><x type=\"Float\">0</x>\
             <y type=\"Float\">0</y><z type=\"Float\">{h}</z></rotation><translation type=\"Structure\"><x type=\"Float\">10</x>\
             <y type=\"Float\">0</y><z type=\"Float\">2</z></translation></pose>"
        );
        let two = Scan57 { fields: cartesian(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]), more: pose };
        let s = read(&e57(&[one, two])).unwrap();
        assert_eq!(all(&s), vec![[1000.0, 0.0, 0.0], [10000.0, 1000.0, 2000.0], [9000.0, 0.0, 2000.0]]);
    }

    #[test]
    fn scaled_integers_spherical_points_and_invalid_ones_come_in_as_the_standard_says() {
        // Millimetres in odd widths of bits, an intensity in a double, and a point marked invalid, left out.
        let fields = vec![
            scaled("cartesianX", -1000, 2000, 0.001, 0.0, &[0.0, 1.234, -0.5]),
            scaled("cartesianY", 0, 5, 0.5, 10.0, &[10.0, 11.5, 12.5]),
            float("cartesianZ", false, &[0.25, 0.5, 0.75]),
            float("intensity", false, &[0.0, 0.5, 1.0]),
            integer("cartesianInvalidState", 0, 2, &[0.0, 0.0, 2.0]),
        ];
        let s = read(&e57(&[Scan57 { fields, more: String::new() }])).unwrap();
        assert_eq!(all(&s), vec![[0.0, 10000.0, 250.0], [1234.0, 11500.0, 500.0]]);
        let grey: Vec<u8> = s.colours.unwrap().iter().map(|c| c[0]).collect();
        assert_eq!(grey, vec![0, 255], "intensity from the darkest to the lightest");
        // Range, azimuth and elevation: 2 m north, then 1 m straight up.
        let q = std::f64::consts::FRAC_PI_2;
        let fields = vec![
            float("sphericalRange", false, &[2.0, 1.0]),
            float("sphericalAzimuth", false, &[q, 0.0]),
            float("sphericalElevation", false, &[0.0, q]),
        ];
        let s = read(&e57(&[Scan57 { fields, more: String::new() }])).unwrap();
        assert_eq!(all(&s), vec![[0.0, 2000.0, 0.0], [0.0, 0.0, 1000.0]]);
    }

    #[test]
    fn colour_limits_say_what_full_colour_is() {
        let mut fields = cartesian(&[[0.0; 3], [1.0, 0.0, 0.0]]);
        for n in ["colorRed", "colorGreen", "colorBlue"] {
            fields.push(integer(n, 0, 65535, &[65535.0, 0.0]));
        }
        let more = "<colorLimits type=\"Structure\"><colorRedMinimum type=\"Integer\">0</colorRedMinimum>\
                    <colorRedMaximum type=\"Integer\">65535</colorRedMaximum></colorLimits>";
        let s = read(&e57(&[Scan57 { fields, more: more.into() }])).unwrap();
        assert_eq!(s.colours.unwrap(), vec![[255, 255, 255], [0, 0, 0]]);
    }

    #[test]
    fn a_broken_e57_says_why_and_never_crashes() {
        let good = e57(&[Scan57 { fields: cartesian(&grid(300)), more: String::new() }]);
        assert!(read(&good).is_ok());
        let says = |b: &[u8], what: &str| {
            let e = read(b).unwrap_err();
            assert!(e.contains(what), "{what}: {e}");
        };
        says(b"not an e57 at all", "ASTM-E57");
        says(&good[..40], "cut short");
        says(&good[..good.len() / 2], "cut short");
        let mut v2 = good.clone();
        v2[8] = 2;
        says(&v2, "version 2");
        let with = |from: &str, to: &str| rewritten(&good, from, to);
        assert!(read(&with("", "")).is_ok());
        says(&with("\"1\"></codecs>", "\"1\"><x/></codecs>"), "codec");
        says(&with("recordCount=\"300\"", "recordCount=\"900\""), "cut short");
        // A count only a little over what the packets hold is found out as they are unpacked.
        says(&with("recordCount=\"300\"", "recordCount=\"301\""), "missing");
        says(&with("<data3D", "<data3X"), "broken");
        says(&with("type=\"Float\" precision", "type=\"String\" precision"), "String");
        // Bytes changed all through it: an answer, never a panic.
        for i in (0..good.len()).step_by(3) {
            for v in [0u8, 0xFF, 0x7F, 0x80] {
                let mut b = good.clone();
                b[i] = v;
                let _ = read(&b);
            }
        }
    }

    /// The XML of a file.
    fn xml_text(file: &[u8]) -> String {
        let xml_at = u64::from_le_bytes(file[24..32].try_into().unwrap());
        let xml_len = u64::from_le_bytes(file[32..40].try_into().unwrap()) as usize;
        let mut raw = Vec::new();
        Paged::new(&mut std::io::Cursor::new(file), file.len() as u64)
            .take(xml_at / PAGE * DATA + xml_at % PAGE, xml_len, &mut raw)
            .unwrap();
        String::from_utf8(raw).unwrap()
    }

    /// The file with its XML changed in place (`from` to `to`), its length in the header with it.
    fn rewritten(file: &[u8], from: &str, to: &str) -> Vec<u8> {
        let xml_at = u64::from_le_bytes(file[24..32].try_into().unwrap());
        let start = xml_at / PAGE * DATA + xml_at % PAGE;
        let xml = xml_text(file).replace(from, to);
        let mut b = file[..xml_at as usize].to_vec();
        b[32..40].copy_from_slice(&(xml.len() as u64).to_le_bytes());
        for (j, c) in xml.bytes().enumerate() {
            let at = physical(start + j as u64) as usize;
            b.resize(b.len().max(at + 1), 0);
            b[at] = c;
        }
        b
    }

    /// Reads every point of a file under these caps, or says why not.
    fn within(file: &[u8], caps: Caps) -> Result<usize, String> {
        let mut n = 0;
        read_within(
            &mut std::io::Cursor::new(file),
            file.len() as u64,
            &mut |_| {
                n += 1;
                Ok(())
            },
            caps,
        )
        .map(|()| n)
    }

    fn plain(n: usize) -> Scan57 {
        Scan57 { fields: cartesian(&grid(n)), more: String::new() }
    }

    #[test]
    fn a_file_holds_no_more_scans_or_points_than_a_project_could_use_and_says_so_before_unpacking() {
        let two = e57(&[plain(5), plain(5)]);
        assert_eq!(within(&two, CAPS), Ok(10));
        assert_eq!(within(&two, Caps { scans: 2, ..CAPS }), Ok(10));
        let e = within(&two, Caps { scans: 1, ..CAPS }).unwrap_err();
        assert!(e.contains("2 scans") && e.contains("up to 1"), "{e}");
        // The budget is the file's, spent by one scan after another.
        assert_eq!(within(&two, Caps { points: 10, ..CAPS }), Ok(10));
        let e = within(&two, Caps { points: 9, ..CAPS }).unwrap_err();
        assert!(e.contains("scan 2") && e.contains("9 points together"), "{e}");
        // A count far past anything, or past what the section's bytes could hold, is refused with nothing unpacked.
        let big = e57(&[plain(300)]);
        for count in ["9000000000000", "9223372036854775807", "100000001"] {
            let e = read(&rewritten(&big, "recordCount=\"300\"", &format!("recordCount=\"{count}\""))).unwrap_err();
            assert!(e.contains("together") || e.contains("cut short"), "{count}: {e}");
        }
        // More fields than any scanner writes.
        let mut fields = cartesian(&grid(3));
        fields.extend((0..CAPS.fields).map(|k| float(&format!("extra{k}"), true, &[0.0, 1.0, 2.0])));
        let e = read(&e57(&[Scan57 { fields, more: String::new() }])).unwrap_err();
        assert!(e.contains("fields") && e.contains(&CAPS.fields.to_string()), "{e}");
    }

    #[test]
    fn fields_that_take_no_bits_cant_make_a_small_file_ask_for_more_points_than_the_cap() {
        // Three constants and one bit a point: eight points to the byte.
        let n = 40;
        let bit: Vec<f64> = (0..n).map(|i| (i % 2) as f64).collect();
        let fields = ["cartesianX", "cartesianY", "cartesianZ"].map(|k| integer(k, 5, 5, &vec![5.0; n])).to_vec();
        let file = e57(&[Scan57 { fields: [fields, vec![integer("intensity", 0, 1, &bit)]].concat(), more: String::new() }]);
        assert_eq!(within(&file, CAPS), Ok(n));
        let e = within(&file, Caps { points: 39, ..CAPS }).unwrap_err();
        assert!(e.contains("together"), "{e}");
        // Its count raised past what the bytes of its section hold: found out at once, not by unpacking.
        let lie = rewritten(&file, &format!("recordCount=\"{n}\""), "recordCount=\"4000000\"");
        let mut handed = 0;
        let e = read_within(
            &mut std::io::Cursor::new(&lie),
            lie.len() as u64,
            &mut |_| {
                handed += 1;
                Ok(())
            },
            CAPS,
        )
        .unwrap_err();
        assert!(e.contains("cut short") && e.contains("4000000"), "{e}");
        assert_eq!(handed, 0, "points were unpacked before the lie was found");
    }

    #[test]
    fn two_scans_cant_read_the_same_section() {
        let two = e57(&[plain(5), plain(5)]);
        let xml = xml_text(&two);
        let offsets: Vec<&str> = xml.split("fileOffset=\"").skip(1).filter_map(|s| s.split('"').next()).collect();
        assert_eq!(offsets.len(), 2);
        let same = rewritten(&two, &format!("fileOffset=\"{}\"", offsets[1]), &format!("fileOffset=\"{}\"", offsets[0]));
        let e = read(&same).unwrap_err();
        assert!(e.contains("scan 2") && e.contains("another scan"), "{e}");
    }

    /// A source that counts the bytes read from it, and seems `len` bytes long: past what it holds, it reads as noughts.
    struct Counted<'a> {
        bytes: &'a [u8],
        len: u64,
        at: u64,
        read: u64,
    }

    impl std::io::Read for Counted<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = (buf.len() as u64).min(self.len.saturating_sub(self.at)) as usize;
            for (k, b) in buf[..n].iter_mut().enumerate() {
                *b = self.bytes.get((self.at + k as u64) as usize).copied().unwrap_or(0);
            }
            self.at += n as u64;
            self.read += n as u64;
            Ok(n)
        }
    }

    impl std::io::Seek for Counted<'_> {
        fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
            self.at = match to {
                std::io::SeekFrom::Start(n) => n,
                std::io::SeekFrom::End(d) => self.len.saturating_add_signed(d),
                std::io::SeekFrom::Current(d) => self.at.saturating_add_signed(d),
            };
            Ok(self.at)
        }
    }

    /// The points `read` hands from a source, in metres to a thousandth of a millimetre, and how many bytes it read.
    fn streamed(bytes: &[u8], len: u64) -> (Result<Vec<[f64; 3]>, String>, u64) {
        let mut src = Counted { bytes, len, at: 0, read: 0 };
        let mut got = Vec::new();
        let r = super::read(&mut src, len, &mut |raw: Raw| {
            got.push(raw.p.map(|v| (v * 1e6).round() / 1e6));
            Ok(())
        });
        (r.map(|()| got), src.read)
    }

    #[test]
    fn an_e57_read_from_disk_a_window_at_a_time_gives_what_memory_gives() {
        let file = e57(&[plain(3000), plain(2000), plain(700)]);
        assert!(file.len() > 4 * WINDOW, "the file should take several windows");
        let mut disk = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut disk, &file).unwrap();
        let from_disk = super::super::read_from(disk.as_file_mut(), Format::E57, &Options::default(), None).unwrap();
        let from_memory = read(&file).unwrap();
        assert_eq!(from_disk.points.len(), 5700);
        assert_eq!(all(&from_disk), all(&from_memory));
    }

    #[test]
    fn a_pass_over_an_e57_reads_its_bytes_about_once_whatever_its_size() {
        let file = e57(&[plain(3000), plain(2000), plain(700)]);
        let (points, read) = streamed(&file, file.len() as u64);
        assert_eq!(points.unwrap().len(), 5700);
        assert!(read as f64 <= 1.2 * file.len() as f64, "{read} bytes read of {}", file.len());
        // The same file followed by gigabytes it never points at: its size costs nothing, neither memory nor reading.
        let huge = 5_u64 << 30;
        let (tail, read) = streamed(&file, huge);
        assert_eq!(tail.unwrap().len(), 5700);
        assert!(read as f64 <= 1.2 * file.len() as f64, "{read} bytes read of {huge}");
    }

    #[test]
    fn an_e57_says_so_when_its_xml_is_more_than_is_held() {
        let file = e57(&[plain(5)]);
        let mut b = file.clone();
        b[32..40].copy_from_slice(&(MAX_XML as u64 + 1).to_le_bytes());
        let (r, _) = streamed(&b, 2 * MAX_XML as u64);
        let e = r.unwrap_err();
        assert!(e.contains("XML") && e.contains(&MAX_XML.to_string()), "{e}");
        let (r, _) = streamed(&b, b.len() as u64);
        assert!(r.unwrap_err().contains("longer than the file"));
    }

    #[test]
    fn a_stream_may_not_wait_on_the_others_for_more_than_a_few_packets() {
        let file = e57(&[plain(300)]);
        assert_eq!(within(&file, CAPS), Ok(300));
        let e = within(&file, Caps { ahead: 8, ..CAPS }).unwrap_err();
        assert!(e.contains("ahead of the others") && e.contains("up to 8"), "{e}");
    }
}
