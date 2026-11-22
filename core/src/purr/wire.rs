//! The .purr container on disk: header, footer and the index (table of contents).
//! See docs/PURR-FORMAT.md. Everything read from a file is treated as hostile: every
//! count, length and reference is checked before use, and nothing is allocated on a
//! file's say-so beyond the bytes that are actually there.

use crate::{Error, Result};

pub const MAGIC: [u8; 8] = [0x89, b'P', b'U', b'R', b'R', 0x0D, 0x0A, 0x1A];
pub const HEADER_LEN: usize = 96;
pub const FOOTER_MAGIC: [u8; 8] = *b"PURREND1";
pub const FOOTER_LEN: usize = 64;
pub const VERSION_MAJOR: u8 = 1;
pub const VERSION_MINOR: u8 = 0;

/// Pack codecs.
pub const CODEC_STORE: u8 = 0;
pub const CODEC_ZSTD: u8 = 1;
pub const CODEC_XZ: u8 = 2;
pub const CODEC_PPMD7: u8 = 3;

/// File transforms.
pub const TRANSFORM_NONE: u8 = 0;
pub const TRANSFORM_LEPTON: u8 = 1;

/// Hard limits a reader enforces (far above anything the writer produces).
pub const MAX_INDEX_RAW: u64 = 256 << 20;
pub const MAX_PACK_RAW: u64 = 1 << 30;
pub const MAX_BLOB: u64 = 256 << 20;

/// Chunks under this size carry a 4-byte check, larger ones 8 bytes.
pub const SMALL_CHUNK: u64 = 16 * 1024;
pub fn check_len(chunk_len: u64) -> usize {
    if chunk_len < SMALL_CHUNK { 4 } else { 8 }
}
/// The stored check: the first 4 or 8 bytes of the chunk's BLAKE3 hash, as a little-endian number.
pub fn check_of(hash: &[u8; 32], chunk_len: u64) -> u64 {
    let n = check_len(chunk_len);
    let mut b = [0u8; 8];
    b[..n].copy_from_slice(&hash[..n]);
    u64::from_le_bytes(b)
}

