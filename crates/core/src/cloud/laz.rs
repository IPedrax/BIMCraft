//! LAZ: LAS whose points LASzip compressed, and COPC (`.copc.laz`), which is LAZ 1.4 with its chunks indexed. The header
//! is LAS's ([`LasHead`]); the record of its own LASzip adds (user "laszip encoded", record 22204) says how the points were
//! compressed, and the laz crate (Apache-2.0, a Rust port of LASzip, whose format is public) gives them back as LAS records.

use std::io::SeekFrom;

use super::{LasHead, Raw, Source, unread};

/// How long the header of a record of a file's own is, bytes.
const VLR_HEADER: u64 = 54;
/// LASzip's record of its own: who wrote it and its number.
const LASZIP_USER: &[u8] = b"laszip encoded";
const LASZIP_RECORD: u16 = 22204;
/// How much is decompressed at a time, bytes.
const BATCH: usize = 1 << 20;
/// How much is decompressed at a time when its chunks are decompressed side by side, bytes.
const PAR_BATCH: u64 = 128 << 20;
/// The most a chunk may decompress to and still be decompressed beside the others, bytes: past it, the file is decompressed
/// as one stream, a megabyte at a time.
const MAX_CHUNK: u64 = 64 << 20;

/// What a failed decompression says.
fn broken(e: impl std::fmt::Display) -> String {
    format!("its compressed points can't be read: {e}")
}

