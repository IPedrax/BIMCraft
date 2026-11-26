//! A corpus of synthetic laser scans, the same every time, for measuring how point clouds are read
//! (`scripts/bench-clouds.sh`): a room 10 m by 8 m by 3 m, its floor, ceiling and walls scanned from its middle, each point with
//! an intensity and a colour by the surface it lies on, written as PTS, XYZ, binary PLY, LAS 1.4 and E57.
//!
//! `cargo run --release -p bimcraft-core --example cloud_corpus -- OUT_DIR [POINTS]` (10 million points unless given). Each file is
//! written as it's made, a point at a time, so a corpus of billions takes no more memory than one of thousands.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// A point: where it is, metres, its intensity (0 to 65535) and its colour (0 to 255).
struct Point {
    p: [f64; 3],
    intensity: u16,
    rgb: [u8; 3],
}

/// The `i`th of `n` points: a surface of the room picked by `i`, a place on it by a mix of `i`, and a millimetre of noise.
fn point(i: u64) -> Point {
    let mix = |mut x: u64| {
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        x ^ (x >> 33)
    };
    let r = mix(i.wrapping_add(0x9E37_79B9_7F4A_7C15));
    let (a, b) = ((r & 0xFFFF) as f64 / 65535.0, ((r >> 16) & 0xFFFF) as f64 / 65535.0);
    let noise = ((r >> 32) & 0xFF) as f64 / 255.0 * 0.002 - 0.001;
    let (w, d, h) = (10.0, 8.0, 3.0);
    let (p, intensity, rgb) = match i % 6 {
        0 => ([a * w, b * d, noise], 9000, [120, 110, 100]),
        1 => ([a * w, b * d, h + noise], 30000, [235, 235, 230]),
        2 => ([a * w, noise, b * h], 20000, [200, 180, 150]),
        3 => ([a * w, d + noise, b * h], 22000, [200, 180, 150]),
        4 => ([noise, a * d, b * h], 45000, [160, 60, 50]),
        _ => ([w + noise, a * d, b * h], 60000, [90, 110, 160]),
    };
    // Survey coordinates, far from the origin, as a real scan's are.
    let p = [p[0] + 500_000.0, p[1] + 7_000_000.0, p[2] + 120.0];
    let shade = (((r >> 40) & 0x1F) as i32 - 16) as i16;
    let intensity = (intensity + i32::from(shade) * 300).clamp(0, 65535) as u16;
    Point { p, intensity, rgb }
}

fn text(path: &Path, n: u64, pts: bool) -> std::io::Result<()> {
    let mut w = BufWriter::with_capacity(1 << 20, File::create(path)?);
    if pts {
        writeln!(w, "{n}")?;
    }
    for i in 0..n {
        let q = point(i);
        let [r, g, b] = q.rgb;
        if pts {
            writeln!(w, "{:.4} {:.4} {:.4} {} {r} {g} {b}", q.p[0], q.p[1], q.p[2], i32::from(q.intensity) - 32768)?;
        } else {
            writeln!(w, "{:.4} {:.4} {:.4} {r} {g} {b}", q.p[0], q.p[1], q.p[2])?;
        }
    }
    w.flush()
}

fn ply(path: &Path, n: u64) -> std::io::Result<()> {
    let mut w = BufWriter::with_capacity(1 << 20, File::create(path)?);
    write!(
        w,
        "ply\nformat binary_little_endian 1.0\nelement vertex {n}\nproperty double x\nproperty double y\nproperty double z\n\
         property ushort intensity\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n"
    )?;
    for i in 0..n {
        let q = point(i);
        for v in q.p {
            w.write_all(&v.to_le_bytes())?;
        }
        w.write_all(&q.intensity.to_le_bytes())?;
        w.write_all(&q.rgb)?;
    }
    w.flush()
}