fn damaged(what: &str) -> Error {
    Error::Damaged(format!("the archive's {} is not valid", what))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Kdf {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: [u8; 16],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub major: u8,
    pub minor: u8,
    pub kdf: Option<Kdf>,
    pub key_check: [u8; 16],
}

impl Header {
    pub fn encrypted(&self) -> bool {
        self.kdf.is_some()
    }

    /// The bytes the password check is computed over (everything before the check itself).
    pub fn check_prefix(bytes: &[u8; HEADER_LEN]) -> &[u8] {
        &bytes[..48]
    }

    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[..8].copy_from_slice(&MAGIC);
        b[8] = self.major;
        b[9] = self.minor;
        if let Some(k) = &self.kdf {
            b[10] = 1; // flags bit 0: encrypted
            b[16] = 1; // KDF 1 = Argon2id v1.3
            b[20..24].copy_from_slice(&k.m_kib.to_le_bytes());
            b[24..28].copy_from_slice(&k.t.to_le_bytes());
            b[28..32].copy_from_slice(&k.p.to_le_bytes());
            b[32..48].copy_from_slice(&k.salt);
            b[48..64].copy_from_slice(&self.key_check);
        }
        let crc = crc32fast::hash(&b[..92]);
        b[92..96].copy_from_slice(&crc.to_le_bytes());
        b
    }

    pub fn parse(b: &[u8]) -> Result<Header> {
        if b.len() < HEADER_LEN || b[..8] != MAGIC {
            return Err(damaged("start"));
        }
        if u32::from_le_bytes([b[92], b[93], b[94], b[95]]) != crc32fast::hash(&b[..92]) {
            return Err(damaged("start"));
        }
        let (major, minor) = (b[8], b[9]);
        if major != VERSION_MAJOR {
            return Err(Error::Unsupported("a .purr made by a newer Purrzip. Update Purrzip to open it".into()));
        }
        let flags = u16::from_le_bytes([b[10], b[11]]);
        if flags & !1 != 0 {
            return Err(Error::Unsupported("a .purr made by a newer Purrzip. Update Purrzip to open it".into()));
        }
        let kdf = if flags & 1 != 0 {
            if b[16] != 1 {
                return Err(Error::Unsupported("a .purr locked in a way this Purrzip doesn't know. Update Purrzip to open it".into()));
            }
            let m_kib = u32::from_le_bytes([b[20], b[21], b[22], b[23]]);
            let t = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
            let p = u32::from_le_bytes([b[28], b[29], b[30], b[31]]);
            // Refuse absurd settings: a hostile file must not make us use gigabytes or run for hours.
            if !(8..=1 << 20).contains(&m_kib) || !(1..=64).contains(&t) || !(1..=16).contains(&p) {
                return Err(damaged("lock settings"));
            }
            let mut salt = [0u8; 16];
            salt.copy_from_slice(&b[32..48]);
            Some(Kdf { m_kib, t, p, salt })
        } else {
            None
        };
        let mut key_check = [0u8; 16];
        key_check.copy_from_slice(&b[48..64]);
        Ok(Header { major, minor, kdf, key_check })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Footer {
    pub index_offset: u64,
    pub index_stored: u64,
    pub index_raw: u64,
    pub index_codec: u8,
    /// First 16 bytes of the BLAKE3 hash of the unpacked table of contents. All zero in locked
    /// archives, where the lock itself already proves the table is unchanged.
    pub index_hash: [u8; 16],
}

impl Footer {
    pub fn to_bytes(&self) -> [u8; FOOTER_LEN] {
        let mut b = [0u8; FOOTER_LEN];
        b[..8].copy_from_slice(&FOOTER_MAGIC);
        b[8..16].copy_from_slice(&self.index_offset.to_le_bytes());
        b[16..24].copy_from_slice(&self.index_stored.to_le_bytes());
        b[24..32].copy_from_slice(&self.index_raw.to_le_bytes());
        b[32] = self.index_codec;
        b[40..56].copy_from_slice(&self.index_hash);
        let crc = crc32fast::hash(&b[..60]);
        b[60..64].copy_from_slice(&crc.to_le_bytes());
        b
    }

    pub fn parse(b: &[u8]) -> Option<Footer> {
        if b.len() < FOOTER_LEN || b[..8] != FOOTER_MAGIC {
            return None;
        }
        if u32::from_le_bytes([b[60], b[61], b[62], b[63]]) != crc32fast::hash(&b[..60]) {
            return None;
        }
        let u = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let mut index_hash = [0u8; 16];
        index_hash.copy_from_slice(&b[40..56]);
        Some(Footer { index_offset: u(8), index_stored: u(16), index_raw: u(24), index_codec: b[32], index_hash })
    }
}

pub fn index_hash(raw: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&blake3::hash(raw).as_bytes()[..16]);
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackInfo {
    pub offset: u64,
    pub stored_len: u64,
    pub raw_len: u64,
    pub codec: u8,
    pub first_chunk: u32,
    pub chunk_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkInfo {
    pub pack: u32,
    /// Offset inside the pack's unpacked bytes.
    pub off: u64,
    pub len: u64,
    pub check: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub path: String,
    pub is_dir: bool,
    /// Size of the file as the user sees it (for transformed files: after undoing the transform).
    pub size: u64,
    pub mtime_ms: i64,
    /// Unix permission bits, 0 when unknown.
    pub mode: u32,
    pub transform: u8,
    /// Check of the original bytes for transformed files (first 8 bytes of BLAKE3).
    pub orig_check: u64,
    pub ext_start: u32,
    pub ext_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Stats {
    /// Bytes of all files as the user sees them.
    pub total_in: u64,
    /// Bytes that turned out to be repeats of something already stored.
    pub dup_bytes: u64,
    /// JPEG bytes that went through the photo shrinker, and what they became.
    pub jpeg_in: u64,
    pub jpeg_out: u64,
    pub jpeg_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Index {
    pub creator: String,
    pub created_s: u64,
    pub packs: Vec<PackInfo>,
    pub chunks: Vec<ChunkInfo>,
    pub files: Vec<FileInfo>,
    /// Runs of chunk ids: (first chunk, how many). Files point into this list.
    pub extents: Vec<(u32, u32)>,
    pub stats: Option<Stats>,
}

// ---------- varints ----------

pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}
fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}
fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

pub struct Cur<'a> {
    b: &'a [u8],
    p: usize,
    what: &'static str,
}
impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8], what: &'static str) -> Self {
        Cur { b, p: 0, what }
    }
    pub fn left(&self) -> usize {
        self.b.len() - self.p
    }
    fn bad(&self) -> Error {
        damaged(self.what)
    }
    pub fn u8(&mut self) -> Result<u8> {
        let v = *self.b.get(self.p).ok_or_else(|| self.bad())?;
        self.p += 1;
        Ok(v)
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.left() {
            return Err(self.bad());
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    pub fn varint(&mut self) -> Result<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            let low = (byte & 0x7f) as u64;
            if shift == 63 && low > 1 {
                return Err(self.bad());
            }
            v |= low << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(self.bad())
    }
    /// A count of things that each take at least `min_bytes` more bytes: can't exceed what's left.
    pub fn count(&mut self, min_bytes: usize) -> Result<usize> {
        let n = self.varint()?;
        if n > (self.left() / min_bytes.max(1)) as u64 {
            return Err(self.bad());
        }
        Ok(n as usize)
    }
}