/// Hands each point of a LAZ file, whose header `h` has been read, to `f` with its place among the file's points. LASzip
/// can't step within a chunk, so every point is handed and `f` picks.
pub(super) fn read(src: &mut dyn Source, size: u64, h: &LasHead, f: &mut impl FnMut(u64, Raw) -> Result<(), String>) -> Result<(), String> {
    let data = laszip_record(src, h)?;
    let vlr = laz::LazVlr::from_buffer(&data).map_err(broken)?;
    let len = vlr.items_size();
    if len != h.len as u64 {
        return Err(format!(
            "its point records are {} bytes, but LASzip's record says its compressed points are {len}: it can't be read",
            h.len
        ));
    }
    // The compressor: 1 is one stream; 2 and 3, in chunks with a table of them, which decompress side by side.
    let compressor = data.get(..2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    if matches!(compressor, 2 | 3)
        && let Some(chunks) = chunks(src, size, h, &vlr)?
    {
        return side_by_side(src, h, &vlr, &chunks, f);
    }
    src.seek(SeekFrom::Start(h.start)).map_err(unread)?;
    let mut d = laz::LasZipDecompressor::new(&mut *src, vlr).map_err(broken)?;
    let per = (BATCH / h.len).max(1);
    let mut buf = vec![0u8; per * h.len];
    let mut at = 0_u64;
    while at < h.count {
        let n = (h.count - at).min(per as u64) as usize;
        let part = buf.get_mut(..n * h.len).ok_or_else(|| broken("a batch of points"))?;
        d.decompress_many(part).map_err(broken)?;
        for r in part.chunks_exact(h.len) {
            f(at, h.point(r)?)?;
            at += 1;
        }
    }
    Ok(())
}

/// The data of LASzip's record of its own, found among the file's records before its points.
fn laszip_record(src: &mut dyn Source, h: &LasHead) -> Result<Vec<u8>, String> {
    // Every record lies between the header and the points: that bounds how many there can be.
    let room = h.start.saturating_sub(h.header);
    if u64::from(h.vlrs) > room / VLR_HEADER {
        return Err(format!("it says it has {} records of its own, more than fit in the {room} bytes before its points", h.vlrs));
    }
    let mut at = h.header;
    for _ in 0..h.vlrs {
        let mut head = [0u8; VLR_HEADER as usize];
        if at + VLR_HEADER > h.start {
            return Err("its records of its own run into its points".into());
        }
        src.seek(SeekFrom::Start(at)).map_err(unread)?;
        src.read_exact(&mut head).map_err(unread)?;
        let user = &head[2..18];
        let user = &user[..user.iter().position(|&b| b == 0).unwrap_or(user.len())];
        let id = u16::from_le_bytes([head[18], head[19]]);
        let len = u64::from(u16::from_le_bytes([head[20], head[21]]));
        let end = at + VLR_HEADER + len;
        if end > h.start {
            return Err("its records of its own run into its points".into());
        }
        if user == LASZIP_USER && id == LASZIP_RECORD {
            let mut data = vec![0u8; len as usize];
            src.read_exact(&mut data).map_err(unread)?;
            return Ok(data);
        }
        at = end;
    }
    Err("its points are compressed, but it has no LASzip record to say how: it can't be read".into())
}

/// A chunk of the points: how many it holds and how many bytes they're compressed to.
struct Chunk {
    points: u64,
    bytes: u64,
}

/// The chunks the points are compressed in, from the table the file keeps of them, checked before anything is held: their
/// points add up to the header's, their bytes fit between the points' start and the table, and a table of more chunks than
/// the file has points or bytes isn't read. None when the file has no table, or a chunk too big to decompress beside the
/// others: the file is then decompressed as one stream.
fn chunks(src: &mut dyn Source, size: u64, h: &LasHead, vlr: &laz::LazVlr) -> Result<Option<Vec<Chunk>>, String> {
    let mut b = [0u8; 8];
    let i64_at = |src: &mut dyn Source, to: SeekFrom, b: &mut [u8; 8]| -> Option<i64> {
        src.seek(to).ok()?;
        src.read_exact(b).ok()?;
        Some(i64::from_le_bytes(*b))
    };
    let Some(mut table) = i64_at(src, SeekFrom::Start(h.start), &mut b) else {
        return Err(broken("it ends before its points"));
    };
    // A writer that couldn't go back puts the table's place at the end of the file.
    if table <= h.start as i64 && size >= 8 {
        table = i64_at(src, SeekFrom::Start(size - 8), &mut b).unwrap_or(-1);
    }
    let Ok(table) = u64::try_from(table) else { return Ok(None) };
    let first = h.start.saturating_add(8);
    if table < first || table.saturating_add(8) > size {
        // No table: the laz crate reads chunks of a fixed size without one, and refuses chunks of their own sizes.
        return Ok(None);
    }
    src.seek(SeekFrom::Start(table + 4)).map_err(unread)?;
    let mut n = [0u8; 4];
    src.read_exact(&mut n).map_err(unread)?;
    let count = u64::from(u32::from_le_bytes(n));
    if count > h.count.saturating_add(1) || count > size {
        return Err(format!("its table of compressed chunks lists {count}, more than its {} points could fill", h.count));
    }
    src.seek(SeekFrom::Start(table)).map_err(unread)?;
    let variable = vlr.uses_variable_size_chunks();
    let read = laz::laszip::ChunkTable::read(&mut &mut *src, variable).map_err(broken)?;
    let fixed = u64::from(vlr.chunk_size());
    let (mut chunks, mut points, mut bytes) = (Vec::new(), 0_u64, 0_u64);
    for e in read.as_ref() {
        let held = if variable { e.point_count } else { fixed.min(h.count.saturating_sub(points)) };
        points = points.saturating_add(held);
        bytes = bytes.saturating_add(e.byte_count);
        chunks.push(Chunk { points: held, bytes: e.byte_count });
    }
    if points != h.count {
        return Err(format!("its table of compressed chunks holds {points} points, and its header says {}", h.count));
    }
    if bytes > table - first {
        return Err(format!("its table of compressed chunks says they take {bytes} bytes; {} lie before the table", table - first));
    }
    let wide = h.len as u64;
    Ok(chunks.iter().all(|c| c.points.saturating_mul(wide) <= MAX_CHUNK).then_some(chunks))
}

/// A decompressor of one chunk's bytes, as LASzip's items for the points say: point by point (items of version 1 and 2), or
/// in layers (3 and 4).
fn decompressor<'a>(
    vlr: &laz::LazVlr,
    bytes: &'a [u8],
) -> Result<Box<dyn laz::record::RecordDecompressor<std::io::Cursor<&'a [u8]>> + Send + 'a>, String> {
    use laz::record::{LayeredPointRecordDecompressor, RecordDecompressor, SequentialPointRecordDecompressor};
    let input = std::io::Cursor::new(bytes);
    let mut d: Box<dyn RecordDecompressor<_> + Send> = match vlr.items().first().map(laz::LazItem::version) {
        Some(1 | 2) => Box::new(SequentialPointRecordDecompressor::new(input)),
        Some(3 | 4) => Box::new(LayeredPointRecordDecompressor::new(input)),
        Some(v) => return Err(broken(format!("items of version {v}, which LASzip doesn't write"))),
        None => return Err(broken("it lists no items")),
    };
    d.set_fields_from(vlr.items()).map_err(broken)?;
    Ok(d)
}