/// LAS 1.4, point format 7 (a place, an intensity and a colour), a millimetre to a unit.
fn las(path: &Path, n: u64) -> std::io::Result<()> {
    let (header, len) = (375_usize, 36_usize);
    let (scale, offset) = (0.001, [500_000.0, 7_000_000.0, 0.0]);
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let mut w = BufWriter::with_capacity(1 << 20, File::create(path)?);
    w.write_all(&vec![0u8; header])?;
    for i in 0..n {
        let q = point(i);
        let mut r = [0u8; 36];
        for k in 0..3 {
            lo[k] = lo[k].min(q.p[k]);
            hi[k] = hi[k].max(q.p[k]);
            let v = ((q.p[k] - offset[k]) / scale).round() as i32;
            r[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        r[12..14].copy_from_slice(&q.intensity.to_le_bytes());
        for (k, c) in q.rgb.iter().enumerate() {
            r[30 + 2 * k..32 + 2 * k].copy_from_slice(&(u16::from(*c) * 257).to_le_bytes());
        }
        w.write_all(&r[..len])?;
    }
    let mut f = w.into_inner().map_err(std::io::IntoInnerError::into_error)?;
    let mut h = vec![0u8; header];
    h[0..4].copy_from_slice(b"LASF");
    (h[24], h[25]) = (1, 4);
    h[94..96].copy_from_slice(&(header as u16).to_le_bytes());
    h[96..100].copy_from_slice(&(header as u32).to_le_bytes());
    h[104] = 7;
    h[105..107].copy_from_slice(&(len as u16).to_le_bytes());
    for k in 0..3 {
        h[131 + 8 * k..139 + 8 * k].copy_from_slice(&scale.to_le_bytes());
        h[155 + 8 * k..163 + 8 * k].copy_from_slice(&offset[k].to_le_bytes());
        h[179 + 16 * k..187 + 16 * k].copy_from_slice(&hi[k].to_le_bytes());
        h[187 + 16 * k..195 + 16 * k].copy_from_slice(&lo[k].to_le_bytes());
    }
    h[247..255].copy_from_slice(&n.to_le_bytes());
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&h)
}

/// An E57 file's pages, written as they fill: 1020 bytes of each page are the file's, the last four its checksum (left nought,
/// which the reader doesn't check).
struct Pages {
    w: BufWriter<File>,
    /// Logical bytes written.
    at: u64,
}

impl Pages {
    fn write(&mut self, mut b: &[u8]) -> std::io::Result<()> {
        while !b.is_empty() {
            let room = 1020 - (self.at % 1020) as usize;
            let n = room.min(b.len());
            self.w.write_all(&b[..n])?;
            self.at += n as u64;
            b = &b[n..];
            if self.at.is_multiple_of(1020) {
                self.w.write_all(&[0; 4])?;
            }
        }
        Ok(())
    }

    fn physical(logical: u64) -> u64 {
        logical / 1020 * 1024 + logical % 1020
    }
}

/// E57: one scan, its coordinates and intensity as single floats and its colour as bytes, a packet of 1024 points at a time.
fn e57(path: &Path, n: u64) -> std::io::Result<()> {
    let mut pages = Pages { w: BufWriter::with_capacity(1 << 20, File::create(path)?), at: 0 };
    pages.write(&[0u8; 48])?;
    let section = pages.at;
    pages.write(&[0u8; 32])?;
    let per = 1024_u64;
    let mut i = 0;
    while i < n {
        let k = per.min(n - i);
        let mut streams: Vec<Vec<u8>> = vec![Vec::new(); 7];
        for j in i..i + k {
            let q = point(j);
            for (s, v) in streams.iter_mut().zip(q.p) {
                s.extend((v as f32).to_le_bytes());
            }
            streams[3].extend((f32::from(q.intensity) / 65535.0).to_le_bytes());
            for c in 0..3 {
                streams[4 + c].push(q.rgb[c]);
            }
        }
        let mut p = vec![1u8, 0, 0, 0];
        p.extend((streams.len() as u16).to_le_bytes());
        for s in &streams {
            p.extend((s.len() as u16).to_le_bytes());
        }
        for s in &streams {
            p.extend(s);
        }
        while !p.len().is_multiple_of(4) {
            p.push(0);
        }
        let len = (p.len() as u16 - 1).to_le_bytes();
        (p[2], p[3]) = (len[0], len[1]);
        pages.write(&p)?;
        i += k;
    }
    while !pages.at.is_multiple_of(4) {
        pages.write(&[0])?;
    }
    let end = pages.at;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<e57Root type=\"Structure\" xmlns=\"http://www.astm.org/COMMIT/E57/2010-e57-v1.0\">\n\
         <formatName type=\"String\"><![CDATA[ASTM E57 3D Imaging Data File]]></formatName>\n\
         <data3D type=\"Vector\" allowHeterogeneousChildren=\"1\">\n<vectorChild type=\"Structure\">\n\
         <name type=\"String\"><![CDATA[corpus]]></name>\n\
         <points type=\"CompressedVector\" fileOffset=\"{}\" recordCount=\"{n}\">\n<prototype type=\"Structure\">\
         <cartesianX type=\"Float\" precision=\"single\"/><cartesianY type=\"Float\" precision=\"single\"/>\
         <cartesianZ type=\"Float\" precision=\"single\"/><intensity type=\"Float\" precision=\"single\"/>\
         <colorRed type=\"Integer\" minimum=\"0\" maximum=\"255\">0</colorRed>\
         <colorGreen type=\"Integer\" minimum=\"0\" maximum=\"255\">0</colorGreen>\
         <colorBlue type=\"Integer\" minimum=\"0\" maximum=\"255\">0</colorBlue></prototype>\n\
         <codecs type=\"Vector\" allowHeterogeneousChildren=\"1\"></codecs>\n</points>\n</vectorChild>\n</data3D>\n\
         <images2D type=\"Vector\" allowHeterogeneousChildren=\"1\"/>\n</e57Root>\n",
        Pages::physical(section)
    );
    let xml_at = pages.at;
    pages.write(xml.as_bytes())?;
    let rest = (1020 - pages.at % 1020) % 1020;
    pages.write(&vec![0u8; rest as usize])?;
    let total = pages.at / 1020 * 1024;
    let mut f = pages.w.into_inner().map_err(std::io::IntoInnerError::into_error)?;
    // The section's header, then the file's, now that they're known.
    let mut s = vec![1u8, 0, 0, 0, 0, 0, 0, 0];
    s.extend((end - section).to_le_bytes());
    s.extend(Pages::physical(section + 32).to_le_bytes());
    s.extend(0u64.to_le_bytes());
    f.seek(SeekFrom::Start(Pages::physical(section)))?;
    f.write_all(&s)?;
    let mut h = b"ASTM-E57".to_vec();
    h.extend(1u32.to_le_bytes());
    h.extend(0u32.to_le_bytes());
    h.extend(total.to_le_bytes());
    h.extend(Pages::physical(xml_at).to_le_bytes());
    h.extend((xml.len() as u64).to_le_bytes());
    h.extend(1024u64.to_le_bytes());
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&h)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = args.first() else {
        eprintln!("usage: cloud_corpus OUT_DIR [POINTS]");
        std::process::exit(2);
    };
    let n: u64 = args.get(1).and_then(|s| s.replace('_', "").parse().ok()).unwrap_or(10_000_000);
    let dir = Path::new(out);
    let made = std::fs::create_dir_all(dir)
        .and_then(|()| text(&dir.join("corpus.pts"), n, true))
        .and_then(|()| text(&dir.join("corpus.xyz"), n, false))
        .and_then(|()| ply(&dir.join("corpus.ply"), n))
        .and_then(|()| las(&dir.join("corpus.las"), n))
        .and_then(|()| e57(&dir.join("corpus.e57"), n));
    match made {
        Ok(()) => println!("{n} points as corpus.pts, .xyz, .ply, .las and .e57 in {}", dir.display()),
        Err(e) => {
            eprintln!("the corpus couldn't be written: {e}");
            std::process::exit(1);
        }
    }
}