// ---------- index ----------

const TAG_META: u64 = 1;
const TAG_PACKS: u64 = 2;
const TAG_CHUNKS: u64 = 3;
const TAG_FILES: u64 = 4;
const TAG_STATS: u64 = 64; // tags from 64 up may be skipped by readers that don't know them

const F_DIR: u8 = 1;
const F_TRANSFORM: u8 = 2;
const F_MODE: u8 = 4;

fn section(out: &mut Vec<u8>, tag: u64, body: &[u8]) {
    put_varint(out, tag);
    put_varint(out, body.len() as u64);
    out.extend_from_slice(body);
}

impl Index {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // meta
        let mut m = vec![VERSION_MAJOR, VERSION_MINOR];
        put_varint(&mut m, self.created_s);
        put_varint(&mut m, self.creator.len() as u64);
        m.extend_from_slice(self.creator.as_bytes());
        section(&mut out, TAG_META, &m);
        // packs
        let mut p = Vec::new();
        put_varint(&mut p, self.packs.len() as u64);
        let mut end = HEADER_LEN as u64;
        for k in &self.packs {
            put_varint(&mut p, k.offset - end);
            put_varint(&mut p, k.stored_len);
            put_varint(&mut p, k.raw_len);
            p.push(k.codec);
            put_varint(&mut p, k.chunk_count as u64);
            end = k.offset + k.stored_len;
        }
        section(&mut out, TAG_PACKS, &p);
        // chunks: lengths, then checks
        let mut c = Vec::with_capacity(self.chunks.len() * 10);
        put_varint(&mut c, self.chunks.len() as u64);
        for k in &self.chunks {
            put_varint(&mut c, k.len);
        }
        for k in &self.chunks {
            c.extend_from_slice(&k.check.to_le_bytes()[..check_len(k.len)]);
        }
        section(&mut out, TAG_CHUNKS, &c);
        // files, column by column (squeezes much better than row by row)
        let mut f = Vec::new();
        put_varint(&mut f, self.files.len() as u64);
        let paths_len: usize = self.files.iter().map(|x| x.path.len() + 1).sum();
        put_varint(&mut f, paths_len as u64);
        for x in &self.files {
            f.extend_from_slice(x.path.as_bytes());
            f.push(0);
        }
        for x in &self.files {
            f.push(if x.is_dir { F_DIR } else { 0 } | if x.transform != 0 { F_TRANSFORM } else { 0 } | if x.mode != 0 { F_MODE } else { 0 });
        }
        for x in self.files.iter().filter(|x| !x.is_dir) {
            put_varint(&mut f, x.size);
        }
        let mut prev = 0i64;
        for x in &self.files {
            put_varint(&mut f, zigzag(x.mtime_ms.wrapping_sub(prev)));
            prev = x.mtime_ms;
        }
        for x in self.files.iter().filter(|x| x.mode != 0) {
            put_varint(&mut f, x.mode as u64);
        }
        for x in self.files.iter().filter(|x| x.transform != 0) {
            f.push(x.transform);
            f.extend_from_slice(&x.orig_check.to_le_bytes());
        }
        let mut prev_end = 0i64;
        for x in self.files.iter().filter(|x| !x.is_dir) {
            put_varint(&mut f, x.ext_count as u64);
            for &(first, count) in &self.extents[x.ext_start as usize..(x.ext_start + x.ext_count) as usize] {
                put_varint(&mut f, zigzag(first as i64 - prev_end));
                put_varint(&mut f, count as u64);
                prev_end = first as i64 + count as i64;
            }
        }
        section(&mut out, TAG_FILES, &f);
        if let Some(s) = &self.stats {
            let mut b = Vec::new();
            for v in [s.total_in, s.dup_bytes, s.jpeg_in, s.jpeg_out, s.jpeg_count] {
                put_varint(&mut b, v);
            }
            section(&mut out, TAG_STATS, &b);
        }
        out
    }

    /// Parse and fully check an index. `data_end` is where the pack area must end (the index offset).
    pub fn parse(bytes: &[u8], data_end: u64) -> Result<Index> {
        let mut top = Cur::new(bytes, "table of contents");
        let mut ix = Index::default();
        let (mut got_meta, mut got_packs, mut got_chunks, mut got_files) = (false, false, false, false);
        while top.left() > 0 {
            let tag = top.varint()?;
            let len = top.count(1)?;
            let body = top.bytes(len)?;
            let mut c = Cur::new(body, "table of contents");
            match tag {
                TAG_META if !got_meta => {
                    got_meta = true;
                    let (major, _minor) = (c.u8()?, c.u8()?);
                    if major > VERSION_MAJOR {
                        return Err(Error::Unsupported("a .purr made by a newer Purrzip. Update Purrzip to open it".into()));
                    }
                    ix.created_s = c.varint()?;
                    let n = c.count(1)?;
                    ix.creator = String::from_utf8_lossy(c.bytes(n)?).into_owned();
                }
                TAG_PACKS if !got_packs && !got_chunks => {
                    got_packs = true;
                    let n = c.count(5)?;
                    let mut end = HEADER_LEN as u64;
                    let mut first_chunk = 0u64;
                    for _ in 0..n {
                        let gap = c.varint()?;
                        let stored_len = c.varint()?;
                        let raw_len = c.varint()?;
                        let codec = c.u8()?;
                        let chunk_count = c.varint()?;
                        let offset = end.checked_add(gap).ok_or_else(|| damaged("table of contents"))?;
                        end = offset.checked_add(stored_len).ok_or_else(|| damaged("table of contents"))?;
                        if end > data_end || raw_len > MAX_PACK_RAW || chunk_count > u32::MAX as u64 {
                            return Err(damaged("table of contents"));
                        }
                        if codec > CODEC_PPMD7 {
                            return Err(Error::Unsupported("a .purr that uses a newer squeezing method. Update Purrzip to open it".into()));
                        }
                        ix.packs.push(PackInfo { offset, stored_len, raw_len, codec, first_chunk: first_chunk as u32, chunk_count: chunk_count as u32 });
                        first_chunk += chunk_count;
                        if first_chunk > u32::MAX as u64 {
                            return Err(damaged("table of contents"));
                        }
                    }
                }
                TAG_CHUNKS if got_packs && !got_chunks => {
                    got_chunks = true;
                    let n = c.count(5)?;
                    let expected: u64 = ix.packs.iter().map(|p| p.chunk_count as u64).sum();
                    if n as u64 != expected {
                        return Err(damaged("table of contents"));
                    }
                    ix.chunks.reserve_exact(n);
                    for (pi, p) in ix.packs.iter().enumerate() {
                        let mut off = 0u64;
                        for _ in 0..p.chunk_count {
                            let len = c.varint()?;
                            if len > p.raw_len - off {
                                return Err(damaged("table of contents"));
                            }
                            ix.chunks.push(ChunkInfo { pack: pi as u32, off, len, check: 0 });
                            off += len;
                        }
                        if off != p.raw_len {
                            return Err(damaged("table of contents"));
                        }
                    }
                    for k in ix.chunks.iter_mut() {
                        let n = check_len(k.len);
                        let mut b = [0u8; 8];
                        b[..n].copy_from_slice(c.bytes(n)?);
                        k.check = u64::from_le_bytes(b);
                    }
                }
                TAG_FILES if got_chunks && !got_files => {
                    got_files = true;
                    let n = c.count(2)?;
                    let paths_len = c.count(1)?;
                    let paths = c.bytes(paths_len)?;
                    let mut names = paths.split(|&b| b == 0);
                    let flags = c.bytes(n)?;
                    ix.files.reserve_exact(n);
                    for &fl in flags {
                        if fl & !(F_DIR | F_TRANSFORM | F_MODE) != 0 || (fl & F_DIR != 0 && fl & F_TRANSFORM != 0) {
                            return Err(damaged("table of contents"));
                        }
                        let name = names.next().ok_or_else(|| damaged("table of contents"))?;
                        ix.files.push(FileInfo {
                            path: String::from_utf8_lossy(name).into_owned(),
                            is_dir: fl & F_DIR != 0,
                            size: 0,
                            mtime_ms: 0,
                            mode: if fl & F_MODE != 0 { 1 } else { 0 },
                            transform: if fl & F_TRANSFORM != 0 { 1 } else { 0 },
                            orig_check: 0,
                            ext_start: 0,
                            ext_count: 0,
                        });
                    }
                    // exactly one empty piece must remain after the last NUL
                    if names.next() != Some(&[][..]) || names.next().is_some() {
                        return Err(damaged("table of contents"));
                    }
                    for x in ix.files.iter_mut().filter(|x| !x.is_dir) {
                        x.size = c.varint()?;
                    }
                    let mut prev = 0i64;
                    for x in ix.files.iter_mut() {
                        prev = prev.wrapping_add(unzigzag(c.varint()?));
                        x.mtime_ms = prev;
                    }
                    for x in ix.files.iter_mut().filter(|x| x.mode != 0) {
                        x.mode = (c.varint()? & 0o7777) as u32;
                    }
                    for x in ix.files.iter_mut().filter(|x| x.transform != 0) {
                        x.transform = c.u8()?;
                        if x.transform != TRANSFORM_LEPTON {
                            return Err(Error::Unsupported("a .purr that uses a newer file shrinker. Update Purrzip to open it".into()));
                        }
                        x.orig_check = u64::from_le_bytes(c.bytes(8)?.try_into().unwrap());
                    }
                    let total_chunks = ix.chunks.len() as i64;
                    let mut prev_end = 0i64;
                    for x in ix.files.iter_mut().filter(|x| !x.is_dir) {
                        let k = c.count(2)?;
                        x.ext_start = ix.extents.len() as u32;
                        x.ext_count = k as u32;
                        let mut stored = 0u64;
                        for _ in 0..k {
                            let first = prev_end.checked_add(unzigzag(c.varint()?)).ok_or_else(|| damaged("table of contents"))?;
                            let count = c.varint()?;
                            if first < 0 || count == 0 || count > total_chunks as u64 || first > total_chunks - count as i64 {
                                return Err(damaged("table of contents"));
                            }
                            for ch in &ix.chunks[first as usize..first as usize + count as usize] {
                                stored = stored.checked_add(ch.len).ok_or_else(|| damaged("table of contents"))?;
                            }
                            ix.extents.push((first as u32, count as u32));
                            prev_end = first + count as i64;
                            if ix.extents.len() > u32::MAX as usize {
                                return Err(damaged("table of contents"));
                            }
                        }
                        if x.transform == TRANSFORM_NONE {
                            if stored != x.size {
                                return Err(damaged("table of contents"));
                            }
                        } else if stored > MAX_BLOB || x.size > MAX_BLOB * 4 {
                            return Err(damaged("table of contents"));
                        }
                    }
                }
                TAG_STATS => {
                    let mut s = Stats::default();
                    for v in [&mut s.total_in, &mut s.dup_bytes, &mut s.jpeg_in, &mut s.jpeg_out, &mut s.jpeg_count] {
                        *v = c.varint()?;
                    }
                    ix.stats = Some(s);
                    continue; // newer writers may add fields at the end
                }
                t if t >= 64 => continue,
                _ => return Err(Error::Unsupported("a .purr made by a newer Purrzip. Update Purrzip to open it".into())),
            }
            if c.left() != 0 {
                return Err(damaged("table of contents"));
            }
        }
        if !(got_meta && got_packs && got_chunks && got_files) {
            return Err(damaged("table of contents"));
        }
        Ok(ix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Index {
        Index {
            creator: "test".into(),
            created_s: 1_800_000_000,
            packs: vec![
                PackInfo { offset: 96, stored_len: 50, raw_len: 30000, codec: CODEC_ZSTD, first_chunk: 0, chunk_count: 2 },
                PackInfo { offset: 146, stored_len: 20000, raw_len: 20000, codec: CODEC_STORE, first_chunk: 2, chunk_count: 1 },
            ],
            chunks: vec![
                ChunkInfo { pack: 0, off: 0, len: 100, check: 0xAABBCCDD },
                ChunkInfo { pack: 0, off: 100, len: 29900, check: 0x1122334455667788 },
                ChunkInfo { pack: 1, off: 0, len: 20000, check: 7 },
            ],
            files: vec![
                FileInfo { path: "a".into(), is_dir: true, size: 0, mtime_ms: 5, mode: 0, transform: 0, orig_check: 0, ext_start: 0, ext_count: 0 },
                FileInfo { path: "a/x.txt".into(), is_dir: false, size: 30000, mtime_ms: -3, mode: 0o644, transform: 0, orig_check: 0, ext_start: 0, ext_count: 1 },
                FileInfo { path: "a/y.txt".into(), is_dir: false, size: 100, mtime_ms: 99, mode: 0, transform: 0, orig_check: 0, ext_start: 1, ext_count: 1 },
                FileInfo { path: "p.jpg".into(), is_dir: false, size: 25000, mtime_ms: 99, mode: 0, transform: 1, orig_check: 42, ext_start: 2, ext_count: 1 },
                FileInfo { path: "empty".into(), is_dir: false, size: 0, mtime_ms: 0, mode: 0, transform: 0, orig_check: 0, ext_start: 3, ext_count: 0 },
            ],
            extents: vec![(0, 2), (0, 1), (2, 1)],
            stats: Some(Stats { total_in: 55100, dup_bytes: 100, jpeg_in: 25000, jpeg_out: 20000, jpeg_count: 1 }),
        }
    }

    #[test]
    fn index_round_trip() {
        let ix = sample();
        let b = ix.to_bytes();
        assert_eq!(Index::parse(&b, 20146).unwrap(), ix);
    }

    #[test]
    fn index_rejects_bad_references() {
        let mut ix = sample();
        ix.extents[2] = (2, 5); // past the last chunk
        assert!(Index::parse(&ix.to_bytes(), 20146).is_err());
        let mut ix = sample();
        ix.files[1].size = 30001; // size doesn't match its chunks
        assert!(Index::parse(&ix.to_bytes(), 20146).is_err());
        let ix = sample();
        assert!(Index::parse(&ix.to_bytes(), 20000).is_err()); // packs run past the data area
    }

    #[test]
    fn index_survives_any_cut_or_flip() {
        let b = sample().to_bytes();
        for cut in 0..b.len() {
            let _ = Index::parse(&b[..cut], 20146);
        }
        for i in 0..b.len() {
            for bit in 0..8 {
                let mut m = b.clone();
                m[i] ^= 1 << bit;
                let _ = Index::parse(&m, 20146);
            }
        }
    }

    #[test]
    fn header_footer_round_trip() {
        let h = Header { major: 1, minor: 0, kdf: Some(Kdf { m_kib: 65536, t: 3, p: 1, salt: [7; 16] }), key_check: [9; 16] };
        assert_eq!(Header::parse(&h.to_bytes()).unwrap(), h);
        let plain = Header { major: 1, minor: 0, kdf: None, key_check: [0; 16] };
        assert_eq!(Header::parse(&plain.to_bytes()).unwrap(), plain);
        let mut bad = h.to_bytes();
        bad[33] ^= 1;
        assert!(Header::parse(&bad).is_err());
        let f = Footer { index_offset: 1234, index_stored: 99, index_raw: 500, index_codec: 1, index_hash: [5; 16] };
        assert_eq!(Footer::parse(&f.to_bytes()).unwrap(), f);
    }

    #[test]
    fn varints() {
        for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut b = Vec::new();
            put_varint(&mut b, v);
            assert_eq!(Cur::new(&b, "t").varint().unwrap(), v);
        }
        for v in [0i64, -1, 1, i64::MIN, i64::MAX] {
            assert_eq!(unzigzag(zigzag(v)), v);
        }
        assert!(Cur::new(&[0xff; 11], "t").varint().is_err());
    }
}