/// Hands each point to `f` with its place, its chunks decompressed side by side: as many at a time as there are threads,
/// twice over, and no more than [`PAR_BATCH`] bytes of points, then handed on in the file's order.
fn side_by_side(
    src: &mut dyn Source,
    h: &LasHead,
    vlr: &laz::LazVlr,
    chunks: &[Chunk],
    f: &mut impl FnMut(u64, Raw) -> Result<(), String>,
) -> Result<(), String> {
    use rayon::prelude::*;
    let wide = h.len as u64;
    let most = rayon::current_num_threads().max(1) * 2;
    src.seek(SeekFrom::Start(h.start + 8)).map_err(unread)?;
    let (mut packed, mut out) = (Vec::new(), Vec::new());
    let (mut i, mut at) = (0, 0_u64);
    while i < chunks.len() {
        let (mut j, mut points, mut bytes) = (i, 0_u64, 0_u64);
        while let Some(c) = chunks.get(j) {
            let more = points.saturating_add(c.points).saturating_mul(wide);
            if j > i && (j - i >= most || more > PAR_BATCH) {
                break;
            }
            points = points.saturating_add(c.points);
            bytes = bytes.saturating_add(c.bytes);
            j += 1;
        }
        // Each is within what `chunks` checked: the points of a batch fit in it, and its bytes in the file.
        let to = |n: u64| usize::try_from(n).map_err(|_| broken("a batch larger than memory"));
        packed.resize(to(bytes)?, 0);
        src.read_exact(&mut packed).map_err(|e| broken(format!("its chunks are cut short: {e}")))?;
        out.resize(to(points.saturating_mul(wide))?, 0);
        let mut pieces = Vec::with_capacity(j - i);
        let (mut rest_in, mut rest_out) = (packed.as_slice(), out.as_mut_slice());
        for c in chunks.get(i..j).unwrap_or(&[]) {
            let (one_in, more_in) = rest_in.split_at(to(c.bytes)?.min(rest_in.len()));
            let n = to(c.points * wide)?.min(rest_out.len());
            let (one_out, more_out) = std::mem::take(&mut rest_out).split_at_mut(n);
            pieces.push((one_in, one_out));
            (rest_in, rest_out) = (more_in, more_out);
        }
        pieces
            .into_par_iter()
            .map(|(bytes, points)| decompressor(vlr, bytes)?.decompress_many(points).map_err(broken))
            .collect::<Result<Vec<()>, String>>()?;
        for r in out.chunks_exact(h.len) {
            f(at, h.point(r)?)?;
            at += 1;
        }
        i = j;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Seek, SeekFrom, Write};

    use super::super::tests::las;
    use super::super::{Format, Options, read, read_from};

    fn mm() -> Options {
        Options { scale: 1.0, ..Options::default() }
    }

    /// How the points of a LAZ file made in a test are cut into chunks.
    #[derive(Clone, Copy, Debug)]
    enum Chunks {
        /// So many points each, as LASzip writes by default (50 000).
        Fixed(u32),
        /// So many points each, written as chunks of their own size, as COPC does.
        Variable(usize),
    }

    /// A record of a file's own: user, record id and its data.
    fn vlr(user: &str, id: u16, data: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 54];
        b[2..2 + user.len()].copy_from_slice(user.as_bytes());
        b[18..20].copy_from_slice(&id.to_le_bytes());
        b[20..22].copy_from_slice(&(data.len() as u16).to_le_bytes());
        b.extend_from_slice(data);
        b
    }

    /// The LAS file `las` (written with no records of its own) as LAZ, its points compressed by the laz crate, with the
    /// records `before` ahead of LASzip's.
    fn laz_with(las: &[u8], chunks: Chunks, before: &[Vec<u8>]) -> Vec<u8> {
        let header = u16::from_le_bytes([las[94], las[95]]) as usize;
        let start = u32::from_le_bytes(las[96..100].try_into().unwrap()) as usize;
        assert_eq!(header, start, "the test's LAS has no records of its own");
        let format = las[104];
        let builder = laz::LazVlrBuilder::default().with_point_format(format, 0).unwrap();
        let built = match chunks {
            Chunks::Fixed(n) => builder.with_fixed_chunk_size(n),
            Chunks::Variable(_) => builder.with_variable_chunk_size(),
        }
        .build();
        let len = built.items_size() as usize;
        assert_eq!(len, u16::from_le_bytes([las[105], las[106]]) as usize);
        let mut data = Vec::new();
        built.write_to(&mut data).unwrap();
        let mut records: Vec<u8> = before.concat();
        records.extend(vlr("laszip encoded", 22204, &data));
        let mut head = las[..header].to_vec();
        head[96..100].copy_from_slice(&((header + records.len()) as u32).to_le_bytes());
        head[100..104].copy_from_slice(&(before.len() as u32 + 1).to_le_bytes());
        head[104] |= 0x80;
        let mut out = Cursor::new(Vec::new());
        out.write_all(&head).unwrap();
        out.write_all(&records).unwrap();
        let points = &las[start..];
        let mut c = laz::LasZipCompressor::new(out, built).unwrap();
        match chunks {
            Chunks::Fixed(_) => c.compress_many(points).unwrap(),
            Chunks::Variable(n) => c.compress_chunks(points.chunks(n * len)).unwrap(),
        }
        c.done().unwrap();
        let mut out = c.into_inner();
        out.seek(SeekFrom::End(0)).unwrap();
        out.into_inner()
    }

    fn laz_of(las: &[u8], chunks: Chunks) -> Vec<u8> {
        laz_with(las, chunks, &[])
    }

    fn records(n: i32) -> Vec<([i32; 3], u16, [u16; 3])> {
        (0..n)
            .map(|i| {
                (
                    [i * 3, (i * 7) % 1013 - 500, (i % 97) * (i % 89)],
                    (i % 5000 * 13) as u16,
                    [(i % 200 * 300) as u16, (i % 9000 * 5) as u16, 9],
                )
            })
            .collect()
    }

    /// The LAS files the tests compress: point formats 0 to 3 in LAS 1.2, 6 and 7 in LAS 1.4.
    fn sample_las(n: i32) -> Vec<Vec<u8>> {
        let pts = records(n);
        vec![
            las(2, 0, [0.01; 3], [5.0, 6.0, 7.0], 0, &pts),
            las(2, 1, [0.01; 3], [0.0; 3], 0, &pts),
            las(2, 2, [0.001; 3], [0.0; 3], 0, &pts),
            las(2, 3, [0.01; 3], [5.0, 6.0, 7.0], 0, &pts),
            las(4, 6, [0.001; 3], [0.0; 3], 0, &pts),
            las(4, 7, [0.001; 3], [100.0, 0.0, 0.0], 0, &pts),
        ]
    }

    fn options() -> [Options; 3] {
        [mm(), Options { cap: 37, ..mm() }, Options { spacing: 3.0, ..mm() }]
    }

    #[test]
    fn laz_reads_as_the_las_it_was_compressed_from_across_its_chunks() {
        for (k, las) in sample_las(1000).iter().enumerate() {
            for chunks in [Chunks::Fixed(128), Chunks::Fixed(50_000), Chunks::Variable(300)] {
                let laz = laz_of(las, chunks);
                assert!(laz.len() < las.len(), "{k} {chunks:?}: compressed it's {} of {} bytes", laz.len(), las.len());
                for o in options() {
                    let want = read(las, Format::Las, &o).unwrap();
                    assert_eq!(read(&laz, Format::Laz, &o), Ok(want.clone()), "{k} {chunks:?} {o:?}");
                    // A LAZ file named .las is read as what it is.
                    assert_eq!(read(&laz, Format::Las, &o), Ok(want), "{k} {chunks:?} {o:?} as .las");
                }
            }
        }
        // An uncompressed file named .laz is read too.
        let las = &sample_las(10)[3];
        assert_eq!(read(las, Format::Laz, &mm()), read(las, Format::Las, &mm()));
    }

    #[test]
    fn laz_of_more_than_one_default_chunk_reads_every_point() {
        let las = las(2, 3, [0.001; 3], [0.0; 3], 0, &records(120_000));
        let laz = laz_of(&las, Chunks::Fixed(50_000));
        let s = read(&laz, Format::Laz, &mm()).unwrap();
        assert_eq!((s.read, s.points.len()), (120_000, 120_000));
        assert_eq!(s, read(&las, Format::Las, &mm()).unwrap());
    }

    #[test]
    fn copc_reads_as_the_laz_it_is() {
        // COPC: LAZ 1.4, its info record first, chunks of their own sizes.
        let info = vlr("copc", 1, &[0; 160]);
        for las in &sample_las(1000)[4..] {
            let copc = laz_with(las, Chunks::Variable(256), std::slice::from_ref(&info));
            for o in options() {
                assert_eq!(read(&copc, Format::Laz, &o), read(las, Format::Las, &o), "{o:?}");
            }
        }
        assert_eq!(Format::from_extension(std::path::Path::new("scan.copc.laz").extension().unwrap().to_str().unwrap()), Ok(Format::Laz));
    }

    #[test]
    fn a_laz_file_read_from_disk_a_piece_at_a_time_reads_as_its_bytes_do() {
        let dir = tempfile::tempdir().unwrap();
        for (k, las) in sample_las(700).iter().enumerate() {
            let laz = laz_of(las, Chunks::Fixed(100));
            let path = dir.path().join(format!("{k}.laz"));
            std::fs::write(&path, &laz).unwrap();
            for o in options() {
                let mut file = std::fs::File::open(&path).unwrap();
                assert_eq!(read_from(&mut file, Format::Laz, &o, None), read(&laz, Format::Laz, &o), "{k} {o:?}");
            }
        }
    }

    #[test]
    fn hostile_laz_files_are_refused_and_never_crash_it() {
        let o = mm();
        let las = las(2, 3, [0.01; 3], [0.0; 3], 0, &records(100));
        let good = laz_of(&las, Chunks::Fixed(32));
        // Where LASzip's record's data starts: after the header (227) and its own 54-byte header.
        let data = 227 + 54;
        let start = u32::from_le_bytes(good[96..100].try_into().unwrap()) as usize;
        // No record says how it was compressed.
        let mut missing = las.clone();
        missing[104] |= 0x80;
        // LASzip's record is there, but under another name.
        let mut renamed = good.clone();
        renamed[227 + 2] = b'X';
        // Its record says a compressor there's none of.
        let mut compressor = good.clone();
        compressor[data..data + 2].copy_from_slice(&99u16.to_le_bytes());
        // Its record says items no point is made of.
        let mut items = good.clone();
        let n_items = data + 32;
        items[n_items..n_items + 2].copy_from_slice(&0u16.to_le_bytes());
        // Its records run past the points.
        let mut vlrs = good.clone();
        vlrs[100..104].copy_from_slice(&u32::MAX.to_le_bytes());
        // Its record is longer than the space before the points.
        let mut long = good.clone();
        long[227 + 20..227 + 22].copy_from_slice(&u16::MAX.to_le_bytes());
        // Its records are a different length from what LASzip's items make.
        let mut length = good.clone();
        length[105..107].copy_from_slice(&40u16.to_le_bytes());
        // Its chunk table says more chunks than there could be.
        let mut table = good.clone();
        let offset = i64::from_le_bytes(good[start..start + 8].try_into().unwrap()) as usize;
        table[offset + 4..offset + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        // It says more points than it holds.
        let mut more = good.clone();
        more[107..111].copy_from_slice(&1_000_000u32.to_le_bytes());
        for (bytes, says) in [
            (missing, "LASzip"),
            (renamed, "LASzip"),
            (compressor, "compressed"),
            (items, "compressed"),
            (vlrs, "records"),
            (long, "records"),
            (length, "40 bytes"),
            (table, "chunk"),
            (more, "compressed"),
            (good[..start + 20].to_vec(), "compressed"),
            (good[..good.len() / 2].to_vec(), "compressed"),
        ] {
            let e = read(&bytes, Format::Laz, &o).unwrap_err();
            assert!(e.contains(says), "{says}: {e}");
        }
        // Every byte of it changed, and cut short anywhere: an answer each time, never a panic.
        for i in 0..good.len() {
            for v in [0xFF, good[i] ^ 0x5A] {
                let mut bytes = good.clone();
                bytes[i] = v;
                let _ = read(&bytes, Format::Laz, &o);
            }
            if i % 7 == 0 {
                let _ = read(&good[..i], Format::Laz, &o);
            }
        }
        // And of the layered compression LAS 1.4's formats take.
        let las14 = super::super::tests::las(4, 7, [0.01; 3], [0.0; 3], 0, &records(100));
        let good = laz_of(&las14, Chunks::Variable(32));
        for i in 0..good.len() {
            for v in [0xFF, good[i] ^ 0x5A] {
                let mut bytes = good.clone();
                bytes[i] = v;
                let _ = read(&bytes, Format::Laz, &o);
            }
            if i % 7 == 0 {
                let _ = read(&good[..i], Format::Laz, &o);
            }
        }
    }

    /// A reader that counts the bytes read through it.
    struct Counted<R> {
        inner: R,
        read: u64,
    }

    impl<R: std::io::Read> std::io::Read for Counted<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read += n as u64;
            Ok(n)
        }
    }

    impl<R: Seek> Seek for Counted<R> {
        fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(to)
        }
    }

    #[test]
    fn a_laz_file_is_decompressed_once_however_hard_it_is_thinned() {
        let las = las(2, 3, [0.001; 3], [0.0; 3], 0, &records(20_000));
        for chunks in [Chunks::Fixed(1000), Chunks::Fixed(50_000), Chunks::Variable(700)] {
            let laz = laz_of(&las, chunks);
            for o in options() {
                let mut r = Counted { inner: Cursor::new(&laz), read: 0 };
                let got = read_from(&mut r, Format::Laz, &o, None);
                assert_eq!(got, read(&las, Format::Las, &o), "{chunks:?} {o:?}");
                // Its header, its records and its points, each read once.
                assert!(r.read < laz.len() as u64 * 13 / 10, "{chunks:?} {o:?}: {} bytes read of {}", r.read, laz.len());
            }
        }
    }

    /// The LAZ file `laz` with its table of chunks written again from `entries`, at its end.
    fn with_table(laz: &[u8], entries: &[(u64, u64)], variable: bool) -> Vec<u8> {
        let start = u32::from_le_bytes(laz[96..100].try_into().unwrap()) as usize;
        let mut b = laz.to_vec();
        let at = b.len() as i64;
        b[start..start + 8].copy_from_slice(&at.to_le_bytes());
        let mut table = laz::laszip::ChunkTable::default();
        for &(point_count, byte_count) in entries {
            table.push(laz::laszip::ChunkTableEntry { point_count, byte_count });
        }
        let vlr = if variable {
            laz::LazVlrBuilder::default().with_point_format(3, 0).unwrap().with_variable_chunk_size().build()
        } else {
            laz::LazVlrBuilder::default().with_point_format(3, 0).unwrap().with_fixed_chunk_size(1000).build()
        };
        table.write_to(&mut b, &vlr).unwrap();
        b
    }

    #[test]
    fn a_chunk_table_that_lies_is_refused_before_anything_is_held() {
        let las = las(2, 3, [0.001; 3], [0.0; 3], 0, &records(5000));
        let laz = laz_of(&las, Chunks::Variable(1000));
        let o = mm();
        // Chunks of more bytes than the file holds: refused, never a buffer of their size.
        let huge = with_table(&laz, &[(1000, u64::from(u32::MAX)); 5], true);
        let e = read(&huge, Format::Laz, &o).unwrap_err();
        assert!(e.contains("chunk"), "{e}");
        // Chunks whose points don't add up to the header's count.
        let short = with_table(&laz, &[(1000, 10); 4], true);
        let e = read(&short, Format::Laz, &o).unwrap_err();
        assert!(e.contains("chunk") || e.contains("cut short"), "{e}");
        let over = with_table(&laz, &[(4000, 10); 5], true);
        let e = read(&over, Format::Laz, &o).unwrap_err();
        assert!(e.contains("chunk"), "{e}");
        // A chunk of a billion points: refused, never a buffer for them.
        let wide = with_table(&laz, &[(1_000_000_000, 10)], true);
        assert!(read(&wide, Format::Laz, &o).is_err());
    }
}
